# Campus Crypt — Game state machine

The AI only writes prose: the floor character's reaction to your choice, and the AI extras (the Floor Keeper's taunt and verdict line, Hoot's hint, the suspicious messages, the difficulty narration, and the post-run study guide). It never chooses an outcome, never applies resource changes, never grades the Floor Keeper's quiz or a side game, never picks the difficulty tier, and never decides whether the player wins or loses. Scoring, health, focus, knowledge, warning signs, safer next moves, the Keeper's question, answer and verdict, which message is a scam and which red flags it carries, the skill tier, the mistake log, and progression are deterministic and come from the content files and game code. Every AI text has a written fallback: if the API is off, slow, over its call budget, or writes something the filters reject (a Keeper line that disagrees with the verdict, a hint that names a choice, a link, ...), the engine shows the written text instead and continues the same loop. Nothing ever waits on the network.

```mermaid
stateDiagram-v2
    [*] --> Title

    Title --> Explore: start game (floor 1 of 10)

    state "Explore (real-time floor)" as Explore
    note right of Explore
        walk, fight monsters, drink coffee (+1 health);
        a monster or projectile hit costs 1 health.
        Side games (main.rs overlay, floor paused, Run stays in Explore):
        floors 2/5/8 a buzzing phone (report or trust a generated scam),
        floors 3/6/9 a notice board (spot the fake of two messages).
        Code labels the answer; it's logged for the debrief, no stat change.
    end note

    Explore --> Encounter: talk to the floor's character (once per floor)
    note right of Encounter
        Hoot (owl hint, H toggles): after ~12 s without input
        (8-20 s by skill tier), one nudge at a warning sign, once per floor
    end note
    Encounter --> ChoiceMade: player picks a choice
    ChoiceMade --> AIReaction: request an in-character line

    state AIReaction {
        [*] --> ApiCall
        ApiCall --> ApiOk: response received
        ApiCall --> ApiFail: timeout or error
        ApiOk --> [*]
        ApiFail --> UseFallback: use fallback_reaction
        UseFallback --> [*]
    }

    AIReaction --> Lesson: show warning signs and safer next move;\napply deterministic effects
    Lesson --> Explore: back to the floor (health above 0)

    Explore --> KeeperQuiz: talk to the Floor Keeper at the stairs\n(only after the floor's lesson)
    KeeperQuiz --> KeeperVerdict: answer (code checks it; the Keeper's line is AI-voiced\nonly if it agrees with the verdict, else scripted)\nfloor 1: the Inbox Mimic fight (SPARE when spareable or\nDELETE after 3 tells = right; 3 bad acts or DELETE unread = wrong)
    KeeperQuiz --> KeeperQuiz: floor 1 box hearts run out\n(fight restarts, no verdict, Health untouched)
    KeeperVerdict --> StairsOpen: correct (+1 knowledge)
    KeeperVerdict --> BossFight: wrong (full arena-only hearts)

    state "BossFight (2 wardens)" as BossFight
    BossFight --> StairsOpen: both wardens beaten (pre-quiz health restored)

    state "Explore, stairs open" as StairsOpen
    StairsOpen --> Explore: take the stairs (floors 1-9: next floor;\ncode recomputes the skill tier, a change is narrated on the floor banner)
    StairsOpen --> EndWin: take the stairs on floor 10

    Lesson --> EndLose: health reaches 0
    Explore --> EndLose: health reaches 0
    BossFight --> EndLose: health reaches 0

    EndWin --> Title: play again
    EndLose --> Title: play again
    note right of EndWin
        ending screen tabs: Hoot's study guide (debrief from the
        mistake log; AI within ~8.5 s or templated) and field notes
    end note
```

**Floors.** A run has **10 floors** (`state::RUN_FLOORS`). Floor *n* uses encounter *n* of `encounters.json`, in order; the shipped file has 10, one per floor. A file with more than 10 encounters gets one floor per encounter; a file with fewer reuses its encounters in order (floor 5 of a 4-encounter file is encounter 1 again), so the run always works while content is being written. The difficulty curve (monster count and health, map size, warden health and which 2 wardens) runs from the old floor 1 to the old floor 4 across the 10 floors; see README "Difficulty". Each floor is a generated dungeon (rooms, corridors, walls that block line of sight) with monsters, coffee, the floor's character in a room part-way through, and the Floor Keeper standing on the stairs.

**The Floor Keeper.** It won't take a question until you've talked to the floor's character. It asks about the choice you made there, using that choice's `keeper` block in `encounters.json` (question, right answer, two clearly unsafe wrong answers; see README "Floor Keeper questions"). The answer screen ignores input for 0.4 s and until swing/confirm keys are released, and Space never answers. Code decides the verdict (`Quiz::is_correct`). Its taunt and verdict line may be AI-voiced, but the verdict line is used only when it agrees with the verdict; otherwise (and whenever the AI is off) the written line is shown. A correct answer gives +1 Knowledge and the Keeper steps aside. A wrong answer sends you to its **2 wardens** (bosses, one after the other, a coffee between them) in an arena with full **arena-only hearts**: beat both and the stairs open and your Health goes back to what it was before the quiz (never more, so answering wrong is no way to heal). Losing every arena heart is still game over. Either way you then see the right answer and a warning sign.

**AI extras** (`src/ai_features/`, wired in `main.rs`; see README "AI extras"). They sit on top of the same loop and never change `Run`: the **Keeper's taunt** when the quiz opens and its **verdict line** (checked against the verdict by `banter::verdict_consistent`); **Hoot's hint** on the encounter screen; the **suspicious message** (floors 2, 5, 8: AI-prefetched scam slots 0-2) and **spot-the-fake** (floors 3, 6, 9: scripted slots, skipping any template the phones already used, so no message repeats in a run) side games; the **skill tier** (Gentle / Standard / Sharp / Expert, recomputed by code on every descent from your choices, Keeper answers and health, at most one step per floor), which paces the next floor (monster speed and count; warden health only downward) and is narrated on the floor banner when it changes; and the **study guide** on the ending screen.

**Floor 1's Inbox Mimic fight.** On floor 1 only, the KeeperQuiz phase is an Undertale-style fight (`src/keeper_fight.rs`, content from the Inbox Mimic's `keeper_box` block). Player turn: ACT / INSPECT / SPARE / DELETE, behind the same 0.4 s lock. Mimic turn: a 6 s dodge in the box; its pattern and hazards depend only on seed, turn and escalation (bad acts), never on the right answer. 3 distinct tells make it spareable; SPARE then (or DELETE after 3 tells) answers right; 3 bad acts or DELETE with no tells answers wrong. Early SPARE or DELETE with 1-2 tells just get a taunt. Box hearts only restart the fight. Every verdict goes through the same `answer_keeper`, so right and wrong lead to the same states as above. Floors 2-10 keep the quiz.

**Win** when you take the stairs out of floor 10 with health above 0. You only ever reach the next floor through open stairs, and the stairs only open once that floor's Keeper is cleared (a right answer, or both wardens beaten). **Lose** when health hits 0 at any point: a lesson's effects, a monster, a projectile, or a warden. Either ending returns to the title screen. **Crypt Keeper** (the best ending) needs every choice safe and every Keeper answered correctly.
