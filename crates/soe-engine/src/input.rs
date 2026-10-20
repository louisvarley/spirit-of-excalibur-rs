//! Input boundary (keyboard and INT 33h-style mouse).
//!
//! Game logic consumes logical events, never platform APIs.

#![forbid(unsafe_code)]

/// Logical key identifier. 0 is reserved; values follow the original scan-code
/// ordering where practical.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct KeyCode(pub u16);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputEvent {
    KeyDown(KeyCode),
    KeyUp(KeyCode),
    MouseMove { x: i16, y: i16 },
    MouseDown(MouseButton),
    MouseUp(MouseButton),
}

/// The platform input boundary.
pub trait Input {
    /// Drain pending events.
    fn poll(&mut self) -> Vec<InputEvent>;
    /// Current logical mouse position (320x200 coordinates).
    fn mouse_position(&self) -> (i16, i16);
    /// Whether the mouse driver/device is present.
    fn mouse_present(&self) -> bool;
}

/// Deterministic input source for tests and scripted runs.
#[derive(Debug, Default, Clone)]
pub struct HeadlessInput {
    queue: Vec<InputEvent>,
    mouse: (i16, i16),
    present: bool,
}

impl HeadlessInput {
    #[must_use]
    pub fn new() -> Self {
        Self {
            queue: Vec::new(),
            mouse: (0, 0),
            present: true,
        }
    }

    pub fn push(&mut self, event: InputEvent) {
        self.queue.push(event);
    }

    pub fn set_mouse(&mut self, x: i16, y: i16) {
        self.mouse = (x, y);
    }

    pub fn set_mouse_present(&mut self, present: bool) {
        self.present = present;
    }
}

impl Input for HeadlessInput {
    fn poll(&mut self) -> Vec<InputEvent> {
        std::mem::take(&mut self.queue)
    }

    fn mouse_position(&self) -> (i16, i16) {
        self.mouse
    }

    fn mouse_present(&self) -> bool {
        self.present
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drains_events_once() {
        let mut input = HeadlessInput::new();
        input.push(InputEvent::KeyDown(KeyCode(0x39)));
        input.push(InputEvent::MouseMove { x: 10, y: 20 });
        assert_eq!(input.poll().len(), 2);
        assert!(input.poll().is_empty());
    }

    #[test]
    fn mouse_state_is_logical() {
        let mut input = HeadlessInput::new();
        input.set_mouse(123, 45);
        assert_eq!(input.mouse_position(), (123, 45));
        assert!(input.mouse_present());
        input.set_mouse_present(false);
        assert!(!input.mouse_present());
    }
}
