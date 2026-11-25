use wasm_bindgen::prelude::*;

use crate::qr::{self, GenerateRequest, OutputFormat};

#[wasm_bindgen]
pub struct WasmQrResult {
    bytes: Vec<u8>,
    content_type: String,
    width: u32,
    height: u32,
    qr_version: i32,
    ecc_used: String,
    contrast: Option<String>,
    warnings_json: String,
}

#[wasm_bindgen]
impl WasmQrResult {
    #[wasm_bindgen(getter)]
    pub fn bytes(&self) -> Vec<u8> {
        self.bytes.clone()
    }

    #[wasm_bindgen(getter)]
    pub fn content_type(&self) -> String {
        self.content_type.clone()
    }

    #[wasm_bindgen(getter)]
    pub fn width(&self) -> u32 {
        self.width
    }

    #[wasm_bindgen(getter)]
    pub fn height(&self) -> u32 {
        self.height
    }

    #[wasm_bindgen(getter)]
    pub fn qr_version(&self) -> i32 {
        self.qr_version
    }

    #[wasm_bindgen(getter)]
    pub fn ecc_used(&self) -> String {
        self.ecc_used.clone()
    }

    #[wasm_bindgen(getter)]
    pub fn contrast(&self) -> Option<String> {
        self.contrast.clone()
    }

    #[wasm_bindgen(getter)]
    pub fn warnings_json(&self) -> String {
        self.warnings_json.clone()
    }
}

/// Generates a QR code from a JSON `GenerateRequest` payload directly inside WebAssembly.
#[wasm_bindgen]
pub fn generate_qr(request_json: &str) -> Result<WasmQrResult, JsValue> {
    let req: GenerateRequest = serde_json::from_str(request_json)
        .map_err(|e| JsValue::from_str(&format!("invalid JSON: {e}")))?;

    let (config, format) =
        qr::parse_generate_request(req).map_err(|e| JsValue::from_str(&e.to_string()))?;

    let output = qr::generate(&config, format).map_err(|e| JsValue::from_str(&e.to_string()))?;

    let content_type = match output.format {
        OutputFormat::Png => "image/png",
        OutputFormat::Svg => "image/svg+xml; charset=utf-8",
    }
    .to_string();

    let contrast = output.contrast_ratio.map(|r| format!("{r:.2}"));
    let warnings_json = serde_json::to_string(&output.warnings).unwrap_or_else(|_| "[]".into());

    Ok(WasmQrResult {
        bytes: output.bytes,
        content_type,
        width: output.width,
        height: output.height,
        qr_version: output.qr_version,
        ecc_used: output.ecc_used.to_string(),
        contrast,
        warnings_json,
    })
}
