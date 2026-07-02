use super::*;

#[derive(Debug, Clone, Default)]
pub(crate) struct NoteRefContext {
    pub(crate) targets: HashMap<String, NoteRefTarget>,
    field_positions: Vec<NoteRefFieldPosition>,
    ref_field_positions: Vec<NoteRefFieldPosition>,
    markers: Vec<NoteRefMarker>,
    generated_ref_note_fields: Vec<NoteRefGeneratedField>,
}

impl NoteRefContext {
    pub(crate) fn empty() -> Self {
        Self::default()
    }

    pub(super) fn target(&self, name: &str) -> Option<NoteRefTarget> {
        self.targets.get(name).copied()
    }

    pub(crate) fn field_position(&self, index: usize) -> Option<NoteRefFieldPosition> {
        self.field_positions.get(index).copied()
    }

    pub(crate) fn ref_field_position(&self, index: usize) -> Option<NoteRefFieldPosition> {
        self.ref_field_positions.get(index).copied()
    }

    pub(crate) fn target_is_note_marker(&self, name: &str) -> bool {
        self.target(name).is_some_and(NoteRefTarget::is_note_marker)
    }

    pub(super) fn ref_note_number(
        &self,
        name: &str,
        field_position: Option<NoteRefFieldPosition>,
    ) -> Option<usize> {
        let target = self.target(name)?;
        let field = field_position?;
        let actual_before = self
            .markers
            .iter()
            .filter(|marker| marker.kind == target.kind && marker.order < field.order)
            .count();
        let generated_before = self
            .generated_ref_note_fields
            .iter()
            .filter(|generated| generated.order < field.order)
            .filter_map(|generated| self.target(&generated.target))
            .filter(|generated_target| generated_target.kind == target.kind)
            .count();
        Some(actual_before + generated_before + 1)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct NoteRefTarget {
    kind: NoteRefKind,
    number: usize,
    start: usize,
    end: usize,
}

impl NoteRefTarget {
    fn is_note_marker(self) -> bool {
        matches!(self.kind, NoteRefKind::Footnote | NoteRefKind::Endnote)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NoteRefKind {
    Footnote,
    Endnote,
    Comment,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct NoteRefMarker {
    kind: NoteRefKind,
    order: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct NoteRefGeneratedField {
    target: String,
    order: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct NoteRefFieldPosition {
    order: usize,
}

#[derive(Debug, Clone)]
struct NoteRefScanField {
    instruction: String,
    phase: FieldPhase,
}

#[derive(Debug, Clone)]
struct NoteRefActiveBookmark {
    id: String,
    name: String,
    start: usize,
}

#[derive(Debug, Clone)]
struct NoteRefActiveCommentRange {
    id: String,
    bookmark_names: Vec<String>,
    start: usize,
}

#[derive(Debug, Clone)]
struct NoteRefCommentRangeTarget {
    name: String,
    start: usize,
    end: usize,
}

pub(crate) fn note_ref_context(xml: &str) -> NoteRefContext {
    let mut r = Reader::from_str(xml);
    let mut targets = HashMap::new();
    let mut field_positions = Vec::new();
    let mut ref_field_positions = Vec::new();
    let mut markers = Vec::new();
    let mut generated_ref_note_fields = Vec::new();
    let mut active_bookmarks: Vec<NoteRefActiveBookmark> = Vec::new();
    let mut active_comment_ranges = Vec::new();
    let mut comment_range_targets: HashMap<String, Vec<NoteRefCommentRangeTarget>> = HashMap::new();
    let mut source_order = 0usize;
    let mut footnote_number = 0usize;
    let mut endnote_number = 0usize;
    let mut comment_number = 0usize;
    let mut current: Option<NoteRefScanField> = None;
    let mut computed_fields = NoteRefComputedFieldState::default();
    let mut xml_depth = 0usize;
    let mut alternate_content_stack = Vec::new();
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) => {
                let qname = e.name();
                let name = local(qname.as_ref());
                if should_skip_alternate_branch(&mut alternate_content_stack, xml_depth, name) {
                    skip_subtree(&mut r);
                    continue;
                }
                match name {
                    b"del" | b"moveFrom" => {
                        skip_subtree(&mut r);
                        continue;
                    }
                    b"AlternateContent" => {
                        alternate_content_stack.push(AlternateContentBranchState {
                            branch_depth: xml_depth + 1,
                            took_branch: false,
                        });
                    }
                    b"fldSimple" => {
                        if let Some(text) = computed_note_ref_scan_field_result(
                            attr_local(&e, b"instr").as_deref(),
                            &mut computed_fields,
                        ) {
                            if !text.is_empty() {
                                source_order += 1;
                            }
                            skip_subtree(&mut r);
                            continue;
                        }
                        record_note_ref_scan_field_position(
                            attr_local(&e, b"instr").as_deref(),
                            &mut source_order,
                            &mut field_positions,
                            &mut ref_field_positions,
                            &mut generated_ref_note_fields,
                        );
                    }
                    b"fldChar" => apply_note_ref_scan_fld_char(
                        &e,
                        &mut source_order,
                        &mut current,
                        &mut field_positions,
                        &mut ref_field_positions,
                        &mut generated_ref_note_fields,
                    ),
                    b"instrText" => {
                        let text = read_text(&mut r);
                        if let Some(field) = current.as_mut() {
                            if field.phase == FieldPhase::Instruction {
                                field.instruction.push_str(&text);
                            }
                        }
                        continue;
                    }
                    b"bookmarkStart" => {
                        if let Some((id, name)) = bookmark_start(&e) {
                            active_bookmarks.push(NoteRefActiveBookmark {
                                id,
                                name,
                                start: source_order,
                            });
                            source_order += 1;
                        }
                    }
                    b"bookmarkEnd" => {
                        let id = bookmark_end_id(&e);
                        record_note_ref_contained_comment_range_target(
                            id.as_deref(),
                            source_order,
                            &active_bookmarks,
                            &active_comment_ranges,
                            &mut comment_range_targets,
                        );
                        close_note_ref_bookmark(
                            id.as_deref(),
                            source_order,
                            &mut active_bookmarks,
                            &mut targets,
                        );
                        source_order += 1;
                    }
                    b"commentRangeStart" => {
                        record_note_ref_comment_range_start(
                            &e,
                            &active_bookmarks,
                            source_order,
                            &mut active_comment_ranges,
                        );
                    }
                    b"commentRangeEnd" => {
                        close_note_ref_comment_range(
                            &e,
                            source_order,
                            &active_bookmarks,
                            &mut active_comment_ranges,
                            &mut comment_range_targets,
                        );
                    }
                    b"footnoteReference" => {
                        footnote_number += 1;
                        markers.push(NoteRefMarker {
                            kind: NoteRefKind::Footnote,
                            order: source_order,
                        });
                        record_note_ref_target(
                            &active_bookmarks,
                            NoteRefKind::Footnote,
                            footnote_number,
                            source_order,
                            &mut targets,
                        );
                        source_order += 1;
                        skip_subtree(&mut r);
                        continue;
                    }
                    b"endnoteReference" => {
                        endnote_number += 1;
                        markers.push(NoteRefMarker {
                            kind: NoteRefKind::Endnote,
                            order: source_order,
                        });
                        record_note_ref_target(
                            &active_bookmarks,
                            NoteRefKind::Endnote,
                            endnote_number,
                            source_order,
                            &mut targets,
                        );
                        source_order += 1;
                        skip_subtree(&mut r);
                        continue;
                    }
                    b"commentReference" => {
                        comment_number += 1;
                        markers.push(NoteRefMarker {
                            kind: NoteRefKind::Comment,
                            order: source_order,
                        });
                        record_note_ref_target(
                            &active_bookmarks,
                            NoteRefKind::Comment,
                            comment_number,
                            source_order,
                            &mut targets,
                        );
                        record_note_ref_comment_range_targets(
                            &e,
                            comment_number,
                            &comment_range_targets,
                            &mut targets,
                        );
                        source_order += 1;
                        skip_subtree(&mut r);
                        continue;
                    }
                    b"t" => {
                        if !read_text(&mut r).is_empty() {
                            source_order += 1;
                        }
                        continue;
                    }
                    b"tab" | b"br" | b"cr" | b"noBreakHyphen" | b"softHyphen" | b"drawing"
                    | b"pict" | b"object" => {
                        source_order += 1;
                    }
                    _ => {}
                }
                xml_depth = xml_depth.saturating_add(1);
            }
            Ok(Event::Empty(e)) => {
                let qname = e.name();
                let name = local(qname.as_ref());
                if should_skip_alternate_branch(&mut alternate_content_stack, xml_depth, name) {
                    continue;
                }
                match name {
                    b"fldSimple" => {
                        if let Some(text) = computed_note_ref_scan_field_result(
                            attr_local(&e, b"instr").as_deref(),
                            &mut computed_fields,
                        ) {
                            if !text.is_empty() {
                                source_order += 1;
                            }
                        } else {
                            record_note_ref_scan_field_position(
                                attr_local(&e, b"instr").as_deref(),
                                &mut source_order,
                                &mut field_positions,
                                &mut ref_field_positions,
                                &mut generated_ref_note_fields,
                            );
                        }
                    }
                    b"fldChar" => apply_note_ref_scan_fld_char(
                        &e,
                        &mut source_order,
                        &mut current,
                        &mut field_positions,
                        &mut ref_field_positions,
                        &mut generated_ref_note_fields,
                    ),
                    b"bookmarkStart" => {
                        if let Some((id, name)) = bookmark_start(&e) {
                            active_bookmarks.push(NoteRefActiveBookmark {
                                id,
                                name,
                                start: source_order,
                            });
                            source_order += 1;
                        }
                    }
                    b"bookmarkEnd" => {
                        let id = bookmark_end_id(&e);
                        record_note_ref_contained_comment_range_target(
                            id.as_deref(),
                            source_order,
                            &active_bookmarks,
                            &active_comment_ranges,
                            &mut comment_range_targets,
                        );
                        close_note_ref_bookmark(
                            id.as_deref(),
                            source_order,
                            &mut active_bookmarks,
                            &mut targets,
                        );
                        source_order += 1;
                    }
                    b"commentRangeStart" => {
                        record_note_ref_comment_range_start(
                            &e,
                            &active_bookmarks,
                            source_order,
                            &mut active_comment_ranges,
                        );
                    }
                    b"commentRangeEnd" => {
                        close_note_ref_comment_range(
                            &e,
                            source_order,
                            &active_bookmarks,
                            &mut active_comment_ranges,
                            &mut comment_range_targets,
                        );
                    }
                    b"footnoteReference" => {
                        footnote_number += 1;
                        markers.push(NoteRefMarker {
                            kind: NoteRefKind::Footnote,
                            order: source_order,
                        });
                        record_note_ref_target(
                            &active_bookmarks,
                            NoteRefKind::Footnote,
                            footnote_number,
                            source_order,
                            &mut targets,
                        );
                        source_order += 1;
                    }
                    b"endnoteReference" => {
                        endnote_number += 1;
                        markers.push(NoteRefMarker {
                            kind: NoteRefKind::Endnote,
                            order: source_order,
                        });
                        record_note_ref_target(
                            &active_bookmarks,
                            NoteRefKind::Endnote,
                            endnote_number,
                            source_order,
                            &mut targets,
                        );
                        source_order += 1;
                    }
                    b"commentReference" => {
                        comment_number += 1;
                        markers.push(NoteRefMarker {
                            kind: NoteRefKind::Comment,
                            order: source_order,
                        });
                        record_note_ref_target(
                            &active_bookmarks,
                            NoteRefKind::Comment,
                            comment_number,
                            source_order,
                            &mut targets,
                        );
                        record_note_ref_comment_range_targets(
                            &e,
                            comment_number,
                            &comment_range_targets,
                            &mut targets,
                        );
                        source_order += 1;
                    }
                    b"tab" | b"br" | b"cr" | b"noBreakHyphen" | b"softHyphen" | b"drawing"
                    | b"pict" | b"object" => {
                        source_order += 1;
                    }
                    _ => {}
                }
            }
            Ok(Event::End(e)) => {
                if local(e.name().as_ref()) == b"AlternateContent" {
                    alternate_content_stack.pop();
                }
                xml_depth = xml_depth.saturating_sub(1);
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    NoteRefContext {
        targets,
        field_positions,
        ref_field_positions,
        markers,
        generated_ref_note_fields,
    }
}

#[derive(Debug, Default)]
struct NoteRefComputedFieldState {
    field_bookmarks: HashMap<String, String>,
    sequence_counters: HashMap<String, i64>,
    autonum_counter: i64,
    listnum_counter: i64,
}

fn computed_note_ref_scan_field_result(
    instruction: Option<&str>,
    state: &mut NoteRefComputedFieldState,
) -> Option<String> {
    let instruction = normalize_instruction(instruction?);
    match FieldKind::from_instruction(&instruction) {
        FieldKind::Dynamic(kind) if kind == "SET" => {
            return computed_set_result(&instruction, &mut state.field_bookmarks);
        }
        FieldKind::Dynamic(kind) if kind == "ASK" => {
            return computed_ask_result(&instruction, &mut state.field_bookmarks);
        }
        _ => {}
    }
    computed_numbering_result(&instruction, &mut state.autonum_counter)
        .or_else(|| computed_listnum_result(&instruction, &mut state.listnum_counter))
        .or_else(|| computed_sequence_result(&instruction, &mut state.sequence_counters))
        .or_else(|| computed_dynamic_result_with_bookmarks(&instruction, &state.field_bookmarks))
        .or_else(|| computed_display_result(&instruction))
        .or_else(|| computed_action_result(&instruction))
        .or_else(|| computed_reference_index_result(&instruction))
        .or_else(|| computed_toc_entry_result(&instruction))
}

pub(crate) fn note_ref_target_names(xml: &str) -> HashSet<String> {
    note_ref_context(xml)
        .targets
        .into_iter()
        .filter_map(|(name, target)| target.is_note_marker().then_some(name))
        .collect()
}

fn record_note_ref_target(
    active_bookmarks: &[NoteRefActiveBookmark],
    kind: NoteRefKind,
    number: usize,
    order: usize,
    targets: &mut HashMap<String, NoteRefTarget>,
) {
    for bookmark in active_bookmarks {
        targets
            .entry(bookmark.name.clone())
            .or_insert(NoteRefTarget {
                kind,
                number,
                start: bookmark.start,
                end: order,
            });
    }
}

fn record_note_ref_comment_range_start(
    e: &BytesStart<'_>,
    active_bookmarks: &[NoteRefActiveBookmark],
    source_order: usize,
    active_comment_ranges: &mut Vec<NoteRefActiveCommentRange>,
) {
    let Some(id) = attr_local_trimmed(e, b"id") else {
        return;
    };
    let bookmark_names = active_bookmarks
        .iter()
        .map(|bookmark| bookmark.name.clone())
        .collect::<Vec<_>>();
    active_comment_ranges.push(NoteRefActiveCommentRange {
        id,
        bookmark_names,
        start: source_order,
    });
}

fn close_note_ref_comment_range(
    e: &BytesStart<'_>,
    end: usize,
    active_bookmarks: &[NoteRefActiveBookmark],
    active_comment_ranges: &mut Vec<NoteRefActiveCommentRange>,
    comment_range_targets: &mut HashMap<String, Vec<NoteRefCommentRangeTarget>>,
) {
    let Some(id) = attr_local_trimmed(e, b"id") else {
        return;
    };
    let Some(index) = active_comment_ranges
        .iter()
        .rposition(|range| range.id == id)
    else {
        return;
    };
    let range = active_comment_ranges.remove(index);
    let active_names = active_bookmarks
        .iter()
        .map(|bookmark| bookmark.name.as_str())
        .collect::<Vec<_>>();
    let range_targets = comment_range_targets.entry(id).or_default();
    for name in range.bookmark_names {
        if active_names.iter().any(|active| *active == name) {
            range_targets.push(NoteRefCommentRangeTarget {
                name,
                start: range.start,
                end,
            });
        }
    }
}

fn record_note_ref_comment_range_targets(
    e: &BytesStart<'_>,
    number: usize,
    comment_range_targets: &HashMap<String, Vec<NoteRefCommentRangeTarget>>,
    targets: &mut HashMap<String, NoteRefTarget>,
) {
    let Some(id) = attr_local_trimmed(e, b"id") else {
        return;
    };
    let Some(range_targets) = comment_range_targets.get(&id) else {
        return;
    };
    for range in range_targets {
        targets.entry(range.name.clone()).or_insert(NoteRefTarget {
            kind: NoteRefKind::Comment,
            number,
            start: range.start,
            end: range.end,
        });
    }
}

fn record_note_ref_contained_comment_range_target(
    id: Option<&str>,
    end: usize,
    active_bookmarks: &[NoteRefActiveBookmark],
    active_comment_ranges: &[NoteRefActiveCommentRange],
    comment_range_targets: &mut HashMap<String, Vec<NoteRefCommentRangeTarget>>,
) {
    let Some(id) = id else {
        return;
    };
    let Some(bookmark) = active_bookmarks.iter().find(|bookmark| bookmark.id == id) else {
        return;
    };
    for range in active_comment_ranges
        .iter()
        .filter(|range| bookmark.start >= range.start)
    {
        comment_range_targets
            .entry(range.id.clone())
            .or_default()
            .push(NoteRefCommentRangeTarget {
                name: bookmark.name.clone(),
                start: bookmark.start,
                end,
            });
    }
}

fn close_note_ref_bookmark(
    id: Option<&str>,
    end: usize,
    active_bookmarks: &mut Vec<NoteRefActiveBookmark>,
    targets: &mut HashMap<String, NoteRefTarget>,
) {
    let Some(id) = id else {
        return;
    };
    if let Some(index) = active_bookmarks
        .iter()
        .position(|bookmark| bookmark.id == id)
    {
        let bookmark = active_bookmarks.remove(index);
        if let Some(target) = targets.get_mut(&bookmark.name) {
            if target.start == bookmark.start {
                target.end = end;
            }
        }
    }
}

fn record_note_ref_scan_field_position(
    instruction: Option<&str>,
    source_order: &mut usize,
    field_positions: &mut Vec<NoteRefFieldPosition>,
    ref_field_positions: &mut Vec<NoteRefFieldPosition>,
    generated_ref_note_fields: &mut Vec<NoteRefGeneratedField>,
) {
    let Some(instruction) = instruction.map(normalize_instruction) else {
        return;
    };
    let mut recorded = false;
    if field_kind(&instruction) == FieldKind::NoteRef {
        field_positions.push(NoteRefFieldPosition {
            order: *source_order,
        });
        recorded = true;
    }
    if is_ref_position_field_instruction(&instruction) {
        ref_field_positions.push(NoteRefFieldPosition {
            order: *source_order,
        });
        if let Some(target) = ref_note_field_target(&instruction) {
            generated_ref_note_fields.push(NoteRefGeneratedField {
                target,
                order: *source_order,
            });
        }
        recorded = true;
    }
    if recorded {
        *source_order += 1;
    }
}

fn apply_note_ref_scan_fld_char(
    e: &BytesStart<'_>,
    source_order: &mut usize,
    current: &mut Option<NoteRefScanField>,
    field_positions: &mut Vec<NoteRefFieldPosition>,
    ref_field_positions: &mut Vec<NoteRefFieldPosition>,
    generated_ref_note_fields: &mut Vec<NoteRefGeneratedField>,
) {
    match field_char_type(e).as_deref() {
        Some("begin") => {
            *current = Some(NoteRefScanField {
                instruction: String::new(),
                phase: FieldPhase::Instruction,
            });
        }
        Some("separate") => {
            if let Some(field) = current.as_mut() {
                field.phase = FieldPhase::Result;
            }
        }
        Some("end") => {
            if let Some(field) = current.take() {
                record_note_ref_scan_field_position(
                    Some(&field.instruction),
                    source_order,
                    field_positions,
                    ref_field_positions,
                    generated_ref_note_fields,
                );
            }
        }
        _ => {}
    }
}

pub(crate) fn computed_note_ref_result(
    instruction: &str,
    note_refs: &NoteRefContext,
    field_position: Option<NoteRefFieldPosition>,
) -> Option<String> {
    let spec = note_ref_instruction(instruction)?;
    let target = note_refs.target(&spec.target)?;
    if !target.is_note_marker() {
        return None;
    }
    let text = if spec.relative {
        computed_relative_note_ref_result(target, field_position)?
    } else {
        format_page_number(target.number, spec.number_format)?
    };
    Some(apply_field_text_format(text, spec.text_format))
}

fn computed_relative_note_ref_result(
    target: NoteRefTarget,
    field_position: Option<NoteRefFieldPosition>,
) -> Option<String> {
    let field = field_position?;
    if field.order < target.start {
        return Some("below".to_string());
    }
    (field.order > target.end).then(|| "above".to_string())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct NoteRefInstruction {
    target: String,
    number_format: Option<PageNumberFormat>,
    text_format: Option<FieldTextFormat>,
    relative: bool,
}

pub(super) fn note_ref_instruction(instruction: &str) -> Option<NoteRefInstruction> {
    let syntax = note_ref_field_syntax(instruction)?;
    Some(NoteRefInstruction {
        target: syntax.target,
        number_format: syntax
            .number_format
            .map(page_number_format_from_field_format),
        text_format: syntax.text_format,
        relative: syntax.relative,
    })
}
