//! AI-generated encounters, verified by code.
//!
//! The AI writes a whole new floor encounter: scene, scenario, three choices,
//! each choice's safe/risky/mixed label, a reaction line, two warning signs and a
//! safer next move. Code never takes the AI's word for it:
//!
//! * `plan` (code) picks the theme and the two red flags the scenario must show;
//! * `build` (code) parses the labelled reply, runs every text through the game's
//!   filters, checks the scenario really contains cue words for both planted
//!   flags, and checks each choice's label against a word rubric: a choice
//!   labelled SAFE must read as a safe action (verify, ask, report, decline...),
//!   a RISKY one as a risky action (click, pay, send, share, reply...), and a
//!   MIXED one as neither extreme. Any miss rejects the whole encounter and the
//!   game plays the scripted one instead.
//! * Effects (resource changes) come from `effects_for(outcome)`; the AI never
//!   sets a number.
//!
//! Generated encounters have no Keeper block, so the Floor Keeper asks the
//! generic question built from each choice's `safer_next_move`.

use super::scamgen::RedFlag;
use super::validate::{game_line, has_domain_trust, has_host, has_payment, has_phrase, labeled, norm, scam_prose};
use crate::ai::{self, TokenLimits};
use crate::content::{Character, Choice, Effects, Encounter, Outcome};
use crate::keeper::mix;

/// Generated encounters per run (one AI call each).
pub const GEN_SLOTS: usize = 3;
/// `max_tokens` (Groq with reasoning, other providers): the reply is ~20 labelled lines.
pub const GEN_TOKENS: TokenLimits = TokenLimits(2400, 800);

pub const MAX_NAME: usize = 30;
pub const MAX_LABEL: usize = 24;
pub const MAX_INTRO: usize = 200;
pub const MAX_SCENARIO: usize = 380;
pub const MAX_REACTION: usize = 160;
pub const MAX_SIGN: usize = 120;
pub const MAX_NEXT: usize = 150;

/// The floor that generated slot `slot` replaces (floor 0 stays scripted as the tutorial).
pub fn floor_for_slot(slot: usize) -> usize {
    2 + 3 * slot
}

/// Which generated slot (if any) belongs to `floor`.
pub fn slot_for_floor(floor: usize) -> Option<usize> {
    (floor >= 2 && (floor - 2).is_multiple_of(3) && (floor - 2) / 3 < GEN_SLOTS).then(|| (floor - 2) / 3)
}

/// A topic the generator may write about. Code owns the themes and the flags.
pub struct Theme {
    pub id: &'static str,
    pub name: &'static str,
    /// one line of setting for the prompt
    pub pitch: &'static str,
    /// red flags the scenario must show (non-header flags only: they have cue words)
    pub flags: [RedFlag; 2],
}

pub const THEMES: [Theme; 7] = [
    Theme {
        id: "sublease_specter",
        name: "The Sublease Specter",
        pitch: "a far-too-cheap off-campus apartment listing from a landlord who cannot meet in person",
        flags: [RedFlag::UpfrontFee, RedFlag::Urgency],
    },
    Theme {
        id: "scholarship_siren",
        name: "The Scholarship Siren",
        pitch: "a surprise scholarship award that wants a processing fee before it pays out",
        flags: [RedFlag::UpfrontFee, RedFlag::TooGoodPay],
    },
    Theme {
        id: "helpdesk_hobgoblin",
        name: "The Help Desk Hobgoblin",
        pitch: "a fake campus IT help desk saying your account is locked and asking for your login to fix it",
        flags: [RedFlag::CredentialRequest, RedFlag::Urgency],
    },
    Theme {
        id: "textbook_trickster",
        name: "The Textbook Trickster",
        pitch: "a used-textbook seller who wants to leave the marketplace and chat privately",
        flags: [RedFlag::Secrecy, RedFlag::UpfrontFee],
    },
    Theme {
        id: "parking_phantom",
        name: "The Parking Phantom",
        pitch: "a text claiming an unpaid campus parking ticket that must be settled by signing in",
        flags: [RedFlag::Urgency, RedFlag::CredentialRequest],
    },
    Theme {
        id: "survey_sprite",
        name: "The Survey Sprite",
        pitch: "a paid research study that offers huge pay for a few minutes and asks for personal details",
        flags: [RedFlag::TooGoodPay, RedFlag::CredentialRequest],
    },
    Theme {
        id: "club_cloaker",
        name: "The Club Cloaker",
        pitch: "someone claiming to be a club officer who wants a favor and asks you to tell no one",
        flags: [RedFlag::Secrecy, RedFlag::Urgency],
    },
];

/// The theme for slot `slot` of the run with `seed`: slots never repeat a theme.
pub fn theme_for(seed: u32, slot: usize) -> &'static Theme {
    let start = mix(seed, 0xE1C0) as usize % THEMES.len();
    &THEMES[(start + slot * 2) % THEMES.len()]
}

/// How far the three choices rotate, so the safe one is not always in the same place.
pub fn rotation_for(seed: u32, slot: usize) -> usize {
    mix(seed, 0xB00 + slot as u32) as usize % 3
}

/// System prompt: the voice, the hard rules and the exact reply format.
pub const GEN_SYSTEM: &str = r#"You write one encounter for a teaching game about staying safe from scams and online tricks. The player is a college student exploring a dungeon floor and meets a spooky character who pitches something. Keep it playful, never scary, and always safe for school.

Reply with ONLY these labelled lines, one per line, no extra text, no markdown:
NAME: the encounter's name (max 30 characters)
INTRO: one sentence describing the room (max 190 characters)
SCENARIO: 2 or 3 sentences, the character's pitch to the player (max 360 characters)
CHARACTER: the character's name (max 30 characters)
Then for each of three choices, N = 1, 2, 3:
CN_LABEL: the player's action, 2 to 4 words (max 24 characters)
CN_OUTCOME: exactly one of safe, risky, mixed
CN_REACTION: what the character says back, one or two sentences (max 150 characters)
CN_SIGN1: one warning sign from the scenario the player could notice (max 110 characters)
CN_SIGN2: a second, different warning sign (max 110 characters)
CN_NEXT: the safer next move, an action starting with a verb (max 140 characters)

Hard rules:
- The scenario MUST clearly show the red flags you are given, using their cue words.
- Exactly: at least one safe choice and at least one risky choice. The third may be any outcome.
- A safe label starts with a protective verb (Verify, Check, Ask, Call, Report, Decline, Ignore, Block, Tour, Confirm). A risky label starts with a risky verb (Click, Pay, Send, Share, Reply, Enter, Agree, Accept). A mixed label combines a good and a bad step, like "Ask, then pay half".
- Every CN_NEXT must be a safe action, and must not rely on a website address, a phone number or an email address.
- Never write links, email addresses, phone numbers, domain names, or the words health, focus, knowledge, score.
- Never name a payment app or a money-transfer service.
- Never promise that a habit will prevent scams or keep anyone safe.
- Use only plain ASCII characters.
"#;

/// The per-call user message.
pub fn user_message(seed: u32, slot: usize) -> String {
    let t = theme_for(seed, slot);
    let flags: Vec<String> = t
        .flags
        .iter()
        .map(|f| {
            let cues: Vec<&str> = f.markers().iter().take(5).copied().collect();
            format!("{} (use words like: {})", f.default_label(), cues.join(", "))
        })
        .collect();
    format!(
        "TASK: generate_encounter\nTHEME: {}\nSETTING: {}\nRED FLAGS THE SCENARIO MUST SHOW: {}\nWrite the encounter now, using exactly the labelled lines.",
        t.name,
        t.pitch,
        flags.join("; ")
    )
}

/// The reply's labels, in order.
const LABELS: [&str; 22] = [
    "NAME", "INTRO", "SCENARIO", "CHARACTER", //
    "C1_LABEL", "C1_OUTCOME", "C1_REACTION", "C1_SIGN1", "C1_SIGN2", "C1_NEXT", //
    "C2_LABEL", "C2_OUTCOME", "C2_REACTION", "C2_SIGN1", "C2_SIGN2", "C2_NEXT", //
    "C3_LABEL", "C3_OUTCOME", "C3_REACTION", "C3_SIGN1", "C3_SIGN2", "C3_NEXT",
];

/// The resource changes for an outcome (same numbers as the scripted encounters).
pub fn effects_for(o: Outcome) -> Effects {
    match o {
        Outcome::Safe => Effects { health: 1, focus: 1, knowledge: 1 },
        Outcome::Risky => Effects { health: -2, focus: 0, knowledge: 1 },
        Outcome::Mixed => Effects { health: 0, focus: -1, knowledge: 1 },
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stance {
    Safe,
    Risky,
    Mixed,
    Unclear,
}

/// Verbs and phrases that read as a protective step.
const SAFE_CUES: [&str; 34] = [
    "verify", "check", "confirm", "look up", "call", "contact", "report", "ask", "official", "block", "delete", "ignore",
    "decline", "refuse", "walk away", "say no", "visit", "in person", "search", "tour", "meet", "compare", "research",
    "flag", "hang up", "close", "wait", "pause", "slow down", "talk to", "tell", "review", "bookmark", "forward",
];

/// Verbs and phrases that read as handing the scammer what they want.
const RISKY_CUES: [&str; 22] = [
    "click", "tap", "pay", "send", "share", "give", "reply", "enter", "sign in", "log in", "login", "transfer", "deposit",
    "agree", "accept", "buy", "hurry", "rush", "trust", "scan", "download", "hand over",
];

/// Openers that turn the rest of a label into a refusal ("Don't pay yet").
const REFUSALS: [&str; 8] = ["don t", "do not", "never", "refuse", "decline", "say no", "skip", "ignore"];

fn count(n: &str, cues: &[&str]) -> usize {
    cues.iter().filter(|c| has_phrase(n, c)).count()
}

/// Read a label or advice line as a safe, risky or mixed action by its words.
pub fn stance(text: &str) -> Stance {
    let n = norm(text);
    if REFUSALS.iter().any(|r| n.starts_with(&format!(" {r} "))) {
        return Stance::Safe;
    }
    match (count(&n, &SAFE_CUES) > 0, count(&n, &RISKY_CUES) > 0) {
        (true, false) => Stance::Safe,
        (false, true) => Stance::Risky,
        (true, true) => Stance::Mixed,
        (false, false) => Stance::Unclear,
    }
}

fn outcome_of(raw: &str) -> Option<Outcome> {
    match raw.trim().trim_matches(|c: char| !c.is_ascii_alphabetic()).to_ascii_lowercase().as_str() {
        "safe" => Some(Outcome::Safe),
        "risky" => Some(Outcome::Risky),
        "mixed" => Some(Outcome::Mixed),
        _ => None,
    }
}

/// A short title or button label: cleaned for the font, within `max`, no links or
/// money apps. (Labels have no final period, so `game_line` does not apply.)
fn label_ok(raw: &str, max: usize) -> Option<String> {
    let cleaned = ai::clean_for_font(raw);
    let c = cleaned.trim().trim_end_matches(['.', '!']).trim().to_string();
    let low = c.to_ascii_lowercase();
    let bad = c.len() < 3
        || c.len() > max
        || !c.is_ascii()
        || has_host(&c)
        || has_payment(&c)
        || ["http", "www.", "@", "://"].iter().any(|b| low.contains(b));
    (!bad).then_some(c)
}

fn get<'a>(got: &[(&'static str, &'a str)], key: &str) -> Option<&'a str> {
    got.iter().find(|(k, _)| *k == key).map(|(_, v)| *v)
}

fn req<T>(v: Option<T>, what: &str) -> Result<T, String> {
    v.ok_or_else(|| format!("bad or missing {what}"))
}

/// A spoken or advice line in the game's voice.
fn voice(raw: Option<&str>, what: &str, max: usize) -> Result<String, String> {
    let line = req(raw.and_then(|r| game_line(r, max)), what)?;
    if has_domain_trust(&line) {
        return Err(format!("{what} treats a domain as proof"));
    }
    Ok(line)
}

/// Parse and verify the AI's reply for `theme`. `rotation` shifts the choice order.
pub fn build(theme: &Theme, raw: &str, rotation: usize) -> Result<Encounter, String> {
    let got = labeled(raw, &LABELS);
    let name = req(get(&got, "NAME").and_then(|s| label_ok(s, MAX_NAME)), "NAME")?;
    let intro = voice(get(&got, "INTRO"), "INTRO", MAX_INTRO)?;
    let scenario = req(get(&got, "SCENARIO").and_then(|s| scam_prose(s, MAX_SCENARIO)), "SCENARIO")?;
    // the scenario must actually show every flag code planted
    for f in theme.flags {
        if !f.present_in(&scenario) {
            return Err(format!("scenario does not show the '{}' flag", f.id()));
        }
    }
    let character = get(&got, "CHARACTER").and_then(|s| label_ok(s, MAX_NAME)).unwrap_or_else(|| name.clone());

    let mut choices: Vec<Choice> = Vec::new();
    for k in 1..=3 {
        let key = |s: &str| format!("C{k}_{s}");
        let label = req(get(&got, &key("LABEL")).and_then(|s| label_ok(s, MAX_LABEL)), &key("LABEL"))?;
        let outcome = req(get(&got, &key("OUTCOME")).and_then(outcome_of), &key("OUTCOME"))?;
        // the rubric: the label's words must agree with the AI's outcome
        let read = stance(&label);
        let agrees = match outcome {
            Outcome::Safe => read == Stance::Safe,
            Outcome::Risky => read == Stance::Risky,
            Outcome::Mixed => matches!(read, Stance::Mixed | Stance::Unclear),
        };
        if !agrees {
            return Err(format!("choice {k} '{label}' reads {read:?} but is labelled {}", outcome.as_str()));
        }
        let reaction = voice(get(&got, &key("REACTION")), &key("REACTION"), MAX_REACTION)?;
        if has_payment(&reaction) {
            return Err(format!("choice {k} reaction names a payment method"));
        }
        let (s1, s2) = (voice(get(&got, &key("SIGN1")), &key("SIGN1"), MAX_SIGN)?, voice(get(&got, &key("SIGN2")), &key("SIGN2"), MAX_SIGN)?);
        if norm(&s1) == norm(&s2) {
            return Err(format!("choice {k} repeats a warning sign"));
        }
        let next = voice(get(&got, &key("NEXT")), &key("NEXT"), MAX_NEXT)?;
        if has_payment(&next) || !matches!(stance(&next), Stance::Safe | Stance::Mixed) {
            return Err(format!("choice {k} safer move is not a clear safe action"));
        }
        choices.push(Choice {
            id: format!("{}_c{k}", theme.id),
            label,
            outcome,
            effects: effects_for(outcome),
            fallback_reaction: reaction,
            warning_signs: vec![s1, s2],
            safer_next_move: next,
            keeper: None,
        });
    }
    if !choices.iter().any(|c| c.outcome == Outcome::Safe) || !choices.iter().any(|c| c.outcome == Outcome::Risky) {
        return Err("needs at least one safe and one risky choice".into());
    }
    for i in 0..choices.len() {
        for j in i + 1..choices.len() {
            if norm(&choices[i].label) == norm(&choices[j].label) {
                return Err("two choices share a label".into());
            }
        }
    }
    choices.rotate_left(rotation % 3);
    Ok(Encounter {
        id: format!("gen_{}", theme.id),
        name,
        room_intro: intro,
        character: Character { name: character, portrait_hint: String::new() },
        scenario,
        choices,
        keeper: None,
        keeper_box: None,
        source: "ai".into(),
    })
}

/// Verify the reply for slot `slot` of the run with `seed`.
pub fn generate(seed: u32, slot: usize, raw: &str) -> Result<Encounter, String> {
    build(theme_for(seed, slot), raw, rotation_for(seed, slot))
}

#[cfg(test)]
pub(crate) const SAMPLE_REPLY: &str = "NAME: The Sublease Specter\n\
INTRO: A flickering listing board hangs in the hall, covered in too-good rent offers.\n\
SCENARIO: The Specter shows a cozy apartment near campus for half the usual rent. It needs a deposit today to hold the unit, and it cannot meet in person until the money is sent.\n\
CHARACTER: The Sublease Specter\n\
C1_LABEL: Pay the deposit\n\
C1_OUTCOME: risky\n\
C1_REACTION: Ha, a ghost landlord and a fast deposit, what a bargain for me!\n\
C1_SIGN1: A price far below the market is bait.\n\
C1_SIGN2: Rushing a deposit before a tour is a pressure tactic.\n\
C1_NEXT: Tour the place in person and verify the landlord first.\n\
C2_LABEL: Ask, then pay half\n\
C2_OUTCOME: mixed\n\
C2_REACTION: Half a deposit is still bait, and my sweet talk grows louder now!\n\
C2_SIGN1: A partial payment still rewards the pressure.\n\
C2_SIGN2: Sellers who dodge video tours may not own the unit.\n\
C2_NEXT: Verify the owner through the campus housing office before anything else.\n\
C3_LABEL: Tour it in person\n\
C3_OUTCOME: safe\n\
C3_REACTION: Drat, a real visit ruins my whole spooky scheme, so I vanish!\n\
C3_SIGN1: Real landlords expect you to see the place first.\n\
C3_SIGN2: A listing you can verify holds up to simple checks.\n\
C3_NEXT: Confirm the listing with the campus housing office, then decide calmly.\n";

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = SAMPLE_REPLY;

    fn theme0() -> &'static Theme {
        assert_eq!(THEMES[0].id, "sublease_specter");
        &THEMES[0]
    }

    #[test]
    fn a_good_reply_builds_and_effects_come_from_code() {
        let e = build(theme0(), GOOD, 0).unwrap();
        assert_eq!(e.id, "gen_sublease_specter");
        assert_eq!(e.choices.len(), 3);
        assert_eq!(e.choices[0].outcome, Outcome::Risky);
        assert_eq!(e.choices[2].effects, effects_for(Outcome::Safe));
        assert_eq!(e.choices[0].effects, effects_for(Outcome::Risky));
        assert!(e.keeper.is_none() && e.keeper_box.is_none());
        assert!(e.choices.iter().all(|c| c.warning_signs.len() == 2 && c.keeper.is_none()));
    }

    #[test]
    fn rotation_moves_choices_without_changing_them() {
        let a = build(theme0(), GOOD, 0).unwrap();
        let b = build(theme0(), GOOD, 1).unwrap();
        assert_eq!(a.choices[1].label, b.choices[0].label);
        assert_eq!(b.choices.len(), 3);
    }

    #[test]
    fn mislabelled_choices_are_rejected() {
        // a risky action labelled safe
        let bad = GOOD.replace("C1_OUTCOME: risky", "C1_OUTCOME: safe");
        assert!(build(theme0(), &bad, 0).unwrap_err().contains("reads"));
        // a safe action labelled risky
        let bad = GOOD.replace("C3_OUTCOME: safe", "C3_OUTCOME: risky");
        assert!(build(theme0(), &bad, 0).is_err());
        // a vague label cannot be safe
        let bad = GOOD.replace("C3_LABEL: Tour it in person", "C3_LABEL: Think about it");
        assert!(build(theme0(), &bad, 0).is_err());
    }

    #[test]
    fn scenario_must_show_both_planted_flags() {
        let no_urgency = GOOD.replace("needs a deposit today to hold", "needs a deposit to hold");
        assert!(build(theme0(), &no_urgency, 0).unwrap_err().contains("urgency"));
        let no_fee = GOOD.replace("a deposit today", "a quick yes today");
        assert!(build(theme0(), &no_fee, 0).unwrap_err().contains("upfront_fee"));
    }

    #[test]
    fn unsafe_text_is_rejected() {
        for (from, to) in [
            ("C1_REACTION: Ha, a ghost landlord and a fast deposit, what a bargain for me!", "C1_REACTION: Just use Zelle to settle it, which is a great bargain for me!"),
            ("C3_NEXT: Confirm the listing with the campus housing office, then decide calmly.", "C3_NEXT: Confirm the listing at housing.example, then decide calmly."),
            ("C2_SIGN1: A partial payment still rewards the pressure.", "C2_SIGN1: Email me at ghost@mail.example for a partial payment trick."),
            ("C3_NEXT: Confirm the listing with the campus housing office, then decide calmly.", "C3_NEXT: Pay the deposit and sign the lease right away, friend."),
            ("C1_SIGN2: Rushing a deposit before a tour is a pressure tactic.", "C1_SIGN2: A price far below the market is bait."),
        ] {
            assert!(GOOD.contains(from), "{from}");
            assert!(build(theme0(), &GOOD.replace(from, to), 0).is_err(), "{to}");
        }
    }

    #[test]
    fn missing_or_duplicate_pieces_are_rejected() {
        assert!(build(theme0(), "just some chatter", 0).is_err());
        assert!(build(theme0(), &GOOD.replace("C2_NEXT:", "C2_NOTE:"), 0).is_err());
        // no risky choice at all
        let all_safe = GOOD.replace("C1_LABEL: Pay the deposit", "C1_LABEL: Decline the offer").replace("C1_OUTCOME: risky", "C1_OUTCOME: safe");
        assert!(build(theme0(), &all_safe, 0).unwrap_err().contains("risky"));
        // two choices with one label
        let dup = GOOD.replace("C3_LABEL: Tour it in person", "C3_LABEL: Pay the deposit").replace("C3_OUTCOME: safe", "C3_OUTCOME: risky");
        assert!(build(theme0(), &dup, 0).is_err());
    }

    #[test]
    fn stance_reads_words_not_labels() {
        assert_eq!(stance("Click the link"), Stance::Risky);
        assert_eq!(stance("Verify the sender"), Stance::Safe);
        assert_eq!(stance("Don't pay yet"), Stance::Safe);
        assert_eq!(stance("Ask, then pay half"), Stance::Mixed);
        assert_eq!(stance("Think about it"), Stance::Unclear);
    }

    #[test]
    fn plans_are_deterministic_distinct_and_flagged() {
        for seed in [0u32, 1, 7, 12345, 0xdead_beef] {
            let ids: Vec<&str> = (0..GEN_SLOTS).map(|s| theme_for(seed, s).id).collect();
            assert_eq!(ids, (0..GEN_SLOTS).map(|s| theme_for(seed, s).id).collect::<Vec<_>>());
            let mut u = ids.clone();
            u.sort();
            u.dedup();
            assert_eq!(u.len(), GEN_SLOTS, "{ids:?}");
        }
        for t in &THEMES {
            for f in t.flags {
                assert!(!f.markers().is_empty(), "{}: {} has no cue words", t.id, f.id());
            }
            assert!(t.flags[0] != t.flags[1], "{}", t.id);
        }
    }

    #[test]
    fn floors_map_to_slots_and_back() {
        assert_eq!((0..GEN_SLOTS).map(floor_for_slot).collect::<Vec<_>>(), vec![2, 5, 8]);
        assert_eq!((0..10).filter_map(slot_for_floor).collect::<Vec<_>>(), vec![0, 1, 2]);
        assert_eq!(slot_for_floor(0), None);
        assert_eq!(slot_for_floor(11), None);
    }

    #[test]
    fn user_message_names_theme_and_flag_cues() {
        let m = user_message(5, 0);
        let t = theme_for(5, 0);
        assert!(m.contains(t.name) && m.contains(t.pitch));
        assert!(m.contains(t.flags[0].default_label()));
        assert!(m.starts_with("TASK: generate_encounter"));
    }
}
