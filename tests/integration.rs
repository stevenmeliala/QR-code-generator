use qrforge::color::{contrast_ratio, Rgba};
use qrforge::qr::{self, EccLevel, LogoConfig, LogoShape, ModuleStyle, OutputFormat, QrConfig};

fn make_sample_logo_png(width: u32, height: u32, rgba: [u8; 4]) -> Vec<u8> {
    let mut pixels = Vec::with_capacity((width * height * 4) as usize);
    for _ in 0..(width * height) {
        pixels.extend_from_slice(&rgba);
    }
    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().expect("write logo PNG header");
        writer
            .write_image_data(&pixels)
            .expect("write logo PNG pixels");
        writer.finish().expect("finish logo PNG");
    }
    out
}

fn decode_qr_from_png(png_bytes: &[u8]) -> String {
    let gray = image::load_from_memory(png_bytes)
        .expect("decode PNG via image crate")
        .to_luma8();
    let mut prepared = rqrr::PreparedImage::prepare_from_greyscale(
        gray.width() as usize,
        gray.height() as usize,
        |x, y| gray.get_pixel(x as u32, y as u32)[0],
    );
    let grids = prepared.detect_grids();
    assert!(
        !grids.is_empty(),
        "expected at least one detectable QR grid"
    );
    let (_meta, content) = grids[0].decode().expect("decode detected QR grid");
    content
}

#[test]
fn test_color_parsing_and_contrast() {
    assert_eq!(Rgba::parse("#fff").unwrap(), Rgba::WHITE);
    assert_eq!(Rgba::parse("#000000").unwrap(), Rgba::BLACK);
    assert_eq!(
        Rgba::parse("11223380").unwrap(),
        Rgba::new(0x11, 0x22, 0x33, 0x80)
    );
    assert_eq!(Rgba::parse("TRANSPARENT").unwrap(), Rgba::TRANSPARENT);
    assert!(Rgba::parse("#12345").is_err());
    assert!(Rgba::parse("#gggggg").is_err());

    let ratio = contrast_ratio(Rgba::BLACK, Rgba::WHITE).unwrap();
    assert!((ratio - 21.0).abs() < 0.05, "unexpected contrast: {ratio}");
    assert!(contrast_ratio(Rgba::BLACK, Rgba::TRANSPARENT).is_none());
}

#[test]
fn test_png_generation_and_decode() {
    let mut cfg = QrConfig::new("https://rust-lang.org/learn");
    cfg.fg = Rgba::parse("#1d4ed8").unwrap();
    cfg.bg = Rgba::parse("#f8fafc").unwrap();
    cfg.size = 512;

    let out = qr::generate(&cfg, OutputFormat::Png).expect("generate PNG");
    assert_eq!(&out.bytes[..8], b"\x89PNG\r\n\x1a\n");
    assert_eq!(out.ecc_used, "M");
    assert!(out.warnings.is_empty());

    let decoded = decode_qr_from_png(&out.bytes);
    assert_eq!(decoded, "https://rust-lang.org/learn");
}

#[test]
fn test_png_with_center_logo_is_scannable() {
    let logo_bytes = make_sample_logo_png(64, 64, [220, 38, 38, 255]);

    let mut cfg = QrConfig::new("https://example.com/qrforge-logo-test");
    cfg.size = 600;
    cfg.fg = Rgba::parse("#0f172a").unwrap();
    cfg.bg = Rgba::parse("#ffffff").unwrap();
    cfg.ecc = EccLevel::Auto;
    cfg.logo = Some(LogoConfig {
        bytes: logo_bytes,
        scale: 0.22,
        padding_px: 8,
        shape: LogoShape::Rounded,
        background: None,
        clear_modules: true,
    });

    let out = qr::generate(&cfg, OutputFormat::Png).expect("generate logo PNG");
    assert_eq!(out.ecc_used, "H");

    let img = image::load_from_memory(&out.bytes)
        .expect("decode output PNG")
        .to_rgba8();
    let center = img.get_pixel(img.width() / 2, img.height() / 2);
    assert_eq!(center.0, [220, 38, 38, 255], "center pixel should be logo red");

    let decoded = decode_qr_from_png(&out.bytes);
    assert_eq!(decoded, "https://example.com/qrforge-logo-test");
}

#[test]
fn test_png_rounded_gradient_and_circle_logo() {
    let logo_bytes = make_sample_logo_png(64, 64, [16, 185, 129, 255]);

    let mut cfg = QrConfig::new("qrforge:gradient+rounded+circle");
    cfg.size = 640;
    cfg.fg = Rgba::parse("#0f172a").unwrap();
    cfg.fg_gradient_end = Some(Rgba::parse("#2563eb").unwrap());
    cfg.bg = Rgba::parse("#ffffff").unwrap();
    cfg.module_style = ModuleStyle::Rounded;
    cfg.logo = Some(LogoConfig {
        bytes: logo_bytes,
        scale: 0.20,
        padding_px: 10,
        shape: LogoShape::Circle,
        background: Some(Rgba::WHITE),
        clear_modules: true,
    });

    let out = qr::generate(&cfg, OutputFormat::Png).expect("generate styled PNG");
    let decoded = decode_qr_from_png(&out.bytes);
    assert_eq!(decoded, "qrforge:gradient+rounded+circle");
}

#[test]
fn test_svg_generation_and_embedded_logo() {
    let logo_bytes = make_sample_logo_png(48, 48, [37, 99, 235, 255]);

    let mut cfg = QrConfig::new("https://example.com/svg");
    cfg.fg = Rgba::parse("#111827").unwrap();
    cfg.fg_gradient_end = Some(Rgba::parse("#7c3aed").unwrap());
    cfg.bg = Rgba::parse("#ffffff").unwrap();

    let plain_svg = qr::generate(&cfg, OutputFormat::Svg).expect("generate plain SVG");
    let plain_str = String::from_utf8(plain_svg.bytes).expect("valid UTF-8 SVG");
    assert!(plain_str.contains("<svg "));
    assert!(plain_str.contains("<path "));
    assert!(plain_str.contains("<linearGradient"));

    cfg.logo = Some(LogoConfig::new(logo_bytes));
    let logo_svg = qr::generate(&cfg, OutputFormat::Svg).expect("generate logo SVG");
    let logo_str = String::from_utf8(logo_svg.bytes).expect("valid UTF-8 SVG");
    assert!(logo_str.contains("data:image/png;base64,"));
    assert!(logo_str.contains("clipPath"));
}

#[test]
fn test_low_contrast_warning() {
    let mut cfg = QrConfig::new("low contrast");
    cfg.fg = Rgba::parse("#cccccc").unwrap();
    cfg.bg = Rgba::parse("#dddddd").unwrap();

    let out = qr::generate(&cfg, OutputFormat::Png).expect("generate low-contrast PNG");
    assert!(
        out.warnings.iter().any(|w| w.contains("Low contrast")),
        "expected contrast warning, got: {:?}",
        out.warnings
    );
}
