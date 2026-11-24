//! Daily Crypt: the New York date, the seed, the score checksum, and the
//! spoiler-free share card.
//!
//! The date is the calendar date in `America/New_York`. The rules embedded
//! here are the US Energy Policy Act rules in force since 2007: daylight time
//! starts at 02:00 EST on the second Sunday of March (07:00 UTC) and ends at
//! 02:00 EDT on the first Sunday of November (06:00 UTC). The offset is never
//! a fixed -5. The day rolls over at local midnight.
//!
//! `seed` is the first 8 bytes of SHA-256("campus-crypt:YYYY-MM-DD"), as a
//! little-endian u64. That u64 seeds the layout RNG. Keeper and investigation
//! picks take a u32, so they use the low 32 bits (the first 4 digest bytes).
//!
//! The checksum is integrity only. It is forgeable; the contract says so.

use sha2::{Digest, Sha256};

/// Shown when the board cannot take the run. The share card still works.
pub const BOARD_OFFLINE: &str = "Board offline - your run still counts on your share card";

pub const PLAY_URL: &str = "https://vacarionprogramming.github.io/campus-crypt/";

/// What the score body sends for the Pages build. Dey's allowlist must include it.
pub const BUILD_WEB: &str = "0.5.0-web";
/// Native builds do not post scores. This is only the label if one ever does.
pub const BUILD_NATIVE: &str = "0.5.0-dev";

pub fn build_id() -> &'static str {
    if cfg!(target_arch = "wasm32") {
        BUILD_WEB
    } else {
        BUILD_NATIVE
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct NyDate {
    pub year: i32,
    pub month: u8,
    pub day: u8,
}

impl NyDate {
    pub fn parse(s: &str) -> Option<NyDate> {
        if s.len() != 10 || s.as_bytes().get(4) != Some(&b'-') || s.as_bytes().get(7) != Some(&b'-')
        {
            return None;
        }
        let year: i32 = s[..4].parse().ok()?;
        let month: u8 = s[5..7].parse().ok()?;
        let day: u8 = s[8..10].parse().ok()?;
        if !(1..=12).contains(&month) || day == 0 || day > days_in_month(year, month) {
            return None;
        }
        Some(NyDate { year, month, day })
    }

    pub fn format(self) -> String {
        format!("{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }

    pub fn pred(self) -> NyDate {
        from_unix_days(to_unix_days(self) - 1)
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn succ(self) -> NyDate {
        from_unix_days(to_unix_days(self) + 1)
    }
}

impl std::fmt::Display for NyDate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.format())
    }
}

fn is_leap(year: i32) -> bool {
    year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)
}

fn days_in_month(year: i32, month: u8) -> u8 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if is_leap(year) {
                29
            } else {
                28
            }
        }
        _ => 0,
    }
}

/// Howard Hinnant's `days_from_civil` / `civil_from_days`. Day 0 is 1970-01-01.
fn to_unix_days(d: NyDate) -> i64 {
    let y = d.year as i64;
    let m = d.month as i64;
    let day = d.day as i64;
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = (y - era * 400) as u64;
    let mp = if m > 2 { m - 3 } else { m + 9 };
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy as u64;
    era * 146097 + doe as i64 - 719468
}

fn from_unix_days(days: i64) -> NyDate {
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u8;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    NyDate {
        year: y as i32,
        month: m as u8,
        day: d,
    }
}

/// Sunday = 0. 1970-01-01 was a Thursday.
fn weekday_sun0(days: i64) -> u8 {
    (days + 4).rem_euclid(7) as u8
}

/// The `n`th Sunday of `month` (1-based), as a day of the month.
fn nth_sunday(year: i32, month: u8, n: u8) -> u8 {
    let first = weekday_sun0(to_unix_days(NyDate {
        year,
        month,
        day: 1,
    }));
    let first_sunday = if first == 0 { 1 } else { 1 + (7 - first) };
    first_sunday + (n - 1) * 7
}

/// UTC instant when New York springs forward, and when it falls back, that year.
fn dst_bounds_utc(year: i32) -> (i64, i64) {
    let start_day = nth_sunday(year, 3, 2);
    let end_day = nth_sunday(year, 11, 1);
    let start = to_unix_days(NyDate {
        year,
        month: 3,
        day: start_day,
    }) * 86400
        + 7 * 3600;
    let end = to_unix_days(NyDate {
        year,
        month: 11,
        day: end_day,
    }) * 86400
        + 6 * 3600;
    (start, end)
}

/// True while New York is on EDT (UTC-4). The instant is Unix seconds.
pub fn is_edt(unix_secs: i64) -> bool {
    let days = unix_secs.div_euclid(86400);
    let year = from_unix_days(days).year;
    let (start, end) = dst_bounds_utc(year);
    unix_secs >= start && unix_secs < end
}

/// Calendar date in America/New_York for a Unix instant.
pub fn ny_date(unix_secs: i64) -> NyDate {
    let off = if is_edt(unix_secs) {
        -4 * 3600
    } else {
        -5 * 3600
    };
    from_unix_days((unix_secs + off).div_euclid(86400))
}

/// The local clock, as Unix seconds. Native uses the OS clock. The web build
/// uses the browser clock through miniquad (milliseconds).
pub fn unix_now() -> i64 {
    #[cfg(not(target_arch = "wasm32"))]
    {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0)
    }
    #[cfg(target_arch = "wasm32")]
    {
        (macroquad::miniquad::date::now() / 1000.0) as i64
    }
}

pub fn today_ny() -> NyDate {
    ny_date(unix_now())
}

/// `?daily=` is kept only when it is today or yesterday. Anything else plays today.
pub fn resolve_date(today: NyDate, query: Option<&str>) -> NyDate {
    match query.and_then(NyDate::parse) {
        Some(q) if q == today || q == today.pred() => q,
        _ => today,
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Seed {
    pub bytes: [u8; 8],
    pub hex: String,
    pub u64: u64,
}

impl Seed {
    /// Low 32 bits: the first 4 digest bytes, which is what `run_seed: u32` holds.
    pub fn run_seed(&self) -> u32 {
        self.u64 as u32
    }
}

pub fn seed_for(date: &str) -> Seed {
    let digest = Sha256::digest(format!("campus-crypt:{date}").as_bytes());
    let mut bytes = [0u8; 8];
    bytes.copy_from_slice(&digest[..8]);
    let hex = bytes.iter().map(|b| format!("{b:02x}")).collect();
    let u64 = u64::from_le_bytes(bytes);
    Seed { bytes, hex, u64 }
}

pub fn seed_from_hex(hex: &str) -> Option<Seed> {
    if hex.len() != 16 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let mut bytes = [0u8; 8];
    for i in 0..8 {
        bytes[i] = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).ok()?;
    }
    let hex = hex.to_ascii_lowercase();
    let u64 = u64::from_le_bytes(bytes);
    Some(Seed { bytes, hex, u64 })
}

/// What `GET /v1/daily/today` contributes. Unknown JSON fields are ignored by the caller.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Today {
    pub date: NyDate,
    pub seed: Seed,
}

/// Pick the date and seed for this launch.
///
/// Online, `server` is trusted for today's date and, when the run is that
/// date, for the seed. Offline, both come from the local New York clock.
/// A `?daily=` of today or yesterday selects that day; any other query is today.
pub fn choose(server: Option<&Today>, local_today: NyDate, query: Option<&str>) -> (NyDate, Seed) {
    let today = server.map(|s| s.date).unwrap_or(local_today);
    let date = resolve_date(today, query);
    let seed = match server {
        Some(s) if s.date == date => s.seed.clone(),
        _ => seed_for(&date.format()),
    };
    (date, seed)
}

/// 3-16 chars, letters, digits, underscore, space and hyphen. No leading or
/// trailing space, no double space. The server still runs its own profanity filter.
pub fn nickname_ok(s: &str) -> bool {
    let n = s.chars().count();
    if !(3..=16).contains(&n) || s.starts_with(' ') || s.ends_with(' ') || s.contains("  ") {
        return false;
    }
    s.chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == ' ' || c == '-')
}

/// Empty means no club. Otherwise 2-12 chars, letters, digits, underscore, hyphen.
pub fn club_ok(s: &str) -> bool {
    if s.is_empty() {
        return true;
    }
    let n = s.chars().count();
    (2..=12).contains(&n)
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// Clubs are stored uppercased. Empty stays empty.
pub fn club_sent(s: &str) -> String {
    s.trim().to_ascii_uppercase()
}

pub fn install_id_ok(s: &str) -> bool {
    let n = s.chars().count();
    (8..=64).contains(&n) && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

/// A fresh anonymous id. 32 hex chars, which matches `[A-Za-z0-9-]`.
pub fn fresh_install_id(unix_secs: i64, salt: u64) -> String {
    format!("{unix_secs:016x}{salt:016x}")
}

#[derive(Clone, Debug)]
pub struct ScoreParts<'a> {
    pub seed_hex: &'a str,
    pub nickname: &'a str,
    pub club: &'a str,
    pub floors_reached: u32,
    pub floor_count: u32,
    pub time_ms: u64,
    pub glyphs: &'a str,
    pub build: &'a str,
}

impl ScoreParts<'_> {
    /// Lowercase hex SHA-256 of the canonical pipe string. `club` is "" when none.
    pub fn checksum(&self) -> String {
        let raw = format!(
            "{}|{}|{}|{}|{}|{}|{}|{}",
            self.seed_hex,
            self.nickname,
            self.club,
            self.floors_reached,
            self.floor_count,
            self.time_ms,
            self.glyphs,
            self.build
        );
        hex_encode(&Sha256::digest(raw.as_bytes()))
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn canonical(&self) -> String {
        format!(
            "{}|{}|{}|{}|{}|{}|{}|{}",
            self.seed_hex,
            self.nickname,
            self.club,
            self.floors_reached,
            self.floor_count,
            self.time_ms,
            self.glyphs,
            self.build
        )
    }
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Glyphs for a finished run.
///
/// `keeper_correct[i]` is the Floor Keeper on floor `i` (0 = floor 1):
/// `Some(true)` answered right, `Some(false)` missed, `None` not answered.
/// `died` means health hit 0, so the last floor entered is `X` even if the
/// Keeper was already answered. Floors you left are `C` or `M`. The rest are `.`.
pub fn glyphs(
    floor_count: usize,
    floors_reached: usize,
    keeper_correct: &[Option<bool>],
    died: bool,
) -> String {
    let n = floor_count.max(1);
    let reached = floors_reached.min(n);
    let mut out = vec![b'.'; n];
    for (i, slot) in out.iter_mut().enumerate().take(reached) {
        let last = i + 1 == reached;
        *slot = if last && died {
            b'X'
        } else if keeper_correct.get(i).copied().flatten() == Some(true) {
            b'C'
        } else {
            b'M'
        };
    }
    String::from_utf8(out).unwrap_or_else(|_| ".".repeat(n))
}

/// The contract's glyph rules: length, alphabet, dots after the floors entered,
/// and `X` at most once, only as the last entered floor.
pub fn glyphs_valid(glyphs: &str, floor_count: usize, floors_reached: usize) -> bool {
    if floor_count == 0 || floors_reached > floor_count || glyphs.chars().count() != floor_count {
        return false;
    }
    let chars: Vec<char> = glyphs.chars().collect();
    let mut xs = 0usize;
    for (i, c) in chars.iter().enumerate() {
        if i < floors_reached {
            match c {
                'C' | 'M' => {}
                'X' => {
                    xs += 1;
                    if i + 1 != floors_reached {
                        return false;
                    }
                }
                _ => return false,
            }
        } else if *c != '.' {
            return false;
        }
    }
    xs <= 1 && (floors_reached > 0 || glyphs.chars().all(|c| c == '.'))
}

pub fn emoji_row(glyphs: &str) -> String {
    glyphs
        .chars()
        .map(|c| match c {
            'C' => "🟩",
            'M' => "🟨",
            'X' => "🟥",
            _ => "⬛",
        })
        .collect()
}

pub fn format_time(ms: u64) -> String {
    let total = ms / 1000;
    let (h, m, s) = (total / 3600, (total % 3600) / 60, total % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

/// Spoiler-free card: date, floors, time, emoji squares, play link.
/// No question text, no answers, no layout.
pub fn share_card(
    date: &str,
    floors_reached: usize,
    floor_count: usize,
    time_ms: u64,
    glyphs: &str,
) -> String {
    format!(
        "Campus Crypt  {date}\n{floors_reached}/{floor_count} floors  {}\n{}\n{PLAY_URL}?daily={date}",
        format_time(time_ms),
        emoji_row(glyphs)
    )
}

pub fn play_link(date: &str) -> String {
    format!("{PLAY_URL}?daily={date}")
}

/// One posted score, as the client reads it back. Unknown fields are ignored.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScoreAccepted {
    pub rank: u32,
    pub players: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ScoreRead {
    Accepted(ScoreAccepted),
    /// `nickname_taken` or `bad_nickname`: ask for another name. Do not retry this body.
    FixName {
        code: String,
        message: String,
    },
    /// Network error or 5xx: the caller may retry once.
    Retry,
    /// Any other failure, including other 4xx. Do not retry.
    Fail,
}

pub fn read_score(status: u16, body: &str) -> ScoreRead {
    if status == 201 {
        #[derive(serde::Deserialize)]
        struct OkBody {
            rank: u32,
            players: u32,
        }
        return match serde_json::from_str::<OkBody>(body) {
            Ok(v) => ScoreRead::Accepted(ScoreAccepted {
                rank: v.rank,
                players: v.players,
            }),
            Err(_) => ScoreRead::Fail,
        };
    }
    if status == 0 || status >= 500 {
        return ScoreRead::Retry;
    }
    #[derive(serde::Deserialize)]
    struct ErrBody {
        error: ErrInner,
    }
    #[derive(serde::Deserialize)]
    struct ErrInner {
        code: String,
        #[serde(default)]
        message: String,
    }
    if let Ok(v) = serde_json::from_str::<ErrBody>(body) {
        if status == 409 && v.error.code == "nickname_taken"
            || status == 400 && v.error.code == "bad_nickname"
        {
            return ScoreRead::FixName {
                code: v.error.code,
                message: v.error.message,
            };
        }
    }
    ScoreRead::Fail
}

#[derive(Clone, Debug, PartialEq)]
pub struct BoardRow {
    pub rank: u32,
    pub nickname: String,
    pub club: Option<String>,
    pub floors_reached: u32,
    pub floor_count: u32,
    pub time_ms: u64,
    pub glyphs: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct BoardClub {
    pub club: String,
    pub players: u32,
    pub best_floors: u32,
    pub avg_floors: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Board {
    pub date: String,
    pub players: u32,
    pub rows: Vec<BoardRow>,
    pub clubs: Vec<BoardClub>,
}

pub fn read_today(body: &str) -> Option<Today> {
    #[derive(serde::Deserialize)]
    struct Raw {
        api: u32,
        date: String,
        seed_hex: String,
    }
    let raw: Raw = serde_json::from_str(body).ok()?;
    if raw.api != 1 {
        return None;
    }
    Some(Today {
        date: NyDate::parse(&raw.date)?,
        seed: seed_from_hex(&raw.seed_hex)?,
    })
}

pub fn read_board(body: &str) -> Option<Board> {
    #[derive(serde::Deserialize)]
    struct Raw {
        api: u32,
        date: String,
        players: u32,
        rows: Vec<RawRow>,
        #[serde(default)]
        clubs: Vec<RawClub>,
    }
    #[derive(serde::Deserialize)]
    struct RawRow {
        rank: u32,
        nickname: String,
        #[serde(default)]
        club: Option<String>,
        floors_reached: u32,
        floor_count: u32,
        time_ms: u64,
        glyphs: String,
    }
    #[derive(serde::Deserialize)]
    struct RawClub {
        club: String,
        players: u32,
        best_floors: u32,
        avg_floors: f64,
    }
    let raw: Raw = serde_json::from_str(body).ok()?;
    if raw.api != 1 {
        return None;
    }
    Some(Board {
        date: raw.date,
        players: raw.players,
        rows: raw
            .rows
            .into_iter()
            .map(|r| BoardRow {
                rank: r.rank,
                nickname: r.nickname,
                club: r.club.filter(|c| !c.is_empty()),
                floors_reached: r.floors_reached,
                floor_count: r.floor_count,
                time_ms: r.time_ms,
                glyphs: r.glyphs,
            })
            .collect(),
        clubs: raw
            .clubs
            .into_iter()
            .map(|c| BoardClub {
                club: c.club,
                players: c.players,
                best_floors: c.best_floors,
                avg_floors: c.avg_floors,
            })
            .collect(),
    })
}

/// JSON object for `POST /v1/daily/:date/scores`. Club is omitted when empty.
/// No extra fields: the server answers `invalid_field` to ones it does not know.
pub fn score_json(parts: &ScoreParts<'_>, checksum: &str, install_id: Option<&str>) -> String {
    let mut body = serde_json::json!({
        "mode": "daily",
        "nickname": parts.nickname,
        "floors_reached": parts.floors_reached,
        "floor_count": parts.floor_count,
        "time_ms": parts.time_ms,
        "glyphs": parts.glyphs,
        "build": parts.build,
        "checksum": checksum,
    });
    if !parts.club.is_empty() {
        body["club"] = serde_json::Value::String(parts.club.to_string());
    }
    if let Some(id) = install_id.filter(|s| install_id_ok(s)) {
        body["install_id"] = serde_json::Value::String(id.to_string());
    }
    body.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn instant(iso: &str) -> i64 {
        let date = NyDate::parse(&iso[..10]).unwrap();
        let h: i64 = iso[11..13].parse().unwrap();
        let m: i64 = iso[14..16].parse().unwrap();
        let s: i64 = iso[17..19].parse().unwrap();
        to_unix_days(date) * 86400 + h * 3600 + m * 60 + s
    }

    #[test]
    fn seed_vectors() {
        let rows = [
            ("2026-03-08", "85a51d3a6782180c", 871589907764454789u64),
            ("2026-11-01", "75a410a8686ba38e", 10278176871830037621),
            ("2026-11-24", "b4a84055cea0c3d1", 15115101582439524532),
            ("2027-01-01", "1ff8796591f7e09c", 11304307268544755743),
        ];
        for (date, hex, n) in rows {
            let s = seed_for(date);
            assert_eq!(s.hex, hex, "{date}");
            assert_eq!(s.u64, n, "{date}");
            assert_eq!(u64::from_le_bytes(s.bytes), n);
            assert_eq!(seed_from_hex(hex).unwrap(), s);
            // the u32 the Keeper uses is the first 4 digest bytes
            assert_eq!(
                s.run_seed(),
                u32::from_le_bytes(s.bytes[..4].try_into().unwrap())
            );
        }
    }

    #[test]
    fn date_boundary_vectors() {
        let rows = [
            ("2026-03-08T04:59:59Z", "2026-03-07"),
            ("2026-03-08T05:00:00Z", "2026-03-08"),
            ("2026-11-02T04:59:59Z", "2026-11-01"),
            ("2026-11-02T05:00:00Z", "2026-11-02"),
            ("2026-07-01T03:59:59Z", "2026-06-30"),
            ("2026-07-01T04:00:00Z", "2026-07-01"),
        ];
        for (iso, want) in rows {
            assert_eq!(ny_date(instant(iso)).format(), want, "{iso}");
        }
        // July is EDT, January is EST. The offset is not a fixed -5.
        assert!(is_edt(instant("2026-07-01T04:00:00Z")));
        assert!(!is_edt(instant("2026-03-08T05:00:00Z")));
        assert!(!is_edt(instant("2026-11-02T05:00:00Z")));
    }

    #[test]
    fn query_date_is_only_today_or_yesterday() {
        let today = NyDate::parse("2026-11-24").unwrap();
        assert_eq!(resolve_date(today, None), today);
        assert_eq!(resolve_date(today, Some("2026-11-24")), today);
        assert_eq!(resolve_date(today, Some("2026-11-23")), today.pred());
        assert_eq!(
            resolve_date(today, Some("2026-11-22")),
            today,
            "older than yesterday"
        );
        assert_eq!(resolve_date(today, Some("2026-11-25")), today, "future");
        assert_eq!(
            resolve_date(today, Some("11-24-2026")),
            today,
            "not YYYY-MM-DD"
        );
        assert_eq!(
            resolve_date(today, Some("2026-02-31")),
            today,
            "impossible day"
        );
        assert_eq!(today.succ().pred(), today);
    }

    #[test]
    fn server_today_wins_and_yesterday_is_derived() {
        let local = NyDate::parse("2026-11-23").unwrap();
        let server_seed = seed_for("2026-11-24");
        let server = Today {
            date: NyDate::parse("2026-11-24").unwrap(),
            seed: server_seed.clone(),
        };
        let (d, s) = choose(Some(&server), local, None);
        assert_eq!(d.format(), "2026-11-24");
        assert_eq!(s, server_seed, "today's seed comes from the server");
        let (d, s) = choose(Some(&server), local, Some("2026-11-23"));
        assert_eq!(d.format(), "2026-11-23");
        assert_eq!(
            s,
            seed_for("2026-11-23"),
            "yesterday is not the /today seed"
        );
        let (d, s) = choose(None, local, Some("nope"));
        assert_eq!((d, s.hex), (local, seed_for("2026-11-23").hex));
    }

    #[test]
    fn checksum_vectors() {
        let with_club = ScoreParts {
            seed_hex: "b4a84055cea0c3d1",
            nickname: "Hoot_Fan",
            club: "CSE",
            floors_reached: 7,
            floor_count: 10,
            time_ms: 612345,
            glyphs: "CCMCCCX...",
            build: "0.4.0-web",
        };
        assert_eq!(
            with_club.canonical(),
            "b4a84055cea0c3d1|Hoot_Fan|CSE|7|10|612345|CCMCCCX...|0.4.0-web"
        );
        assert_eq!(
            with_club.checksum(),
            "2bf452ddf46484223201b048857c26bce28e860c8c15860eb2849f67e410c1fd"
        );
        let no_club = ScoreParts {
            seed_hex: "b4a84055cea0c3d1",
            nickname: "owl 42",
            club: "",
            floors_reached: 10,
            floor_count: 10,
            time_ms: 1_500_000,
            glyphs: "CCCCCCCCCC",
            build: "0.4.0-web",
        };
        assert_eq!(
            no_club.canonical(),
            "b4a84055cea0c3d1|owl 42||10|10|1500000|CCCCCCCCCC|0.4.0-web"
        );
        assert_eq!(
            no_club.checksum(),
            "f4b260b3becaf4e653d05b5a38dc42df90b683e15575e4afc0fe95e6def18bad"
        );
    }

    #[test]
    fn glyph_rules_and_the_floor_7_example() {
        let marks = [
            Some(true),
            Some(true),
            Some(false),
            Some(true),
            Some(true),
            Some(true),
        ];
        let g = glyphs(10, 7, &marks, true);
        assert_eq!(g, "CCMCCCX...");
        assert!(glyphs_valid(&g, 10, 7));
        assert!(!glyphs_valid("CCMCCCX..", 10, 7), "wrong length");
        assert!(
            !glyphs_valid("CCXCCCM...", 10, 7),
            "X only as the last reached glyph"
        );
        assert!(!glyphs_valid("CCMCCCXX..", 10, 7), "X at most once");
        assert!(
            !glyphs_valid("CCMCCC....", 10, 7),
            "reached floors are C, M or X"
        );
        assert!(!glyphs_valid("CCMCCCX.C.", 10, 7), "the rest are dots");
        let win = glyphs(10, 10, &[Some(true); 10], false);
        assert_eq!(win, "CCCCCCCCCC");
        assert!(glyphs_valid(&win, 10, 10));
        let missed = glyphs(
            20,
            20,
            &{
                let mut m = vec![Some(true); 20];
                m[4] = Some(false);
                m
            },
            false,
        );
        assert_eq!(missed.chars().nth(4), Some('M'));
        assert!(glyphs_valid(&missed, 20, 20));
        assert!(!missed.contains('X'));
    }

    #[test]
    fn share_card_has_no_spoilers() {
        let g = "CCMCCCX...";
        let card = share_card("2026-11-24", 7, 10, 612_345, g);
        assert!(card.contains("2026-11-24"));
        assert!(card.contains("7/10 floors"));
        assert!(card.contains("10:12"));
        assert!(card.contains(&emoji_row(g)));
        assert!(
            card.contains("https://vacarionprogramming.github.io/campus-crypt/?daily=2026-11-24")
        );
        assert!(!card.contains('C') || card.contains("Campus Crypt"));
        let lower = card.to_ascii_lowercase();
        assert!(!lower.contains("password"));
        assert!(!lower.contains("question"));
        assert!(
            !card.contains("CCMCCC"),
            "the letter row is not the share text"
        );
    }

    #[test]
    fn names_and_clubs_match_the_contract() {
        assert!(nickname_ok("Hoot_Fan"));
        assert!(nickname_ok("owl 42"));
        assert!(nickname_ok("ab-c"));
        assert!(!nickname_ok("ab"));
        assert!(!nickname_ok(" owl"));
        assert!(!nickname_ok("owl "));
        assert!(!nickname_ok("owl  fan"));
        assert!(!nickname_ok("owl@fan"));
        assert!(!nickname_ok("this_name_is_too_long"));
        assert!(club_ok(""));
        assert!(club_ok("CSE"));
        assert!(club_ok("a1_-"));
        assert!(!club_ok("C"));
        assert!(!club_ok("has space"));
        assert_eq!(club_sent("cse"), "CSE");
        assert_eq!(club_sent(""), "");
        assert!(install_id_ok(&fresh_install_id(1_700_000_000, 7)));
        assert!(!install_id_ok("short"));
        assert!(!install_id_ok("has space!"));
    }

    #[test]
    fn score_responses() {
        let ok = r#"{"api":1,"accepted":true,"date":"2026-11-24","rank":4,"players":37,"extra":1}"#;
        assert_eq!(
            read_score(201, ok),
            ScoreRead::Accepted(ScoreAccepted {
                rank: 4,
                players: 37
            })
        );
        let taken =
            r#"{"api":1,"error":{"code":"nickname_taken","message":"taken","field":"nickname"}}"#;
        assert!(
            matches!(read_score(409, taken), ScoreRead::FixName { code, .. } if code == "nickname_taken")
        );
        let bad = r#"{"api":1,"error":{"code":"bad_nickname","message":"no"}}"#;
        assert!(
            matches!(read_score(400, bad), ScoreRead::FixName { code, .. } if code == "bad_nickname")
        );
        assert_eq!(
            read_score(
                400,
                r#"{"api":1,"error":{"code":"bad_checksum","message":"no"}}"#
            ),
            ScoreRead::Fail
        );
        assert_eq!(read_score(0, ""), ScoreRead::Retry);
        assert_eq!(read_score(503, ""), ScoreRead::Retry);
        assert_eq!(
            read_score(
                429,
                r#"{"api":1,"error":{"code":"rate_limited","message":"slow"}}"#
            ),
            ScoreRead::Fail
        );
    }

    #[test]
    fn board_ignores_unknown_fields_and_a_missing_club() {
        let body = r#"{"api":1,"date":"2026-11-24","seed_hex":"b4a84055cea0c3d1","players":37,"future":true,"rows":[
            {"rank":1,"nickname":"owl 42","club":"CSE","floors_reached":10,"floor_count":10,"time_ms":1500000,"glyphs":"CCCCCCCCCC","badge":"x"},
            {"rank":2,"nickname":"Hoot_Fan","floors_reached":7,"floor_count":10,"time_ms":612345,"glyphs":"CCMCCCX..."}
        ],"clubs":[{"club":"CSE","players":6,"best_floors":10,"avg_floors":7.5}]}"#;
        let b = read_board(body).unwrap();
        assert_eq!(b.players, 37);
        assert_eq!(b.rows[1].club, None);
        assert_eq!(b.clubs[0].club, "CSE");
        assert!((b.clubs[0].avg_floors - 7.5).abs() < 1e-9);
        let today = read_today(r#"{"api":1,"date":"2026-11-24","seed_hex":"b4a84055cea0c3d1","tz":"America/New_York","extra":9}"#).unwrap();
        assert_eq!(today.seed.hex, "b4a84055cea0c3d1");
        assert!(
            read_today(r#"{"api":2,"date":"2026-11-24","seed_hex":"b4a84055cea0c3d1"}"#).is_none()
        );
    }

    #[test]
    fn clock_reads_as_unix_seconds() {
        let t = unix_now();
        assert!(t > 1_700_000_000 && t < 2_000_000_000, "unix_now={t}");
    }

    #[test]
    fn posted_json_omits_an_empty_club() {
        let parts = ScoreParts {
            seed_hex: "b4a84055cea0c3d1",
            nickname: "owl 42",
            club: "",
            floors_reached: 10,
            floor_count: 10,
            time_ms: 1_500_000,
            glyphs: "CCCCCCCCCC",
            build: "0.4.0-web",
        };
        let body = score_json(&parts, &parts.checksum(), None);
        assert!(!body.contains("club"));
        assert!(body.contains(r#""mode":"daily""#));
        let with = ScoreParts {
            club: "CSE",
            nickname: "Hoot_Fan",
            ..parts
        };
        let body = score_json(&with, &with.checksum(), Some("abc-1234"));
        assert!(body.contains(r#""club":"CSE""#));
        assert!(body.contains(r#""install_id":"abc-1234""#));
    }
}
