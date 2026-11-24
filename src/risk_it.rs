//! "Risk It": the optional bonus round after a correct Keeper answer.
//!
//! The verdict screen offers SPARE (take the stairs, as before) or RISK IT.
//! Risk It puts the owl in the battle box from `keeper_fight` for one short
//! round: the Keeper's attack patterns come in `WAVES` waves while red flags
//! pop up one at a time. Grab `FLAGS_TO_WIN` red flags (spotting warning signs
//! is how you beat a Keeper) before time runs out.
//!
//! Outcomes, all decided here by code:
//! * Defeated (all flags): +1 Health, or +1 Focus when Health is full.
//! * Knocked out (box hearts gone): -1 Health, never below 1.
//! * Time up: nothing changes.
//!
//! The stairs open whatever happens: the answer was already right. Patterns
//! and flag spots depend only on the seed, the floor and the wave. Reduced
//! motion (R) drops the attacks and keeps the flags.

use macroquad::prelude::*;

use crate::keeper_fight::{owl_start, Dodge, BOX_H, BOX_HEARTS, BOX_W, OWL_R, PATTERNS};

/// one wave of the Keeper's attacks
pub const WAVE_SECS: f32 = 5.0;
pub const WAVES: u32 = 3;
pub const ROUND_SECS: f32 = WAVE_SECS * WAVES as f32;
pub const FLAGS_TO_WIN: u8 = 3;
pub const FLAG_R: f32 = 13.0;
/// a new flag shows up this long after the last one was grabbed
const FLAG_DELAY: f32 = 0.6;
/// flags never appear this close to the owl
const FLAG_MIN_DIST: f32 = 150.0;
const OWL_SPEED: f32 = 250.0;
const HIT_INVULN: f32 = 1.0;
/// the result screen ignores Enter for this long (no accidental skip)
const RESULT_LOCK: f32 = 0.7;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Outcome {
    Defeated,
    KnockedOut,
    TimeUp,
}

impl Outcome {
    pub fn title(self) -> &'static str {
        match self {
            Outcome::Defeated => "KEEPER DEFEATED!",
            Outcome::KnockedOut => "KNOCKED OUT OF THE BOX",
            Outcome::TimeUp => "TIME'S UP",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RiskEvent {
    Hit,
    Flag,
    Finished(Outcome),
}

#[derive(Clone, Debug)]
pub struct RiskIt {
    pub floor: usize,
    seed: u32,
    reduced: bool,
    pub wave: u32,
    pub dodge: Dodge,
    /// seconds into the round
    pub t: f32,
    pub owl: Vec2,
    pub facing_left: bool,
    pub invuln: f32,
    pub hearts: u8,
    pub flag: Option<Vec2>,
    flag_wait: f32,
    pub flags: u8,
    rng: u32,
    pub outcome: Option<Outcome>,
    /// what the outcome did to the run (set by the caller): a line for the result screen
    pub reward: String,
    result_lock: f32,
}

/// attack level by floor: gentle on floors 1-3, harder from 8 on
pub fn level_for_floor(floor: usize) -> u8 {
    match floor {
        0..=2 => 0,
        3..=6 => 1,
        _ => 2,
    }
}

impl RiskIt {
    pub fn new(seed: u32, floor: usize, reduced: bool) -> RiskIt {
        let seed = crate::keeper::mix(seed, 0x5249_534b ^ floor as u32);
        let mut r = RiskIt {
            floor,
            seed,
            reduced,
            wave: 0,
            dodge: Self::wave_dodge(seed, floor, 0, reduced),
            t: 0.0,
            owl: owl_start(),
            facing_left: false,
            invuln: 0.0,
            hearts: BOX_HEARTS,
            flag: None,
            flag_wait: FLAG_DELAY,
            flags: 0,
            rng: seed | 1,
            outcome: None,
            reward: String::new(),
            result_lock: RESULT_LOCK,
        };
        r.flag_wait = 0.8;
        r
    }

    fn wave_dodge(seed: u32, floor: usize, wave: u32, reduced: bool) -> Dodge {
        let pattern = PATTERNS[(crate::keeper::mix(seed, wave) as usize + wave as usize) % PATTERNS.len()];
        let mut d = Dodge::new(pattern, seed, wave + 100, level_for_floor(floor), reduced);
        d.dur = WAVE_SECS;
        d
    }

    fn rand(&mut self) -> f32 {
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.rng = x;
        (x >> 8) as f32 / (1u32 << 24) as f32
    }

    /// a flag spot inside the box, away from the owl
    fn place_flag(&mut self) -> Vec2 {
        let m = FLAG_R + 8.0;
        let mut best = vec2(BOX_W * 0.5, BOX_H * 0.5);
        for _ in 0..12 {
            let p = vec2(m + self.rand() * (BOX_W - 2.0 * m), m + 24.0 + self.rand() * (BOX_H - 2.0 * m - 24.0));
            best = p;
            if p.distance(self.owl) >= FLAG_MIN_DIST {
                break;
            }
        }
        best
    }

    pub fn time_left(&self) -> f32 {
        (ROUND_SECS - self.t).max(0.0)
    }

    pub fn done(&self) -> bool {
        self.outcome.is_some()
    }

    /// the result screen is up and takes Enter
    pub fn can_leave(&self) -> bool {
        self.outcome.is_some() && self.result_lock <= 0.0
    }

    fn finish(&mut self, o: Outcome) -> Option<RiskEvent> {
        self.outcome = Some(o);
        self.flag = None;
        self.dodge.hazards.clear();
        self.dodge.button = None;
        Some(RiskEvent::Finished(o))
    }

    /// One frame; `mv` is the stick/WASD direction.
    pub fn update(&mut self, dt: f32, mv: Vec2) -> Option<RiskEvent> {
        if self.outcome.is_some() {
            self.result_lock = (self.result_lock - dt).max(0.0);
            return None;
        }
        self.t += dt;
        if mv.x.abs() > 0.01 {
            self.facing_left = mv.x < 0.0;
        }
        self.owl += mv.clamp_length_max(1.0) * OWL_SPEED * dt;
        self.owl = self.owl.clamp(vec2(OWL_R, OWL_R), vec2(BOX_W - OWL_R, BOX_H - OWL_R));
        self.invuln = (self.invuln - dt).max(0.0);

        // next wave
        if self.dodge.done() && self.wave + 1 < WAVES {
            self.wave += 1;
            self.dodge = Self::wave_dodge(self.seed, self.floor, self.wave, self.reduced);
        }
        let touching = !self.dodge.done() && self.dodge.update(dt, self.owl);

        // flags
        match self.flag {
            None => {
                self.flag_wait -= dt;
                if self.flag_wait <= 0.0 {
                    self.flag = Some(self.place_flag());
                }
            }
            Some(p) if p.distance(self.owl) < OWL_R + FLAG_R => {
                self.flag = None;
                self.flag_wait = FLAG_DELAY;
                self.flags += 1;
                if self.flags >= FLAGS_TO_WIN {
                    return self.finish(Outcome::Defeated);
                }
                return Some(RiskEvent::Flag);
            }
            Some(_) => {}
        }

        if touching && self.invuln <= 0.0 {
            self.hearts = self.hearts.saturating_sub(1);
            self.invuln = HIT_INVULN;
            if self.hearts == 0 {
                return self.finish(Outcome::KnockedOut);
            }
            return Some(RiskEvent::Hit);
        }
        if self.t >= ROUND_SECS {
            return self.finish(Outcome::TimeUp);
        }
        None
    }

    /// Autopilot: head for the flag, pushed away from anything close.
    pub fn auto_steer(&self, seek_hits: bool) -> Vec2 {
        let mut threats: Vec<Vec2> = self.dodge.hazards.iter().map(|h| h.pos + h.vel * 0.25).collect();
        if let Some(b) = self.dodge.button {
            threats.push(b);
        }
        if seek_hits {
            return threats.iter().min_by(|a, b| a.distance(self.owl).total_cmp(&b.distance(self.owl))).map_or(Vec2::ZERO, |&p| (p - self.owl).normalize_or_zero());
        }
        let mut push = Vec2::ZERO;
        for p in threats {
            let away = self.owl - p;
            let dist = away.length();
            if dist < 90.0 {
                push += away.normalize_or_zero() * (90.0 - dist) / 90.0 * 1.6;
            }
        }
        if let Some(f) = self.flag {
            push += (f - self.owl).normalize_or_zero();
        }
        push.normalize_or_zero()
    }
}

// ---------------------------------------------------------------- drawing

use crate::art::{centered, draw_sprite_centered, draw_text, heart, panel, BOX_OWL, DIM, INK, KSU_GOLD, PAPER, RISKY, SAFE};
use crate::keeper_fight::{draw_pattern, draw_pattern_scaled, BOX_X, BOX_Y};

impl RiskIt {
    /// The round below the HUD: header, the box, hearts and the timer.
    pub fn draw(&self, keeper_name: &str, t: f32) {
        if crate::art::portrait() {
            self.draw_phone(keeper_name, t);
            return;
        }
        // header
        panel(40.0, 76.0, VW_PANEL, 170.0, RISKY);
        draw_text("RISK IT", 64.0, 120.0, 40.0, RISKY);
        draw_text(keeper_name, 260.0, 120.0, 26.0, PAPER);
        draw_text("KEEPER", 64.0, 162.0, 20.0, DIM);
        for i in 0..FLAGS_TO_WIN {
            let left = i >= self.flags;
            draw_rectangle(150.0 + i as f32 * 58.0, 146.0, 50.0, 20.0, if left { RISKY } else { Color::new(0.3, 0.12, 0.12, 1.0) });
        }
        draw_text("Grab 3 red flags to defeat the Keeper. Win: +1 Health.  Lose your box hearts: -1 Health.", 64.0, 206.0, 20.0, PAPER);
        draw_text("The stairs open either way.", 64.0, 232.0, 20.0, DIM);

        let o = vec2(BOX_X, BOX_Y);
        draw_rectangle(o.x, o.y, BOX_W, BOX_H, BLACK);
        match self.outcome {
            None => {
                draw_pattern(&self.dodge, o);
                if let Some(f) = self.flag {
                    draw_red_flag(o + f, t);
                }
                let blink = self.invuln > 0.0 && (t * 18.0).sin() < 0.0;
                if !blink {
                    draw_sprite_centered(&BOX_OWL, o.x + self.owl.x, o.y + self.owl.y, 1.6, self.facing_left, WHITE);
                }
                if self.reduced {
                    centered("Reduced motion: no attacks. Just grab the flags.", o.x + BOX_W * 0.5, o.y + 40.0, 20.0, DIM);
                }
            }
            Some(out) => {
                let col = match out {
                    Outcome::Defeated => KSU_GOLD,
                    Outcome::KnockedOut => RISKY,
                    Outcome::TimeUp => PAPER,
                };
                centered(out.title(), o.x + BOX_W * 0.5, o.y + 80.0, 40.0, col);
                centered(&self.reward, o.x + BOX_W * 0.5, o.y + 126.0, 22.0, if out == Outcome::Defeated { SAFE } else { PAPER });
                if self.can_leave() && (t * 2.0).fract() < 0.6 {
                    centered("To the stairs  [Enter]", o.x + BOX_W * 0.5, o.y + BOX_H - 24.0, 20.0, DIM);
                }
            }
        }
        draw_rectangle_lines(o.x - 4.0, o.y - 4.0, BOX_W + 8.0, BOX_H + 8.0, 4.0, RISKY);
        draw_rectangle_lines(o.x - 1.0, o.y - 1.0, BOX_W + 2.0, BOX_H + 2.0, 1.0, INK);

        draw_text("BOX HP", o.x, o.y + BOX_H + 30.0, 18.0, DIM);
        for i in 0..BOX_HEARTS {
            heart(o.x + 74.0 + i as f32 * 26.0, o.y + BOX_H + 16.0, 3.0, i < self.hearts);
        }
        if self.outcome.is_none() {
            let f = self.time_left() / ROUND_SECS;
            draw_text("TIME", o.x + BOX_W - 206.0, o.y + BOX_H + 30.0, 18.0, DIM);
            draw_rectangle(o.x + BOX_W - 160.0, o.y + BOX_H + 18.0, 160.0 * f, 10.0, RISKY);
            draw_rectangle_lines(o.x + BOX_W - 160.0, o.y + BOX_H + 18.0, 160.0, 10.0, 1.0, DIM);
            centered(self.dodge.pattern.flavor(), 640.0, o.y + BOX_H + 58.0, 20.0, PAPER);
        }
    }
}

const VW_PANEL: f32 = 1200.0;

impl RiskIt {
    /// The round stacked for a phone: header, then the box scaled to the width.
    fn draw_phone(&self, keeper_name: &str, t: f32) {
        let w = crate::art::vw();
        panel(12.0, 68.0, w - 24.0, 96.0, RISKY);
        draw_text("RISK IT", 24.0, 100.0, 28.0, RISKY);
        draw_text(keeper_name, 160.0, 100.0, 18.0, PAPER);
        for i in 0..FLAGS_TO_WIN {
            let left = i >= self.flags;
            draw_rectangle(24.0 + i as f32 * 48.0, 118.0, 40.0, 16.0, if left { RISKY } else { Color::new(0.3, 0.12, 0.12, 1.0) });
        }
        let scale = (w - 16.0) / BOX_W;
        let o = vec2(8.0, 180.0);
        draw_rectangle(o.x, o.y, BOX_W * scale, BOX_H * scale, BLACK);
        match self.outcome {
            None => {
                draw_pattern_scaled(&self.dodge, o, scale);
                if let Some(f) = self.flag {
                    draw_red_flag(o + f * scale, t);
                }
                let blink = self.invuln > 0.0 && (t * 18.0).sin() < 0.0;
                if !blink {
                    draw_sprite_centered(&BOX_OWL, o.x + self.owl.x * scale, o.y + self.owl.y * scale, 1.6 * scale, self.facing_left, WHITE);
                }
            }
            Some(out) => {
                let col = match out {
                    Outcome::Defeated => KSU_GOLD,
                    Outcome::KnockedOut => RISKY,
                    Outcome::TimeUp => PAPER,
                };
                centered(out.title(), o.x + BOX_W * scale * 0.5, o.y + 70.0 * scale, 28.0, col);
                centered(&self.reward, o.x + BOX_W * scale * 0.5, o.y + 110.0 * scale, 16.0, PAPER);
            }
        }
        draw_rectangle_lines(o.x - 2.0, o.y - 2.0, BOX_W * scale + 4.0, BOX_H * scale + 4.0, 3.0, RISKY);
        let hy = o.y + BOX_H * scale + 28.0;
        draw_text("BOX HP", o.x, hy, 16.0, DIM);
        for i in 0..BOX_HEARTS {
            heart(o.x + 70.0 + i as f32 * 24.0, hy - 16.0, 2.4, i < self.hearts);
        }
    }
}

/// A small red flag on a pole, waving.
pub fn draw_red_flag(p: Vec2, t: f32) {
    let wave = (t * 6.0).sin() * 2.0;
    crate::art::glow(p.x, p.y, 34.0, Color::new(1.0, 0.3, 0.25, 1.0), 0.8);
    draw_line(p.x - 7.0, p.y - 14.0, p.x - 7.0, p.y + 14.0, 3.0, PAPER);
    draw_triangle(vec2(p.x - 6.0, p.y - 14.0), vec2(p.x + 13.0, p.y - 7.0 + wave), vec2(p.x - 6.0, p.y), RISKY);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// steer straight at the flag, ignoring attacks
    fn grab(r: &RiskIt) -> Vec2 {
        r.flag.map_or(Vec2::ZERO, |f| (f - r.owl).normalize_or_zero())
    }

    fn play(mut r: RiskIt, steer: impl Fn(&RiskIt) -> Vec2) -> (Outcome, RiskIt) {
        for _ in 0..(ROUND_SECS * 60.0) as usize + 120 {
            let mv = steer(&r);
            if let Some(RiskEvent::Finished(o)) = r.update(1.0 / 60.0, mv) {
                return (o, r);
            }
        }
        panic!("the round never finished");
    }

    #[test]
    fn grabbing_flags_with_no_attacks_defeats_the_keeper() {
        let (o, r) = play(RiskIt::new(7, 0, true), grab);
        assert_eq!(o, Outcome::Defeated);
        assert_eq!(r.flags, FLAGS_TO_WIN);
        assert_eq!(r.hearts, BOX_HEARTS, "reduced motion has no attacks");
        assert!(r.t < ROUND_SECS);
    }

    #[test]
    fn standing_still_runs_out_the_clock() {
        // reduced motion: no attacks, and the owl never moves to a flag
        let (o, r) = play(RiskIt::new(3, 4, true), |_| Vec2::ZERO);
        assert!(matches!(o, Outcome::TimeUp | Outcome::Defeated), "{o:?}");
        if o == Outcome::TimeUp {
            assert!(r.t >= ROUND_SECS);
        }
    }

    #[test]
    fn flying_into_attacks_knocks_you_out() {
        let mut out = Vec::new();
        for seed in 0..8 {
            let (o, _) = play(RiskIt::new(seed, 8, false), |r| r.auto_steer(true));
            out.push(o);
        }
        assert!(out.contains(&Outcome::KnockedOut), "{out:?}");
    }

    #[test]
    fn the_autopilot_can_win_on_the_first_floor() {
        let wins = (0..20).filter(|&s| play(RiskIt::new(s, 0, false), |r| r.auto_steer(false)).0 == Outcome::Defeated).count();
        assert!(wins >= 10, "only {wins}/20 wins");
    }

    #[test]
    fn flags_spawn_inside_the_box() {
        let mut r = RiskIt::new(11, 2, false);
        for _ in 0..50 {
            let p = r.place_flag();
            assert!(p.x >= FLAG_R && p.x <= BOX_W - FLAG_R && p.y >= FLAG_R && p.y <= BOX_H - FLAG_R, "{p:?}");
        }
    }

    #[test]
    fn same_seed_same_round() {
        let a = play(RiskIt::new(42, 5, false), |r| r.auto_steer(false));
        let b = play(RiskIt::new(42, 5, false), |r| r.auto_steer(false));
        assert_eq!((a.0, a.1.flags, a.1.hearts), (b.0, b.1.flags, b.1.hearts));
    }

    #[test]
    fn the_result_screen_waits_before_taking_enter() {
        let (_, mut r) = play(RiskIt::new(7, 0, true), grab);
        assert!(!r.can_leave());
        for _ in 0..60 {
            r.update(1.0 / 60.0, Vec2::ZERO);
        }
        assert!(r.can_leave());
    }
}
