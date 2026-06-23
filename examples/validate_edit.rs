//! Package-preserving edit validator: for every `.docx` in <indir>, write a
//! passthrough-save copy to <outdir>/pass and an element-tree image-insert copy to
//! <outdir>/bimg. The companion `scripts/validate_edit_check.py` then checks every
//! output opens in python-docx (stricter OPC validation than rdoc's own reader),
//! that passthrough copies are byte-identical per part, and that image-insert copies
//! contain an inline image.
//!
//! ```text
//! cargo run --example validate_edit --features docx -- <indir> <outdir>
//! python scripts/validate_edit_check.py <indir> <outdir>
//! ```

use rdoc::Document;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (indir, outdir) = (&args[1], &args[2]);
    std::fs::create_dir_all(format!("{outdir}/pass")).unwrap();
    std::fs::create_dir_all(format!("{outdir}/bimg")).unwrap();

    // A genuinely valid 2×3 RGB PNG (correct chunk CRCs + a real zlib IDAT) for the
    // image-insertion pass — passes the reader's CRC-checked PNG validation.
    let png: Vec<u8> = vec![
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x03, 0x08, 0x02, 0x00, 0x00, 0x00, 0x36,
        0x88, 0x49, 0xD6, 0x00, 0x00, 0x00, 0x0B, 0x49, 0x44, 0x41, 0x54, 0x78, 0xDA, 0x63, 0x60,
        0xC0, 0x02, 0x00, 0x00, 0x15, 0x00, 0x01, 0x39, 0xC1, 0xE0, 0x23, 0x00, 0x00, 0x00, 0x00,
        0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ];

    let (mut ok, mut openfail, mut passfail, mut bimgfail) = (0, 0, 0, 0);
    for entry in std::fs::read_dir(indir).unwrap() {
        let p = entry.unwrap().path();
        if p.extension().and_then(|e| e.to_str()) != Some("docx") {
            continue;
        }
        let name = p.file_name().unwrap().to_string_lossy().to_string();
        let bytes = std::fs::read(&p).unwrap();

        let doc = match Document::open(&bytes) {
            Ok(d) => d,
            Err(_) => {
                openfail += 1;
                continue;
            }
        };
        match doc.save() {
            Ok(b) => std::fs::write(format!("{outdir}/pass/{name}"), b).unwrap(),
            Err(_) => passfail += 1,
        }

        // Element-tree image insertion + transactional rels.
        let mut doc3 = Document::open(&bytes).unwrap();
        let bimg = doc3
            .add_image_png(&png, "rdocimg.png")
            .and_then(|_| doc3.save());
        match bimg {
            Ok(b) => std::fs::write(format!("{outdir}/bimg/{name}"), b).unwrap(),
            Err(_) => bimgfail += 1,
        }
        ok += 1;
    }
    println!("processed={ok} openfail={openfail} passfail={passfail} bimgfail={bimgfail}");
}
