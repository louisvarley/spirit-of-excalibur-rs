use std::path::Path;

use soe_engine::runtime::{Key, Runtime, RuntimeEvent, Screen, TRANSITION};
use soe_engine::save::GameState;
use soe_engine::session::Session;
use soe_engine::story::HintBook;
use soe_engine::worldmap::{markers, resolve_scene_id};

fn scene_ids() -> Vec<u16> {
    let file = soe_res::ResFile::parse(
        std::fs::read(game_dir().join("SCENES.RES")).expect("read SCENES.RES"),
    )
    .expect("parse SCENES.RES");
    let mut ids: Vec<u16> = file.records().iter().map(|record| record.id).collect();
    ids.sort_unstable();
    ids.dedup();
    ids
}

fn example_runtime() -> Option<Runtime> {
    if !game_dir().join("EXAMPLE.SAV").exists() || !game_dir().join("RES32").exists() {
        eprintln!("SKIP: EXAMPLE.SAV or game data absent");
        return None;
    }
    let session = Session::load_save(game_dir(), exe_path(), 5, "EXAMPLE.SAV").unwrap();
    let story = HintBook::load(game_dir().join("HINT.RES")).ok();
    Some(
        Runtime::new(
            session,
            vec![5],
            0,
            game_dir().to_path_buf(),
            exe_path().to_path_buf(),
        )
        .with_assets(None, None, story),
    )
}

#[test]
fn example_save_talk_opens_friar_dialogue() {
    let Some(mut runtime) = example_runtime() else {
        return;
    };
    // Select Constantine, open the action menu and choose Talk.
    runtime.select_character(0);
    assert_eq!(runtime.screen(), Screen::Action);
    assert_eq!(runtime.actor_name(), "Constantine");
    runtime.handle_key(Key::Right);
    runtime.handle_key(Key::Right);
    assert_eq!(runtime.action_index(), 2);
    runtime.handle_key(Key::Confirm);
    assert_eq!(runtime.screen(), Screen::Target);
    assert!(runtime.target_names().iter().any(|name| name == "friar"));

    // Move to the friar and confirm.
    let friar = runtime
        .target_names()
        .iter()
        .rposition(|name| name == "friar")
        .expect("friar target");
    while runtime.target_index() < friar {
        runtime.handle_key(Key::Down);
    }
    assert_eq!(runtime.target_character(), 45);
    runtime.handle_key(Key::Confirm);
    assert_eq!(runtime.screen(), Screen::Dialogue);
    let dialogue = runtime.dialogue().expect("dialogue open");
    assert_eq!(dialogue.speaker, 45);
    assert_eq!(dialogue.listener, 0);
    assert_eq!(dialogue.line_id, 120);
    assert!(dialogue.text.contains("we welcome you"));
    runtime.handle_key(Key::Confirm);
    assert_eq!(runtime.screen(), Screen::Room);
}

#[test]
fn example_save_give_moves_a_carried_item() {
    let Some(mut runtime) = example_runtime() else {
        return;
    };
    // The apothecary (0-based 43) owns objects 15..18 in EXAMPLE.SAV.
    runtime.select_character(43);
    assert_eq!(runtime.screen(), Screen::Action);
    runtime.handle_key(Key::Down);
    runtime.handle_key(Key::Down);
    runtime.handle_key(Key::Right);
    assert_eq!(runtime.action_index(), 9);
    runtime.handle_key(Key::Confirm);
    assert_eq!(runtime.screen(), Screen::Target);
    runtime.handle_key(Key::Confirm);
    assert_eq!(runtime.screen(), Screen::Item);

    let entry = runtime
        .item_entries()
        .first()
        .cloned()
        .expect("carried item");
    let target = runtime.target_character();
    let before = runtime.session().state.to_bytes();
    runtime.handle_key(Key::Confirm);
    assert_ne!(
        runtime.session().state.to_bytes(),
        before,
        "save bytes changed"
    );
    let moved = &runtime.session().state.instances[entry.slot];
    assert_eq!(moved.owner(), (target + 1) as u8);
    assert!(moved.is_carried());
    assert!(
        runtime
            .session()
            .state
            .inventory(target)
            .iter()
            .any(|row| row.def == entry.def),
        "target now holds the item"
    );
}

#[test]
fn provided_room_saves_parse_with_event_tables() {
    for name in ["EXAMPLE.SAV", "D1.SAV", "TRY1.SAV"] {
        let path = game_dir().join(name);
        let Ok(data) = std::fs::read(&path) else {
            continue;
        };
        let state = GameState::parse(&data).unwrap();
        assert_eq!(state.events.len(), 80, "{name}");
        assert_eq!(state.instances.len(), 85, "{name}");
        assert_eq!(state.to_bytes(), data, "{name} round-trip");
        assert_eq!(state.characters.len(), 92, "{name} includes NPC slots");
    }
}

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

    let ids = scene_ids();
    let available = |id: u16| ids.contains(&id);
    let locations = markers(&session.state);
    assert_eq!(locations[0].name, "Colchester");
    // Colchester has no region-1 scene 101, so the loader falls back to scene 1.
    assert_eq!(
        resolve_scene_id(
            locations[0].location_index,
            locations[0].icon_id,
            1,
            available
        ),
        Some(1)
    );
    // Gloucester and London have explicit region-1 scenes.
    assert_eq!(
        resolve_scene_id(
            locations[1].location_index,
            locations[1].icon_id,
            1,
            available
        ),
        Some(102)
    );
    assert_eq!(
        resolve_scene_id(
            locations[4].location_index,
            locations[4].icon_id,
            1,
            available
        ),
        Some(105)
    );
}

#[test]
fn every_location_resolves_to_a_scene_record() {
    if !game_dir().join("EPISODE1.DAT").exists() {
        eprintln!("SKIP: game data absent");
        return;
    }
    let data = std::fs::read(game_dir().join("EPISODE1.DAT")).unwrap();
    let state = GameState::parse(&data).unwrap();
    let ids = scene_ids();
    let available = |id: u16| ids.contains(&id);
    let markers = markers(&state);
    assert_eq!(markers.len(), 49);
    for marker in &markers {
        let scene = resolve_scene_id(marker.location_index, marker.icon_id, 1, available);
        assert!(
            scene.is_some_and(|id| available(id)),
            "{} ({}) has no scene",
            marker.name,
            marker.icon_id
        );
    }
}

#[test]
fn map_click_travels_to_the_clicked_marker() {
    if !game_dir().join("EPISODE1.DAT").exists() {
        eprintln!("SKIP: game data absent");
        return;
    }
    let session = Session::load(game_dir(), exe_path(), 5).unwrap();
    let ids = scene_ids();
    let start = ids.iter().position(|id| *id == 5).unwrap();
    let mut runtime = Runtime::new(
        session,
        ids,
        start,
        game_dir().to_path_buf(),
        exe_path().to_path_buf(),
    );
    runtime.handle_key(Key::Travel);
    let marker = runtime.map_markers()[runtime.travel_index()].clone();
    let (camera_x, camera_y) = runtime.map_camera();
    let x =
        i16::try_from(soe_engine::worldmap::VIEW_X + i32::from(marker.world_x) - camera_x).unwrap();
    let y =
        i16::try_from(soe_engine::worldmap::VIEW_Y + i32::from(marker.world_y) - camera_y).unwrap();
    let events = runtime.handle_click(x, y);
    assert!(
        events
            .iter()
            .any(|event| matches!(event, RuntimeEvent::SceneChanged(_))),
        "click did not travel: {events:?}"
    );
    assert_eq!(runtime.current_location(), Some(marker.location_index));
    runtime.tick(TRANSITION);
    assert_eq!(runtime.screen(), Screen::Room);
    runtime.handle_key(Key::Map);
    assert_eq!(runtime.screen(), Screen::Map);
    assert_eq!(runtime.selected_location(), Some(marker.location_index));
}

#[test]
fn runtime_routes_live_save_through_every_screen() {
    if !game_dir().join("EPISODE1.DAT").exists() || !game_dir().join("RES32").exists() {
        eprintln!("SKIP: game data absent");
        return;
    }

    let session = Session::load(game_dir(), exe_path(), 5).unwrap();
    let ids = scene_ids();
    let start = ids.iter().position(|id| *id == 5).unwrap();
    let temp = std::env::temp_dir().join(format!("soe-runtime-{}", std::process::id()));
    std::fs::create_dir_all(&temp).unwrap();
    let mut runtime = Runtime::new(session, ids, start, temp.clone(), exe_path().to_path_buf());
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
    let mut guard = 0;
    while runtime.selected_location_name().as_deref() != Some("London") {
        runtime.handle_key(Key::Down);
        guard += 1;
        assert!(guard < 64, "London marker not reachable");
    }
    let events = runtime.handle_key(Key::Confirm);
    assert!(events.contains(&RuntimeEvent::SceneChanged(105)));
    assert_eq!(runtime.state_code(), -3);
    runtime.tick(TRANSITION);
    assert_eq!(runtime.screen(), Screen::Room);
    assert_eq!(runtime.scene_id(), 105);
    assert_eq!(runtime.state_code(), -1);
    assert_eq!(runtime.current_location_name().as_deref(), Some("London"));

    // Returning to the map keeps the newly visited location current.
    runtime.handle_key(Key::Map);
    assert_eq!(runtime.screen(), Screen::Map);
    assert_eq!(runtime.current_location_name().as_deref(), Some("London"));
    assert_eq!(runtime.selected_location_name().as_deref(), Some("London"));
    runtime.handle_key(Key::Map);
    assert_eq!(runtime.screen(), Screen::Room);

    let events = runtime.handle_key(Key::QuickSave);
    assert!(events.contains(&RuntimeEvent::Saved("QUICK.SAV".to_string())));
    assert!(temp.join("QUICK.SAV").exists());
    std::fs::remove_dir_all(&temp).ok();
}
