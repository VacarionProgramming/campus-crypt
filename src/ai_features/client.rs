//! The AI features' request queue: one background worker per game that runs
//! prefetch jobs one at a time, spaced `gap` apart, with a per-run call cap.
//!
//! Every job goes through `ai::call_chat` (same Groq client, 4.5 s budget with
//! the 429 retry, key never in errors). Results are tagged with the run they were
//! made for; `begin_run` bumps the run id, so queued jobs of an old run are
//! skipped without calling the API and late answers are dropped (stale guard).
//! Nothing here blocks the frame: `submit` and `pump` only touch channels.

use crate::ai::{AiError, CallResult, ChatCaller, Config, TokenLimits};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum JobKey {
    /// tier narration lines (one call at run start)
    RunPack,
    /// scam body for pool slot n
    Scam(usize),
    /// Keeper taunt/right/wrong + owl hint for floor n
    Floor(usize),
    /// end-of-run study guide
    Debrief,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Got<'a> {
    /// never requested (or a new run cleared it)
    Missing,
    /// queued or in flight since `queued` (game time)
    Pending(f64),
    Ready(&'a str),
    /// why there's no text: "offline", "AI unavailable", "AI call cap reached"
    Failed(&'static str),
}

enum Slot {
    Pending(f64),
    Ready(String),
    Failed(&'static str),
}

struct Job {
    run: u64,
    key: JobKey,
    system: Arc<str>,
    user: String,
    limits: TokenLimits,
}

type Answer = (u64, JobKey, CallResult);

pub struct FeatureClient {
    config: Option<Arc<Config>>,
    caller: ChatCaller,
    gap: Duration,
    /// calls allowed per run (the last one is kept for the debrief)
    pub cap: u32,
    jobs: Option<Sender<Job>>,
    worker: Option<std::thread::JoinHandle<()>>,
    answers_tx: Sender<Answer>,
    answers_rx: Receiver<Answer>,
    current: Arc<AtomicU64>,
    run: u64,
    /// Options > AI extras
    enabled: bool,
    calls: u32,
    slots: BTreeMap<JobKey, Slot>,
}

impl FeatureClient {
    pub fn new(config: Option<Arc<Config>>, caller: ChatCaller, gap: Duration, cap: u32) -> FeatureClient {
        let (answers_tx, answers_rx) = channel();
        FeatureClient {
            config,
            caller,
            gap,
            cap,
            jobs: None,
            worker: None,
            answers_tx,
            answers_rx,
            current: Arc::new(AtomicU64::new(0)),
            run: 0,
            enabled: true,
            calls: 0,
            slots: BTreeMap::new(),
        }
    }

    pub fn online(&self) -> bool {
        self.config.is_some() && self.enabled
    }

    /// Options > AI extras. Off: queued calls are dropped, anything not yet answered
    /// (or already answered) reads as failed, so every feature falls back to scripted.
    pub fn set_enabled(&mut self, on: bool) {
        if on == self.enabled {
            return;
        }
        self.enabled = on;
        if !on {
            // the worker skips jobs from an older run id: drop everything queued
            self.current.store(u64::MAX, Ordering::SeqCst);
            for s in self.slots.values_mut() {
                *s = Slot::Failed("AI switched off");
            }
        } else {
            self.current.store(self.run, Ordering::SeqCst);
            // forget the "switched off" answers so the next request may call again
            self.slots.retain(|_, s| !matches!(s, Slot::Failed("AI switched off")));
        }
    }

    pub fn model(&self) -> Option<&str> {
        self.config.as_deref().map(Config::model)
    }

    /// New run: forget everything, reset the call count, drop queued old jobs.
    pub fn begin_run(&mut self) {
        self.run += 1;
        if self.enabled {
            self.current.store(self.run, Ordering::SeqCst);
        }
        self.slots.clear();
        self.calls = 0;
    }

    pub fn calls_used(&self) -> u32 {
        self.calls
    }

    fn ensure_worker(&mut self) -> Option<&Sender<Job>> {
        if self.worker.as_ref().is_some_and(|w| w.is_finished()) {
            self.worker = None;
            self.jobs = None;
        }
        if self.jobs.is_none() {
            let cfg = Arc::clone(self.config.as_ref()?);
            let (tx, rx) = channel::<Job>();
            let out = self.answers_tx.clone();
            let current = Arc::clone(&self.current);
            let (caller, gap) = (self.caller, self.gap);
            let spawned = std::thread::Builder::new().name("ai-features".into()).spawn(move || {
                let mut last: Option<std::time::Instant> = None;
                for job in rx {
                    if job.run != current.load(Ordering::SeqCst) {
                        continue; // a newer run started: never spend a call on it
                    }
                    if let Some(t) = last {
                        std::thread::sleep(gap.saturating_sub(t.elapsed()));
                    }
                    let res = caller(&cfg, &job.system, &job.user, job.limits);
                    last = Some(std::time::Instant::now());
                    if out.send((job.run, job.key, res)).is_err() {
                        break;
                    }
                }
            });
            match spawned {
                Ok(h) => {
                    self.worker = Some(h);
                    self.jobs = Some(tx);
                }
                Err(e) => eprintln!("[ai_features] could not start worker: {e}"),
            }
        }
        self.jobs.as_ref()
    }

    /// Queue a call unless it's already known for this run. `reserved` jobs (the
    /// debrief) may use the last call of the cap. Returns true if a call was queued.
    pub fn submit(&mut self, key: JobKey, system: Arc<str>, user: String, limits: TokenLimits, now: f64) -> bool {
        if self.slots.contains_key(&key) {
            return false;
        }
        if self.config.is_none() {
            self.slots.insert(key, Slot::Failed("offline"));
            return false;
        }
        if !self.enabled {
            self.slots.insert(key, Slot::Failed("AI switched off"));
            return false;
        }
        let limit = if key == JobKey::Debrief { self.cap } else { self.cap.saturating_sub(1) };
        if self.calls >= limit {
            self.slots.insert(key, Slot::Failed("AI call cap reached"));
            return false;
        }
        let job = Job { run: self.run, key, system, user, limits };
        let sent = self.ensure_worker().is_some_and(|tx| tx.send(job).is_ok());
        if sent {
            self.calls += 1;
            self.slots.insert(key, Slot::Pending(now));
        } else {
            self.jobs = None;
            self.slots.insert(key, Slot::Failed("AI unavailable"));
        }
        sent
    }

    /// Collect finished calls (non-blocking; call once per frame).
    pub fn pump(&mut self) {
        while let Ok((run, key, res)) = self.answers_rx.try_recv() {
            if run != self.run || !self.enabled {
                continue; // stale: made for an earlier run, or the AI was switched off since
            }
            let slot = match res {
                Ok(s) => Slot::Ready(s),
                Err(AiError::Offline) => Slot::Failed("offline"),
                Err(AiError::Failed(e)) => {
                    eprintln!("[ai_features] {key:?}: {e}");
                    Slot::Failed("AI unavailable")
                }
            };
            self.slots.insert(key, slot);
        }
        // a worker that died leaves its jobs pending forever: fail them now
        if self.worker.as_ref().is_some_and(|w| w.is_finished()) {
            self.worker = None;
            self.jobs = None;
            for s in self.slots.values_mut() {
                if matches!(s, Slot::Pending(_)) {
                    *s = Slot::Failed("AI unavailable");
                }
            }
        }
    }

    pub fn get(&self, key: JobKey) -> Got<'_> {
        match self.slots.get(&key) {
            None => Got::Missing,
            Some(Slot::Pending(t)) => Got::Pending(*t),
            Some(Slot::Ready(s)) => Got::Ready(s),
            Some(Slot::Failed(w)) => Got::Failed(w),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const L: TokenLimits = TokenLimits(10, 10);

    fn echo(_: &Config, _: &str, user: &str, _: TokenLimits) -> CallResult {
        Ok(format!("echo {user}"))
    }
    fn slow_echo(_: &Config, _: &str, user: &str, _: TokenLimits) -> CallResult {
        std::thread::sleep(Duration::from_millis(120));
        Ok(format!("echo {user}"))
    }
    fn boom(_: &Config, _: &str, _: &str, _: TokenLimits) -> CallResult {
        Err(AiError::Failed("boom".into()))
    }

    fn client(caller: ChatCaller, gap_ms: u64, cap: u32) -> FeatureClient {
        let mut c = FeatureClient::new(Some(Arc::new(Config::for_test("k", "http://127.0.0.1:9"))), caller, Duration::from_millis(gap_ms), cap);
        c.begin_run();
        c
    }

    fn wait(c: &mut FeatureClient, key: JobKey) -> Got<'_> {
        for _ in 0..400 {
            c.pump();
            if !matches!(c.get(key), Got::Pending(_)) {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        c.get(key)
    }

    #[test]
    fn offline_fails_immediately_without_calls() {
        let mut c = FeatureClient::new(None, echo, Duration::ZERO, 5);
        c.begin_run();
        assert!(!c.submit(JobKey::RunPack, "s".into(), "u".into(), L, 0.0));
        assert_eq!(c.get(JobKey::RunPack), Got::Failed("offline"));
        assert_eq!(c.calls_used(), 0);
    }

    #[test]
    fn results_arrive_and_duplicates_are_not_resubmitted() {
        let mut c = client(echo, 0, 5);
        assert!(c.submit(JobKey::Floor(0), "s".into(), "f0".into(), L, 1.0));
        assert!(!c.submit(JobKey::Floor(0), "s".into(), "again".into(), L, 1.0));
        assert_eq!(c.get(JobKey::Floor(0)), Got::Pending(1.0));
        assert_eq!(wait(&mut c, JobKey::Floor(0)), Got::Ready("echo f0"));
        assert_eq!(c.calls_used(), 1);
    }

    #[test]
    fn cap_keeps_the_last_call_for_the_debrief() {
        let mut c = client(echo, 0, 3);
        assert!(c.submit(JobKey::Scam(0), "s".into(), "a".into(), L, 0.0));
        assert!(c.submit(JobKey::Scam(1), "s".into(), "b".into(), L, 0.0));
        assert!(!c.submit(JobKey::Scam(2), "s".into(), "c".into(), L, 0.0));
        assert_eq!(c.get(JobKey::Scam(2)), Got::Failed("AI call cap reached"));
        assert!(c.submit(JobKey::Debrief, "s".into(), "d".into(), L, 0.0));
        assert!(!c.submit(JobKey::Floor(3), "s".into(), "e".into(), L, 0.0));
        assert_eq!(c.calls_used(), 3);
    }

    #[test]
    fn calls_are_spaced_by_the_gap() {
        let mut c = client(echo, 150, 10);
        let t = std::time::Instant::now();
        for i in 0..3 {
            c.submit(JobKey::Scam(i), "s".into(), format!("{i}"), L, 0.0);
        }
        assert!(matches!(wait(&mut c, JobKey::Scam(2)), Got::Ready(_)));
        assert!(t.elapsed() >= Duration::from_millis(300), "{:?}", t.elapsed());
    }

    #[test]
    fn new_run_drops_old_answers_and_queued_jobs() {
        let mut c = client(slow_echo, 0, 10);
        c.submit(JobKey::Floor(0), "s".into(), "old0".into(), L, 0.0);
        c.submit(JobKey::Floor(1), "s".into(), "old1".into(), L, 0.0);
        c.begin_run();
        assert_eq!(c.get(JobKey::Floor(0)), Got::Missing);
        c.submit(JobKey::Floor(0), "s".into(), "new0".into(), L, 5.0);
        assert_eq!(wait(&mut c, JobKey::Floor(0)), Got::Ready("echo new0"));
        std::thread::sleep(Duration::from_millis(300));
        c.pump();
        assert_eq!(c.get(JobKey::Floor(1)), Got::Missing, "old run's answer never lands");
    }

    #[test]
    fn api_errors_become_failed_slots() {
        let mut c = client(boom, 0, 5);
        c.submit(JobKey::Debrief, "s".into(), "u".into(), L, 0.0);
        assert_eq!(wait(&mut c, JobKey::Debrief), Got::Failed("AI unavailable"));
    }
}
