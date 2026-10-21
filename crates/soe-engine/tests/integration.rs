use std::path::Path;

use soe_engine::runtime::{Key, Runtime, RuntimeEvent, Screen, TRANSITION};
use soe_engine::save::GameState;
use soe_engine::session::Session;
use soe_engine::story::HintBook;

fn game_dir() -> &'static Path {
    Path::new("../../game")
}

fn exe_path() -> &'static Path {
    Path::new("../../ghidra/EXCAL_UNPACKED.EXE")
}

#[test]
fn end_to_end_room_animation_save_font_story_travel() {
    if !game_dir().join("EPISODE1.DAT").exists() || !game_dir().join("RES32").exists() {
        eprintln!("SKIP: game data absent");
        return;
    }

    let mut session = Session::load(game_dir(), exe_path(), 5).unwrap();
    assert_eq!(session.scene_id, 5);
    assert!(session.font().is_some());
    assert!(session.animation_count() >= 3);
    assert!(session.frame.pixels.iter().any(|pixel| *pixel != 0));

    let first = session.frame.pixels.clone();
    session.advance_animation_frame();
    assert_ne!(first, session.frame.pixels, "animation frame changed");
    session.reset_animation_frame();
    assert_eq!(first, session.frame.pixels, "animation reset");

    let temp = std::env::temp_dir().join(format!("soe-e2e-{}", std::process::id()));
    std::fs::create_dir_all(&temp).unwrap();
    session.save_to(&temp, "E2E.SAV").unwrap();
    let saved = std::fs::read(temp.join("E2E.SAV")).unwrap();
    let state = GameState::parse(&saved).unwrap();
    assert_eq!(state.characters[0].name(), "Constantine");
    assert_eq!(state.to_bytes(), saved);
    std::fs::remove_dir_all(&temp).ok();

    let book = HintBook::load(game_dir().join("HINT.RES")).unwrap();
    assert!(book.paragraphs.len() > 100);

    let locations = &session.state.locations;
    assert_eq!(locations[0].name(), "Colchester");
    assert_eq!(locations[0].scene_id(), Some(5));
}

#[test]
fn runtime_routes_live_save_through_every_screen() {
    if !game_dir().join("EPISODE1.DAT").exists() || !game_dir().join("RES32").exists() {
        eprintln!("SKIP: game data absent");
        return;
    }

    let session = Session::load(game_dir(), exe_path(), 5).unwrap();
    let temp = std::env::temp_dir().join(format!("soe-runtime-{}", std::process::id()));
    std::fs::create_dir_all(&temp).unwrap();
    let mut runtime = Runtime::new(
        session,
        vec![4, 5, 6],
        1,
        temp.clone(),
        exe_path().to_path_buf(),
    );
    assert_eq!(runtime.screen(), Screen::Room);
    assert_eq!(runtime.scene_id(), 5);

    assert_eq!(
        runtime.handle_key(Key::Map),
        vec![RuntimeEvent::ScreenChanged(Screen::Map)]
    );
    assert_eq!(runtime.screen(), Screen::Map);
    assert_eq!(
        runtime.handle_key(Key::Map),
        vec![RuntimeEvent::ScreenChanged(Screen::Room)]
    );
    assert_eq!(runtime.screen(), Screen::Room);

    runtime.handle_key(Key::Status);
    assert_eq!(runtime.screen(), Screen::Status);
    runtime.handle_key(Key::Cancel);
    assert_eq!(runtime.screen(), Screen::Room);

    runtime.handle_key(Key::Pause);
    assert_eq!(runtime.screen(), Screen::Pause);
    runtime.handle_key(Key::Space);
    assert_eq!(runtime.screen(), Screen::Room);

    runtime.handle_key(Key::Travel);
    assert_eq!(runtime.screen(), Screen::Travel);
    runtime.handle_key(Key::Down);
    let events = runtime.handle_key(Key::Confirm);
    assert!(events.contains(&RuntimeEvent::SceneChanged(4)));
    assert_eq!(runtime.state_code(), -3);
    runtime.tick(TRANSITION);
    assert_eq!(runtime.screen(), Screen::Room);
    assert_eq!(runtime.scene_id(), 4);
    assert_eq!(runtime.state_code(), -1);

    let events = runtime.handle_key(Key::QuickSave);
    assert!(events.contains(&RuntimeEvent::Saved("QUICK.SAV".to_string())));
    assert!(temp.join("QUICK.SAV").exists());
    std::fs::remove_dir_all(&temp).ok();
}
