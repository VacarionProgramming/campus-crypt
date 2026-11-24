//! On-screen touch controls for phones and tablets (the web build).
//!
//! Nothing shows until the first touch, so desktop play is unchanged. While a
//! floor, the arena, floor 1's fight or the Risk It round needs movement, the
//! left part of the screen is a floating joystick (put a thumb down anywhere
//! there and drag) and the right side has buttons: FIRE (hold), ROLL, TALK
//! (E / Enter: talk, answer, confirm), HOOT (T) and a pause button. On every
//! other screen the controls hide and a tap is a click, as macroquad already
//! turns a touch into the mouse.
//!
//! The pad only reads touches and says what they mean (`TouchOut`); `main.rs`
//! merges that into the frame's `Input`, so every rule (choice locks, aim
//! assist) works the same as with a keyboard.

use macroquad::prelude::*;

use crate::art::{centered, VW};
#[cfg(test)]
use crate::art::VH;

/// Touches that start left of this x (virtual px) drive the joystick.
const STICK_ZONE_X: f32 = VW * 0.45;
/// Full deflection, in virtual px from where the thumb went down.
const STICK_RADIUS: f32 = 90.0;
const DEADZONE: f32 = 0.18;
/// Where the joystick is drawn while no thumb is on it.
const STICK_HOME: Vec2 = Vec2::new(190.0, 520.0);

fn stick_home() -> Vec2 {
    if crate::art::portrait() {
        vec2(100.0, crate::art::vh() - 120.0)
    } else {
        STICK_HOME
    }
}

fn stick_zone(p: Vec2) -> bool {
    if crate::art::portrait() {
        p.y > crate::art::vh() - 240.0 && p.x < crate::art::vw() * 0.5
    } else {
        p.x < STICK_ZONE_X && p.y > 150.0
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Button {
    Fire,
    Roll,
    Talk,
    Hoot,
    Pause,
}

pub const BUTTONS: [Button; 5] = [Button::Fire, Button::Roll, Button::Talk, Button::Hoot, Button::Pause];

impl Button {
    /// Centre and radius. Landscape keeps the 1280x720 spots.
    pub fn circle(self) -> (Vec2, f32) {
        if crate::art::portrait() {
            let h = crate::art::vh();
            return match self {
                Button::Fire => (vec2(450.0, h - 110.0), 62.0),
                Button::Roll => (vec2(330.0, h - 78.0), 44.0),
                Button::Talk => (vec2(450.0, h - 210.0), 44.0),
                Button::Hoot => (vec2(330.0, h - 170.0), 34.0),
                Button::Pause => (vec2(492.0, 88.0), 26.0),
            };
        }
        match self {
            Button::Fire => (vec2(1150.0, 590.0), 78.0),
            Button::Roll => (vec2(990.0, 640.0), 52.0),
            Button::Talk => (vec2(1165.0, 425.0), 52.0),
            Button::Hoot => (vec2(1010.0, 505.0), 40.0),
            Button::Pause => (vec2(1235.0, 112.0), 30.0),
        }
    }

    fn label(self) -> &'static str {
        match self {
            Button::Fire => "FIRE",
            Button::Roll => "ROLL",
            Button::Talk => "TALK",
            Button::Hoot => "HOOT",
            Button::Pause => "II",
        }
    }

    pub fn hit(self, p: Vec2) -> bool {
        let (c, r) = self.circle();
        // a little slack: thumbs are wide
        p.distance(c) <= r * 1.2
    }
}

/// What the touches meant this frame.
#[derive(Clone, Copy, Default, Debug, PartialEq)]
pub struct TouchOut {
    /// joystick direction, length 0..=1
    pub mv: Vec2,
    pub fire: bool,
    pub roll: bool,
    pub talk: bool,
    pub hoot: bool,
    pub pause: bool,
    /// one-shot menu steps from flicking the stick
    pub up: bool,
    pub down: bool,
    pub left: bool,
    pub right: bool,
    /// a touch is on the controls: its simulated mouse click must not count
    pub on_controls: bool,
}

#[derive(Default)]
pub struct TouchPad {
    /// a touch has been seen: this is a touch screen
    pub seen: bool,
    /// (touch id, where it went down, where it is now)
    stick: Option<(u64, Vec2, Vec2)>,
    /// the stick was pushed past the step threshold (for menu steps)
    stepped: bool,
    /// buttons held this frame (for drawing)
    held: [bool; 5],
}

/// One raw touch, in virtual coordinates.
#[derive(Clone, Copy, Debug)]
pub struct Point {
    pub id: u64,
    pub phase: TouchPhase,
    pub pos: Vec2,
}

impl TouchPad {
    /// Read this frame's touches. `to_virtual` maps screen points to the
    /// 1280x720 canvas; `controls` says whether the on-screen controls are up.
    pub fn update(&mut self, to_virtual: impl Fn(Vec2) -> Vec2, controls: bool) -> TouchOut {
        // macroquad reports touches in physical pixels but the mouse (and View) in logical ones
        let dpi = screen_dpi_scale().max(0.01);
        let points: Vec<Point> = touches().into_iter().map(|t| Point { id: t.id, phase: t.phase, pos: to_virtual(t.position / dpi) }).collect();
        self.step(&points, controls)
    }

    /// The logic behind `update`, kept apart from macroquad's touch list so it can be tested.
    pub fn step(&mut self, points: &[Point], controls: bool) -> TouchOut {
        let mut out = TouchOut::default();
        if !points.is_empty() {
            self.seen = true;
        }
        if !controls {
            self.stick = None;
            self.stepped = false;
            self.held = [false; 5];
            return out;
        }
        let mut held = [false; 5];
        for t in points {
            let live = !matches!(t.phase, TouchPhase::Ended | TouchPhase::Cancelled);
            // the stick follows its own thumb wherever it goes
            if let Some((id, origin, _)) = self.stick {
                if id == t.id {
                    out.on_controls = true;
                    self.stick = if live { Some((id, origin, t.pos)) } else { None };
                    continue;
                }
            }
            let button = BUTTONS.iter().copied().find(|b| b.hit(t.pos));
            match (t.phase, button) {
                (TouchPhase::Started, Some(b)) => {
                    out.on_controls = true;
                    match b {
                        Button::Fire => out.fire = true,
                        Button::Roll => out.roll = true,
                        Button::Talk => out.talk = true,
                        Button::Hoot => out.hoot = true,
                        Button::Pause => out.pause = true,
                    }
                    held[b as usize] = true;
                }
                (TouchPhase::Started, None) if stick_zone(t.pos) => {
                    out.on_controls = true;
                    self.stick = Some((t.id, t.pos, t.pos));
                }
                (_, Some(b)) if live => {
                    out.on_controls = true;
                    held[b as usize] = true;
                    if b == Button::Fire {
                        out.fire = true;
                    }
                }
                _ => {}
            }
        }
        self.held = held;

        if let Some((_, origin, now)) = self.stick {
            let d = (now - origin) / STICK_RADIUS;
            let len = d.length().min(1.0);
            if len > DEADZONE {
                out.mv = d.normalize() * ((len - DEADZONE) / (1.0 - DEADZONE));
            }
            // menu steps: one per push past 0.6, re-armed once the stick comes back under 0.35
            if len > 0.6 && !self.stepped {
                self.stepped = true;
                if d.x.abs() > d.y.abs() {
                    if d.x < 0.0 {
                        out.left = true;
                    } else {
                        out.right = true;
                    }
                } else if d.y < 0.0 {
                    out.up = true;
                } else {
                    out.down = true;
                }
            } else if len < 0.35 {
                self.stepped = false;
            }
        } else {
            self.stepped = false;
        }
        out
    }

    /// Where Esc is the only way out (Options, the tip log): just a BACK
    /// button where the pause button sits. A tap on it is a click.
    pub fn draw_back(&self) {
        let (c, r) = Button::Pause.circle();
        draw_circle(c.x, c.y, r * 1.5, Color::new(1.0, 1.0, 1.0, 0.18));
        draw_circle_lines(c.x, c.y, r * 1.5, 3.0, Color::new(1.0, 0.77, 0.16, 0.9));
        centered("BACK", c.x, c.y + 8.0, 22.0, Color::new(1.0, 1.0, 1.0, 0.95));
    }

    /// Draw the controls in canvas coordinates (call with the game camera set).
    pub fn draw(&self) {
        let (base, knob) = match self.stick {
            Some((_, origin, now)) => {
                let d = (now - origin).clamp_length_max(STICK_RADIUS);
                (origin, origin + d)
            }
            None => {
                let home = stick_home();
                (home, home)
            }
        };
        let active = self.stick.is_some();
        draw_circle(base.x, base.y, STICK_RADIUS, Color::new(1.0, 1.0, 1.0, if active { 0.12 } else { 0.07 }));
        draw_circle_lines(base.x, base.y, STICK_RADIUS, 3.0, Color::new(1.0, 0.77, 0.16, if active { 0.7 } else { 0.35 }));
        draw_circle(knob.x, knob.y, 36.0, Color::new(1.0, 0.77, 0.16, if active { 0.75 } else { 0.4 }));
        if !active {
            let home = stick_home();
            let (dy, size) = if crate::art::portrait() { (22.0, 18.0) } else { (30.0, 22.0) };
            centered("MOVE", home.x, home.y + STICK_RADIUS + dy, size, Color::new(1.0, 1.0, 1.0, 0.55));
        }
        for b in BUTTONS {
            let (c, r) = b.circle();
            let on = self.held[b as usize];
            let fill = if b == Button::Fire { Color::new(0.95, 0.36, 0.33, if on { 0.7 } else { 0.38 }) } else { Color::new(1.0, 1.0, 1.0, if on { 0.32 } else { 0.14 }) };
            draw_circle(c.x, c.y, r, fill);
            draw_circle_lines(c.x, c.y, r, 3.0, Color::new(1.0, 1.0, 1.0, if on { 0.9 } else { 0.5 }));
            // big enough to read when the whole canvas is shrunk onto a phone
            let size = if b == Button::Fire { 34.0 } else if b == Button::Pause { 24.0 } else if r < 45.0 { 22.0 } else { 26.0 };
            centered(b.label(), c.x, c.y + size * 0.35, size, Color::new(1.0, 1.0, 1.0, 0.9));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pt(id: u64, phase: TouchPhase, x: f32, y: f32) -> Point {
        Point { id, phase, pos: vec2(x, y) }
    }

    #[test]
    fn nothing_happens_without_touches() {
        let mut pad = TouchPad::default();
        assert_eq!(pad.step(&[], true), TouchOut::default());
        assert!(!pad.seen);
    }

    #[test]
    fn the_stick_floats_where_the_thumb_lands() {
        let mut pad = TouchPad::default();
        pad.step(&[pt(1, TouchPhase::Started, 300.0, 500.0)], true);
        let out = pad.step(&[pt(1, TouchPhase::Moved, 390.0, 500.0)], true);
        assert!(out.mv.x > 0.95 && out.mv.y.abs() < 0.01, "{:?}", out.mv);
        assert!(out.right, "a full push is one menu step");
        let out = pad.step(&[pt(1, TouchPhase::Moved, 395.0, 500.0)], true);
        assert!(!out.right, "one step per push");
        let out = pad.step(&[pt(1, TouchPhase::Ended, 395.0, 500.0)], true);
        assert_eq!(out.mv, Vec2::ZERO);
    }

    #[test]
    fn a_small_wiggle_is_ignored() {
        let mut pad = TouchPad::default();
        pad.step(&[pt(1, TouchPhase::Started, 300.0, 500.0)], true);
        let out = pad.step(&[pt(1, TouchPhase::Moved, 310.0, 505.0)], true);
        assert_eq!(out.mv, Vec2::ZERO);
    }

    #[test]
    fn fire_holds_while_other_buttons_tap() {
        let mut pad = TouchPad::default();
        let (fire, _) = Button::Fire.circle();
        let (roll, _) = Button::Roll.circle();
        let out = pad.step(&[pt(2, TouchPhase::Started, fire.x, fire.y)], true);
        assert!(out.fire && out.on_controls);
        let out = pad.step(&[pt(2, TouchPhase::Stationary, fire.x, fire.y), pt(3, TouchPhase::Started, roll.x, roll.y)], true);
        assert!(out.fire && out.roll);
        let out = pad.step(&[pt(2, TouchPhase::Stationary, fire.x, fire.y), pt(3, TouchPhase::Stationary, roll.x, roll.y)], true);
        assert!(out.fire && !out.roll, "roll is a tap, fire is held");
    }

    #[test]
    fn stick_and_fire_at_once() {
        let mut pad = TouchPad::default();
        let (fire, _) = Button::Fire.circle();
        pad.step(&[pt(1, TouchPhase::Started, 200.0, 550.0)], true);
        let out = pad.step(&[pt(1, TouchPhase::Moved, 200.0, 460.0), pt(2, TouchPhase::Started, fire.x, fire.y)], true);
        assert!(out.fire && out.mv.y < -0.9);
    }

    #[test]
    fn hidden_controls_take_nothing() {
        let mut pad = TouchPad::default();
        let (talk, _) = Button::Talk.circle();
        let out = pad.step(&[pt(1, TouchPhase::Started, talk.x, talk.y)], false);
        assert_eq!(out, TouchOut::default());
        assert!(pad.seen, "still learns it's a touch screen");
    }

    #[test]
    fn controls_stay_on_the_canvas() {
        for b in BUTTONS {
            let (c, r) = b.circle();
            assert!(c.x - r >= 0.0 && c.x + r <= VW && c.y - r >= 0.0 && c.y + r <= VH, "{b:?}");
        }
        const { assert!(STICK_HOME.x + STICK_RADIUS < STICK_ZONE_X); }
    }
}
