//! Owned math: the functions the engine needs beyond `+ - * /` and `sqrt`.
//!
//! The platform's sine and cosine give different last digits on different
//! machines, which would break the determinism contract (ADR 0005). These
//! are built from exact operations only, in a fixed order, so every platform
//! computes the same bits.

/// tan(22.5°): half of the 45° vertical field of view of the perspective camera.
pub const TAN_22_5: f64 = 0.414_213_562_373_095_05;
/// sin and cos of 30°, the perspective camera's height above the horizon.
pub const SIN_30: f64 = 0.5;
pub const COS_30: f64 = 0.866_025_403_784_438_6;
pub const SQRT_2: f64 = std::f64::consts::SQRT_2;

const RAD_PER_DEG: f64 = 0.017_453_292_519_943_295;

/// Sine and cosine of an angle in degrees.
///
/// Exact at every multiple of 90 (`sin_cos_deg(90.0)` is `(1.0, 0.0)`), so
/// quarter turns add no rounding. Elsewhere the error is far below one unit
/// in the last place of an `f32`.
pub fn sin_cos_deg(deg: f32) -> (f32, f32) {
    let (s, c) = sin_cos_deg64(deg as f64);
    (s as f32, c as f32)
}

/// [`sin_cos_deg`] in double precision.
pub fn sin_cos_deg64(deg: f64) -> (f64, f64) {
    if !deg.is_finite() {
        return (0.0, 1.0);
    }
    // Bring the angle into 0..360, then into a quarter and what is left.
    let turns = deg / 360.0;
    let a = (turns - turns.floor()) * 360.0;
    let quarter = (a / 90.0).floor();
    let rest = a - quarter * 90.0;
    // The series are only used on 0..=45 degrees, where they converge fast.
    let (s, c) = if rest <= 45.0 {
        series(rest)
    } else {
        let (s, c) = series(90.0 - rest);
        (c, s)
    };
    // Adding 0.0 turns a negative zero into zero.
    let (s, c) = match quarter as i64 & 3 {
        0 => (s, c),
        1 => (c, -s),
        2 => (-s, -c),
        _ => (-c, s),
    };
    (s + 0.0, c + 0.0)
}

/// Taylor series for 0..=45 degrees, summed from the smallest term. The
/// first terms left out are below 1e-20.
fn series(deg: f64) -> (f64, f64) {
    // 1/3!, 1/5!, ... 1/19! and 1/2!, 1/4!, ... 1/18!, signs alternating.
    const SIN: [f64; 9] = [
        -1.0 / 6.0,
        1.0 / 120.0,
        -1.0 / 5_040.0,
        1.0 / 362_880.0,
        -1.0 / 39_916_800.0,
        1.0 / 6_227_020_800.0,
        -1.0 / 1_307_674_368_000.0,
        1.0 / 355_687_428_096_000.0,
        -1.0 / 121_645_100_408_832_000.0,
    ];
    const COS: [f64; 9] = [
        -1.0 / 2.0,
        1.0 / 24.0,
        -1.0 / 720.0,
        1.0 / 40_320.0,
        -1.0 / 3_628_800.0,
        1.0 / 479_001_600.0,
        -1.0 / 87_178_291_200.0,
        1.0 / 20_922_789_888_000.0,
        -1.0 / 6_402_373_705_728_000.0,
    ];
    let x = deg * RAD_PER_DEG;
    let x2 = x * x;
    let (mut s, mut c) = (0.0, 0.0);
    for i in (0..9).rev() {
        s = x2 * (SIN[i] + s);
        c = x2 * (COS[i] + c);
    }
    (x * (1.0 + s), 1.0 + c)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quarter_turns_are_exact() {
        assert_eq!(sin_cos_deg(0.0), (0.0, 1.0));
        assert_eq!(sin_cos_deg(90.0), (1.0, 0.0));
        assert_eq!(sin_cos_deg(180.0), (0.0, -1.0));
        assert_eq!(sin_cos_deg(270.0), (-1.0, 0.0));
        assert_eq!(sin_cos_deg(360.0), (0.0, 1.0));
        assert_eq!(sin_cos_deg(-90.0), (-1.0, 0.0));
        assert_eq!(sin_cos_deg(720.0 + 90.0), (1.0, 0.0));
        // No negative zeros: their bits differ from zero's.
        for d in [0.0, 90.0, 180.0, 270.0, -180.0] {
            let (s, c) = sin_cos_deg(d);
            assert!(s.to_bits() != (-0.0f32).to_bits() && c.to_bits() != (-0.0f32).to_bits(), "{d}");
        }
    }

    #[test]
    fn known_angles() {
        let close = |a: f64, b: f64| (a - b).abs() < 1e-14;
        let (s, c) = sin_cos_deg64(30.0);
        assert!(close(s, 0.5) && close(c, COS_30), "{s} {c}");
        let (s, c) = sin_cos_deg64(45.0);
        assert!(close(s, SQRT_2 / 2.0) && close(c, SQRT_2 / 2.0), "{s} {c}");
        let (s, c) = sin_cos_deg64(60.0);
        assert!(close(s, COS_30) && close(c, 0.5), "{s} {c}");
        let (s, c) = sin_cos_deg64(-135.0);
        assert!(close(s, -SQRT_2 / 2.0) && close(c, -SQRT_2 / 2.0), "{s} {c}");
        assert_eq!(sin_cos_deg(30.0).0, 0.5);
        assert!(close(sin_cos_deg64(22.5).0 / sin_cos_deg64(22.5).1, TAN_22_5));
    }

    #[test]
    fn stays_on_the_unit_circle() {
        let mut d = -400.0f64;
        while d < 400.0 {
            let (s, c) = sin_cos_deg64(d);
            assert!((s * s + c * c - 1.0).abs() < 1e-14, "{d}: {s} {c}");
            d += 0.37;
        }
    }

    #[test]
    fn the_bits_are_pinned() {
        // If these change, pictures and state on some platform changed too.
        let bits = |d: f32| {
            let (s, c) = sin_cos_deg(d);
            (s.to_bits(), c.to_bits())
        };
        assert_eq!(bits(17.0), (0x3e95_b1be, 0x3f74_d063));
        assert_eq!(bits(123.4), (0x3f55_b897, 0xbf0c_ec4f));
    }
}
