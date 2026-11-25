use std::fs;
use std::io::{self, Read as _};
use std::path::PathBuf;
use std::process;

use clap::{Args, Parser, Subcommand, ValueEnum};

use qrforge::color::Rgba;
use qrforge::qr::{self, EccLevel, LogoConfig, LogoShape, ModuleStyle, OutputFormat, QrConfig};
use qrforge::server;
use qrforge::QrError;

#[derive(Parser)]
#[command(
    name = "qrforge",
    version,
    about = "High-efficiency Rust QR code generator with custom colors, styles, center logos, and local Web UI"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Generate a QR code image (PNG or SVG) or terminal preview
    #[command(alias = "gen")]
    Generate(GenerateArgs),

    /// Launch the embedded local Web UI for live QR customization
    Serve(ServeArgs),
}

#[derive(Clone, Copy, ValueEnum)]
enum FormatArg {
    Png,
    Svg,
}

#[derive(Clone, Copy, ValueEnum)]
enum StyleArg {
    Square,
    Rounded,
    Dots,
}

#[derive(Clone, Copy, ValueEnum)]
enum EccArg {
    Auto,
    L,
    M,
    Q,
    H,
}

#[derive(Clone, Copy, ValueEnum)]
enum LogoShapeArg {
    Rounded,
    Circle,
    Square,
    None,
}

#[derive(Args)]
struct GenerateArgs {
    /// Text, URL, or payload to encode (omit when using --stdin)
    data: Option<String>,

    /// Read payload from stdin
    #[arg(long)]
    stdin: bool,

    /// Output file path (extension .svg selects SVG automatically unless --format is set)
    #[arg(short, long, default_value = "qr.png")]
    output: PathBuf,

    /// Explicit output format
    #[arg(short, long, value_enum)]
    format: Option<FormatArg>,

    /// Target width & height in pixels
    #[arg(short, long, default_value_t = 512)]
    size: u32,

    /// Quiet zone border width in modules
    #[arg(short, long, default_value_t = 4)]
    quiet_zone: u32,

    /// Foreground color (#RGB, #RGBA, #RRGGBB, #RRGGBBAA)
    #[arg(long, default_value = "#000000")]
    fg: String,

    /// Optional diagonal gradient end color
    #[arg(long)]
    fg_end: Option<String>,

    /// Background color (hex or 'transparent')
    #[arg(long, default_value = "#ffffff")]
    bg: String,

    /// Error correction level
    #[arg(short, long, value_enum, default_value_t = EccArg::Auto)]
    ecc: EccArg,

    /// Module geometry style
    #[arg(long, value_enum, default_value_t = StyleArg::Square)]
    style: StyleArg,

    /// Path to center logo image (PNG, JPEG, WebP, GIF, BMP)
    #[arg(short, long)]
    logo: Option<PathBuf>,

    /// Logo width ratio relative to QR symbol area (0.08..=0.35)
    #[arg(long, default_value_t = 0.22)]
    logo_scale: f32,

    /// Padding around logo inside its backdrop frame (pixels)
    #[arg(long, default_value_t = 8)]
    logo_padding: u32,

    /// Logo backdrop frame shape
    #[arg(long, value_enum, default_value_t = LogoShapeArg::Rounded)]
    logo_shape: LogoShapeArg,

    /// Logo backdrop fill color (defaults to QR background or white)
    #[arg(long)]
    logo_bg: Option<String>,

    /// Keep QR modules underneath the logo instead of clearing them
    #[arg(long)]
    no_clear_logo: bool,

    /// Print Unicode half-block preview to stdout
    #[arg(long)]
    terminal: bool,
}

#[derive(Args)]
struct ServeArgs {
    /// Interface address to bind
    #[arg(long, default_value = "127.0.0.1")]
    host: String,

    /// TCP port to listen on
    #[arg(short, long, default_value_t = 3000)]
    port: u16,

    /// Open the default web browser automatically
    #[arg(long)]
    open: bool,
}

fn main() {
    let cli = Cli::parse();
    if let Err(err) = run(cli) {
        eprintln!("error: {err}");
        process::exit(1);
    }
}

fn run(cli: Cli) -> Result<(), QrError> {
    match cli.command {
        Commands::Generate(args) => run_generate(args),
        Commands::Serve(args) => server::serve(&args.host, args.port, args.open),
    }
}

fn run_generate(args: GenerateArgs) -> Result<(), QrError> {
    let data = if args.stdin {
        let mut buf = String::new();
        io::stdin().read_to_string(&mut buf)?;
        buf.trim_end_matches(&['\r', '\n'][..]).to_string()
    } else if let Some(d) = args.data {
        d
    } else {
        return Err(QrError::InvalidInput(
            "provide <DATA> or pass --stdin".into(),
        ));
    };

    let format = match args.format {
        Some(FormatArg::Png) => OutputFormat::Png,
        Some(FormatArg::Svg) => OutputFormat::Svg,
        None => {
            if args
                .output
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| e.eq_ignore_ascii_case("svg"))
                .unwrap_or(false)
            {
                OutputFormat::Svg
            } else {
                OutputFormat::Png
            }
        }
    };

    let mut cfg = QrConfig::new(data);
    cfg.size = args.size;
    cfg.quiet_zone = args.quiet_zone;
    cfg.fg = Rgba::parse(&args.fg)?;
    cfg.fg_gradient_end = args.fg_end.as_deref().map(Rgba::parse).transpose()?;
    cfg.bg = Rgba::parse(&args.bg)?;
    cfg.ecc = match args.ecc {
        EccArg::Auto => EccLevel::Auto,
        EccArg::L => EccLevel::Low,
        EccArg::M => EccLevel::Medium,
        EccArg::Q => EccLevel::Quartile,
        EccArg::H => EccLevel::High,
    };
    cfg.module_style = match args.style {
        StyleArg::Square => ModuleStyle::Square,
        StyleArg::Rounded => ModuleStyle::Rounded,
        StyleArg::Dots => ModuleStyle::Dots,
    };

    if let Some(logo_path) = args.logo {
        let bytes = fs::read(&logo_path).map_err(|e| {
            QrError::IoError(format!("failed to read logo '{}': {e}", logo_path.display()))
        })?;
        let mut logo = LogoConfig::new(bytes);
        logo.scale = args.logo_scale;
        logo.padding_px = args.logo_padding;
        logo.shape = match args.logo_shape {
            LogoShapeArg::Rounded => LogoShape::Rounded,
            LogoShapeArg::Circle => LogoShape::Circle,
            LogoShapeArg::Square => LogoShape::Square,
            LogoShapeArg::None => LogoShape::None,
        };
        logo.background = args.logo_bg.as_deref().map(Rgba::parse).transpose()?;
        logo.clear_modules = !args.no_clear_logo;
        cfg.logo = Some(logo);
    }

    if args.terminal {
        let preview = qr::render_terminal(&cfg)?;
        print!("{preview}");
    }

    let output = qr::generate(&cfg, format)?;
    if let Some(parent) = args.output.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent)?;
    }
    fs::write(&args.output, &output.bytes)?;

    for warn in &output.warnings {
        eprintln!("warning: {warn}");
    }

    let ms = (output.generation_us as f64) / 1000.0;
    let contrast_str = output
        .contrast_ratio
        .map(|r| format!(" | contrast {r:.2}:1"))
        .unwrap_or_default();
    println!(
        "wrote {} ({}x{}, v{}, ECC {}{}, {:.2} ms)",
        args.output.display(),
        output.width,
        output.height,
        output.qr_version,
        output.ecc_used,
        contrast_str,
        ms
    );

    Ok(())
}
