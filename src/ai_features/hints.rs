//! Owl companion hints: one short nudge when the player hesitates.
//!
//! Code decides when (hesitation over `after_secs` with no input, at most once
//! per encounter, only when enabled). The text is the floor pack's AI HINT if it
//! passed `hint_ok`, else a scripted hint. A hint points at warning signs; it
//! never names a choice or restates a safe move.

use super::data::FeatureData;
use super::validate::{has_domain_trust, has_payment, has_phrase, norm};
use crate::content::{looks_like, Encounter};
use crate::keeper::mix;

/// Default hesitation before the owl speaks (difficulty tiers adjust it).
pub const HINT_AFTER_SECS: f64 = 12.0;

/// Phrases that turn a nudge into an answer.
const GIVEAWAY: [&str; 9] = ["the answer", "you should pick", "pick ", "choose", "select", "option", "the right move", "the safe move", "go with"];

/// A hint must not name a choice label, resemble a safe move, or tell you what to pick.
pub fn hint_ok(line: &str, enc: &Encounter) -> bool {
    let n = norm(line);
    if GIVEAWAY.iter().any(|g| has_phrase(&n, g.trim())) {
        return false;
    }
    if enc.choices.iter().any(|c| has_phrase(&n, &c.label)) {
        return false;
    }
    // no payment methods, and never a domain as proof of safety (".edu", "official domain")
    if has_payment(line) || has_domain_trust(line) {
        return false;
    }
    !super::banter::safe_moves(enc).iter().any(|m| looks_like(m, line))
}

/// The owl's lines always open with "Hoo" (followed by punctuation or a space).
pub fn starts_with_hoo(line: &str) -> bool {
    let t = line.trim_start();
    t.len() >= 3 && t[..3].eq_ignore_ascii_case("hoo") && !t[3..].starts_with(|c: char| c.is_ascii_alphanumeric())
}

/// Adds the owl's "Hoo! " opener if it is missing; None if that would make the line too long.
pub fn with_hoo(line: &str) -> Option<String> {
    let t = line.trim();
    if starts_with_hoo(t) {
        return Some(t.to_string());
    }
    let s = format!("Hoo! {t}");
    (s.len() <= super::banter::MAX_BANTER_CHARS).then_some(s)
}

/// The AI's HINT after every check, with the "Hoo" opener guaranteed (deterministic).
pub fn hoo_hint(line: &str, enc: &Encounter) -> Option<String> {
    if hint_ok(line, enc) {
        with_hoo(line)
    } else {
        None
    }
}

pub fn scripted_hint(data: &FeatureData, enc_id: &str, seed: u32, floor: usize) -> String {
    let list = FeatureData::pick_list(&data.owl_hints, enc_id);
    match list.len() {
        0 => "Hoo, slow down. Who sent this, and what are they asking you to do?".into(),
        n => {
            let h = &list[mix(seed, 0x0071 + floor as u32) as usize % n];
            with_hoo(h).unwrap_or_else(|| h.clone())
        }
    }
}

/// When to show the hint. One per encounter (keyed by floor), toggleable.
#[derive(Clone, Debug)]
pub struct HintTracker {
    pub enabled: bool,
    pub after_secs: f64,
    /// (floor, time of the last input) while an encounter is open
    active: Option<(usize, f64)>,
    shown: Vec<usize>,
}

impl Default for HintTracker {
    fn default() -> Self {
        HintTracker { enabled: true, after_secs: HINT_AFTER_SECS, active: None, shown: Vec::new() }
    }
}

impl HintTracker {
    /// New run: forget which floors got hints (keeps the toggle).
    pub fn reset(&mut self) {
        self.active = None;
        self.shown.clear();
    }
    /// The encounter screen for `floor` opened.
    pub fn start(&mut self, floor: usize, now: f64) {
        self.active = Some((floor, now));
    }
    /// The player moved the cursor or pressed something: hesitation restarts.
    pub fn input(&mut self, now: f64) {
        if let Some((_, t)) = &mut self.active {
            *t = now;
        }
    }
    /// The encounter closed (choice made or left).
    pub fn stop(&mut self) {
        self.active = None;
    }
    pub fn toggle(&mut self) -> bool {
        self.enabled = !self.enabled;
        self.enabled
    }
    /// True exactly once per floor, when the player has hesitated long enough.
    pub fn due(&mut self, now: f64) -> Option<usize> {
        let (floor, since) = self.active?;
        if !self.enabled || self.shown.contains(&floor) || now - since < self.after_secs {
            return None;
        }
        self.shown.push(floor);
        Some(floor)
    }
    pub fn shown_on(&self, floor: usize) -> bool {
        self.shown.contains(&floor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai_features::data::embedded;
    use crate::ai_features::validate::game_line;
    use crate::content::{parse, EMBEDDED_ENCOUNTERS};

    #[test]
    fn hint_fires_once_after_hesitation_and_respects_toggle() {
        let mut h = HintTracker::default();
        h.start(0, 10.0);
        assert_eq!(h.due(10.0 + HINT_AFTER_SECS - 0.1), None);
        h.input(15.0); // moving the cursor restarts the clock
        assert_eq!(h.due(10.0 + HINT_AFTER_SECS + 0.1), None);
        assert_eq!(h.due(15.0 + HINT_AFTER_SECS), Some(0));
        assert_eq!(h.due(99.0), None, "once per encounter");
        h.start(0, 100.0);
        assert_eq!(h.due(200.0), None, "re-opening the same floor doesn't repeat it");
        h.start(1, 0.0);
        assert!(!h.toggle());
        assert_eq!(h.due(100.0), None, "off means off");
        h.toggle();
        assert_eq!(h.due(100.0), Some(1));
        h.stop();
        assert_eq!(h.due(500.0), None);
        h.reset();
        h.start(1, 0.0);
        assert_eq!(h.due(100.0), Some(1), "new run");
    }

    #[test]
    fn giveaway_hints_are_rejected() {
        let c = parse(EMBEDDED_ENCOUNTERS).unwrap().0;
        let e = &c.encounters[0];
        for bad in [
            "Hoo, the answer is to report it.",
            "Hoo, Inspect sender, obviously!",
            "Hoo, you should choose the careful option here.",
            "Hoo, report the message with the Phish Alert button without clicking any links.",
        ] {
            assert!(!hint_ok(bad, e), "{bad}");
        }
        assert!(hint_ok("Hoo, does that sender address really belong to an office you know?", e));
    }

    #[test]
    fn every_scripted_hint_is_valid_for_its_encounter() {
        let d = embedded().data;
        let c = parse(EMBEDDED_ENCOUNTERS).unwrap().0;
        for e in &c.encounters {
            assert!(d.owl_hints.contains_key(&e.id), "no hint for {}", e.id);
            for h in FeatureData::pick_list(&d.owl_hints, &e.id).iter().chain(d.owl_hints["default"].iter()) {
                assert!(hint_ok(h, e) && game_line(h, 150).is_some(), "{}: {h}", e.id);
            }
            assert_eq!(scripted_hint(&d, &e.id, 5, 2), scripted_hint(&d, &e.id, 5, 2));
        }
    }

    #[test]
    fn hints_always_open_with_hoo() {
        let c = parse(EMBEDDED_ENCOUNTERS).unwrap().0;
        let e = &c.encounters[0];
        assert_eq!(hoo_hint("Who really sent this message?", e).as_deref(), Some("Hoo! Who really sent this message?"));
        assert_eq!(hoo_hint("Hoo, who really sent this?", e).as_deref(), Some("Hoo, who really sent this?"));
        assert_eq!(hoo_hint("hoo... who sent this?", e).as_deref(), Some("hoo... who sent this?"));
        assert!(!starts_with_hoo("Hoot, look closer.") && !starts_with_hoo("Hook line and sinker."));
        assert_eq!(hoo_hint(&"word ".repeat(30), e), None, "too long once prefixed");
        let raw = "TAUNT: Fine.\nRIGHT: x\nWRONG: y\nHINT: What is that message rushing you to do?";
        assert_eq!(crate::ai_features::banter::parse_floor_pack(raw, e).hint.as_deref(), Some("Hoo! What is that message rushing you to do?"));
        let d = embedded().data;
        for enc in &c.encounters {
            for f in 0..10 {
                assert!(starts_with_hoo(&scripted_hint(&d, &enc.id, 7, f)), "{}", enc.id);
            }
        }
    }

    #[test]
    fn hints_never_name_payments_or_trust_a_domain() {
        let c = parse(EMBEDDED_ENCOUNTERS).unwrap().0;
        let e = &c.encounters[0];
        for bad in [
            "Hoo, why would a seller want gift cards?",
            "Hoo, would a real office ask for Zelle?",
            "Hoo, does that link end in .edu?",
            "Hoo, is it from the official domain?",
            "Hoo, does that link end in a campus address?",
        ] {
            assert_eq!(hoo_hint(bad, e), None, "{bad}");
        }
    }
}
