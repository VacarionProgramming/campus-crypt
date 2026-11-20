//! Floor Keeper banter: a taunt before the quiz and a reaction after the verdict.
//!
//! One AI call per floor (the "floor pack", prefetched when the floor starts)
//! writes TAUNT, RIGHT, WRONG and the owl's HINT. The quiz itself stays scripted
//! (keeper.rs). Code picks RIGHT or WRONG from the verdict it computed and runs
//! `verdict_consistent` on it; anything that doesn't clearly agree with the
//! verdict is replaced by keeper.rs's scripted line.

use super::data::FeatureData;
use super::validate::{game_line, has_payment, has_phrase, labeled_or_ordered, norm, speech};
use crate::ai::LineSource;
use crate::content::{looks_like, Encounter};
use crate::keeper::{self, mix};

pub const FLOOR_LABELS: [&str; 4] = ["TAUNT", "RIGHT", "WRONG", "HINT"];
pub const MAX_BANTER_CHARS: usize = 150;

/// Words that only belong after a WRONG answer. (Wardens may appear in a RIGHT
/// line, e.g. "my wardens can stay asleep", so they're required below, not banned.)
const WRONG_CUES: [&str; 12] =
    ["wrong", "not quite", "incorrect", "risky", "fail", "failed", "mistake", "nope", "not the safer", "not safe", "unsafe", "try again"];
/// Words that only belong after a RIGHT answer. ("the stairs are yours" may close a
/// WRONG line: "survive my wardens, then the stairs are yours".)
const RIGHT_CUES: [&str; 12] = [
    "correct", "well done", "well answered", "right answer", "you pass", "you may pass", "step aside", "steps aside",
    "good answer", "you got it", "bravo", "sharp",
];
/// A RIGHT line must point at the way on; a WRONG line at the wardens or the miss.
const RIGHT_NEEDS: [&str; 11] =
    ["stairs", "staircase", "stairway", "stairwell", "stair", "pass", "step aside", "steps aside", "go on", "onward", "the way down"];
const WRONG_NEEDS: [&str; 10] =
    ["warden", "wardens", "guardians", "not quite", "wrong", "not the safer", "not the safest", "wasn t the safest", "not the move", "risky"];

/// The verdict-consistency check: `line` reads as a reaction to a `correct`
/// (or wrong) answer and has none of the other verdict's cues. keeper.rs's
/// scripted lines pass it (tested), so it is the bar every AI line must meet.
pub fn verdict_consistent(line: &str, correct: bool) -> bool {
    let n = norm(line);
    let any = |list: &[&str]| list.iter().any(|p| has_phrase(&n, p));
    if correct {
        !any(&WRONG_CUES) && any(&RIGHT_NEEDS)
    } else {
        !any(&RIGHT_CUES) && any(&WRONG_NEEDS)
    }
}

/// Every piece of good advice in an encounter (choice safer moves + Keeper answers).
pub fn safe_moves(enc: &Encounter) -> Vec<&str> {
    let answers = enc.keeper.iter().chain(enc.choices.iter().filter_map(|c| c.keeper.as_ref())).map(|k| k.answer.as_str());
    enc.choices.iter().map(|c| c.safer_next_move.as_str()).chain(answers).filter(|s| !s.trim().is_empty()).collect()
}

/// Lines in the student's voice, not the Keeper's: the student describing their own
/// pick or answer, or talking *to* the Keeper about its stairs and wardens.
const PLAYER_VOICE: [&str; 38] = [
    "i chose", "i choose", "i picked", "i pick", "i answered", "i answer", "i went with", "i ll go with", "i will go with",
    "i select", "i selected", "i ll pick", "i ll choose", "my answer", "my choice", "my pick", "i got it right",
    "i got it wrong", "i was right", "i was wrong", "did i pass", "can i pass", "may i pass", "let me pass",
    "let me through", "i ll take the stairs", "i will take the stairs", "i take the stairs", "i ll face", "i will face",
    "i must face", "your wardens", "your question", "your quiz", "your riddle", "your stairs", "thank you keeper",
    "thanks keeper",
];

/// The line is spoken by the Floor Keeper to the student. Rejects the student's
/// voice ("I chose to verify...", "Thank you, Keeper", "your wardens") and lines
/// that open by addressing the Keeper.
pub fn keeper_voice_ok(line: &str) -> bool {
    let n = norm(line);
    let lead = line.trim_start().to_ascii_lowercase();
    let vocative = ["keeper,", "keeper!", "floor keeper,", "floor keeper!", "o keeper", "oh keeper", "dear keeper"].iter().any(|v| lead.starts_with(v));
    !vocative && !PLAYER_VOICE.iter().any(|p| has_phrase(&n, p))
}

/// Unwraps unquoted stage narration ("the floor keeper chuckles, Ah, dear owl...")
/// and capitalizes the first letter. None if a narration lead has no clear end.
pub fn keeper_speech(line: &str) -> Option<String> {
    let t = line.trim();
    let lower = t.to_ascii_lowercase();
    let mut rest = t;
    if ["the floor keeper ", "floor keeper ", "the keeper ", "keeper "].iter().any(|p| lower.starts_with(p)) {
        let end = t.find([',', ':', ';']).filter(|&i| i <= 48 && t[..i].split_whitespace().count() <= 5)?;
        rest = t[end + 1..].trim_start();
    }
    let mut c = rest.chars();
    let first = c.next()?;
    let out = format!("{}{}", first.to_ascii_uppercase(), c.as_str());
    (out.split_whitespace().count() >= 4).then_some(out)
}

/// Checks every Keeper line shares: the Keeper's voice and no payment methods.
fn keeper_line_ok(line: &str) -> bool {
    keeper_voice_ok(line) && !has_payment(line)
}

/// A taunt is said before the quiz: no verdict words, no answer.
pub fn taunt_ok(line: &str, enc: &Encounter) -> bool {
    let n = norm(line);
    keeper_line_ok(line)
        && !WRONG_CUES.iter().chain(RIGHT_CUES.iter()).any(|p| has_phrase(&n, p))
        && !safe_moves(enc).iter().any(|m| looks_like(m, line))
}

/// Validated floor pack. Any field may be None (that slot uses its scripted line).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FloorPack {
    pub taunt: Option<String>,
    pub right: Option<String>,
    pub wrong: Option<String>,
    pub hint: Option<String>,
}

pub fn parse_floor_pack(raw: &str, enc: &Encounter) -> FloorPack {
    let mut p = FloorPack::default();
    for (label, value) in labeled_or_ordered(raw, &FLOOR_LABELS) {
        let Some(line) = game_line(&speech(value), MAX_BANTER_CHARS) else { continue };
        let line = if label == "HINT" { Some(line) } else { keeper_speech(&line) };
        let Some(line) = line else { continue };
        match label {
            "TAUNT" if taunt_ok(&line, enc) => p.taunt = Some(line),
            "RIGHT" if keeper_line_ok(&line) && verdict_consistent(&line, true) => p.right = Some(line),
            "WRONG" if keeper_line_ok(&line) && verdict_consistent(&line, false) => p.wrong = Some(line),
            "HINT" => p.hint = super::hints::hoo_hint(&line, enc),
            _ => {}
        }
    }
    p
}

/// Facts for the floor-pack call (system prompt: `## floor_pack`). Includes the
/// warning signs for the owl's hint and the safe moves the hint must not give away.
pub fn user_message(floor: usize, enc: &Encounter, owl_name: &str) -> String {
    let mut s = format!(
        "TASK: floor_pack\nfloor: {} of the run\nencounter_id: {}\nthis floor's trickster: {}\nthe trap: {}\nowl companion's name: {owl_name}\nwarning signs the hint may point toward:\n",
        floor + 1,
        enc.id,
        enc.character.name,
        enc.scenario
    );
    let mut seen = Vec::new();
    for w in enc.choices.iter().flat_map(|c| c.warning_signs.iter()) {
        if !seen.contains(&w) && seen.len() < 6 {
            s.push_str(&format!("- {w}\n"));
            seen.push(w);
        }
    }
    s.push_str("choices the hint must NOT name or recommend: ");
    s.push_str(&enc.choices.iter().map(|c| c.label.as_str()).collect::<Vec<_>>().join(", "));
    s.push_str("\n\nWrite the four lines now.");
    s
}

/// Scripted taunt for this floor (per-encounter list, else default).
pub fn scripted_taunt(data: &FeatureData, enc_id: &str, seed: u32, floor: usize) -> String {
    let list = FeatureData::pick_list(&data.keeper_taunts, enc_id);
    match list.len() {
        0 => "So, you survived my floor. Let us see what you learned.".into(),
        n => list[mix(seed, 0x7a00 + floor as u32) as usize % n].clone(),
    }
}

/// The taunt to show: the AI's if it passed, else scripted.
pub fn taunt(pack: Option<&FloorPack>, data: &FeatureData, enc_id: &str, seed: u32, floor: usize, why: &'static str) -> (String, LineSource) {
    match pack.and_then(|p| p.taunt.clone()) {
        Some(t) => (t, LineSource::Ai),
        None => (scripted_taunt(data, enc_id, seed, floor), LineSource::Fallback(if pack.is_some() { "AI line rejected by filter" } else { why })),
    }
}

/// The verdict reaction. `correct` comes from `Quiz::is_correct`, never from the AI;
/// the line is re-checked here so a mismatch can never reach the screen.
pub fn reaction(pack: Option<&FloorPack>, correct: bool, floor: usize, why: &'static str) -> (String, LineSource) {
    let ai = pack.and_then(|p| if correct { p.right.clone() } else { p.wrong.clone() });
    match ai.filter(|l| verdict_consistent(l, correct)) {
        Some(l) => (l, LineSource::Ai),
        None => (
            keeper::line(correct, floor).to_string(),
            LineSource::Fallback(if pack.is_some() { "AI line rejected by filter" } else { why }),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai_features::data::embedded;
    use crate::content::{parse, EMBEDDED_ENCOUNTERS};

    fn enc(i: usize) -> Encounter {
        parse(EMBEDDED_ENCOUNTERS).unwrap().0.encounters[i].clone()
    }

    #[test]
    fn scripted_keeper_lines_pass_the_verdict_check() {
        for f in 0..12 {
            assert!(verdict_consistent(keeper::line(true, f), true), "{}", keeper::line(true, f));
            assert!(verdict_consistent(keeper::line(false, f), false), "{}", keeper::line(false, f));
            assert!(!verdict_consistent(keeper::line(true, f), false));
            assert!(!verdict_consistent(keeper::line(false, f), true));
        }
    }

    #[test]
    fn mismatched_reactions_are_rejected() {
        for bad_right in ["Wrong, but the stairs are yours anyway.", "Risky move, yet you may pass.", "Hmm, interesting choice, Owl."] {
            assert!(!verdict_consistent(bad_right, true), "{bad_right}");
        }
        for bad_wrong in ["Correct! My wardens salute you.", "Well done, though my wardens wake.", "Hmm, interesting choice, Owl."] {
            assert!(!verdict_consistent(bad_wrong, false), "{bad_wrong}");
        }
        assert!(verdict_consistent("Sharp thinking, Owl. I step aside; the stairs await you.", true));
        assert!(verdict_consistent("Not quite, little Owl. My two wardens will test you now.", false));
    }

    #[test]
    fn floor_pack_parses_and_filters_each_line() {
        let e = enc(0);
        let raw = "TAUNT: So the Mimic did not swallow you whole. Let us see what you learned.\n\
                   RIGHT: Correct, though that was a risky guess. Go on.\n\
                   WRONG: Not quite, Owl. My two wardens will test you before the stairs.\n\
                   HINT: Hoo, who really sent this, and did you ask for it?";
        let p = parse_floor_pack(raw, &e);
        assert!(p.taunt.is_some() && p.wrong.is_some() && p.hint.is_some(), "{p:?}");
        assert_eq!(p.right, None, "RIGHT calls the answer risky: mismatched");
        let (line, src) = reaction(Some(&p), true, 3, "offline");
        assert_eq!((line.as_str(), src), (keeper::line(true, 3), LineSource::Fallback("AI line rejected by filter")));
        let (line, src) = reaction(Some(&p), false, 3, "offline");
        assert_eq!((line.as_str(), src), ("Not quite, Owl. My two wardens will test you before the stairs.", LineSource::Ai));
    }

    #[test]
    fn no_pack_means_scripted_lines() {
        let d = embedded().data;
        let e = enc(4);
        for correct in [true, false] {
            let (l, s) = reaction(None, correct, 7, "offline");
            assert_eq!((l.as_str(), s), (keeper::line(correct, 7), LineSource::Fallback("offline")));
        }
        let (t, s) = taunt(None, &d, &e.id, 9, 4, "AI not ready");
        assert_eq!(s, LineSource::Fallback("AI not ready"));
        assert!(taunt_ok(&t, &e) && game_line(&t, MAX_BANTER_CHARS).is_some(), "{t}");
        assert_eq!(t, scripted_taunt(&d, &e.id, 9, 4), "deterministic");
    }

    #[test]
    fn every_scripted_taunt_is_a_clean_pre_verdict_line() {
        let d = embedded().data;
        let c = parse(EMBEDDED_ENCOUNTERS).unwrap().0;
        for e in &c.encounters {
            for t in FeatureData::pick_list(&d.keeper_taunts, &e.id) {
                assert!(taunt_ok(t, e) && game_line(t, MAX_BANTER_CHARS).is_some(), "{}: {t}", e.id);
            }
            assert!(d.keeper_taunts.contains_key(&e.id), "no taunt for {}", e.id);
        }
    }

    #[test]
    fn user_message_names_choices_to_avoid_and_no_answers() {
        let e = enc(1);
        let u = user_message(1, &e, "Hoot");
        assert!(u.contains("floor: 2 of the run") && u.contains("Prioritize"));
        assert!(!safe_moves(&e).iter().any(|m| u.contains(*m)), "{u}");
    }

    #[test]
    fn player_voiced_keeper_lines_are_rejected() {
        let e = enc(0);
        for bad in [
            "I chose to verify the sender first, so the stairs are mine.",
            "I picked the careful path, Keeper, now let me pass.",
            "Thank you, Keeper, I will take the stairs now.",
            "Keeper, your wardens do not scare me one bit.",
            "My answer was the cautious one; surely that earns the stairs.",
        ] {
            assert!(!keeper_voice_ok(bad), "{bad}");
            let pack = parse_floor_pack(&format!("TAUNT: {bad}\nRIGHT: {bad}\nWRONG: {bad}"), &e);
            assert_eq!((pack.taunt, pack.right, pack.wrong), (None, None, None), "{bad}");
        }
        for ok in ["You read that well. The stairs are yours, little owl.", "I have guarded these stairs a long time. Show me what you noticed."] {
            assert!(keeper_voice_ok(ok), "{ok}");
        }
        for f in 0..12 {
            assert!(keeper_voice_ok(keeper::line(true, f)) && keeper_voice_ok(keeper::line(false, f)));
        }
    }

    #[test]
    fn banter_never_names_a_payment_method() {
        let e = enc(0);
        let raw = "TAUNT: The Gargoyle loves a gift card, and so do I.\n\
                   RIGHT: Well done; no Venmo for you today. The stairs are yours.\n\
                   WRONG: Not quite. Wire money and my wardens will collect it.\n\
                   HINT: Hoo, why would anyone want Bitcoin for a textbook?";
        assert_eq!(parse_floor_pack(raw, &e), FloorPack::default());
        let data = embedded().data;
        for (id, lines) in data.keeper_taunts.iter().chain(data.owl_hints.iter()) {
            for l in lines {
                assert!(!has_payment(l), "{id}: {l}");
            }
        }
    }

    #[test]
    fn keeper_narration_is_unwrapped_and_capitalized() {
        let e = enc(0);
        // seen live on floor 0 and floor 4
        let raw = "TAUNT: the floor keeper chuckles, Ah, dear owl, the Inbox Mimic whispers sweet urgency at midnight.\n\
                   RIGHT: the floor keeper smiles, Wonderful choice, owl! The stairs await your next step.\n\
                   WRONG: that choice was not the gentlest path; my two wardens will now test your patience.";
        let p = parse_floor_pack(raw, &e);
        assert_eq!(p.taunt.as_deref(), Some("Ah, dear owl, the Inbox Mimic whispers sweet urgency at midnight."));
        assert_eq!(p.right.as_deref(), Some("Wonderful choice, owl! The stairs await your next step."));
        assert_eq!(p.wrong.as_deref(), Some("That choice was not the gentlest path; my two wardens will now test your patience."));
        assert_eq!(keeper_speech("The Floor Keeper watches the Mimic and the stairs and the dust settle slowly"), None);
    }
}
