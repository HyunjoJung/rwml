#![no_main]
//! Fuzz the `.docx` preservation edit surface on arbitrary bytes: if the input
//! opens, run a short scripted edit sequence and save. Every edit may fail on
//! malformed input; the invariant is no panic, abort, OOM, or hang while
//! exercising XmlTree promotion/serialization and rollback paths.
//!
//! ```text
//! cargo +nightly fuzz run edit
//! ```

use libfuzzer_sys::fuzz_target;

const MINIMAL_PNG: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x03, 0x08, 0x02, 0x00, 0x00, 0x00, 0x36, 0x88, 0x49,
    0xD6, 0x00, 0x00, 0x00, 0x0B, 0x49, 0x44, 0x41, 0x54, 0x78, 0xDA, 0x63, 0x60, 0xC0, 0x02, 0x00,
    0x00, 0x15, 0x00, 0x01, 0x39, 0xC1, 0xE0, 0x23, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44,
    0xAE, 0x42, 0x60, 0x82,
];

fuzz_target!(|data: &[u8]| {
    if let Ok(mut doc) = rwml::Document::open(data) {
        let _ = doc.set_core_property(rwml::CoreProperty::Title, "fuzz");
        let _ = doc.add_comment_on_text("a", "fuzz comment", "fuzzer");
        let _ = doc.add_footnote_on_text("a", "fuzz footnote");
        let _ = doc.add_endnote_on_text("a", "fuzz endnote");
        let _ = doc.set_table_cell_text(0, 0, 0, "x");
        let _ = doc.set_hyperlink_target(0, "https://example.invalid/fuzz");
        let _ = doc.set_comment_text("0", "x");
        let _ = doc.replace_note_text("a", "b");
        let _ = doc.replace_header_footer_text("a", "b");
        let _ = doc.replace_text_in_part("word/document.xml", "a", "b");
        let _ = doc.replace_body_text("a", "b");
        let _ = doc.set_field_result(0, "x");
        let _ = doc.fill_content_control_by_tag("k", "v");
        let _ = doc.fill_template_fields([("k", "v")]);
        let _ = doc.add_image_png(MINIMAL_PNG, "f.png");
        if data.first().copied().unwrap_or_default() & 1 == 0 {
            let _ = doc.accept_all_revisions();
        } else {
            let _ = doc.reject_all_revisions();
        }
        let _ = doc.save();
    }
});
