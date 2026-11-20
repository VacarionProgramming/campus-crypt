//! AI flavor features (backend). See /workspace/campus-crypt-plan/AI_FEATURES_INTEGRATION.md.
//!
//! 1. Live scam generator (`scamgen`)      4. Spot-the-fake (`spotfake`)
//! 2. Floor Keeper banter (`banter`)        5. Owl companion hints (`hints`)
//! 3. Post-run debrief (`debrief`)          6. Adaptive difficulty narration (`difficulty`)
//!
//! Guardrails: game state stays deterministic. Code picks every template, red
//! flag, verdict, label, tier and mistake; the AI only writes prose around them,
//! and every AI text has a scripted fallback, so everything works with
//! `CAMPUS_CRYPT_AI=off`, no key, no network, and on the web build.
//!
//! Call budget per run (all prefetched in the background, spaced `MIN_GAP_SECS`
//! apart, capped at `MAX_CALLS_PER_RUN`): run start = floor 0 pack + 3 scam bodies
//! + tier narration pack (5 calls); each later floor = 1 floor pack; run end =
//!   1 debrief. A 10-floor run uses 15. The character reaction (ai::Reactor, 1 per
//!   floor) is separate and unchanged.

// The UI wiring lands later (Darby); until then only tests call most of this.
#![allow(dead_code)]

pub mod banter;
pub mod client;
pub mod data;
pub mod debrief;
pub mod difficulty;
pub mod hints;
pub mod scamgen;
pub mod spotfake;
pub mod validate;

use crate::ai::{self, ChatCaller, Config, LineSource, TokenLimits};
use crate::content::{Content, Encounter};
use crate::state::{encounter_for_floor, Run};
use client::{FeatureClient, Got, JobKey};
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

pub use banter::FloorPack;
pub use debrief::{Debrief, FeatureLog, MistakeLog};
pub use difficulty::{SkillTier, TierChange};
pub use scamgen::{RedFlag, ScamMessage};
pub use spotfake::SpotRound;

/// Hard cap on feature calls per run (the last one is reserved for the debrief).
pub const MAX_CALLS_PER_RUN: u32 = 16;
/// Minimum time between two feature calls (Groq free-tier friendly).
pub const MIN_GAP_SECS: f64 = 3.0;
/// How long the ending screen waits for the AI debrief before the templated one.
pub const DEBRIEF_WAIT_SECS: f64 = MIN_GAP_SECS + ai::TIMEOUT_SECS + 1.0;

const SCAM_TOKENS: TokenLimits = TokenLimits(900, 200);
const PACK_TOKENS: TokenLimits = TokenLimits(1000, 260);
const DEBRIEF_TOKENS: TokenLimits = TokenLimits(1100, 320);

/// Why a slot has no AI text yet.
fn why(g: &Got) -> &'static str {
    match g {
        Got::Failed(w) => w,
        Got::Pending(_) => "AI not ready",
        Got::Missing | Got::Ready(_) => "offline",
    }
}

pub struct AiFeatures {
    client: FeatureClient,
    pub data: data::FeatureData,
    prompts: [Arc<str>; 4],
    seed: u32,
    /// the encounter each prefetched floor pack was written for
    floor_enc: BTreeMap<usize, Encounter>,
    pub log: FeatureLog,
    pub hints: hints::HintTracker,
    tier: SkillTier,
    debrief: Option<(MistakeLog, f64)>,
    debrief_done: Option<Debrief>,
    status: String,
}

impl AiFeatures {
    /// Live content files + env config (`CAMPUS_CRYPT_AI=off` / no key = scripted).
    pub fn new() -> AiFeatures {
        AiFeatures::with(Config::from_env().map(Arc::new), data::load(), ai::call_chat, Duration::from_secs_f64(MIN_GAP_SECS))
    }

    fn with(config: Option<Arc<Config>>, assets: data::Assets, caller: ChatCaller, gap: Duration) -> AiFeatures {
        let client = FeatureClient::new(config, caller, gap, MAX_CALLS_PER_RUN);
        let status = Self::status_for(&client, true);
        let p = assets.prompts;
        AiFeatures {
            client,
            data: assets.data,
            prompts: [p.scam_body.into(), p.floor_pack.into(), p.run_pack.into(), p.debrief.into()],
            seed: 0,
            floor_enc: BTreeMap::new(),
            log: FeatureLog::default(),
            hints: hints::HintTracker::default(),
            tier: SkillTier::START,
            debrief: None,
            debrief_done: None,
            status,
        }
    }

    pub fn status(&self) -> &str {
        &self.status
    }

    fn status_for(client: &FeatureClient, on: bool) -> String {
        match (client.model(), on) {
            (Some(m), true) => format!("AI extras: on ({m})"),
            (Some(_), false) => "AI extras: off (Options)".into(),
            (None, _) => "AI extras: offline (scripted)".into(),
        }
    }

    /// Options > AI extras: off = scripted scams, banter, hints and study guide, no calls.
    pub fn set_enabled(&mut self, on: bool) {
        self.client.set_enabled(on);
        self.status = Self::status_for(&self.client, on);
    }

    /// An API key is set (so the AI switch does something).
    pub fn has_key(&self) -> bool {
        self.client.model().is_some()
    }
    pub fn calls_used(&self) -> u32 {
        self.client.calls_used()
    }

    /// Start of a run: reset and prefetch floor 0's pack, the scam pool and the
    /// tier narration (5 calls, spaced out in the background).
    pub fn begin_run(&mut self, content: &Content, run_seed: u32, now: f64) {
        self.client.begin_run();
        self.seed = run_seed;
        self.floor_enc.clear();
        self.log = FeatureLog::default();
        self.hints.reset();
        self.tier = SkillTier::START;
        self.hints.after_secs = self.tier.params().hint_after_secs;
        self.debrief = None;
        self.debrief_done = None;
        self.begin_floor(content, 0, now);
        for slot in 0..scamgen::POOL_SIZE {
            let p = scamgen::plan(&self.data, self.seed, slot);
            let user = scamgen::user_message(&self.data, &p);
            self.client.submit(JobKey::Scam(slot), Arc::clone(&self.prompts[0]), user, SCAM_TOKENS, now);
            if slot == 0 {
                self.client.submit(JobKey::RunPack, Arc::clone(&self.prompts[2]), "TASK: run_pack\nWrite the six lines now.".into(), PACK_TOKENS, now);
            }
        }
    }

    /// A floor starts (call on every new floor, including 0; repeats are free):
    /// prefetch its Keeper banter + owl hint.
    pub fn begin_floor(&mut self, content: &Content, floor: usize, now: f64) {
        let Some(enc) = content.encounters.get(encounter_for_floor(floor, content.encounters.len())) else { return };
        if self.floor_enc.contains_key(&floor) {
            return;
        }
        let user = banter::user_message(floor, enc, &self.data.owl_name);
        self.floor_enc.insert(floor, enc.clone());
        self.client.submit(JobKey::Floor(floor), Arc::clone(&self.prompts[1]), user, PACK_TOKENS, now);
    }

    /// Collect finished calls. Cheap; call once per frame.
    pub fn update(&mut self) {
        self.client.pump();
    }

    fn floor_pack(&self, floor: usize) -> (Option<FloorPack>, &'static str) {
        let g = self.client.get(JobKey::Floor(floor));
        match (&g, self.floor_enc.get(&floor)) {
            (Got::Ready(raw), Some(enc)) => (Some(banter::parse_floor_pack(raw, enc)), ""),
            _ => (None, why(&g)),
        }
    }

    // ---- 1. live scam generator

    /// The scam message for pool slot `slot` (AI prose if it arrived and passed,
    /// else scripted; flags always injected by code). Slots past the pool are scripted.
    pub fn scam_message(&self, slot: usize) -> ScamMessage {
        let p = scamgen::plan(&self.data, self.seed, slot);
        let g = self.client.get(JobKey::Scam(slot));
        let raw = if let Got::Ready(r) = g { Some(r) } else { None };
        let w = if slot >= scamgen::POOL_SIZE { "scripted slot" } else { why(&g) };
        scamgen::build(&self.data, &p, raw, w)
    }

    /// The player judged a generated message. Generated messages are always scams,
    /// so `said_scam` is the correct answer. Logged for the debrief.
    pub fn record_scam_verdict(&mut self, floor: usize, msg: &ScamMessage, said_scam: bool) -> bool {
        let f = msg.flags.first().copied().unwrap_or(RedFlag::Urgency);
        let (label, tip) = (debrief::flag_label(&self.data, f), debrief::flag_tip(&self.data, f));
        // the display name only: the fictional address would crowd the debrief line
        let who = msg.from.split(" <").next().unwrap_or(&msg.from);
        self.log.record_scam(floor, who, !said_scam, &label, &tip);
        said_scam
    }

    // ---- 4. spot the fake

    /// Round `round` of this run: scam pool slot `round` vs a scripted safe message.
    pub fn spot_round(&self, round: usize) -> SpotRound {
        spotfake::build(&self.data, self.seed, round, &self.scam_message(round))
    }

    /// Judge the pick with code's label and log it. Returns true if right.
    pub fn answer_spot(&mut self, floor: usize, round: &SpotRound, picked: usize) -> bool {
        let ok = round.is_fake(picked);
        let f = round.fake_flags.first().copied().unwrap_or(RedFlag::MismatchedSender);
        let (label, tip) = (debrief::flag_label(&self.data, f), debrief::flag_tip(&self.data, f));
        self.log.record_spot(floor, ok, &label, &tip);
        ok
    }

    // ---- 2. Floor Keeper banter

    pub fn keeper_taunt(&self, floor: usize) -> (String, LineSource) {
        let (pack, w) = self.floor_pack(floor);
        let enc_id = self.floor_enc.get(&floor).map(|e| e.id.as_str()).unwrap_or("default");
        banter::taunt(pack.as_ref(), &self.data, enc_id, self.seed, floor, w)
    }

    /// `correct` from `Quiz::is_correct`. The line always agrees with it.
    pub fn keeper_reaction(&self, floor: usize, correct: bool) -> (String, LineSource) {
        let (pack, w) = self.floor_pack(floor);
        banter::reaction(pack.as_ref(), correct, floor, w)
    }

    // ---- 5. owl hints (code decides when)

    /// Call every frame while the encounter screen is open (after `hints.start`).
    /// Returns the hint once, when the player has hesitated long enough.
    pub fn poll_hint(&mut self, now: f64) -> Option<(String, LineSource)> {
        let floor = self.hints.due(now)?;
        let (pack, w) = self.floor_pack(floor);
        let enc_id = self.floor_enc.get(&floor).map(|e| e.id.clone()).unwrap_or_else(|| "default".into());
        Some(match pack.and_then(|p| p.hint) {
            Some(h) => (h, LineSource::Ai),
            None => (hints::scripted_hint(&self.data, &enc_id, self.seed, floor), LineSource::Fallback(if w.is_empty() { "AI line rejected by filter" } else { w })),
        })
    }

    // ---- 6. adaptive difficulty

    pub fn tier(&self) -> SkillTier {
        self.tier
    }

    /// A floor was cleared (call right after `Run::descend`, before `begin_floor`).
    /// Code recomputes the tier; returns the narration if it changed.
    pub fn on_floor_cleared(&mut self, run: &Run, floor: usize) -> Option<TierChange> {
        let to = difficulty::next_tier(self.tier, &difficulty::Performance::from_run(run));
        let from = std::mem::replace(&mut self.tier, to);
        self.hints.after_secs = to.params().hint_after_secs;
        let key = difficulty::change_key(from, to)?;
        let g = self.client.get(JobKey::RunPack);
        let ai = if let Got::Ready(raw) = g { difficulty::parse_run_pack(raw).into_iter().find(|(k, _)| *k == key).map(|(_, l)| l) } else { None };
        let (line, source) = match ai {
            Some(l) => (l, LineSource::Ai),
            None => (
                difficulty::scripted_narration(&self.data, key, self.seed, floor),
                LineSource::Fallback(if matches!(g, Got::Ready(_)) { "AI line rejected by filter" } else { why(&g) }),
            ),
        };
        Some(TierChange { from, to, line, source })
    }

    // ---- 3. debrief

    /// Run over: build the mistake log (deterministic) and ask for the study guide.
    pub fn request_debrief(&mut self, run: &Run, content: &Content, now: f64) {
        let log = MistakeLog::from_run(run, content, &self.log);
        self.client.submit(JobKey::Debrief, Arc::clone(&self.prompts[3]), log.user_message(), DEBRIEF_TOKENS, now);
        self.debrief = Some((log, now));
        self.debrief_done = None;
    }

    /// None while waiting (at most `DEBRIEF_WAIT_SECS`); then the same Debrief every call.
    pub fn poll_debrief(&mut self, content: &Content, now: f64) -> Option<Debrief> {
        if let Some(d) = &self.debrief_done {
            return Some(d.clone());
        }
        let (log, asked) = self.debrief.as_ref()?;
        let g = self.client.get(JobKey::Debrief);
        let done = match g {
            Got::Ready(raw) => match debrief::parse_ai(raw, log, content) {
                Some(lines) => Debrief { lines, source: LineSource::Ai },
                None => Debrief { lines: debrief::fallback(log, &self.data), source: LineSource::Fallback("AI line rejected by filter") },
            },
            Got::Pending(_) if now - asked < DEBRIEF_WAIT_SECS => return None,
            Got::Pending(_) => Debrief { lines: debrief::fallback(log, &self.data), source: LineSource::Fallback("AI timed out") },
            g => Debrief { lines: debrief::fallback(log, &self.data), source: LineSource::Fallback(why(&g)) },
        };
        self.debrief_done = Some(done.clone());
        Some(done)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::{AiError, CallResult};
    use crate::content::{parse, Outcome, EMBEDDED_ENCOUNTERS};
    use crate::state::{self, Phase};

    fn content() -> Content {
        parse(EMBEDDED_ENCOUNTERS).unwrap().0
    }

    /// A fake model: answers each task with a well-behaved reply.
    fn good_model(_: &Config, _: &str, user: &str, _: TokenLimits) -> CallResult {
        let t = user.lines().next().unwrap_or("");
        Ok(match t {
            "TASK: floor_pack" => "TAUNT: Welcome to my stairs, little scholar. Let us see what stuck.\nRIGHT: Sharp thinking, Owl. I step aside and the stairs are open.\nWRONG: Not quite, Owl. My two wardens would like a word first.\nHINT: Hoo, does that sender really belong to an office you know?".into(),
            "TASK: run_pack" => "UP_STANDARD: The crypt stops going easy and the torches burn hotter.\nUP_SHARP: The crypt notices your sharp eyes and sends sneakier tricks.\nUP_EXPERT: The crypt senses you are getting cocky and wakes its oldest tricks.\nDOWN_GENTLE: The crypt eases off and lets you catch your breath.\nDOWN_STANDARD: The crypt loosens its grip a little for now.\nDOWN_SHARP: The crypt backs off a step but keeps watching closely.".into(),
            "TASK: debrief" => "- What a run, Owl, you made it through the crypt in style.\n- You kept a cool head when messages tried to rush you.\n- Keep checking senders yourself before you reply to anything.".into(),
            _ => "Hi Owl, great news about your account is waiting for you. We only need a quick confirmation from you to finish things up.".into(),
        })
    }
    fn offline_model(_: &Config, _: &str, _: &str, _: TokenLimits) -> CallResult {
        Err(AiError::Offline)
    }
    /// A misbehaving model: links, wrong verdicts, giveaways, foreign floors.
    fn evil_model(_: &Config, _: &str, user: &str, _: TokenLimits) -> CallResult {
        let t = user.lines().next().unwrap_or("");
        Ok(match t {
            "TASK: floor_pack" => "TAUNT: Correct answers only, Owl.\nRIGHT: Wrong! My wardens wake.\nWRONG: Correct, the stairs are yours.\nHINT: Hoo, the answer is Inspect sender.".into(),
            "TASK: run_pack" => "UP_SHARP: The crypt eases off and gets gentler.".into(),
            "TASK: debrief" => "- Floor 9 was rough.\n- The Password Hydra got you.\n- You lost health.".into(),
            _ => "Visit ksu-aid.example today and email help@freemail.example your password.".into(),
        })
    }

    fn features(caller: ChatCaller) -> AiFeatures {
        let cfg = Config::for_test("sk-SECRET", "http://127.0.0.1:9");
        AiFeatures::with(Some(Arc::new(cfg)), data::embedded(), caller, Duration::ZERO)
    }

    fn settle(f: &mut AiFeatures) {
        for _ in 0..200 {
            f.update();
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn offline_everything_is_scripted_and_playable() {
        let c = content();
        let mut f = AiFeatures::with(None, data::embedded(), ai::call_chat, Duration::ZERO);
        assert_eq!(f.status(), "AI extras: offline (scripted)");
        f.begin_run(&c, 77, 0.0);
        assert_eq!(f.calls_used(), 0);
        let m = f.scam_message(0);
        assert!(scamgen::verify(&m) && m.source == LineSource::Fallback("offline"));
        let r = f.spot_round(1);
        assert!(f.answer_spot(1, &r, r.fake_index()));
        assert_eq!(f.keeper_taunt(0).1, LineSource::Fallback("offline"));
        assert_eq!(f.keeper_reaction(0, false).0, crate::keeper::line(false, 0));
        f.hints.start(0, 0.0);
        assert!(f.poll_hint(1.0).is_none());
        let (h, src) = f.poll_hint(100.0).unwrap();
        assert!(hints::starts_with_hoo(&h) && src == LineSource::Fallback("offline"));
        let n = c.encounters.len();
        let run = Run::new(state::floors_for(n), n);
        f.request_debrief(&run, &c, 0.0);
        let d = f.poll_debrief(&c, 0.0).unwrap();
        assert_eq!(d.source, LineSource::Fallback("offline"));
        assert!((debrief::MIN_LINES..=debrief::MAX_LINES).contains(&d.lines.len()));
        // a model that says Offline behaves the same
        let mut g = features(offline_model);
        g.begin_run(&c, 77, 0.0);
        settle(&mut g);
        assert_eq!(g.scam_message(0).body, f.scam_message(0).body, "same seed, same scripted message");
    }

    #[test]
    fn good_model_lines_are_used_and_budget_is_respected() {
        let c = content();
        let mut f = features(good_model);
        f.begin_run(&c, 5, 0.0);
        assert_eq!(f.calls_used(), 5, "floor 0 + 3 scams + run pack");
        settle(&mut f);
        assert_eq!(f.keeper_taunt(0).1, LineSource::Ai);
        assert_eq!(f.keeper_reaction(0, true), ("Sharp thinking, Owl. I step aside and the stairs are open.".into(), LineSource::Ai));
        assert_eq!(f.keeper_reaction(0, false).1, LineSource::Ai);
        let m = f.scam_message(0);
        assert_eq!(m.source, LineSource::Ai);
        assert!(scamgen::verify(&m), "{m:#?}");
        f.hints.start(0, 0.0);
        assert_eq!(f.poll_hint(60.0).map(|h| h.1), Some(LineSource::Ai));
        // a full 10-floor run: one pack per floor + debrief stays under the cap
        for floor in 1..state::RUN_FLOORS {
            f.begin_floor(&c, floor, floor as f64);
            f.begin_floor(&c, floor, floor as f64); // repeat is free
        }
        let n = c.encounters.len();
        let run = Run::new(state::floors_for(n), n);
        f.request_debrief(&run, &c, 100.0);
        assert_eq!(f.calls_used(), 5 + 9 + 1);
        assert!(f.calls_used() <= MAX_CALLS_PER_RUN);
        settle(&mut f);
        let d = f.poll_debrief(&c, 101.0).unwrap();
        assert_eq!((d.lines.len(), d.source.clone()), (3, LineSource::Ai));
        assert_eq!(f.poll_debrief(&c, 500.0).unwrap(), d, "stable once decided");
        // floor 9 uses encounter 9 of the 10-encounter file
        assert_eq!(f.floor_enc[&9].id, c.encounters[state::encounter_for_floor(9, n)].id);
    }

    #[test]
    fn evil_model_never_reaches_the_screen() {
        let c = content();
        let mut f = features(evil_model);
        f.begin_run(&c, 5, 0.0);
        settle(&mut f);
        for slot in 0..scamgen::POOL_SIZE {
            let m = f.scam_message(slot);
            assert_eq!(m.source, LineSource::Fallback("AI line rejected by filter"));
            assert!(scamgen::verify(&m) && !m.body.contains("example"));
        }
        assert_eq!(f.keeper_taunt(0).1, LineSource::Fallback("AI line rejected by filter"));
        for ok in [true, false] {
            assert_eq!(f.keeper_reaction(0, ok).0, crate::keeper::line(ok, 0), "verdict mismatch -> scripted");
        }
        f.hints.start(0, 0.0);
        assert_eq!(f.poll_hint(99.0).unwrap().1, LineSource::Fallback("AI line rejected by filter"));
        // tier narration that contradicts the direction is replaced
        let n = c.encounters.len();
        let mut run = Run::new(state::floors_for(n), n);
        for _ in 0..2 {
            run.begin_encounter();
            let e = &c.encounters[run.encounter()];
            run.choose(&c, e.choices.iter().position(|ch| ch.outcome == Outcome::Safe).unwrap());
            run.reveal_lesson(&c);
            run.advance();
            run.begin_keeper();
            run.answer_keeper(true);
            run.leave_verdict();
            run.descend();
        }
        assert_eq!(run.phase, Phase::Explore);
        let ch = f.on_floor_cleared(&run, 1).unwrap();
        assert_eq!((ch.from, ch.to), (SkillTier::Standard, SkillTier::Sharp));
        assert_eq!(ch.source, LineSource::Fallback("AI line rejected by filter"));
        assert_eq!(f.hints.after_secs, SkillTier::Sharp.params().hint_after_secs);
        assert!(f.on_floor_cleared(&run, 2).is_some_and(|c| c.to == SkillTier::Expert));
        assert!(f.on_floor_cleared(&run, 3).is_none(), "no change, no narration");
        f.request_debrief(&run, &c, 10.0);
        settle(&mut f);
        assert_eq!(f.poll_debrief(&c, 11.0).unwrap().source, LineSource::Fallback("AI line rejected by filter"));
    }

    #[test]
    fn debrief_waits_then_falls_back() {
        fn hang(_: &Config, _: &str, _: &str, _: TokenLimits) -> CallResult {
            std::thread::sleep(Duration::from_millis(300));
            Err(AiError::Failed("late".into()))
        }
        let c = content();
        let mut f = features(hang);
        f.begin_run(&c, 1, 0.0);
        let n = c.encounters.len();
        f.request_debrief(&Run::new(state::floors_for(n), n), &c, 10.0);
        assert!(f.poll_debrief(&c, 10.0 + DEBRIEF_WAIT_SECS - 0.1).is_none());
        let d = f.poll_debrief(&c, 10.0 + DEBRIEF_WAIT_SECS).unwrap();
        assert_eq!(d.source, LineSource::Fallback("AI timed out"));
    }

    #[test]
    fn same_seed_same_run_content() {
        let c = content();
        let mut a = AiFeatures::with(None, data::embedded(), ai::call_chat, Duration::ZERO);
        let mut b = AiFeatures::with(None, data::embedded(), ai::call_chat, Duration::ZERO);
        a.begin_run(&c, 4242, 0.0);
        b.begin_run(&c, 4242, 0.0);
        for i in 0..5 {
            assert_eq!(a.scam_message(i), b.scam_message(i));
            assert_eq!(a.spot_round(i), b.spot_round(i));
            assert_eq!(a.keeper_taunt(0), b.keeper_taunt(0));
        }
        b.begin_run(&c, 4243, 0.0);
        assert!((0..5).any(|i| a.spot_round(i) != b.spot_round(i)));
    }

    /// Live check of every feature prompt against the configured provider.
    /// Run: LIVE_SLEEP_MS=4500 cargo test --release live_ai_features -- --ignored --nocapture
    /// LIVE_ONLY=scam,floor,run,debrief picks tasks; LIVE_FLOORS=0,4,9 picks floors.
    #[test]
    #[ignore = "calls the live API; needs GROQ_API_KEY"]
    #[cfg(all(feature = "ai", not(target_arch = "wasm32")))]
    fn live_ai_features() {
        let cfg = Config::from_env().expect("set GROQ_API_KEY or CAMPUS_CRYPT_AI_KEY");
        let c = content();
        let a = data::embedded();
        let p = a.prompts.clone();
        let d = a.data;
        let only = std::env::var("LIVE_ONLY").ok();
        let want = |t: &str| only.as_deref().is_none_or(|o| o.split(',').any(|x| x == t));
        let sleep = || {
            let ms = std::env::var("LIVE_SLEEP_MS").ok().and_then(|v| v.parse().ok()).unwrap_or(4500);
            std::thread::sleep(Duration::from_millis(ms));
        };
        let (mut ok, mut total) = (0, 0);
        let call = |sys: &str, user: &str, lim: TokenLimits| {
            sleep();
            let t0 = std::time::Instant::now();
            let r = ai::call_chat(&cfg, sys, user, lim);
            (r, t0.elapsed().as_millis())
        };
        if want("scam") {
            for seed in [1u32, 2, 3, 4, 5] {
                let plan = scamgen::plan(&d, seed, 0);
                let (r, ms) = call(&p.scam_body, &scamgen::user_message(&d, &plan), SCAM_TOKENS);
                total += 1;
                match r {
                    Ok(raw) => {
                        let m = scamgen::build(&d, &plan, Some(&raw), "x");
                        let pass = m.source == LineSource::Ai;
                        ok += pass as u32;
                        println!("{} | scam {} {:?} | {ms} ms | injected {} | {}", if pass { "PASS" } else { "REJECT" }, m.template_id, plan.flags, m.injected, if pass { m.body.clone() } else { raw.replace('\n', " / ") });
                        assert!(scamgen::verify(&m));
                    }
                    Err(e) => println!("ERROR | scam | {ms} ms | {e:?}"),
                }
            }
        }
        if want("floor") {
            let floors: Vec<usize> = std::env::var("LIVE_FLOORS").ok().map(|v| v.split(',').filter_map(|x| x.parse().ok()).collect()).unwrap_or((0..state::RUN_FLOORS).collect());
            for floor in floors {
                let e = &c.encounters[state::encounter_for_floor(floor, c.encounters.len())];
                let (r, ms) = call(&p.floor_pack, &banter::user_message(floor, e, &d.owl_name), PACK_TOKENS);
                total += 1;
                match r {
                    Ok(raw) => {
                        let fp = banter::parse_floor_pack(&raw, e);
                        let n = [&fp.taunt, &fp.right, &fp.wrong, &fp.hint].iter().filter(|x| x.is_some()).count();
                        ok += (n == 4) as u32;
                        println!("{}/4 | floor {floor} {} | {ms} ms\n  TAUNT {:?}\n  RIGHT {:?}\n  WRONG {:?}\n  HINT  {:?}", n, e.id, fp.taunt, fp.right, fp.wrong, fp.hint);
                        if n < 4 {
                            println!("  raw: {}", raw.replace('\n', " / "));
                        }
                    }
                    Err(e) => println!("ERROR | floor {floor} | {ms} ms | {e:?}"),
                }
            }
        }
        if want("run") {
            let (r, ms) = call(&p.run_pack, "TASK: run_pack\nWrite the six lines now.", PACK_TOKENS);
            total += 1;
            match r {
                Ok(raw) => {
                    let got = difficulty::parse_run_pack(&raw);
                    ok += (got.len() == 6) as u32;
                    println!("{}/6 | run pack | {ms} ms", got.len());
                    for (k, l) in &got {
                        println!("  {k}: {l}");
                    }
                    if got.len() < 6 {
                        println!("  raw: {}", raw.replace('\n', " / "));
                    }
                }
                Err(e) => println!("ERROR | run pack | {ms} ms | {e:?}"),
            }
        }
        if want("debrief") {
            let n = c.encounters.len();
            let clean = Run::new(state::floors_for(n), n);
            let mut messy = Run::new(state::floors_for(n), n);
            for (o, k) in [(Outcome::Safe, true), (Outcome::Risky, false), (Outcome::Mixed, true)] {
                messy.begin_encounter();
                let e = &c.encounters[messy.encounter()];
                messy.choose(&c, e.choices.iter().position(|ch| ch.outcome == o).unwrap_or(0));
                messy.reveal_lesson(&c);
                messy.advance();
                messy.begin_keeper();
                messy.answer_keeper(k);
                messy.leave_verdict();
                while messy.phase == Phase::BossFight {
                    messy.boss_defeated();
                }
                messy.descend();
            }
            let mut extra = FeatureLog::default();
            extra.record_spot(3, false, "Gift card payment", "Nobody legit asks to be paid in gift cards.");
            let empty = FeatureLog::default();
            for (name, run) in [("clean", &clean), ("messy", &messy), ("lost", &messy)] {
                let mut log = MistakeLog::from_run(run, &c, if name == "clean" { &empty } else { &extra });
                if name == "lost" {
                    // same slips, but health ran out: the guide must not praise
                    log.ending = Some(state::Ending::Overwhelmed);
                }
                let (r, ms) = call(&p.debrief, &log.user_message(), DEBRIEF_TOKENS);
                total += 1;
                match r {
                    Ok(raw) => match debrief::parse_ai(&raw, &log, &c) {
                        Some(lines) => {
                            ok += 1;
                            println!("PASS | debrief {name} | {ms} ms");
                            lines.iter().for_each(|l| println!("  {l}"));
                        }
                        None => println!("REJECT | debrief {name} | {ms} ms | raw: {}", raw.replace('\n', " / ")),
                    },
                    Err(e) => println!("ERROR | debrief {name} | {ms} ms | {e:?}"),
                }
            }
        }
        println!("{ok}/{total} calls fully passed validation");
        assert!(ok > 0);
    }
}
