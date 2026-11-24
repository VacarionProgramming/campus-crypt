//! Player settings kept between launches: master, music, and voice volume, music and voices on/off,
//! reduced motion, Hoot's hints, the AI extras and the Arduino status display.
//!
//! * native: a small text file, `key=value` per line, at
//!   `$CAMPUS_CRYPT_SETTINGS` if set, else the OS config folder:
//!   `$XDG_CONFIG_HOME` or `~/.config` + `/campus-crypt/settings` (Linux),
//!   `~/Library/Application Support/campus-crypt/settings` (macOS),
//!   `%APPDATA%\campus-crypt\settings` (Windows).
//! * web: the browser's localStorage, through two tiny functions in
//!   `web/settings.js` (missing script = defaults, nothing saved).
//!
//! `CAMPUS_CRYPT_MUSIC_VOLUME` (0..1) and `CAMPUS_CRYPT_REDUCED_MOTION=1`
//! still override the saved values natively.

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    pub master: f32,
    pub music: f32,
    /// spoken lines, times the master volume
    pub voice: f32,
    pub reduced_motion: bool,
    /// N: music on/off (sound effects stay)
    pub music_on: bool,
    /// spoken lines on/off (subtitles stay)
    pub voices_on: bool,
    /// H: Hoot's hints on the encounter screen
    pub hints: bool,
    /// the AI extras and AI voices (off = scripted lines, no network calls)
    pub ai: bool,
    /// the USB link to the Arduino status display
    pub arduino: bool,
}

pub const DEFAULT_MASTER: f32 = 1.0;
pub const DEFAULT_VOICE: f32 = 0.9;

impl Default for Settings {
    fn default() -> Settings {
        Settings {
            master: DEFAULT_MASTER,
            music: crate::music::DEFAULT_VOLUME,
            voice: DEFAULT_VOICE,
            reduced_motion: false,
            music_on: true,
            voices_on: true,
            hints: true,
            ai: true,
            arduino: true,
        }
    }
}

// the text format is native-only (the web stores each value in localStorage)
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
impl Settings {
    pub fn parse(text: &str) -> Settings {
        let mut s = Settings::default();
        for line in text.lines() {
            let Some((k, v)) = line.split_once('=') else { continue };
            let v = v.trim();
            match k.trim() {
                "master" => s.master = v.parse::<f32>().ok().filter(|x| x.is_finite()).map_or(s.master, |x| x.clamp(0.0, 1.0)),
                "music" => s.music = v.parse::<f32>().ok().filter(|x| x.is_finite()).map_or(s.music, |x| x.clamp(0.0, 1.0)),
                "voice" => s.voice = v.parse::<f32>().ok().filter(|x| x.is_finite()).map_or(s.voice, |x| x.clamp(0.0, 1.0)),
                "reduced_motion" => s.reduced_motion = flag(v),
                "music_on" => s.music_on = flag(v),
                "voices_on" => s.voices_on = flag(v),
                "hints" => s.hints = flag(v),
                "ai" => s.ai = flag(v),
                "arduino" => s.arduino = flag(v),
                _ => {}
            }
        }
        s
    }

    pub fn to_text(self) -> String {
        format!(
            "master={:.2}\nmusic={:.2}\nvoice={:.2}\nreduced_motion={}\nmusic_on={}\nvoices_on={}\nhints={}\nai={}\narduino={}\n",
            self.master,
            self.music,
            self.voice,
            u8::from(self.reduced_motion),
            u8::from(self.music_on),
            u8::from(self.voices_on),
            u8::from(self.hints),
            u8::from(self.ai),
            u8::from(self.arduino)
        )
    }
}

#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
fn flag(v: &str) -> bool {
    v == "1" || v.eq_ignore_ascii_case("true") || v.eq_ignore_ascii_case("on")
}

#[cfg(not(target_arch = "wasm32"))]
pub fn path() -> Option<std::path::PathBuf> {
    use std::path::PathBuf;
    let env = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty()).map(PathBuf::from);
    if let Some(p) = env("CAMPUS_CRYPT_SETTINGS") {
        return Some(p);
    }
    let dir = if cfg!(windows) {
        env("APPDATA")?
    } else if cfg!(target_os = "macos") {
        env("HOME")?.join("Library/Application Support")
    } else {
        env("XDG_CONFIG_HOME").or_else(|| env("HOME").map(|h| h.join(".config")))?
    };
    Some(dir.join("campus-crypt").join("settings"))
}

#[cfg(not(target_arch = "wasm32"))]
pub fn load() -> Settings {
    let mut s = path().and_then(|p| std::fs::read_to_string(p).ok()).map_or_else(Settings::default, |t| Settings::parse(&t));
    if let Some(v) = std::env::var("CAMPUS_CRYPT_MUSIC_VOLUME").ok().and_then(|v| v.parse::<f32>().ok()) {
        s.music = v.clamp(0.0, 1.0);
    }
    if std::env::var("CAMPUS_CRYPT_REDUCED_MOTION").is_ok_and(|v| v == "1" || v.eq_ignore_ascii_case("on")) {
        s.reduced_motion = true;
    }
    s
}

#[cfg(not(target_arch = "wasm32"))]
pub fn save(s: Settings) {
    let Some(p) = path() else { return };
    if let Some(dir) = p.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Err(e) = std::fs::write(&p, s.to_text()) {
        eprintln!("settings: couldn't save {}: {e}", p.display());
    }
}

#[cfg(target_arch = "wasm32")]
extern "C" {
    /// web/settings.js: the stored value, or a negative number / NaN if none
    fn cc_setting_get(id: u32) -> f32;
    fn cc_setting_set(id: u32, v: f32);
}

#[cfg(target_arch = "wasm32")]
pub fn load() -> Settings {
    let mut s = Settings::default();
    let get = |id| {
        // SAFETY: plain f32 in/out, provided by web/settings.js (or a no-op stub)
        let v = unsafe { cc_setting_get(id) };
        (v.is_finite() && v >= 0.0).then_some(v.min(1.0))
    };
    if let Some(v) = get(0) {
        s.master = v;
    }
    if let Some(v) = get(1) {
        s.music = v;
    }
    if let Some(v) = get(7) {
        s.voice = v;
    }
    if let Some(v) = get(2) {
        s.reduced_motion = v >= 0.5;
    }
    for (id, field) in [(3, &mut s.music_on), (8, &mut s.voices_on), (4, &mut s.hints), (5, &mut s.ai), (6, &mut s.arduino)] {
        if let Some(v) = get(id) {
            *field = v >= 0.5;
        }
    }
    s
}

#[cfg(target_arch = "wasm32")]
pub fn save(s: Settings) {
    // SAFETY: plain f32 in, provided by web/settings.js (or a no-op stub)
    unsafe {
        cc_setting_set(0, s.master);
        cc_setting_set(1, s.music);
        cc_setting_set(7, s.voice);
        cc_setting_set(2, if s.reduced_motion { 1.0 } else { 0.0 });
        for (id, on) in [(3, s.music_on), (8, s.voices_on), (4, s.hints), (5, s.ai), (6, s.arduino)] {
            cc_setting_set(id, if on { 1.0 } else { 0.0 });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_and_ignores_junk() {
        let s = Settings { master: 0.35, music: 0.8, voice: 0.4, reduced_motion: true, music_on: false, voices_on: false, hints: false, ai: false, arduino: false };
        assert_eq!(Settings::parse(&s.to_text()), s);
        let d = Settings::parse("master=loud\nmusic=7\n# hi\nreduced_motion=0\nother=1");
        assert_eq!((d.master, d.music, d.reduced_motion), (DEFAULT_MASTER, 1.0, false));
        assert_eq!(Settings::parse(""), Settings::default());
        // a settings file from before the toggles existed keeps them on
        let old = Settings::parse("master=0.50\nmusic=0.25\nreduced_motion=1\n");
        assert!(old.music_on && old.voices_on && old.hints && old.ai && old.arduino);
        assert_eq!(old.voice, DEFAULT_VOICE);
        assert!(old.reduced_motion);
        assert_eq!(old.voice, Settings { reduced_motion: false, ..old }.voice, "reduced motion does not change the voice level");
    }

    #[test]
    fn saves_and_loads_from_the_settings_file() {
        let dir = std::env::temp_dir().join(format!("cc_settings_test_{}", std::process::id()));
        let p = dir.join("nested").join("settings");
        let s = Settings { master: 0.5, music: 0.25, reduced_motion: true, ..Settings::default() };
        // write/read the same way save/load do, without touching the process env
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, s.to_text()).unwrap();
        assert_eq!(Settings::parse(&std::fs::read_to_string(&p).unwrap()), s);
        let _ = std::fs::remove_dir_all(dir);
        assert!(path().is_some_and(|p| p.ends_with("settings")));
    }
}
