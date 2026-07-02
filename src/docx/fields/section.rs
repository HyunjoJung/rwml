use super::*;

#[derive(Debug, Clone, Default)]
pub(crate) struct SectionContext {
    field_sections: Vec<usize>,
    section_page_counts: Vec<Option<usize>>,
}

impl SectionContext {
    pub(crate) fn empty() -> Self {
        Self::default()
    }

    pub(crate) fn field_position(&self, index: usize) -> Option<SectionFieldPosition> {
        let section = self.field_sections.get(index).copied()?;
        Some(SectionFieldPosition {
            section,
            section_pages: self
                .section_page_counts
                .get(section.saturating_sub(1))
                .copied()
                .flatten(),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SectionFieldPosition {
    section: usize,
    section_pages: Option<usize>,
}

#[derive(Debug, Clone)]
struct SectionScanField {
    instruction: String,
    phase: FieldPhase,
}

pub(crate) fn section_context(xml: &str) -> SectionContext {
    let mut r = Reader::from_str(xml);
    let mut field_sections = Vec::new();
    let mut section_page_counts = Vec::new();
    let mut current_section = 1usize;
    let mut current_page = 1usize;
    let mut section_start_page = 1usize;
    let mut section_has_visible_content = false;
    let mut paragraph_properties_depth = 0usize;
    let mut section_break_pending = false;
    let mut section_break_kind = Some(PageRefSectionBreak::Next);
    let mut section_type_seen = false;
    let mut section_properties_depth = 0usize;
    let mut current: Option<SectionScanField> = None;
    let mut simple_section_field_result_depth: Option<usize> = None;
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
                    b"pPr" => paragraph_properties_depth += 1,
                    b"sectPr" if paragraph_properties_depth > 0 => {
                        section_properties_depth += 1;
                        section_break_pending = true;
                        section_break_kind = Some(PageRefSectionBreak::Next);
                        section_type_seen = false;
                    }
                    b"type" if section_properties_depth > 0 && !section_type_seen => {
                        section_type_seen = true;
                        section_break_kind = page_ref_section_break(&e);
                    }
                    b"pageBreakBefore"
                        if paragraph_properties_depth > 0 && page_ref_on_off_enabled(&e) =>
                    {
                        current_page += 1;
                    }
                    b"fldSimple" => {
                        if record_section_field(
                            attr_local(&e, b"instr").as_deref(),
                            current_section,
                            &mut field_sections,
                        ) {
                            simple_section_field_result_depth = Some(xml_depth + 1);
                        }
                    }
                    b"fldChar" => {
                        apply_section_scan_fld_char(
                            &e,
                            current_section,
                            &mut current,
                            &mut field_sections,
                        );
                    }
                    b"instrText" => {
                        let text = read_text(&mut r);
                        consumed_element = true;
                        if let Some(field) = current.as_mut() {
                            if field.phase == FieldPhase::Instruction {
                                field.instruction.push_str(&text);
                            }
                        }
                    }
                    b"t" => {
                        consumed_element = true;
                        if !read_text(&mut r).is_empty() {
                            mark_visible_section_content(
                                &mut section_has_visible_content,
                                &current,
                                simple_section_field_result_depth,
                            );
                        }
                    }
                    b"br" if is_page_break_type(&e) => {
                        current_page += 1;
                    }
                    b"lastRenderedPageBreak" => {
                        current_page += 1;
                    }
                    b"sym" if is_supported_run_symbol(&e) => {
                        mark_visible_section_content(
                            &mut section_has_visible_content,
                            &current,
                            simple_section_field_result_depth,
                        );
                    }
                    _ if is_visible_reference_mark(name) => {
                        mark_visible_section_content(
                            &mut section_has_visible_content,
                            &current,
                            simple_section_field_result_depth,
                        );
                    }
                    b"tab" | b"cr" | b"noBreakHyphen" | b"softHyphen" | b"drawing" | b"pict"
                    | b"object" => {
                        mark_visible_section_content(
                            &mut section_has_visible_content,
                            &current,
                            simple_section_field_result_depth,
                        );
                    }
                    b"br" => {
                        mark_visible_section_content(
                            &mut section_has_visible_content,
                            &current,
                            simple_section_field_result_depth,
                        );
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
                    b"sectPr" if paragraph_properties_depth > 0 => {
                        section_break_pending = true;
                        section_break_kind = Some(PageRefSectionBreak::Next);
                        section_type_seen = false;
                    }
                    b"type" if section_properties_depth > 0 && !section_type_seen => {
                        section_type_seen = true;
                        section_break_kind = page_ref_section_break(&e);
                    }
                    b"pageBreakBefore"
                        if paragraph_properties_depth > 0 && page_ref_on_off_enabled(&e) =>
                    {
                        current_page += 1;
                    }
                    b"fldSimple" => {
                        record_section_field(
                            attr_local(&e, b"instr").as_deref(),
                            current_section,
                            &mut field_sections,
                        );
                    }
                    b"fldChar" => {
                        apply_section_scan_fld_char(
                            &e,
                            current_section,
                            &mut current,
                            &mut field_sections,
                        );
                    }
                    b"br" if is_page_break_type(&e) => {
                        current_page += 1;
                    }
                    b"lastRenderedPageBreak" => {
                        current_page += 1;
                    }
                    b"sym" if is_supported_run_symbol(&e) => {
                        mark_visible_section_content(
                            &mut section_has_visible_content,
                            &current,
                            simple_section_field_result_depth,
                        );
                    }
                    _ if is_visible_reference_mark(name) => {
                        mark_visible_section_content(
                            &mut section_has_visible_content,
                            &current,
                            simple_section_field_result_depth,
                        );
                    }
                    b"tab" | b"br" | b"cr" | b"noBreakHyphen" | b"softHyphen" | b"drawing"
                    | b"pict" | b"object" => {
                        mark_visible_section_content(
                            &mut section_has_visible_content,
                            &current,
                            simple_section_field_result_depth,
                        );
                    }
                    _ => {}
                }
            }
            Ok(Event::End(e)) => {
                let qname = e.name();
                let name = local(qname.as_ref());
                if name == b"AlternateContent" {
                    alternate_content_stack.pop();
                }
                if name == b"sectPr" {
                    section_properties_depth = section_properties_depth.saturating_sub(1);
                }
                if name == b"pPr" {
                    paragraph_properties_depth = paragraph_properties_depth.saturating_sub(1);
                }
                if name == b"fldSimple" && simple_section_field_result_depth == Some(xml_depth) {
                    simple_section_field_result_depth = None;
                }
                if name == b"p" && section_break_pending {
                    push_section_page_count(
                        &mut section_page_counts,
                        current_page,
                        section_start_page,
                        section_has_visible_content,
                    );
                    if let Some(section_break) = section_break_kind {
                        current_page = page_after_section_break(current_page, section_break);
                    }
                    current_section += 1;
                    section_start_page = current_page;
                    section_has_visible_content = false;
                    section_break_pending = false;
                    section_break_kind = Some(PageRefSectionBreak::Next);
                    section_type_seen = false;
                }
                xml_depth = xml_depth.saturating_sub(1);
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    push_section_page_count(
        &mut section_page_counts,
        current_page,
        section_start_page,
        section_has_visible_content,
    );
    SectionContext {
        field_sections,
        section_page_counts,
    }
}

fn push_section_page_count(
    section_page_counts: &mut Vec<Option<usize>>,
    current_page: usize,
    section_start_page: usize,
    section_has_visible_content: bool,
) {
    section_page_counts.push(
        (!section_has_visible_content)
            .then_some(current_page.saturating_sub(section_start_page) + 1),
    );
}

fn record_section_field(
    instruction: Option<&str>,
    current_section: usize,
    field_sections: &mut Vec<usize>,
) -> bool {
    let is_section_field = instruction
        .map(normalize_instruction)
        .as_deref()
        .and_then(section_instruction)
        .is_some();
    if is_section_field {
        field_sections.push(current_section);
    }
    is_section_field
}

fn section_field_result_content_is_hidden(
    current: &Option<SectionScanField>,
    simple_section_field_result_depth: Option<usize>,
) -> bool {
    simple_section_field_result_depth.is_some()
        || current.as_ref().is_some_and(|field| {
            field.phase == FieldPhase::Result && is_section_scan_instruction(&field.instruction)
        })
}

fn mark_visible_section_content(
    section_has_visible_content: &mut bool,
    current: &Option<SectionScanField>,
    simple_section_field_result_depth: Option<usize>,
) {
    if !section_field_result_content_is_hidden(current, simple_section_field_result_depth) {
        *section_has_visible_content = true;
    }
}

fn is_section_scan_instruction(instruction: &str) -> bool {
    let instruction = normalize_instruction(instruction);
    section_instruction(&instruction).is_some()
}

fn apply_section_scan_fld_char(
    e: &BytesStart<'_>,
    current_section: usize,
    current: &mut Option<SectionScanField>,
    field_sections: &mut Vec<usize>,
) {
    match field_char_type(e).as_deref() {
        Some("begin") => {
            *current = Some(SectionScanField {
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
                record_section_field(Some(&field.instruction), current_section, field_sections);
            }
        }
        _ => {}
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SectionInstruction {
    result: SectionResult,
    number_format: Option<PageNumberFormat>,
    text_format: Option<FieldTextFormat>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SectionResult {
    Number,
    Pages,
}

pub(crate) fn is_section_field_instruction(instruction: &str) -> bool {
    section_instruction(instruction).is_some()
}

pub(crate) fn computed_section_result(
    instruction: &str,
    position: Option<SectionFieldPosition>,
) -> Option<String> {
    let spec = section_instruction(instruction)?;
    let position = position?;
    let value = match spec.result {
        SectionResult::Number => position.section,
        SectionResult::Pages => position.section_pages?,
    };
    Some(apply_field_text_format(
        format_page_number(value, spec.number_format)?,
        spec.text_format,
    ))
}

fn section_instruction(instruction: &str) -> Option<SectionInstruction> {
    let tokens = instruction_parts(instruction);
    let mut parts = tokens.iter().map(String::as_str);
    let kind = parts.next()?;
    let result = if kind.eq_ignore_ascii_case("SECTION") {
        SectionResult::Number
    } else if kind.eq_ignore_ascii_case("SECTIONPAGES") {
        SectionResult::Pages
    } else {
        return None;
    };
    let format = page_field_format_syntax_tail(&mut parts)?;
    Some(SectionInstruction {
        result,
        number_format: format
            .number_format
            .map(page_number_format_from_field_format),
        text_format: format.text_format,
    })
}
