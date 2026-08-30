//! Resumable flow-fragment prototypes.

use super::paragraph_flow::{
    shape_paragraph_fragment, shape_paragraph_fragment_with_pagination, FragmentTrack,
    ParagraphFragment, ParagraphFragmentCursor,
};
use super::*;

const MAX_WIDOW_TRACK_PROBES: usize = 4;

#[derive(Clone, Copy, Debug, PartialEq)]
struct FragmentTrackSlot {
    page_index: usize,
    column_index: usize,
    x: f32,
    fresh: bool,
    track: FragmentTrack,
}

struct SlottedParagraphFragment {
    slot: FragmentTrackSlot,
    fragment: ParagraphFragment,
}

struct ParagraphTrackFragments {
    fragments: Vec<SlottedParagraphFragment>,
    next: Option<ParagraphFragmentCursor>,
}

fn column_fragment_tracks(
    columns: ColumnLayout,
    page_index: usize,
    height: f32,
    rtl: bool,
) -> Vec<FragmentTrackSlot> {
    (0..columns.count)
        .map(|offset| {
            let column_index = if rtl {
                columns.count - 1 - offset
            } else {
                offset
            };
            FragmentTrackSlot {
                page_index,
                column_index,
                x: columns.x(column_index),
                fresh: true,
                track: FragmentTrack {
                    width: columns.width(column_index),
                    height,
                },
            }
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn shape_paragraph_across_tracks(
    paragraph: &Paragraph,
    marker: Option<&str>,
    tab_stops: &[TabStop],
    default_tab_stop_pt: Option<f32>,
    line_spacing_hint: Option<LineSpacingHint>,
    tracks: &[FragmentTrackSlot],
    cursor: ParagraphFragmentCursor,
    cx: &mut TextCx<'_>,
    capture: &mut LayoutCapture,
) -> ParagraphTrackFragments {
    shape_paragraph_across_tracks_with_pagination(
        paragraph,
        marker,
        tab_stops,
        default_tab_stop_pt,
        line_spacing_hint,
        PaginationHint::default(),
        tracks,
        cursor,
        cx,
        capture,
    )
}

#[allow(clippy::too_many_arguments)]
fn shape_paragraph_across_tracks_with_pagination(
    paragraph: &Paragraph,
    marker: Option<&str>,
    tab_stops: &[TabStop],
    default_tab_stop_pt: Option<f32>,
    line_spacing_hint: Option<LineSpacingHint>,
    pagination: PaginationHint,
    tracks: &[FragmentTrackSlot],
    cursor: ParagraphFragmentCursor,
    cx: &mut TextCx<'_>,
    capture: &mut LayoutCapture,
) -> ParagraphTrackFragments {
    // keep_next remains coordinator-owned because it requires the following block.
    let start = if pagination.keep_lines && cursor.source_char == 0 {
        let fitting = tracks.iter().enumerate().find_map(|(index, slot)| {
            let mut scratch_capture = LayoutCapture::default();
            let candidate = shape_paragraph_fragment(
                paragraph,
                marker,
                tab_stops,
                default_tab_stop_pt,
                line_spacing_hint,
                slot.track,
                cursor.clone(),
                cx,
                &mut scratch_capture,
            );
            candidate.next.is_none().then_some(index)
        });
        fitting.unwrap_or_else(|| tracks.iter().position(|slot| slot.fresh).unwrap_or(0))
    } else {
        0
    };

    let mut fragments = Vec::new();
    let mut next = Some(cursor);
    for (slot_index, slot) in tracks.iter().enumerate().skip(start) {
        let Some(cursor) = next.take() else {
            break;
        };
        let source_before = cursor.source_char;
        let marker_before = cursor.marker_emitted;
        let mut fragment = shape_paragraph_fragment_with_pagination(
            paragraph,
            marker,
            tab_stops,
            default_tab_stop_pt,
            line_spacing_hint,
            slot.track,
            pagination,
            slot.fresh,
            cursor,
            cx,
            capture,
        );
        if pagination.widow_control && !fragment.deferred {
            if let Some(next_slot) = tracks.get(slot_index + 1) {
                let mut probes = 0usize;
                while let Some(continuation) = fragment.next.clone() {
                    if probes >= MAX_WIDOW_TRACK_PROBES {
                        if !slot.fresh {
                            fragment.lines.clear();
                            fragment.images.clear();
                            fragment.next = Some(ParagraphFragmentCursor {
                                source_char: source_before,
                                marker_emitted: marker_before,
                                ..continuation
                            });
                            fragment.deferred = true;
                        }
                        break;
                    }
                    probes += 1;
                    let mut scratch_capture = LayoutCapture::default();
                    let probe = shape_paragraph_fragment(
                        paragraph,
                        marker,
                        tab_stops,
                        default_tab_stop_pt,
                        line_spacing_hint,
                        next_slot.track,
                        continuation.clone(),
                        cx,
                        &mut scratch_capture,
                    );
                    if probe.next.is_some() || probe.lines.len() != 1 {
                        break;
                    }
                    if fragment.lines.len() > 2 {
                        fragment.lines.pop();
                        let Some(source_char) = fragment
                            .lines
                            .iter()
                            .filter_map(|line| line.char_range.map(|range| range.end))
                            .max()
                            .filter(|source_char| *source_char > source_before)
                        else {
                            break;
                        };
                        fragment.next = Some(ParagraphFragmentCursor {
                            source_char,
                            ..continuation
                        });
                        continue;
                    }
                    if !slot.fresh {
                        fragment.lines.clear();
                        fragment.images.clear();
                        fragment.next = Some(ParagraphFragmentCursor {
                            source_char: source_before,
                            marker_emitted: marker_before,
                            ..continuation
                        });
                        fragment.deferred = true;
                    }
                    break;
                }
            }
        }
        next = fragment.next.clone();
        if fragment.deferred {
            continue;
        }
        fragments.push(SlottedParagraphFragment {
            slot: *slot,
            fragment,
        });
    }
    ParagraphTrackFragments { fragments, next }
}

fn record_fragment_page_fields(
    fragments: &[SlottedParagraphFragment],
    page_fields: &mut [Option<usize>],
) {
    for placed in fragments {
        let page_number = placed.slot.page_index.saturating_add(1);
        for line in &placed.fragment.lines {
            record_line_page_fields(line, page_number, page_fields);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeSet, HashMap};

    use parley::fontique::{Blob, Collection, CollectionOptions, SourceCache};
    use parley::{FontContext, LayoutContext};

    use super::*;
    use crate::model::{Indent, SectionColumnHint};

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

    fn constrained_track_fragments(
        paragraph: &Paragraph,
        tracks: &[FragmentTrackSlot],
        pagination: PaginationHint,
    ) -> ParagraphTrackFragments {
        let mut font_cx = strict_font_context(rwml_fonts::noto_sans_kr_subset().to_vec());
        let mut layout_cx: LayoutContext<rgb::Color> = LayoutContext::new();
        let mut font_cache = HashMap::new();
        let mut text_cx = TextCx {
            font_cx: &mut font_cx,
            layout_cx: &mut layout_cx,
            font_cache: &mut font_cache,
        };
        let mut capture = LayoutCapture::default();
        shape_paragraph_across_tracks_with_pagination(
            paragraph,
            None,
            &[],
            Some(DEFAULT_TAB_STOP_PT),
            Some(LineSpacingHint::Exact(10.0)),
            pagination,
            tracks,
            ParagraphFragmentCursor::default(),
            &mut text_cx,
            &mut capture,
        )
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

    #[test]
    fn paragraph_cursor_emits_visible_inline_media_once_after_text() {
        let visible = Image {
            alt: Some("visible-before-cursor".to_string()),
            ..Image::default()
        };
        let paragraph = Paragraph {
            runs: vec![
                Run {
                    text: "lead ".to_string(),
                    ..Run::default()
                },
                Run {
                    image: Some(visible.clone()),
                    ..Run::default()
                },
                Run {
                    image: Some(Image {
                        alt: Some("hidden".to_string()),
                        ..Image::default()
                    }),
                    props: CharProps {
                        hidden: true,
                        ..CharProps::default()
                    },
                    ..Run::default()
                },
                Run {
                    text: std::iter::repeat_n("alpha beta gamma delta", 8)
                        .collect::<Vec<_>>()
                        .join(" "),
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
        let mut capture = LayoutCapture::default();
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
            first.next.clone().expect("text must continue"),
            &mut text_cx,
            &mut capture,
        );

        assert!(first.images.is_empty());
        assert_eq!(second.images, vec![visible.clone()]);
        assert!(second.next.is_none());

        let media_only = Paragraph {
            runs: vec![Run {
                image: Some(visible.clone()),
                ..Run::default()
            }],
            ..Paragraph::default()
        };
        let media_fragment = shape_paragraph_fragment(
            &media_only,
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
        assert!(media_fragment.lines.is_empty());
        assert_eq!(media_fragment.images, vec![visible]);
        assert!(media_fragment.next.is_none());
    }

    #[test]
    fn paragraph_track_driver_crosses_page_then_column_with_tabs() {
        let geom = Geom::from_setup(&PageSetup {
            width_pt: 220.0,
            height_pt: 100.0,
            margin_pt: 10.0,
            ..PageSetup::default()
        });
        let layout = SectionColumnLayoutHints {
            columns: vec![
                SectionColumnHint {
                    width_pt: 70.0,
                    space_after_pt: 20.0,
                },
                SectionColumnHint {
                    width_pt: 110.0,
                    space_after_pt: 0.0,
                },
            ],
        };
        let columns = ColumnLayout::new_with_layout(geom, Some(2), None, Some(&layout));
        let page_zero = column_fragment_tracks(columns, 0, 1.0, true);
        let page_one = column_fragment_tracks(columns, 1, 1.0, true);

        assert_eq!(
            page_zero
                .iter()
                .map(|slot| (slot.page_index, slot.column_index, slot.x, slot.track.width))
                .collect::<Vec<_>>(),
            vec![(0, 1, 90.0, 110.0), (0, 0, 0.0, 70.0)]
        );
        assert_eq!(
            page_one
                .iter()
                .map(|slot| (slot.page_index, slot.column_index, slot.x, slot.track.width))
                .collect::<Vec<_>>(),
            vec![(1, 1, 90.0, 110.0), (1, 0, 0.0, 70.0)]
        );

        let mut tracks = vec![page_zero[1], page_one[0], page_one[1]];
        tracks[2].track.height = 1_000.0;
        let paragraph = Paragraph {
            props: ParaProps {
                indent: Indent {
                    left_pt: Some(5.0),
                    ..Indent::default()
                },
                list: Some(ListInfo {
                    level: 0,
                    ordered: true,
                    label: "4.".to_string(),
                }),
                ..ParaProps::default()
            },
            runs: vec![Run {
                text: "alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu nu xi omicron\nA\tB"
                    .to_string(),
                ..Run::default()
            }],
        };
        let tab_stops = [TabStop {
            position_pt: 45.0,
            alignment: TabAlignment::Left,
            leader: TabLeader::Dot,
        }];
        let mut font_cx = strict_font_context(rwml_fonts::noto_sans_kr_subset().to_vec());
        let mut layout_cx: LayoutContext<rgb::Color> = LayoutContext::new();
        let mut font_cache = HashMap::new();
        let mut text_cx = TextCx {
            font_cx: &mut font_cx,
            layout_cx: &mut layout_cx,
            font_cache: &mut font_cache,
        };
        let mut capture = LayoutCapture::default();
        let result = shape_paragraph_across_tracks(
            &paragraph,
            Some("4."),
            &tab_stops,
            Some(DEFAULT_TAB_STOP_PT),
            None,
            &tracks,
            ParagraphFragmentCursor::default(),
            &mut text_cx,
            &mut capture,
        );

        assert!(result.next.is_none());
        assert_eq!(result.fragments.len(), 3);
        assert_eq!(
            result
                .fragments
                .iter()
                .map(|placed| (placed.slot.page_index, placed.slot.column_index))
                .collect::<Vec<_>>(),
            vec![(0, 0), (1, 1), (1, 0)]
        );
        let ranges = result
            .fragments
            .iter()
            .map(|placed| {
                let first = placed
                    .fragment
                    .lines
                    .first()
                    .and_then(|line| line.char_range)
                    .expect("fragment start");
                let last = placed
                    .fragment
                    .lines
                    .last()
                    .and_then(|line| line.char_range)
                    .expect("fragment end");
                (first.start, last.end)
            })
            .collect::<Vec<_>>();
        assert_eq!(ranges[0].0, 0);
        assert_eq!(ranges[0].1, ranges[1].0);
        assert_eq!(ranges[1].1, ranges[2].0);
        assert_eq!(ranges[2].1, paragraph.text().chars().count());

        assert!(result.fragments[0]
            .fragment
            .lines
            .iter()
            .flat_map(|line| &line.runs)
            .any(|run| run.text.contains("4. ")));
        assert!(result.fragments[1..]
            .iter()
            .flat_map(|placed| &placed.fragment.lines)
            .flat_map(|line| &line.runs)
            .all(|run| !run.text.contains("4. ")));
        let tab_line = result.fragments[1..]
            .iter()
            .flat_map(|placed| &placed.fragment.lines)
            .find(|line| !line.leaders.is_empty())
            .expect("continued tab line");
        assert_eq!(tab_line.leaders.len(), 1);
        assert_eq!(tab_line.leaders[0].style, TabLeader::Dot);
        assert!((tab_line.x_indent + tab_line.leaders[0].end - 45.0).abs() <= 0.1);
    }

    #[test]
    fn paragraph_track_driver_projects_links_and_page_fields_by_physical_page() {
        let paragraph = Paragraph {
            runs: vec![
                Run {
                    text: std::iter::repeat_n("linked alpha beta gamma", 8)
                        .collect::<Vec<_>>()
                        .join(" "),
                    field: FieldRole::Hyperlink {
                        url: "https://example.invalid/across-pages".to_string(),
                    },
                    ..Run::default()
                },
                Run {
                    text: " page field ".to_string(),
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
        let tracks = [
            FragmentTrackSlot {
                page_index: 0,
                column_index: 0,
                x: 0.0,
                fresh: true,
                track: FragmentTrack {
                    width: 80.0,
                    height: 1.0,
                },
            },
            FragmentTrackSlot {
                page_index: 1,
                column_index: 0,
                x: 0.0,
                fresh: true,
                track: FragmentTrack {
                    width: 150.0,
                    height: 1_000.0,
                },
            },
        ];
        let mut font_cx = strict_font_context(rwml_fonts::noto_sans_kr_subset().to_vec());
        let mut layout_cx: LayoutContext<rgb::Color> = LayoutContext::new();
        let mut font_cache = HashMap::new();
        let mut text_cx = TextCx {
            font_cx: &mut font_cx,
            layout_cx: &mut layout_cx,
            font_cache: &mut font_cache,
        };
        let mut capture = LayoutCapture::page_fields();
        let result = shape_paragraph_across_tracks(
            &paragraph,
            None,
            &[],
            Some(DEFAULT_TAB_STOP_PT),
            None,
            &tracks,
            ParagraphFragmentCursor::default(),
            &mut text_cx,
            &mut capture,
        );

        assert!(result.next.is_none());
        assert_eq!(result.fragments.len(), 2);
        let linked_pages = result
            .fragments
            .iter()
            .filter(|placed| {
                placed
                    .fragment
                    .lines
                    .iter()
                    .flat_map(|line| &line.runs)
                    .any(|run| run.link.as_deref() == Some("https://example.invalid/across-pages"))
            })
            .map(|placed| placed.slot.page_index)
            .collect::<BTreeSet<_>>();
        assert_eq!(linked_pages, BTreeSet::from([0, 1]));

        let dynamic_pages = result
            .fragments
            .iter()
            .flat_map(|placed| {
                placed
                    .fragment
                    .lines
                    .iter()
                    .flat_map(|line| &line.runs)
                    .filter_map(|run| run.dynamic.as_ref())
                    .map(|dynamic| (placed.slot.page_index, dynamic.page_field_index))
            })
            .collect::<Vec<_>>();
        assert_eq!(dynamic_pages, vec![(1, Some(0))]);
        assert_eq!(capture.page_fields, vec![None]);

        let mut projected_page_fields = capture.page_fields.clone();
        record_fragment_page_fields(&result.fragments, &mut projected_page_fields);
        assert_eq!(projected_page_fields, vec![Some(2)]);
    }

    #[test]
    fn paragraph_track_driver_moves_keep_lines_to_a_fresh_track() {
        let paragraph = Paragraph {
            runs: vec![Run {
                text: "one\ntwo\nthree".to_string(),
                ..Run::default()
            }],
            ..Paragraph::default()
        };
        let tracks = [
            FragmentTrackSlot {
                page_index: 0,
                column_index: 0,
                x: 0.0,
                fresh: false,
                track: FragmentTrack {
                    width: 180.0,
                    height: 20.0,
                },
            },
            FragmentTrackSlot {
                page_index: 1,
                column_index: 0,
                x: 0.0,
                fresh: true,
                track: FragmentTrack {
                    width: 180.0,
                    height: 31.0,
                },
            },
        ];
        let result = constrained_track_fragments(
            &paragraph,
            &tracks,
            PaginationHint {
                keep_lines: true,
                ..PaginationHint::default()
            },
        );

        assert!(result.next.is_none());
        assert_eq!(result.fragments.len(), 1);
        assert_eq!(result.fragments[0].slot.page_index, 1);
        assert!(result.fragments[0].slot.fresh);
        assert_eq!(result.fragments[0].fragment.lines.len(), 3);
    }

    #[test]
    fn paragraph_track_driver_avoids_a_three_plus_one_widow_split() {
        let paragraph = Paragraph {
            runs: vec![Run {
                text: "one\ntwo\nthree\nfour".to_string(),
                ..Run::default()
            }],
            ..Paragraph::default()
        };
        let tracks = [
            FragmentTrackSlot {
                page_index: 0,
                column_index: 0,
                x: 0.0,
                fresh: false,
                track: FragmentTrack {
                    width: 180.0,
                    height: 31.0,
                },
            },
            FragmentTrackSlot {
                page_index: 1,
                column_index: 0,
                x: 0.0,
                fresh: true,
                track: FragmentTrack {
                    width: 180.0,
                    height: 100.0,
                },
            },
        ];
        let result = constrained_track_fragments(
            &paragraph,
            &tracks,
            PaginationHint {
                widow_control: true,
                ..PaginationHint::default()
            },
        );

        assert!(result.next.is_none());
        assert_eq!(result.fragments.len(), 2);
        assert_eq!(
            result
                .fragments
                .iter()
                .map(|placed| placed.fragment.lines.len())
                .collect::<Vec<_>>(),
            vec![2, 2]
        );
        let first_end = result.fragments[0]
            .fragment
            .lines
            .last()
            .and_then(|line| line.char_range)
            .map(|range| range.end)
            .expect("first fragment source end");
        let second_start = result.fragments[1]
            .fragment
            .lines
            .first()
            .and_then(|line| line.char_range)
            .map(|range| range.start)
            .expect("second fragment source start");
        assert_eq!(first_end, second_start);
    }

    #[test]
    fn paragraph_track_driver_defers_a_single_orphan_line_from_a_partial_track() {
        let paragraph = Paragraph {
            runs: vec![Run {
                text: "one\ntwo\nthree\nfour".to_string(),
                ..Run::default()
            }],
            ..Paragraph::default()
        };
        let tracks = [
            FragmentTrackSlot {
                page_index: 0,
                column_index: 0,
                x: 0.0,
                fresh: false,
                track: FragmentTrack {
                    width: 180.0,
                    height: 11.0,
                },
            },
            FragmentTrackSlot {
                page_index: 1,
                column_index: 0,
                x: 0.0,
                fresh: true,
                track: FragmentTrack {
                    width: 180.0,
                    height: 100.0,
                },
            },
        ];
        let result = constrained_track_fragments(
            &paragraph,
            &tracks,
            PaginationHint {
                widow_control: true,
                ..PaginationHint::default()
            },
        );

        assert!(result.next.is_none());
        assert_eq!(result.fragments.len(), 1);
        assert_eq!(result.fragments[0].slot.page_index, 1);
        assert_eq!(result.fragments[0].fragment.lines.len(), 4);
    }

    #[test]
    fn paragraph_track_driver_uses_the_next_track_width_for_widow_control() {
        let paragraph = Paragraph {
            runs: vec![Run {
                text: "alpha beta gamma delta epsilon zeta eta theta".to_string(),
                ..Run::default()
            }],
            ..Paragraph::default()
        };
        let tracks = [
            FragmentTrackSlot {
                page_index: 0,
                column_index: 0,
                x: 0.0,
                fresh: false,
                track: FragmentTrack {
                    width: 60.0,
                    height: 31.0,
                },
            },
            FragmentTrackSlot {
                page_index: 1,
                column_index: 0,
                x: 0.0,
                fresh: true,
                track: FragmentTrack {
                    width: 200.0,
                    height: 100.0,
                },
            },
        ];
        let result = constrained_track_fragments(
            &paragraph,
            &tracks,
            PaginationHint {
                widow_control: true,
                ..PaginationHint::default()
            },
        );

        assert!(result.next.is_none());
        assert_eq!(result.fragments.len(), 1);
        assert_eq!(result.fragments[0].fragment.lines.len(), 2);
        assert_eq!(
            result
                .fragments
                .iter()
                .map(|placed| placed.slot.page_index)
                .collect::<Vec<_>>(),
            vec![1]
        );
    }

    #[test]
    fn paragraph_track_driver_forces_progress_for_oversized_keep_lines() {
        let paragraph = Paragraph {
            runs: vec![Run {
                text: "one\ntwo\nthree\nfour".to_string(),
                ..Run::default()
            }],
            ..Paragraph::default()
        };
        let tracks = [0, 1].map(|page_index| FragmentTrackSlot {
            page_index,
            column_index: 0,
            x: 0.0,
            fresh: true,
            track: FragmentTrack {
                width: 180.0,
                height: 11.0,
            },
        });
        let result = constrained_track_fragments(
            &paragraph,
            &tracks,
            PaginationHint {
                keep_lines: true,
                ..PaginationHint::default()
            },
        );

        assert_eq!(result.fragments.len(), 2);
        assert_eq!(
            result
                .fragments
                .iter()
                .map(|placed| placed.fragment.lines.len())
                .collect::<Vec<_>>(),
            vec![1, 1]
        );
        let next = result.next.expect("oversized keep-lines continuation");
        assert!(next.source_char > 0);
        assert_eq!(
            result.fragments[0]
                .fragment
                .lines
                .last()
                .and_then(|line| line.char_range)
                .map(|range| range.end),
            result.fragments[1]
                .fragment
                .lines
                .first()
                .and_then(|line| line.char_range)
                .map(|range| range.start)
        );
    }
}
