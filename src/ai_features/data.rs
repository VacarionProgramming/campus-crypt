//! Scripted content and prompts for the AI features:
//! `assets/content/ai_features.json` (fallback lines, scam templates, safe
//! messages; schema in CONTENT_SCHEMA.md) and `assets/content/ai_features_prompts.md`.
//! Both are embedded at compile time; native builds prefer the live files in
//! the content folder (`$CAMPUS_CRYPT_CONTENT`, default `assets/content`) like
//! encounters.json does. A broken live file falls back to the embedded copy.

use crate::content::fold_for_font;
use serde::Deserialize;
use std::collections::BTreeMap;

pub const EMBEDDED_DATA: &str = include_str!("../../assets/content/ai_features.json");
pub const EMBEDDED_PROMPTS: &str = include_str!("../../assets/content/ai_features_prompts.md");

/// Every fake sender must use this reserved, non-routable suffix.
pub const FICTIONAL_SUFFIX: &str = ".example";
pub const DEFAULT_FAKE_SENDER: &str = "careers-ksu-owls@freemail.example";

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct FlagText {
    /// short chip shown on the lesson / spot-the-fake reveal, e.g. "Rush pressure"
    pub label: String,
    /// one-sentence advice for the debrief
    pub tip: String,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct ScamTemplate {
    pub id: String,
    /// "email" | "text" | "dm" (display only)
    pub channel: String,
    pub sender_name: String,
    /// shown only when the `mismatched_sender` flag is picked; must end in `.example`
    pub fake_address: String,
    pub subject: String,
    /// what the scam is about (sent to the AI)
    pub topic: String,
    pub sign_off: String,
    /// red flag ids this template can carry (code picks 2-3 of them per message)
    pub flags: Vec<String>,
    /// a scripted sentence per prose flag, injected by code when the body lacks it
    pub flag_lines: BTreeMap<String, String>,
    /// scripted body prose (no flags needed; code injects them)
    pub bodies: Vec<String>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct SafeMessage {
    pub id: String,
    pub channel: String,
    pub sender: String,
    pub subject: String,
    pub body: String,
    /// why it's safe, shown after the player picks
    pub tells: Vec<String>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct DebriefText {
    pub intro_clean: String,
    pub intro_mistakes: String,
    /// intro when the run ended Overwhelmed (health ran out): no praise
    pub intro_lost: String,
    /// placeholders: {floor} {who} {choice} {flag} {tip}
    pub line_choice: String,
    pub line_keeper: String,
    pub line_spot: String,
    pub line_scam: String,
    pub outro: Vec<String>,
    pub clean_tips: Vec<String>,
    /// habits for an Overwhelmed run with nothing logged (clean_tips would praise)
    pub lost_tips: Vec<String>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct FeatureData {
    pub owl_name: String,
    pub red_flags: BTreeMap<String, FlagText>,
    pub scam_templates: Vec<ScamTemplate>,
    pub safe_messages: Vec<SafeMessage>,
    /// "default" plus optional per-encounter-id lists
    pub keeper_taunts: BTreeMap<String, Vec<String>>,
    pub owl_hints: BTreeMap<String, Vec<String>>,
    /// up_standard, up_sharp, up_expert, down_gentle, down_standard, down_sharp
    pub tier_lines: BTreeMap<String, Vec<String>>,
    pub debrief: DebriefText,
}

pub const TIER_KEYS: [&str; 6] = ["up_standard", "up_sharp", "up_expert", "down_gentle", "down_standard", "down_sharp"];

impl FeatureData {
    /// The per-encounter list if present and non-empty, else "default".
    pub fn pick_list<'a>(map: &'a BTreeMap<String, Vec<String>>, encounter_id: &str) -> &'a [String] {
        match map.get(encounter_id) {
            Some(v) if !v.is_empty() => v,
            _ => map.get("default").map(Vec::as_slice).unwrap_or(&[]),
        }
    }
}

fn fold_all(d: &mut FeatureData) {
    let f = |s: &mut String| *s = fold_for_font(s);
    f(&mut d.owl_name);
    for t in d.red_flags.values_mut() {
        f(&mut t.label);
        f(&mut t.tip);
    }
    for t in &mut d.scam_templates {
        for s in [&mut t.sender_name, &mut t.subject, &mut t.topic, &mut t.sign_off, &mut t.fake_address] {
            f(s);
        }
        t.flag_lines.values_mut().for_each(f);
        t.bodies.iter_mut().for_each(f);
    }
    for m in &mut d.safe_messages {
        for s in [&mut m.sender, &mut m.subject, &mut m.body] {
            f(s);
        }
        m.tells.iter_mut().for_each(f);
    }
    for map in [&mut d.keeper_taunts, &mut d.owl_hints, &mut d.tier_lines] {
        map.values_mut().flat_map(|v| v.iter_mut()).for_each(f);
    }
    let b = &mut d.debrief;
    for s in [&mut b.intro_clean, &mut b.intro_mistakes, &mut b.intro_lost, &mut b.line_choice, &mut b.line_keeper, &mut b.line_spot, &mut b.line_scam] {
        f(s);
    }
    b.outro.iter_mut().for_each(f);
    b.clean_tips.iter_mut().for_each(f);
    b.lost_tips.iter_mut().for_each(f);
}

/// Parse and validate. Errors only when the file can't drive the features
/// (bad JSON, no usable scam template or safe message); everything else is a warning
/// and is repaired with built-in defaults.
pub fn parse(json: &str) -> Result<(FeatureData, Vec<String>), String> {
    use super::scamgen::RedFlag;
    let mut d: FeatureData = serde_json::from_str(json).map_err(|e| format!("ai_features.json: {e}"))?;
    fold_all(&mut d);
    let mut warn = Vec::new();
    if d.owl_name.trim().is_empty() {
        d.owl_name = "Hoot".into();
    }
    for t in &mut d.scam_templates {
        let known: Vec<String> = t.flags.iter().filter(|f| RedFlag::from_id(f).is_some()).cloned().collect();
        for bad in t.flags.iter().filter(|f| RedFlag::from_id(f).is_none()) {
            warn.push(format!("scam template '{}': unknown red flag '{bad}' (ignored)", t.id));
        }
        t.flags = known;
        if !t.fake_address.ends_with(FICTIONAL_SUFFIX) || !t.fake_address.contains('@') {
            warn.push(format!("scam template '{}': fake_address must be a fictional ...{FICTIONAL_SUFFIX} address (using {DEFAULT_FAKE_SENDER})", t.id));
            t.fake_address = DEFAULT_FAKE_SENDER.into();
        }
        for id in &t.flags {
            let flag = RedFlag::from_id(id).expect("filtered above");
            if flag.in_header() {
                continue;
            }
            match t.flag_lines.get(id) {
                Some(l) if flag.present_in(l) => {}
                Some(l) => warn.push(format!("scam template '{}': flag_line for '{id}' has none of the cue words {:?}: \"{l}\" (built-in line used)", t.id, flag.markers())),
                None => warn.push(format!("scam template '{}': no flag_line for '{id}' (built-in line used)", t.id)),
            }
        }
        t.bodies.retain(|b| !b.trim().is_empty());
        if t.bodies.is_empty() {
            warn.push(format!("scam template '{}': no bodies (generic body used)", t.id));
        }
    }
    d.scam_templates.retain(|t| t.flags.len() >= 2);
    if d.scam_templates.is_empty() {
        return Err("ai_features.json: no scam template with at least 2 known red flags".into());
    }
    d.safe_messages.retain(|m| !m.body.trim().is_empty());
    if d.safe_messages.is_empty() {
        return Err("ai_features.json: no safe_messages".into());
    }
    for m in &d.safe_messages {
        let text = format!("{} {} {}", m.sender, m.subject, m.body);
        for f in RedFlag::ALL.iter().filter(|f| !f.in_header() && f.present_in(&text)) {
            warn.push(format!("safe message '{}' contains a cue word for '{}' (it should read as clearly safe)", m.id, f.id()));
        }
        if m.sender.contains(DEFAULT_FAKE_SENDER.split('@').nth(1).unwrap_or("freemail")) {
            warn.push(format!("safe message '{}' uses the fake sender domain", m.id));
        }
    }
    for (what, map) in [("keeper_taunts", &d.keeper_taunts), ("owl_hints", &d.owl_hints)] {
        if map.get("default").is_none_or(|v| v.is_empty()) {
            warn.push(format!("{what} has no \"default\" list (built-in lines used)"));
        }
    }
    for k in TIER_KEYS {
        if d.tier_lines.get(k).is_none_or(|v| v.is_empty()) {
            warn.push(format!("tier_lines.{k} is missing (built-in line used)"));
        }
    }
    let b = &d.debrief;
    for (k, v) in [("intro_clean", &b.intro_clean), ("intro_mistakes", &b.intro_mistakes), ("intro_lost", &b.intro_lost), ("line_choice", &b.line_choice), ("line_keeper", &b.line_keeper), ("line_spot", &b.line_spot), ("line_scam", &b.line_scam)] {
        if v.trim().is_empty() {
            warn.push(format!("debrief.{k} is empty (built-in text used)"));
        }
    }
    Ok((d, warn))
}

/// System prompts, one per task, each prefixed with the shared rules.
#[derive(Clone, Debug)]
pub struct Prompts {
    pub scam_body: String,
    pub floor_pack: String,
    pub run_pack: String,
    pub debrief: String,
}

pub const PROMPT_SECTIONS: [&str; 4] = ["scam_body", "floor_pack", "run_pack", "debrief"];

/// `## name` sections (name = first word after `## `, lowercased) -> body text.
fn sections(md: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let mut cur: Option<String> = None;
    let mut buf = String::new();
    for line in md.lines() {
        if let Some(h) = line.strip_prefix("## ") {
            if let Some(name) = cur.take() {
                out.insert(name, buf.trim().to_string());
            }
            buf.clear();
            cur = h.split_whitespace().next().map(str::to_ascii_lowercase);
        } else if cur.is_some() {
            buf.push_str(line);
            buf.push('\n');
        }
    }
    if let Some(name) = cur {
        out.insert(name, buf.trim().to_string());
    }
    out
}

/// Build prompts from `md`; any missing/empty section comes from the embedded file.
pub fn parse_prompts(md: &str) -> (Prompts, Vec<String>) {
    let live = sections(md);
    let embedded = sections(EMBEDDED_PROMPTS);
    let mut warn = Vec::new();
    let mut get = |name: &str| -> String {
        match live.get(name) {
            Some(s) if !s.is_empty() => s.clone(),
            _ => {
                if md != EMBEDDED_PROMPTS {
                    warn.push(format!("ai_features_prompts.md: section '## {name}' missing (using built-in)"));
                }
                embedded.get(name).cloned().unwrap_or_default()
            }
        }
    };
    let shared = get("shared");
    let mut task = |name: &str| format!("{shared}\n\n{}", get(name));
    let p = Prompts { scam_body: task("scam_body"), floor_pack: task("floor_pack"), run_pack: task("run_pack"), debrief: task("debrief") };
    (p, warn)
}

pub struct Assets {
    pub data: FeatureData,
    pub prompts: Prompts,
    pub warnings: Vec<String>,
}

/// Embedded data + prompts (always valid; checked by tests).
pub fn embedded() -> Assets {
    let (data, warnings) = parse(EMBEDDED_DATA).expect("embedded ai_features.json must parse (checked by unit tests)");
    Assets { data, prompts: parse_prompts(EMBEDDED_PROMPTS).0, warnings }
}

/// Live files from `dir` when present and valid, else embedded. `None` = web build.
pub fn load_from(dir: Option<&str>) -> Assets {
    let mut a = embedded();
    let Some(dir) = dir else { return a };
    let path = format!("{dir}/ai_features.json");
    match std::fs::read_to_string(&path) {
        Ok(json) => match parse(&json) {
            Ok((data, w)) => {
                a.data = data;
                a.warnings = w;
            }
            Err(e) => a.warnings.push(format!("{path}: {e} (using embedded)")),
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => a.warnings.push(format!("{path}: {e} (using embedded)")),
    }
    if let Ok(md) = std::fs::read_to_string(format!("{dir}/ai_features_prompts.md")) {
        if !md.trim().is_empty() {
            let (p, w) = parse_prompts(&md);
            a.prompts = p;
            a.warnings.extend(w);
        }
    }
    a
}

/// Same lookup as `content::load`.
pub fn load() -> Assets {
    #[cfg(not(target_arch = "wasm32"))]
    let a = load_from(Some(&std::env::var("CAMPUS_CRYPT_CONTENT").unwrap_or_else(|_| "assets/content".into())));
    #[cfg(target_arch = "wasm32")]
    let a = load_from(None);
    for w in &a.warnings {
        eprintln!("[ai_features] {w}");
    }
    a
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_data_parses_without_warnings() {
        let (d, w) = parse(EMBEDDED_DATA).unwrap();
        assert!(w.is_empty(), "{w:#?}");
        assert!(d.scam_templates.len() >= 4 && d.safe_messages.len() >= 3);
        for t in &d.scam_templates {
            assert!(t.fake_address.ends_with(FICTIONAL_SUFFIX), "{}", t.id);
        }
    }

    #[test]
    fn embedded_prompts_have_every_section() {
        let s = sections(EMBEDDED_PROMPTS);
        for name in PROMPT_SECTIONS.iter().chain(["shared"].iter()) {
            assert!(s.get(*name).is_some_and(|b| b.len() > 100), "section {name}");
        }
        let (p, w) = parse_prompts(EMBEDDED_PROMPTS);
        assert!(w.is_empty());
        assert!(p.floor_pack.starts_with("You write short flavor text") && p.floor_pack.contains("TAUNT:"));
    }

    #[test]
    fn missing_prompt_section_uses_builtin() {
        let (p, w) = parse_prompts("## shared\nBe nice.\n\n## debrief\nCustom debrief.\n");
        assert!(p.debrief.contains("Custom debrief") && p.debrief.starts_with("Be nice."));
        assert!(p.floor_pack.contains("TAUNT:"));
        assert_eq!(w.len(), 3, "{w:?}");
    }

    #[test]
    fn bad_files_are_repaired_or_rejected() {
        assert!(parse("{").is_err());
        assert!(parse(r#"{"scam_templates": [], "safe_messages": [{"body": "x"}]}"#).is_err());
        let json = r#"{
            "scam_templates": [{"id": "t", "fake_address": "hr@realcompany.com", "flags": ["urgency", "gift_cards", "telepathy"],
                                "flag_lines": {"urgency": "Please consider it."}}],
            "safe_messages": [{"id": "s", "body": "Send me your password today."}]
        }"#;
        let (d, w) = parse(json).unwrap();
        assert_eq!(d.scam_templates[0].fake_address, DEFAULT_FAKE_SENDER);
        assert_eq!(d.scam_templates[0].flags, ["urgency", "gift_cards"]);
        let all = w.join("\n");
        for needle in ["telepathy", "fictional", "cue words", "no flag_line for 'gift_cards'", "no bodies", "safe message 's'", "tier_lines.up_sharp", "owl_hints"] {
            assert!(all.contains(needle), "missing warning '{needle}' in:\n{all}");
        }
    }

    /// Words that give the answer away if they sit in a header.
    const LEAKS: [&str; 14] = [
        "scam", "fake", "phish", "official", "suspicious", "fraud", "spoof", "red flag", "too good to be true", "legit",
        "trap", "no interview", "not real", "verified",
    ];

    /// Every host-looking token (`a.b`, `x@y.z`) in `s`, lowercased, trailing punctuation trimmed.
    fn hosts(s: &str) -> Vec<String> {
        s.split(|c: char| c.is_whitespace() || matches!(c, '<' | '>' | '(' | ')' | ','))
            .map(|t| t.trim_matches(|c: char| !c.is_ascii_alphanumeric()).to_ascii_lowercase())
            .filter(|t| t.contains('@') || crate::ai_features::validate::has_host(t))
            .collect()
    }

    /// Scam and safe message headers (sender display, subject, sign-off) never
    /// hint at the answer: no red-flag cue words or labels, no giveaway words, and
    /// only fictional `.example` addresses.
    #[test]
    fn headers_do_not_leak_the_answer() {
        use crate::ai_features::scamgen::RedFlag;
        use crate::ai_features::validate::{has_phrase, norm};
        let d = parse(EMBEDDED_DATA).unwrap().0;
        let labels: Vec<String> = RedFlag::ALL
            .iter()
            .map(|f| d.red_flags.get(f.id()).map(|t| t.label.clone()).filter(|l| !l.is_empty()).unwrap_or(f.default_label().into()))
            .collect();
        let mut headers: Vec<(String, &str, &str)> = Vec::new();
        for t in &d.scam_templates {
            headers.push((format!("scam {} sender_name", t.id), &t.sender_name, ""));
            headers.push((format!("scam {} subject", t.id), &t.subject, ""));
            headers.push((format!("scam {} sign_off", t.id), &t.sign_off, ""));
            headers.push((format!("scam {} fake_address", t.id), "", &t.fake_address));
        }
        for m in &d.safe_messages {
            // the safe sender may carry a placeholder address: check its display part
            let (name, addr) = m.sender.split_once('<').unwrap_or((&m.sender, ""));
            headers.push((format!("safe {} sender", m.id), name, addr));
            headers.push((format!("safe {} subject", m.id), &m.subject, ""));
        }
        let mut bad = Vec::new();
        for (what, text, addr) in &headers {
            let n = norm(text);
            let lower = text.to_ascii_lowercase();
            for w in LEAKS.iter().filter(|w| lower.contains(*w)) {
                bad.push(format!("{what}: giveaway word '{w}' in \"{text}\""));
            }
            for l in labels.iter().filter(|l| has_phrase(&n, l)) {
                bad.push(format!("{what}: red-flag label '{l}' in \"{text}\""));
            }
            for f in RedFlag::ALL.iter().filter(|f| f.present_in(text)) {
                bad.push(format!("{what}: cue word for {} in \"{text}\"", f.id()));
            }
            for h in hosts(text).into_iter().chain(hosts(addr)) {
                if !h.ends_with(FICTIONAL_SUFFIX) {
                    bad.push(format!("{what}: non-fictional domain '{h}'"));
                }
            }
        }
        // the shown address itself
        for t in &d.scam_templates {
            assert!(t.fake_address.contains('@') && t.fake_address.ends_with(FICTIONAL_SUFFIX), "{}", t.id);
        }
        // and the checker catches what it should
        assert!(hosts("Jobs <hr@realjobs.com>").iter().any(|h| !h.ends_with(FICTIONAL_SUFFIX)));
        assert!(norm("Remote internship, no interview needed!").contains(" no interview "));
        assert!(bad.is_empty(), "{} header problems:\n{}", bad.len(), bad.join("\n"));
    }

    #[test]
    fn missing_live_dir_uses_embedded() {
        let a = load_from(Some("/definitely/not/here"));
        assert!(a.warnings.is_empty());
        assert!(!a.data.scam_templates.is_empty());
    }
}
