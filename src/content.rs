//! Encounter content: the data contract shared with the content author.
//!
//! Schema (see docs / CONTENT_SCHEMA): `assets/content/encounters.json` holds an
//! ordered list of encounters; array order = room order. Each choice carries the
//! integer `effects` that are the ONLY thing allowed to change game state, plus the
//! deterministic teaching text (warning signs, safer next move) and an offline
//! `fallback_reaction` line for when the AI is unavailable.

use serde::Deserialize;

/// Content baked in at compile time, so the web build and a lone binary always work.
pub const EMBEDDED_ENCOUNTERS: &str = include_str!("../assets/content/encounters.json");
const EMBEDDED_PROMPT: &str = include_str!("../assets/content/ai_prompt.md");

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Outcome {
    Safe,
    Risky,
    Mixed,
}

impl Outcome {
    pub fn label(self) -> &'static str {
        match self {
            Outcome::Safe => "SAFE MOVE",
            Outcome::Risky => "RISKY MOVE",
            Outcome::Mixed => "MIXED MOVE",
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Outcome::Safe => "safe",
            Outcome::Risky => "risky",
            Outcome::Mixed => "mixed",
        }
    }
}

/// Resource changes for one choice. Missing fields default to 0.
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Effects {
    pub health: i32,
    pub focus: i32,
    pub knowledge: i32,
}

/// The Floor Keeper's question (see README "Floor Keeper questions").
/// Every field is optional; whatever is missing falls back to the encounter's
/// block, then to a generic question built from `safer_next_move`.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct Keeper {
    /// what the Keeper asks, e.g. "You tapped the link and it asks for your password. What now?"
    pub question: String,
    /// the one right answer
    pub answer: String,
    /// two clearly unsafe moves (extra entries are ignored)
    pub wrong: Vec<String>,
}

impl Keeper {
    fn fold(&mut self) {
        self.question = fold_for_font(&self.question);
        self.answer = fold_for_font(&self.answer);
        for w in &mut self.wrong {
            *w = fold_for_font(w);
        }
    }
}

/// Floor 1's battle-box round (keeper_box.rs): the Keeper "attacks" with a
/// message preview, and the answers float in the box as short targets.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct KeeperBox {
    /// "Name <address>" as an inbox would show it
    pub from: String,
    pub subject: String,
    pub snippet: String,
    /// the small line at the bottom of the message (often where the red flag hides)
    #[serde(default)]
    pub fine_print: String,
    /// where the message's button really goes (shown by INSPECT / "Hover the link")
    #[serde(default)]
    pub link: String,
    /// question / answer / two wrong moves, as in a `keeper` block; the
    /// options are short action text (they label the box's targets)
    #[serde(flatten)]
    pub keeper: Keeper,
    /// the warning sign shown with the verdict
    #[serde(default)]
    pub warning: String,
}

impl KeeperBox {
    fn fold(&mut self) {
        for s in [&mut self.from, &mut self.subject, &mut self.snippet, &mut self.fine_print, &mut self.link, &mut self.warning] {
            *s = fold_for_font(s);
        }
        self.keeper.fold();
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct Choice {
    pub id: String,
    pub label: String,
    pub outcome: Outcome,
    #[serde(default)]
    pub effects: Effects,
    pub fallback_reaction: String,
    #[serde(default)]
    pub warning_signs: Vec<String>,
    #[serde(default)]
    pub safer_next_move: String,
    /// the Keeper's question for a player who made this choice
    #[serde(default)]
    pub keeper: Option<Keeper>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Character {
    pub name: String,
    /// art direction note for the sprite (read by humans, not the engine)
    #[serde(default)]
    #[allow(dead_code)]
    pub portrait_hint: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Encounter {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub room_intro: String,
    pub character: Character,
    pub scenario: String,
    pub choices: Vec<Choice>,
    /// the Keeper's question when a choice has no `keeper` block of its own
    #[serde(default)]
    pub keeper: Option<Keeper>,
    /// floor 1 only: the Keeper's battle-box round (replaces the quiz there)
    #[serde(default)]
    pub keeper_box: Option<KeeperBox>,
    /// "scripted" or "ai". Older content files omit it and stay scripted.
    /// Daily runs never swap in an AI encounter, so this stays "scripted".
    #[serde(default = "source_scripted")]
    pub source: String,
}

fn source_scripted() -> String {
    "scripted".into()
}

#[derive(Clone, Debug, Deserialize)]
pub struct Content {
    pub encounters: Vec<Encounter>,
}

/// Everything the engine needs from the content folder, plus where it came from.
pub struct Loaded {
    pub content: Content,
    pub ai_prompt: String,
    /// human-readable source, shown on the title screen in small print
    pub source: String,
    /// non-fatal problems found while validating (printed to stderr, shown in dev overlay)
    pub warnings: Vec<String>,
}

/// The built-in pixel font only has ASCII glyphs, so fold common typographic
/// characters (em dashes, curly quotes, ellipses) for display. The file is untouched.
pub fn fold_for_font(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\u{2014}' => out.push_str(" - "),
            '\u{2013}' | '\u{2212}' => out.push('-'),
            '\u{2018}' | '\u{2019}' => out.push('\''),
            '\u{201c}' | '\u{201d}' => out.push('"'),
            '\u{2026}' => out.push_str("..."),
            '\u{00a0}' => out.push(' '),
            c if c.is_ascii() => out.push(c),
            _ => out.push('?'),
        }
    }
    // " - " next to existing spaces would double up
    out.replace("  - ", " - ").replace(" -  ", " - ")
}

fn fold_content(c: &mut Content) {
    for e in &mut c.encounters {
        for s in [&mut e.name, &mut e.room_intro, &mut e.scenario, &mut e.character.name] {
            *s = fold_for_font(s);
        }
        if let Some(k) = &mut e.keeper {
            k.fold();
        }
        if let Some(b) = &mut e.keeper_box {
            b.fold();
        }
        for ch in &mut e.choices {
            ch.label = fold_for_font(&ch.label);
            ch.fallback_reaction = fold_for_font(&ch.fallback_reaction);
            ch.safer_next_move = fold_for_font(&ch.safer_next_move);
            for w in &mut ch.warning_signs {
                *w = fold_for_font(w);
            }
            if let Some(k) = &mut ch.keeper {
                k.fold();
            }
        }
    }
}

/// Lowercase words of 3+ letters/digits ("Don't" -> "don").
fn words(s: &str) -> Vec<String> {
    s.split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| w.len() >= 3)
        .map(str::to_ascii_lowercase)
        .collect()
}

/// Fuzzy "this is the same advice": one contains the other after normalizing,
/// or at least 70% of the shorter one's words appear in the other.
pub(crate) fn looks_like(a: &str, b: &str) -> bool {
    let (wa, wb) = (words(a), words(b));
    let (na, nb) = (wa.join(" "), wb.join(" "));
    if na.is_empty() || nb.is_empty() {
        return false;
    }
    if na.contains(&nb) || nb.contains(&na) {
        return true;
    }
    let (short, long) = if wa.len() <= wb.len() { (&wa, &wb) } else { (&wb, &wa) };
    let shared = short.iter().filter(|w| long.contains(w)).count();
    shared * 10 >= short.len() * 7
}

/// Longest answer text on a battle-box target (two short lines).
pub const BOX_OPTION_MAX: usize = 40;

/// Parse and validate encounter JSON. Errors only on unusable content
/// (bad JSON, no encounters, an encounter without choices).
pub fn parse(json: &str) -> Result<(Content, Vec<String>), String> {
    let mut c: Content = serde_json::from_str(json).map_err(|e| format!("encounters.json: {e}"))?;
    fold_content(&mut c);
    if c.encounters.is_empty() {
        return Err("encounters.json has no encounters".into());
    }
    let mut warn = Vec::new();
    // every piece of good advice in the file: a Keeper's wrong option must not resemble any of it
    let safe_moves: Vec<&str> = c
        .encounters
        .iter()
        .flat_map(|e| {
            let answers = e
                .keeper
                .iter()
                .chain(e.choices.iter().filter_map(|ch| ch.keeper.as_ref()))
                .chain(e.keeper_box.iter().map(|b| &b.keeper))
                .map(|k| k.answer.as_str());
            e.choices.iter().map(|ch| ch.safer_next_move.as_str()).chain(answers)
        })
        .filter(|m| !m.trim().is_empty())
        .collect();
    for e in &c.encounters {
        if e.choices.is_empty() {
            return Err(format!("encounter '{}' has no choices", e.id));
        }
        if e.choices.len() > 4 {
            warn.push(format!("{}: {} choices (UI fits 4; extra are hidden)", e.id, e.choices.len()));
        }
        if e.choices.iter().all(|ch| ch.outcome != Outcome::Safe) {
            warn.push(format!("{}: no safe choice (the lesson has no right answer)", e.id));
        }
        let lim = |what: &str, s: &str, max: usize, warn: &mut Vec<String>| {
            let n = s.chars().count();
            if n > max {
                warn.push(format!("{}: {what} is {n} chars (limit {max})", e.id));
            }
        };
        lim("room_intro", &e.room_intro, 200, &mut warn);
        lim("scenario", &e.scenario, 400, &mut warn);
        for ch in &e.choices {
            lim(&format!("{} label", ch.id), &ch.label, 24, &mut warn);
            lim(&format!("{} fallback_reaction", ch.id), &ch.fallback_reaction, 160, &mut warn);
        }
        // the Floor Keeper's questions (box targets hold short action text)
        let blocks = e
            .keeper
            .iter()
            .map(|k| ("keeper".to_string(), k, 150))
            .chain(e.choices.iter().filter_map(|ch| ch.keeper.as_ref().map(|k| (format!("{} keeper", ch.id), k, 150))))
            .chain(e.keeper_box.iter().map(|b| ("keeper_box".to_string(), &b.keeper, BOX_OPTION_MAX)));
        for (what, k, opt_max) in blocks {
            lim(&format!("{what} question"), &k.question, 120, &mut warn);
            lim(&format!("{what} answer"), &k.answer, opt_max, &mut warn);
            let wrong: Vec<&String> = k.wrong.iter().filter(|w| !w.trim().is_empty()).collect();
            if wrong.len() < 2 {
                warn.push(format!("{}: {what} has {} wrong options (needs 2; generic ones fill in)", e.id, wrong.len()));
            }
            for w in wrong.iter().take(2) {
                lim(&format!("{what} wrong option"), w, opt_max, &mut warn);
                if k.answer.trim() == w.trim() || safe_moves.iter().any(|m| looks_like(m, w)) {
                    warn.push(format!("{}: {what} lists a safe move as a wrong option: \"{w}\"", e.id));
                }
            }
        }
        if let Some(b) = &e.keeper_box {
            lim("keeper_box from", &b.from, 60, &mut warn);
            lim("keeper_box subject", &b.subject, 70, &mut warn);
            lim("keeper_box snippet", &b.snippet, 220, &mut warn);
            lim("keeper_box fine_print", &b.fine_print, 80, &mut warn);
            lim("keeper_box link", &b.link, 60, &mut warn);
            lim("keeper_box warning", &b.warning, 150, &mut warn);
            if b.keeper.answer.trim().is_empty() || b.snippet.trim().is_empty() {
                warn.push(format!("{}: keeper_box needs an answer and a snippet (the floor uses the plain quiz)", e.id));
            }
        }
        if e.keeper.is_none() && e.choices.iter().any(|ch| ch.keeper.is_none()) {
            warn.push(format!("{}: no keeper block (the Floor Keeper asks a generic question)", e.id));
        }
    }
    let mut seen = std::collections::HashSet::new();
    for e in &mut c.encounters {
        if !seen.insert(e.id.clone()) {
            warn.push(format!("duplicate encounter id '{}'", e.id));
        }
        let mut ids = std::collections::HashSet::new();
        for ch in &mut e.choices {
            if !ids.insert(ch.id.clone()) {
                warn.push(format!("{}: duplicate choice id '{}'", e.id, ch.id));
            }
            // the offline line is what shows when the AI is off; never show a blank bubble
            if ch.fallback_reaction.trim().is_empty() {
                warn.push(format!("{}: {} has an empty fallback_reaction", e.id, ch.id));
                ch.fallback_reaction = "...".into();
            }
        }
    }
    Ok((c, warn))
}

/// Load content: native builds prefer the live files in assets/content/ (or
/// `$CAMPUS_CRYPT_CONTENT`), so edits show up on relaunch without recompiling;
/// everything falls back to the embedded copy.
pub fn load() -> Loaded {
    #[cfg(not(target_arch = "wasm32"))]
    let dir = std::env::var("CAMPUS_CRYPT_CONTENT").unwrap_or_else(|_| "assets/content".into());
    #[cfg(not(target_arch = "wasm32"))]
    let loaded = load_from(Some(&dir));
    #[cfg(target_arch = "wasm32")]
    let loaded = load_from(None);
    for w in &loaded.warnings {
        eprintln!("[content] {w}");
    }
    loaded
}

/// `dir = None` uses only the embedded content (web build).
pub fn load_from(dir: Option<&str>) -> Loaded {
    let mut warnings = Vec::new();
    if let Some(dir) = dir {
        let path = format!("{dir}/encounters.json");
        match std::fs::read_to_string(&path) {
            Ok(json) => match parse(&json) {
                Ok((content, w)) => {
                    warnings.extend(w);
                    return Loaded { content, ai_prompt: read_prompt(dir, &mut warnings), source: path, warnings };
                }
                Err(e) => warnings.push(format!("{path}: {e} (using embedded content)")),
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => warnings.push(format!("{path}: {e} (using embedded content)")),
        }
    }
    let (content, w) = parse(EMBEDDED_ENCOUNTERS).expect("embedded encounters.json must parse (checked by unit tests)");
    warnings.extend(w);
    let ai_prompt = match dir {
        Some(d) => read_prompt(d, &mut warnings),
        None => EMBEDDED_PROMPT.to_string(),
    };
    Loaded { content, ai_prompt, source: "embedded content".into(), warnings }
}

/// The live `ai_prompt.md` if present and non-empty, else the embedded one.
fn read_prompt(dir: &str, warnings: &mut Vec<String>) -> String {
    match std::fs::read_to_string(format!("{dir}/ai_prompt.md")) {
        Ok(s) if !s.trim().is_empty() => s,
        Ok(_) => {
            warnings.push(format!("{dir}/ai_prompt.md is empty (using embedded prompt)"));
            EMBEDDED_PROMPT.to_string()
        }
        Err(_) => EMBEDDED_PROMPT.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shipped_content_parses_within_limits_and_starts_with_inbox_mimic() {
        let (c, warn) = parse(EMBEDDED_ENCOUNTERS).unwrap();
        assert_eq!(c.encounters[0].id, "inbox_mimic");
        assert!(warn.is_empty(), "{warn:?}");
        for e in &c.encounters {
            assert!(e.choices.iter().any(|ch| ch.outcome == Outcome::Safe), "{} needs a safe choice", e.id);
        }
    }

    #[test]
    fn safe_move_check_is_fuzzy() {
        let m = "Use a unique password on each account, starting with email and campus logins.";
        assert!(looks_like(m, "use a UNIQUE password on each account -- starting with email and campus logins!"));
        assert!(looks_like(m, "Use a unique password on each account."));
        assert!(looks_like(m, "Start with email and campus logins: a unique password on each account."));
        assert!(!looks_like(m, "Use one familiar password on every account, so you never forget it."));
        assert!(!looks_like(m, ""));
        let j = r#"{"encounters":[{"id":"a","name":"A","character":{"name":"x"},"scenario":"s",
            "choices":[{"id":"c","label":"L","outcome":"safe","fallback_reaction":"r",
              "safer_next_move":"Report the message to KSU without clicking any links.",
              "keeper":{"question":"q?","answer":"Report it.","wrong":["Report the message to KSU, without clicking links","Click it."]}}]}]}"#;
        let (_, w) = parse(j).unwrap();
        assert!(w.iter().any(|x| x.contains("lists a safe move")), "{w:?}");
    }

    #[test]
    fn typographic_characters_fold_to_ascii() {
        assert_eq!(fold_for_font("a\n\n\u{2014} Office"), "a\n\n - Office");
        assert_eq!(fold_for_font("\u{201c}hi\u{201d} it\u{2019}s\u{2026}"), "\"hi\" it's...");
    }

    #[test]
    fn missing_effects_default_to_zero() {
        let j = r#"{"encounters":[{"id":"a","name":"A","character":{"name":"x"},"scenario":"s",
            "choices":[{"id":"c","label":"L","outcome":"mixed","fallback_reaction":"r"}]}]}"#;
        let (c, _) = parse(j).unwrap();
        assert_eq!(c.encounters[0].choices[0].effects, Effects::default());
    }

    #[test]
    fn rejects_empty_and_bad_outcomes() {
        assert!(parse(r#"{"encounters":[]}"#).is_err());
        let bad = r#"{"encounters":[{"id":"a","name":"A","character":{"name":"x"},"scenario":"s",
            "choices":[{"id":"c","label":"L","outcome":"great","fallback_reaction":"r"}]}]}"#;
        assert!(parse(bad).is_err());
    }

    fn tmpdir(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("campus_crypt_test_{}_{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn shipped_content_has_one_encounter_per_floor_with_complete_choices() {
        let (c, _) = parse(EMBEDDED_ENCOUNTERS).unwrap();
        let ids: Vec<&str> = c.encounters.iter().map(|e| e.id.as_str()).collect();
        // ids are unique and non-empty; the shipped file authors every floor of the run
        let unique: std::collections::HashSet<&str> = ids.iter().copied().collect();
        assert_eq!(unique.len(), ids.len(), "duplicate encounter id in {ids:?}");
        assert!(ids.iter().all(|id| !id.trim().is_empty()));
        assert!(ids.len() >= crate::state::RUN_FLOORS, "{} encounters for {} floors", ids.len(), crate::state::RUN_FLOORS);
        assert_eq!(ids[0], "inbox_mimic", "the run opens with the Inbox Mimic");
        for e in &c.encounters {
            assert!(!e.name.is_empty() && !e.scenario.is_empty() && !e.character.name.is_empty(), "{}", e.id);
            assert!((2..=4).contains(&e.choices.len()), "{}", e.id);
            for ch in &e.choices {
                assert!(!ch.label.is_empty() && !ch.fallback_reaction.is_empty(), "{}/{}", e.id, ch.id);
                assert!(!ch.warning_signs.is_empty() && !ch.safer_next_move.is_empty(), "{}/{}", e.id, ch.id);
                for s in [&ch.label, &ch.fallback_reaction, &ch.safer_next_move] {
                    assert!(s.is_ascii(), "{}/{}: font can't draw {s:?}", e.id, ch.id);
                }
            }
        }
    }

    #[test]
    fn embedded_prompt_is_not_empty() {
        assert!(!EMBEDDED_PROMPT.trim().is_empty());
        assert!(!load_from(None).ai_prompt.trim().is_empty());
    }

    #[test]
    fn malformed_local_file_falls_back_to_embedded() {
        let d = tmpdir("bad");
        std::fs::write(d.join("encounters.json"), "{ not json").unwrap();
        let l = load_from(Some(d.to_str().unwrap()));
        assert_eq!(l.source, "embedded content");
        assert_eq!(l.content.encounters.len(), parse(EMBEDDED_ENCOUNTERS).unwrap().0.encounters.len());
        assert!(l.warnings.iter().any(|w| w.contains("using embedded content")), "{:?}", l.warnings);

        std::fs::write(d.join("encounters.json"), r#"{"encounters":[]}"#).unwrap();
        assert_eq!(load_from(Some(d.to_str().unwrap())).source, "embedded content");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn missing_dir_uses_embedded_quietly_and_valid_local_file_wins() {
        let l = load_from(Some("/definitely/not/here"));
        assert_eq!(l.source, "embedded content");
        assert!(l.warnings.is_empty(), "{:?}", l.warnings);

        let d = tmpdir("good");
        std::fs::write(
            d.join("encounters.json"),
            r#"{"encounters":[{"id":"a","name":"A","character":{"name":"x"},"scenario":"s",
               "choices":[{"id":"c","label":"L","outcome":"safe","fallback_reaction":"r"}]}]}"#,
        )
        .unwrap();
        std::fs::write(d.join("ai_prompt.md"), "   \n").unwrap();
        let l = load_from(Some(d.to_str().unwrap()));
        assert!(l.source.ends_with("encounters.json"));
        assert_eq!(l.content.encounters.len(), 1);
        assert_eq!(l.ai_prompt, EMBEDDED_PROMPT, "blank prompt file falls back");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn warns_on_duplicates_blank_fallback_and_no_safe_choice() {
        let j = r#"{"encounters":[
            {"id":"a","name":"A","character":{"name":"x"},"scenario":"s",
             "choices":[{"id":"c","label":"L","outcome":"risky","fallback_reaction":"  "},
                        {"id":"c","label":"M","outcome":"mixed","fallback_reaction":"r"}]},
            {"id":"a","name":"B","character":{"name":"y"},"scenario":"s",
             "choices":[{"id":"d","label":"L","outcome":"safe","fallback_reaction":"r"}]}]}"#;
        let (c, w) = parse(j).unwrap();
        assert_eq!(c.encounters[0].choices[0].fallback_reaction, "...");
        for needle in ["duplicate encounter id", "duplicate choice id", "empty fallback_reaction", "no safe choice"] {
            assert!(w.iter().any(|x| x.contains(needle)), "missing '{needle}' in {w:?}");
        }
    }

    #[test]
    fn rejects_encounter_without_choices_and_missing_required_fields() {
        assert!(parse(r#"{"encounters":[{"id":"a","name":"A","character":{"name":"x"},"scenario":"s","choices":[]}]}"#).is_err());
        assert!(parse(r#"{"encounters":[{"id":"a","name":"A","scenario":"s","choices":[]}]}"#).is_err());
        assert!(parse("").is_err());
        assert!(parse("[]").is_err());
    }

    /// Real offices the lessons may name. Anything else in the content JSON that
    /// looks like a domain has to be a reserved suffix (`.example`, `.test`, `.invalid`).
    const OFFICIAL_SITES: [&str; 3] = ["studentaid.gov", "kennesaw.edu", "ftc.gov"];

    fn domain_hosts(text: &str) -> Vec<String> {
        let mut out = Vec::new();
        for raw in text.split(|c: char| c.is_whitespace() || matches!(c, '"' | '\'' | '<' | '>' | '(' | ')' | ',' | ';' | '[' | ']' | '{' | '}')) {
            let tok = raw.trim_matches(|c: char| !c.is_ascii_alphanumeric());
            let host = tok.split(['/', '?', '#']).next().unwrap_or("");
            let host = host.rsplit_once('@').map(|(_, h)| h).unwrap_or(host);
            let host = host.trim_matches('.').to_ascii_lowercase();
            let Some((name, tld)) = host.rsplit_once('.') else { continue };
            if name.is_empty() || tld.len() < 2 || !tld.bytes().all(|b| b.is_ascii_alphabetic()) {
                continue;
            }
            if !host.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-')) || name.ends_with('.') {
                continue;
            }
            out.push(host);
        }
        out
    }

    fn domain_allowed(host: &str) -> bool {
        let tld = host.rsplit_once('.').map(|(_, t)| t).unwrap_or("");
        if matches!(tld, "example" | "test" | "invalid") {
            return true;
        }
        OFFICIAL_SITES.iter().any(|site| host == *site || host.ends_with(&format!(".{site}")))
    }

    #[test]
    fn scam_domains_in_content_json_are_example_or_official() {
        assert!(!domain_allowed("ksu-grants-secure.net"));
        assert!(!domain_allowed("owl-interns-careers.com"));
        assert!(!domain_allowed("campus-parking-pay.info"));
        assert!(!domain_allowed("notkennesaw.edu"));
        assert!(domain_allowed("kenesaw-aid.example"));
        assert!(domain_allowed("lookalike.test"));
        assert!(domain_allowed("gone.invalid"));
        assert!(domain_allowed("studentaid.gov"));
        assert!(domain_allowed("noreply.studentaid.gov"));
        assert!(domain_allowed("kennesaw.edu"));
        assert!(domain_allowed("reportfraud.ftc.gov"));

        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/content");
        let mut bad = Vec::new();
        for ent in std::fs::read_dir(&root).unwrap() {
            let path = ent.unwrap().path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let text = std::fs::read_to_string(&path).unwrap();
            let name = path.file_name().unwrap().to_string_lossy();
            for host in domain_hosts(&text) {
                if !domain_allowed(&host) {
                    bad.push(format!("{name}: {host}"));
                }
            }
        }
        bad.sort();
        bad.dedup();
        assert!(bad.is_empty(), "registrable fake domains (use .example, .test, or .invalid):\n{}", bad.join("\n"));
    }
}
