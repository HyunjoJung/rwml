#![cfg(all(feature = "render", feature = "bundled-fonts"))]

use rdoc::DocBuilder;

#[test]
fn bundled_font_rendering_produces_pdf_for_korean_hanja_and_latin_text() {
    let model = DocBuilder::new()
        .paragraph("안녕하세요 rdoc 보고서")
        .paragraph("契約書 第一條 (계약서)")
        .paragraph("Latin line")
        .build();

    let pdf = rdoc::try_render_pdf_bundled(&model).expect("bundled font render succeeds");

    assert!(!pdf.is_empty());
    assert!(pdf.starts_with(b"%PDF"));
}
