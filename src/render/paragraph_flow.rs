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

enum BodyFlowNode<'a> {
    Ready(usize),
    Paragraph(ParagraphFlowRequest<'a>),
}

#[derive(Default)]
pub(super) struct BodyFlowQueue<'a> {
    nodes: Vec<BodyFlowNode<'a>>,
    // Ready items share one buffer; node counts splice paragraph requests into it.
    ready: Vec<FlowItem>,
    segment_start: usize,
}

impl<'a> BodyFlowQueue<'a> {
    #[cfg(test)]
    pub(super) fn push_ready(&mut self, item: FlowItem) {
        self.ready.push(item);
    }

    pub(super) fn push_paragraph(&mut self, request: ParagraphFlowRequest<'a>) {
        self.flush_ready();
        self.nodes.push(BodyFlowNode::Paragraph(request));
    }

    pub(super) fn lower(
        mut self,
        cx: &mut TextCx<'_>,
        capture: &mut LayoutCapture,
    ) -> Vec<FlowItem> {
        self.flush_ready();
        lower_body_flow_nodes(self.nodes, self.ready, cx, capture)
    }

    fn materialize(&mut self, cx: &mut TextCx<'_>, capture: &mut LayoutCapture) {
        self.flush_ready();
        let items = lower_body_flow_nodes(
            std::mem::take(&mut self.nodes),
            std::mem::take(&mut self.ready),
            cx,
            capture,
        );
        self.ready = items;
        self.segment_start = self.ready.len();
        self.nodes.push(BodyFlowNode::Ready(self.ready.len()));
    }

    fn ready_items(&mut self) -> &mut Vec<FlowItem> {
        &mut self.ready
    }

    fn flush_ready(&mut self) {
        let count = self.ready.len().saturating_sub(self.segment_start);
        if count == 0 {
            return;
        }
        match self.nodes.last_mut() {
            Some(BodyFlowNode::Ready(previous)) => *previous = previous.saturating_add(count),
            _ => self.nodes.push(BodyFlowNode::Ready(count)),
        }
        self.segment_start = self.ready.len();
    }
}

fn lower_body_flow_nodes(
    nodes: Vec<BodyFlowNode<'_>>,
    ready: Vec<FlowItem>,
    cx: &mut TextCx<'_>,
    capture: &mut LayoutCapture,
) -> Vec<FlowItem> {
    let mut ready = ready.into_iter();
    let mut items = Vec::with_capacity(ready.len());
    for node in nodes {
        match node {
            BodyFlowNode::Ready(count) => items.extend(ready.by_ref().take(count)),
            BodyFlowNode::Paragraph(request) => {
                layout_paragraph(request, &mut items, cx, capture);
            }
        }
    }
    debug_assert!(ready.next().is_none());
    items
}

pub(super) trait BlockFlowSink<'a> {
    fn ready_items(&mut self) -> &mut Vec<FlowItem>;

    fn push_paragraph(
        &mut self,
        request: ParagraphFlowRequest<'a>,
        cx: &mut TextCx<'_>,
        capture: &mut LayoutCapture,
    );

    fn has_non_anchor(&mut self, cx: &mut TextCx<'_>, capture: &mut LayoutCapture) -> bool;
}

impl<'a> BlockFlowSink<'a> for Vec<FlowItem> {
    fn ready_items(&mut self) -> &mut Vec<FlowItem> {
        self
    }

    fn push_paragraph(
        &mut self,
        request: ParagraphFlowRequest<'a>,
        cx: &mut TextCx<'_>,
        capture: &mut LayoutCapture,
    ) {
        layout_paragraph(request, self, cx, capture);
    }

    fn has_non_anchor(&mut self, _cx: &mut TextCx<'_>, _capture: &mut LayoutCapture) -> bool {
        self.iter()
            .any(|item| !matches!(item, FlowItem::BlockStart { .. }))
    }
}

impl<'a> BlockFlowSink<'a> for BodyFlowQueue<'a> {
    fn ready_items(&mut self) -> &mut Vec<FlowItem> {
        BodyFlowQueue::ready_items(self)
    }

    fn push_paragraph(
        &mut self,
        request: ParagraphFlowRequest<'a>,
        _cx: &mut TextCx<'_>,
        _capture: &mut LayoutCapture,
    ) {
        BodyFlowQueue::push_paragraph(self, request);
    }

    fn has_non_anchor(&mut self, cx: &mut TextCx<'_>, capture: &mut LayoutCapture) -> bool {
        self.materialize(cx, capture);
        self.ready
            .iter()
            .any(|item| !matches!(item, FlowItem::BlockStart { .. }))
    }
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
    use crate::Row;

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

    fn flow_snapshot(items: &[FlowItem]) -> Vec<String> {
        items
            .iter()
            .map(|item| match item {
                FlowItem::BlockStart { index, .. } => format!("block:{index}"),
                FlowItem::TopBottomBand { top, bottom, .. } => {
                    format!("band:{}:{}", top.to_bits(), bottom.to_bits())
                }
                FlowItem::PaginationBoundary => "boundary".to_string(),
                FlowItem::Gap(gap) => format!("gap:{}", gap.to_bits()),
                FlowItem::Line(line) => format!(
                    "line:{}",
                    line.runs
                        .iter()
                        .map(|run| run.text.as_ref())
                        .collect::<String>()
                ),
                FlowItem::Row(row) => format!("row:{}", row.cells.len()),
                FlowItem::PageBreak => "page-break".to_string(),
                FlowItem::ColumnBreak => "column-break".to_string(),
                FlowItem::SectionColumnGap(gap) => {
                    format!("section-gap:{}", gap.to_bits())
                }
                FlowItem::SectionColumnLayout(layout) => {
                    format!("section-layout:{}", layout.columns.len())
                }
                FlowItem::SectionColumnRtl => "section-rtl".to_string(),
                FlowItem::SectionBreak(_) => "section-break".to_string(),
                FlowItem::Table { rows, header_rows } => {
                    format!("table:{}:{header_rows}", rows.len())
                }
                FlowItem::Picture { layout, .. } => {
                    format!("picture:{}", layout.bounds_h.to_bits())
                }
                FlowItem::Chart { w, h, .. } => {
                    format!("chart:{}:{}", w.to_bits(), h.to_bits())
                }
            })
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

    #[test]
    fn body_flow_queue_lowers_paragraphs_and_ready_items_in_source_order() {
        let paragraph = Paragraph {
            runs: vec![Run {
                text: "body".to_string(),
                ..Run::default()
            }],
            ..Paragraph::default()
        };
        let column_break_offsets = [paragraph.text().chars().count()];
        let geom = Geom::from_setup(&PageSetup::default());
        let mut capture = LayoutCapture::default();
        let mut queue = BodyFlowQueue::default();
        queue.push_ready(FlowItem::BlockStart {
            index: 0,
            pagination: PaginationHint::default(),
        });
        queue.push_ready(FlowItem::Gap(3.0));
        queue.push_ready(FlowItem::PageBreak);
        queue.push_paragraph(ParagraphFlowRequest {
            paragraph: &paragraph,
            marker: Some(Cow::Owned("7.".to_string())),
            tab_stops: &[],
            column_break_offsets: &column_break_offsets,
            default_tab_stop_pt: None,
            line_spacing_hint: None,
            geom,
            page_field_indices: None,
        });
        queue.push_ready(FlowItem::Table {
            rows: Vec::new(),
            header_rows: 0,
        });

        let mut font_cx = strict_font_context(rwml_fonts::noto_sans_kr_subset().to_vec());
        let mut layout_cx: LayoutContext<rgb::Color> = LayoutContext::new();
        let mut font_cache = HashMap::new();
        let mut text_cx = TextCx {
            font_cx: &mut font_cx,
            layout_cx: &mut layout_cx,
            font_cache: &mut font_cache,
        };
        let items = queue.lower(&mut text_cx, &mut capture);

        assert!(matches!(items[0], FlowItem::BlockStart { index: 0, .. }));
        assert!(matches!(items[1], FlowItem::Gap(gap) if gap == 3.0));
        assert!(matches!(items[2], FlowItem::PageBreak));
        let FlowItem::Line(line) = &items[3] else {
            panic!("deferred paragraph line");
        };
        let rendered = line
            .runs
            .iter()
            .map(|run| run.text.as_ref())
            .collect::<String>();
        assert!(rendered.contains("7."));
        assert!(rendered.contains("body"));
        assert!(matches!(items[4], FlowItem::ColumnBreak));
        assert!(matches!(items[5], FlowItem::Table { .. }));
        assert_eq!(items.len(), 6);
    }

    #[test]
    fn deferred_body_collector_matches_eager_flow_order() {
        let blocks = vec![
            Block::Paragraph(Paragraph {
                runs: vec![Run {
                    text: "first paragraph".to_string(),
                    ..Run::default()
                }],
                ..Paragraph::default()
            }),
            Block::Table(Table {
                rows: vec![Row {
                    cells: vec![Cell {
                        blocks: vec![Block::Paragraph(Paragraph {
                            runs: vec![Run {
                                text: "table cell".to_string(),
                                ..Run::default()
                            }],
                            ..Paragraph::default()
                        })],
                        ..Cell::default()
                    }],
                }],
                ..Table::default()
            }),
            Block::PageBreak,
            Block::Paragraph(Paragraph {
                props: ParaProps {
                    page_break_before: true,
                    ..ParaProps::default()
                },
                runs: vec![Run {
                    text: "last paragraph".to_string(),
                    ..Run::default()
                }],
            }),
        ];
        let geom = Geom::from_setup(&PageSetup::default());
        let mut font_cx = strict_font_context(rwml_fonts::noto_sans_kr_subset().to_vec());
        let mut layout_cx: LayoutContext<rgb::Color> = LayoutContext::new();
        let mut font_cache = HashMap::new();
        let mut text_cx = TextCx {
            font_cx: &mut font_cx,
            layout_cx: &mut layout_cx,
            font_cache: &mut font_cache,
        };

        let mut eager = Vec::new();
        let mut eager_capture = LayoutCapture::default();
        collect_blocks_inner(
            &blocks,
            &mut eager,
            geom,
            &mut text_cx,
            &mut eager_capture,
            BlockCollectionOptions {
                include_block_anchors: true,
                ..BlockCollectionOptions::default()
            },
        );
        let mut deferred = BodyFlowQueue::default();
        let mut deferred_capture = LayoutCapture::default();
        collect_blocks_inner(
            &blocks,
            &mut deferred,
            geom,
            &mut text_cx,
            &mut deferred_capture,
            BlockCollectionOptions {
                include_block_anchors: true,
                ..BlockCollectionOptions::default()
            },
        );
        let deferred = deferred.lower(&mut text_cx, &mut deferred_capture);

        assert_eq!(flow_snapshot(&deferred), flow_snapshot(&eager));
    }

    #[test]
    fn deferred_body_collector_preserves_page_field_order_around_tables() {
        let blocks = vec![
            Block::Paragraph(page_field_paragraph("1", false)),
            Block::Table(Table {
                rows: vec![Row {
                    cells: vec![Cell {
                        blocks: vec![Block::Paragraph(page_field_paragraph("2", false))],
                        ..Cell::default()
                    }],
                }],
                ..Table::default()
            }),
            Block::Paragraph(page_field_paragraph("3", false)),
        ];
        let geom = Geom::from_setup(&PageSetup::default());
        let mut font_cx = strict_font_context(rwml_fonts::noto_sans_kr_subset().to_vec());
        let mut layout_cx: LayoutContext<rgb::Color> = LayoutContext::new();
        let mut font_cache = HashMap::new();
        let mut text_cx = TextCx {
            font_cx: &mut font_cx,
            layout_cx: &mut layout_cx,
            font_cache: &mut font_cache,
        };
        let mut capture = LayoutCapture::page_fields();
        let mut queue = BodyFlowQueue::default();
        collect_blocks_inner(
            &blocks,
            &mut queue,
            geom,
            &mut text_cx,
            &mut capture,
            BlockCollectionOptions::default(),
        );
        let items = queue.lower(&mut text_cx, &mut capture);
        let mut indices = Vec::new();
        let mut record = |line: &LineLayout| {
            indices.extend(
                line.runs
                    .iter()
                    .filter_map(|run| run.dynamic.as_ref())
                    .map(|dynamic| dynamic.page_field_index),
            );
        };
        for item in &items {
            match item {
                FlowItem::Line(line) => record(line),
                FlowItem::Table { rows, .. } => {
                    for row in rows {
                        for cell in &row.cells {
                            for line in &cell.lines {
                                record(line);
                            }
                        }
                    }
                }
                _ => {}
            }
        }

        assert_eq!(capture.page_fields, vec![None, None, None]);
        assert_eq!(indices, vec![Some(0), Some(1), Some(2)]);
    }
}
