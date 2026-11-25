use std::io::Read as _;
use std::process::Command;
use std::sync::Arc;
use std::thread;

use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine as _;
use serde::Deserialize;
use serde_json::json;
use tiny_http::{Header, Method, Request, Response, Server, StatusCode};

use crate::color::Rgba;
use crate::qr::{self, EccLevel, LogoConfig, LogoShape, ModuleStyle, OutputFormat, QrConfig};
use crate::QrError;

const INDEX_HTML: &str = include_str!("../web/index.html");
const MAX_BODY_BYTES: usize = 10 * 1024 * 1024;

#[derive(Debug, Deserialize)]
struct GenerateRequest {
    data: String,
    format: Option<String>,
    size: Option<u32>,
    quiet_zone: Option<u32>,
    fg: Option<String>,
    fg_end: Option<String>,
    bg: Option<String>,
    ecc: Option<String>,
    style: Option<String>,
    logo_base64: Option<String>,
    logo_scale: Option<f32>,
    logo_padding: Option<u32>,
    logo_shape: Option<String>,
    logo_bg: Option<String>,
    clear_logo_modules: Option<bool>,
}

/// Starts the embedded multi-threaded HTTP server for the QR code studio web UI.
pub fn serve(host: &str, port: u16, open_browser: bool) -> Result<(), QrError> {
    let addr = format!("{host}:{port}");
    let server = Arc::new(
        Server::http(&addr)
            .map_err(|e| QrError::IoError(format!("failed to bind {addr}: {e}")))?,
    );

    let url = format!("http://{addr}");
    println!("qrforge studio listening on {url}");
    println!("Press Ctrl+C to stop.");

    if open_browser {
        open_url(&url);
    }

    let workers = thread::available_parallelism()
        .map(|n| n.get().clamp(2, 8))
        .unwrap_or(4);

    let mut handles = Vec::with_capacity(workers);
    for _ in 0..workers {
        let srv = Arc::clone(&server);
        handles.push(thread::spawn(move || {
            for request in srv.incoming_requests() {
                handle_request(request);
            }
        }));
    }

    for handle in handles {
        let _ = handle.join();
    }

    Ok(())
}

fn open_url(url: &str) {
    #[cfg(target_os = "macos")]
    let _ = Command::new("open").arg(url).spawn();
    #[cfg(target_os = "linux")]
    let _ = Command::new("xdg-open").arg(url).spawn();
}

fn handle_request(mut request: Request) {
    let path = request.url().split('?').next().unwrap_or("/");
    match (request.method(), path) {
        (&Method::Get, "/" | "/index.html") => {
            let resp = Response::from_string(INDEX_HTML)
                .with_status_code(StatusCode(200))
                .with_header(header("Content-Type", "text/html; charset=utf-8"))
                .with_header(header("Cache-Control", "no-store"));
            let _ = request.respond(resp);
        }
        (&Method::Get, "/api/health") => {
            let resp = json_response(200, &json!({ "status": "ok" }).to_string());
            let _ = request.respond(resp);
        }
        (&Method::Post, "/api/generate") => {
            let resp = handle_generate(&mut request);
            let _ = request.respond(resp);
        }
        _ => {
            let resp = json_response(404, &json!({ "error": "not found" }).to_string());
            let _ = request.respond(resp);
        }
    }
}

fn handle_generate(request: &mut Request) -> Response<std::io::Cursor<Vec<u8>>> {
    if let Some(len) = request.body_length() {
        if len > MAX_BODY_BYTES {
            return json_response(413, &json!({ "error": "request body exceeds 10 MB limit" }).to_string());
        }
    }

    let mut body = Vec::with_capacity(4096);
    if let Err(err) = request
        .as_reader()
        .take((MAX_BODY_BYTES + 1) as u64)
        .read_to_end(&mut body)
    {
        return json_response(400, &json!({ "error": format!("failed to read body: {err}") }).to_string());
    }
    if body.len() > MAX_BODY_BYTES {
        return json_response(413, &json!({ "error": "request body exceeds 10 MB limit" }).to_string());
    }

    let req: GenerateRequest = match serde_json::from_slice(&body) {
        Ok(r) => r,
        Err(err) => {
            return json_response(400, &json!({ "error": format!("invalid JSON: {err}") }).to_string());
        }
    };

    let format = match req.format.as_deref().unwrap_or("png").trim().to_ascii_lowercase().as_str() {
        "png" => OutputFormat::Png,
        "svg" => OutputFormat::Svg,
        other => {
            return json_response(
                400,
                &json!({ "error": format!("unknown format '{other}' (expected png or svg)") }).to_string(),
            );
        }
    };

    let mut cfg = QrConfig::new(req.data);
    if let Some(size) = req.size {
        cfg.size = size;
    }
    if let Some(qz) = req.quiet_zone {
        cfg.quiet_zone = qz;
    }

    if let Some(fg) = req.fg.as_deref() {
        match Rgba::parse(fg) {
            Ok(c) => cfg.fg = c,
            Err(e) => return json_response(400, &json!({ "error": e.to_string() }).to_string()),
        }
    }
    if let Some(fg_end) = req.fg_end.as_deref().filter(|s| !s.trim().is_empty()) {
        match Rgba::parse(fg_end) {
            Ok(c) => cfg.fg_gradient_end = Some(c),
            Err(e) => return json_response(400, &json!({ "error": e.to_string() }).to_string()),
        }
    }
    if let Some(bg) = req.bg.as_deref() {
        match Rgba::parse(bg) {
            Ok(c) => cfg.bg = c,
            Err(e) => return json_response(400, &json!({ "error": e.to_string() }).to_string()),
        }
    }
    if let Some(ecc) = req.ecc.as_deref() {
        match EccLevel::parse(ecc) {
            Ok(e) => cfg.ecc = e,
            Err(e) => return json_response(400, &json!({ "error": e.to_string() }).to_string()),
        }
    }
    if let Some(style) = req.style.as_deref() {
        match ModuleStyle::parse(style) {
            Ok(s) => cfg.module_style = s,
            Err(e) => return json_response(400, &json!({ "error": e.to_string() }).to_string()),
        }
    }

    if let Some(raw_logo) = req.logo_base64.as_deref().filter(|s| !s.trim().is_empty()) {
        let clean_b64 = raw_logo
            .split_once(";base64,")
            .map(|(_, rest)| rest)
            .unwrap_or(raw_logo)
            .trim();
        let logo_bytes = match BASE64_STANDARD.decode(clean_b64) {
            Ok(b) => b,
            Err(e) => {
                return json_response(
                    400,
                    &json!({ "error": format!("invalid logo base64: {e}") }).to_string(),
                );
            }
        };
        let mut logo = LogoConfig::new(logo_bytes);
        if let Some(scale) = req.logo_scale {
            logo.scale = scale;
        }
        if let Some(pad) = req.logo_padding {
            logo.padding_px = pad;
        }
        if let Some(shape) = req.logo_shape.as_deref() {
            match LogoShape::parse(shape) {
                Ok(s) => logo.shape = s,
                Err(e) => return json_response(400, &json!({ "error": e.to_string() }).to_string()),
            }
        }
        if let Some(lbg) = req.logo_bg.as_deref().filter(|s| !s.trim().is_empty()) {
            match Rgba::parse(lbg) {
                Ok(c) => logo.background = Some(c),
                Err(e) => return json_response(400, &json!({ "error": e.to_string() }).to_string()),
            }
        }
        if let Some(clear) = req.clear_logo_modules {
            logo.clear_modules = clear;
        }
        cfg.logo = Some(logo);
    }

    let output = match qr::generate(&cfg, format) {
        Ok(out) => out,
        Err(e) => return json_response(400, &json!({ "error": e.to_string() }).to_string()),
    };

    let content_type = match output.format {
        OutputFormat::Png => "image/png",
        OutputFormat::Svg => "image/svg+xml; charset=utf-8",
    };

    let mut resp = Response::from_data(output.bytes)
        .with_status_code(StatusCode(200))
        .with_header(header("Content-Type", content_type))
        .with_header(header("Cache-Control", "no-store"))
        .with_header(header("X-QR-Gen-Time-Us", &output.generation_us.to_string()))
        .with_header(header("X-QR-Version", &output.qr_version.to_string()))
        .with_header(header("X-QR-Ecc", output.ecc_used))
        .with_header(header("X-QR-Dimensions", &format!("{}x{}", output.width, output.height)));

    if let Some(ratio) = output.contrast_ratio {
        resp.add_header(header("X-QR-Contrast", &format!("{ratio:.2}")));
    }
    if !output.warnings.is_empty() {
        if let Ok(warn_json) = serde_json::to_string(&output.warnings) {
            resp.add_header(header("X-QR-Warnings", &warn_json));
        }
    }

    resp
}

fn json_response(status: u16, body: &str) -> Response<std::io::Cursor<Vec<u8>>> {
    Response::from_data(body.as_bytes().to_vec())
        .with_status_code(StatusCode(status))
        .with_header(header("Content-Type", "application/json"))
        .with_header(header("Cache-Control", "no-store"))
}

fn header(name: &str, value: &str) -> Header {
    Header::from_bytes(name.as_bytes(), value.as_bytes())
        .unwrap_or_else(|_| Header::from_bytes(&b"X-Invalid"[..], &b"1"[..]).unwrap())
}
