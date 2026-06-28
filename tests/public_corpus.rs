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

use rdoc::{Document, DocumentWarning};

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

#[derive(Debug)]
struct ExpectedReport {
    path: String,
    comments: usize,
    footnotes: usize,
    endnotes: usize,
    text_boxes: usize,
    fields: usize,
    tracked_insertions: usize,
    tracked_deletions: usize,
    tracked_moves: usize,
    tracked_property_changes: usize,
    content_controls: usize,
    hyperlinks: usize,
    nested_tables: usize,
    floating_shapes: usize,
    charts: usize,
    ole_objects: usize,
    unsupported_metafiles: usize,
    warnings: Vec<String>,
}

fn parse_expected_reports(manifest: &str) -> Vec<ExpectedReport> {
    manifest
        .lines()
        .filter(|line| {
            let trimmed = line.trim();
            !trimmed.is_empty() && !trimmed.starts_with('#') && !trimmed.starts_with("path\t")
        })
        .map(|line| {
            let cols: Vec<_> = line.split('\t').collect();
            assert_eq!(cols.len(), 18, "bad manifest row: {line}");
            let parse = |i: usize| {
                cols[i]
                    .parse::<usize>()
                    .unwrap_or_else(|e| panic!("bad numeric column {i} in {line}: {e}"))
            };
            let warnings = if cols[17] == "-" {
                Vec::new()
            } else {
                cols[17].split('|').map(str::to_owned).collect()
            };
            ExpectedReport {
                path: cols[0].to_owned(),
                comments: parse(1),
                footnotes: parse(2),
                endnotes: parse(3),
                text_boxes: parse(4),
                fields: parse(5),
                tracked_insertions: parse(6),
                tracked_deletions: parse(7),
                tracked_moves: parse(8),
                tracked_property_changes: parse(9),
                content_controls: parse(10),
                hyperlinks: parse(11),
                nested_tables: parse(12),
                floating_shapes: parse(13),
                charts: parse(14),
                ole_objects: parse(15),
                unsupported_metafiles: parse(16),
                warnings,
            }
        })
        .collect()
}

fn warning_name(warning: &DocumentWarning) -> &'static str {
    match warning {
        DocumentWarning::UnsupportedFieldEvaluation { .. } => "UnsupportedFieldEvaluation",
        DocumentWarning::TrackedChangesPresent { .. } => "TrackedChangesPresent",
        DocumentWarning::IncompleteRevisionView { .. } => "IncompleteRevisionView",
        DocumentWarning::FloatingShapePlaceholderOnly { .. } => "FloatingShapePlaceholderOnly",
        DocumentWarning::ChartsPreservedButNotModeled { .. } => "ChartsPreservedButNotModeled",
        DocumentWarning::OleObjectsPreservedButNotModeled { .. } => {
            "OleObjectsPreservedButNotModeled"
        }
        DocumentWarning::UnsupportedMetafileImages { .. } => "UnsupportedMetafileImages",
        DocumentWarning::LegacyDocFlattenedSubdocuments { .. } => "LegacyDocFlattenedSubdocuments",
        DocumentWarning::PackageReadOnly { .. } => "PackageReadOnly",
    }
}

#[test]
fn public_corpus_report_matches_manifest() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("corpus/public");
    let manifest = fs::read_to_string(root.join("MANIFEST.tsv")).expect("read corpus manifest");
    let expected = parse_expected_reports(&manifest);
    assert!(
        expected.len() >= 5,
        "expected the report manifest to describe the synthetic feature corpus"
    );

    for row in expected {
        let path = root.join(&row.path);
        let bytes = fs::read(&path).unwrap_or_else(|e| panic!("read {}: {e}", row.path));
        let doc = Document::open(&bytes).unwrap_or_else(|e| panic!("open {}: {e}", row.path));
        let report = doc.report();
        let features = report.features;

        assert_eq!(features.comments, row.comments, "comments in {}", row.path);
        assert_eq!(
            features.footnotes, row.footnotes,
            "footnotes in {}",
            row.path
        );
        assert_eq!(features.endnotes, row.endnotes, "endnotes in {}", row.path);
        assert_eq!(
            features.text_boxes, row.text_boxes,
            "text boxes in {}",
            row.path
        );
        assert_eq!(features.fields, row.fields, "fields in {}", row.path);
        assert_eq!(
            features.tracked_insertions, row.tracked_insertions,
            "insertions in {}",
            row.path
        );
        assert_eq!(
            features.tracked_deletions, row.tracked_deletions,
            "deletions in {}",
            row.path
        );
        assert_eq!(
            features.tracked_moves, row.tracked_moves,
            "moves in {}",
            row.path
        );
        assert_eq!(
            features.tracked_property_changes, row.tracked_property_changes,
            "property changes in {}",
            row.path
        );
        assert_eq!(
            features.content_controls, row.content_controls,
            "content controls in {}",
            row.path
        );
        assert_eq!(
            features.hyperlinks, row.hyperlinks,
            "hyperlinks in {}",
            row.path
        );
        assert_eq!(
            features.nested_tables, row.nested_tables,
            "nested tables in {}",
            row.path
        );
        assert_eq!(
            features.floating_shapes, row.floating_shapes,
            "floating shapes in {}",
            row.path
        );
        assert_eq!(features.charts, row.charts, "charts in {}", row.path);
        assert_eq!(
            features.ole_objects, row.ole_objects,
            "OLE objects in {}",
            row.path
        );
        assert_eq!(
            features.unsupported_metafiles, row.unsupported_metafiles,
            "unsupported metafiles in {}",
            row.path
        );

        let mut warnings: Vec<_> = report
            .warnings
            .iter()
            .map(|warning| warning_name(warning).to_owned())
            .collect();
        warnings.sort();
        let mut expected_warnings = row.warnings;
        expected_warnings.sort();
        assert_eq!(warnings, expected_warnings, "warnings in {}", row.path);
    }
}

#[cfg(feature = "render")]
#[derive(Debug)]
struct ExpectedRenderReport {
    path: String,
    pages: usize,
    warnings: Vec<String>,
}

#[cfg(feature = "render")]
fn parse_expected_render_reports(manifest: &str) -> Vec<ExpectedRenderReport> {
    manifest
        .lines()
        .filter(|line| {
            let trimmed = line.trim();
            !trimmed.is_empty() && !trimmed.starts_with('#') && !trimmed.starts_with("path\t")
        })
        .map(|line| {
            let cols: Vec<_> = line.split('\t').collect();
            assert_eq!(cols.len(), 3, "bad render manifest row: {line}");
            let warnings = if cols[2] == "-" {
                Vec::new()
            } else {
                cols[2].split('|').map(str::to_owned).collect()
            };
            ExpectedRenderReport {
                path: cols[0].to_owned(),
                pages: cols[1]
                    .parse()
                    .unwrap_or_else(|e| panic!("bad page count in {line}: {e}")),
                warnings,
            }
        })
        .collect()
}

#[cfg(feature = "render")]
fn render_warning_name(warning: &rdoc::RenderWarning) -> &'static str {
    match warning {
        rdoc::RenderWarning::UnsupportedFieldEvaluation { .. } => "UnsupportedFieldEvaluation",
        rdoc::RenderWarning::FloatingShapePlaceholderOnly { .. } => "FloatingShapePlaceholderOnly",
        rdoc::RenderWarning::ChartsPreservedButNotModeled { .. } => "ChartsPreservedButNotModeled",
        rdoc::RenderWarning::OleObjectsPreservedButNotModeled { .. } => {
            "OleObjectsPreservedButNotModeled"
        }
        rdoc::RenderWarning::UnsupportedMetafileImages { .. } => "UnsupportedMetafileImages",
        rdoc::RenderWarning::UndecodableRasterImages { .. } => "UndecodableRasterImages",
    }
}

#[cfg(feature = "render")]
fn expected_render_unsupported(mut features: rdoc::FeatureInventory) -> rdoc::FeatureInventory {
    features.field_kinds = features.unsupported_field_kinds.clone();
    features.fields = features.field_kinds.iter().map(|item| item.count).sum();
    features
}

#[cfg(feature = "render")]
#[test]
fn public_corpus_render_report_matches_manifest() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("corpus/public");
    let manifest =
        fs::read_to_string(root.join("RENDER_MANIFEST.tsv")).expect("read render manifest");
    let expected = parse_expected_render_reports(&manifest);
    assert!(
        expected.len() >= 5,
        "expected the render manifest to describe the synthetic feature corpus"
    );

    for row in expected {
        let path = root.join(&row.path);
        let bytes = fs::read(&path).unwrap_or_else(|e| panic!("read {}: {e}", row.path));
        let doc = Document::open(&bytes).unwrap_or_else(|e| panic!("open {}: {e}", row.path));
        let features = doc.report().features;
        let rendered = doc
            .try_to_pdf_with_report()
            .unwrap_or_else(|e| panic!("render {}: {e}", row.path));

        assert!(
            rendered.pdf.starts_with(b"%PDF") && rendered.pdf.len() > 800,
            "invalid or tiny PDF for {}",
            row.path
        );
        assert_eq!(rendered.report.pages, row.pages, "pages in {}", row.path);
        assert_eq!(
            rendered.report.unsupported,
            expected_render_unsupported(features),
            "render unsupported inventory in {}",
            row.path
        );

        let mut warnings: Vec<_> = rendered
            .report
            .warnings
            .iter()
            .map(|warning| render_warning_name(warning).to_owned())
            .collect();
        warnings.sort();
        let mut expected_warnings = row.warnings;
        expected_warnings.sort();
        assert_eq!(
            warnings, expected_warnings,
            "render warnings in {}",
            row.path
        );
    }
}
