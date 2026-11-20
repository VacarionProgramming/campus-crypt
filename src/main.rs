//! Campus Crypt: a humorous pixel-art dungeon crawler of scams, deadlines, and
//! split-second choices, for KSU students. Built on Delve (macroquad).
//!
//! Module map:
//!   content.rs  - loads/validates assets/content/encounters.json (the content contract)
//!   state.rs    - deterministic state machine + resources (the only place state changes)
//!   ai.rs       - optional AI reaction line with offline fallback + output filter
//!   ai_features/ - AI extras (Dey's backend): scam generator, Keeper banter, debrief,
//!                 spot-the-fake, owl hints, difficulty narration; all with scripted fallbacks
//!   crawl.rs    - real-time floors: line of sight, monsters, combat, wardens (from Delve)
//!   keeper.rs   - the Floor Keeper's question, built from the content file
//!   keeper_fight.rs - floor 1's Keeper fight: Undertale-style turns with the Inbox Mimic
//!   settings.rs - Options settings (volume, motion, hints, AI, Arduino), saved between launches
//!   display_live.rs - live LCD/matrix layouts and sketch-upload hand-off for the Arduino display
//!   art.rs      - procedural pixel art, text layout helpers
//!   audio.rs    - procedurally synthesised sound effects and music (from Delve)
//!   hardware.rs - optional Arduino/ELEGOO controller link (from Delve)
//!   main.rs     - screens, input, rendering of each phase, the frame loop

mod ai;
mod ai_features;
mod art;
mod perf;
mod risk_it;
mod audio;
mod content;
mod crawl;
mod display_live;
mod hardware;
mod keeper;
mod keeper_fight;
mod settings;
mod music;
mod state;

use ai::{LineSource, ReactionRequest, Reactor};
use ai_features::{scamgen, tips, AiFeatures, ScamMessage, SpotRound, TierChange};
use art::*;
// crisp text: these shadow macroquad's versions from the prelude glob
use art::{draw_text, measure_text};
use audio::{Audio, Sfx};
use music::{Music, Track};
use content::{Effects, Encounter, Loaded, Outcome};
use crawl::{wardens_for, Control, Pace, Power, Tier, World, WorldEvent, TILE};
use hardware::{Hardware, HwEvent, HwState};
use keeper::Quiz;
use keeper_fight::{Fight, FightEvent, FightInput, Stage as FightStage};
use risk_it::{RiskEvent, RiskIt};
use macroquad::prelude::*;
use state::{Ending, Phase, Run, MAX_FOCUS, MAX_HEALTH};
use std::fmt::Write as _;
use std::rc::Rc;

const HUD_H: f32 = 56.0;
const ROOM_TOP: f32 = HUD_H;
/// how close you need to be to talk to the floor's character or the Keeper
const TALK_RANGE: f32 = 2.2 * TILE;
/// the floor's narration shows up when you get this close to its character
const NARRATE_RANGE: f32 = 4.0 * TILE;
/// characters per second for the typewriter effect on the reaction line
const TYPE_CPS: f32 = 70.0;
/// a freshly opened choice screen ignores answers for this long
const CHOICE_LOCK_SECS: f32 = keeper_fight::LOCK_SECS;
/// floors (0-based) with a suspicious-message prop, using AI-prefetched scam slots 0, 1, 2
const SCAM_FLOORS: [usize; 3] = [1, 4, 7];
/// floors (0-based) with a spot-the-fake board; rounds use scripted slots 3+ (never a
/// prefetched one) and skip any template the scam props already used this run
const SPOT_FLOORS: [usize; 3] = [2, 5, 8];

// ---------------------------------------------------------------- input

/// One frame of input from keyboard, mouse and (optionally) the joystick,
/// already converted to virtual-canvas coordinates.
#[derive(Default, Clone, Copy)]
struct Input {
    up: bool,
    down: bool,
    left: bool,
    right: bool,
    confirm: bool,
    back: bool,
    /// E / Enter (or the joystick button near something): talk, answer, descend
    interact: bool,
    /// Space / J / left mouse held (or the joystick button): fire the foam-dart blaster
    attack: bool,
    /// Shift / K / right-click: dodge roll
    dodge: bool,
    /// the attack came from the mouse: aim at it
    mouse_attack: bool,
    /// autopilot aim in world coordinates
    aim_world: Option<Vec2>,
    digit: Option<usize>,
    mouse: Vec2,
    click: bool,
    mouse_moved: bool,
    mv: Vec2,
}

impl Input {
    fn read(view: &View, hw: &Hardware) -> Input {
        let p = |k: KeyCode| is_key_pressed(k);
        let d = |k: KeyCode| is_key_down(k);
        let nav = hw.nav();
        let mut mv = Vec2::ZERO;
        if d(KeyCode::W) || d(KeyCode::Up) {
            mv.y -= 1.0;
        }
        if d(KeyCode::S) || d(KeyCode::Down) {
            mv.y += 1.0;
        }
        if d(KeyCode::A) || d(KeyCode::Left) {
            mv.x -= 1.0;
        }
        if d(KeyCode::D) || d(KeyCode::Right) {
            mv.x += 1.0;
        }
        if mv != Vec2::ZERO {
            mv = mv.normalize();
        } else {
            mv = hw.move_vec();
        }
        let digit = [KeyCode::Key1, KeyCode::Key2, KeyCode::Key3, KeyCode::Key4, KeyCode::Key5, KeyCode::Key6, KeyCode::Key7]
            .iter()
            .position(|&k| p(k));
        Input {
            up: p(KeyCode::Up) || p(KeyCode::W) || nav.up,
            down: p(KeyCode::Down) || p(KeyCode::S) || nav.down,
            left: p(KeyCode::Left) || p(KeyCode::A) || nav.left,
            right: p(KeyCode::Right) || p(KeyCode::D) || nav.right,
            // no Space: it's the fire key, and a held trigger must never pick an answer
            confirm: p(KeyCode::Enter) || p(KeyCode::KpEnter) || p(KeyCode::E) || nav.select,
            back: p(KeyCode::Escape),
            interact: p(KeyCode::E) || p(KeyCode::Enter) || p(KeyCode::KpEnter) || nav.select,
            attack: d(KeyCode::Space) || d(KeyCode::J) || is_mouse_button_down(MouseButton::Left) || hw.button_held(),
            mouse_attack: is_mouse_button_down(MouseButton::Left),
            dodge: p(KeyCode::LeftShift) || p(KeyCode::RightShift) || p(KeyCode::K) || is_mouse_button_pressed(MouseButton::Right),
            aim_world: None,
            digit,
            mouse: view.to_virtual(mouse_position().into()),
            click: is_mouse_button_pressed(MouseButton::Left),
            mouse_moved: mouse_delta_position() != Vec2::ZERO,
            mv,
        }
    }
}

/// Letterboxed camera mapping the 1280x720 virtual canvas onto the window.
///
/// `Camera2D::viewport` is in physical framebuffer pixels while
/// `screen_width()`/`mouse_position()` are in logical pixels, so the old code
/// was only right at dpi_scale 1. We size the viewport in physical pixels and
/// map the mouse ourselves through the same integer rectangle, so clicks line
/// up exactly at any window size or DPI. A minimized (0x0) window yields a
/// harmless 1x1 viewport instead of NaN mouse coordinates.
struct View {
    cam: Camera2D,
    /// letterbox rectangle in logical pixels
    rect: Rect,
}

impl View {
    fn new() -> View {
        let dpi = screen_dpi_scale().max(0.01);
        let (pw, ph) = ((screen_width() * dpi).max(1.0), (screen_height() * dpi).max(1.0));
        let s = (pw / VW).min(ph / VH);
        let (w, h) = ((VW * s).round().max(1.0), (VH * s).round().max(1.0));
        let (x, y) = (((pw - w) * 0.5).floor(), ((ph - h) * 0.5).floor());
        let mut cam = Camera2D::from_display_rect(Rect::new(0.0, VH, VW, -VH));
        cam.viewport = Some((x as i32, y as i32, w as i32, h as i32));
        // text is rasterized at this many screen pixels per canvas pixel
        art::set_text_scale(w / VW);
        View { cam, rect: Rect::new(x / dpi, y / dpi, w / dpi, h / dpi) }
    }

    fn to_virtual(&self, p: Vec2) -> Vec2 {
        let r = self.rect;
        vec2((p.x - r.x) / r.w * VW, (p.y - r.y) / r.h * VH)
    }
}

/// A formatted string rebuilt only when its inputs (`key`) change, so
/// steady-state frames don't allocate for HUD/title text.
#[derive(Default)]
struct Cached {
    key: Option<u64>,
    s: String,
}

impl Cached {
    fn get(&mut self, key: u64, f: impl FnOnce(&mut String)) -> &str {
        if self.key != Some(key) {
            self.s.clear();
            f(&mut self.s);
            self.key = Some(key);
        }
        &self.s
    }
}

/// Pack small counters into one cache key.
fn key(parts: &[usize]) -> u64 {
    parts.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, &p| (h ^ p as u64).wrapping_mul(0x100_0000_01b3))
}

#[derive(Default)]
struct Texts {
    content: Cached,
    controller: Cached,
    status_age: f32,
    room_line: Cached,
    knowledge: Cached,
    room_hud: Cached,
    kills: Cached,
    objective: Cached,
    keeper_hint: Cached,
    enc_name: Cached,
    thinking: Cached,
    chose: Cached,
    ending_stats: Cached,
    ending_keepers: Cached,
    boss_bar: Cached,
    /// (header, "Spot it", "Next time") per decision, built once per ending
    notes: Vec<(String, Option<String>, String, Outcome)>,
    notes_key: Option<u64>,
    /// (heading, body) font sizes that fit the field notes in the ending panel
    notes_sizes: (f32, f32),
    /// effect chips for the lesson, built once per reveal: (text, width, colour)
    chips: Vec<(String, f32, Color)>,
    help: Cached,
    enc_help: Cached,
}

// ---------------------------------------------------------------- app

/// The encounter for floor `room` (floors past the end of the content reuse
/// its encounters; free fn so callers can hold it across `&mut self` calls via
/// a cloned `Rc<Loaded>`).
fn enc_at(loaded: &Loaded, room: usize) -> &Encounter {
    let list = &loaded.content.encounters;
    &list[state::encounter_for_floor(room, list.len())]
}

/// How long a tip bubble stays up on the floor (on a scenario it stays until you choose).
const OWL_FLOOR_SECS: f32 = 11.0;

fn new_run(loaded: &Loaded) -> Run {
    let n = loaded.content.encounters.len();
    Run::new(state::floors_for(n), n)
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Screen {
    Title,
    Playing,
}

/// What the title screen shows over the crypt backdrop.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum TitleView {
    Main,
    Options,
    HowTo,
}

/// The title's crypt door list (the web build can't quit).
#[cfg(not(target_arch = "wasm32"))]
const MENU_ITEMS: &[&str] = &["ENTER THE CRYPT", "OPTIONS", "HOW TO PLAY", "QUIT"];
#[cfg(target_arch = "wasm32")]
const MENU_ITEMS: &[&str] = &["ENTER THE CRYPT", "OPTIONS", "HOW TO PLAY"];

/// One row of the Options menu (title and pause screen).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Opt {
    Master,
    Music,
    MusicOn,
    Motion,
    Hints,
    Ai,
    Arduino,
    /// title only
    Back,
}

const OPTS: [Opt; 8] = [Opt::Master, Opt::Music, Opt::MusicOn, Opt::Motion, Opt::Hints, Opt::Ai, Opt::Arduino, Opt::Back];

/// A small flickering flame (the title menu's cursor).
fn flame(x: f32, y: f32, t: f32, seed: f32) {
    let fl = 1.0 + 0.15 * (t * 11.0 + seed).sin() + 0.08 * (t * 27.0 + seed * 3.0).sin();
    glow(x, y, 46.0 * fl, Color::new(1.0, 0.6, 0.2, 1.0), 1.2);
    draw_rectangle(x - 5.0, y - 6.0 - 4.0 * fl, 10.0, 14.0 + 4.0 * fl, Color::new(1.0, 0.55, 0.12, 1.0));
    draw_rectangle(x - 2.5, y - 2.0 - 2.0 * fl, 5.0, 8.0 + 2.0 * fl, Color::new(1.0, 0.92, 0.55, 1.0));
}

/// The reaction line for the current choice, once decided.
struct Line {
    text: String,
    /// char count of `text` (not recounted every frame)
    len: usize,
    /// "AI-voiced line" / "scripted line (why)", built once
    tag: String,
    tag_w: f32,
    shown: f32, // typewriter progress in chars
}

impl Line {
    fn new(text: String, source: LineSource) -> Line {
        let tag = source_tag(&source, "line");
        let tag_w = measure_text(&tag, None, 18, 1.0).width;
        Line { len: text.chars().count(), text, tag, tag_w, shown: 0.0 }
    }
}

/// "AI-voiced line" / "scripted line (why)": tiny print under anything an AI may write.
fn source_tag(source: &LineSource, what: &str) -> String {
    match source {
        LineSource::Ai if what == "line" => "AI-voiced line".to_string(),
        LineSource::Ai => format!("AI-written {what}"),
        LineSource::Fallback(why) => format!("scripted {what} ({why})"),
    }
}

/// A floor's side game (planned at run start, so a run with CC_SEED replays it).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Side {
    /// a suspicious message (scam generator pool slot)
    Scam(usize),
    /// spot-the-fake round (scam slot used for the fake card)
    Spot(usize),
}

/// Which floors get a side game: scam props on `SCAM_FLOORS` with the AI-prefetched
/// pool slots 0..3; spot-the-fake on `SPOT_FLOORS` with the first scripted slots
/// (3+) whose template no earlier side game used, so no message repeats in a run
/// (a board is left out when every template has been shown).
fn plan_sides(data: &ai_features::data::FeatureData, seed: u32, rooms: usize) -> Vec<Option<Side>> {
    let template = |slot: usize| scamgen::plan(data, seed, slot).template;
    let mut sides = vec![None; rooms];
    let mut used = Vec::new();
    for (slot, &f) in SCAM_FLOORS.iter().enumerate().take(scamgen::POOL_SIZE) {
        if f < rooms {
            sides[f] = Some(Side::Scam(slot));
            used.push(template(slot));
        }
    }
    let mut next = scamgen::POOL_SIZE;
    let templates = data.scam_templates.len().max(1);
    for &f in &SPOT_FLOORS {
        let pick = (next..scamgen::POOL_SIZE + templates * 2).find(|&s| !used.contains(&template(s)));
        if let (Some(slot), true) = (pick, f < rooms) {
            sides[f] = Some(Side::Spot(slot));
            used.push(template(slot));
            next = slot + 1;
        }
    }
    sides
}

/// One message card's text (a generated scam, or either spot-the-fake card).
struct Msg {
    channel: String,
    from: String,
    subject: String,
    body: String,
    link: Option<&'static str>,
    sign_off: String,
}

impl Msg {
    fn of_scam(m: &ScamMessage) -> Msg {
        Msg { channel: m.channel.clone(), from: m.from.clone(), subject: m.subject.clone(), body: m.body.clone(), link: m.link, sign_off: m.sign_off.clone() }
    }
    fn of_spot(r: &SpotRound, i: usize) -> Msg {
        let c = &r.cards[i];
        Msg { channel: c.channel.clone(), from: c.from.clone(), subject: c.subject.clone(), body: c.body.clone(), link: c.link, sign_off: c.sign_off.clone() }
    }
}

enum MiniKind {
    /// the message, and the tips for its red flags (built once)
    Scam(ScamMessage, Vec<String>),
    Spot(SpotRound),
}

/// A side game open on top of the paused floor.
struct Mini {
    kind: MiniKind,
    /// what the player picked and whether it was right (None while choosing)
    verdict: Option<(usize, bool)>,
    /// "AI-written message" / "scripted message (why)"
    tag: String,
}

/// Tiny right-aligned source print (on a dark strip, so it reads over the floor).
fn small_print(text: &str, right: f32, top: f32) {
    let w = measure_text(text, None, 16, 1.0).width;
    draw_rectangle(right - w - 12.0, top, w + 12.0, 20.0, PANEL);
    draw_text(text, right - w - 6.0, top + 15.0, 16.0, DIM);
}

/// What a side-game frame decided.
enum MiniStep {
    Stay,
    /// picked button / card `n`
    Answer(usize),
    /// closed (true = it was answered, so the prop goes dim)
    Close(bool),
}

/// Draw a message as a card (email, text or DM frame): a gold header strip with
/// the channel, sender and subject, the body, the link placeholder drawn as a
/// link that goes nowhere, and the sign-off. `stamp` marks it FAKE / REAL / SCAM.
/// Returns the bottom y. Sized from the whole text, so it never jumps.
fn message_card(m: &Msg, x: f32, y: f32, w: f32, size: f32, border: Color, stamp: Option<&str>) -> f32 {
    let pad = 18.0;
    let lh = size * 1.15;
    let tw = w - 2.0 * pad;
    let small = (size - 3.0).max(16.0);
    let from = format!("From: {}", m.from);
    let subj = format!("Subject: {}", m.subject);
    let n_from = wrap(&from, small, tw).len() as f32;
    let n_subj = if m.subject.is_empty() { 0.0 } else { wrap(&subj, small, tw).len() as f32 };
    let n_body = wrap(&m.body, size, tw - 16.0).len() as f32;
    let extra = f32::from(u8::from(m.link.is_some())) + f32::from(u8::from(!m.sign_off.is_empty()));
    let h = 38.0 + (n_from + n_subj) * small * 1.15 + 14.0 + n_body * lh + 16.0 + extra * lh + pad;
    panel(x, y, w, h, border);
    // header strip: what kind of message this is
    let (label, col) = match m.channel.as_str() {
        "text" => ("TEXT MESSAGE", FOCUS),
        "dm" => ("DIRECT MESSAGE", Color::new(0.75, 0.55, 1.0, 1.0)),
        _ => ("EMAIL", KSU_GOLD),
    };
    draw_rectangle(x + 3.0, y + 3.0, w - 6.0, 30.0, Color::new(col.r * 0.28, col.g * 0.28, col.b * 0.28, 1.0));
    // a tiny envelope / phone / bubble icon
    let (ix, iy) = (x + pad, y + 9.0);
    match m.channel.as_str() {
        "text" => {
            draw_rectangle_lines(ix + 4.0, iy - 2.0, 12.0, 20.0, 2.0, col);
            draw_rectangle(ix + 8.0, iy + 13.0, 4.0, 2.0, col);
        }
        "dm" => {
            draw_rectangle_lines(ix, iy, 20.0, 13.0, 2.0, col);
            draw_triangle(vec2(ix + 4.0, iy + 13.0), vec2(ix + 10.0, iy + 13.0), vec2(ix + 3.0, iy + 19.0), col);
        }
        _ => {
            draw_rectangle_lines(ix, iy, 22.0, 15.0, 2.0, col);
            draw_line(ix, iy, ix + 11.0, iy + 8.0, 2.0, col);
            draw_line(ix + 22.0, iy, ix + 11.0, iy + 8.0, 2.0, col);
        }
    }
    draw_text(label, x + pad + 32.0, y + 25.0, 20.0, col);
    if let Some(s) = stamp {
        let sw = measure_text(s, None, 24, 1.0).width + 20.0;
        draw_rectangle(x + w - sw - 10.0, y + 6.0, sw, 24.0, border);
        draw_text(s, x + w - sw, y + 25.0, 24.0, INK);
    }
    let mut yy = y + 38.0 + small;
    yy = text_block(&from, x + pad, yy, small, tw, DIM);
    if n_subj > 0.0 {
        yy = text_block(&subj, x + pad, yy, small, tw, PAPER);
    }
    draw_line(x + pad, yy - small * 0.6, x + w - pad, yy - small * 0.6, 1.0, Color::new(border.r, border.g, border.b, 0.3));
    yy += 14.0;
    // texts and DMs read as a chat bubble; emails as a page
    if m.channel != "email" {
        let bh = n_body * lh + 10.0;
        draw_rectangle(x + pad - 6.0, yy - size - 2.0, tw + 12.0, bh, Color::new(1.0, 1.0, 1.0, 0.06));
    }
    yy = text_block(&m.body, x + pad + 8.0, yy, size, tw - 16.0, PAPER) + 8.0;
    if let Some(link) = m.link {
        let lw = measure_text(link, None, size as u16, 1.0).width;
        draw_text(link, x + pad + 8.0, yy, size, FOCUS);
        draw_line(x + pad + 8.0, yy + 3.0, x + pad + 8.0 + lw, yy + 3.0, 1.5, FOCUS);
        yy += lh;
    }
    if !m.sign_off.is_empty() {
        draw_text(&m.sign_off, x + pad + 8.0, yy, size, DIM);
    }
    y + h
}

/// Test/screenshot automation (native env vars; inert on the web).
struct Auto {
    /// CC_AUTO=1: play by itself (start, crawl, pick CC_PICKS, answer CC_KEEPER, fight)
    on: bool,
    picks: Vec<usize>,
    /// CC_KEEPER=r,w,...: answer each floor's Keeper right (r) or wrong (w); default right
    keeper: Vec<bool>,
    /// CC_SHOTS=t1,t2,...: save screenshots/<prefix>N.png at those wall-clock seconds
    shots: Vec<f32>,
    prefix: String,
    exit_at: f32,
    timer: f32,
    /// CC_GOD=1: monsters and wardens can't hurt you
    god: bool,
    /// CC_FLOOR=n: start the run on floor n (0-based)
    start_floor: usize,
    /// CC_MINI=r,w,...: answer each side game (scam message, spot-the-fake) right or wrong; default right
    mini: Vec<bool>,
    /// CC_ENC_WAIT=secs: how long the autopilot hesitates on an encounter (default 2; >12 shows Hoot)
    enc_wait: f32,
    /// CC_FIGHT_SEEK=1: on floor 1's fight, fly into the attacks (hearts-reset check)
    fight_seek: bool,
    /// CC_DEBRIEF_LONG=1: replace the study guide with five ~160-200 char lines (layout check)
    long_debrief: bool,
    /// CC_SHOT_EVENTS=1: save screenshots/<prefix><screen>.png the first time each AI-extras
    /// screen is on show (side games, Hoot, Keeper taunt/verdict, tier banner, debrief), then quit
    shot_events: bool,
    /// CC_RISK=1: pick RISK IT after every right answer
    risk: bool,
    /// CC_TITLE=options|howto: open the title on that view (for screenshots)
    title_view: Option<String>,
    shots_taken: Vec<String>,
    /// a screenshot to save after this frame is drawn
    shot_request: Option<String>,
    /// last (screen, phase, floor, wardens left) printed by the autopilot trace
    last: Option<(Screen, Phase, usize, u8)>,
}

/// A big title card that fades out ("FLOOR 2", "WARDEN 1 OF 2", ...).
struct Banner {
    title: String,
    sub: String,
    color: Color,
    t: f32,
    /// difficulty narration under the subtitle: (line, small print, colour)
    note: Option<(String, String, Color)>,
    /// total seconds on screen
    secs: f32,
}

const BANNER_SECS: f32 = 3.6;
/// a banner that also carries a difficulty narration line stays up longer
const BANNER_NOTE_SECS: f32 = 5.5;
/// Lowest pixel the ending panel's text may reach (the source tag sits just below, the panel ends at 636).
const DEBRIEF_BOTTOM: f32 = 608.0;

struct App {
    /// shared so per-frame code can borrow an encounter while mutating `self`
    /// (previously the whole Encounter was cloned every frame)
    loaded: Rc<Loaded>,
    /// the content as loaded from disk; `loaded` is this plus any AI-written encounters swapped in for the run
    orig_loaded: Rc<Loaded>,
    /// set while the current floor's encounter was written by the AI (and passed the rubric)
    gen_note: Option<&'static str>,
    reactor: Reactor,
    audio: Audio,
    hw: Hardware,
    screen: Screen,
    run: Run,
    /// the current floor (monsters, the floor's character, the Keeper, the stairs)
    world: World,
    /// the wardens' arena, only during a BossFight
    arena: Option<World>,
    /// the Floor Keeper's question, while it's being asked / answered
    quiz: Option<Rc<Quiz>>,
    /// the answer picked for `quiz`
    quiz_pick: Option<usize>,
    /// floor 1's Keeper fight, while it's on (None = the plain quiz)
    fight: Option<Fight>,
    /// the Risk It bonus round, on a right answer's verdict screen
    risk: Option<RiskIt>,
    /// verdict screen after a right answer: 0 = SPARE (stairs), 1 = RISK IT
    verdict_sel: usize,
    /// R: reduced motion (the Keeper fight drops its attacks)
    reduced_motion: bool,
    /// master volume 0..1 (sound effects and music)
    master_volume: f32,
    /// the Options overlay (Esc / O on a floor or in the arena)
    settings_open: bool,
    /// which Options row is selected (an index into `OPTS`)
    settings_sel: usize,
    /// what the title screen shows (menu, Options, How to play)
    title_view: TitleView,
    /// the title menu's selected item
    menu_sel: usize,
    /// Options > AI extras (saved; off = scripted lines, no network calls)
    ai_on: bool,
    /// Options > Arduino display (saved; off = the USB port is left alone)
    arduino_on: bool,
    /// live layouts / sketch uploads for the Arduino display (display_live.rs)
    display_live: display_live::DisplayLive,
    /// a setting changed: save once the mouse is let go
    settings_dirty: bool,
    /// per-run seed (from the game's RNG, so CC_SEED makes it repeatable): the quiz layout
    run_seed: u32,
    /// seconds left before a freshly opened choice screen takes an answer
    choice_lock: f32,
    /// the choice screen takes input (lock over and every confirm/fire key released)
    choice_open: bool,
    /// the stairs were taken: build the next floor next frame, behind a black screen
    pending_floor: bool,
    /// the Keeper's verdict line has finished (or was skipped) and the result is showing
    verdict_ready: bool,
    sel: usize,
    line: Option<Line>,
    /// what the last choice actually changed (shown as chips on the lesson screen)
    applied: Option<Effects>,
    banner: Option<Banner>,
    /// monsters + wardens defeated this run (hardware KILLS counter)
    kills: u32,
    room_fade: f32,
    t: f32,
    /// M: everything silent (music and sound effects)
    muted: bool,
    /// N: music on/off (sound effects stay)
    music_on: bool,
    /// music volume 0..1 (`CAMPUS_CRYPT_MUSIC_VOLUME` natively)
    music_volume: f32,
    /// one looping track per screen / floor, loaded lazily (music.rs)
    music: Music,
    auto: Auto,
    hw_timer: f32,
    quit: bool,
    txt: Texts,
    /// the AI extras (scam generator, Keeper banter, debrief, spot-the-fake, owl hints,
    /// difficulty narration); every getter answers at once, scripted when the AI isn't there
    ai_fx: AiFeatures,
    /// Hoot's hint bubble on the encounter screen, once it pops in
    owl: Option<Line>,
    /// seconds since Hoot popped in (pop-in animation)
    owl_t: f32,
    /// last time the mouse moved or fired: while recent, the blaster aims at it (twin-stick)
    mouse_aim_t: f64,
    /// a tip that arrived where it can't be drawn (the lesson screen); it pops in once the floor is back
    tip_wait: Option<tips::Tip>,
    /// Hoot's tip log overlay (L) is open
    tip_log_open: bool,
    /// how many tips the log is scrolled down (0 = newest first)
    tip_log_scroll: usize,
    /// the Floor Keeper's opening taunt on the quiz screen
    taunt: Option<Line>,
    /// each floor's side game, planned at run start
    sides: Vec<Option<Side>>,
    /// the side game open over the paused floor
    mini: Option<Mini>,
    /// side games opened this run (CC_MINI answers them in this order)
    minis_opened: usize,
    /// the skill tier changed on the last descent: narrate it on the next floor's banner
    tier_change: Option<TierChange>,
    /// ending screen: 0 = Hoot's study guide, 1 = field notes
    ending_tab: usize,
    /// CC_AUTO printed this ending's debrief
    debrief_logged: bool,
    /// font size that fits this run's study guide in the ending panel (picked once)
    debrief_size: Option<f32>,
}

impl App {
    fn new(loaded: Loaded, audio: Audio, auto: Auto) -> App {
        let loaded = Rc::new(loaded);
        let saved = settings::load();
        let mut reactor = Reactor::new(loaded.ai_prompt.clone());
        reactor.set_enabled(saved.ai);
        let mut ai_fx = AiFeatures::new();
        ai_fx.set_enabled(saved.ai);
        ai_fx.hints.enabled = saved.hints;
        #[cfg(not(target_arch = "wasm32"))]
        let hw_on = saved.arduino && std::env::var("CAMPUS_CRYPT_HW").map_or(true, |v| v != "off");
        #[cfg(target_arch = "wasm32")]
        let hw_on = false;
        let run = new_run(&loaded);
        App {
            orig_loaded: loaded.clone(),
            gen_note: None,
            loaded,
            reactor,
            audio,
            hw: Hardware::new(hw_on, hardware::port_override()),
            screen: Screen::Title,
            run,
            world: World::floor(Tier::of(0, state::RUN_FLOORS as u32)),
            arena: None,
            quiz: None,
            quiz_pick: None,
            fight: None,
            risk: None,
            verdict_sel: 0,
            reduced_motion: saved.reduced_motion,
            master_volume: saved.master,
            settings_open: false,
            settings_sel: 0,
            title_view: match auto.title_view.as_deref() {
                Some("options") => TitleView::Options,
                Some("howto") => TitleView::HowTo,
                _ => TitleView::Main,
            },
            menu_sel: 0,
            ai_on: saved.ai,
            arduino_on: hw_on,
            display_live: display_live::DisplayLive::new(),
            settings_dirty: false,
            run_seed: 0,
            choice_lock: 0.0,
            choice_open: true,
            pending_floor: false,
            verdict_ready: false,
            sel: 0,
            line: None,
            applied: None,
            banner: None,
            kills: 0,
            room_fade: 1.0,
            t: 0.0,
            muted: false,
            music_on: saved.music_on,
            music_volume: saved.music,
            music: Music::new(),
            auto,
            hw_timer: 0.0,
            quit: false,
            txt: Texts::default(),
            ai_fx,
            owl: None,
            owl_t: 0.0,
            mouse_aim_t: -99.0,
            tip_wait: None,
            tip_log_open: false,
            tip_log_scroll: 0,
            taunt: None,
            sides: Vec::new(),
            mini: None,
            minis_opened: 0,
            tier_change: None,
            ending_tab: 0,
            debrief_logged: false,
            debrief_size: None,
        }
    }

    fn save_settings(&self) {
        settings::save(settings::Settings {
            master: self.master_volume,
            music: self.music_volume,
            reduced_motion: self.reduced_motion,
            music_on: self.music_on,
            hints: self.ai_fx.hints.enabled,
            ai: self.ai_on,
            arduino: self.arduino_on,
        });
    }

    /// Flip one on/off option (Options menu, or its keyboard shortcut).
    fn toggle_opt(&mut self, o: Opt) {
        match o {
            Opt::MusicOn => self.music_on = !self.music_on,
            Opt::Motion => {
                self.reduced_motion = !self.reduced_motion;
                if let Some(f) = &mut self.fight {
                    f.set_reduced(self.reduced_motion);
                }
            }
            Opt::Hints => {
                if !self.ai_fx.hints.toggle() {
                    self.owl = None;
                }
            }
            Opt::Ai => {
                self.ai_on = !self.ai_on;
                self.reactor.set_enabled(self.ai_on);
                self.ai_fx.set_enabled(self.ai_on);
            }
            Opt::Arduino => {
                self.arduino_on = !self.arduino_on;
                self.hw.set_enabled(self.arduino_on);
            }
            Opt::Master | Opt::Music | Opt::Back => return,
        }
        self.settings_dirty = true;
    }

    /// The Options rows at (x, y), `w` wide: volume sliders, then on/off switches,
    /// then (on the title) BACK. Up/Down pick a row, Left/Right adjust or flip,
    /// Enter flips, the mouse drags a bar or clicks a switch.
    /// Returns (mouse over the panel, BACK chosen).
    fn options_panel(&mut self, inp: &Input, x: f32, y: f32, w: f32, with_back: bool) -> (bool, bool) {
        let n = if with_back { OPTS.len() } else { OPTS.len() - 1 };
        let row_h = 44.0;
        let area = Rect::new(x, y, w, row_h * n as f32);
        self.settings_sel = self.settings_sel.min(n - 1);
        if inp.up {
            self.settings_sel = (self.settings_sel + n - 1) % n;
            self.sfx(Sfx::Click);
        }
        if inp.down {
            self.settings_sel = (self.settings_sel + 1) % n;
            self.sfx(Sfx::Click);
        }
        let step = if inp.right { 0.05 } else if inp.left { -0.05 } else { 0.0 };
        let (bx, bw) = (x + 250.0, w - 340.0);
        let mut back = false;
        for (row, &o) in OPTS.iter().take(n).enumerate() {
            let ry = y + row as f32 * row_h;
            let hit = Rect::new(x, ry, w, row_h);
            if inp.mouse_moved && hit.contains(inp.mouse) && !is_mouse_button_down(MouseButton::Left) {
                self.settings_sel = row;
            }
            let on = self.settings_sel == row;
            let col = if on { KSU_GOLD } else { PAPER };
            if on {
                draw_rectangle(x, ry + 2.0, w, row_h - 4.0, Color::new(1.0, 0.77, 0.16, 0.10));
                draw_text(">", x + 6.0, ry + 29.0, 24.0, KSU_GOLD);
            }
            let label = match o {
                Opt::Master => "MASTER VOLUME",
                Opt::Music => "MUSIC VOLUME",
                Opt::MusicOn => "MUSIC  (N)",
                Opt::Motion => "REDUCED MOTION  (R)",
                Opt::Hints => "HOOT'S HINTS  (H)",
                Opt::Ai => "AI EXTRAS",
                Opt::Arduino => "ARDUINO DISPLAY",
                Opt::Back => "BACK",
            };
            draw_text(label, x + 26.0, ry + 29.0, 22.0, col);
            match o {
                Opt::Master | Opt::Music => {
                    let bar = Rect::new(bx, ry + 15.0, bw, 14.0);
                    let grab = Rect::new(bar.x - 6.0, ry, bar.w + 12.0, row_h);
                    let v = if o == Opt::Master { &mut self.master_volume } else { &mut self.music_volume };
                    let mut nv = *v;
                    if on && step != 0.0 {
                        nv = ((nv + step) * 20.0).round() / 20.0;
                    }
                    if is_mouse_button_down(MouseButton::Left) && grab.contains(inp.mouse) {
                        nv = ((inp.mouse.x - bar.x) / bar.w).clamp(0.0, 1.0);
                        self.settings_sel = row;
                    }
                    let nv = nv.clamp(0.0, 1.0);
                    if (nv - *v).abs() > 1e-4 {
                        *v = nv;
                        self.settings_dirty = true;
                    }
                    let v = *v;
                    draw_rectangle(bar.x, bar.y, bar.w, bar.h, Color::new(0.2, 0.17, 0.1, 1.0));
                    draw_rectangle(bar.x, bar.y, bar.w * v, bar.h, col);
                    draw_rectangle_lines(bar.x, bar.y, bar.w, bar.h, 1.0, DIM);
                    draw_rectangle(bar.x + bar.w * v - 3.0, bar.y - 4.0, 6.0, bar.h + 8.0, WHITE);
                    let pct = (v * 100.0).round() as usize;
                    let t = ["0%", "5%", "10%", "15%", "20%", "25%", "30%", "35%", "40%", "45%", "50%", "55%", "60%", "65%", "70%", "75%", "80%", "85%", "90%", "95%", "100%"];
                    draw_text(t.get((pct + 2) / 5).copied().unwrap_or("100%"), bar.x + bar.w + 14.0, ry + 29.0, 22.0, col);
                }
                Opt::Back => {
                    if (on && inp.confirm) || (inp.click && hit.contains(inp.mouse)) {
                        back = true;
                    }
                }
                _ => {
                    if (on && (step != 0.0 || inp.confirm)) || (inp.click && hit.contains(inp.mouse)) {
                        self.toggle_opt(o);
                        self.sfx(Sfx::Click);
                    }
                    let value = match o {
                        Opt::MusicOn => self.music_on,
                        Opt::Motion => self.reduced_motion,
                        Opt::Hints => self.ai_fx.hints.enabled,
                        Opt::Ai => self.ai_on,
                        _ => self.arduino_on,
                    };
                    // the switch: a pill with the knob on the right when ON
                    let (sx, sy) = (bx, ry + 11.0);
                    let fill = if value { Color::new(0.40, 0.86, 0.48, 0.85) } else { Color::new(0.25, 0.22, 0.2, 1.0) };
                    draw_rectangle(sx, sy, 56.0, 22.0, fill);
                    draw_rectangle_lines(sx, sy, 56.0, 22.0, 1.5, col);
                    draw_rectangle(if value { sx + 34.0 } else { sx + 4.0 }, sy + 3.0, 18.0, 16.0, PAPER);
                    draw_text(if value { "ON" } else { "OFF" }, sx + 68.0, ry + 29.0, 22.0, col);
                    // what it means right now
                    let note: std::borrow::Cow<str> = match o {
                        Opt::Ai if !self.ai_fx.has_key() => "no API key: scripted".into(),
                        Opt::Ai if self.ai_on => "live AI lines".into(),
                        Opt::Ai => "scripted lines only".into(),
                        Opt::Arduino if *self.hw.status() == hardware::HwStatus::Disabled => "".into(),
                        Opt::Arduino => self.hw.status().label(),
                        _ => "".into(),
                    };
                    if !note.is_empty() {
                        let c = if o == Opt::Arduino && self.hw.status().connected() { SAFE } else { DIM };
                        draw_text(&note, sx + 120.0, ry + 28.0, 18.0, c);
                    }
                }
            }
        }
        (area.contains(inp.mouse), back)
    }

    fn sfx(&mut self, s: Sfx) {
        let v = if self.muted { 0.0 } else { 0.6 * self.master_volume };
        self.audio.play(s, v);
    }

    /// This floor's place on the difficulty curve, with the skill tier's pacing.
    fn tier(&self) -> Tier {
        Tier::of(self.run.room as u32, self.run.rooms as u32).with_pace(self.pace())
    }

    /// Adaptive pacing from the skill tier (code-decided, `difficulty::next_tier`).
    /// Applied: monster speed and count on floors, warden health in the arena,
    /// capped by `Pace::capped` (wardens never above the tuned curve) so the
    /// slow bot still beats floor 10's wardens about half the time at every tier.
    fn pace(&self) -> Pace {
        let p = self.ai_fx.tier().params();
        Pace::capped(p.monster_speed_pct, p.monster_count_delta, p.warden_hp_pct)
    }

    /// Which floors get a side game this run: scam props use the AI-prefetched
    /// slots, spot-the-fake uses scripted slots with templates the props didn't.
    fn plan_sides(&mut self) {
        self.sides = plan_sides(&self.ai_fx.data, self.run_seed, self.run.rooms);
    }

    fn side(&self) -> Option<Side> {
        self.sides.get(self.run.room).copied().flatten()
    }

    fn enc(&self) -> &Encounter {
        enc_at(&self.loaded, self.run.room)
    }

    /// Swap in this floor's AI-written encounter if one arrived and passed the rubric
    /// (`ai_features::genenc`); otherwise the scripted encounter stays.
    fn apply_generated(&mut self) {
        self.gen_note = None;
        let Some(res) = self.ai_fx.generated_encounter(self.run.room) else { return };
        match res {
            Ok(enc) => {
                let mut encounters = self.loaded.content.encounters.clone();
                let idx = state::encounter_for_floor(self.run.room, encounters.len());
                encounters[idx] = enc;
                self.loaded = Rc::new(Loaded {
                    content: content::Content { encounters },
                    ai_prompt: self.loaded.ai_prompt.clone(),
                    source: self.loaded.source.clone(),
                    warnings: self.loaded.warnings.clone(),
                });
                self.txt = Texts::default();
                self.gen_note = Some("AI-WRITTEN");
            }
            Err(why) => eprintln!("floor {}: scripted encounter (AI-written one skipped: {why})", self.run.room),
        }
    }

    fn start_run(&mut self) {
        // drop last run's AI-written encounters and any text cached for them
        self.loaded = self.orig_loaded.clone();
        self.gen_note = None;
        self.txt = Texts::default();
        self.run = new_run(&self.loaded);
        self.run_seed = macroquad::rand::rand();
        self.pending_floor = false;
        if self.auto.start_floor > 0 {
            self.run.debug_start_floor(self.auto.start_floor);
        }
        self.kills = 0;
        self.txt.notes_key = None;
        // resets the AI extras and prefetches floor 1's banter, the 3 scam bodies and the tier lines
        self.ai_fx.begin_run(&self.loaded.content, self.run_seed, get_time());
        self.tip_wait = None;
        self.tip_log_open = false;
        self.plan_sides();
        self.mini = None;
        self.minis_opened = 0;
        self.tier_change = None;
        self.enter_floor();
        self.screen = Screen::Playing;
        self.sfx(Sfx::Stairs);
    }

    /// Build the current floor and announce it.
    fn enter_floor(&mut self) {
        self.apply_generated();
        self.world = World::floor(self.tier());
        self.world.god = self.auto.god;
        // this floor's Keeper banter + owl hint start downloading now (free if already asked)
        self.ai_fx.begin_floor(&self.loaded.content, self.run.room, get_time());
        self.mini = None;
        self.owl = None;
        self.tip_wait = None;
        self.ai_fx.cancel_tip();
        self.taunt = None;
        match self.side() {
            Some(Side::Scam(_)) => {
                self.world.place_prop(&PHONE, RISKY);
            }
            Some(Side::Spot(_)) => {
                self.world.place_prop(&NOTICE_BOARD, KSU_GOLD);
            }
            None => {}
        }
        self.world.snap_camera();
        self.world.prewarm(); // raster the map now (behind the black frame), not on the first draw
        self.arena = None;
        self.quiz = None;
        self.quiz_pick = None;
        self.fight = None;
        self.risk = None;
        self.settings_open = false;
        self.sel = 0;
        self.line = None;
        self.applied = None;
        self.room_fade = 1.0;
        self.reactor.cancel();
        let (room, rooms) = (self.run.room, self.run.rooms);
        let name = self.enc().character.name.clone();
        let extra = match self.side() {
            Some(Side::Scam(_)) => " A phone is buzzing somewhere.",
            Some(Side::Spot(_)) => " Someone pinned two messages to a board.",
            None => "",
        };
        self.show_banner(format!("FLOOR {} OF {}", room + 1, rooms), format!("Somewhere down here: {name}. The Floor Keeper guards the stairs.{extra}"), KSU_GOLD);
        // the crypt's pace changed on the way down: say so under the floor banner
        if let (Some(ch), Some(b)) = (self.tier_change.take(), self.banner.as_mut()) {
            let col = if ch.to > ch.from { MIXED } else { SAFE };
            let tag = format!("crypt pace: {} -> {}   -   {}", ch.from.id(), ch.to.id(), source_tag(&ch.source, "line"));
            if self.auto.on {
                println!("[auto] skill tier {:?} -> {:?}: {} ({})", ch.from, ch.to, ch.line, tag);
            }
            b.note = Some((ch.line, tag, col));
            b.t = BANNER_NOTE_SECS;
            b.secs = BANNER_NOTE_SECS;
        }
    }

    fn show_banner(&mut self, title: String, sub: String, color: Color) {
        self.banner = Some(Banner { title, sub, color, t: BANNER_SECS, note: None, secs: BANNER_SECS });
    }

    fn frame(&mut self, dt: f32) {
        self.t += dt;
        self.hw.poll(dt);
        self.display_live.update(&mut self.hw, dt);
        let view = View::new();
        let mut inp = Input::read(&view, &self.hw);
        if is_key_pressed(KeyCode::M) {
            self.muted = !self.muted;
        }
        if is_key_pressed(KeyCode::N) {
            self.toggle_opt(Opt::MusicOn);
        }
        if is_key_pressed(KeyCode::H) {
            // Hoot's hints on/off (saved)
            self.toggle_opt(Opt::Hints);
        }
        // collect finished AI-extras calls (cheap: drains a channel, never waits)
        self.ai_fx.update();
        if is_key_pressed(KeyCode::R) {
            self.toggle_opt(Opt::Motion);
        }
        if self.settings_dirty && !is_mouse_button_down(MouseButton::Left) {
            self.settings_dirty = false;
            self.save_settings();
        }
        // title march on the title, the floor's own track on a floor (and its arena);
        // the next one loads in the background. Silent until loaded, or if missing.
        let (track, next) = match self.screen {
            Screen::Title => (Track::Title, Track::for_floor(self.auto.start_floor)),
            Screen::Playing => (Track::for_floor(self.run.room), Track::for_floor(self.run.room + 1)),
        };
        let vol = if self.muted || !self.music_on { 0.0 } else { self.music_volume * self.master_volume };
        // finishing a native load costs ~0.1 s: only on the title or a black build
        // frame (between floors, or entering the warden arena)
        let black = self.screen == Screen::Playing
            && (self.pending_floor || (self.run.phase == Phase::BossFight && self.arena.is_none()));
        let allow_upload = self.screen == Screen::Title || black;
        self.music.update(Some(track), Some(next), vol, dt, allow_upload);
        if self.auto.on {
            self.autopilot(&mut inp, dt);
            self.trace();
        }

        clear_background(BLACK);
        set_camera(&view.cam);
        match self.screen {
            Screen::Title => self.title(inp),
            Screen::Playing => self.playing(inp, dt),
        }
        set_default_camera();
        self.hw_sync(dt);
    }

    // ------------------------------------------------------------ title

    fn title(&mut self, inp: Input) {
        // the candlelit crypt stays up behind every title view
        crypt_backdrop(7, ROOM_TOP - 56.0, VH);
        for (i, x) in [140.0, 420.0, 860.0, 1140.0].iter().enumerate() {
            candle(*x, 90.0, self.t, i as f32 * 1.7);
        }
        vignette(0.0, VH);
        let cx = VW * 0.5;
        let view = self.title_view;
        let (title_y, title_size) = if view == TitleView::Main { (170.0, 112.0) } else { (140.0, 64.0) };
        shadow_centered("CAMPUS CRYPT", cx, title_y, title_size, KSU_GOLD);
        if view == TitleView::Main {
            centered("A dungeon crawler of scams, deadlines, and split-second choices.", cx, 218.0, 28.0, PAPER);
            centered("by Deyanna Gladney & Rythm-Prince Ogburn", cx, 250.0, 20.0, DIM);
            // the student and the Keeper flank the menu
            let bob = (self.t * 2.0).sin() * 4.0;
            glow(cx - 372.0, 420.0, 140.0, Color::new(1.0, 0.6, 0.2, 1.0), 1.2);
            draw_sprite(&STUDENT, cx - 420.0, 372.0 + bob, 6.0, false);
            glow(cx + 372.0, 420.0, 140.0, Color::new(0.7, 0.4, 1.0, 1.0), 1.0);
            draw_sprite(&KEEPER, cx + 324.0, 372.0 - bob, 6.0, false);
        }
        match view {
            TitleView::Main => self.title_menu(&inp),
            TitleView::Options => {
                let (x, y, w) = (cx - 330.0, 196.0, 660.0);
                panel(x - 20.0, y - 26.0, w + 40.0, 44.0 * OPTS.len() as f32 + 70.0, KSU_GOLD);
                let (_, back) = self.options_panel(&inp, x, y, w, true);
                centered("Up/Down pick  -  Left/Right or Enter change  -  Esc: back", cx, y + 44.0 * OPTS.len() as f32 + 28.0, 18.0, DIM);
                if back || inp.back {
                    self.title_view = TitleView::Main;
                    self.sfx(Sfx::Click);
                }
            }
            TitleView::HowTo => self.title_howto(&inp),
        }
        // status lines along the bottom
        centered(self.reactor.status(), cx - 230.0, 668.0, 18.0, DIM);
        centered(self.ai_fx.status(), cx + 230.0, 668.0, 18.0, DIM);
        // only the controller line can allocate (port name), so refresh it a few times a second
        self.txt.status_age -= get_frame_time();
        if self.txt.status_age <= 0.0 || self.txt.controller.key.is_none() {
            self.txt.status_age = 0.25;
            self.txt.controller.key = Some(0);
            let st = self.hw.status();
            self.txt.controller.s.clear();
            if st.connected() {
                let _ = write!(self.txt.controller.s, "Status display: {}", st.label());
            }
        }
        centered("Practice game: it teaches warning signs; it can't spot real scams for you.", cx, 694.0, 18.0, DIM);
        let loaded = &self.loaded;
        let t = self.txt.content.get(0, |s| {
            let _ = write!(s, "content: {}", loaded.source);
            if !loaded.warnings.is_empty() {
                let _ = write!(s, "  ({} content warnings, see console)", loaded.warnings.len());
            }
        });
        draw_text(t, 12.0, 714.0, 16.0, Color::new(1.0, 1.0, 1.0, 0.3));
        if !self.txt.controller.s.is_empty() {
            draw_text(&self.txt.controller.s, 12.0, 24.0, 20.0, SAFE);
        }
    }

    /// The crypt door list: ENTER THE CRYPT / OPTIONS / HOW TO PLAY / QUIT.
    fn title_menu(&mut self, inp: &Input) {
        let items = MENU_ITEMS;
        let n = items.len();
        if inp.up {
            self.menu_sel = (self.menu_sel + n - 1) % n;
            self.sfx(Sfx::Click);
        }
        if inp.down {
            self.menu_sel = (self.menu_sel + 1) % n;
            self.sfx(Sfx::Click);
        }
        let cx = VW * 0.5;
        let mut chosen = None;
        for (i, label) in items.iter().enumerate() {
            let y = 318.0 + i as f32 * 64.0;
            let hit = Rect::new(cx - 230.0, y - 42.0, 460.0, 58.0);
            if inp.mouse_moved && hit.contains(inp.mouse) {
                self.menu_sel = i;
            }
            let on = self.menu_sel == i;
            if on {
                // a lit slab behind the choice, with a flame on each side
                draw_rectangle(hit.x, hit.y, hit.w, hit.h, Color::new(0.10, 0.07, 0.03, 0.75));
                draw_rectangle_lines(hit.x, hit.y, hit.w, hit.h, 2.0, Color::new(1.0, 0.77, 0.16, 0.7));
                let w = measure_text(label, None, 40, 1.0).width;
                for (k, fx) in [cx - w * 0.5 - 34.0, cx + w * 0.5 + 34.0].into_iter().enumerate() {
                    flame(fx, y - 12.0, self.t, k as f32 * 2.3);
                }
                shadow_centered(label, cx, y, 40.0, KSU_GOLD);
            } else {
                shadow_centered(label, cx, y, 36.0, Color::new(0.93, 0.90, 0.82, 0.75));
            }
            if inp.click && hit.contains(inp.mouse) {
                chosen = Some(i);
            }
        }
        if inp.confirm {
            chosen = Some(self.menu_sel);
        }
        centered("Up/Down or mouse to choose  -  Enter to pick", cx, 318.0 + n as f32 * 64.0 - 8.0, 18.0, DIM);
        match chosen.map(|i| items[i]) {
            Some("ENTER THE CRYPT") => self.start_run(),
            Some("OPTIONS") => {
                self.title_view = TitleView::Options;
                self.settings_sel = 0;
                self.sfx(Sfx::Click);
            }
            Some("HOW TO PLAY") => {
                self.title_view = TitleView::HowTo;
                self.sfx(Sfx::Click);
            }
            Some(_) => self.quit = true,
            None => {}
        }
        #[cfg(not(target_arch = "wasm32"))]
        if inp.back {
            self.quit = true;
        }
    }

    fn title_howto(&mut self, inp: &Input) {
        let cx = VW * 0.5;
        panel(cx - 400.0, 170.0, 800.0, 450.0, KSU_GOLD);
        centered("HOW TO PLAY", cx, 214.0, 34.0, KSU_GOLD);
        let rooms = self.run.rooms;
        let lines: [(&str, String); 9] = [
            ("Goal", format!("Go down {rooms} floors. Each hides a trap that looks like real campus life.")),
            ("Move", "WASD or the arrow keys".into()),
            ("Fire", "Hold Space, J or the left mouse button (the mouse aims)".into()),
            ("Roll", "Shift, K or right-click: a quick dodge".into()),
            ("Talk", "E or Enter near the floor's character, then 1-3 to answer".into()),
            ("Keeper", "Answer the Floor Keeper to open the stairs; miss = fight 2 wardens".into()),
            ("Hoot", "T ask  -  L tip log  -  H hints  -  M mute  -  N music".into()),
            ("Pause", "Esc or O: pause and Options".into()),
            ("Arduino", "Optional: a Mega 2560 mirrors the game on an LCD and an 8x8 matrix".into()),
        ];
        for (i, (k, v)) in lines.iter().enumerate() {
            let y = 262.0 + i as f32 * 36.0;
            draw_text(k, cx - 370.0, y, 22.0, KSU_GOLD);
            draw_text(v, cx - 250.0, y, 20.0, PAPER);
        }
        centered("Esc, Enter or click: back", cx, 600.0, 18.0, DIM);
        if inp.back || inp.confirm || inp.click {
            self.title_view = TitleView::Main;
            self.sfx(Sfx::Click);
        }
    }

    // ------------------------------------------------------------ playing

    fn playing(&mut self, inp: Input, dt: f32) {
        if self.pending_floor {
            // the last frame went black; build the new floor now so its hitch is never seen
            self.pending_floor = false;
            self.enter_floor();
        }
        self.room_fade = (self.room_fade - dt * 2.0).max(0.0);
        let inp = self.gate_choices(inp, dt);
        // Hoot's tip log (L). Handled before the settings overlay so Esc closes it instead of opening settings.
        if self.tip_log_open || (is_key_pressed(KeyCode::L) && self.tip_log_ok()) {
            let opening = !self.tip_log_open;
            self.tip_log_open = true;
            if opening {
                self.tip_log_scroll = 0;
                self.sfx(Sfx::Click);
            }
            self.draw_tip_log(&inp);
            if !opening && (inp.back || inp.confirm || is_key_pressed(KeyCode::L)) {
                self.tip_log_open = false;
                self.sfx(Sfx::Click);
            }
            return;
        }
        // Esc / O: the Options overlay (pauses a floor or the arena)
        let can_pause = !self.pending_floor
            && ((self.run.phase == Phase::Explore && self.mini.is_none()) || (self.run.phase == Phase::BossFight && self.arena.is_some()));
        let toggle = inp.back || is_key_pressed(KeyCode::O);
        if self.settings_open || (can_pause && toggle) {
            let opening = !self.settings_open;
            self.settings_open = true;
            self.draw_world();
            self.draw_hud();
            draw_rectangle(0.0, 0.0, VW, VH, Color::new(0.0, 0.0, 0.0, 0.55));
            let rows = OPTS.len() as f32 - 1.0;
            panel(VW * 0.5 - 350.0, 140.0, 700.0, rows * 44.0 + 130.0, KSU_GOLD);
            centered("PAUSED - OPTIONS", VW * 0.5, 186.0, 32.0, KSU_GOLD);
            if opening {
                // the Enter/Esc that opened it must not also flip a row
                self.settings_sel = 0;
                self.sfx(Sfx::Click);
                return;
            }
            self.options_panel(&inp, VW * 0.5 - 330.0, 206.0, 660.0, false);
            centered("Up/Down pick  -  Left/Right or Enter change  -  Esc / O: resume", VW * 0.5, 206.0 + rows * 44.0 + 36.0, 18.0, DIM);
            if toggle || !can_pause {
                self.settings_open = false;
            }
            return;
        }
        // poll the AI while waiting for a character's line (the Keeper's lines are scripted)
        if self.run.phase == Phase::Reaction && self.line.is_none() {
            if let Some((text, source)) = self.reactor.poll(get_time()) {
                self.line = Some(Line::new(text, source));
            }
        }
        for l in [&mut self.line, &mut self.taunt, &mut self.owl].into_iter().flatten() {
            l.shown += TYPE_CPS * dt;
        }

        // Hoot's tips: T asks anytime; a tip is also written after every lesson
        self.hoot_tips(dt);

        // one handler per frame, picked before anything runs, so the key that
        // opens a screen can't also make a choice on it in the same frame
        let phase = self.run.phase;
        let mini = self.mini.is_some();
        match phase {
            // a side game pauses the floor
            Phase::Explore if mini => self.draw_world(),
            Phase::Explore => self.explore(inp, dt),
            Phase::BossFight => self.boss_fight(inp, dt),
            _ => self.draw_world(),
        }
        match phase {
            Phase::Explore if mini => self.mini_screen(inp),
            Phase::Encounter => self.encounter(inp, dt),
            Phase::Reaction | Phase::Lesson => self.reaction(inp),
            Phase::KeeperQuiz => self.keeper_quiz(inp, dt),
            Phase::KeeperVerdict => self.keeper_verdict(inp, dt),
            Phase::Ending => self.ending(inp),
            Phase::Explore | Phase::BossFight => {}
        }
        // Hoot's bubble on the floor (on a scenario, `encounter` draws it) and its thinking line
        let on_floor = phase == Phase::Explore && !mini;
        if on_floor && self.owl.is_some() {
            self.draw_owl(56.0);
        }
        if self.ai_fx.tip_busy() && (on_floor || phase == Phase::Encounter) {
            let dots = 1 + (self.t * 3.0) as usize % 3;
            draw_text(&"Hoot is thinking..."[..16 + dots], 40.0, VH - 58.0, 22.0, DIM);
        }
        if self.run.phase != Phase::Ending {
            self.draw_hud();
            self.draw_banner(dt);
        }
        if self.room_fade > 0.0 {
            draw_rectangle(0.0, 0.0, VW, VH, Color::new(0.0, 0.0, 0.0, self.room_fade));
        }
    }

    /// A choice screen just opened: ignore answers for a moment, and until every
    /// confirm/swing key and the mouse button are let go, so a held swing or a
    /// mashed key can't pick an answer by accident.
    fn lock_choices(&mut self) {
        self.choice_lock = CHOICE_LOCK_SECS;
        self.choice_open = false;
    }

    fn gate_choices(&mut self, mut inp: Input, dt: f32) -> Input {
        let side_game = self.run.phase == Phase::Explore && self.mini.is_some();
        if !(matches!(self.run.phase, Phase::Encounter | Phase::KeeperQuiz | Phase::Ending) || side_game) {
            return inp;
        }
        self.choice_lock = (self.choice_lock - dt).max(0.0);
        if !self.choice_open && self.choice_lock <= 0.0 {
            let held = [KeyCode::Enter, KeyCode::KpEnter, KeyCode::E, KeyCode::Space, KeyCode::J].iter().any(|&k| is_key_down(k))
                || is_mouse_button_down(MouseButton::Left)
                || self.hw.button_held();
            self.choice_open = !held || self.auto.on;
        }
        if !self.choice_open {
            inp.confirm = false;
            inp.click = false;
            inp.digit = None;
        }
        inp
    }

    fn power(&self) -> Power {
        let s = self.run.stats;
        Power { knowledge: s.knowledge, focus: s.focus, can_heal: s.health < MAX_HEALTH }
    }

    /// Mouse aiming stays on for a few seconds after the mouse moves or fires;
    /// keyboard-only and joystick players get aim assist instead (`World::aim_dir`).
    const MOUSE_AIM_SECS: f64 = 3.0;

    fn mouse_aiming(&self) -> bool {
        get_time() - self.mouse_aim_t < Self::MOUSE_AIM_SECS
    }

    fn control(&mut self, inp: &Input, offset: Vec2) -> Control {
        if inp.mouse_moved || inp.mouse_attack {
            self.mouse_aim_t = get_time();
        }
        let aim = inp.aim_world.or_else(|| self.mouse_aiming().then(|| inp.mouse - offset));
        Control { mv: inp.mv, attack: inp.attack, aim, dodge: inp.dodge }
    }

    /// A small crosshair at the mouse while it aims the blaster.
    fn draw_reticle(&self, inp: &Input) {
        if !self.mouse_aiming() || inp.aim_world.is_some() {
            return;
        }
        let m = inp.mouse;
        let c = Color::from_rgba(255, 140, 40, 230);
        draw_circle_lines(m.x, m.y, 10.0, 2.0, c);
        for (dx, dy) in [(1.0, 0.0), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0)] {
            draw_line(m.x + dx * 6.0, m.y + dy * 6.0, m.x + dx * 15.0, m.y + dy * 15.0, 2.0, c);
        }
        draw_circle(m.x, m.y, 2.0, Color::from_rgba(70, 140, 255, 255));
    }

    /// Turn what happened in the world into run changes, sounds and hardware events.
    fn apply_world_events(&mut self, events: Vec<WorldEvent>, sounds: Vec<Sfx>) {
        for s in sounds {
            self.sfx(s);
        }
        for e in events {
            match e {
                WorldEvent::PlayerHit => {
                    self.run.hurt(1);
                    self.hw.send_event(HwEvent::Hit);
                }
                WorldEvent::Healed => {
                    self.run.heal(1);
                }
                WorldEvent::EnemyKilled => self.kills += 1,
                WorldEvent::BossEntered(kind) => {
                    let n = self.arena.as_ref().map_or(1, |a| a.boss_number());
                    let total = self.arena.as_ref().map_or(2, |a| a.bosses_total);
                    self.show_banner(format!("WARDEN {n} OF {total}: {}", kind.name()), kind.tell().to_string(), kind.color());
                }
                WorldEvent::BossDown(kind) => {
                    self.kills += 1;
                    self.run.boss_defeated();
                    self.hw.send_event(HwEvent::LevelUp);
                    if self.run.phase == Phase::BossFight {
                        self.show_banner(format!("{} FALLS", kind.name()), "One warden left. Grab the coffee if you need it.".into(), SAFE);
                    }
                }
            }
            if self.run.phase == Phase::Ending {
                self.on_run_end();
                return;
            }
        }
    }

    fn on_run_end(&mut self) {
        let lose = self.run.ending() == Ending::Overwhelmed;
        self.sfx(if lose { Sfx::Death } else { Sfx::LevelUp });
        if lose {
            self.hw.send_event(HwEvent::Die);
        }
        self.reactor.cancel();
        self.room_fade = 1.0;
        self.mini = None;
        self.ai_fx.hints.stop();
        // the mistake log is fixed now; the study guide is written in the background (<= 8.5 s)
        self.ai_fx.request_debrief(&self.run, &self.loaded.content, get_time());
        self.ending_tab = 0;
        self.debrief_logged = false;
        self.debrief_size = None;
        // a swing held through the last hit can't skip the ending
        self.lock_choices();
    }

    fn draw_world(&mut self) {
        let t = self.t;
        if let Some(a) = &mut self.arena {
            a.draw(t, None, false);
        } else {
            let sprite = character_sprite(&enc_at(&self.loaded, self.run.room).id);
            self.world.draw(t, Some((sprite, self.run.met)), self.run.stairs_open);
        }
        vignette(ROOM_TOP, VH);
    }

    /// Where the current objective is, and what to call it.
    /// 0 = find the floor's character, 1 = answer the Keeper, 2 = take the stairs.
    fn objective(&self) -> (Vec2, usize) {
        if !self.run.met {
            (self.world.npc, 0)
        } else if !self.run.stairs_open {
            (self.world.keeper, 1)
        } else {
            (self.world.stairs, 2)
        }
    }

    // ------------------------------------------------------------ explore (the floor)

    fn explore(&mut self, inp: Input, dt: f32) {
        let offset = self.world.offset();
        let ctl = self.control(&inp, offset);
        let power = self.power();
        self.world.update(dt, ctl, power);
        let events = std::mem::take(&mut self.world.events);
        let sounds = std::mem::take(&mut self.world.sounds);
        self.apply_world_events(events, sounds);
        self.draw_world();
        self.draw_reticle(&inp);
        if self.run.phase != Phase::Explore {
            return; // died to a monster
        }

        let ppos = self.world.player.pos;
        let near_npc = !self.run.met && ppos.distance(self.world.npc) < TALK_RANGE;
        let narrate = !self.run.met && ppos.distance(self.world.npc) < NARRATE_RANGE;
        let near_keeper = !self.run.stairs_open && ppos.distance(self.world.keeper) < TALK_RANGE;
        let near_stairs = self.run.stairs_open && ppos.distance(self.world.stairs) < TILE * 1.2;
        let off = self.world.offset();

        if near_npc {
            let p = self.world.npc + off;
            let bob = (self.t * 4.0).sin() * 3.0;
            panel(p.x - 100.0, p.y - 104.0 + bob, 200.0, 40.0, KSU_GOLD);
            centered("[E] Talk", p.x, p.y - 76.0 + bob, 26.0, KSU_GOLD);
            if inp.interact {
                self.run.begin_encounter();
                self.sel = 0;
                self.lock_choices();
                // Hoot watches for hesitation on this encounter (code decides when it speaks)
                self.owl = None;
                self.ai_fx.hints.start(self.run.room, get_time());
                self.sfx(Sfx::Click);
                return;
            }
        }
        if near_keeper {
            let p = self.world.keeper + off;
            let bob = (self.t * 4.0).sin() * 3.0;
            if self.run.keeper_ready() {
                panel(p.x - 170.0, p.y - 110.0 + bob, 340.0, 40.0, KSU_GOLD);
                centered("[E] Answer the Floor Keeper", p.x, p.y - 82.0 + bob, 26.0, KSU_GOLD);
                if inp.interact && self.run.begin_keeper() {
                    let q = self.build_quiz();
                    // floor 1: the Inbox Mimic fight (the message comes with the quiz);
                    // its lines are scripted, so it skips the AI taunt
                    self.fight = q.mail.is_some().then(|| Fight::new(q.id, self.reduced_motion));
                    let fight = self.fight.is_some();
                    self.quiz = Some(Rc::new(q));
                    if !fight {
                        // the Keeper's opening taunt: AI-voiced if its floor pack arrived, else scripted
                        let (t, src) = self.ai_fx.keeper_taunt(self.run.room);
                        if self.auto.on {
                            println!("[auto] keeper taunt ({}): {t}", source_tag(&src, "line"));
                        }
                        self.taunt = Some(Line::new(format!("\"{t}\""), src));
                    }
                    self.quiz_pick = None;
                    self.txt.chose.key = None;
                    self.sel = 0;
                    self.lock_choices();
                    self.sfx(Sfx::Click);
                    return;
                }
            } else if !self.run.met {
                panel(p.x - 290.0, p.y - 110.0 + bob, 580.0, 40.0, DIM);
                let name = &enc_at(&self.loaded, self.run.room).character.name;
                let t = self.txt.keeper_hint.get(key(&[self.run.room]), |b| {
                    let _ = write!(b, "\"Learn this floor's trap first. Find {name}.\"");
                });
                centered(t, p.x, p.y - 82.0 + bob, 22.0, PAPER);
            }
        }
        let near_prop = self.world.prop.is_some_and(|pr| !pr.done && ppos.distance(pr.pos) < TALK_RANGE);
        if near_prop && !near_npc {
            if let Some(pr) = self.world.prop {
                let p = pr.pos + off;
                let bob = (self.t * 4.0).sin() * 3.0;
                let (label, col) = match self.side() {
                    Some(Side::Spot(_)) => ("[E] Compare the two messages", KSU_GOLD),
                    _ => ("[E] Check the buzzing phone", RISKY),
                };
                panel(p.x - 190.0, p.y - 96.0 + bob, 380.0, 40.0, col);
                centered(label, p.x, p.y - 68.0 + bob, 26.0, col);
                if inp.interact {
                    self.open_mini();
                    return;
                }
            }
        }
        if near_stairs {
            let p = self.world.stairs + off;
            let label = if self.run.is_last_floor() { "[E] Climb out of the crypt" } else { "[E] Take the stairs down" };
            panel(p.x - 170.0, p.y - 84.0, 340.0, 40.0, SAFE);
            centered(label, p.x, p.y - 56.0, 26.0, SAFE);
            if inp.interact {
                self.run.descend();
                if self.run.phase == Phase::Ending {
                    self.on_run_end();
                } else {
                    // code recomputes the skill tier; a change is narrated on the next banner
                    self.tier_change = self.ai_fx.on_floor_cleared(&self.run, self.run.room);
                    self.sfx(Sfx::Stairs);
                    self.hw.send_event(HwEvent::Stairs);
                    self.pending_floor = true;
                    self.room_fade = 1.0;
                }
                return;
            }
        }

        if narrate {
            // the floor's narration, as the old room intro did
            let loaded = self.loaded.clone();
            let e = enc_at(&loaded, self.run.room);
            let (room, rooms) = (self.run.room, self.run.rooms);
            panel(40.0, 560.0, VW - 80.0, 140.0, KSU_GOLD);
            let t = self.txt.room_line.get(key(&[room, rooms]), |s| {
                let _ = write!(s, "Floor {} of {}: {}", room + 1, rooms, e.name);
            });
            draw_text(t, 64.0, 596.0, 30.0, KSU_GOLD);
            text_block(&e.room_intro, 64.0, 630.0, 26.0, VW - 130.0, PAPER);
            draw_text("walk up to them and press E", VW - 340.0, 690.0, 20.0, DIM);
        } else {
            self.draw_objective();
        }
    }

    /// Bottom-left objective strip + an arrow around the student pointing at it.
    fn draw_objective(&mut self) {
        let (target, which) = self.objective();
        let loaded = self.loaded.clone();
        let e = enc_at(&loaded, self.run.room);
        let last = self.run.is_last_floor();
        let k = key(&[self.run.room, which, last as usize]);
        let t = self.txt.objective.get(k, |b| match which {
            0 => {
                let _ = write!(b, "Find {} somewhere on this floor", e.character.name);
            }
            1 => b.push_str("Answer the Floor Keeper at the stairs"),
            _ if last => b.push_str("The way out is open: take the stairs"),
            _ => b.push_str("The stairs are open: go down"),
        });
        let w = measure_text(t, None, 22, 1.0).width;
        draw_rectangle(20.0, VH - 50.0, w + 56.0, 34.0, PANEL);
        draw_rectangle_lines(20.0, VH - 50.0, w + 56.0, 34.0, 2.0, KSU_GOLD);
        draw_text(">", 32.0, VH - 26.0, 24.0, KSU_GOLD);
        draw_text(t, 56.0, VH - 26.0, 22.0, PAPER);
        let on = self.ai_fx.hints.enabled;
        let help = self.txt.help.get(key(&[on as usize]), |b| {
            let _ = write!(b, "Click/Space: fire   Shift: roll   E: talk   T: ask Hoot   L: tips   H: hints {}", if on { "on" } else { "off" });
        });
        let hw = measure_text(help, None, 20, 1.0).width;
        draw_text(help, VW - 24.0 - hw, VH - 26.0, 20.0, DIM);
        // compass arrow
        let off = self.world.offset();
        let p = self.world.player.pos + off;
        let to = target - self.world.player.pos;
        if to.length() > 4.0 * TILE {
            let d = to.normalize();
            let tip = p + d * 58.0;
            let base = p + d * 42.0;
            let side = vec2(-d.y, d.x) * 9.0;
            let a = 0.55 + 0.35 * (self.t * 4.0).sin();
            draw_triangle(tip, base + side, base - side, Color::new(1.0, 0.77, 0.16, a));
        }
    }

    // ------------------------------------------------------------ side games (AI extras)

    /// Walked up to this floor's prop: snapshot its message(s) (never waits on the
    /// network: a slot that hasn't arrived is scripted) and pause the floor.
    fn open_mini(&mut self) {
        let Some(side) = self.side() else { return };
        let (kind, source) = match side {
            Side::Scam(slot) => {
                let m = self.ai_fx.scam_message(slot);
                let data = &self.ai_fx.data;
                let tips = m.flags.iter().map(|&f| ai_features::debrief::flag_tip(data, f)).collect();
                let src = m.source.clone();
                (MiniKind::Scam(m, tips), src)
            }
            Side::Spot(slot) => {
                let r = self.ai_fx.spot_round(slot);
                let src = r.fake_source.clone();
                (MiniKind::Spot(r), src)
            }
        };
        let tag = source_tag(&source, "message");
        if self.auto.on {
            println!("[auto] side game {side:?} on floor {} ({tag})", self.run.room + 1);
        }
        self.mini = Some(Mini { kind, verdict: None, tag });
        self.minis_opened += 1;
        self.banner = None; // the floor banner would cover the cards
        self.sel = 0;
        self.lock_choices();
        self.sfx(Sfx::Click);
    }

    fn close_mini(&mut self, answered: bool) {
        self.mini = None;
        if answered {
            if let Some(p) = &mut self.world.prop {
                p.done = true;
            }
        }
        self.world.player.invuln = 1.0;
        self.sfx(Sfx::Click);
    }

    fn mini_screen(&mut self, inp: Input) {
        draw_rectangle(0.0, 0.0, VW, VH, Color::new(0.0, 0.0, 0.0, 0.66));
        let Some(m) = self.mini.take() else { return };
        let keep = match &m.kind {
            MiniKind::Scam(msg, tips) => self.scam_screen(&inp, msg, tips, &m),
            MiniKind::Spot(r) => self.spot_screen(&inp, r, &m),
        };
        match keep {
            MiniStep::Stay => self.mini = Some(m),
            MiniStep::Answer(pick) => {
                let right = match &m.kind {
                    // generated messages are always scams: "report" (0) is right
                    MiniKind::Scam(msg, _) => self.ai_fx.record_scam_verdict(self.run.room, msg, pick == 0),
                    // code's label decides; logged for the debrief
                    MiniKind::Spot(r) => self.ai_fx.answer_spot(self.run.room, r, pick),
                };
                if self.auto.on {
                    println!("[auto] side game answer {pick}: {}", if right { "right" } else { "wrong" });
                }
                self.sfx(if right { Sfx::Coin } else { Sfx::Hurt });
                self.hw.send_event(if right { HwEvent::LevelUp } else { HwEvent::Hit });
                self.mini = Some(Mini { verdict: Some((pick, right)), ..m });
            }
            MiniStep::Close(answered) => self.close_mini(answered),
        }
    }

    /// Arrow/digit/click/Enter selection between `n` buttons. Space never answers.
    fn pick_between(&mut self, inp: &Input, rects: &[Rect]) -> Option<usize> {
        let n = rects.len();
        if inp.left || inp.up {
            self.sel = (self.sel + n - 1) % n;
            self.sfx(Sfx::Click);
        }
        if inp.right || inp.down {
            self.sel = (self.sel + 1) % n;
            self.sfx(Sfx::Click);
        }
        let mut picked = None;
        for (i, r) in rects.iter().enumerate() {
            if r.contains(inp.mouse) {
                if inp.mouse_moved {
                    self.sel = i;
                }
                if inp.click {
                    picked = Some(i);
                }
            }
        }
        if let Some(d) = inp.digit.filter(|&d| d < n) {
            picked = Some(d);
        }
        if inp.confirm {
            picked = Some(self.sel.min(n - 1));
        }
        picked
    }

    /// "Back to the floor [Enter]" under a revealed side game.
    fn mini_continue(&self, inp: &Input, col: Color) -> bool {
        let r = Rect::new(VW * 0.5 - 200.0, 656.0, 400.0, 48.0);
        let hover = r.contains(inp.mouse);
        panel(r.x, r.y, r.w, r.h, if hover { WHITE } else { col });
        centered("Back to the floor  [Enter]", VW * 0.5, 688.0, 26.0, col);
        inp.confirm || inp.back || (inp.click && hover)
    }

    fn scam_screen(&mut self, inp: &Input, msg: &ScamMessage, tips: &[String], m: &Mini) -> MiniStep {
        let title = match msg.channel.as_str() {
            "text" => "YOUR PHONE BUZZES: A NEW TEXT",
            "dm" => "YOUR PHONE BUZZES: A NEW DM",
            _ => "YOUR PHONE BUZZES: A NEW EMAIL",
        };
        shadow_centered(title, VW * 0.5, 104.0, 34.0, KSU_GOLD);
        let card = Msg::of_scam(msg);
        let (x, w) = (170.0, VW - 340.0);
        let border = match m.verdict {
            None => KSU_GOLD,
            Some(_) => RISKY,
        };
        let bottom = message_card(&card, x, 124.0, w, 24.0, border, if m.verdict.is_some() { Some("SCAM") } else { None });
        small_print(&m.tag, x + w, bottom + 4.0);
        match m.verdict {
            None => {
                let y = (bottom + 40.0).min(560.0);
                let rects = [Rect::new(250.0, y, 370.0, 70.0), Rect::new(660.0, y, 370.0, 70.0)];
                for (i, (r, label)) in rects.iter().zip(["Report it: it's a scam", "Trust it: looks fine"]).enumerate() {
                    let on = self.sel == i;
                    let lift = if on { -4.0 } else { 0.0 };
                    panel(r.x, r.y + lift, r.w, r.h, if on { KSU_GOLD } else { DIM });
                    draw_text(["1", "2"][i], r.x + 20.0, r.y + 46.0 + lift, 34.0, if on { KSU_GOLD } else { DIM });
                    draw_text(label, r.x + 62.0, r.y + 44.0 + lift, 26.0, if on { WHITE } else { PAPER });
                }
                let hint = if self.choice_open { "Read it like it landed on your real phone.   1-2, arrows + Enter, or click   -   Esc: leave it" } else { "Read carefully..." };
                centered(hint, VW * 0.5, 710.0, 20.0, DIM);
                if inp.back {
                    return MiniStep::Close(false);
                }
                match self.pick_between(inp, &rects) {
                    Some(i) => MiniStep::Answer(i),
                    None => MiniStep::Stay,
                }
            }
            Some((pick, right)) => {
                let y = (bottom + 34.0).min(470.0);
                let (badge, sub, col) = if right {
                    ("GOOD CALL", "You reported it. Every one of these was planted by the crypt.", SAFE)
                } else {
                    ("THAT WAS A SCAM", "You trusted it. Here's what gave it away.", RISKY)
                };
                let _ = pick;
                let bw = measure_text(badge, None, 28, 1.0).width + 30.0;
                draw_rectangle(x, y - 28.0, bw, 40.0, col);
                draw_text(badge, x + 15.0, y, 28.0, INK);
                draw_text(sub, x + bw + 16.0, y, 24.0, PAPER);
                // the red flags code planted, with a tip each
                let mut cx = x;
                let cy = y + 26.0;
                for l in &msg.flag_labels {
                    let cw = measure_text(l, None, 22, 1.0).width + 24.0;
                    draw_rectangle(cx, cy, cw, 32.0, Color::new(RISKY.r * 0.3, RISKY.g * 0.3, RISKY.b * 0.3, 1.0));
                    draw_rectangle_lines(cx, cy, cw, 32.0, 2.0, RISKY);
                    draw_text(l, cx + 12.0, cy + 23.0, 22.0, RISKY);
                    cx += cw + 10.0;
                }
                let mut ty = cy + 62.0;
                for (l, tip) in msg.flag_labels.iter().zip(tips) {
                    if tip.is_empty() || ty > 640.0 {
                        continue;
                    }
                    draw_rectangle(x + 2.0, ty - 13.0, 8.0, 8.0, RISKY);
                    let t = format!("{l}: {tip}");
                    ty = text_block(&t, x + 22.0, ty, 22.0, w - 30.0, PAPER) + 4.0;
                }
                if self.mini_continue(inp, col) {
                    MiniStep::Close(true)
                } else {
                    MiniStep::Stay
                }
            }
        }
    }

    fn spot_screen(&mut self, inp: &Input, r: &SpotRound, m: &Mini) -> MiniStep {
        shadow_centered("SPOT THE FAKE: ONE OF THESE IS A SCAM", VW * 0.5, 100.0, 34.0, KSU_GOLD);
        let cards = [Msg::of_spot(r, 0), Msg::of_spot(r, 1)];
        let (w, xs) = (585.0, [40.0, 655.0]);
        let size = 21.0;
        let top = 120.0;
        let mut bottoms = [0.0f32; 2];
        for i in 0..2 {
            let (border, stamp) = match m.verdict {
                None if self.sel == i => (KSU_GOLD, None),
                None => (DIM, None),
                Some(_) if r.is_fake(i) => (RISKY, Some("FAKE")),
                Some(_) => (SAFE, Some("REAL")),
            };
            bottoms[i] = message_card(&cards[i], xs[i], top, w, size, border, stamp);
            if m.verdict.is_none() {
                // the card's number, right side of its header strip
                let label = ["CARD 1", "CARD 2"][i];
                let lw = measure_text(label, None, 20, 1.0).width;
                draw_text(label, xs[i] + w - lw - 16.0, top + 25.0, 20.0, if self.sel == i { KSU_GOLD } else { DIM });
            }
        }
        let low = bottoms[0].max(bottoms[1]);
        match m.verdict {
            None => {
                let rects = [Rect::new(xs[0], top, w, low - top), Rect::new(xs[1], top, w, low - top)];
                let hint = if self.choice_open { "Which one is the fake?   1-2, arrows + Enter, or click a card   -   Esc: leave it" } else { "Read both carefully..." };
                centered(hint, VW * 0.5, 710.0, 22.0, DIM);
                if inp.back {
                    return MiniStep::Close(false);
                }
                match self.pick_between(inp, &rects) {
                    Some(i) => MiniStep::Answer(i),
                    None => MiniStep::Stay,
                }
            }
            Some((_, right)) => {
                let y = low + 30.0;
                let f = r.fake_index();
                // under the fake: the planted red flags; under the real one: why it's safe
                let mut cy = y;
                let mut cx = xs[f];
                for l in &r.fake_flag_labels {
                    let cw = measure_text(l, None, 20, 1.0).width + 20.0;
                    if cx + cw > xs[f] + w {
                        cx = xs[f];
                        cy += 36.0;
                    }
                    draw_rectangle(cx, cy - 22.0, cw, 30.0, Color::new(RISKY.r * 0.3, RISKY.g * 0.3, RISKY.b * 0.3, 1.0));
                    draw_rectangle_lines(cx, cy - 22.0, cw, 30.0, 2.0, RISKY);
                    draw_text(l, cx + 10.0, cy, 20.0, RISKY);
                    cx += cw + 8.0;
                }
                small_print(&m.tag, xs[f] + w, cy + 12.0);
                let s = 1 - f;
                let mut ty = y;
                for t in &r.safe_tells {
                    if ty > 640.0 {
                        break;
                    }
                    draw_rectangle(xs[s] + 2.0, ty - 13.0, 8.0, 8.0, SAFE);
                    ty = text_block(t, xs[s] + 20.0, ty, 20.0, w - 30.0, PAPER) + 2.0;
                }
                let (badge, col) = if right { ("You caught the fake.", SAFE) } else { ("The fake slipped past you this time.", RISKY) };
                centered(badge, VW * 0.5, 644.0, 24.0, col);
                if self.mini_continue(inp, col) {
                    MiniStep::Close(true)
                } else {
                    MiniStep::Stay
                }
            }
        }
    }

    // ------------------------------------------------------------ the wardens

    fn boss_fight(&mut self, inp: Input, dt: f32) {
        if self.arena.is_none() {
            // built a frame after the verdict, while the screen is black
            let mut a = World::arena(self.tier(), &wardens_for(self.run.room));
            a.god = self.auto.god;
            a.prewarm();
            self.arena = Some(a);
            self.room_fade = 1.0;
        }
        let power = self.power();
        if let Some(offset) = self.arena.as_ref().map(|a| a.offset()) {
            let ctl = self.control(&inp, offset);
            if let Some(a) = self.arena.as_mut() {
                a.update(dt, ctl, power);
            }
        }
        let (events, sounds) = match self.arena.as_mut() {
            Some(a) => (std::mem::take(&mut a.events), std::mem::take(&mut a.sounds)),
            None => (Vec::new(), Vec::new()),
        };
        self.apply_world_events(events, sounds);
        if self.run.phase == Phase::Explore {
            // both wardens down: back to the floor, in front of the open stairs
            self.arena = None;
            self.world.player.pos = self.world.keeper_aside;
            self.world.player.invuln = 1.0;
            self.world.snap_camera();
            self.room_fade = 0.8;
            self.show_banner("THE STAIRS ARE OPEN".into(), "Both wardens beaten. The Keeper steps aside.".into(), SAFE);
            self.sfx(Sfx::LevelUp);
            self.draw_world();
            return;
        }
        self.draw_world();
        if self.run.phase != Phase::BossFight {
            return;
        }
        // warden health bar
        if let Some(a) = &self.arena {
            if let Some(b) = &a.boss {
                let (n, total) = (a.boss_number(), a.bosses_total);
                let name = b.kind.name();
                let t = self.txt.boss_bar.get(key(&[n, total, b.kind as usize]), |s| {
                    let _ = write!(s, "WARDEN {n}/{total}  {name}");
                });
                let (x, y, w) = (240.0, 76.0, VW - 480.0);
                draw_rectangle(x - 4.0, y - 4.0, w + 8.0, 30.0, PANEL);
                let f = (b.hp / b.max_hp).clamp(0.0, 1.0);
                draw_rectangle(x, y, w, 22.0, Color::from_rgba(50, 20, 26, 255));
                draw_rectangle(x, y, w * f, 22.0, if b.enraged() { RISKY } else { b.kind.color() });
                draw_rectangle_lines(x - 4.0, y - 4.0, w + 8.0, 30.0, 2.0, KSU_GOLD);
                centered(t, VW * 0.5, y + 46.0, 24.0, PAPER);
            }
        }
        draw_text("Beat both wardens to open the stairs.   Click/Space: fire   Shift: roll", 20.0, VH - 22.0, 20.0, DIM);
        self.draw_reticle(&inp);
    }

    fn draw_banner(&mut self, dt: f32) {
        let Some(b) = &mut self.banner else { return };
        b.t -= dt;
        if b.t <= 0.0 {
            self.banner = None;
            return;
        }
        let a = (b.t / 0.6).min(1.0).min((b.secs - b.t) / 0.25 + 0.2).clamp(0.0, 1.0);
        let y = if self.run.phase == Phase::BossFight { 150.0 } else { 96.0 };
        let mut sub_w = measure_text(&b.sub, None, 22, 1.0).width.max(measure_text(&b.title, None, 48, 1.0).width);
        if let Some((line, _, _)) = &b.note {
            sub_w = sub_w.max(measure_text(line, None, 24, 1.0).width);
        }
        let w = (sub_w + 80.0).min(VW - 40.0);
        let h = if b.note.is_some() { 150.0 } else { 96.0 };
        draw_rectangle(VW * 0.5 - w * 0.5, y, w, h, Color::new(0.06, 0.05, 0.08, 0.85 * a));
        draw_rectangle_lines(VW * 0.5 - w * 0.5, y, w, h, 2.0, Color::new(b.color.r, b.color.g, b.color.b, a));
        centered(&b.title, VW * 0.5 + 2.0, y + 50.0, 48.0, Color::new(0.0, 0.0, 0.0, a));
        centered(&b.title, VW * 0.5, y + 48.0, 48.0, Color::new(b.color.r, b.color.g, b.color.b, a));
        centered(&b.sub, VW * 0.5, y + 82.0, 22.0, Color::new(PAPER.r, PAPER.g, PAPER.b, a));
        // difficulty narration: the crypt's pace changed (code decided; the line is AI or scripted)
        if let Some((line, tag, col)) = &b.note {
            draw_line(VW * 0.5 - w * 0.5 + 30.0, y + 96.0, VW * 0.5 + w * 0.5 - 30.0, y + 96.0, 1.0, Color::new(col.r, col.g, col.b, 0.4 * a));
            centered(line, VW * 0.5, y + 122.0, 24.0, Color::new(col.r, col.g, col.b, a));
            centered(tag, VW * 0.5, y + 142.0, 16.0, Color::new(DIM.r, DIM.g, DIM.b, a));
        }
    }

    fn draw_hud(&mut self) {
        draw_rectangle(0.0, 0.0, VW, HUD_H, Color::from_rgba(14, 10, 18, 255));
        draw_line(0.0, HUD_H, VW, HUD_H, 2.0, KSU_GOLD);
        let s = self.run.stats;
        draw_text("HEALTH", 20.0, 35.0, 22.0, DIM);
        for i in 0..MAX_HEALTH {
            heart(102.0 + i as f32 * 34.0, 17.0, 4.0, i < s.health);
        }
        draw_text("FOCUS", 370.0, 35.0, 22.0, DIM);
        for i in 0..MAX_FOCUS {
            let col = if i < s.focus { FOCUS } else { Color::from_rgba(40, 50, 70, 255) };
            draw_rectangle(370.0 + 72.0 + i as f32 * 30.0, 18.0, 24.0, 22.0, col);
        }
        let kn = s.knowledge;
        let t = self.txt.knowledge.get(kn as i64 as u64, |b| {
            let _ = write!(b, "KNOWLEDGE {kn}");
        });
        draw_text(t, 640.0, 35.0, 24.0, KSU_GOLD);
        let kills = self.kills as usize;
        let t = self.txt.kills.get(key(&[kills]), |b| {
            let _ = write!(b, "DEFEATED {kills}");
        });
        draw_text(t, 850.0, 35.0, 20.0, DIM);
        let (room, rooms) = (self.run.room, self.run.rooms);
        let t = self.txt.room_hud.get(key(&[room, rooms]), |b| {
            let _ = write!(b, "Floor {}/{}", room + 1, rooms);
        });
        draw_text(t, VW - 150.0, 35.0, 24.0, PAPER);
        if self.muted {
            draw_text("muted", VW - 240.0, 35.0, 20.0, DIM);
        } else if !self.music_on {
            draw_text("music off", VW - 270.0, 35.0, 20.0, DIM);
        }
        if !self.ai_fx.hints.enabled {
            draw_text("hints off", VW - 370.0, 35.0, 20.0, DIM);
        }
    }
    // ------------------------------------------------------------ encounter

    fn encounter(&mut self, inp: Input, dt: f32) {
        // Hoot: any cursor move or key restarts the hesitation clock; code decides when it speaks
        let now = get_time();
        if inp.up || inp.down || inp.left || inp.right || inp.mouse_moved || inp.digit.is_some() || inp.click {
            self.ai_fx.hints.input(now);
        }
        if self.owl.is_none() {
            if let Some((hint, src)) = self.ai_fx.poll_hint(now) {
                if self.auto.on {
                    println!("[auto] owl hint ({}): {hint}", source_tag(&src, "line"));
                }
                self.owl = Some(Line::new(hint, src));
                self.owl_t = 0.0;
                self.sfx(Sfx::Equip);
            }
        }
        self.owl_t += dt;
        draw_rectangle(0.0, 0.0, VW, VH, Color::new(0.0, 0.0, 0.0, 0.55));
        let loaded = self.loaded.clone();
        let e = enc_at(&loaded, self.run.room);
        // portrait
        panel(40.0, 76.0, 320.0, 400.0, KSU_GOLD);
        glow(200.0, 250.0, 140.0, Color::new(0.6, 0.4, 1.0, 1.0), 1.2);
        let bob = (self.t * 2.2).sin() * 4.0;
        draw_sprite(character_sprite(&e.id), 72.0, 110.0 + bob, 16.0, false);
        let nm = wrap(&e.character.name, 28.0, 290.0);
        for (i, l) in nm.iter().enumerate() {
            centered(l, 200.0, 412.0 + i as f32 * 30.0, 28.0, KSU_GOLD);
        }
        // scenario card
        panel(390.0, 76.0, 850.0, 400.0, KSU_GOLD);
        let t = self.txt.enc_name.get(key(&[self.run.room]), |b| b.push_str(&e.name.to_uppercase()));
        draw_text(t, 420.0, 120.0, 32.0, KSU_GOLD);
        draw_line(420.0, 134.0, 1210.0, 134.0, 1.0, Color::new(1.0, 0.77, 0.16, 0.4));
        text_block(&e.scenario, 420.0, 176.0, 30.0, 790.0, PAPER);
        if let Some(tag) = self.gen_note {
            draw_text(tag, 1100.0, 108.0, 18.0, DIM);
        }

        // choices
        let n = e.choices.len().min(4);
        let gap = 20.0;
        let bw = (VW - 80.0 - gap * (n as f32 - 1.0)) / n as f32;
        let by = 500.0;
        let bh = 110.0;
        if inp.left || inp.up {
            self.sel = (self.sel + n - 1) % n;
            self.sfx(Sfx::Click);
        }
        if inp.right || inp.down {
            self.sel = (self.sel + 1) % n;
            self.sfx(Sfx::Click);
        }
        let mut picked = None;
        for (i, ch) in e.choices.iter().take(n).enumerate() {
            let r = Rect::new(40.0 + i as f32 * (bw + gap), by, bw, bh);
            if r.contains(inp.mouse) {
                if inp.mouse_moved {
                    self.sel = i; // hover highlights
                }
                if inp.click {
                    picked = Some(i);
                }
            }
            let on = self.sel == i;
            let lift = if on { -6.0 } else { 0.0 };
            panel(r.x, r.y + lift, r.w, r.h, if on { KSU_GOLD } else { DIM });
            draw_text(["1", "2", "3", "4"][i], r.x + 18.0, r.y + 44.0 + lift, 34.0, if on { KSU_GOLD } else { DIM });
            let lines = wrap(&ch.label, 32.0, r.w - 80.0);
            let total = lines.len() as f32 * 34.0;
            for (j, l) in lines.iter().enumerate() {
                draw_text(l, r.x + 60.0, r.y + bh * 0.5 - total * 0.5 + 26.0 + j as f32 * 34.0 + lift, 32.0, if on { WHITE } else { PAPER });
            }
        }
        self.draw_owl(0.0);
        let on = self.ai_fx.hints.enabled;
        let t = self.txt.enc_help.get(key(&[on as usize]), |b| {
            let _ = write!(b, "1-3, or arrows + Enter, or click a choice   -   T: ask Hoot   -   H: owl hints {}", if on { "on" } else { "off" });
        });
        centered(t, VW * 0.5, 710.0, 22.0, DIM);
        if let Some(d) = inp.digit {
            if d < n {
                picked = Some(d);
            }
        }
        if inp.confirm {
            picked = Some(self.sel);
        }
        if let Some(i) = picked {
            self.pick(i);
        }
    }

    // ------------------------------------------------------------ Hoot's tips

    /// The facts a tip may use: this floor's encounter (and, after a choice, the choice).
    fn tip_context(&self, kind: tips::TipKind, stage: tips::TipStage) -> tips::TipContext {
        tips::TipContext { kind, stage, floor: self.run.room, enc: enc_at(&self.loaded, self.run.room).clone(), picked: None }
    }

    /// Hoot's tips, once per frame: pop a waiting tip back in on the floor, collect a finished
    /// one, and answer the T key (on the floor or while reading a scenario).
    fn hoot_tips(&mut self, dt: f32) {
        let now = get_time();
        let phase = self.run.phase;
        let on_floor = phase == Phase::Explore && self.mini.is_none();
        let reading = phase == Phase::Encounter;
        if on_floor && self.owl.is_none() {
            if let Some(t) = self.tip_wait.take() {
                self.show_tip(t);
            }
        }
        if let Some(t) = self.ai_fx.poll_tip(now) {
            if t.kind == tips::TipKind::Ask && (on_floor || reading) {
                self.show_tip(t);
            } else {
                // a post-floor tip, or one that finished somewhere it can't be drawn
                self.tip_wait = Some(t);
            }
        }
        if is_key_pressed(KeyCode::T) && (on_floor || reading) && !self.pending_floor {
            if !self.ai_fx.hints.enabled {
                self.show_banner("HOOT IS NAPPING".into(), "Press H to wake Hoot up.".into(), DIM);
            } else {
                let stage = if reading { tips::TipStage::Deciding } else { tips::TipStage::Exploring };
                let ctx = self.tip_context(tips::TipKind::Ask, stage);
                if self.ai_fx.ask_tip(ctx, now) {
                    self.sfx(Sfx::Click);
                }
            }
        }
        // on the floor a bubble fades on its own; on a scenario it stays until a choice is made
        if on_floor && self.owl.is_some() {
            self.owl_t += dt;
            if self.owl_t > OWL_FLOOR_SECS {
                self.owl = None;
            }
        }
    }

    /// Hoot speaks a tip: the bubble pops in, and the board's matrix shows the owl.
    fn show_tip(&mut self, t: tips::Tip) {
        if self.auto.on {
            println!("[auto] hoot tip ({}): {}", source_tag(&t.source, "line"), t.text);
        }
        self.owl = Some(Line::new(t.text, t.source));
        self.owl_t = 0.0;
        self.sfx(Sfx::Equip);
        self.hw.send_event(HwEvent::Tip);
    }

    /// The tip log opens on the floor, on a scenario and on a lesson.
    fn tip_log_ok(&self) -> bool {
        !self.pending_floor
            && !self.settings_open
            && self.mini.is_none()
            && matches!(self.run.phase, Phase::Explore | Phase::Encounter | Phase::Lesson)
    }

    /// Hoot's tip log: this run's tips, newest first, Up/Down to scroll.
    fn draw_tip_log(&mut self, inp: &Input) {
        const PER_PAGE: usize = 4;
        let total = self.ai_fx.tip_log().len();
        if inp.up {
            self.tip_log_scroll = self.tip_log_scroll.saturating_sub(1);
        }
        if inp.down {
            self.tip_log_scroll += 1;
        }
        self.tip_log_scroll = self.tip_log_scroll.min(total.saturating_sub(PER_PAGE));
        self.draw_world();
        self.draw_hud();
        draw_rectangle(0.0, 0.0, VW, VH, Color::new(0.0, 0.0, 0.0, 0.72));
        panel(120.0, 80.0, VW - 240.0, 560.0, KSU_GOLD);
        centered("HOOT'S TIP LOG", VW * 0.5, 130.0, 34.0, KSU_GOLD);
        let log = self.ai_fx.tip_log();
        if log.is_empty() {
            centered("No tips yet.", VW * 0.5, 300.0, 28.0, PAPER);
            centered("Press T on a floor or while reading a scenario to ask Hoot.", VW * 0.5, 340.0, 22.0, DIM);
        }
        for (i, tip) in log.iter().rev().skip(self.tip_log_scroll).take(PER_PAGE).enumerate() {
            let y0 = 160.0 + i as f32 * 104.0;
            let head = format!("FLOOR {}  -  {}", tip.floor + 1, tip.kind.tag());
            draw_text(&head, 150.0, y0 + 22.0, 22.0, KSU_GOLD);
            let tag = source_tag(&tip.source, "tip");
            let tw = measure_text(&tag, None, 18, 1.0).width;
            draw_text(&tag, VW - 150.0 - tw, y0 + 22.0, 18.0, DIM);
            text_block(&tip.text, 150.0, y0 + 52.0, 24.0, VW - 300.0, PAPER);
        }
        let foot = if total > PER_PAGE {
            format!("Up/Down: scroll   -   {total} tips   -   L / Esc / Enter: close")
        } else {
            "L / Esc / Enter: close".to_string()
        };
        centered(&foot, VW * 0.5, 620.0, 20.0, DIM);
    }

    /// Hoot pops in at the bottom of the encounter screen with its one-line nudge
    /// (stays until the choice is made).
    fn draw_owl(&self, lift: f32) {
        let Some(l) = &self.owl else { return };
        if !self.ai_fx.hints.enabled {
            return;
        }
        let pop = (self.owl_t / 0.25).min(1.0);
        // one line at 24 px fits the short bubble; a longer hint (up to 150 chars) gets a
        // slightly taller bubble and a smaller size so it never runs into the help line at y 710
        let bx = 118.0;
        let text_w = VW - 40.0 - bx - 36.0;
        let one_line = wrap(&l.text, 24.0, text_w).len() <= 1;
        let (y, h) = if one_line { (622.0 - lift, 68.0) } else { (612.0 - lift, 80.0) };
        let px = 4.0;
        let ox = 40.0;
        let oy = y + h - 16.0 * px * pop + (self.t * 3.0).sin() * 2.0 * pop;
        glow(ox + 32.0, y + 34.0, 46.0, KSU_GOLD, 0.8 * pop);
        draw_sprite(&HOOT, ox, oy, px * pop.max(0.25), false);
        if pop < 1.0 {
            return;
        }
        panel(bx, y, VW - 40.0 - bx, h, KSU_GOLD);
        // the bubble's tail points at Hoot
        draw_triangle(vec2(bx + 2.0, y + 26.0), vec2(bx + 2.0, y + 44.0), vec2(bx - 12.0, y + 36.0), KSU_GOLD);
        let name = &self.ai_fx.data.owl_name;
        draw_text(name, bx + 18.0, y + 24.0, 20.0, KSU_GOLD);
        if one_line {
            text_typed(&l.text, l.shown as usize, bx + 18.0, y + 52.0, 24.0, text_w, PAPER);
        } else {
            // the largest size whose lines end above the bubble's bottom edge
            let size = [21.0, 20.0, 18.0, 16.0].into_iter().find(|&sz| wrap(&l.text, sz, text_w).len() as f32 * sz * 1.15 <= h - 34.0).unwrap_or(16.0);
            text_typed(&l.text, l.shown as usize, bx + 18.0, y + 46.0, size, text_w, PAPER);
        }
        draw_text(&l.tag, VW - 52.0 - l.tag_w, y + 22.0, 18.0, DIM);
    }

    fn pick(&mut self, i: usize) {
        let loaded = self.loaded.clone();
        let e = enc_at(&loaded, self.run.room);
        if self.run.choose(&self.loaded.content, i).is_none() {
            return;
        }
        self.ai_fx.hints.stop();
        self.owl = None;
        // a tip still being written was for the undecided scenario: drop it
        self.tip_wait = None;
        self.ai_fx.cancel_tip();
        let ch = &e.choices[i];
        self.sel = i;
        self.line = None;
        self.applied = None;
        self.reactor.request(
            ReactionRequest {
                encounter_id: e.id.clone(),
                character: e.character.name.clone(),
                scenario: e.scenario.clone(),
                choice_label: ch.label.clone(),
                outcome: ch.outcome.as_str(),
            },
            ch.fallback_reaction.clone(),
            get_time(),
        );
        self.sfx(Sfx::Click);
    }

    // ------------------------------------------------------------ reaction + lesson

    fn reaction(&mut self, inp: Input) {
        draw_rectangle(0.0, 0.0, VW, VH, Color::new(0.0, 0.0, 0.0, 0.6));
        let loaded = self.loaded.clone();
        let e = enc_at(&loaded, self.run.room);
        let Some(d) = self.run.history.last() else {
            return; // can't happen (reaction implies a decision); don't crash the frame
        };
        let ch = &e.choices[d.choice];
        let oc = match ch.outcome {
            Outcome::Safe => SAFE,
            Outcome::Risky => RISKY,
            Outcome::Mixed => MIXED,
        };

        // portrait + speech bubble
        panel(40.0, 76.0, 200.0, 200.0, oc);
        let shake = if ch.outcome == Outcome::Risky { 0.0 } else { (self.t * 18.0).sin() * 1.5 };
        draw_sprite(character_sprite(&e.id), 52.0 + shake, 88.0, 11.0, false);
        panel(270.0, 76.0, 970.0, 200.0, PAPER);
        draw_text(&e.character.name, 296.0, 112.0, 26.0, KSU_GOLD);
        match &self.line {
            None => {
                let dots = 1 + (self.t * 3.0) as usize % 3;
                let t = self.txt.thinking.get(key(&[self.run.room, dots]), |b| {
                    let _ = write!(b, "{} is thinking{}", e.character.name, &"..."[..dots]);
                });
                draw_text(t, 296.0, 160.0, 28.0, DIM);
            }
            Some(l) => {
                text_typed(&l.text, l.shown as usize, 296.0, 156.0, 30.0, 920.0, WHITE);
                draw_text(&l.tag, 1220.0 - l.tag_w, 262.0, 18.0, DIM);
            }
        }

        // reveal the lesson (applies effects) once the line has finished typing
        let typed = self.line.as_ref().is_some_and(|l| l.shown as usize >= l.len);
        if self.run.phase == Phase::Reaction && self.line.is_some() && (typed || inp.confirm || inp.click) {
            if let Some(l) = &mut self.line {
                l.shown = l.len as f32;
            }
            self.applied = self.run.reveal_lesson(&self.loaded.content);
            // Hoot writes one "for next time" tip while the lesson is read; it pops in back on the floor
            if self.ai_fx.hints.enabled && !self.run.is_last_lesson() {
                let picked = tips::Picked {
                    label: ch.label.clone(),
                    outcome: ch.outcome,
                    warning_signs: ch.warning_signs.clone(),
                    safer_next_move: ch.safer_next_move.clone(),
                };
                let ctx = tips::TipContext {
                    kind: tips::TipKind::PostFloor,
                    stage: tips::TipStage::Reviewed,
                    floor: self.run.room,
                    enc: e.clone(),
                    picked: Some(picked),
                };
                self.ai_fx.ask_tip(ctx, get_time());
            }
            self.txt.chips.clear();
            if let Some(a) = self.applied {
                for (name, v, col) in [("Knowledge", a.knowledge, KSU_GOLD), ("Focus", a.focus, FOCUS), ("Health", a.health, RISKY)] {
                    if v != 0 {
                        let txt = format!("{:+} {}", v, name);
                        let w = measure_text(&txt, None, 24, 1.0).width + 24.0;
                        self.txt.chips.push((txt, w, col));
                    }
                }
            }
            self.sfx(match ch.outcome {
                Outcome::Safe => Sfx::Coin,
                Outcome::Risky => Sfx::Hurt,
                Outcome::Mixed => Sfx::Equip,
            });
            self.hw.send_event(match ch.outcome {
                Outcome::Risky => HwEvent::Hit,
                _ => HwEvent::LevelUp,
            });
            return; // don't let the same keypress also continue
        }
        if self.run.phase != Phase::Lesson {
            return;
        }

        // outcome + effects
        let y = 312.0;
        let badge = ch.outcome.label();
        let bw = measure_text(badge, None, 28, 1.0).width + 30.0;
        draw_rectangle(40.0, y - 28.0, bw, 40.0, oc);
        draw_text(badge, 55.0, y, 28.0, INK);
        let t = self.txt.chose.get(key(&[self.run.room, d.choice]), |b| {
            let _ = write!(b, "You chose: {}", ch.label);
        });
        draw_text(t, 60.0 + bw, y, 28.0, PAPER);
        let mut x = VW - 40.0;
        for (txt, w, col) in &self.txt.chips {
            let (w, col) = (*w, *col);
            x -= w + 10.0;
            draw_rectangle(x, y - 26.0, w, 36.0, Color::new(col.r * 0.3, col.g * 0.3, col.b * 0.3, 1.0));
            draw_rectangle_lines(x, y - 26.0, w, 36.0, 2.0, col);
            draw_text(txt, x + 12.0, y, 24.0, col);
        }

        // warning signs + safer next move (deterministic, from content)
        panel(40.0, 340.0, 590.0, 300.0, RISKY);
        draw_text("WARNING SIGNS", 64.0, 376.0, 28.0, RISKY);
        let mut yy = 414.0;
        for w in &ch.warning_signs {
            draw_rectangle(66.0, yy - 14.0, 8.0, 8.0, RISKY);
            yy = text_block(w, 88.0, yy, 25.0, 520.0, PAPER) + 8.0;
        }
        panel(650.0, 340.0, 590.0, 300.0, SAFE);
        draw_text("SAFER NEXT MOVE", 674.0, 376.0, 28.0, SAFE);
        text_block(&ch.safer_next_move, 674.0, 414.0, 27.0, 540.0, PAPER);

        let last = self.run.is_last_lesson();
        let label = if last { "See how you did  [Enter]" } else { "Back to the floor  [Enter]" };
        let r = Rect::new(VW * 0.5 - 200.0, 652.0, 400.0, 52.0);
        let hover = r.contains(inp.mouse);
        panel(r.x, r.y, r.w, r.h, if hover { WHITE } else { KSU_GOLD });
        centered(label, VW * 0.5, 686.0, 28.0, KSU_GOLD);
        if inp.confirm || (inp.click && hover) {
            self.run.advance();
            match self.run.phase {
                Phase::Explore => {
                    self.sfx(Sfx::Click);
                    self.line = None;
                    self.world.player.invuln = 1.0; // a moment to get your bearings
                }
                Phase::Ending => self.on_run_end(),
                _ => {}
            }
        }
    }

    // ------------------------------------------------------------ the Floor Keeper

    /// The Keeper's portrait; floor 1's fight shows the Inbox Mimic (same art and
    /// violet glow as in the fight) so the verdict screen matches what you just fought.
    fn keeper_portrait(&self, x: f32, y: f32, w: f32, h: f32, border: Color, mimic: bool) {
        panel(x, y, w, h, border);
        let (sprite, glow_col) = if mimic {
            (art::character_sprite("inbox_mimic"), Color::new(0.6, 0.4, 1.0, 1.0))
        } else {
            (&KEEPER, Color::new(1.0, 0.7, 0.3, 1.0))
        };
        glow(x + w * 0.5, y + h * 0.45, w * 0.45, glow_col, 1.4);
        let px = ((h - 40.0) / 16.0).min((w - 30.0) / 16.0).floor();
        let bob = (self.t * 1.8).sin() * 3.0;
        draw_sprite(sprite, x + w * 0.5 - 8.0 * px, y + 14.0 + bob, px, false);
    }

    fn keeper_quiz(&mut self, inp: Input, dt: f32) {
        draw_rectangle(0.0, 0.0, VW, VH, Color::new(0.0, 0.0, 0.0, 0.6));
        let Some(q) = self.quiz.as_ref().map(Rc::clone) else {
            // can't happen (the quiz is built when the phase starts); rebuild instead of stalling
            self.quiz = Some(Rc::new(self.build_quiz()));
            return;
        };
        if self.fight.is_some() {
            // Undertale-style: the fight gets a clean black stage, no dungeon behind the panels
            draw_rectangle(0.0, 0.0, VW, VH, BLACK);
            self.keeper_fight_round(&q, inp, dt);
            return;
        }
        self.keeper_portrait(40.0, 76.0, 260.0, 236.0, KSU_GOLD, false);
        panel(320.0, 76.0, 920.0, 236.0, KSU_GOLD);
        draw_text("THE FLOOR KEEPER BLOCKS THE STAIRS", 346.0, 112.0, 30.0, KSU_GOLD);
        draw_line(346.0, 124.0, 1210.0, 124.0, 1.0, Color::new(1.0, 0.77, 0.16, 0.4));
        // the Keeper's taunt (AI-voiced or scripted), typed out; the layout below is
        // sized for the whole line so nothing jumps while it types
        let y = match &self.taunt {
            Some(l) => {
                text_typed(&l.text, l.shown as usize, 346.0, 154.0, 24.0, 860.0, DIM);
                centered(&l.tag, 170.0, 304.0, 16.0, DIM);
                154.0 + wrap(&l.text, 24.0, 860.0).len() as f32 * 24.0 * 1.15
            }
            None => text_block("\"None pass without proving they listened.\"", 346.0, 154.0, 24.0, 860.0, DIM),
        };
        let y = text_block(&q.situation, 346.0, y + 4.0, 25.0, 860.0, PAPER);
        text_block(&q.question, 346.0, y + 10.0, 28.0, 860.0, KSU_GOLD);

        let n = q.options.len().min(4);
        if inp.left || inp.up {
            self.sel = (self.sel + n - 1) % n;
            self.sfx(Sfx::Click);
        }
        if inp.right || inp.down {
            self.sel = (self.sel + 1) % n;
            self.sfx(Sfx::Click);
        }
        let mut picked = None;
        for (i, opt) in q.options.iter().take(n).enumerate() {
            let r = Rect::new(40.0, 330.0 + i as f32 * 122.0, VW - 80.0, 110.0);
            if r.contains(inp.mouse) {
                if inp.mouse_moved {
                    self.sel = i;
                }
                if inp.click {
                    picked = Some(i);
                }
            }
            let on = self.sel == i;
            let lift = if on { -4.0 } else { 0.0 };
            panel(r.x, r.y + lift, r.w, r.h, if on { KSU_GOLD } else { DIM });
            draw_text(["1", "2", "3", "4"][i], r.x + 22.0, r.y + 64.0 + lift, 36.0, if on { KSU_GOLD } else { DIM });
            let lines = wrap(opt, 26.0, r.w - 110.0);
            let total = lines.len() as f32 * 30.0;
            for (j, l) in lines.iter().enumerate() {
                draw_text(l, r.x + 72.0, r.y + r.h * 0.5 - total * 0.5 + 22.0 + j as f32 * 30.0 + lift, 26.0, if on { WHITE } else { PAPER });
            }
        }
        let hint = if self.choice_open { "Right: the stairs open.  Wrong: fight 2 wardens first.   1-3, arrows + Enter, or click" } else { "Read carefully..." };
        centered(hint, VW * 0.5, 712.0, 20.0, DIM);
        if let Some(d) = inp.digit {
            if d < n {
                picked = Some(d);
            }
        }
        if inp.confirm {
            picked = Some(self.sel);
        }
        if let Some(i) = picked {
            self.answer_keeper(&q, i);
        }
    }

    /// The Keeper's question about the choice made on this floor; the layout is
    /// fixed by the run seed and floor, so a replay with CC_SEED is identical.
    fn build_quiz(&self) -> Quiz {
        keeper::build_for_floor(&self.loaded.content, self.run.room, self.run.choice_on_floor(), self.run_seed)
    }

    /// Floor 1: the Inbox Mimic fight (keeper_fight.rs). Turns: pick a command,
    /// then dodge its attack in the box. The fight decides the verdict.
    fn keeper_fight_round(&mut self, q: &Quiz, inp: Input, dt: f32) {
        let Some(mail) = &q.mail else { return };
        let fi = FightInput { up: inp.up, down: inp.down, left: inp.left, right: inp.right, confirm: inp.confirm, back: inp.back, digit: inp.digit, mv: inp.mv };
        let open = self.choice_open;
        let Some(f) = self.fight.as_mut() else { return };
        let ev = keeper_fight::step(&mut self.run, f, dt, &fi, open);
        f.draw(mail, self.t);
        let help = match f.stage {
            FightStage::Menu => "Left/Right + Enter or E, or 1-4: pick a command",
            FightStage::Acts => "Arrows + Enter or E, or 1-7: pick an act   -   Esc: back",
            FightStage::Dodge(_) => "WASD/arrows: dodge!   Hits cost box HP, never your Health",
            _ => "Enter / E: continue",
        };
        let (takeaway, ending) = (f.takeaway(), f.ending);
        centered(help, VW * 0.5, 676.0, 20.0, DIM);
        let rm = if self.reduced_motion { "R: reduced motion ON (no attacks)" } else { "R: reduced motion OFF" };
        centered(rm, VW * 0.5, 702.0, 18.0, DIM);
        match ev {
            Some(FightEvent::Hit) => {
                self.sfx(Sfx::Hurt);
                self.hw.send_event(HwEvent::Hit); // box HP only, but the matrix flashes too
            }
            Some(FightEvent::Reset) => {
                self.sfx(Sfx::Stairs);
                if self.auto.on {
                    let (h, f) = (self.run.stats.health, self.fight.as_ref().map_or(0, |f| f.resets));
                    println!("[auto] fight box hearts ran out: reset #{f}, run health still {h}, no verdict");
                }
            }
            Some(FightEvent::Tell) => self.sfx(Sfx::Coin),
            Some(FightEvent::PowerUp) => self.sfx(Sfx::Equip),
            Some(FightEvent::Nav | FightEvent::Picked) => self.sfx(Sfx::Click),
            Some(FightEvent::Verdict(right)) if self.run.phase == Phase::KeeperVerdict => {
                // the verdict screen: the safe move vs. what this fight did wrong
                let wrong = match ending {
                    Some(keeper_fight::Ending::DeletedUnread) => "Delete it unread, without checking a single detail",
                    _ => "Do what the message asks: reply, pay, or click Review",
                };
                let summary = Quiz {
                    situation: "You faced the Inbox Mimic's refund message.".into(),
                    question: q.question.clone(),
                    options: vec![q.options[q.correct].clone(), wrong.into()],
                    correct: 0,
                    warning: q.warning.clone(),
                    id: q.id,
                    mail: q.mail.clone(),
                };
                if self.auto.on {
                    println!("[auto] keeper fight verdict {}: {takeaway}", if right { "right" } else { "wrong" });
                }
                self.quiz = Some(Rc::new(summary));
                self.quiz_pick = Some(if right { 0 } else { 1 });
                self.verdict_ready = false;
                self.line = Some(Line::new(takeaway.to_string(), ai::LineSource::Fallback("the Mimic's takeaway")));
                self.fight = None;
                self.sfx(Sfx::Click);
            }
            _ => {}
        }
    }

    fn answer_keeper(&mut self, q: &Quiz, i: usize) {
        let correct = q.is_correct(i);
        if self.run.answer_keeper(correct).is_none() {
            return;
        }
        self.after_keeper_answer(correct, i);
    }

    /// The verdict screen's setup once the run has the answer.
    fn after_keeper_answer(&mut self, correct: bool, i: usize) {
        self.quiz_pick = Some(i);
        self.verdict_ready = false;
        // Keeper banter: code picked the verdict (`correct`); the backend checks the AI line
        // agrees with it (`banter::verdict_consistent`) and otherwise hands back the written
        // `keeper::line`, so the line on screen can never contradict the verdict
        let (t, src) = self.ai_fx.keeper_reaction(self.run.room, correct);
        if self.auto.on {
            println!("[auto] keeper verdict {} ({}): {t}", if correct { "right" } else { "wrong" }, source_tag(&src, "line"));
        }
        self.line = Some(Line::new(t, src));
        self.sfx(Sfx::Click);
    }

    fn keeper_verdict(&mut self, inp: Input, dt: f32) {
        let (Some(q), Some(pick)) = (self.quiz.as_ref().map(Rc::clone), self.quiz_pick) else {
            draw_rectangle(0.0, 0.0, VW, VH, Color::new(0.0, 0.0, 0.0, 0.62));
            self.run.leave_verdict();
            return;
        };
        // after the Mimic fight the verdict stays on the fight's black stage
        let mimic = q.mail.is_some();
        if self.risk.is_some() {
            draw_rectangle(0.0, 0.0, VW, VH, BLACK);
            self.risk_round(&inp, dt, mimic);
            return;
        }
        draw_rectangle(0.0, 0.0, VW, VH, if mimic { BLACK } else { Color::new(0.0, 0.0, 0.0, 0.62) });
        let correct = q.is_correct(pick);
        let oc = if correct { SAFE } else { RISKY };
        self.keeper_portrait(40.0, 76.0, 200.0, 200.0, oc, mimic);
        panel(270.0, 76.0, 970.0, 200.0, PAPER);
        draw_text(if mimic { "The Inbox Mimic" } else { keeper::KEEPER_NAME }, 296.0, 112.0, 26.0, KSU_GOLD);
        match &self.line {
            None => {
                let dots = 1 + (self.t * 3.0) as usize % 3;
                draw_text(&"The Keeper considers your answer..."[..32 + dots], 296.0, 160.0, 28.0, DIM);
            }
            Some(l) => {
                text_typed(&l.text, l.shown as usize, 296.0, 156.0, 30.0, 920.0, WHITE);
                draw_text(&l.tag, 1220.0 - l.tag_w, 262.0, 18.0, DIM);
            }
        }
        if !self.verdict_ready {
            let typed = self.line.as_ref().is_some_and(|l| l.shown as usize >= l.len);
            if self.line.is_some() && (typed || inp.confirm || inp.click) {
                if let Some(l) = &mut self.line {
                    l.shown = l.len as f32;
                }
                self.verdict_ready = true;
                if correct {
                    self.sfx(Sfx::Coin);
                    self.hw.send_event(HwEvent::LevelUp);
                } else {
                    self.sfx(Sfx::Hurt);
                    self.hw.send_event(HwEvent::Hit);
                }
            }
            return; // don't let the same keypress also continue
        }

        let y = 312.0;
        let badge = if correct { "CORRECT" } else { "NOT QUITE" };
        let bw = measure_text(badge, None, 28, 1.0).width + 30.0;
        draw_rectangle(40.0, y - 28.0, bw, 40.0, oc);
        draw_text(badge, 55.0, y, 28.0, INK);
        let reward = match (correct, mimic) {
            (true, true) => "+1 Knowledge.  The Mimic snaps shut and steps aside.",
            (true, false) => "+1 Knowledge.  The Keeper steps aside.",
            (false, true) => "The Mimic rings for its wardens.",
            (false, false) => "The Keeper rings for its wardens.",
        };
        draw_text(reward, 60.0 + bw, y, 28.0, PAPER);

        panel(40.0, 340.0, VW - 80.0, 300.0, SAFE);
        draw_text("THE SAFER NEXT MOVE", 64.0, 376.0, 28.0, SAFE);
        let mut yy = text_block(&q.situation, 64.0, 414.0, 24.0, VW - 140.0, DIM);
        yy = text_block(&q.options[q.correct], 64.0, yy + 4.0, 27.0, VW - 140.0, PAPER);
        if !correct {
            let t = self.txt.chose.get(key(&[self.run.room, pick, 99, q.id as usize]), |b| {
                let _ = write!(b, "You picked: {}  That's the move the trap is hoping for.", q.options[pick]);
            });
            yy = text_block(t, 64.0, yy + 10.0, 22.0, VW - 140.0, RISKY);
        }
        if let Some(w) = &q.warning {
            draw_rectangle(66.0, yy + 8.0, 8.0, 8.0, RISKY);
            text_block(w, 88.0, yy + 20.0, 24.0, VW - 160.0, PAPER);
        }

        if correct {
            // SPARE (the stairs, as always) or RISK IT (the bonus round)
            let stairs = if self.run.is_last_floor() { "SPARE: to the way out" } else { "SPARE: to the stairs" };
            let labels = [stairs, "RISK IT: bonus round"];
            let cols = [SAFE, RISKY];
            if inp.left || inp.up || inp.right || inp.down {
                self.verdict_sel = 1 - self.verdict_sel.min(1);
                self.sfx(Sfx::Click);
            }
            let mut picked = None;
            for (i, label) in labels.iter().enumerate() {
                let r = Rect::new(VW * 0.5 - 420.0 + i as f32 * 430.0, 652.0, 410.0, 52.0);
                let hover = r.contains(inp.mouse);
                if hover && inp.mouse_moved {
                    self.verdict_sel = i;
                }
                if hover && inp.click {
                    picked = Some(i);
                }
                let on = self.verdict_sel == i;
                panel(r.x, r.y - if on { 3.0 } else { 0.0 }, r.w, r.h, if on || hover { WHITE } else { cols[i] });
                let t = self.txt.chose.get(key(&[i, 77]), |b| {
                    let _ = write!(b, "{}  {label}", i + 1);
                });
                centered(t, r.x + r.w * 0.5, 686.0 - if on { 3.0 } else { 0.0 }, 26.0, cols[i]);
            }
            centered("Left/Right + Enter, 1-2 or click.  Risk It: grab 3 red flags in the box for +1 Health.", VW * 0.5, 624.0, 18.0, DIM);
            if let Some(d) = inp.digit.filter(|&d| d < 2) {
                picked = Some(d);
            }
            if inp.confirm {
                picked = Some(self.verdict_sel);
            }
            match picked {
                Some(1) => {
                    self.risk = Some(RiskIt::new(self.run_seed ^ q.id, self.run.room, self.reduced_motion));
                    self.verdict_sel = 0;
                    self.sfx(Sfx::Equip);
                    self.hw.send_event(HwEvent::Menu);
                    if self.auto.on {
                        println!("[auto] risk it: round starts on floor {}", self.run.room + 1);
                    }
                }
                Some(_) => self.leave_keeper_verdict(),
                None => {}
            }
            return;
        }
        let r = Rect::new(VW * 0.5 - 200.0, 652.0, 400.0, 52.0);
        let hover = r.contains(inp.mouse);
        panel(r.x, r.y, r.w, r.h, if hover { WHITE } else { oc });
        centered("Face 2 wardens  [Enter]", VW * 0.5, 686.0, 28.0, oc);
        if inp.confirm || (inp.click && hover) {
            self.leave_keeper_verdict();
        }
    }

    /// Off the verdict screen: the stairs open (right) or the wardens' arena (wrong).
    fn leave_keeper_verdict(&mut self) {
        self.run.leave_verdict();
        self.line = None;
        self.risk = None;
        self.verdict_sel = 0;
        match self.run.phase {
            Phase::Explore => {
                self.sfx(Sfx::LevelUp);
                self.world.player.invuln = 1.0;
                self.show_banner("THE STAIRS ARE OPEN".into(), "The Keeper steps aside. Take the stairs.".into(), SAFE);
            }
            Phase::BossFight => {
                self.arena = None; // built next frame, behind the black fade
                self.room_fade = 1.0;
                self.sfx(Sfx::Stairs);
                self.show_banner("THE KEEPER'S WARDENS".into(), "Full hearts for this fight only. Beat both to open the stairs.".into(), RISKY);
            }
            _ => {}
        }
    }

    /// The Risk It round (risk_it.rs): dodge the Keeper's attacks, grab red flags.
    fn risk_round(&mut self, inp: &Input, dt: f32, mimic: bool) {
        let Some(r) = self.risk.as_mut() else { return };
        let ev = r.update(dt, inp.mv);
        r.draw(if mimic { "The Inbox Mimic" } else { keeper::KEEPER_NAME }, self.t);
        let leave = r.can_leave() && (inp.confirm || inp.click);
        let help = if r.done() { "Enter: to the stairs" } else { "WASD/arrows: move.  Touch the red flags, dodge everything else." };
        centered(help, VW * 0.5, 676.0, 20.0, DIM);
        match ev {
            Some(RiskEvent::Hit) => {
                self.sfx(Sfx::Hurt);
                self.hw.send_event(HwEvent::Hit);
            }
            Some(RiskEvent::Flag) => self.sfx(Sfx::Coin),
            Some(RiskEvent::Finished(o)) => {
                let before = self.run.stats;
                self.run.risk_it(o);
                let after = self.run.stats;
                let reward = match o {
                    risk_it::Outcome::Defeated if after.health > before.health => "+1 Health. The Keeper bows out.".to_string(),
                    risk_it::Outcome::Defeated if after.focus > before.focus => "Health is full: +1 Focus instead.".to_string(),
                    risk_it::Outcome::Defeated => "Health and Focus are full. Bragging rights only.".to_string(),
                    risk_it::Outcome::KnockedOut if after.health < before.health => "-1 Health. The stairs still open.".to_string(),
                    risk_it::Outcome::KnockedOut => "Your last heart is safe: the box can't end a run.".to_string(),
                    risk_it::Outcome::TimeUp => "No harm done. The stairs are open.".to_string(),
                };
                if self.auto.on {
                    println!("[auto] risk it: {:?}, health {} -> {}, focus {} -> {}", o, before.health, after.health, before.focus, after.focus);
                }
                match o {
                    risk_it::Outcome::Defeated => {
                        self.sfx(Sfx::LevelUp);
                        self.hw.send_event(HwEvent::LevelUp);
                    }
                    risk_it::Outcome::KnockedOut => {
                        self.sfx(Sfx::Hurt);
                        self.hw.send_event(HwEvent::Hit);
                    }
                    risk_it::Outcome::TimeUp => self.sfx(Sfx::Click),
                }
                if let Some(r) = self.risk.as_mut() {
                    r.reward = reward;
                }
            }
            None => {}
        }
        if leave {
            self.leave_keeper_verdict();
        }
    }

    // ------------------------------------------------------------ ending

    fn ending(&mut self, inp: Input) {
        draw_rectangle(0.0, 0.0, VW, VH, Color::new(0.0, 0.0, 0.0, 0.75));
        let (title, sub, col) = match self.run.ending() {
            Ending::CryptKeeper => ("CRYPT KEEPER", "Every trap spotted, every Keeper answered. The monsters are filing complaints.", KSU_GOLD),
            Ending::Survivor => ("YOU ESCAPED THE CRYPT", "A few traps bit, but you walked out knowing their tricks.", SAFE),
            Ending::Overwhelmed => ("OVERWHELMED (BUT WISER)", "The crypt got the better of you this time. The traps don't change; you can.", RISKY),
        };
        shadow_centered(title, VW * 0.5, 90.0, 64.0, col);
        centered(sub, VW * 0.5, 130.0, 26.0, PAPER);
        let s = self.run.stats;
        let (safe, n) = (self.run.safe_choices(), self.run.history.len());
        let k = key(&[s.health as usize, s.focus as usize, s.knowledge as usize, safe, n]);
        let t = self.txt.ending_stats.get(k, |b| {
            let _ = write!(
                b,
                "Health {}/{}   -   Focus {}/{}   -   Knowledge {}   -   Safe choices {}/{}",
                s.health, MAX_HEALTH, s.focus, MAX_FOCUS, s.knowledge, safe, n
            );
        });
        centered(t, VW * 0.5, 166.0, 24.0, KSU_GOLD);
        // out of the Keepers actually reached (Run::keeper_tally): an early death never reads "0/10"
        let (kc, kn, wb, kills) = (self.run.keepers_correct(), self.run.keepers.len(), self.run.wardens_beaten(), self.kills as usize);
        let run = &self.run;
        let t = self.txt.ending_keepers.get(key(&[kc, kn, wb, kills, run.room, run.keepers_defeated as usize]), |b| {
            let _ = write!(b, "{}   -   Wardens beaten {wb}   -   Monsters defeated {kills}", run.keeper_tally());
            if run.keepers_defeated > 0 {
                let _ = write!(b, "   -   Keepers defeated {}", run.keepers_defeated);
            }
        });
        centered(t, VW * 0.5, 192.0, 20.0, DIM);
        panel(40.0, 204.0, VW - 80.0, 432.0, KSU_GOLD);
        // two tabs: Hoot's study guide (the debrief) and the field notes
        if inp.left || inp.right || is_key_pressed(KeyCode::Tab) {
            self.ending_tab = 1 - self.ending_tab;
            self.sfx(Sfx::Click);
        }
        let tabs = [("HOOT'S STUDY GUIDE", 64.0), ("YOUR FIELD NOTES", 400.0)];
        for (i, (label, x)) in tabs.iter().enumerate() {
            let on = self.ending_tab == i;
            let w = measure_text(label, None, 28, 1.0).width;
            let r = Rect::new(*x - 8.0, 214.0, w + 16.0, 36.0);
            if inp.click && r.contains(inp.mouse) && self.choice_open {
                self.ending_tab = i;
            }
            draw_text(label, *x, 240.0, 28.0, if on { KSU_GOLD } else { DIM });
            if on {
                draw_line(*x, 248.0, *x + w, 248.0, 2.0, KSU_GOLD);
            }
        }
        draw_text("<- / -> or Tab: switch", VW - 290.0, 238.0, 20.0, DIM);
        if self.ending_tab == 0 {
            self.draw_debrief();
        } else {
            self.draw_field_notes();
        }
        centered("Can you name one warning sign from the crypt? That's the whole trick.", VW * 0.5, 666.0, 24.0, PAPER);
        let r = Rect::new(VW * 0.5 - 150.0, 680.0, 300.0, 34.0);
        centered("[Enter] Play again", VW * 0.5, 704.0, 26.0, if r.contains(inp.mouse) { WHITE } else { KSU_GOLD });
        // docs/FLOWCHART.md: EndWin/EndLose --> Title ("play again"); Enter on the title starts the next run
        if inp.confirm || inp.back || (inp.click && r.contains(inp.mouse)) {
            self.screen = Screen::Title;
            self.title_view = TitleView::Main;
            self.menu_sel = 0;
            self.hw.send_event(HwEvent::Menu);
        }
    }

    /// The post-run debrief: Hoot's 3-5 line study guide from the mistake log
    /// (AI-written if it arrives within ~8.5 s and passes the filters, else templated).
    fn draw_debrief(&mut self) {
        let glow_a = 0.7 + 0.3 * (self.t * 2.0).sin();
        glow(124.0, 330.0, 70.0, KSU_GOLD, glow_a);
        draw_sprite(&HOOT, 68.0, 270.0 + (self.t * 2.0).sin() * 3.0, 7.0, false);
        let name = self.ai_fx.data.owl_name.clone();
        centered(&name, 124.0, 410.0, 24.0, KSU_GOLD);
        let (x, w) = (210.0, VW - 290.0);
        match self.ai_fx.poll_debrief(&self.loaded.content, get_time()) {
            None => {
                let dots = 1 + (self.t * 3.0) as usize % 3;
                let t = self.txt.thinking.get(key(&[777, dots]), |b| {
                    let _ = write!(b, "{name} is writing your study guide{}", &"..."[..dots]);
                });
                draw_text(t, x, 300.0, 26.0, DIM);
                // a quill scratching: three gold dashes that grow
                for i in 0..3 {
                    let len = 30.0 + 20.0 * ((self.t * 4.0 + i as f32).sin() * 0.5 + 0.5);
                    draw_rectangle(x, 330.0 + i as f32 * 18.0, len * (i as f32 + 2.0), 6.0, Color::new(1.0, 0.77, 0.16, 0.35));
                }
            }
            Some(mut d) => {
                if self.auto.long_debrief {
                    d.lines = self.long_debrief_lines();
                }
                if self.auto.on && !self.debrief_logged {
                    self.debrief_logged = true;
                    println!("[auto] debrief ({}):", source_tag(&d.source, "study guide"));
                    d.lines.iter().for_each(|l| println!("[auto]   {l} ({} chars)", l.len()));
                }
                // every line wraps inside the panel; if they don't all fit at 24px, step the
                // size down (never drop a line). Picked once: the guide doesn't change.
                let (top, gap) = (290.0, 10.0);
                let size = *self.debrief_size.get_or_insert_with(|| {
                    let items: Vec<Vec<(&str, f32)>> = d.lines.iter().map(|l| vec![(l.as_str(), 0.0)]).collect();
                    let sizes = [(24.0, 24.0), (22.0, 22.0), (21.0, 21.0), (20.0, 20.0), (18.0, 18.0), (16.0, 16.0)];
                    fit_sizes(&items, top, DEBRIEF_BOTTOM, w, gap, &sizes).0
                });
                let mut y = top;
                for l in &d.lines {
                    draw_rectangle(x - 18.0, y - size * 0.58, 8.0, 8.0, KSU_GOLD);
                    y = text_block(l, x, y, size, w, PAPER) + gap;
                }
                let tag = source_tag(&d.source, "study guide");
                draw_text(&tag, VW - 56.0 - measure_text(&tag, None, 16, 1.0).width, 626.0, 16.0, DIM);
            }
        }
    }

    /// CC_DEBRIEF_LONG=1: a stress study guide of five ~160-200 char lines built from real
    /// content (Ticket Troll's "Tap pay link" first), to check the ending panel's wrapping.
    fn long_debrief_lines(&self) -> Vec<String> {
        let enc = &self.loaded.content.encounters;
        let mut picks: Vec<(usize, usize)> = Vec::new();
        for (i, e) in enc.iter().enumerate() {
            for (j, ch) in e.choices.iter().enumerate() {
                if ch.outcome != Outcome::Safe {
                    picks.push((i, j));
                }
            }
        }
        let line = |&(i, j): &(usize, usize)| {
            let (e, ch) = (&enc[i], &enc[i].choices[j]);
            let sign = ch.warning_signs.first().map_or("", String::as_str);
            format!("Floor {}, {}: you picked {}. {} Next time: {}", i + 1, e.character.name, ch.label, sign, ch.safer_next_move)
        };
        let mut lines: Vec<String> = picks.iter().map(line).collect();
        lines.sort_by_key(|l| std::cmp::Reverse(l.len()));
        let troll = picks.iter().find(|&&(i, j)| enc[i].choices[j].label == "Tap pay link").map(line);
        lines.retain(|l| Some(l) != troll.as_ref());
        troll.into_iter().chain(lines).take(5).collect()
    }

    fn draw_field_notes(&mut self) {
        let n = self.run.history.len();
        // field notes are fixed once the run ends: format them once
        if self.txt.notes_key != Some(n as u64) {
            self.txt.notes_key = Some(n as u64);
            self.txt.notes_sizes = (0.0, 0.0);
            self.txt.notes = self
                .run
                .history
                .iter()
                .map(|d| {
                    let e = &self.loaded.content.encounters[d.encounter];
                    let ch = &e.choices[d.choice];
                    (
                        format!("{}: {}", e.name, ch.label),
                        ch.warning_signs.first().map(|w| format!("Spot it: {w}")),
                        format!("Next time: {}", ch.safer_next_move),
                        d.outcome,
                    )
                })
                .collect();
        }
        let colour = |o: &Outcome| match o {
            Outcome::Safe => SAFE,
            Outcome::Risky => RISKY,
            Outcome::Mixed => MIXED,
        };
        // Everything wraps; the sizes step down until the whole stack fits the panel
        // (picked once per run: the notes don't change after the run ends).
        let short = self.txt.notes.len() <= 4;
        let col_w = (VW - 80.0 - 72.0) * 0.5;
        let (top, gap) = if short { (274.0, 8.0) } else { (268.0, 10.0) };
        if self.txt.notes_sizes.0 == 0.0 {
            let fit = |notes: &[(String, Option<String>, String, Outcome)], w: f32, sizes: &[(f32, f32)], with_spot: bool| {
                let items: Vec<Vec<(&str, f32)>> = notes
                    .iter()
                    .map(|(head, spot, next, _)| {
                        let mut v = vec![(head.as_str(), 0.0)];
                        if let (true, Some(spot)) = (with_spot, spot) {
                            v.push((spot.as_str(), 20.0));
                        }
                        v.push((next.as_str(), 20.0));
                        v
                    })
                    .collect();
                fit_sizes(&items, top, DEBRIEF_BOTTOM, w, gap, sizes)
            };
            self.txt.notes_sizes = if short {
                fit(&self.txt.notes, VW - 160.0, &[(24.0, 21.0), (22.0, 20.0), (20.0, 18.0), (18.0, 16.0)], true)
            } else {
                // both columns must fit: take the smaller pair of the two
                let sizes = [(21.0, 18.0), (20.0, 18.0), (20.0, 16.0), (18.0, 16.0), (16.0, 16.0)];
                let (l, r) = self.txt.notes.split_at(5.min(self.txt.notes.len()));
                let (a, b) = (fit(l, col_w, &sizes, false), fit(r, col_w, &sizes, false));
                if a.0 + a.1 <= b.0 + b.1 { a } else { b }
            };
        }
        let (hs, bs) = self.txt.notes_sizes;
        if short {
            let mut y = top;
            for (head, spot, next, outcome) in &self.txt.notes {
                y = text_block(head, 64.0, y, hs, VW - 160.0, colour(outcome));
                if let Some(spot) = spot {
                    y = text_block(spot, 84.0, y, bs, VW - 180.0, PAPER);
                }
                y = text_block(next, 84.0, y, bs, VW - 180.0, DIM) + gap;
            }
        } else {
            // a long run: two columns of five, the choice and the safer next move
            for col in 0..2 {
                let x = 64.0 + col as f32 * (col_w + 24.0);
                let mut y = top;
                for (head, _, next, outcome) in self.txt.notes.iter().skip(col * 5).take(5) {
                    y = text_block(head, x, y, hs, col_w, colour(outcome));
                    y = text_block(next, x + 12.0, y, bs, col_w - 20.0, DIM) + gap;
                }
            }
        }
    }

    // ------------------------------------------------------------ hardware + automation

    /// Mirror resources to the optional output-only status display (hardware/README.md):
    /// HP = health, LVL = knowledge, FLOOR = floor (of `floors`), GOLD = focus,
    /// KILLS = monsters + wardens defeated, mode F = the floor-1 Keeper fight.
    fn hw_sync(&mut self, dt: f32) {
        self.hw_timer -= dt;
        if self.hw_timer > 0.0 {
            return;
        }
        self.hw_timer = 0.2;
        let s = self.run.stats;
        let mode = match (self.screen, self.run.phase) {
            (Screen::Title, _) => 'M',
            (_, Phase::Ending) if self.run.ending() == Ending::Overwhelmed => 'D',
            (_, Phase::KeeperQuiz) if self.fight.is_some() => 'F',
            (_, Phase::KeeperVerdict) if self.risk.is_some() => 'F',
            _ if self.settings_open => 'X',
            _ => 'P',
        };
        let st = HwState {
            hp: s.health,
            max_hp: MAX_HEALTH,
            level: s.knowledge as u32,
            floor: self.run.room as u32 + 1,
            gold: s.focus as u32,
            kills: self.kills,
            mode,
            show_kills: false,
            floors: self.run.rooms as u32,
        };
        // every tick, not only on change: send_state rate-limits changes and adds the 0.5 s
        // heartbeat the display needs (it falls back to "waiting..." after 3 s of silence)
        self.hw.send_state(st);
    }

    /// Which screen/phase this frame belongs to, for the CC_PERF report.
    fn perf_tag(&self) -> &'static str {
        if self.pending_floor || (self.run.phase == Phase::BossFight && self.arena.is_none()) {
            return "build (black frame)";
        }
        match (self.screen, self.run.phase) {
            (Screen::Title, _) => "title",
            (_, Phase::Explore) => "explore",
            (_, Phase::Encounter) => "encounter",
            (_, Phase::Reaction) => "reaction",
            (_, Phase::Lesson) => "lesson",
            (_, Phase::KeeperQuiz) => "keeper quiz",
            (_, Phase::KeeperVerdict) => "keeper verdict",
            (_, Phase::BossFight) => "wardens",
            (_, Phase::Ending) => "ending",
        }
    }

    /// CC_AUTO: print each state change, so a play-test can be read back from the console.
    fn trace(&mut self) {
        let now = (self.screen, self.run.phase, self.run.room, self.run.bosses_left);
        if self.auto.last == Some(now) {
            return;
        }
        self.auto.last = Some(now);
        let s = self.run.stats;
        println!(
            "[auto] t={:6.1}s {:?} floor {}/{} phase {:?} health {} focus {} knowledge {} wardens_left {} stairs_open {} defeated {}",
            self.t,
            self.screen,
            self.run.room + 1,
            self.run.rooms,
            self.run.phase,
            s.health,
            s.focus,
            s.knowledge,
            self.run.bosses_left,
            self.run.stairs_open,
            self.kills
        );
        if self.run.phase == Phase::Ending && self.screen == Screen::Playing {
            println!(
                "[auto] ending {:?}: keepers {}/{}, wardens beaten {}",
                self.run.ending(),
                self.run.keepers_correct(),
                self.run.keepers.len(),
                self.run.wardens_beaten()
            );
        }
    }

    /// CC_SHOT_EVENTS: which AI-extras screen is settled on show right now (once each).
    fn event_shot(&mut self) {
        let floor = self.run.room + 1;
        let t = self.auto.timer;
        let typed = |l: &Option<Line>| l.as_ref().is_some_and(|l| l.shown as usize >= l.len);
        let tag = match (self.screen, self.run.phase) {
            (Screen::Title, _) if t > 1.0 => Some("title".to_string()),
            (Screen::Playing, Phase::Explore) => match &self.mini {
                Some(m) if m.verdict.is_none() && t > 2.6 => Some(format!("f{floor:02}_side_ask")),
                Some(m) if m.verdict.is_some() && t > 3.1 => Some(format!("f{floor:02}_side_reveal")),
                Some(_) => None,
                None if self.world.prop.is_some_and(|p| !p.done && self.world.player.pos.distance(p.pos) < TALK_RANGE) => {
                    Some(format!("f{floor:02}_side_prop"))
                }
                None => self
                    .banner
                    .as_ref()
                    .filter(|b| b.note.is_some() && b.secs - b.t > 1.0)
                    .map(|_| format!("f{floor:02}_tier_banner")),
            },
            (Screen::Playing, Phase::Encounter) if typed(&self.owl) && self.owl_t > 1.6 => Some(format!("f{floor:02}_owl_hint")),
            (Screen::Playing, Phase::KeeperQuiz) if self.fight.is_some() => self.fight.as_ref().and_then(|f| match &f.stage {
                FightStage::Menu if f.spareable() && t > 0.8 => Some("f01_fight_spareable".to_string()),
                FightStage::Menu if f.tells() == 0 && f.bad == 0 && t > 0.8 => Some("f01_fight_menu".to_string()),
                FightStage::Talk(_) if f.tells() > 0 && !f.spareable() && t > 0.8 => Some("f01_fight_reveal".to_string()),
                FightStage::Dodge(d) if d.t > 2.4 => Some(format!("f01_fight_attack_{:?}", d.pattern).to_lowercase()),
                _ => None,
            }),
            (Screen::Playing, Phase::KeeperQuiz) if typed(&self.taunt) && t > 2.1 => Some(format!("f{floor:02}_keeper_taunt")),
            (Screen::Playing, Phase::KeeperVerdict) if self.risk.is_some() => self.risk.as_ref().and_then(|r| match r.outcome {
                None if r.t > 4.0 && r.flag.is_some() => Some(format!("f{floor:02}_risk_round")),
                Some(_) if r.can_leave() => Some(format!("f{floor:02}_risk_result")),
                _ => None,
            }),
            (Screen::Playing, Phase::KeeperVerdict) if self.verdict_ready && t > 1.0 => Some(format!("f{floor:02}_keeper_verdict")),
            (Screen::Playing, Phase::Ending) if self.room_fade <= 0.0 => {
                let ready = self.ai_fx.poll_debrief(&self.loaded.content, get_time()).is_some();
                if !ready && !self.auto.shots_taken.iter().any(|s| s == "ending_debrief_wait") {
                    Some("ending_debrief_wait".to_string())
                } else if ready && t > 1.5 && self.ending_tab == 0 {
                    if self.auto.shots_taken.iter().any(|s| s == "ending_debrief") {
                        self.ending_tab = 1;
                        self.auto.timer = 0.0;
                        None
                    } else {
                        Some("ending_debrief".to_string())
                    }
                } else if self.ending_tab == 1 && t > 1.0 {
                    if self.auto.shots_taken.iter().any(|s| s == "ending_notes") {
                        self.quit = true;
                        None
                    } else {
                        Some("ending_notes".to_string())
                    }
                } else {
                    None
                }
            }
            _ => None,
        };
        if let Some(tag) = tag {
            if !self.auto.shots_taken.contains(&tag) {
                self.auto.shots_taken.push(tag.clone());
                self.auto.shot_request = Some(tag);
            }
        }
    }

    /// Hands-free play for automated testing and screenshots.
    fn autopilot(&mut self, inp: &mut Input, dt: f32) {
        self.auto.timer += dt;
        if self.auto.shot_events {
            self.event_shot();
        }
        if self.screen == Screen::Playing {
            match self.run.phase {
                Phase::Explore if self.mini.is_some() => {
                    // a side game: read for a moment, answer per CC_MINI, then read the reveal
                    let answered = self.mini.as_ref().is_some_and(|m| m.verdict.is_some());
                    if self.auto.timer < if answered { 3.5 } else { 3.0 } {
                        return;
                    }
                    self.auto.timer = 0.0;
                    if answered {
                        inp.confirm = true;
                    } else {
                        let right = self.auto.mini.get(self.minis_opened - 1).copied().unwrap_or(true);
                        let pick = match self.mini.as_ref().map(|m| &m.kind) {
                            Some(MiniKind::Spot(r)) => if right { r.fake_index() } else { 1 - r.fake_index() },
                            _ => usize::from(!right), // 0 = report (right), 1 = trust
                        };
                        inp.digit = Some(pick);
                    }
                    return;
                }
                Phase::Explore => {
                    // visit this floor's side game first (screenshots and play-tests cover it)
                    let prop = self.world.prop.filter(|p| !p.done).map(|p| p.pos);
                    let target = prop.unwrap_or_else(|| self.objective().0);
                    let ctl = self.world.auto_control(Some(target));
                    inp.mv = ctl.mv;
                    inp.attack = ctl.attack;
                    inp.aim_world = ctl.aim;
                    let close = self.world.player.pos.distance(target) < TILE * 1.0;
                    if close && self.auto.timer > 0.6 {
                        self.auto.timer = 0.0;
                        inp.interact = true;
                    }
                    return;
                }
                Phase::KeeperVerdict if self.risk.is_some() => {
                    // CC_RISK=1: the bonus round: head for the flags, dodge the rest
                    // (CC_FIGHT_SEEK=1: fly into the attacks instead)
                    if let Some(r) = &self.risk {
                        if r.done() {
                            if r.can_leave() && self.auto.timer > 1.5 {
                                self.auto.timer = 0.0;
                                inp.confirm = true;
                            }
                        } else {
                            inp.mv = r.auto_steer(self.auto.fight_seek);
                        }
                    }
                    return;
                }
                Phase::KeeperQuiz if self.fight.is_some() => {
                    // floor 1's fight: CC_KEEPER r = investigate then SPARE, w = bad acts;
                    // dodge on the Mimic's turns (CC_FIGHT_SEEK=1: fly into attacks instead)
                    let right = self.auto.keeper.get(self.run.room).copied().unwrap_or(true);
                    if let Some(f) = &self.fight {
                        if matches!(f.stage, FightStage::Dodge(_)) {
                            inp.mv = f.auto_steer(self.auto.fight_seek);
                        } else if self.auto.timer > 1.6 {
                            self.auto.timer = 0.0;
                            match f.auto_digit(right) {
                                Some(d) => inp.digit = Some(d),
                                None => inp.confirm = true,
                            }
                        }
                    }
                    return;
                }
                Phase::BossFight => {
                    if let Some(a) = self.arena.as_mut() {
                        let ctl = a.auto_control(None);
                        inp.mv = ctl.mv;
                        inp.attack = ctl.attack;
                        inp.aim_world = ctl.aim;
                    }
                    return;
                }
                _ => {}
            }
        }
        let wait = match (self.screen, self.run.phase) {
            (Screen::Title, _) => 1.5,
            (_, Phase::Encounter) => self.auto.enc_wait,
            (_, Phase::Reaction) => return,
            (_, Phase::Lesson) => 3.0,
            (_, Phase::KeeperQuiz) => 2.5,
            (_, Phase::KeeperVerdict) => 3.0,
            _ => f32::INFINITY,
        };
        if self.auto.timer < wait {
            return;
        }
        self.auto.timer = 0.0;
        match (self.screen, self.run.phase) {
            (Screen::Playing, Phase::Encounter) => {
                inp.digit = Some(self.auto.picks.get(self.run.room).copied().unwrap_or(0));
            }
            (Screen::Playing, Phase::KeeperQuiz) => {
                if let Some(q) = &self.quiz {
                    let right = self.auto.keeper.get(self.run.room).copied().unwrap_or(true);
                    inp.digit = Some(if right { q.correct } else { (q.correct + 1) % q.options.len() });
                }
            }
            (Screen::Playing, Phase::KeeperVerdict) if self.auto.risk && self.verdict_ready && self.run.keepers.last().is_some_and(|k| k.correct) => {
                inp.digit = Some(1);
            }
            _ => inp.confirm = true,
        }
    }
}

// ---------------------------------------------------------------- entry point

#[cfg(not(target_arch = "wasm32"))]
fn save_screenshot(path: &str) {
    let mut shot = get_screen_data();
    for px in shot.bytes.as_chunks_mut::<4>().0 {
        px[3] = 255;
    }
    if let Some(dir) = std::path::Path::new(path).parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    shot.export_png(path);
    println!("saved {path}");
}
#[cfg(target_arch = "wasm32")]
fn save_screenshot(_path: &str) {}

fn automation() -> Auto {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let env = |k: &str| std::env::var(k).ok();
        Auto {
            on: env("CC_AUTO").as_deref() == Some("1"),
            picks: env("CC_PICKS").map(|s| s.split(',').filter_map(|x| x.trim().parse().ok()).collect()).unwrap_or_default(),
            shots: env("CC_SHOTS").map(|s| s.split(',').filter_map(|x| x.trim().parse().ok()).collect()).unwrap_or_default(),
            prefix: env("CC_SHOT_PREFIX").unwrap_or_else(|| "shot_".into()),
            exit_at: env("CC_EXIT").and_then(|s| s.parse().ok()).unwrap_or(f32::INFINITY),
            timer: 0.0,
            keeper: env("CC_KEEPER").map(|s| s.split(',').map(|x| !x.trim().eq_ignore_ascii_case("w")).collect()).unwrap_or_default(),
            god: env("CC_GOD").as_deref() == Some("1"),
            start_floor: env("CC_FLOOR").and_then(|s| s.parse().ok()).unwrap_or(0),
            mini: env("CC_MINI").map(|s| s.split(',').map(|x| !x.trim().eq_ignore_ascii_case("w")).collect()).unwrap_or_default(),
            enc_wait: env("CC_ENC_WAIT").and_then(|s| s.parse().ok()).unwrap_or(2.0),
            fight_seek: env("CC_FIGHT_SEEK").as_deref() == Some("1"),
            long_debrief: env("CC_DEBRIEF_LONG").as_deref() == Some("1"),
            shot_events: env("CC_SHOT_EVENTS").as_deref() == Some("1"),
            risk: env("CC_RISK").as_deref() == Some("1"),
            title_view: env("CC_TITLE"),
            shots_taken: Vec::new(),
            shot_request: None,
            last: None,
        }
    }
    #[cfg(target_arch = "wasm32")]
    Auto {
        on: false,
        picks: Vec::new(),
        keeper: Vec::new(),
        shots: Vec::new(),
        prefix: String::new(),
        exit_at: f32::INFINITY,
        timer: 0.0,
        god: false,
        start_floor: 0,
        mini: Vec::new(),
        enc_wait: 2.0,
        fight_seek: false,
        long_debrief: false,
        shot_events: false,
        risk: false,
        title_view: None,
        shots_taken: Vec::new(),
        shot_request: None,
        last: None,
    }
}

fn window_conf() -> Conf {
    #[cfg_attr(target_arch = "wasm32", allow(unused_mut))] // only native builds tweak it
    let mut c = Conf {
        window_title: "Campus Crypt".to_owned(),
        window_width: 1280,
        window_height: 720,
        high_dpi: false,
        sample_count: 1,
        ..Default::default()
    };
    // CC_NOVSYNC=1: uncapped frame rate, for measuring (native only)
    #[cfg(not(target_arch = "wasm32"))]
    if std::env::var("CC_NOVSYNC").is_ok() {
        c.platform.swap_interval = Some(0);
    }
    c
}

/// Clamp the frame delta. A long gap (window dragged, minimized, tab hidden,
/// focus lost, debugger) is treated as a pause and becomes one normal frame,
/// instead of a 100 ms jump that teleports the player / skips the typewriter.
fn frame_dt(raw: f32) -> f32 {
    if !raw.is_finite() || raw <= 0.0 {
        0.0
    } else if raw > 0.25 {
        1.0 / 60.0
    } else {
        raw.min(0.1)
    }
}

#[macroquad::main(window_conf)]
async fn main() {
    let seed = (macroquad::miniquad::date::now() * 1000.0) as u64;
    // CC_SEED=n: the same floors every run (screenshots, play-tests)
    #[cfg(not(target_arch = "wasm32"))]
    let seed = std::env::var("CC_SEED").ok().and_then(|s| s.parse().ok()).unwrap_or(seed);
    rand::srand(seed);
    clear_background(Color::from_rgba(8, 6, 12, 255));
    draw_text("Loading...", 40.0, 60.0, 32.0, WHITE);
    next_frame().await;
    // grow the font atlas now, not mid-frame (avoids "glBindTexture ... deleted texture")
    art::prewarm_font_atlas();

    let loaded = content::load();
    let audio = Audio::new().await;
    let mut app = App::new(loaded, audio, automation());
    let mut wall = 0.0f32;
    let mut shot_n = 0;
    let mut manual_n = 0;
    let mut perf = perf::Perf::from_env();
    loop {
        let dt = frame_dt(get_frame_time());
        wall += dt;
        let tag = app.perf_tag();
        perf.begin();
        app.frame(dt);
        perf.end(tag, dt);
        if is_key_pressed(KeyCode::F12) {
            manual_n += 1;
            save_screenshot(&format!("screenshots/manual_{manual_n:02}.png"));
        }
        if let Some(tag) = app.auto.shot_request.take() {
            save_screenshot(&format!("screenshots/{}{tag}.png", app.auto.prefix));
        }
        if let Some(&t) = app.auto.shots.first() {
            if wall >= t {
                app.auto.shots.remove(0);
                shot_n += 1;
                save_screenshot(&format!("screenshots/{}{}.png", app.auto.prefix, shot_n));
            }
        }
        if app.quit || wall >= app.auto.exit_at {
            perf.report();
            break;
        }
        next_frame().await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn side_games_never_repeat_a_message_and_keep_their_slots_apart() {
        let data = ai_features::data::embedded().data;
        for seed in 0..300u32 {
            let sides = plan_sides(&data, seed, state::RUN_FLOORS);
            let mut templates = Vec::new();
            for (f, side) in sides.iter().enumerate() {
                match side {
                    Some(Side::Scam(slot)) => {
                        assert_eq!(SCAM_FLOORS[*slot], f);
                        assert!(*slot < scamgen::POOL_SIZE, "phones use the prefetched slots");
                        templates.push(scamgen::plan(&data, seed, *slot).template);
                    }
                    Some(Side::Spot(slot)) => {
                        assert!(SPOT_FLOORS.contains(&f));
                        assert!(*slot >= scamgen::POOL_SIZE, "boards never reuse a prefetched slot");
                        templates.push(scamgen::plan(&data, seed, *slot).template);
                    }
                    None => assert!(!SCAM_FLOORS.contains(&f), "every phone floor has a phone"),
                }
            }
            let n = templates.len();
            templates.sort_unstable();
            templates.dedup();
            assert_eq!(templates.len(), n, "seed {seed}: a template repeats");
            // as many boards as there are unused templates (9 templates today: all 3 boards)
            assert_eq!(n, (SCAM_FLOORS.len() + SPOT_FLOORS.len()).min(data.scam_templates.len()));
        }
        // a short run only plans the floors it has
        assert!(plan_sides(&data, 1, 2).iter().all(|s| !matches!(s, Some(Side::Spot(_)))));
    }
}
