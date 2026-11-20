//! AI character reactions.
//!
//! The AI's ONLY job is to voice one short in-character line reacting to the
//! player's choice. It never decides anything: the outcome, effects, warning signs
//! and safer next move all come from the content file and state.rs.
//!
//! Flow: when the player picks a choice we start a request on a background thread
//! (native builds with the `ai` feature and an API key). The Reaction screen shows a
//! "thinking" indicator; if a valid line arrives within `TIMEOUT_SECS` we show it,
//! otherwise (no key, network error, timeout, or a line that fails `sanitize`) we show
//! the choice's hand-written `fallback_reaction`. So the game is fully playable offline.
//!
//! Config (environment variables, native only):
//!   CAMPUS_CRYPT_AI_KEY    API key (falls back to GROQ_API_KEY). No key = offline lines.
//!   CAMPUS_CRYPT_AI_URL    OpenAI-compatible chat completions URL
//!                          (default Groq: https://api.groq.com/openai/v1/chat/completions)
//!   CAMPUS_CRYPT_AI_MODEL  model name (default openai/gpt-oss-20b)
//!   CAMPUS_CRYPT_AI=off    force offline lines

#![cfg_attr(not(all(feature = "ai", not(target_arch = "wasm32"))), allow(dead_code))]

use std::sync::mpsc::{channel, Receiver, TryRecvError};
use std::sync::Arc;

pub const MAX_LINE_CHARS: usize = 160;
/// How long the Reaction screen waits for the AI before using the fallback line.
/// The background request uses the same budget (including one 429 retry), so a
/// worker never outlives the screen that is waiting for it by more than a moment.
pub const TIMEOUT_SECS: f64 = 4.5;
/// Pause before the single retry after an HTTP 429 (free-tier burst limit).
pub const RETRY_DELAY_SECS: f64 = 1.2;
/// Don't bother retrying unless at least this much budget would be left for it.
const MIN_ATTEMPT_SECS: f64 = 1.0;
pub const DEFAULT_URL: &str = "https://api.groq.com/openai/v1/chat/completions";
pub const DEFAULT_MODEL: &str = "openai/gpt-oss-20b";

/// Everything the AI is told about the moment (mirrors `ai_prompt.md`'s inputs).
#[derive(Clone, Debug)]
pub struct ReactionRequest {
    pub encounter_id: String,
    pub character: String,
    pub scenario: String,
    pub choice_label: String,
    pub outcome: &'static str,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LineSource {
    /// generated live by the AI model
    Ai,
    /// hand-written fallback; the string says why (shown in tiny print for judges/devs)
    Fallback(&'static str),
}

/// Why a background call produced no line.
#[derive(Debug, PartialEq, Eq)]
pub enum AiError {
    /// no key / AI switched off / web build: not an error, just scripted lines
    Offline,
    /// network, HTTP, or response-shape problem (message never contains the key)
    Failed(String),
}

pub type CallResult = Result<String, AiError>;
/// The function that actually talks to the model; swapped out in unit tests.
type Caller = fn(&Config, &str, &ReactionRequest) -> CallResult;
/// A raw chat call (system prompt, user message); used by `ai_features`, swapped out in its tests.
pub type ChatCaller = fn(&Config, &str, &str, TokenLimits) -> CallResult;

/// `max_tokens` for (gpt-oss / Groq, where reasoning shares the budget; other providers).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TokenLimits(pub u32, pub u32);
/// The reaction line's budget (unchanged from before `ai_features`).
pub const LINE_TOKENS: TokenLimits = TokenLimits(600, 80);

struct Pending {
    rx: Receiver<CallResult>,
    started: f64,
    fallback: String,
}

pub struct Reactor {
    config: Option<Arc<Config>>,
    system_prompt: Arc<str>,
    pending: Option<Pending>,
    caller: Caller,
    /// Options > AI extras: off = every line scripted, no network calls
    enabled: bool,
    /// title-screen status, rebuilt only when `enabled` changes
    status: String,
}

pub struct Config {
    key: String,
    url: String,
    model: String,
}

impl Reactor {
    pub fn new(system_prompt: impl Into<Arc<str>>) -> Reactor {
        Reactor::with(Config::from_env().map(Arc::new), system_prompt.into(), call_api)
    }

    fn with(config: Option<Arc<Config>>, system_prompt: Arc<str>, caller: Caller) -> Reactor {
        let mut r = Reactor { config, system_prompt, pending: None, caller, enabled: true, status: String::new() };
        r.set_enabled(true);
        r
    }

    /// Options > AI extras. Off: every new reaction uses its scripted line at once.
    pub fn set_enabled(&mut self, on: bool) {
        self.enabled = on;
        self.status = match (&self.config, on) {
            (Some(c), true) => format!("AI voices: on ({})", c.model),
            (Some(_), false) => "AI voices: off (Options)".into(),
            (None, _) => "AI voices: offline (scripted lines)".into(),
        };
    }

    /// Short status for the title screen (built once in `new`; no per-call allocation).
    pub fn status(&self) -> &str {
        &self.status
    }

    /// Start a reaction. `fallback` is used if the AI is off, slow, or misbehaves.
    ///
    /// Each request gets its own channel and replaces any earlier pending one, so a
    /// slow answer from a previous encounter can never be shown on a later one: its
    /// sender just finds the receiver gone and the worker exits.
    pub fn request(&mut self, req: ReactionRequest, fallback: String, now: f64) {
        let (tx, rx) = channel();
        match self.config.as_ref().filter(|_| self.enabled) {
            Some(cfg) => {
                let cfg = Arc::clone(cfg);
                let prompt = Arc::clone(&self.system_prompt);
                let caller = self.caller;
                // If the OS refuses a thread, the closure (and `tx`) is dropped and
                // `poll` sees a disconnected channel: fallback line, no panic.
                let spawned = std::thread::Builder::new()
                    .name("ai-reaction".into())
                    .spawn(move || {
                        let _ = tx.send(caller(&cfg, &prompt, &req));
                    });
                if let Err(e) = spawned {
                    eprintln!("[ai] could not start request thread: {e}");
                }
            }
            None => {
                let _ = tx.send(Err(AiError::Offline));
            }
        }
        self.pending = Some(Pending { rx, started: now, fallback });
    }

    /// Poll the pending request without blocking. Returns the line to show once
    /// it's decided (exactly once per request).
    pub fn poll(&mut self, now: f64) -> Option<(String, LineSource)> {
        let p = self.pending.as_ref()?;
        let why = match p.rx.try_recv() {
            Ok(Ok(raw)) => match sanitize(&raw) {
                Some(line) => {
                    self.pending = None;
                    return Some((line, LineSource::Ai));
                }
                None => "AI line rejected by filter",
            },
            Ok(Err(AiError::Offline)) => "offline",
            Ok(Err(AiError::Failed(e))) => {
                eprintln!("[ai] {e}");
                "AI unavailable"
            }
            // worker died without answering (e.g. thread could not start)
            Err(TryRecvError::Disconnected) => "AI unavailable",
            Err(TryRecvError::Empty) if now - p.started >= TIMEOUT_SECS => "AI timed out",
            Err(TryRecvError::Empty) => return None,
        };
        let p = self.pending.take()?;
        Some((p.fallback, LineSource::Fallback(why)))
    }

    pub fn cancel(&mut self) {
        self.pending = None;
    }
}

impl Config {
    #[cfg(all(feature = "ai", not(target_arch = "wasm32")))]
    pub(crate) fn from_env() -> Option<Config> {
        // trimmed: a key pasted with a trailing newline would otherwise fail auth
        let env = |k: &str| std::env::var(k).ok().map(|v| v.trim().to_string()).filter(|v| !v.is_empty());
        if env("CAMPUS_CRYPT_AI").is_some_and(|v| v.eq_ignore_ascii_case("off")) {
            return None;
        }
        let key = env("CAMPUS_CRYPT_AI_KEY").or_else(|| env("GROQ_API_KEY"))?;
        Some(Config {
            key,
            url: env("CAMPUS_CRYPT_AI_URL").unwrap_or_else(|| DEFAULT_URL.into()),
            model: env("CAMPUS_CRYPT_AI_MODEL").unwrap_or_else(|| DEFAULT_MODEL.into()),
        })
    }
    #[cfg(not(all(feature = "ai", not(target_arch = "wasm32"))))]
    pub(crate) fn from_env() -> Option<Config> {
        None
    }
    /// The model name (for status text; never the key).
    pub fn model(&self) -> &str {
        &self.model
    }
    #[cfg(test)]
    pub(crate) fn for_test(key: &str, url: &str) -> Config {
        Config { key: key.into(), url: url.into(), model: "m".into() }
    }
}

/// Build the user message from the request (the system prompt is `ai_prompt.md`,
/// unchanged). The last line pins the persona: the room's own character speaks,
/// in first person, so each room has one dedicated voice.
pub fn user_message(r: &ReactionRequest) -> String {
    format!(
        "encounter_id: {}\ncharacter_name: {}\nscenario: {}\nchoice: {}\noutcome: {}\n\n\
         You are {} and nobody else. Speak as {} directly to the player, in first person, \
         in your own voice. One line, plain text.",
        r.encounter_id, r.character, r.scenario, r.choice_label, r.outcome, r.character, r.character
    )
}

/// Request body. Reasoning models (gpt-oss) otherwise spend the whole token budget
/// thinking and return empty content, so the reasoning knobs are added only for
/// gpt-oss models or Groq, where they're supported; other providers get a plain body.
#[cfg(test)]
pub fn request_body(model: &str, url: &str, system: &str, user: &str) -> serde_json::Value {
    request_body_sized(model, url, system, user, LINE_TOKENS)
}

/// `request_body` with a caller-chosen token budget (longer texts in `ai_features`).
pub fn request_body_sized(model: &str, url: &str, system: &str, user: &str, limits: TokenLimits) -> serde_json::Value {
    let mut body = serde_json::json!({
        "model": model,
        "temperature": 0.9,
        "messages": [
            { "role": "system", "content": system },
            { "role": "user", "content": user }
        ]
    });
    if model.contains("gpt-oss") || url.contains("groq.com") {
        body["reasoning_effort"] = "low".into();
        body["include_reasoning"] = false.into();
        body["max_tokens"] = limits.0.into();
    } else {
        body["max_tokens"] = limits.1.into();
    }
    body
}

/// Time left before `deadline`, or None if there isn't enough for another attempt.
fn budget_left(deadline: std::time::Instant, min_secs: f64) -> Option<std::time::Duration> {
    let left = deadline.saturating_duration_since(std::time::Instant::now());
    (left.as_secs_f64() >= min_secs).then_some(left)
}

/// One shared HTTPS agent: TLS roots are loaded once and connections are reused,
/// so later reactions skip the TCP/TLS handshake.
#[cfg(all(feature = "ai", not(target_arch = "wasm32")))]
fn agent() -> &'static ureq::Agent {
    static AGENT: std::sync::OnceLock<ureq::Agent> = std::sync::OnceLock::new();
    AGENT.get_or_init(|| ureq::AgentBuilder::new().build())
}

#[cfg(all(feature = "ai", not(target_arch = "wasm32")))]
fn call_api(cfg: &Config, system: &str, r: &ReactionRequest) -> CallResult {
    call_chat(cfg, system, &user_message(r), LINE_TOKENS)
}

/// One chat completion within the shared `TIMEOUT_SECS` budget (one 429 retry
/// included). Errors never contain the key.
#[cfg(all(feature = "ai", not(target_arch = "wasm32")))]
pub fn call_chat(cfg: &Config, system: &str, user: &str, limits: TokenLimits) -> CallResult {
    use std::time::{Duration, Instant};
    let fail = |m: String| AiError::Failed(m);
    // whole call, retry included, fits inside the Reaction screen's wait
    let deadline = Instant::now() + Duration::from_secs_f64(TIMEOUT_SECS);
    let body = request_body_sized(&cfg.model, &cfg.url, system, user, limits).to_string();
    let auth = format!("Bearer {}", cfg.key);
    let send = |budget: Duration| {
        agent()
            .post(&cfg.url)
            .timeout(budget)
            .set("Authorization", &auth)
            .set("Content-Type", "application/json")
            .send_string(&body)
            .map_err(Box::new)
    };
    let first_budget = budget_left(deadline, 0.0).unwrap_or_default();
    let resp = match send(first_budget) {
        // free tiers rate-limit bursts; one short retry, only if it still fits
        Err(e) if matches!(*e, ureq::Error::Status(429, _)) => {
            let retry_budget = budget_left(deadline, RETRY_DELAY_SECS + MIN_ATTEMPT_SECS)
                .ok_or_else(|| fail("rate limited (429), no time left to retry".into()))?;
            std::thread::sleep(Duration::from_secs_f64(RETRY_DELAY_SECS));
            send(retry_budget - Duration::from_secs_f64(RETRY_DELAY_SECS))
        }
        other => other,
    };
    // ureq's error text names the URL and status, never request headers (the key)
    let resp: serde_json::Value = resp
        .map_err(|e| fail(format!("request failed: {e}")))?
        .into_json()
        .map_err(|e| fail(format!("bad JSON: {e}")))?;
    match resp["choices"][0]["message"]["content"].as_str() {
        Some(s) if !s.trim().is_empty() => Ok(s.to_string()),
        _ => Err(fail(format!(
            "empty content (finish_reason: {})",
            resp["choices"][0]["finish_reason"].as_str().unwrap_or("?")
        ))),
    }
}

#[cfg(not(all(feature = "ai", not(target_arch = "wasm32"))))]
fn call_api(_cfg: &Config, _system: &str, _r: &ReactionRequest) -> CallResult {
    Err(AiError::Offline)
}

#[cfg(not(all(feature = "ai", not(target_arch = "wasm32"))))]
pub fn call_chat(_cfg: &Config, _system: &str, _user: &str, _limits: TokenLimits) -> CallResult {
    Err(AiError::Offline)
}

/// Make model text drawable by the ASCII pixel font: fold typographic punctuation,
/// drop emoji and other non-ASCII, remove markdown emphasis/headings (`*` `_` `` ` `` `#`),
/// collapse all whitespace (including newlines) to single spaces, and strip wrapping quotes.
pub fn clean_for_font(raw: &str) -> String {
    let mut s = String::with_capacity(raw.len());
    // single pass: whitespace is collapsed as we go (no split/join round trip)
    let mut space = false;
    let mut put = |s: &mut String, c: char| {
        if c == ' ' {
            space = true;
        } else {
            if space && !s.is_empty() {
                s.push(' ');
            }
            space = false;
            s.push(c);
        }
    };
    for c in raw.chars() {
        let folded: &str = match c {
            '\u{2014}' | '\u{2013}' | '\u{2212}' => " - ",
            '\u{2010}' | '\u{2011}' => "-", // (non-breaking) hyphen
            '\u{e0}'..='\u{e5}' => "a",
            '\u{e8}'..='\u{eb}' => "e",
            '\u{ec}'..='\u{ef}' => "i",
            '\u{f2}'..='\u{f6}' => "o",
            '\u{f9}'..='\u{fc}' => "u",
            '\u{f1}' => "n",
            '\u{e7}' => "c",
            '\u{2018}' | '\u{2019}' => "'",
            '\u{201c}' | '\u{201d}' => "\"",
            '\u{2026}' => "...",
            '*' | '_' | '`' | '#' => "",
            // JSON/markup debris models sometimes leave behind (seen live: a trailing `"}`)
            '{' | '}' | '[' | ']' | '<' | '>' | '|' | '\\' | '^' | '~' => "",
            c if c.is_whitespace() => " ",
            c if c.is_ascii() && !c.is_ascii_control() => {
                put(&mut s, c);
                continue;
            }
            _ => "", // emoji, symbols the font can't draw
        };
        for f in folded.chars() {
            put(&mut s, f);
        }
    }
    // strip quotes that wrap the whole line (all ASCII now, so byte slicing is safe)
    let mut t = s.as_str();
    while t.len() >= 2 && ((t.starts_with('"') && t.ends_with('"')) || (t.starts_with('\'') && t.ends_with('\''))) {
        t = t[1..t.len() - 1].trim();
    }
    // an unbalanced quote (opened and never closed) reads as a typo: drop them all
    if t.bytes().filter(|&b| b == b'"').count() % 2 == 1 {
        return t.split(['"', ' ']).filter(|w| !w.is_empty()).collect::<Vec<_>>().join(" ");
    }
    if t.len() == s.len() {
        s
    } else {
        t.to_string()
    }
}

/// Words that mean the model is talking about game mechanics, plus tells of a
/// link or email. The AI must only voice a reaction, never report state.
const BANNED: [&str; 12] = [
    "http", "www.", "://", "@", "health", "focus", "knowledge", "score", "damage", "you lose", "hit point", "game over",
];
/// Top-level domains that make `word.tld` read as a link (e.g. `bit.ly/x`, `ksu-jobs.net`).
const LINK_TLDS: [&str; 20] = [
    "com", "org", "net", "edu", "gov", "io", "ly", "co", "me", "info", "biz", "app", "xyz", "gg", "us", "uk", "ru", "cn",
    "tk", "link",
];

fn looks_like_link(lower: &str) -> bool {
    lower.split_whitespace().any(|tok| {
        let host = tok.split('/').next().unwrap_or("").trim_matches(|c: char| !c.is_ascii_alphanumeric());
        host.rsplit_once('.').is_some_and(|(name, tld)| {
            name.bytes().any(|b| b.is_ascii_alphanumeric()) && !name.ends_with('.') && LINK_TLDS.contains(&tld)
        })
    })
}

/// A finished line: at least `MIN_LINE_WORDS` words and `MIN_LINE_CHARS` chars, ending
/// in `.` `!` `?` (an ellipsis ends in `.`), optionally followed by a closing quote/paren.
/// Catches truncated replies like "Postpone, you".
pub const MIN_LINE_WORDS: usize = 6;
pub const MIN_LINE_CHARS: usize = 30;

fn is_finished(line: &str) -> bool {
    let body = line.trim_end_matches(['"', '\'', ')']);
    line.len() >= MIN_LINE_CHARS
        && line.split_whitespace().count() >= MIN_LINE_WORDS
        && body.ends_with(['.', '!', '?'])
}

/// Guardrail filter for model output. Returns None (use the fallback line) if the
/// cleaned line is empty, too short or unfinished, too long, contains a link/email,
/// or talks about game state.
pub fn sanitize(raw: &str) -> Option<String> {
    sanitize_len(raw, MAX_LINE_CHARS)
}

/// `sanitize` with a different length cap (multi-sentence texts in `ai_features`).
pub fn sanitize_len(raw: &str, max_chars: usize) -> Option<String> {
    let line = clean_for_font(raw);
    // ASCII only after cleaning, so len() is the character count
    if line.len() > max_chars || !is_finished(&line) {
        return None;
    }
    let lower = line.to_ascii_lowercase();
    if BANNED.iter().any(|b| lower.contains(b)) || looks_like_link(&lower) {
        return None;
    }
    Some(line)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn req(id: &str) -> ReactionRequest {
        ReactionRequest {
            encounter_id: id.into(),
            character: "Mimic".into(),
            scenario: "s".into(),
            choice_label: "Report it".into(),
            outcome: "safe",
        }
    }

    fn online(caller: Caller) -> Reactor {
        let cfg = Config { key: "test".into(), url: "http://127.0.0.1:9".into(), model: "m".into() };
        Reactor::with(Some(Arc::new(cfg)), "".into(), caller)
    }

    /// Poll like the frame loop does, with a wall-clock budget.
    fn poll_until(r: &mut Reactor, t0: f64, max_ms: u64) -> Option<(String, LineSource)> {
        for i in 0..max_ms / 5 {
            if let Some(x) = r.poll(t0 + i as f64 * 0.005) {
                return Some(x);
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        None
    }

    // ---- sanitizer / validator

    #[test]
    fn clean_strips_wrapping_quotes() {
        assert_eq!(clean_for_font("  \"Curses, foiled again!\"\n"), "Curses, foiled again!");
        assert_eq!(clean_for_font("'Curses.'"), "Curses.");
        assert_eq!(clean_for_font("\"'Nested.'\""), "Nested.");
    }

    #[test]
    fn sanitize_rejects_links() {
        for s in [
            "Visit http://evil.example now",
            "Try https://x.y",
            "Go to www.totally-real.edu",
            "Check bit.ly/abc123 fast",
            "Apply at ksu-jobs.net today!",
            "My portal is careers.com.",
            "Open ftp://files",
        ] {
            assert!(sanitize(s).is_none(), "should reject link: {s}");
        }
        // ordinary punctuation is not a link
        assert!(sanitize("Clever. Very clever... I hiss and slink away.").is_some());
        assert!(sanitize("Fine, you win this round, student.").is_some());
    }

    #[test]
    fn sanitize_rejects_emails() {
        assert!(sanitize("Mail me at boss@ksu.edu").is_none());
        assert!(sanitize("Write to hr at company dot com @ once").is_none());
    }

    #[test]
    fn sanitize_rejects_stat_and_game_words() {
        for s in [
            "You lost 2 health, ha!",
            "Your FOCUS is slipping.",
            "Knowledge is power, mortal.",
            "That will cost you damage.",
            "Nice score.",
            "You lose, student!",
            "Game over for you.",
            "Minus three hit points!",
        ] {
            assert!(sanitize(s).is_none(), "should reject state talk: {s}");
        }
    }

    #[test]
    fn sanitize_enforces_length_after_cleaning() {
        assert!(sanitize(&"a".repeat(MAX_LINE_CHARS + 1)).is_none());
        let full = format!("{}!", "ab ".repeat((MAX_LINE_CHARS - 1) / 3)).replace(" !", "!!");
        assert_eq!(full.len(), MAX_LINE_CHARS);
        assert_eq!(sanitize(&full).map(|s| s.len()), Some(MAX_LINE_CHARS));
        // whitespace and markdown don't count against the limit once cleaned
        let padded = format!("**{full}**   \n\n");
        assert_eq!(sanitize(&padded).map(|s| s.len()), Some(MAX_LINE_CHARS));
        assert!(sanitize("   \n  ").is_none());
        assert!(sanitize("").is_none());
    }

    #[test]
    fn clean_folds_or_drops_non_ascii() {
        assert_eq!(clean_for_font("all\u{2011}eyes, r\u{e9}sum\u{e9}"), "all-eyes, resume");
        assert_eq!(clean_for_font("Wait\u{2014}what?"), "Wait - what?");
        assert_eq!(clean_for_font("caf\u{e9} pi\u{f1}ata \u{fc}ber"), "cafe pinata uber");
        assert_eq!(clean_for_font("\u{1F600}\u{1F600}"), "");
        assert_eq!(clean_for_font("Hi \u{4f60}\u{597d} there\u{0007}"), "Hi there");
        let out = clean_for_font("\u{1F40D} Hiss \u{2192} done \u{00a0}ok");
        assert!(out.is_ascii(), "{out}");
        assert_eq!(out, "Hiss done ok");
    }

    #[test]
    fn clean_strips_markdown() {
        assert_eq!(
            clean_for_font("**Hiss!** You\u{2019}re   _clever_ \u{1F40D}\n  for a hero\u{2026}"),
            "Hiss! You're clever for a hero..."
        );
        assert_eq!(clean_for_font("# Curses\n`foiled`"), "Curses foiled");
        assert_eq!(clean_for_font("I hiss, \"Clever."), "I hiss, Clever.");
        assert_eq!(clean_for_font("\"I snarl, \"Bold.\""), "I snarl, Bold.");
        // real gpt-oss output that ended in JSON debris
        assert_eq!(
            clean_for_font("Oh no! I hiss, \"Back to the abyss!\"}"),
            "Oh no! I hiss, \"Back to the abyss!\""
        );
        assert_eq!(clean_for_font("[Hydra] <hiss> {ok}"), "Hydra hiss ok");
    }

    /// The 23 good lines from the live gpt-oss-20b run on Ry's tuned prompt.
    const LIVE_GOOD: [&str; 23] = [
        "Ah, you eyed my sender line! My shiny seal fizzled, and I retreat to the spam folder, still dreaming of glittering loot.",
        "Hey, you peeked at my sender! My shiny seal won't lure you now, I'm back to my chest and dust, sulking on the spam shelf.",
        "Ah, the click! My treasure door swings wide, glittering grants and shiny seals await, just for a brave owl like you!",
        "Ooh, a click! My treasure trap glows brighter than ever - more urgent surprises await, dear adventurer, as I gleefully gobble up your gleaming inbox.",
        "Oops, you spotted my scam! My shiny seal has vanished into the junk folder, and I must retreat to a dusty shelf of empty promises.",
        "Hey, you spotted my fake seal - my treasure trick fizzles. Back to the dusty inbox I go.",
        "You sliced my looming stack! I must retreat to the lost-and-found, where deadlines sigh.",
        "I hoped a plan would keep me from chaos, but your smart ordering broke my pile - now I'm retreating to the lost-and-found of procrastination.",
        "Hey, you sliced my looming stack into tiny crumbs! That's no fun. I'm off to the lost-and-found, sulking in a dusty shelf.",
        "You sliced my tower of deadlines into crumbs - no more thunderous crash. I must retreat to the lost-and-found for a moment of quiet.",
        "Ah, a gentle delay! I'll just stack these due dates higher, a pile of excitement. Watch me swirl as you nap.",
        "Mmm, a single key, you say? My heads cheer - now every door sings your name, and I dance in the hallway of endless passwords!",
        "Oh, how delightfully simple! My many heads cheer as your single key sings through every door, and I twirl in triumphant delight.",
        "Drat, that key is too twisty for my teeth! But if you use it everywhere, my other heads will still be sniffing at every door.",
        "Drat, that key is far too twisty for my teeth, yet if you use it on every door, my other heads will come sniffing around your other doors.",
        "A vault of unique keys? My heads are arguing over whose fault this is. Fine, fine, we give up and slink back to our puddle.",
        "A locked vault of unique keys? My heads are arguing over whose fault this is. Fine, fine, we give up and slink back to our puddle.",
        "Wonderful, a team player! My dream-job pitch is going perfectly. Just sign right here on my very official, very shiny clipboard!",
        "Wonderful, a team player! My dream-job pitch is going perfectly. Just sign right here on my very official, very shiny clipboard!",
        "Wait, you are calling the real company? No need, no need! My synergy-driven offer is, uh, expiring. I must flee. Clipboard, away!",
        "Wow, you caught me! My fake offer fizzles, so I'm off to the lost-and-found with my clipboard.",
        "Oh, you walk away? My glossy pitch must be off the shelf now. I'm retreating to the lost-and-found, my clipboard in tow.",
        "Oh, you're leaving me empty-handed? My glossy offer vanishes now, I retreat to the dusty office, hoping another clueless soul will bite.",
    ];

    #[test]
    fn finished_live_lines_pass() {
        for l in LIVE_GOOD {
            assert_eq!(sanitize(l).as_deref(), Some(l), "should pass: {l}");
        }
    }

    #[test]
    fn unfinished_or_short_lines_fall_back() {
        for s in [
            "Postpone, you",                                // truncated live reply
            "Curses, foiled by a careful student and",      // no terminal punctuation
            "Ha! Gotcha.",                                  // too short
            "Nope. Not. Today. Student. Bye.",              // 5 words
            "Incomprehensibly-overconfident-hyphenation!",  // long but one word
            "My seal fizzles and I slink off to the spam -", // dangling dash
        ] {
            assert!(sanitize(s).is_none(), "should reject: {s}");
        }
        // ellipsis and a closing quote/paren after the punctuation are fine
        assert!(sanitize("You caught me, so I slink away to sulk...").is_some());
        assert!(sanitize("I hiss, \"You caught my shiny seal, fine!\"").is_some());
        assert!(sanitize("Fine, I retreat to my dusty chest (for now.)").is_some());
    }

    #[test]
    fn reasoning_knobs_only_for_gpt_oss_or_groq() {
        let b = request_body(DEFAULT_MODEL, DEFAULT_URL, "s", "u");
        assert_eq!(b["reasoning_effort"], "low");
        assert_eq!(b["include_reasoning"], false);
        let o = request_body("gpt-4o-mini", "https://api.openai.com/v1/chat/completions", "s", "u");
        assert!(o.get("reasoning_effort").is_none() && o.get("include_reasoning").is_none());
    }

    // ---- reactor: never blocks, never shows stale lines, times out

    #[test]
    fn offline_reactor_returns_fallback_immediately() {
        let mut r = Reactor::with(None, "".into(), call_api);
        assert_eq!(r.status(), "AI voices: offline (scripted lines)");
        r.request(req("inbox_mimic"), "fallback!".into(), 0.0);
        assert_eq!(r.poll(0.0), Some(("fallback!".into(), LineSource::Fallback("offline"))));
        assert_eq!(r.poll(0.1), None, "a decided line is returned exactly once");
    }

    #[test]
    fn good_ai_line_is_used() {
        let mut r = online(|_, _, _| Ok("**Curses!** Foiled by a careful student.".into()));
        r.request(req("a"), "fb".into(), 0.0);
        assert_eq!(poll_until(&mut r, 0.0, 2000), Some(("Curses! Foiled by a careful student.".into(), LineSource::Ai)));
    }

    #[test]
    fn rejected_ai_line_uses_fallback() {
        let mut r = online(|_, _, _| Ok("Go to evil.com for your health".into()));
        r.request(req("a"), "fb".into(), 0.0);
        assert_eq!(poll_until(&mut r, 0.0, 2000), Some(("fb".into(), LineSource::Fallback("AI line rejected by filter"))));
    }

    #[test]
    fn api_error_uses_fallback() {
        let mut r = online(|_, _, _| Err(AiError::Failed("boom".into())));
        r.request(req("a"), "fb".into(), 0.0);
        assert_eq!(poll_until(&mut r, 0.0, 2000), Some(("fb".into(), LineSource::Fallback("AI unavailable"))));
    }

    #[test]
    fn slow_call_times_out_without_blocking_the_frame() {
        let mut r = online(|_, _, _| {
            std::thread::sleep(Duration::from_millis(400));
            Ok("late line".into())
        });
        r.request(req("a"), "fb".into(), 10.0);
        let t = std::time::Instant::now();
        assert_eq!(r.poll(10.0), None);
        assert_eq!(r.poll(10.0 + TIMEOUT_SECS - 0.01), None);
        assert!(t.elapsed() < Duration::from_millis(50), "poll must never block");
        assert_eq!(r.poll(10.0 + TIMEOUT_SECS), Some(("fb".into(), LineSource::Fallback("AI timed out"))));
        std::thread::sleep(Duration::from_millis(500));
        assert_eq!(r.poll(20.0), None, "the late answer is discarded");
    }

    #[test]
    fn worker_that_dies_falls_back_immediately() {
        // a worker that exits without sending (stands in for a failed spawn/panic)
        let mut r = online(|_, _, _| {
            std::thread::sleep(Duration::from_millis(20));
            Err(AiError::Failed("x".into()))
        });
        r.request(req("a"), "fb".into(), 0.0);
        // replace the receiver with one whose sender is already gone
        let (tx, rx) = channel::<CallResult>();
        drop(tx);
        r.pending.as_mut().unwrap().rx = rx;
        assert_eq!(r.poll(0.0), Some(("fb".into(), LineSource::Fallback("AI unavailable"))));
    }

    #[test]
    fn stale_response_never_reaches_a_later_encounter() {
        // the first room's call is slow and answers with its own id; the second is fast
        let mut r = online(|_, _, q| {
            if q.encounter_id == "room1" {
                std::thread::sleep(Duration::from_millis(150));
            }
            Ok(format!("This is my reaction line for {}.", q.encounter_id))
        });
        r.request(req("room1"), "fb1".into(), 0.0);
        r.cancel(); // player left the room (enter_room cancels)
        r.request(req("room2"), "fb2".into(), 0.1);
        std::thread::sleep(Duration::from_millis(250)); // room1's answer has now been sent
        assert_eq!(poll_until(&mut r, 0.1, 2000), Some(("This is my reaction line for room2.".into(), LineSource::Ai)));
        // and a superseding request without cancel behaves the same
        r.request(req("room1"), "fb1".into(), 1.0);
        r.request(req("room3"), "fb3".into(), 1.0);
        std::thread::sleep(Duration::from_millis(250));
        assert_eq!(poll_until(&mut r, 1.0, 2000), Some(("This is my reaction line for room3.".into(), LineSource::Ai)));
        assert_eq!(r.poll(2.0), None);
    }

    #[test]
    fn retry_only_when_budget_allows() {
        let far = std::time::Instant::now() + Duration::from_secs(3);
        assert!(budget_left(far, RETRY_DELAY_SECS + MIN_ATTEMPT_SECS).is_some());
        let near = std::time::Instant::now() + Duration::from_millis(500);
        assert!(budget_left(near, RETRY_DELAY_SECS + MIN_ATTEMPT_SECS).is_none());
        assert_eq!(budget_left(std::time::Instant::now(), 0.0).map(|d| d.as_secs()), Some(0));
        // retry delay + a minimal attempt must fit the screen's wait
        const { assert!(RETRY_DELAY_SECS + MIN_ATTEMPT_SECS < TIMEOUT_SECS) };
    }

    #[test]
    fn errors_never_contain_the_key() {
        let cfg = Config { key: "sk-SECRET-123".into(), url: "http://127.0.0.1:9/v1".into(), model: "m".into() };
        if let Err(AiError::Failed(msg)) = call_api(&cfg, "s", &req("a")) {
            assert!(!msg.contains("SECRET"), "{msg}");
        }
        if let Err(AiError::Failed(msg)) = call_chat(&cfg, "s", "u", TokenLimits(900, 300)) {
            assert!(!msg.contains("SECRET"), "{msg}");
        }
    }

    /// Live check against the configured provider (needs GROQ_API_KEY or
    /// CAMPUS_CRYPT_AI_KEY). Run: cargo test --release live_ai -- --ignored --nocapture
    #[test]
    #[ignore = "calls the live API; needs GROQ_API_KEY"]
    #[cfg(all(feature = "ai", not(target_arch = "wasm32")))]
    fn live_ai_all_choices() {
        let cfg = Config::from_env().expect("set GROQ_API_KEY or CAMPUS_CRYPT_AI_KEY");
        let loaded = crate::content::load();
        let (mut ok, mut total) = (0, 0);
        println!("model {} @ {}", cfg.model, cfg.url);
        for e in &loaded.content.encounters {
            for ch in &e.choices {
                if let Ok(only) = std::env::var("LIVE_ONLY") {
                    if !only.split(',').any(|x| x == format!("{}/{}", e.id, ch.id)) {
                        continue;
                    }
                }
                total += 1;
                let req = ReactionRequest {
                    encounter_id: e.id.clone(),
                    character: e.character.name.clone(),
                    scenario: e.scenario.clone(),
                    choice_label: ch.label.clone(),
                    outcome: ch.outcome.as_str(),
                };
                std::thread::sleep(std::time::Duration::from_millis(std::env::var("LIVE_SLEEP_MS").ok().and_then(|v| v.parse().ok()).unwrap_or(4500))); // stay under free-tier limits
                let t0 = std::time::Instant::now();
                let res = call_api(&cfg, &loaded.ai_prompt, &req);
                let ms = t0.elapsed().as_millis();
                match res {
                    Ok(raw) => match sanitize(&raw) {
                        Some(line) => {
                            ok += 1;
                            println!("PASS | {}/{} ({}) | {} ms | {} chars | {}", e.id, ch.id, ch.outcome.as_str(), ms, line.len(), line);
                        }
                        None => println!("REJECT | {}/{} ({}) | {} ms | raw: {}", e.id, ch.id, ch.outcome.as_str(), ms, raw.replace('\n', " / ")),
                    },
                    Err(err) => println!("ERROR | {}/{} | {} ms | {err:?}", e.id, ch.id, ms),
                }
            }
        }
        println!("{ok}/{total} passed validation");
        assert!(ok > 0);
    }
}
