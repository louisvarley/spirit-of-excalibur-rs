use std::path::Path;

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
