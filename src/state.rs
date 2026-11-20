//! The deterministic game state machine. This file is the ONLY place game state
//! changes. Resources change through the integer `effects` in encounters.json,
//! a correct Floor Keeper answer (+1 Knowledge), and the real-time layer
//! (`hurt` / `heal`, one heart at a time). The AI never touches anything here;
//! it only supplies prose (dialogue, the AI extras' messages and study guide).
//!
//! Matches docs/FLOWCHART.md:
//!
//! ```text
//! Title ─start─► Explore ─talk to the floor's character─► Encounter ─pick─► Reaction ─line ready─► Lesson
//!   ▲            (crawl the floor:                          (scenario)         (AI line or           (warning signs,
//!   │             monsters, coffee)  ◄──────────────── back to the floor ──────── fallback)           effects applied)
//!   │               │
//!   │               └─ Floor Keeper at the stairs ─► KeeperQuiz ─answer─► KeeperVerdict
//!   │                                                                       │ correct: stairs open ──► Explore
//!   │                                                                       │ wrong:   BossFight (2 wardens) ──► Explore
//!   │               Explore + stairs open ─descend─► next floor's Explore, or EndWin after the last floor
//!   └──── play again ◄──── EndWin / EndLose (health = 0 at any point: lesson, monster, warden)
//! ```
//! `Title` is handled by main.rs (no Run exists yet); everything else lives in `Run`.

use crate::content::{Content, Effects, Outcome};

pub const MAX_HEALTH: i32 = 5;
pub const START_HEALTH: i32 = 5;
pub const MAX_FOCUS: i32 = 5;
pub const START_FOCUS: i32 = 3;
/// Wardens (bosses) the Floor Keeper sends after a wrong answer.
pub const WARDENS_PER_WRONG_ANSWER: u8 = 2;
/// Knowledge for answering the Floor Keeper correctly.
pub const KEEPER_REWARD: Effects = Effects { health: 0, focus: 0, knowledge: 1 };
/// Floors in a run. Content with more encounters than this gets one floor per
/// encounter; content with fewer reuses its encounters in order (floor 5 of a
/// 4-encounter file is encounter 1 again), so the run always has at least this
/// many floors. See `floors_for`.
pub const RUN_FLOORS: usize = 10;

/// Floors in a run for a content file with `encounters` encounters.
pub fn floors_for(encounters: usize) -> usize {
    RUN_FLOORS.max(encounters)
}

/// The encounter (index into encounters.json) that floor `floor` uses.
pub fn encounter_for_floor(floor: usize, encounters: usize) -> usize {
    floor % encounters.max(1)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// crawling the current floor: exploring, fighting monsters, finding the
    /// floor's character and the Floor Keeper
    Explore,
    /// reading the floor character's scenario and choosing
    Encounter,
    /// choice made; waiting for the AI line (or its fallback)
    Reaction,
    /// line shown + warning signs + safer next move; effects have been applied
    Lesson,
    /// the Floor Keeper's question at the stairs
    KeeperQuiz,
    /// the Keeper's reaction + the right answer; stairs open or wardens come next
    KeeperVerdict,
    /// wrong answer: beat `bosses_left` wardens to open the stairs
    BossFight,
    /// run is over; see `Run::ending`
    Ending,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stats {
    pub health: i32,
    pub focus: i32,
    pub knowledge: i32,
}

impl Stats {
    pub fn start() -> Stats {
        Stats { health: START_HEALTH, focus: START_FOCUS, knowledge: 0 }
    }
    /// Apply effects with clamping; returns the change that actually happened.
    pub fn apply(&mut self, e: Effects) -> Effects {
        let before = *self;
        // saturating: absurd values from a hand-edited encounters.json can't overflow
        self.health = self.health.saturating_add(e.health).clamp(0, MAX_HEALTH);
        self.focus = self.focus.saturating_add(e.focus).clamp(0, MAX_FOCUS);
        self.knowledge = self.knowledge.saturating_add(e.knowledge).max(0);
        Effects {
            health: self.health - before.health,
            focus: self.focus - before.focus,
            knowledge: self.knowledge - before.knowledge,
        }
    }
}

/// One choice the player made (kept for the ending recap).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Decision {
    /// the floor it was made on
    pub floor: usize,
    /// index into encounters.json (floors past the end of the file reuse encounters)
    pub encounter: usize,
    pub choice: usize,
    pub outcome: Outcome,
    /// what actually changed after clamping (None until the lesson is revealed)
    pub applied: Option<Effects>,
}

/// How a floor's Keeper went (kept for the ending recap).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeeperResult {
    pub floor: usize,
    pub correct: bool,
    pub wardens_beaten: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ending {
    /// EndWin, every choice safe and every Keeper answered correctly
    CryptKeeper,
    /// EndWin
    Survivor,
    /// EndLose: health reached 0
    Overwhelmed,
}

#[derive(Clone, Debug)]
pub struct Run {
    /// current floor (0-based); its encounter is `encounter()`
    pub room: usize,
    /// floors in this run (`floors_for`)
    pub rooms: usize,
    /// encounters in the content file
    pub encounters: usize,
    pub phase: Phase,
    pub stats: Stats,
    pub history: Vec<Decision>,
    pub keepers: Vec<KeeperResult>,
    /// this floor's character has been talked to (lesson done)
    pub met: bool,
    /// wardens still standing on this floor
    pub bosses_left: u8,
    /// the Keeper has stepped aside on this floor
    pub stairs_open: bool,
    /// Health from before the Keeper's quiz, while the arena lends full hearts
    pub arena_saved_health: Option<i32>,
    /// Keepers beaten in a Risk It bonus round
    pub keepers_defeated: u32,
}

impl Run {
    /// A run with `floors` floors over a content file of `encounters` encounters.
    pub fn new(floors: usize, encounters: usize) -> Run {
        Run {
            room: 0,
            rooms: floors.max(1),
            encounters: encounters.max(1),
            phase: Phase::Explore,
            stats: Stats::start(),
            history: Vec::new(),
            keepers: Vec::new(),
            met: false,
            bosses_left: 0,
            stairs_open: false,
            arena_saved_health: None,
            keepers_defeated: 0,
        }
    }

    /// Explore -> Encounter (player talked to the floor's character, once per floor).
    pub fn begin_encounter(&mut self) {
        if self.phase == Phase::Explore && !self.met {
            self.phase = Phase::Encounter;
        }
    }

    /// Encounter -> Reaction. Records the choice; does NOT change stats yet.
    pub fn choose(&mut self, content: &Content, choice: usize) -> Option<&Decision> {
        if self.phase != Phase::Encounter {
            return None;
        }
        let enc = self.encounter();
        let ch = content.encounters.get(enc)?.choices.get(choice)?;
        self.history.push(Decision { floor: self.room, encounter: enc, choice, outcome: ch.outcome, applied: None });
        self.phase = Phase::Reaction;
        self.history.last()
    }

    /// Reaction -> Lesson. Called once the AI line (or fallback) is on screen.
    /// Applies the chosen choice's effects exactly once.
    pub fn reveal_lesson(&mut self, content: &Content) -> Option<Effects> {
        if self.phase != Phase::Reaction {
            return None;
        }
        let d = self.history.last_mut()?;
        let eff = content.encounters.get(d.encounter)?.choices.get(d.choice)?.effects;
        let applied = self.stats.apply(eff);
        d.applied = Some(applied);
        self.phase = Phase::Lesson;
        Some(applied)
    }

    /// Lesson -> back to the floor (Explore), or Ending if health hit 0.
    pub fn advance(&mut self) {
        if self.phase != Phase::Lesson {
            return;
        }
        if self.is_last_lesson() {
            self.phase = Phase::Ending;
        } else {
            self.met = true;
            self.phase = Phase::Explore;
        }
    }

    /// True when the Floor Keeper will take a question (character met, not yet answered).
    pub fn keeper_ready(&self) -> bool {
        self.phase == Phase::Explore && self.met && !self.stairs_open && self.bosses_left == 0 && !self.keeper_answered()
    }

    fn keeper_answered(&self) -> bool {
        self.keepers.last().is_some_and(|k| k.floor == self.room)
    }

    /// Explore -> KeeperQuiz. Returns false if the Keeper isn't ready.
    pub fn begin_keeper(&mut self) -> bool {
        if !self.keeper_ready() {
            return false;
        }
        self.phase = Phase::KeeperQuiz;
        true
    }

    /// KeeperQuiz -> KeeperVerdict. A correct answer earns `KEEPER_REWARD`;
    /// a wrong one costs nothing yet (the wardens do the rest).
    pub fn answer_keeper(&mut self, correct: bool) -> Option<Effects> {
        if self.phase != Phase::KeeperQuiz {
            return None;
        }
        self.keepers.push(KeeperResult { floor: self.room, correct, wardens_beaten: 0 });
        self.phase = Phase::KeeperVerdict;
        Some(if correct { self.stats.apply(KEEPER_REWARD) } else { Effects::default() })
    }

    /// A Risk It round's result, only after a correct answer (still on the
    /// verdict): a win is +1 Health (+1 Focus when Health is full), a knockout
    /// is -1 Health but never below 1, time up changes nothing.
    pub fn risk_it(&mut self, o: crate::risk_it::Outcome) -> Option<Effects> {
        use crate::risk_it::Outcome;
        let correct = self.keepers.last().is_some_and(|k| k.correct);
        if self.phase != Phase::KeeperVerdict || !correct {
            return None;
        }
        let e = match o {
            Outcome::Defeated => {
                self.keepers_defeated += 1;
                if self.stats.health < MAX_HEALTH {
                    Effects { health: 1, ..Effects::default() }
                } else {
                    Effects { focus: 1, ..Effects::default() }
                }
            }
            Outcome::KnockedOut if self.stats.health > 1 => Effects { health: -1, ..Effects::default() },
            _ => Effects::default(),
        };
        Some(self.stats.apply(e))
    }

    /// KeeperVerdict -> Explore with the stairs open (correct), or BossFight (wrong).
    pub fn leave_verdict(&mut self) {
        if self.phase != Phase::KeeperVerdict {
            return;
        }
        let correct = self.keepers.last().is_some_and(|k| k.correct);
        if correct {
            self.stairs_open = true;
            self.phase = Phase::Explore;
        } else {
            // arena-only hearts: fight at full health, then get your own Health back
            // afterwards, so a wrong answer is never a way to heal
            self.arena_saved_health = Some(self.stats.health);
            self.stats.health = MAX_HEALTH;
            self.bosses_left = WARDENS_PER_WRONG_ANSWER;
            self.phase = Phase::BossFight;
        }
    }

    /// One warden down. The last one opens the stairs and returns to the floor.
    pub fn boss_defeated(&mut self) {
        if self.phase != Phase::BossFight || self.bosses_left == 0 {
            return;
        }
        self.bosses_left -= 1;
        if let Some(k) = self.keepers.last_mut() {
            k.wardens_beaten += 1;
        }
        if self.bosses_left == 0 {
            if let Some(h) = self.arena_saved_health.take() {
                self.stats.health = h;
            }
            self.stairs_open = true;
            self.phase = Phase::Explore;
        }
    }

    /// Real-time damage (monster, projectile, warden). Only while crawling or
    /// fighting; health 0 ends the run. Returns the change that happened.
    pub fn hurt(&mut self, hearts: i32) -> Effects {
        if !matches!(self.phase, Phase::Explore | Phase::BossFight) {
            return Effects::default();
        }
        let applied = self.stats.apply(Effects { health: -hearts.max(0), ..Effects::default() });
        if self.stats.health <= 0 {
            self.phase = Phase::Ending;
        }
        applied
    }

    /// A coffee pickup. Only while crawling or fighting.
    pub fn heal(&mut self, hearts: i32) -> Effects {
        if !matches!(self.phase, Phase::Explore | Phase::BossFight) {
            return Effects::default();
        }
        self.stats.apply(Effects { health: hearts.max(0), ..Effects::default() })
    }

    /// Explore (stairs open) -> next floor's Explore, or Ending after the last floor.
    pub fn descend(&mut self) {
        if self.phase != Phase::Explore || !self.stairs_open {
            return;
        }
        if self.room + 1 >= self.rooms {
            self.phase = Phase::Ending;
        } else {
            self.room += 1;
            self.met = false;
            self.stairs_open = false;
            self.bosses_left = 0;
        }
    }

    /// CC_FLOOR (automation only): begin the run on a later floor.
    pub fn debug_start_floor(&mut self, floor: usize) {
        if self.phase == Phase::Explore && self.history.is_empty() {
            self.room = floor.min(self.rooms - 1);
        }
    }

    /// The encounter this floor uses (index into encounters.json).
    pub fn encounter(&self) -> usize {
        encounter_for_floor(self.room, self.encounters)
    }

    pub fn is_last_floor(&self) -> bool {
        self.room + 1 >= self.rooms
    }

    pub fn ending(&self) -> Ending {
        if self.stats.health <= 0 {
            Ending::Overwhelmed
        } else if self.history.iter().all(|d| d.outcome == Outcome::Safe) && self.keepers.iter().all(|k| k.correct) {
            Ending::CryptKeeper
        } else {
            Ending::Survivor
        }
    }

    /// True when the next `advance` from the Lesson ends the run (health is gone).
    pub fn is_last_lesson(&self) -> bool {
        self.stats.health <= 0
    }

    pub fn safe_choices(&self) -> usize {
        self.history.iter().filter(|d| d.outcome == Outcome::Safe).count()
    }

    pub fn keepers_correct(&self) -> usize {
        self.keepers.iter().filter(|k| k.correct).count()
    }

    /// The choice made in this floor's encounter (what the Keeper asks about).
    pub fn choice_on_floor(&self) -> Option<usize> {
        self.history.iter().rev().find(|d| d.floor == self.room).map(|d| d.choice)
    }

    pub fn wardens_beaten(&self) -> usize {
        self.keepers.iter().map(|k| k.wardens_beaten as usize).sum()
    }

    /// The ending screen's Keeper tally, out of the Keepers actually reached: "answered 3/10"
    /// on a full run, "answered 1 of 2 reached (floor 3/10)" after an early death, and just
    /// "Reached floor 1/10" when no Keeper was reached (never a misleading "0/10").
    pub fn keeper_tally(&self) -> String {
        let (kc, reached, floor, rooms) = (self.keepers_correct(), self.keepers.len(), self.room + 1, self.rooms);
        if reached == 0 {
            format!("Reached floor {floor}/{rooms}")
        } else if reached >= rooms {
            format!("Floor Keepers answered {kc}/{rooms}")
        } else {
            format!("Floor Keepers answered {kc} of {reached} reached (floor {floor}/{rooms})")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::{parse, EMBEDDED_ENCOUNTERS};

    fn content() -> Content {
        parse(EMBEDDED_ENCOUNTERS).unwrap().0
    }

    fn new_run(c: &Content) -> Run {
        Run::new(floors_for(c.encounters.len()), c.encounters.len())
    }

    /// The encounter shown on `floor`.
    fn enc(c: &Content, floor: usize) -> &crate::content::Encounter {
        &c.encounters[encounter_for_floor(floor, c.encounters.len())]
    }

    fn floors(c: &Content) -> usize {
        floors_for(c.encounters.len())
    }

    fn pick(c: &Content, room: usize, o: Outcome) -> usize {
        enc(c, room).choices.iter().position(|ch| ch.outcome == o).unwrap()
    }

    /// After a floor's lesson: answer the Keeper correctly and take the stairs.
    fn clear_floor(r: &mut Run) {
        if r.phase != Phase::Explore {
            return;
        }
        assert!(r.begin_keeper());
        r.answer_keeper(true).unwrap();
        r.leave_verdict();
        assert!(r.stairs_open);
        r.descend();
    }

    #[test]
    fn full_safe_run_reaches_crypt_keeper() {
        let c = content();
        let mut r = new_run(&c);
        for room in 0..floors(&c) {
            assert_eq!(r.phase, Phase::Explore);
            r.begin_encounter();
            r.choose(&c, pick(&c, room, Outcome::Safe)).unwrap();
            assert_eq!(r.phase, Phase::Reaction);
            r.reveal_lesson(&c).unwrap();
            assert_eq!(r.phase, Phase::Lesson);
            r.advance();
            clear_floor(&mut r);
        }
        assert_eq!(r.phase, Phase::Ending);
        assert_eq!(r.ending(), Ending::CryptKeeper);
        assert!(r.stats.knowledge >= floors(&c) as i32);
    }

    #[test]
    fn effects_apply_only_at_lesson_and_only_once() {
        let c = content();
        let mut r = new_run(&c);
        r.begin_encounter();
        r.choose(&c, pick(&c, 0, Outcome::Risky)).unwrap();
        assert_eq!(r.stats, Stats::start(), "no change while the AI line is pending");
        let eff = c.encounters[0].choices[pick(&c, 0, Outcome::Risky)].effects;
        r.reveal_lesson(&c).unwrap();
        assert_eq!(r.stats.health, START_HEALTH + eff.health);
        assert!(r.reveal_lesson(&c).is_none());
        assert_eq!(r.stats.health, START_HEALTH + eff.health);
    }

    #[test]
    fn effects_clamp_and_report_actual_change() {
        let mut s = Stats::start();
        let got = s.apply(Effects { health: 5, focus: -99, knowledge: -3 });
        assert_eq!(s, Stats { health: MAX_HEALTH, focus: 0, knowledge: 0 });
        assert_eq!(got, Effects { health: 0, focus: -START_FOCUS, knowledge: 0 });
    }

    #[test]
    fn zero_health_ends_run_early() {
        let c = content();
        let mut r = new_run(&c);
        r.stats.health = 1;
        r.begin_encounter();
        r.choose(&c, pick(&c, 0, Outcome::Risky)).unwrap();
        r.reveal_lesson(&c);
        r.advance();
        assert_eq!(r.phase, Phase::Ending);
        assert_eq!(r.ending(), Ending::Overwhelmed);
    }

    #[test]
    fn worst_case_run_is_still_reachable() {
        // always pick the most damaging choice: the run must end in EndLose
        // (keeps the lose branch of docs/FLOWCHART.md reachable with the shipped content)
        let c = content();
        let mut r = new_run(&c);
        for _ in 0..floors(&c) {
            if r.phase == Phase::Ending {
                break;
            }
            r.begin_encounter();
            let e = enc(&c, r.room);
            let worst = (0..e.choices.len())
                .min_by_key(|&i| e.choices[i].effects.health)
                .unwrap();
            r.choose(&c, worst).unwrap();
            r.reveal_lesson(&c);
            r.advance();
            clear_floor(&mut r);
        }
        assert_eq!(r.phase, Phase::Ending);
        assert_eq!(r.ending(), Ending::Overwhelmed);
    }

    #[test]
    fn choose_is_ignored_outside_encounter_and_bad_index() {
        let c = content();
        let mut r = new_run(&c);
        assert!(r.choose(&c, 0).is_none()); // still exploring
        r.begin_encounter();
        assert!(r.choose(&c, 99).is_none());
        assert_eq!(r.phase, Phase::Encounter);
        assert_eq!(r.stats, Stats::start());
    }

    /// Play a whole run picking `picks[room]`; returns the finished run.
    fn play(c: &Content, picks: &[usize]) -> Run {
        let mut r = new_run(c);
        for &p in picks {
            if r.phase == Phase::Ending {
                break;
            }
            assert_eq!(r.phase, Phase::Explore);
            r.begin_encounter();
            r.begin_encounter(); // double input is harmless
            r.choose(c, p).unwrap();
            assert!(r.choose(c, p).is_none(), "second pick in the same room is ignored");
            r.reveal_lesson(c).unwrap();
            r.advance();
            r.advance(); // double advance can't skip a floor
            clear_floor(&mut r);
        }
        r
    }

    /// Every combination of choices across all rooms.
    fn all_paths(c: &Content) -> Vec<Vec<usize>> {
        let mut paths = vec![vec![]];
        for e in (0..floors(c)).map(|f| enc(c, f)) {
            paths = paths
                .into_iter()
                .flat_map(|p| (0..e.choices.len()).map(move |i| [p.clone(), vec![i]].concat()))
                .collect();
        }
        paths
    }

    #[test]
    fn every_path_terminates_and_all_three_endings_are_reachable() {
        let c = content();
        let paths = all_paths(&c);
        assert_eq!(paths.len(), (0..floors(&c)).map(|f| enc(&c, f).choices.len()).product::<usize>());
        let (mut keeper, mut survivor, mut lose) = (0, 0, 0);
        for p in &paths {
            let r = play(&c, p);
            assert_eq!(r.phase, Phase::Ending, "path {p:?} must finish");
            assert!(r.history.len() <= floors(&c));
            assert!((0..=MAX_HEALTH).contains(&r.stats.health) && (0..=MAX_FOCUS).contains(&r.stats.focus));
            assert!(r.stats.knowledge >= 0);
            match r.ending() {
                Ending::CryptKeeper => keeper += 1,
                Ending::Survivor => survivor += 1,
                Ending::Overwhelmed => {
                    lose += 1;
                    assert_eq!(r.stats.health, 0);
                }
            }
            // win endings only when every room was completed
            if r.ending() != Ending::Overwhelmed {
                assert_eq!(r.history.len(), floors(&c), "{p:?}");
            }
        }
        assert!(keeper > 0 && survivor > 0 && lose > 0, "keeper {keeper}, survivor {survivor}, lose {lose}");
    }

    #[test]
    fn mixed_choice_run_ends_as_survivor() {
        let c = content();
        let picks: Vec<usize> = (0..floors(&c))
            .map(|room| {
                enc(&c, room).choices.iter().position(|ch| ch.outcome == Outcome::Mixed).unwrap_or(pick(&c, room, Outcome::Safe))
            })
            .collect();
        assert!(picks.iter().enumerate().any(|(room, &i)| enc(&c, room).choices[i].outcome == Outcome::Mixed));
        let r = play(&c, &picks);
        assert_eq!(r.ending(), Ending::Survivor);
        assert!(r.stats.health > 0);
    }

    #[test]
    fn every_choice_applies_exactly_its_effects_from_a_fresh_run() {
        let c = content();
        for (room, e) in c.encounters.iter().enumerate() {
            for (i, ch) in e.choices.iter().enumerate() {
                let mut r = new_run(&c);
                r.room = room;
                r.begin_encounter();
                r.choose(&c, i).unwrap();
                let applied = r.reveal_lesson(&c).unwrap();
                let mut expect = Stats::start();
                expect.apply(ch.effects);
                assert_eq!(r.stats, expect, "{}/{}", e.id, ch.id);
                assert_eq!(r.history.last().unwrap().applied, Some(applied));
                // shipped content: learning always happens, and outcomes match their sign
                assert!(ch.effects.knowledge > 0, "{}/{} should teach something", e.id, ch.id);
                match ch.outcome {
                    Outcome::Safe => assert!(ch.effects.health >= 0 && ch.effects.focus >= 0, "{}/{}", e.id, ch.id),
                    Outcome::Risky => assert!(ch.effects.health < 0, "{}/{}", e.id, ch.id),
                    Outcome::Mixed => assert!(ch.effects.health >= 0, "{}/{}", e.id, ch.id),
                }
            }
        }
    }

    #[test]
    fn extreme_effects_saturate_instead_of_overflowing() {
        let mut s = Stats::start();
        s.apply(Effects { health: i32::MAX, focus: i32::MIN, knowledge: i32::MAX });
        assert_eq!((s.health, s.focus, s.knowledge), (MAX_HEALTH, 0, i32::MAX));
        s.apply(Effects { health: i32::MIN, focus: i32::MAX, knowledge: 1 });
        assert_eq!((s.health, s.focus, s.knowledge), (0, MAX_FOCUS, i32::MAX));
    }

    #[test]
    fn lose_at_zero_health_mid_run_skips_remaining_rooms() {
        let c = content();
        let mut r = new_run(&c);
        r.stats.health = 2;
        r.begin_encounter();
        r.choose(&c, pick(&c, 0, Outcome::Risky)).unwrap();
        r.reveal_lesson(&c);
        assert!(r.is_last_lesson());
        r.advance();
        assert_eq!((r.phase, r.room, r.ending()), (Phase::Ending, 0, Ending::Overwhelmed));
        // nothing moves once ended
        r.begin_encounter();
        assert!(r.choose(&c, 0).is_none() && r.reveal_lesson(&c).is_none());
        r.advance();
        assert_eq!(r.phase, Phase::Ending);
    }

    #[test]
    fn reveal_and_advance_ignored_out_of_order() {
        let c = content();
        let mut r = new_run(&c);
        assert!(r.reveal_lesson(&c).is_none());
        r.advance();
        assert_eq!((r.phase, r.room), (Phase::Explore, 0));
        r.begin_encounter();
        r.advance();
        assert_eq!(r.phase, Phase::Encounter);
    }

    #[test]
    fn zero_rooms_still_makes_a_valid_run() {
        assert_eq!((Run::new(0, 0).rooms, Run::new(0, 0).encounters), (1, 1));
    }

    /// Talk to floor 0's character (safe pick) and come back to the floor.
    fn meet(c: &Content, r: &mut Run) {
        r.begin_encounter();
        r.choose(c, pick(c, r.room, Outcome::Safe)).unwrap();
        r.reveal_lesson(c).unwrap();
        r.advance();
        assert_eq!(r.phase, Phase::Explore);
        assert!(r.met);
    }

    #[test]
    fn keeper_waits_until_the_floor_character_is_met() {
        let c = content();
        let mut r = new_run(&c);
        assert!(!r.begin_keeper(), "Keeper won't quiz before the lesson");
        r.descend();
        assert_eq!((r.room, r.phase), (0, Phase::Explore), "stairs stay shut");
        meet(&c, &mut r);
        r.begin_encounter();
        assert_eq!(r.phase, Phase::Explore, "the character only talks once per floor");
        assert!(r.begin_keeper());
        assert_eq!(r.phase, Phase::KeeperQuiz);
    }

    #[test]
    fn correct_keeper_answer_opens_the_stairs() {
        let c = content();
        let mut r = new_run(&c);
        meet(&c, &mut r);
        let kn = r.stats.knowledge;
        assert!(r.begin_keeper());
        assert_eq!(r.answer_keeper(true), Some(KEEPER_REWARD));
        assert!(r.answer_keeper(false).is_none(), "one answer per floor");
        assert_eq!((r.phase, r.stats.knowledge), (Phase::KeeperVerdict, kn + 1));
        r.leave_verdict();
        assert_eq!((r.phase, r.stairs_open, r.bosses_left), (Phase::Explore, true, 0));
        assert!(!r.begin_keeper(), "no second quiz");
        r.descend();
        assert_eq!((r.phase, r.room, r.met, r.stairs_open), (Phase::Explore, 1, false, false));
    }

    #[test]
    fn wrong_keeper_answer_needs_two_wardens_before_the_stairs() {
        let c = content();
        let mut r = new_run(&c);
        meet(&c, &mut r);
        r.hurt(MAX_HEALTH - 1); // limp up to the Keeper on 1 heart (or less after the lesson)
        let before = r.stats;
        assert!(r.begin_keeper());
        assert_eq!(r.answer_keeper(false), Some(Effects::default()));
        assert_eq!(r.stats, before, "a wrong answer costs nothing by itself");
        r.leave_verdict();
        assert_eq!((r.phase, r.bosses_left), (Phase::BossFight, WARDENS_PER_WRONG_ANSWER));
        assert_eq!(r.stats.health, MAX_HEALTH, "the arena lends full hearts");
        r.descend();
        assert_eq!((r.room, r.phase), (0, Phase::BossFight), "can't leave mid-fight");
        r.boss_defeated();
        assert_eq!((r.phase, r.bosses_left, r.stairs_open), (Phase::BossFight, 1, false));
        r.boss_defeated();
        assert_eq!((r.phase, r.bosses_left, r.stairs_open), (Phase::Explore, 0, true));
        assert_eq!(r.stats.health, before.health, "your own Health comes back after the arena");
        r.boss_defeated(); // extra calls are ignored
        assert_eq!(r.wardens_beaten(), 2);
        assert!(!r.begin_keeper());
        r.descend();
        assert_eq!(r.room, 1);
        assert_eq!(r.keepers, vec![KeeperResult { floor: 0, correct: false, wardens_beaten: 2 }]);
    }

    /// Answering wrong on purpose must never heal: whatever happens in the arena
    /// (hits taken, coffee drunk), you leave it with at most your pre-quiz Health.
    #[test]
    fn a_wrong_answer_never_leaves_you_healthier() {
        let c = content();
        for start in 1..=MAX_HEALTH {
            for (hits, coffees) in [(0, 0), (2, 0), (2, 1), (0, 3), (4, 4)] {
                let mut r = new_run(&c);
                meet(&c, &mut r);
                r.hurt(MAX_HEALTH - start);
                let before = r.stats.health;
                r.begin_keeper();
                r.answer_keeper(false);
                r.leave_verdict();
                r.hurt(hits);
                r.boss_defeated();
                r.heal(coffees);
                r.boss_defeated();
                assert_eq!(r.phase, Phase::Explore);
                assert!(r.stats.health <= before, "start {start}, hits {hits}, coffees {coffees}: {} > {before}", r.stats.health);
                assert_eq!(r.stats.health, before);
                assert!(r.arena_saved_health.is_none());
            }
        }
    }

    #[test]
    fn dying_to_a_warden_ends_the_run() {
        let c = content();
        let mut r = new_run(&c);
        meet(&c, &mut r);
        r.begin_keeper();
        r.answer_keeper(false);
        r.leave_verdict();
        r.boss_defeated();
        while r.phase == Phase::BossFight {
            r.hurt(1);
        }
        assert_eq!((r.phase, r.stats.health, r.ending()), (Phase::Ending, 0, Ending::Overwhelmed));
        r.boss_defeated();
        r.descend();
        assert_eq!(r.phase, Phase::Ending);
    }

    #[test]
    fn hurt_and_heal_only_in_real_time_phases() {
        let c = content();
        let mut r = new_run(&c);
        assert_eq!(r.hurt(1).health, -1);
        assert_eq!(r.heal(3).health, 1, "heal clamps at max health");
        r.begin_encounter();
        assert_eq!(r.hurt(1), Effects::default(), "no damage while reading a scenario");
        assert_eq!(r.stats.health, MAX_HEALTH);
    }

    #[test]
    fn wrong_keeper_answer_blocks_crypt_keeper_ending() {
        let c = content();
        let mut r = new_run(&c);
        for room in 0..floors(&c) {
            assert_eq!(r.room, room);
            meet(&c, &mut r);
            r.begin_keeper();
            r.answer_keeper(room != 2);
            r.leave_verdict();
            while r.phase == Phase::BossFight {
                r.boss_defeated();
            }
            r.descend();
        }
        assert_eq!(r.phase, Phase::Ending);
        assert_eq!(r.ending(), Ending::Survivor);
        assert_eq!((r.keepers_correct(), r.wardens_beaten()), (floors(&c) - 1, 2));
    }

    #[test]
    fn a_run_has_ten_floors_whatever_the_content_length() {
        assert_eq!(RUN_FLOORS, 10);
        let c = content();
        assert_eq!(new_run(&c).rooms, 10);
        assert_eq!(floors_for(1), 10);
        assert_eq!(floors_for(4), 10);
        assert_eq!(floors_for(10), 10);
        assert_eq!(floors_for(12), 12, "more authored encounters than floors: one floor each");
        // floors past the end of the file reuse encounters in order
        assert_eq!((0..10).map(|f| encounter_for_floor(f, 4)).collect::<Vec<_>>(), vec![0, 1, 2, 3, 0, 1, 2, 3, 0, 1]);
        assert_eq!((0..10).map(|f| encounter_for_floor(f, 10)).collect::<Vec<_>>(), (0..10).collect::<Vec<_>>());
        assert_eq!(encounter_for_floor(3, 0), 0, "empty content can't divide by zero");
        let mut r = Run::new(floors_for(3), 3);
        r.room = 7;
        assert_eq!(r.encounter(), 1);
    }

    /// The stairs only open once the Keeper is cleared (right answer, or both
    /// wardens beaten), and floor 10's stairs end the run instead of descending.
    #[test]
    fn you_reach_the_next_floor_only_after_the_keeper_and_floor_ten_ends_the_run() {
        let c = content();
        for wrong_on in [None, Some(0usize), Some(6), Some(9)] {
            let mut r = new_run(&c);
            for floor in 0..floors(&c) {
                assert_eq!((r.phase, r.room), (Phase::Explore, floor));
                // no descending before the encounter, or before the Keeper
                r.descend();
                assert_eq!(r.room, floor, "stairs are shut before the encounter");
                assert!(!r.begin_keeper(), "the Keeper waits for the floor's lesson");
                meet(&c, &mut r);
                assert_eq!(r.history.last().unwrap().floor, floor);
                assert_eq!(r.history.last().unwrap().encounter, encounter_for_floor(floor, c.encounters.len()));
                r.descend();
                assert_eq!((r.room, r.stairs_open), (floor, false), "stairs are shut until the Keeper is cleared");
                assert!(r.begin_keeper());
                let right = wrong_on != Some(floor);
                r.answer_keeper(right);
                r.descend();
                assert_eq!(r.room, floor, "no descending mid-verdict");
                r.leave_verdict();
                if !right {
                    assert_eq!((r.phase, r.bosses_left), (Phase::BossFight, WARDENS_PER_WRONG_ANSWER));
                    r.boss_defeated();
                    r.descend();
                    assert_eq!(r.room, floor, "one warden isn't enough");
                    r.boss_defeated();
                }
                assert!(r.stairs_open);
                assert_eq!(r.is_last_floor(), floor == floors(&c) - 1);
                r.descend();
                if floor + 1 < floors(&c) {
                    assert_eq!((r.phase, r.room, r.stairs_open, r.met), (Phase::Explore, floor + 1, false, false));
                }
            }
            assert_eq!(r.phase, Phase::Ending, "floor 10's exit gives the ending");
            assert_eq!(r.room, floors(&c) - 1);
            assert_eq!(r.keepers.len(), 10);
            assert_eq!(r.keepers_correct(), if wrong_on.is_some() { 9 } else { 10 });
            assert!(matches!(r.ending(), Ending::CryptKeeper | Ending::Survivor));
            if wrong_on.is_some() {
                assert_eq!(r.ending(), Ending::Survivor, "a missed Keeper blocks the best ending");
            }
        }
    }

    #[test]
    fn keeper_tally_counts_only_keepers_reached() {
        let c = content();
        let mut r = new_run(&c);
        assert_eq!(r.keeper_tally(), "Reached floor 1/10", "an early death never shows 0/10");
        r.keepers.push(KeeperResult { floor: 0, correct: true, wardens_beaten: 0 });
        r.keepers.push(KeeperResult { floor: 1, correct: false, wardens_beaten: 2 });
        r.room = 2;
        assert_eq!(r.keeper_tally(), "Floor Keepers answered 1 of 2 reached (floor 3/10)");
        for f in 2..10 {
            r.keepers.push(KeeperResult { floor: f, correct: true, wardens_beaten: 0 });
        }
        r.room = 9;
        assert_eq!(r.keeper_tally(), "Floor Keepers answered 9/10");
    }

    #[test]
    fn risk_it_rewards_and_never_kills() {
        use crate::risk_it::Outcome;
        let at_verdict = |health: i32, correct: bool| {
            let mut r = Run::new(10, 10);
            r.stats.health = health;
            r.phase = Phase::KeeperQuiz;
            r.answer_keeper(correct).unwrap();
            r
        };
        let mut r = at_verdict(3, true);
        r.risk_it(Outcome::Defeated).unwrap();
        assert_eq!((r.stats.health, r.keepers_defeated), (4, 1));
        let mut r = at_verdict(MAX_HEALTH, true);
        let focus = r.stats.focus;
        r.risk_it(Outcome::Defeated).unwrap();
        assert_eq!((r.stats.health, r.stats.focus), (MAX_HEALTH, (focus + 1).min(MAX_FOCUS)));
        let mut r = at_verdict(3, true);
        r.risk_it(Outcome::KnockedOut).unwrap();
        assert_eq!(r.stats.health, 2);
        let mut r = at_verdict(1, true);
        r.risk_it(Outcome::KnockedOut).unwrap();
        assert_eq!(r.stats.health, 1, "a knockout never ends the run");
        let mut r = at_verdict(3, true);
        r.risk_it(Outcome::TimeUp).unwrap();
        assert_eq!((r.stats.health, r.keepers_defeated), (3, 0));
        // only after a right answer, and the stairs still open
        let mut r = at_verdict(3, false);
        assert!(r.risk_it(Outcome::Defeated).is_none());
        let mut r = at_verdict(3, true);
        r.risk_it(Outcome::KnockedOut);
        r.leave_verdict();
        assert!(r.stairs_open && r.phase == Phase::Explore);
    }

}
