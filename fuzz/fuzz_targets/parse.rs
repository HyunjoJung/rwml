#![no_main]
//! Fuzz the `.doc` / `.docx` parser on arbitrary bytes: the panic-free,
//! bounds-checked contract means no input may panic, abort, OOM, or hang — only
//! return an `Err` or a bounded document model.
//!
//! ```text
//! cargo +nightly fuzz run parse
//! ```

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = rdoc::extract_text(data);
    if let Ok(doc) = rdoc::Document::open(data) {
        let _ = doc.text();
        let _ = doc.to_markdown();
        let _ = doc.to_html();
        let _ = doc.images();
        // Also fuzz the write path: serializing a parsed model must never panic
        // or amplify (the `docx` feature is enabled in fuzz/Cargo.toml).
        let _ = doc.to_docx();
    }
});
