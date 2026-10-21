//! Game logic for the Spirit of Excalibur port.
//!
//! Intentionally empty in the foundation step. Game code will use logical
//! 320x200 coordinates only and will reach images through `soe-assets`.

#![forbid(unsafe_code)]

pub mod save;

pub mod economy;

pub mod interaction;

pub mod scene;

pub mod ani;

pub mod input;

pub mod timer;

pub mod audio;

pub mod files;

pub mod platform;

pub mod resources;

pub mod text;

pub mod room;

pub mod palette;

pub mod session;

pub mod runtime;

pub mod worldmap;

pub mod font;

pub mod story;

pub mod battle;

pub mod smus;
