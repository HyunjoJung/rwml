#![no_main]
//! Fuzz the PDF renderer: open arbitrary bytes and, on a successful parse, render
//! the model to PDF. The panic-free contract extends to the typesetter — no parsed
//! document (however malformed the source) may panic, abort, OOM, or hang the
//! `parley`/`krilla` layout and emit pipeline.
//!
//! ```text
//! cargo +nightly fuzz run render
//! ```

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(doc) = rwml::Document::open(data) {
        let _ = doc.to_pdf();
    }
});
