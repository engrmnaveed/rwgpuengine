//! RGBA8 colors. Scene files write colors as hex strings: `#rgb`, `#rgba`,
//! `#rrggbb` or `#rrggbbaa`.

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Rgba {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Rgba {
    pub const BLACK: Rgba = Rgba::rgb(0, 0, 0);
    pub const WHITE: Rgba = Rgba::rgb(255, 255, 255);

    pub const fn rgb(r: u8, g: u8, b: u8) -> Rgba {
        Rgba { r, g, b, a: 255 }
    }

    /// Parse `#rgb`, `#rgba`, `#rrggbb` or `#rrggbbaa`.
    pub fn parse_hex(s: &str) -> Option<Rgba> {
        let hex = s.strip_prefix('#')?;
        if !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        let nib = |i: usize| u8::from_str_radix(&hex[i..i + 1], 16).ok().map(|v| v * 17);
        let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
        match hex.len() {
            3 => Some(Rgba { r: nib(0)?, g: nib(1)?, b: nib(2)?, a: 255 }),
            4 => Some(Rgba { r: nib(0)?, g: nib(1)?, b: nib(2)?, a: nib(3)? }),
            6 => Some(Rgba { r: byte(0)?, g: byte(2)?, b: byte(4)?, a: 255 }),
            8 => Some(Rgba { r: byte(0)?, g: byte(2)?, b: byte(4)?, a: byte(6)? }),
            _ => None,
        }
    }

    /// `#rrggbb`, or `#rrggbbaa` when not opaque.
    pub fn to_hex(self) -> String {
        if self.a == 255 {
            format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
        } else {
            format!("#{:02x}{:02x}{:02x}{:02x}", self.r, self.g, self.b, self.a)
        }
    }

    /// Scale RGB by `num/den` with integer math (deterministic shading).
    pub fn scale(self, num: u32, den: u32) -> Rgba {
        let s = |c: u8| ((c as u32 * num) / den).min(255) as u8;
        Rgba { r: s(self.r), g: s(self.g), b: s(self.b), a: self.a }
    }

    /// Source-over blend of `self` onto opaque `dst`, integer math.
    pub fn over(self, dst: Rgba) -> Rgba {
        if self.a == 255 {
            return self;
        }
        let a = self.a as u32;
        let mix = |s: u8, d: u8| ((s as u32 * a + d as u32 * (255 - a) + 127) / 255) as u8;
        Rgba { r: mix(self.r, dst.r), g: mix(self.g, dst.g), b: mix(self.b, dst.b), a: 255 }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_all_hex_forms() {
        assert_eq!(Rgba::parse_hex("#fff"), Some(Rgba::WHITE));
        assert_eq!(Rgba::parse_hex("#ff8800"), Some(Rgba::rgb(255, 136, 0)));
        assert_eq!(Rgba::parse_hex("#ff880080").map(|c| c.a), Some(128));
        assert_eq!(Rgba::parse_hex("#f80c").map(|c| c.a), Some(204));
        assert_eq!(Rgba::parse_hex("orange"), None);
        assert_eq!(Rgba::parse_hex("#12345"), None);
        assert_eq!(Rgba::parse_hex("#ggg"), None);
    }

    #[test]
    fn hex_round_trips() {
        for s in ["#102030", "#a1b2c3d4"] {
            assert_eq!(Rgba::parse_hex(s).unwrap().to_hex(), s);
        }
    }
}
