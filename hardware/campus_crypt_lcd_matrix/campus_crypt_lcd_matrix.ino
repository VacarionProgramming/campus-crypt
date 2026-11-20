/*
  Campus Crypt status display  -  ELEGOO Mega 2560, LCD1602 + MAX7219 matrix only
  -------------------------------------------------------------------------------
  Mirrors the Campus Crypt game over USB serial (115200 baud, one ASCII line per
  message) on:
    * LCD1602 (parallel, 4-bit mode)  - HP bar, Knowledge, floor of 10, Keeper fight
    * MAX7219 8x8 LED matrix module   - CRYPT title scroll, torch, fight icon,
                                        hit / level-up / stairs / skull

  No 4-digit display, no RTC, no joystick, no buttons.

  OUTPUT ONLY: this sketch never reads an input pin and never sends movement
  ("J:") lines. It sends HELLO:CRYPT while waiting and an HB heartbeat every 0.5 s.
  The keyboard is the only control.

  LIVE LAYOUTS: the two LCD lines for each screen and the matrix picture for
  each screen/event can be changed over serial with L... lines (see "live
  layout" below and hardware/README.md), with no re-upload. LSAVE keeps them in
  EEPROM, so they survive a reset; LRESET goes back to the built-in layout.

  Libraries: only LiquidCrystal and EEPROM, which ship with the Arduino IDE.
  The MAX7219 is driven directly by this sketch.

  Pin map (Arduino Mega 2560). D0/D1 are USB serial: do not use.
    D51 -> MAX7219 DIN     D52 -> MAX7219 CLK     D53 -> MAX7219 CS
    5V / GND -> MAX7219 VCC / GND
    D7  LCD RS (pin 4)     D8  LCD E (pin 6)      D9-D12  LCD D4-D7 (pins 11-14)
    LCD: pin 1 VSS, 5 RW, 16 K -> GND; pin 2 VDD, 15 A -> 5V (A through 220 ohm);
         pin 3 V0 -> contrast pot middle pin
*/

#include <LiquidCrystal.h>
#include <EEPROM.h>

// ------------------------------------------------------------------ pins
const uint8_t PIN_MAX_DIN = 51;
const uint8_t PIN_MAX_CLK = 52;
const uint8_t PIN_MAX_CS = 53;
const uint8_t PIN_LCD_RS = 7;
const uint8_t PIN_LCD_E = 8;

// ------------------------------------------------------------------ options
#define MATRIX_FLIP_X 0      // set to 1 if matrix images look mirrored
#define MATRIX_FLIP_Y 0      // set to 1 if matrix images look upside down
#define MATRIX_BRIGHTNESS 3  // 0..15 (default; LB:n changes it live)

LiquidCrystal lcd(PIN_LCD_RS, PIN_LCD_E, 9, 10, 11, 12);

// ------------------------------------------------------------------ game state
struct GameState {
  int hp = 0, maxhp = 5, lvl = 0, floorNo = 1, floors = 10;
  long gold = 0, kills = 0;
  char mode = 'M';   // M menu, P playing, F Keeper fight, X paused, D overwhelmed
  char disp = 'F';   // F floor, G focus ("gold"), K kills (parsed, unused)
};
GameState gs;
unsigned long lastStateMs = 0;   // when the last S: line arrived
bool gameConnected() { return lastStateMs != 0 && millis() - lastStateMs < 3000; }

// ------------------------------------------------------------------ MAX7219 8x8 matrix
uint8_t brightness = MATRIX_BRIGHTNESS;
uint8_t fb[8];        // what we want to show (row 0 = top, bit 7 = left)
uint8_t fbShown[8];   // what the chip currently shows
bool fbForce = true;

void maxShiftByte(uint8_t b) {
  for (int8_t i = 7; i >= 0; i--) {
    digitalWrite(PIN_MAX_DIN, (b >> i) & 1);
    digitalWrite(PIN_MAX_CLK, HIGH);
    digitalWrite(PIN_MAX_CLK, LOW);
  }
}

void maxWrite(uint8_t reg, uint8_t val) {
  digitalWrite(PIN_MAX_CS, LOW);
  maxShiftByte(reg);
  maxShiftByte(val);
  digitalWrite(PIN_MAX_CS, HIGH);  // rising edge latches the 16 bits
}

void maxInit() {
  maxWrite(0x0F, 0);                  // display test off
  maxWrite(0x09, 0);                  // no BCD decode
  maxWrite(0x0A, brightness);         // intensity
  maxWrite(0x0B, 7);                  // scan all 8 rows
  maxWrite(0x0C, 1);                  // normal operation
  fbForce = true;
}

uint8_t reverseBits(uint8_t b) {
  uint8_t r = 0;
  for (uint8_t i = 0; i < 8; i++) if (b & (1 << i)) r |= 0x80 >> i;
  return r;
}

void matrixFlush() {
  for (uint8_t r = 0; r < 8; r++) {
    uint8_t src = MATRIX_FLIP_Y ? fb[7 - r] : fb[r];
    uint8_t v = MATRIX_FLIP_X ? reverseBits(src) : src;
    if (fbForce || v != fbShown[r]) {
      maxWrite(r + 1, v);
      fbShown[r] = v;
    }
  }
  fbForce = false;
}

void fbClear() { memset(fb, 0, 8); }
void fbIcon(const uint8_t* icon) { memcpy_P(fb, icon, 8); }
void fbSet(int8_t x, int8_t y) { if (x >= 0 && x < 8 && y >= 0 && y < 8) fb[y] |= 0x80 >> x; }

const uint8_t ICON_HEART[8] PROGMEM = {0x00, 0x66, 0xFF, 0xFF, 0xFF, 0x7E, 0x3C, 0x18};
const uint8_t ICON_HEART_SMALL[8] PROGMEM = {0x00, 0x00, 0x24, 0x7E, 0x7E, 0x3C, 0x18, 0x00};
const uint8_t ICON_SKULL[8] PROGMEM = {0x3C, 0x7E, 0xDB, 0xDB, 0xFF, 0x66, 0x3C, 0x24};
const uint8_t ICON_STAIRS[8] PROGMEM = {0x03, 0x03, 0x0F, 0x0F, 0x3F, 0x3F, 0xFF, 0xFF};
const uint8_t ICON_PAUSE[8] PROGMEM = {0x00, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x00};

// 5x7 font for the title, column-major (bit 0 = top row)
const uint8_t FONT_C[5] PROGMEM = {0x3E, 0x41, 0x41, 0x41, 0x22};
const uint8_t FONT_R[5] PROGMEM = {0x7F, 0x09, 0x19, 0x29, 0x46};
const uint8_t FONT_Y[5] PROGMEM = {0x07, 0x08, 0x70, 0x08, 0x07};
const uint8_t FONT_P[5] PROGMEM = {0x7F, 0x09, 0x09, 0x09, 0x06};
const uint8_t FONT_T[5] PROGMEM = {0x01, 0x01, 0x7F, 0x01, 0x01};
const uint8_t FONT_SP[5] PROGMEM = {0, 0, 0, 0, 0};
const uint8_t* const TITLE[] = {FONT_C, FONT_R, FONT_Y, FONT_P, FONT_T, FONT_SP, FONT_SP};
const uint8_t TITLE_LEN = 7;
const uint8_t TITLE_COLS = TITLE_LEN * 6;

uint8_t titleColumn(uint8_t c) {
  c %= TITLE_COLS;
  uint8_t ch = c / 6, col = c % 6;
  if (col == 5) return 0;  // gap between letters
  return pgm_read_byte(TITLE[ch] + col);
}

// --- animation engine
// overlay animations (plain constants keep the Arduino IDE's auto-prototypes happy)
const uint8_t OV_NONE = 0, OV_HIT = 1, OV_LVL = 2, OV_STAIRS = 3;
uint8_t overlay = OV_NONE;
unsigned long overlayStart = 0;
unsigned long lastAnimMs = 0;
uint16_t animTick = 0;

void startOverlay(uint8_t o) { overlay = o; overlayStart = millis(); }

void drawTitleScroll() {
  fbClear();
  uint8_t start = animTick % TITLE_COLS;
  for (uint8_t x = 0; x < 8; x++) {
    uint8_t col = titleColumn(start + x);
    for (uint8_t y = 0; y < 7; y++) if (col & (1 << y)) fbSet(x, y);
  }
}

void drawWaiting() {  // a dot circling the border, like a radar
  fbClear();
  uint8_t p = animTick % 28;
  int8_t x, y;
  if (p < 7) { x = p; y = 0; }
  else if (p < 14) { x = 7; y = p - 7; }
  else if (p < 21) { x = 7 - (p - 14); y = 7; }
  else { x = 0; y = 7 - (p - 21); }
  fbSet(x, y);
  fbSet(3, 3); fbSet(4, 4);
  if ((animTick / 4) % 2) { fbSet(3, 4); fbSet(4, 3); }
}

void drawTorch() {  // idle in-game: flickering torch flame
  fbClear();
  fb[7] = 0x18; fb[6] = 0x18; fb[5] = 0x3C;
  fb[4] = 0x3C | (random(2) ? 0x40 : 0x02);
  fb[3] = 0x18 | (random(2) ? 0x24 : 0x00);
  fb[2] = random(3) ? 0x18 : 0x10;
  fb[1] = random(2) ? 0x10 : 0x08;
  if (random(4) == 0) fb[0] = 0x08;
}

void drawFight() {  // the Keeper fight: a heart dodging inside the battle box
  const uint8_t heart[4] = {0x24, 0x3C, 0x3C, 0x18};  // narrow, so a 1-column sway stays inside
  int8_t sway = ((animTick / 3) % 4) - 1;  // -1, 0, 1, 2 -> clamp below
  if (sway > 1) sway = 0;
  fb[0] = 0xFF; fb[7] = 0xFF; fb[1] = 0x81; fb[6] = 0x81;
  for (uint8_t r = 0; r < 4; r++) {
    uint8_t h = sway < 0 ? heart[r] << 1 : (sway > 0 ? heart[r] >> 1 : heart[r]);
    fb[r + 2] = 0x81 | (h & 0x7E);
  }
}

void drawLowHp() {  // heartbeat when HP is low
  if ((animTick % 8) < 2) fbIcon(ICON_HEART); else fbIcon(ICON_HEART_SMALL);
}

void drawBurst(unsigned long t) {  // expanding rings for level-up
  fbClear();
  uint8_t r = (t / 90) % 5;
  for (int8_t i = 0; i < 8; i++) {
    for (int8_t j = 0; j < 8; j++) {
      int8_t dx = i * 2 - 7, dy = j * 2 - 7;  // centre at 3.5,3.5 (doubled)
      int d = (abs(dx) > abs(dy) ? abs(dx) : abs(dy)) / 2;  // square ring
      if (d == r || (d == r - 2 && r >= 2)) fbSet(i, j);
    }
  }
}

// ------------------------------------------------------------------ live layout
// Screens: W waiting (no game), M title, P playing, F Keeper fight, X paused, D overwhelmed.
// Matrix slots: the six screens, then L low health, H hit, V right answer / level-up, S stairs.
const char SCREENS[] = "WMPFXD";
const char SLOTS[] = "WMPFXDLHVS";
const uint8_t N_SCREENS = 6, N_SLOTS = 10, TPL_LEN = 33, MAX_FRAMES = 4;
const uint8_t SLOT_LOW = 6, SLOT_HIT = 7, SLOT_LVL = 8, SLOT_STAIRS = 9;

struct MatrixSlot {
  uint8_t frames;    // 0 = the built-in animation
  uint16_t frameMs;  // time per frame
  uint8_t px[MAX_FRAMES][8];  // row 0 = top, bit 7 = left
};

char lcdTpl[N_SCREENS][2][TPL_LEN];
MatrixSlot slots[N_SLOTS];

const char DEFAULT_TPL[N_SCREENS][2][TPL_LEN] PROGMEM = {
  {"CAMPUS CRYPT", "waiting{dots}"},
  {"CAMPUS CRYPT", "Press Enter"},
  {"HP {hp}/{max} {bar}", "Kn{kn} F{fl}/{fls}"},
  {"* INBOX MIMIC *", "F{fl}/{fls} FIGHT"},
  {"HP {hp}/{max} {bar}", "PAUSE F{fl}/{fls}"},
  {"OVERWHELMED x_x", "F{fl}/{fls}"},
};

void layoutDefaults() {
  memcpy_P(lcdTpl, DEFAULT_TPL, sizeof(lcdTpl));
  memset(slots, 0, sizeof(slots));
  brightness = MATRIX_BRIGHTNESS;
}

int8_t indexOf(const char* set, char c) {
  const char* p = c ? strchr(set, c) : NULL;
  return p ? (int8_t)(p - set) : -1;
}

// LPREV:<c> shows one screen for 15 s (to try a layout with no game running)
char previewMode = 0;
unsigned long previewUntil = 0;

// the screen to draw: a preview, else the game's mode, else W (waiting)
char screenMode() {
  if (previewMode && millis() < previewUntil) return previewMode;
  previewMode = 0;
  return gameConnected() ? gs.mode : 'W';
}

// --- EEPROM: bytes 0-2 = 'C' 'L' version, then brightness, the LCD templates, the matrix slots
const uint8_t EE_VERSION = 1;
const uint16_t EE_BASE = 4;
const uint16_t EE_SIZE = 1 + sizeof(lcdTpl) + sizeof(slots);
int16_t savePos = -1;  // next byte LSAVE writes; -1 = not saving

uint8_t* layoutByte(uint16_t i) {
  if (i == 0) return &brightness;
  i--;
  if (i < sizeof(lcdTpl)) return (uint8_t*)lcdTpl + i;
  return (uint8_t*)slots + (i - sizeof(lcdTpl));
}

void layoutLoad() {
  layoutDefaults();
  if (EEPROM.read(0) != 'C' || EEPROM.read(1) != 'L' || EEPROM.read(2) != EE_VERSION) return;
  for (uint16_t i = 0; i < EE_SIZE; i++) *layoutByte(i) = EEPROM.read(EE_BASE + i);
  // never trust stored bytes blindly
  if (brightness > 15) brightness = MATRIX_BRIGHTNESS;
  for (uint8_t s = 0; s < N_SCREENS; s++)
    for (uint8_t r = 0; r < 2; r++) lcdTpl[s][r][TPL_LEN - 1] = 0;
  for (uint8_t i = 0; i < N_SLOTS; i++)
    if (slots[i].frames > MAX_FRAMES) slots[i].frames = 0;
}

// One byte per loop pass (an EEPROM write takes ~3.3 ms), so the heartbeat never stalls.
void savePump() {
  if (savePos < 0) return;
  EEPROM.update(EE_BASE + savePos, *layoutByte(savePos));
  if (++savePos >= (int16_t)EE_SIZE) {
    EEPROM.update(1, 'L');
    EEPROM.update(2, EE_VERSION);
    EEPROM.update(0, 'C');  // written last: a half-done save is never loaded
    savePos = -1;
    Serial.println(F("LOK:SAVED"));
  }
}

int8_t hexVal(char c) {
  if (c >= '0' && c <= '9') return c - '0';
  if (c >= 'a' && c <= 'f') return c - 'a' + 10;
  if (c >= 'A' && c <= 'F') return c - 'A' + 10;
  return -1;
}

bool parseHex8(const char* p, uint8_t* out) {
  for (uint8_t i = 0; i < 8; i++) {
    int8_t hi = hexVal(p[i * 2]), lo = hi < 0 ? -1 : hexVal(p[i * 2 + 1]);
    if (hi < 0 || lo < 0) return false;
    out[i] = (hi << 4) | lo;
  }
  return true;
}

// LT:<screen><row>:<text>          an LCD line template (up to 32 chars, see expand())
// LM:<slot>:<ms>[:<hex16>[,...]]   a matrix picture, up to 4 frames of 16 hex digits; no frames = built-in
// LB:<0-15>                        matrix brightness
// LPREV:<screen or slot>           show a screen for 15 s, or play an event (H, V, S)
// LSAVE / LRESET                   keep the layout in EEPROM / go back to the built-in one
// Each answers LOK (LSAVE answers LOK:SAVED once written) or LERR.
void handleLayout(char* line) {
  if (strncmp(line, "LT:", 3) == 0) {
    int8_t s = indexOf(SCREENS, line[3]);
    uint8_t r = line[4] - '0';
    if (s < 0 || r > 1 || line[5] != ':') { Serial.println(F("LERR")); return; }
    strncpy(lcdTpl[s][r], line + 6, TPL_LEN - 1);
    lcdTpl[s][r][TPL_LEN - 1] = 0;
  } else if (strncmp(line, "LM:", 3) == 0) {
    int8_t i = indexOf(SLOTS, line[3]);
    if (i < 0 || line[4] != ':') { Serial.println(F("LERR")); return; }
    MatrixSlot m;
    memset(&m, 0, sizeof(m));
    char* p = line + 5;
    m.frameMs = constrain(atol(p), 30, 5000);
    p = strchr(p, ':');
    while (p && m.frames < MAX_FRAMES) {
      p++;
      if (!parseHex8(p, m.px[m.frames])) { Serial.println(F("LERR")); return; }
      m.frames++;
      p = strchr(p, ',');
    }
    slots[i] = m;
  } else if (strncmp(line, "LB:", 3) == 0) {
    brightness = constrain(atoi(line + 3), 0, 15);
    maxInit();
  } else if (strncmp(line, "LPREV:", 6) == 0) {
    char c = line[6];
    if (c == 'H') startOverlay(OV_HIT);
    else if (c == 'V') startOverlay(OV_LVL);
    else if (c == 'S') startOverlay(OV_STAIRS);
    else if (indexOf(SCREENS, c) >= 0 || c == 'L') { previewMode = c; previewUntil = millis() + 15000; }
    else { Serial.println(F("LERR")); return; }
  } else if (strcmp(line, "LSAVE") == 0) {
    EEPROM.update(0, 0);  // invalid until the save finishes
    savePos = 0;
    return;               // LOK:SAVED comes from savePump
  } else if (strcmp(line, "LRESET") == 0) {
    savePos = -1;
    EEPROM.update(0, 0);
    layoutDefaults();
    maxInit();
  } else {
    Serial.println(F("LERR"));
    return;
  }
  Serial.println(F("LOK"));
}

// a custom picture for this slot, if one was sent; t = ms into the animation
bool drawSlot(uint8_t i, unsigned long t) {
  const MatrixSlot& m = slots[i];
  if (!m.frames) return false;
  uint16_t ms = m.frameMs ? m.frameMs : 200;
  memcpy(fb, m.px[(t / ms) % m.frames], 8);
  return true;
}

void matrixUpdate() {
  unsigned long now = millis();
  if (now - lastAnimMs < 90) return;
  lastAnimMs = now;
  animTick++;
  unsigned long t = now - overlayStart;
  char mode = screenMode();

  if (overlay == OV_HIT && t < 700) {
    if (!drawSlot(SLOT_HIT, t)) { if ((t / 120) % 2 == 0) fbIcon(ICON_HEART); else fbClear(); }
  } else if (overlay == OV_LVL && t < 1400) {
    if (!drawSlot(SLOT_LVL, t)) drawBurst(t);
  } else if (overlay == OV_STAIRS && t < 1500) {
    if (!drawSlot(SLOT_STAIRS, t)) {
      fbIcon(ICON_STAIRS);
      if ((t / 250) % 2) for (uint8_t r = 0; r < 8; r++) fb[r] = (fb[r] << 1) | (fb[r] >> 7);
    }
  } else {
    overlay = OV_NONE;
    bool low = mode == 'L' || (mode == 'P' && gs.maxhp > 0 && gs.hp * 4 <= gs.maxhp);
    int8_t slot = low ? SLOT_LOW : indexOf(SCREENS, mode);
    if (slot < 0) slot = 2;
    if (!drawSlot(slot, now)) {
      if (slot == 0) drawWaiting();
      else if (slot == 1) drawTitleScroll();
      else if (slot == 5) fbIcon(ICON_SKULL);
      else if (slot == 4) fbIcon(ICON_PAUSE);
      else if (slot == 3) drawFight();
      else if (slot == SLOT_LOW) drawLowHp();
      else drawTorch();
    }
  }
  matrixFlush();
}


// ------------------------------------------------------------------ LCD
// Custom characters 0..5 = bar cells with 0..5 columns lit.
char lcdWant[2][17];
char lcdShown[2][17];

void lcdMakeBarChars() {
  for (uint8_t n = 0; n <= 5; n++) {
    uint8_t row = 0;
    for (uint8_t c = 0; c < n; c++) row |= 0x10 >> c;
    uint8_t ch[8];
    ch[0] = 0x1F; ch[7] = 0x1F;  // frame top/bottom
    for (uint8_t i = 1; i < 7; i++) ch[i] = row | 0x01 | 0x10;  // frame sides
    if (n == 0) for (uint8_t i = 1; i < 7; i++) ch[i] = 0x11;
    lcd.createChar(n, ch);
  }
}

void lcdSetLine(uint8_t line, const char* text) {
  uint8_t i = 0;
  for (; i < 16 && text[i]; i++) lcdWant[line][i] = text[i];
  for (; i < 16; i++) lcdWant[line][i] = ' ';
  lcdWant[line][16] = 0;
}

// Write at most a few changed characters per call so the LCD never blocks
// the serial port for long.
void lcdPump() {
  uint8_t budget = 3;
  for (uint8_t l = 0; l < 2 && budget; l++) {
    for (uint8_t c = 0; c < 16 && budget; c++) {
      if (lcdWant[l][c] != lcdShown[l][c]) {
        lcd.setCursor(c, l);
        lcd.write((uint8_t)lcdWant[l][c]);
        lcdShown[l][c] = lcdWant[l][c];
        budget--;
      }
    }
  }
}

uint8_t putNum(char* out, uint8_t pos, uint8_t limit, long v) {
  char n[12];
  ltoa(v, n, 10);
  for (uint8_t i = 0; n[i] && pos < limit; i++) out[pos++] = n[i];
  return pos;
}

// health bar cells (custom characters 0..5)
uint8_t putBar(char* out, uint8_t pos, uint8_t limit, uint8_t cells) {
  int maxhp = gs.maxhp > 0 ? gs.maxhp : 1;
  long units = (long)constrain(gs.hp, 0, maxhp) * cells * 5 / maxhp;
  for (uint8_t c = 0; c < cells && pos < limit; c++) {
    long u = units - c * 5;
    out[pos++] = (char)constrain(u, 0, 5);
  }
  return pos;
}

// Expand a template into at most `limit` LCD cells; returns how many were written.
// Fields: {hp} {max} {kn} {fl} {fls} {focus} {kills} {dots}, {bar} (health bar filling
// the line up to whatever follows it) or {barN} (N cells). Anything else is shown as typed.
uint8_t expand(const char* t, char* out, uint8_t limit) {
  uint8_t pos = 0;
  while (*t && pos < limit) {
    if (*t == '{') {
      const char* e = strchr(t, '}');
      if (e && e - t <= 7) {
        char k[8];
        uint8_t n = e - t - 1;
        memcpy(k, t + 1, n);
        k[n] = 0;
        t = e + 1;
        if (!strcmp(k, "hp")) pos = putNum(out, pos, limit, gs.hp);
        else if (!strcmp(k, "max")) pos = putNum(out, pos, limit, gs.maxhp);
        else if (!strcmp(k, "kn")) pos = putNum(out, pos, limit, gs.lvl);
        else if (!strcmp(k, "fl")) pos = putNum(out, pos, limit, gs.floorNo);
        else if (!strcmp(k, "fls")) pos = putNum(out, pos, limit, gs.floors);
        else if (!strcmp(k, "focus")) pos = putNum(out, pos, limit, gs.gold);
        else if (!strcmp(k, "kills")) pos = putNum(out, pos, limit, gs.kills);
        else if (!strcmp(k, "dots")) {
          for (uint8_t d = (millis() / 500) % 4; d && pos < limit; d--) out[pos++] = '.';
        } else if (!strncmp(k, "bar", 3)) {
          uint8_t w;
          if (k[3]) {
            w = atoi(k + 3);
          } else {
            char tail[16];
            w = limit - pos - expand(t, tail, limit - pos);
          }
          pos = putBar(out, pos, limit, w);
        } else {
          out[pos++] = '?';  // unknown field
        }
        continue;
      }
    }
    out[pos++] = *t++;
  }
  return pos;
}

void buildLcd() {
  char mode = screenMode();
  int8_t s = indexOf(SCREENS, mode == 'L' ? 'P' : mode);
  if (s < 0) s = 2;
  for (uint8_t r = 0; r < 2; r++) {
    char out[16];
    uint8_t n = expand(lcdTpl[s][r], out, 16);
    for (uint8_t i = 0; i < 16; i++) lcdWant[r][i] = i < n ? out[i] : ' ';  // may contain char 0
  }
}


// ------------------------------------------------------------------ serial protocol
char rxBuf[96];  // long enough for a 4-frame LM: line
uint8_t rxLen = 0;
unsigned long lastHelloMs = 0;
unsigned long lastBeatMs = 0;
unsigned long lastMaintMs = 0;

void sayHello() { Serial.println(F("HELLO:CRYPT")); lastHelloMs = millis(); }

void handleLine(char* line) {
  if (strncmp(line, "S:", 2) == 0) {
    // S:hp,maxhp,lvl,floor,gold,kills,mode[,disp[,floors]]
    long v[6] = {0, 0, 0, 0, 0, 0};
    char* p = line + 2;
    for (uint8_t i = 0; i < 6; i++) {
      v[i] = atol(p);
      p = strchr(p, ',');
      if (!p) return;  // malformed
      p++;
    }
    gs.hp = v[0]; gs.maxhp = v[1]; gs.lvl = v[2]; gs.floorNo = v[3];
    gs.gold = v[4]; gs.kills = v[5];
    gs.mode = *p ? *p : 'P';
    char* d = strchr(p, ',');
    gs.disp = (d && d[1] && d[1] != ',') ? d[1] : 'F';
    char* f = d ? strchr(d + 1, ',') : NULL;
    gs.floors = f ? constrain(atoi(f + 1), 1, 99) : 10;
    lastStateMs = millis();
  } else if (strcmp(line, "E:HIT") == 0) {
    startOverlay(OV_HIT);
  } else if (strcmp(line, "E:LVL") == 0) {
    startOverlay(OV_LVL);
  } else if (strcmp(line, "E:STAIRS") == 0) {
    startOverlay(OV_STAIRS);
  } else if (strcmp(line, "E:DIE") == 0) {
    overlay = OV_NONE;
    gs.mode = 'D';
  } else if (strcmp(line, "E:MENU") == 0) {
    overlay = OV_NONE;
    gs.mode = 'M';
    animTick = 0;
  } else if (strcmp(line, "PING") == 0) {
    sayHello();
  } else if (line[0] == 'L') {
    handleLayout(line);
  }
}

void serialPump() {
  while (Serial.available()) {
    char c = Serial.read();
    if (c == '\r') continue;
    if (c == '\n') {
      rxBuf[rxLen] = 0;
      if (rxLen) handleLine(rxBuf);
      rxLen = 0;
    } else if (rxLen < sizeof(rxBuf) - 1) {
      rxBuf[rxLen++] = c;
    } else {
      rxLen = 0;  // line too long: drop it
    }
  }
}

// ------------------------------------------------------------------ setup / loop
void setup() {
  Serial.begin(115200);
  pinMode(PIN_MAX_DIN, OUTPUT);
  pinMode(PIN_MAX_CLK, OUTPUT);
  pinMode(PIN_MAX_CS, OUTPUT);
  digitalWrite(PIN_MAX_CS, HIGH);

  lcd.begin(16, 2);
  lcdMakeBarChars();
  lcd.clear();
  memset(lcdShown, ' ', sizeof(lcdShown));
  memset(lcdWant, ' ', sizeof(lcdWant));

  layoutLoad();
  maxInit();
  randomSeed(micros());
  sayHello();
}

void loop() {
  serialPump();

  unsigned long now = millis();
  // the game drops a board that is silent for 2 s: beat every 0.5 s while it talks to us
  if (gameConnected() && now - lastBeatMs >= 500) {
    lastBeatMs = now;
    Serial.println(F("HB"));
  }
  if (now - lastMaintMs > 500) {
    lastMaintMs = now;
    // re-send the hello now and then while no game is talking to us
    if (!gameConnected() && now - lastHelloMs > 2000) sayHello();
    // the MAX7219 can glitch on power dips; refresh its setup every so often
    static uint8_t reinit = 0;
    if (++reinit >= 20) { reinit = 0; maxInit(); }
  }

  savePump();
  matrixUpdate();
  buildLcd();
  lcdPump();
}
