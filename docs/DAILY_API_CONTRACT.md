# Daily Crypt API contract (v1)

Any breaking change bumps the path to /v2; additive fields may appear in v1 responses at any time (clients must ignore unknown fields).

## 1. Daily date and seed
* Date = the calendar date in America/New_York (YYYY-MM-DD), using the IANA zone (EST/EDT handled by the zone rules, never a fixed -5). The day rolls over at local midnight.
* seed bytes = first 8 bytes of SHA-256("campus-crypt:YYYY-MM-DD") (UTF-8, no newline).
* seed_hex = those 8 bytes as 16 lowercase hex chars, in digest order.
* seed (u64) = u64::from_le_bytes(seed_bytes). This is what goes into run_seed.

Test vectors:
| date | seed_hex | seed (u64, decimal) |
| 2026-03-08 | 85a51d3a6782180c | 871589907764454789 | DST starts
| 2026-11-01 | 75a410a8686ba38e | 10278176871830037621 | DST ends
| 2026-11-24 | b4a84055cea0c3d1 | 15115101582439524532 |
| 2027-01-01 | 1ff8796591f7e09c | 11304307268544755743 |

Date-boundary vectors (UTC instant -> NY date): 2026-03-08T04:59:59Z -> 2026-03-07; 2026-03-08T05:00:00Z -> 2026-03-08; 2026-11-02T04:59:59Z -> 2026-11-01; 2026-11-02T05:00:00Z -> 2026-11-02; 2026-07-01T03:59:59Z -> 2026-06-30; 2026-07-01T04:00:00Z -> 2026-07-01.

Client rule: when online, trust GET /v1/daily/today; when offline (or it fails/times out), compute the date from the local clock in America/New_York and derive the seed locally.

## 2. Endpoints
Base: https://<worker-host>/v1. JSON bodies, UTF-8. Every success response includes "api": 1.

GET /v1/daily/today -> { "api": 1, "date": "2026-11-24", "seed_hex": "b4a84055cea0c3d1", "tz": "America/New_York" }

POST /v1/daily/:date/scores. :date must be today or yesterday (NY); older or future -> date_closed. Max body 2 KB.
Request: { "mode": "daily", "nickname": "Hoot_Fan", "club": "CSE", "floors_reached": 7, "floor_count": 10, "time_ms": 612345, "glyphs": "CCMCCCX...", "build": "0.4.0-web", "checksum": "2bf452ddf46484223201b048857c26bce28e860c8c15860eb2849f67e410c1fd", "install_id": "optional-anonymous-id" }
Field rules:
- mode: exactly "daily" (else bad_mode)
- nickname: 3-16 chars, ^[A-Za-z0-9_ -]+$, no leading/trailing space, no double spaces, passes the server profanity filter. Unique per day case-insensitively (first come keeps it).
- club: optional; omit, null or "" for none. 2-12 chars, ^[A-Za-z0-9_-]+$. Stored uppercased.
- floor_count: integer in server allowlist (default 10,20)
- floors_reached: integer 0..=floor_count = number of floors entered
- glyphs: string, length == floor_count, one char per floor in order
- time_ms: >= floors_reached * 20000 and <= 6 h, else impossible_time
- build: 1-32 chars [A-Za-z0-9._+-]; must be on server ALLOWED_BUILDS else unknown_build
- checksum: 64 lowercase hex (section 3)
- install_id: optional, 8-64 chars [A-Za-z0-9-], anonymous
Glyph alphabet (index 0 = floor 1): C cleared (Keeper answered right), M floor passed but Keeper missed, X died on this floor, . not reached. First floors_reached glyphs are in {C,M,X}, the rest are '.'; X at most once and only as the last reached glyph. Example died on floor 7 of 10: CCMCCCX...
Response 201: { "api": 1, "accepted": true, "date": "2026-11-24", "rank": 4, "players": 37 }

GET /v1/daily/:date/leaderboard?limit=20 (1-100, default 20; date not in future). Sort: floors_reached desc, time_ms asc, earliest submit.
{ "api": 1, "date": "2026-11-24", "seed_hex": "b4a84055cea0c3d1", "players": 37, "rows": [ { "rank": 1, "nickname": "owl 42", "club": "CSE", "floors_reached": 10, "floor_count": 10, "time_ms": 1500000, "glyphs": "CCCCCCCCCC" }, { "rank": 2, "nickname": "Hoot_Fan", "floors_reached": 7, "floor_count": 10, "time_ms": 612345, "glyphs": "CCMCCCX..." } ], "clubs": [ { "club": "CSE", "players": 6, "best_floors": 10, "avg_floors": 7.5 } ] }
Small-club rule: clubs with fewer than 5 distinct nicknames that date are hidden (club key omitted from rows, absent from clubs).

## 3. Checksum
checksum = lowercase_hex(SHA-256(UTF-8("seed_hex|nickname|club|floors_reached|floor_count|time_ms|glyphs|build"))). seed_hex for the path date; nickname and club exactly as sent (club = empty string when absent/null); integers in plain decimal.
Vector: b4a84055cea0c3d1|Hoot_Fan|CSE|7|10|612345|CCMCCCX...|0.4.0-web -> 2bf452ddf46484223201b048857c26bce28e860c8c15860eb2849f67e410c1fd
Vector (no club): b4a84055cea0c3d1|owl 42||10|10|1500000|CCCCCCCCCC|0.4.0-web -> f4b260b3becaf4e653d05b5a38dc42df90b683e15575e4afc0fe95e6def18bad
This checksum is forgeable; integrity only, not anti-cheat.

## 4. Errors
Status + body { "api": 1, "error": { "code": "bad_nickname", "message": "human text", "field": "nickname" } } (field only when relevant).
400: bad_json, bad_date, bad_mode, invalid_field, bad_nickname, bad_club, bad_glyphs, bad_checksum, impossible_time, unknown_build, bad_floor_count. 403: origin_not_allowed. 404: not_found. 405: method_not_allowed. 409: date_closed, nickname_taken. 413: payload_too_large. 429: rate_limited (with Retry-After). 500: internal.

## 5. Rate limits
One score per (IP hash, nickname) per date; max 5 accepted scores per IP per date.

## 6. CORS
Allowed origin: https://vacarionprogramming.github.io (localhost:8080 for testing). Requests with no Origin (native builds) are allowed.

## 7. Client behaviour (fail-open)
* The run never waits on the network. Start the daily from the local seed if /today has not answered within 3 s.
* Submit once after the run, 5 s timeout, at most one retry on network error/5xx. Never retry 4xx.
* On any failure show "Board offline - your run still counts on your share card" and keep the share card working.
* Show nickname_taken / bad_nickname as a prompt to pick another name. Store nickname/club/install_id in localStorage only.
* No API keys in the wasm or JS. Daily runs are scripted-only (no live AI).

## 8. Play link
PLAY_URL?daily=YYYY-MM-DD with PLAY_URL = https://vacarionprogramming.github.io/campus-crypt/. Client reads daily from the query string (only today or yesterday; otherwise today's).

## 9. Privacy
No KSU IDs, names, emails, dorms or majors requested or stored client-side beyond nickname/club/install_id in localStorage.
