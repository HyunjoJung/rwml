//! Paragraph-to-flow lowering for the production renderer.

use std::borrow::Cow;

use super::*;

pub(super) struct ParagraphFlowRequest<'a> {
    pub(super) paragraph: &'a Paragraph,
    pub(super) marker: Option<Cow<'a, str>>,
    pub(super) tab_stops: &'a [TabStop],
    pub(super) column_break_offsets: &'a [usize],
    pub(super) default_tab_stop_pt: Option<f32>,
    pub(super) line_spacing_hint: Option<LineSpacingHint>,
    pub(super) geom: Geom,
}

pub(super) fn layout_paragraph(
    request: ParagraphFlowRequest<'_>,
    out: &mut Vec<FlowItem>,
    cx: &mut TextCx<'_>,
    capture: &mut LayoutCapture,
) {
    let shaped = shape_paragraph_content(
        request.paragraph,
        request.marker.as_deref(),
        request.tab_stops,
        request.default_tab_stop_pt,
        request.line_spacing_hint,
        request.geom.content_w(),
        cx,
        capture,
        true,
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
