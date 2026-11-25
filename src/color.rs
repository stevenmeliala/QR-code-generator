use crate::QrError;

/// Compact 8-bit RGBA color representation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgba {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Rgba {
    pub const BLACK: Self = Self { r: 0, g: 0, b: 0, a: 255 };
    pub const WHITE: Self = Self { r: 255, g: 255, b: 255, a: 255 };
    pub const TRANSPARENT: Self = Self { r: 0, g: 0, b: 0, a: 0 };

    pub const fn new(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    /// Parses `#RGB`, `#RGBA`, `#RRGGBB`, `#RRGGBBAA` (with or without `#`), or `"transparent"`.
    pub fn parse(input: &str) -> Result<Self, QrError> {
        let trimmed = input.trim();
        if trimmed.eq_ignore_ascii_case("transparent") {
            return Ok(Self::TRANSPARENT);
        }

        let hex = trimmed.strip_prefix('#').unwrap_or(trimmed);
        let bytes = hex.as_bytes();

        match bytes.len() {
            3 => {
                let r = parse_hex_nibble(bytes[0], trimmed)?;
                let g = parse_hex_nibble(bytes[1], trimmed)?;
                let b = parse_hex_nibble(bytes[2], trimmed)?;
                Ok(Self::new(r * 17, g * 17, b * 17, 255))
            }
            4 => {
                let r = parse_hex_nibble(bytes[0], trimmed)?;
                let g = parse_hex_nibble(bytes[1], trimmed)?;
                let b = parse_hex_nibble(bytes[2], trimmed)?;
                let a = parse_hex_nibble(bytes[3], trimmed)?;
                Ok(Self::new(r * 17, g * 17, b * 17, a * 17))
            }
            6 => {
                let r = parse_hex_byte(bytes[0], bytes[1], trimmed)?;
                let g = parse_hex_byte(bytes[2], bytes[3], trimmed)?;
                let b = parse_hex_byte(bytes[4], bytes[5], trimmed)?;
                Ok(Self::new(r, g, b, 255))
            }
            8 => {
                let r = parse_hex_byte(bytes[0], bytes[1], trimmed)?;
                let g = parse_hex_byte(bytes[2], bytes[3], trimmed)?;
                let b = parse_hex_byte(bytes[4], bytes[5], trimmed)?;
                let a = parse_hex_byte(bytes[6], bytes[7], trimmed)?;
                Ok(Self::new(r, g, b, a))
            }
            _ => Err(QrError::InvalidColor(format!(
                "expected #RGB, #RGBA, #RRGGBB, #RRGGBBAA, or 'transparent', got '{trimmed}'"
            ))),
        }
    }

    /// Formats color as `#rrggbb` (when opaque) or `#rrggbbaa`.
    pub fn to_hex(&self) -> String {
        if self.a == 255 {
            format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
        } else {
            format!("#{:02x}{:02x}{:02x}{:02x}", self.r, self.g, self.b, self.a)
        }
    }

    /// Returns (`#rrggbb`, opacity in `0.0..=1.0`) for broad SVG viewer compatibility.
    pub fn to_svg_fill_and_opacity(&self) -> (String, f32) {
        (
            format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b),
            (self.a as f32) / 255.0,
        )
    }

    /// Linearly interpolates between two colors at `t` clamped to `[0.0, 1.0]`.
    #[inline]
    pub fn lerp(self, other: Self, t: f32) -> Self {
        let t = t.clamp(0.0, 1.0);
        let inv = 1.0 - t;
        Self {
            r: (self.r as f32 * inv + other.r as f32 * t).round() as u8,
            g: (self.g as f32 * inv + other.g as f32 * t).round() as u8,
            b: (self.b as f32 * inv + other.b as f32 * t).round() as u8,
            a: (self.a as f32 * inv + other.a as f32 * t).round() as u8,
        }
    }
}

#[inline]
fn parse_hex_nibble(b: u8, original: &str) -> Result<u8, QrError> {
    match b {
        b'0'..=b'9' => Ok(b - b'0'),
        b'a'..=b'f' => Ok(b - b'a' + 10),
        b'A'..=b'F' => Ok(b - b'A' + 10),
        _ => Err(QrError::InvalidColor(format!(
            "invalid hex digit in '{original}'"
        ))),
    }
}

#[inline]
fn parse_hex_byte(hi: u8, lo: u8, original: &str) -> Result<u8, QrError> {
    Ok((parse_hex_nibble(hi, original)? << 4) | parse_hex_nibble(lo, original)?)
}

#[inline]
fn linearize_channel(c: u8) -> f64 {
    let s = (c as f64) / 255.0;
    if s <= 0.04045 {
        s / 12.92
    } else {
        ((s + 0.055) / 1.055).powf(2.4)
    }
}

/// Computes WCAG relative luminance in `[0.0, 1.0]`.
pub fn relative_luminance(c: Rgba) -> f64 {
    0.2126 * linearize_channel(c.r)
        + 0.7152 * linearize_channel(c.g)
        + 0.0722 * linearize_channel(c.b)
}

/// Computes WCAG contrast ratio between foreground and background (`None` when background is fully transparent).
pub fn contrast_ratio(fg: Rgba, bg: Rgba) -> Option<f64> {
    if bg.a == 0 {
        return None;
    }
    // Alpha-composite fg over bg for semi-transparent foregrounds.
    let alpha = (fg.a as f64) / 255.0;
    let blended = Rgba::new(
        (fg.r as f64 * alpha + bg.r as f64 * (1.0 - alpha)).round() as u8,
        (fg.g as f64 * alpha + bg.g as f64 * (1.0 - alpha)).round() as u8,
        (fg.b as f64 * alpha + bg.b as f64 * (1.0 - alpha)).round() as u8,
        255,
    );
    let l1 = relative_luminance(blended);
    let l2 = relative_luminance(bg);
    let (bright, dark) = if l1 >= l2 { (l1, l2) } else { (l2, l1) };
    Some((bright + 0.05) / (dark + 0.05))
}
