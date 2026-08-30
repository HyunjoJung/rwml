//! Resumable flow-fragment prototypes.

use std::rc::Rc;

use super::*;

#[derive(Clone, Debug, PartialEq, Eq)]
struct ParagraphFragmentCursor {
    source_char: usize,
    marker_emitted: bool,
    page_field_indices: Rc<[Option<usize>]>,
}

impl Default for ParagraphFragmentCursor {
    fn default() -> Self {
        Self {
            source_char: 0,
            marker_emitted: false,
            page_field_indices: Rc::from(Vec::<Option<usize>>::new()),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct FragmentTrack {
    width: f32,
    height: f32,
}

struct ParagraphFragment {
    lines: Vec<LineLayout>,
    next: Option<ParagraphFragmentCursor>,
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
fn shape_paragraph_fragment(
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
    let total_chars = paragraph_source_chars(paragraph);
    let source_start = cursor.source_char.min(total_chars);
    if source_start == total_chars {
        return ParagraphFragment {
            lines: Vec::new(),
            next: None,
        };
    }

    let page_field_indices = if cursor.page_field_indices.len() == paragraph.runs.len() {
        cursor.page_field_indices.clone()
    } else {
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
    };
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
        return ParagraphFragment { lines, next: None };
    }
    let next = (advanced_to < total_chars).then_some(ParagraphFragmentCursor {
        source_char: advanced_to,
        marker_emitted: cursor.marker_emitted || fragment_marker.is_some(),
        page_field_indices,
    });
    ParagraphFragment { lines, next }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use parley::fontique::{Blob, Collection, CollectionOptions, SourceCache};
    use parley::{FontContext, LayoutContext};

    use super::*;
    use crate::model::Indent;

    type RunSnapshot = (u32, u32, usize, Option<String>);
    type LineSnapshot = (Option<(usize, usize)>, u32, Vec<RunSnapshot>);

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

    fn line_snapshot(lines: &[LineLayout]) -> Vec<LineSnapshot> {
        lines
            .iter()
            .map(|line| {
                (
                    line.char_range.map(|range| (range.start, range.end)),
                    line.height.to_bits(),
                    line.runs
                        .iter()
                        .map(|run| {
                            (
                                run.x.to_bits(),
                                run.size.to_bits(),
                                run.glyphs.len(),
                                run.link.as_deref().map(str::to_owned),
                            )
                        })
                        .collect(),
                )
            })
            .collect()
    }

    fn unequal_track_fragments() -> (Paragraph, ParagraphFragment, ParagraphFragment) {
        let paragraph = Paragraph {
            props: ParaProps {
                list: Some(ListInfo {
                    level: 0,
                    ordered: true,
                    label: "7.".to_string(),
                }),
                ..ParaProps::default()
            },
            runs: vec![
                Run {
                    text: "alpha beta ".to_string(),
                    props: CharProps {
                        bold: true,
                        ..CharProps::default()
                    },
                    ..Run::default()
                },
                Run {
                    text: "gamma delta epsilon zeta eta theta iota kappa lambda mu nu xi omicron pi rho sigma"
                        .to_string(),
                    props: CharProps {
                        color: Some(Color::rgb(20, 80, 160)),
                        italic: true,
                        ..CharProps::default()
                    },
                    field: FieldRole::Hyperlink {
                        url: "https://example.invalid/fragment".to_string(),
                    },
                    ..Run::default()
                },
                Run {
                    text: " tau upsilon phi chi psi omega".to_string(),
                    ..Run::default()
                },
            ],
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
        let first = shape_paragraph_fragment(
            &paragraph,
            Some("7."),
            &[],
            Some(DEFAULT_TAB_STOP_PT),
            None,
            FragmentTrack {
                width: 82.0,
                height: 31.0,
            },
            ParagraphFragmentCursor::default(),
            &mut text_cx,
            &mut capture,
        );
        let second = shape_paragraph_fragment(
            &paragraph,
            Some("7."),
            &[],
            Some(DEFAULT_TAB_STOP_PT),
            None,
            FragmentTrack {
                width: 180.0,
                height: 1_000.0,
            },
            first
                .next
                .clone()
                .expect("the narrow track must leave a continuation"),
            &mut text_cx,
            &mut capture,
        );
        (paragraph, first, second)
    }

    #[test]
    fn paragraph_cursor_resumes_styled_linked_list_text_across_unequal_tracks() {
        let (paragraph, first, second) = unequal_track_fragments();
        let total_chars = paragraph.text().chars().count();
        let continuation = first.next.clone().expect("first fragment continues");

        assert_eq!(first.lines.len(), 2);
        assert_eq!(first.lines[0].char_range.map(|range| range.start), Some(0));
        assert_eq!(
            first
                .lines
                .last()
                .and_then(|line| line.char_range)
                .map(|range| range.end),
            Some(continuation.source_char)
        );
        assert_eq!(
            second
                .lines
                .first()
                .and_then(|line| line.char_range)
                .map(|range| range.start),
            Some(continuation.source_char)
        );
        assert_eq!(
            second
                .lines
                .last()
                .and_then(|line| line.char_range)
                .map(|range| range.end),
            Some(total_chars)
        );
        assert!(second.next.is_none());
        assert!(first
            .lines
            .iter()
            .flat_map(|line| &line.runs)
            .any(|run| run.text.contains("7. ")));
        assert!(second
            .lines
            .iter()
            .flat_map(|line| &line.runs)
            .all(|run| !run.text.contains("7. ")));
        assert!(first
            .lines
            .iter()
            .chain(&second.lines)
            .flat_map(|line| &line.runs)
            .filter_map(|run| run.link.as_deref())
            .all(|url| url == "https://example.invalid/fragment"));
        assert!(first
            .lines
            .iter()
            .flat_map(|line| &line.runs)
            .any(|run| run.link.is_some()));
        assert!(second
            .lines
            .iter()
            .flat_map(|line| &line.runs)
            .any(|run| run.link.is_some()));

        let (_, repeated_first, repeated_second) = unequal_track_fragments();
        assert_eq!(
            line_snapshot(&first.lines),
            line_snapshot(&repeated_first.lines)
        );
        assert_eq!(
            line_snapshot(&second.lines),
            line_snapshot(&repeated_second.lines)
        );
        assert_eq!(first.next, repeated_first.next);
        assert_eq!(second.next, repeated_second.next);
    }

    #[test]
    fn paragraph_cursor_uses_continuation_indent_after_fragment_boundary() {
        for (indent, expected_origin) in [
            (
                Indent {
                    left_pt: Some(12.0),
                    first_line_pt: Some(18.0),
                    ..Indent::default()
                },
                12.0,
            ),
            (
                Indent {
                    left_pt: Some(30.0),
                    hanging_pt: Some(18.0),
                    ..Indent::default()
                },
                30.0,
            ),
        ] {
            let paragraph = Paragraph {
                props: ParaProps {
                    indent,
                    ..ParaProps::default()
                },
                runs: vec![Run {
                    text: "alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu"
                        .to_string(),
                    ..Run::default()
                }],
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
            let first = shape_paragraph_fragment(
                &paragraph,
                None,
                &[],
                Some(DEFAULT_TAB_STOP_PT),
                None,
                FragmentTrack {
                    width: 120.0,
                    height: 1.0,
                },
                ParagraphFragmentCursor::default(),
                &mut text_cx,
                &mut capture,
            );
            let second = shape_paragraph_fragment(
                &paragraph,
                None,
                &[],
                Some(DEFAULT_TAB_STOP_PT),
                None,
                FragmentTrack {
                    width: 120.0,
                    height: 1_000.0,
                },
                first
                    .next
                    .expect("the first line must leave a continuation"),
                &mut text_cx,
                &mut capture,
            );
            let first_continuation_line = second.lines.first().expect("continued line");
            let origin = first_continuation_line.x_indent
                + first_continuation_line
                    .runs
                    .first()
                    .map_or(0.0, |run| run.x);

            assert!(
                (origin - expected_origin).abs() < 0.1,
                "expected continuation origin {expected_origin}, got {origin}"
            );
        }
    }

    #[test]
    fn paragraph_cursor_guarantees_progress_for_an_oversized_cluster() {
        let paragraph = Paragraph {
            runs: vec![Run {
                text: "👩‍💻👩‍💻👩‍💻".to_string(),
                ..Run::default()
            }],
            ..Paragraph::default()
        };
        let total_chars = paragraph.text().chars().count();
        let mut font_cx = strict_font_context(rwml_fonts::noto_sans_kr_subset().to_vec());
        let mut layout_cx: LayoutContext<rgb::Color> = LayoutContext::new();
        let mut font_cache = HashMap::new();
        let mut text_cx = TextCx {
            font_cx: &mut font_cx,
            layout_cx: &mut layout_cx,
            font_cache: &mut font_cache,
        };
        let mut capture = LayoutCapture::default();
        let mut cursor = ParagraphFragmentCursor::default();
        let mut passes = 0usize;

        loop {
            let fragment = shape_paragraph_fragment(
                &paragraph,
                None,
                &[],
                Some(DEFAULT_TAB_STOP_PT),
                None,
                FragmentTrack {
                    width: f32::NAN,
                    height: 0.0,
                },
                cursor.clone(),
                &mut text_cx,
                &mut capture,
            );
            assert!(!fragment.lines.is_empty());
            passes += 1;
            assert!(passes <= total_chars.max(1));
            let Some(next) = fragment.next else {
                assert_eq!(
                    fragment
                        .lines
                        .last()
                        .and_then(|line| line.char_range)
                        .map(|range| range.end),
                    Some(total_chars)
                );
                break;
            };
            assert!(next.source_char > cursor.source_char);
            cursor = next;
        }
    }

    #[test]
    fn paragraph_cursor_reuses_page_field_identity_after_reshaping() {
        let paragraph = Paragraph {
            runs: vec![
                Run {
                    text: "hidden page".to_string(),
                    props: CharProps {
                        hidden: true,
                        ..CharProps::default()
                    },
                    field: FieldRole::Simple {
                        instruction: "PAGE".to_string(),
                    },
                    ..Run::default()
                },
                Run {
                    text: std::iter::repeat_n("alpha beta gamma delta", 8)
                        .collect::<Vec<_>>()
                        .join(" "),
                    ..Run::default()
                },
                Run {
                    text: "9".to_string(),
                    field: FieldRole::Simple {
                        instruction: "PAGE".to_string(),
                    },
                    ..Run::default()
                },
            ],
            ..Paragraph::default()
        };
        let mut font_cx = strict_font_context(rwml_fonts::noto_sans_kr_subset().to_vec());
        let mut layout_cx: LayoutContext<rgb::Color> = LayoutContext::new();
        let mut font_cache = HashMap::new();
        let mut text_cx = TextCx {
            font_cx: &mut font_cx,
            layout_cx: &mut layout_cx,
            font_cache: &mut font_cache,
        };
        let mut capture = LayoutCapture::page_fields();
        let first = shape_paragraph_fragment(
            &paragraph,
            None,
            &[],
            Some(DEFAULT_TAB_STOP_PT),
            None,
            FragmentTrack {
                width: 80.0,
                height: 1.0,
            },
            ParagraphFragmentCursor::default(),
            &mut text_cx,
            &mut capture,
        );
        let second = shape_paragraph_fragment(
            &paragraph,
            None,
            &[],
            Some(DEFAULT_TAB_STOP_PT),
            None,
            FragmentTrack {
                width: 180.0,
                height: 1_000.0,
            },
            first.next.expect("plain prefix leaves the PAGE field"),
            &mut text_cx,
            &mut capture,
        );
        let dynamic_indices = second
            .lines
            .iter()
            .flat_map(|line| &line.runs)
            .filter_map(|run| run.dynamic.as_ref())
            .map(|dynamic| dynamic.page_field_index)
            .collect::<Vec<_>>();

        assert_eq!(capture.page_fields.len(), 1);
        assert!(!dynamic_indices.is_empty());
        assert!(dynamic_indices.iter().all(|index| *index == Some(0)));
    }
}
