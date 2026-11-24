//! Music: one looping track per screen and floor, loaded lazily.
//!
//! * `Track::Title` (`assets/audio/title_march.ogg`) on the title screen.
//! * `Track::Floor(n)` (`assets/audio/floor_01.ogg` .. `floor_10.ogg`) on floors
//!   1-10, in the warden arena too. Floors 11-20 reuse the later, louder tracks
//!   so the back half rises instead of opening on floor 1's theme.
//!
//! Nothing loads at start-up. main.rs asks for the track it wants now plus the
//! one it'll want next (the title prefetches floor 1, floor n prefetches n+1),
//! and each loads in the background:
//! * native: a thread reads the file, decodes it with lewton (quad-snd's own
//!   Vorbis decoder) and resamples it to 44.1 kHz stereo. A missing or broken
//!   file is caught on the thread (quad-snd would panic on it, and release
//!   builds abort). Handing the finished PCM to quad-snd still costs the main
//!   thread ~0.1 s for a 60 s track, so that step only happens when main.rs
//!   says the frame can take it (`allow_upload`): on the title screen and on
//!   the black frame between floors. The next floor's track is decoded during
//!   the current floor, so it's ready on that black frame.
//! * web: a coroutine fetches the file and the browser decodes it
//!   (`decodeAudioData`), off the main loop.
//!
//! A track that isn't ready yet is silence, and fades in once it arrives. A
//! track whose file is missing or unplayable stays silent for the whole run;
//! the game never waits on music. Changing track crossfades over
//! `CROSSFADE_SECS`. Only the playing, fading and prefetched tracks stay in
//! memory.
//!
//! Adding a track: add a `Track` variant with a `file_name()` and ask for it
//! from main.rs (`Music::update`).

use macroquad::audio::{load_sound_from_bytes, play_sound, set_sound_volume, stop_sound, PlaySoundParams, Sound};
#[cfg(target_arch = "wasm32")]
use macroquad::experimental::coroutines::{start_coroutine, Coroutine};
#[cfg(not(target_arch = "wasm32"))]
use std::sync::{Arc, Mutex};

/// Default music volume (0..1; times the master volume). Saved by the settings
/// sliders; native override: `CAMPUS_CRYPT_MUSIC_VOLUME`.
pub const DEFAULT_VOLUME: f32 = 0.75;
pub const CROSSFADE_SECS: f32 = 1.0;
/// Floors with their own file (`floor_01.ogg` .. `floor_10.ogg`).
pub const FLOOR_TRACKS: usize = 10;
/// Track numbers (1-based files) for floors 11-20. Opens on floor 6's theme,
/// the start of the louder half, and climbs to floor 10, holding that peak.
const BACK_HALF: [u8; 10] = [6, 7, 8, 8, 9, 9, 10, 10, 10, 10];
/// Folder the tracks live in (relative to the working directory or the
/// executable natively, to the page on the web: `web/assets/audio/`).
pub const DIR: &str = "assets/audio";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Track {
    Title,
    /// 1-based floor number (1..=FLOOR_TRACKS)
    Floor(u8),
}

impl Track {
    /// The track for 0-based floor `floor`.
    /// Floors 0-9 use files 1-10. Floors 10-19 (floors 11-20) use `BACK_HALF`,
    /// and anything past that repeats the back-half rise.
    pub fn for_floor(floor: usize) -> Track {
        let n = if floor < FLOOR_TRACKS {
            (floor + 1) as u8
        } else {
            BACK_HALF[(floor - FLOOR_TRACKS) % BACK_HALF.len()]
        };
        Track::Floor(n)
    }

    pub fn file_name(self) -> String {
        match self {
            Track::Title => "title_march.ogg".to_string(),
            Track::Floor(n) => format!("floor_{n:02}.ogg"),
        }
    }
}

/// Why a track is silent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MusicError {
    Missing,
    NotOgg,
    /// Native only: lewton could not decode it (the browser just never reports it loaded).
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    Undecodable(String),
}

/// Cheap header check (all platforms).
pub fn check_ogg(bytes: Option<&[u8]>) -> Result<(), MusicError> {
    let b = bytes.ok_or(MusicError::Missing)?;
    if b.is_empty() {
        return Err(MusicError::Missing);
    }
    if !b.starts_with(b"OggS") {
        return Err(MusicError::NotOgg);
    }
    Ok(())
}

/// Native: decode a whole Ogg Vorbis file into a 44.1 kHz stereo 16-bit WAV
/// (quad-snd's native mixer format, so loading it is just a copy).
#[cfg(not(target_arch = "wasm32"))]
pub fn decode_to_wav(bytes: Option<&[u8]>) -> Result<Vec<u8>, MusicError> {
    use lewton::inside_ogg::OggStreamReader;
    check_ogg(bytes)?;
    let b = bytes.unwrap_or_default();
    let bad = |e: lewton::VorbisError| MusicError::Undecodable(e.to_string());
    let mut r = OggStreamReader::new(std::io::Cursor::new(b)).map_err(bad)?;
    let ch = r.ident_hdr.audio_channels as usize;
    let rate = r.ident_hdr.audio_sample_rate;
    if !(1..=2).contains(&ch) || rate == 0 {
        return Err(MusicError::Undecodable(format!("{ch} channels at {rate} Hz")));
    }
    // interleaved source frames as stereo f32
    let mut src: Vec<[f32; 2]> = Vec::new();
    while let Some(pkt) = r.read_dec_packet_itl().map_err(bad)? {
        for f in pkt.chunks_exact(ch) {
            let l = f[0] as f32 / 32768.0;
            src.push([l, if ch == 2 { f[1] as f32 / 32768.0 } else { l }]);
        }
    }
    if src.is_empty() {
        return Err(MusicError::Undecodable("no audio".into()));
    }
    // resample to 44.1 kHz (Catmull-Rom, below)
    const OUT: u32 = 44_100;
    let n_out = ((src.len() as u64 * OUT as u64) / rate as u64).max(1) as usize;
    let step = rate as f64 / OUT as f64;
    let data_len = (n_out * 4) as u32;
    let mut v = Vec::with_capacity(44 + data_len as usize);
    v.extend_from_slice(b"RIFF");
    v.extend_from_slice(&(36 + data_len).to_le_bytes());
    v.extend_from_slice(b"WAVEfmt ");
    v.extend_from_slice(&16u32.to_le_bytes());
    v.extend_from_slice(&1u16.to_le_bytes()); // PCM
    v.extend_from_slice(&2u16.to_le_bytes()); // stereo
    v.extend_from_slice(&OUT.to_le_bytes());
    v.extend_from_slice(&(OUT * 4).to_le_bytes());
    v.extend_from_slice(&4u16.to_le_bytes());
    v.extend_from_slice(&16u16.to_le_bytes());
    v.extend_from_slice(b"data");
    v.extend_from_slice(&data_len.to_le_bytes());
    for i in 0..n_out {
        let pos = i as f64 * step;
        let a = (pos as usize).min(src.len() - 1);
        let b = (a + 1) % src.len(); // wraps: the tracks are seamless loops
        let t = (pos - a as f64) as f32;
        // Catmull-Rom between the 4 nearest source frames: linear interpolation
        // dulls the top end audibly when upsampling 32 kHz tracks
        let p0 = src[(a + src.len() - 1) % src.len()];
        let p3 = src[(b + 1) % src.len()];
        for c in 0..2 {
            let (y0, y1, y2, y3) = (p0[c], src[a][c], src[b][c], p3[c]);
            let s = y1 + 0.5 * t * (y2 - y0 + t * (2.0 * y0 - 5.0 * y1 + 4.0 * y2 - y3 + t * (3.0 * (y1 - y2) + y3 - y0)));
            v.extend_from_slice(&((s.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes());
        }
    }
    Ok(v)
}

/// Run a future that is known to finish without waiting (quad-snd's native
/// load) to completion now.
#[cfg(not(target_arch = "wasm32"))]
fn now<F: std::future::Future>(f: F) -> Option<F::Output> {
    let mut f = std::pin::pin!(f);
    let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
    match f.as_mut().poll(&mut cx) {
        std::task::Poll::Ready(v) => Some(v),
        std::task::Poll::Pending => None,
    }
}

/// (WAV bytes or why not, file path, ms spent decoding on the thread)
#[cfg(not(target_arch = "wasm32"))]
type Pending = Arc<Mutex<Option<(Result<Vec<u8>, MusicError>, String, f64)>>>;

/// `CC_PERF` / `CC_AUTO` (native): log when each track is ready and what it cost.
#[cfg(not(target_arch = "wasm32"))]
fn log_loads() -> bool {
    std::env::var("CC_PERF").is_ok() || std::env::var("CC_AUTO").is_ok_and(|v| v == "1")
}

enum Load {
    #[cfg(not(target_arch = "wasm32"))]
    Thread(Pending),
    /// decoded on the thread, waiting for a frame that may upload it
    #[cfg(not(target_arch = "wasm32"))]
    Decoded(Vec<u8>, String, f64),
    #[cfg(target_arch = "wasm32")]
    Fetch(Coroutine<Option<Sound>>),
    Ready(Sound),
    Missing,
}

struct Slot {
    track: Track,
    load: Load,
    /// crossfade position 0..1
    level: f32,
    playing: bool,
    /// volume last sent to quad-snd
    sent: f32,
}

pub struct Music {
    /// None = music disabled (`CAMPUS_CRYPT_MUSIC_DIR=off`)
    dir: Option<String>,
    slots: Vec<Slot>,
}

impl Default for Music {
    fn default() -> Music {
        Music::new()
    }
}

impl Music {
    /// Loads nothing yet. Native: `CAMPUS_CRYPT_MUSIC_DIR` overrides the folder (`off` = no music).
    pub fn new() -> Music {
        #[cfg(not(target_arch = "wasm32"))]
        let dir = match std::env::var("CAMPUS_CRYPT_MUSIC_DIR") {
            Ok(v) if v == "off" => None,
            Ok(v) if !v.is_empty() => Some(v),
            _ => Some(native_dir()),
        };
        #[cfg(target_arch = "wasm32")]
        let dir = Some(DIR.to_string());
        Music { dir, slots: Vec::new() }
    }

    #[cfg(test)]
    fn with_dir(dir: &str) -> Music {
        Music { dir: Some(dir.to_string()), slots: Vec::new() }
    }

    fn slot(&self, t: Track) -> Option<&Slot> {
        self.slots.iter().find(|s| s.track == t)
    }

    /// Start loading `t` in the background unless it's already loaded or loading.
    fn request(&mut self, t: Track) {
        if self.slot(t).is_some() {
            return;
        }
        let load = match &self.dir {
            None => Load::Missing,
            Some(dir) => start_load(format!("{dir}/{}", t.file_name())),
        };
        self.slots.push(Slot { track: t, load, level: 0.0, playing: false, sent: 0.0 });
    }

    /// True once `t` is decoded and can play.
    #[cfg(test)]
    pub fn ready(&self, t: Track) -> bool {
        self.slot(t).is_some_and(|s| matches!(s.load, Load::Ready(_)))
    }

    /// True if `t` turned out to be missing or unplayable (it stays silent).
    #[cfg(test)]
    pub fn missing(&self, t: Track) -> bool {
        self.slot(t).is_some_and(|s| matches!(s.load, Load::Missing))
    }

    /// Call every frame: `want` plays (crossfading from whatever played
    /// before) at `volume`; `next` loads in the background for later.
    /// `allow_upload`: this frame may spend ~0.1 s finishing a native load
    /// (a black or static frame); ignored on the web.
    pub fn update(&mut self, want: Option<Track>, next: Option<Track>, volume: f32, dt: f32, allow_upload: bool) {
        if let Some(t) = want {
            self.request(t);
        }
        if let Some(t) = next {
            self.request(t);
        }
        for s in &mut self.slots {
            poll(s, allow_upload);
            let target = if Some(s.track) == want && matches!(s.load, Load::Ready(_)) { 1.0 } else { 0.0 };
            let step = dt.max(0.0) / CROSSFADE_SECS;
            s.level = if target > s.level { (s.level + step).min(1.0) } else { (s.level - step).max(0.0) };
            let vol = volume.clamp(0.0, 1.0) * s.level;
            if let Load::Ready(snd) = &s.load {
                if vol > 0.0005 {
                    if !s.playing {
                        play_sound(snd, PlaySoundParams { looped: true, volume: vol });
                        s.playing = true;
                    } else if (vol - s.sent).abs() > 0.002 {
                        set_sound_volume(snd, vol);
                    } else {
                        continue;
                    }
                    s.sent = vol;
                } else if s.playing {
                    stop_sound(snd);
                    s.playing = false;
                    s.sent = 0.0;
                }
            }
        }
        // free decoded tracks nobody needs (still-loading ones finish first; missing ones are remembered)
        self.slots.retain(|s| {
            let keep = Some(s.track) == want || Some(s.track) == next || s.playing || s.level > 0.0;
            keep || !matches!(s.load, Load::Ready(_))
        });
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn native_dir() -> String {
    let exe_dir = std::env::current_exe().ok().and_then(|e| e.parent().map(|d| d.to_path_buf()));
    let mut tries = vec![std::path::PathBuf::from(DIR)];
    if let Some(d) = exe_dir {
        tries.push(d.join(DIR));
        // macOS app bundle: Contents/MacOS/<exe> -> Contents/Resources/assets/audio
        tries.push(d.join("../Resources").join(DIR));
    }
    tries.iter().find(|p| p.is_dir()).unwrap_or(&tries[0]).to_string_lossy().into_owned()
}

#[cfg(not(target_arch = "wasm32"))]
fn start_load(path: String) -> Load {
    let out: Pending = Arc::new(Mutex::new(None));
    let res = out.clone();
    let spawned = std::thread::Builder::new().name("music".into()).spawn(move || {
        let t0 = std::time::Instant::now();
        let r = decode_to_wav(std::fs::read(&path).ok().as_deref());
        if let Err(e) = &r {
            eprintln!("[music] {path}: {e:?} (this track is silent)");
        }
        let ms = t0.elapsed().as_secs_f64() * 1000.0;
        if let Ok(mut g) = res.lock() {
            *g = Some((r, path, ms));
        }
    });
    if spawned.is_err() {
        return Load::Missing;
    }
    Load::Thread(out)
}

#[cfg(target_arch = "wasm32")]
fn start_load(path: String) -> Load {
    Load::Fetch(start_coroutine(async move {
        let bytes = macroquad::file::load_file(&path).await.ok();
        check_ogg(bytes.as_deref()).ok()?;
        // the browser decodes it; this resolves once the buffer is ready (never, if it can't decode)
        load_sound_from_bytes(bytes.as_deref()?).await.ok()
    }))
}

fn poll(s: &mut Slot, allow_upload: bool) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        if let Load::Thread(p) = &s.load {
            let done = p.lock().ok().and_then(|mut g| g.take());
            match done {
                None => {}
                Some((Ok(wav), path, decode_ms)) => s.load = Load::Decoded(wav, path, decode_ms),
                Some((Err(_), _, _)) => s.load = Load::Missing,
            }
        }
        if allow_upload {
            if let Load::Decoded(wav, path, decode_ms) = &s.load {
                let t0 = std::time::Instant::now();
                let sound = now(load_sound_from_bytes(wav)).and_then(Result::ok);
                if log_loads() {
                    let up = t0.elapsed().as_secs_f64() * 1000.0;
                    eprintln!("[music] {path} ready: decoded in {decode_ms:.0} ms on the loader thread, {up:.0} ms upload on a black/title frame");
                }
                s.load = sound.map_or(Load::Missing, Load::Ready);
            }
        }
    }
    #[cfg(target_arch = "wasm32")]
    let _ = allow_upload;
    #[cfg(target_arch = "wasm32")]
    if let Load::Fetch(co) = &s.load {
        if co.is_done() {
            s.load = co.retrieve().flatten().map_or(Load::Missing, Load::Ready);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tracks_name_their_files() {
        assert_eq!(Track::Title.file_name(), "title_march.ogg");
        assert_eq!(Track::for_floor(0), Track::Floor(1));
        assert_eq!(Track::for_floor(9).file_name(), "floor_10.ogg");
        assert_eq!(Track::for_floor(10), Track::Floor(6), "floor 11 opens on a later theme, not floor 1");
        assert_ne!(Track::for_floor(10), Track::for_floor(0));
        let back: Vec<u8> = (10..20)
            .map(|f| match Track::for_floor(f) {
                Track::Floor(n) => n,
                Track::Title => 0,
            })
            .collect();
        assert_eq!(back, vec![6, 7, 8, 8, 9, 9, 10, 10, 10, 10]);
        assert!(back.windows(2).all(|w| w[1] >= w[0]), "the back half rises: {back:?}");
        assert!(back.iter().all(|&n| (6..=10).contains(&n)));
        assert_eq!(Track::for_floor(19), Track::Floor(10));
        assert_eq!(Track::for_floor(20), Track::Floor(6), "past the shipped run, the rise repeats");
    }

    #[test]
    fn missing_or_broken_files_are_rejected_before_quad_snd_sees_them() {
        assert_eq!(decode_to_wav(None), Err(MusicError::Missing));
        assert_eq!(decode_to_wav(Some(&[])), Err(MusicError::Missing));
        assert_eq!(decode_to_wav(Some(b"RIFF....WAVEfmt ")), Err(MusicError::NotOgg));
        let mut junk = b"OggS".to_vec();
        junk.extend(std::iter::repeat_n(7u8, 400));
        assert!(matches!(decode_to_wav(Some(&junk)), Err(MusicError::Undecodable(_))));
    }

    /// With no files at all, every track settles as silent and nothing panics
    /// or touches the audio device.
    #[test]
    fn a_missing_file_falls_back_to_silence() {
        let mut m = Music::with_dir("/nonexistent/campus-crypt-audio");
        for _ in 0..2000 {
            m.update(Some(Track::Title), Some(Track::Floor(1)), 1.0, 1.0 / 60.0, true);
            if m.missing(Track::Title) && m.missing(Track::Floor(1)) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert!(m.missing(Track::Title) && m.missing(Track::Floor(1)));
        assert!(!m.ready(Track::Title));
        // a missing track is remembered (no reload every frame) and stays silent
        m.update(Some(Track::Floor(2)), None, 1.0, 0.5, false);
        assert!(m.slots.iter().all(|s| !s.playing));
        let mut off = Music { dir: None, slots: Vec::new() };
        off.update(Some(Track::Title), None, 1.0, 0.1, true);
        assert!(off.missing(Track::Title), "music off = silence");
    }

    /// Every shipped track decodes (whichever are present).
    #[test]
    fn shipped_tracks_decode() {
        let dir = format!("{}/{DIR}", env!("CARGO_MANIFEST_DIR"));
        let tracks = std::iter::once(Track::Title).chain((0..FLOOR_TRACKS).map(Track::for_floor));
        for t in tracks {
            let path = format!("{dir}/{}", t.file_name());
            if let Ok(b) = std::fs::read(&path) {
                let wav = decode_to_wav(Some(&b)).unwrap_or_else(|e| panic!("{path}: {e:?}"));
                assert!(wav.len() > 44 + 44_100 * 4, "{path}: under a second of audio");
            }
        }
    }
}
