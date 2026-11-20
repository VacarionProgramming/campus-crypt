//! Live changes to the Arduino status display while the game runs (native only).
//!
//! Everything goes through three small files next to the settings file
//! (`settings::path()`'s folder, e.g. `~/Library/Application Support/campus-crypt/`):
//!
//! * `display_layout.txt`: the live layout, one `LT:` / `LM:` / `LB:` line each
//!   (hardware/README.md, "Live layout"). Sent to the board whenever the file changes
//!   and whenever a board connects, followed by `LSAVE` so the board keeps it.
//! * `display_send.txt`: one-off lines (e.g. `LPREV:F`), sent once and then deleted.
//! * `upload.lock`: while it exists (and is less than 3 minutes old) the game lets go
//!   of the USB port, so `hardware/tools/flash.sh` can upload a new sketch.
//!
//! The game writes `display_status.txt` (`updated=<unix secs>`, `status=...`,
//! `connected=0|1`, `layout_sent=<layout file's mtime in unix ms>`), so a tool can
//! tell whether the game is running and has the board.

use crate::hardware::Hardware;

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use super::Hardware;
    use std::path::PathBuf;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    const CHECK_SECS: f32 = 0.5;
    /// a lock older than this is stale (an upload that crashed): ignore it
    const LOCK_MAX: Duration = Duration::from_secs(180);
    /// a board line is at most this long (the sketch's receive buffer is 96 bytes)
    const MAX_LINE: usize = 90;

    pub struct DisplayLive {
        dir: Option<PathBuf>,
        timer: f32,
        sent_for: Option<SystemTime>,
        was_connected: bool,
        last_status: String,
        status_age: f32,
    }

    fn unix_ms(t: SystemTime) -> u128 {
        t.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis())
    }

    /// The board-bound lines in a layout file: `L...` lines only, trimmed, short enough.
    pub fn layout_lines(text: &str) -> Vec<String> {
        text.lines()
            .map(|l| l.trim_end_matches(['\r', '\n']))
            .filter(|l| l.starts_with('L') && l.len() <= MAX_LINE && l.is_ascii())
            .map(str::to_string)
            .collect()
    }

    impl DisplayLive {
        pub fn new() -> DisplayLive {
            let dir = crate::settings::path().and_then(|p| p.parent().map(|d| d.to_path_buf()));
            DisplayLive { dir, timer: 0.0, sent_for: None, was_connected: false, last_status: String::new(), status_age: 0.0 }
        }

        pub fn update(&mut self, hw: &mut Hardware, dt: f32) {
            self.timer -= dt;
            self.status_age -= dt;
            if self.timer > 0.0 {
                return;
            }
            self.timer = CHECK_SECS;
            let Some(dir) = self.dir.clone() else { return };

            // 1. upload lock: let go of the port while a sketch uploads
            let lock = std::fs::metadata(dir.join("upload.lock")).ok();
            let fresh = lock.and_then(|m| m.modified().ok()).is_some_and(|t| t.elapsed().map_or(true, |e| e < LOCK_MAX));
            hw.hold(fresh);

            let connected = hw.status().connected();
            if !connected {
                self.was_connected = false;
            }

            // 2. the layout: on change, or when a board (re)connects
            let layout = dir.join("display_layout.txt");
            let mtime = std::fs::metadata(&layout).and_then(|m| m.modified()).ok();
            if connected && mtime.is_some() && (mtime != self.sent_for || !self.was_connected) {
                if let Ok(text) = std::fs::read_to_string(&layout) {
                    let mut lines = layout_lines(&text);
                    lines.retain(|l| l != "LSAVE");
                    lines.push("LSAVE".into());
                    if hw.send_layout(lines) {
                        self.sent_for = mtime;
                    }
                }
            }
            if connected {
                self.was_connected = true;
            }

            // 3. one-off lines
            let once = dir.join("display_send.txt");
            if connected {
                if let Ok(text) = std::fs::read_to_string(&once) {
                    let _ = std::fs::remove_file(&once);
                    hw.send_layout(layout_lines(&text));
                }
            }

            // 4. tell tools what the game sees (on change, else every 2 s)
            let status = format!(
                "status={}\nconnected={}\nlayout_sent={}\n",
                hw.status().label(),
                u8::from(connected),
                self.sent_for.map_or(0, unix_ms)
            );
            if status != self.last_status || self.status_age <= 0.0 {
                self.status_age = 2.0;
                let now = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
                let _ = std::fs::create_dir_all(&dir);
                let _ = std::fs::write(dir.join("display_status.txt"), format!("updated={now}\n{status}"));
                self.last_status = status;
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn keeps_only_board_lines() {
            let long = format!("LT:P0:{}", "x".repeat(100));
            let text = format!("# a comment\nLT:P0:HP {{hp}} {{bar}}\r\n\nS:1,2\nLB:7\n{long}\nLM:H:120:00663C1818000000");
            assert_eq!(layout_lines(&text), vec!["LT:P0:HP {hp} {bar}", "LB:7", "LM:H:120:00663C1818000000"]);
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub use native::DisplayLive;

/// The web build has no serial port: nothing to watch.
#[cfg(target_arch = "wasm32")]
pub struct DisplayLive;

#[cfg(target_arch = "wasm32")]
impl DisplayLive {
    pub fn new() -> DisplayLive {
        DisplayLive
    }
    pub fn update(&mut self, _hw: &mut Hardware, _dt: f32) {}
}
