//! The browser bridge for the daily board.
//!
//! `web/api.js` owns the fetch calls. The base URL is `DAILY_API_BASE` in that
//! file, empty until Dey deploys the Worker. An empty base, a timeout, or any
//! other failure comes back here as a network error and the game fails open:
//! the run already happened, and the share card does not need the board.
//!
//! Native builds have no board. They keep nickname, club and install id in
//! memory for the session only. Nothing is written to disk, and no request
//! leaves the machine.
//!
//! No API key is read or sent. The checksum in the score body is not a secret.

use crate::daily::{self, Board, ScoreRead, Today};

const TODAY_TIMEOUT: f64 = 3.0;
const SCORE_TIMEOUT: f64 = 5.0;
const BOARD_TIMEOUT: f64 = 5.0;
#[cfg(target_arch = "wasm32")]
const BODY_CAP: usize = 16 * 1024;

#[derive(Clone, Debug)]
pub struct Raw {
    /// HTTP status, or 0 when the network failed or the base URL is empty.
    pub status: u16,
    pub body: String,
}

enum Job {
    Idle,
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    Pending {
        id: u32,
        started: f64,
    },
    Done(Raw),
}

pub struct Net {
    today: Job,
    score: Job,
    board: Job,
}

impl Default for Net {
    fn default() -> Net {
        Net {
            today: Job::Idle,
            score: Job::Idle,
            board: Job::Idle,
        }
    }
}

impl Net {
    pub fn new() -> Net {
        Net::default()
    }

    pub fn begin_today(&mut self, now: f64) {
        self.today = start(
            0,
            "/v1/daily/today",
            None,
            (TODAY_TIMEOUT * 1000.0) as u32,
            now,
        );
    }

    /// `Some` once the call has finished or the 3 s budget is spent.
    pub fn poll_today(&mut self, now: f64) -> Option<Result<Today, ()>> {
        let raw = take(&mut self.today, now, TODAY_TIMEOUT)?;
        Some(daily::read_today(&raw.body).ok_or(()))
    }

    pub fn begin_score(&mut self, date: &str, body: &str, now: f64) {
        let path = format!("/v1/daily/{date}/scores");
        self.score = start(1, &path, Some(body), (SCORE_TIMEOUT * 1000.0) as u32, now);
    }

    pub fn poll_score(&mut self, now: f64) -> Option<ScoreRead> {
        let raw = take(&mut self.score, now, SCORE_TIMEOUT)?;
        Some(daily::read_score(raw.status, &raw.body))
    }

    pub fn begin_board(&mut self, date: &str, now: f64) {
        let path = format!("/v1/daily/{date}/leaderboard?limit=20");
        self.board = start(0, &path, None, (BOARD_TIMEOUT * 1000.0) as u32, now);
    }

    pub fn poll_board(&mut self, now: f64) -> Option<Result<Board, ()>> {
        let raw = take(&mut self.board, now, BOARD_TIMEOUT)?;
        Some(daily::read_board(&raw.body).ok_or(()))
    }
}

fn take(job: &mut Job, now: f64, timeout: f64) -> Option<Raw> {
    match job {
        Job::Idle => None,
        Job::Done(_) => match std::mem::replace(job, Job::Idle) {
            Job::Done(raw) => Some(raw),
            other => {
                *job = other;
                None
            }
        },
        Job::Pending { id, started } => {
            if now - *started >= timeout {
                let id = *id;
                *job = Job::Idle;
                forget(id);
                return Some(Raw {
                    status: 0,
                    body: String::new(),
                });
            }
            match poll_job(*id) {
                None => None,
                Some(raw) => {
                    *job = Job::Idle;
                    Some(raw)
                }
            }
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn start(_method: u32, _path: &str, _body: Option<&str>, _timeout_ms: u32, _now: f64) -> Job {
    Job::Done(Raw {
        status: 0,
        body: String::new(),
    })
}

#[cfg(not(target_arch = "wasm32"))]
fn poll_job(_id: u32) -> Option<Raw> {
    None
}

#[cfg(not(target_arch = "wasm32"))]
fn forget(_id: u32) {}

#[cfg(target_arch = "wasm32")]
fn start(method: u32, path: &str, body: Option<&str>, timeout_ms: u32, now: f64) -> Job {
    let empty = "";
    let (ptr, len) = match body {
        Some(b) => (b.as_ptr(), b.len() as u32),
        None => (empty.as_ptr(), 0),
    };
    let id = unsafe {
        cc_api_start(
            method,
            path.as_ptr(),
            path.len() as u32,
            ptr,
            len,
            timeout_ms,
        )
    };
    if id == 0 {
        Job::Done(Raw {
            status: 0,
            body: String::new(),
        })
    } else {
        Job::Pending { id, started: now }
    }
}

#[cfg(target_arch = "wasm32")]
fn poll_job(id: u32) -> Option<Raw> {
    let mut buf = vec![0u8; BODY_CAP];
    let mut len = 0u32;
    let status = unsafe { cc_api_poll(id, buf.as_mut_ptr(), buf.len() as u32, &mut len) };
    if status == 0 {
        return None;
    }
    if status < 0 {
        return Some(Raw {
            status: 0,
            body: String::new(),
        });
    }
    let n = (len as usize).min(buf.len());
    Some(Raw {
        status: status as u16,
        body: String::from_utf8_lossy(&buf[..n]).into_owned(),
    })
}

#[cfg(target_arch = "wasm32")]
fn forget(id: u32) {
    unsafe { cc_api_forget(id) }
}

#[cfg(target_arch = "wasm32")]
extern "C" {
    /// web/api.js. method 0 = GET, 1 = POST. Returns 0 if it could not start.
    fn cc_api_start(
        method: u32,
        path: *const u8,
        path_len: u32,
        body: *const u8,
        body_len: u32,
        timeout_ms: u32,
    ) -> u32;
    /// 0 while pending, -1 on a network error, otherwise the HTTP status.
    /// Writes the body into `buf` and sets `out_len`.
    fn cc_api_poll(id: u32, buf: *mut u8, cap: u32, out_len: *mut u32) -> i32;
    fn cc_api_forget(id: u32);
}

// ---------------------------------------------------------------- profile (local only)

const KEY_NICK: &str = "campus_crypt_daily_nickname";
const KEY_CLUB: &str = "campus_crypt_daily_club";
const KEY_INSTALL: &str = "campus_crypt_daily_install_id";

#[cfg(not(target_arch = "wasm32"))]
thread_local! {
    static MEM: std::cell::RefCell<std::collections::HashMap<String, String>> = std::cell::RefCell::new(std::collections::HashMap::new());
}

pub fn load_profile() -> (String, String, String) {
    let nick = store_get(KEY_NICK).unwrap_or_default();
    let club = store_get(KEY_CLUB).unwrap_or_default();
    let mut install = store_get(KEY_INSTALL).unwrap_or_default();
    if !daily::install_id_ok(&install) {
        install = daily::fresh_install_id(daily::unix_now(), mix_salt());
        store_set(KEY_INSTALL, &install);
    }
    (nick, club, install)
}

pub fn save_profile(nickname: &str, club: &str) {
    store_set(KEY_NICK, nickname);
    store_set(KEY_CLUB, club);
}

fn mix_salt() -> u64 {
    macroquad::rand::rand() as u64
}

#[cfg(not(target_arch = "wasm32"))]
fn store_get(key: &str) -> Option<String> {
    MEM.with(|m| m.borrow().get(key).cloned())
}

#[cfg(not(target_arch = "wasm32"))]
fn store_set(key: &str, val: &str) {
    MEM.with(|m| {
        m.borrow_mut().insert(key.to_string(), val.to_string());
    });
}

#[cfg(target_arch = "wasm32")]
fn store_get(key: &str) -> Option<String> {
    let mut buf = vec![0u8; 256];
    let n = unsafe {
        cc_store_get(
            key.as_ptr(),
            key.len() as u32,
            buf.as_mut_ptr(),
            buf.len() as u32,
        )
    };
    if n < 0 {
        return None;
    }
    let n = (n as usize).min(buf.len());
    Some(String::from_utf8_lossy(&buf[..n]).into_owned())
}

#[cfg(target_arch = "wasm32")]
fn store_set(key: &str, val: &str) {
    unsafe {
        cc_store_set(
            key.as_ptr(),
            key.len() as u32,
            val.as_ptr(),
            val.len() as u32,
        )
    }
}

#[cfg(target_arch = "wasm32")]
extern "C" {
    fn cc_store_get(key: *const u8, key_len: u32, buf: *mut u8, cap: u32) -> i32;
    fn cc_store_set(key: *const u8, key_len: u32, val: *const u8, val_len: u32);
}

/// `?daily=YYYY-MM-DD` from the page URL, or nothing on native.
pub fn page_query() -> Option<String> {
    #[cfg(target_arch = "wasm32")]
    {
        let mut buf = vec![0u8; 32];
        let n = unsafe { cc_daily_query(buf.as_mut_ptr(), buf.len() as u32) };
        if n <= 0 {
            return None;
        }
        let n = (n as usize).min(buf.len());
        let s = String::from_utf8_lossy(&buf[..n]).into_owned();
        daily::NyDate::parse(&s).map(|d| d.format())
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        None
    }
}

#[cfg(target_arch = "wasm32")]
extern "C" {
    fn cc_daily_query(buf: *mut u8, cap: u32) -> i32;
}

/// Copy the share card. Returns false when there is no clipboard (native).
pub fn copy_text(text: &str) -> bool {
    #[cfg(target_arch = "wasm32")]
    {
        unsafe { cc_clipboard(text.as_ptr(), text.len() as u32) != 0 }
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = text;
        false
    }
}

#[cfg(target_arch = "wasm32")]
extern "C" {
    fn cc_clipboard(ptr: *const u8, len: u32) -> i32;
}

/// The web name form. Native draws its own fields, so these are no-ops there.
pub fn name_form_open(nickname: &str, club: &str, error: &str) {
    #[cfg(target_arch = "wasm32")]
    unsafe {
        cc_name_open(
            nickname.as_ptr(),
            nickname.len() as u32,
            club.as_ptr(),
            club.len() as u32,
            error.as_ptr(),
            error.len() as u32,
        );
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = (nickname, club, error);
    }
}

pub fn name_form_close() {
    #[cfg(target_arch = "wasm32")]
    unsafe {
        cc_name_close();
    }
}

/// `Some((nickname, club))` once the player submits the web form.
pub fn name_form_poll() -> Option<(String, String)> {
    #[cfg(target_arch = "wasm32")]
    {
        let mut nick = vec![0u8; 64];
        let mut club = vec![0u8; 32];
        let mut nick_len = 0u32;
        let mut club_len = 0u32;
        let st = unsafe {
            cc_name_poll(
                nick.as_mut_ptr(),
                nick.len() as u32,
                &mut nick_len,
                club.as_mut_ptr(),
                club.len() as u32,
                &mut club_len,
            )
        };
        if st != 1 {
            return None;
        }
        let n = (nick_len as usize).min(nick.len());
        let c = (club_len as usize).min(club.len());
        Some((
            String::from_utf8_lossy(&nick[..n]).into_owned(),
            String::from_utf8_lossy(&club[..c]).into_owned(),
        ))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        None
    }
}

#[cfg(target_arch = "wasm32")]
extern "C" {
    fn cc_name_open(
        nick: *const u8,
        nick_len: u32,
        club: *const u8,
        club_len: u32,
        err: *const u8,
        err_len: u32,
    );
    fn cc_name_close();
    fn cc_name_poll(
        nick: *mut u8,
        nick_cap: u32,
        nick_len: *mut u32,
        club: *mut u8,
        club_cap: u32,
        club_len: *mut u32,
    ) -> i32;
}
