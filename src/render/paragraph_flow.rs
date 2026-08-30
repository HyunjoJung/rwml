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

#[derive(Clone, Debug, PartialEq)]
pub(super) struct ParagraphFragmentCursor {
    pub(super) source_char: usize,
    pub(super) marker_emitted: bool,
    pub(super) page_field_indices: Rc<[Option<usize>]>,
    pub(super) pending_images: Rc<[Image]>,
}

impl Default for ParagraphFragmentCursor {
    fn default() -> Self {
        Self {
            source_char: 0,
            marker_emitted: false,
            page_field_indices: Rc::from(Vec::<Option<usize>>::new()),
            pending_images: Rc::from(Vec::<Image>::new()),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct FragmentTrack {
    pub(super) width: f32,
    pub(super) height: f32,
}

pub(super) struct ParagraphFragment {
    pub(super) lines: Vec<LineLayout>,
    pub(super) images: Vec<Image>,
    pub(super) next: Option<ParagraphFragmentCursor>,
    pub(super) deferred: bool,
}

fn paragraph_source_chars(paragraph: &Paragraph) -> usize {
    paragraph
        .runs
        .iter()
        .map(|run| run.text.chars().count())
        .fold(0usize, usize::saturating_add)
}

fn text_after_chars(text: &str, count: usize) -> String {
    if count == 0 {
        return text.to_owned();
    }
    text.char_indices()
        .nth(count)
        .map_or_else(String::new, |(byte, _)| text[byte..].to_owned())
}

fn paragraph_tail(paragraph: &Paragraph, source_char: usize) -> (Paragraph, Vec<usize>) {
    let mut remaining = source_char;
    let mut runs = Vec::new();
    let mut original_run_indices = Vec::new();
    for (run_index, run) in paragraph.runs.iter().enumerate() {
        let run_chars = run.text.chars().count();
        if run_chars == 0 {
            if remaining == 0 {
                runs.push(run.clone());
                original_run_indices.push(run_index);
            }
            continue;
        }
        if remaining >= run_chars {
            remaining -= run_chars;
            continue;
        }
        let mut tail = run.clone();
        tail.text = text_after_chars(&tail.text, remaining);
        runs.push(tail);
        original_run_indices.push(run_index);
        remaining = 0;
    }
    let mut props = paragraph.props.clone();
    if source_char > 0 {
        props.indent.first_line_pt = None;
        props.indent.hanging_pt = None;
    }
    (Paragraph { props, runs }, original_run_indices)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn shape_paragraph_fragment(
    paragraph: &Paragraph,
    marker: Option<&str>,
    tab_stops: &[TabStop],
    default_tab_stop_pt: Option<f32>,
    line_spacing_hint: Option<LineSpacingHint>,
    track: FragmentTrack,
    cursor: ParagraphFragmentCursor,
    cx: &mut TextCx<'_>,
    capture: &mut LayoutCapture,
) -> ParagraphFragment {
    shape_paragraph_fragment_with_pagination(
        paragraph,
        marker,
        tab_stops,
        default_tab_stop_pt,
        line_spacing_hint,
        track,
        PaginationHint::default(),
        true,
        cursor,
        cx,
        capture,
    )
}

#[allow(clippy::too_many_arguments)]
pub(super) fn shape_paragraph_fragment_with_pagination(
    paragraph: &Paragraph,
    marker: Option<&str>,
    tab_stops: &[TabStop],
    default_tab_stop_pt: Option<f32>,
    line_spacing_hint: Option<LineSpacingHint>,
    track: FragmentTrack,
    pagination: PaginationHint,
    fresh_track: bool,
    cursor: ParagraphFragmentCursor,
    cx: &mut TextCx<'_>,
    capture: &mut LayoutCapture,
) -> ParagraphFragment {
    let total_chars = paragraph_source_chars(paragraph);
    let source_start = cursor.source_char.min(total_chars);
    let initialized = cursor.page_field_indices.len() == paragraph.runs.len();
    let (page_field_indices, pending_images) = if initialized {
        (
            cursor.page_field_indices.clone(),
            cursor.pending_images.clone(),
        )
    } else {
        (
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
            ),
            Rc::from(
                paragraph
                    .runs
                    .iter()
                    .filter(|run| !run.props.hidden)
                    .filter_map(|run| run.image.clone())
                    .collect::<Vec<_>>(),
            ),
        )
    };
    if source_start == total_chars {
        return ParagraphFragment {
            lines: Vec::new(),
            images: pending_images.to_vec(),
            next: None,
            deferred: false,
        };
    }

    let (tail, original_run_indices) = paragraph_tail(paragraph, source_start);
    let tail_page_field_indices = original_run_indices
        .iter()
        .map(|run_index| page_field_indices[*run_index])
        .collect::<Vec<_>>();
    let fragment_marker = (!cursor.marker_emitted && source_start == 0)
        .then_some(marker)
        .flatten();
    let width = if track.width.is_finite() && track.width > 0.0 {
        track.width
    } else {
        1.0
    };
    let height = if track.height.is_finite() {
        track.height.max(0.0)
    } else {
        0.0
    };
    let shaped = shape_paragraph_content_with_page_fields(
        &tail,
        fragment_marker,
        tab_stops,
        default_tab_stop_pt,
        line_spacing_hint,
        width,
        cx,
        capture,
        true,
        Some(&tail_page_field_indices),
    );
    let shaped_line_count = shaped.lines.len();

    let mut lines = Vec::new();
    let mut used_height = 0.0_f32;
    let mut advanced_to = source_start;
    for mut line in shaped.lines {
        let next_height = used_height + line.height.max(0.0);
        if !lines.is_empty() && advanced_to > source_start && next_height > height + f32::EPSILON {
            break;
        }
        if let Some(range) = line.char_range.as_mut() {
            range.start = range.start.saturating_add(source_start).min(total_chars);
            range.end = range.end.saturating_add(source_start).min(total_chars);
            advanced_to = advanced_to.max(range.end);
        }
        used_height = next_height;
        lines.push(line);
    }

    if lines.is_empty() || advanced_to <= source_start {
        return ParagraphFragment {
            lines,
            images: pending_images.to_vec(),
            next: None,
            deferred: false,
        };
    }
    if advanced_to < total_chars {
        let admitted = lines.len();
        let defer_keep_lines = pagination.keep_lines && source_start == 0 && !fresh_track;
        let defer_widow =
            pagination.widow_control && !fresh_track && (shaped_line_count <= 3 || admitted < 2);
        if defer_keep_lines || defer_widow {
            return ParagraphFragment {
                lines: Vec::new(),
                images: Vec::new(),
                next: Some(ParagraphFragmentCursor {
                    source_char: source_start,
                    marker_emitted: cursor.marker_emitted,
                    page_field_indices,
                    pending_images,
                }),
                deferred: true,
            };
        }
    }
    let next = (advanced_to < total_chars).then(|| ParagraphFragmentCursor {
        source_char: advanced_to,
        marker_emitted: cursor.marker_emitted || fragment_marker.is_some(),
        page_field_indices,
        pending_images: pending_images.clone(),
    });
    let images = if next.is_none() {
        pending_images.to_vec()
    } else {
        Vec::new()
    };
    ParagraphFragment {
        lines,
        images,
        next,
        deferred: false,
    }
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

pub(super) enum BodyFlowEntry<'a> {
    Ready(FlowItem),
    Paragraph(ParagraphFlowRequest<'a>),
}

pub(super) struct BodyFlowEntries<'a> {
    nodes: std::vec::IntoIter<BodyFlowNode<'a>>,
    ready: std::vec::IntoIter<FlowItem>,
    ready_remaining: usize,
}

impl<'a> Iterator for BodyFlowEntries<'a> {
    type Item = BodyFlowEntry<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if self.ready_remaining > 0 {
                self.ready_remaining -= 1;
                let item = self.ready.next();
                debug_assert!(item.is_some(), "ready-node count exceeds ready buffer");
                return item.map(BodyFlowEntry::Ready);
            }
            match self.nodes.next() {
                Some(BodyFlowNode::Ready(count)) => self.ready_remaining = count,
                Some(BodyFlowNode::Paragraph(request)) => {
                    return Some(BodyFlowEntry::Paragraph(request));
                }
                None => {
                    debug_assert!(
                        self.ready.as_slice().is_empty(),
                        "ready buffer exceeds node counts"
                    );
                    return None;
                }
            }
        }
    }
}

#[derive(Default)]
pub(super) struct BodyFlowQueue<'a> {
    nodes: Vec<BodyFlowNode<'a>>,
    // Ready items share one buffer; node counts splice paragraph requests into it.
    ready: Vec<FlowItem>,
    segment_start: usize,
}

impl<'a> BodyFlowQueue<'a> {
    pub(super) fn push_ready(&mut self, item: FlowItem) {
        self.ready.push(item);
    }

    pub(super) fn extend_ready(&mut self, items: impl IntoIterator<Item = FlowItem>) {
        self.ready.extend(items);
    }

    pub(super) fn push_paragraph(&mut self, request: ParagraphFlowRequest<'a>) {
        self.flush_ready();
        self.nodes.push(BodyFlowNode::Paragraph(request));
    }

    pub(super) fn ready_item_count(&self) -> usize {
        self.ready.len()
    }

    pub(super) fn into_entries(mut self) -> BodyFlowEntries<'a> {
        self.flush_ready();
        BodyFlowEntries {
            nodes: self.nodes.into_iter(),
            ready: self.ready.into_iter(),
            ready_remaining: 0,
        }
    }

    #[cfg(test)]
    pub(super) fn lower(self, cx: &mut TextCx<'_>, capture: &mut LayoutCapture) -> Vec<FlowItem> {
        lower_body_flow_entries(self.into_entries(), cx, capture)
    }

    fn materialize(&mut self, cx: &mut TextCx<'_>, capture: &mut LayoutCapture) {
        self.flush_ready();
        let entries = BodyFlowEntries {
            nodes: std::mem::take(&mut self.nodes).into_iter(),
            ready: std::mem::take(&mut self.ready).into_iter(),
            ready_remaining: 0,
        };
        let items = lower_body_flow_entries(entries, cx, capture);
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

fn lower_body_flow_entries(
    entries: BodyFlowEntries<'_>,
    cx: &mut TextCx<'_>,
    capture: &mut LayoutCapture,
) -> Vec<FlowItem> {
    let mut items = Vec::with_capacity(entries.ready.len());
    for entry in entries {
        match entry {
            BodyFlowEntry::Ready(item) => items.push(item),
            BodyFlowEntry::Paragraph(request) => {
                layout_paragraph(&request, &mut items, cx, capture)
            }
        }
    }
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
        layout_paragraph(&request, self, cx, capture);
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
    request: &ParagraphFlowRequest<'_>,
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
            &request(&second, second_indices),
            &mut second_items,
            &mut text_cx,
            &mut capture,
        );
        let mut first_items = Vec::new();
        layout_paragraph(
            &request(&first, first_indices),
            &mut first_items,
            &mut text_cx,
            &mut capture,
        );

        assert_eq!(capture.page_fields, vec![None, None]);
        assert_eq!(dynamic_page_field_indices(&first_items), vec![Some(0)]);
        assert_eq!(dynamic_page_field_indices(&second_items), vec![Some(1)]);
    }

    #[test]
    fn paragraph_request_can_be_lowered_repeatedly_by_shared_reference() {
        let paragraph = Paragraph {
            runs: vec![Run {
                text: "retained request".to_string(),
                ..Run::default()
            }],
            ..Paragraph::default()
        };
        let request = ParagraphFlowRequest {
            paragraph: &paragraph,
            marker: Some(Cow::Owned("3.".to_string())),
            tab_stops: &[],
            column_break_offsets: &[],
            default_tab_stop_pt: None,
            line_spacing_hint: None,
            geom: Geom::from_setup(&PageSetup::default()),
            page_field_indices: None,
        };
        let mut font_cx = strict_font_context(rwml_fonts::noto_sans_kr_subset().to_vec());
        let mut layout_cx: LayoutContext<rgb::Color> = LayoutContext::new();
        let mut font_cache = HashMap::new();
        let mut text_cx = TextCx {
            font_cx: &mut font_cx,
            layout_cx: &mut layout_cx,
            font_cache: &mut font_cache,
        };
        let mut capture = LayoutCapture::default();
        let mut first = Vec::new();
        let mut second = Vec::new();

        layout_paragraph(&request, &mut first, &mut text_cx, &mut capture);
        layout_paragraph(&request, &mut second, &mut text_cx, &mut capture);

        assert_eq!(flow_snapshot(&first), flow_snapshot(&second));
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
    fn body_flow_entry_cursor_exposes_ready_items_and_paragraphs_in_source_order() {
        let first = page_field_paragraph("1", false);
        let second = page_field_paragraph("2", false);
        let geom = Geom::from_setup(&PageSetup::default());
        let mut capture = LayoutCapture::page_fields();
        let first_indices = reserve_paragraph_page_fields(&first, &mut capture);
        let second_indices = reserve_paragraph_page_fields(&second, &mut capture);
        let mut queue = BodyFlowQueue::default();
        queue.push_ready(FlowItem::PageBreak);
        queue.push_paragraph(ParagraphFlowRequest {
            paragraph: &first,
            marker: None,
            tab_stops: &[],
            column_break_offsets: &[],
            default_tab_stop_pt: None,
            line_spacing_hint: None,
            geom,
            page_field_indices: first_indices,
        });
        queue.push_ready(FlowItem::Table {
            rows: Vec::new(),
            header_rows: 0,
        });
        queue.push_paragraph(ParagraphFlowRequest {
            paragraph: &second,
            marker: None,
            tab_stops: &[],
            column_break_offsets: &[],
            default_tab_stop_pt: None,
            line_spacing_hint: None,
            geom,
            page_field_indices: second_indices,
        });
        queue.push_ready(FlowItem::PaginationBoundary);

        let mut font_cx = strict_font_context(rwml_fonts::noto_sans_kr_subset().to_vec());
        let mut layout_cx: LayoutContext<rgb::Color> = LayoutContext::new();
        let mut font_cache = HashMap::new();
        let mut text_cx = TextCx {
            font_cx: &mut font_cx,
            layout_cx: &mut layout_cx,
            font_cache: &mut font_cache,
        };
        let mut entries = queue.into_entries();

        assert!(matches!(
            entries.next(),
            Some(BodyFlowEntry::Ready(FlowItem::PageBreak))
        ));
        let Some(BodyFlowEntry::Paragraph(first_request)) = entries.next() else {
            panic!("first deferred paragraph");
        };
        let mut first_items = Vec::new();
        layout_paragraph(&first_request, &mut first_items, &mut text_cx, &mut capture);
        assert_eq!(dynamic_page_field_indices(&first_items), vec![Some(0)]);
        assert!(matches!(
            entries.next(),
            Some(BodyFlowEntry::Ready(FlowItem::Table { .. }))
        ));
        let Some(BodyFlowEntry::Paragraph(second_request)) = entries.next() else {
            panic!("second deferred paragraph");
        };
        let mut second_items = Vec::new();
        layout_paragraph(
            &second_request,
            &mut second_items,
            &mut text_cx,
            &mut capture,
        );
        assert_eq!(dynamic_page_field_indices(&second_items), vec![Some(1)]);
        assert!(matches!(
            entries.next(),
            Some(BodyFlowEntry::Ready(FlowItem::PaginationBoundary))
        ));
        assert!(entries.next().is_none());
        assert_eq!(capture.page_fields, vec![None, None]);
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
