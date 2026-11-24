mod key;

pub use key::Key;

use std::collections::HashSet;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
    Back,
    Forward,
}

#[derive(Clone, Debug, Default)]
pub struct Input {
    down: HashSet<Key>,
    pressed: HashSet<Key>,
    released: HashSet<Key>,
    mouse_down: HashSet<MouseButton>,
    mouse_pressed: HashSet<MouseButton>,
    mouse_released: HashSet<MouseButton>,
    cursor: [f32; 2],
    mouse_delta: [f32; 2],
    scroll: [f32; 2],
    focused: bool,
    cursor_inside: bool,
}

impl Input {
    pub fn key(&self, key: Key) -> bool {
        self.down.contains(&key)
    }

    pub fn key_pressed(&self, key: Key) -> bool {
        self.pressed.contains(&key)
    }

    pub fn key_released(&self, key: Key) -> bool {
        self.released.contains(&key)
    }

    pub fn mouse_button(&self, button: MouseButton) -> bool {
        self.mouse_down.contains(&button)
    }

    pub fn mouse_button_pressed(&self, button: MouseButton) -> bool {
        self.mouse_pressed.contains(&button)
    }

    pub fn mouse_button_released(&self, button: MouseButton) -> bool {
        self.mouse_released.contains(&button)
    }

    pub fn cursor(&self) -> [f32; 2] {
        self.cursor
    }

    pub fn mouse_delta(&self) -> [f32; 2] {
        self.mouse_delta
    }

    pub fn scroll(&self) -> [f32; 2] {
        self.scroll
    }

    pub fn focused(&self) -> bool {
        self.focused
    }

    pub fn cursor_inside(&self) -> bool {
        self.cursor_inside
    }

    pub fn any_down(&self) -> bool {
        !self.down.is_empty() || !self.mouse_down.is_empty()
    }

    pub fn set_key(&mut self, key: Key, pressed: bool, repeat: bool) {
        if pressed {
            let fresh = self.down.insert(key);
            if fresh && !repeat {
                self.pressed.insert(key);
            }
        } else if self.down.remove(&key) {
            self.released.insert(key);
        }
    }

    pub fn set_mouse_button(&mut self, button: MouseButton, pressed: bool) {
        if pressed {
            if self.mouse_down.insert(button) {
                self.mouse_pressed.insert(button);
            }
        } else if self.mouse_down.remove(&button) {
            self.mouse_released.insert(button);
        }
    }

    pub fn set_cursor(&mut self, x: f32, y: f32) {
        self.cursor = [x, y];
    }

    pub fn add_mouse_delta(&mut self, x: f32, y: f32) {
        self.mouse_delta[0] += x;
        self.mouse_delta[1] += y;
    }

    pub fn add_scroll(&mut self, x: f32, y: f32) {
        self.scroll[0] += x;
        self.scroll[1] += y;
    }

    pub fn set_cursor_inside(&mut self, inside: bool) {
        self.cursor_inside = inside;
    }

    pub fn set_focused(&mut self, focused: bool) {
        self.focused = focused;
        if !focused {
            self.down.clear();
            self.mouse_down.clear();
            self.mouse_delta = [0.0, 0.0];
        }
    }

    pub fn end_frame(&mut self) {
        self.pressed.clear();
        self.released.clear();
        self.mouse_pressed.clear();
        self.mouse_released.clear();
        self.mouse_delta = [0.0, 0.0];
        self.scroll = [0.0, 0.0];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_hold_survives_end_frame_and_release_is_an_edge() {
        let mut input = Input::default();

        input.set_key(Key::A, true, false);
        assert!(input.key(Key::A));
        assert!(input.key_pressed(Key::A));
        assert!(!input.key_released(Key::A));

        input.end_frame();
        assert!(input.key(Key::A));
        assert!(!input.key_pressed(Key::A));

        input.set_key(Key::A, true, false);
        assert!(input.key(Key::A));
        assert!(!input.key_pressed(Key::A));

        input.set_key(Key::A, false, false);
        assert!(!input.key(Key::A));
        assert!(input.key_released(Key::A));

        input.end_frame();
        assert!(!input.key(Key::A));
        assert!(!input.key_released(Key::A));
    }

    #[test]
    fn mouse_button_hold_survives_end_frame_and_release_is_an_edge() {
        let mut input = Input::default();

        input.set_mouse_button(MouseButton::Left, true);
        assert!(input.mouse_button(MouseButton::Left));
        assert!(input.mouse_button_pressed(MouseButton::Left));
        assert!(!input.mouse_button_released(MouseButton::Left));

        input.end_frame();
        assert!(input.mouse_button(MouseButton::Left));
        assert!(!input.mouse_button_pressed(MouseButton::Left));

        input.set_mouse_button(MouseButton::Left, false);
        assert!(!input.mouse_button(MouseButton::Left));
        assert!(input.mouse_button_released(MouseButton::Left));

        input.end_frame();
        assert!(!input.mouse_button_released(MouseButton::Left));
    }

    #[test]
    fn key_repeat_does_not_emit_a_press_edge() {
        let mut input = Input::default();

        input.set_key(Key::Space, true, true);
        assert!(input.key(Key::Space));
        assert!(!input.key_pressed(Key::Space));

        input.end_frame();
        input.set_key(Key::W, true, false);
        input.end_frame();
        input.set_key(Key::W, true, true);
        assert!(input.key(Key::W));
        assert!(!input.key_pressed(Key::W));
    }

    #[test]
    fn focus_loss_clears_held_input_without_release_edges() {
        let mut input = Input::default();
        input.set_focused(true);
        input.set_key(Key::Escape, true, false);
        input.set_mouse_button(MouseButton::Right, true);
        input.end_frame();

        input.set_focused(false);

        assert!(!input.focused());
        assert!(!input.key(Key::Escape));
        assert!(!input.key_pressed(Key::Escape));
        assert!(!input.key_released(Key::Escape));
        assert!(!input.mouse_button(MouseButton::Right));
        assert!(!input.mouse_button_pressed(MouseButton::Right));
        assert!(!input.mouse_button_released(MouseButton::Right));
        assert!(!input.any_down());
    }

    #[test]
    fn focus_gain_only_sets_the_flag() {
        let mut input = Input::default();
        input.set_key(Key::A, true, false);
        input.add_mouse_delta(1.0, 2.0);

        input.set_focused(true);

        assert!(input.focused());
        assert!(input.key(Key::A));
        assert_eq!(input.mouse_delta(), [1.0, 2.0]);
    }

    #[test]
    fn mouse_delta_accumulates_until_end_frame() {
        let mut input = Input::default();
        input.add_mouse_delta(1.0, -2.0);
        input.add_mouse_delta(0.5, 4.0);
        assert_eq!(input.mouse_delta(), [1.5, 2.0]);

        input.set_key(Key::W, true, false);
        input.end_frame();

        assert_eq!(input.mouse_delta(), [0.0, 0.0]);
        assert!(input.key(Key::W));
        assert!(!input.key_pressed(Key::W));
    }

    #[test]
    fn focus_loss_clears_mouse_delta() {
        let mut input = Input::default();
        input.add_mouse_delta(3.0, -4.5);
        input.add_scroll(1.0, 0.0);

        input.set_focused(false);

        assert_eq!(input.mouse_delta(), [0.0, 0.0]);
        assert_eq!(input.scroll(), [1.0, 0.0]);
    }

    #[test]
    fn scroll_accumulates_until_end_frame() {
        let mut input = Input::default();
        input.add_scroll(1.0, 2.0);
        input.add_scroll(-0.25, 0.5);
        assert_eq!(input.scroll(), [0.75, 2.5]);

        input.end_frame();
        assert_eq!(input.scroll(), [0.0, 0.0]);
    }

    #[test]
    fn any_down_tracks_keys_and_buttons_after_end_frame() {
        let mut input = Input::default();
        assert!(!input.any_down());

        input.set_key(Key::A, true, false);
        assert!(input.any_down());
        input.end_frame();
        assert!(input.any_down());

        input.set_key(Key::A, false, false);
        assert!(!input.any_down());

        input.set_mouse_button(MouseButton::Middle, true);
        assert!(input.any_down());
        input.set_focused(false);
        assert!(!input.any_down());
    }
}
