//! Procedurally generated sound effects (no asset files).
//! Every sound is synthesised into an in-memory 16-bit mono WAV at startup.
//! Music (one looping file per screen / floor, loaded lazily) is in music.rs.

use macroquad::audio::{load_sound_from_bytes, play_sound, PlaySoundParams, Sound};
use std::f32::consts::TAU;

const SR: u32 = 22050;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Sfx {
    Hit,
    Swing,
    Coin,
    Potion,
    LevelUp,
    Hurt,
    Stairs,
    Shoot,
    Death,
    Click,
    Equip,
    /// the foam-dart blaster
    Pew,
}

const SFX_COUNT: usize = 12;

pub struct Audio {
    sounds: Vec<Sound>,
    last_played: [f64; SFX_COUNT],
}

struct Rng(u32);
impl Rng {
    fn next(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        (self.0 as f32 / u32::MAX as f32) * 2.0 - 1.0
    }
}

fn wav_bytes(samples: &[f32]) -> Vec<u8> {
    let data_len = (samples.len() * 2) as u32;
    let mut v = Vec::with_capacity(44 + data_len as usize);
    v.extend_from_slice(b"RIFF");
    v.extend_from_slice(&(36 + data_len).to_le_bytes());
    v.extend_from_slice(b"WAVEfmt ");
    v.extend_from_slice(&16u32.to_le_bytes());
    v.extend_from_slice(&1u16.to_le_bytes()); // PCM
    v.extend_from_slice(&1u16.to_le_bytes()); // mono
    v.extend_from_slice(&SR.to_le_bytes());
    v.extend_from_slice(&(SR * 2).to_le_bytes());
    v.extend_from_slice(&2u16.to_le_bytes());
    v.extend_from_slice(&16u16.to_le_bytes());
    v.extend_from_slice(b"data");
    v.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        let x = (s.clamp(-1.0, 1.0) * 32000.0) as i16;
        v.extend_from_slice(&x.to_le_bytes());
    }
    v
}

fn buf(secs: f32) -> Vec<f32> {
    vec![0.0; (secs * SR as f32) as usize]
}

fn square(phase: f32) -> f32 {
    if phase.fract() < 0.5 {
        1.0
    } else {
        -1.0
    }
}

/// Add a tone with an exponential-ish decay envelope.
fn tone(out: &mut [f32], start: f32, dur: f32, f0: f32, f1: f32, amp: f32, wave: u8) {
    let s0 = (start * SR as f32) as usize;
    let n = (dur * SR as f32) as usize;
    let mut phase = 0.0f32;
    for i in 0..n {
        let idx = s0 + i;
        if idx >= out.len() {
            break;
        }
        let t = i as f32 / n as f32;
        let f = f0 + (f1 - f0) * t;
        phase += f / SR as f32;
        let env = (1.0 - t).powf(1.6) * (i as f32 / 60.0).min(1.0);
        let w = match wave {
            0 => (phase * TAU).sin(),
            1 => square(phase) * 0.5,
            _ => (phase.fract() * 2.0 - 1.0) * 0.6, // saw
        };
        out[idx] += w * env * amp;
    }
}

fn noise(out: &mut [f32], start: f32, dur: f32, amp: f32, smooth: f32, seed: u32) {
    let mut rng = Rng(seed);
    let s0 = (start * SR as f32) as usize;
    let n = (dur * SR as f32) as usize;
    let mut lp = 0.0f32;
    for i in 0..n {
        let idx = s0 + i;
        if idx >= out.len() {
            break;
        }
        let t = i as f32 / n as f32;
        lp += (rng.next() - lp) * smooth;
        out[idx] += lp * (1.0 - t).powf(2.0) * amp;
    }
}

fn synth(s: Sfx) -> Vec<f32> {
    match s {
        Sfx::Hit => {
            let mut b = buf(0.14);
            noise(&mut b, 0.0, 0.1, 0.9, 0.6, 1234);
            tone(&mut b, 0.0, 0.14, 180.0, 60.0, 0.8, 0);
            b
        }
        Sfx::Swing => {
            let mut b = buf(0.12);
            noise(&mut b, 0.0, 0.12, 0.35, 0.15, 99);
            b
        }
        Sfx::Coin => {
            let mut b = buf(0.18);
            tone(&mut b, 0.0, 0.07, 988.0, 988.0, 0.35, 1);
            tone(&mut b, 0.06, 0.12, 1319.0, 1319.0, 0.35, 1);
            b
        }
        Sfx::Potion => {
            let mut b = buf(0.35);
            for k in 0..5 {
                tone(&mut b, k as f32 * 0.05, 0.12, 400.0 + k as f32 * 120.0, 700.0 + k as f32 * 120.0, 0.35, 0);
            }
            b
        }
        Sfx::LevelUp => {
            let mut b = buf(0.9);
            let notes = [523.25, 659.25, 783.99, 1046.5];
            for (i, f) in notes.iter().enumerate() {
                tone(&mut b, i as f32 * 0.09, 0.5, *f, *f, 0.28, 1);
                tone(&mut b, i as f32 * 0.09, 0.6, *f * 2.0, *f * 2.0, 0.12, 0);
            }
            b
        }
        Sfx::Hurt => {
            let mut b = buf(0.25);
            tone(&mut b, 0.0, 0.25, 320.0, 110.0, 0.6, 2);
            noise(&mut b, 0.0, 0.12, 0.5, 0.4, 7);
            b
        }
        Sfx::Stairs => {
            let mut b = buf(0.9);
            for k in 0..4 {
                tone(&mut b, k as f32 * 0.16, 0.3, 392.0 / (1.0 + k as f32 * 0.25), 300.0 / (1.0 + k as f32 * 0.25), 0.3, 0);
            }
            noise(&mut b, 0.0, 0.9, 0.25, 0.03, 42);
            b
        }
        Sfx::Shoot => {
            let mut b = buf(0.16);
            tone(&mut b, 0.0, 0.16, 700.0, 200.0, 0.35, 1);
            noise(&mut b, 0.0, 0.05, 0.3, 0.5, 5);
            b
        }
        Sfx::Death => {
            let mut b = buf(0.3);
            noise(&mut b, 0.0, 0.3, 0.7, 0.25, 77);
            tone(&mut b, 0.0, 0.3, 220.0, 50.0, 0.5, 0);
            b
        }
        Sfx::Click => {
            let mut b = buf(0.05);
            tone(&mut b, 0.0, 0.05, 1200.0, 900.0, 0.3, 1);
            b
        }
        Sfx::Equip => {
            let mut b = buf(0.25);
            noise(&mut b, 0.0, 0.06, 0.4, 0.8, 3);
            tone(&mut b, 0.02, 0.2, 1568.0, 1568.0, 0.25, 0);
            tone(&mut b, 0.02, 0.2, 2093.0, 2093.0, 0.12, 0);
            b
        }
        Sfx::Pew => {
            // a soft foam "fwip": a puff of air and a quick upward chirp
            let mut b = buf(0.09);
            noise(&mut b, 0.0, 0.05, 0.25, 0.5, 41);
            tone(&mut b, 0.0, 0.07, 520.0, 980.0, 0.18, 0);
            b
        }
    }
}

impl Audio {
    pub async fn new() -> Audio {
        let all = [
            Sfx::Hit,
            Sfx::Swing,
            Sfx::Coin,
            Sfx::Potion,
            Sfx::LevelUp,
            Sfx::Hurt,
            Sfx::Stairs,
            Sfx::Shoot,
            Sfx::Death,
            Sfx::Click,
            Sfx::Equip,
            Sfx::Pew,
        ];
        let mut sounds = Vec::with_capacity(SFX_COUNT);
        for s in all {
            let bytes = wav_bytes(&synth(s));
            match load_sound_from_bytes(&bytes).await {
                Ok(snd) => sounds.push(snd),
                Err(_) => return Audio::silent(),
            }
        }
        Audio { sounds, last_played: [-1.0; SFX_COUNT] }
    }

    fn silent() -> Audio {
        Audio { sounds: Vec::new(), last_played: [-1.0; SFX_COUNT] }
    }

    pub fn play(&mut self, s: Sfx, volume: f32) {
        let i = s as usize;
        if volume <= 0.001 || i >= self.sounds.len() {
            return;
        }
        // throttle identical sounds that fire in the same instant
        let now = macroquad::time::get_time();
        if now - self.last_played[i] < 0.045 {
            return;
        }
        self.last_played[i] = now;
        play_sound(
            &self.sounds[i],
            PlaySoundParams {
                looped: false,
                volume,
            },
        );
    }
}
