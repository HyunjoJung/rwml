//! Render any Word file (legacy `.doc` or modern `.docx`) to a native A4 PDF
//! through the shared document model. Requires the `render` feature.
//!
//! ```text
//! cargo run --example to_pdf --features render -- input.doc [output.pdf]
//! ```

use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let Some(input) = args.next() else {
        eprintln!("usage: to_pdf <input.doc|.docx> [output.pdf]");
        return ExitCode::from(2);
    };
    let out = args.next().map(PathBuf::from).unwrap_or_else(|| {
        let mut p = PathBuf::from(&input);
        p.set_extension("pdf");
        p
    });

    let bytes = match std::fs::read(&input) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("read {input}: {e}");
            return ExitCode::FAILURE;
        }
    };
    let doc = match rdoc::Document::open(&bytes) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("parse {input}: {e}");
            return ExitCode::FAILURE;
        }
    };
    let pdf = doc.to_pdf();
    if let Err(e) = std::fs::write(&out, &pdf) {
        eprintln!("write {}: {e}", out.display());
        return ExitCode::FAILURE;
    }
    eprintln!("wrote {} ({} bytes)", out.display(), pdf.len());
    ExitCode::SUCCESS
}
