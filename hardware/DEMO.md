# Demo guide (Mac): the Campus Crypt status display

From a bag of parts to a working demo. Plan on about 45 minutes the first time; most of it is wiring. The display is a bonus: the game runs the same without it.

## Part A: Wire it up

Follow **[WIRING.md](WIRING.md)** with **[wiring_diagram.png](wiring_diagram.png)** open next to it (36 wires: 9 female-to-male Duponts + 27 breadboard jumpers). Go through the checklist at the end of WIRING.md before you plug in USB. There is no joystick in this build.

## Part B: Put the sketch on the Mega

### 1. Install Arduino IDE 2

* Download **Arduino IDE 2** for macOS from <https://www.arduino.cc/en/software> and drag it into Applications (or `brew install --cask arduino-ide`).

### 2. Board package and libraries

* **Tools → Board → Boards Manager…**, search **"Arduino AVR Boards"**, **Install** (it may already be there).
* The sketch needs only **LiquidCrystal** and **Wire**, which come with the IDE. If the upload fails with `LiquidCrystal.h: No such file or directory`, install **LiquidCrystal by Arduino** from **Tools → Manage Libraries…**.

### 3. Open the sketch

**File → Open…** → `hardware/campus_crypt_display/campus_crypt_display.ino` (the `.ino` stays in its same-named folder).

### 4. Board and port

1. Plug the Mega into the Mac with the USB cable.
2. **Tools → Board → Arduino AVR Boards → Arduino Mega or Mega 2560** (processor: **ATmega2560**).
3. **Tools → Port** → the new `/dev/cu.usbmodem…` entry (or `/dev/cu.usbserial-…` / `/dev/cu.wchusbserial…` on CH340 clone boards; see Troubleshooting).

Command-line alternative:

```bash
arduino-cli core install arduino:avr
arduino-cli compile --fqbn arduino:avr:mega hardware/campus_crypt_display
arduino-cli upload  --fqbn arduino:avr:mega -p /dev/cu.usbmodem1101 hardware/campus_crypt_display
```

### 5. Upload

Click **→ (Upload)**. After about 20 seconds the IDE says **"Done uploading."** The LCD shows `CAMPUS CRYPT` / `waiting...` and the time, the 4-digit display shows the clock with a blinking dot, and the matrix shows a small spinning dot.

### 6. Check it in the Serial Monitor (optional)

1. **Tools → Serial Monitor**, **115200 baud**, line ending **New Line**.
2. You should see `HELLO:CRYPT` every 2 seconds.
3. Drive the displays by hand (type each line, press Enter):
   ```
   S:4,5,2,3,4,1,P,F,10
   E:HIT
   E:LVL
   S:5,5,1,1,4,0,F,F,10
   E:STAIRS
   E:TIP
   S:0,5,2,3,4,1,D,F,10
   ```
   You should get `HP 4/5` + bar / `Kn2 F3/10`, `F  3` on the 4-digit display, a heart flash, a burst, then the fight screen (`* INBOX MIMIC *`, a blinking `5:00`, a heart dodging in a box), stairs, a blinking owl (Hoot's tip), and the skull. While it gets `S:` lines it also prints `HB` twice a second. After 3 s without an `S:` line it goes back to "waiting...".

### 7. Close the Serial Monitor (important)

Only one program can use the USB port. Close the Serial Monitor (or quit the IDE) or the game will report the port as **busy**.

## Part C: Run Campus Crypt

Start the game as usual (the `Campus Crypt.app`, or `cargo run --release` from the project folder). Within about 3 seconds the title screen's top-left corner says **Status display: Connected on /dev/cu.usbmodem…**, the LCD changes to `CAMPUS CRYPT` / `Press Enter`, and the matrix scrolls **CRYPT**.

## Part D: What to show

| Do this (keyboard) | What the board does |
|---|---|
| Start a run | LCD `HP 5/5` + bar / `Kn0 F1/10`, 7-segment `F  1`, matrix torch |
| Make a safe choice, or answer a Keeper right | Burst on the matrix; `Kn` goes up |
| Take a risky choice or get hit by a monster | Heart flash; HP and the bar drop |
| Reach the floor-1 Keeper (the Inbox Mimic fight) | LCD `* INBOX MIMIC *` / `F1/10 FIGHT`, 7-segment blinks `5:00`, matrix shows a heart dodging in a box |
| Get hit in the fight's dodge box | Heart flash (box HP only; your Health doesn't change) |
| Take the stairs | Stairs icon; `F1/10` → `F2/10` and `F  2` |
| Drop to 1 Health | Beating heart on the matrix |
| Press Esc / O (sound settings) | Pause icon, LCD `PAUSE F3/10` |
| Get overwhelmed | Skull, LCD `OVERWHELMED x_x` |
| Back to the title | Title scroll returns |
| Pull the USB cable mid-run, plug it back in | The game keeps running; a few seconds later it reconnects and the displays catch up |

## Troubleshooting

**No port, or the game never connects**

* Try another USB cable (many are charge-only) and plug straight into the Mac.
* CH340 clone boards show up as `/dev/cu.usbserial-…`; if nothing appears, install WCH's CH34x driver ("CH34XSER_MAC"), allow it in **System Settings → Privacy & Security**, restart.
* `ls /dev/cu.*` in Terminal lists the ports.

**"… is busy (close Serial Monitor)"** — the IDE still has the port. Close it. The reverse also happens: an upload fails with "Resource busy" while the game is running; quit the game, upload, start it again.

**LCD lit but blank, or a row of solid blocks** — turn the contrast pot end to end. Then check D9–D12 → LCD 11–14 in order, E (6) → D8, RS (4) → D7, RW (5) → GND. Garbled text usually means a loose data wire or a missing GND.

**LCD backlight off** — pin 15 → 220Ω → + rail, pin 16 → – rail, and Mega 5V → + rail.

**Matrix blank** — VCC/GND on the rails, DIN → D2, CLK → D3, CS → D4, and the **DIN** (input) side of the module. Mirrored or upside down: set `MATRIX_FLIP_X` / `MATRIX_FLIP_Y` to 1.

**4-digit display wrong** — garbage segments: a segment wire is swapped (WIRING.md section 2). A missing digit: check D30–D33 → pins 12/9/8/6. Very dim: the resistors belong on the segment pins only.

**Clock wrong, or stuck at 12:00** — SDA/SCL must be on **D20/D21** (not A4/A5 on a Mega). To reset the time, take the coin cell out for a few seconds, put it back and upload again (the first run sets the compile time).

## No board handy? Use the simulator

```bash
python3 hardware/sim/crypt_sim.py --duration 60 --strict     # terminal 1
cargo run --release -- --hw-port /tmp/crypt-sim-port          # terminal 2
```

Terminal 1 prints every line the game sends with the LCD text it would produce, then a summary.
