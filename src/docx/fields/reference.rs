use super::*;

pub(crate) fn ref_targets(xml: &str) -> HashMap<String, String> {
    let mut r = Reader::from_str(xml);
    let mut active: Vec<(String, String)> = Vec::new();
    let mut out: HashMap<String, String> = HashMap::new();
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
                if matches!(name, b"del" | b"moveFrom") {
                    skip_subtree(&mut r);
                    continue;
                }
                match name {
                    b"AlternateContent" => {
                        alternate_content_stack.push(AlternateContentBranchState {
                            branch_depth: xml_depth + 1,
                            took_branch: false,
                        });
                    }
                    b"p" => append_ref_paragraph_breaks(&active, &mut out),
                    b"bookmarkStart" => {
                        if let Some((id, name)) = bookmark_start(&e) {
                            out.entry(name.clone()).or_default();
                            active.push((id, name));
                        }
                    }
                    b"bookmarkEnd" => {
                        if let Some(id) = bookmark_end_id(&e) {
                            active.retain(|(active_id, _)| active_id != &id);
                        }
                    }
                    b"t" => {
                        let text = read_text(&mut r);
                        if !text.is_empty() {
                            append_ref_text(&active, &mut out, &text);
                        }
                        continue;
                    }
                    b"tab" => append_ref_text(&active, &mut out, "\t"),
                    b"br" => {
                        if is_page_break_type(&e) {
                            append_ref_text(&active, &mut out, "\u{000C}");
                        } else {
                            append_ref_text(&active, &mut out, "\n");
                        }
                    }
                    b"cr" => append_ref_text(&active, &mut out, "\n"),
                    b"noBreakHyphen" => append_ref_text(&active, &mut out, "-"),
                    b"softHyphen" => append_ref_text(&active, &mut out, "\u{00ad}"),
                    b"sym" => append_ref_symbol(&active, &mut out, &e),
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
                    b"bookmarkStart" => {
                        if let Some((id, name)) = bookmark_start(&e) {
                            out.entry(name.clone()).or_default();
                            active.push((id, name));
                        }
                    }
                    b"bookmarkEnd" => {
                        if let Some(id) = bookmark_end_id(&e) {
                            active.retain(|(active_id, _)| active_id != &id);
                        }
                    }
                    b"tab" => append_ref_text(&active, &mut out, "\t"),
                    b"br" => {
                        if is_page_break_type(&e) {
                            append_ref_text(&active, &mut out, "\u{000C}");
                        } else {
                            append_ref_text(&active, &mut out, "\n");
                        }
                    }
                    b"cr" => append_ref_text(&active, &mut out, "\n"),
                    b"noBreakHyphen" => append_ref_text(&active, &mut out, "-"),
                    b"softHyphen" => append_ref_text(&active, &mut out, "\u{00ad}"),
                    b"sym" => append_ref_symbol(&active, &mut out, &e),
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
    out
}

fn append_ref_symbol(
    active: &[(String, String)],
    out: &mut HashMap<String, String>,
    e: &BytesStart<'_>,
) {
    let font = attr_local_trimmed(e, b"font");
    let Some(value) = attr_local_trimmed(e, b"char") else {
        return;
    };
    let Some(ch) = super::display::computed_run_symbol_char(font.as_deref(), &value) else {
        return;
    };
    append_ref_text(active, out, &ch.to_string());
}

#[derive(Debug, Clone, Default)]
pub(crate) struct RefPositionContext {
    target_positions: HashMap<String, RefTargetPosition>,
    field_positions: Vec<RefFieldPosition>,
}

impl RefPositionContext {
    pub(crate) fn target_position(&self, name: &str) -> Option<RefTargetPosition> {
        self.target_positions.get(name).copied()
    }

    pub(crate) fn field_position(&self, index: usize) -> Option<RefFieldPosition> {
        self.field_positions.get(index).cloned()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RefTargetPosition {
    start: usize,
    end: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RefFieldPosition {
    order: usize,
    number_context: Option<String>,
}

#[derive(Debug, Clone)]
struct RefScanField {
    instruction: String,
    phase: FieldPhase,
}

#[derive(Debug, Clone, Default)]
struct RefPositionParagraph {
    depth: usize,
    properties_depth: usize,
    num_id: Option<String>,
    ilvl: u8,
    field_position_indices: Vec<usize>,
}

impl RefPositionParagraph {
    fn active(&self) -> bool {
        self.depth > 0
    }

    fn reset(&mut self) {
        *self = Self {
            depth: 1,
            ..Self::default()
        };
    }
}

pub(crate) fn ref_position_context(xml: &str, numbering: &Numbering) -> RefPositionContext {
    let mut r = Reader::from_str(xml);
    let mut target_positions = HashMap::new();
    let mut field_positions = Vec::new();
    let mut active_bookmarks: Vec<(String, String, usize)> = Vec::new();
    let mut paragraph = RefPositionParagraph::default();
    let mut counters: HashMap<String, [u32; 9]> = HashMap::new();
    let mut source_order = 0usize;
    let mut current: Option<RefScanField> = None;
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
                if matches!(name, b"del" | b"moveFrom" | b"pPrChange") {
                    skip_subtree(&mut r);
                    continue;
                }
                let mut consumed_element = false;
                match name {
                    b"AlternateContent" => {
                        alternate_content_stack.push(AlternateContentBranchState {
                            branch_depth: xml_depth + 1,
                            took_branch: false,
                        });
                    }
                    b"p" => {
                        if paragraph.active() {
                            paragraph.depth += 1;
                        } else {
                            paragraph.reset();
                        }
                    }
                    b"pPr" if paragraph.active() => paragraph.properties_depth += 1,
                    b"ilvl" if paragraph.properties_depth > 0 => {
                        if let Some(value) = attr_u8(&e, b"val") {
                            paragraph.ilvl = value;
                        }
                    }
                    b"numId" if paragraph.properties_depth > 0 => {
                        paragraph.num_id = attr_local_trimmed(&e, b"val");
                    }
                    b"fldSimple" => record_ref_field_position(
                        attr_local(&e, b"instr").as_deref(),
                        &mut source_order,
                        &mut field_positions,
                        &mut paragraph,
                    ),
                    b"fldChar" => apply_ref_scan_fld_char(
                        &e,
                        &mut source_order,
                        &mut current,
                        &mut field_positions,
                        &mut paragraph,
                    ),
                    b"instrText" => {
                        let text = read_text(&mut r);
                        consumed_element = true;
                        if let Some(field) = current.as_mut() {
                            if field.phase == FieldPhase::Instruction {
                                field.instruction.push_str(&text);
                            }
                        }
                    }
                    b"bookmarkStart" => {
                        if let Some((id, name)) = bookmark_start(&e) {
                            active_bookmarks.push((id, name, source_order));
                            source_order += 1;
                        }
                    }
                    b"bookmarkEnd" => {
                        close_ref_position_bookmark(
                            bookmark_end_id(&e).as_deref(),
                            source_order,
                            &mut active_bookmarks,
                            &mut target_positions,
                        );
                        source_order += 1;
                    }
                    b"t" => {
                        if !read_text(&mut r).is_empty() {
                            source_order += 1;
                        }
                        consumed_element = true;
                    }
                    b"tab" | b"br" | b"cr" | b"noBreakHyphen" | b"softHyphen" | b"drawing"
                    | b"pict" | b"object" => {
                        source_order += 1;
                    }
                    _ => {}
                }
                if !consumed_element {
                    xml_depth = xml_depth.saturating_add(1);
                }
            }
            Ok(Event::Empty(e)) => {
                let qname = e.name();
                let name = local(qname.as_ref());
                if should_skip_alternate_branch(&mut alternate_content_stack, xml_depth, name) {
                    continue;
                }
                match name {
                    b"p" => {
                        paragraph.reset();
                        finish_ref_position_paragraph(
                            &mut paragraph,
                            numbering,
                            &mut counters,
                            &mut field_positions,
                        );
                    }
                    b"pPr" if paragraph.active() => {}
                    b"ilvl" if paragraph.properties_depth > 0 => {
                        if let Some(value) = attr_u8(&e, b"val") {
                            paragraph.ilvl = value;
                        }
                    }
                    b"numId" if paragraph.properties_depth > 0 => {
                        paragraph.num_id = attr_local_trimmed(&e, b"val");
                    }
                    b"fldSimple" => record_ref_field_position(
                        attr_local(&e, b"instr").as_deref(),
                        &mut source_order,
                        &mut field_positions,
                        &mut paragraph,
                    ),
                    b"fldChar" => apply_ref_scan_fld_char(
                        &e,
                        &mut source_order,
                        &mut current,
                        &mut field_positions,
                        &mut paragraph,
                    ),
                    b"bookmarkStart" => {
                        if let Some((id, name)) = bookmark_start(&e) {
                            active_bookmarks.push((id, name, source_order));
                            source_order += 1;
                        }
                    }
                    b"bookmarkEnd" => {
                        close_ref_position_bookmark(
                            bookmark_end_id(&e).as_deref(),
                            source_order,
                            &mut active_bookmarks,
                            &mut target_positions,
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
                let qname = e.name();
                let name = local(qname.as_ref());
                if name == b"AlternateContent" {
                    alternate_content_stack.pop();
                } else {
                    match name {
                        b"p" if paragraph.active() => {
                            if paragraph.depth == 1 {
                                finish_ref_position_paragraph(
                                    &mut paragraph,
                                    numbering,
                                    &mut counters,
                                    &mut field_positions,
                                );
                            } else {
                                paragraph.depth -= 1;
                            }
                        }
                        b"pPr" if paragraph.properties_depth > 0 => {
                            paragraph.properties_depth -= 1;
                        }
                        _ => {}
                    }
                }
                xml_depth = xml_depth.saturating_sub(1);
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    RefPositionContext {
        target_positions,
        field_positions,
    }
}

fn close_ref_position_bookmark(
    id: Option<&str>,
    end: usize,
    active_bookmarks: &mut Vec<(String, String, usize)>,
    target_positions: &mut HashMap<String, RefTargetPosition>,
) {
    let Some(id) = id else {
        return;
    };
    if let Some(index) = active_bookmarks
        .iter()
        .position(|(active_id, _, _)| active_id == id)
    {
        let (_, name, start) = active_bookmarks.remove(index);
        target_positions
            .entry(name)
            .or_insert(RefTargetPosition { start, end });
    }
}

fn record_ref_field_position(
    instruction: Option<&str>,
    source_order: &mut usize,
    field_positions: &mut Vec<RefFieldPosition>,
    paragraph: &mut RefPositionParagraph,
) {
    if instruction
        .map(normalize_instruction)
        .as_deref()
        .is_some_and(is_ref_position_field_instruction)
    {
        let index = field_positions.len();
        field_positions.push(RefFieldPosition {
            order: *source_order,
            number_context: None,
        });
        if paragraph.active() {
            paragraph.field_position_indices.push(index);
        }
        *source_order += 1;
    }
}

fn apply_ref_scan_fld_char(
    e: &BytesStart<'_>,
    source_order: &mut usize,
    current: &mut Option<RefScanField>,
    field_positions: &mut Vec<RefFieldPosition>,
    paragraph: &mut RefPositionParagraph,
) {
    match field_char_type(e).as_deref() {
        Some("begin") => {
            *current = Some(RefScanField {
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
                record_ref_field_position(
                    Some(&field.instruction),
                    source_order,
                    field_positions,
                    paragraph,
                );
            }
        }
        _ => {}
    }
}

fn finish_ref_position_paragraph(
    paragraph: &mut RefPositionParagraph,
    numbering: &Numbering,
    counters: &mut HashMap<String, [u32; 9]>,
    field_positions: &mut [RefFieldPosition],
) {
    if let Some(num_id) = paragraph.num_id.as_deref().filter(|num_id| *num_id != "0") {
        let counter = counters.entry(num_id.to_string()).or_insert([0; 9]);
        let number_context = numbering
            .label(num_id, paragraph.ilvl, counter)
            .and_then(|_| numbering.full_context_label(num_id, paragraph.ilvl, counter))
            .and_then(|label| ref_paragraph_number(&label));
        if let Some(number_context) = number_context {
            for index in &paragraph.field_position_indices {
                if let Some(field) = field_positions.get_mut(*index) {
                    field.number_context = Some(number_context.clone());
                }
            }
        }
    }
    *paragraph = RefPositionParagraph::default();
}

#[derive(Debug, Clone, Default)]
pub(crate) struct RefNumberContext {
    target_numbers: HashMap<String, RefTargetNumber>,
}

impl RefNumberContext {
    pub(crate) fn empty() -> Self {
        Self::default()
    }

    fn target_number(&self, name: &str, suppress_non_numeric: bool) -> Option<&str> {
        let number = self.target_numbers.get(name)?;
        if suppress_non_numeric {
            number.numeric.as_deref()
        } else {
            Some(number.text.as_str())
        }
    }

    fn target_full_context_number(&self, name: &str) -> Option<&str> {
        self.target_numbers.get(name)?.full_context.as_deref()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RefTargetNumber {
    text: String,
    numeric: Option<String>,
    full_context: Option<String>,
}

#[derive(Debug, Clone, Default)]
struct RefNumberParagraph {
    depth: usize,
    properties_depth: usize,
    num_id: Option<String>,
    ilvl: u8,
    bookmarks: Vec<String>,
}

impl RefNumberParagraph {
    fn active(&self) -> bool {
        self.depth > 0
    }

    fn reset(&mut self) {
        *self = Self {
            depth: 1,
            ..Self::default()
        };
    }
}

pub(crate) fn ref_number_context(xml: &str, numbering: &Numbering) -> RefNumberContext {
    let mut r = Reader::from_str(xml);
    let mut paragraph = RefNumberParagraph::default();
    let mut counters: HashMap<String, [u32; 9]> = HashMap::new();
    let mut target_numbers = HashMap::new();
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
                if matches!(name, b"del" | b"moveFrom" | b"pPrChange") {
                    skip_subtree(&mut r);
                    continue;
                }
                match name {
                    b"AlternateContent" => {
                        alternate_content_stack.push(AlternateContentBranchState {
                            branch_depth: xml_depth + 1,
                            took_branch: false,
                        });
                    }
                    b"p" => {
                        if paragraph.active() {
                            paragraph.depth += 1;
                        } else {
                            paragraph.reset();
                        }
                    }
                    b"pPr" if paragraph.active() => paragraph.properties_depth += 1,
                    b"ilvl" if paragraph.properties_depth > 0 => {
                        if let Some(value) = attr_u8(&e, b"val") {
                            paragraph.ilvl = value;
                        }
                    }
                    b"numId" if paragraph.properties_depth > 0 => {
                        paragraph.num_id = attr_local_trimmed(&e, b"val");
                    }
                    b"bookmarkStart" if paragraph.active() => {
                        if let Some(name) = bookmark_name(&e) {
                            push_unique(&mut paragraph.bookmarks, name);
                        }
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
                    b"p" => {
                        paragraph.reset();
                        finish_ref_number_paragraph(
                            &mut paragraph,
                            numbering,
                            &mut counters,
                            &mut target_numbers,
                        );
                    }
                    b"pPr" if paragraph.active() => {}
                    b"ilvl" if paragraph.properties_depth > 0 => {
                        if let Some(value) = attr_u8(&e, b"val") {
                            paragraph.ilvl = value;
                        }
                    }
                    b"numId" if paragraph.properties_depth > 0 => {
                        paragraph.num_id = attr_local_trimmed(&e, b"val");
                    }
                    b"bookmarkStart" if paragraph.active() => {
                        if let Some(name) = bookmark_name(&e) {
                            push_unique(&mut paragraph.bookmarks, name);
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::End(e)) => {
                let qname = e.name();
                let name = local(qname.as_ref());
                match name {
                    b"AlternateContent" => {
                        alternate_content_stack.pop();
                    }
                    b"p" if paragraph.active() => {
                        if paragraph.depth == 1 {
                            finish_ref_number_paragraph(
                                &mut paragraph,
                                numbering,
                                &mut counters,
                                &mut target_numbers,
                            );
                        } else {
                            paragraph.depth -= 1;
                        }
                    }
                    b"pPr" if paragraph.properties_depth > 0 => {
                        paragraph.properties_depth -= 1;
                    }
                    _ => {}
                }
                xml_depth = xml_depth.saturating_sub(1);
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    RefNumberContext { target_numbers }
}

fn finish_ref_number_paragraph(
    paragraph: &mut RefNumberParagraph,
    numbering: &Numbering,
    counters: &mut HashMap<String, [u32; 9]>,
    target_numbers: &mut HashMap<String, RefTargetNumber>,
) {
    if let Some(num_id) = paragraph.num_id.as_deref().filter(|num_id| *num_id != "0") {
        let counter = counters.entry(num_id.to_string()).or_insert([0; 9]);
        if let Some(label) = numbering.label(num_id, paragraph.ilvl, counter) {
            if let Some(number) = ref_paragraph_number(&label) {
                let full_context = numbering
                    .full_context_label(num_id, paragraph.ilvl, counter)
                    .and_then(|label| ref_paragraph_number(&label));
                let target_number = RefTargetNumber {
                    full_context,
                    numeric: ref_numeric_paragraph_number(&number),
                    text: number,
                };
                for bookmark in &paragraph.bookmarks {
                    target_numbers
                        .entry(bookmark.clone())
                        .or_insert_with(|| target_number.clone());
                }
            }
        }
    }
    *paragraph = RefNumberParagraph::default();
}

pub(super) fn ref_paragraph_number(label: &str) -> Option<String> {
    let without_periods = label.trim().trim_end_matches('.').trim_end();
    (!without_periods.is_empty()).then(|| without_periods.to_string())
}

pub(super) fn ref_numeric_paragraph_number(label: &str) -> Option<String> {
    let retained: String = label
        .chars()
        .filter(|ch| ch.is_ascii_digit() || matches!(ch, '.' | ',' | ':' | '-' | '/'))
        .collect();
    let numeric = retained.trim_matches(|ch: char| !ch.is_ascii_digit());
    (!numeric.is_empty()).then(|| numeric.to_string())
}

pub(crate) struct RefResultContext<'a> {
    pub(crate) bookmarks: &'a HashMap<String, String>,
    pub(crate) ref_positions: &'a RefPositionContext,
    pub(crate) ref_numbers: &'a RefNumberContext,
    pub(crate) note_refs: &'a NoteRefContext,
    pub(crate) field_bookmarks: &'a HashMap<String, String>,
}

pub(crate) fn computed_direct_bookmark_ref_result(
    instruction: &str,
    ctx: &RefResultContext<'_>,
    field_position: Option<RefFieldPosition>,
    note_ref_field_position: Option<NoteRefFieldPosition>,
) -> Option<String> {
    let spec = direct_bookmark_ref_instruction(instruction)?;
    let text =
        computed_ref_instruction_result(&spec, ctx, field_position, note_ref_field_position)?;
    Some(apply_field_text_format(text, spec.text_format))
}

pub(crate) fn computed_ref_result(
    instruction: &str,
    ctx: &RefResultContext<'_>,
    field_position: Option<RefFieldPosition>,
    note_ref_field_position: Option<NoteRefFieldPosition>,
) -> Option<String> {
    let spec = ref_instruction(instruction)?;
    let text =
        computed_ref_instruction_result(&spec, ctx, field_position, note_ref_field_position)?;
    Some(apply_field_text_format(text, spec.text_format))
}

pub(super) fn computed_ref_instruction_result(
    spec: &RefInstruction,
    ctx: &RefResultContext<'_>,
    field_position: Option<RefFieldPosition>,
    note_ref_field_position: Option<NoteRefFieldPosition>,
) -> Option<String> {
    if spec.sequence_separator {
        let _separator = spec.sequence_separator_value.as_deref()?;
        return None;
    }
    if spec.note_reference {
        let number = ctx
            .note_refs
            .ref_note_number(&spec.target, note_ref_field_position)?;
        return format_page_number(number, spec.number_format);
    }
    if spec.relative_context_number {
        let number =
            computed_relative_context_ref_number(spec, ctx.ref_numbers, field_position.clone())?;
        return if spec.relative {
            let relative = computed_relative_ref_result(spec, ctx.ref_positions, field_position)?;
            Some(format!("{number} {relative}"))
        } else {
            Some(number)
        };
    }
    if spec.full_context_number {
        let number = ctx.ref_numbers.target_full_context_number(&spec.target)?;
        return if spec.relative {
            let relative = computed_relative_ref_result(spec, ctx.ref_positions, field_position)?;
            Some(format!("{number} {relative}"))
        } else {
            Some(number.to_string())
        };
    }
    if spec.paragraph_number {
        let number = ctx
            .ref_numbers
            .target_number(&spec.target, spec.suppress_non_numeric)?;
        return if spec.relative {
            let relative = computed_relative_ref_result(spec, ctx.ref_positions, field_position)?;
            Some(format!("{number} {relative}"))
        } else {
            Some(number.to_string())
        };
    }
    if spec.relative {
        computed_relative_ref_result(spec, ctx.ref_positions, field_position)
    } else if let Some(text) = ctx.field_bookmarks.get(&spec.target) {
        computed_ref_bookmark_text_result(text, spec.number_format)
    } else {
        ctx.bookmarks
            .get(&spec.target)
            .and_then(|text| computed_ref_bookmark_text_result(text, spec.number_format))
    }
}

fn computed_ref_bookmark_text_result(
    text: &str,
    number_format: Option<PageNumberFormat>,
) -> Option<String> {
    if let Some(format) = number_format {
        let number = text.trim().parse::<usize>().ok()?;
        return format_page_number(number, Some(format));
    }
    Some(text.to_string())
}

pub(super) fn ref_instruction_target_known(
    spec: &RefInstruction,
    ctx: &RefResultContext<'_>,
) -> bool {
    ctx.bookmarks.contains_key(&spec.target)
        || ctx.field_bookmarks.contains_key(&spec.target)
        || ctx.ref_positions.target_position(&spec.target).is_some()
        || ctx.ref_numbers.target_numbers.contains_key(&spec.target)
        || ctx.note_refs.target(&spec.target).is_some()
}

fn computed_relative_context_ref_number(
    spec: &RefInstruction,
    ref_numbers: &RefNumberContext,
    field_position: Option<RefFieldPosition>,
) -> Option<String> {
    let target = ref_numbers.target_full_context_number(&spec.target)?;
    let field = field_position?.number_context?;
    Some(relative_context_ref_number(target, &field))
}

pub(super) fn relative_context_ref_number(target: &str, field: &str) -> String {
    let target_parts = target.split('.').collect::<Vec<_>>();
    let field_parts = field.split('.').collect::<Vec<_>>();
    let common = target_parts
        .iter()
        .zip(field_parts.iter())
        .take_while(|(target, field)| target == field)
        .count();
    let relative = target_parts
        .get(common..)
        .filter(|parts| !parts.is_empty())
        .unwrap_or(&target_parts);
    relative.join(".")
}

pub(crate) fn is_ref_position_field_instruction(instruction: &str) -> bool {
    field_kind(instruction) == FieldKind::Ref
        || direct_bookmark_ref_instruction(instruction).is_some()
}

pub(crate) fn is_direct_bookmark_ref_field_instruction(instruction: &str) -> bool {
    field_kind(instruction) != FieldKind::Ref
        && direct_bookmark_ref_instruction(instruction).is_some()
}

fn computed_relative_ref_result(
    spec: &RefInstruction,
    ref_positions: &RefPositionContext,
    field_position: Option<RefFieldPosition>,
) -> Option<String> {
    let target = ref_positions.target_position(&spec.target)?;
    let field = field_position?;
    if field.order < target.start {
        return Some("below".to_string());
    }
    (field.order > target.end).then(|| "above".to_string())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct RefInstruction {
    pub(super) target: String,
    pub(super) number_format: Option<PageNumberFormat>,
    pub(super) text_format: Option<FieldTextFormat>,
    pub(super) note_reference: bool,
    pub(super) sequence_separator: bool,
    pub(super) sequence_separator_value: Option<String>,
    pub(super) relative: bool,
    pub(super) paragraph_number: bool,
    pub(super) full_context_number: bool,
    pub(super) relative_context_number: bool,
    pub(super) suppress_non_numeric: bool,
}

pub(super) fn ref_instruction(instruction: &str) -> Option<RefInstruction> {
    ref_instruction_from_syntax(ref_field_syntax(instruction)?)
}

pub(super) fn direct_bookmark_ref_instruction(instruction: &str) -> Option<RefInstruction> {
    ref_instruction_from_syntax(direct_ref_field_syntax(instruction)?)
}

fn ref_instruction_from_syntax(
    syntax: crate::annotation::RefFieldSyntax,
) -> Option<RefInstruction> {
    Some(RefInstruction {
        target: syntax.target,
        number_format: syntax
            .number_format
            .map(page_number_format_from_field_format),
        text_format: syntax.text_format,
        note_reference: syntax.note_reference,
        sequence_separator: syntax.sequence_separator,
        sequence_separator_value: syntax.sequence_separator_value,
        relative: syntax.relative,
        paragraph_number: syntax.paragraph_number,
        full_context_number: syntax.full_context_number,
        relative_context_number: syntax.relative_context_number,
        suppress_non_numeric: syntax.suppress_non_numeric,
    })
}

pub(super) fn ref_note_field_target(instruction: &str) -> Option<String> {
    let spec =
        ref_instruction(instruction).or_else(|| direct_bookmark_ref_instruction(instruction))?;
    (spec.note_reference && !spec.sequence_separator).then_some(spec.target)
}
