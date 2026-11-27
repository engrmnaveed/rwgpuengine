//! The measurement behind [`Tuning`]: how two fingers are told apart as a
//! pan, a pinch and a twist (ADR 0030).
//!
//! A seeded set of two-finger gestures is played through [`Input`] the way a
//! touch screen sends them: one finger's move, then the other's, 24 times,
//! each point off by up to 1.5 frame pixels. Each gesture means one thing
//! and is made with a little of the others, as hands do:
//!
//! - **pan**: both fingers move 60 to 300 pixels one way; their spread and
//!   their angle drift.
//! - **pinch**: the spread goes to 0.5–0.8 or 1.25–2 times, about their
//!   midpoint or with one finger still; the angle and the midpoint drift.
//! - **twist**: the angle turns 20 to 90 degrees either way, meant as a
//!   quarter turn; the spread and the midpoint drift.
//! - **two-finger tap**: two fingers down and up, only the jitter.
//!
//! The drift has three sizes (low, medium, high): angle ±5/10/20 degrees,
//! spread ±5/10/20 %, midpoint 10/20/40 pixels. These are a model, not
//! measured hands: no person's touches were recorded here. What it settles
//! is how each choice trades a turn nobody meant against a turn that does
//! not happen, at the same drift.
//!
//! `cargo test -p rwgpuengine_player gesture_table -- --ignored --nocapture`
//! prints the table of ADR 0030; the test beside it holds the chosen tuning
//! to its row.

use rwgpuengine_core::math::sin_cos_deg64;
use rwgpuengine_core::rng::Rng;

use crate::input::{Does, Gesture, Input, TWIST_DEG, Tuning};

#[derive(Clone, Copy, Debug, PartialEq)]
enum Kind {
    Pan,
    Pinch,
    Twist,
    Tap,
}

/// One synthetic gesture: how big what it means is, and the fingers' path.
struct Made {
    /// Pinch: the spread's factor. Twist: the angle in degrees.
    size: f64,
    /// Per sample: both fingers.
    path: Vec<[[f32; 2]; 2]>,
}

/// Drift: angle (degrees), spread (fraction), midpoint (pixels).
#[derive(Clone, Copy)]
struct Drift {
    name: &'static str,
    angle: f64,
    spread: f64,
    mid: f64,
}

const DRIFTS: [Drift; 3] = [
    Drift { name: "low", angle: 5.0, spread: 0.05, mid: 10.0 },
    Drift { name: "medium", angle: 10.0, spread: 0.10, mid: 20.0 },
    Drift { name: "high", angle: 20.0, spread: 0.20, mid: 40.0 },
];

const SAMPLES: usize = 24;
const JITTER: f64 = 1.5;

fn uniform(rng: &mut Rng, lo: f64, hi: f64) -> f64 {
    lo + (hi - lo) * f64::from(rng.next_u32()) / f64::from(u32::MAX)
}

fn sign(rng: &mut Rng) -> f64 {
    if rng.below(2) == 0 { -1.0 } else { 1.0 }
}

fn make(rng: &mut Rng, kind: Kind, d: Drift) -> Made {
    let d0 = uniform(rng, 60.0, 250.0);
    let phi0 = uniform(rng, 0.0, 180.0);
    let c0 = [uniform(rng, 150.0, 490.0), uniform(rng, 120.0, 360.0)];
    // What it means, and the drift of the other two.
    let mut spread = 1.0 + uniform(rng, -d.spread, d.spread);
    let mut angle = uniform(rng, -d.angle, d.angle);
    let (mut size, mut anchored) = (0.0, false);
    let dir = uniform(rng, 0.0, 360.0);
    let (ds, dc) = sin_cos_deg64(dir);
    let drift = uniform(rng, 0.0, d.mid);
    let mut mid = [dc * drift, ds * drift];
    match kind {
        Kind::Pan => {
            let t = uniform(rng, 60.0, 300.0);
            mid = [mid[0] + dc * t, mid[1] + ds * t];
        }
        Kind::Pinch => {
            spread = if rng.below(2) == 0 { uniform(rng, 0.5, 0.8) } else { uniform(rng, 1.25, 2.0) };
            size = spread;
            anchored = rng.below(2) == 0;
        }
        Kind::Twist => {
            angle = sign(rng) * uniform(rng, 20.0, 90.0);
            size = angle;
        }
        Kind::Tap => {
            spread = 1.0;
            angle = 0.0;
            mid = [0.0, 0.0];
        }
    }
    let mut path = Vec::with_capacity(SAMPLES + 1);
    for k in 0..=SAMPLES {
        let t = k as f64 / SAMPLES as f64;
        let e = if kind == Kind::Tap { 0.0 } else { t * t * (3.0 - 2.0 * t) };
        let (half, phi) = (d0 / 2.0 * (1.0 + (spread - 1.0) * e), phi0 + angle * e);
        let (s, c) = sin_cos_deg64(phi);
        let mut m = [c0[0] + mid[0] * e, c0[1] + mid[1] * e];
        if anchored {
            // One finger stays: the midpoint follows the spread.
            let (s0, c0d) = sin_cos_deg64(phi0);
            let a0 = [c0[0] - c0d * d0 / 2.0, c0[1] - s0 * d0 / 2.0];
            m = [a0[0] + c * half + mid[0] * e, a0[1] + s * half + mid[1] * e];
        }
        let mut f = [[m[0] - c * half, m[1] - s * half], [m[0] + c * half, m[1] + s * half]];
        for p in &mut f {
            p[0] += uniform(rng, -JITTER, JITTER);
            p[1] += uniform(rng, -JITTER, JITTER);
        }
        path.push(f.map(|p| [p[0] as f32, p[1] as f32]));
    }
    Made { size, path }
}

/// What a gesture did through `Input`: the zoom it made, the quarter turns
/// it settled on, and the key it gave.
#[derive(Debug, Default)]
struct Did {
    zoom: f64,
    quarters: i32,
    key: Option<Does>,
}

fn play(made: &Made, tuning: Tuning) -> Did {
    let mut i = Input::default();
    i.tuning = tuning;
    i.set_drag_pans(true);
    let mut did = Did { zoom: 1.0, ..Did::default() };
    let take = |g: Option<Gesture>, did: &mut Did| match g {
        Some(Gesture::Grab { zoom, .. }) => did.zoom *= f64::from(zoom),
        Some(Gesture::Settle { quarters, .. }) => did.quarters = quarters,
        Some(Gesture::Key(k)) => did.key = Some(k),
        _ => {}
    };
    let first = made.path[0];
    take(i.finger_down(1, first[0], true), &mut did);
    take(i.finger_down(2, first[1], true), &mut did);
    for f in &made.path[1..] {
        take(i.finger_moved(1, f[0]), &mut did);
        take(i.finger_moved(2, f[1]), &mut did);
    }
    let last = made.path[SAMPLES];
    take(i.finger_up(1, last[0], true), &mut did);
    take(i.finger_up(2, last[1], true), &mut did);
    did
}

/// Rates over the set, in percent.
#[derive(Debug, Default)]
struct Rates {
    /// Pans and pinches that turned the view a quarter turn.
    false_turn: f64,
    /// Twists of 20–45 / 45–90 degrees that settled a quarter turn the way they twisted.
    turned_small: f64,
    turned_large: f64,
    /// Pans whose zoom changed by more than 5 %.
    pan_zoomed: f64,
    /// Pinches whose zoom is more than 5 % from the fingers' spread.
    pinch_off: f64,
    /// Twists whose zoom changed by more than 10 %.
    twist_zoomed: f64,
    /// Two-finger taps that switched the view.
    tapped: f64,
}

fn measure(tuning: Tuning, d: Drift, n: usize) -> Rates {
    let mut rng = Rng::new(0x0030);
    let mut count = [0usize; 7];
    let mut of = [0usize; 7];
    let pct = |k: usize, count: &[usize; 7], of: &[usize; 7]| 100.0 * count[k] as f64 / of[k].max(1) as f64;
    for j in 0..n {
        let kind = [Kind::Pan, Kind::Pinch, Kind::Twist, Kind::Tap][j % 4];
        let made = make(&mut rng, kind, d);
        let did = play(&made, tuning);
        let zoomed = |by: f64| (did.zoom / 1.0).max(1.0 / did.zoom) > 1.0 + by;
        match kind {
            Kind::Pan | Kind::Pinch => {
                of[0] += 1;
                count[0] += usize::from(did.quarters != 0);
                if kind == Kind::Pan {
                    of[3] += 1;
                    count[3] += usize::from(zoomed(0.05));
                } else {
                    let r = did.zoom / made.size;
                    of[4] += 1;
                    count[4] += usize::from(r.max(1.0 / r) > 1.05);
                }
            }
            Kind::Twist => {
                let k = if made.size.abs() < 45.0 { 1 } else { 2 };
                let want = if made.size > 0.0 { 1 } else { -1 };
                of[k] += 1;
                count[k] += usize::from(did.quarters == want);
                of[5] += 1;
                count[5] += usize::from(zoomed(0.10));
            }
            Kind::Tap => {
                of[6] += 1;
                count[6] += usize::from(did.key == Some(Does::View) && did.zoom == 1.0);
            }
        }
    }
    Rates {
        false_turn: pct(0, &count, &of),
        turned_small: pct(1, &count, &of),
        turned_large: pct(2, &count, &of),
        pan_zoomed: pct(3, &count, &of),
        pinch_off: pct(4, &count, &of),
        twist_zoomed: pct(5, &count, &of),
        tapped: pct(6, &count, &of),
    }
}

/// Every twist threshold with every way to start zooming; then turning and
/// zooming as one or the other, at the chosen threshold.
fn candidates() -> Vec<(String, Tuning)> {
    let zooms = [(1.0, false, "at once"), (1.1, false, "after 10 %"), (1.1, true, "after 10 %, catching up")];
    let mut out = Vec::new();
    for exclusive in [false, true] {
        for (zoom_after, zoom_catches_up, how) in zooms {
            for twist_deg in [0.0, 10.0, 15.0, 20.0, 25.0, 30.0] {
                if exclusive && twist_deg != TWIST_DEG {
                    continue;
                }
                let name =
                    format!("twist {twist_deg:>2}°, zoom {how}{}", if exclusive { ", one of the two" } else { "" });
                out.push((name, Tuning { twist_deg, zoom_after, zoom_catches_up, exclusive }));
            }
        }
    }
    out
}

/// The table of ADR 0030.
#[test]
#[ignore = "prints the measurement; run it with --ignored --nocapture"]
fn gesture_table() {
    let n = 4000;
    println!(
        "{n} gestures per drift (a quarter each of pans, pinches, twists and two-finger taps), rates in %\n\n| tuning | drift | pans and pinches turned | twists 20–45° turned | twists 45–90° turned | pans zoomed > 5 % | pinches > 5 % off | twists zoomed > 10 % | two-finger taps |\n|---|---|---|---|---|---|---|---|---|"
    );
    for (name, tuning) in candidates() {
        for d in DRIFTS {
            let r = measure(tuning, d, n);
            println!(
                "| {name} | {} | {:.1} | {:.1} | {:.1} | {:.1} | {:.1} | {:.1} | {:.1} |",
                d.name,
                r.false_turn,
                r.turned_small,
                r.turned_large,
                r.pan_zoomed,
                r.pinch_off,
                r.twist_zoomed,
                r.tapped
            );
        }
    }
}

/// The chosen tuning keeps the numbers ADR 0030 chose it for.
#[test]
fn the_chosen_tuning_keeps_its_measured_rates() {
    let t = Tuning::default();
    let medium = measure(t, DRIFTS[1], 2000);
    assert!(medium.false_turn < 0.5, "{medium:?}");
    assert!(medium.turned_large > 99.0 && medium.turned_small > 80.0, "{medium:?}");
    assert!(medium.tapped == 100.0, "{medium:?}");
    let high = measure(t, DRIFTS[2], 2000);
    assert!(high.false_turn < 5.0, "{high:?}");
}
