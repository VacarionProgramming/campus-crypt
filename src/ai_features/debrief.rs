//! Post-run debrief: a short study guide built ONLY from a deterministic
//! mistake log.
//!
//! `MistakeLog::from_run` reads the run's decisions and Keeper results from
//! state.rs (`Run::history` / `Run::keepers`), plus the mini-game results the
//! UI recorded in `FeatureLog`. The AI writes 3-5 lines from that log; lines that
//! mention a floor or character not in the log are rejected and the templated
//! debrief (built from the same log) is used instead.

use super::data::FeatureData;
use super::scamgen::RedFlag;
use super::validate::{game_line, has_domain_trust, has_phrase, norm, speech};
use crate::ai::LineSource;
use crate::content::{Content, Outcome};
use crate::state::{Ending, Run};

pub const MIN_LINES: usize = 3;
pub const MAX_LINES: usize = 5;
/// Longest debrief line, AI or templated (the ending screen's wrap budget: two
/// lines of the pixel font). Templated lines are checked for every encounter and
/// choice by `every_rendered_debrief_line_fits`.
pub const MAX_DEBRIEF_LINE_CHARS: usize = 120;
/// Mistakes listed in the prompt / templated debrief (the rest are summarized).
pub const MAX_LISTED: usize = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum MistakeKind {
    /// a risky choice in the floor's encounter
    RiskyChoice,
    /// a mixed choice
    MixedChoice,
    /// a wrong Floor Keeper answer
    KeeperWrong,
    /// picked the safe message in spot-the-fake
    SpotMissed,
    /// trusted a generated scam message
    ScamMissed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mistake {
    /// 0-based floor
    pub floor: usize,
    pub kind: MistakeKind,
    pub encounter_id: String,
    /// the character, e.g. "The Inbox Mimic" ("" for mini-games)
    pub who: String,
    /// the choice label ("" if none)
    pub choice: String,
    /// the warning sign / red flag that was missed
    pub red_flag: String,
    /// the safer move
    pub tip: String,
}

/// Mini-game results the UI reports (deterministic: what the player did).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FeatureLog {
    pub entries: Vec<Mistake>,
    pub spot_rounds: u32,
    pub spot_correct: u32,
}

impl FeatureLog {
    pub fn record_spot(&mut self, floor: usize, correct: bool, fake_flag_label: &str, tip: &str) {
        self.spot_rounds += 1;
        if correct {
            self.spot_correct += 1;
        } else {
            self.entries.push(Mistake {
                floor,
                kind: MistakeKind::SpotMissed,
                encounter_id: "spot_the_fake".into(),
                who: String::new(),
                choice: String::new(),
                red_flag: fake_flag_label.into(),
                tip: tip.into(),
            });
        }
    }
    pub fn record_scam(&mut self, floor: usize, who: &str, trusted_it: bool, flag_label: &str, tip: &str) {
        if trusted_it {
            self.entries.push(Mistake {
                floor,
                kind: MistakeKind::ScamMissed,
                encounter_id: "scam_message".into(),
                who: who.into(),
                choice: String::new(),
                red_flag: flag_label.into(),
                tip: tip.into(),
            });
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MistakeLog {
    /// sorted by floor, then kind
    pub mistakes: Vec<Mistake>,
    pub floors_played: usize,
    pub decisions: usize,
    /// how the run ended (None: not known, treated like Survivor)
    pub ending: Option<Ending>,
}

/// Praise that doesn't belong in a study guide for a run that ended Overwhelmed.
const PRAISE: [&str; 30] = [
    "great", "nice", "awesome", "amazing", "well done", "good job", "great job", "congrats", "congratulations",
    "brilliant", "excellent", "fantastic", "perfect", "flawless", "nailed", "proud", "victory", "triumph", "win",
    "won", "winner", "clean run", "champion", "superb", "impressive", "stellar", "crushed", "bravo", "sharp",
    "survived",
];
/// Claims of a spotless run, wrong whenever anything was logged.
const SPOTLESS: [&str; 7] = ["clean run", "flawless", "perfect run", "perfect", "every trap", "not a single", "no mistakes"];

/// First warning sign that doesn't lean on a domain as proof of safety
/// ("The link's domain is not a campus .edu address."), else the first one.
fn pick_sign(signs: &[String], from: usize) -> String {
    signs.iter().skip(from).chain(signs.iter()).find(|s| !has_domain_trust(s)).or(signs.get(from)).or(signs.first()).cloned().unwrap_or_default()
}

impl MistakeLog {
    pub fn from_run(run: &Run, content: &Content, extra: &FeatureLog) -> MistakeLog {
        let mut m = Vec::new();
        for d in &run.history {
            let Some(e) = content.encounters.get(d.encounter) else { continue };
            let Some(ch) = e.choices.get(d.choice) else { continue };
            let kind = match d.outcome {
                Outcome::Safe => continue,
                Outcome::Risky => MistakeKind::RiskyChoice,
                Outcome::Mixed => MistakeKind::MixedChoice,
            };
            m.push(Mistake {
                floor: d.floor,
                kind,
                encounter_id: e.id.clone(),
                who: e.character.name.clone(),
                choice: ch.label.clone(),
                red_flag: pick_sign(&ch.warning_signs, 0),
                tip: ch.safer_next_move.clone(),
            });
        }
        for k in run.keepers.iter().filter(|k| !k.correct) {
            let d = run.history.iter().find(|d| d.floor == k.floor);
            let e = d.and_then(|d| content.encounters.get(d.encounter));
            let ch = d.zip(e).and_then(|(d, e)| e.choices.get(d.choice));
            let answer = ch.and_then(|c| c.keeper.as_ref()).map(|k| k.answer.clone()).filter(|a| !a.is_empty());
            m.push(Mistake {
                floor: k.floor,
                kind: MistakeKind::KeeperWrong,
                encounter_id: e.map(|e| e.id.clone()).unwrap_or_default(),
                who: e.map(|e| e.character.name.clone()).unwrap_or_default(),
                choice: ch.map(|c| c.label.clone()).unwrap_or_default(),
                red_flag: ch.map(|c| pick_sign(&c.warning_signs, 1)).unwrap_or_default(),
                tip: answer.or(ch.map(|c| c.safer_next_move.clone())).unwrap_or_default(),
            });
        }
        m.extend(extra.entries.iter().cloned());
        m.sort_by_key(|a| (a.floor, a.kind));
        let floors_played = run.history.iter().map(|d| d.floor + 1).max().unwrap_or(0);
        MistakeLog {
            mistakes: m,
            floors_played,
            decisions: run.history.len() + run.keepers.len() + extra.spot_rounds as usize,
            ending: Some(run.ending()),
        }
    }

    /// 1-based floor numbers that appear in the log.
    pub fn floors(&self) -> Vec<usize> {
        let mut f: Vec<usize> = self.mistakes.iter().map(|m| m.floor + 1).collect();
        f.dedup();
        f
    }

    /// The facts sent to the AI (system prompt: `## debrief`).
    pub fn user_message(&self) -> String {
        let ending = match self.ending {
            Some(Ending::CryptKeeper) => "Crypt Keeper ending: the student finished every floor",
            Some(Ending::Overwhelmed) => "Overwhelmed ending: the student ran out of health and did not finish (no praise)",
            _ => "Survivor ending: the student finished, with some slips",
        };
        let mut s = format!(
            "TASK: debrief\nhow the run ended: {ending}\nfloors played: {}\nmistakes logged: {}\nmistake log:\n",
            self.floors_played,
            self.mistakes.len()
        );
        if self.mistakes.is_empty() {
            s.push_str("- none: every choice was safe and every question was answered correctly\n");
        }
        for m in self.mistakes.iter().take(MAX_LISTED) {
            let what = match m.kind {
                MistakeKind::RiskyChoice => format!("{}: picked {} (a risky move)", m.who, m.choice),
                MistakeKind::MixedChoice => format!("{}: picked {} (a half-safe move)", m.who, m.choice),
                MistakeKind::KeeperWrong => "the Floor Keeper's question: answered wrong".to_string(),
                MistakeKind::SpotMissed => "spot the fake: trusted the fake message".to_string(),
                MistakeKind::ScamMissed => format!("{}: trusted a scam message", m.who),
            };
            s.push_str(&format!("- Floor {}, {what}. Warning sign: {} Safer move: {}\n", m.floor + 1, m.red_flag, m.tip));
        }
        if self.mistakes.len() > MAX_LISTED {
            s.push_str(&format!("- plus {} smaller slips (do not list them)\n", self.mistakes.len() - MAX_LISTED));
        }
        s.push_str("\nWrite the study guide now.");
        s
    }
}

/// "The Inbox Mimic" -> "inbox mimic" (what a model would write).
fn short_name(name: &str) -> String {
    let n = name.trim();
    n.strip_prefix("The ").unwrap_or(n).to_ascii_lowercase()
}

/// Validate an AI debrief against the log. Every line must pass the game-line
/// filter, and no line may mention a floor or a character the log doesn't have.
pub fn parse_ai(raw: &str, log: &MistakeLog, content: &Content) -> Option<Vec<String>> {
    let floors = log.floors();
    let logged: Vec<&str> = log.mistakes.iter().map(|m| m.encounter_id.as_str()).collect();
    let foreign: Vec<String> =
        content.encounters.iter().filter(|e| !logged.contains(&e.id.as_str())).map(|e| short_name(&e.character.name)).collect();
    let mut lines = Vec::new();
    for raw_line in raw.lines() {
        let t = raw_line.trim().trim_start_matches(['-', '*', ' ']).trim_start_matches(|c: char| c.is_ascii_digit() || c == '.' || c == ')');
        if t.trim().is_empty() {
            continue;
        }
        let line = game_line(&speech(t), MAX_DEBRIEF_LINE_CHARS)?;
        let n = norm(&line);
        if foreign.iter().any(|f| has_phrase(&n, f)) {
            return None;
        }
        // tone follows the ending code computed; a domain is never proof of safety
        if (log.ending == Some(Ending::Overwhelmed) && PRAISE.iter().any(|p| has_phrase(&n, p)))
            || (!log.mistakes.is_empty() && SPOTLESS.iter().any(|p| has_phrase(&n, p)))
            || has_domain_trust(&line)
        {
            return None;
        }
        // every "floor N" must be a logged floor
        let words: Vec<&str> = n.split_whitespace().collect();
        for w in words.windows(2) {
            if w[0] == "floor" {
                if let Ok(f) = w[1].parse::<usize>() {
                    if !floors.contains(&f) {
                        return None;
                    }
                }
            }
        }
        lines.push(line);
    }
    (MIN_LINES..=MAX_LINES).contains(&lines.len()).then_some(lines)
}

fn fill(tpl: &str, m: &Mistake) -> String {
    let flag = m.red_flag.trim_end_matches(['.', '!', '?']);
    let grade = if m.kind == MistakeKind::MixedChoice { "half-safe" } else { "risky" };
    tpl.replace("{floor}", &(m.floor + 1).to_string())
        .replace("{grade}", grade)
        .replace("{who}", &m.who)
        .replace("{choice}", &m.choice)
        .replace("{flag}", flag)
        .replace("{tip}", &m.tip)
}

/// The templated debrief: same log, same 3-5 line shape.
pub fn fallback(log: &MistakeLog, data: &FeatureData) -> Vec<String> {
    let b = &data.debrief;
    let or = |s: &str, d: &str| if s.trim().is_empty() { d.to_string() } else { s.to_string() };
    let mut out = Vec::new();
    let lost = log.ending == Some(Ending::Overwhelmed);
    if lost && log.mistakes.is_empty() {
        out.push(or(&b.intro_lost, "The crypt wore you down this time, Owl. Here is what to practice next run."));
        let tips = if b.lost_tips.is_empty() { vec!["Take each message slowly: check who sent it and look things up yourself.".to_string()] } else { b.lost_tips.clone() };
        out.extend(tips.into_iter().take(MAX_LINES - 1));
        while out.len() < MIN_LINES {
            out.push("When a message rushes you, that rush is your cue to slow down.".into());
        }
        return out;
    }
    if log.mistakes.is_empty() {
        out.push(or(&b.intro_clean, "Clean run, Owl! You read every trap the crypt set."));
        let tips = if b.clean_tips.is_empty() { vec!["Keep checking senders and looking things up yourself.".to_string()] } else { b.clean_tips.clone() };
        out.extend(tips.into_iter().take(MAX_LINES - 1));
        while out.len() < MIN_LINES {
            out.push("When a message rushes you, that rush is your cue to slow down.".into());
        }
        return out;
    }
    if lost {
        out.push(or(&b.intro_lost, "The crypt wore you down this time, Owl. Here is what to practice next run."));
    } else {
        out.push(or(&b.intro_mistakes, "Here is your study guide from this run, Owl."));
    }
    for m in log.mistakes.iter().take(MAX_LISTED) {
        out.push(render_line(m, data).0);
    }
    out.push(b.outro.first().cloned().unwrap_or_else(|| "Next time, pause and check through an official channel you look up yourself.".into()));
    out
}

/// Which form `render_line` had to use to stay within `MAX_DEBRIEF_LINE_CHARS`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fit {
    /// the content file's template
    Full,
    /// built-in short form, keeps the warning sign / tip
    Compact,
    /// built-in shortest form (no quoted content text)
    Minimal,
}

/// One mistake line: the content template if it fits, else shorter built-in forms,
/// so a rendered line never exceeds `MAX_DEBRIEF_LINE_CHARS`.
pub fn render_line(m: &Mistake, data: &FeatureData) -> (String, Fit) {
    let full = full_template(m, data);
    let (compact, minimal) = match m.kind {
        MistakeKind::RiskyChoice | MistakeKind::MixedChoice => ("Floor {floor}, {who}: watch for: {flag}", "Floor {floor}, {who}: {choice} was a {grade} pick."),
        MistakeKind::KeeperWrong => ("Floor {floor}, Keeper tip: {tip}", "Floor {floor}: the Floor Keeper's question is worth another look."),
        MistakeKind::SpotMissed => ("Floor {floor}: a fake slipped by. Giveaway: {flag}.", "Floor {floor}: a fake message slipped by you."),
        MistakeKind::ScamMissed => ("Floor {floor}, {who}: a trap. Giveaway: {flag}.", "Floor {floor}: a scam message slipped by you."),
    };
    let mut last = String::new();
    for (tpl, fit) in [(full.as_str(), Fit::Full), (compact, Fit::Compact), (minimal, Fit::Minimal)] {
        let line = filled_line(tpl, m);
        if line.len() <= MAX_DEBRIEF_LINE_CHARS {
            return (line, fit);
        }
        last = line;
    }
    // only reachable with absurd names; cut at a word boundary
    let cut = last[..MAX_DEBRIEF_LINE_CHARS - 3].rfind(' ').unwrap_or(MAX_DEBRIEF_LINE_CHARS - 3);
    (format!("{}...", &last[..cut]), Fit::Minimal)
}

fn filled_line(tpl: &str, m: &Mistake) -> String {
    let mut line = fill(tpl, m);
    if !line.ends_with(['.', '!', '?']) {
        line.push('.');
    }
    line
}

/// The content file's line for `m`, before any shortening (for authoring checks).
pub fn full_line(m: &Mistake, data: &FeatureData) -> String {
    filled_line(&full_template(m, data), m)
}

fn full_template(m: &Mistake, data: &FeatureData) -> String {
    let b = &data.debrief;
    let or = |s: &str, d: &str| if s.trim().is_empty() { d.to_string() } else { s.to_string() };
    match m.kind {
        MistakeKind::RiskyChoice | MistakeKind::MixedChoice => or(&b.line_choice, "Floor {floor}, {who}: you picked {choice}. Watch for this: {flag}"),
        MistakeKind::KeeperWrong => or(&b.line_keeper, "Floor {floor}, the Floor Keeper: worth a second look. Safer move: {tip}"),
        MistakeKind::SpotMissed => or(&b.line_spot, "Floor {floor}, spot the fake: its giveaway was {flag}."),
        MistakeKind::ScamMissed => or(&b.line_scam, "Floor {floor}, {who}: that message was a trap. Its giveaway: {flag}."),
    }
}

/// A finished debrief for the ending screen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Debrief {
    pub lines: Vec<String>,
    pub source: LineSource,
}

/// Label for a flag (content label, else built-in) for `FeatureLog` records.
pub fn flag_label(data: &FeatureData, f: RedFlag) -> String {
    data.red_flags.get(f.id()).map(|t| t.label.clone()).filter(|l| !l.is_empty()).unwrap_or(f.default_label().into())
}

pub fn flag_tip(data: &FeatureData, f: RedFlag) -> String {
    data.red_flags.get(f.id()).map(|t| t.tip.clone()).filter(|l| !l.is_empty()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai_features::data::embedded;
    use crate::content::{parse, EMBEDDED_ENCOUNTERS};
    use crate::state;

    fn content() -> Content {
        parse(EMBEDDED_ENCOUNTERS).unwrap().0
    }

    /// Play `picks` (outcome per floor) and keeper answers through the real state machine.
    fn play(c: &Content, picks: &[(Outcome, bool)]) -> Run {
        let n = c.encounters.len();
        let mut r = Run::new(state::floors_for(n), n);
        for &(o, keeper_ok) in picks {
            r.begin_encounter();
            let e = &c.encounters[r.encounter()];
            let i = e.choices.iter().position(|ch| ch.outcome == o).unwrap_or(0);
            r.choose(c, i).unwrap();
            r.reveal_lesson(c);
            r.advance();
            if r.phase != state::Phase::Explore {
                break;
            }
            r.begin_keeper();
            r.answer_keeper(keeper_ok);
            r.leave_verdict();
            while r.phase == state::Phase::BossFight {
                r.boss_defeated();
            }
            r.descend();
        }
        r
    }

    #[test]
    fn log_is_deterministic_and_lists_only_real_mistakes() {
        let c = content();
        let picks = [(Outcome::Safe, true), (Outcome::Risky, true), (Outcome::Mixed, false), (Outcome::Safe, true), (Outcome::Safe, false)];
        let mut extra = FeatureLog::default();
        extra.record_spot(3, false, "Gift card payment", "Nobody legit asks for gift cards.");
        extra.record_spot(4, true, "x", "y");
        let r = play(&c, &picks);
        let a = MistakeLog::from_run(&r, &c, &extra);
        let b = MistakeLog::from_run(&play(&c, &picks), &c, &extra);
        assert_eq!(a, b);
        let kinds: Vec<(usize, MistakeKind)> = a.mistakes.iter().map(|m| (m.floor, m.kind)).collect();
        assert_eq!(
            kinds,
            vec![(1, MistakeKind::RiskyChoice), (2, MistakeKind::MixedChoice), (2, MistakeKind::KeeperWrong), (3, MistakeKind::SpotMissed), (4, MistakeKind::KeeperWrong)]
        );
        assert_eq!(a.floors(), vec![2, 3, 4, 5]);
        assert_eq!(a.mistakes[0].who, c.encounters[1].character.name);
        assert!(!a.mistakes[0].red_flag.is_empty() && !a.mistakes[0].tip.is_empty());
        assert_eq!(a.floors_played, 5);
    }

    #[test]
    fn ten_floor_run_maps_floors_past_the_file_to_encounters() {
        let c = content();
        let picks = vec![(Outcome::Safe, true); 9].into_iter().chain([(Outcome::Risky, true)]).collect::<Vec<_>>();
        let r = play(&c, &picks);
        let log = MistakeLog::from_run(&r, &c, &FeatureLog::default());
        assert_eq!(log.mistakes.len(), 1);
        let m = &log.mistakes[0];
        assert_eq!(m.floor, 9);
        assert_eq!(m.encounter_id, c.encounters[state::encounter_for_floor(9, c.encounters.len())].id);
    }

    #[test]
    fn fallback_debrief_is_built_from_the_log() {
        let c = content();
        let d = embedded().data;
        let clean = MistakeLog::from_run(&play(&c, &[(Outcome::Safe, true); 3]), &c, &FeatureLog::default());
        let lines = fallback(&clean, &d);
        assert!((MIN_LINES..=MAX_LINES).contains(&lines.len()) && lines[0].starts_with("Clean run"), "{lines:?}");
        let r = play(&c, &[(Outcome::Risky, false), (Outcome::Risky, false), (Outcome::Risky, true)]);
        let log = MistakeLog::from_run(&r, &c, &FeatureLog::default());
        let lines = fallback(&log, &d);
        assert_eq!(lines.len(), 5, "intro + 3 listed + outro: {lines:?}");
        assert!(lines[1].starts_with("Floor 1, The Inbox Mimic: "), "{}", lines[1]);
        assert!(lines.iter().all(|l| l.is_ascii() && l.ends_with(['.', '!', '?'])), "{lines:?}");
        let one = MistakeLog::from_run(&play(&c, &[(Outcome::Safe, false)]), &c, &FeatureLog::default());
        assert_eq!(fallback(&one, &d).len(), 3);
    }

    /// Every templated line the debrief can render, for every encounter and choice
    /// in the shipped content (choice and Keeper mistakes), every red flag (spot and
    /// scam mistakes, with every scam sender as {who}), plus intros, tips and outro.
    #[test]
    fn every_rendered_debrief_line_fits() {
        let c = content();
        let d = embedded().data;
        let mut lines: Vec<(String, String)> = Vec::new();
        let mk = |floor, kind, who: &str, choice: &str, flag: &str, tip: &str| Mistake {
            floor,
            kind,
            encounter_id: String::new(),
            who: who.into(),
            choice: choice.into(),
            red_flag: flag.into(),
            tip: tip.into(),
        };
        let compacted = std::cell::RefCell::new(Vec::new());
        let render = |m: Mistake| {
            let (line, fit) = render_line(&m, &d);
            let log = MistakeLog { mistakes: vec![m.clone()], floors_played: 10, decisions: 1, ..Default::default() };
            assert_eq!(fallback(&log, &d)[1], line, "fallback renders through render_line");
            if fit != Fit::Full {
                compacted.borrow_mut().push(format!("{fit:?} (full line {} chars) -> {line}", full_line(&m, &d).len()));
            }
            line
        };
        for (i, e) in c.encounters.iter().enumerate() {
            // a 10-floor run shows encounter i on floor i + 1
            let floor = i;
            for ch in &e.choices {
                let flag = ch.warning_signs.first().cloned().unwrap_or_default();
                if ch.outcome != Outcome::Safe {
                    let kind = if ch.outcome == Outcome::Risky { MistakeKind::RiskyChoice } else { MistakeKind::MixedChoice };
                    lines.push((format!("{}/{} choice", e.id, ch.id), render(mk(floor, kind, &e.character.name, &ch.label, &flag, &ch.safer_next_move))));
                }
                let tip = ch.keeper.as_ref().map(|k| k.answer.clone()).filter(|a| !a.is_empty()).unwrap_or(ch.safer_next_move.clone());
                lines.push((format!("{}/{} keeper", e.id, ch.id), render(mk(floor, MistakeKind::KeeperWrong, &e.character.name, &ch.label, &flag, &tip))));
            }
        }
        for f in RedFlag::ALL {
            let label = flag_label(&d, f);
            lines.push((format!("spot/{}", f.id()), render(mk(9, MistakeKind::SpotMissed, "", "", &label, ""))));
            for t in &d.scam_templates {
                let who = t.sender_name.clone();
                lines.push((format!("scam/{}/{}", t.id, f.id()), render(mk(9, MistakeKind::ScamMissed, &who, "", &label, ""))));
            }
        }
        let b = &d.debrief;
        for (k, v) in [("intro_clean", &b.intro_clean), ("intro_mistakes", &b.intro_mistakes), ("intro_lost", &b.intro_lost)] {
            lines.push((k.into(), v.clone()));
        }
        lines.extend(b.outro.iter().chain(b.clean_tips.iter()).chain(b.lost_tips.iter()).map(|l| ("outro/tip".to_string(), l.clone())));
        let too_long: Vec<String> =
            lines.iter().filter(|(_, l)| l.len() > MAX_DEBRIEF_LINE_CHARS).map(|(k, l)| format!("{k} ({}): {l}", l.len())).collect();
        assert!(too_long.is_empty(), "{} of {} debrief lines over {MAX_DEBRIEF_LINE_CHARS} chars:\n{}", too_long.len(), lines.len(), too_long.join("\n"));
        // the content templates that didn't fit (shown with --nocapture; for Ry)
        let c = compacted.borrow();
        eprintln!("debrief: {} of {} mistake lines needed a shorter built-in form", c.len(), lines.len());
        for l in c.iter() {
            eprintln!("  {l}");
        }
    }

    #[test]
    fn overlong_lines_use_shorter_forms() {
        let d = embedded().data;
        let m = Mistake {
            floor: 9,
            kind: MistakeKind::KeeperWrong,
            encounter_id: "x".into(),
            who: "The Internship Impostor".into(),
            choice: "Comply".into(),
            red_flag: "A".repeat(200),
            tip: "word ".repeat(40),
        };
        let (l, fit) = render_line(&m, &d);
        assert_eq!((l.as_str(), fit), ("Floor 10: the Floor Keeper's question is worth another look.", Fit::Minimal));
        let m = Mistake { kind: MistakeKind::MixedChoice, tip: "x".into(), ..m };
        assert_eq!(render_line(&m, &d).0, "Floor 10, The Internship Impostor: Comply was a half-safe pick.");
        let m = Mistake { red_flag: "Pay before you check.".into(), ..m };
        assert_eq!(render_line(&m, &d).1, Fit::Full);
    }

    #[test]
    fn ai_debrief_must_stay_inside_the_log() {
        let c = content();
        let r = play(&c, &[(Outcome::Safe, true), (Outcome::Risky, true)]);
        let log = MistakeLog::from_run(&r, &c, &FeatureLog::default());
        assert_eq!(log.floors(), vec![2]);
        let good = "- Nice run, Owl, you kept your head through most of the crypt.\n\
                    - On Floor 2 the Deadline Wraith got you to put it all off; watch for that avoidance.\n\
                    - Next time, pick one small start time and stick with it.";
        assert_eq!(parse_ai(good, &log, &c).map(|l| l.len()), Some(3));
        let wrong_floor = good.replace("Floor 2", "Floor 4");
        assert!(parse_ai(&wrong_floor, &log, &c).is_none());
        let foreign = good.replace("Deadline Wraith", "Password Hydra");
        assert!(parse_ai(&foreign, &log, &c).is_none(), "mentions a character not in the log");
        assert!(parse_ai("- Great job.\n- Bye.", &log, &c).is_none(), "too few lines");
        let stats = good.replace("watch for that avoidance", "it cost you health");
        assert!(parse_ai(&stats, &log, &c).is_none());
        let msg = log.user_message();
        assert!(msg.contains("Floor 2, The Deadline Wraith: picked Postpone"), "{msg}");
    }

    #[test]
    fn debrief_tone_follows_the_ending() {
        let c = content();
        let d = embedded().data;
        let all_risky = play(&c, &[(Outcome::Risky, false); 10]);
        let log = MistakeLog::from_run(&all_risky, &c, &FeatureLog::default());
        assert_eq!(log.ending, Some(all_risky.ending()));
        assert!(log.user_message().contains("how the run ended: "));
        let clean = MistakeLog::from_run(&play(&c, &[(Outcome::Safe, true); 3]), &c, &FeatureLog::default());
        assert_eq!(clean.ending, Some(Ending::CryptKeeper));

        let one = MistakeLog::from_run(&play(&c, &[(Outcome::Risky, true)]), &c, &FeatureLog::default());
        let praise = "- Great job down there, Owl, you were close.\n- Floor 1 had a trap worth a second look next time.\n- Next time, slow down and look things up yourself.";
        let lost = MistakeLog { ending: Some(Ending::Overwhelmed), ..one.clone() };
        let survived = MistakeLog { ending: Some(Ending::Survivor), ..one.clone() };
        assert!(parse_ai(praise, &survived, &c).is_some());
        assert_eq!(parse_ai(praise, &lost, &c), None, "no praise on an Overwhelmed ending");
        let spotless = "- Clean run, Owl, not one slip anywhere.\n- Floor 1 had a trap worth a second look next time.\n- Keep looking things up yourself.";
        assert_eq!(parse_ai(spotless, &survived, &c), None, "a run with slips is never 'clean'");
        assert!(parse_ai(&praise.replace("Great job down there", "The crypt wore you down"), &lost, &c).is_some());

        // fallback: no praise on Overwhelmed, with or without logged mistakes
        for log in [lost.clone(), MistakeLog { ending: Some(Ending::Overwhelmed), ..Default::default() }] {
            let lines = fallback(&log, &d);
            assert_eq!(lines[0], d.debrief.intro_lost);
            assert!((MIN_LINES..=MAX_LINES).contains(&lines.len()));
            for l in &lines {
                let n = norm(l);
                assert!(!PRAISE.iter().chain(SPOTLESS.iter()).any(|p| has_phrase(&n, p)), "{l}");
            }
        }
        assert_eq!(fallback(&survived, &d)[0], d.debrief.intro_mistakes);
        assert_eq!(fallback(&clean, &d)[0], d.debrief.intro_clean);
    }

    #[test]
    fn debrief_never_presents_a_domain_as_proof_of_safety() {
        let c = content();
        let one = MistakeLog::from_run(&play(&c, &[(Outcome::Risky, true)]), &c, &FeatureLog::default());
        for bad in ["Trust messages from a .edu address.", "Check that it comes from the official domain.", "A campus address means it is real."] {
            let raw = format!("- The crypt had tricks this run, Owl.\n- Floor 1: {bad}\n- Next time, slow down and look things up yourself.");
            assert_eq!(parse_ai(&raw, &one, &c), None, "{bad}");
        }
        // fallback warning signs skip domain-trust wording when another sign exists
        for (i, e) in c.encounters.iter().enumerate() {
            for ch in &e.choices {
                let sign = pick_sign(&ch.warning_signs, 0);
                if ch.warning_signs.iter().any(|w| !has_domain_trust(w)) {
                    assert!(!has_domain_trust(&sign), "{i} {}: {sign}", e.id);
                }
            }
        }
    }
}
