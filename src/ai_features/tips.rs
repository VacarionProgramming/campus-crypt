//! Hoot's tips: the owl companion you can ask anytime (T) and the short "for next
//! time" tip after each floor, plus the run's tip log (L).
//!
//! Same guardrails as the rest of `ai_features`: code decides WHEN Hoot speaks and
//! checks every word; the AI only phrases the tip. Every request has a scripted
//! fallback from the offline bank below, so tips work with `CAMPUS_CRYPT_AI=off`,
//! no key, no network and on the web build (the scripted tip just arrives at once).
//!
//! * Exploring (nothing read yet): ONE general habit. The model is not told what the
//!   floor holds, so it cannot spoil it.
//! * Deciding (reading a scenario): the same rules as the hesitation hint
//!   (`hints::hint_ok`): point at a warning sign, never name a choice or a safe move.
//! * Reviewed (floor finished): one habit for next time, written from the choice made.
//!
//! Budget: tips use their own small queue, separate from the per-run feature budget.
//! At most `MAX_AI_TIPS_PER_RUN` calls per run (sized for a 20-floor run: one
//! post-floor tip on every floor but the last, plus a few asked tips), and an
//! asked tip needs `ASK_COOLDOWN_SECS` between live calls (Groq free tier).
//! Past either limit the scripted bank answers instantly.

use super::banter::MAX_BANTER_CHARS;
use super::data::FeatureData;
use super::hints;
use super::validate;
use crate::ai::{AiError, CallResult, ChatCaller, Config, LineSource, TokenLimits, TIMEOUT_SECS};
use crate::content::{Encounter, Outcome};
use crate::keeper::mix;
use std::sync::mpsc::{channel, Receiver, TryRecvError};
use std::sync::Arc;

/// Longest tip (fits Hoot's bubble).
pub const MAX_TIP_CHARS: usize = MAX_BANTER_CHARS;
/// Minimum time between two live (AI) calls for asked tips.
pub const ASK_COOLDOWN_SECS: f64 = 6.0;
/// Live tip calls per run; after that the scripted bank answers.
/// A 20-floor run writes one post-floor tip on every floor but the last (19).
/// The extra calls cover a handful of asked tips (T).
pub const MAX_AI_TIPS_PER_RUN: u32 = 24;
/// Tips kept in the log (oldest dropped).
pub const MAX_LOG: usize = 40;
/// (gpt-oss / Groq, where reasoning shares the budget; other providers).
const TIP_TOKENS: TokenLimits = TokenLimits(700, 90);

const DEFAULT_TIP: &str = "Hoo, slow down. Who sent this, and what are they asking you to do?";

/// System prompt for tip calls (the hard rules mirror `ai_features_prompts.md`'s shared block).
pub const TIP_SYSTEM: &str = r#"You are Hoot, a friendly owl companion in Campus Crypt, a funny pixel-art dungeon crawler for Kennesaw State (KSU) students about scams and deadline pressure. Students are called Owls. The tone is a cozy cartoon crypt: warm, a little silly, never scary.

You give ONE short, practical tip: one or two sentences, 150 characters or fewer in total, starting with Hoo. Follow the situation line exactly. The game code has already decided every outcome; you only phrase the tip.

Hard rules:
- Plain ASCII text only. No markdown, no emoji, no quotation marks.
- Never write a link, web address, domain name, email address, phone number, or the @ sign. Never write click here.
- Never use game words: health, focus (or focused), knowledge, score, damage, points, level, hit points, game over, you lose.
- Never claim that a habit, a tool or the game prevents scams, protects anyone, or guarantees safety.
- Never name real companies, real websites, real people, or official campus offices by their real names. Generic descriptions are fine (the help desk, the advisor).
- Keep it PG and kind. Tease the trap, never the student. Never say the student was dumb, fell for it, or should have known.
- Every sentence ends with . ! or ?"#;

// ---------------------------------------------------------------- the offline bank

/// Used while exploring (and topped up with the red-flag tips from ai_features.json).
const GENERAL: [&str; 20] = [
    "Hoo! Slow down when a message says act now. A real office can wait a day while you check.",
    "Hoo! Look at who actually sent it, not just the name on top. Names are easy to fake.",
    "Hoo, if a message asks for a password, close it. A real office does not ask for those by message.",
    "Hoo! Unsure about a link? Open the site yourself from a bookmark instead of using theirs.",
    "Hoo, a prize you never entered for is bait. Ask what they want from you in return.",
    "Hoo! Pressure is a tell. A real deadline is still real after you call the office to check.",
    "Hoo, a quick second opinion is free. Show a friend or the help desk before you reply.",
    "Hoo! If it sounds too good to be true, take five minutes and ask someone you trust.",
    "Hoo, typos, odd greetings and strange timing are small clues. Add them up before you trust a message.",
    "Hoo! Use a different password for every account, so one lost key does not open every door.",
    "Hoo, never share a verification code with anyone. It is meant for your eyes only.",
    "Hoo! A job offer with no interview deserves a closer look. Real employers want to meet you first.",
    "Hoo, check details yourself using contact info you already trust, not what the message gives you.",
    "Hoo! If you feel rushed, stop. Scammers love a hurried owl, and a short pause costs you nothing.",
    "Hoo, keep a screenshot of anything suspicious. Showing it to the help desk helps everyone.",
    "Hoo! A big deadline feels smaller in steps. Pick the smallest piece and start it now.",
    "Hoo, a short email to your professor beats a panicked shortcut. Most people appreciate an early heads-up.",
    "Hoo! Take a breath before you tap. One slow look at the sender takes only a few seconds.",
    "Hoo, treat surprise attachments like unlabeled potions. Ask before you open them.",
    "Hoo! Write down what is due and when. A clear list makes pressure tricks easier to spot.",
];

/// After a safe choice.
const NEXT_SAFE: [&str; 5] = [
    "Hoo! Nice move. Next time, keep the same habit: pause, check the sender, then decide.",
    "Hoo, well done. Carry that calm pause onto the next floor, because the traps get sneakier.",
    "Hoo! Good instincts. Say your reasoning out loud next time and it will stick even better.",
    "Hoo, you spotted the trick. Tell a friend about it this week, since explaining makes it stronger.",
    "Hoo! Sharp work. Keep checking details yourself, even when a message looks friendly.",
];

/// After a risky choice.
const NEXT_RISKY: [&str; 5] = [
    "Hoo! Everyone gets tricked sometimes. Next time, pause and look for the pressure first.",
    "Hoo, no shame in that one. Next time, check the sender and ask a second person before you act.",
    "Hoo! That trap is built to be tempting. Next time, take five minutes and verify it another way.",
    "Hoo, learn it and move on. Next time, look for what they want you to hurry or hand over.",
    "Hoo! Reread the warning signs once more. Spotting them early is the whole trick.",
];

/// Every written tip the offline bank can say. The voice generator records these.
pub fn bank_lines() -> Vec<&'static str> {
    GENERAL.into_iter().chain(NEXT_SAFE).chain(NEXT_RISKY).chain(NEXT_MIXED).collect()
}

/// After a mixed choice.
const NEXT_MIXED: [&str; 4] = [
    "Hoo! A decent move, and there is a stronger one. Reread the warning signs and aim for the safest path next time.",
    "Hoo, half right is a start. Next time, add one more check before you act.",
    "Hoo! You were close. Next time, ask a second person before you commit.",
    "Hoo, not bad. Pausing a little longer next time would make this one a clean win.",
];

// ---------------------------------------------------------------- types

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TipKind {
    /// the player pressed T
    Ask,
    /// automatically, when a floor's lesson is revealed
    PostFloor,
}

impl TipKind {
    pub fn tag(self) -> &'static str {
        match self {
            TipKind::Ask => "ASKED",
            TipKind::PostFloor => "FOR NEXT TIME",
        }
    }
}

/// What the player is doing, which decides how much Hoot may say.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TipStage {
    /// walking the floor; has not read the scenario
    Exploring,
    /// reading the scenario and choosing
    Deciding,
    /// the floor's choice is made and its lesson shown
    Reviewed,
}

/// The choice that was made, for a `Reviewed` tip.
#[derive(Clone, Debug)]
pub struct Picked {
    pub label: String,
    pub outcome: Outcome,
    pub warning_signs: Vec<String>,
    pub safer_next_move: String,
}

#[derive(Clone, Debug)]
pub struct TipContext {
    pub kind: TipKind,
    pub stage: TipStage,
    pub floor: usize,
    pub enc: Encounter,
    pub picked: Option<Picked>,
    /// Hoot's written lines about this floor's trap (investigations.json); empty = none
    pub floor_tips: Vec<String>,
}

/// One tip that was shown (or is waiting to be shown).
#[derive(Clone, Debug)]
pub struct Tip {
    pub floor: usize,
    pub kind: TipKind,
    pub text: String,
    pub source: LineSource,
}

// ---------------------------------------------------------------- prompt + checks

/// The user message: facts only. The Deciding stage never includes the choices.
pub fn user_message(ctx: &TipContext, owl: &str) -> String {
    let e = &ctx.enc;
    let mut s = format!("TASK: companion_tip\nowl_name: {owl}\nfloor: {}\nencounter: {}\n", ctx.floor + 1, e.name);
    match ctx.stage {
        TipStage::Exploring if !ctx.floor_tips.is_empty() => s.push_str(&format!(
            "situation: the student is walking this floor, whose trap is {}. Give ONE habit for spotting that kind of trap. Do not describe this floor's message or its choices.\nexample tips for this floor: {}\n",
            e.name,
            ctx.floor_tips.join(" | ")
        )),
        TipStage::Exploring => s.push_str(
            "situation: the student is walking the floor and has not met its character yet. Give ONE general habit for spotting scams or handling deadline pressure. Do not guess what this floor holds.\n",
        ),
        TipStage::Deciding => {
            s.push_str("situation: the student is reading this scenario and deciding. Point at ONE warning sign in it. Never say which choice to pick or what the safe move is.\n");
            s.push_str(&format!("character: {}\nscenario: {}\n", e.character.name, e.scenario));
        }
        TipStage::Reviewed => {
            s.push_str("situation: the student just finished this floor. Give ONE habit for next time.\n");
            if let Some(p) = &ctx.picked {
                s.push_str(&format!("their choice: {}\nresult: {}\n", p.label, p.outcome.as_str()));
                if !p.warning_signs.is_empty() {
                    s.push_str(&format!("warning signs: {}\n", p.warning_signs.join(" | ")));
                }
            }
        }
    }
    s.push_str("\nWrite one or two short sentences, 150 characters or fewer in total, starting with Hoo. Plain text only.");
    s
}

/// Every check an AI tip must pass, or the scripted tip is used instead. The "Hoo"
/// opener is added by code if the model forgot it.
pub fn validate_tip(raw: &str, ctx: &TipContext) -> Option<String> {
    let said = validate::speech(raw);
    let line = validate::game_line(&said, MAX_TIP_CHARS)?;
    let line = hints::with_hoo(&line)?;
    if validate::has_domain_trust(&line) {
        return None;
    }
    match ctx.stage {
        // reading a scenario: same bar as the hesitation hint (no choice names, no giveaways)
        TipStage::Deciding => hints::hint_ok(&line, &ctx.enc).then_some(line),
        TipStage::Exploring | TipStage::Reviewed => Some(line),
    }
}

// ---------------------------------------------------------------- the scripted tip

fn bank(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

/// The general bank plus every red-flag tip from the data file (as an owl line).
fn general_pool(data: &FeatureData) -> Vec<String> {
    let mut v = bank(&GENERAL);
    for f in data.red_flags.values() {
        if let Some(h) = hints::with_hoo(&f.tip) {
            if validate::game_line(&h, MAX_TIP_CHARS).is_some() {
                v.push(h);
            }
        }
    }
    v
}

/// The first pool entry at or after a seeded start that was not already given this run.
fn pick_fresh(pool: &[String], seed: u32, salt: u32, history: &[Tip]) -> String {
    let n = pool.len();
    if n == 0 {
        return DEFAULT_TIP.to_string();
    }
    let start = mix(seed, salt) as usize % n;
    (0..n)
        .map(|i| &pool[(start + i) % n])
        .find(|t| !history.iter().any(|h| &h.text == *t))
        .unwrap_or(&pool[start])
        .clone()
}

/// The offline tip for `ctx`: deterministic for a run seed and the tips given so far.
pub fn scripted_tip(ctx: &TipContext, data: &FeatureData, seed: u32, history: &[Tip]) -> String {
    let salt = 0x7190_u32.wrapping_add((ctx.floor as u32).wrapping_mul(31)).wrapping_add(history.len() as u32);
    match (ctx.stage, &ctx.picked) {
        // the floor's own hints are written for this scenario and already pass `hint_ok`
        (TipStage::Deciding, _) => hints::scripted_hint(data, &ctx.enc.id, seed.wrapping_add(history.len() as u32), ctx.floor),
        (TipStage::Reviewed, Some(p)) => {
            let pool = match p.outcome {
                Outcome::Safe => bank(&NEXT_SAFE),
                Outcome::Risky => bank(&NEXT_RISKY),
                Outcome::Mixed => bank(&NEXT_MIXED),
            };
            pick_fresh(&pool, seed, salt, history)
        }
        // exploring a floor with its own lines: those first, then the general bank
        (TipStage::Exploring, _) if ctx.floor_tips.iter().any(|t| !history.iter().any(|h| &h.text == t)) => pick_fresh(&ctx.floor_tips, seed, salt, history),
        _ => pick_fresh(&general_pool(data), seed, salt, history),
    }
}

// ---------------------------------------------------------------- the desk

enum Wait {
    /// a live call is in flight
    Call(Receiver<CallResult>),
    /// no call: use the scripted tip now; the string says why
    Now(&'static str),
}

struct Pending {
    wait: Wait,
    started: f64,
    ctx: TipContext,
}

enum Step {
    Text(String),
    Why(&'static str),
    Wait,
}

/// Hoot's request queue: at most one tip in flight, never blocks a frame.
pub struct TipDesk {
    config: Option<Arc<Config>>,
    caller: ChatCaller,
    system: Arc<str>,
    owl: String,
    pending: Option<Pending>,
    last_call: f64,
    ai_used: u32,
    seed: u32,
    history: Vec<Tip>,
}

impl TipDesk {
    pub fn new(config: Option<Arc<Config>>, caller: ChatCaller, owl: String) -> TipDesk {
        TipDesk {
            config,
            caller,
            system: Arc::from(TIP_SYSTEM),
            owl,
            pending: None,
            last_call: f64::NEG_INFINITY,
            ai_used: 0,
            seed: 0,
            history: Vec::new(),
        }
    }

    /// New run: forget the log, the budget and anything in flight.
    pub fn begin_run(&mut self, seed: u32) {
        self.seed = seed;
        self.pending = None;
        self.last_call = f64::NEG_INFINITY;
        self.ai_used = 0;
        self.history.clear();
    }

    /// Drop a tip that is still being written (the player moved on).
    pub fn cancel(&mut self) {
        self.pending = None;
    }

    pub fn busy(&self) -> bool {
        self.pending.is_some()
    }

    pub fn log(&self) -> &[Tip] {
        &self.history
    }

    pub fn calls_used(&self) -> u32 {
        self.ai_used
    }

    /// Start a tip. An asked tip is refused (false) while another is in flight; a
    /// post-floor tip replaces it. Offline, over budget or inside the cooldown, the
    /// scripted tip is used (it arrives on the next `poll`).
    pub fn ask(&mut self, ctx: TipContext, now: f64) -> bool {
        if self.pending.is_some() && ctx.kind == TipKind::Ask {
            return false;
        }
        let wait = match self.config.clone() {
            None => Wait::Now("offline"),
            Some(_) if self.ai_used >= MAX_AI_TIPS_PER_RUN => Wait::Now("tip budget used"),
            Some(_) if ctx.kind == TipKind::Ask && now - self.last_call < ASK_COOLDOWN_SECS => Wait::Now("cooling down"),
            Some(cfg) => {
                let (tx, rx) = channel();
                let system = Arc::clone(&self.system);
                let user = user_message(&ctx, &self.owl);
                let caller = self.caller;
                // if the OS refuses a thread, `tx` is dropped with the closure and
                // `poll` sees a disconnected channel: scripted tip, no panic
                let spawned = std::thread::Builder::new().name("ai-tip".into()).spawn(move || {
                    let _ = tx.send(caller(&cfg, &system, &user, TIP_TOKENS));
                });
                if let Err(e) = spawned {
                    eprintln!("[tips] could not start request thread: {e}");
                }
                self.ai_used += 1;
                self.last_call = now;
                Wait::Call(rx)
            }
        };
        self.pending = Some(Pending { wait, started: now, ctx });
        true
    }

    /// The finished tip, exactly once per request (non-blocking).
    pub fn poll(&mut self, data: &FeatureData, now: f64) -> Option<Tip> {
        let p = self.pending.take()?;
        let step = match &p.wait {
            Wait::Now(w) => Step::Why(w),
            Wait::Call(rx) => match rx.try_recv() {
                Ok(Ok(raw)) => match validate_tip(&raw, &p.ctx) {
                    Some(t) => Step::Text(t),
                    None => Step::Why("AI line rejected by filter"),
                },
                Ok(Err(AiError::Offline)) => Step::Why("offline"),
                Ok(Err(AiError::Failed(e))) => {
                    eprintln!("[tips] {e}");
                    Step::Why("AI unavailable")
                }
                Err(TryRecvError::Disconnected) => Step::Why("AI unavailable"),
                Err(TryRecvError::Empty) if now - p.started >= TIMEOUT_SECS => Step::Why("AI timed out"),
                Err(TryRecvError::Empty) => Step::Wait,
            },
        };
        match step {
            Step::Wait => {
                self.pending = Some(p);
                None
            }
            Step::Text(t) => Some(self.record(&p.ctx, t, LineSource::Ai)),
            Step::Why(w) => {
                let t = scripted_tip(&p.ctx, data, self.seed, &self.history);
                Some(self.record(&p.ctx, t, LineSource::Fallback(w)))
            }
        }
    }

    fn record(&mut self, ctx: &TipContext, text: String, source: LineSource) -> Tip {
        let tip = Tip { floor: ctx.floor, kind: ctx.kind, text, source };
        self.history.push(tip.clone());
        if self.history.len() > MAX_LOG {
            self.history.remove(0);
        }
        tip
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai_features::data;
    use crate::content::{parse, EMBEDDED_ENCOUNTERS};
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::time::Duration;

    fn enc() -> Encounter {
        parse(EMBEDDED_ENCOUNTERS).unwrap().0.encounters[0].clone()
    }

    fn ctx(kind: TipKind, stage: TipStage) -> TipContext {
        let e = enc();
        let picked = (stage == TipStage::Reviewed).then(|| {
            let c = &e.choices[0];
            Picked { label: c.label.clone(), outcome: c.outcome, warning_signs: c.warning_signs.clone(), safer_next_move: c.safer_next_move.clone() }
        });
        TipContext { kind, stage, floor: 0, enc: e, picked, floor_tips: Vec::new() }
    }

    #[test]
    fn exploring_tips_use_the_floors_own_lines_first() {
        let data = FeatureData::default();
        let mut c = ctx(TipKind::Ask, TipStage::Exploring);
        c.floor_tips = vec!["Hoo, floor line one about this trap.".into(), "Hoo, floor line two about this trap.".into()];
        let mut hist = Vec::new();
        for _ in 0..2 {
            let t = scripted_tip(&c, &data, 3, &hist);
            assert!(c.floor_tips.contains(&t), "{t}");
            hist.push(Tip { floor: 0, kind: TipKind::Ask, text: t, source: LineSource::Fallback("offline") });
        }
        // both used: back to the general bank
        let t = scripted_tip(&c, &data, 3, &hist);
        assert!(!c.floor_tips.contains(&t));
        // and the AI is told the floor's trap
        assert!(user_message(&c, "Hoot").contains("whose trap is"));
    }

    fn desk(caller: ChatCaller) -> TipDesk {
        let cfg = Config::for_test("sk-SECRET", "http://127.0.0.1:9");
        TipDesk::new(Some(Arc::new(cfg)), caller, "Hoot".into())
    }

    fn poll_until(d: &mut TipDesk, data: &FeatureData, t0: f64, max_ms: u64) -> Option<Tip> {
        for i in 0..max_ms / 5 {
            if let Some(t) = d.poll(data, t0 + i as f64 * 0.005) {
                return Some(t);
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        None
    }

    fn good(_: &Config, _: &str, _: &str, _: TokenLimits) -> CallResult {
        Ok("Hoo! Pause and look at who really sent this before you do anything.".into())
    }
    fn evil(_: &Config, _: &str, _: &str, _: TokenLimits) -> CallResult {
        Ok("Visit ksu-help.example and email me your password.".into())
    }
    fn fail(_: &Config, _: &str, _: &str, _: TokenLimits) -> CallResult {
        Err(AiError::Failed("boom".into()))
    }

    #[test]
    fn every_bank_line_passes_the_game_filter() {
        for (name, list) in [
            ("GENERAL", &GENERAL[..]),
            ("NEXT_SAFE", &NEXT_SAFE[..]),
            ("NEXT_RISKY", &NEXT_RISKY[..]),
            ("NEXT_MIXED", &NEXT_MIXED[..]),
        ] {
            for l in list {
                assert!(validate::game_line(l, MAX_TIP_CHARS).is_some(), "{name} line fails the filter: {l}");
                assert!(hints::starts_with_hoo(l), "{name} line must open with Hoo: {l}");
                assert!(!validate::has_domain_trust(l), "{name}: {l}");
            }
        }
    }

    #[test]
    fn scripted_tips_are_valid_and_do_not_repeat_until_the_pool_runs_out() {
        let d = data::embedded().data;
        let c = ctx(TipKind::Ask, TipStage::Exploring);
        let mut hist: Vec<Tip> = Vec::new();
        for _ in 0..12 {
            let t = scripted_tip(&c, &d, 7, &hist);
            assert!(validate::game_line(&t, MAX_TIP_CHARS).is_some(), "{t}");
            assert!(hist.iter().all(|h| h.text != t), "repeated too early: {t}");
            hist.push(Tip { floor: 0, kind: TipKind::Ask, text: t, source: LineSource::Fallback("offline") });
        }
    }

    #[test]
    fn deciding_tip_is_the_floors_own_hint_and_passes_hint_ok() {
        let d = data::embedded().data;
        let c = ctx(TipKind::Ask, TipStage::Deciding);
        let t = scripted_tip(&c, &d, 3, &[]);
        assert!(hints::hint_ok(&t, &c.enc), "{t}");
    }

    #[test]
    fn reviewed_tip_follows_the_outcome() {
        let d = data::embedded().data;
        let mut c = ctx(TipKind::PostFloor, TipStage::Reviewed);
        c.picked.as_mut().unwrap().outcome = Outcome::Safe;
        assert!(NEXT_SAFE.contains(&scripted_tip(&c, &d, 1, &[]).as_str()));
        c.picked.as_mut().unwrap().outcome = Outcome::Risky;
        assert!(NEXT_RISKY.contains(&scripted_tip(&c, &d, 1, &[]).as_str()));
        c.picked.as_mut().unwrap().outcome = Outcome::Mixed;
        assert!(NEXT_MIXED.contains(&scripted_tip(&c, &d, 1, &[]).as_str()));
    }

    #[test]
    fn validate_adds_hoo_and_rejects_bad_text() {
        let c = ctx(TipKind::Ask, TipStage::Exploring);
        assert_eq!(validate_tip("Check the sender before you reply to anything.", &c).as_deref(), Some("Hoo! Check the sender before you reply to anything."));
        assert_eq!(validate_tip("**Hoo!** Check the sender before you reply to anything.", &c).as_deref(), Some("Hoo! Check the sender before you reply to anything."));
        for bad in [
            "Hoo! Visit ksu-help.example to verify.",
            "Hoo! Mail help@school.example for a fix.",
            "Hoo! Call 555 123 4567 right now to confirm.",
            "Hoo, this habit will prevent every scam for you.",
            "Hoo! Your health is low, so be careful out there.",
            "Hoo!",
            "Hoo, if the address ends in .edu it is official and safe.",
        ] {
            assert!(validate_tip(bad, &c).is_none(), "should reject: {bad}");
        }
    }

    #[test]
    fn deciding_tips_may_not_give_the_answer() {
        let c = ctx(TipKind::Ask, TipStage::Deciding);
        let label = c.enc.choices[0].label.clone();
        assert!(validate_tip("Hoo, the answer is to check the sender first.", &c).is_none());
        assert!(validate_tip("Hoo, you should pick the careful option here.", &c).is_none());
        assert!(validate_tip(&format!("Hoo! Try this: {label}. It sounds right to me."), &c).is_none());
        assert!(validate_tip("Hoo, does that sender really belong to someone you know?", &c).is_some());
    }

    #[test]
    fn reviewed_tips_may_talk_about_the_choice_made() {
        let c = ctx(TipKind::PostFloor, TipStage::Reviewed);
        let label = c.picked.as_ref().unwrap().label.clone();
        let line = format!("Hoo! Next time, think twice before you try {label} again.");
        // the label may be long; only assert it is not rejected for naming it
        if line.len() <= MAX_TIP_CHARS {
            assert!(validate_tip(&line, &c).is_some(), "{line}");
        }
    }

    #[test]
    fn user_message_never_leaks_choices_while_deciding() {
        let c = ctx(TipKind::Ask, TipStage::Deciding);
        let m = user_message(&c, "Hoot");
        assert!(m.starts_with("TASK: companion_tip"));
        for ch in &c.enc.choices {
            // a label that the scenario itself happens to contain would be a false alarm
            if c.enc.scenario.contains(&ch.label) || c.enc.scenario.contains(&ch.safer_next_move) {
                continue;
            }
            assert!(!m.contains(&ch.label), "choice leaked: {}", ch.label);
            assert!(ch.safer_next_move.is_empty() || !m.contains(&ch.safer_next_move), "safe move leaked");
        }
        let x = user_message(&ctx(TipKind::Ask, TipStage::Exploring), "Hoot");
        assert!(!x.contains(&c.enc.scenario), "exploring must not describe the scenario");
    }

    #[test]
    fn offline_desk_answers_with_the_bank_at_once() {
        let d = data::embedded().data;
        let mut t = TipDesk::new(None, good, "Hoot".into());
        assert!(t.ask(ctx(TipKind::Ask, TipStage::Exploring), 0.0));
        assert!(t.busy());
        let tip = t.poll(&d, 0.0).expect("scripted tip");
        assert_eq!(tip.source, LineSource::Fallback("offline"));
        assert!(validate::game_line(&tip.text, MAX_TIP_CHARS).is_some());
        assert!(!t.busy());
        assert_eq!(t.log().len(), 1);
        assert!(t.poll(&d, 0.1).is_none(), "a decided tip is returned once");
    }

    #[test]
    fn good_ai_tip_is_used_and_logged() {
        let d = data::embedded().data;
        let mut t = desk(good);
        assert!(t.ask(ctx(TipKind::Ask, TipStage::Exploring), 0.0));
        let tip = poll_until(&mut t, &d, 0.0, 2000).expect("tip");
        assert_eq!(tip.source, LineSource::Ai);
        assert_eq!(tip.text, "Hoo! Pause and look at who really sent this before you do anything.");
        assert_eq!(t.log().len(), 1);
        assert_eq!(t.calls_used(), 1);
    }

    #[test]
    fn rejected_or_failed_ai_falls_back_to_the_bank() {
        let d = data::embedded().data;
        let mut t = desk(evil);
        t.ask(ctx(TipKind::Ask, TipStage::Exploring), 0.0);
        let tip = poll_until(&mut t, &d, 0.0, 2000).expect("tip");
        assert_eq!(tip.source, LineSource::Fallback("AI line rejected by filter"));
        assert!(validate::game_line(&tip.text, MAX_TIP_CHARS).is_some());

        let mut t = desk(fail);
        t.ask(ctx(TipKind::Ask, TipStage::Exploring), 0.0);
        assert_eq!(poll_until(&mut t, &d, 0.0, 2000).map(|x| x.source), Some(LineSource::Fallback("AI unavailable")));
    }

    #[test]
    fn slow_call_times_out_without_blocking() {
        fn slow(_: &Config, _: &str, _: &str, _: TokenLimits) -> CallResult {
            std::thread::sleep(Duration::from_millis(400));
            Ok("Hoo! This answer comes far too late to be used.".into())
        }
        let d = data::embedded().data;
        let mut t = desk(slow);
        t.ask(ctx(TipKind::Ask, TipStage::Exploring), 10.0);
        let started = std::time::Instant::now();
        assert!(t.poll(&d, 10.0).is_none());
        assert!(t.poll(&d, 10.0 + TIMEOUT_SECS - 0.01).is_none());
        assert!(started.elapsed() < Duration::from_millis(50), "poll must never block");
        assert!(t.busy());
        let tip = t.poll(&d, 10.0 + TIMEOUT_SECS).expect("fallback");
        assert_eq!(tip.source, LineSource::Fallback("AI timed out"));
    }

    #[test]
    fn asking_while_busy_is_refused_but_post_floor_replaces() {
        let d = data::embedded().data;
        let mut t = desk(good);
        assert!(t.ask(ctx(TipKind::Ask, TipStage::Exploring), 0.0));
        assert!(!t.ask(ctx(TipKind::Ask, TipStage::Exploring), 0.1), "second ask while busy");
        assert!(t.ask(ctx(TipKind::PostFloor, TipStage::Reviewed), 0.2), "post-floor replaces");
        let tip = poll_until(&mut t, &d, 0.2, 2000).expect("tip");
        assert_eq!(tip.kind, TipKind::PostFloor);
        assert!(t.poll(&d, 5.0).is_none(), "the replaced request never answers");
    }

    #[test]
    fn cooldown_and_budget_use_the_bank_without_calling_the_api() {
        static CALLS: AtomicU32 = AtomicU32::new(0);
        fn counted(_: &Config, _: &str, _: &str, _: TokenLimits) -> CallResult {
            CALLS.fetch_add(1, Ordering::SeqCst);
            Ok("Hoo! Pause and look at who really sent this before you do anything.".into())
        }
        let d = data::embedded().data;
        let mut t = desk(counted);
        t.begin_run(1);
        t.ask(ctx(TipKind::Ask, TipStage::Exploring), 0.0);
        assert_eq!(poll_until(&mut t, &d, 0.0, 2000).map(|x| x.source), Some(LineSource::Ai));
        // inside the cooldown
        t.ask(ctx(TipKind::Ask, TipStage::Exploring), 1.0);
        let cooled = t.poll(&d, 1.0).expect("scripted");
        assert_eq!(cooled.source, LineSource::Fallback("cooling down"));
        assert_eq!(CALLS.load(Ordering::SeqCst), 1);
        // a post-floor tip ignores the cooldown (it is automatic, not spammable)
        t.ask(ctx(TipKind::PostFloor, TipStage::Reviewed), 1.5);
        assert_eq!(poll_until(&mut t, &d, 1.5, 2000).map(|x| x.source), Some(LineSource::Ai));
        // budget
        let mut now = 100.0;
        while t.calls_used() < MAX_AI_TIPS_PER_RUN {
            t.ask(ctx(TipKind::PostFloor, TipStage::Reviewed), now);
            poll_until(&mut t, &d, now, 2000);
            now += 10.0;
        }
        t.ask(ctx(TipKind::Ask, TipStage::Exploring), now + 100.0);
        let capped = t.poll(&d, now + 100.0).expect("scripted");
        assert_eq!(capped.source, LineSource::Fallback("tip budget used"));
        // one post-floor tip per floor except the last still fits, so floors
        // 1-19 are not forced onto the scripted bank by the cap alone
        const { assert!(MAX_AI_TIPS_PER_RUN >= 19); }
    }

    #[test]
    fn begin_run_resets_the_log_and_the_budget() {
        let d = data::embedded().data;
        let mut t = TipDesk::new(None, good, "Hoot".into());
        t.ask(ctx(TipKind::Ask, TipStage::Exploring), 0.0);
        t.poll(&d, 0.0);
        assert_eq!(t.log().len(), 1);
        t.ask(ctx(TipKind::Ask, TipStage::Exploring), 1.0);
        t.begin_run(9);
        assert!(!t.busy());
        assert!(t.log().is_empty());
        assert_eq!(t.calls_used(), 0);
    }

    #[test]
    fn log_is_capped() {
        let d = data::embedded().data;
        let mut t = TipDesk::new(None, good, "Hoot".into());
        for i in 0..MAX_LOG + 5 {
            t.ask(ctx(TipKind::PostFloor, TipStage::Reviewed), i as f64);
            t.poll(&d, i as f64);
        }
        assert_eq!(t.log().len(), MAX_LOG);
    }

    #[test]
    fn errors_never_contain_the_key() {
        // the desk only ever logs the AiError text, and the client never puts the key in it
        let d = data::embedded().data;
        let mut t = desk(fail);
        t.ask(ctx(TipKind::Ask, TipStage::Exploring), 0.0);
        let tip = poll_until(&mut t, &d, 0.0, 2000).expect("tip");
        assert!(!format!("{tip:?}").contains("SECRET"));
    }
}
