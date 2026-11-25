use std::fmt::Write as _;
use std::time::Instant;

use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine as _;
use image::imageops::FilterType;
use qrcodegen::{QrCode, QrCodeEcc};

use crate::color::{contrast_ratio, Rgba};
use crate::QrError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    Png,
    Svg,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModuleStyle {
    Square,
    Rounded,
    Dots,
}

impl ModuleStyle {
    pub fn parse(s: &str) -> Result<Self, QrError> {
        match s.trim().to_ascii_lowercase().as_str() {
            "square" => Ok(Self::Square),
            "rounded" => Ok(Self::Rounded),
            "dots" | "dot" => Ok(Self::Dots),
            other => Err(QrError::InvalidInput(format!(
                "unknown module style '{other}' (expected square, rounded, or dots)"
            ))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EccLevel {
    Auto,
    Low,
    Medium,
    Quartile,
    High,
}

impl EccLevel {
    pub fn parse(s: &str) -> Result<Self, QrError> {
        match s.trim().to_ascii_lowercase().as_str() {
            "auto" => Ok(Self::Auto),
            "l" | "low" => Ok(Self::Low),
            "m" | "medium" => Ok(Self::Medium),
            "q" | "quartile" => Ok(Self::Quartile),
            "h" | "high" => Ok(Self::High),
            other => Err(QrError::InvalidInput(format!(
                "unknown ECC level '{other}' (expected auto, L, M, Q, or H)"
            ))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogoShape {
    Rounded,
    Circle,
    Square,
    None,
}

impl LogoShape {
    pub fn parse(s: &str) -> Result<Self, QrError> {
        match s.trim().to_ascii_lowercase().as_str() {
            "rounded" => Ok(Self::Rounded),
            "circle" => Ok(Self::Circle),
            "square" => Ok(Self::Square),
            "none" => Ok(Self::None),
            other => Err(QrError::InvalidInput(format!(
                "unknown logo shape '{other}' (expected rounded, circle, square, or none)"
            ))),
        }
    }
}

#[derive(Debug, Clone)]
pub struct LogoConfig {
    pub bytes: Vec<u8>,
    /// Fraction of the QR symbol width (without quiet zone), clamped to `0.08..=0.35`.
    pub scale: f32,
    /// Padding around the logo inside its backdrop frame (in pixels at target output size).
    pub padding_px: u32,
    pub shape: LogoShape,
    pub background: Option<Rgba>,
    pub clear_modules: bool,
}

impl LogoConfig {
    pub fn new(bytes: Vec<u8>) -> Self {
        Self {
            bytes,
            scale: 0.22,
            padding_px: 8,
            shape: LogoShape::Rounded,
            background: None,
            clear_modules: true,
        }
    }
}

#[derive(Debug, Clone)]
pub struct QrConfig {
    pub data: String,
    pub size: u32,
    pub quiet_zone: u32,
    pub fg: Rgba,
    pub fg_gradient_end: Option<Rgba>,
    pub bg: Rgba,
    pub ecc: EccLevel,
    pub module_style: ModuleStyle,
    pub logo: Option<LogoConfig>,
}

impl QrConfig {
    pub fn new(data: impl Into<String>) -> Self {
        Self {
            data: data.into(),
            size: 512,
            quiet_zone: 4,
            fg: Rgba::BLACK,
            fg_gradient_end: None,
            bg: Rgba::WHITE,
            ecc: EccLevel::Auto,
            module_style: ModuleStyle::Square,
            logo: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct QrOutput {
    pub bytes: Vec<u8>,
    pub format: OutputFormat,
    pub width: u32,
    pub height: u32,
    pub qr_version: i32,
    pub ecc_used: &'static str,
    pub contrast_ratio: Option<f64>,
    pub warnings: Vec<String>,
    pub generation_us: u128,
}

#[derive(Debug, Clone, Copy)]
struct ClearBox {
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
}

impl ClearBox {
    #[inline]
    fn overlaps_rect(&self, rx0: f32, ry0: f32, rx1: f32, ry1: f32) -> bool {
        rx1 > self.x0 && rx0 < self.x1 && ry1 > self.y0 && ry0 < self.y1
    }
}

struct PreparedLogo {
    rgba: image::RgbaImage,
    backdrop_w: u32,
    backdrop_h: u32,
    backdrop_color: Rgba,
    shape: LogoShape,
}

/// Encodes and renders a QR code with optional color customization and center logo embedding.
pub fn generate(config: &QrConfig, format: OutputFormat) -> Result<QrOutput, QrError> {
    let start = Instant::now();

    validate_config(config)?;

    let (ecc, ecc_label) = resolve_ecc(config.ecc, config.logo.is_some());
    let mut warnings = collect_warnings(config, ecc_label);

    let qr = QrCode::encode_text(&config.data, ecc)
        .map_err(|e| QrError::EncodeError(format!("data too long for QR code: {e}")))?;

    let qr_size = qr.size() as u32;
    let qr_version = ((qr.size() - 17) / 4).max(1);
    let contrast = contrast_ratio(config.fg, config.bg);

    if let Some(end) = config.fg_gradient_end {
        if let Some(c_end) = contrast_ratio(end, config.bg) {
            if c_end < 3.0 && !warnings.iter().any(|w| w.contains("contrast")) {
                warnings.push(format!(
                    "Low contrast ({c_end:.2}:1) between gradient end color and background may reduce scannability."
                ));
            }
        }
    }

    let (bytes, width, height) = match format {
        OutputFormat::Png => render_png(&qr, qr_size, config)?,
        OutputFormat::Svg => render_svg(&qr, qr_size, config)?,
    };

    Ok(QrOutput {
        bytes,
        format,
        width,
        height,
        qr_version,
        ecc_used: ecc_label,
        contrast_ratio: contrast,
        warnings,
        generation_us: start.elapsed().as_micros(),
    })
}

/// Renders a QR code using Unicode half-block characters for fast terminal preview.
pub fn render_terminal(config: &QrConfig) -> Result<String, QrError> {
    validate_config(config)?;
    let (ecc, _) = resolve_ecc(config.ecc, config.logo.is_some());
    let qr = QrCode::encode_text(&config.data, ecc)
        .map_err(|e| QrError::EncodeError(format!("data too long for QR code: {e}")))?;

    let qr_size = qr.size();
    let qz = config.quiet_zone.min(4) as i32;
    let total = qr_size + 2 * qz;
    let mut out = String::with_capacity(((total as usize + 1) * (total as usize / 2 + 1)) * 3);

    let is_dark = |x: i32, y: i32| -> bool {
        let qx = x - qz;
        let qy = y - qz;
        if qx < 0 || qy < 0 || qx >= qr_size || qy >= qr_size {
            false
        } else {
            qr.get_module(qx, qy)
        }
    };

    let mut y = 0;
    while y < total {
        for x in 0..total {
            let top = is_dark(x, y);
            let bottom = if y + 1 < total {
                is_dark(x, y + 1)
            } else {
                false
            };
            let ch = match (top, bottom) {
                (true, true) => '█',
                (true, false) => '▀',
                (false, true) => '▄',
                (false, false) => ' ',
            };
            out.push(ch);
        }
        out.push('\n');
        y += 2;
    }

    Ok(out)
}

fn validate_config(config: &QrConfig) -> Result<(), QrError> {
    if config.data.is_empty() {
        return Err(QrError::InvalidInput(
            "QR code content must not be empty".into(),
        ));
    }
    if !(64..=4096).contains(&config.size) {
        return Err(QrError::InvalidInput(format!(
            "size {} out of allowed range 64..=4096",
            config.size
        )));
    }
    if config.quiet_zone > 16 {
        return Err(QrError::InvalidInput(format!(
            "quiet_zone {} out of allowed range 0..=16",
            config.quiet_zone
        )));
    }
    if let Some(logo) = &config.logo {
        if logo.bytes.is_empty() {
            return Err(QrError::InvalidInput(
                "logo image bytes must not be empty".into(),
            ));
        }
        if !(0.08..=0.35).contains(&logo.scale) {
            return Err(QrError::InvalidInput(format!(
                "logo scale {:.2} out of allowed range 0.08..=0.35",
                logo.scale
            )));
        }
    }
    Ok(())
}

fn resolve_ecc(ecc: EccLevel, has_logo: bool) -> (QrCodeEcc, &'static str) {
    match ecc {
        EccLevel::Auto => {
            if has_logo {
                (QrCodeEcc::High, "H")
            } else {
                (QrCodeEcc::Medium, "M")
            }
        }
        EccLevel::Low => (QrCodeEcc::Low, "L"),
        EccLevel::Medium => (QrCodeEcc::Medium, "M"),
        EccLevel::Quartile => (QrCodeEcc::Quartile, "Q"),
        EccLevel::High => (QrCodeEcc::High, "H"),
    }
}

fn collect_warnings(config: &QrConfig, ecc_label: &str) -> Vec<String> {
    let mut warnings = Vec::new();
    if let Some(ratio) = contrast_ratio(config.fg, config.bg) {
        if ratio < 3.0 {
            warnings.push(format!(
                "Low contrast ({ratio:.2}:1) between foreground and background may make the QR code hard to scan."
            ));
        }
    }
    if let Some(logo) = &config.logo {
        if matches!(ecc_label, "L" | "M") {
            warnings.push(format!(
                "Center logo used with ECC {ecc_label}; High (H) error correction is recommended for reliable scanning."
            ));
        }
        if logo.scale > 0.28 {
            warnings.push(format!(
                "Large logo scale ({:.0}%) covers many modules and may impair scannability.",
                logo.scale * 100.0
            ));
        }
    }
    warnings
}

#[inline]
fn is_finder_module(mx: u32, my: u32, qr_size: u32) -> bool {
    let in_top = my < 7;
    let in_bottom = my >= qr_size.saturating_sub(7);
    let in_left = mx < 7;
    let in_right = mx >= qr_size.saturating_sub(7);
    (in_top && in_left) || (in_top && in_right) || (in_bottom && in_left)
}

fn prepare_logo(logo: &LogoConfig, max_box_px: u32, qr_bg: Rgba) -> Result<PreparedLogo, QrError> {
    let decoded = image::load_from_memory(&logo.bytes)?;
    let target = max_box_px.max(16);
    let resized = decoded.resize(target, target, FilterType::Lanczos3).to_rgba8();

    let pad = if logo.shape == LogoShape::None {
        0
    } else {
        logo.padding_px.min(target / 2)
    };

    let backdrop_color = logo.background.unwrap_or_else(|| {
        if qr_bg.a == 255 {
            qr_bg
        } else {
            Rgba::WHITE
        }
    });

    Ok(PreparedLogo {
        backdrop_w: resized.width() + pad * 2,
        backdrop_h: resized.height() + pad * 2,
        rgba: resized,
        backdrop_color,
        shape: logo.shape,
    })
}

fn render_png(qr: &QrCode, qr_size: u32, config: &QrConfig) -> Result<(Vec<u8>, u32, u32), QrError> {
    let total_modules = qr_size + 2 * config.quiet_zone;
    let module_px = (config.size / total_modules).max(1);
    let canvas_size = module_px * total_modules;
    let canvas_usize = canvas_size as usize;

    let mut buffer = vec![0u8; canvas_usize * canvas_usize * 4];
    fill_solid_rgba(&mut buffer, config.bg);

    let qr_area_px = qr_size * module_px;
    let prepared_logo = if let Some(logo_cfg) = &config.logo {
        let max_box = ((qr_area_px as f32) * logo_cfg.scale).round().max(16.0) as u32;
        Some((prepare_logo(logo_cfg, max_box, config.bg)?, logo_cfg.clear_modules))
    } else {
        None
    };

    let clear_box = prepared_logo.as_ref().and_then(|(prep, clear)| {
        if *clear {
            let cx = (canvas_size as f32) * 0.5;
            let cy = (canvas_size as f32) * 0.5;
            let hw = (prep.backdrop_w as f32) * 0.5;
            let hh = (prep.backdrop_h as f32) * 0.5;
            Some(ClearBox {
                x0: cx - hw,
                y0: cy - hh,
                x1: cx + hw,
                y1: cy + hh,
            })
        } else {
            None
        }
    });

    let qz = config.quiet_zone;
    let fast_square_path = config.module_style == ModuleStyle::Square
        && config.fg_gradient_end.is_none()
        && config.fg.a == 255;

    if fast_square_path {
        let m_usize = module_px as usize;
        let stride = canvas_usize * 4;
        let mut strip = Vec::with_capacity(m_usize * 4);
        for _ in 0..m_usize {
            strip.extend_from_slice(&[config.fg.r, config.fg.g, config.fg.b, 255]);
        }

        for my in 0..qr_size {
            let py0 = ((my + qz) * module_px) as usize;
            let py_f0 = py0 as f32;
            let py_f1 = (py0 + m_usize) as f32;
            for mx in 0..qr_size {
                if !qr.get_module(mx as i32, my as i32) {
                    continue;
                }
                let px0 = ((mx + qz) * module_px) as usize;
                let px_f0 = px0 as f32;
                let px_f1 = (px0 + m_usize) as f32;
                if let Some(cb) = clear_box {
                    if cb.overlaps_rect(px_f0, py_f0, px_f1, py_f1) {
                        continue;
                    }
                }
                let byte_x = px0 * 4;
                for row in py0..(py0 + m_usize) {
                    let dst = row * stride + byte_x;
                    buffer[dst..dst + strip.len()].copy_from_slice(&strip);
                }
            }
        }
    } else {
        let m_f = module_px as f32;
        let inv_diag = 1.0 / ((2 * canvas_size).max(1) as f32);
        let stride = canvas_usize * 4;

        for my in 0..qr_size {
            let py0 = (my + qz) * module_px;
            let py1 = py0 + module_px;
            for mx in 0..qr_size {
                if !qr.get_module(mx as i32, my as i32) {
                    continue;
                }
                let px0 = (mx + qz) * module_px;
                let px1 = px0 + module_px;
                if let Some(cb) = clear_box {
                    if cb.overlaps_rect(px0 as f32, py0 as f32, px1 as f32, py1 as f32) {
                        continue;
                    }
                }

                // Keep finder patterns solid/rounded-square so scanners always lock cleanly.
                let effective_style = if config.module_style == ModuleStyle::Dots
                    && is_finder_module(mx, my, qr_size)
                {
                    ModuleStyle::Rounded
                } else {
                    config.module_style
                };

                let half = m_f * 0.5;
                let cx = px0 as f32 + half;
                let cy = py0 as f32 + half;

                for py in py0..py1 {
                    let row_off = (py as usize) * stride;
                    let dy = (py as f32 + 0.5) - cy;
                    for px in px0..px1 {
                        let dx = (px as f32 + 0.5) - cx;
                        let cov = match effective_style {
                            ModuleStyle::Square => 1.0,
                            ModuleStyle::Rounded => {
                                let radius = (m_f * 0.28).max(0.8);
                                let sd = sdf_rounded_box(dx, dy, half, half, radius);
                                (0.5 - sd).clamp(0.0, 1.0)
                            }
                            ModuleStyle::Dots => {
                                let radius = (m_f * 0.44).max(0.5);
                                let sd = (dx * dx + dy * dy).sqrt() - radius;
                                (0.5 - sd).clamp(0.0, 1.0)
                            }
                        };

                        if cov <= 0.0 {
                            continue;
                        }

                        let mut fg_col = if let Some(end) = config.fg_gradient_end {
                            let t = ((px + py) as f32) * inv_diag;
                            config.fg.lerp(end, t)
                        } else {
                            config.fg
                        };
                        fg_col.a = ((fg_col.a as f32) * cov).round() as u8;

                        let idx = row_off + (px as usize) * 4;
                        blend_pixel(&mut buffer[idx..idx + 4], fg_col);
                    }
                }
            }
        }
    }

    if let Some((prep, _)) = prepared_logo {
        composite_logo(&mut buffer, canvas_size, &prep);
    }

    let png_bytes = encode_rgba_png(&buffer, canvas_size, canvas_size)?;
    Ok((png_bytes, canvas_size, canvas_size))
}

fn fill_solid_rgba(buffer: &mut [u8], color: Rgba) {
    if buffer.is_empty() {
        return;
    }
    let pattern = [color.r, color.g, color.b, color.a];
    if pattern == [0, 0, 0, 0] {
        return;
    }
    buffer[..4].copy_from_slice(&pattern);
    let mut filled = 4;
    while filled < buffer.len() {
        let chunk = filled.min(buffer.len() - filled);
        buffer.copy_within(0..chunk, filled);
        filled += chunk;
    }
}

#[inline]
fn sdf_rounded_box(dx: f32, dy: f32, half_w: f32, half_h: f32, radius: f32) -> f32 {
    let qx = dx.abs() - (half_w - radius);
    let qy = dy.abs() - (half_h - radius);
    let ox = qx.max(0.0);
    let oy = qy.max(0.0);
    (ox * ox + oy * oy).sqrt() + qx.max(qy).min(0.0) - radius
}

#[inline]
fn blend_pixel(dst: &mut [u8], src: Rgba) {
    match src.a {
        0 => {}
        255 => {
            dst[0] = src.r;
            dst[1] = src.g;
            dst[2] = src.b;
            dst[3] = 255;
        }
        sa => {
            let sa_f = sa as f32 / 255.0;
            let da_f = dst[3] as f32 / 255.0;
            let out_a = sa_f + da_f * (1.0 - sa_f);
            if out_a <= 0.0001 {
                dst.copy_from_slice(&[0, 0, 0, 0]);
                return;
            }
            let inv = da_f * (1.0 - sa_f);
            dst[0] = ((src.r as f32 * sa_f + dst[0] as f32 * inv) / out_a).round() as u8;
            dst[1] = ((src.g as f32 * sa_f + dst[1] as f32 * inv) / out_a).round() as u8;
            dst[2] = ((src.b as f32 * sa_f + dst[2] as f32 * inv) / out_a).round() as u8;
            dst[3] = (out_a * 255.0).round() as u8;
        }
    }
}

fn composite_logo(buffer: &mut [u8], canvas_size: u32, prep: &PreparedLogo) {
    let stride = (canvas_size as usize) * 4;
    let cx = (canvas_size as f32) * 0.5;
    let cy = (canvas_size as f32) * 0.5;

    if prep.shape != LogoShape::None {
        let bw = prep.backdrop_w as f32;
        let bh = prep.backdrop_h as f32;
        let hw = bw * 0.5;
        let hh = bh * 0.5;
        let circle_r = hw.min(hh);
        let round_r = (bw.min(bh) * 0.18).max(2.0);

        let x0 = ((cx - hw).floor().max(0.0)) as u32;
        let y0 = ((cy - hh).floor().max(0.0)) as u32;
        let x1 = ((cx + hw).ceil().min(canvas_size as f32)) as u32;
        let y1 = ((cy + hh).ceil().min(canvas_size as f32)) as u32;

        for py in y0..y1 {
            let dy = (py as f32 + 0.5) - cy;
            let row_off = (py as usize) * stride;
            for px in x0..x1 {
                let dx = (px as f32 + 0.5) - cx;
                let sd = match prep.shape {
                    LogoShape::Circle => (dx * dx + dy * dy).sqrt() - circle_r,
                    LogoShape::Rounded => sdf_rounded_box(dx, dy, hw, hh, round_r),
                    LogoShape::Square => dx.abs().max(dy.abs()) - hw.max(hh),
                    LogoShape::None => 1.0,
                };
                let cov = (0.5 - sd).clamp(0.0, 1.0);
                if cov <= 0.0 {
                    continue;
                }
                let mut col = prep.backdrop_color;
                col.a = ((col.a as f32) * cov).round() as u8;
                let idx = row_off + (px as usize) * 4;
                blend_pixel(&mut buffer[idx..idx + 4], col);
            }
        }
    }

    let lw = prep.rgba.width();
    let lh = prep.rgba.height();
    let lx0 = ((canvas_size as i32 - lw as i32) / 2).max(0) as u32;
    let ly0 = ((canvas_size as i32 - lh as i32) / 2).max(0) as u32;
    let lhw = (lw as f32) * 0.5;
    let lhh = (lh as f32) * 0.5;
    let l_circle_r = lhw.min(lhh);
    let l_round_r = (lw.min(lh) as f32 * 0.14).max(1.5);
    let raw = prep.rgba.as_raw();

    for ly in 0..lh {
        let py = ly0 + ly;
        if py >= canvas_size {
            break;
        }
        let dy = (py as f32 + 0.5) - cy;
        let dst_row = (py as usize) * stride;
        let src_row = (ly as usize) * (lw as usize) * 4;
        for lx in 0..lw {
            let px = lx0 + lx;
            if px >= canvas_size {
                break;
            }
            let dx = (px as f32 + 0.5) - cx;
            let clip_cov = match prep.shape {
                LogoShape::Circle => (0.5 - ((dx * dx + dy * dy).sqrt() - l_circle_r)).clamp(0.0, 1.0),
                LogoShape::Rounded => {
                    (0.5 - sdf_rounded_box(dx, dy, lhw, lhh, l_round_r)).clamp(0.0, 1.0)
                }
                LogoShape::Square | LogoShape::None => 1.0,
            };
            if clip_cov <= 0.0 {
                continue;
            }
            let sidx = src_row + (lx as usize) * 4;
            let src = Rgba::new(
                raw[sidx],
                raw[sidx + 1],
                raw[sidx + 2],
                ((raw[sidx + 3] as f32) * clip_cov).round() as u8,
            );
            let didx = dst_row + (px as usize) * 4;
            blend_pixel(&mut buffer[didx..didx + 4], src);
        }
    }
}

fn encode_rgba_png(rgba: &[u8], width: u32, height: u32) -> Result<Vec<u8>, QrError> {
    let mut out = Vec::with_capacity(rgba.len() / 8 + 1024);
    {
        let mut encoder = png::Encoder::new(&mut out, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_compression(png::Compression::Fast);
        encoder.set_filter(png::FilterType::Sub);
        let mut writer = encoder
            .write_header()
            .map_err(|e| QrError::ImageError(format!("PNG header error: {e}")))?;
        writer
            .write_image_data(rgba)
            .map_err(|e| QrError::ImageError(format!("PNG write error: {e}")))?;
        writer
            .finish()
            .map_err(|e| QrError::ImageError(format!("PNG finish error: {e}")))?;
    }
    Ok(out)
}

fn render_svg(qr: &QrCode, qr_size: u32, config: &QrConfig) -> Result<(Vec<u8>, u32, u32), QrError> {
    let total_modules = qr_size + 2 * config.quiet_zone;
    let qz = config.quiet_zone;
    let mut svg = String::with_capacity((qr_size * qr_size * 12) as usize + 1024);

    let _ = writeln!(
        svg,
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {total_modules} {total_modules}" width="{0}" height="{0}" shape-rendering="geometricPrecision">"#,
        config.size
    );

    let prepared_logo = if let Some(logo_cfg) = &config.logo {
        let qr_area_px = ((config.size as f32) * (qr_size as f32 / total_modules as f32)).max(64.0);
        let max_box = (qr_area_px * logo_cfg.scale).round().max(32.0) as u32;
        Some((prepare_logo(logo_cfg, max_box, config.bg)?, logo_cfg.clear_modules))
    } else {
        None
    };

    let clear_box = prepared_logo.as_ref().and_then(|(prep, clear)| {
        if *clear {
            let scale_to_mod = (total_modules as f32) / (config.size as f32);
            let center = (total_modules as f32) * 0.5;
            let hw = (prep.backdrop_w as f32 * scale_to_mod) * 0.5;
            let hh = (prep.backdrop_h as f32 * scale_to_mod) * 0.5;
            Some(ClearBox {
                x0: center - hw,
                y0: center - hh,
                x1: center + hw,
                y1: center + hh,
            })
        } else {
            None
        }
    });

    if let Some(end) = config.fg_gradient_end {
        let (c1, o1) = config.fg.to_svg_fill_and_opacity();
        let (c2, o2) = end.to_svg_fill_and_opacity();
        let _ = writeln!(
            svg,
            r#"  <defs><linearGradient id="qr-fg" x1="0%" y1="0%" x2="100%" y2="100%"><stop offset="0%" stop-color="{c1}" stop-opacity="{o1:.3}"/><stop offset="100%" stop-color="{c2}" stop-opacity="{o2:.3}"/></linearGradient></defs>"#
        );
    }

    if config.bg.a > 0 {
        let (bg_hex, bg_op) = config.bg.to_svg_fill_and_opacity();
        if config.bg.a == 255 {
            let _ = writeln!(svg, r#"  <rect width="100%" height="100%" fill="{bg_hex}"/>"#);
        } else {
            let _ = writeln!(
                svg,
                r#"  <rect width="100%" height="100%" fill="{bg_hex}" fill-opacity="{bg_op:.3}"/>"#
            );
        }
    }

    let (fg_attr, fg_op_attr) = if config.fg_gradient_end.is_some() {
        (String::from("url(#qr-fg)"), String::new())
    } else {
        let (hex, op) = config.fg.to_svg_fill_and_opacity();
        let op_str = if config.fg.a == 255 {
            String::new()
        } else {
            format!(r#" fill-opacity="{op:.3}""#)
        };
        (hex, op_str)
    };

    let should_skip = |mx: u32, my: u32| -> bool {
        if let Some(cb) = clear_box {
            let x0 = (mx + qz) as f32;
            let y0 = (my + qz) as f32;
            cb.overlaps_rect(x0, y0, x0 + 1.0, y0 + 1.0)
        } else {
            false
        }
    };

    match config.module_style {
        ModuleStyle::Square => {
            // Merge contiguous horizontal dark runs per row into a single compact <path>.
            let mut path_data = String::with_capacity((qr_size * qr_size * 5) as usize);
            for my in 0..qr_size {
                let mut mx = 0;
                while mx < qr_size {
                    if !qr.get_module(mx as i32, my as i32) || should_skip(mx, my) {
                        mx += 1;
                        continue;
                    }
                    let start_x = mx;
                    while mx < qr_size
                        && qr.get_module(mx as i32, my as i32)
                        && !should_skip(mx, my)
                    {
                        mx += 1;
                    }
                    let run = mx - start_x;
                    let _ = write!(path_data, "M{} {}h{}v1h-{}z", start_x + qz, my + qz, run, run);
                }
            }
            let _ = writeln!(svg, r#"  <path fill="{fg_attr}"{fg_op_attr} d="{path_data}"/>"#);
        }
        ModuleStyle::Rounded => {
            let _ = writeln!(svg, r#"  <g fill="{fg_attr}"{fg_op_attr}>"#);
            for my in 0..qr_size {
                for mx in 0..qr_size {
                    if !qr.get_module(mx as i32, my as i32) || should_skip(mx, my) {
                        continue;
                    }
                    let _ = writeln!(
                        svg,
                        r#"    <rect x="{}" y="{}" width="1" height="1" rx="0.28"/>"#,
                        mx + qz,
                        my + qz
                    );
                }
            }
            svg.push_str("  </g>\n");
        }
        ModuleStyle::Dots => {
            let _ = writeln!(svg, r#"  <g fill="{fg_attr}"{fg_op_attr}>"#);
            for my in 0..qr_size {
                for mx in 0..qr_size {
                    if !qr.get_module(mx as i32, my as i32) || should_skip(mx, my) {
                        continue;
                    }
                    if is_finder_module(mx, my, qr_size) {
                        let _ = writeln!(
                            svg,
                            r#"    <rect x="{}" y="{}" width="1" height="1" rx="0.25"/>"#,
                            mx + qz,
                            my + qz
                        );
                    } else {
                        let _ = writeln!(
                            svg,
                            r#"    <circle cx="{}.5" cy="{}.5" r="0.44"/>"#,
                            mx + qz,
                            my + qz
                        );
                    }
                }
            }
            svg.push_str("  </g>\n");
        }
    }

    if let Some((prep, _)) = prepared_logo {
        let scale_to_mod = (total_modules as f32) / (config.size as f32);
        let center = (total_modules as f32) * 0.5;
        let bw = prep.backdrop_w as f32 * scale_to_mod;
        let bh = prep.backdrop_h as f32 * scale_to_mod;
        let lw = prep.rgba.width() as f32 * scale_to_mod;
        let lh = prep.rgba.height() as f32 * scale_to_mod;
        let bx = center - bw * 0.5;
        let by = center - bh * 0.5;
        let lx = center - lw * 0.5;
        let ly = center - lh * 0.5;

        if prep.shape != LogoShape::None {
            let (bg_hex, bg_op) = prep.backdrop_color.to_svg_fill_and_opacity();
            let op_attr = if prep.backdrop_color.a == 255 {
                String::new()
            } else {
                format!(r#" fill-opacity="{bg_op:.3}""#)
            };
            match prep.shape {
                LogoShape::Circle => {
                    let r = bw.min(bh) * 0.5;
                    let _ = writeln!(
                        svg,
                        r#"  <circle cx="{center:.3}" cy="{center:.3}" r="{r:.3}" fill="{bg_hex}"{op_attr}/>"#
                    );
                }
                LogoShape::Rounded => {
                    let rx = bw.min(bh) * 0.18;
                    let _ = writeln!(
                        svg,
                        r#"  <rect x="{bx:.3}" y="{by:.3}" width="{bw:.3}" height="{bh:.3}" rx="{rx:.3}" fill="{bg_hex}"{op_attr}/>"#
                    );
                }
                LogoShape::Square => {
                    let _ = writeln!(
                        svg,
                        r#"  <rect x="{bx:.3}" y="{by:.3}" width="{bw:.3}" height="{bh:.3}" fill="{bg_hex}"{op_attr}/>"#
                    );
                }
                LogoShape::None => {}
            }
        }

        let logo_png = encode_rgba_png(prep.rgba.as_raw(), prep.rgba.width(), prep.rgba.height())?;
        let b64 = BASE64_STANDARD.encode(&logo_png);

        match prep.shape {
            LogoShape::Circle => {
                let r = lw.min(lh) * 0.5;
                let _ = writeln!(
                    svg,
                    r#"  <defs><clipPath id="qr-logo-clip"><circle cx="{center:.3}" cy="{center:.3}" r="{r:.3}"/></clipPath></defs>"#
                );
                let _ = writeln!(
                    svg,
                    r#"  <image href="data:image/png;base64,{b64}" x="{lx:.3}" y="{ly:.3}" width="{lw:.3}" height="{lh:.3}" clip-path="url(#qr-logo-clip)"/>"#
                );
            }
            LogoShape::Rounded => {
                let rx = lw.min(lh) * 0.14;
                let _ = writeln!(
                    svg,
                    r#"  <defs><clipPath id="qr-logo-clip"><rect x="{lx:.3}" y="{ly:.3}" width="{lw:.3}" height="{lh:.3}" rx="{rx:.3}"/></clipPath></defs>"#
                );
                let _ = writeln!(
                    svg,
                    r#"  <image href="data:image/png;base64,{b64}" x="{lx:.3}" y="{ly:.3}" width="{lw:.3}" height="{lh:.3}" clip-path="url(#qr-logo-clip)"/>"#
                );
            }
            LogoShape::Square | LogoShape::None => {
                let _ = writeln!(
                    svg,
                    r#"  <image href="data:image/png;base64,{b64}" x="{lx:.3}" y="{ly:.3}" width="{lw:.3}" height="{lh:.3}"/>"#
                );
            }
        }
    }

    svg.push_str("</svg>\n");
    Ok((svg.into_bytes(), config.size, config.size))
}
