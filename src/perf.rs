#![allow(unexpected_cfgs)]
//! Tiny opt-in profiler (native only; inert on the web).
//!
//! `CC_PERF=1` prints, at exit, per screen/phase: frames, average and worst CPU
//! time spent building the frame (`App::frame`, i.e. game logic + draw-call
//! submission, not GPU time), and the average wall time between frames.
//! Build with `RUSTFLAGS="--cfg cc_alloc_count"` to also count heap
//! allocations per frame (a counting global allocator; off in normal builds).

#[cfg(cc_alloc_count)]
mod alloc_count {
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::sync::atomic::{AtomicU64, Ordering};
    pub static ALLOCS: AtomicU64 = AtomicU64::new(0);
    pub static BYTES: AtomicU64 = AtomicU64::new(0);
    pub struct Counting;
    unsafe impl GlobalAlloc for Counting {
        unsafe fn alloc(&self, l: Layout) -> *mut u8 {
            ALLOCS.fetch_add(1, Ordering::Relaxed);
            BYTES.fetch_add(l.size() as u64, Ordering::Relaxed);
            System.alloc(l)
        }
        unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
            System.dealloc(p, l)
        }
        unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
            ALLOCS.fetch_add(1, Ordering::Relaxed);
            BYTES.fetch_add(n as u64, Ordering::Relaxed);
            System.realloc(p, l, n)
        }
    }
    #[global_allocator]
    static GLOBAL: Counting = Counting;
}

fn alloc_counts() -> (u64, u64) {
    #[cfg(cc_alloc_count)]
    {
        use std::sync::atomic::Ordering;
        return (alloc_count::ALLOCS.load(Ordering::Relaxed), alloc_count::BYTES.load(Ordering::Relaxed));
    }
    #[allow(unreachable_code)]
    (0, 0)
}

#[derive(Default, Clone)]
struct Stat {
    frames: u64,
    cpu_total: f64,
    cpu_max: f64,
    dt_total: f64,
    allocs: u64,
    bytes: u64,
}

pub struct Perf {
    on: bool,
    #[cfg(not(target_arch = "wasm32"))]
    start: Option<std::time::Instant>,
    a0: (u64, u64),
    stats: Vec<(&'static str, Stat)>,
    skip: u32,
}

impl Perf {
    pub fn from_env() -> Perf {
        #[cfg(not(target_arch = "wasm32"))]
        let on = std::env::var("CC_PERF").is_ok();
        #[cfg(target_arch = "wasm32")]
        let on = false;
        Perf {
            on,
            #[cfg(not(target_arch = "wasm32"))]
            start: None,
            a0: (0, 0),
            stats: Vec::new(),
            skip: 3, // ignore the first frames (startup, shader compile)
        }
    }

    pub fn begin(&mut self) {
        if !self.on {
            return;
        }
        self.a0 = alloc_counts();
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.start = Some(std::time::Instant::now());
        }
    }

    pub fn end(&mut self, tag: &'static str, dt: f32) {
        if !self.on {
            return;
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let cpu = self.start.map_or(0.0, |s| s.elapsed().as_secs_f64() * 1000.0);
            let (a, b) = alloc_counts();
            if self.skip > 0 {
                self.skip -= 1;
                return;
            }
            let i = match self.stats.iter().position(|(t, _)| *t == tag) {
                Some(i) => i,
                None => {
                    self.stats.push((tag, Stat::default()));
                    self.stats.len() - 1
                }
            };
            let s = &mut self.stats[i].1;
            s.frames += 1;
            s.cpu_total += cpu;
            s.cpu_max = s.cpu_max.max(cpu);
            s.dt_total += dt as f64 * 1000.0;
            s.allocs += a - self.a0.0;
            s.bytes += b - self.a0.1;
        }
        #[cfg(target_arch = "wasm32")]
        let _ = (tag, dt);
    }

    pub fn report(&self) {
        if !self.on {
            return;
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            eprintln!("[perf] {:<10} {:>7} {:>10} {:>10} {:>10} {:>12} {:>12}", "phase", "frames", "cpu avg ms", "cpu max ms", "frame ms", "allocs/frame", "bytes/frame");
            for (t, s) in &self.stats {
                let n = s.frames.max(1) as f64;
                eprintln!(
                    "[perf] {:<10} {:>7} {:>10.3} {:>10.3} {:>10.2} {:>12.1} {:>12.0}",
                    t,
                    s.frames,
                    s.cpu_total / n,
                    s.cpu_max,
                    s.dt_total / n,
                    s.allocs as f64 / n,
                    s.bytes as f64 / n
                );
            }
        }
    }
}
