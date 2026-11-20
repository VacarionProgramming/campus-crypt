//! Extra checks layered on top of `ai::sanitize_len` for the AI features.
//!
//! `ai::sanitize_len` already folds text for the font and rejects links, emails,
//! stat words and unfinished lines. Here we add a stricter link check (any
//! `word.word` host, including the fictional `.example` placeholders), long digit
//! runs (phone numbers), causal claims, and phrase matching helpers.

use crate::ai;

/// Phrases that claim the game or a habit causes a real-world result.
const CAUSAL: [&str; 14] = [
    "prevent",
    "guarantee",
    "proven",
    "studies show",
    "research shows",
    "will protect you",
    "keeps you safe",
    "keep you safe",
    "makes you safe",
    "never get scammed",
    "never be scammed",
    "improve your grade",
    "raise your gpa",
    "better grades",
];

/// Lowercase, every non-alphanumeric run becomes one space, padded with spaces:
/// `" don t tell "`. Phrase matching on this is word-boundary safe ("fee" does
/// not match "coffee").
pub fn norm(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push(' ');
    for c in s.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with(' ') {
            out.push(' ');
        }
    }
    if !out.ends_with(' ') {
        out.push(' ');
    }
    out
}

/// `normed` (from `norm`) contains `phrase` as whole words.
pub fn has_phrase(normed: &str, phrase: &str) -> bool {
    let p = norm(phrase);
    !p.trim().is_empty() && normed.contains(&p)
}

/// Any `host.tld`-shaped token: a dot between an alphanumeric and two letters.
/// Stricter than the reaction filter (catches `freemail.example`, `ksu.owls`),
/// and still lets "e.g." and ellipses through.
pub fn has_host(s: &str) -> bool {
    let b = s.as_bytes();
    (1..b.len().saturating_sub(2)).any(|i| {
        b[i] == b'.' && b[i - 1].is_ascii_alphanumeric() && b[i + 1].is_ascii_alphabetic() && b[i + 2].is_ascii_alphabetic()
    })
}

/// 7+ digits in a row, ignoring spaces, dashes, dots and parens: a phone number.
pub fn has_phone(s: &str) -> bool {
    let mut run = 0;
    for c in s.chars() {
        if c.is_ascii_digit() {
            run += 1;
            if run >= 7 {
                return true;
            }
        } else if !matches!(c, ' ' | '-' | '.' | '(' | ')') {
            run = 0;
        }
    }
    false
}

pub fn has_causal(s: &str) -> bool {
    let n = norm(s);
    CAUSAL.iter().any(|p| has_phrase(&n, p))
}

/// A line in the game's own voice (Keeper, owl, narrator, debrief): the reaction
/// filter plus strict links, phone numbers and causal claims.
pub fn game_line(raw: &str, max_chars: usize) -> Option<String> {
    let line = ai::sanitize_len(raw, max_chars)?;
    (!has_host(&line) && !has_phone(&line) && !has_causal(&line)).then_some(line)
}

/// Scam prose: same, but causal words are allowed (a scammer "guarantees" pay;
/// that's the bait, not a claim by the game).
pub fn scam_prose(raw: &str, max_chars: usize) -> Option<String> {
    let line = ai::sanitize_len(raw, max_chars)?;
    (!has_host(&line) && !has_phone(&line) && !norm(&line).contains(" click here ")).then_some(line)
}

/// Pull the spoken line out of what models like to wrap around it:
/// a speaker tag (`The Floor Keeper: ...`, `Hoo: ...`), stage directions around a
/// quote (`The Lich grins, "..."`), and a stray space before a closing quote.
pub fn speech(raw: &str) -> String {
    let mut s: String = raw
        .trim()
        .chars()
        .map(|c| match c {
            '\u{201c}' | '\u{201d}' => '"',
            '\u{2018}' | '\u{2019}' => '\'',
            c => c,
        })
        .collect();
    // speaker tag: a short name with no sentence punctuation, then a colon
    if let Some((head, rest)) = s.split_once(':') {
        let words = head.split_whitespace().count();
        if (1..=4).contains(&words) && !head.contains(['.', '!', '?', ',', '"']) && !rest.trim().is_empty() {
            s = rest.trim().to_string();
        }
    }
    // narration around one quoted line: keep the quote
    if s.matches('"').count() == 2 {
        let (a, b) = (s.find('"').unwrap_or(0), s.rfind('"').unwrap_or(0));
        let inner = s[a + 1..b].trim();
        if inner.len() >= 20 && inner.len() * 10 >= (s.len() - 2) * 5 {
            s = inner.to_string();
        }
    }
    s.trim().to_string()
}

/// Split a labelled reply (`TAUNT: ...` lines) into (label, raw value) pairs.
/// Labels are matched case-insensitively; list markers and quotes before the
/// label are ignored. Unlabelled lines are dropped. The first value per label wins.
pub fn labeled<'a>(raw: &'a str, labels: &[&'static str]) -> Vec<(&'static str, &'a str)> {
    let mut out: Vec<(&'static str, &'a str)> = Vec::new();
    for line in raw.lines() {
        let t = line.trim().trim_start_matches(['-', '*', '"', '\'', ' ', '#']);
        let Some((head, value)) = t.split_once(':') else { continue };
        let head = head.trim().trim_matches(['*', '_', '`']).trim();
        if let Some(&l) = labels.iter().find(|l| l.eq_ignore_ascii_case(head)) {
            if !out.iter().any(|(k, _)| *k == l) && !value.trim().is_empty() {
                out.push((l, value.trim()));
            }
        }
    }
    out
}

/// `labeled`, but when the model forgot the labels and wrote exactly one line per
/// label, take them in order.
pub fn labeled_or_ordered<'a>(raw: &'a str, labels: &[&'static str]) -> Vec<(&'static str, &'a str)> {
    let got = labeled(raw, labels);
    if !got.is_empty() {
        return got;
    }
    let lines: Vec<&str> = raw.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    if lines.len() == labels.len() {
        labels.iter().copied().zip(lines).collect()
    } else {
        Vec::new()
    }
}

/// Payment methods and money-moving words. Banter (taunts, verdicts, hints,
/// tier narration) never names one, so the game never voices a scam's
/// instructions; only scam bodies carry money cues, and only for picked flags.
const PAYMENT: [&str; 30] = [
    "gift card",
    "gift cards",
    "giftcard",
    "giftcards",
    "zelle",
    "venmo",
    "cashapp",
    "cash app",
    "paypal",
    "wire",
    "wired",
    "wiring",
    "wire transfer",
    "crypto",
    "cryptocurrency",
    "bitcoin",
    "bitcoins",
    "btc",
    "ethereum",
    "usdt",
    "western union",
    "moneygram",
    "money order",
    "apple pay",
    "google pay",
    "prepaid card",
    "send money",
    "send cash",
    "routing number",
    "bank account",
];

/// Names a payment method or tells someone to move money (see `PAYMENT`).
pub fn has_payment(s: &str) -> bool {
    let n = norm(s);
    PAYMENT.iter().any(|p| has_phrase(&n, p))
}

/// Phrases that present an address or domain as proof a message is safe.
const DOMAIN_TRUST: [&str; 24] = [
    "edu",
    "gov",
    "official domain",
    "real domain",
    "right domain",
    "trusted domain",
    "safe domain",
    "school domain",
    "campus domain",
    "campus address",
    "university domain",
    "official address",
    "official email address",
    "domain is safe",
    "domain checks out",
    "domain looks right",
    "own domain",
    "address belong",
    "address belongs",
    "address feels",
    "trusted address",
    "trusted sender",
    "trusted ally",
    "address you trust",
];

/// Treats a domain (".edu", ".gov", "the official domain") as a safety signal.
/// Look-alike domains are easy to buy and real accounts get hacked, so the
/// owl and the debrief point at contact you start yourself instead.
pub fn has_domain_trust(s: &str) -> bool {
    let n = norm(s);
    DOMAIN_TRUST.iter().any(|p| has_phrase(&n, p))
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strict_links_catch_placeholders_and_any_host() {
        for s in [
            "Write to careers-ksu-owls@freemail.example today.",
            "Visit freemail.example for more.",
            "Go to ksu.owls now, friend.",
            "Our site owl-jobs.biz has it all.",
        ] {
            assert!(has_host(s) || s.contains('@'), "{s}");
            assert!(game_line(s, 160).is_none() && scam_prose(s, 300).is_none(), "{s}");
        }
        for s in ["Wait...what was that, Owl?", "Bring snacks, e.g. coffee.", "It is 5.30 already."] {
            assert!(!has_host(s), "{s}");
        }
    }

    #[test]
    fn phones_and_causal_claims_are_rejected() {
        assert!(has_phone("Call 470-555-0199 now"));
        assert!(has_phone("(770) 555 0100"));
        assert!(!has_phone("Pay 45 dollars by 9 tonight"));
        assert!(game_line("This habit will prevent every scam you ever see.", 160).is_none());
        assert!(game_line("Checking senders keeps you safe forever, little Owl.", 160).is_none());
        assert!(game_line("Hoo, who really sent this message to you?", 160).is_some());
        // scam bait may "guarantee" things, but no links
        assert!(scam_prose("Guaranteed pay for easy work, start today and earn big!", 300).is_some());
        assert!(scam_prose("Just click here to claim your refund before it is gone.", 300).is_none());
    }

    #[test]
    fn phrases_match_whole_words_only() {
        let n = norm("Grab a coffee, Owl! Don't tell anyone.");
        assert!(!has_phrase(&n, "fee"));
        assert!(has_phrase(&n, "don't tell"));
        assert!(has_phrase(&n, "coffee"));
        assert!(!has_phrase(&n, ""));
    }

    #[test]
    fn speech_unwraps_tags_and_narration() {
        assert_eq!(speech("The Floor Keeper: You answered well, Owl."), "You answered well, Owl.");
        assert_eq!(speech("Hoo: Does the sender look odd?"), "Does the sender look odd?");
        assert_eq!(
            speech("The Ogre chuckles, \u{201c}Excellent choice, Owl! The stairs await your next move. \u{201d}"),
            "Excellent choice, Owl! The stairs await your next move."
        );
        // a real sentence with a colon is left alone
        assert_eq!(speech("Listen well, Owl: the stairs are yours."), "Listen well, Owl: the stairs are yours.");
        assert_eq!(speech("I hiss, \"Fine.\" and leave in a huff today."), "I hiss, \"Fine.\" and leave in a huff today.");
        let raw = "One line here.\nTwo.\n\nThree.\nFour.";
        assert_eq!(labeled_or_ordered(raw, &["A", "B", "C", "D"])[3], ("D", "Four."));
        assert!(labeled_or_ordered("Only one.", &["A", "B"]).is_empty());
    }

    #[test]
    fn labeled_lines_are_parsed_loosely() {
        let raw = "Sure!\n- TAUNT: Ha, there you are.\n**Right**: Well done.\nwrong:  Nope.\nTAUNT: second one ignored\nHINT:";
        let got = labeled(raw, &["TAUNT", "RIGHT", "WRONG", "HINT"]);
        assert_eq!(got, vec![("TAUNT", "Ha, there you are."), ("RIGHT", "Well done."), ("WRONG", "Nope.")]);
    }

    #[test]
    fn payment_and_domain_trust_words_are_caught() {
        for bad in ["Pay with a Gift Card now", "Venmo me", "send it by wire", "a little Bitcoin", "Cash App works", "zelle it"] {
            assert!(has_payment(bad), "{bad}");
        }
        for ok in ["the wireless printer", "a cryptic riddle", "a cardigan sale", "Hoo, who started this chat?"] {
            assert!(!has_payment(ok), "{ok}");
        }
        for bad in ["ask yourself if the address feels like a trusted ally", "Sent from the school's own domain", "Trust it if it ends in .edu", "a .gov address is safe", "Check for the official domain", "it came from a campus address"] {
            assert!(has_domain_trust(bad), "{bad}");
        }
        for ok in ["Look up the office yourself", "the schedule education fair", "Hoo, who sent it?"] {
            assert!(!has_domain_trust(ok), "{ok}");
        }
    }
}
