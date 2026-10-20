//! Platform bundle. Keeps all OS/runtime services behind traits.

#![forbid(unsafe_code)]

use crate::audio::Sound;
use crate::files::GameFiles;
use crate::input::Input;
use crate::timer::Timer;
use soe_video::Video;

pub struct Platform {
    pub video: Box<dyn Video>,
    pub input: Box<dyn Input>,
    pub timer: Box<dyn Timer>,
    pub sound: Box<dyn Sound>,
    pub files: Box<dyn GameFiles>,
}
