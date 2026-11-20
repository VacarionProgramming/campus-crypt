//! Spot-the-fake: a scripted SAFE message next to a FAKE one (generated or
//! scripted, from scamgen). Which card is fake is set by code; the order comes
//! from the seed; the AI never judges anything.

use super::data::{FeatureData, SafeMessage};
use super::scamgen::{RedFlag, ScamMessage};
use crate::ai::LineSource;
use crate::keeper::mix;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpotCard {
    pub channel: String,
    pub from: String,
    pub subject: String,
    pub body: String,
    pub link: Option<&'static str>,
    pub sign_off: String,
}

impl SpotCard {
    pub fn lines(&self) -> Vec<String> {
        let mut v = vec![format!("From: {}", self.from), format!("Subject: {}", self.subject), self.body.clone()];
        if let Some(l) = self.link {
            v.push(l.to_string());
        }
        if !self.sign_off.is_empty() {
            v.push(self.sign_off.clone());
        }
        v
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpotRound {
    /// changes per seed/round (for UI caches)
    pub id: u32,
    pub cards: [SpotCard; 2],
    fake: usize,
    /// the fake's red flags and labels (the reveal)
    pub fake_flags: Vec<RedFlag>,
    pub fake_flag_labels: Vec<String>,
    /// why the safe one is safe (scripted)
    pub safe_tells: Vec<String>,
    /// where the fake's prose came from
    pub fake_source: LineSource,
}

impl SpotRound {
    /// The only judge: code's label.
    pub fn is_fake(&self, i: usize) -> bool {
        i == self.fake
    }
    pub fn fake_index(&self) -> usize {
        self.fake
    }
}

fn safe_card(m: &SafeMessage) -> SpotCard {
    SpotCard { channel: m.channel.clone(), from: m.sender.clone(), subject: m.subject.clone(), body: m.body.clone(), link: None, sign_off: String::new() }
}

/// Safe message index for round `round` of a run.
pub fn safe_pick(data: &FeatureData, seed: u32, round: usize) -> usize {
    let n = data.safe_messages.len().max(1);
    (mix(seed, 0x5afe) as usize + round) % n
}

pub fn build(data: &FeatureData, seed: u32, round: usize, fake: &ScamMessage) -> SpotRound {
    let safe = &data.safe_messages[safe_pick(data, seed, round).min(data.safe_messages.len() - 1)];
    let fake_card = SpotCard {
        channel: fake.channel.clone(),
        from: fake.from.clone(),
        subject: fake.subject.clone(),
        body: fake.body.clone(),
        link: fake.link,
        sign_off: fake.sign_off.clone(),
    };
    let id = mix(seed, 0x5900 + round as u32);
    let fake_i = (id >> 7) as usize & 1;
    let cards = if fake_i == 0 { [fake_card, safe_card(safe)] } else { [safe_card(safe), fake_card] };
    SpotRound {
        id,
        cards,
        fake: fake_i,
        fake_flags: fake.flags.clone(),
        fake_flag_labels: fake.flag_labels.clone(),
        safe_tells: safe.tells.clone(),
        fake_source: fake.source.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai_features::data::embedded;
    use crate::ai_features::scamgen::{self, plan};

    #[test]
    fn order_is_seeded_and_label_comes_from_code() {
        let d = embedded().data;
        let mut sides = [0; 2];
        for seed in 0..200u32 {
            let fake = scamgen::build(&d, &plan(&d, seed, 0), None, "offline");
            let r = build(&d, seed, 0, &fake);
            assert_eq!(r, build(&d, seed, 0, &fake), "deterministic");
            let f = r.fake_index();
            assert!(r.is_fake(f) && !r.is_fake(1 - f));
            assert_eq!(r.cards[f].body, fake.body);
            assert!(d.safe_messages.iter().any(|m| m.body == r.cards[1 - f].body), "the other card is a scripted safe message");
            assert!(!r.cards[1 - f].from.contains("freemail"));
            sides[f] += 1;
        }
        assert!(sides[0] > 60 && sides[1] > 60, "fake lands on both sides: {sides:?}");
    }

    #[test]
    fn rounds_in_one_run_rotate_safe_messages() {
        let d = embedded().data;
        let picks: std::collections::HashSet<usize> = (0..d.safe_messages.len()).map(|r| safe_pick(&d, 9, r)).collect();
        assert_eq!(picks.len(), d.safe_messages.len());
    }

    #[test]
    fn safe_tells_and_flag_tips_never_trust_a_domain() {
        let d = crate::ai_features::data::embedded().data;
        for m in &d.safe_messages {
            for t in &m.tells {
                assert!(!crate::ai_features::validate::has_domain_trust(t), "{}: {t}", m.id);
            }
        }
        for (id, f) in &d.red_flags {
            assert!(!crate::ai_features::validate::has_domain_trust(&f.tip), "{id}: {}", f.tip);
        }
    }
}
