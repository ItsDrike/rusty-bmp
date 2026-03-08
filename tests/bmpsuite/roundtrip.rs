use std::{io::Cursor, path::PathBuf};

use bmp::raw::Bmp;
use rstest::rstest;

use super::support::{parse_bmp, require_suite_generated, to_rel_suite_path};

// NOTE: We intentionally do not test the questionable set of BMP images (x/)
// as this parser explicitly does not support such formats.

// "Bad" bmpsuite files that are intentionally accepted by the current parser:
// either because the issue is metadata-only, decode-stage semantic invalidity,
// or a policy choice to allow ambiguous masks.
static ALLOWED_TO_PARSE_BAD: &[&str] = &[
    // Extreme X/Y pixels-per-meter metadata skew; this is advisory metadata and
    // does not affect parse safety or structural validity.
    "b/baddens1.bmp",
    // Same as baddens1 with the axes swapped; still metadata-only weirdness.
    "b/baddens2.bmp",
    // Invalid RLE stream intended to trigger decoder overruns. We currently parse
    // container/header + payload bytes only and do not decode RLE yet.
    "b/badrle.bmp",
    // Invalid RLE4 stream with overrun patterns; accepted for the same reason as above.
    "b/badrle4.bmp",
    // Another malformed RLE4 payload (decoder-level invalidity, not header-level).
    "b/badrle4bis.bmp",
    // Another malformed RLE4 payload (decoder-level invalidity, not header-level).
    "b/badrle4ter.bmp",
    // 8-bit variant of malformed RLE payload; accepted until RLE decode validation exists.
    "b/badrlebis.bmp",
    // 8-bit variant of malformed RLE payload; accepted until RLE decode validation exists.
    "b/badrleter.bmp",
    // Palette indices in pixel data exceed palette length. This requires semantic pixel
    // interpretation, which is outside the current parser-only scope.
    "b/pal8badindex.bmp",
    // BITFIELDS mask has a missing blue channel (8:8:0 layout). This is ambiguous in spec
    // and intentionally allowed per project policy (do not reject missing mask channel).
    "b/rgb16-880.bmp",
];

fn assert_roundtrip_equivalent(original: &Bmp, rel_path: &str) {
    let mut serialized = Cursor::new(Vec::<u8>::new());
    original
        .write_unchecked(&mut serialized)
        .unwrap_or_else(|err| panic!("failed to write {rel_path}: {err}"));

    serialized.set_position(0);
    let reparsed = Bmp::read_checked(&mut serialized)
        .unwrap_or_else(|err| panic!("failed to re-parse roundtrip {rel_path}: {err}"));

    assert_eq!(original, &reparsed, "{rel_path}");
}

#[rstest]
fn bmpsuite_good_images(
    #[files("bmpsuite/g/*.bmp")]
    #[mode = path]
    path: PathBuf,
) {
    require_suite_generated();

    let rel_path = to_rel_suite_path(&path);

    let parsed = parse_bmp(&path).unwrap_or_else(|err| panic!("expected successful parse for {rel_path}: {err}"));
    assert_roundtrip_equivalent(&parsed, &rel_path);
}

#[rstest]
fn bmpsuite_bad_images(
    #[files("bmpsuite/b/*.bmp")]
    #[mode = path]
    path: PathBuf,
) {
    require_suite_generated();

    let rel_path = to_rel_suite_path(&path);

    let parsed = parse_bmp(&path);
    let should_parse = ALLOWED_TO_PARSE_BAD.contains(&rel_path.as_str());

    match (parsed, should_parse) {
        (Ok(parsed), true) => assert_roundtrip_equivalent(&parsed, &rel_path),
        (Ok(_), false) => panic!("expected parse failure for {rel_path}, but parser accepted it"),
        (Err(_), true) => panic!("expected parser to accept known-semantic bad fixture {rel_path}"),
        (Err(_), false) => {}
    }
}

#[rstest]
fn bmpsuite_questionable_images(
    #[files("bmpsuite/q/*.bmp")]
    #[mode = path]
    path: PathBuf,
) {
    require_suite_generated();

    let rel_path = to_rel_suite_path(&path);

    if let Ok(parsed) = parse_bmp(&path) {
        assert_roundtrip_equivalent(&parsed, &rel_path);
    }
}
