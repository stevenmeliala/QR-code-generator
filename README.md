# qrforge

A fast, zero-bloat Rust QR code generator with custom colors, gradients, module styles, center logo embedding, CLI support, an embedded local Web UI, and a browser WebAssembly build for static hosting.

## Features

- **Fast native & WebAssembly Rust rendering**: Direct flat-buffer RGBA compositing with fast scanline-copy paths and path-merged SVG output—running natively in the CLI/local server or directly in the browser via `wasm32-unknown-unknown`.
- **Custom colors & gradients**: Foreground/background hex colors (`#RGB`, `#RGBA`, `#RRGGBB`, `#RRGGBBAA`), `transparent` backgrounds, optional diagonal foreground gradients, and automatic WCAG contrast warnings.
- **Module styles**: `square`, `rounded`, and `dots` (with finder-pattern preservation for reliable scanning).
- **Center logo embedding**: Embeds PNG, JPEG, WebP, GIF, or BMP logos with Lanczos3 downscaling, automatic High (`H`) error correction, smart underlying module clearing, and `rounded`, `circle`, `square`, or `none` backdrop frames.
- **Dual interface**: Fast CLI (`qrforge generate`) plus a single-file Web UI (`qrforge serve` locally, or static `public/` + WASM on Vercel).

## Build

```sh
cargo build --release
```

The optimized binary is written to `target/release/qrforge`.

## CLI Usage

Generate a standard colored PNG:

```sh
./target/release/qrforge generate "https://rust-lang.org" \
  -o rust.png --fg "#0f172a" --fg-end "#2563eb" --bg "#f8fafc" --style rounded
```

Embed a center logo with a rounded backdrop (automatically switches ECC to `H`):

```sh
./target/release/qrforge generate "https://example.com" \
  -o branded.png --logo logo.png --logo-scale 0.22 --logo-shape rounded --fg "#1e293b"
```

Generate a compact SVG with a transparent background and preview in the terminal:

```sh
./target/release/qrforge generate "Hello World" \
  -o hello.svg --bg transparent --terminal
```

## Local Web UI

Launch the embedded studio server and open it in your browser:

```sh
./target/release/qrforge serve --port 3000 --open
```

## Deploy to Vercel

Vercel serves `public/` directly as a zero-function static site powered by the compiled Rust WebAssembly bundle in `public/pkg/` (configured in `vercel.json` and `.vercelignore`).

If you modify Rust QR code generation logic (`src/qr.rs`, `src/color.rs`, or `src/wasm.rs`), rebuild the static WASM bundle before committing and pushing:

```sh
./build-wasm.sh
```
