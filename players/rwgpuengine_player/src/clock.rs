//! The player's clock: fixed steps at the project's rate, whatever the
//! display does.
//!
//! The engine crates never read the time (ADR 0005); the player is where the
//! clock lives. [`Steps`] is plain arithmetic on nanoseconds, so it is tested
//! without waiting, and both players use it; [`Watch`] is the one place that
//! reads the wall clock: `std::time::Instant` natively, the page's
//! `performance.now()` in a browser, where `Instant` does not exist.

// The player reads the wall clock on purpose: it is what paces the steps and
// times the frames. The ban (clippy.toml) is for the engine crates.
#![allow(clippy::disallowed_methods)]

use std::time::Duration;
#[cfg(not(target_arch = "wasm32"))]
use std::time::Instant;

/// The most steps taken at one wake-up. A player that falls further behind
/// (a slow frame, a window being dragged, a laptop waking up) takes these
/// and lets the rest of the lost time go: the game slows down for a moment
/// rather than racing to catch up, and a step is never longer than another.
pub const MAX_CATCH_UP: u32 = 5;

const SECOND: u128 = 1_000_000_000;

/// When each fixed step is due.
#[derive(Clone, Debug)]
pub struct Steps {
    rate: u128,
    /// Time (ns since the start) that step 0 of the count below is due at.
    origin: u128,
    /// Steps counted from `origin`.
    taken: u128,
    /// Wake-ups that found more than [`MAX_CATCH_UP`] steps due.
    pub behind: u64,
    /// Steps that were due and were let go.
    pub dropped: u64,
}

impl Steps {
    /// `rate` steps per second (a project's `tick_rate`).
    pub fn new(rate: u32) -> Steps {
        Steps { rate: u128::from(rate.max(1)), origin: 0, taken: 0, behind: 0, dropped: 0 }
    }

    fn at(&self, step: u128) -> u128 {
        self.origin + step * SECOND / self.rate
    }

    /// How many steps to take now, `now` being the time since the start.
    /// At most [`MAX_CATCH_UP`].
    pub fn due(&mut self, now: Duration) -> u32 {
        let now = now.as_nanos();
        let mut n = 0;
        while self.at(self.taken + 1) <= now {
            if n == MAX_CATCH_UP {
                // Count from here: the next step is due one step from now.
                let late = (now - self.at(self.taken)) * self.rate / SECOND;
                self.behind += 1;
                self.dropped += late as u64;
                self.origin = now;
                self.taken = 0;
                break;
            }
            self.taken += 1;
            n += 1;
        }
        n
    }

    /// When the next step is due, as time since the start.
    pub fn next(&self) -> Duration {
        let ns = self.at(self.taken + 1);
        Duration::new((ns / SECOND) as u64, (ns % SECOND) as u32)
    }
}

/// The wall clock: the start of the run, and timings.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Clone, Copy, Debug)]
pub struct Watch(Instant);

#[cfg(not(target_arch = "wasm32"))]
impl Watch {
    pub fn start() -> Watch {
        Watch(Instant::now())
    }

    pub fn elapsed(&self) -> Duration {
        self.0.elapsed()
    }

    /// The moment `since` after the start, for the event loop to wait for.
    pub fn at(&self, since: Duration) -> Instant {
        self.0 + since
    }

    /// Microseconds since the start, for timings.
    pub fn micros(&self) -> u64 {
        self.0.elapsed().as_micros() as u64
    }
}

/// The wall clock in a browser: `performance.now()`, in milliseconds since
/// the page started, with a fraction (browsers round it to 5 µs or 100 µs).
#[cfg(target_arch = "wasm32")]
#[derive(Clone, Copy, Debug)]
pub struct Watch(f64);

#[cfg(target_arch = "wasm32")]
mod page {
    use wasm_bindgen::prelude::wasm_bindgen;

    #[wasm_bindgen]
    extern "C" {
        #[wasm_bindgen(js_namespace = performance, js_name = now)]
        pub fn now() -> f64;
    }
}

#[cfg(target_arch = "wasm32")]
impl Watch {
    pub fn start() -> Watch {
        Watch(page::now())
    }

    /// The watch started at `ms` on the page's clock (an animation frame's
    /// time stamp, say).
    pub fn started_at(ms: f64) -> Watch {
        Watch(ms)
    }

    /// The time since the start at `ms` on the page's clock.
    pub fn since(&self, ms: f64) -> Duration {
        Duration::from_secs_f64(((ms - self.0) / 1000.0).max(0.0))
    }

    pub fn elapsed(&self) -> Duration {
        self.since(page::now())
    }

    /// Microseconds since the start, for timings.
    pub fn micros(&self) -> u64 {
        self.elapsed().as_micros() as u64
    }
}

/// Timings in microseconds, summed up as a median and a maximum.
#[derive(Clone, Debug, Default)]
pub struct Timings(Vec<u64>);

impl Timings {
    pub fn add(&mut self, us: u64) {
        self.0.push(us);
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// All of them together.
    pub fn sum(&self) -> u64 {
        self.0.iter().sum()
    }

    /// (median, maximum), or zeros when nothing was timed.
    pub fn summary(&self) -> (u64, u64) {
        let mut v = self.0.clone();
        v.sort_unstable();
        (v.get(v.len() / 2).copied().unwrap_or(0), v.last().copied().unwrap_or(0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MS: u64 = 1_000_000;

    fn ns(n: u64) -> Duration {
        Duration::from_nanos(n)
    }

    #[test]
    fn steps_come_at_the_fixed_rate_whatever_the_display_does() {
        // Woken at 60, 144 and 30 frames a second for one second: 60 steps each time.
        for every in [16_666_667u64, 6_944_444, 33_333_333] {
            let mut s = Steps::new(60);
            let mut total = 0;
            let mut t = 0;
            while t < 1_000 * MS {
                total += s.due(ns(t));
                t += every;
            }
            total += s.due(ns(1_000 * MS));
            assert_eq!(total, 60, "woken every {every} ns");
            assert_eq!((s.behind, s.dropped), (0, 0));
        }
        // Waking exactly when told: one step each time, never early.
        let mut s = Steps::new(60);
        assert_eq!(s.due(ns(0)), 0);
        for k in 1..=120u64 {
            let next = s.next();
            assert_eq!(next.as_nanos(), u128::from(k) * 1_000_000_000 / 60);
            assert_eq!(s.due(next - ns(1)), 0);
            assert_eq!(s.due(next), 1);
        }
    }

    #[test]
    fn a_player_that_falls_behind_catches_up_by_a_bounded_number_of_steps() {
        let mut s = Steps::new(60);
        assert_eq!(s.due(ns(100 * MS)), 5); // 6 were due
        assert_eq!((s.behind, s.dropped), (1, 1));
        // The next step is one step after the wake-up that fell behind.
        assert_eq!(s.next(), ns(100 * MS) + ns(16_666_666));
        assert_eq!(s.due(ns(117 * MS)), 1);
        // Two seconds lost: five steps, the rest let go.
        assert_eq!(s.due(ns(2_117 * MS)), 5);
        assert_eq!((s.behind, s.dropped), (2, 1 + 115));
        assert_eq!(s.due(ns(2_117 * MS + 16_666_667)), 1);
    }

    #[test]
    fn timings_sum_up_as_median_and_maximum() {
        let mut t = Timings::default();
        assert_eq!(t.summary(), (0, 0));
        for us in [30, 10, 20, 400, 25] {
            t.add(us);
        }
        assert_eq!(t.summary(), (25, 400));
    }
}
