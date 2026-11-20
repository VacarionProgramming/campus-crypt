//! Floor 1's Keeper fight (prototype): the Inbox Mimic, Undertale-style.
//!
//! Turns alternate. On the player's turn a command menu offers ACT, INSPECT,
//! SPARE and DELETE ("just delete it"); on the Mimic's turn the player steers
//! a tiny owl around the battle box and dodges a themed attack pattern.
//!
//! The lesson lives in ACT and INSPECT:
//! * INSPECT (free, no Mimic turn) shows the message's details: sender, where
//!   the link really goes, the small print, the deadline.
//! * Good ACTs ("Check sender", "Hover the link", "Call the office yourself",
//!   "Wait it out") make the Mimic flinch and give away one tell, in character.
//!   `SPARE_AT` distinct tells and the Mimic is spareable: its name turns gold.
//! * Bad ACTs ("Reply with password", "Pay the release fee", "Click Review")
//!   power it up (harder patterns) and it gloats. `LOSE_AT` of them and it wins
//!   the argument.
//!
//! Verdicts, all decided here by code (never by an AI line):
//! * SPARE when spareable -> right (stairs open, no wardens). SPARE earlier is
//!   just taunted.
//! * DELETE with no tells found -> wrong (2 wardens). DELETE after `SPARE_AT`
//!   tells -> right (deleting a fake you've confirmed is fine). DELETE in
//!   between is taunted ("you aren't even sure I'm fake").
//! * `LOSE_AT` bad acts -> wrong (2 wardens).
//!
//! Hits only cost temporary box hearts. Out of hearts, the encounter resets
//! (tells and power-ups cleared, fresh hearts); it never gives or changes a
//! verdict. Attack patterns depend only on the seed, the turn number and the
//! escalation level, never on which act is the "right" one. Reduced motion
//! (R) drops the hazards and shortens the Mimic's turn.

use macroquad::prelude::*;

pub const BOX_W: f32 = 640.0;
pub const BOX_H: f32 = 220.0;
pub const BOX_HEARTS: u8 = 3;
/// a freshly opened menu / text ignores input for this long (as every choice screen)
pub const LOCK_SECS: f32 = 0.4;
pub const OWL_R: f32 = 10.0;
const OWL_SPEED: f32 = 240.0;
pub const HAZARD_R: f32 = 9.0;
const HIT_INVULN: f32 = 1.0;
pub const DODGE_SECS: f32 = 6.0;
pub const DODGE_SECS_REDUCED: f32 = 1.5;
/// distinct tells needed before the Mimic can be spared
pub const SPARE_AT: usize = 3;
/// bad acts until the Mimic wins the argument
pub const LOSE_AT: u8 = 3;
/// escalation levels 0..=MAX_LEVEL (one per bad act)
pub const MAX_LEVEL: u8 = 2;
pub const BUTTON_W: f32 = 132.0;
pub const BUTTON_H: f32 = 30.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Command {
    Act,
    Inspect,
    Spare,
    Delete,
}

pub const COMMANDS: [Command; 4] = [Command::Act, Command::Inspect, Command::Spare, Command::Delete];

impl Command {
    pub fn label(self) -> &'static str {
        match self {
            Command::Act => "ACT",
            Command::Inspect => "INSPECT",
            Command::Spare => "SPARE",
            Command::Delete => "DELETE",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Act {
    CheckSender,
    HoverLink,
    CallOffice,
    WaitItOut,
    ReplyPassword,
    PayFee,
    ClickReview,
}

pub const ACTS: [Act; 7] = [Act::CheckSender, Act::HoverLink, Act::CallOffice, Act::WaitItOut, Act::ReplyPassword, Act::PayFee, Act::ClickReview];

impl Act {
    pub fn label(self) -> &'static str {
        match self {
            Act::CheckSender => "Check sender",
            Act::HoverLink => "Hover the link",
            Act::CallOffice => "Call the office yourself",
            Act::WaitItOut => "Wait it out",
            Act::ReplyPassword => "Reply with password",
            Act::PayFee => "Pay the release fee",
            Act::ClickReview => "Click Review",
        }
    }

    /// a good act: it exposes one of the Mimic's tells
    pub fn is_tell(self) -> bool {
        matches!(self, Act::CheckSender | Act::HoverLink | Act::CallOffice | Act::WaitItOut)
    }

    fn bit(self) -> u8 {
        1 << (self as u8)
    }

    /// what the player does (shown in the box)
    pub fn flavor(self) -> &'static str {
        match self {
            Act::CheckSender => "* You read the sender address. Slowly. Letter by letter.",
            Act::HoverLink => "* You rest the cursor on Review without clicking. A tiny address appears.",
            Act::CallOffice => "* You look up the aid office's number yourself and call.",
            Act::WaitItOut => "* You put the phone down and let 5 PM come and go.",
            Act::ReplyPassword => "* You start typing your password into a reply.",
            Act::PayFee => "* You reach for your card to pay a release fee.",
            Act::ClickReview => "* You click Review. A login box pops up, very eager.",
        }
    }

    /// the Mimic's answer (first person, playful)
    pub fn mimic_line(self) -> &'static str {
        match self {
            Act::CheckSender => "...k-e-n-e-s-a-w? I spell it how I like! One N is plenty!",
            Act::HoverLink => "No peeking! ...Fine, it goes to kenesaw-aid.example. Close enough, right?",
            Act::CallOffice => "You found the number YOURSELF? And they never heard of me? Rude.",
            Act::WaitItOut => "Tick, tock... 5:01 and nothing happened?! The deadline was decorative.",
            Act::ReplyPassword => "Ooh, keep typing! Every letter makes my envelopes sharper!",
            Act::PayFee => "A fee to get your own refund? Love it. Powering up!",
            Act::ClickReview => "Click! The login page missed you. Let's make this harder!",
        }
    }
}

const REPEAT_LINE: &str = "You already caught me on that one. Got anything new?";
const SPARE_EARLY: &str = "Spare me? You haven't even figured me out yet!";
const SPARE_OK: &str = "...Fine. You win. I was never the aid office.";
const DELETE_EARLY: &str = "Delete me? You aren't even sure I'm fake yet!";
const DELETE_UNREAD: &str = "Delete me unread? Then you never learn my tricks. Wardens!";
const DELETE_OK: &str = "Deleted, with evidence. Cold. Correct, but cold.";
const ARGUMENT_WON: &str = "I win the argument! Wardens, take it from here!";
const RESET_LINE: &str = "Out of hearts? Snap! Fresh start. I forget everything, and so do you.";
const OPENING: &str = "Psst. Your refund is READY. Just one tiny review first...";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Pattern {
    /// URGENT envelopes raining down
    Envelopes,
    /// a 5:00 PM clock hand sweeping the box
    Clock,
    /// @ signs slowly homing in
    Homing,
    /// a fake CONFIRM LOGIN button that chases you
    ConfirmButton,
}

pub const PATTERNS: [Pattern; 4] = [Pattern::Envelopes, Pattern::Clock, Pattern::Homing, Pattern::ConfirmButton];

impl Pattern {
    /// one quiet line per pattern, shown under the box while it plays
    pub fn flavor(self) -> &'static str {
        match self {
            Pattern::Envelopes => "* URGENT envelopes pour in. When everything is urgent, nothing is.",
            Pattern::Clock => "* A 5:00 PM clock hand sweeps the box. Real offices give you more than an afternoon.",
            Pattern::Homing => "* @ signs drift after you, each a slightly different address.",
            Pattern::ConfirmButton => "* A shiny CONFIRM LOGIN button chases you. Real offices don't chase.",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HazardKind {
    Envelope,
    At,
    Numeral,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hazard {
    pub kind: HazardKind,
    pub pos: Vec2,
    pub vel: Vec2,
    pub t: f32,
}

/// The Mimic's turn: one attack pattern at one escalation level. Built from
/// (seed, turn, level, reduced) only.
#[derive(Clone, Debug)]
pub struct Dodge {
    pub pattern: Pattern,
    pub level: u8,
    pub t: f32,
    pub dur: f32,
    pub hazards: Vec<Hazard>,
    /// the clock hand's angle from straight down (Clock)
    pub hand: f32,
    /// the CONFIRM LOGIN button's centre (ConfirmButton)
    pub button: Option<Vec2>,
    pub reduced: bool,
    spawn_t: f32,
    rng: u32,
    phase: f32,
}

pub const HAND_PIVOT: Vec2 = Vec2::new(BOX_W * 0.5, 6.0);
pub const HAND_LEN: f32 = BOX_H * 0.96;
/// The clock attack's "5:00 PM" label (box-local rect): the top-left corner, which the
/// hand can never reach at any level (see `clock_label_stays_clear_of_the_hand`).
pub const CLOCK_LABEL: Rect = Rect { x: 12.0, y: 8.0, w: 92.0, h: 24.0 };

fn mix(a: u32, b: u32) -> u32 {
    crate::keeper::mix(a, b)
}

impl Dodge {
    pub fn new(pattern: Pattern, seed: u32, turn: u32, level: u8, reduced: bool) -> Dodge {
        let mut d = Dodge {
            pattern,
            level,
            t: 0.0,
            dur: if reduced { DODGE_SECS_REDUCED } else { DODGE_SECS },
            hazards: Vec::with_capacity(48),
            hand: 0.0,
            button: None,
            reduced,
            spawn_t: 0.3,
            rng: mix(seed, turn.wrapping_mul(97) + level as u32) | 1,
            phase: 0.0,
        };
        d.phase = d.rand() * std::f32::consts::TAU;
        if pattern == Pattern::ConfirmButton && !reduced {
            d.button = Some(vec2(if d.rand() < 0.5 { 80.0 } else { BOX_W - 80.0 }, 30.0));
        }
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

    fn lv(&self) -> f32 {
        self.level as f32
    }

    /// seconds between spawns for this pattern and level (falls with level)
    pub fn spawn_every(&self) -> f32 {
        match self.pattern {
            Pattern::Envelopes => 0.42 - 0.05 * self.lv(),
            Pattern::Clock => 1.1 - 0.15 * self.lv(),
            Pattern::Homing => 0.95 - 0.12 * self.lv(),
            Pattern::ConfirmButton => 0.9 - 0.1 * self.lv(),
        }
    }

    /// clock hand swing (radians either side of straight down) and speed
    pub fn hand_swing(&self) -> (f32, f32) {
        (1.0 + 0.1 * self.lv(), 1.2 + 0.2 * self.lv())
    }

    /// how fast the CONFIRM LOGIN button chases
    pub fn button_speed(&self) -> f32 {
        75.0 + 15.0 * self.lv()
    }

    pub fn hand_tip(&self) -> Vec2 {
        HAND_PIVOT + vec2(self.hand.sin(), self.hand.cos()) * HAND_LEN
    }

    pub fn done(&self) -> bool {
        self.t >= self.dur
    }

    /// Advance; true if the owl at `owl` is touching something.
    pub fn update(&mut self, dt: f32, owl: Vec2) -> bool {
        self.t += dt;
        if self.reduced {
            return false;
        }
        let lv = self.lv();
        // spawns
        self.spawn_t -= dt;
        while self.spawn_t <= 0.0 {
            self.spawn_t += self.spawn_every();
            self.spawn();
        }
        // the hand and the button
        if self.pattern == Pattern::Clock {
            let (amp, w) = self.hand_swing();
            self.hand = (self.t * w + self.phase).sin() * amp;
        }
        let speed = self.button_speed();
        if let Some(b) = &mut self.button {
            *b += (owl - *b).normalize_or_zero() * speed * dt;
            *b = b.clamp(vec2(BUTTON_W * 0.5, BUTTON_H * 0.5), vec2(BOX_W - BUTTON_W * 0.5, BOX_H - BUTTON_H * 0.5));
        }
        // hazards
        for h in &mut self.hazards {
            h.t += dt;
            if h.kind == HazardKind::At {
                // slow homing: turn toward the owl a little each frame
                let want = (owl - h.pos).normalize_or_zero() * (55.0 + 25.0 * lv);
                h.vel += (want - h.vel) * (dt * 1.2).min(1.0);
            }
            h.pos += h.vel * dt;
        }
        self.hazards.retain(|h| {
            let inside = h.pos.x > -20.0 && h.pos.x < BOX_W + 20.0 && h.pos.y > -24.0 && h.pos.y < BOX_H - HAZARD_R;
            inside && !(h.kind == HazardKind::At && h.t > 4.5)
        });
        // contact
        let hit_hazard = self.hazards.iter().position(|h| h.pos.distance(owl) < OWL_R + HAZARD_R - 2.0);
        if let Some(k) = hit_hazard {
            self.hazards.swap_remove(k);
            return true;
        }
        if self.pattern == Pattern::Clock && seg_dist(owl, HAND_PIVOT, self.hand_tip()) < OWL_R + 3.0 {
            return true;
        }
        if let Some(b) = self.button {
            let r = Rect::new(b.x - BUTTON_W * 0.5, b.y - BUTTON_H * 0.5, BUTTON_W, BUTTON_H);
            let c = vec2(owl.x.clamp(r.x, r.x + r.w), owl.y.clamp(r.y, r.y + r.h));
            if c.distance(owl) < OWL_R {
                return true;
            }
        }
        false
    }

    fn spawn(&mut self) {
        let lv = self.lv();
        match self.pattern {
            Pattern::Envelopes | Pattern::ConfirmButton => {
                let x = HAZARD_R + self.rand() * (BOX_W - 2.0 * HAZARD_R);
                let vx = if self.level >= 2 { (self.rand() - 0.5) * 120.0 } else { 0.0 };
                let vy = 115.0 + 15.0 * lv + self.rand() * 30.0;
                self.hazards.push(Hazard { kind: HazardKind::Envelope, pos: vec2(x, -HAZARD_R), vel: vec2(vx, vy), t: 0.0 });
            }
            Pattern::Clock => {
                // falling 5s and 00s around the dial
                let x = HAZARD_R + self.rand() * (BOX_W - 2.0 * HAZARD_R);
                let vy = 90.0 + 12.0 * lv;
                self.hazards.push(Hazard { kind: HazardKind::Numeral, pos: vec2(x, -HAZARD_R), vel: vec2(0.0, vy), t: 0.0 });
            }
            Pattern::Homing => {
                let side = (self.rand() * 3.0) as u32;
                let pos = match side {
                    0 => vec2(-HAZARD_R, self.rand() * BOX_H * 0.8),
                    1 => vec2(BOX_W + HAZARD_R, self.rand() * BOX_H * 0.8),
                    _ => vec2(self.rand() * BOX_W, -HAZARD_R),
                };
                self.hazards.push(Hazard { kind: HazardKind::At, pos, vel: Vec2::ZERO, t: 0.0 });
            }
        }
    }
}

fn seg_dist(p: Vec2, a: Vec2, b: Vec2) -> f32 {
    let ab = b - a;
    let t = ((p - a).dot(ab) / ab.length_squared().max(1e-6)).clamp(0.0, 1.0);
    p.distance(a + ab * t)
}

pub fn owl_start() -> Vec2 {
    vec2(BOX_W * 0.5, BOX_H - 30.0)
}

/// What happens after a line of dialogue.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Then {
    Menu,
    Dodge,
    Verdict(bool),
}

#[derive(Clone, Debug)]
pub enum Stage {
    /// the Mimic speaks (bubble) and the box narrates; Enter goes on
    Talk(Then),
    Menu,
    Acts,
    Inspect,
    Dodge(Dodge),
    /// verdict reached (the run has been told)
    Done,
}

/// How a fight ended, for the Keeper's last line.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ending {
    Spared(Act),
    Deleted(Act),
    ArgumentLost,
    DeletedUnread,
}

#[derive(Clone, Copy, Default, Debug)]
pub struct FightInput {
    pub up: bool,
    pub down: bool,
    pub left: bool,
    pub right: bool,
    pub confirm: bool,
    pub back: bool,
    /// 0-based number key
    pub digit: Option<usize>,
    pub mv: Vec2,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FightEvent {
    Nav,
    Picked,
    Tell,
    PowerUp,
    Hit,
    Reset,
    Verdict(bool),
}

pub struct Fight {
    pub stage: Stage,
    pub sel: usize,
    pub act_sel: usize,
    /// the act menu's order (shuffled by seed)
    pub act_order: [Act; 7],
    tells: u8,
    pub first_tell: Option<Act>,
    pub bad: u8,
    pub hearts: u8,
    pub invuln: f32,
    pub owl: Vec2,
    pub facing_left: bool,
    pub lock: f32,
    pub resets: u32,
    /// Mimic turns so far (picks the next pattern)
    pub turns: u32,
    pub seed: u32,
    pub reduced: bool,
    /// the Mimic's current line (speech bubble)
    pub said: &'static str,
    /// the box's narration while it isn't a dodge turn
    pub flavor: String,
    pub ending: Option<Ending>,
}

impl Fight {
    pub fn new(seed: u32, reduced: bool) -> Fight {
        let mut order = ACTS;
        // Fisher-Yates by seed, so the good acts aren't always on top
        let mut h = seed;
        for i in (1..order.len()).rev() {
            h = mix(h, i as u32 + 11);
            order.swap(i, h as usize % (i + 1));
        }
        Fight {
            stage: Stage::Talk(Then::Menu),
            sel: 0,
            act_sel: 0,
            act_order: order,
            tells: 0,
            first_tell: None,
            bad: 0,
            hearts: BOX_HEARTS,
            invuln: 0.0,
            owl: owl_start(),
            facing_left: false,
            lock: LOCK_SECS,
            resets: 0,
            turns: 0,
            seed,
            reduced,
            said: OPENING,
            flavor: "* The Inbox Mimic rattles open. A message slides out of its teeth.".into(),
            ending: None,
        }
    }

    pub fn tells(&self) -> usize {
        self.tells.count_ones() as usize
    }

    pub fn found(&self, a: Act) -> bool {
        self.tells & a.bit() != 0
    }

    pub fn spareable(&self) -> bool {
        self.tells() >= SPARE_AT
    }

    pub fn level(&self) -> u8 {
        self.bad.min(MAX_LEVEL)
    }

    /// the pattern the Mimic's next (or current) turn uses: seed and turn only
    pub fn pattern_for(seed: u32, turn: u32) -> Pattern {
        PATTERNS[((seed % 4) + turn) as usize % PATTERNS.len()]
    }

    pub fn set_reduced(&mut self, on: bool) {
        self.reduced = on;
        if let Stage::Dodge(d) = &mut self.stage {
            if on {
                d.reduced = true;
                d.hazards.clear();
                d.button = None;
                d.dur = d.dur.min(d.t + DODGE_SECS_REDUCED);
            }
        }
    }

    fn talk(&mut self, said: &'static str, flavor: impl Into<String>, then: Then) {
        self.said = said;
        self.flavor = flavor.into();
        self.stage = Stage::Talk(then);
        self.lock = LOCK_SECS;
    }

    fn menu(&mut self) {
        self.stage = Stage::Menu;
        self.lock = LOCK_SECS;
    }

    fn start_dodge(&mut self) {
        let pattern = Fight::pattern_for(self.seed, self.turns);
        self.turns += 1;
        self.owl = owl_start();
        self.invuln = 0.0;
        self.stage = Stage::Dodge(Dodge::new(pattern, self.seed, self.turns, self.level(), self.reduced));
    }

    fn command(&mut self, c: Command) -> Option<FightEvent> {
        self.sel = COMMANDS.iter().position(|&x| x == c).unwrap_or(0);
        match c {
            Command::Act => {
                self.stage = Stage::Acts;
                self.lock = LOCK_SECS;
            }
            Command::Inspect => {
                self.stage = Stage::Inspect;
                self.lock = LOCK_SECS;
            }
            Command::Spare if self.spareable() => {
                let a = self.first_tell.unwrap_or(Act::CheckSender);
                self.ending = Some(Ending::Spared(a));
                self.talk(SPARE_OK, "* You SPARE the Inbox Mimic. It slinks off to the spam folder.", Then::Verdict(true));
            }
            Command::Spare => self.talk(SPARE_EARLY, "* You offer mercy. The Mimic snaps at it.", Then::Dodge),
            Command::Delete if self.spareable() => {
                let a = self.first_tell.unwrap_or(Act::CheckSender);
                self.ending = Some(Ending::Deleted(a));
                self.talk(DELETE_OK, "* You delete the message. You know exactly what it was.", Then::Verdict(true));
            }
            Command::Delete if self.tells() == 0 => {
                self.ending = Some(Ending::DeletedUnread);
                self.talk(DELETE_UNREAD, "* You delete it without a single look.", Then::Verdict(false));
            }
            Command::Delete => self.talk(DELETE_EARLY, "* Your finger hovers over delete. The Mimic smirks.", Then::Dodge),
        }
        Some(FightEvent::Picked)
    }

    fn act(&mut self, a: Act) -> Option<FightEvent> {
        if a.is_tell() {
            if self.found(a) {
                self.talk(REPEAT_LINE, a.flavor(), Then::Dodge);
                return Some(FightEvent::Picked);
            }
            self.tells |= a.bit();
            self.first_tell.get_or_insert(a);
            let mut flavor = a.flavor().to_string();
            if self.spareable() {
                flavor.push_str("  * The Mimic is out of excuses. Its name turns gold.");
            }
            self.talk(a.mimic_line(), flavor, Then::Dodge);
            return Some(FightEvent::Tell);
        }
        self.bad += 1;
        if self.bad >= LOSE_AT {
            self.ending = Some(Ending::ArgumentLost);
            self.talk(ARGUMENT_WON, a.flavor(), Then::Verdict(false));
        } else {
            self.talk(a.mimic_line(), a.flavor(), Then::Dodge);
        }
        Some(FightEvent::PowerUp)
    }

    /// Out of box hearts: the encounter starts over (tells and power-ups
    /// cleared, fresh hearts). Never a verdict.
    fn reset(&mut self) {
        self.tells = 0;
        self.first_tell = None;
        self.bad = 0;
        self.hearts = BOX_HEARTS;
        self.invuln = 0.0;
        self.owl = owl_start();
        self.resets += 1;
        self.talk(RESET_LINE, "* The box resets. Your tells and the Mimic's power-ups are gone.", Then::Menu);
    }

    /// One frame. `open` is the caller's choice gate (0.4 s lock over and
    /// every confirm/swing key released); picks also wait for the fight's own
    /// lock on each new screen. Navigation is always allowed.
    pub fn update(&mut self, dt: f32, inp: &FightInput, open: bool) -> Option<FightEvent> {
        self.lock = (self.lock - dt).max(0.0);
        let ok = open && self.lock <= 0.0;
        match &mut self.stage {
            Stage::Talk(then) => {
                let then = *then;
                if ok && inp.confirm {
                    match then {
                        Then::Menu => self.menu(),
                        Then::Dodge => self.start_dodge(),
                        Then::Verdict(right) => {
                            self.stage = Stage::Done;
                            return Some(FightEvent::Verdict(right));
                        }
                    }
                    return Some(FightEvent::Nav);
                }
                None
            }
            Stage::Menu => {
                let n = COMMANDS.len();
                if inp.left || inp.up {
                    self.sel = (self.sel + n - 1) % n;
                    return Some(FightEvent::Nav);
                }
                if inp.right || inp.down {
                    self.sel = (self.sel + 1) % n;
                    return Some(FightEvent::Nav);
                }
                if !ok {
                    return None;
                }
                if let Some(d) = inp.digit.filter(|&d| d < n) {
                    return self.command(COMMANDS[d]);
                }
                if inp.confirm {
                    return self.command(COMMANDS[self.sel]);
                }
                None
            }
            Stage::Acts => {
                let n = self.act_order.len();
                if inp.back {
                    self.menu();
                    return Some(FightEvent::Nav);
                }
                if inp.up {
                    self.act_sel = (self.act_sel + n - 1) % n;
                    return Some(FightEvent::Nav);
                }
                if inp.down {
                    self.act_sel = (self.act_sel + 1) % n;
                    return Some(FightEvent::Nav);
                }
                if inp.left || inp.right {
                    // two columns of four
                    self.act_sel = if self.act_sel < 4 { (self.act_sel + 4).min(n - 1) } else { self.act_sel - 4 };
                    return Some(FightEvent::Nav);
                }
                if !ok {
                    return None;
                }
                if let Some(d) = inp.digit.filter(|&d| d < n) {
                    self.act_sel = d;
                    return self.act(self.act_order[d]);
                }
                if inp.confirm {
                    return self.act(self.act_order[self.act_sel]);
                }
                None
            }
            Stage::Inspect => {
                if inp.back || (ok && inp.confirm) {
                    self.menu();
                    return Some(FightEvent::Nav);
                }
                None
            }
            Stage::Dodge(d) => {
                if inp.mv.x.abs() > 0.01 {
                    self.facing_left = inp.mv.x < 0.0;
                }
                self.owl += inp.mv.clamp_length_max(1.0) * OWL_SPEED * dt;
                self.owl = self.owl.clamp(vec2(OWL_R, OWL_R), vec2(BOX_W - OWL_R, BOX_H - OWL_R));
                self.invuln = (self.invuln - dt).max(0.0);
                let touching = d.update(dt, self.owl);
                if d.done() {
                    self.flavor = if self.spareable() {
                        "* The Inbox Mimic is out of excuses. Its name glows gold.".into()
                    } else if self.bad > 0 {
                        "* The Inbox Mimic puffs up, humming with stolen confidence.".into()
                    } else {
                        "* The Inbox Mimic shuffles its envelopes, waiting.".into()
                    };
                    self.menu();
                    return None;
                }
                if touching && self.invuln <= 0.0 {
                    self.hearts = self.hearts.saturating_sub(1);
                    self.invuln = HIT_INVULN;
                    if self.hearts == 0 {
                        self.reset();
                        return Some(FightEvent::Reset);
                    }
                    return Some(FightEvent::Hit);
                }
                None
            }
            Stage::Done => None,
        }
    }

    /// The Keeper's last line: the one in-character takeaway.
    pub fn takeaway(&self) -> &'static str {
        match self.ending {
            Some(Ending::Spared(a) | Ending::Deleted(a)) => match a {
                Act::HoverLink => "You hovered before you clicked. Who even does that? Ugh.",
                Act::CallOffice => "You called the real office yourself. So unsporting. So effective.",
                Act::WaitItOut => "You let the deadline pass and nothing broke. My favorite trick, wasted.",
                _ => "Ugh, you checked the sender. Nobody checks the sender.",
            },
            Some(Ending::DeletedUnread) => "Deleted without one look? Then you never learned my tricks. Wardens!",
            Some(Ending::ArgumentLost) | None => "Ha! You did everything my message asked. Wardens, they're all yours.",
        }
    }

    /// Autopilot (CC_AUTO): the next key for this screen. `right` plays the
    /// spare path, else the bad-acts path.
    pub fn auto_digit(&self, right: bool) -> Option<usize> {
        match self.stage {
            Stage::Menu if right && self.spareable() => Some(2),
            Stage::Menu => Some(0),
            Stage::Acts => self.act_order.iter().position(|&a| if right { a.is_tell() && !self.found(a) } else { !a.is_tell() }),
            _ => None,
        }
    }

    /// Autopilot dodge: steer away from what's close (or into it with `seek`).
    pub fn auto_steer(&self, seek: bool) -> Vec2 {
        let Stage::Dodge(d) = &self.stage else { return Vec2::ZERO };
        let mut threats: Vec<Vec2> = d.hazards.iter().map(|h| h.pos + h.vel * 0.25).collect();
        if let Some(b) = d.button {
            threats.push(b);
        }
        if d.pattern == Pattern::Clock {
            let ab = d.hand_tip() - HAND_PIVOT;
            let t = ((self.owl - HAND_PIVOT).dot(ab) / ab.length_squared().max(1e-6)).clamp(0.0, 1.0);
            threats.push(HAND_PIVOT + ab * t);
        }
        if seek {
            return threats.iter().min_by(|a, b| a.distance(self.owl).total_cmp(&b.distance(self.owl))).map_or(Vec2::ZERO, |&p| (p - self.owl).normalize_or_zero());
        }
        let mut push = Vec2::ZERO;
        for p in threats {
            let away = self.owl - p;
            let dist = away.length();
            if dist < 110.0 {
                push += away.normalize_or_zero() * (110.0 - dist) / 110.0;
            }
        }
        // drift home (bottom middle) when nothing is close
        push += (owl_start() - self.owl) * 0.004;
        push.normalize_or_zero()
    }
}

/// One frame against the run: a verdict answers the Keeper exactly as the quiz
/// does (`run.answer_keeper`). Hits and resets never reach the run.
pub fn step(run: &mut crate::state::Run, f: &mut Fight, dt: f32, inp: &FightInput, open: bool) -> Option<FightEvent> {
    let ev = f.update(dt, inp, open);
    if let Some(FightEvent::Verdict(right)) = ev {
        run.answer_keeper(right)?;
    }
    ev
}

// ---------------------------------------------------------------- drawing

use crate::art::{centered, draw_sprite_centered, heart, panel, text_block, wrap, AT_SIGN, BOX_OWL, DIM, ENVELOPE, INK, KSU_GOLD, PAPER, RISKY};
use crate::keeper::Mail;

/// Box position on the 1280x720 canvas.
pub const BOX_X: f32 = (1280.0 - BOX_W) * 0.5;
pub const BOX_Y: f32 = 300.0;

impl Fight {
    /// The whole fight screen (below the HUD).
    pub fn draw(&self, mail: &Mail, t: f32) {
        let gold = self.spareable();
        // the message card (top left)
        panel(40.0, 68.0, 480.0, 218.0, KSU_GOLD);
        draw_text("NEW MESSAGE", 60.0, 94.0, 20.0, KSU_GOLD);
        draw_text(&mail.from, 60.0, 118.0, 17.0, PAPER);
        let y = text_block(&mail.subject, 60.0, 142.0, 18.0, 440.0, WHITE);
        let y = text_block(&mail.snippet, 60.0, y + 2.0, 16.0, 440.0, DIM);
        draw_text(&mail.fine_print, 60.0, (y + 6.0).min(278.0), 13.0, DIM);

        // the Mimic, its name (gold when spareable) and meters
        let name_col = if gold { KSU_GOLD } else { PAPER };
        draw_text("INBOX MIMIC", 560.0, 104.0, 34.0, name_col);
        if gold {
            draw_text("(spareable)", 790.0, 104.0, 20.0, KSU_GOLD);
        }
        draw_text("TELLS", 560.0, 136.0, 18.0, DIM);
        for i in 0..SPARE_AT {
            let on = i < self.tells();
            draw_rectangle(630.0 + i as f32 * 26.0, 122.0, 18.0, 16.0, if on { KSU_GOLD } else { Color::new(0.3, 0.25, 0.1, 1.0) });
        }
        draw_text("ARGUMENT", 740.0, 136.0, 18.0, DIM);
        for i in 0..LOSE_AT {
            let on = i < self.bad;
            draw_rectangle(840.0 + i as f32 * 26.0, 122.0, 18.0, 16.0, if on { RISKY } else { Color::new(0.3, 0.12, 0.12, 1.0) });
        }
        let bob = (t * 2.4).sin() * 3.0 + if matches!(self.stage, Stage::Talk(_)) { (t * 22.0).sin() * 1.5 } else { 0.0 };
        crate::art::glow(1150.0, 130.0, 70.0, if gold { Color::new(1.0, 0.77, 0.16, 1.0) } else { Color::new(0.6, 0.4, 1.0, 1.0) }, 1.2);
        draw_sprite_centered(crate::art::character_sprite("inbox_mimic"), 1150.0, 130.0 + bob, 7.0, false, WHITE);
        // speech bubble
        panel(560.0, 200.0, 680.0, 86.0, PAPER);
        text_block(self.said, 580.0, 232.0, 21.0, 640.0, WHITE);

        // the box
        let o = vec2(BOX_X, BOX_Y);
        draw_rectangle(o.x, o.y, BOX_W, BOX_H, BLACK);
        match &self.stage {
            Stage::Talk(_) | Stage::Menu | Stage::Done => {
                text_block(&self.flavor, o.x + 24.0, o.y + 40.0, 22.0, BOX_W - 48.0, WHITE);
                if matches!(self.stage, Stage::Talk(_)) && (t * 2.0).fract() < 0.6 {
                    draw_text("[Enter]", o.x + BOX_W - 92.0, o.y + BOX_H - 16.0, 18.0, DIM);
                }
            }
            Stage::Acts => {
                for (i, a) in self.act_order.iter().enumerate() {
                    let (col, row) = (i / 4, i % 4);
                    let (x, y) = (o.x + 40.0 + col as f32 * 300.0, o.y + 44.0 + row as f32 * 46.0);
                    let on = i == self.act_sel;
                    if on {
                        draw_sprite_centered(&BOX_OWL, x - 14.0, y - 7.0, 1.2, false, WHITE);
                    }
                    let c = if self.found(*a) { DIM } else if on { KSU_GOLD } else { PAPER };
                    draw_text(format!("{} {}", i + 1, a.label()), x + 6.0, y, 22.0, c);
                }
            }
            Stage::Inspect => {
                let rows: [(&str, &str); 4] = [("From", &mail.from), ("Link goes to", &mail.link), ("Small print", &mail.fine_print), ("Subject", &mail.subject)];
                let mut y = o.y + 36.0;
                draw_text("* You look closer. Nothing clicks, nothing sends.", o.x + 24.0, y, 20.0, DIM);
                for (k, v) in rows {
                    y += 40.0;
                    draw_text(k, o.x + 24.0, y, 18.0, KSU_GOLD);
                    let lines = wrap(v, 18.0, BOX_W - 200.0);
                    for (j, l) in lines.iter().enumerate().take(2) {
                        draw_text(l, o.x + 160.0, y + j as f32 * 18.0, 18.0, WHITE);
                    }
                }
            }
            Stage::Dodge(d) => self.draw_dodge(d, o, t),
        }
        // the border goes on last so the clock dial and edge hazards tuck under it
        draw_rectangle_lines(o.x - 4.0, o.y - 4.0, BOX_W + 8.0, BOX_H + 8.0, 4.0, KSU_GOLD);
        draw_rectangle_lines(o.x - 1.0, o.y - 1.0, BOX_W + 2.0, BOX_H + 2.0, 1.0, INK);

        // box hearts and (in a dodge) the pattern's line
        draw_text("BOX HP", o.x, o.y + BOX_H + 30.0, 18.0, DIM);
        for i in 0..BOX_HEARTS {
            heart(o.x + 74.0 + i as f32 * 26.0, o.y + BOX_H + 16.0, 3.0, i < self.hearts);
        }
        if let Stage::Dodge(d) = &self.stage {
            let f = (1.0 - d.t / d.dur).clamp(0.0, 1.0);
            draw_text("TIME", o.x + BOX_W - 206.0, o.y + BOX_H + 30.0, 18.0, DIM);
            draw_rectangle(o.x + BOX_W - 160.0, o.y + BOX_H + 18.0, 160.0 * f, 10.0, KSU_GOLD);
            draw_rectangle_lines(o.x + BOX_W - 160.0, o.y + BOX_H + 18.0, 160.0, 10.0, 1.0, DIM);
            centered(d.pattern.flavor(), 640.0, o.y + BOX_H + 58.0, 20.0, PAPER);
        }

        // the command buttons
        let dodge = matches!(self.stage, Stage::Dodge(_));
        for (i, c) in COMMANDS.iter().enumerate() {
            let (w, gap) = (200.0, 24.0);
            let x = 640.0 - (4.0 * w + 3.0 * gap) * 0.5 + i as f32 * (w + gap);
            let y = 596.0;
            let on = matches!(self.stage, Stage::Menu) && i == self.sel;
            let spare_gold = *c == Command::Spare && gold;
            let col = if dodge {
                Color::new(0.4, 0.36, 0.3, 1.0)
            } else if on || spare_gold {
                KSU_GOLD
            } else {
                DIM
            };
            draw_rectangle(x, y, w, 46.0, if on { Color::new(0.3, 0.22, 0.04, 0.95) } else { Color::new(0.08, 0.06, 0.1, 0.95) });
            draw_rectangle_lines(x, y, w, 46.0, if on { 3.0 } else { 2.0 }, col);
            if on {
                draw_sprite_centered(&BOX_OWL, x + 24.0, y + 23.0, 1.3, false, WHITE);
            }
            centered(&format!("{} {}", i + 1, c.label()), x + w * 0.5 + 10.0, y + 31.0, 24.0, col);
        }
    }

    fn draw_dodge(&self, d: &Dodge, o: Vec2, t: f32) {
        if d.pattern == Pattern::Clock && !d.reduced {
            // the 5:00 PM dial behind the hand
            let p = o + HAND_PIVOT;
            // the lower half only: a full circle would poke out of the box into the speech bubble
            draw_arc(p.x, p.y, 32, 32.0, 0.0, 2.0, 180.0, Color::new(1.0, 0.77, 0.16, 0.5));
            let l = CLOCK_LABEL;
            draw_rectangle_lines(o.x + l.x, o.y + l.y, l.w, l.h, 1.0, Color::new(1.0, 0.77, 0.16, 0.5));
            centered("DUE 5:00 PM", o.x + l.x + l.w * 0.5, o.y + l.y + 17.0, 16.0, KSU_GOLD);
            let tip = o + d.hand_tip();
            draw_line(p.x, p.y, tip.x, tip.y, 6.0, KSU_GOLD);
            draw_line(p.x, p.y, tip.x, tip.y, 2.0, WHITE);
            draw_circle(p.x, p.y, 7.0, KSU_GOLD);
        }
        for h in &d.hazards {
            let (x, y) = (o.x + h.pos.x, o.y + h.pos.y);
            match h.kind {
                HazardKind::Envelope => {
                    draw_sprite_centered(&ENVELOPE, x, y, 1.6, false, WHITE);
                    draw_text("URGENT", x - 16.0, y - 12.0, 11.0, RISKY);
                }
                HazardKind::At => draw_sprite_centered(&AT_SIGN, x, y, 1.5, false, WHITE),
                HazardKind::Numeral => {
                    let s = if (h.pos.x as i32) % 2 == 0 { "5" } else { "00" };
                    draw_text(s, x - 8.0, y + 8.0, 24.0, KSU_GOLD);
                }
            }
        }
        if let Some(b) = d.button {
            let (x, y) = (o.x + b.x - BUTTON_W * 0.5, o.y + b.y - BUTTON_H * 0.5);
            draw_rectangle(x, y, BUTTON_W, BUTTON_H, Color::new(0.2, 0.45, 0.95, 1.0));
            draw_rectangle_lines(x, y, BUTTON_W, BUTTON_H, 2.0, WHITE);
            centered("CONFIRM LOGIN", x + BUTTON_W * 0.5, y + 20.0, 16.0, WHITE);
        }
        let blink = self.invuln > 0.0 && (t * 18.0).sin() < 0.0;
        if !blink {
            draw_sprite_centered(&BOX_OWL, o.x + self.owl.x, o.y + self.owl.y, 1.6, self.facing_left, WHITE);
        }
        if d.reduced {
            centered("Reduced motion: no attacks. Catch your breath.", o.x + BOX_W * 0.5, o.y + 40.0, 20.0, DIM);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::{parse, Content, EMBEDDED_ENCOUNTERS};
    use crate::state::{floors_for, Phase, Run};

    fn shipped() -> Content {
        parse(EMBEDDED_ENCOUNTERS).unwrap().0
    }

    fn run_at_keeper(c: &Content) -> Run {
        let mut r = Run::new(floors_for(c.encounters.len()), c.encounters.len());
        r.begin_encounter();
        let safe = c.encounters[0].choices.iter().position(|ch| ch.outcome == crate::content::Outcome::Safe).unwrap();
        r.choose(c, safe).unwrap();
        r.reveal_lesson(c).unwrap();
        r.advance();
        assert!(r.begin_keeper());
        r
    }

    const DT: f32 = 1.0 / 60.0;

    fn press(f: &mut Fight, inp: FightInput) -> Option<FightEvent> {
        f.lock = 0.0;
        f.update(DT, &inp, true)
    }
    fn enter(f: &mut Fight) -> Option<FightEvent> {
        press(f, FightInput { confirm: true, ..Default::default() })
    }
    fn key(f: &mut Fight, d: usize) -> Option<FightEvent> {
        press(f, FightInput { digit: Some(d), ..Default::default() })
    }
    /// Sit out the Mimic's turn without moving (reduced motion: nothing hits).
    fn sit_out(f: &mut Fight) {
        for _ in 0..(DODGE_SECS * 60.0) as usize + 10 {
            if !matches!(f.stage, Stage::Dodge(_)) {
                return;
            }
            f.update(DT, &FightInput::default(), true);
        }
    }
    fn act_index(f: &Fight, a: Act) -> usize {
        f.act_order.iter().position(|&x| x == a).unwrap()
    }
    /// From the menu: ACT -> `a` -> the Mimic's line -> its turn -> menu.
    fn do_act(f: &mut Fight, a: Act) -> Option<FightEvent> {
        assert!(matches!(f.stage, Stage::Menu), "{:?}", f.stage);
        key(f, 0);
        assert!(matches!(f.stage, Stage::Acts));
        let i = act_index(f, a);
        let ev = key(f, i);
        assert!(matches!(f.stage, Stage::Talk(_)));
        if matches!(f.stage, Stage::Talk(Then::Dodge)) {
            enter(f);
            sit_out(f);
        }
        ev
    }
    fn to_menu(seed: u32) -> Fight {
        let mut f = Fight::new(seed, true);
        enter(&mut f);
        assert!(matches!(f.stage, Stage::Menu));
        f
    }

    #[test]
    fn menu_flow() {
        let mut f = Fight::new(3, true);
        // the opening line waits for the lock, then Enter opens the menu
        assert_eq!(f.update(DT, &FightInput { confirm: true, ..Default::default() }, true), None, "locked");
        enter(&mut f);
        assert!(matches!(f.stage, Stage::Menu));
        // arrows move the selection, Enter confirms it
        press(&mut f, FightInput { right: true, ..Default::default() });
        assert_eq!(f.sel, 1);
        enter(&mut f);
        assert!(matches!(f.stage, Stage::Inspect), "INSPECT");
        enter(&mut f);
        assert!(matches!(f.stage, Stage::Menu), "INSPECT is free: straight back to the menu");
        assert_eq!(f.turns, 0);
        // number keys pick; Esc backs out of the act list
        key(&mut f, 0);
        assert!(matches!(f.stage, Stage::Acts));
        press(&mut f, FightInput { back: true, ..Default::default() });
        assert!(matches!(f.stage, Stage::Menu));
        // an act: the Mimic answers, then takes its turn, then the menu again
        do_act(&mut f, Act::CheckSender);
        assert!(matches!(f.stage, Stage::Menu));
        assert_eq!(f.turns, 1);
        // a closed gate (keys held) blocks picks but not navigation
        let before = f.sel;
        f.lock = 0.0;
        assert_eq!(f.update(DT, &FightInput { digit: Some(0), ..Default::default() }, false), None);
        assert!(matches!(f.stage, Stage::Menu));
        f.update(DT, &FightInput { down: true, ..Default::default() }, false);
        assert_ne!(f.sel, before);
    }

    #[test]
    fn spare_condition_is_deterministic() {
        for seed in [0u32, 1, 7, 99, 4242] {
            let mut f = to_menu(seed);
            // spare too early: a taunt and the Mimic's turn, no verdict
            key(&mut f, 2);
            assert!(matches!(f.stage, Stage::Talk(Then::Dodge)));
            enter(&mut f);
            sit_out(&mut f);
            let goods = [Act::CheckSender, Act::HoverLink, Act::CallOffice];
            for (k, a) in goods.iter().enumerate() {
                assert!(!f.spareable());
                assert_eq!(do_act(&mut f, *a), Some(FightEvent::Tell));
                assert_eq!(f.tells(), k + 1);
                // the same tell twice doesn't count again
                do_act(&mut f, *a);
                assert_eq!(f.tells(), k + 1);
            }
            assert!(f.spareable() && SPARE_AT == 3);
            key(&mut f, 2);
            assert!(matches!(f.stage, Stage::Talk(Then::Verdict(true))));
            assert_eq!(enter(&mut f), Some(FightEvent::Verdict(true)));
            assert!(f.takeaway().contains("sender"), "{}", f.takeaway());
        }
    }

    #[test]
    fn delete_depends_on_investigation() {
        let mut f = to_menu(5);
        key(&mut f, 3);
        assert!(matches!(f.stage, Stage::Talk(Then::Verdict(false))), "delete unread is wrong");
        let mut f = to_menu(5);
        do_act(&mut f, Act::WaitItOut);
        key(&mut f, 3);
        assert!(matches!(f.stage, Stage::Talk(Then::Dodge)), "half-sure: taunted");
        enter(&mut f);
        sit_out(&mut f);
        do_act(&mut f, Act::HoverLink);
        do_act(&mut f, Act::CheckSender);
        key(&mut f, 3);
        assert!(matches!(f.stage, Stage::Talk(Then::Verdict(true))), "delete with evidence is right");
        assert!(f.takeaway().contains("deadline"), "first tell found was the deadline");
    }

    #[test]
    fn bad_acts_escalate_the_patterns() {
        let mut f = to_menu(11);
        assert_eq!(f.level(), 0);
        assert_eq!(do_act(&mut f, Act::ReplyPassword), Some(FightEvent::PowerUp));
        assert_eq!(f.level(), 1);
        do_act(&mut f, Act::ClickReview);
        assert_eq!(f.level(), 2);
        // harder at every level, for every pattern
        for p in PATTERNS {
            let d: Vec<Dodge> = (0..=MAX_LEVEL).map(|l| Dodge::new(p, 11, 1, l, false)).collect();
            for w in d.windows(2) {
                assert!(w[1].spawn_every() < w[0].spawn_every(), "{p:?}");
                assert!(w[1].hand_swing().1 > w[0].hand_swing().1 && w[1].button_speed() > w[0].button_speed());
            }
        }
        // the third bad act loses the argument
        key(&mut f, 0);
        let i = act_index(&f, Act::PayFee);
        key(&mut f, i);
        assert!(matches!(f.stage, Stage::Talk(Then::Verdict(false))));
        assert_eq!(f.ending, Some(Ending::ArgumentLost));
    }

    /// Patterns and hazards come from (seed, turn, level) only. Two fights with
    /// different acts (good vs. neutral) at the same turn and level dodge the
    /// exact same attack, frame by frame.
    #[test]
    fn hazards_are_independent_of_the_right_answer() {
        for seed in 0..30u32 {
            let mut a = to_menu(seed);
            let mut b = to_menu(seed);
            // a investigates; b spares too early (no tells): both level 0, turn 1
            key(&mut a, 0);
            let i = act_index(&a, Act::CheckSender);
            key(&mut a, i);
            key(&mut b, 2);
            enter(&mut a);
            enter(&mut b);
            a.set_reduced(false);
            b.set_reduced(false);
            let (Stage::Dodge(da), Stage::Dodge(db)) = (&mut a.stage, &mut b.stage) else { panic!() };
            *da = Dodge::new(da.pattern, seed, 1, 0, false);
            *db = Dodge::new(db.pattern, seed, 1, 0, false);
            assert_eq!(da.pattern, db.pattern);
            for k in 0..300 {
                let mv = vec2(((k / 40) % 2) as f32 * 2.0 - 1.0, 0.0);
                a.update(DT, &FightInput { mv, ..Default::default() }, true);
                b.update(DT, &FightInput { mv, ..Default::default() }, true);
                let (Stage::Dodge(da), Stage::Dodge(db)) = (&a.stage, &b.stage) else { break };
                assert_eq!(da.hazards, db.hazards, "seed {seed} frame {k}");
                assert_eq!((da.hand, da.button), (db.hand, db.button));
                assert_eq!(a.hearts, b.hearts);
            }
        }
        // every pattern shows up across turns, and the act menu order varies by seed
        let pats: std::collections::HashSet<_> = (0..4).map(|t| format!("{:?}", Fight::pattern_for(9, t))).collect();
        assert_eq!(pats.len(), 4);
        let firsts: std::collections::HashSet<_> = (0..40).map(|s| format!("{:?}", Fight::new(s, true).act_order[0])).collect();
        assert!(firsts.len() >= 4, "{firsts:?}");
    }

    #[test]
    fn box_hearts_never_change_the_verdict() {
        let c = shipped();
        let mut run = run_at_keeper(&c);
        let before = run.stats;
        let mut f = Fight::new(21, false);
        f.lock = 0.0;
        step(&mut run, &mut f, DT, &FightInput { confirm: true, ..Default::default() }, true);
        f.lock = 0.0;
        step(&mut run, &mut f, DT, &FightInput { digit: Some(0), ..Default::default() }, true);
        let i = act_index(&f, Act::CheckSender);
        f.lock = 0.0;
        step(&mut run, &mut f, DT, &FightInput { digit: Some(i), ..Default::default() }, true);
        f.lock = 0.0;
        step(&mut run, &mut f, DT, &FightInput { confirm: true, ..Default::default() }, true);
        assert!(matches!(f.stage, Stage::Dodge(_)));
        assert_eq!(f.tells(), 1);
        // fly into everything until the box hearts run out
        let mut reset = false;
        for _ in 0..60 * 60 {
            let mv = f.auto_steer(true);
            if let Some(Stage::Dodge(d)) = Some(&mut f.stage) {
                if d.hazards.is_empty() && d.button.is_none() && d.pattern != Pattern::Clock {
                    d.hazards.push(Hazard { kind: HazardKind::Envelope, pos: f.owl, vel: Vec2::ZERO, t: 0.0 });
                }
            }
            match step(&mut run, &mut f, DT, &FightInput { mv, ..Default::default() }, true) {
                Some(FightEvent::Reset) => {
                    reset = true;
                    break;
                }
                Some(FightEvent::Verdict(_)) => panic!("hits gave a verdict"),
                _ => {}
            }
            if matches!(f.stage, Stage::Menu) {
                // keep the Mimic attacking: spare early (no verdict) for another turn
                f.lock = 0.0;
                step(&mut run, &mut f, DT, &FightInput { digit: Some(2), ..Default::default() }, true);
                f.lock = 0.0;
                step(&mut run, &mut f, DT, &FightInput { confirm: true, ..Default::default() }, true);
            }
        }
        assert!(reset, "the hearts ran out");
        assert_eq!((f.hearts, f.tells(), f.bad, f.resets), (BOX_HEARTS, 0, 0, 1), "the encounter starts over");
        assert_eq!(run.phase, Phase::KeeperQuiz, "no verdict");
        assert!(run.keepers.is_empty());
        assert_eq!((run.stats.health, run.stats.focus, run.stats.knowledge), (before.health, before.focus, before.knowledge));
    }

    #[test]
    fn verdicts_reach_the_run_like_the_quiz() {
        let c = shipped();
        for right in [true, false] {
            let mut run = run_at_keeper(&c);
            let mut f = Fight::new(2, true);
            let mut verdict = None;
            for _ in 0..4000 {
                let inp = match f.auto_digit(right) {
                    Some(d) => FightInput { digit: Some(d), ..Default::default() },
                    None => FightInput { confirm: true, ..Default::default() },
                };
                f.lock = 0.0;
                if let Some(FightEvent::Verdict(r)) = step(&mut run, &mut f, DT, &inp, true) {
                    verdict = Some(r);
                    break;
                }
            }
            assert_eq!(verdict, Some(right));
            assert_eq!(run.phase, Phase::KeeperVerdict);
            assert_eq!(run.keepers[0].correct, right);
        }
    }

    #[test]
    fn reduced_motion_has_no_hazards() {
        for p in PATTERNS {
            for l in 0..=MAX_LEVEL {
                let mut d = Dodge::new(p, 4, 2, l, true);
                for _ in 0..600 {
                    assert!(!d.update(DT, owl_start()));
                    assert!(d.hazards.is_empty() && d.button.is_none());
                }
                assert!(d.done() && d.dur == DODGE_SECS_REDUCED);
            }
        }
    }

    #[test]
    fn lines_are_short_ascii_and_use_no_real_domains() {
        let mut all: Vec<&str> = ACTS.iter().flat_map(|a| [a.flavor(), a.mimic_line(), a.label()]).collect();
        all.extend(PATTERNS.iter().map(|p| p.flavor()));
        all.extend([REPEAT_LINE, SPARE_EARLY, SPARE_OK, DELETE_EARLY, DELETE_UNREAD, DELETE_OK, ARGUMENT_WON, RESET_LINE, OPENING]);
        let mut f = Fight::new(0, true);
        for e in [Ending::Spared(Act::HoverLink), Ending::Spared(Act::CallOffice), Ending::Spared(Act::WaitItOut), Ending::Deleted(Act::CheckSender), Ending::ArgumentLost, Ending::DeletedUnread] {
            f.ending = Some(e);
            all.push(f.takeaway());
        }
        for l in all {
            assert!(l.is_ascii() && l.len() <= 96, "{l}");
            for bad in [".com", ".edu", ".gov", ".net", ".org", "http", "gift card", "wire", "zelle"] {
                assert!(!l.to_ascii_lowercase().contains(bad), "{l}");
            }
        }
    }

    #[test]
    fn clock_label_stays_clear_of_the_hand() {
        let pad = CLOCK_LABEL;
        let r = Rect::new(pad.x - 4.0, pad.y - 4.0, pad.w + 8.0, pad.h + 8.0);
        for level in 0..=MAX_LEVEL {
            let mut d = Dodge::new(Pattern::Clock, 7, 1, level, false);
            let (amp, _) = d.hand_swing();
            assert!(amp < 1.4, "the swing grew past what the label was placed for");
            while !d.done() {
                d.update(1.0 / 120.0, vec2(BOX_W - 20.0, BOX_H - 20.0));
                for k in 0..=40 {
                    let p = HAND_PIVOT.lerp(d.hand_tip(), k as f32 / 40.0);
                    assert!(!r.contains(p), "level {level}: the hand crosses the 5:00 PM label at {p:?}");
                }
            }
        }
    }
}
