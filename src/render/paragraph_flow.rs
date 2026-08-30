//! Paragraph-to-flow lowering for the production renderer.

use super::*;

#[allow(clippy::too_many_arguments)]
pub(super) fn layout_paragraph(
    p: &Paragraph,
    out: &mut Vec<FlowItem>,
    marker: Option<&str>,
    tab_stops: &[TabStop],
    column_break_offsets: &[usize],
    default_tab_stop_pt: Option<f32>,
    line_spacing_hint: Option<LineSpacingHint>,
    geom: Geom,
    cx: &mut TextCx<'_>,
    capture: &mut LayoutCapture,
) {
    let shaped = shape_paragraph_content(
        p,
        marker,
        tab_stops,
        default_tab_stop_pt,
        line_spacing_hint,
        geom.content_w(),
        cx,
        capture,
        true,
    );
    let mut column_breaks = column_break_offsets.iter().copied().peekable();
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
        if let Some(item) = image_flow_item(img, geom) {
            out.push(FlowItem::Gap(PARA_GAP));
            out.push(item);
        }
    }
}
