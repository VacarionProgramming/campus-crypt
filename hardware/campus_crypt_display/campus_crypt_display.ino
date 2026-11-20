/*
  Campus Crypt status display  -  ELEGOO Mega 2560 starter-kit build (output only)
  -------------------------------------------------------------------------------
  Mirrors the Campus Crypt game over USB serial (115200 baud, one ASCII line per
  message) on:
    * LCD1602 (parallel, 4-bit mode)      - HP bar, Knowledge, floor of 10, clock
    * bare 4-digit 7-segment (5461AS, common cathode, 12 pins), driven directly
                                          - floor ("F  3"), "5:00" in the Keeper fight
    * MAX7219 8x8 LED matrix module       - CRYPT title scroll, torch, fight icon,
                                            hit / level-up / stairs / skull
    * DS1307 RTC module (I2C)             - the clock when no game is running

  OUTPUT ONLY: no joystick or buttons. The keyboard is the only control. The sketch
  never reads an input pin (so nothing floating is ever read) and never sends J:
  lines; it sends HELLO:CRYPT while waiting and a HB heartbeat every 0.5 s.

  Libraries: only LiquidCrystal and Wire, which ship with the Arduino IDE.
  The MAX7219 and DS1307 are driven directly by this sketch.

  Pin map (Arduino Mega 2560; see hardware/WIRING.md). Nothing is shared:
    D0/D1    USB serial (do not use)
    D2       MAX7219 DIN
    D3       MAX7219 CLK
    D4       MAX7219 CS (LOAD)
    D7       LCD RS (pin 4)
    D8       LCD E  (pin 6)
    D9-D12   LCD D4-D7 (pins 11-14)
    D20/D21  DS1307 SDA / SCL  (the Mega's I2C pins; A4/A5 are NOT I2C on a Mega)
    D22-D29  7-seg segments A,B,C,D,E,F,G,DP through 1 kOhm resistors
    D30-D33  7-seg digits 1-4 (common cathodes, LOW = on)
*/

#include <LiquidCrystal.h>
#include <Wire.h>

// ------------------------------------------------------------------ pins
const uint8_t PIN_MAX_DIN = 2;
const uint8_t PIN_MAX_CLK = 3;
const uint8_t PIN_MAX_CS = 4;
const uint8_t PIN_LCD_RS = 7;
const uint8_t PIN_LCD_E = 8;
const uint8_t PIN_SEG[8] = {22, 23, 24, 25, 26, 27, 28, 29};  // A,B,C,D,E,F,G,DP
const uint8_t PIN_DIGIT[4] = {30, 31, 32, 33};                // digit 1..4 (left to right)

// ------------------------------------------------------------------ options
#define MATRIX_FLIP_X 0      // set to 1 if matrix images look mirrored
#define MATRIX_FLIP_Y 0      // set to 1 if matrix images look upside down
#define MATRIX_BRIGHTNESS 3  // 0..15
const uint8_t RTC_ADDR = 0x68;

LiquidCrystal lcd(PIN_LCD_RS, PIN_LCD_E, 9, 10, 11, 12);

// ------------------------------------------------------------------ game state
struct GameState {
  int hp = 0, maxhp = 5, lvl = 0, floorNo = 1, floors = 10;
  long gold = 0, kills = 0;
  char mode = 'M';   // M menu, P playing, F Keeper fight, X paused, D overwhelmed
  char disp = 'F';   // F floor, G focus ("gold"), K kills on the 4-digit display
};
GameState gs;
unsigned long lastStateMs = 0;   // when the last S: line arrived
bool gameConnected() { return lastStateMs != 0 && millis() - lastStateMs < 3000; }

// ------------------------------------------------------------------ clock
uint8_t clkH = 12, clkM = 0, clkS = 0;
bool rtcOk = false;
unsigned long lastRtcRead = 0;

uint8_t bcd2dec(uint8_t v) { return (v >> 4) * 10 + (v & 0x0F); }
uint8_t dec2bcd(uint8_t v) { return ((v / 10) << 4) | (v % 10); }

// Set the DS1307 from the time this sketch was compiled.
void rtcSetFromCompileTime() {
  const char* t = __TIME__;  // "hh:mm:ss"
  const char* d = __DATE__;  // "Mmm dd yyyy"
  uint8_t hh = (t[0] - '0') * 10 + (t[1] - '0');
  uint8_t mm = (t[3] - '0') * 10 + (t[4] - '0');
  uint8_t ss = (t[6] - '0') * 10 + (t[7] - '0');
  const char* months = "JanFebMarAprMayJunJulAugSepOctNovDec";
  uint8_t mon = 1;
  for (uint8_t i = 0; i < 12; i++) {
    if (strncmp(d, months + i * 3, 3) == 0) mon = i + 1;
  }
  uint8_t day = (d[4] == ' ' ? 0 : (d[4] - '0') * 10) + (d[5] - '0');
  uint8_t yr = (d[9] - '0') * 10 + (d[10] - '0');
  Wire.beginTransmission(RTC_ADDR);
  Wire.write((uint8_t)0);
  Wire.write(dec2bcd(ss));  // bit 7 = 0 -> oscillator running
  Wire.write(dec2bcd(mm));
  Wire.write(dec2bcd(hh));  // 24-hour mode
  Wire.write((uint8_t)1);
  Wire.write(dec2bcd(day));
  Wire.write(dec2bcd(mon));
  Wire.write(dec2bcd(yr));
  Wire.endTransmission();
}

void rtcBegin() {
  Wire.begin();
  Wire.beginTransmission(RTC_ADDR);
  Wire.write((uint8_t)0);
  rtcOk = (Wire.endTransmission() == 0);
  if (!rtcOk) return;
  Wire.requestFrom(RTC_ADDR, (uint8_t)1);
  uint8_t sec = Wire.available() ? Wire.read() : 0x80;
  if (sec & 0x80) rtcSetFromCompileTime();  // clock-halt bit set: not running
}

void rtcRead() {
  if (!rtcOk) {
    // no RTC found: keep a rough clock from millis()
    unsigned long s = millis() / 1000;
    clkH = (12 + s / 3600) % 24; clkM = (s / 60) % 60; clkS = s % 60;
    return;
  }
  Wire.beginTransmission(RTC_ADDR);
  Wire.write((uint8_t)0);
  if (Wire.endTransmission() != 0) return;
  Wire.requestFrom(RTC_ADDR, (uint8_t)3);
  if (Wire.available() < 3) return;
  clkS = bcd2dec(Wire.read() & 0x7F);
  clkM = bcd2dec(Wire.read() & 0x7F);
  uint8_t h = Wire.read();
  if (h & 0x40) {  // 12-hour mode
    uint8_t h12 = bcd2dec(h & 0x1F);
    bool pm = h & 0x20;
    clkH = (h12 % 12) + (pm ? 12 : 0);
  } else {
    clkH = bcd2dec(h & 0x3F);
  }
}

// ------------------------------------------------------------------ 7-segment (direct drive)
// bit 0..7 = segments A,B,C,D,E,F,G,DP
const uint8_t SEG_DIGITS[10] = {
  0b00111111, 0b00000110, 0b01011011, 0b01001111, 0b01100110,
  0b01101101, 0b01111101, 0b00000111, 0b01111111, 0b01101111};
const uint8_t SEG_DASH = 0b01000000;
const uint8_t SEG_DP = 0b10000000;
const uint8_t SEG_F = 0b01110001;
uint8_t segBuf[4] = {SEG_DASH, SEG_DASH, SEG_DASH, SEG_DASH};
uint8_t curDigit = 0;
unsigned long lastDigitUs = 0;

void sevenSegShowNumber(long n) {
  if (n < 0) n = 0;
  if (n > 9999) n = 9999;
  for (int8_t i = 3; i >= 0; i--) {
    segBuf[i] = SEG_DIGITS[n % 10];
    n /= 10;
    if (n == 0) {  // blank leading zeros
      for (int8_t j = i - 1; j >= 0; j--) segBuf[j] = 0;
      break;
    }
  }
}

void sevenSegShowFloor(int f) {  // "F  3" / "F 10"
  sevenSegShowNumber(constrain(f, 0, 99));
  segBuf[0] = SEG_F;
}

void sevenSegShowTime(uint8_t h, uint8_t m, bool colonOn) {
  segBuf[0] = h >= 10 ? SEG_DIGITS[h / 10] : 0;
  segBuf[1] = SEG_DIGITS[h % 10] | (colonOn ? SEG_DP : 0);
  segBuf[2] = SEG_DIGITS[m / 10];
  segBuf[3] = SEG_DIGITS[m % 10];
}

// Called every loop; moves to the next digit every 2 ms (non-blocking).
void sevenSegRefresh() {
  unsigned long now = micros();
  if (now - lastDigitUs < 2000) return;
  lastDigitUs = now;
  for (uint8_t d = 0; d < 4; d++) digitalWrite(PIN_DIGIT[d], HIGH);  // all off
  curDigit = (curDigit + 1) & 3;
  uint8_t b = segBuf[curDigit];
  for (uint8_t i = 0; i < 8; i++) digitalWrite(PIN_SEG[i], (b >> i) & 1);
  digitalWrite(PIN_DIGIT[curDigit], LOW);  // common cathode: LOW = on
}

// ------------------------------------------------------------------ MAX7219 8x8 matrix
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
  maxWrite(0x0A, MATRIX_BRIGHTNESS);  // intensity
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
// Hoot the owl: ears, round face with two eyes (the gaps), beak, feet. The blink fills the eyes in.
const uint8_t ICON_OWL[8] PROGMEM = {0x42, 0x7E, 0xDB, 0xDB, 0x7E, 0x3C, 0x18, 0x66};
const uint8_t ICON_OWL_BLINK[8] PROGMEM = {0x42, 0x7E, 0xFF, 0xFF, 0x7E, 0x3C, 0x18, 0x66};

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
const uint8_t OV_NONE = 0, OV_HIT = 1, OV_LVL = 2, OV_STAIRS = 3, OV_TIP = 4;
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

void matrixUpdate() {
  unsigned long now = millis();
  if (now - lastAnimMs < 90) return;
  lastAnimMs = now;
  animTick++;
  unsigned long t = now - overlayStart;
  bool connected = gameConnected();

  if (overlay == OV_HIT && t < 700) {
    if ((t / 120) % 2 == 0) fbIcon(ICON_HEART); else fbClear();
  } else if (overlay == OV_LVL && t < 1400) {
    drawBurst(t);
  } else if (overlay == OV_TIP && t < 2800) {
    // Hoot has a tip: the owl watches you, blinks twice, and its ears twitch
    bool blink = (t % 900) < 150;
    fbIcon(blink ? ICON_OWL_BLINK : ICON_OWL);
    if ((t / 300) % 4 == 3) fb[0] = 0x24;
  } else if (overlay == OV_STAIRS && t < 1500) {
    fbIcon(ICON_STAIRS);
    if ((t / 250) % 2) for (uint8_t r = 0; r < 8; r++) fb[r] = (fb[r] << 1) | (fb[r] >> 7);
  } else {
    overlay = OV_NONE;
    if (!connected) drawWaiting();
    else if (gs.mode == 'M') drawTitleScroll();
    else if (gs.mode == 'D') fbIcon(ICON_SKULL);
    else if (gs.mode == 'X') fbIcon(ICON_PAUSE);
    else if (gs.mode == 'F') drawFight();
    else if (gs.maxhp > 0 && gs.hp * 4 <= gs.maxhp) drawLowHp();
    else drawTorch();
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
// the display multiplexing or the serial port for long.
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

// pad `b` to 11 characters and put the clock in the last 5 (16 total)
void withClock(char* b, const char* clock) {
  uint8_t l = strlen(b);
  if (l > 11) l = 11;
  while (l < 11) b[l++] = ' ';
  b[l] = 0;
  strcat(b, clock);
}

void hpBar() {  // "HP 4/5" followed by a bar using the custom characters
  char a[17];
  int len = snprintf(a, sizeof(a), "HP %d/%d", gs.hp, gs.maxhp);
  if (len < 0) len = 0;
  if (len > 12) len = 12;
  a[len++] = ' ';
  int8_t cells = 16 - len;
  int maxhp = gs.maxhp > 0 ? gs.maxhp : 1;
  long units = (long)constrain(gs.hp, 0, maxhp) * cells * 5 / maxhp;
  for (int8_t c = 0; c < cells; c++) {
    long u = units - c * 5;
    a[len + c] = (char)constrain(u, 0, 5);
  }
  for (uint8_t i = 0; i < 16; i++) lcdWant[0][i] = a[i];  // may contain char 0
}

void buildLcd() {
  char b[24];
  char clock[6];
  snprintf(clock, sizeof(clock), "%02u:%02u", clkH, clkM);
  if (!gameConnected()) {
    lcdSetLine(0, "CAMPUS CRYPT");
    bool dots = (millis() / 500) % 4;
    snprintf(b, sizeof(b), "waiting%s", dots ? "..." : "");
    withClock(b, clock);
    lcdSetLine(1, b);
    return;
  }
  switch (gs.mode) {
    case 'M':
      lcdSetLine(0, "CAMPUS CRYPT");
      snprintf(b, sizeof(b), "Press Enter");
      break;
    case 'D':
      lcdSetLine(0, "OVERWHELMED x_x");
      snprintf(b, sizeof(b), "F%d/%d", gs.floorNo, gs.floors);
      break;
    case 'F':
      lcdSetLine(0, "* INBOX MIMIC *");
      snprintf(b, sizeof(b), "F%d/%d FIGHT", gs.floorNo, gs.floors);
      break;
    case 'X':
      hpBar();
      snprintf(b, sizeof(b), "PAUSE F%d/%d", gs.floorNo, gs.floors);
      break;
    default:
      hpBar();
      snprintf(b, sizeof(b), "Kn%d F%d/%d", gs.lvl, gs.floorNo, gs.floors);
  }
  withClock(b, clock);
  lcdSetLine(1, b);
}

// ------------------------------------------------------------------ serial protocol
char rxBuf[64];
uint8_t rxLen = 0;
unsigned long lastHelloMs = 0;
unsigned long lastBeatMs = 0;

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
  } else if (strcmp(line, "E:TIP") == 0) {
    startOverlay(OV_TIP);
  } else if (strcmp(line, "E:DIE") == 0) {
    overlay = OV_NONE;
    gs.mode = 'D';
  } else if (strcmp(line, "E:MENU") == 0) {
    overlay = OV_NONE;
    gs.mode = 'M';
    animTick = 0;
  } else if (strcmp(line, "PING") == 0) {
    sayHello();
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
  for (uint8_t i = 0; i < 8; i++) {
    pinMode(PIN_SEG[i], OUTPUT);
    digitalWrite(PIN_SEG[i], LOW);
  }
  for (uint8_t d = 0; d < 4; d++) {
    pinMode(PIN_DIGIT[d], OUTPUT);
    digitalWrite(PIN_DIGIT[d], HIGH);
  }

  lcd.begin(16, 2);
  lcdMakeBarChars();
  lcd.clear();
  memset(lcdShown, ' ', sizeof(lcdShown));
  memset(lcdWant, ' ', sizeof(lcdWant));

  maxInit();
  rtcBegin();
  rtcRead();
  randomSeed(micros());
  sayHello();
}

void loop() {
  serialPump();
  sevenSegRefresh();

  unsigned long now = millis();
  // the game drops a board that is silent for 2 s: beat every 0.5 s while it talks to us
  if (gameConnected() && now - lastBeatMs >= 500) {
    lastBeatMs = now;
    Serial.println(F("HB"));
  }
  if (now - lastRtcRead > 500) {
    lastRtcRead = now;
    rtcRead();
    // re-send the hello now and then while no game is talking to us
    if (!gameConnected() && now - lastHelloMs > 2000) sayHello();
    // the MAX7219 can glitch on power dips; refresh its setup every so often
    static uint8_t reinit = 0;
    if (++reinit >= 20) { reinit = 0; maxInit(); }
  }

  // 4-digit display content
  bool colon = (now / 500) % 2;
  if (!gameConnected() || gs.mode == 'M') sevenSegShowTime(clkH, clkM, colon);
  else if (gs.mode == 'F') sevenSegShowTime(5, 0, colon);  // the Mimic's "5:00 PM" deadline
  else if (gs.disp == 'K') sevenSegShowNumber(gs.kills);
  else if (gs.disp == 'G') sevenSegShowNumber(gs.gold);
  else sevenSegShowFloor(gs.floorNo);

  matrixUpdate();
  sevenSegRefresh();
  buildLcd();
  lcdPump();
}
