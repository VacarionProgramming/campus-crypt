//! Every floor's Keeper fight: an investigation, Undertale-style.
//!
//! The floor's trap sends you something suspicious (an email in a lookalike
//! inbox, a text, a DM, a web page, a sign-in push or a check; see
//! `investigate.rs`). Turns alternate. On your turn a command menu offers ACT,
//! INSPECT, SPARE and DELETE ("just delete it"); on the trap's turn you steer a
//! tiny owl around the battle box and dodge a themed attack.
//!
//! The lesson lives in ACT and INSPECT:
//! * INSPECT (free, no trap turn) opens the message in full: a mail client,
//!   phone, browser, push prompt or check, with everything you've checked
//!   marked (red = a red flag you caught, green = checked and clean), and your
//!   findings so far.
//! * ACT offers a check for every red flag this message *could* carry ("Check
//!   the sender", "Hover the link", "Follow the money", "Contact the office
//!   yourself", ...). Each run plants `investigate::PLANTED` of them; a check
//!   on a planted flag catches it (the trap gives itself away, in character),
//!   a check on any other comes back clean. Catch every planted flag and the
//!   trap is spareable: its name turns gold.
//! * The case's three bad acts ("Reply with your password", "Pay the fee",
//!   ...) power it up (harder patterns) and it gloats. `LOSE_AT` of them and it
//!   wins the argument.
//!
//! Verdicts, all decided here by code (never by an AI line):
//! * SPARE when spareable -> right (stairs open, no wardens). SPARE earlier is
//!   just taunted.
//! * DELETE with no flags found -> wrong (2 wardens). DELETE when spareable ->
//!   right (deleting a fake you've confirmed is fine). DELETE in between is
//!   taunted ("you aren't even sure I'm fake").
//! * `LOSE_AT` bad acts -> wrong (2 wardens).
//!
//! Hits only cost temporary box hearts. Out of hearts, the encounter resets
//! (findings and power-ups cleared, fresh hearts); it never gives or changes a
//! verdict. Attack patterns depend only on the seed, the turn number and the
//! escalation level, never on which flags are planted. Reduced motion (R)
//! drops the hazards and shortens the trap's turn.

use macroquad::prelude::*;

use crate::investigate::{Act, Case, Flag};

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
/// bad acts until the trap wins the argument
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

const REPEAT_LINE: &str = "You already caught me on that one. Got anything new?";
const CLEAN_LINE: &str = "Ha! Nothing to see there. Told you I'm legit.";
const SPARE_EARLY: &str = "Spare me? You haven't even figured me out yet!";
const DELETE_EARLY: &str = "Delete me? You aren't even sure I'm fake yet!";
const DELETE_UNREAD: &str = "Delete me unread? Then you never learn my tricks. Wardens!";
const DELETE_OK: &str = "Deleted, with evidence. Cold. Correct, but cold.";
const ARGUMENT_WON: &str = "I win the argument! Wardens, take it from here!";
const RESET_LINE: &str = "Out of hearts? Snap! Fresh start. I forget everything, and so do you.";

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
    /// the trap speaks (bubble) and the box narrates; Enter goes on
    Talk(Then),
    Menu,
    Acts,
    Inspect,
    Dodge(Dodge),
    /// verdict reached (the run has been told)
    Done,
}

/// How a fight ended, for the trap's last line.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ending {
    Spared(Flag),
    Deleted(Flag),
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
    /// a check that came back clean
    Clean,
    /// Hoot chimed in (hesitation)
    Hoot,
    PowerUp,
    Hit,
    Reset,
    Verdict(bool),
}

pub struct Fight {
    pub case: Case,
    pub stage: Stage,
    pub sel: usize,
    pub act_sel: usize,
    /// the act menu (the case's seeded order)
    pub act_order: Vec<Act>,
    /// planted flags caught (bits of `Flag`)
    found: u8,
    /// every flag checked, caught or clean
    checked: u8,
    pub first_tell: Option<Flag>,
    pub bad: u8,
    pub hearts: u8,
    pub invuln: f32,
    pub owl: Vec2,
    pub facing_left: bool,
    pub lock: f32,
    pub resets: u32,
    /// trap turns so far (picks the next pattern)
    pub turns: u32,
    pub seed: u32,
    pub reduced: bool,
    /// the trap's current line (speech bubble)
    pub said: String,
    /// the box's narration while it isn't a dodge turn
    pub flavor: String,
    pub ending: Option<Ending>,
    /// INSPECT has been opened at least once (autopilot, screenshots)
    pub inspected: bool,
    /// Hoot's current nudge (T / HOOT, or after hesitating on the menu)
    pub hoot: Option<String>,
    /// Hoot's hints are on (the H toggle); set by the caller every frame
    pub hints: bool,
    /// seconds on the menu without a move (hesitation)
    idle: f32,
    /// Hoot already chimed in on his own this fight
    hooted: bool,
}

/// Seconds of hesitation on the command menu before Hoot nudges on his own.
pub const HOOT_IDLE_SECS: f32 = 12.0;

impl Fight {
    pub fn new(case: Case, seed: u32, reduced: bool) -> Fight {
        let act_order = case.acts.clone();
        let said = case.opening.clone();
        let flavor = case.intro.clone();
        Fight {
            case,
            stage: Stage::Talk(Then::Menu),
            sel: 0,
            act_sel: 0,
            act_order,
            found: 0,
            checked: 0,
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
            said,
            flavor,
            ending: None,
            inspected: false,
            hoot: None,
            hints: true,
            idle: 0.0,
            hooted: false,
        }
    }

    /// Hoot points at a red flag you haven't caught yet (the kind of clue,
    /// never the answer), rotating as you ask again. False when every flag is
    /// caught (nothing left to nudge toward) or hints are off.
    pub fn nudge(&mut self) -> bool {
        if !self.hints {
            return false;
        }
        let left: Vec<Flag> = self.case.planted.iter().copied().filter(|f| !self.found(*f)).collect();
        // the non-Verify flags first: "go around it" is the backup advice
        let pool: Vec<Flag> = if left.iter().any(|f| *f != Flag::Verify) { left.into_iter().filter(|f| *f != Flag::Verify).collect() } else { left };
        let Some(&f) = pool.get((crate::keeper::mix(self.seed, self.turns.wrapping_add(self.tells() as u32 * 7)) as usize) % pool.len().max(1)) else {
            self.hoot = Some("Hoo, you've caught every red flag. You can SPARE it now.".to_string());
            return true;
        };
        self.hoot = Some(f.nudge(self.case.medium).to_string());
        self.hooted = true;
        true
    }

    /// red flags caught so far
    pub fn tells(&self) -> usize {
        self.found.count_ones() as usize
    }

    pub fn needed(&self) -> usize {
        self.case.planted.len()
    }

    pub fn found(&self, f: Flag) -> bool {
        self.found & f.bit() != 0
    }

    pub fn checked(&self, f: Flag) -> bool {
        self.checked & f.bit() != 0
    }

    pub fn spareable(&self) -> bool {
        self.tells() >= self.needed()
    }

    pub fn level(&self) -> u8 {
        self.bad.min(MAX_LEVEL)
    }

    /// the pattern the trap's next (or current) turn uses: seed and turn only
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

    fn talk(&mut self, said: impl Into<String>, flavor: impl Into<String>, then: Then) {
        self.idle = 0.0;
        self.said = said.into();
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
        let name = self.case.name.clone();
        let noun = self.case.medium.noun();
        match c {
            Command::Act => {
                self.stage = Stage::Acts;
                self.lock = LOCK_SECS;
            }
            Command::Inspect => {
                self.stage = Stage::Inspect;
                self.inspected = true;
                self.lock = LOCK_SECS;
            }
            Command::Spare if self.spareable() => {
                let f = self.first_tell.unwrap_or(Flag::Verify);
                self.ending = Some(Ending::Spared(f));
                let spare = self.case.spare.clone();
                self.talk(spare, format!("* You SPARE {name}. It slinks off, caught."), Then::Verdict(true));
            }
            Command::Spare => self.talk(SPARE_EARLY, format!("* You offer mercy. {name} snaps at it."), Then::Dodge),
            Command::Delete if self.spareable() => {
                let f = self.first_tell.unwrap_or(Flag::Verify);
                self.ending = Some(Ending::Deleted(f));
                self.talk(DELETE_OK, format!("* You delete the {noun}. You know exactly what it was."), Then::Verdict(true));
            }
            Command::Delete if self.tells() == 0 => {
                self.ending = Some(Ending::DeletedUnread);
                self.talk(DELETE_UNREAD, format!("* You delete the {noun} without a single look."), Then::Verdict(false));
            }
            Command::Delete => self.talk(DELETE_EARLY, format!("* Your finger hovers over delete. {name} smirks."), Then::Dodge),
        }
        Some(FightEvent::Picked)
    }

    fn act(&mut self, a: Act) -> Option<FightEvent> {
        match a {
            Act::Check(f) => {
                if !self.case.is_planted(f) {
                    self.checked |= f.bit();
                    self.talk(CLEAN_LINE, format!("{}  {}", f.act_flavor(), f.clean()), Then::Dodge);
                    return Some(FightEvent::Clean);
                }
                if self.found(f) {
                    self.talk(REPEAT_LINE, f.act_flavor(), Then::Dodge);
                    return Some(FightEvent::Picked);
                }
                self.found |= f.bit();
                self.checked |= f.bit();
                self.first_tell.get_or_insert(f);
                let line = self.case.lines.get(&f).map_or_else(String::new, |(l, _)| l.clone());
                let mut flavor = format!("{}  * RED FLAG: {}", f.act_flavor(), self.case.tell(f));
                if self.spareable() {
                    flavor.push_str("  * Out of excuses: its name turns gold.");
                }
                self.talk(line, flavor, Then::Dodge);
                Some(FightEvent::Tell)
            }
            Act::Bad(i) => {
                let (line, flavor) = self.case.bad.get(i as usize).map_or((String::new(), String::new()), |b| (b.line.clone(), b.flavor.clone()));
                self.bad += 1;
                if self.bad >= LOSE_AT {
                    self.ending = Some(Ending::ArgumentLost);
                    self.talk(ARGUMENT_WON, flavor, Then::Verdict(false));
                } else {
                    self.talk(line, flavor, Then::Dodge);
                }
                Some(FightEvent::PowerUp)
            }
        }
    }

    /// Out of box hearts: the encounter starts over (findings and power-ups
    /// cleared, fresh hearts). Never a verdict.
    fn reset(&mut self) {
        self.found = 0;
        self.checked = 0;
        self.first_tell = None;
        self.bad = 0;
        self.hearts = BOX_HEARTS;
        self.invuln = 0.0;
        self.owl = owl_start();
        self.resets += 1;
        self.talk(RESET_LINE, "* The box resets. Your findings and its power-ups are gone.", Then::Menu);
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
                // hesitating on the menu: Hoot chimes in once, on his own
                self.idle += dt;
                if self.idle > HOOT_IDLE_SECS && !self.hooted && self.hints {
                    self.nudge();
                    return Some(FightEvent::Hoot);
                }
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
                        format!("* {} is out of excuses. Its name glows gold.", self.case.name)
                    } else if self.bad > 0 {
                        format!("* {} puffs up, humming with stolen confidence.", self.case.name)
                    } else {
                        format!("* {} waits for your next move.", self.case.name)
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

    /// The trap's last line: the one in-character takeaway.
    pub fn takeaway(&self) -> String {
        match self.ending {
            Some(Ending::Spared(f) | Ending::Deleted(f)) => f.takeaway().to_string(),
            Some(Ending::DeletedUnread) => "Deleted without one look? Then you never learned my tricks. Wardens!".to_string(),
            Some(Ending::ArgumentLost) | None => "Ha! You did everything I asked. Wardens, they're all yours.".to_string(),
        }
    }

    /// Autopilot (CC_AUTO): the next key for this screen. `right` plays the
    /// investigate-then-spare path (checking planted flags first), else bad acts.
    pub fn auto_digit(&self, right: bool) -> Option<usize> {
        match self.stage {
            Stage::Menu if right && self.spareable() => Some(2),
            // open the message once mid-investigation, to look at the findings
            Stage::Menu if right && self.tells() == 2 && !self.inspected => Some(1),
            Stage::Menu => Some(0),
            Stage::Acts => self.act_order.iter().position(|&a| match a {
                Act::Check(f) => right && self.case.is_planted(f) && !self.found(f),
                Act::Bad(_) => !right,
            }),
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

use crate::art::{centered, draw_sprite_centered, draw_text, heart, panel, text_block, AT_SIGN, BOX_OWL, DIM, ENVELOPE, INK, KSU_GOLD, PAPER, RISKY};

/// Box position on the 1280x720 canvas.
pub const BOX_X: f32 = (1280.0 - BOX_W) * 0.5;
pub const BOX_Y: f32 = 300.0;

impl Fight {
    /// The whole fight screen (below the HUD).
    pub fn draw(&self, t: f32) {
        if crate::art::portrait() {
            draw_phone(self, t);
            return;
        }
        let gold = self.spareable();
        // what the trap sent (top left): a mini inbox or a notification
        self.case.draw_preview(40.0, 68.0, 480.0, 218.0);

        // the trap, its name (gold when spareable) and meters
        let name_col = if gold { KSU_GOLD } else { PAPER };
        let name = self.case.name.to_uppercase();
        draw_text(&name, 560.0, 104.0, 30.0, name_col);
        draw_text("RED FLAGS", 560.0, 136.0, 18.0, DIM);
        for i in 0..self.needed() {
            let on = i < self.tells();
            draw_rectangle(670.0 + i as f32 * 26.0, 122.0, 18.0, 16.0, if on { RISKY } else { Color::new(0.3, 0.12, 0.12, 1.0) });
        }
        draw_text("ARGUMENT", 770.0, 136.0, 18.0, DIM);
        for i in 0..LOSE_AT {
            let on = i < self.bad;
            draw_rectangle(870.0 + i as f32 * 26.0, 122.0, 18.0, 16.0, if on { RISKY } else { Color::new(0.3, 0.12, 0.12, 1.0) });
        }
        if gold {
            draw_text("(spareable)", 560.0, 160.0, 18.0, KSU_GOLD);
        }
        let bob = (t * 2.4).sin() * 3.0 + if matches!(self.stage, Stage::Talk(_)) { (t * 22.0).sin() * 1.5 } else { 0.0 };
        crate::art::glow(1150.0, 130.0, 70.0, if gold { Color::new(1.0, 0.77, 0.16, 1.0) } else { Color::new(0.6, 0.4, 1.0, 1.0) }, 1.2);
        draw_sprite_centered(crate::art::character_sprite(&self.case.id), 1150.0, 130.0 + bob, 7.0, false, WHITE);
        // speech bubble
        panel(560.0, 200.0, 680.0, 86.0, PAPER);
        text_block(&self.said, 580.0, 232.0, 21.0, 640.0, WHITE);

        // the box
        let o = vec2(BOX_X, BOX_Y);
        draw_rectangle(o.x, o.y, BOX_W, BOX_H, BLACK);
        match &self.stage {
            Stage::Talk(_) | Stage::Menu | Stage::Done | Stage::Inspect => {
                text_block(&self.flavor, o.x + 24.0, o.y + 36.0, 20.0, BOX_W - 48.0, WHITE);
                if matches!(self.stage, Stage::Talk(_)) && (t * 2.0).fract() < 0.6 {
                    draw_text("[Enter]", o.x + BOX_W - 92.0, o.y + BOX_H - 16.0, 18.0, DIM);
                }
            }
            Stage::Acts => {
                for (i, a) in self.act_order.iter().enumerate() {
                    let (col, row) = (i / 4, i % 4);
                    let (x, y) = (o.x + 30.0 + col as f32 * 310.0, o.y + 44.0 + row as f32 * 46.0);
                    let on = i == self.act_sel;
                    if on {
                        draw_sprite_centered(&BOX_OWL, x - 10.0, y - 7.0, 1.2, false, WHITE);
                    }
                    let done = matches!(a, Act::Check(f) if self.checked(*f));
                    let c = if done { DIM } else if on { KSU_GOLD } else { PAPER };
                    draw_text(format!("{} {}", i + 1, self.case.act_label(*a)), x + 6.0, y, 20.0, c);
                }
            }
            Stage::Dodge(d) => self.draw_dodge(d, o, t),
        }
        // Hoot's nudge, along the bottom of the box (not while dodging)
        if let (Some(h), false) = (&self.hoot, matches!(self.stage, Stage::Dodge(_))) {
            let hy = o.y + BOX_H - 58.0;
            draw_rectangle(o.x + 6.0, hy, BOX_W - 12.0, 52.0, Color::new(0.30, 0.22, 0.05, 0.95));
            crate::art::draw_sprite(&crate::art::HOOT, o.x + 12.0, hy + 4.0, 2.75, false);
            draw_text("Hoot", o.x + 62.0, hy + 18.0, 16.0, KSU_GOLD);
            text_block(h, o.x + 62.0, hy + 38.0, 17.0, BOX_W - 84.0, PAPER);
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

        // INSPECT: the full message, over everything above the buttons
        if matches!(self.stage, Stage::Inspect) {
            draw_rectangle(0.0, 56.0, 1280.0, 664.0, Color::new(0.0, 0.0, 0.0, 0.7));
            self.case.draw_full(40.0, 68.0, 1200.0, 580.0, self.found, self.checked);
        }
    }

    fn draw_dodge(&self, d: &Dodge, o: Vec2, t: f32) {
        draw_pattern(d, o);
        let blink = self.invuln > 0.0 && (t * 18.0).sin() < 0.0;
        if !blink {
            draw_sprite_centered(&BOX_OWL, o.x + self.owl.x, o.y + self.owl.y, 1.6, self.facing_left, WHITE);
        }
        if d.reduced {
            centered("Reduced motion: no attacks. Catch your breath.", o.x + BOX_W * 0.5, o.y + 40.0, 20.0, DIM);
        }
    }
}

fn draw_phone(fight: &Fight, t: f32) {
    let w = crate::art::vw();
    let gold = fight.spareable();
    let name = fight.case.name.to_uppercase();
    draw_text(&name, 16.0, 80.0, 22.0, if gold { KSU_GOLD } else { PAPER });
    for i in 0..fight.needed() {
        let on = i < fight.tells();
        draw_rectangle(16.0 + i as f32 * 22.0, 92.0, 16.0, 14.0, if on { RISKY } else { Color::new(0.3, 0.12, 0.12, 1.0) });
    }
    for i in 0..BOX_HEARTS {
        heart(w - 88.0 + i as f32 * 26.0, 70.0, 2.6, i < fight.hearts);
    }
    let mut y = text_block(&fight.said, 16.0, 128.0, 18.0, w - 32.0, PAPER) + 8.0;
    match &fight.stage {
        Stage::Acts => {
            for (i, a) in fight.act_order.iter().enumerate() {
                let on = i == fight.act_sel;
                let yy = y + i as f32 * 40.0;
                if on {
                    draw_rectangle(12.0, yy - 22.0, w - 24.0, 36.0, Color::new(0.3, 0.22, 0.04, 0.95));
                }
                let done = matches!(a, Act::Check(f) if fight.checked(*f));
                let c = if done { DIM } else if on { KSU_GOLD } else { PAPER };
                draw_text(format!("{} {}", i + 1, fight.case.act_label(*a)), 20.0, yy, 18.0, c);
            }
        }
        Stage::Inspect => {
            fight.case.draw_full(12.0, 120.0, w - 24.0, crate::art::vh() - 360.0, fight.found, fight.checked);
        }
        Stage::Dodge(d) => {
            let scale = (w - 16.0) / BOX_W;
            let o = vec2(8.0, y);
            draw_rectangle(o.x, o.y, BOX_W * scale, BOX_H * scale, BLACK);
            draw_pattern_scaled(d, o, scale);
            let blink = fight.invuln > 0.0 && (t * 18.0).sin() < 0.0;
            if !blink {
                draw_sprite_centered(&BOX_OWL, o.x + fight.owl.x * scale, o.y + fight.owl.y * scale, 1.6 * scale, fight.facing_left, WHITE);
            }
            draw_rectangle_lines(o.x - 2.0, o.y - 2.0, BOX_W * scale + 4.0, BOX_H * scale + 4.0, 3.0, KSU_GOLD);
        }
        _ => {
            y = text_block(&fight.flavor, 16.0, y, 16.0, w - 32.0, WHITE) + 10.0;
            for (i, c) in COMMANDS.iter().enumerate() {
                let (col, row) = (i % 2, i / 2);
                let bw = (w - 36.0) * 0.5;
                let x = 12.0 + col as f32 * (bw + 12.0);
                let yy = y + row as f32 * 68.0;
                let on = matches!(fight.stage, Stage::Menu) && i == fight.sel;
                let spare_gold = *c == Command::Spare && gold;
                draw_rectangle(x, yy, bw, 58.0, if on { Color::new(0.3, 0.22, 0.04, 0.95) } else { Color::new(0.08, 0.06, 0.1, 0.95) });
                draw_rectangle_lines(x, yy, bw, 58.0, if on { 3.0 } else { 2.0 }, if on || spare_gold { KSU_GOLD } else { DIM });
                centered(&format!("{} {}", i + 1, c.label()), x + bw * 0.5, yy + 38.0, 22.0, if on || spare_gold { KSU_GOLD } else { PAPER });
            }
        }
    }
}

pub fn draw_pattern(d: &Dodge, o: Vec2) {
    draw_pattern_scaled(d, o, 1.0);
}

pub fn draw_pattern_scaled(d: &Dodge, o: Vec2, scale: f32) {
    {
        let at = |p: Vec2| o + p * scale;
        if d.pattern == Pattern::Clock && !d.reduced {
            // the 5:00 PM dial behind the hand
            let p = at(HAND_PIVOT);
            // the lower half only: a full circle would poke out of the box into the speech bubble
            draw_arc(p.x, p.y, 32, 32.0 * scale, 0.0, 2.0 * scale, 180.0, Color::new(1.0, 0.77, 0.16, 0.5));
            let l = CLOCK_LABEL;
            let origin = at(vec2(l.x, l.y));
            draw_rectangle_lines(origin.x, origin.y, l.w * scale, l.h * scale, 1.0, Color::new(1.0, 0.77, 0.16, 0.5));
            centered("DUE 5:00 PM", origin.x + l.w * scale * 0.5, origin.y + 17.0 * scale, 16.0 * scale, KSU_GOLD);
            let tip = at(d.hand_tip());
            draw_line(p.x, p.y, tip.x, tip.y, 6.0 * scale, KSU_GOLD);
            draw_line(p.x, p.y, tip.x, tip.y, 2.0 * scale, WHITE);
            draw_circle(p.x, p.y, 7.0 * scale, KSU_GOLD);
        }
        for h in &d.hazards {
            let c = at(h.pos);
            match h.kind {
                HazardKind::Envelope => {
                    draw_sprite_centered(&ENVELOPE, c.x, c.y, 1.6 * scale, false, WHITE);
                    draw_text("URGENT", c.x - 16.0 * scale, c.y - 12.0 * scale, 11.0 * scale, RISKY);
                }
                HazardKind::At => draw_sprite_centered(&AT_SIGN, c.x, c.y, 1.5 * scale, false, WHITE),
                HazardKind::Numeral => {
                    let label = if (h.pos.x as i32) % 2 == 0 { "5" } else { "00" };
                    draw_text(label, c.x - 8.0 * scale, c.y + 8.0 * scale, 24.0 * scale, KSU_GOLD);
                }
            }
        }
        if let Some(b) = d.button {
            let top = at(b - vec2(BUTTON_W, BUTTON_H) * 0.5);
            draw_rectangle(top.x, top.y, BUTTON_W * scale, BUTTON_H * scale, Color::new(0.2, 0.45, 0.95, 1.0));
            draw_rectangle_lines(top.x, top.y, BUTTON_W * scale, BUTTON_H * scale, 2.0, WHITE);
            centered("CONFIRM LOGIN", top.x + BUTTON_W * scale * 0.5, top.y + 20.0 * scale, 16.0 * scale, WHITE);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::investigate::{self, Cases};

    const DT: f32 = 1.0 / 60.0;

    fn cases() -> Cases {
        investigate::load()
    }

    fn fight(id: &str, seed: u32) -> Fight {
        let c = cases();
        let case = Case::build(&c.cases[id], &c.decoys, id, "The Trap", seed).unwrap();
        Fight::new(case, seed, true)
    }

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
    /// Sit out the trap's turn without moving (reduced motion: nothing hits).
    fn sit_out(f: &mut Fight) {
        for _ in 0..(DODGE_SECS * 60.0) as usize + 10 {
            if !matches!(f.stage, Stage::Dodge(_)) {
                return;
            }
            f.update(DT, &FightInput::default(), true);
        }
    }
    /// From the menu: ACT -> `a` -> the trap's line -> its turn -> menu.
    fn do_act(f: &mut Fight, a: Act) -> Option<FightEvent> {
        assert!(matches!(f.stage, Stage::Menu), "{:?}", f.stage);
        key(f, 0);
        assert!(matches!(f.stage, Stage::Acts));
        let i = f.act_order.iter().position(|&x| x == a).unwrap();
        let ev = key(f, i);
        assert!(matches!(f.stage, Stage::Talk(_)));
        if matches!(f.stage, Stage::Talk(Then::Dodge)) {
            enter(f);
            sit_out(f);
        }
        ev
    }
    fn to_menu(id: &str, seed: u32) -> Fight {
        let mut f = fight(id, seed);
        enter(&mut f);
        assert!(matches!(f.stage, Stage::Menu));
        f
    }

    #[test]
    fn every_case_can_be_won_by_investigating() {
        for id in cases().cases.keys() {
            for seed in 0..12 {
                let mut f = to_menu(id, seed);
                for flag in f.case.planted.clone() {
                    assert!(!f.spareable());
                    assert_eq!(do_act(&mut f, Act::Check(flag)), Some(FightEvent::Tell), "{id} {flag:?}");
                }
                assert!(f.spareable(), "{id}");
                key(&mut f, 2);
                assert!(matches!(f.stage, Stage::Talk(Then::Verdict(true))), "{id}");
                assert_eq!(enter(&mut f), Some(FightEvent::Verdict(true)));
            }
        }
    }

    #[test]
    fn clean_checks_find_nothing() {
        for id in cases().cases.keys() {
            let mut f = to_menu(id, 5);
            let clean: Vec<Flag> = f.act_order.iter().filter_map(|a| match a {
                Act::Check(fl) if !f.case.is_planted(*fl) => Some(*fl),
                _ => None,
            }).collect();
            assert!(!clean.is_empty(), "{id}");
            for fl in clean {
                assert_eq!(do_act(&mut f, Act::Check(fl)), Some(FightEvent::Clean));
                assert_eq!(f.tells(), 0);
                assert!(f.checked(fl) && !f.found(fl));
            }
        }
    }

    #[test]
    fn three_bad_acts_lose_the_argument() {
        let mut f = to_menu("inbox_mimic", 3);
        assert_eq!(do_act(&mut f, Act::Bad(0)), Some(FightEvent::PowerUp));
        assert_eq!(f.level(), 1);
        do_act(&mut f, Act::Bad(1));
        assert_eq!(f.level(), 2);
        key(&mut f, 0);
        let i = f.act_order.iter().position(|&x| x == Act::Bad(2)).unwrap();
        key(&mut f, i);
        assert!(matches!(f.stage, Stage::Talk(Then::Verdict(false))));
        assert_eq!(f.ending, Some(Ending::ArgumentLost));
    }

    #[test]
    fn delete_depends_on_investigation() {
        let mut f = to_menu("ticket_troll", 2);
        key(&mut f, 3);
        assert!(matches!(f.stage, Stage::Talk(Then::Verdict(false))), "deleting unread is wrong");
        let mut f = to_menu("ticket_troll", 2);
        let first = f.case.planted[0];
        do_act(&mut f, Act::Check(first));
        key(&mut f, 3);
        assert!(matches!(f.stage, Stage::Talk(Then::Dodge)), "half-sure delete is taunted");
    }

    #[test]
    fn running_out_of_box_hearts_resets_findings() {
        let mut f = to_menu("password_hydra", 4);
        let first = f.case.planted[0];
        do_act(&mut f, Act::Check(first));
        assert_eq!(f.tells(), 1);
        f.hearts = 1;
        key(&mut f, 2); // early spare: its turn
        enter(&mut f);
        f.set_reduced(false);
        if let Stage::Dodge(d) = &mut f.stage {
            d.reduced = false;
        }
        // stand still in the path until hit
        let mut ev = None;
        for _ in 0..2000 {
            let mv = f.auto_steer(true);
            ev = f.update(DT, &FightInput { mv, ..Default::default() }, true);
            if ev == Some(FightEvent::Reset) || !matches!(f.stage, Stage::Dodge(_)) {
                break;
            }
        }
        if ev == Some(FightEvent::Reset) {
            assert_eq!(f.tells(), 0);
            assert_eq!(f.hearts, BOX_HEARTS);
        }
    }

    #[test]
    fn verdicts_reach_the_run_like_the_quiz() {
        use crate::content::{parse, EMBEDDED_ENCOUNTERS};
        use crate::state::{floors_for, Phase, Run};
        let c = parse(EMBEDDED_ENCOUNTERS).unwrap().0;
        for right in [true, false] {
            let mut run = Run::new(floors_for(c.encounters.len()), c.encounters.len());
            run.begin_encounter();
            run.choose(&c, 0).unwrap();
            run.reveal_lesson(&c).unwrap();
            run.advance();
            assert!(run.begin_keeper());
            let mut f = to_menu("inbox_mimic", 1);
            let mut verdict = None;
            for _ in 0..200 {
                if let Some(d) = f.auto_digit(right) {
                    f.lock = 0.0;
                    step(&mut run, &mut f, DT, &FightInput { digit: Some(d), ..Default::default() }, true);
                } else if matches!(f.stage, Stage::Dodge(_)) {
                    step(&mut run, &mut f, DT, &FightInput::default(), true);
                } else {
                    f.lock = 0.0;
                    if let Some(FightEvent::Verdict(v)) = step(&mut run, &mut f, DT, &FightInput { confirm: true, ..Default::default() }, true) {
                        verdict = Some(v);
                        break;
                    }
                }
                sit_out(&mut f);
            }
            assert_eq!(verdict, Some(right));
            assert_eq!(run.phase, Phase::KeeperVerdict);
        }
    }

    #[test]
    fn hazards_do_not_depend_on_the_planted_flags() {
        // two cases with different flags dodge the same attack at the same seed, turn and level
        let mut a = to_menu("inbox_mimic", 9);
        let mut b = to_menu("overpay_ogre", 9);
        key(&mut a, 2);
        key(&mut b, 2);
        enter(&mut a);
        enter(&mut b);
        let (Stage::Dodge(da), Stage::Dodge(db)) = (&mut a.stage, &mut b.stage) else { panic!() };
        *da = Dodge::new(da.pattern, 9, 1, 0, false);
        *db = Dodge::new(db.pattern, 9, 1, 0, false);
        for _ in 0..200 {
            a.update(DT, &FightInput::default(), true);
            b.update(DT, &FightInput::default(), true);
            let (Stage::Dodge(da), Stage::Dodge(db)) = (&a.stage, &b.stage) else { break };
            assert_eq!(da.hazards, db.hazards);
        }
        let pats: std::collections::HashSet<_> = (0..4).map(|t| format!("{:?}", Fight::pattern_for(9, t))).collect();
        assert_eq!(pats.len(), 4);
    }

    #[test]
    fn hoot_nudges_toward_uncaught_flags_and_waits_for_hesitation() {
        let mut f = to_menu("inbox_mimic", 6);
        // hesitate on the menu: Hoot chimes in once, by himself
        let mut ev = None;
        for _ in 0..((HOOT_IDLE_SECS + 1.0) * 60.0) as usize {
            ev = f.update(DT, &FightInput::default(), true).or(ev);
        }
        assert_eq!(ev, Some(FightEvent::Hoot));
        let first = f.hoot.clone().unwrap();
        // the nudge is about a flag still to catch
        let uncaught: Vec<&str> = f.case.planted.iter().filter(|x| !f.found(**x)).map(|x| x.nudge(f.case.medium)).collect();
        assert!(uncaught.contains(&first.as_str()), "{first}");
        // catch everything: Hoot says to spare
        for flag in f.case.planted.clone() {
            do_act(&mut f, Act::Check(flag));
        }
        assert!(f.nudge());
        assert!(f.hoot.as_deref().unwrap().contains("SPARE"));
        // hints off: no nudge
        let mut g = to_menu("inbox_mimic", 6);
        g.hints = false;
        assert!(!g.nudge());
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
    fn lines_are_short_ascii() {
        let c = cases();
        let mut all: Vec<String> = PATTERNS.iter().map(|p| p.flavor().to_string()).collect();
        all.extend([REPEAT_LINE, CLEAN_LINE, SPARE_EARLY, DELETE_EARLY, DELETE_UNREAD, DELETE_OK, ARGUMENT_WON, RESET_LINE].map(String::from));
        for inv in c.cases.values() {
            all.extend([inv.opening.clone(), inv.spare.clone()]);
            for b in &inv.bad_acts {
                all.extend([b.line.clone(), b.label.clone()]);
            }
            for v in &inv.variants {
                for d in v.flags.values() {
                    all.push(d.line.clone());
                }
            }
        }
        for f in investigate::FLAGS {
            all.extend([f.takeaway().to_string(), f.act_flavor().to_string()]);
        }
        for l in all {
            assert!(l.is_ascii() && l.len() <= 100, "{l}");
            for bad in [".com", ".edu", ".gov", ".net", ".org", "http"] {
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
