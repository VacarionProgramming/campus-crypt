# Campus Crypt

**A dungeon crawler of scams, deadlines, and split-second choices.**

### ▶ [Play in your browser](https://vacarionprogramming.github.io/campus-crypt/)

Campus Crypt is a funny pixel-art dungeon crawler for KSU students. You play a student fighting your way down through **10 floors** of a candlelit crypt under the Kennesaw and Marietta campuses. Each floor hides a trap that looks like real campus life: a fake financial-aid message, a pile-up of deadlines, a password shortcut, an internship offer that's too good to be true, a fake parking-ticket text, the Kennesaw-Marietta commute, a textbook seller who wants gift cards, sign-in approvals you didn't start, a sublease overpayment check, and finals-week cramming. Find the floor's character, choose what to do, and the game shows you the warning signs and a safer next move. Then the **Floor Keeper** at the stairs checks what you learned: answer right and the stairs open; answer wrong and you fight its **two wardens** first.

The game teaches you to spot warning signs and take a better next step. It does not claim to prevent scams or improve grades.

![Title](screenshots/01_title.png)

| Floor | Encounter | Lesson | Ending |
|---|---|---|---|
| ![](screenshots/02_room_inbox_mimic.png) | ![](screenshots/03_encounter_choices.png) | ![](screenshots/04_lesson_warning_signs.png) | ![](screenshots/05_ending_crypt_keeper.png) |

| Monsters | Floor Keeper | Wrong answer | Wardens |
|---|---|---|---|
| ![](screenshots/08_floor_combat.png) | ![](screenshots/09_keeper_quiz.png) | ![](screenshots/10_keeper_wrong.png) | ![](screenshots/11_warden_fight.png) |

| Spam Owls | Campus monsters | A KSU warden | Keeper, floor 7+ |
|---|---|---|---|
| ![](screenshots/13_spam_owls.png) | ![](screenshots/14_campus_monsters.png) | ![](screenshots/15_ksu_warden.png) | ![](screenshots/16_keeper_deep_floor.png) |

**AI extras** (every one also works with the AI off; see "AI extras" below):

| Suspicious message | Its red flags | Spot the fake | The reveal |
|---|---|---|---|
| ![](screenshots/17_scam_message.png) | ![](screenshots/18_scam_reveal.png) | ![](screenshots/19_spot_the_fake.png) | ![](screenshots/20_spot_reveal.png) |

| Hoot's hint | Keeper taunt | Keeper banter | Pace narration | Study guide |
|---|---|---|---|---|
| ![](screenshots/21_owl_hint.png) | ![](screenshots/22_keeper_taunt.png) | ![](screenshots/23_keeper_banter.png) | ![](screenshots/24_tier_narration.png) | ![](screenshots/25_debrief.png) |

| The buzzing phone | The notice board | Study guide, still writing | Very long lines, wrapped |
|---|---|---|---|
| ![](screenshots/27_phone_prop.png) | ![](screenshots/28_notice_board.png) | ![](screenshots/26_debrief_writing.png) | ![](screenshots/29_debrief_long_lines.png) |

| Floor 1 fight: commands | An ACT finds a tell | The 5:00 PM clock attack | Spareable (gold name) | The Mimic's verdict | Volume sliders |
|---|---|---|---|---|---|
| ![](screenshots/30_fight_menu.png) | ![](screenshots/31_fight_act_reveal.png) | ![](screenshots/32_fight_attack_clock.png) | ![](screenshots/33_fight_spareable.png) | ![](screenshots/35_fight_verdict.png) | ![](screenshots/34_volume_sliders.png) |

## How a floor plays

1. **Explore.** Each floor is a generated dungeon: rooms, corridors, and walls that block your line of sight. Campus monsters (Mystery Mac Slimes, Spam Owls, Night Owls, Parking Patrol, Dead-Zone Casters, Finals Week Brutes, Late Shuttles; see "The KSU cast" below) roam and chase you. A hit costs 1 **Health**; a coffee gives 1 back. You fight with a **foam-dart blaster**, twin-stick style like *Enter the Gungeon*: aim with the mouse, hold to fire, and **dodge roll** (Shift) through shots. Your **Knowledge** makes your darts hit harder and your **Focus** makes you fire faster. A gold arrow points to your next goal.
2. **Encounter.** Find the floor's character (the Inbox Mimic, the Deadline Wraith, ...), read the narration, talk to them, read the scenario, and pick one of 3 choices. The floor pauses while you read. If you sit on the choices for a while without touching anything, **Hoot** the owl pops in with one nudge toward a warning sign (never the answer; **H** turns Hoot off).
3. **Reaction.** The character answers your choice in one line. An AI writes the line if one is set up; if not, the game uses a line written by hand.
4. **Lesson.** The game shows whether the move was safe, risky, or mixed, the warning signs, and a safer next move. Then it changes your **Health**, **Focus**, and **Knowledge**, and you go back to the floor.
5. **Floor Keeper.** The Keeper stands on the stairs and won't move until you've learned the floor's trap. Then it asks about **the choice you made** ("Back at the Inbox Mimic, you picked *Click link*. You tapped the portal link and it asks for your password. What now?"). One answer is the safe move; the other two are moves the trap is hoping for. The answer screen ignores input for a moment when it opens, and Space (the fire key) never answers, so a held trigger can't pick by accident. The Keeper opens with a taunt and answers your pick with a line. The AI may voice both, but code picks the verdict and checks the AI's line agrees with it; otherwise the written line is used, so the Keeper never calls a wrong answer right.
   **Floor 1 is a turn-based fight with the Inbox Mimic instead** (floors 2-10 keep the quiz). Its refund message sits top left (sender, subject, snippet, small print). On your turn pick a command (Left/Right + Enter/E, or 1-4): **ACT** opens 7 acts (1-7, order shuffled by seed); **INSPECT** reads the sender, link and small print for free; **SPARE**; **DELETE** ("just delete it"). Good acts make the Mimic give itself away in character: *Check sender* ("k-e-n-e-s-a-w? I spell it how I like!"), *Hover the link* (a look-alike `.example` address), *Call the office yourself* (they never heard of it), *Wait it out* (5:01 and nothing happened). Bad acts (*Reply with password*, *Pay the release fee*, *Click Review*) power it up and it gloats. After each act it's the Mimic's turn: dodge for 6 s in the box (WASD/arrows) against one of four patterns picked by seed and turn only: URGENT envelopes, a 5:00 PM clock hand sweep, homing @ signs, or a chasing CONFIRM LOGIN button. Each bad act makes the patterns faster and denser. Hits cost **box hearts** (3), never Health; running out restarts the fight (tells and power-ups cleared, no verdict). **Verdicts:** 3 different tells make the Mimic *spareable* (its name turns gold); SPARE then = right (stairs open, no wardens). SPARE earlier gets a taunt and its turn. 3 bad acts = wrong (2 wardens). DELETE with no tells = wrong (deleting unread teaches nothing); DELETE after 3 tells = right (you proved it's fake, and deleting a confirmed fake is fine); DELETE with 1-2 tells = a taunt and its turn. The Mimic signs off with one takeaway line. The fight plays on a plain black stage (no dungeon behind it), and its verdict screen keeps the Mimic's portrait. The ending's Keeper tally counts only the Keepers you reached ("answered 0 of 1 reached (floor 2/10)"; just "Reached floor 1/10" if none). All its lines are scripted, never AI. **R** = reduced motion (1.5 s turns with no attacks).
6. **Right answer:** +1 Knowledge, then a choice: **SPARE** (take the stairs, safe) or **RISK IT** (a bonus round). Risk It puts the owl back in the battle box for 15 s: the Keeper's attack patterns come in 3 waves (harder on deeper floors) while red flags pop up one at a time. Grab **3 red flags** to defeat the Keeper: +1 Health, or +1 Focus if Health is full, and it counts toward "Keepers defeated" on the ending screen. Lose all 3 box hearts and it costs 1 Health (never your last one); run out the clock and nothing changes. The stairs open whichever way it goes. **R** (reduced motion) drops the attacks and keeps the flags. The round lives in `src/risk_it.rs` ([screenshot](screenshots/20_risk_it_round.png)). **Wrong answer:** you fight the Keeper's 2 wardens with full arena-only hearts (afterwards your Health goes back to what it was before the quiz, so a wrong answer never heals; running out of hearts in the arena is still game over), one after another, in an arena: the **Tow Truck Tyrant** (its beacon flashes and a red line aims, then it charges), the **Finals Week Golem** (a growing ring, then a ring of exam pages), the **Night Owl Queen** (club-flyer sprays and Spam Owls), and the **Library Stacks Lich** (fades into the stacks, reappears, spins padlocks). Each floor sends its own pair (easiest first; see "Difficulty"), and each gets angrier (red glow, more shots) below half health. Every attack is telegraphed; only the Tow Truck Tyrant's charge hurts on contact (the others just shove you back). A coffee appears between the two. Beat both and the stairs open ([the Golem's page ring](screenshots/12_warden_golem.png)).
7. **Stairs.** Take them to the next floor. On the way down the game re-reads how you're doing and may change the crypt's pace; the floor banner says so in one line. The run ends when you climb out of floor 10 (**win**) or when Health hits 0 anywhere (**lose**). The ending screen opens on **Hoot's study guide** (3-5 lines on what tripped you up and what to watch for) and has a second tab with your "field notes" (one warning sign and one next step per choice); it also shows how many of the 10 Keepers you answered (Keepers x/10) and how many wardens and monsters you beat. Enter takes you back to the title screen.

**Side games.** Floors 2, 5 and 8 have a **buzzing phone** somewhere: read the message (an email, text or DM) and report it or trust it. Floors 3, 6 and 9 have a **notice board** with two messages: pick the fake. Either way the game then shows the red flags it planted (and, for spot-the-fake, why the real one is safe). They're optional, pause the floor while open, don't change Health/Focus/Knowledge, and any miss goes into Hoot's study guide.

The full state diagram is in [docs/FLOWCHART.md](docs/FLOWCHART.md). The engine follows it in `src/state.rs`, which has the same diagram in its header.

## Controls

| Action | Keys |
|---|---|
| Walk | WASD / arrow keys |
| Fire the foam-dart blaster | Hold left-click (aims at the mouse), or hold Space / J / the joystick button (aims at the mouse if you've moved it in the last 3 s, otherwise aim assist picks the nearest monster in sight) |
| Dodge roll | Shift, K or right-click: a quick roll you can't be hit during (short cooldown) |
| Talk / answer the Keeper / take the stairs | E or Enter when you're close |
| Choose / answer | 1-3, or arrows + Enter / E, or click (not Space; choice screens ignore input for 0.4 s after opening) |
| Read the phone / the notice board | E or Enter when you're close |
| Report / trust a message, pick the fake | 1-2, or arrows + Enter / E, or click (not Space; same 0.4 s lock) |
| Leave a side game unanswered | Esc |
| Continue | Enter / click |
| Owl hints (Hoot) on/off | H (anywhere; kept across runs) |
| Ask Hoot for a tip | T (on a floor, or while reading a scenario) |
| Hoot's tip log (this run's tips) | L (on a floor, a scenario or a lesson); Up/Down scroll, L / Esc / Enter close |
| Ending screen: study guide / field notes | ← / → or Tab, or click a tab |
| Floor 1 fight: commands / acts | Left/Right (acts: arrows) + Enter / E, or 1-4 (acts 1-7); Esc backs out of the act list |
| Floor 1 fight: dodge | WASD / arrows |
| Reduced motion (floor 1 fight: no attacks; saved) | R |
| Settings (master / music volume, reduced motion) | Title: arrows or mouse, top right. On a floor or in the arena: Esc or O pauses and opens them |

**On a phone or tablet** (the web version): turn the phone sideways. Touch controls appear the first time you touch the screen: a joystick on the left (put your thumb anywhere there and drag), and on the right **FIRE** (hold; aim assist picks the nearest monster), **ROLL**, **TALK** (talk, answer, confirm), **HOOT** and a pause button. They show only while you move or fight; on every other screen, tap what you want. Screens that only Esc closes get a **BACK** button. Code: `src/touch.rs`.
| Mute everything | M |
| Music on/off (sound effects stay) | N |
| Screenshot (native) | F12 → `screenshots/manual_NN.png` |
| Quit (title screen) | Esc |

## Setup

You need Rust (stable), which you can install from <https://rustup.rs>.

    cargo run --release                         # native game (Linux / macOS / Windows)
    cargo run --release --no-default-features   # no AI HTTP client, no Arduino link (offline-only build)
    cargo test --release                        # unit tests: content contract, state machine, AI filter

Linux also needs the ALSA dev package for audio: `sudo apt install libasound2-dev`.

**Web build** (always uses the written fallback lines):

    rustup target add wasm32-unknown-unknown
    cargo build --profile wasm-release --target wasm32-unknown-unknown    # size-tuned (opt-level "s")
    cp target/wasm32-unknown-unknown/wasm-release/campus_crypt.wasm web/
    # optional, ~10% smaller: binaryen's wasm-opt (flags used for the shipped web/campus_crypt.wasm)
    wasm-opt -Os --enable-bulk-memory --enable-bulk-memory-opt --enable-nontrapping-float-to-int \
      --enable-sign-ext --enable-mutable-globals --enable-multivalue --enable-reference-types \
      web/campus_crypt.wasm -o web/campus_crypt.wasm
    cd web && python3 -m http.server 8765      # then open http://localhost:8765

## AI reactions (optional)

The AI writes words, never outcomes. Here it writes the line the floor's character says back to your choice, in that character's own voice (`src/ai.rs`); the "AI extras" below add a few more lines. Everything else is fixed by the game code and the content file: the outcome, the Health/Focus/Knowledge changes, the warning signs, the safer next move, the Floor Keeper's question and answer, whether your answer was right, and whether you win.

The game uses the **Groq API (free tier)** with model `openai/gpt-oss-20b`. Get your own free key at <https://console.groq.com/keys>, then on a Mac (Terminal):

    export GROQ_API_KEY=...        # paste your own key
    cargo run --release

* Without a key, the game uses the written lines. `CAMPUS_CRYPT_AI=off` also forces the written lines.
* The free tier allows 30 requests per minute and 1,000 per day for this model. The game sends one request per floor character (10 per play-through), plus at most 16 for the AI extras, spaced at least 3 s apart: about 25 over a 10+ minute run.
* If you hit a limit (HTTP 429), the game retries once. If that fails too, it uses the written line, so the game keeps going.
* The **web build** always uses the written lines.
* To use other providers, set `CAMPUS_CRYPT_AI_KEY`, `CAMPUS_CRYPT_AI_URL` (any OpenAI-compatible chat-completions URL), and `CAMPUS_CRYPT_AI_MODEL`. The game only sends the `reasoning_effort: low` / `include_reasoning: false` settings to gpt-oss models or Groq.
* The system prompt is `assets/content/ai_prompt.md`. The engine sends it `encounter_id`, `character_name`, `scenario`, `choice`, and `outcome`, and tells the model to speak as that floor's character. The Floor Keeper's lines come from the AI extras (below), never from this prompt.
* The request runs on a background thread, so the game never freezes. If no usable line arrives within 4.5 s, or the reply is empty, the game shows that choice's `fallback_reaction`.
* Each AI line is cleaned up for the pixel font: emoji, other non-ASCII characters, markdown `*`/`_`, wrapping or unbalanced quotes, and extra whitespace are removed. Then the line is checked. It's rejected if it's under 6 words or 30 characters, doesn't end with `.` `!` or `?` (a cut-off reply), is over 160 characters, has a link, bare domain or email address, or mentions health, focus, knowledge, score, or damage. A rejected line is replaced by the written line.
* Small print under each line says **AI-voiced line**, or that it's a written line and why ([screenshot](screenshots/07_live_ai_reaction.png)).
* Live test of every choice (30 with the shipped content) against your key: `cargo test --release live_ai -- --ignored --nocapture`. `LIVE_SLEEP_MS=6000` sets the pause between calls (default 4500); `LIVE_ONLY=deadline_wraith/postpone,inbox_mimic/report_it` runs only those choices.

## AI extras

Six extras built on the same rule: **code decides, the AI only writes, and every text has a written fallback.** They use the same key and model as the reactions. With `CAMPUS_CRYPT_AI=off`, no key, no network, or on the **web build**, every one of them still works with written text. Nothing ever waits on the network: messages are fetched in the background ahead of time, and anything not ready is written text. Small print under each AI-capable text says **AI-voiced line** / **AI-written message**, or "scripted ... (why)": `offline`, `AI not ready`, `AI unavailable`, `AI call cap reached`, `AI line rejected by filter`, `AI timed out`, or `scripted slot`. The title screen shows `AI extras: on (model)` or `AI extras: offline (scripted)`. Backend and filters: `src/ai_features/` (Dey); content: `assets/content/ai_features.json` and `ai_features_prompts.md` (Ry).

| Extra | When you see it | What code decides | What the AI may write |
|---|---|---|---|
| **Live scam generator** | Floors 2, 5, 8: walk up to the buzzing phone, press E ([phone](screenshots/27_phone_prop.png)). Read the email / text / DM, then **1 Report it** or **2 Trust it**. The reveal shows the planted red flags as chips with a tip each ([screenshot](screenshots/17_scam_message.png), [reveal](screenshots/18_scam_reveal.png)). | The template, the 2-3 red flags (every one visibly in the message), that it is a scam (so Report is always right), the fake sender address. | The message body around the flags (prefetched at run start, slots 0-2). |
| **Spot-the-fake** | Floors 3, 6, 9: the notice board ([board](screenshots/28_notice_board.png)). Two messages side by side; pick the fake with 1-2 / arrows / click. The fake turns red with its red-flag chips; the real one turns green with why it's safe ([screenshot](screenshots/19_spot_the_fake.png), [reveal](screenshots/20_spot_reveal.png)). | Which card is fake, the order, the safe message, the flags. | Nothing new: the fake uses scripted scam slots 3+ so it never repeats a phone message. With today's 9 scam templates all three boards appear; a round would only be skipped if every template had already been shown this run. |
| **Keeper banter** | The quiz opens with the Keeper's taunt; after you answer, its verdict line ([taunt](screenshots/22_keeper_taunt.png), [verdict](screenshots/23_keeper_banter.png)). | The verdict (`Quiz::is_correct`). The backend checks the AI line agrees with it (`banter::verdict_consistent`); on any mismatch you get the written `keeper::line`. | The taunt and the right / wrong line. |
| **Owl companion hints** | On the encounter screen, after you sit on the choices without input (12 s; 8-20 s by skill tier), Hoot pops in at the bottom with one nudge, once per floor, until you choose ([screenshot](screenshots/21_owl_hint.png)). **H** toggles Hoot anywhere; the HUD help shows its state. | When (hesitation, once per floor, toggle). A hint must not name a choice, say pick/choose, or echo a safe move. | The hint. |
| **Hoot's tips (ask anytime + after each floor)** | **T** asks Hoot for a tip on a floor or while reading a scenario; a short "for next time" tip is also written while you read each lesson and pops in when you are back on the floor. **L** opens the tip log for the run. The matrix on the Elegoo board shows the owl whenever Hoot speaks (`E:TIP`). | When (T, or after a lesson), what Hoot may say per situation: walking the floor = one general habit (the AI is not told what the floor holds); reading a scenario = the same rules as the hesitation hint (no choice names, no giveaways); after a floor = one habit for next time. Live calls are capped at 12 a run with a 6 s cooldown between asked tips, past that the scripted bank answers at once. | The tip. Offline, rejected by the filters, timed out or over budget, Hoot reads a scripted tip from the built-in bank (`ai_features/tips.rs`) plus the red-flag tips in `ai_features.json`; the log tags each tip "AI-written" or "scripted". |
| **AI-written encounters** | Floors 2, 5 and 8 can play a brand-new encounter written by the AI (tagged "AI-WRITTEN" on the scenario card): scene, scenario, three choices, each choice's safe / risky / mixed label, two warning signs and a safer next move. | Code picks the theme and the two red flags the scenario must show, then verifies the reply: every text passes the game's filters, the scenario contains cue words for both flags, and each choice's words must agree with its label (a SAFE choice must read as verify / ask / report / decline, a RISKY one as click / pay / send / share). Health, focus and knowledge changes come from code by outcome, never from the AI. | The encounter. Offline, late, or failing any check, the scripted encounter for that floor plays instead (`ai_features/genenc.rs`). The Floor Keeper asks the generic question for these floors. |
| **Adaptive difficulty narration** | On the floor banner, under the floor title, when the crypt's pace changes ([screenshot](screenshots/24_tier_narration.png)). | The skill tier (Gentle / Standard / Sharp / Expert, from your choices, Keeper answers and health, one step per floor at most) and what it changes (see "Difficulty"). | The one narration line. |
| **Post-run debrief** | The ending screen opens on "Hoot's study guide": 3-5 lines from your mistake log (risky or mixed choices, wrong Keeper answers, missed fakes, trusted scams). "Hoot is writing your study guide..." shows while it waits (8.5 s at most); every line wraps inside the panel, and if the guide is too tall the text steps down from 24 px (never cut off) ([screenshot](screenshots/25_debrief.png), [waiting](screenshots/26_debrief_writing.png)). | The mistake log. An AI line that names a floor or character not in the log is rejected. | The study guide; otherwise it's templated from the same log. |

Budget: one background worker, one call at a time, at least 3 s apart, 4.5 s timeout each, at most 16 per run (the last one is saved for the debrief). A new run drops the old run's queued calls. Live check of the prompts: `LIVE_SLEEP_MS=4000 cargo test --release live_ai_features -- --ignored --nocapture`.

## Content

The content author edits the files in `assets/content/`:

* `encounters.json` holds the floors' encounters, in order (one floor each). A run has 10 floors; with fewer than 10 encounters the game reuses them in order, so it still runs (and tests pass) while content is being written, and with more than 10 it gives each its own floor. The schema is in `src/content.rs`. Each choice has an `outcome` (`safe` / `risky` / `mixed`), integer `effects`, a `fallback_reaction`, `warning_signs`, a `safer_next_move`, and a `keeper` question (below).
* `ai_prompt.md` is the AI system prompt.

Both files are built into the binary, so the web build works. Native builds also read them again at launch, so you can test content edits without recompiling. To use a different folder, set `CAMPUS_CRYPT_CONTENT=/path`. The game prints length-limit warnings in the console and notes them on the title screen. Sprites are chosen by encounter id (`inbox_mimic`, `deadline_wraith`, `password_hydra`, `internship_impostor`, `ticket_troll`, `commute_chimera`, `gift_card_gargoyle`, `push_poltergeist`, `overpay_ogre`, `cram_lich`). An unknown id gets a generic hooded figure, so you can add floors without touching the engine.

### Floor Keeper questions

The Keeper asks about the choice the player made on that floor. Its question lives next to that choice as a `keeper` block:

```json
{
  "id": "click_link",
  "label": "Click link",
  ...
  "keeper": {
    "question": "You tapped the portal link and it asks for your StudentAid.gov password. What now?",
    "answer": "Don't type it. Close the page, go to StudentAid.gov yourself, change your password if needed, and report the message.",
    "wrong": [
      "Enter the password so the grant isn't lost, and change it some time next week.",
      "Forward the link to your friends so they can claim their grants too."
    ]
  }
}
```

* `question` (up to 120 characters): what the Keeper asks. The game shows "Back at *encounter*, you picked *label*." above it.
* `answer` (up to 150): the one right answer.
* `wrong` (exactly 2, up to 150 each): moves that are **clearly unsafe** for this situation. Never put good advice here, even advice for another floor; the game warns if a wrong option matches any `safer_next_move` or the answer.
* The game shuffles where the right answer goes (fixed per run and floor).

**Floor 1's Inbox Mimic fight** comes from the encounter's `keeper_box` block: `from`, `subject`, `snippet` (up to 220), `fine_print` (the small line under it), `link` (the look-alike address INSPECT and *Hover the link* show; `.example` only), `question`, `answer` and 2 `wrong` (up to 40 characters each; they now feed the verdict summary), and `warning` (shown with the verdict). Only floor 1 uses it; if it's missing or incomplete, floor 1 gets the normal quiz. The code (not the AI) decides the right answer; the AI scam generator isn't used for this round yet.

Each encounter can also have its own `keeper` block (same three fields) as a fallback. The block that supplies the question also supplies the answer and wrong options, so options never mix between questions: a choice block with a `question` is used on its own (a missing `answer` becomes the choice's `safer_next_move`, missing wrong options become generic unsafe ones); a choice with no block, or a block without a `question`, uses the encounter's block. The game also warns if a wrong option even resembles good advice anywhere in the file (same words after ignoring case and punctuation, or 70% word overlap). Keep the three options about the same length so the longest one isn't a giveaway; a unit test checks the shipped questions. With no blocks at all, the Keeper asks "What is the safer next move?" with the choice's `safer_next_move` as the answer and generic unsafe moves as the wrong ones, and the console warns. The Keeper's spoken lines (right / wrong) are in `src/keeper.rs`.

Resources (in `src/state.rs`): Health 5/5, Focus 3/5, Knowledge starts at 0. Changes are clamped to these limits. Encounter effects, a right Keeper answer (+1 Knowledge), and the real-time layer (a hit -1 Health, a coffee +1 Health) are the only things that change them. If you pick the most damaging choice in every encounter, you lose on the last floor even without taking a hit; a unit test checks this.

## The KSU cast

Original black-and-gold pixel art, drawn in code (`src/art.rs`). It's a playful nod to campus life at Kennesaw State: no official KSU logos, wordmarks or other marks, and no real mascot likeness (the owls are generic owls).

| Floor monster | Where it shows up | What it does |
|---|---|---|
| **Mystery Mac Slime** | floors 1-10 | the dining hall's special; wobbles after you |
| **Spam Owl** (was the Spam Bat) | floors 1-10 | swoops side to side carrying junk mail; the Night Owl Queen calls more |
| **Night Owl** | floors 5-6, 8-10 | an all-nighter owl: faster and tougher |
| **Parking Patrol** | floors 3-10 | keeps its distance and fires parking tickets |
| **Dead-Zone Caster** | floors 5-10 | a campus Wi-Fi dead zone that lobs no-signal orbs |
| **Finals Week Brute** | floors 7-10 | slow, heavy, shrugs off knockback |
| **Late Shuttle** | floors 8-10 | the Kennesaw-Marietta shuttle, running late and straight at you |

| Warden | Tell | Attack |
|---|---|---|
| **The Tow Truck Tyrant** | beacon flashes, red aim line | charges (the only contact hit), tow hooks when angry |
| **The Finals Week Golem** | a red ring grows | a ring of exam pages |
| **The Night Owl Queen** | a gold glow | club-flyer sprays, summons Spam Owls |
| **The Library Stacks Lich** | fades into the stacks | padlock bolts while solid |

The 10 floor characters (Ry's content) each have their own sprite too, and the Floor Keeper is now a charcoal owl with a gold lantern.

## Difficulty

Difficulty climbs in **ten steps, one per floor** (`crawl::Tier::level`, 1 to 10), and the whole run is scaled back from the original tuning: floor 10 plays like the old 4-floor run's floor 3.25 instead of its floor 4.

* **Monsters:** 4 on floor 1 up to 9 on floor 10, and the mix shifts a little on every floor (tougher kinds come in gradually). They move at 78% of their listed speed on floor 1, rising 1.5 points a floor to about 92% on floor 10.
* **Wardens** walk and charge at 85% speed, have 15% less health than before, and every enemy shot flies 15% slower (same range).
* **Maps:** 44x32 tiles and 7 rooms on floor 1, up to 49x34 and 9 rooms on floor 10.
* **Monster and warden health** grow a little each floor. Knowledge also grows by at least 1 per floor and makes every dart hit harder, so health is scaled so that a player with the least Knowledge possible on a floor needs the same number of hits as the curve expects. Players who learned more hit harder than that.
* **Warden pairs** matter more than warden health (the ranged Queen and Lich are the hard ones), so the pairs go easiest first: Tyrant + Golem, Tyrant + Queen, Golem + Queen, Tyrant + Lich, Golem + Lich, Queen + Lich, then Tyrant + Lich and Queen + Lich alternating. All six pairings appear.

The survival table below was measured before the blaster and the gentler curve; rerun it to refresh it. `cargo test --release warden_difficulty -- --ignored --nocapture` plays both wardens of every floor 40 times from full arena hearts with the autopilot (Knowledge = floor number). Survival out of 40:

| Floor | 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 | 9 | 10 |
|---|---|---|---|---|---|---|---|---|---|---|
| Sharp bot (reacts at once) | 40 | 40 | 40 | 40 | 40 | 39 | 40 | 40 | 40 | 40 |
| Slow bot (reacts 0.25 s late) | 40 | 38 | 32 | 31 | 27 | 27 | 28 | 25 | 26 | 20 |
| Old 4-floor run, slow bot | 40 | 31 | 26 | 18 | | | | | | |

The hardest floor of the new run (floor 10, 50%) is a little easier than the old run's floor 4 (45%). `two_wardens_are_beatable_but_not_trivial` checks every floor stays in range.

**Adaptive pacing (skill tier).** On every descent code recomputes the skill tier (`ai_features::difficulty`; it starts at Standard and moves one step per floor at most) and the next floor is built with it (`crawl::Pace`). The backend suggests knobs per tier; the game applies them capped by `Pace::capped`, so **wardens are never tougher than the tuned curve** and floor monsters are at most 10% faster:

| Tier | Floor monsters: speed / count | Warden health | Hoot's hint after |
|---|---|---|---|
| Gentle | 85% / -1 | 85% | 8 s |
| Standard | 100% / +0 | 100% | 12 s |
| Sharp | 110% / +1 | 100% (suggested 110%) | 16 s |
| Expert | 110% (suggested 120%) / +2 | 100% (suggested 120%) | 20 s |

Why the cap: the warden sim with the slow bot (`cargo test --release skill_tier_difficulty -- --ignored --nocapture`, 40 seeds per floor, survival /40 on floors 1-10):

| Tier | Applied (what ships) | With the suggested warden health |
|---|---|---|
| Gentle | 40 39 35 38 38 20 36 19 35 26 | same |
| Standard | 40 38 32 31 27 27 28 25 26 20 | same |
| Sharp | 40 38 32 31 27 27 28 25 26 20 | 40 35 12 23 10 19 22 20 22 18 |
| Expert | 40 38 32 31 27 27 28 25 26 20 | 34 32 2 19 5 11 13 11 14 10 |

Uncapped, an Expert-tier player who misses one Keeper on floor 10 would survive the wardens 25% of the time (10/40). Capped, every tier keeps floor 10 at 50% or better (Gentle 65%); `skill_tier_pacing_keeps_the_last_wardens_fair` checks at least 45% for every tier. Pacing reaches the arena only as warden health (the Night Owl Queen's summoned owls keep normal speed). With 200 seeds, floor 10 is 51% at Standard / Sharp / Expert and 59% at Gentle.

## Music

One looping track per screen and floor, all original compositions by Dey (CC0; not the KSU fight song): a title march and ten floor themes that speed up and intensify floor by floor (100 to 150 BPM).

* `assets/audio/title_march.ogg` on the title screen.
* `assets/audio/floor_01.ogg` ... `floor_10.ogg` on floors 1-10 (the warden arena keeps its floor's track).

How it works (`src/music.rs`):

* **Nothing loads at start-up.** The game asks for the track it needs now plus the next one (the title loads floor 1, floor *n* loads floor *n*+1) in the background. Natively a thread decodes the file; on the web the browser does (`decodeAudioData`). The last step natively (handing ~0.1 s of PCM work to the mixer) only runs on the title screen or the black frame between floors, so it never hitches play.
* **A track that isn't ready is silence**, and fades in when it arrives. **A missing or broken file is silence** for that track (a console note natively); the game never crashes or waits on music. A unit test covers this.
* Tracks **loop** and **crossfade over about 1 s** when the screen or floor changes. Only the playing, fading and next tracks stay in memory.
* **M** mutes everything; **N** turns music off and on. **Options** (title menu > OPTIONS, or Esc / O to pause): master and music volume (master 100%, music 75% by default; music plays at music × master, effects at 0.6 × master), music on/off, reduced motion, Hoot's hints, **AI extras** on/off (off = scripted lines and no network calls, even with a key set) and the **Arduino display** on/off. All of them are saved between launches: natively in `~/.config/campus-crypt/settings` (`$XDG_CONFIG_HOME` if set; `%APPDATA%\campus-crypt\settings` on Windows, `~/Library/Application Support/campus-crypt/settings` on macOS; `CAMPUS_CRYPT_SETTINGS=/path` overrides), on the web in `localStorage` (`web/settings.js`). The v3 tracks are mastered loud (about −16 to −14 LUFS), so there's no extra gain; the native decoder upsamples 32 kHz to 44.1 kHz with Catmull-Rom interpolation (measured: same loudness, top end within about 1 dB of the source). Native overrides: `CAMPUS_CRYPT_MUSIC_VOLUME=0.0-1.0`, `CAMPUS_CRYPT_MUSIC_DIR=/path` (a different folder) or `CAMPUS_CRYPT_MUSIC_DIR=off`.
* Native builds look in `assets/audio/` under the working directory, then next to the executable, then in a macOS bundle's `Contents/Resources/assets/audio/`. **The web build fetches `assets/audio/` relative to the page, so the files are also copied to `web/assets/audio/`.**
* To add a track: add a `Track` variant with its file name in `src/music.rs` and ask for it from `main.rs`.

## Code map

| File | What it does |
|---|---|
| `src/state.rs` | The game's state machine and resources (floors, the Keeper, wardens). Game state changes only here. |
| `src/crawl.rs` | The real-time floors (restored from Delve): map generation, line of sight, monsters, combat, projectiles, coffee, the four wardens, and the autopilot used for tests. It reports events; it never changes resources itself. |
| `src/keeper.rs` | The Floor Keeper's question (from the `keeper` blocks in `encounters.json`) and its written lines. |
| `src/keeper_fight.rs` | Floor 1's Inbox Mimic fight: command menu, acts and tells, spare/delete verdicts, the four dodge patterns, box hearts and resets, reduced motion; a verdict answers the Keeper through the same state change as the quiz. |
| `src/risk_it.rs` | The Risk It bonus round after a right answer: 3 waves of the box attack patterns, red flags to grab, win / knocked out / time up. Rewards are applied by `Run::risk_it` in `src/state.rs`. |
| `src/settings.rs`, `web/settings.js` | Saved Options (volume, music, reduced motion, hints, AI extras, Arduino display): a small text file natively, `localStorage` on the web. |
| `src/display_live.rs`, `src/bin/crypt_display.rs` | Live LCD/matrix layouts for the Arduino display and the sketch-upload hand-off (see `hardware/README.md`, "Live layout"). |
| `src/content.rs` | Loads and checks `encounters.json`. Converts typographic characters to ASCII so the pixel font can show them. |
| `src/ai.rs` | Optional AI reaction line: background request, timeout, output filter, fallback. |
| `src/ai_features/` | The AI extras' backend (Dey): scam generator, Keeper banter, debrief, spot-the-fake, owl hints, difficulty tier and narration, call budget and filters. `main.rs` draws them. |
| `src/art.rs` | Pixel art drawn in code (sprites as text grids, including the Keeper, wardens, and monsters), dungeon tiles, lighting, text wrapping. |
| `src/main.rs` | Screens, input, layout for each phase, autopilot for tests and screenshots. |
| `src/audio.rs` | Sound effects synthesized in code (from Delve). |
| `src/music.rs` | The per-floor music tracks: lazy background loading, silent fallback, looping, crossfades. |
| `src/hardware.rs` | Optional USB link to the Mega 2560 status display (from Delve; output only). |
| `src/perf.rs` | Opt-in frame profiler (`CC_PERF=1`). |

Developer switches (native, environment variables):

* `CAMPUS_CRYPT_AI=off` forces the written lines even when an API key is set (handy for demos, screenshots, and offline judging).
* `CC_PERF=1` prints a per-screen table at exit: frames, average and worst CPU ms per frame, frame time, and heap allocations per frame.
* `CC_NOVSYNC=1` turns off vsync so the frame rate is uncapped, for measuring.
* `CAMPUS_CRYPT_HW=off` skips the Arduino port scan.

Automated play-through (native) is used to make the screenshots and to play-test both Keeper paths:

    CC_AUTO=1 CC_PICKS=0,0,2,1,2,2,2,0,2,1 CC_KEEPER=w,r,w,r,r,r,w,r,r,r CC_GOD=1 CC_SEED=42 CC_SHOTS=10,20,30 CC_SHOT_PREFIX=demo_ CC_EXIT=130 cargo run --release

* `CC_AUTO=1` plays by itself: it walks the floor (BFS path to the next goal), shoots monsters, talks, answers, and fights the wardens (it circles them at blaster range). It prints one `[auto]` line per state change (floor, phase, health, wardens left) and the ending.
* `CC_PICKS` = the choice per floor; `CC_KEEPER` = `r` (right) or `w` (wrong) per floor's Keeper (default right).
* `CC_MINI` = `r` / `w` per side game in the order they're opened (phone: report / trust; board: the fake / the real one). The autopilot visits each floor's phone or board before the floor's character.
* `CC_ENC_WAIT=secs` = how long the autopilot sits on each encounter (default 2; 16 lets Hoot speak).
* `CC_SHOT_EVENTS=1` = save `screenshots/<CC_SHOT_PREFIX><screen>.png` the first time each AI-extras screen is on show (title, side game ask / reveal, Hoot, Keeper taunt / verdict, pace banner, study guide wait / ready, field notes), then quit after the ending.
* `CC_DEBRIEF_LONG=1` = swap the study guide for five 250-280 character lines built from real content (Ticket Troll's "Tap pay link" first) to check the ending panel's wrapping ([screenshot](screenshots/29_debrief_long_lines.png)).
* `CC_GOD=1` = monsters and wardens can't hurt you. Warden difficulty numbers: `cargo test --release warden_difficulty -- --ignored --nocapture` (see "Difficulty"). `CC_SEED=n` = the same floors every run. `CC_FLOOR=n` = start on floor n (0-based).

## Bonus: ELEGOO Mega 2560 status display

This is optional. The game runs without it, and the keyboard is the only control. An Arduino Mega 2560 from the ELEGOO Most Complete Starter Kit mirrors the run on an LCD1602 (Health bar, Knowledge, floor of 10, clock), the bare 4-digit 7-segment display (the floor, or a blinking `5:00` in the Inbox Mimic fight), a MAX7219 8x8 matrix (CRYPT title, torch, a heart dodging in a box during the fight, hit / right-answer / stairs / skull animations) and a DS1307 clock. It is output only: no joystick, and the sketch never reads an input pin. 36 wires (9 female-to-male Duponts + 27 breadboard jumpers). The sketch, wiring, diagram, demo guide and a protocol-checking serial simulator are in [`hardware/`](hardware/README.md). Port override: `--hw-port <path>` or `CAMPUS_CRYPT_HW_PORT`. Turn it off in **Options > Arduino display** or with `CAMPUS_CRYPT_HW=off`. The LCD + matrix sketch's layout can be changed live, with no re-upload (hardware/README.md, "Live layout").

## Credits and sources

Campus Crypt was a collaborative effort between **Deyanna Gladney** and **Rythm-Prince Ogburn**, working with the agents **itcan.do**, **Claude Code**, **Claude**, **Groq** and **Grokbot**.

* Built on **Delve** by Darby (Rust + macroquad).
* **Music:** "Campus Crypt — Floor Themes & Title March" (v3 remaster, louder and clearer), original compositions composed and generated by Dey for Campus Crypt: a heroic title march and ten seamless floor loops in a 32-bit-era console-RPG orchestral-synth style that build on an original Fight March motif and escalate from 100 BPM (floor 1) to 150 BPM (floor 10). Released under [CC0 1.0](https://creativecommons.org/publicdomain/zero/1.0/). Not a quotation, paraphrase, arrangement, or derivation of the Kennesaw State University fight song (James Sochinski, 2012), nor of any other existing fight song, march, or game soundtrack. Details and track list: [docs/CREDITS.md](docs/CREDITS.md).
* Asset, crate, and AI credits: [docs/CREDITS.md](docs/CREDITS.md). Guidance sources: [docs/SOURCES.md](docs/SOURCES.md).
