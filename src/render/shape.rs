//! Parley shaping, source-cluster lowering, and bounded tab reflow.

use super::*;

pub(super) fn paragraph_indent_layout(
    props: &ParaProps,
    available_width: f32,
    extra_left: f32,
) -> ParagraphIndentLayout {
    let left = props.indent.left_pt.unwrap_or(0.0).max(0.0) + extra_left.max(0.0);
    let right = props.indent.right_pt.unwrap_or(0.0).max(0.0);
    let first_line = props
        .indent
        .first_line_pt
        .filter(|value| value.is_finite() && *value != 0.0);
    let hanging = props
        .indent
        .hanging_pt
        .filter(|value| value.is_finite() && *value > 0.0);
    let (x_indent, text_indent, hanging_indent) = if let Some(first_line) = first_line {
        (left, first_line, false)
    } else if let Some(hanging) = hanging {
        ((left - hanging).max(0.0), hanging.min(left), true)
    } else {
        (left, 0.0, false)
    };
    ParagraphIndentLayout {
        x_indent,
        wrap_width: (available_width - x_indent - right).max(20.0),
        text_indent,
        hanging_indent,
    }
}

impl<'a> StyledText<'a> {
    /// A styled string with only character-property ranges (no links or dynamics).
    pub(super) fn plain(ranges: &'a [(usize, usize, CharProps)]) -> StyledText<'a> {
        StyledText {
            ranges,
            links: &[],
            dynamic_ranges: &[],
        }
    }
}

/// Shape a styled text string into positioned lines at a given wrap `width`.
pub(super) fn shape(
    text: &str,
    styled: StyledText<'_>,
    heading_level: Option<u8>,
    align: Alignment,
    width: f32,
    cx: &mut TextCx<'_>,
) -> Vec<LineLayout> {
    shape_with_options(
        text,
        styled,
        heading_level,
        align,
        width,
        ShapeOptions::default(),
        cx,
    )
}

pub(super) fn shape_with_options(
    text: &str,
    styled: StyledText<'_>,
    heading_level: Option<u8>,
    align: Alignment,
    width: f32,
    options: ShapeOptions<'_>,
    cx: &mut TextCx<'_>,
) -> Vec<LineLayout> {
    let StyledText {
        ranges,
        links,
        dynamic_ranges,
    } = styled;
    let base_size = heading_size(heading_level);
    let heading = heading_level.is_some();

    let mut builder = cx.layout_cx.ranged_builder(cx.font_cx, text, 1.0, false);
    builder.push_default(StyleProperty::Brush(rgb::Color::new(0, 0, 0)));
    builder.push_default(StyleProperty::FontFamily(font_stack()));
    builder.push_default(StyleProperty::FontSize(base_size));
    let line_height = options
        .line_height
        .filter(|value| value.is_finite() && *value > 0.0)
        .map(|value| value.clamp(0.25, 10.0))
        .unwrap_or(1.35);
    builder.push_default(StyleProperty::LineHeight(
        parley::style::LineHeight::FontSizeRelative(line_height),
    ));
    if heading {
        builder.push_default(StyleProperty::FontWeight(FontWeight::new(700.0)));
    }
    for (s, e, props) in ranges {
        if props.bold && !heading {
            builder.push(StyleProperty::FontWeight(FontWeight::new(700.0)), *s..*e);
        }
        if props.italic {
            builder.push(StyleProperty::FontStyle(FontStyle::Italic), *s..*e);
        }
        if props.underline {
            builder.push(StyleProperty::Underline(true), *s..*e);
        }
        if props.strike {
            builder.push(StyleProperty::Strikethrough(true), *s..*e);
        }
        // Authored size is the base for synthetic small capitals and vertical
        // alignment. Parley then shapes the reduced glyphs at their real advance.
        let authored_size = props
            .size_half_pt
            .filter(|half| *half > 0)
            .map(|half| half as f32 / 2.0);
        let scale = synthetic_font_scale(props);
        if authored_size.is_some() || scale != 1.0 {
            builder.push(
                StyleProperty::FontSize(authored_size.unwrap_or(base_size) * scale),
                *s..*e,
            );
        }
        // Authored font family, tried before the Korean-capable fallbacks.
        if let Some(name) = &props.font {
            if !name.is_empty() {
                builder.push(StyleProperty::FontFamily(named_stack(name)), *s..*e);
            }
        }
    }

    let mut layout = builder.build(text);
    if options.text_indent.is_finite() && options.text_indent != 0.0 {
        layout.set_text_indent(
            options.text_indent.clamp(-width + 1.0, width - 1.0),
            IndentOptions {
                each_line: false,
                hanging: options.hanging_indent,
            },
        );
    }
    let layout_rtl = layout.is_rtl();
    let rtl_tabs = options.rtl_tabs && layout_rtl;
    let adjust_tabs = text.contains('\t')
        && if rtl_tabs {
            matches!(align, Alignment::Right | Alignment::Start)
                && (options.tab_stops.is_empty()
                    || options
                        .tab_stops
                        .iter()
                        .any(|stop| stop.alignment != TabAlignment::Clear))
        } else {
            !layout_rtl
                && (matches!(align, Alignment::Left | Alignment::Start)
                    || options.tab_stops.is_empty()
                    || options
                        .tab_stops
                        .iter()
                        .any(|stop| stop.alignment != TabAlignment::Clear))
        };

    let text_rc: Rc<str> = Rc::from(text);
    let break_width = width.max(1.0);
    // Tab advances resolve only after breaking, so a tab that pushes content
    // past the paragraph box needs its line re-broken with room reserved for
    // the widening. Reservations only ever tighten, so this settles.
    let mut line_caps: Vec<f32> = Vec::new();
    let mut out;
    let mut pass = 0usize;
    loop {
        if line_caps.is_empty() {
            layout.break_all_lines(Some(break_width));
        } else {
            break_lines_with_caps(&mut layout, break_width, &line_caps);
        }
        layout.align(align, Default::default());
        out = shape_extract_lines(&layout, text, &text_rc, ranges, links, dynamic_ranges, cx);
        if !adjust_tabs {
            break;
        }
        let reservations = if rtl_tabs {
            apply_rtl_tab_stops(
                text,
                &mut out,
                options.tab_stops,
                width,
                options.tab_origin,
                options.default_tab_stop_pt,
            )
        } else {
            apply_tab_stops(
                text,
                &mut out,
                options.tab_stops,
                width,
                options.tab_origin,
                options.default_tab_stop_pt,
            )
        };
        pass += 1;
        if pass > TAB_REFLOW_PASSES
            || !tighten_line_caps(&mut line_caps, &reservations, break_width)
        {
            break;
        }
    }
    out
}

pub(super) fn apply_line_spacing_hint(lines: &mut [LineLayout], hint: Option<LineSpacingHint>) {
    let Some(hint) = hint else {
        return;
    };
    let requested = match hint {
        LineSpacingHint::Exact(value) | LineSpacingHint::AtLeast(value) => value,
    };
    if !requested.is_finite() || requested <= 0.0 {
        return;
    }
    let requested = requested.min(MAX_ABSOLUTE_LINE_HEIGHT_PT);
    for line in lines {
        match hint {
            LineSpacingHint::AtLeast(_) if requested > line.height => {
                line.baseline += (requested - line.height) * 0.5;
                line.height = requested;
            }
            LineSpacingHint::AtLeast(_) => {}
            LineSpacingHint::Exact(_) => {
                line.clip_to_height = true;
                let content_bounds = line.runs.iter().fold(None, |bounds, run| {
                    let top = run.baseline_shift - run.ascent;
                    let bottom = run.baseline_shift + run.descent;
                    Some(match bounds {
                        Some((current_top, current_bottom)) => {
                            (f32::min(current_top, top), f32::max(current_bottom, bottom))
                        }
                        None => (top, bottom),
                    })
                });
                if let Some((top, bottom)) = content_bounds {
                    let content_height = bottom - top;
                    line.baseline = if content_height <= requested {
                        (requested - content_height) * 0.5 - top
                    } else {
                        requested - bottom
                    };
                } else {
                    line.baseline += (requested - line.height) * 0.5;
                }
                line.height = requested;
            }
        }
    }
}

/// Build the drawable lines for an already-broken layout.
fn shape_extract_lines(
    layout: &Layout<rgb::Color>,
    text: &str,
    text_rc: &Rc<str>,
    ranges: &[(usize, usize, CharProps)],
    links: &[(usize, usize, Rc<str>)],
    dynamic_ranges: &[(usize, usize, DynamicTextRun)],
    cx: &mut TextCx<'_>,
) -> Vec<LineLayout> {
    let mut out = Vec::new();
    for line in layout.lines() {
        let m = line.metrics();
        let mut baseline = m.ascent + m.leading * 0.5;
        let mut height = m.line_height;
        let mut line_start_byte = usize::MAX;
        let mut line_end_byte = 0usize;
        for run in line.runs() {
            for cluster in run.visual_clusters() {
                let range = cluster.text_range();
                line_start_byte = line_start_byte.min(range.start);
                line_end_byte = line_end_byte.max(range.end);
            }
        }
        let char_range = (line_start_byte != usize::MAX).then(|| LineCharRange {
            start: text[..line_start_byte].chars().count(),
            end: text[..line_end_byte].chars().count(),
        });
        let mut runs: Vec<RunDraw> = Vec::new();
        // `offset` is the line's alignment shift (0 for Start/left).
        let mut x_cursor = m.offset;
        for run in line.runs() {
            let run_x = x_cursor;
            let font = run.font().clone();
            let font_index = font.index;
            let (font_data, id) = font.data.into_raw_parts();
            let scene_font = SceneFontResource {
                bytes: font_data.clone(),
                source_id: id,
                index: font_index,
            };
            // A face parley can shape but krilla cannot ingest (bitmap/COLR/odd
            // index) makes `Font::new` return `None` — skip the run rather than
            // panic, honoring the crate's panic-free contract.
            if let std::collections::hash_map::Entry::Vacant(entry) = cx.font_cache.entry(id) {
                let Some(krilla_font) = Font::new(font_data.into(), font_index) else {
                    continue;
                };
                entry.insert(krilla_font);
            }
            let is_rtl = run.is_rtl();
            let font_size = run.font_size();
            let metrics = *run.metrics();
            let mut glyphs: Vec<KrillaGlyph> = Vec::new();
            // Paint and hyperlink can change within a single uniformly-shaped
            // Parley run, so accumulate glyphs into segments and flush each change.
            let mut seg_paint: Option<RunPaint> = None;
            let mut seg_link: Option<Rc<str>> = None;
            let mut seg_dynamic: Option<DynamicTextRun> = None;
            let mut seg_x = run_x;
            let mut clusters = run.visual_clusters().peekable();
            while let Some(cluster) = clusters.next() {
                // Continuations follow their glyph-bearing cluster in visual order;
                // RTL extends its start, and every glyph needs the complete range.
                let mut text_range = cluster.text_range();
                while let Some(component) = clusters.next_if(|next| next.is_ligature_continuation())
                {
                    let range = component.text_range();
                    text_range.start = text_range.start.min(range.start);
                    text_range.end = text_range.end.max(range.end);
                }
                let text_range = drawable_text_range(text, text_range);
                let paint = paint_at(ranges, cluster.text_range().start, font_size);
                let lk = link_at(links, cluster.text_range().start);
                let dynamic = dynamic_at(dynamic_ranges, cluster.text_range().start);
                if seg_paint.is_some()
                    && (seg_paint != Some(paint) || lk != seg_link || dynamic != seg_dynamic)
                    && !glyphs.is_empty()
                {
                    let previous = seg_paint.unwrap_or_else(default_run_paint);
                    runs.push(RunDraw {
                        x: seg_x,
                        glyphs: std::mem::take(&mut glyphs),
                        scene_font: scene_font.clone(),
                        size: font_size,
                        color: previous.color,
                        highlight: previous.highlight,
                        ascent: metrics.ascent,
                        descent: metrics.descent,
                        baseline_shift: previous.baseline_shift,
                        underline: previous.underline.then_some(TextDecoration {
                            offset: metrics.underline_offset,
                            thickness: metrics.underline_size.max(0.25),
                        }),
                        strikethrough: previous.strikethrough.then_some(TextDecoration {
                            offset: metrics.strikethrough_offset,
                            thickness: metrics.strikethrough_size.max(0.25),
                        }),
                        link: seg_link.clone(),
                        dynamic: seg_dynamic.clone(),
                        text: text_rc.clone(),
                        is_rtl,
                    });
                    seg_x = x_cursor;
                }
                seg_paint = Some(paint);
                seg_link = lk;
                seg_dynamic = dynamic;
                for glyph in cluster.glyphs() {
                    if !text_range.is_empty() {
                        glyphs.push(KrillaGlyph::new(
                            GlyphId::new(glyph.id),
                            glyph.advance / font_size,
                            glyph.x / font_size,
                            glyph.y / font_size,
                            0.0,
                            text_range.clone(),
                            None,
                        ));
                    }
                    x_cursor += glyph.advance;
                }
            }
            if !glyphs.is_empty() {
                let paint = seg_paint.unwrap_or_else(default_run_paint);
                runs.push(RunDraw {
                    x: seg_x,
                    glyphs,
                    scene_font,
                    size: font_size,
                    color: paint.color,
                    highlight: paint.highlight,
                    ascent: metrics.ascent,
                    descent: metrics.descent,
                    baseline_shift: paint.baseline_shift,
                    underline: paint.underline.then_some(TextDecoration {
                        offset: metrics.underline_offset,
                        thickness: metrics.underline_size.max(0.25),
                    }),
                    strikethrough: paint.strikethrough.then_some(TextDecoration {
                        offset: metrics.strikethrough_offset,
                        thickness: metrics.strikethrough_size.max(0.25),
                    }),
                    link: seg_link,
                    dynamic: seg_dynamic,
                    text: text_rc.clone(),
                    is_rtl,
                });
            }
        }
        let mut top = -baseline;
        let mut bottom = height - baseline;
        for run in runs.iter().filter(|run| run.baseline_shift != 0.0) {
            top = top.min(run.baseline_shift - run.ascent);
            bottom = bottom.max(run.baseline_shift + run.descent);
        }
        baseline = -top;
        height = bottom - top;
        out.push(LineLayout {
            height,
            baseline,
            clip_to_height: false,
            x_indent: 0.0,
            char_range,
            background: None,
            cell_spacing: CellLineSpacing::default(),
            cell_paragraph: None,
            cell_cant_split_group: None,
            cell_visual: None,
            leaders: Vec::new(),
            runs,
        });
    }
    out
}

/// How far a line's tab advances widened it, and how far past the paragraph
/// box the line ended up as a result.
#[derive(Clone, Copy, Default)]
struct TabReservation {
    shift: f32,
    overflow: f32,
}

/// Re-breaking passes allowed while reserving room for tab advances. Caps only
/// tighten, so a small bound keeps the result deterministic.
const TAB_REFLOW_PASSES: usize = 3;

/// Tighten the per-line breaking caps for lines whose tab advances pushed
/// content past the box. Returns whether any cap actually tightened.
fn tighten_line_caps(caps: &mut Vec<f32>, reservations: &[TabReservation], width: f32) -> bool {
    let mut changed = false;
    for (index, reservation) in reservations.iter().enumerate() {
        if reservation.overflow <= 0.5 || !reservation.shift.is_finite() || reservation.shift <= 0.0
        {
            continue;
        }
        let want = (width - reservation.shift).max(1.0);
        if caps.len() <= index {
            caps.resize(index + 1, width);
        }
        if want < caps[index] - 0.001 {
            caps[index] = want;
            changed = true;
        }
    }
    changed
}

/// Break a layout with a per-line maximum advance, so lines that must reserve
/// room for tab advances fit less content.
fn break_lines_with_caps(layout: &mut Layout<rgb::Color>, width: f32, caps: &[f32]) {
    let mut breaker = layout.break_lines();
    breaker.state_mut().set_layout_max_advance(width);
    let mut index = 0usize;
    loop {
        let cap = caps.get(index).copied().unwrap_or(width).clamp(1.0, width);
        breaker.state_mut().set_line_max_advance(cap);
        if breaker.break_next().is_none() {
            break;
        }
        index += 1;
    }
    breaker.finish();
}

#[derive(Clone, Copy, Default)]
struct TabFieldMetrics {
    advance: f32,
    decimal_offset: Option<f32>,
}

pub(super) fn glyph_text<'a>(text: &'a str, glyph: &KrillaGlyph) -> Option<&'a str> {
    text.get(glyph.text_range.clone())
}

fn tab_field_metrics(
    text: &str,
    line: &LineLayout,
    tab_run_index: usize,
    tab_glyph_index: usize,
) -> TabFieldMetrics {
    let mut metrics = TabFieldMetrics::default();
    let mut found_preferred_decimal = false;
    for (run_index, run) in line.runs.iter().enumerate().skip(tab_run_index) {
        let glyph_start = if run_index == tab_run_index {
            tab_glyph_index.saturating_add(1)
        } else {
            0
        };
        for glyph in run.glyphs.iter().skip(glyph_start) {
            let Some(glyph_text) = glyph_text(text, glyph) else {
                continue;
            };
            if glyph_text == "\t" {
                return metrics;
            }
            let contains_preferred_decimal =
                glyph_text.chars().any(|ch| matches!(ch, '.' | '\u{066B}'));
            let contains_fallback_decimal = glyph_text.contains(',');
            if contains_preferred_decimal && !found_preferred_decimal {
                metrics.decimal_offset =
                    Some((metrics.advance + glyph.x_offset * run.size).max(metrics.advance));
                found_preferred_decimal = true;
            } else if contains_fallback_decimal
                && !found_preferred_decimal
                && metrics.decimal_offset.is_none()
            {
                metrics.decimal_offset =
                    Some((metrics.advance + glyph.x_offset * run.size).max(metrics.advance));
            }
            let advance = glyph.x_advance * run.size;
            if advance.is_finite() {
                metrics.advance += advance.max(0.0);
            }
        }
    }
    metrics
}

fn explicit_tab_field_start(
    tab_stops: &[TabStop],
    cursor: f32,
    field: TabFieldMetrics,
    width: f32,
    origin: f32,
) -> Option<(TabStop, f32)> {
    let absolute_cursor = origin + cursor;
    let absolute_end = origin + width;
    if !absolute_cursor.is_finite() || !absolute_end.is_finite() {
        return None;
    }
    tab_stops
        .iter()
        .filter_map(|stop| {
            let alignment_offset = match stop.alignment {
                TabAlignment::Left => 0.0,
                TabAlignment::Center => field.advance / 2.0,
                TabAlignment::Right => field.advance,
                TabAlignment::Decimal => field.decimal_offset.unwrap_or(field.advance),
                TabAlignment::Bar => return None,
                TabAlignment::Clear => return None,
            };
            let absolute_field_start = stop.position_pt - alignment_offset;
            let absolute_field_end = absolute_field_start + field.advance;
            (stop.position_pt.is_finite()
                && stop.position_pt > absolute_cursor + f32::EPSILON
                && absolute_field_start >= absolute_cursor
                && absolute_field_end <= absolute_end)
                .then_some((stop.position_pt, *stop, absolute_field_start - origin))
        })
        .min_by(|left, right| left.0.total_cmp(&right.0))
        .map(|(_, stop, field_start)| (stop, field_start))
}

fn next_bar_tab_position(
    tab_stops: &[TabStop],
    cursor: f32,
    width: f32,
    origin: f32,
) -> Option<f32> {
    let absolute_cursor = origin + cursor;
    let absolute_end = origin + width;
    if !absolute_cursor.is_finite() || !absolute_end.is_finite() {
        return None;
    }
    tab_stops
        .iter()
        .filter(|stop| {
            stop.alignment == TabAlignment::Bar
                && stop.position_pt.is_finite()
                && stop.position_pt > absolute_cursor + f32::EPSILON
                && stop.position_pt <= absolute_end
        })
        .min_by(|left, right| left.position_pt.total_cmp(&right.position_pt))
        .map(|stop| stop.position_pt - origin)
}

#[cfg(test)]
pub(super) fn default_tab_field_start(cursor: f32, width: f32, origin: f32) -> f32 {
    default_tab_field_start_with_interval(cursor, width, origin, None)
}

fn default_tab_field_start_with_interval(
    cursor: f32,
    width: f32,
    origin: f32,
    default_tab_stop_pt: Option<f32>,
) -> f32 {
    let absolute_cursor = origin + cursor;
    let absolute_end = origin + width;
    if !absolute_cursor.is_finite() || !absolute_end.is_finite() {
        return cursor.max(0.0).min(width);
    }
    if absolute_cursor >= absolute_end {
        return width;
    }
    let interval = default_tab_stop_pt
        .filter(|value| value.is_finite() && *value > 0.0)
        .unwrap_or(DEFAULT_TAB_STOP_PT);
    let absolute_stop = (((absolute_cursor / interval).floor() + 1.0) * interval)
        .min(absolute_end)
        .max(absolute_cursor);
    (absolute_stop - origin).max(cursor).min(width)
}

fn apply_tab_stops(
    text: &str,
    lines: &mut [LineLayout],
    tab_stops: &[TabStop],
    width: f32,
    origin: f32,
    default_tab_stop_pt: Option<f32>,
) -> Vec<TabReservation> {
    let mut reservations = Vec::with_capacity(lines.len());
    for line in lines {
        line.leaders.clear();
        let mut accumulated_shift = 0.0;
        let mut line_end: f32 = 0.0;
        for run_index in 0..line.runs.len() {
            line.runs[run_index].x += accumulated_shift;
            let mut cursor = line.runs[run_index].x;
            for glyph_index in 0..line.runs[run_index].glyphs.len() {
                let run_size = line.runs[run_index].size;
                let glyph = &line.runs[run_index].glyphs[glyph_index];
                let original_advance = glyph.x_advance * run_size;
                if glyph_text(text, glyph) == Some("\t") && run_size > 0.0 {
                    let field = tab_field_metrics(text, line, run_index, glyph_index);
                    let explicit =
                        explicit_tab_field_start(tab_stops, cursor, field, width, origin);
                    let field_start =
                        explicit
                            .map(|(_, field_start)| field_start)
                            .unwrap_or_else(|| {
                                default_tab_field_start_with_interval(
                                    cursor,
                                    width,
                                    origin,
                                    default_tab_stop_pt,
                                )
                            });
                    if let Some((stop, _)) = explicit {
                        if stop.leader != TabLeader::None && field_start > cursor {
                            line.leaders.push(TabLeaderSpan {
                                start: cursor,
                                end: field_start,
                                style: stop.leader,
                                color: line.runs[run_index].color,
                            });
                        }
                    } else if let Some(bar) =
                        next_bar_tab_position(tab_stops, cursor, width, origin)
                    {
                        line.leaders.push(TabLeaderSpan {
                            start: bar,
                            end: bar,
                            style: TabLeader::Bar,
                            color: line.runs[run_index].color,
                        });
                    }
                    let advance = (field_start - cursor)
                        .max(0.0)
                        .min((width - cursor).max(0.0));
                    line.runs[run_index].glyphs[glyph_index].x_advance = advance / run_size;
                    accumulated_shift += advance - original_advance;
                    cursor += advance;
                } else {
                    cursor += original_advance;
                }
            }
            line_end = line_end.max(cursor);
        }
        reservations.push(TabReservation {
            shift: accumulated_shift,
            overflow: (line_end - width).max(0.0),
        });
    }
    reservations
}

fn rtl_tab_field_metrics(
    text: &str,
    line: &LineLayout,
    tab_run_index: usize,
    tab_glyph_index: usize,
) -> TabFieldMetrics {
    let mut metrics = TabFieldMetrics::default();
    let mut found_preferred_decimal = false;
    for run_index in (0..=tab_run_index).rev() {
        let glyph_end = if run_index == tab_run_index {
            tab_glyph_index
        } else {
            line.runs[run_index].glyphs.len()
        };
        for glyph in line.runs[run_index].glyphs[..glyph_end].iter().rev() {
            let Some(glyph_text) = glyph_text(text, glyph) else {
                continue;
            };
            if glyph_text == "\t" {
                return metrics;
            }
            let run_size = line.runs[run_index].size;
            let contains_preferred_decimal =
                glyph_text.chars().any(|ch| matches!(ch, '.' | '\u{066B}'));
            let contains_fallback_decimal = glyph_text.contains(',');
            if contains_preferred_decimal && !found_preferred_decimal {
                metrics.decimal_offset =
                    Some((metrics.advance + glyph.x_offset * run_size).max(metrics.advance));
                found_preferred_decimal = true;
            } else if contains_fallback_decimal
                && !found_preferred_decimal
                && metrics.decimal_offset.is_none()
            {
                metrics.decimal_offset =
                    Some((metrics.advance + glyph.x_offset * run_size).max(metrics.advance));
            }
            let glyph_advance = glyph.x_advance * run_size;
            if glyph_advance.is_finite() {
                metrics.advance += glyph_advance.max(0.0);
            }
        }
    }
    metrics
}

fn explicit_rtl_tab_field_start(
    tab_stops: &[TabStop],
    cursor: f32,
    field: TabFieldMetrics,
    width: f32,
    origin: f32,
) -> Option<(TabStop, f32)> {
    let absolute_cursor = origin + cursor;
    let absolute_end = origin + width;
    if !absolute_cursor.is_finite() || !absolute_end.is_finite() {
        return None;
    }
    tab_stops
        .iter()
        .filter_map(|stop| {
            let alignment_offset = match stop.alignment {
                TabAlignment::Left => 0.0,
                TabAlignment::Center => field.advance / 2.0,
                TabAlignment::Right => field.advance,
                TabAlignment::Decimal => field.decimal_offset.unwrap_or(field.advance),
                TabAlignment::Bar => return None,
                TabAlignment::Clear => return None,
            };
            let absolute_field_start = stop.position_pt - alignment_offset;
            let absolute_field_end = absolute_field_start + field.advance;
            (stop.position_pt.is_finite()
                && stop.position_pt > absolute_cursor + f32::EPSILON
                && absolute_field_start >= absolute_cursor
                && absolute_field_end <= absolute_end)
                .then_some((stop.position_pt, *stop, absolute_field_start - origin))
        })
        .min_by(|left, right| left.0.total_cmp(&right.0))
        .map(|(_, stop, field_start)| (stop, field_start))
}

fn apply_rtl_tab_stops(
    text: &str,
    lines: &mut [LineLayout],
    tab_stops: &[TabStop],
    width: f32,
    origin: f32,
    default_tab_stop_pt: Option<f32>,
) -> Vec<TabReservation> {
    let use_default_fallback = tab_stops.is_empty()
        || tab_stops.iter().all(|stop| {
            matches!(
                stop.alignment,
                TabAlignment::Left | TabAlignment::Bar | TabAlignment::Clear
            )
        });
    let mut reservations = Vec::with_capacity(lines.len());
    for line in lines {
        line.leaders.clear();
        let mut run_deltas = vec![0.0_f32; line.runs.len()];
        let mut shift_from_right = 0.0_f32;
        for run_index in (0..line.runs.len()).rev() {
            for glyph_index in (0..line.runs[run_index].glyphs.len()).rev() {
                let run_size = line.runs[run_index].size;
                if !run_size.is_finite()
                    || run_size <= 0.0
                    || glyph_text(text, &line.runs[run_index].glyphs[glyph_index]) != Some("\t")
                {
                    continue;
                }
                let tab_end = line.runs[run_index].x
                    + line.runs[run_index]
                        .glyphs
                        .iter()
                        .take(glyph_index + 1)
                        .map(|glyph| glyph.x_advance * run_size)
                        .sum::<f32>();
                if !tab_end.is_finite() || !shift_from_right.is_finite() {
                    continue;
                }
                let cursor = width - tab_end + shift_from_right;
                // ISO/IEC 29500-1 17.3.1.37 measures w:pos from the paragraph's
                // leading edge, which is the right edge for an RTL paragraph.
                let field = rtl_tab_field_metrics(text, line, run_index, glyph_index);
                let explicit =
                    explicit_rtl_tab_field_start(tab_stops, cursor, field, width, origin);
                let field_start = explicit.map(|(_, field_start)| field_start).or_else(|| {
                    use_default_fallback.then(|| {
                        default_tab_field_start_with_interval(
                            cursor,
                            width,
                            origin,
                            default_tab_stop_pt,
                        )
                    })
                });
                let Some(field_start) = field_start else {
                    continue;
                };
                if let Some((stop, _)) = explicit {
                    if stop.leader != TabLeader::None && field_start > cursor {
                        line.leaders.push(TabLeaderSpan {
                            start: cursor,
                            end: field_start,
                            style: stop.leader,
                            color: line.runs[run_index].color,
                        });
                    }
                } else if let Some(bar) = next_bar_tab_position(tab_stops, cursor, width, origin) {
                    line.leaders.push(TabLeaderSpan {
                        start: bar,
                        end: bar,
                        style: TabLeader::Bar,
                        color: line.runs[run_index].color,
                    });
                }
                let original_advance =
                    line.runs[run_index].glyphs[glyph_index].x_advance * run_size;
                let advance = (field_start - cursor)
                    .max(0.0)
                    .min((width - cursor).max(0.0));
                let delta = advance - original_advance;
                line.runs[run_index].glyphs[glyph_index].x_advance = advance / run_size;
                run_deltas[run_index] += delta;
                shift_from_right += delta;
            }
        }

        let mut run_shift = 0.0_f32;
        for run_index in (0..line.runs.len()).rev() {
            run_shift += run_deltas[run_index];
            line.runs[run_index].x -= run_shift;
        }
        let line_start = line.runs.iter().map(|run| run.x).fold(width, f32::min);
        reservations.push(TabReservation {
            shift: shift_from_right,
            overflow: (-line_start).max(0.0),
        });
    }
    reservations
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shape_bundled(text: &str, styled: StyledText<'_>, width: f32) -> Vec<LineLayout> {
        let fonts = [
            rwml_fonts::noto_sans_arabic_subset().to_vec(),
            rwml_fonts::noto_sans_hebrew_subset().to_vec(),
            rwml_fonts::noto_sans_kr_subset().to_vec(),
        ];
        let mut font_cx = strict_font_context(&fonts, true).unwrap();
        let mut layout_cx = LayoutContext::new();
        let mut font_cache = HashMap::new();
        shape(
            text,
            styled,
            None,
            Alignment::Start,
            width,
            &mut TextCx {
                font_cx: &mut font_cx,
                layout_cx: &mut layout_cx,
                font_cache: &mut font_cache,
            },
        )
    }

    #[test]
    fn ligature_ranges_cover_rtl_ltr_and_mixed_source_text() {
        for text in [
            "أولى",
            "لا",
            "لَا",
            "لَّا",
            "office e\u{301}",
            "אבג أولى 123",
            "أولى أولى أولى",
        ] {
            for width in [320.0, 32.0] {
                let props = [(0, text.len(), CharProps::default())];
                let lines = shape_bundled(text, StyledText::plain(&props), width);
                let mut ranges = lines
                    .iter()
                    .flat_map(|line| &line.runs)
                    .flat_map(|run| &run.glyphs)
                    .map(|glyph| {
                        assert_ne!(glyph.glyph_id.to_u32(), 0, "missing font glyph: {text}");
                        assert!(text.is_char_boundary(glyph.text_range.start));
                        assert!(text.is_char_boundary(glyph.text_range.end));
                        glyph.text_range.clone()
                    })
                    .collect::<Vec<_>>();
                ranges.sort_unstable_by_key(|range| (range.start, range.end));
                ranges.dedup();
                let mut cursor = 0;
                for range in &ranges {
                    assert_eq!(range.start, cursor, "{text:?} at {width}: {ranges:?}");
                    cursor = range.end;
                }
                assert_eq!(cursor, text.len(), "{text:?} at {width}: {ranges:?}");
            }
        }
    }

    #[test]
    fn ligature_ranges_cover_every_glyph_of_an_arabic_mark_cluster() {
        let text = "لَا";
        let props = [(0, text.len(), CharProps::default())];
        let lines = shape_bundled(text, StyledText::plain(&props), 320.0);
        let glyphs = lines
            .iter()
            .flat_map(|line| &line.runs)
            .flat_map(|run| &run.glyphs)
            .collect::<Vec<_>>();
        assert_eq!(glyphs.len(), 3, "alef plus the lam/vowel cluster");
        assert_eq!(glyphs[0].text_range, 4..6);
        assert_eq!(glyphs[1].text_range, 0..4);
        assert_eq!(glyphs[2].text_range, 0..4);
    }

    #[test]
    fn ligature_ranges_preserve_paint_and_link_ownership() {
        let text = "لَا";
        let props = [
            (0, 2, CharProps::default()),
            (
                2,
                4,
                CharProps {
                    color: Some(crate::model::Color { r: 255, g: 0, b: 0 }),
                    ..CharProps::default()
                },
            ),
            (4, text.len(), CharProps::default()),
        ];
        let link: Rc<str> = Rc::from("https://example.com/ligature");
        let links = [(2, 4, link.clone())];
        let lines = shape_bundled(
            text,
            StyledText {
                ranges: &props,
                links: &links,
                dynamic_ranges: &[],
            },
            320.0,
        );
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].runs.len(), 2);
        let ligature = &lines[0].runs[1];
        assert!(ligature.is_rtl);
        assert_eq!(ligature.color, rgb::Color::new(255, 0, 0));
        assert_eq!(ligature.link.as_ref(), Some(&link));
        assert_eq!(ligature.glyphs.len(), 2);
        assert_eq!(ligature.glyphs[0].text_range, 0..4);
        assert_eq!(ligature.glyphs[1].text_range, 0..4);
        let preceding = &lines[0].runs[0];
        assert_eq!(preceding.color, rgb::Color::new(0, 0, 0));
        assert!(preceding.link.is_none());
        assert_eq!(preceding.glyphs.len(), 1);
        assert_eq!(preceding.glyphs[0].text_range, 4..6);
        assert!(
            (ligature.x - preceding.x - preceding.glyphs[0].x_advance * preceding.size).abs()
                < 0.001
        );
    }
}
