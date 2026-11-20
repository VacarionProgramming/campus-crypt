//! Live scam generator.
//!
//! Code decides everything that matters: which template, which red flags, the
//! sender line and the link placeholder (`plan`). The AI only writes the body
//! prose around the code-chosen flags. `build` then checks the prose for each
//! flag's cue words and injects the template's scripted flag sentence for any
//! flag it can't find, so every listed flag is visibly in the message and the
//! right answer always comes from `ScamMessage::flags`, never from the AI.

use super::data::{FeatureData, ScamTemplate, DEFAULT_FAKE_SENDER};
use super::validate::{has_phrase, norm, scam_prose};
use crate::ai::LineSource;
use crate::keeper::mix;

/// Shown where a link would be. Code inserts it; the AI never writes links.
pub const LINK_PLACEHOLDER: &str = "[suspicious link]";
/// Max chars for the AI's body prose (after font cleaning).
pub const MAX_BODY_CHARS: usize = 340;
/// Messages prefetched per run (one AI call each).
pub const POOL_SIZE: usize = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum RedFlag {
    MismatchedSender,
    SuspiciousLink,
    Urgency,
    UpfrontFee,
    CredentialRequest,
    TooGoodPay,
    GiftCards,
    Secrecy,
}

impl RedFlag {
    pub const ALL: [RedFlag; 8] = [
        RedFlag::MismatchedSender,
        RedFlag::SuspiciousLink,
        RedFlag::Urgency,
        RedFlag::UpfrontFee,
        RedFlag::CredentialRequest,
        RedFlag::TooGoodPay,
        RedFlag::GiftCards,
        RedFlag::Secrecy,
    ];

    pub fn id(self) -> &'static str {
        match self {
            RedFlag::MismatchedSender => "mismatched_sender",
            RedFlag::SuspiciousLink => "suspicious_link",
            RedFlag::Urgency => "urgency",
            RedFlag::UpfrontFee => "upfront_fee",
            RedFlag::CredentialRequest => "credential_request",
            RedFlag::TooGoodPay => "too_good_pay",
            RedFlag::GiftCards => "gift_cards",
            RedFlag::Secrecy => "secrecy",
        }
    }

    pub fn from_id(id: &str) -> Option<RedFlag> {
        RedFlag::ALL.into_iter().find(|f| f.id() == id.trim())
    }

    /// Shown by the header (sender line / link line), not the prose.
    pub fn in_header(self) -> bool {
        matches!(self, RedFlag::MismatchedSender | RedFlag::SuspiciousLink)
    }

    /// Whole-word cue phrases that show this flag in prose (empty for header flags).
    pub fn markers(self) -> &'static [&'static str] {
        match self {
            RedFlag::MismatchedSender | RedFlag::SuspiciousLink => &[],
            RedFlag::Urgency => &[
                "today", "tonight", "right now", "right away", "immediately", "urgent", "urgently", "expire", "expires",
                "expiring", "asap", "act now", "by midnight", "before midnight", "next hour", "minutes", "last chance",
            ],
            RedFlag::UpfrontFee => &[
                "fee", "fees", "pay the", "pay a", "pay now", "deposit", "wire", "send money", "processing charge",
                // a named way to pay is a money ask: only allowed when code picked this flag
                "zelle", "venmo", "cash app", "cashapp", "paypal", "crypto", "bitcoin", "money order", "western union",
                "moneygram",
            ],
            RedFlag::CredentialRequest => &[
                "password", "passwords", "login", "log in", "sign in", "netid", "username", "verification code", "passcode",
                "social security", "ssn", "bank details", "account number",
            ],
            RedFlag::TooGoodPay => &[
                "per hour", "an hour", "a week", "weekly", "salary", "pay rate", "for just", "no experience", "no interview",
                "easy money", "brand new",
            ],
            RedFlag::GiftCards => &["gift card", "gift cards"],
            RedFlag::Secrecy => &[
                "between us", "don't tell", "do not tell", "don't mention", "do not mention", "keep this", "secret", "confidential",
                "keep it quiet",
            ],
        }
    }

    /// Prose contains one of the cue phrases.
    pub fn present_in(self, text: &str) -> bool {
        let n = norm(text);
        self.markers().iter().any(|m| has_phrase(&n, m))
    }

    /// Built-in label if the content file has none.
    pub fn default_label(self) -> &'static str {
        match self {
            RedFlag::MismatchedSender => "Sender doesn't match",
            RedFlag::SuspiciousLink => "Mystery link",
            RedFlag::Urgency => "Rush pressure",
            RedFlag::UpfrontFee => "Pay first",
            RedFlag::CredentialRequest => "Asks for your login",
            RedFlag::TooGoodPay => "Too-good pay",
            RedFlag::GiftCards => "Gift card payment",
            RedFlag::Secrecy => "Keep it secret",
        }
    }

    /// Built-in flag sentence if the template has none (always contains a marker).
    pub fn default_line(self) -> &'static str {
        match self {
            RedFlag::MismatchedSender | RedFlag::SuspiciousLink => "",
            RedFlag::Urgency => "You must respond today or this expires.",
            RedFlag::UpfrontFee => "A small fee is required before anything can be released.",
            RedFlag::CredentialRequest => "Reply with your NetID and password to confirm.",
            RedFlag::TooGoodPay => "It pays 50 dollars per hour for almost no work.",
            RedFlag::GiftCards => "Only gift cards are accepted, so buy some and send the codes.",
            RedFlag::Secrecy => "Please keep this between us for now.",
        }
    }

    /// What the AI must leave out when this flag isn't picked.
    pub fn ai_avoid(self) -> &'static str {
        match self {
            RedFlag::MismatchedSender | RedFlag::SuspiciousLink => "",
            RedFlag::Urgency => "no deadline or rush (avoid today, tonight, now, expires, minutes)",
            RedFlag::UpfrontFee => "no fee or deposit",
            RedFlag::CredentialRequest => "no password or login request",
            RedFlag::TooGoodPay => "no pay rate or bargain price",
            RedFlag::GiftCards => "no gift cards",
            RedFlag::Secrecy => "no request for secrecy",
        }
    }

    /// Cue words to show the AI, so it includes a detectable phrase.
    pub fn ai_cue(self) -> &'static str {
        match self {
            RedFlag::MismatchedSender | RedFlag::SuspiciousLink => "",
            RedFlag::Urgency => "pressure to act fast (use a word like today, tonight, right now, or expires)",
            RedFlag::UpfrontFee => "asks the student to pay a fee or deposit first (use the word fee, payment, or deposit)",
            RedFlag::CredentialRequest => "asks for the student's login (use the word password, login, or NetID)",
            RedFlag::TooGoodPay => "pay or a deal that is too good to be true (use per hour, a week, or for just)",
            RedFlag::GiftCards => "asks to be paid in gift cards (use the words gift card)",
            RedFlag::Secrecy => "asks the student to keep it secret (use keep this between us, or don't tell)",
        }
    }
}

/// Code's decision for one message: deterministic from (run seed, slot).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ScamPlan {
    pub slot: usize,
    pub template: usize,
    /// sorted, 2-3 flags
    pub flags: Vec<RedFlag>,
    /// which scripted body to use when the AI has none
    pub body: usize,
}

/// Pick template and flags from the seed. Same seed + slot = same plan.
/// Slots of one run walk through templates in a seeded order, so a pool doesn't repeat
/// templates until it has used them all.
pub fn plan(data: &FeatureData, seed: u32, slot: usize) -> ScamPlan {
    let n = data.scam_templates.len().max(1);
    let start = mix(seed, 0x5ca3) as usize % n;
    let template = (start + slot) % n;
    let t = &data.scam_templates[template.min(data.scam_templates.len().saturating_sub(1))];
    let mut flags: Vec<RedFlag> = t.flags.iter().filter_map(|f| RedFlag::from_id(f)).collect();
    flags.dedup();
    // seeded Fisher-Yates, then keep 2 or 3
    let h = mix(seed, 0x10_000 + slot as u32);
    for i in (1..flags.len()).rev() {
        let j = mix(h, i as u32) as usize % (i + 1);
        flags.swap(i, j);
    }
    let keep = (2 + (h as usize % 2)).min(flags.len());
    flags.truncate(keep);
    flags.sort();
    let body = mix(h, 0xb0d1) as usize % t.bodies.len().max(1);
    ScamPlan { slot, template, flags, body }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScamMessage {
    pub template_id: String,
    /// "email" | "text" | "dm"
    pub channel: String,
    /// "From: ..." text (includes the fictional address only for `MismatchedSender`)
    pub from: String,
    pub subject: String,
    /// prose with every prose flag present
    pub body: String,
    /// `Some(LINK_PLACEHOLDER)` when the plan has `SuspiciousLink`
    pub link: Option<&'static str>,
    pub sign_off: String,
    /// the answer key: exactly the flags code picked, all visibly present
    pub flags: Vec<RedFlag>,
    /// display labels, same order as `flags`
    pub flag_labels: Vec<String>,
    /// AI prose or scripted body (flag sentences may be injected either way)
    pub source: LineSource,
    /// how many flag sentences code had to add
    pub injected: usize,
}

impl ScamMessage {
    /// The whole message as display lines (sender, subject, body, link, sign-off).
    pub fn lines(&self) -> Vec<String> {
        let mut v = vec![format!("From: {}", self.from), format!("Subject: {}", self.subject), self.body.clone()];
        if let Some(l) = self.link {
            v.push(l.to_string());
        }
        v.push(self.sign_off.clone());
        v
    }
}

fn template<'a>(data: &'a FeatureData, p: &ScamPlan) -> &'a ScamTemplate {
    &data.scam_templates[p.template.min(data.scam_templates.len() - 1)]
}

/// The user message for the AI (facts only; the system prompt is `## scam_body`).
pub fn user_message(data: &FeatureData, p: &ScamPlan) -> String {
    let t = template(data, p);
    let mut s = format!(
        "TASK: scam_body\nchannel: {}\npretends to be from: {}\nsubject (added by the game): {}\nwhat it is about: {}\nred flags to work in:\n",
        t.channel, t.sender_name, t.subject, t.topic
    );
    for f in p.flags.iter().filter(|f| !f.in_header()) {
        s.push_str(&format!("- {}: {}\n", f.id(), f.ai_cue()));
    }
    let avoid: Vec<&str> = RedFlag::ALL.iter().filter(|f| !f.in_header() && !p.flags.contains(f)).map(|f| f.ai_avoid()).collect();
    if !avoid.is_empty() {
        s.push_str(&format!("do NOT include (not part of this message): {}\n", avoid.join("; ")));
    }
    if p.flags.iter().all(|f| f.in_header()) {
        s.push_str("- none in the text (the sender and link carry the flags); keep it bland and friendly\n");
    }
    s.push_str("\nWrite only the body paragraph.");
    s
}

/// Validate AI prose for a scam body. None = use the scripted body.
pub fn validate_body(raw: &str) -> Option<String> {
    let body = scam_prose(raw, MAX_BODY_CHARS)?;
    // a body that addresses or signs as someone with an address/number is caught above;
    // also refuse bodies that try to supply their own link wording
    let n = norm(&body);
    (!has_phrase(&n, "link below") && !has_phrase(&n, "this link")).then_some(body)
}

/// Assemble the message. `ai_prose` is the raw AI reply (None = scripted body).
/// Every prose flag missing from the body gets its scripted sentence appended.
pub fn build(data: &FeatureData, p: &ScamPlan, ai_prose: Option<&str>, why_fallback: &'static str) -> ScamMessage {
    let t = template(data, p);
    let no_extra = |b: &String| extra_flags(b, &p.flags).is_empty();
    let (mut body, source) = match ai_prose.and_then(validate_body).filter(no_extra) {
        Some(b) => (b, LineSource::Ai),
        None => {
            let b = t.bodies.get(p.body).or(t.bodies.first()).cloned();
            let why = if ai_prose.is_some() { "AI line rejected by filter" } else { why_fallback };
            (b.unwrap_or_else(|| "Hi Owl, we have an important update about your account.".into()), LineSource::Fallback(why))
        }
    };
    let mut injected = 0;
    for f in p.flags.iter().filter(|f| !f.in_header()) {
        if !f.present_in(&body) {
            let line = t.flag_lines.get(f.id()).filter(|l| f.present_in(l)).map(String::as_str).unwrap_or(f.default_line());
            body.push(' ');
            body.push_str(line);
            injected += 1;
        }
    }
    let address = if t.fake_address.is_empty() { DEFAULT_FAKE_SENDER } else { &t.fake_address };
    let from = if p.flags.contains(&RedFlag::MismatchedSender) {
        format!("{} <{}>", t.sender_name, address)
    } else {
        t.sender_name.clone()
    };
    let flag_labels = p
        .flags
        .iter()
        .map(|f| data.red_flags.get(f.id()).map(|x| x.label.clone()).filter(|l| !l.is_empty()).unwrap_or(f.default_label().into()))
        .collect();
    ScamMessage {
        template_id: t.id.clone(),
        channel: t.channel.clone(),
        from,
        subject: t.subject.clone(),
        body,
        link: p.flags.contains(&RedFlag::SuspiciousLink).then_some(LINK_PLACEHOLDER),
        sign_off: t.sign_off.clone(),
        flags: p.flags.clone(),
        flag_labels,
        source,
        injected,
    }
}

/// Prose flags whose cue words appear in `body` but that the plan didn't pick.
/// AI prose with any of these is rejected, so the answer key is never short a flag.
pub fn extra_flags(body: &str, picked: &[RedFlag]) -> Vec<RedFlag> {
    RedFlag::ALL.into_iter().filter(|f| !f.in_header() && !picked.contains(f) && f.present_in(body)).collect()
}

/// Code's check that the message really carries every flag in its answer key.
pub fn verify(m: &ScamMessage) -> bool {
    m.flags.iter().all(|f| match f {
        RedFlag::MismatchedSender => m.from.contains('@') && m.from.contains(super::data::FICTIONAL_SUFFIX),
        RedFlag::SuspiciousLink => m.link == Some(LINK_PLACEHOLDER),
        f => f.present_in(&m.body),
    }) && !m.flags.is_empty()
        && extra_flags(&m.body, &m.flags).is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai_features::data::embedded;

    #[test]
    fn plans_are_deterministic_and_vary() {
        let d = embedded().data;
        for seed in [0, 1, 42, 0xdead_beef] {
            for slot in 0..POOL_SIZE {
                assert_eq!(plan(&d, seed, slot), plan(&d, seed, slot));
            }
        }
        let distinct: std::collections::HashSet<_> = (0..40).map(|s| plan(&d, s, 0)).collect();
        assert!(distinct.len() > 10, "seeds should spread plans: {}", distinct.len());
        // one run's pool doesn't repeat a template
        let p: Vec<usize> = (0..POOL_SIZE).map(|s| plan(&d, 7, s).template).collect();
        assert_eq!(p.iter().collect::<std::collections::HashSet<_>>().len(), POOL_SIZE);
    }

    #[test]
    fn scripted_messages_carry_every_flag_for_many_seeds() {
        let d = embedded().data;
        for seed in 0..300 {
            let p = plan(&d, seed, seed as usize % POOL_SIZE);
            assert!((2..=3).contains(&p.flags.len()), "{p:?}");
            let m = build(&d, &p, None, "offline");
            assert!(verify(&m), "{m:#?}");
            assert_eq!(m.source, LineSource::Fallback("offline"));
            assert_eq!(m.flags.len(), m.flag_labels.len());
            assert!(m.lines().iter().all(|l| l.is_ascii()));
        }
    }

    #[test]
    fn ai_prose_missing_a_flag_gets_it_injected() {
        let d = embedded().data;
        let p = ScamPlan { slot: 0, template: 0, flags: vec![RedFlag::Urgency, RedFlag::CredentialRequest], body: 0 };
        let m = build(&d, &p, Some("Hi Owl, your refund is ready and waiting for you. Just reply with your password to confirm it."), "x");
        assert_eq!(m.source, LineSource::Ai);
        assert_eq!(m.injected, 1, "urgency was missing: {}", m.body);
        assert!(verify(&m) && RedFlag::Urgency.present_in(&m.body));
        // all flags already present: nothing injected
        let m = build(&d, &p, Some("Hi Owl, act today! Reply with your password so your refund can move."), "x");
        assert_eq!((m.injected, m.source), (0, LineSource::Ai));
    }

    #[test]
    fn ai_links_emails_and_numbers_fall_back_to_scripted() {
        let d = embedded().data;
        let p = plan(&d, 3, 0);
        for raw in [
            "Hi Owl, go to ksu-refunds.example to claim it today.",
            "Email finaid@freemail.example today with your password.",
            "Call 470-555-0101 right now with your password, Owl.",
            "Hi Owl, click here to claim your refund today please.",
            "Visit https://evil.test now.",
            "short.",
            // a flag the plan didn't pick (gift cards) would leave the answer key short
            "Hi Owl, your refund is ready. Send three gift card codes today to release it.",
        ] {
            let m = build(&d, &p, Some(raw), "x");
            assert_eq!(m.source, LineSource::Fallback("AI line rejected by filter"), "{raw}");
            assert!(verify(&m));
            assert!(!m.body.contains("ksu-refunds") && !m.body.contains("555"));
        }
    }

    #[test]
    fn header_flags_come_from_code_placeholders() {
        let d = embedded().data;
        let p = ScamPlan { slot: 0, template: 0, flags: vec![RedFlag::MismatchedSender, RedFlag::SuspiciousLink], body: 0 };
        let m = build(&d, &p, None, "offline");
        assert!(m.from.ends_with("@freemail.example>"), "{}", m.from);
        assert_eq!(m.link, Some(LINK_PLACEHOLDER));
        assert!(verify(&m));
        let p = ScamPlan { flags: vec![RedFlag::Urgency, RedFlag::UpfrontFee], ..p };
        let m = build(&d, &p, None, "offline");
        assert!(!m.from.contains('@') && m.link.is_none());
    }

    #[test]
    fn every_template_flag_line_and_default_line_shows_its_flag() {
        let d = embedded().data;
        for f in RedFlag::ALL.iter().filter(|f| !f.in_header()) {
            assert!(f.present_in(f.default_line()), "{f:?}");
            for t in &d.scam_templates {
                if let Some(l) = t.flag_lines.get(f.id()) {
                    assert!(f.present_in(l), "{}: {l}", t.id);
                }
            }
        }
        // scripted bodies carry no flags of their own, and each flag line only its own
        for t in &d.scam_templates {
            for b in &t.bodies {
                assert!(extra_flags(b, &[]).is_empty(), "{}: body has cues {:?}: {b}", t.id, extra_flags(b, &[]));
            }
            for (id, l) in &t.flag_lines {
                let f = RedFlag::from_id(id).unwrap();
                assert_eq!(extra_flags(l, &[f]), vec![], "{}: {l}", t.id);
            }
        }
        for f in RedFlag::ALL.iter().filter(|f| !f.in_header()) {
            assert_eq!(extra_flags(f.default_line(), &[*f]), vec![], "{f:?}");
        }
        // safe messages have no cue words at all
        for m in &d.safe_messages {
            for f in RedFlag::ALL {
                assert!(!f.present_in(&m.body), "{} has a {f:?} cue", m.id);
            }
        }
    }

    #[test]
    fn user_message_lists_only_code_flags_and_no_addresses() {
        let d = embedded().data;
        let p = plan(&d, 11, 1);
        let u = user_message(&d, &p);
        for f in &p.flags {
            assert_eq!(u.contains(&format!("- {}:", f.id())), !f.in_header(), "{u}");
        }
        assert!(!u.contains('@') && !u.contains(".example"), "{u}");
    }

    #[test]
    fn named_payment_methods_count_as_an_unpicked_money_flag() {
        for body in ["Reply with your NetID and send the deposit by Zelle.", "Text back and we will Venmo you.", "Bitcoin only, please."] {
            assert!(extra_flags(body, &[RedFlag::CredentialRequest, RedFlag::Urgency]).contains(&RedFlag::UpfrontFee), "{body}");
            assert!(!extra_flags(body, &[RedFlag::UpfrontFee, RedFlag::CredentialRequest]).contains(&RedFlag::UpfrontFee));
        }
    }
}
