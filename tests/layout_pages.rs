#![cfg(feature = "render")]

use std::panic::{catch_unwind, AssertUnwindSafe};

use rwml::{DocBuilder, Error};

fn fonts() -> Vec<Vec<u8>> {
    vec![rwml_fonts::noto_sans_kr_subset().to_vec()]
}

#[test]
fn layout_pages_rejects_empty_or_unregistered_strict_fonts() {
    let model = DocBuilder::new().paragraph("strict font test").build();

    assert!(matches!(
        rwml::layout_pages_with_fonts(&model, &[]),
        Err(Error::Render(_))
    ));

    let garbage = catch_unwind(AssertUnwindSafe(|| {
        rwml::layout_pages_with_fonts(&model, &[vec![1, 2, 3, 4, 5]])
    }));
    assert!(garbage.is_ok(), "garbage font bytes must not panic");
    assert!(matches!(garbage.unwrap(), Err(Error::Render(_))));
}

#[test]
fn layout_pages_reports_physical_pages_for_body_page_fields() {
    let model = DocBuilder::new()
        .field("PAGE", "stale one")
        .page_break()
        .field("PAGE", "stale two")
        .page_break()
        .paragraph("last page")
        .build();

    let pages = rwml::layout_pages_with_fonts(&model, &fonts()).expect("layout pages");

    assert_eq!(pages.pages, 3);
    assert_eq!(pages.page_fields, vec![Some(1), Some(2)]);
    assert_eq!(
        rwml::layout_pages_with_fonts(&model, &fonts()).expect("layout pages repeat"),
        pages
    );
}

#[test]
fn layout_pages_reports_first_page_each_top_level_block_touches() {
    let model = DocBuilder::new()
        .paragraph("first block")
        .page_break()
        .paragraph("post-break block")
        .build();

    let pages = rwml::layout_pages_with_fonts(&model, &fonts()).expect("layout pages");

    assert_eq!(pages.block_pages[0], Some(1));
    assert_eq!(pages.block_pages[2], Some(2));
}
