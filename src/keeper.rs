//! The Floor Keeper: the question minigame at the stairs of every floor.
//!
//! The Keeper asks about the choice *you* made in this floor's encounter. The
//! question, the right answer and two clearly unsafe wrong answers come from
//! the `keeper` blocks in encounters.json (per choice, else per encounter; see
//! README "Floor Keeper questions"). With no block at all it falls back to
//! "what's the safer next move?" with that choice's `safer_next_move` and two
//! generic unsafe moves. The Keeper's lines are scripted only (no AI), so they
//! always agree with the verdict.
//!
//! v0.4: a floor whose trap has an investigation case (investigations.json)
//! plays the investigation fight instead (keeper_fight.rs, investigate.rs);
//! the quiz above is the fallback for content without one.

use crate::content::{Content, Keeper};
use crate::investigate::{Case, Cases};

pub const KEEPER_NAME: &str = "The Floor Keeper";

/// Clearly unsafe moves, used only when the content doesn't supply two of its own.
const GENERIC_WRONG: [&str; 3] = [
    "Act right away so the chance doesn't slip away, and sort out the details later.",
    "Send them the password or personal details they ask for so they can confirm it's you.",
    "Keep it to yourself; asking anyone about it would only slow you down.",
];
const GENERIC_QUESTION: &str = "What is the safer next move?";

/// First person, no quotes: they're shown as the Keeper speaking.
const RIGHT_LINES: [&str; 4] = [
    "Sharp eyes, Owl. You listened down here, so the stairs are yours.",
    "That is the move. Few who pass me know it. Go on, and mind the next floor.",
    "Correct. I'll step aside; my wardens can stay asleep tonight.",
    "Well answered, scholar. Take the stairs, and take that habit with you.",
];
const WRONG_LINES: [&str; 4] = [
    "Not quite. That isn't the safer move. My two wardens will test you instead.",
    "No, that's the risky road. Wardens, if you please.",
    "Wrong answer, and a risky one. Survive my wardens, then the stairs are yours.",
    "That's the move the trap is hoping for. Two wardens, then we'll talk.",
];

#[derive(Clone, Debug)]
pub struct Quiz {
    /// "Back at The Inbox Mimic, you picked "Click link"."
    pub situation: String,
    pub question: String,
    pub options: Vec<String>,
    pub correct: usize,
    /// a warning sign from the choice you made, shown after answering
    pub warning: Option<String>,
    /// mixes `seed` (so caches keyed on the quiz can't go stale across quizzes)
    pub id: u32,
    /// this floor's investigation (None = the plain quiz)
    pub case: Option<Case>,
    /// "scripted" or "ai". Questions are chosen by code; daily runs are always scripted.
    pub source: &'static str,
}

impl Quiz {
    pub fn is_correct(&self, i: usize) -> bool {
        i == self.correct
    }
}

/// Tiny deterministic mixer: same run seed + floor = same quiz layout.
pub fn mix(seed: u32, v: u32) -> u32 {
    let mut h = seed ^ v.wrapping_mul(0x9e37_79b9);
    h ^= h >> 16;
    h = h.wrapping_mul(0x85eb_ca6b);
    h ^= h >> 13;
    h = h.wrapping_mul(0xc2b2_ae35);
    h ^= h >> 16;
    h
}

fn filled(s: &str) -> Option<&str> {
    let t = s.trim();
    (!t.is_empty()).then_some(t)
}

/// Build the Keeper's question for `floor`, about the choice the player made
/// there (`picked`; None = the floor's first choice). Floors past the end of
/// the content reuse its encounters (`state::encounter_for_floor`).
pub fn build(content: &Content, floor: usize, picked: Option<usize>, seed: u32) -> Quiz {
    let list = &content.encounters;
    let e = &list[crate::state::encounter_for_floor(floor, list.len())];
    let ci = picked.unwrap_or(0).min(e.choices.len().saturating_sub(1));
    let ch = &e.choices[ci];
    let none = Keeper::default();
    let (own, enc) = (ch.keeper.as_ref().unwrap_or(&none), e.keeper.as_ref().unwrap_or(&none));
    // The block that supplies the question also supplies the answer and the wrong
    // options, so options written for one question never show up under another:
    // the choice's block if it has a question, else the encounter's, else a
    // generic question with the choice's own (question-less) block. Gaps fall
    // back to the choice's safer_next_move and the generic unsafe moves.
    let (question, block) = match (filled(&own.question), filled(&enc.question)) {
        (Some(q), _) => (q, own),
        (None, Some(q)) => (q, enc),
        (None, None) => (GENERIC_QUESTION, own),
    };
    let question = question.to_string();
    let answer = filled(&block.answer)
        .or(filled(&ch.safer_next_move))
        .unwrap_or("Stop, and check through an official channel you look up yourself.")
        .to_string();

    let mut wrong: Vec<String> = Vec::new();
    let candidates = block.wrong.iter().map(String::as_str).chain(GENERIC_WRONG);
    for w in candidates {
        let Some(w) = filled(w) else { continue };
        if wrong.len() < 2 && w != answer && !wrong.iter().any(|x| x == w) {
            wrong.push(w.to_string());
        }
    }

    let id = mix(seed, floor as u32 + 1);
    let correct = (id % 3) as usize;
    let mut options = wrong;
    options.insert(correct.min(options.len()), answer);
    Quiz {
        situation: format!("Back at {}, you picked \"{}\".", e.name, ch.label),
        question,
        options,
        correct,
        warning: ch.warning_signs.first().cloned(),
        id,
        case: None,
        source: "scripted",
    }
}

/// The Keeper round for `floor`: this run's investigation of the floor's trap
/// when it has a case, else the quiz from `build`. Code decides what is fake
/// and which red flags are planted (investigate.rs), never an AI.
pub fn build_for_floor(content: &Content, cases: &Cases, floor: usize, picked: Option<usize>, seed: u32) -> Quiz {
    let list = &content.encounters;
    let e = &list[crate::state::encounter_for_floor(floor, list.len())];
    let id = mix(seed, floor as u32 + 1);
    let Some(case) = cases.cases.get(&e.id).and_then(|inv| Case::build(inv, &cases.decoys, &e.id, &e.name, id)) else {
        return build(content, floor, picked, seed);
    };
    Quiz {
        situation: format!("{} sent you a {}.", e.name, case.medium.noun()),
        question: "What is the safer next move?".to_string(),
        options: vec![case.safe_move.clone()],
        correct: 0,
        warning: case.planted.first().map(|f| case.tell(*f).to_string()),
        id,
        case: Some(case),
        source: "scripted",
    }
}

/// The Keeper's line for a right or wrong answer (scripted, never AI).
pub fn line(correct: bool, floor: usize) -> &'static str {
    let lines = if correct { &RIGHT_LINES } else { &WRONG_LINES };
    lines[floor % lines.len()]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::{parse, EMBEDDED_ENCOUNTERS};

    fn shipped() -> Content {
        parse(EMBEDDED_ENCOUNTERS).unwrap().0
    }

    #[test]
    fn asks_about_the_choice_you_made_with_authored_answers() {
        let c = shipped();
        for seed in [0u32, 1, 7, 12345, u32::MAX] {
            for (f, e) in c.encounters.iter().enumerate() {
                for (ci, ch) in e.choices.iter().enumerate() {
                    let q = build(&c, f, Some(ci), seed);
                    let k = ch.keeper.as_ref().expect("shipped content has a keeper block per choice");
                    assert!(q.situation.contains(&ch.label), "{}: {}", e.id, q.situation);
                    assert_eq!(q.question, k.question);
                    assert_eq!(q.options.len(), 3);
                    assert_eq!(q.options[q.correct], k.answer);
                    let mut wrong: Vec<&String> = q.options.iter().enumerate().filter(|(i, _)| *i != q.correct).map(|(_, o)| o).collect();
                    wrong.sort();
                    let mut authored: Vec<&String> = k.wrong.iter().take(2).collect();
                    authored.sort();
                    assert_eq!(wrong, authored, "{}/{}", e.id, ch.id);
                    assert!(q.options.iter().all(|o| o.is_ascii()) && q.question.is_ascii());
                }
            }
        }
    }

    /// The point of the review: a wrong option is never good advice from anywhere.
    #[test]
    fn no_wrong_option_is_ever_a_safe_move() {
        let c = shipped();
        let mut safe: Vec<&str> = Vec::new();
        for e in &c.encounters {
            safe.extend(e.choices.iter().map(|ch| ch.safer_next_move.as_str()));
            safe.extend(e.keeper.iter().map(|k| k.answer.as_str()));
            safe.extend(e.choices.iter().filter_map(|ch| ch.keeper.as_ref().map(|k| k.answer.as_str())));
        }
        for (f, e) in c.encounters.iter().enumerate() {
            for ci in 0..e.choices.len() {
                for seed in 0..12 {
                    let q = build(&c, f, Some(ci), seed);
                    for (i, o) in q.options.iter().enumerate() {
                        if i != q.correct {
                            assert!(!safe.contains(&o.as_str()), "{}: wrong option is a safe move: {o}", e.id);
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn layout_is_deterministic_per_seed_and_varies_across_seeds() {
        let c = shipped();
        let a = build(&c, 1, Some(2), 99);
        let b = build(&c, 1, Some(2), 99);
        assert_eq!((a.correct, a.id, &a.options), (b.correct, b.id, &b.options));
        let spots: std::collections::HashSet<usize> = (0..30).map(|s| build(&c, 1, Some(2), s).correct).collect();
        assert_eq!(spots.len(), 3, "the right answer moves around");
    }

    #[test]
    fn content_without_keeper_blocks_still_gets_a_fair_question() {
        let mut c = shipped();
        for e in &mut c.encounters {
            e.keeper = None;
            for ch in &mut e.choices {
                ch.keeper = None;
            }
        }
        c.encounters.truncate(1);
        let q = build(&c, 5, Some(1), 3);
        let ch = &c.encounters[0].choices[1];
        assert_eq!(q.question, GENERIC_QUESTION);
        assert_eq!(q.options.len(), 3);
        assert_eq!(q.options[q.correct], ch.safer_next_move);
        for (i, o) in q.options.iter().enumerate() {
            if i != q.correct {
                assert!(GENERIC_WRONG.contains(&o.as_str()));
            }
        }
    }

    #[test]
    fn options_never_mix_questions() {
        let mut c = shipped();
        let e = &mut c.encounters[0];
        let ek = e.keeper.clone().unwrap();
        // a choice block with its own question but only one wrong option: the
        // second comes from the generic list, never from the encounter's question
        e.choices[0].keeper = Some(Keeper { question: "Own question?".into(), answer: String::new(), wrong: vec!["Only one".into()] });
        let q = build(&c, 0, Some(0), 1);
        assert_eq!(q.question, "Own question?");
        assert_eq!(q.options[q.correct], c.encounters[0].choices[0].safer_next_move);
        assert!(q.options.contains(&"Only one".to_string()));
        assert!(!q.options.iter().any(|o| ek.wrong.contains(o) || *o == ek.answer), "{:?}", q.options);
        assert!(q.options.iter().filter(|o| GENERIC_WRONG.contains(&o.as_str())).count() == 1);
        // a block with no question: the encounter's whole block is used, nothing of the choice's
        c.encounters[0].choices[0].keeper = Some(Keeper { question: String::new(), answer: "Mine".into(), wrong: vec!["Stray".into()] });
        let q = build(&c, 0, Some(0), 1);
        assert_eq!(q.question, ek.question);
        assert_eq!(q.options[q.correct], ek.answer);
        assert!(!q.options.contains(&"Stray".to_string()) && !q.options.contains(&"Mine".to_string()));
    }

    /// Length mustn't give the answer away: across the shipped questions the right
    /// answer is the longest option at most about a third of the time, and never
    /// much longer than the longest wrong one.
    #[test]
    fn answer_length_is_no_giveaway() {
        let c = shipped();
        let (mut blocks, mut longest) = (0, 0);
        for e in &c.encounters {
            for k in e.keeper.iter().chain(e.choices.iter().filter_map(|ch| ch.keeper.as_ref())) {
                let a = k.answer.chars().count() as f32;
                let w = k.wrong.iter().map(|w| w.chars().count()).max().unwrap_or(0) as f32;
                blocks += 1;
                longest += usize::from(a > w);
                assert!(a <= w * 1.15, "{}: answer {a} chars vs longest wrong {w}: {}", e.id, k.answer);
            }
        }
        assert!(longest * 3 <= blocks + 2, "answer is the longest option in {longest} of {blocks} questions");
    }

    #[test]
    fn lines_are_first_person_font_safe_and_unquoted() {
        for f in 0..8 {
            for ok in [true, false] {
                let l = line(ok, f);
                assert!(l.is_ascii() && l.len() < 160, "{l}");
                assert!(!l.starts_with('\'') && !l.contains('"') && !l.contains("Keeper"), "{l}");
            }
        }
    }

    #[test]
    fn floors_with_a_case_investigate_and_the_rest_keep_the_quiz() {
        let c = shipped();
        let cases = crate::investigate::load();
        let n = c.encounters.len();
        for floor in 0..crate::state::floors_for(n) {
            let e = &c.encounters[crate::state::encounter_for_floor(floor, n)];
            let q = build_for_floor(&c, &cases, floor, Some(0), 77);
            assert_eq!(q.case.is_some(), cases.cases.contains_key(&e.id), "floor {}", floor + 1);
            if let Some(k) = &q.case {
                assert_eq!(k.id, e.id);
                assert!(q.warning.is_some());
            }
        }
        // no cases at all: every floor keeps the quiz
        let none = Cases::default();
        for floor in 0..crate::state::floors_for(n) {
            let q = build_for_floor(&c, &none, floor, Some(1), 5);
            let p = build(&c, floor, Some(1), 5);
            assert!(q.case.is_none());
            assert_eq!((q.question, q.options, q.correct), (p.question, p.options, p.correct));
        }
    }

    /// Every floor of a 10-floor run gets a full quiz, including floors past the
    /// end of the content (they reuse encounters in order until Ry's floors land).
    #[test]
    fn every_floor_of_the_run_has_a_keeper_quiz() {
        let c = shipped();
        let n = c.encounters.len();
        for floor in 0..crate::state::floors_for(n) {
            let e = &c.encounters[crate::state::encounter_for_floor(floor, n)];
            for ci in 0..e.choices.len() {
                let q = build(&c, floor, Some(ci), 4242);
                assert_eq!(q.options.len(), 3, "floor {floor}");
                assert!(q.correct < 3);
                assert!(q.situation.contains(&e.name), "floor {floor}: {}", q.situation);
                assert!(q.situation.contains(&e.choices[ci].label));
            }
        }
        // the same encounter on two floors still gets its own layout seed
        if n < 10 {
            assert_ne!(build(&c, 0, Some(0), 7).id, build(&c, n, Some(0), 7).id);
        }
    }
}
