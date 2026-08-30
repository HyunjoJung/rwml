//! Paragraph-to-flow lowering for the production renderer.

use std::borrow::Cow;
use std::rc::Rc;

use super::*;

pub(super) struct ParagraphFlowRequest<'a> {
    pub(super) paragraph: &'a Paragraph,
    pub(super) marker: Option<Cow<'a, str>>,
    pub(super) tab_stops: &'a [TabStop],
    pub(super) column_break_offsets: &'a [usize],
    pub(super) default_tab_stop_pt: Option<f32>,
    pub(super) line_spacing_hint: Option<LineSpacingHint>,
    pub(super) geom: Geom,
    pub(super) page_field_indices: Option<Rc<[Option<usize>]>>,
}

pub(super) fn reserve_paragraph_page_fields(
    paragraph: &Paragraph,
    capture: &mut LayoutCapture,
) -> Option<Rc<[Option<usize>]>> {
    capture.collect_page_fields.then(|| {
        Rc::from(
            paragraph
                .runs
                .iter()
                .map(|run| {
                    if run.props.hidden {
                        None
                    } else {
                        page_field_index_for_field(&run.field, capture)
                    }
                })
                .collect::<Vec<_>>(),
        )
    })
}

pub(super) fn layout_paragraph(
    request: ParagraphFlowRequest<'_>,
    out: &mut Vec<FlowItem>,
    cx: &mut TextCx<'_>,
    capture: &mut LayoutCapture,
) {
    let shaped = shape_paragraph_content_with_page_fields(
        request.paragraph,
        request.marker.as_deref(),
        request.tab_stops,
        request.default_tab_stop_pt,
        request.line_spacing_hint,
        request.geom.content_w(),
        cx,
        capture,
        true,
        request.page_field_indices.as_deref(),
    );
    let mut column_breaks = request.column_break_offsets.iter().copied().peekable();
    for line in shaped.lines {
        if let Some(start) = line.char_range.map(|range| range.start) {
            while column_breaks
                .peek()
                .is_some_and(|break_offset| *break_offset < start)
            {
                out.push(FlowItem::ColumnBreak);
                column_breaks.next();
            }
        }
        out.push(FlowItem::Line(line));
    }
    out.extend(column_breaks.map(|_| FlowItem::ColumnBreak));
    for img in shaped.images {
        if let Some(item) = image_flow_item(img, request.geom) {
            out.push(FlowItem::Gap(PARA_GAP));
            out.push(item);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use parley::fontique::{Blob, Collection, CollectionOptions, SourceCache};
    use parley::{FontContext, LayoutContext};

    use super::*;

    fn strict_font_context(font: Vec<u8>) -> FontContext {
        let mut collection = Collection::new(CollectionOptions {
            shared: false,
            system_fonts: false,
        });
        collection.register_fonts(Blob::from(font), None);
        FontContext {
            collection,
            source_cache: SourceCache::default(),
        }
    }

    fn page_field_paragraph(text: &str, hidden: bool) -> Paragraph {
        Paragraph {
            runs: vec![Run {
                text: text.to_string(),
                props: CharProps {
                    hidden,
                    ..CharProps::default()
                },
                field: FieldRole::Simple {
                    instruction: "PAGE".to_string(),
                },
                ..Run::default()
            }],
            ..Paragraph::default()
        }
    }

    fn dynamic_page_field_indices(items: &[FlowItem]) -> Vec<Option<usize>> {
        items
            .iter()
            .filter_map(|item| match item {
                FlowItem::Line(line) => Some(line),
                _ => None,
            })
            .flat_map(|line| &line.runs)
            .filter_map(|run| run.dynamic.as_ref())
            .map(|dynamic| dynamic.page_field_index)
            .collect()
    }

    #[test]
    fn paragraph_requests_reserve_page_fields_before_deferred_lowering() {
        let first = Paragraph {
            runs: vec![
                page_field_paragraph("1", false).runs.remove(0),
                page_field_paragraph("hidden", true).runs.remove(0),
            ],
            ..Paragraph::default()
        };
        let second = page_field_paragraph("2", false);
        let geom = Geom::from_setup(&PageSetup::default());
        let mut capture = LayoutCapture::page_fields();
        let first_indices = reserve_paragraph_page_fields(&first, &mut capture);
        let second_indices = reserve_paragraph_page_fields(&second, &mut capture);
        let request = |paragraph, page_field_indices| ParagraphFlowRequest {
            paragraph,
            marker: None,
            tab_stops: &[],
            column_break_offsets: &[],
            default_tab_stop_pt: None,
            line_spacing_hint: None,
            geom,
            page_field_indices,
        };

        let mut font_cx = strict_font_context(rwml_fonts::noto_sans_kr_subset().to_vec());
        let mut layout_cx: LayoutContext<rgb::Color> = LayoutContext::new();
        let mut font_cache = HashMap::new();
        let mut text_cx = TextCx {
            font_cx: &mut font_cx,
            layout_cx: &mut layout_cx,
            font_cache: &mut font_cache,
        };
        let mut second_items = Vec::new();
        layout_paragraph(
            request(&second, second_indices),
            &mut second_items,
            &mut text_cx,
            &mut capture,
        );
        let mut first_items = Vec::new();
        layout_paragraph(
            request(&first, first_indices),
            &mut first_items,
            &mut text_cx,
            &mut capture,
        );

        assert_eq!(capture.page_fields, vec![None, None]);
        assert_eq!(dynamic_page_field_indices(&first_items), vec![Some(0)]);
        assert_eq!(dynamic_page_field_indices(&second_items), vec![Some(1)]);
    }
}
