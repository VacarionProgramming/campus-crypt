//! Dungeon-layout RNG, separate from cosmetic and combat randomness.
//!
//! `macroquad::rand` is one global stream. Particles, screen shake, dart spread
//! and enemy animation offsets all draw from it, and they do so at different
//! times on a 60 Hz desktop build and a phone. If floor generation shared that
//! stream, the same seed would not build the same crypt on both.
//!
//! This module is the other stream. It is consulted only while placing rooms,
//! corridors, enemies, coffees and side props, and while a daily run is
//! installed. With nothing installed, every call forwards to `macroquad::rand`
//! in the same order as before, so a normal run is unchanged.
//!
//! Keeper question picks do not use this stream. They already come from the
//! run seed (`keeper::mix` / `investigate::Case::build`). A daily run sets that
//! seed from the low 32 bits of the daily u64, so those picks are deterministic
//! too and still do not consume layout draws.

use std::cell::RefCell;

thread_local! {
    static LAYOUT: RefCell<Option<Rng>> = const { RefCell::new(None) };
}

/// xorshift64*, seeded with splitmix64 so a zero seed is not a stuck state.
#[derive(Clone, Debug)]
pub struct Rng {
    state: u64,
}

impl Rng {
    pub fn new(seed: u64) -> Rng {
        let mut z = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^= z >> 31;
        if z == 0 {
            z = 0x9E37_79B9_7F4A_7C15;
        }
        Rng { state: z }
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.state = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
}

/// Install `seed` for the rest of this thread until [`clear`] or the guard drops.
pub fn install(seed: u64) {
    LAYOUT.with(|slot| *slot.borrow_mut() = Some(Rng::new(seed)));
}

pub fn clear() {
    LAYOUT.with(|slot| *slot.borrow_mut() = None);
}

/// Install `seed` and restore whatever stream was active when this drops.
#[must_use]
#[cfg_attr(not(test), allow(dead_code))]
pub struct Guard {
    prev: Option<Rng>,
}

impl Drop for Guard {
    fn drop(&mut self) {
        let prev = self.prev.take();
        LAYOUT.with(|slot| *slot.borrow_mut() = prev);
    }
}

#[cfg_attr(not(test), allow(dead_code))]
pub fn guard(seed: u64) -> Guard {
    LAYOUT.with(|slot| Guard {
        prev: slot.borrow_mut().replace(Rng::new(seed)),
    })
}

/// A number from `low` inclusive to `high` exclusive.
/// With no layout stream installed, this is `macroquad::rand::gen_range`.
pub fn gen_range<T: Sample>(low: T, high: T) -> T {
    let drawn = LAYOUT.with(|slot| {
        let mut slot = slot.borrow_mut();
        slot.as_mut().map(|rng| T::from_rng(rng, low, high))
    });
    drawn.unwrap_or_else(|| T::from_global(low, high))
}

pub trait Sample: Copy {
    fn from_rng(rng: &mut Rng, low: Self, high: Self) -> Self;
    fn from_global(low: Self, high: Self) -> Self;
}

fn span_u64(rng: &mut Rng, span: u64) -> u64 {
    if span == 0 {
        0
    } else {
        rng.next_u64() % span
    }
}

impl Sample for i32 {
    fn from_rng(rng: &mut Rng, low: i32, high: i32) -> i32 {
        if high <= low {
            return low;
        }
        low + span_u64(rng, (high as i64 - low as i64) as u64) as i32
    }
    fn from_global(low: i32, high: i32) -> i32 {
        macroquad::rand::gen_range(low, high)
    }
}

impl Sample for u32 {
    fn from_rng(rng: &mut Rng, low: u32, high: u32) -> u32 {
        if high <= low {
            return low;
        }
        low + span_u64(rng, (high - low) as u64) as u32
    }
    fn from_global(low: u32, high: u32) -> u32 {
        macroquad::rand::gen_range(low, high)
    }
}

impl Sample for usize {
    fn from_rng(rng: &mut Rng, low: usize, high: usize) -> usize {
        if high <= low {
            return low;
        }
        low + span_u64(rng, (high - low) as u64) as usize
    }
    fn from_global(low: usize, high: usize) -> usize {
        macroquad::rand::gen_range(low, high)
    }
}
