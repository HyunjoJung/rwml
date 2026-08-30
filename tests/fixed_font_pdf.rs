#![cfg(feature = "render")]

use rwml::{DocBuilder, Error};
use skrifa::MetadataProvider;

fn fonts() -> Vec<Vec<u8>> {
    vec![
        rwml_fonts::noto_sans_kr_subset_with_hanja().to_vec(),
        rwml_fonts::noto_sans_arabic_subset().to_vec(),
        rwml_fonts::noto_sans_hebrew_subset().to_vec(),
    ]
}

#[test]
fn fixed_pdf_rejects_empty_and_unregistered_fonts() {
    let model = DocBuilder::new().paragraph("Body").build();
    for fonts in [vec![], vec![vec![]], vec![vec![1, 2, 3, 4]]] {
        assert!(matches!(
            rwml::try_render_pdf_with_fixed_fonts_and_report(&model, &fonts),
            Err(Error::Render(_))
        ));
    }
    assert!(rwml::try_render_pdf_with_fonts_and_report(&model, &[]).is_ok());
}

#[test]
fn fixed_pdf_rejects_fonts_that_cannot_be_embedded() {
    let mut font = rwml_fonts::noto_sans_kr_subset().to_vec();
    let table_count = u16::from_be_bytes([font[4], font[5]]) as usize;
    for table in font[12..12 + table_count * 16].chunks_exact_mut(16) {
        if matches!(&table[..4], b"glyf" | b"loca" | b"CFF " | b"CFF2") {
            table[..4].copy_from_slice(b"VOID");
        }
    }
    let mut collection = parley::fontique::Collection::new(parley::fontique::CollectionOptions {
        shared: false,
        system_fonts: false,
    });
    assert!(!collection
        .register_fonts(parley::fontique::Blob::from(font.clone()), None)
        .is_empty());
    let model = DocBuilder::new()
        .paragraph("Body must not disappear")
        .build();
    for fonts in [vec![font.clone()], vec![font, fonts().remove(0)]] {
        assert!(matches!(
            rwml::try_render_pdf_with_fixed_fonts_and_report(&model, &fonts),
            Err(Error::Render(_))
        ));
        #[cfg(feature = "docx")]
        {
            let document = rwml::Document::open(&rwml::write_docx(&model)).unwrap();
            assert!(matches!(
                document.try_to_pdf_with_fixed_fonts_and_report(&fonts),
                Err(Error::Render(_))
            ));
        }
    }
}

#[test]
fn fixed_pdf_rejects_missing_glyph_artwork_in_a_registered_outline_font() {
    let mut font = rwml_fonts::noto_sans_kr_subset().to_vec();
    let table_count = u16::from_be_bytes([font[4], font[5]]) as usize;
    for table in font[12..12 + table_count * 16].chunks_exact_mut(16) {
        if &table[..4] == b"glyf" {
            table[12..16].copy_from_slice(&0u32.to_be_bytes());
        }
    }
    let face = skrifa::FontRef::from_index(&font, 0).unwrap();
    assert!(face.outline_glyphs().format().is_some());
    assert!(krilla::text::Font::new(font.clone().into(), 0).is_some());
    let model = DocBuilder::new().paragraph("Missing outline data").build();
    assert!(matches!(
        rwml::try_render_pdf_with_fixed_fonts_and_report(&model, &[font]),
        Err(Error::Render(_))
    ));
}

#[test]
fn fixed_pdf_ignores_unrendered_missing_glyphs() {
    let hidden = rwml::Run {
        text: "Hidden \u{1f600}".into(),
        props: rwml::CharProps {
            hidden: true,
            ..Default::default()
        },
        ..Default::default()
    };
    let model = DocBuilder::new()
        .paragraph_runs([hidden, rwml::RunBuilder::new("Visible text").build()])
        .build();
    assert!(rwml::try_render_pdf_with_fixed_fonts_and_report(&model, &fonts()).is_ok());
}

#[test]
fn fixed_pdf_is_repeatable_and_matches_strict_page_layout() {
    let model = DocBuilder::new()
        .paragraph("Latin and Hangul \u{d55c}\u{ae00}")
        .paragraph("Arabic \u{633}\u{644}\u{627}\u{645}")
        .paragraph("Tab\tstop\nsoft\u{ad}hyphen zero\u{200b}width")
        .page_break()
        .paragraph("Hebrew \u{5e9}\u{5dc}\u{5d5}\u{5dd}")
        .header("Running header")
        .footer("Running footer")
        .table([["Cell one", "Cell two"]])
        .build();
    let fonts = fonts();
    let first = rwml::try_render_pdf_with_fixed_fonts_and_report(&model, &fonts).unwrap();
    let second = rwml::try_render_pdf_with_fixed_fonts_and_report(&model, &fonts).unwrap();
    let layout = rwml::layout_pages_with_fonts(&model, &fonts).unwrap();

    assert!(first.pdf.starts_with(b"%PDF"));
    assert_eq!(first.pdf, second.pdf);
    assert_eq!(first.report.to_json(), second.report.to_json());
    assert_eq!(first.report.pages, layout.pages);
}

#[test]
fn fixed_pdf_rejects_missing_glyphs_across_rendered_surfaces() {
    let missing = "Uncovered emoji \u{1f600}";
    let models = [
        DocBuilder::new().paragraph(missing).build(),
        DocBuilder::new().paragraph("Body").header(missing).build(),
        DocBuilder::new().paragraph("Body").footer(missing).build(),
        DocBuilder::new().table([[missing]]).build(),
    ];
    for (index, model) in models.iter().enumerate() {
        let result = rwml::try_render_pdf_with_fixed_fonts_and_report(model, &fonts());
        assert!(
            matches!(result, Err(Error::Render(_))),
            "surface {index} must not draw a host glyph or silently accept .notdef"
        );
        assert!(rwml::try_render_pdf_with_fonts_and_report(model, &fonts()).is_ok());
    }
}

#[cfg(feature = "docx")]
#[test]
fn opened_docx_fixed_pdf_preserves_source_hints_and_rejects_missing_note_glyphs() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let source = std::fs::read(root.join("corpus/public/synthetic/two-columns.docx")).unwrap();
    let document = rwml::Document::open(&source).unwrap();
    let fonts = fonts();
    let rendered = document
        .try_to_pdf_with_fixed_fonts_and_report(&fonts)
        .unwrap();
    let layout = document.layout_pages_with_fonts(&fonts).unwrap();
    assert_eq!(rendered.report.pages, layout.pages);

    let model = DocBuilder::new()
        .paragraph_runs([rwml::RunBuilder::new("Body")
            .footnote("Missing \u{1f600}")
            .build()])
        .build();
    let document = rwml::Document::open(&rwml::write_docx(&model)).unwrap();
    assert!(matches!(
        document.try_to_pdf_with_fixed_fonts_and_report(&fonts),
        Err(Error::Render(_))
    ));
}
