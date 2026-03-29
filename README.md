# Rusty BMP

[![wakatime](https://wakatime.com/badge/github/ItsDrike/rusty-bmp.svg)](https://wakatime.com/badge/github/ItsDrike/rusty-bmp)

Rusty BMP is a native Rust BMP viewer, editor, encoder, and parser. It handles low-level BMP structures, exposes a reusable raw/runtime library layer, and ships with a desktop GUI for inspecting, transforming, converting, and saving bitmap images.

It started as a format deep-dive and slowly turned into a full image tool with transform history, steganography support, quantization, convolution filters, and a lot of attention paid to correctness and performance.

<p align="center">
  <em>Demo video coming soon</em>
</p>

<!--
Replace the placeholder above with a small demo clip / GIF later.

Example:

<p align="center">
  <img src="docs/demo.gif" alt="Rusty BMP demo" width="900" />
</p>
-->

## Highlights

- Spec-driven BMP parsing and validation, not just a minimal happy-path loader
- Desktop GUI for viewing, inspecting, transforming, and saving BMP images
- Support for indexed, direct-color, compressed, and bitfield-based BMP variants
- Rich transform pipeline with undo/redo, arbitrary step removal, and optional replay checkpoint caching
- Configurable LSB steganography with automatic detection and safe removal
- Multiple save targets, header versions, format conversion, and save-quality warnings
- Clean internal split between raw BMP structures, runtime image logic, and GUI code

## Feature Summary

### BMP parsing and validation

- `BITMAPCOREHEADER`, `BITMAPINFOHEADER`, `BITMAPV4HEADER`, and `BITMAPV5HEADER`
- Indexed-color BMPs with palettes
- Top-down and bottom-up images
- `BI_RGB`, `BI_RLE4`, `BI_RLE8`, and `BI_BITFIELDS`
- Strict structural validation for offsets, sizes, palette layout, masks, and header consistency
- Rich metadata inspection in the GUI, including palette entries and extended header fields

### Viewing and editing

- Smooth zooming and panning
- Transparency checkerboard with stable snapping behavior while zooming
- Pixel inspector with coordinates, RGBA values, and color swatch
- Interactive crop rectangle with on-image handles
- Transform history panel with undo, redo, clear, and arbitrary operation removal

### Image transforms

- Rotate left/right by 90 degrees
- Arbitrary-angle rotation with nearest, bilinear, and bicubic interpolation
- Horizontal and vertical mirroring
- Resize, skew, translate, and crop
- Invert, grayscale, sepia, brightness, and contrast
- Convolution presets: blur, sharpen, edge detect, emboss
- Fully custom convolution kernels with editable size, divisor, and bias

### Save and conversion features

- Save using multiple BMP header versions: Core, Info, V4, and V5
- Save as 1-bit, 4-bit, 8-bit, 16-bit, 24-bit, and 32-bit BMP variants
- Save indexed and compressed outputs including `RLE4`, `RLE8`, and `BITFIELDS`
- Wu quantization for paletted re-encoding
- Save warnings for transparency loss, color precision loss, heavy quantization, and steganography breakage
- Preservation of advanced V4/V5 metadata where applicable, including color-space related fields and ICC profile data

### Steganography

- Configurable LSB embedding with independent bit counts for `R`, `G`, `B`, and `A`
- Compact custom 80-bit steganography header
- Automatic hidden-data discovery by trying all valid channel configurations
- False-positive resistance through header validation and configuration self-checking
- Inspection/extraction of embedded payloads
- Secure steganography removal by zeroing the bits used by the detected payload

## Technical Notes

### Architecture

The project is intentionally split into layers:

- `src/raw` - low-level BMP structures, binary parsing, validation, and raw writing
- `src/runtime` - decoded image operations, transforms, quantization, steganography, and encoders
- `src/gui` + `src/main.rs` - desktop application, viewer, editing tools, and UI workflow

That split keeps the format logic separate from the GUI, and makes the core BMP functionality reusable outside the desktop app.

### Transform pipeline and history

Edits are modeled as a transform pipeline rather than being applied and forgotten. This makes it possible to:

- undo/redo cleanly,
- remove arbitrary operations from the middle of history,
- replay the remaining pipeline from the original image when needed,
- and combine reversible operations with lossy ones in a consistent way.

The runtime keeps `TransformPipeline` itself stateless and predictable. Checkpoint caching is handled by a separate executor type with configurable policy, so library users can opt into cached replay only when they want it.

### Performance

Performance was a major goal throughout the project. The implementation stays CPU-based, but it is not naive:

- many pixel-heavy operations are parallelized by rows with Rayon,
- output buffers are preallocated exactly,
- arbitrary rotation snaps to dedicated exact paths when the angle is effectively a multiple of 90 degrees,
- and separable convolution kernels automatically switch to a faster two-pass implementation.

The result is a tool that stays responsive even after the feature set grew well beyond a minimal BMP viewer.

### Testing

The project includes a fairly extensive automated test suite covering:

- raw parse/write roundtrips,
- decode/encode behavior,
- transform correctness,
- transform pipeline replay logic and executor checkpoint caching,
- steganography,
- and BMP fixture coverage via BMPSuite.

## Build and Run

### Prerequisites

- Rust stable with Edition 2024 support
- `cargo`
- On Linux, the usual native GUI dependencies required by `eframe` / `rfd` for your distro

### Run the app

```bash
cargo run --release
```

The release build is recommended; some transforms are intentionally fairly heavy, and the optimized build makes a noticeable difference.

### Build a release binary

```bash
cargo build --release
```

The resulting executable will be in `target/release/`.

## Tests

Run the full test suite with:

```bash
cargo test
```

Some BMPSuite-based tests auto-generate fixtures on demand, so if you want the full suite to work you should also have:

- `make`
- a C compiler

## Basic Usage

1. Launch the app with `cargo run --release`
2. Open a BMP file via `Browse...` or by pasting a path into the toolbar
3. Inspect file metadata and decoded image details in the right-side panel
4. Apply transforms from the editor controls
5. Use the history panel to undo, redo, clear, or remove individual operations
6. Choose the save header version / output format and save the result
7. Zoom in to inspect exact pixel values and color data

## Library Example

The repository also exposes a reusable library layer. A minimal parse + decode example looks like this:

```rust
use std::fs::File;

use bmp::{
    raw::Bmp,
    runtime::decode::decode_to_rgba,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut file = File::open("image.bmp")?;
    let bmp = Bmp::read_unchecked(&mut file)?;
    bmp.validate()?;
    let decoded = decode_to_rgba(&bmp)?;

    println!("{}x{}", decoded.width(), decoded.height());
    Ok(())
}
```

## Repository Layout

```text
src/
  raw/       BMP structures, headers, validation, binary IO
  runtime/   decoded image logic, transforms, quantization, steganography, encoding
  gui/       egui panels, tools, viewer widgets
  main.rs    native desktop application entry point
  lib.rs     reusable library exports
tests/       unit tests, integration tests, BMPSuite coverage
```

## License

This project is licensed under the GNU Lesser General Public License v3.0 only (`LGPL-3.0-only`). See `LICENSE.txt` for the full text.
