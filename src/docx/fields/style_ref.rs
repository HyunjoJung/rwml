use super::*;

#[derive(Debug, Clone, Default)]
pub(crate) struct StyleRefContext {
    entries: Vec<StyleRefEntry>,
    field_positions: Vec<StyleRefFieldPosition>,
}

impl StyleRefContext {
    pub(crate) fn empty() -> Self {
        Self::default()
    }

    pub(crate) fn field_position(&self, index: usize) -> Option<StyleRefFieldPosition> {
        self.field_positions.get(index).cloned()
    }

    fn entry_for_style(
        &self,
        style_identifier: &str,
        field_order: usize,
    ) -> Option<&StyleRefEntry> {
        self.entries
            .iter()
            .rev()
            .find(|entry| {
                entry.order < field_order && style_ref_entry_matches(entry, style_identifier)
            })
            .or_else(|| {
                self.entries.iter().find(|entry| {
                    entry.order > field_order && style_ref_entry_matches(entry, style_identifier)
                })
            })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct StyleRefEntry {
    style_id: Option<String>,
    style_name: Option<String>,
    text: String,
    number_text: Option<String>,
    number_numeric: Option<String>,
    number_full_context: Option<String>,
    order: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StyleRefFieldPosition {
    order: usize,
    number_context: Option<String>,
}

#[derive(Debug, Clone)]
struct StyleRefScanField {
    instruction: String,
    phase: FieldPhase,
}

#[derive(Debug, Clone)]
struct StyleRefParagraphNumber {
    text: String,
    numeric: Option<String>,
    full_context: Option<String>,
}

pub(crate) fn style_ref_context(
    xml: &str,
    styles: &Styles,
    numbering: &Numbering,
) -> StyleRefContext {
    let mut r = Reader::from_str(xml);
    let mut entries = Vec::new();
    let mut field_positions = Vec::new();
    let mut counters: HashMap<String, [u32; 9]> = HashMap::new();
    let mut next_order = 0usize;
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
                let mut consumed_element = false;
                match name {
                    b"AlternateContent" => {
                        alternate_content_stack.push(AlternateContentBranchState {
                            branch_depth: xml_depth + 1,
                            took_branch: false,
                        });
                    }
                    b"p" => {
                        read_style_ref_paragraph(
                            &mut r,
                            styles,
                            numbering,
                            &mut counters,
                            &mut entries,
                            &mut field_positions,
                            &mut next_order,
                        );
                        consumed_element = true;
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
            }
            Ok(Event::End(e)) => {
                let qname = e.name();
                let name = local(qname.as_ref());
                if name == b"AlternateContent" {
                    alternate_content_stack.pop();
                }
                xml_depth = xml_depth.saturating_sub(1);
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    StyleRefContext {
        entries,
        field_positions,
    }
}

fn read_style_ref_paragraph(
    r: &mut Xml<'_>,
    styles: &Styles,
    numbering: &Numbering,
    counters: &mut HashMap<String, [u32; 9]>,
    entries: &mut Vec<StyleRefEntry>,
    field_positions: &mut Vec<StyleRefFieldPosition>,
    next_order: &mut usize,
) {
    let mut style_id = None;
    let mut num_id = None;
    let mut ilvl = 0u8;
    let mut number = None;
    let mut text = String::new();
    let mut current: Option<StyleRefScanField> = None;
    let mut xml_depth = 0usize;
    let mut alternate_content_stack = Vec::new();
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) => {
                let qname = e.name();
                let name = local(qname.as_ref());
                if should_skip_alternate_branch(&mut alternate_content_stack, xml_depth, name) {
                    skip_subtree(r);
                    continue;
                }
                if matches!(name, b"del" | b"moveFrom") {
                    skip_subtree(r);
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
                    b"pPr" => {
                        read_style_ref_ppr(r, &mut style_id, &mut num_id, &mut ilvl);
                        consumed_element = true;
                    }
                    b"r" => {
                        ensure_style_ref_paragraph_number(
                            &mut number,
                            numbering,
                            counters,
                            num_id.as_deref(),
                            ilvl,
                        );
                        read_style_ref_run(
                            r,
                            StyleRefRunScan {
                                styles,
                                entries,
                                field_positions,
                                next_order,
                                paragraph_number: &number,
                                current: &mut current,
                                paragraph_text: &mut text,
                            },
                        );
                        consumed_element = true;
                    }
                    b"fldSimple" => {
                        ensure_style_ref_paragraph_number(
                            &mut number,
                            numbering,
                            counters,
                            num_id.as_deref(),
                            ilvl,
                        );
                        let instruction = attr_local(&e, b"instr");
                        let is_style_ref = instruction
                            .as_deref()
                            .is_some_and(is_style_ref_field_instruction);
                        record_style_ref_field(
                            instruction.as_deref(),
                            field_positions,
                            next_order,
                            &number,
                        );
                        if is_style_ref {
                            skip_element(r, b"fldSimple");
                        } else {
                            text.push_str(&read_style_ref_simple_field_result(
                                r,
                                styles,
                                entries,
                                field_positions,
                                next_order,
                                &number,
                            ));
                        }
                        consumed_element = true;
                    }
                    b"t" => {
                        let run_text = read_text(r);
                        consumed_element = true;
                        if !style_ref_suppresses_source_text(&current) {
                            text.push_str(&run_text);
                        }
                    }
                    _ => {
                        if let Some(marker) = inline_marker_text(&e) {
                            if !style_ref_suppresses_source_text(&current) {
                                text.push_str(marker);
                            }
                        }
                    }
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
                    b"fldSimple" => {
                        ensure_style_ref_paragraph_number(
                            &mut number,
                            numbering,
                            counters,
                            num_id.as_deref(),
                            ilvl,
                        );
                        record_style_ref_field(
                            attr_local(&e, b"instr").as_deref(),
                            field_positions,
                            next_order,
                            &number,
                        );
                    }
                    _ => {
                        if let Some(marker) = inline_marker_text(&e) {
                            if !style_ref_suppresses_source_text(&current) {
                                text.push_str(marker);
                            }
                        }
                    }
                }
            }
            Ok(Event::End(e)) if local(e.name().as_ref()) == b"p" => break,
            Ok(Event::End(e)) => {
                let qname = e.name();
                let name = local(qname.as_ref());
                if name == b"AlternateContent" {
                    alternate_content_stack.pop();
                }
                xml_depth = xml_depth.saturating_sub(1);
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    let style_name = style_id
        .as_deref()
        .and_then(|style_id| styles.name(style_id))
        .map(str::to_string);
    let text = normalize_toc_text(&text);
    ensure_style_ref_paragraph_number(&mut number, numbering, counters, num_id.as_deref(), ilvl);
    if !text.is_empty() && (style_id.is_some() || style_name.is_some()) {
        entries.push(StyleRefEntry {
            style_id,
            style_name,
            text,
            number_text: number.as_ref().map(|number| number.text.clone()),
            number_numeric: number.as_ref().and_then(|number| number.numeric.clone()),
            number_full_context: number.and_then(|number| number.full_context),
            order: take_style_ref_order(next_order),
        });
    }
}

struct StyleRefRunScan<'a> {
    styles: &'a Styles,
    entries: &'a mut Vec<StyleRefEntry>,
    field_positions: &'a mut Vec<StyleRefFieldPosition>,
    next_order: &'a mut usize,
    paragraph_number: &'a Option<StyleRefParagraphNumber>,
    current: &'a mut Option<StyleRefScanField>,
    paragraph_text: &'a mut String,
}

fn read_style_ref_run(r: &mut Xml<'_>, scan: StyleRefRunScan<'_>) {
    let StyleRefRunScan {
        styles,
        entries,
        field_positions,
        next_order,
        paragraph_number,
        current,
        paragraph_text,
    } = scan;
    let mut run_style_id = None;
    let mut run_text = String::new();
    let mut xml_depth = 0usize;
    let mut alternate_content_stack = Vec::new();
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) => {
                let qname = e.name();
                let name = local(qname.as_ref());
                if should_skip_alternate_branch(&mut alternate_content_stack, xml_depth, name) {
                    skip_subtree(r);
                    continue;
                }
                if matches!(name, b"del" | b"moveFrom" | b"rPrChange") {
                    skip_subtree(r);
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
                    b"rStyle" => {
                        run_style_id = attr_local_trimmed(&e, b"val");
                    }
                    b"fldChar" => {
                        apply_style_ref_scan_fld_char(
                            &e,
                            current,
                            field_positions,
                            next_order,
                            paragraph_number,
                        );
                    }
                    b"instrText" => {
                        let field_text = read_text(r);
                        consumed_element = true;
                        if let Some(field) = current.as_mut() {
                            if field.phase == FieldPhase::Instruction {
                                field.instruction.push_str(&field_text);
                            }
                        }
                    }
                    b"t" => {
                        let text = read_text(r);
                        consumed_element = true;
                        if !style_ref_suppresses_source_text(current) {
                            paragraph_text.push_str(&text);
                            run_text.push_str(&text);
                        }
                    }
                    _ => {
                        if let Some(marker) = inline_marker_text(&e) {
                            if !style_ref_suppresses_source_text(current) {
                                paragraph_text.push_str(marker);
                                run_text.push_str(marker);
                            }
                        }
                    }
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
                    b"rStyle" => {
                        run_style_id = attr_local_trimmed(&e, b"val");
                    }
                    b"fldChar" => {
                        apply_style_ref_scan_fld_char(
                            &e,
                            current,
                            field_positions,
                            next_order,
                            paragraph_number,
                        );
                    }
                    _ => {
                        if let Some(marker) = inline_marker_text(&e) {
                            if !style_ref_suppresses_source_text(current) {
                                paragraph_text.push_str(marker);
                                run_text.push_str(marker);
                            }
                        }
                    }
                }
            }
            Ok(Event::End(e)) if local(e.name().as_ref()) == b"r" => break,
            Ok(Event::End(e)) => {
                let qname = e.name();
                let name = local(qname.as_ref());
                if name == b"AlternateContent" {
                    alternate_content_stack.pop();
                }
                xml_depth = xml_depth.saturating_sub(1);
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    let text = normalize_toc_text(&run_text);
    if !text.is_empty() && run_style_id.is_some() {
        let style_name = run_style_id
            .as_deref()
            .and_then(|style_id| styles.name(style_id))
            .map(str::to_string);
        entries.push(StyleRefEntry {
            style_id: run_style_id,
            style_name,
            text,
            number_text: paragraph_number.as_ref().map(|number| number.text.clone()),
            number_numeric: paragraph_number
                .as_ref()
                .and_then(|number| number.numeric.clone()),
            number_full_context: paragraph_number
                .as_ref()
                .and_then(|number| number.full_context.clone()),
            order: take_style_ref_order(next_order),
        });
    }
}

fn read_style_ref_simple_field_result(
    r: &mut Xml<'_>,
    styles: &Styles,
    entries: &mut Vec<StyleRefEntry>,
    field_positions: &mut Vec<StyleRefFieldPosition>,
    next_order: &mut usize,
    paragraph_number: &Option<StyleRefParagraphNumber>,
) -> String {
    let mut text = String::new();
    let mut current: Option<StyleRefScanField> = None;
    let mut depth = 1usize;
    let mut xml_depth = 0usize;
    let mut alternate_content_stack = Vec::new();
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) => {
                let qname = e.name();
                let name = local(qname.as_ref());
                if should_skip_alternate_branch(&mut alternate_content_stack, xml_depth, name) {
                    skip_subtree(r);
                    continue;
                }
                if matches!(name, b"del" | b"moveFrom") {
                    skip_subtree(r);
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
                    b"r" => {
                        read_style_ref_run(
                            r,
                            StyleRefRunScan {
                                styles,
                                entries,
                                field_positions,
                                next_order,
                                paragraph_number,
                                current: &mut current,
                                paragraph_text: &mut text,
                            },
                        );
                        consumed_element = true;
                    }
                    b"fldSimple"
                        if attr_local(&e, b"instr")
                            .as_deref()
                            .is_some_and(is_style_ref_field_instruction) =>
                    {
                        skip_element(r, b"fldSimple");
                        consumed_element = true;
                    }
                    b"instrText" => {
                        read_text(r);
                        consumed_element = true;
                    }
                    b"t" => {
                        text.push_str(&read_text(r));
                        consumed_element = true;
                    }
                    _ => {
                        if let Some(marker) = inline_marker_text(&e) {
                            text.push_str(marker);
                        }
                    }
                }
                if !consumed_element {
                    depth += 1;
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
                    b"fldSimple"
                        if attr_local(&e, b"instr")
                            .as_deref()
                            .is_some_and(is_style_ref_field_instruction) => {}
                    _ => {
                        if let Some(marker) = inline_marker_text(&e) {
                            text.push_str(marker);
                        }
                    }
                }
            }
            Ok(Event::End(e)) => {
                let qname = e.name();
                let name = local(qname.as_ref());
                if name == b"fldSimple" && depth == 1 {
                    break;
                }
                if name == b"AlternateContent" {
                    alternate_content_stack.pop();
                }
                depth = depth.saturating_sub(1);
                xml_depth = xml_depth.saturating_sub(1);
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    text
}

fn ensure_style_ref_paragraph_number(
    number: &mut Option<StyleRefParagraphNumber>,
    numbering: &Numbering,
    counters: &mut HashMap<String, [u32; 9]>,
    num_id: Option<&str>,
    ilvl: u8,
) {
    if number.is_none() {
        *number = style_ref_paragraph_number(numbering, counters, num_id, ilvl);
    }
}

fn take_style_ref_order(next_order: &mut usize) -> usize {
    let order = *next_order;
    *next_order += 1;
    order
}

fn read_style_ref_ppr(
    r: &mut Xml<'_>,
    style_id: &mut Option<String>,
    num_id: &mut Option<String>,
    ilvl: &mut u8,
) {
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) if local(e.name().as_ref()) == b"pPrChange" => skip_subtree(r),
            Ok(Event::Start(e)) => match local(e.name().as_ref()) {
                b"pStyle" => *style_id = attr_local_trimmed(&e, b"val"),
                b"ilvl" => {
                    if let Some(value) = attr_u8(&e, b"val") {
                        *ilvl = value;
                    }
                }
                b"numId" => *num_id = attr_local_trimmed(&e, b"val"),
                _ => {}
            },
            Ok(Event::Empty(e)) => match local(e.name().as_ref()) {
                b"pStyle" => *style_id = attr_local_trimmed(&e, b"val"),
                b"ilvl" => {
                    if let Some(value) = attr_u8(&e, b"val") {
                        *ilvl = value;
                    }
                }
                b"numId" => *num_id = attr_local_trimmed(&e, b"val"),
                _ => {}
            },
            Ok(Event::End(e)) if local(e.name().as_ref()) == b"pPr" => break,
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
}

fn style_ref_paragraph_number(
    numbering: &Numbering,
    counters: &mut HashMap<String, [u32; 9]>,
    num_id: Option<&str>,
    ilvl: u8,
) -> Option<StyleRefParagraphNumber> {
    let num_id = num_id.filter(|num_id| *num_id != "0")?;
    let counter = counters.entry(num_id.to_string()).or_insert([0; 9]);
    let label = numbering.label(num_id, ilvl, counter)?;
    let text = ref_paragraph_number(&label)?;
    let full_context = numbering
        .full_context_label(num_id, ilvl, counter)
        .and_then(|label| ref_paragraph_number(&label));
    Some(StyleRefParagraphNumber {
        numeric: ref_numeric_paragraph_number(&text),
        text,
        full_context,
    })
}

fn style_ref_suppresses_source_text(current: &Option<StyleRefScanField>) -> bool {
    current.as_ref().is_some_and(|field| {
        field.phase == FieldPhase::Result && is_style_ref_field_instruction(&field.instruction)
    })
}

fn record_style_ref_field(
    instruction: Option<&str>,
    field_positions: &mut Vec<StyleRefFieldPosition>,
    next_order: &mut usize,
    paragraph_number: &Option<StyleRefParagraphNumber>,
) {
    if instruction.is_some_and(is_style_ref_field_instruction) {
        field_positions.push(StyleRefFieldPosition {
            order: take_style_ref_order(next_order),
            number_context: paragraph_number
                .as_ref()
                .and_then(|number| number.full_context.clone()),
        });
    }
}

fn apply_style_ref_scan_fld_char(
    e: &BytesStart<'_>,
    current: &mut Option<StyleRefScanField>,
    field_positions: &mut Vec<StyleRefFieldPosition>,
    next_order: &mut usize,
    paragraph_number: &Option<StyleRefParagraphNumber>,
) {
    match field_char_type(e).as_deref() {
        Some("begin") => {
            *current = Some(StyleRefScanField {
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
                record_style_ref_field(
                    Some(&field.instruction),
                    field_positions,
                    next_order,
                    paragraph_number,
                );
            }
        }
        _ => {}
    }
}

pub(crate) fn is_style_ref_field_instruction(instruction: &str) -> bool {
    instruction_parts(instruction)
        .first()
        .is_some_and(|kind| kind.eq_ignore_ascii_case("STYLEREF"))
}

fn style_ref_entry_matches(entry: &StyleRefEntry, style_identifier: &str) -> bool {
    entry
        .style_id
        .as_ref()
        .is_some_and(|style_id| style_id.eq_ignore_ascii_case(style_identifier))
        || entry
            .style_name
            .as_ref()
            .is_some_and(|style_name| style_name.eq_ignore_ascii_case(style_identifier))
}

pub(crate) fn computed_style_ref_result(
    instruction: &str,
    style_refs: &StyleRefContext,
    field_position: Option<StyleRefFieldPosition>,
) -> Option<String> {
    let spec = style_ref_instruction(instruction)?;
    let field_position = field_position?;
    let entry = style_refs.entry_for_style(&spec.style_identifier, field_position.order)?;
    let text = match spec.result {
        StyleRefResult::Text => entry.text.clone(),
        StyleRefResult::ParagraphNumber if spec.suppress_non_numeric => {
            entry.number_numeric.clone()?
        }
        StyleRefResult::ParagraphNumber => entry.number_text.clone()?,
        StyleRefResult::RelativeContextNumber => {
            let target = entry.number_full_context.as_deref()?;
            let field = field_position.number_context.as_deref()?;
            relative_context_ref_number(target, field)
        }
        StyleRefResult::FullContextNumber => entry.number_full_context.clone()?,
        StyleRefResult::RelativePosition => {
            if entry.order < field_position.order {
                "above".to_string()
            } else if entry.order > field_position.order {
                "below".to_string()
            } else {
                return None;
            }
        }
    };
    Some(apply_field_text_format(text, spec.text_format))
}

pub(crate) fn supports_style_ref_field_syntax(instruction: &str) -> bool {
    style_ref_instruction(instruction).is_some()
}

pub(super) fn style_ref_instruction(instruction: &str) -> Option<StyleRefInstruction> {
    style_ref_field_syntax(instruction)
}

pub(super) type StyleRefInstruction = StyleRefFieldSyntax;
