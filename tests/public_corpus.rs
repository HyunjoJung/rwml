//! Validates the in-repo public corpus (`corpus/public/**/*.docx`) through rdoc's public
//! API — a license-clean, dependency-free gate anyone (and CI) can run with `cargo test`.
//!
//! For each `.docx` it asserts the editor's core contract holds on a real/feature-rich file:
//!   1. `Document::open` succeeds;
//!   2. a no-op `open -> save` round-trips and **re-opens** (and is idempotent on a second
//!      save — the serialization is stable);
//!   3. `add_image_png` produces a package that re-opens with rdoc.
//!
//! Per-*part* byte-stability (the stronger "unmodeled content preserved" claim) is covered by
//! the crate's unit tests and by `examples/validate_edit.rs` + `scripts/validate_edit_check.py`
//! against the larger private corpus; this test keeps the public gate self-contained.
#![cfg(feature = "docx")]

use std::fs;
use std::path::{Path, PathBuf};

use rdoc::Document;

/// A genuinely valid 2x3 RGB PNG (correct chunk CRCs + a real zlib IDAT), so
/// `add_image_png`'s CRC-checked PNG validation accepts it.
const TINY_PNG: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x03, 0x08, 0x02, 0x00, 0x00, 0x00, 0x36, 0x88, 0x49,
    0xD6, 0x00, 0x00, 0x00, 0x0B, 0x49, 0x44, 0x41, 0x54, 0x78, 0xDA, 0x63, 0x60, 0xC0, 0x02, 0x00,
    0x00, 0x15, 0x00, 0x01, 0x39, 0xC1, 0xE0, 0x23, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44,
    0xAE, 0x42, 0x60, 0x82,
];

fn collect_docx(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    for entry in rd.flatten() {
        let p = entry.path();
        if p.is_dir() {
            collect_docx(&p, out);
        } else if p.extension().and_then(|e| e.to_str()) == Some("docx") {
            out.push(p);
        }
    }
}

#[test]
fn public_corpus_opens_roundtrips_and_edits() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("corpus/public");
    let mut files = Vec::new();
    collect_docx(&root, &mut files);
    files.sort();

    assert!(
        files.len() >= 5,
        "expected the public corpus to hold >=5 .docx (found {}); is corpus/public present?",
        files.len()
    );

    for path in &files {
        let label = path.strip_prefix(&root).unwrap_or(path).display();
        let bytes = fs::read(path).unwrap_or_else(|e| panic!("read {label}: {e}"));

        // 1. opens
        let doc = Document::open(&bytes).unwrap_or_else(|e| panic!("open {label}: {e}"));

        // 2. no-op save re-opens, and a second save is byte-identical (stable serialization)
        let saved = doc.save().unwrap_or_else(|e| panic!("save {label}: {e}"));
        let reopened = Document::open(&saved).unwrap_or_else(|e| panic!("reopen {label}: {e}"));
        let saved2 = reopened
            .save()
            .unwrap_or_else(|e| panic!("re-save {label}: {e}"));
        assert_eq!(saved, saved2, "no-op save is not idempotent for {label}");

        // 3. add_image_png yields a package that re-opens
        let mut edit = Document::open(&bytes).unwrap();
        edit.add_image_png(TINY_PNG, "rdoc_public_corpus_test.png")
            .unwrap_or_else(|e| panic!("add_image_png {label}: {e}"));
        let edited = edit
            .save()
            .unwrap_or_else(|e| panic!("save edited {label}: {e}"));
        Document::open(&edited).unwrap_or_else(|e| panic!("reopen edited {label}: {e}"));
    }
}
