//! Adaptive difficulty. Code computes the tier from performance (deterministic,
//! tested); the AI only narrates a change in one line, from lines prefetched at
//! run start (the "run pack"), with scripted lines per change as fallback.

use super::data::FeatureData;
use super::validate::{game_line, has_phrase, labeled_or_ordered, norm, speech};
use crate::ai::LineSource;
use crate::content::Outcome;
use crate::keeper::mix;
use crate::state::Run;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SkillTier {
    Gentle,
    Standard,
    Sharp,
    Expert,
}

/// Suggested knobs per tier for the crawl layer (Darby decides what to apply).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TierParams {
    /// percent of normal monster speed
    pub monster_speed_pct: u32,
    /// extra (or fewer) monsters per floor
    pub monster_count_delta: i32,
    /// percent of normal warden HP
    pub warden_hp_pct: u32,
    /// owl hint hesitation threshold
    pub hint_after_secs: f64,
}

impl SkillTier {
    pub const START: SkillTier = SkillTier::Standard;

    pub fn id(self) -> &'static str {
        match self {
            SkillTier::Gentle => "gentle",
            SkillTier::Standard => "standard",
            SkillTier::Sharp => "sharp",
            SkillTier::Expert => "expert",
        }
    }
    fn up(self) -> SkillTier {
        match self {
            SkillTier::Gentle => SkillTier::Standard,
            SkillTier::Standard => SkillTier::Sharp,
            _ => SkillTier::Expert,
        }
    }
    fn down(self) -> SkillTier {
        match self {
            SkillTier::Expert => SkillTier::Sharp,
            SkillTier::Sharp => SkillTier::Standard,
            _ => SkillTier::Gentle,
        }
    }
    pub fn params(self) -> TierParams {
        match self {
            SkillTier::Gentle => TierParams { monster_speed_pct: 85, monster_count_delta: -1, warden_hp_pct: 85, hint_after_secs: 8.0 },
            SkillTier::Standard => TierParams { monster_speed_pct: 100, monster_count_delta: 0, warden_hp_pct: 100, hint_after_secs: 12.0 },
            SkillTier::Sharp => TierParams { monster_speed_pct: 110, monster_count_delta: 1, warden_hp_pct: 110, hint_after_secs: 16.0 },
            SkillTier::Expert => TierParams { monster_speed_pct: 120, monster_count_delta: 2, warden_hp_pct: 120, hint_after_secs: 20.0 },
        }
    }
}

/// What the tier is computed from. Built from `Run` only.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Performance {
    pub safe: u32,
    pub mixed: u32,
    pub risky: u32,
    pub keepers_right: u32,
    pub keepers_wrong: u32,
    pub health: i32,
}

impl Performance {
    pub fn from_run(run: &Run) -> Performance {
        let mut p = Performance { health: run.stats.health, ..Default::default() };
        for d in &run.history {
            match d.outcome {
                Outcome::Safe => p.safe += 1,
                Outcome::Mixed => p.mixed += 1,
                Outcome::Risky => p.risky += 1,
            }
        }
        for k in &run.keepers {
            if k.correct {
                p.keepers_right += 1;
            } else {
                p.keepers_wrong += 1;
            }
        }
        p
    }
    pub fn decisions(&self) -> u32 {
        self.safe + self.mixed + self.risky + self.keepers_right + self.keepers_wrong
    }
    /// 0-100: safe and right answers are worth 2, mixed 1, risky and wrong 0.
    pub fn accuracy_pct(&self) -> u32 {
        let d = self.decisions();
        if d == 0 {
            return 50;
        }
        100 * (2 * self.safe + self.mixed + 2 * self.keepers_right) / (2 * d)
    }
}

/// The tier performance points to.
pub fn target_tier(p: &Performance) -> SkillTier {
    let acc = p.accuracy_pct();
    let d = p.decisions();
    if d == 0 {
        SkillTier::START
    } else if p.health <= 1 || acc < 50 {
        SkillTier::Gentle
    } else if acc >= 90 && d >= 4 && p.health >= 4 {
        SkillTier::Expert
    } else if acc >= 75 && d >= 2 {
        SkillTier::Sharp
    } else {
        SkillTier::Standard
    }
}

/// One step at most toward the target (called once per floor, on descend).
pub fn next_tier(current: SkillTier, p: &Performance) -> SkillTier {
    let t = target_tier(p);
    match t.cmp(&current) {
        std::cmp::Ordering::Greater => current.up(),
        std::cmp::Ordering::Less => current.down(),
        std::cmp::Ordering::Equal => current,
    }
}

/// tier_lines / run pack key for a one-step change.
pub fn change_key(from: SkillTier, to: SkillTier) -> Option<&'static str> {
    Some(match (from, to) {
        (SkillTier::Gentle, SkillTier::Standard) => "up_standard",
        (SkillTier::Standard, SkillTier::Sharp) => "up_sharp",
        (SkillTier::Sharp, SkillTier::Expert) => "up_expert",
        (SkillTier::Standard, SkillTier::Gentle) => "down_gentle",
        (SkillTier::Sharp, SkillTier::Standard) => "down_standard",
        (SkillTier::Expert, SkillTier::Sharp) => "down_sharp",
        _ => return None,
    })
}

pub const RUN_LABELS: [&str; 6] = ["UP_STANDARD", "UP_SHARP", "UP_EXPERT", "DOWN_GENTLE", "DOWN_STANDARD", "DOWN_SHARP"];
const EASIER: [&str; 10] = ["easier", "eases", "ease", "gentle", "gentler", "breather", "rest", "relax", "loosens", "backs off"];
const HARDER: [&str; 10] = ["harder", "tougher", "cocky", "sneakier", "no mercy", "fiercer", "deadlier", "wakes", "stops going easy", "sharpens"];
pub const MAX_NARRATION_CHARS: usize = 140;

/// An UP line never sounds easier, a DOWN line never harder; no numbers.
pub fn narration_ok(key: &str, line: &str) -> bool {
    let n = norm(line);
    let bad: &[&str] = if key.starts_with("up_") { &EASIER } else { &HARDER };
    !bad.iter().any(|w| has_phrase(&n, w)) && !line.bytes().any(|b| b.is_ascii_digit()) && !super::validate::has_payment(line)
}

/// Validated AI narration lines keyed by `change_key` (missing = scripted).
pub fn parse_run_pack(raw: &str) -> Vec<(&'static str, String)> {
    labeled_or_ordered(raw, &RUN_LABELS)
        .into_iter()
        .filter_map(|(label, v)| {
            let key = TIER_KEY_OF[RUN_LABELS.iter().position(|l| *l == label)?];
            game_line(&speech(v), MAX_NARRATION_CHARS).filter(|l| narration_ok(key, l)).map(|l| (key, l))
        })
        .collect()
}
const TIER_KEY_OF: [&str; 6] = super::data::TIER_KEYS;

pub fn scripted_narration(data: &FeatureData, key: &str, seed: u32, floor: usize) -> String {
    match data.tier_lines.get(key).filter(|v| !v.is_empty()) {
        Some(v) => v[mix(seed, 0x7e00 + floor as u32) as usize % v.len()].clone(),
        None if key.starts_with("up_") => "The crypt notices you and sharpens its tricks.".into(),
        None => "The crypt loosens its grip and lets you breathe.".into(),
    }
}

/// A tier change to announce (code decided `from`/`to`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TierChange {
    pub from: SkillTier,
    pub to: SkillTier,
    pub line: String,
    pub source: LineSource,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai_features::data::embedded;
    use crate::content::{parse, EMBEDDED_ENCOUNTERS};
    use crate::state;

    fn perf(safe: u32, mixed: u32, risky: u32, kr: u32, kw: u32, health: i32) -> Performance {
        Performance { safe, mixed, risky, keepers_right: kr, keepers_wrong: kw, health }
    }

    #[test]
    fn tier_function_table() {
        assert_eq!(target_tier(&Performance { health: 5, ..Default::default() }), SkillTier::Standard);
        assert_eq!(target_tier(&perf(1, 0, 0, 0, 0, 5)), SkillTier::Standard, "one decision isn't enough to judge");
        assert_eq!(target_tier(&perf(1, 0, 0, 1, 0, 5)), SkillTier::Sharp);
        assert_eq!(target_tier(&perf(2, 0, 0, 2, 0, 5)), SkillTier::Expert);
        assert_eq!(target_tier(&perf(2, 0, 0, 2, 0, 3)), SkillTier::Sharp, "expert needs health too");
        assert_eq!(target_tier(&perf(1, 1, 0, 1, 1, 5)), SkillTier::Standard);
        assert_eq!(target_tier(&perf(0, 0, 2, 1, 1, 5)), SkillTier::Gentle);
        assert_eq!(target_tier(&perf(3, 0, 0, 3, 0, 1)), SkillTier::Gentle, "low health eases off");
        assert_eq!(perf(1, 1, 0, 0, 0, 5).accuracy_pct(), 75);
    }

    #[test]
    fn tier_moves_one_step_and_is_deterministic() {
        let p = perf(4, 0, 0, 4, 0, 5);
        assert_eq!(next_tier(SkillTier::Gentle, &p), SkillTier::Standard);
        assert_eq!(next_tier(SkillTier::Standard, &p), SkillTier::Sharp);
        assert_eq!(next_tier(SkillTier::Sharp, &p), SkillTier::Expert);
        assert_eq!(next_tier(SkillTier::Expert, &p), SkillTier::Expert);
        let bad = perf(0, 0, 4, 0, 4, 2);
        assert_eq!(next_tier(SkillTier::Expert, &bad), SkillTier::Sharp);
        for a in [SkillTier::Gentle, SkillTier::Standard, SkillTier::Sharp, SkillTier::Expert] {
            for b in [SkillTier::Gentle, SkillTier::Standard, SkillTier::Sharp, SkillTier::Expert] {
                let one_step = (a as i32 - b as i32).abs() == 1;
                assert_eq!(change_key(a, b).is_some(), one_step, "{a:?}->{b:?}");
            }
        }
    }

    #[test]
    fn performance_comes_from_the_run() {
        let c = parse(EMBEDDED_ENCOUNTERS).unwrap().0;
        let n = c.encounters.len();
        let mut r = Run::new(state::floors_for(n), n);
        r.begin_encounter();
        let safe = c.encounters[0].choices.iter().position(|ch| ch.outcome == Outcome::Safe).unwrap();
        r.choose(&c, safe);
        r.reveal_lesson(&c);
        r.advance();
        r.begin_keeper();
        r.answer_keeper(false);
        let p = Performance::from_run(&r);
        assert_eq!((p.safe, p.keepers_wrong, p.decisions(), p.accuracy_pct()), (1, 1, 2, 50));
    }

    #[test]
    fn narration_direction_is_checked() {
        assert!(narration_ok("up_sharp", "The crypt notices your sharp eyes and sends sneakier tricks."));
        assert!(!narration_ok("up_sharp", "The crypt eases off for a while."));
        assert!(!narration_ok("down_gentle", "The crypt gets tougher now."));
        assert!(!narration_ok("down_gentle", "The crypt gives you 3 breaths."));
        let d = embedded().data;
        for k in crate::ai_features::data::TIER_KEYS {
            for l in &d.tier_lines[k] {
                assert!(narration_ok(k, l) && game_line(l, MAX_NARRATION_CHARS).is_some(), "{k}: {l}");
            }
        }
        let got = parse_run_pack("UP_SHARP: The crypt grows wary and sends sneakier tricks your way.\nDOWN_GENTLE: The crypt gets harder now, no mercy at all.\n");
        assert_eq!(got, vec![("up_sharp", "The crypt grows wary and sends sneakier tricks your way.".to_string())]);
    }

    #[test]
    fn tier_narration_never_names_a_payment_method() {
        assert!(!narration_ok("up_standard", "The crypt grows bolder: tricksters now want gift cards and crypto."));
        assert!(!narration_ok("down_gentle", "The crypt eases off; nobody asks for Venmo here."));
        assert!(narration_ok("up_standard", "The crypt grows bolder, and the tricksters talk faster now."));
    }
}
