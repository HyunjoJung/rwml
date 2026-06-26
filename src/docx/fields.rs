//! `.docx` field marker parsing.

use std::collections::HashMap;

use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;

use crate::annotation::{Field, FieldKind};
use crate::{numfmt, CoreProperties};

use super::numbering::Numbering;
use super::styles::Styles;
use super::{attr_local, local};

type Xml<'a> = Reader<&'a [u8]>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FieldPhase {
    Instruction,
    Result,
}

#[derive(Debug, Clone)]
struct ComplexField {
    instruction: String,
    result: String,
    phase: FieldPhase,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TocEntry {
    pub level: u8,
    pub text: String,
    source: TocEntrySource,
    tc_type: Option<String>,
    sequence_identifier: Option<String>,
    sequence_caption_text: Option<String>,
    bookmarks: Vec<String>,
    style_id: Option<String>,
    style_name: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TocEntrySource {
    HeadingStyle,
    OutlineLevel,
    StyledParagraph,
    TcField,
    SequenceField,
}

pub(crate) fn parse(
    xml: &str,
    styles: &Styles,
    toc_entries: &[TocEntry],
    numbering: &Numbering,
    properties: FieldDocumentProperties<'_>,
) -> Vec<Field> {
    let bookmarks = ref_targets(xml);
    let ref_positions = ref_position_context(xml, numbering);
    let ref_numbers = ref_number_context(xml, numbering);
    let page_refs = page_ref_context(xml);
    let note_refs = note_ref_context(xml);
    let sections = section_context(xml);
    let style_refs = style_ref_context(xml, styles, numbering);
    let legacy_forms = legacy_form_context(xml);
    let table_formulas = table_formula_context(xml);
    let mut r = Reader::from_str(xml);
    let mut fields = Vec::new();
    let mut current = Vec::new();
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
                    b"fldSimple" => {
                        record_simple_field(
                            read_simple_field(&mut r, &e),
                            &mut current,
                            &mut fields,
                        );
                        consumed_element = true;
                    }
                    b"fldChar" => {
                        apply_fld_char(&e, &mut current, &mut fields);
                    }
                    b"instrText" => {
                        let text = read_text(&mut r);
                        consumed_element = true;
                        if let Some(field) = current.last_mut() {
                            if field.phase == FieldPhase::Instruction {
                                field.instruction.push_str(&text);
                            }
                        }
                    }
                    b"t" => {
                        let text = read_text(&mut r);
                        consumed_element = true;
                        if let Some(field) = current.last_mut() {
                            if field.phase == FieldPhase::Result {
                                field.result.push_str(&text);
                            }
                        }
                    }
                    _ => {
                        if let Some(text) = inline_marker_text(&e) {
                            if let Some(field) = current.last_mut() {
                                if field.phase == FieldPhase::Result {
                                    field.result.push_str(text);
                                }
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
                        record_simple_field(
                            make_field(attr_local(&e, b"instr").unwrap_or_default(), String::new()),
                            &mut current,
                            &mut fields,
                        );
                    }
                    b"fldChar" => {
                        apply_fld_char(&e, &mut current, &mut fields);
                    }
                    _ => {
                        if let Some(text) = inline_marker_text(&e) {
                            if let Some(field) = current.last_mut() {
                                if field.phase == FieldPhase::Result {
                                    field.result.push_str(text);
                                }
                            }
                        }
                    }
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
    apply_computed_results(
        &mut fields,
        ComputedResultContexts {
            bookmarks: &bookmarks,
            ref_positions: &ref_positions,
            ref_numbers: &ref_numbers,
            page_refs: &page_refs,
            note_refs: &note_refs,
            sections: &sections,
            style_refs: &style_refs,
            legacy_forms: &legacy_forms,
            table_formulas: &table_formulas,
            toc_entries,
            core_properties: properties.core,
            custom_properties: properties.custom,
            document_variables: properties.variables,
            extended_properties: properties.extended,
            file_size_bytes: properties.file_size_bytes,
        },
    );
    fields
}

#[derive(Clone, Copy)]
pub(crate) struct FieldDocumentProperties<'a> {
    pub(crate) core: &'a CoreProperties,
    pub(crate) custom: &'a HashMap<String, String>,
    pub(crate) variables: &'a HashMap<String, String>,
    pub(crate) extended: &'a HashMap<String, String>,
    pub(crate) file_size_bytes: Option<usize>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct TableFormulaContext {
    results: Vec<Option<String>>,
}

impl TableFormulaContext {
    pub(crate) fn empty() -> Self {
        Self::default()
    }

    pub(crate) fn field_result(&self, index: usize) -> Option<String> {
        self.results.get(index).and_then(Clone::clone)
    }
}

pub(crate) fn table_formula_context(xml: &str) -> TableFormulaContext {
    let mut r = Reader::from_str(xml);
    let mut results = Vec::new();
    let mut current = Vec::new();
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
                    b"tbl" => {
                        results.extend(read_table_formula_table(&mut r));
                        consumed_element = true;
                    }
                    b"fldSimple" => {
                        if is_formula_instruction(attr_local(&e, b"instr").as_deref()) {
                            results.push(None);
                        }
                        skip_element(&mut r, b"fldSimple");
                        consumed_element = true;
                    }
                    b"fldChar" => {
                        apply_table_formula_scan_fld_char(&e, &mut current, |_, _| {
                            results.push(None)
                        });
                    }
                    b"instrText" => {
                        let text = read_text(&mut r);
                        consumed_element = true;
                        if let Some(field) = current.last_mut() {
                            if field.phase == FieldPhase::Instruction {
                                field.instruction.push_str(&text);
                            }
                        }
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
                    b"fldSimple" if is_formula_instruction(attr_local(&e, b"instr").as_deref()) => {
                        results.push(None);
                    }
                    b"fldChar" => {
                        apply_table_formula_scan_fld_char(&e, &mut current, |_, _| {
                            results.push(None)
                        });
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
    TableFormulaContext { results }
}

#[derive(Debug, Clone)]
enum TableFormulaRecord {
    Nested(Option<String>),
    Local {
        row: usize,
        col: usize,
        instruction: String,
    },
}

#[derive(Debug, Clone, Default)]
struct TableFormulaCell {
    text: String,
    contains_formula: bool,
    has_span: bool,
    records: Vec<TableFormulaRecord>,
}

fn read_table_formula_table(r: &mut Xml<'_>) -> Vec<Option<String>> {
    let mut rows = Vec::new();
    let mut records = Vec::new();
    let mut xml_depth = 1usize;
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
                match name {
                    b"AlternateContent" => {
                        alternate_content_stack.push(AlternateContentBranchState {
                            branch_depth: xml_depth + 1,
                            took_branch: false,
                        });
                        xml_depth = xml_depth.saturating_add(1);
                    }
                    b"tr" => {
                        let row_index = rows.len();
                        let mut row = read_table_formula_row(r, row_index);
                        for cell in &mut row {
                            records.append(&mut cell.records);
                        }
                        rows.push(row);
                    }
                    _ => skip_subtree(r),
                }
            }
            Ok(Event::End(e)) => {
                let qname = e.name();
                let name = local(qname.as_ref());
                if name == b"tbl" {
                    break;
                }
                if name == b"AlternateContent" {
                    alternate_content_stack.pop();
                }
                xml_depth = xml_depth.saturating_sub(1);
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    let has_spans = rows.iter().flatten().any(|cell| cell.has_span);
    records
        .into_iter()
        .map(|record| match record {
            TableFormulaRecord::Nested(result) => result,
            TableFormulaRecord::Local {
                row,
                col,
                instruction,
            } if !has_spans => computed_table_formula_result(&instruction, &rows, row, col),
            TableFormulaRecord::Local { .. } => None,
        })
        .collect()
}

fn read_table_formula_row(r: &mut Xml<'_>, row_index: usize) -> Vec<TableFormulaCell> {
    let mut row = Vec::new();
    let mut xml_depth = 1usize;
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
                match name {
                    b"AlternateContent" => {
                        alternate_content_stack.push(AlternateContentBranchState {
                            branch_depth: xml_depth + 1,
                            took_branch: false,
                        });
                        xml_depth = xml_depth.saturating_add(1);
                    }
                    b"tc" => {
                        let col_index = row.len();
                        row.push(read_table_formula_cell(r, row_index, col_index));
                    }
                    _ => skip_subtree(r),
                }
            }
            Ok(Event::Empty(e)) if local(e.name().as_ref()) == b"tc" => {
                row.push(TableFormulaCell::default());
            }
            Ok(Event::End(e)) => {
                let qname = e.name();
                let name = local(qname.as_ref());
                if name == b"tr" {
                    break;
                }
                if name == b"AlternateContent" {
                    alternate_content_stack.pop();
                }
                xml_depth = xml_depth.saturating_sub(1);
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    row
}

fn read_table_formula_cell(r: &mut Xml<'_>, row: usize, col: usize) -> TableFormulaCell {
    let mut cell = TableFormulaCell::default();
    let mut current = Vec::new();
    let mut xml_depth = 1usize;
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
                    b"tcPr" => {
                        cell.has_span |= read_table_formula_cell_props(r);
                        consumed_element = true;
                    }
                    b"tbl" => {
                        for result in read_table_formula_table(r) {
                            cell.records.push(TableFormulaRecord::Nested(result));
                        }
                        if !cell.text.is_empty() {
                            cell.text.push('\n');
                        }
                        consumed_element = true;
                    }
                    b"fldSimple" => {
                        let instruction =
                            attr_local(&e, b"instr").map(|value| normalize_instruction(&value));
                        if instruction.as_deref().is_some_and(|value| {
                            FieldKind::from_instruction(value)
                                == FieldKind::Dynamic("=".to_string())
                        }) {
                            cell.contains_formula = true;
                            cell.records.push(TableFormulaRecord::Local {
                                row,
                                col,
                                instruction: instruction.unwrap_or_default(),
                            });
                        }
                        cell.text.push_str(&read_field_result_text(r));
                        consumed_element = true;
                    }
                    b"fldChar" => {
                        apply_table_formula_scan_fld_char(&e, &mut current, |instruction, _| {
                            if FieldKind::from_instruction(instruction)
                                == FieldKind::Dynamic("=".to_string())
                            {
                                cell.contains_formula = true;
                                cell.records.push(TableFormulaRecord::Local {
                                    row,
                                    col,
                                    instruction: instruction.to_string(),
                                });
                            }
                        });
                    }
                    b"instrText" => {
                        let text = read_text(r);
                        consumed_element = true;
                        if let Some(field) = current.last_mut() {
                            if field.phase == FieldPhase::Instruction {
                                field.instruction.push_str(&text);
                            }
                        }
                    }
                    b"t" => {
                        cell.text.push_str(&read_text(r));
                        consumed_element = true;
                    }
                    _ => {
                        if let Some(text) = inline_marker_text(&e) {
                            cell.text.push_str(text);
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
                        let instruction =
                            attr_local(&e, b"instr").map(|value| normalize_instruction(&value));
                        if instruction.as_deref().is_some_and(|value| {
                            FieldKind::from_instruction(value)
                                == FieldKind::Dynamic("=".to_string())
                        }) {
                            cell.contains_formula = true;
                            cell.records.push(TableFormulaRecord::Local {
                                row,
                                col,
                                instruction: instruction.unwrap_or_default(),
                            });
                        }
                    }
                    b"fldChar" => {
                        apply_table_formula_scan_fld_char(&e, &mut current, |instruction, _| {
                            if FieldKind::from_instruction(instruction)
                                == FieldKind::Dynamic("=".to_string())
                            {
                                cell.contains_formula = true;
                                cell.records.push(TableFormulaRecord::Local {
                                    row,
                                    col,
                                    instruction: instruction.to_string(),
                                });
                            }
                        });
                    }
                    _ => {
                        if let Some(text) = inline_marker_text(&e) {
                            cell.text.push_str(text);
                        }
                    }
                }
            }
            Ok(Event::End(e)) => {
                let qname = e.name();
                let name = local(qname.as_ref());
                if name == b"tc" {
                    break;
                }
                if name == b"AlternateContent" {
                    alternate_content_stack.pop();
                }
                xml_depth = xml_depth.saturating_sub(1);
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    cell
}

fn read_table_formula_cell_props(r: &mut Xml<'_>) -> bool {
    let mut has_span = false;
    let mut xml_depth = 1usize;
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
                match name {
                    b"AlternateContent" => {
                        alternate_content_stack.push(AlternateContentBranchState {
                            branch_depth: xml_depth + 1,
                            took_branch: false,
                        });
                        xml_depth = xml_depth.saturating_add(1);
                    }
                    b"gridSpan" => {
                        has_span |= attr_local(&e, b"val").as_deref().unwrap_or("1") != "1";
                        skip_subtree(r);
                    }
                    b"vMerge" => {
                        has_span = true;
                        skip_subtree(r);
                    }
                    _ => {}
                }
            }
            Ok(Event::Empty(e)) => {
                let qname = e.name();
                let name = local(qname.as_ref());
                if should_skip_alternate_branch(&mut alternate_content_stack, xml_depth, name) {
                    continue;
                }
                match name {
                    b"gridSpan" => {
                        has_span |= attr_local(&e, b"val").as_deref().unwrap_or("1") != "1";
                    }
                    b"vMerge" => has_span = true,
                    _ => {}
                }
            }
            Ok(Event::End(e)) => {
                let qname = e.name();
                let name = local(qname.as_ref());
                if name == b"tcPr" {
                    break;
                }
                if name == b"AlternateContent" {
                    alternate_content_stack.pop();
                }
                xml_depth = xml_depth.saturating_sub(1);
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    has_span
}

fn read_field_result_text(r: &mut Xml<'_>) -> String {
    let mut text = String::new();
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) if matches!(local(e.name().as_ref()), b"del" | b"moveFrom") => {
                skip_subtree(r);
            }
            Ok(Event::Start(e)) if local(e.name().as_ref()) == b"t" => {
                text.push_str(&read_text(r));
            }
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                if let Some(marker) = inline_marker_text(&e) {
                    text.push_str(marker);
                }
            }
            Ok(Event::End(e)) if local(e.name().as_ref()) == b"fldSimple" => break,
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    text
}

fn apply_table_formula_scan_fld_char(
    e: &BytesStart<'_>,
    current: &mut Vec<ComplexField>,
    mut record: impl FnMut(&str, &str),
) {
    apply_complex_field_scan_fld_char(e, current, |field| {
        let instruction = normalize_instruction(&field.instruction);
        if is_formula_instruction(Some(&instruction)) {
            record(&instruction, &field.result);
        }
    });
}

fn is_formula_instruction(instruction: Option<&str>) -> bool {
    instruction
        .map(normalize_instruction)
        .as_deref()
        .is_some_and(|instruction| {
            FieldKind::from_instruction(instruction) == FieldKind::Dynamic("=".to_string())
        })
}

fn record_simple_field(field: Field, current: &mut [ComplexField], fields: &mut Vec<Field>) {
    if let Some(parent) = current.last_mut() {
        if parent.phase == FieldPhase::Result {
            parent.result.push_str(&field.result);
        }
    }
    fields.push(field);
}

fn read_simple_field(r: &mut Xml<'_>, start: &BytesStart<'_>) -> Field {
    let instruction = attr_local(start, b"instr").unwrap_or_default();
    let mut result = String::new();
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) if matches!(local(e.name().as_ref()), b"del" | b"moveFrom") => {
                skip_subtree(r);
            }
            Ok(Event::Start(e)) if local(e.name().as_ref()) == b"t" => {
                result.push_str(&read_text(r));
            }
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                if let Some(text) = inline_marker_text(&e) {
                    result.push_str(text);
                }
            }
            Ok(Event::End(e)) if local(e.name().as_ref()) == b"fldSimple" => break,
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    make_field(instruction, result)
}

fn apply_fld_char(e: &BytesStart<'_>, current: &mut Vec<ComplexField>, out: &mut Vec<Field>) {
    apply_complex_field_scan_fld_char(e, current, |field| {
        out.push(make_field(field.instruction, field.result));
    });
}

fn apply_complex_field_scan_fld_char(
    e: &BytesStart<'_>,
    current: &mut Vec<ComplexField>,
    mut finish: impl FnMut(ComplexField),
) {
    match attr_local(e, b"fldCharType").as_deref() {
        Some("begin") => {
            current.push(ComplexField {
                instruction: String::new(),
                result: String::new(),
                phase: FieldPhase::Instruction,
            });
        }
        Some("separate") => {
            if let Some(field) = current.last_mut() {
                field.phase = FieldPhase::Result;
            }
        }
        Some("end") => {
            if let Some(field) = current.pop() {
                if let Some(parent) = current.last_mut() {
                    if parent.phase == FieldPhase::Result {
                        parent.result.push_str(&field.result);
                    }
                }
                finish(field);
            }
        }
        _ => {}
    }
}

fn make_field(instruction: String, result: String) -> Field {
    let instruction = normalize_instruction(&instruction);
    Field {
        kind: field_kind(&instruction),
        instruction,
        result,
        computed_result: None,
    }
}

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
                        if let (Some(id), Some(name)) =
                            (attr_local(&e, b"id"), attr_local(&e, b"name"))
                        {
                            active.push((id, name));
                        }
                    }
                    b"bookmarkEnd" => {
                        if let Some(id) = attr_local(&e, b"id") {
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
                        if matches!(attr_local(&e, b"type").as_deref(), Some("page")) {
                            append_ref_text(&active, &mut out, "\u{000C}");
                        } else {
                            append_ref_text(&active, &mut out, "\n");
                        }
                    }
                    b"cr" => append_ref_text(&active, &mut out, "\n"),
                    b"noBreakHyphen" => append_ref_text(&active, &mut out, "-"),
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
                        if let (Some(id), Some(name)) =
                            (attr_local(&e, b"id"), attr_local(&e, b"name"))
                        {
                            active.push((id, name));
                        }
                    }
                    b"bookmarkEnd" => {
                        if let Some(id) = attr_local(&e, b"id") {
                            active.retain(|(active_id, _)| active_id != &id);
                        }
                    }
                    b"tab" => append_ref_text(&active, &mut out, "\t"),
                    b"br" => {
                        if matches!(attr_local(&e, b"type").as_deref(), Some("page")) {
                            append_ref_text(&active, &mut out, "\u{000C}");
                        } else {
                            append_ref_text(&active, &mut out, "\n");
                        }
                    }
                    b"cr" => append_ref_text(&active, &mut out, "\n"),
                    b"noBreakHyphen" => append_ref_text(&active, &mut out, "-"),
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

#[derive(Debug, Clone, Default)]
pub(crate) struct RefPositionContext {
    target_positions: HashMap<String, RefTargetPosition>,
    field_positions: Vec<RefFieldPosition>,
}

impl RefPositionContext {
    fn target_position(&self, name: &str) -> Option<RefTargetPosition> {
        self.target_positions.get(name).copied()
    }

    pub(crate) fn field_position(&self, index: usize) -> Option<RefFieldPosition> {
        self.field_positions.get(index).cloned()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RefTargetPosition {
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
                        if paragraph.active() {
                            paragraph.depth += 1;
                        } else {
                            paragraph.reset();
                        }
                    }
                    b"pPr" if paragraph.active() => paragraph.properties_depth += 1,
                    b"ilvl" if paragraph.properties_depth > 0 => {
                        if let Some(value) = attr_local(&e, b"val").and_then(|v| v.parse().ok()) {
                            paragraph.ilvl = value;
                        }
                    }
                    b"numId" if paragraph.properties_depth > 0 => {
                        paragraph.num_id = attr_local(&e, b"val");
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
                        if let (Some(id), Some(name)) =
                            (attr_local(&e, b"id"), attr_local(&e, b"name"))
                        {
                            active_bookmarks.push((id, name, source_order));
                            source_order += 1;
                        }
                    }
                    b"bookmarkEnd" => {
                        close_ref_position_bookmark(
                            attr_local(&e, b"id").as_deref(),
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
                    b"tab" | b"br" | b"cr" | b"noBreakHyphen" | b"drawing" | b"pict"
                    | b"object" => {
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
                        if let Some(value) = attr_local(&e, b"val").and_then(|v| v.parse().ok()) {
                            paragraph.ilvl = value;
                        }
                    }
                    b"numId" if paragraph.properties_depth > 0 => {
                        paragraph.num_id = attr_local(&e, b"val");
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
                        if let (Some(id), Some(name)) =
                            (attr_local(&e, b"id"), attr_local(&e, b"name"))
                        {
                            active_bookmarks.push((id, name, source_order));
                            source_order += 1;
                        }
                    }
                    b"bookmarkEnd" => {
                        close_ref_position_bookmark(
                            attr_local(&e, b"id").as_deref(),
                            source_order,
                            &mut active_bookmarks,
                            &mut target_positions,
                        );
                        source_order += 1;
                    }
                    b"tab" | b"br" | b"cr" | b"noBreakHyphen" | b"drawing" | b"pict"
                    | b"object" => {
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
    match attr_local(e, b"fldCharType").as_deref() {
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
                    b"p" => {
                        if paragraph.active() {
                            paragraph.depth += 1;
                        } else {
                            paragraph.reset();
                        }
                    }
                    b"pPr" if paragraph.active() => paragraph.properties_depth += 1,
                    b"ilvl" if paragraph.properties_depth > 0 => {
                        if let Some(value) = attr_local(&e, b"val").and_then(|v| v.parse().ok()) {
                            paragraph.ilvl = value;
                        }
                    }
                    b"numId" if paragraph.properties_depth > 0 => {
                        paragraph.num_id = attr_local(&e, b"val");
                    }
                    b"bookmarkStart" if paragraph.active() => {
                        if let Some(name) = attr_local(&e, b"name") {
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
                        if let Some(value) = attr_local(&e, b"val").and_then(|v| v.parse().ok()) {
                            paragraph.ilvl = value;
                        }
                    }
                    b"numId" if paragraph.properties_depth > 0 => {
                        paragraph.num_id = attr_local(&e, b"val");
                    }
                    b"bookmarkStart" if paragraph.active() => {
                        if let Some(name) = attr_local(&e, b"name") {
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

fn ref_paragraph_number(label: &str) -> Option<String> {
    let without_periods = label.trim().trim_end_matches('.').trim_end();
    (!without_periods.is_empty()).then(|| without_periods.to_string())
}

fn ref_numeric_paragraph_number(label: &str) -> Option<String> {
    let retained: String = label
        .chars()
        .filter(|ch| ch.is_ascii_digit() || matches!(ch, '.' | ',' | ':' | '-' | '/'))
        .collect();
    let numeric = retained.trim_matches(|ch: char| !ch.is_ascii_digit());
    (!numeric.is_empty()).then(|| numeric.to_string())
}

#[derive(Debug, Clone, Default)]
pub(crate) struct NoteRefContext {
    targets: HashMap<String, NoteRefTarget>,
    field_positions: Vec<NoteRefFieldPosition>,
    ref_field_positions: Vec<NoteRefFieldPosition>,
    markers: Vec<NoteRefMarker>,
    generated_ref_note_fields: Vec<NoteRefGeneratedField>,
}

impl NoteRefContext {
    pub(crate) fn empty() -> Self {
        Self::default()
    }

    fn target(&self, name: &str) -> Option<NoteRefTarget> {
        self.targets.get(name).copied()
    }

    pub(crate) fn field_position(&self, index: usize) -> Option<NoteRefFieldPosition> {
        self.field_positions.get(index).copied()
    }

    pub(crate) fn ref_field_position(&self, index: usize) -> Option<NoteRefFieldPosition> {
        self.ref_field_positions.get(index).copied()
    }

    fn ref_note_number(
        &self,
        name: &str,
        field_position: Option<NoteRefFieldPosition>,
    ) -> Option<String> {
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
        Some((actual_before + generated_before + 1).to_string())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct NoteRefTarget {
    kind: NoteRefKind,
    number: usize,
    start: usize,
    end: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NoteRefKind {
    Footnote,
    Endnote,
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

pub(crate) fn note_ref_context(xml: &str) -> NoteRefContext {
    let mut r = Reader::from_str(xml);
    let mut targets = HashMap::new();
    let mut field_positions = Vec::new();
    let mut ref_field_positions = Vec::new();
    let mut markers = Vec::new();
    let mut generated_ref_note_fields = Vec::new();
    let mut active_bookmarks: Vec<NoteRefActiveBookmark> = Vec::new();
    let mut source_order = 0usize;
    let mut footnote_number = 0usize;
    let mut endnote_number = 0usize;
    let mut current: Option<NoteRefScanField> = None;
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
                    b"fldSimple" => record_note_ref_scan_field_position(
                        attr_local(&e, b"instr").as_deref(),
                        &mut source_order,
                        &mut field_positions,
                        &mut ref_field_positions,
                        &mut generated_ref_note_fields,
                    ),
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
                        if let (Some(id), Some(name)) =
                            (attr_local(&e, b"id"), attr_local(&e, b"name"))
                        {
                            active_bookmarks.push(NoteRefActiveBookmark {
                                id,
                                name,
                                start: source_order,
                            });
                            source_order += 1;
                        }
                    }
                    b"bookmarkEnd" => {
                        close_note_ref_bookmark(
                            attr_local(&e, b"id").as_deref(),
                            source_order,
                            &mut active_bookmarks,
                            &mut targets,
                        );
                        source_order += 1;
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
                    b"t" => {
                        if !read_text(&mut r).is_empty() {
                            source_order += 1;
                        }
                        continue;
                    }
                    b"tab" | b"br" | b"cr" | b"noBreakHyphen" | b"drawing" | b"pict"
                    | b"object" => {
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
                    b"fldSimple" => record_note_ref_scan_field_position(
                        attr_local(&e, b"instr").as_deref(),
                        &mut source_order,
                        &mut field_positions,
                        &mut ref_field_positions,
                        &mut generated_ref_note_fields,
                    ),
                    b"fldChar" => apply_note_ref_scan_fld_char(
                        &e,
                        &mut source_order,
                        &mut current,
                        &mut field_positions,
                        &mut ref_field_positions,
                        &mut generated_ref_note_fields,
                    ),
                    b"bookmarkStart" => {
                        if let (Some(id), Some(name)) =
                            (attr_local(&e, b"id"), attr_local(&e, b"name"))
                        {
                            active_bookmarks.push(NoteRefActiveBookmark {
                                id,
                                name,
                                start: source_order,
                            });
                            source_order += 1;
                        }
                    }
                    b"bookmarkEnd" => {
                        close_note_ref_bookmark(
                            attr_local(&e, b"id").as_deref(),
                            source_order,
                            &mut active_bookmarks,
                            &mut targets,
                        );
                        source_order += 1;
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
                    b"tab" | b"br" | b"cr" | b"noBreakHyphen" | b"drawing" | b"pict"
                    | b"object" => {
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
    match attr_local(e, b"fldCharType").as_deref() {
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
                    b"fldSimple" => record_section_field(
                        attr_local(&e, b"instr").as_deref(),
                        current_section,
                        &mut field_sections,
                    ),
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
                            section_has_visible_content = true;
                        }
                    }
                    b"br" if matches!(attr_local(&e, b"type").as_deref(), Some("page")) => {
                        current_page += 1;
                    }
                    b"lastRenderedPageBreak" => {
                        current_page += 1;
                    }
                    b"tab" | b"cr" | b"noBreakHyphen" | b"drawing" | b"pict" | b"object" => {
                        section_has_visible_content = true;
                    }
                    b"br" => {
                        section_has_visible_content = true;
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
                    b"fldSimple" => record_section_field(
                        attr_local(&e, b"instr").as_deref(),
                        current_section,
                        &mut field_sections,
                    ),
                    b"fldChar" => {
                        apply_section_scan_fld_char(
                            &e,
                            current_section,
                            &mut current,
                            &mut field_sections,
                        );
                    }
                    b"br" if matches!(attr_local(&e, b"type").as_deref(), Some("page")) => {
                        current_page += 1;
                    }
                    b"lastRenderedPageBreak" => {
                        current_page += 1;
                    }
                    b"tab" | b"br" | b"cr" | b"noBreakHyphen" | b"drawing" | b"pict"
                    | b"object" => {
                        section_has_visible_content = true;
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
) {
    if instruction
        .map(normalize_instruction)
        .as_deref()
        .and_then(section_instruction)
        .is_some()
    {
        field_sections.push(current_section);
    }
}

fn apply_section_scan_fld_char(
    e: &BytesStart<'_>,
    current_section: usize,
    current: &mut Option<SectionScanField>,
    field_sections: &mut Vec<usize>,
) {
    match attr_local(e, b"fldCharType").as_deref() {
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
                        record_style_ref_field(
                            attr_local(&e, b"instr").as_deref(),
                            field_positions,
                            next_order,
                            &number,
                        );
                        skip_element(r, b"fldSimple");
                        consumed_element = true;
                    }
                    b"t" => {
                        let run_text = read_text(r);
                        consumed_element = true;
                        if !style_ref_in_field_result(&current) {
                            text.push_str(&run_text);
                        }
                    }
                    _ => {
                        if let Some(marker) = inline_marker_text(&e) {
                            if !style_ref_in_field_result(&current) {
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
                            if !style_ref_in_field_result(&current) {
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
                    b"rStyle" => {
                        run_style_id = attr_local(&e, b"val");
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
                        if !style_ref_in_field_result(current) {
                            paragraph_text.push_str(&text);
                            run_text.push_str(&text);
                        }
                    }
                    _ => {
                        if let Some(marker) = inline_marker_text(&e) {
                            if !style_ref_in_field_result(current) {
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
                        run_style_id = attr_local(&e, b"val");
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
                            if !style_ref_in_field_result(current) {
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
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => match local(e.name().as_ref()) {
                b"pStyle" => *style_id = attr_local(&e, b"val"),
                b"ilvl" => {
                    if let Some(value) = attr_local(&e, b"val").and_then(|value| value.parse().ok())
                    {
                        *ilvl = value;
                    }
                }
                b"numId" => *num_id = attr_local(&e, b"val"),
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

fn style_ref_in_field_result(current: &Option<StyleRefScanField>) -> bool {
    current
        .as_ref()
        .is_some_and(|field| field.phase == FieldPhase::Result)
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
    match attr_local(e, b"fldCharType").as_deref() {
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

#[derive(Debug, Clone, Default)]
pub(crate) struct PageRefContext {
    targets: HashMap<String, PageRefTarget>,
    target_positions: HashMap<String, PageRefPosition>,
    field_positions: Vec<Option<PageRefPosition>>,
    page_field_positions: Vec<Option<PageRefPosition>>,
}

impl PageRefContext {
    pub(crate) fn empty() -> Self {
        Self::default()
    }

    fn target(&self, name: &str) -> Option<PageRefTarget> {
        self.targets.get(name).copied()
    }

    fn target_position(&self, name: &str) -> Option<PageRefPosition> {
        self.target_positions.get(name).copied()
    }

    pub(crate) fn field_position(&self, index: usize) -> Option<PageRefPosition> {
        self.field_positions.get(index).copied().flatten()
    }

    pub(crate) fn page_field_position(&self, index: usize) -> Option<PageRefPosition> {
        self.page_field_positions.get(index).copied().flatten()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PageRefTarget {
    display_page: usize,
    display_format: PageRefDisplayFormat,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PageRefPosition {
    physical_page: usize,
    display_page: usize,
    display_format: PageRefDisplayFormat,
    order: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PageRefDisplayFormat {
    Known(Option<PageNumberFormat>),
    Unsupported,
}

impl Default for PageRefDisplayFormat {
    fn default() -> Self {
        Self::Known(None)
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct LegacyFormContext {
    results: Vec<Option<String>>,
}

impl LegacyFormContext {
    pub(crate) fn empty() -> Self {
        Self::default()
    }

    fn field_result(&self, index: usize) -> Option<String> {
        self.results.get(index).cloned().flatten()
    }
}

#[derive(Debug, Clone, Default)]
struct LegacyFormData {
    checkbox: Option<bool>,
    dropdown_entries: Vec<String>,
    dropdown_default: Option<usize>,
    dropdown_result: Option<usize>,
    text_default: Option<String>,
}

#[derive(Debug, Clone)]
struct LegacyFormScanField {
    instruction: String,
    form_data: Option<LegacyFormData>,
    phase: FieldPhase,
}

#[derive(Debug, Clone)]
struct PageRefScanField {
    instruction: String,
    phase: FieldPhase,
}

#[derive(Debug, Clone, Copy)]
struct AlternateContentBranchState {
    branch_depth: usize,
    took_branch: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PageRefSectionBreak {
    Next,
    Even,
    Odd,
}

#[derive(Debug, Clone, Copy)]
struct PageRefPageState {
    leading_page_number: usize,
    leading_display_page_number: usize,
    leading_display_format: PageRefDisplayFormat,
    rendered_page_number: usize,
    rendered_display_page_number: usize,
    rendered_display_format: PageRefDisplayFormat,
    rendered_context_trusted: bool,
}

impl Default for PageRefPageState {
    fn default() -> Self {
        Self {
            leading_page_number: 1,
            leading_display_page_number: 1,
            leading_display_format: PageRefDisplayFormat::default(),
            rendered_page_number: 1,
            rendered_display_page_number: 1,
            rendered_display_format: PageRefDisplayFormat::default(),
            rendered_context_trusted: true,
        }
    }
}

impl PageRefPageState {
    fn with_initial_page_numbering(
        page_number_start: Option<usize>,
        page_number_format: Option<PageRefDisplayFormat>,
    ) -> Self {
        let mut state = Self::default();
        if let Some(start) = page_number_start {
            state.leading_display_page_number = start;
            state.rendered_display_page_number = start;
        }
        if let Some(format) = page_number_format {
            state.leading_display_format = format;
            state.rendered_display_format = format;
        }
        state
    }

    fn advance_section_break(
        &mut self,
        section_break: PageRefSectionBreak,
        saw_visible_content: bool,
        saw_rendered_page_break: bool,
        page_number_start: Option<usize>,
        page_number_format: Option<PageRefDisplayFormat>,
        source_order: &mut usize,
    ) {
        let previous_leading_page = self.leading_page_number;
        self.leading_page_number =
            page_after_section_break(self.leading_page_number, section_break);
        self.leading_display_page_number = page_after_section_break_display_page(
            self.leading_display_page_number,
            self.leading_page_number - previous_leading_page,
            page_number_start,
        );
        if let Some(format) = page_number_format {
            self.leading_display_format = format;
        }
        if !saw_visible_content || saw_rendered_page_break {
            let previous_rendered_page = self.rendered_page_number;
            self.rendered_page_number =
                page_after_section_break(self.rendered_page_number, section_break);
            self.rendered_display_page_number = page_after_section_break_display_page(
                self.rendered_display_page_number,
                self.rendered_page_number - previous_rendered_page,
                page_number_start,
            );
            if let Some(format) = page_number_format {
                self.rendered_display_format = format;
            }
            self.rendered_context_trusted = true;
        } else {
            if let Some(format) = page_number_format {
                self.rendered_display_format = format;
            }
            self.rendered_context_trusted = false;
        }
        *source_order += 1;
    }

    fn advance_explicit_break(
        &mut self,
        saw_visible_content: bool,
        saw_rendered_page_break: bool,
        source_order: &mut usize,
    ) {
        self.leading_page_number += 1;
        self.leading_display_page_number += 1;
        if !saw_visible_content || saw_rendered_page_break {
            self.rendered_page_number += 1;
            self.rendered_display_page_number += 1;
            self.rendered_context_trusted = true;
        } else {
            self.rendered_context_trusted = false;
        }
        *source_order += 1;
    }

    fn advance_last_rendered_page_break(&mut self, source_order: &mut usize) {
        self.rendered_page_number += 1;
        self.rendered_display_page_number += 1;
        self.rendered_context_trusted = true;
        *source_order += 1;
    }
}

pub(crate) fn legacy_form_context(xml: &str) -> LegacyFormContext {
    let mut r = Reader::from_str(xml);
    let mut results = Vec::new();
    let mut current: Option<LegacyFormScanField> = None;
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
                    b"fldSimple" => {
                        let instruction = attr_local(&e, b"instr").unwrap_or_default();
                        let form_data = read_legacy_form_data_until(&mut r);
                        record_simple_legacy_form_result(&instruction, form_data, &mut results);
                        consumed_element = true;
                    }
                    b"fldChar" => {
                        let kind = attr_local(&e, b"fldCharType");
                        let form_data = read_legacy_form_data_until(&mut r);
                        apply_legacy_form_scan_fld_char(
                            kind.as_deref(),
                            form_data,
                            &mut current,
                            &mut results,
                        );
                        consumed_element = true;
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
                    b"fldSimple" => {
                        record_simple_legacy_form_result(
                            attr_local(&e, b"instr").as_deref().unwrap_or_default(),
                            None,
                            &mut results,
                        );
                    }
                    b"fldChar" => apply_legacy_form_scan_fld_char(
                        attr_local(&e, b"fldCharType").as_deref(),
                        None,
                        &mut current,
                        &mut results,
                    ),
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
    LegacyFormContext { results }
}

fn record_simple_legacy_form_result(
    instruction: &str,
    form_data: Option<LegacyFormData>,
    results: &mut Vec<Option<String>>,
) {
    let instruction = normalize_instruction(instruction);
    if matches!(field_kind(&instruction), FieldKind::FormField(_)) {
        results.push(form_data.and_then(|data| legacy_form_field_result(&instruction, &data)));
    }
}

fn apply_legacy_form_scan_fld_char(
    kind: Option<&str>,
    form_data: Option<LegacyFormData>,
    current: &mut Option<LegacyFormScanField>,
    results: &mut Vec<Option<String>>,
) {
    match kind {
        Some("begin") => {
            *current = Some(LegacyFormScanField {
                instruction: String::new(),
                form_data,
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
                let instruction = normalize_instruction(&field.instruction);
                if matches!(field_kind(&instruction), FieldKind::FormField(_)) {
                    results.push(
                        field
                            .form_data
                            .and_then(|data| legacy_form_field_result(&instruction, &data)),
                    );
                }
            }
        }
        _ => {}
    }
}

fn read_legacy_form_data_until(r: &mut Xml<'_>) -> Option<LegacyFormData> {
    let mut depth = 1usize;
    let mut xml_depth = 1usize;
    let mut alternate_content_stack = Vec::new();
    let mut checkbox_depth = 0usize;
    let mut dropdown_depth = 0usize;
    let mut text_input_depth = 0usize;
    let mut data = LegacyFormData::default();
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
                let is_checkbox = name == b"checkBox";
                let is_dropdown = name == b"ddList";
                let is_text_input = name == b"textInput";
                read_legacy_form_data_element(
                    &e,
                    checkbox_depth > 0,
                    dropdown_depth > 0,
                    text_input_depth > 0,
                    &mut data,
                );
                match name {
                    b"AlternateContent" => {
                        alternate_content_stack.push(AlternateContentBranchState {
                            branch_depth: xml_depth + 1,
                            took_branch: false,
                        });
                    }
                    _ if is_checkbox => checkbox_depth += 1,
                    _ if is_dropdown => dropdown_depth += 1,
                    _ if is_text_input => text_input_depth += 1,
                    _ => {}
                }
                depth += 1;
                xml_depth = xml_depth.saturating_add(1);
            }
            Ok(Event::Empty(e)) => {
                let qname = e.name();
                let name = local(qname.as_ref());
                if should_skip_alternate_branch(&mut alternate_content_stack, xml_depth, name) {
                    continue;
                }
                read_legacy_form_data_element(
                    &e,
                    checkbox_depth > 0,
                    dropdown_depth > 0,
                    text_input_depth > 0,
                    &mut data,
                );
            }
            Ok(Event::End(e)) => {
                let qname = e.name();
                let name = local(qname.as_ref());
                if name == b"AlternateContent" {
                    alternate_content_stack.pop();
                }
                if name == b"checkBox" {
                    checkbox_depth = checkbox_depth.saturating_sub(1);
                } else if name == b"ddList" {
                    dropdown_depth = dropdown_depth.saturating_sub(1);
                } else if name == b"textInput" {
                    text_input_depth = text_input_depth.saturating_sub(1);
                }
                depth = depth.saturating_sub(1);
                xml_depth = xml_depth.saturating_sub(1);
                if depth == 0 {
                    break;
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    (data.checkbox.is_some()
        || !data.dropdown_entries.is_empty()
        || data.dropdown_default.is_some()
        || data.dropdown_result.is_some()
        || data.text_default.is_some())
    .then_some(data)
}

fn read_legacy_form_data_element(
    e: &BytesStart<'_>,
    in_checkbox: bool,
    in_dropdown: bool,
    in_text_input: bool,
    data: &mut LegacyFormData,
) {
    match local(e.name().as_ref()) {
        b"checked" if in_checkbox => data.checkbox = Some(on_off_value(e)),
        b"default" if in_checkbox && data.checkbox.is_none() => {
            data.checkbox = Some(on_off_value(e));
        }
        b"default" if in_dropdown => {
            data.dropdown_default = attr_local(e, b"val").and_then(|value| value.parse().ok());
        }
        b"default" if in_text_input => {
            data.text_default = attr_local(e, b"val");
        }
        b"result" if in_dropdown => {
            data.dropdown_result = attr_local(e, b"val").and_then(|value| value.parse().ok());
        }
        b"listEntry" if in_dropdown => {
            if let Some(value) = attr_local(e, b"val") {
                data.dropdown_entries.push(value);
            }
        }
        _ => {}
    }
}

fn on_off_value(e: &BytesStart<'_>) -> bool {
    !matches!(
        attr_local(e, b"val").as_deref(),
        Some("0")
            | Some("false")
            | Some("False")
            | Some("FALSE")
            | Some("off")
            | Some("Off")
            | Some("OFF")
    )
}

fn legacy_form_field_result(instruction: &str, data: &LegacyFormData) -> Option<String> {
    match field_kind(instruction) {
        FieldKind::FormField(kind) if kind == "FORMCHECKBOX" => Some(
            if data.checkbox? {
                "\u{2612}"
            } else {
                "\u{2610}"
            }
            .to_string(),
        ),
        FieldKind::FormField(kind) if kind == "FORMDROPDOWN" => {
            let index = data.dropdown_result.or(data.dropdown_default)?;
            data.dropdown_entries.get(index).cloned()
        }
        FieldKind::FormField(kind) if kind == "FORMTEXT" => data.text_default.clone(),
        _ => None,
    }
}

pub(crate) fn computed_legacy_form_result(
    instruction: &str,
    current_result: &str,
    legacy_forms: &LegacyFormContext,
    field_index: usize,
) -> Option<String> {
    match field_kind(instruction) {
        FieldKind::FormField(kind) if kind == "FORMTEXT" && !current_result.is_empty() => {
            Some(current_result.to_string())
        }
        FieldKind::FormField(_) => legacy_forms.field_result(field_index),
        _ => None,
    }
}

pub(crate) fn page_ref_context(xml: &str) -> PageRefContext {
    let mut r = Reader::from_str(xml);
    let mut targets = HashMap::new();
    let (initial_page_number_start, initial_page_number_format) =
        single_section_initial_page_numbering(xml);
    let mut pages = PageRefPageState::with_initial_page_numbering(
        initial_page_number_start,
        initial_page_number_format,
    );
    let mut source_order = 0usize;
    let mut saw_visible_content = false;
    let mut saw_rendered_page_break = false;
    let mut rendered_targets = HashMap::new();
    let mut target_positions = HashMap::new();
    let mut field_positions = Vec::new();
    let mut page_field_positions = Vec::new();
    let mut current: Option<PageRefScanField> = None;
    let mut paragraph_properties_depth = 0usize;
    let mut section_properties_depth = 0usize;
    let mut section_type_seen = false;
    let mut section_is_paragraph_break = false;
    let mut section_break_pending = None;
    let mut section_page_number_start = None;
    let mut section_page_number_format = None;
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
                    b"pPr" => paragraph_properties_depth += 1,
                    b"pageBreakBefore"
                        if paragraph_properties_depth > 0 && page_ref_on_off_enabled(&e) =>
                    {
                        pages.advance_explicit_break(
                            saw_visible_content,
                            saw_rendered_page_break,
                            &mut source_order,
                        );
                    }
                    b"sectPr" => {
                        section_properties_depth += 1;
                        if section_properties_depth == 1 {
                            section_type_seen = false;
                            section_is_paragraph_break = paragraph_properties_depth > 0;
                            section_break_pending = None;
                            section_page_number_start = None;
                            section_page_number_format = None;
                        }
                    }
                    b"type" if section_properties_depth > 0 && !section_type_seen => {
                        section_type_seen = true;
                        section_break_pending = page_ref_section_break(&e);
                    }
                    b"pgNumType" if section_properties_depth > 0 => {
                        if section_page_number_start.is_none() {
                            section_page_number_start = page_ref_section_page_number_start(&e);
                        }
                        if section_page_number_format.is_none() {
                            section_page_number_format = page_ref_section_page_number_format(&e);
                        }
                    }
                    b"fldSimple" => record_page_ref_field_position(
                        attr_local(&e, b"instr").as_deref(),
                        current_page_ref_position(&pages, source_order),
                        &mut source_order,
                        &mut field_positions,
                        &mut page_field_positions,
                    ),
                    b"fldChar" => apply_page_ref_scan_fld_char(
                        &e,
                        current_page_ref_position(&pages, source_order),
                        &mut source_order,
                        &mut current,
                        &mut field_positions,
                        &mut page_field_positions,
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
                        if let Some(name) = attr_local(&e, b"name") {
                            if pages.leading_page_number > 1 && !saw_visible_content {
                                targets.entry(name.clone()).or_insert(PageRefTarget {
                                    display_page: pages.leading_display_page_number,
                                    display_format: pages.leading_display_format,
                                });
                                target_positions_insert(
                                    &mut target_positions,
                                    name.clone(),
                                    PageRefPosition {
                                        physical_page: pages.leading_page_number,
                                        display_page: pages.leading_display_page_number,
                                        display_format: pages.leading_display_format,
                                        order: source_order,
                                    },
                                );
                            }
                            if pages.rendered_context_trusted {
                                rendered_targets.entry(name).or_insert(PageRefPosition {
                                    physical_page: pages.rendered_page_number,
                                    display_page: pages.rendered_display_page_number,
                                    display_format: pages.rendered_display_format,
                                    order: source_order,
                                });
                            }
                            source_order += 1;
                        }
                    }
                    b"t" => {
                        let visible_text = !read_text(&mut r).is_empty();
                        consumed_element = true;
                        saw_visible_content |= visible_text;
                        if visible_text {
                            source_order += 1;
                        }
                    }
                    b"br" if matches!(attr_local(&e, b"type").as_deref(), Some("page")) => {
                        pages.advance_explicit_break(
                            saw_visible_content,
                            saw_rendered_page_break,
                            &mut source_order,
                        );
                    }
                    b"lastRenderedPageBreak" => {
                        saw_rendered_page_break = true;
                        pages.advance_last_rendered_page_break(&mut source_order);
                    }
                    b"tab" | b"cr" | b"noBreakHyphen" | b"drawing" | b"pict" | b"object" => {
                        saw_visible_content = true;
                        source_order += 1;
                    }
                    b"br" => {
                        saw_visible_content = true;
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
                    b"pageBreakBefore"
                        if paragraph_properties_depth > 0 && page_ref_on_off_enabled(&e) =>
                    {
                        pages.advance_explicit_break(
                            saw_visible_content,
                            saw_rendered_page_break,
                            &mut source_order,
                        );
                    }
                    b"sectPr" if paragraph_properties_depth > 0 => {
                        pages.advance_section_break(
                            PageRefSectionBreak::Next,
                            saw_visible_content,
                            saw_rendered_page_break,
                            None,
                            None,
                            &mut source_order,
                        );
                    }
                    b"type" if section_properties_depth > 0 && !section_type_seen => {
                        section_type_seen = true;
                        section_break_pending = page_ref_section_break(&e);
                    }
                    b"pgNumType" if section_properties_depth > 0 => {
                        if section_page_number_start.is_none() {
                            section_page_number_start = page_ref_section_page_number_start(&e);
                        }
                        if section_page_number_format.is_none() {
                            section_page_number_format = page_ref_section_page_number_format(&e);
                        }
                    }
                    b"fldSimple" => record_page_ref_field_position(
                        attr_local(&e, b"instr").as_deref(),
                        current_page_ref_position(&pages, source_order),
                        &mut source_order,
                        &mut field_positions,
                        &mut page_field_positions,
                    ),
                    b"fldChar" => apply_page_ref_scan_fld_char(
                        &e,
                        current_page_ref_position(&pages, source_order),
                        &mut source_order,
                        &mut current,
                        &mut field_positions,
                        &mut page_field_positions,
                    ),
                    b"bookmarkStart" => {
                        if let Some(name) = attr_local(&e, b"name") {
                            if pages.leading_page_number > 1 && !saw_visible_content {
                                targets.entry(name.clone()).or_insert(PageRefTarget {
                                    display_page: pages.leading_display_page_number,
                                    display_format: pages.leading_display_format,
                                });
                                target_positions_insert(
                                    &mut target_positions,
                                    name.clone(),
                                    PageRefPosition {
                                        physical_page: pages.leading_page_number,
                                        display_page: pages.leading_display_page_number,
                                        display_format: pages.leading_display_format,
                                        order: source_order,
                                    },
                                );
                            }
                            if pages.rendered_context_trusted {
                                rendered_targets.entry(name).or_insert(PageRefPosition {
                                    physical_page: pages.rendered_page_number,
                                    display_page: pages.rendered_display_page_number,
                                    display_format: pages.rendered_display_format,
                                    order: source_order,
                                });
                            }
                            source_order += 1;
                        }
                    }
                    b"br" if matches!(attr_local(&e, b"type").as_deref(), Some("page")) => {
                        pages.advance_explicit_break(
                            saw_visible_content,
                            saw_rendered_page_break,
                            &mut source_order,
                        );
                    }
                    b"lastRenderedPageBreak" => {
                        saw_rendered_page_break = true;
                        pages.advance_last_rendered_page_break(&mut source_order);
                    }
                    b"tab" | b"br" | b"cr" | b"noBreakHyphen" | b"drawing" | b"pict"
                    | b"object" => {
                        saw_visible_content = true;
                        source_order += 1;
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
                    b"sectPr" => {
                        if section_properties_depth == 1 && section_is_paragraph_break {
                            let section_break = if section_type_seen {
                                section_break_pending
                            } else {
                                Some(PageRefSectionBreak::Next)
                            };
                            if let Some(section_break) = section_break {
                                pages.advance_section_break(
                                    section_break,
                                    saw_visible_content,
                                    saw_rendered_page_break,
                                    section_page_number_start,
                                    section_page_number_format,
                                    &mut source_order,
                                );
                            }
                        }
                        section_properties_depth = section_properties_depth.saturating_sub(1);
                        if section_properties_depth == 0 {
                            section_type_seen = false;
                            section_is_paragraph_break = false;
                            section_break_pending = None;
                            section_page_number_start = None;
                            section_page_number_format = None;
                        }
                    }
                    b"pPr" => {
                        paragraph_properties_depth = paragraph_properties_depth.saturating_sub(1);
                    }
                    _ => {}
                }
                xml_depth = xml_depth.saturating_sub(1);
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    if saw_rendered_page_break {
        for (name, position) in rendered_targets {
            match targets.entry(name.clone()) {
                std::collections::hash_map::Entry::Vacant(entry) => {
                    entry.insert(PageRefTarget {
                        display_page: position.display_page,
                        display_format: position.display_format,
                    });
                    target_positions_insert(&mut target_positions, name, position);
                }
                std::collections::hash_map::Entry::Occupied(entry) => {
                    if entry.get().display_page == position.display_page
                        && entry.get().display_format == position.display_format
                    {
                        target_positions_insert(&mut target_positions, name, position);
                    }
                }
            }
        }
    }
    PageRefContext {
        targets,
        target_positions,
        field_positions,
        page_field_positions,
    }
}

fn single_section_initial_page_numbering(
    xml: &str,
) -> (Option<usize>, Option<PageRefDisplayFormat>) {
    let mut r = Reader::from_str(xml);
    let mut paragraph_properties_depth = 0usize;
    let mut body_section_properties_depth = 0usize;
    let mut has_paragraph_section_properties = false;
    let mut page_number_start = None;
    let mut page_number_format = None;
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
                    b"pPr" => paragraph_properties_depth += 1,
                    b"sectPr" if paragraph_properties_depth > 0 => {
                        has_paragraph_section_properties = true;
                        skip_subtree(&mut r);
                        continue;
                    }
                    b"sectPr" => body_section_properties_depth += 1,
                    b"pgNumType" if body_section_properties_depth > 0 => {
                        if page_number_start.is_none() {
                            page_number_start = page_ref_section_page_number_start(&e);
                        }
                        if page_number_format.is_none() {
                            page_number_format = page_ref_section_page_number_format(&e);
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
                    b"sectPr" if paragraph_properties_depth > 0 => {
                        has_paragraph_section_properties = true;
                    }
                    b"pgNumType" if body_section_properties_depth > 0 => {
                        if page_number_start.is_none() {
                            page_number_start = page_ref_section_page_number_start(&e);
                        }
                        if page_number_format.is_none() {
                            page_number_format = page_ref_section_page_number_format(&e);
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
                    b"sectPr" => {
                        body_section_properties_depth =
                            body_section_properties_depth.saturating_sub(1);
                    }
                    b"pPr" => {
                        paragraph_properties_depth = paragraph_properties_depth.saturating_sub(1);
                    }
                    _ => {}
                }
                xml_depth = xml_depth.saturating_sub(1);
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    if has_paragraph_section_properties {
        (None, None)
    } else {
        (page_number_start, page_number_format)
    }
}

fn page_ref_on_off_enabled(e: &BytesStart<'_>) -> bool {
    !matches!(
        attr_local(e, b"val").as_deref(),
        Some("0")
            | Some("false")
            | Some("False")
            | Some("FALSE")
            | Some("off")
            | Some("Off")
            | Some("OFF")
    )
}

fn page_ref_section_break(e: &BytesStart<'_>) -> Option<PageRefSectionBreak> {
    match attr_local(e, b"val").as_deref() {
        Some("nextPage") => Some(PageRefSectionBreak::Next),
        Some("evenPage") => Some(PageRefSectionBreak::Even),
        Some("oddPage") => Some(PageRefSectionBreak::Odd),
        _ => None,
    }
}

fn page_ref_section_page_number_start(e: &BytesStart<'_>) -> Option<usize> {
    attr_local(e, b"start")
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|start| *start > 0)
}

fn page_ref_section_page_number_format(e: &BytesStart<'_>) -> Option<PageRefDisplayFormat> {
    let format = match attr_local(e, b"fmt").as_deref()? {
        "decimal" => PageNumberFormat::Arabic,
        "decimalZero" => PageNumberFormat::DecimalZero,
        "numberInDash" => PageNumberFormat::ArabicDash,
        "decimalFullWidth" => PageNumberFormat::DecimalFullWidth,
        "decimalHalfWidth" => PageNumberFormat::DecimalHalfWidth,
        "decimalFullWidth2" => PageNumberFormat::DecimalFullWidth2,
        "decimalEnclosedCircle" => PageNumberFormat::DecimalEnclosedCircle,
        "decimalEnclosedFullstop" => PageNumberFormat::DecimalEnclosedFullstop,
        "decimalEnclosedParen" => PageNumberFormat::DecimalEnclosedParen,
        "ganada" => PageNumberFormat::Ganada,
        "chosung" => PageNumberFormat::Chosung,
        "koreanDigital" => PageNumberFormat::KoreanDigital,
        "koreanCounting" => PageNumberFormat::KoreanCounting,
        "koreanLegal" => PageNumberFormat::KoreanLegal,
        "koreanDigital2" => PageNumberFormat::KoreanDigital2,
        "lowerLetter" => PageNumberFormat::AlphabeticLower,
        "upperLetter" => PageNumberFormat::AlphabeticUpper,
        "lowerRoman" => PageNumberFormat::RomanLower,
        "upperRoman" => PageNumberFormat::RomanUpper,
        "ordinal" => PageNumberFormat::Ordinal,
        "cardinalText" => PageNumberFormat::CardText,
        "ordinalText" => PageNumberFormat::OrdText,
        _ => return Some(PageRefDisplayFormat::Unsupported),
    };
    Some(PageRefDisplayFormat::Known(Some(format)))
}

fn page_after_section_break(page: usize, section_break: PageRefSectionBreak) -> usize {
    let next = page + 1;
    match section_break {
        PageRefSectionBreak::Next => next,
        PageRefSectionBreak::Even if next % 2 == 1 => next + 1,
        PageRefSectionBreak::Odd if next % 2 == 0 => next + 1,
        PageRefSectionBreak::Even | PageRefSectionBreak::Odd => next,
    }
}

fn page_after_section_break_display_page(
    display_page: usize,
    physical_delta: usize,
    page_number_start: Option<usize>,
) -> usize {
    page_number_start.unwrap_or(display_page + physical_delta)
}

fn target_positions_insert(
    target_positions: &mut HashMap<String, PageRefPosition>,
    name: String,
    position: PageRefPosition,
) {
    target_positions.entry(name).or_insert(position);
}

fn current_page_ref_position(
    pages: &PageRefPageState,
    source_order: usize,
) -> Option<PageRefPosition> {
    pages.rendered_context_trusted.then_some(PageRefPosition {
        physical_page: pages.rendered_page_number,
        display_page: pages.rendered_display_page_number,
        display_format: pages.rendered_display_format,
        order: source_order,
    })
}

fn record_page_ref_field_position(
    instruction: Option<&str>,
    position: Option<PageRefPosition>,
    source_order: &mut usize,
    field_positions: &mut Vec<Option<PageRefPosition>>,
    page_field_positions: &mut Vec<Option<PageRefPosition>>,
) {
    match instruction
        .map(normalize_instruction)
        .as_deref()
        .map(field_kind)
    {
        Some(FieldKind::PageRef) => {
            field_positions.push(position);
            *source_order += 1;
        }
        Some(FieldKind::Page) => {
            page_field_positions.push(position);
            *source_order += 1;
        }
        _ => {}
    }
}

fn apply_page_ref_scan_fld_char(
    e: &BytesStart<'_>,
    position: Option<PageRefPosition>,
    source_order: &mut usize,
    current: &mut Option<PageRefScanField>,
    field_positions: &mut Vec<Option<PageRefPosition>>,
    page_field_positions: &mut Vec<Option<PageRefPosition>>,
) {
    match attr_local(e, b"fldCharType").as_deref() {
        Some("begin") => {
            *current = Some(PageRefScanField {
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
                record_page_ref_field_position(
                    Some(&field.instruction),
                    position,
                    source_order,
                    field_positions,
                    page_field_positions,
                );
            }
        }
        _ => {}
    }
}

fn skip_subtree(r: &mut Xml<'_>) {
    let mut depth = 1usize;
    loop {
        match r.read_event() {
            Ok(Event::Start(_)) => depth += 1,
            Ok(Event::End(_)) => {
                depth -= 1;
                if depth == 0 {
                    break;
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
}

fn should_skip_alternate_branch(
    stack: &mut [AlternateContentBranchState],
    xml_depth: usize,
    name: &[u8],
) -> bool {
    if !matches!(name, b"Choice" | b"Fallback") {
        return false;
    }
    let Some(state) = stack.last_mut() else {
        return false;
    };
    if state.branch_depth != xml_depth {
        return false;
    }
    if state.took_branch {
        true
    } else {
        state.took_branch = true;
        false
    }
}

fn append_ref_text(active: &[(String, String)], out: &mut HashMap<String, String>, text: &str) {
    for (_, name) in active {
        out.entry(name.clone()).or_default().push_str(text);
    }
}

fn append_ref_paragraph_breaks(active: &[(String, String)], out: &mut HashMap<String, String>) {
    for (_, name) in active {
        let text = out.entry(name.clone()).or_default();
        if !text.is_empty() {
            text.push('\n');
        }
    }
}

pub(crate) fn toc_entries(xml: &str, styles: &Styles) -> Vec<TocEntry> {
    let mut r = Reader::from_str(xml);
    let mut entries = Vec::new();
    let mut active_bookmarks = Vec::new();
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
                        read_toc_paragraph(&mut r, styles, &mut active_bookmarks, &mut entries);
                        consumed_element = true;
                    }
                    b"bookmarkStart" => {
                        push_active_bookmark(&mut active_bookmarks, &e);
                    }
                    b"bookmarkEnd" => {
                        remove_active_bookmark(&mut active_bookmarks, &e);
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
                    b"bookmarkStart" => {
                        push_active_bookmark(&mut active_bookmarks, &e);
                    }
                    b"bookmarkEnd" => {
                        remove_active_bookmark(&mut active_bookmarks, &e);
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
                xml_depth = xml_depth.saturating_sub(1);
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    entries
}

fn read_toc_paragraph(
    r: &mut Xml<'_>,
    styles: &Styles,
    active_bookmarks: &mut Vec<(String, String)>,
    entries: &mut Vec<TocEntry>,
) {
    let mut style_id: Option<String> = None;
    let mut outline: Option<u8> = None;
    let mut text = String::new();
    let mut bookmarks = active_bookmark_names(active_bookmarks);
    let mut current = Vec::new();
    let mut sequence_identifiers = Vec::new();
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
                        read_toc_ppr(r, &mut style_id, &mut outline);
                        consumed_element = true;
                    }
                    b"fldSimple" => {
                        let instruction = attr_local(&e, b"instr");
                        if push_tc_entry_from_instruction(instruction.clone(), &bookmarks, entries)
                        {
                            skip_element(r, b"fldSimple");
                            consumed_element = true;
                        } else if let Some(identifier) =
                            seq_identifier_from_instruction(instruction.as_deref())
                        {
                            push_unique(&mut sequence_identifiers, identifier);
                        }
                    }
                    b"fldChar" => {
                        apply_toc_fld_char(
                            &e,
                            &mut current,
                            &bookmarks,
                            &mut sequence_identifiers,
                            entries,
                        );
                    }
                    b"instrText" => {
                        let field_text = read_text(r);
                        consumed_element = true;
                        if let Some(field) = current.last_mut() {
                            if field.phase == FieldPhase::Instruction {
                                field.instruction.push_str(&field_text);
                            }
                        }
                    }
                    b"t" => {
                        let run_text = read_text(r);
                        consumed_element = true;
                        let hidden_tc_result = current.iter().rev().any(|field| {
                            field.phase == FieldPhase::Result
                                && tc_instruction(&field.instruction).is_some()
                        });
                        if !hidden_tc_result {
                            text.push_str(&run_text);
                        }
                    }
                    b"tab" | b"br" | b"cr" => text.push(' '),
                    b"noBreakHyphen" => text.push('-'),
                    b"bookmarkStart" => {
                        if let Some(name) = push_active_bookmark(active_bookmarks, &e) {
                            push_unique(&mut bookmarks, name);
                        }
                    }
                    b"bookmarkEnd" => remove_active_bookmark(active_bookmarks, &e),
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
                    b"fldSimple" => {
                        let instruction = attr_local(&e, b"instr");
                        if !push_tc_entry_from_instruction(instruction.clone(), &bookmarks, entries)
                        {
                            if let Some(identifier) =
                                seq_identifier_from_instruction(instruction.as_deref())
                            {
                                push_unique(&mut sequence_identifiers, identifier);
                            }
                        }
                    }
                    b"fldChar" => {
                        apply_toc_fld_char(
                            &e,
                            &mut current,
                            &bookmarks,
                            &mut sequence_identifiers,
                            entries,
                        );
                    }
                    b"tab" | b"br" | b"cr" => text.push(' '),
                    b"noBreakHyphen" => text.push('-'),
                    b"bookmarkStart" => {
                        if let Some(name) = push_active_bookmark(active_bookmarks, &e) {
                            push_unique(&mut bookmarks, name);
                        }
                    }
                    b"bookmarkEnd" => remove_active_bookmark(active_bookmarks, &e),
                    _ => {}
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
    if !text.is_empty() {
        for identifier in sequence_identifiers {
            entries.push(TocEntry {
                level: 1,
                sequence_caption_text: caption_text_without_label(&text, &identifier),
                text: text.clone(),
                source: TocEntrySource::SequenceField,
                tc_type: None,
                sequence_identifier: Some(identifier),
                bookmarks: bookmarks.clone(),
                style_id: None,
                style_name: None,
            });
        }
    }
    let (level, source) = match outline {
        Some(o) if o <= 8 => (o + 1, TocEntrySource::OutlineLevel),
        Some(_) => return,
        None => match style_id
            .as_deref()
            .and_then(|style_id| styles.heading_level(style_id))
        {
            Some(level) => (level, TocEntrySource::HeadingStyle),
            None if style_id.is_some() || style_name.is_some() => {
                (0, TocEntrySource::StyledParagraph)
            }
            None => return,
        },
    };
    if !text.is_empty() {
        entries.push(TocEntry {
            level,
            text,
            source,
            tc_type: None,
            sequence_identifier: None,
            sequence_caption_text: None,
            bookmarks,
            style_id,
            style_name,
        });
    }
}

fn skip_element(r: &mut Xml<'_>, element: &[u8]) {
    let mut depth = 1usize;
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) if local(e.name().as_ref()) == element => {
                depth += 1;
            }
            Ok(Event::End(e)) if local(e.name().as_ref()) == element => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    break;
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
}

fn apply_toc_fld_char(
    e: &BytesStart<'_>,
    current: &mut Vec<ComplexField>,
    bookmarks: &[String],
    sequence_identifiers: &mut Vec<String>,
    entries: &mut Vec<TocEntry>,
) {
    apply_complex_field_scan_fld_char(e, current, |field| {
        if !push_tc_entry(&field.instruction, bookmarks, entries) {
            if let Some(identifier) = seq_identifier_from_instruction(Some(&field.instruction)) {
                push_unique(sequence_identifiers, identifier);
            }
        }
    });
}

fn push_tc_entry_from_instruction(
    instruction: Option<String>,
    bookmarks: &[String],
    entries: &mut Vec<TocEntry>,
) -> bool {
    let Some(instruction) = instruction else {
        return false;
    };
    if !is_tc_instruction(&instruction) {
        return false;
    }
    push_tc_entry(&instruction, bookmarks, entries);
    true
}

fn push_tc_entry(instruction: &str, bookmarks: &[String], entries: &mut Vec<TocEntry>) -> bool {
    if let Some(spec) = tc_instruction(instruction) {
        entries.push(TocEntry {
            level: spec.level,
            text: spec.text,
            source: TocEntrySource::TcField,
            tc_type: spec.entry_type,
            sequence_identifier: None,
            sequence_caption_text: None,
            bookmarks: bookmarks.to_vec(),
            style_id: None,
            style_name: None,
        });
        return true;
    }
    false
}

pub(crate) fn computed_toc_entry_result(instruction: &str) -> Option<String> {
    tc_instruction(instruction)?;
    Some(String::new())
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TcInstruction {
    text: String,
    entry_type: Option<String>,
    level: u8,
}

fn tc_instruction(instruction: &str) -> Option<TcInstruction> {
    let tokens = instruction_parts(instruction);
    let mut parts = tokens.iter().map(String::as_str).peekable();
    let kind = parts.next()?;
    if !kind.eq_ignore_ascii_case("TC") {
        return None;
    }
    let text = field_name_token(parts.next()?)?.to_string();
    if text.is_empty() {
        return None;
    }
    let mut entry_type = None;
    let mut level = 1u8;
    let mut saw_level = false;
    while let Some(part) = parts.next() {
        if part.eq_ignore_ascii_case("\\f") {
            let value = parts.next_if(|next| !next.starts_with('\\'))?;
            set_tc_entry_type(&mut entry_type, value)?;
            continue;
        }
        if let Some(value) = strip_ascii_switch_prefix(part, "\\f") {
            if value.is_empty() || set_tc_entry_type(&mut entry_type, value).is_none() {
                return None;
            }
            continue;
        }
        if part.eq_ignore_ascii_case("\\l") {
            let value = parts.next_if(|next| !next.starts_with('\\'))?;
            if saw_level {
                return None;
            }
            level = parse_toc_level(value)?;
            saw_level = true;
            continue;
        }
        if let Some(value) = strip_ascii_switch_prefix(part, "\\l") {
            if value.is_empty() || saw_level {
                return None;
            }
            level = parse_toc_level(value)?;
            saw_level = true;
            continue;
        }
        if part.eq_ignore_ascii_case("\\n") {
            continue;
        }
        return None;
    }
    Some(TcInstruction {
        text,
        entry_type,
        level,
    })
}

fn set_tc_entry_type(slot: &mut Option<String>, value: &str) -> Option<()> {
    let value = tc_type_identifier(value)?;
    if slot.replace(value).is_some() {
        return None;
    }
    Some(())
}

fn tc_type_identifier(value: &str) -> Option<String> {
    Some(field_identifier_token(value)?.to_string())
}

fn field_identifier_token(value: &str) -> Option<&str> {
    let value = value.trim();
    let value = match (value.starts_with('"'), value.ends_with('"')) {
        (true, true) if value.len() >= 2 => &value[1..value.len() - 1],
        (true, _) | (_, true) => return None,
        (false, false) => value,
    }
    .trim();
    if value.is_empty()
        || value.starts_with('\\')
        || value.contains('"')
        || value.chars().any(char::is_whitespace)
    {
        return None;
    }
    Some(value)
}

fn parse_toc_level(value: &str) -> Option<u8> {
    let level = field_name_token(value)?.parse::<u8>().ok()?;
    (1..=9).contains(&level).then_some(level)
}

fn is_tc_instruction(instruction: &str) -> bool {
    instruction_parts(instruction)
        .first()
        .is_some_and(|kind| kind.eq_ignore_ascii_case("TC"))
}

fn seq_identifier_from_instruction(instruction: Option<&str>) -> Option<String> {
    sequence_instruction(instruction?).map(|instruction| instruction.identifier)
}

fn caption_text_without_label(text: &str, identifier: &str) -> Option<String> {
    let rest = strip_caption_identifier_prefix(text.trim_start(), identifier)?.trim_start();
    if rest.is_empty() {
        return None;
    }
    if let Some((_, caption)) = rest.split_once(':') {
        return nonempty_caption_text(caption);
    }
    let (_, caption) = rest.split_once(char::is_whitespace)?;
    nonempty_caption_text(
        caption.trim_start_matches(|ch: char| ch.is_whitespace() || matches!(ch, ':' | '-' | '.')),
    )
}

fn strip_caption_identifier_prefix<'a>(text: &'a str, identifier: &str) -> Option<&'a str> {
    let mut chars = text.chars();
    for expected in identifier.chars() {
        let actual = chars.next()?;
        if actual != expected && !actual.eq_ignore_ascii_case(&expected) {
            return None;
        }
    }
    Some(chars.as_str())
}

fn nonempty_caption_text(text: &str) -> Option<String> {
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_string())
}

fn active_bookmark_names(active_bookmarks: &[(String, String)]) -> Vec<String> {
    let mut names = Vec::new();
    for (_, name) in active_bookmarks {
        push_unique(&mut names, name.clone());
    }
    names
}

fn push_active_bookmark(
    active_bookmarks: &mut Vec<(String, String)>,
    e: &BytesStart<'_>,
) -> Option<String> {
    let id = attr_local(e, b"id")?;
    let name = attr_local(e, b"name")?;
    if !active_bookmarks
        .iter()
        .any(|(active_id, _)| active_id == &id)
    {
        active_bookmarks.push((id, name.clone()));
    }
    Some(name)
}

fn remove_active_bookmark(active_bookmarks: &mut Vec<(String, String)>, e: &BytesStart<'_>) {
    if let Some(id) = attr_local(e, b"id") {
        active_bookmarks.retain(|(active_id, _)| active_id != &id);
    }
}

fn push_unique(names: &mut Vec<String>, name: String) {
    if !names.iter().any(|existing| existing == &name) {
        names.push(name);
    }
}

fn read_toc_ppr(r: &mut Xml<'_>, style_id: &mut Option<String>, outline: &mut Option<u8>) {
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => match local(e.name().as_ref()) {
                b"pStyle" => *style_id = attr_local(&e, b"val"),
                b"outlineLvl" => *outline = attr_local(&e, b"val").and_then(|v| v.parse().ok()),
                _ => {}
            },
            Ok(Event::End(e)) if local(e.name().as_ref()) == b"pPr" => break,
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
}

fn normalize_toc_text(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn inline_marker_text(e: &BytesStart<'_>) -> Option<&'static str> {
    match local(e.name().as_ref()) {
        b"tab" => Some("\t"),
        b"br" => {
            if matches!(attr_local(e, b"type").as_deref(), Some("page")) {
                Some("\u{000C}")
            } else {
                Some("\n")
            }
        }
        b"cr" => Some("\n"),
        b"noBreakHyphen" => Some("-"),
        _ => None,
    }
}

struct ComputedResultContexts<'a> {
    bookmarks: &'a HashMap<String, String>,
    ref_positions: &'a RefPositionContext,
    ref_numbers: &'a RefNumberContext,
    page_refs: &'a PageRefContext,
    note_refs: &'a NoteRefContext,
    sections: &'a SectionContext,
    style_refs: &'a StyleRefContext,
    legacy_forms: &'a LegacyFormContext,
    table_formulas: &'a TableFormulaContext,
    toc_entries: &'a [TocEntry],
    core_properties: &'a CoreProperties,
    custom_properties: &'a HashMap<String, String>,
    document_variables: &'a HashMap<String, String>,
    extended_properties: &'a HashMap<String, String>,
    file_size_bytes: Option<usize>,
}

fn apply_computed_results(fields: &mut [Field], ctx: ComputedResultContexts<'_>) {
    let mut ref_field_index = 0usize;
    let mut page_field_index = 0usize;
    let mut page_ref_field_index = 0usize;
    let mut note_ref_field_index = 0usize;
    let mut section_field_index = 0usize;
    let mut style_ref_field_index = 0usize;
    let mut form_field_index = 0usize;
    let mut formula_field_index = 0usize;
    let mut sequence_counters = HashMap::new();
    let mut autonum_counter = 0i64;
    let mut listnum_counter = 0i64;
    let mut field_bookmarks = HashMap::new();
    for field in fields {
        field.computed_result = match field.kind.clone() {
            FieldKind::Ref => {
                let position = ctx.ref_positions.field_position(ref_field_index);
                let note_ref_position = ctx.note_refs.ref_field_position(ref_field_index);
                ref_field_index += 1;
                let ref_ctx = RefResultContext {
                    bookmarks: ctx.bookmarks,
                    ref_positions: ctx.ref_positions,
                    ref_numbers: ctx.ref_numbers,
                    note_refs: ctx.note_refs,
                    field_bookmarks: &field_bookmarks,
                };
                computed_ref_result(&field.instruction, &ref_ctx, position, note_ref_position)
            }
            FieldKind::Page => {
                let position = ctx.page_refs.page_field_position(page_field_index);
                page_field_index += 1;
                computed_page_result(&field.instruction, position)
            }
            FieldKind::PageRef => {
                let position = ctx.page_refs.field_position(page_ref_field_index);
                page_ref_field_index += 1;
                computed_page_ref_result(&field.instruction, ctx.page_refs, position)
            }
            FieldKind::NoteRef => {
                let position = ctx.note_refs.field_position(note_ref_field_index);
                note_ref_field_index += 1;
                computed_note_ref_result(&field.instruction, ctx.note_refs, position)
            }
            FieldKind::Sequence => {
                computed_sequence_result(&field.instruction, &mut sequence_counters)
            }
            FieldKind::TocEntry => computed_toc_entry_result(&field.instruction),
            FieldKind::Toc => computed_toc_result(&field.instruction, ctx.toc_entries),
            FieldKind::DocumentStructure(kind) if kind == "REVNUM" => {
                computed_revision_number_result(&field.instruction, ctx.core_properties)
            }
            FieldKind::DocumentStructure(kind) if kind == "SECTION" || kind == "SECTIONPAGES" => {
                let position = ctx.sections.field_position(section_field_index);
                section_field_index += 1;
                computed_section_result(&field.instruction, position)
            }
            FieldKind::DocumentStructure(kind) if kind == "STYLEREF" => {
                let position = ctx.style_refs.field_position(style_ref_field_index);
                style_ref_field_index += 1;
                computed_style_ref_result(&field.instruction, ctx.style_refs, position)
            }
            FieldKind::Dynamic(kind) if kind == "=" => {
                let result = ctx.table_formulas.field_result(formula_field_index);
                formula_field_index += 1;
                result.or_else(|| computed_dynamic_result(&field.instruction))
            }
            FieldKind::Dynamic(kind)
                if kind == "QUOTE"
                    || kind == "FILLIN"
                    || kind == "IF"
                    || kind == "COMPARE"
                    || kind == "NEXT"
                    || kind == "NEXTIF"
                    || kind == "SKIPIF" =>
            {
                computed_dynamic_result(&field.instruction)
            }
            FieldKind::Dynamic(kind) if kind == "ASK" => {
                computed_ask_result(&field.instruction, &mut field_bookmarks)
            }
            FieldKind::Dynamic(kind) if kind == "SET" => {
                computed_set_result(&field.instruction, &mut field_bookmarks)
            }
            FieldKind::DocumentInfo(_) => computed_document_info_result(
                &field.instruction,
                ctx.core_properties,
                ctx.custom_properties,
                ctx.document_variables,
                ctx.extended_properties,
                ctx.file_size_bytes,
            ),
            FieldKind::ReferenceIndex(kind) if kind == "RD" || kind == "TA" || kind == "XE" => {
                computed_reference_index_result(&field.instruction)
            }
            FieldKind::Display(_) => computed_display_result(&field.instruction),
            FieldKind::Action(kind)
                if kind == "GOTOBUTTON" || kind == "MACROBUTTON" || kind == "PRINT" =>
            {
                computed_action_result(&field.instruction)
            }
            FieldKind::FormField(_) => {
                let result = computed_legacy_form_result(
                    &field.instruction,
                    &field.result,
                    ctx.legacy_forms,
                    form_field_index,
                );
                form_field_index += 1;
                result
            }
            FieldKind::Numbering(kind)
                if kind == "AUTONUM" || kind == "AUTONUMLGL" || kind == "AUTONUMOUT" =>
            {
                computed_numbering_result(&field.instruction, &mut autonum_counter)
            }
            FieldKind::Numbering(kind) if kind == "LISTNUM" => {
                computed_listnum_result(&field.instruction, &mut listnum_counter)
            }
            FieldKind::Unknown(_) => {
                let position = ctx.ref_positions.field_position(ref_field_index);
                let note_ref_position = ctx.note_refs.ref_field_position(ref_field_index);
                let is_ref_position = is_ref_position_field_instruction(&field.instruction);
                if is_ref_position {
                    ref_field_index += 1;
                }
                let spec = direct_bookmark_ref_instruction(&field.instruction);
                let ref_ctx = RefResultContext {
                    bookmarks: ctx.bookmarks,
                    ref_positions: ctx.ref_positions,
                    ref_numbers: ctx.ref_numbers,
                    note_refs: ctx.note_refs,
                    field_bookmarks: &field_bookmarks,
                };
                let result = spec.as_ref().and_then(|spec| {
                    let text = computed_ref_instruction_result(
                        spec,
                        &ref_ctx,
                        position.clone(),
                        note_ref_position,
                    )?;
                    Some(apply_field_text_format(text, spec.text_format))
                });
                if result.is_some()
                    || spec
                        .as_ref()
                        .is_some_and(|spec| ref_instruction_target_known(spec, &ref_ctx))
                {
                    field.kind = FieldKind::Ref;
                }
                result
            }
            _ => None,
        };
    }
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

fn computed_ref_instruction_result(
    spec: &RefInstruction,
    ctx: &RefResultContext<'_>,
    field_position: Option<RefFieldPosition>,
    note_ref_field_position: Option<NoteRefFieldPosition>,
) -> Option<String> {
    if spec.sequence_separator {
        return None;
    }
    if spec.note_reference {
        return ctx
            .note_refs
            .ref_note_number(&spec.target, note_ref_field_position);
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
        Some(text.clone())
    } else {
        ctx.bookmarks
            .get(&spec.target)
            .filter(|text| !text.is_empty())
            .cloned()
    }
}

fn ref_instruction_target_known(spec: &RefInstruction, ctx: &RefResultContext<'_>) -> bool {
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

fn relative_context_ref_number(target: &str, field: &str) -> String {
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

fn direct_bookmark_ref_instruction(instruction: &str) -> Option<RefInstruction> {
    let tokens = instruction_parts(instruction);
    parse_ref_instruction_parts(tokens.iter().map(String::as_str))
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

pub(crate) fn computed_page_result(
    instruction: &str,
    field_position: Option<PageRefPosition>,
) -> Option<String> {
    let spec = page_instruction(instruction)?;
    let field_position = field_position?;
    let text = format_page_ref_number(
        field_position.display_page,
        spec.number_format,
        field_position.display_format,
    )?;
    Some(apply_field_text_format(text, spec.text_format))
}

pub(crate) fn computed_page_ref_result(
    instruction: &str,
    page_refs: &PageRefContext,
    field_position: Option<PageRefPosition>,
) -> Option<String> {
    let spec = page_ref_instruction(instruction)?;
    let target = page_refs.target(&spec.target)?;
    let text = if spec.relative {
        computed_relative_page_ref_result(&spec, page_refs, field_position)?
    } else {
        format_page_ref_number(
            target.display_page,
            spec.number_format,
            target.display_format,
        )?
    };
    Some(apply_field_text_format(text, spec.text_format))
}

fn computed_relative_page_ref_result(
    spec: &PageRefInstruction,
    page_refs: &PageRefContext,
    field_position: Option<PageRefPosition>,
) -> Option<String> {
    let target = page_refs.target_position(&spec.target)?;
    let field = field_position?;
    if target.physical_page == field.physical_page {
        return Some(if target.order <= field.order {
            "above".to_string()
        } else {
            "below".to_string()
        });
    }
    Some(format!(
        "on page {}",
        format_page_ref_number(
            target.display_page,
            spec.number_format,
            target.display_format
        )?
    ))
}

pub(crate) fn computed_note_ref_result(
    instruction: &str,
    note_refs: &NoteRefContext,
    field_position: Option<NoteRefFieldPosition>,
) -> Option<String> {
    let spec = note_ref_instruction(instruction)?;
    let target = note_refs.target(&spec.target)?;
    let text = if spec.relative {
        computed_relative_note_ref_result(target, field_position)?
    } else {
        target.number.to_string()
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
    let mut number_format = None;
    let mut text_format = None;
    while let Some(part) = parts.next() {
        if part == "\\*" {
            if !accept_page_field_format_switch(parts.next()?, &mut number_format, &mut text_format)
            {
                return None;
            }
            continue;
        }
        if let Some(format) = part.strip_prefix("\\*") {
            if !accept_page_field_format_switch(format, &mut number_format, &mut text_format) {
                return None;
            }
            continue;
        }
        return None;
    }
    Some(SectionInstruction {
        result,
        number_format,
        text_format,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct QuoteInstruction {
    text: String,
    text_format: Option<FieldTextFormat>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FillInInstruction {
    default: Option<String>,
    text_format: Option<FieldTextFormat>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AskInstruction {
    bookmark: String,
    default: Option<String>,
}

pub(crate) fn computed_dynamic_result(instruction: &str) -> Option<String> {
    computed_formula_result(instruction)
        .or_else(|| computed_quote_result(instruction))
        .or_else(|| computed_fill_in_result(instruction))
        .or_else(|| computed_if_result(instruction))
        .or_else(|| computed_compare_result(instruction))
        .or_else(|| computed_merge_control_result(instruction))
}

fn computed_merge_control_result(instruction: &str) -> Option<String> {
    merge_control_instruction(instruction)?;
    Some(String::new())
}

fn merge_control_instruction(instruction: &str) -> Option<()> {
    let tokens = instruction_parts(instruction);
    let mut parts = tokens.iter().map(String::as_str);
    let kind = parts.next()?;
    if kind.eq_ignore_ascii_case("NEXT") {
        accept_field_format_tail(&mut parts)?;
        return Some(());
    }
    if kind.eq_ignore_ascii_case("NEXTIF") || kind.eq_ignore_ascii_case("SKIPIF") {
        let first = parts.next()?;
        let (left, operator, right) = comparison_operands(first, &mut parts)?;
        compare_if_operands(&left, operator, &right)?;
        accept_field_format_tail(&mut parts)?;
        return Some(());
    }
    None
}

pub(crate) fn supports_merge_control_field_syntax(instruction: &str) -> bool {
    merge_control_field_syntax(instruction).is_some()
}

fn merge_control_field_syntax(instruction: &str) -> Option<()> {
    let tokens = instruction_parts(instruction);
    let mut parts = tokens.iter().map(String::as_str);
    let kind = parts.next()?;
    if kind.eq_ignore_ascii_case("NEXT") {
        return accept_field_format_tail(&mut parts);
    }
    if kind.eq_ignore_ascii_case("NEXTIF") || kind.eq_ignore_ascii_case("SKIPIF") {
        let first = parts.next()?;
        merge_control_comparison_syntax(first, &mut parts)?;
        return accept_field_format_tail(&mut parts);
    }
    None
}

fn merge_control_comparison_syntax<'a, I>(first: &str, parts: &mut I) -> Option<()>
where
    I: Iterator<Item = &'a str>,
{
    if let Some((left, operator, right)) = compact_merge_control_comparison(first) {
        return (merge_control_operand_syntax(left)
            && if_operator(operator).is_some()
            && merge_control_operand_syntax(right))
        .then_some(());
    }
    merge_control_operand_syntax(first).then_some(())?;
    if_operator(parts.next()?)?;
    merge_control_operand_syntax(parts.next()?).then_some(())
}

fn compact_merge_control_comparison(token: &str) -> Option<(&str, &str, &str)> {
    for operator in [">=", "<=", "<>", "=", ">", "<"] {
        let Some(index) = find_unquoted_operator(token, operator) else {
            continue;
        };
        let (left, right_with_operator) = token.split_at(index);
        let right = &right_with_operator[operator.len()..];
        if left.is_empty() || right.is_empty() {
            return None;
        }
        return Some((left, operator, right));
    }
    None
}

fn merge_control_operand_syntax(token: &str) -> bool {
    field_literal_token(token).is_some_and(|value| !value.is_empty())
}

fn accept_field_format_tail<'a, I>(parts: &mut I) -> Option<()>
where
    I: Iterator<Item = &'a str>,
{
    let mut text_format = None;
    while let Some(part) = parts.next() {
        if part == "\\*" {
            if !accept_field_format_switch(parts.next()?, &mut text_format) {
                return None;
            }
            continue;
        }
        if let Some(format) = part.strip_prefix("\\*") {
            if accept_field_format_switch(format, &mut text_format) {
                continue;
            }
        }
        return None;
    }
    Some(())
}

fn accept_neutral_field_format_tail<'a, I>(parts: &mut I) -> Option<()>
where
    I: Iterator<Item = &'a str>,
{
    while let Some(part) = parts.next() {
        if part == "\\*" {
            if !is_neutral_field_format_switch(parts.next()?) {
                return None;
            }
            continue;
        }
        if let Some(format) = part.strip_prefix("\\*") {
            if is_neutral_field_format_switch(format) {
                continue;
            }
        }
        return None;
    }
    Some(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SetInstruction {
    name: String,
    value: String,
}

pub(crate) fn computed_set_result(
    instruction: &str,
    field_bookmarks: &mut HashMap<String, String>,
) -> Option<String> {
    let spec = set_instruction(instruction)?;
    field_bookmarks.insert(spec.name, spec.value);
    Some(String::new())
}

fn set_instruction(instruction: &str) -> Option<SetInstruction> {
    let tokens = instruction_parts(instruction);
    let mut parts = tokens.iter().map(String::as_str);
    let kind = parts.next()?;
    if !kind.eq_ignore_ascii_case("SET") {
        return None;
    }
    let name = field_identifier_token(parts.next()?)?;
    let value = set_value_literal(parts.next()?)?;
    accept_field_format_tail(&mut parts)?;
    Some(SetInstruction {
        name: name.to_string(),
        value,
    })
}

fn set_value_literal(token: &str) -> Option<String> {
    if let Some(value) = quoted_literal_text(token) {
        return Some(value);
    }
    (!token.is_empty() && !token.starts_with('\\') && !token.contains('"'))
        .then(|| token.to_string())
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DocumentInfoInstruction {
    property: DocumentInfoProperty,
    text_format: Option<FieldTextFormat>,
    date_format: Option<String>,
    file_size_unit: FileSizeUnit,
    user_override: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum DocumentInfoProperty {
    Title,
    Subject,
    Creator,
    Description,
    Keywords,
    Category,
    ContentStatus,
    LastModifiedBy,
    CreatedDate,
    SavedDate,
    PrintDate,
    Version,
    Custom(String),
    Variable(String),
    Extended(String),
    FileSize,
    UserName,
    UserInitials,
    UserAddress,
    DisplayOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FileSizeUnit {
    Bytes,
    Kilobytes,
    Megabytes,
}

pub(crate) fn computed_document_info_result(
    instruction: &str,
    core_properties: &CoreProperties,
    custom_properties: &HashMap<String, String>,
    document_variables: &HashMap<String, String>,
    extended_properties: &HashMap<String, String>,
    file_size_bytes: Option<usize>,
) -> Option<String> {
    let spec = document_info_instruction(instruction)?;
    let text = match spec.property {
        DocumentInfoProperty::Title => core_properties.title.clone()?,
        DocumentInfoProperty::Subject => core_properties.subject.clone()?,
        DocumentInfoProperty::Creator => core_properties.creator.clone()?,
        DocumentInfoProperty::Description => core_properties.description.clone()?,
        DocumentInfoProperty::Keywords => core_properties.keywords.clone()?,
        DocumentInfoProperty::Category => core_properties.category.clone()?,
        DocumentInfoProperty::ContentStatus => core_properties.content_status.clone()?,
        DocumentInfoProperty::LastModifiedBy => core_properties.last_modified_by.clone()?,
        DocumentInfoProperty::CreatedDate => core_properties.created.clone()?,
        DocumentInfoProperty::SavedDate => core_properties.modified.clone()?,
        DocumentInfoProperty::PrintDate => core_properties.last_printed.clone()?,
        DocumentInfoProperty::Version => core_properties.version.clone()?,
        DocumentInfoProperty::Custom(key) => custom_properties.get(&key).cloned()?,
        DocumentInfoProperty::Variable(key) => document_variables.get(&key).cloned()?,
        DocumentInfoProperty::Extended(key) => extended_properties.get(&key).cloned()?,
        DocumentInfoProperty::FileSize => {
            format_file_size_result(file_size_bytes?, spec.file_size_unit)
        }
        DocumentInfoProperty::UserName
        | DocumentInfoProperty::UserInitials
        | DocumentInfoProperty::UserAddress => spec.user_override.clone()?,
        DocumentInfoProperty::DisplayOnly => return None,
    };
    let text = match spec.date_format {
        Some(format) => format_core_timestamp(&text, &format)?,
        None => text,
    };
    Some(apply_field_text_format(text, spec.text_format))
}

pub(crate) fn supports_document_info_field_syntax(instruction: &str) -> bool {
    document_info_instruction(instruction).is_some()
}

fn document_info_instruction(instruction: &str) -> Option<DocumentInfoInstruction> {
    let tokens = instruction_parts(instruction);
    let mut parts = tokens.iter().map(String::as_str);
    let kind = parts.next()?;
    let mut text_format = None;
    let mut date_format = None;
    let mut file_size_unit = FileSizeUnit::Bytes;
    let mut file_size_unit_seen = false;
    let mut user_override = None;
    let property = if kind.eq_ignore_ascii_case("DOCPROPERTY") {
        doc_property_instruction_property(field_name_token(parts.next()?)?)?
    } else if kind.eq_ignore_ascii_case("DOCVARIABLE") {
        let name = field_name_token(parts.next()?)?;
        (!name.is_empty()).then(|| DocumentInfoProperty::Variable(document_property_key(name)))?
    } else if kind.eq_ignore_ascii_case("INFO") {
        document_info_property(field_name_token(parts.next()?)?)
            .unwrap_or(DocumentInfoProperty::DisplayOnly)
    } else if let Some(property) = user_info_property(kind) {
        property
    } else if kind.eq_ignore_ascii_case("DATE") || kind.eq_ignore_ascii_case("TIME") {
        DocumentInfoProperty::DisplayOnly
    } else {
        document_info_property(kind)?
    };
    while let Some(part) = parts.next() {
        if part == "\\*" {
            if !accept_field_format_switch(parts.next()?, &mut text_format) {
                return None;
            }
            continue;
        }
        if let Some(format) = part.strip_prefix("\\*") {
            if !accept_field_format_switch(format, &mut text_format) {
                return None;
            }
            continue;
        }
        if part.eq_ignore_ascii_case("\\@") {
            if date_format.is_some() {
                return None;
            }
            let format = field_literal_token(parts.next()?)?;
            if format.is_empty() {
                return None;
            }
            date_format = Some(format.to_string());
            continue;
        }
        if let Some(format) = strip_ascii_switch_prefix(part, "\\@") {
            let format = field_literal_token(format)?;
            if format.is_empty() || date_format.is_some() {
                return None;
            }
            date_format = Some(format.to_string());
            continue;
        }
        if let Some(unit) = file_size_unit_switch(part) {
            if !matches!(&property, DocumentInfoProperty::FileSize) || file_size_unit_seen {
                return None;
            }
            file_size_unit = unit;
            file_size_unit_seen = true;
            continue;
        }
        if is_user_info_property(&property) && !part.starts_with('\\') {
            let value = quoted_literal_text(part)?;
            if user_override.replace(value).is_some() {
                return None;
            }
            continue;
        }
        return None;
    }
    Some(DocumentInfoInstruction {
        property,
        text_format,
        date_format,
        file_size_unit,
        user_override,
    })
}

fn field_name_token(value: &str) -> Option<&str> {
    let value = value.trim();
    let value = match (value.starts_with('"'), value.ends_with('"')) {
        (true, true) if value.len() >= 2 => &value[1..value.len() - 1],
        (true, _) | (_, true) => return None,
        (false, false) => value,
    }
    .trim();
    if value.is_empty() || value.starts_with('\\') || value.contains('"') {
        return None;
    }
    Some(value)
}

fn field_literal_token(value: &str) -> Option<&str> {
    let value = value.trim();
    let value = match (value.starts_with('"'), value.ends_with('"')) {
        (true, true) if value.len() >= 2 => &value[1..value.len() - 1],
        (true, _) | (_, true) => return None,
        (false, false) => value,
    };
    (!value.contains('"')).then_some(value)
}

fn file_size_unit_switch(part: &str) -> Option<FileSizeUnit> {
    if part.eq_ignore_ascii_case("\\k") {
        return Some(FileSizeUnit::Kilobytes);
    }
    if part.eq_ignore_ascii_case("\\m") {
        return Some(FileSizeUnit::Megabytes);
    }
    None
}

fn format_file_size_result(bytes: usize, unit: FileSizeUnit) -> String {
    match unit {
        FileSizeUnit::Bytes => bytes.to_string(),
        FileSizeUnit::Kilobytes => rounded_file_size_unit(bytes, 1_000).to_string(),
        FileSizeUnit::Megabytes => rounded_file_size_unit(bytes, 1_000_000).to_string(),
    }
}

fn rounded_file_size_unit(bytes: usize, divisor: usize) -> usize {
    bytes.saturating_add(divisor / 2) / divisor
}

fn user_info_property(value: &str) -> Option<DocumentInfoProperty> {
    Some(match value.to_ascii_uppercase().as_str() {
        "USERNAME" => DocumentInfoProperty::UserName,
        "USERINITIALS" => DocumentInfoProperty::UserInitials,
        "USERADDRESS" => DocumentInfoProperty::UserAddress,
        _ => return None,
    })
}

fn is_user_info_property(property: &DocumentInfoProperty) -> bool {
    matches!(
        property,
        DocumentInfoProperty::UserName
            | DocumentInfoProperty::UserInitials
            | DocumentInfoProperty::UserAddress
    )
}

fn doc_property_instruction_property(value: &str) -> Option<DocumentInfoProperty> {
    document_info_property(value).or_else(|| {
        (!value.is_empty()).then(|| DocumentInfoProperty::Custom(document_property_key(value)))
    })
}

fn document_info_property(value: &str) -> Option<DocumentInfoProperty> {
    Some(match document_property_key(value).as_str() {
        "TITLE" => DocumentInfoProperty::Title,
        "SUBJECT" => DocumentInfoProperty::Subject,
        "AUTHOR" | "CREATOR" => DocumentInfoProperty::Creator,
        "COMMENTS" | "COMMENT" | "DESCRIPTION" => DocumentInfoProperty::Description,
        "KEYWORDS" | "KEYWORD" => DocumentInfoProperty::Keywords,
        "CATEGORY" => DocumentInfoProperty::Category,
        "CONTENTSTATUS" => DocumentInfoProperty::ContentStatus,
        "LASTSAVEDBY" | "LASTMODIFIEDBY" => DocumentInfoProperty::LastModifiedBy,
        "CREATEDATE" => DocumentInfoProperty::CreatedDate,
        "SAVEDATE" => DocumentInfoProperty::SavedDate,
        "PRINTDATE" => DocumentInfoProperty::PrintDate,
        "VERSION" => DocumentInfoProperty::Version,
        "FILESIZE" => DocumentInfoProperty::FileSize,
        "APPLICATION" => DocumentInfoProperty::Extended(document_property_key("Application")),
        "APPVERSION" => DocumentInfoProperty::Extended(document_property_key("AppVersion")),
        "COMPANY" => DocumentInfoProperty::Extended(document_property_key("Company")),
        "DOCSECURITY" => DocumentInfoProperty::Extended(document_property_key("DocSecurity")),
        "HIDDENSLIDES" => DocumentInfoProperty::Extended(document_property_key("HiddenSlides")),
        "HYPERLINKBASE" => DocumentInfoProperty::Extended(document_property_key("HyperlinkBase")),
        "HYPERLINKSCHANGED" => {
            DocumentInfoProperty::Extended(document_property_key("HyperlinksChanged"))
        }
        "LINES" => DocumentInfoProperty::Extended(document_property_key("Lines")),
        "LINKSUPTODATE" => DocumentInfoProperty::Extended(document_property_key("LinksUpToDate")),
        "MANAGER" => DocumentInfoProperty::Extended(document_property_key("Manager")),
        "MMCLIPS" => DocumentInfoProperty::Extended(document_property_key("MMClips")),
        "NOTES" => DocumentInfoProperty::Extended(document_property_key("Notes")),
        "PAGES" | "NUMPAGES" => DocumentInfoProperty::Extended(document_property_key("Pages")),
        "PARAGRAPHS" => DocumentInfoProperty::Extended(document_property_key("Paragraphs")),
        "PRESENTATIONFORMAT" => {
            DocumentInfoProperty::Extended(document_property_key("PresentationFormat"))
        }
        "SCALECROP" => DocumentInfoProperty::Extended(document_property_key("ScaleCrop")),
        "SHAREDDOC" => DocumentInfoProperty::Extended(document_property_key("SharedDoc")),
        "SLIDES" => DocumentInfoProperty::Extended(document_property_key("Slides")),
        "WORDS" | "NUMWORDS" => DocumentInfoProperty::Extended(document_property_key("Words")),
        "CHARACTERS" | "NUMCHARS" => {
            DocumentInfoProperty::Extended(document_property_key("Characters"))
        }
        "CHARACTERSWITHSPACES" => {
            DocumentInfoProperty::Extended(document_property_key("CharactersWithSpaces"))
        }
        "TOTALTIME" | "EDITTIME" => {
            DocumentInfoProperty::Extended(document_property_key("TotalTime"))
        }
        "TEMPLATE" => DocumentInfoProperty::Extended(document_property_key("Template")),
        _ => return None,
    })
}

pub(crate) fn document_property_key(value: &str) -> String {
    value
        .chars()
        .filter(|ch| *ch != '_' && *ch != '-' && !ch.is_whitespace())
        .collect::<String>()
        .to_ascii_uppercase()
}

pub(crate) fn computed_revision_number_result(
    instruction: &str,
    core_properties: &CoreProperties,
) -> Option<String> {
    let tokens = instruction_parts(instruction);
    let mut parts = tokens.iter().map(String::as_str);
    let kind = parts.next()?;
    if !kind.eq_ignore_ascii_case("REVNUM") {
        return None;
    }
    let mut text_format = None;
    while let Some(part) = parts.next() {
        if part == "\\*" {
            if !accept_field_format_switch(parts.next()?, &mut text_format) {
                return None;
            }
            continue;
        }
        if let Some(format) = part.strip_prefix("\\*") {
            if !accept_field_format_switch(format, &mut text_format) {
                return None;
            }
            continue;
        }
        return None;
    }
    let revision = core_properties.revision.clone()?;
    Some(apply_field_text_format(revision, text_format))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CoreTimestamp {
    year: i32,
    month: u8,
    day: u8,
    hour: u8,
    minute: u8,
    second: u8,
}

const MONTH_SHORT_NAMES: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];
const MONTH_LONG_NAMES: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];
const WEEKDAY_SHORT_NAMES: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
const WEEKDAY_LONG_NAMES: [&str; 7] = [
    "Sunday",
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
];

fn format_core_timestamp(value: &str, format: &str) -> Option<String> {
    let timestamp = parse_core_timestamp(value)?;
    let mut out = String::new();
    let chars: Vec<char> = format.chars().collect();
    let mut i = 0usize;
    while i < chars.len() {
        let ch = chars[i];
        if ch == '\'' {
            i += 1;
            while i < chars.len() && chars[i] != '\'' {
                out.push(chars[i]);
                i += 1;
            }
            if i < chars.len() {
                i += 1;
            }
            continue;
        }
        if starts_with_ci(&chars, i, "AM/PM") {
            out.push_str(if timestamp.hour < 12 { "AM" } else { "PM" });
            i += 5;
            continue;
        }
        let run = repeated_run(&chars, i, ch);
        match ch {
            'y' | 'Y' => {
                if run >= 4 {
                    out.push_str(&format!("{:04}", timestamp.year));
                } else if run == 2 {
                    out.push_str(&format!("{:02}", timestamp.year.rem_euclid(100)));
                } else {
                    return None;
                }
            }
            'M' => match run {
                1 | 2 => push_numeric(&mut out, timestamp.month as u32, run)?,
                3 => out.push_str(MONTH_SHORT_NAMES.get(timestamp.month as usize - 1)?),
                4 => out.push_str(MONTH_LONG_NAMES.get(timestamp.month as usize - 1)?),
                _ => return None,
            },
            'd' | 'D' => match run {
                1 | 2 => push_numeric(&mut out, timestamp.day as u32, run)?,
                3 => out.push_str(WEEKDAY_SHORT_NAMES.get(weekday_index(&timestamp)?)?),
                4 => out.push_str(WEEKDAY_LONG_NAMES.get(weekday_index(&timestamp)?)?),
                _ => return None,
            },
            'H' => push_numeric(&mut out, timestamp.hour as u32, run)?,
            'h' => {
                let hour = timestamp.hour % 12;
                push_numeric(&mut out, if hour == 0 { 12 } else { hour } as u32, run)?;
            }
            'm' => push_numeric(&mut out, timestamp.minute as u32, run)?,
            's' | 'S' => push_numeric(&mut out, timestamp.second as u32, run)?,
            _ => {
                out.push(ch);
                i += 1;
                continue;
            }
        }
        i += run;
    }
    Some(out)
}

fn parse_core_timestamp(value: &str) -> Option<CoreTimestamp> {
    let value = value.trim();
    let date = value.get(0..10)?;
    let year = date.get(0..4)?.parse::<i32>().ok()?;
    (date.get(4..5)? == "-" && date.get(7..8)? == "-").then_some(())?;
    let month = date.get(5..7)?.parse::<u8>().ok()?;
    let day = date.get(8..10)?.parse::<u8>().ok()?;
    let mut hour = 0u8;
    let mut minute = 0u8;
    let mut second = 0u8;
    if value.len() >= 19 && matches!(value.as_bytes().get(10), Some(b'T' | b' ')) {
        hour = value.get(11..13)?.parse::<u8>().ok()?;
        minute = value.get(14..16)?.parse::<u8>().ok()?;
        second = value.get(17..19)?.parse::<u8>().ok()?;
        (value.get(13..14)? == ":" && value.get(16..17)? == ":").then_some(())?;
    }
    ((1..=12).contains(&month)
        && day >= 1
        && day <= days_in_month(year, month)?
        && hour <= 23
        && minute <= 59
        && second <= 59)
        .then_some(CoreTimestamp {
            year,
            month,
            day,
            hour,
            minute,
            second,
        })
}

fn days_in_month(year: i32, month: u8) -> Option<u8> {
    Some(match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap_year(year) => 29,
        2 => 28,
        _ => return None,
    })
}

fn is_leap_year(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

fn weekday_index(timestamp: &CoreTimestamp) -> Option<usize> {
    let month_offset = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
    let month_index = timestamp.month.checked_sub(1)? as usize;
    let mut year = timestamp.year;
    if timestamp.month < 3 {
        year -= 1;
    }
    Some(
        (year + year / 4 - year / 100
            + year / 400
            + month_offset[month_index]
            + timestamp.day as i32)
            .rem_euclid(7) as usize,
    )
}

fn repeated_run(chars: &[char], start: usize, ch: char) -> usize {
    chars[start..]
        .iter()
        .take_while(|next| **next == ch)
        .count()
}

fn starts_with_ci(chars: &[char], start: usize, pattern: &str) -> bool {
    let Some(slice) = chars.get(start..start + pattern.chars().count()) else {
        return false;
    };
    slice
        .iter()
        .zip(pattern.chars())
        .all(|(actual, expected)| actual.eq_ignore_ascii_case(&expected))
}

fn push_numeric(out: &mut String, value: u32, width: usize) -> Option<()> {
    match width {
        1 => out.push_str(&value.to_string()),
        2 => out.push_str(&format!("{value:02}")),
        _ => return None,
    }
    Some(())
}

fn computed_formula_result(instruction: &str) -> Option<String> {
    let spec = formula_instruction(instruction)?;
    if spec.expression.is_empty() || spec.expression.contains(['\\', '"']) {
        return None;
    }
    let mut parser = FormulaParser::new(&spec.expression);
    let value = parser.parse()?;
    match spec.number_format {
        Some(format) => format_formula_number(value, &format),
        None => formula_number_text(value),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FormulaInstruction {
    expression: String,
    number_format: Option<String>,
}

enum FormulaNumberFormatSwitch {
    Separate(usize),
    Compact { index: usize, picture: String },
}

fn formula_instruction(instruction: &str) -> Option<FormulaInstruction> {
    let body = instruction.trim().strip_prefix('=')?.trim();
    if body.is_empty() {
        return None;
    }
    let tokens = instruction_parts(body);
    let Some(format_switch) = formula_number_format_switch(&tokens) else {
        let tail_index = tokens.iter().position(|part| is_field_format_start(part));
        if let Some(tail_index) = tail_index {
            if tail_index == 0 {
                return None;
            }
            let mut tail = tokens[tail_index..].iter().map(String::as_str);
            accept_neutral_field_format_tail(&mut tail)?;
            return Some(FormulaInstruction {
                expression: tokens[..tail_index].join(" "),
                number_format: None,
            });
        }
        return Some(FormulaInstruction {
            expression: body.to_string(),
            number_format: None,
        });
    };
    let (format_index, picture, tail_start) = match format_switch {
        FormulaNumberFormatSwitch::Separate(format_index) => (
            format_index,
            formula_number_format_picture(tokens.get(format_index + 1)?)?,
            format_index + 2,
        ),
        FormulaNumberFormatSwitch::Compact { index, picture } => {
            (index, formula_number_format_picture(&picture)?, index + 1)
        }
    };
    if format_index == 0 {
        return None;
    }
    let mut index = tail_start;
    while index < tokens.len() {
        let part = &tokens[index];
        if part == "\\*" {
            index += 1;
            if !is_neutral_field_format_switch(tokens.get(index)?) {
                return None;
            }
            index += 1;
            continue;
        }
        if let Some(format) = part.strip_prefix("\\*") {
            if !is_neutral_field_format_switch(format) {
                return None;
            }
            index += 1;
            continue;
        }
        return None;
    }
    Some(FormulaInstruction {
        expression: tokens[..format_index].join(" "),
        number_format: Some(picture),
    })
}

fn formula_number_format_switch(tokens: &[String]) -> Option<FormulaNumberFormatSwitch> {
    tokens.iter().enumerate().find_map(|(index, part)| {
        if part == "\\#" {
            return Some(FormulaNumberFormatSwitch::Separate(index));
        }
        let picture = strip_ascii_switch_prefix(part, "\\#")?;
        (!picture.is_empty()).then(|| FormulaNumberFormatSwitch::Compact {
            index,
            picture: picture.to_string(),
        })
    })
}

fn formula_number_format_picture(token: &str) -> Option<String> {
    if let Some(text) = quoted_literal_text(token) {
        return Some(text);
    }
    (!token.contains('"') && !token.starts_with('\\')).then(|| token.to_string())
}

fn computed_table_formula_result(
    instruction: &str,
    rows: &[Vec<TableFormulaCell>],
    row: usize,
    col: usize,
) -> Option<String> {
    let spec = formula_instruction(instruction)?;
    if spec.expression.is_empty() || spec.expression.contains(['\\', '"']) {
        return None;
    }
    let expression = resolved_table_formula_expression(&spec.expression, rows, row, col)?;
    let mut parser = FormulaParser::new(&expression);
    let value = parser.parse()?;
    match spec.number_format {
        Some(format) => format_formula_number(value, &format),
        None => formula_number_text(value),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TableFormulaDirection {
    Left,
    Right,
    Above,
    Below,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TableFormulaArgument {
    Direction(TableFormulaDirection),
    CurrentRow,
    CurrentColumn,
    Range {
        start_row: usize,
        start_col: usize,
        end_row: usize,
        end_col: usize,
    },
}

fn resolved_table_formula_expression(
    expression: &str,
    rows: &[Vec<TableFormulaCell>],
    row: usize,
    col: usize,
) -> Option<String> {
    let chars: Vec<_> = expression.chars().collect();
    let mut output = String::new();
    let mut pos = 0usize;
    let mut changed = false;
    while pos < chars.len() {
        if chars[pos].is_ascii_alphabetic() {
            let start = pos;
            while pos < chars.len() && chars[pos].is_ascii_alphabetic() {
                pos += 1;
            }
            let name: String = chars[start..pos].iter().collect();
            let mut after_name = pos;
            while after_name < chars.len() && chars[after_name].is_whitespace() {
                after_name += 1;
            }
            if after_name < chars.len() && chars[after_name] == '(' {
                if let Some(close) = matching_formula_paren(&chars, after_name) {
                    let inner: String = chars[after_name + 1..close].iter().collect();
                    if let Some(value) =
                        computed_table_formula_call_value(&name, &inner, rows, row, col)
                    {
                        output.push_str(&formula_number_text(value)?);
                        pos = close + 1;
                        changed = true;
                        continue;
                    }
                    if let Some(inner) = resolved_table_formula_expression(&inner, rows, row, col) {
                        output.push_str(&name);
                        output.push('(');
                        output.push_str(&inner);
                        output.push(')');
                        pos = close + 1;
                        changed = true;
                        continue;
                    }
                }
            }
            output.push_str(&name);
            continue;
        }
        output.push(chars[pos]);
        pos += 1;
    }
    changed.then_some(output)
}

fn matching_formula_paren(chars: &[char], open: usize) -> Option<usize> {
    if chars.get(open) != Some(&'(') {
        return None;
    }
    let mut depth = 0usize;
    for (index, ch) in chars.iter().enumerate().skip(open + 1) {
        match ch {
            '(' => depth += 1,
            ')' if depth == 0 => return Some(index),
            ')' => depth = depth.checked_sub(1)?,
            _ => {}
        }
    }
    None
}

fn computed_table_formula_call_value(
    function: &str,
    expression: &str,
    rows: &[Vec<TableFormulaCell>],
    row: usize,
    col: usize,
) -> Option<f64> {
    let arguments = table_formula_arguments(expression)?;
    let values = table_formula_values(rows, row, col, &arguments)?;
    if values.is_empty() {
        return None;
    }
    eval_formula_function(&function.to_ascii_uppercase(), &values)
}

fn table_formula_arguments(expression: &str) -> Option<Vec<TableFormulaArgument>> {
    if expression.trim().ends_with([',', ';']) {
        return None;
    }
    let mut arguments = Vec::new();
    let mut separator = None;
    for raw in expression.split_inclusive([',', ';']) {
        let (part, current_separator) = match raw.chars().last() {
            Some(ch @ (',' | ';')) => (&raw[..raw.len() - ch.len_utf8()], Some(ch)),
            _ => (raw, None),
        };
        let argument = table_formula_argument(part.trim())?;
        if arguments.contains(&argument) {
            return None;
        }
        arguments.push(argument);
        if let Some(current_separator) = current_separator {
            if separator
                .replace(current_separator)
                .is_some_and(|seen| seen != current_separator)
            {
                return None;
            }
        }
    }
    (!arguments.is_empty()).then_some(arguments)
}

fn table_formula_argument(value: &str) -> Option<TableFormulaArgument> {
    if let Some(direction) = table_formula_direction(value) {
        return Some(TableFormulaArgument::Direction(direction));
    }
    if value.eq_ignore_ascii_case("R") {
        return Some(TableFormulaArgument::CurrentRow);
    }
    if value.eq_ignore_ascii_case("C") {
        return Some(TableFormulaArgument::CurrentColumn);
    }
    table_formula_range_reference(value)
}

fn table_formula_direction(value: &str) -> Option<TableFormulaDirection> {
    match value.to_ascii_uppercase().as_str() {
        "LEFT" => Some(TableFormulaDirection::Left),
        "RIGHT" => Some(TableFormulaDirection::Right),
        "ABOVE" => Some(TableFormulaDirection::Above),
        "BELOW" => Some(TableFormulaDirection::Below),
        _ => None,
    }
}

fn table_formula_range_reference(value: &str) -> Option<TableFormulaArgument> {
    let (start, end) = match value.split_once(':') {
        Some((start, end)) if !start.is_empty() && !end.is_empty() => (
            table_formula_cell_reference(start)?,
            table_formula_cell_reference(end)?,
        ),
        Some(_) => return None,
        None => {
            let cell = table_formula_cell_reference(value)?;
            (cell, cell)
        }
    };
    Some(TableFormulaArgument::Range {
        start_row: start.0.min(end.0),
        start_col: start.1.min(end.1),
        end_row: start.0.max(end.0),
        end_col: start.1.max(end.1),
    })
}

fn table_formula_cell_reference(value: &str) -> Option<(usize, usize)> {
    parse_rncn_cell_reference(value).or_else(|| parse_a1_cell_reference(value))
}

fn parse_rncn_cell_reference(value: &str) -> Option<(usize, usize)> {
    let value = value.trim();
    let rest = value
        .strip_prefix('R')
        .or_else(|| value.strip_prefix('r'))?;
    let c_index = rest.find(['C', 'c'])?;
    let (row, col_with_c) = rest.split_at(c_index);
    let col = &col_with_c[1..];
    if row.is_empty() || col.is_empty() {
        return None;
    }
    let row = row.parse::<usize>().ok()?.checked_sub(1)?;
    let col = col.parse::<usize>().ok()?.checked_sub(1)?;
    Some((row, col))
}

fn parse_a1_cell_reference(value: &str) -> Option<(usize, usize)> {
    let value = value.trim();
    let split = value.find(|ch: char| ch.is_ascii_digit())?;
    let (col, row) = value.split_at(split);
    if col.is_empty()
        || row.is_empty()
        || !col.chars().all(|ch| ch.is_ascii_alphabetic())
        || !row.chars().all(|ch| ch.is_ascii_digit())
    {
        return None;
    }
    let mut col_index = 0usize;
    for ch in col.chars() {
        let digit = ch.to_ascii_uppercase() as u8 - b'A' + 1;
        col_index = col_index.checked_mul(26)?.checked_add(digit as usize)?;
    }
    let row_index = row.parse::<usize>().ok()?.checked_sub(1)?;
    Some((row_index, col_index.checked_sub(1)?))
}

fn table_formula_values(
    rows: &[Vec<TableFormulaCell>],
    row: usize,
    col: usize,
    arguments: &[TableFormulaArgument],
) -> Option<Vec<f64>> {
    let mut values = Vec::new();
    for argument in arguments {
        push_table_formula_argument_values(rows, row, col, *argument, &mut values)?;
    }
    Some(values)
}

fn push_table_formula_argument_values(
    rows: &[Vec<TableFormulaCell>],
    row: usize,
    col: usize,
    argument: TableFormulaArgument,
    values: &mut Vec<f64>,
) -> Option<()> {
    match argument {
        TableFormulaArgument::Direction(TableFormulaDirection::Left) => {
            for cell in rows.get(row)?.get(..col)? {
                push_table_formula_cell_number(cell, values)?;
            }
        }
        TableFormulaArgument::Direction(TableFormulaDirection::Right) => {
            for cell in rows.get(row)?.get(col + 1..)? {
                push_table_formula_cell_number(cell, values)?;
            }
        }
        TableFormulaArgument::Direction(TableFormulaDirection::Above) => {
            for table_row in rows.get(..row)? {
                if let Some(cell) = table_row.get(col) {
                    push_table_formula_cell_number(cell, values)?;
                }
            }
        }
        TableFormulaArgument::Direction(TableFormulaDirection::Below) => {
            for table_row in rows.get(row + 1..)? {
                if table_row.iter().any(|cell| cell.contains_formula) {
                    break;
                }
                if let Some(cell) = table_row.get(col) {
                    push_table_formula_cell_number(cell, values)?;
                }
            }
        }
        TableFormulaArgument::CurrentRow => {
            for (cell_index, cell) in rows.get(row)?.iter().enumerate() {
                if cell_index != col {
                    push_table_formula_cell_number(cell, values)?;
                }
            }
        }
        TableFormulaArgument::CurrentColumn => {
            for (row_index, table_row) in rows.iter().enumerate() {
                if row_index != row {
                    if let Some(cell) = table_row.get(col) {
                        push_table_formula_cell_number(cell, values)?;
                    }
                }
            }
        }
        TableFormulaArgument::Range {
            start_row,
            start_col,
            end_row,
            end_col,
        } => {
            for row_index in start_row..=end_row {
                let table_row = rows.get(row_index)?;
                for col_index in start_col..=end_col {
                    if row_index == row && col_index == col {
                        continue;
                    }
                    if let Some(cell) = table_row.get(col_index) {
                        push_table_formula_cell_number(cell, values)?;
                    }
                }
            }
        }
    }
    Some(())
}

fn push_table_formula_cell_number(cell: &TableFormulaCell, values: &mut Vec<f64>) -> Option<()> {
    if cell.contains_formula {
        return None;
    }
    let text = cell.text.trim();
    if text.is_empty() {
        return Some(());
    }
    values.push(text.parse::<f64>().ok()?);
    Some(())
}

#[derive(Debug, Clone)]
struct FormulaParser {
    chars: Vec<char>,
    pos: usize,
}

impl FormulaParser {
    fn new(expression: &str) -> Self {
        Self {
            chars: expression.chars().collect(),
            pos: 0,
        }
    }

    fn parse(&mut self) -> Option<f64> {
        let value = self.parse_comparison()?;
        self.skip_ws();
        (self.pos == self.chars.len()).then_some(value)
    }

    fn parse_comparison(&mut self) -> Option<f64> {
        let lhs = self.parse_expression()?;
        self.skip_ws();
        let Some(operator) = self.parse_comparison_operator() else {
            return Some(lhs);
        };
        let rhs = self.parse_expression()?;
        eval_formula_comparison(lhs, rhs, operator)
    }

    fn parse_expression(&mut self) -> Option<f64> {
        let mut value = self.parse_term()?;
        loop {
            self.skip_ws();
            match self.peek() {
                Some('+') => {
                    self.pos += 1;
                    value += self.parse_term()?;
                }
                Some('-') => {
                    self.pos += 1;
                    value -= self.parse_term()?;
                }
                _ => return Some(value),
            }
        }
    }

    fn parse_term(&mut self) -> Option<f64> {
        let mut value = self.parse_power()?;
        loop {
            self.skip_ws();
            match self.peek() {
                Some('*') => {
                    self.pos += 1;
                    value *= self.parse_power()?;
                }
                Some('/') => {
                    self.pos += 1;
                    let rhs = self.parse_power()?;
                    if rhs == 0.0 {
                        return None;
                    }
                    value /= rhs;
                }
                _ => return Some(value),
            }
        }
    }

    fn parse_power(&mut self) -> Option<f64> {
        let base = self.parse_factor()?;
        self.skip_ws();
        if self.peek() != Some('^') {
            return Some(base);
        }
        self.pos += 1;
        let exponent = self.parse_power()?;
        let value = base.powf(exponent);
        value.is_finite().then_some(value)
    }

    fn parse_factor(&mut self) -> Option<f64> {
        self.skip_ws();
        match self.peek()? {
            '+' => {
                self.pos += 1;
                self.parse_factor()
            }
            '-' => {
                self.pos += 1;
                self.parse_factor().map(|value| -value)
            }
            '(' => {
                self.pos += 1;
                let value = self.parse_comparison()?;
                self.skip_ws();
                if self.peek()? != ')' {
                    return None;
                }
                self.pos += 1;
                Some(value)
            }
            ch if ch.is_ascii_digit() || ch == '.' => self.parse_number(),
            ch if ch.is_ascii_alphabetic() => self.parse_function(),
            _ => None,
        }
    }

    fn parse_number(&mut self) -> Option<f64> {
        let start = self.pos;
        let mut saw_digit = false;
        let mut saw_dot = false;
        while let Some(ch) = self.peek() {
            if ch.is_ascii_digit() {
                saw_digit = true;
                self.pos += 1;
            } else if ch == '.' && !saw_dot {
                saw_dot = true;
                self.pos += 1;
            } else {
                break;
            }
        }
        if !saw_digit {
            return None;
        }
        if self.peek().is_some_and(|ch| matches!(ch, 'e' | 'E')) {
            self.pos += 1;
            if self.peek().is_some_and(|ch| matches!(ch, '+' | '-')) {
                self.pos += 1;
            }
            let exponent_start = self.pos;
            while self.peek().is_some_and(|ch| ch.is_ascii_digit()) {
                self.pos += 1;
            }
            if self.pos == exponent_start {
                return None;
            }
        }
        let value = self.chars[start..self.pos]
            .iter()
            .collect::<String>()
            .parse::<f64>()
            .ok()?;
        value.is_finite().then_some(value)
    }

    fn parse_function(&mut self) -> Option<f64> {
        let name = self.parse_identifier()?.to_ascii_uppercase();
        self.skip_ws();
        if self.peek() != Some('(') {
            return eval_formula_function(&name, &[]);
        }
        self.pos += 1;
        if name == "DEFINED" {
            return self.parse_defined_function();
        }
        let arguments = self.parse_function_arguments()?;
        eval_formula_function(&name, &arguments)
    }

    fn parse_defined_function(&mut self) -> Option<f64> {
        let start = self.pos;
        let mut depth = 0usize;
        while let Some(ch) = self.peek() {
            match ch {
                '(' => {
                    depth += 1;
                    self.pos += 1;
                }
                ')' if depth == 0 => {
                    let expression = self.chars[start..self.pos]
                        .iter()
                        .collect::<String>()
                        .trim()
                        .to_string();
                    if expression.is_empty() {
                        return None;
                    }
                    self.pos += 1;
                    let mut parser = FormulaParser::new(&expression);
                    return Some(parser.parse().is_some() as u8 as f64);
                }
                ')' => {
                    depth = depth.checked_sub(1)?;
                    self.pos += 1;
                }
                _ => self.pos += 1,
            }
        }
        None
    }

    fn parse_comparison_operator(&mut self) -> Option<FormulaComparisonOperator> {
        match self.peek()? {
            '<' => {
                self.pos += 1;
                if self.peek() == Some('=') {
                    self.pos += 1;
                    Some(FormulaComparisonOperator::LessOrEqual)
                } else if self.peek() == Some('>') {
                    self.pos += 1;
                    Some(FormulaComparisonOperator::NotEqual)
                } else {
                    Some(FormulaComparisonOperator::Less)
                }
            }
            '>' => {
                self.pos += 1;
                if self.peek() == Some('=') {
                    self.pos += 1;
                    Some(FormulaComparisonOperator::GreaterOrEqual)
                } else {
                    Some(FormulaComparisonOperator::Greater)
                }
            }
            '=' => {
                self.pos += 1;
                Some(FormulaComparisonOperator::Equal)
            }
            _ => None,
        }
    }

    fn parse_identifier(&mut self) -> Option<String> {
        let start = self.pos;
        while self.peek().is_some_and(|ch| ch.is_ascii_alphabetic()) {
            self.pos += 1;
        }
        (self.pos > start).then(|| self.chars[start..self.pos].iter().collect())
    }

    fn parse_function_arguments(&mut self) -> Option<Vec<f64>> {
        let mut arguments = Vec::new();
        let mut separator = None;
        self.skip_ws();
        if self.peek()? == ')' {
            self.pos += 1;
            return Some(arguments);
        }
        loop {
            arguments.push(self.parse_comparison()?);
            self.skip_ws();
            match self.peek()? {
                ',' | ';' => {
                    let current = self.peek()?;
                    if separator
                        .replace(current)
                        .is_some_and(|seen| seen != current)
                    {
                        return None;
                    }
                    self.pos += 1;
                    self.skip_ws();
                }
                ')' => {
                    self.pos += 1;
                    return Some(arguments);
                }
                _ => return None,
            }
        }
    }

    fn skip_ws(&mut self) {
        while self.peek().is_some_and(char::is_whitespace) {
            self.pos += 1;
        }
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FormulaComparisonOperator {
    Equal,
    NotEqual,
    Less,
    LessOrEqual,
    Greater,
    GreaterOrEqual,
}

fn eval_formula_comparison(lhs: f64, rhs: f64, operator: FormulaComparisonOperator) -> Option<f64> {
    if !lhs.is_finite() || !rhs.is_finite() {
        return None;
    }
    let equal = (lhs - rhs).abs() < 1e-12;
    let matches = match operator {
        FormulaComparisonOperator::Equal => equal,
        FormulaComparisonOperator::NotEqual => !equal,
        FormulaComparisonOperator::Less => lhs < rhs && !equal,
        FormulaComparisonOperator::LessOrEqual => lhs < rhs || equal,
        FormulaComparisonOperator::Greater => lhs > rhs && !equal,
        FormulaComparisonOperator::GreaterOrEqual => lhs > rhs || equal,
    };
    Some(matches as u8 as f64)
}

fn eval_formula_function(name: &str, arguments: &[f64]) -> Option<f64> {
    if !arguments.iter().all(|value| value.is_finite()) {
        return None;
    }
    match name {
        "ABS" if arguments.len() == 1 => Some(arguments[0].abs()),
        "AND" if !arguments.is_empty() => {
            Some((arguments.iter().all(|value| formula_truthy(*value))) as u8 as f64)
        }
        "AVERAGE" if !arguments.is_empty() => {
            Some(arguments.iter().sum::<f64>() / arguments.len() as f64)
        }
        "COUNT" if !arguments.is_empty() => Some(arguments.len() as f64),
        "FALSE" if arguments.is_empty() => Some(0.0),
        "IF" if arguments.len() == 3 => Some(if formula_truthy(arguments[0]) {
            arguments[1]
        } else {
            arguments[2]
        }),
        "INT" if arguments.len() == 1 => Some(arguments[0].floor()),
        "MAX" if !arguments.is_empty() => arguments.iter().copied().reduce(f64::max),
        "MIN" if !arguments.is_empty() => arguments.iter().copied().reduce(f64::min),
        "MOD" if arguments.len() == 2 => {
            if arguments[1].abs() < 1e-12 {
                None
            } else {
                Some(arguments[0] % arguments[1])
            }
        }
        "NOT" if arguments.len() == 1 => Some((!formula_truthy(arguments[0])) as u8 as f64),
        "OR" if !arguments.is_empty() => {
            Some((arguments.iter().any(|value| formula_truthy(*value))) as u8 as f64)
        }
        "PRODUCT" if !arguments.is_empty() => Some(arguments.iter().product()),
        "ROUND" if arguments.len() == 2 => {
            let digits = arguments[1].round();
            if (arguments[1] - digits).abs() > 1e-12 || !(-12.0..=12.0).contains(&digits) {
                return None;
            }
            let factor = 10_f64.powi(digits as i32);
            Some((arguments[0] * factor).round() / factor)
        }
        "SIGN" if arguments.len() == 1 => {
            if arguments[0].abs() < 1e-12 {
                Some(0.0)
            } else if arguments[0].is_sign_negative() {
                Some(-1.0)
            } else {
                Some(1.0)
            }
        }
        "SUM" if !arguments.is_empty() => Some(arguments.iter().sum()),
        "TRUE" if arguments.is_empty() => Some(1.0),
        _ => None,
    }
}

fn formula_truthy(value: f64) -> bool {
    value.abs() >= 1e-12
}

fn formula_number_text(value: f64) -> Option<String> {
    if !value.is_finite() {
        return None;
    }
    if value.abs() < 1e-12 {
        return Some("0".to_string());
    }
    let rounded = value.round();
    if (value - rounded).abs() < 1e-12 && rounded >= i64::MIN as f64 && rounded <= i64::MAX as f64 {
        return Some((rounded as i64).to_string());
    }
    let mut text = format!("{value:.12}");
    while text.contains('.') && text.ends_with('0') {
        text.pop();
    }
    if text.ends_with('.') {
        text.pop();
    }
    Some(text)
}

fn format_formula_number(value: f64, picture: &str) -> Option<String> {
    if !value.is_finite() || picture.is_empty() {
        return None;
    }
    if picture.contains(';') {
        let (selected, value) = select_formula_number_section(value, picture)?;
        return format_formula_number_section(value, selected, false);
    }
    format_formula_number_section(value, picture, true)
}

fn format_formula_number_section(
    value: f64,
    picture: &str,
    allow_sign_control: bool,
) -> Option<String> {
    let picture = parse_formula_number_picture(picture)?;
    let (sign_control, prefix) = formula_number_sign_control(&picture.prefix, allow_sign_control);
    let (integer_picture, fraction_picture) = match picture.core.split_once('.') {
        Some((integer, fraction)) if !fraction.contains('.') => (integer, Some(fraction)),
        Some(_) => return None,
        None => (picture.core.as_str(), None),
    };
    if !valid_formula_integer_picture(integer_picture) {
        return None;
    }
    let fraction_picture = fraction_picture.unwrap_or("");
    if !valid_formula_fraction_picture(fraction_picture) {
        return None;
    }
    let max_fraction_digits = fraction_picture.len();
    if max_fraction_digits > 12 {
        return None;
    }
    let required_integer_digits = integer_picture.chars().filter(|ch| *ch == '0').count();
    let required_fraction_digits = if fraction_picture.contains('x') {
        fraction_picture.len()
    } else {
        fraction_picture.chars().filter(|ch| *ch == '0').count()
    };
    let rounded_abs = if value.abs() < 1e-12 {
        0.0
    } else {
        value.abs()
    };
    let fixed = format!("{rounded_abs:.max_fraction_digits$}");
    let (integer_text, fraction_text) = fixed.split_once('.').unwrap_or((fixed.as_str(), ""));
    let mut integer_text = integer_text.to_string();
    if integer_picture.is_empty() {
        integer_text.clear();
    } else {
        while integer_text.len() < required_integer_digits {
            integer_text.insert(0, '0');
        }
        if integer_picture.contains('x') {
            let keep_digits = integer_picture
                .chars()
                .filter(|ch| matches!(ch, '0' | '#' | 'x'))
                .count();
            if integer_text.len() > keep_digits {
                integer_text = integer_text[integer_text.len() - keep_digits..].to_string();
            }
        }
        if integer_picture.contains(',') {
            integer_text = grouped_decimal_digits(&integer_text);
        }
    }
    let mut fraction_text = fraction_text.to_string();
    while fraction_text.len() > required_fraction_digits && fraction_text.ends_with('0') {
        fraction_text.pop();
    }
    let mut text = prefix.to_string();
    text.push_str(&integer_text);
    if !fraction_text.is_empty() {
        text.push('.');
        text.push_str(&fraction_text);
    }
    text.push_str(&picture.suffix);
    let has_nonzero_digit = text.chars().any(|ch| ch.is_ascii_digit() && ch != '0');
    if let Some(sign_control) = sign_control {
        text.insert(
            0,
            formula_number_sign_control_prefix(value, has_nonzero_digit, sign_control),
        );
    } else if value.is_sign_negative() && has_nonzero_digit {
        text.insert(0, '-');
    }
    Some(text)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FormulaNumberSignControl {
    Plus,
    Minus,
}

fn formula_number_sign_control(
    prefix: &str,
    allow_sign_control: bool,
) -> (Option<FormulaNumberSignControl>, &str) {
    if !allow_sign_control {
        return (None, prefix);
    }
    if let Some(prefix) = prefix.strip_prefix('+') {
        return (Some(FormulaNumberSignControl::Plus), prefix);
    }
    if let Some(prefix) = prefix.strip_prefix('-') {
        return (Some(FormulaNumberSignControl::Minus), prefix);
    }
    (None, prefix)
}

fn formula_number_sign_control_prefix(
    value: f64,
    has_nonzero_digit: bool,
    sign_control: FormulaNumberSignControl,
) -> char {
    if !has_nonzero_digit {
        return ' ';
    }
    match sign_control {
        FormulaNumberSignControl::Plus if value.is_sign_negative() => '-',
        FormulaNumberSignControl::Plus => '+',
        FormulaNumberSignControl::Minus if value.is_sign_negative() => '-',
        FormulaNumberSignControl::Minus => ' ',
    }
}

fn select_formula_number_section(value: f64, picture: &str) -> Option<(&str, f64)> {
    let sections: Vec<_> = picture.split(';').collect();
    let selected = match sections.as_slice() {
        [positive, negative] => {
            if value.is_sign_negative() {
                *negative
            } else {
                *positive
            }
        }
        [positive, negative, zero] => {
            if value.abs() < 1e-12 {
                *zero
            } else if value.is_sign_negative() {
                *negative
            } else {
                *positive
            }
        }
        _ => return None,
    };
    if selected.is_empty() || selected.contains(';') {
        return None;
    }
    Some((selected, value.abs()))
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FormulaNumberPicture {
    prefix: String,
    core: String,
    suffix: String,
}

fn parse_formula_number_picture(picture: &str) -> Option<FormulaNumberPicture> {
    if picture.contains('\'') || picture.contains('`') {
        return None;
    }
    let mut first = picture.find(['0', '#', 'x'])?;
    if first > 0 && picture.as_bytes()[first - 1] == b'.' {
        first -= 1;
    }
    let last = picture.rfind(['0', '#', 'x'])?;
    let prefix = &picture[..first];
    let core = &picture[first..=last];
    let suffix = &picture[last + 1..];
    if core.is_empty() || !valid_formula_number_affix(prefix) || !valid_formula_number_affix(suffix)
    {
        return None;
    }
    Some(FormulaNumberPicture {
        prefix: prefix.to_string(),
        core: core.to_string(),
        suffix: suffix.to_string(),
    })
}

fn valid_formula_number_affix(affix: &str) -> bool {
    affix.chars().all(|ch| {
        (ch == ' ' || ch.is_ascii_graphic())
            && !matches!(ch, '0' | '#' | ',' | '.' | '"' | '\\' | 'x')
    })
}

fn valid_formula_integer_picture(picture: &str) -> bool {
    if picture.is_empty() {
        return true;
    }
    if !picture
        .chars()
        .all(|ch| matches!(ch, '0' | '#' | ',' | 'x'))
    {
        return false;
    }
    let x_count = picture.chars().filter(|ch| *ch == 'x').count();
    if x_count > 1 {
        return false;
    }
    if x_count == 1 {
        return !picture.contains(',')
            && picture.starts_with('x')
            && picture.chars().all(|ch| matches!(ch, '0' | '#' | 'x'));
    }
    let groups: Vec<_> = picture.split(',').collect();
    if groups.len() == 1 {
        return picture.chars().any(|ch| matches!(ch, '0' | '#'));
    }
    if groups
        .iter()
        .any(|group| group.is_empty() || !group.chars().all(|ch| matches!(ch, '0' | '#')))
    {
        return false;
    }
    let Some((first, rest)) = groups.split_first() else {
        return false;
    };
    (1..=3).contains(&first.len()) && rest.iter().all(|group| group.len() == 3)
}

fn valid_formula_fraction_picture(picture: &str) -> bool {
    let x_count = picture.chars().filter(|ch| *ch == 'x').count();
    if x_count > 1 || (x_count == 1 && !picture.ends_with('x')) {
        return false;
    }
    let mut seen_optional = false;
    for ch in picture.chars() {
        match ch {
            '0' if seen_optional => return false,
            '0' => {}
            '#' => seen_optional = true,
            'x' => seen_optional = true,
            _ => return false,
        }
    }
    true
}

fn grouped_decimal_digits(digits: &str) -> String {
    let mut grouped = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, ch) in digits.chars().rev().enumerate() {
        if index > 0 && index % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(ch);
    }
    grouped.chars().rev().collect()
}

fn computed_quote_result(instruction: &str) -> Option<String> {
    let spec = quote_instruction(instruction)?;
    Some(apply_field_text_format(spec.text, spec.text_format))
}

pub(crate) fn supports_quote_field_syntax(instruction: &str) -> bool {
    quote_instruction(instruction).is_some()
}

fn quote_instruction(instruction: &str) -> Option<QuoteInstruction> {
    let tokens = instruction_parts(instruction);
    let mut parts = tokens.iter().map(String::as_str);
    let kind = parts.next()?;
    if !kind.eq_ignore_ascii_case("QUOTE") {
        return None;
    }
    let mut text_parts = Vec::new();
    let mut text_format = None;
    let mut saw_format = false;
    while let Some(part) = parts.next() {
        if part == "\\*" {
            saw_format = true;
            if !accept_field_format_switch(parts.next()?, &mut text_format) {
                return None;
            }
            continue;
        }
        if let Some(format) = part.strip_prefix("\\*") {
            saw_format = true;
            if !accept_field_format_switch(format, &mut text_format) {
                return None;
            }
            continue;
        }
        if saw_format || part.starts_with('\\') {
            return None;
        }
        text_parts.push(part);
    }
    let text = text_parts.join(" ");
    let text = field_literal_token(&text)?.to_string();
    if text.is_empty() {
        return None;
    }
    Some(QuoteInstruction { text, text_format })
}

fn computed_fill_in_result(instruction: &str) -> Option<String> {
    let spec = fill_in_instruction(instruction)?;
    let text = spec.default?;
    Some(apply_field_text_format(text, spec.text_format))
}

fn fill_in_instruction(instruction: &str) -> Option<FillInInstruction> {
    let tokens = instruction_parts(instruction);
    let mut parts = tokens.iter().map(String::as_str);
    let kind = parts.next()?;
    if !kind.eq_ignore_ascii_case("FILLIN") {
        return None;
    }
    let mut default = None;
    let mut text_format = None;
    let mut ask_once = false;
    let mut prompt_seen = false;
    while let Some(part) = parts.next() {
        if field_prompt_default_switch(part, &mut parts, &mut default) {
            continue;
        }
        if part.eq_ignore_ascii_case("\\o") {
            if ask_once {
                return None;
            }
            ask_once = true;
            continue;
        }
        if part == "\\*" {
            if !accept_field_format_switch(parts.next()?, &mut text_format) {
                return None;
            }
            continue;
        }
        if let Some(format) = part.strip_prefix("\\*") {
            if !accept_field_format_switch(format, &mut text_format) {
                return None;
            }
            continue;
        }
        let prompt = quoted_literal_text(part)?;
        if prompt.is_empty() || prompt_seen {
            return None;
        }
        prompt_seen = true;
    }
    Some(FillInInstruction {
        default,
        text_format,
    })
}

pub(crate) fn supports_prompt_field_syntax(instruction: &str) -> bool {
    fill_in_instruction(instruction).is_some() || ask_instruction(instruction).is_some()
}

pub(crate) fn computed_ask_result(
    instruction: &str,
    field_bookmarks: &mut HashMap<String, String>,
) -> Option<String> {
    let spec = ask_instruction(instruction)?;
    let value = spec.default?;
    field_bookmarks.insert(spec.bookmark, value);
    Some(String::new())
}

fn ask_instruction(instruction: &str) -> Option<AskInstruction> {
    let tokens = instruction_parts(instruction);
    let mut parts = tokens.iter().map(String::as_str);
    let kind = parts.next()?;
    if !kind.eq_ignore_ascii_case("ASK") {
        return None;
    }
    let bookmark = field_identifier_token(parts.next()?)?;
    let prompt = quoted_literal_text(parts.next()?)?;
    if prompt.is_empty() {
        return None;
    }
    let mut default = None;
    let mut ask_once = false;
    let mut text_format = None;
    while let Some(part) = parts.next() {
        if field_prompt_default_switch(part, &mut parts, &mut default) {
            continue;
        }
        if part.eq_ignore_ascii_case("\\o") {
            if ask_once {
                return None;
            }
            ask_once = true;
            continue;
        }
        if part == "\\*" {
            if !accept_field_format_switch(parts.next()?, &mut text_format) {
                return None;
            }
            continue;
        }
        if let Some(format) = part.strip_prefix("\\*") {
            if accept_field_format_switch(format, &mut text_format) {
                continue;
            }
        }
        return None;
    }
    Some(AskInstruction {
        bookmark: bookmark.to_string(),
        default,
    })
}

fn field_prompt_default_switch<'a, I>(
    part: &str,
    parts: &mut I,
    default: &mut Option<String>,
) -> bool
where
    I: Iterator<Item = &'a str>,
{
    let value = if part.eq_ignore_ascii_case("\\d") {
        parts.next().and_then(quoted_literal_text)
    } else {
        strip_ascii_switch_prefix(part, "\\d").and_then(quoted_literal_text)
    };
    let Some(value) = value else {
        return false;
    };
    default.replace(value).is_none()
}

fn quoted_literal_text(token: &str) -> Option<String> {
    let bytes = token.as_bytes();
    if bytes.len() < 2 || bytes[0] != b'"' || *bytes.last()? != b'"' {
        return None;
    }
    let text = &token[1..token.len() - 1];
    (!text.contains('"')).then(|| text.to_string())
}

#[derive(Debug, Clone, PartialEq)]
enum IfOperand {
    Number(f64),
    Text(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IfOperator {
    Eq,
    Ne,
    Gt,
    Lt,
    Ge,
    Le,
}

#[derive(Debug, Clone, PartialEq)]
struct IfInstruction {
    left: IfOperand,
    operator: IfOperator,
    right: IfOperand,
    true_text: String,
    false_text: String,
    text_format: Option<FieldTextFormat>,
}

fn computed_if_result(instruction: &str) -> Option<String> {
    let spec = if_instruction(instruction)?;
    let selected = if compare_if_operands(&spec.left, spec.operator, &spec.right)? {
        spec.true_text
    } else {
        spec.false_text
    };
    Some(apply_field_text_format(selected, spec.text_format))
}

fn if_instruction(instruction: &str) -> Option<IfInstruction> {
    let tokens = instruction_parts(instruction);
    let mut parts = tokens.iter().map(String::as_str);
    let kind = parts.next()?;
    if !kind.eq_ignore_ascii_case("IF") {
        return None;
    }
    let first = parts.next()?;
    let (left, operator, right) = comparison_operands(first, &mut parts)?;
    let true_text = if_result_text(parts.next()?)?;
    let mut false_text = String::new();
    let mut text_format = None;
    if let Some(part) = parts.next() {
        if is_field_format_start(part) {
            let next = (part == "\\*").then(|| parts.next()).flatten();
            accept_if_format_switch(part, next, &mut text_format)?;
        } else {
            false_text = if_result_text(part)?;
        }
    }
    while let Some(part) = parts.next() {
        let next = (part == "\\*").then(|| parts.next()).flatten();
        accept_if_format_switch(part, next, &mut text_format)?;
    }
    Some(IfInstruction {
        left,
        operator,
        right,
        true_text,
        false_text,
        text_format,
    })
}

pub(crate) fn supports_if_field_syntax(instruction: &str) -> bool {
    if_field_syntax(instruction).is_some()
}

fn if_field_syntax(instruction: &str) -> Option<()> {
    let tokens = instruction_parts(instruction);
    let mut parts = tokens.iter().map(String::as_str);
    let kind = parts.next()?;
    if !kind.eq_ignore_ascii_case("IF") {
        return None;
    }
    let first = parts.next()?;
    merge_control_comparison_syntax(first, &mut parts)?;
    if_result_text(parts.next()?)?;
    let mut text_format = None;
    if let Some(part) = parts.next() {
        if is_field_format_start(part) {
            let next = (part == "\\*").then(|| parts.next()).flatten();
            accept_if_format_switch(part, next, &mut text_format)?;
        } else {
            if_result_text(part)?;
        }
    }
    while let Some(part) = parts.next() {
        let next = (part == "\\*").then(|| parts.next()).flatten();
        accept_if_format_switch(part, next, &mut text_format)?;
    }
    Some(())
}

fn computed_compare_result(instruction: &str) -> Option<String> {
    let spec = compare_instruction(instruction)?;
    let result = if compare_if_operands(&spec.left, spec.operator, &spec.right)? {
        "1"
    } else {
        "0"
    };
    Some(apply_field_text_format(
        result.to_string(),
        spec.text_format,
    ))
}

pub(crate) fn supports_compare_field_syntax(instruction: &str) -> bool {
    compare_instruction(instruction).is_some()
}

fn compare_instruction(instruction: &str) -> Option<IfInstruction> {
    let tokens = instruction_parts(instruction);
    let mut parts = tokens.iter().map(String::as_str);
    let kind = parts.next()?;
    if !kind.eq_ignore_ascii_case("COMPARE") {
        return None;
    }
    let first = parts.next()?;
    let (left, operator, right) = comparison_operands(first, &mut parts)?;
    let mut text_format = None;
    while let Some(part) = parts.next() {
        let next = (part == "\\*").then(|| parts.next()).flatten();
        accept_if_format_switch(part, next, &mut text_format)?;
    }
    Some(IfInstruction {
        left,
        operator,
        right,
        true_text: "1".to_string(),
        false_text: "0".to_string(),
        text_format,
    })
}

fn comparison_operands<'a, I>(
    first: &str,
    parts: &mut I,
) -> Option<(IfOperand, IfOperator, IfOperand)>
where
    I: Iterator<Item = &'a str>,
{
    if let Some(comparison) = compact_if_comparison(first) {
        Some(comparison)
    } else {
        Some((
            if_operand(first)?,
            if_operator(parts.next()?)?,
            if_operand(parts.next()?)?,
        ))
    }
}

fn if_operand(token: &str) -> Option<IfOperand> {
    if let Some(text) = quoted_literal_text(token) {
        return Some(IfOperand::Text(text));
    }
    token
        .parse::<f64>()
        .ok()
        .filter(|value| value.is_finite())
        .map(IfOperand::Number)
}

fn compact_if_comparison(token: &str) -> Option<(IfOperand, IfOperator, IfOperand)> {
    for operator in [">=", "<=", "<>", "=", ">", "<"] {
        let Some(index) = find_unquoted_operator(token, operator) else {
            continue;
        };
        let (left, right_with_operator) = token.split_at(index);
        let right = &right_with_operator[operator.len()..];
        if left.is_empty() || right.is_empty() {
            return None;
        }
        return Some((
            if_operand(left)?,
            if_operator(operator)?,
            if_operand(right)?,
        ));
    }
    None
}

fn find_unquoted_operator(token: &str, operator: &str) -> Option<usize> {
    let mut in_quotes = false;
    for (index, ch) in token.char_indices() {
        if ch == '"' {
            in_quotes = !in_quotes;
        } else if !in_quotes && token[index..].starts_with(operator) {
            return Some(index);
        }
    }
    None
}

fn if_result_text(token: &str) -> Option<String> {
    (!token.starts_with('\\')).then(|| field_literal_token(token).map(str::to_string))?
}

fn if_operator(token: &str) -> Option<IfOperator> {
    match token {
        "=" => Some(IfOperator::Eq),
        "<>" => Some(IfOperator::Ne),
        ">" => Some(IfOperator::Gt),
        "<" => Some(IfOperator::Lt),
        ">=" => Some(IfOperator::Ge),
        "<=" => Some(IfOperator::Le),
        _ => None,
    }
}

fn compare_if_operands(left: &IfOperand, operator: IfOperator, right: &IfOperand) -> Option<bool> {
    match (left, right) {
        (IfOperand::Number(left), IfOperand::Number(right)) => match operator {
            IfOperator::Eq => Some(left == right),
            IfOperator::Ne => Some(left != right),
            IfOperator::Gt => Some(left > right),
            IfOperator::Lt => Some(left < right),
            IfOperator::Ge => Some(left >= right),
            IfOperator::Le => Some(left <= right),
        },
        (IfOperand::Text(left), IfOperand::Text(right)) => match operator {
            IfOperator::Eq => Some(compare_text_operands(left, right)),
            IfOperator::Ne => Some(!compare_text_operands(left, right)),
            _ => None,
        },
        _ => None,
    }
}

fn compare_text_operands(left: &str, right: &str) -> bool {
    if right.contains(['*', '?']) {
        wildcard_match(left, right)
    } else if left.contains(['*', '?']) {
        wildcard_match(right, left)
    } else {
        left == right
    }
}

pub(crate) fn computed_reference_index_result(instruction: &str) -> Option<String> {
    let tokens = instruction_parts(instruction);
    let mut parts = tokens.iter().map(String::as_str);
    let kind = parts.next()?;
    if kind.eq_ignore_ascii_case("TA") {
        reference_index_ta_instruction(parts)?;
        return Some(String::new());
    }
    if kind.eq_ignore_ascii_case("XE") {
        reference_index_xe_instruction(parts)?;
        return Some(String::new());
    }
    if kind.eq_ignore_ascii_case("RD") {
        reference_index_rd_instruction(parts)?;
        return Some(String::new());
    }
    None
}

fn reference_index_rd_instruction<'a>(mut parts: impl Iterator<Item = &'a str>) -> Option<()> {
    reference_index_literal(parts.next()?)?;
    while let Some(part) = parts.next() {
        if part.eq_ignore_ascii_case("\\f") {
            continue;
        }
        if accept_reference_index_field_format(part, &mut parts).is_some() {
            continue;
        }
        return None;
    }
    Some(())
}

fn reference_index_ta_instruction<'a>(mut parts: impl Iterator<Item = &'a str>) -> Option<()> {
    let mut has_entry_text = false;
    while let Some(part) = parts.next() {
        if part.eq_ignore_ascii_case("\\l") || part.eq_ignore_ascii_case("\\s") {
            reference_index_literal(parts.next()?)?;
            has_entry_text = true;
            continue;
        }
        if let Some(value) = strip_ascii_switch_prefix(part, "\\l")
            .or_else(|| strip_ascii_switch_prefix(part, "\\s"))
        {
            if value.is_empty() {
                return None;
            }
            reference_index_literal(value)?;
            has_entry_text = true;
            continue;
        }
        if part.eq_ignore_ascii_case("\\c") {
            parse_reference_index_category(parts.next()?)?;
            continue;
        }
        if let Some(category) = strip_ascii_switch_prefix(part, "\\c") {
            if category.is_empty() {
                return None;
            }
            parse_reference_index_category(category)?;
            continue;
        }
        if accept_reference_index_field_format(part, &mut parts).is_some() {
            continue;
        }
        return None;
    }
    has_entry_text.then_some(())
}

fn reference_index_xe_instruction<'a>(mut parts: impl Iterator<Item = &'a str>) -> Option<()> {
    reference_index_literal(parts.next()?)?;
    while let Some(part) = parts.next() {
        if part.eq_ignore_ascii_case("\\b") || part.eq_ignore_ascii_case("\\i") {
            continue;
        }
        if part.eq_ignore_ascii_case("\\f") || part.eq_ignore_ascii_case("\\r") {
            reference_index_plain_value(parts.next()?)?;
            continue;
        }
        if let Some(value) = strip_ascii_switch_prefix(part, "\\f")
            .or_else(|| strip_ascii_switch_prefix(part, "\\r"))
        {
            if value.is_empty() {
                return None;
            }
            reference_index_plain_value(value)?;
            continue;
        }
        if part.eq_ignore_ascii_case("\\t") {
            reference_index_literal(parts.next()?)?;
            continue;
        }
        if let Some(value) = strip_ascii_switch_prefix(part, "\\t") {
            if value.is_empty() {
                return None;
            }
            reference_index_literal(value)?;
            continue;
        }
        if accept_reference_index_field_format(part, &mut parts).is_some() {
            continue;
        }
        return None;
    }
    Some(())
}

fn reference_index_literal(token: &str) -> Option<String> {
    if let Some(text) = quoted_literal_text(token) {
        return (!text.is_empty()).then_some(text);
    }
    reference_index_plain_value(token)
}

fn reference_index_plain_value(token: &str) -> Option<String> {
    (!token.is_empty() && !token.starts_with('\\') && !token.contains('"'))
        .then(|| token.to_string())
}

fn parse_reference_index_category(value: &str) -> Option<u8> {
    let value = field_name_token(value)?.parse::<u8>().ok()?;
    (1..=16).contains(&value).then_some(value)
}

fn accept_reference_index_field_format<'a>(
    part: &str,
    parts: &mut impl Iterator<Item = &'a str>,
) -> Option<()> {
    let mut text_format = None;
    if part == "\\*" {
        return accept_field_format_switch(parts.next()?, &mut text_format).then_some(());
    }
    accept_field_format_switch(part.strip_prefix("\\*")?, &mut text_format).then_some(())
}

fn wildcard_match(text: &str, pattern: &str) -> bool {
    let text: Vec<char> = text.chars().collect();
    let pattern: Vec<char> = pattern.chars().collect();
    let mut matches = vec![vec![false; pattern.len() + 1]; text.len() + 1];
    matches[0][0] = true;
    for pattern_index in 1..=pattern.len() {
        if pattern[pattern_index - 1] == '*' {
            matches[0][pattern_index] = matches[0][pattern_index - 1];
        }
    }
    for text_index in 1..=text.len() {
        for pattern_index in 1..=pattern.len() {
            matches[text_index][pattern_index] = match pattern[pattern_index - 1] {
                '*' => {
                    matches[text_index][pattern_index - 1] || matches[text_index - 1][pattern_index]
                }
                '?' => matches[text_index - 1][pattern_index - 1],
                ch => matches[text_index - 1][pattern_index - 1] && text[text_index - 1] == ch,
            };
        }
    }
    matches[text.len()][pattern.len()]
}

fn is_field_format_start(part: &str) -> bool {
    part == "\\*" || part.starts_with("\\*")
}

fn accept_if_format_switch(
    part: &str,
    next: Option<&str>,
    text_format: &mut Option<FieldTextFormat>,
) -> Option<()> {
    if part == "\\*" {
        accept_field_format_switch(next?, text_format).then_some(())
    } else {
        accept_field_format_switch(part.strip_prefix("\\*")?, text_format).then_some(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ActionInstruction {
    display_text: String,
    text_format: Option<FieldTextFormat>,
}

pub(crate) fn computed_action_result(instruction: &str) -> Option<String> {
    if print_instruction(instruction).is_some() {
        return Some(String::new());
    }
    let spec = action_instruction(instruction)?;
    Some(apply_field_text_format(spec.display_text, spec.text_format))
}

fn print_instruction(instruction: &str) -> Option<()> {
    let tokens = instruction_parts(instruction);
    let mut parts = tokens.iter().map(String::as_str);
    let kind = parts.next()?;
    if !kind.eq_ignore_ascii_case("PRINT") {
        return None;
    }
    let first = parts.next()?;
    if first.eq_ignore_ascii_case("\\p") {
        field_identifier_token(parts.next()?)?;
        let codes = quoted_literal_text(parts.next()?)?;
        if codes.is_empty() {
            return None;
        }
    } else if let Some(group) = strip_ascii_switch_prefix(first, "\\p") {
        field_identifier_token(group)?;
        let codes = quoted_literal_text(parts.next()?)?;
        if codes.is_empty() {
            return None;
        }
    } else {
        let instructions = field_literal_token(first)?;
        if instructions.is_empty() || instructions.starts_with('\\') {
            return None;
        }
    }
    let mut text_format = None;
    while let Some(part) = parts.next() {
        if part == "\\*" {
            if !accept_field_format_switch(parts.next()?, &mut text_format) {
                return None;
            }
            continue;
        }
        if let Some(format) = part.strip_prefix("\\*") {
            if accept_field_format_switch(format, &mut text_format) {
                continue;
            }
        }
        return None;
    }
    Some(())
}

fn action_instruction(instruction: &str) -> Option<ActionInstruction> {
    let tokens = instruction_parts(instruction);
    let mut parts = tokens.iter().map(String::as_str);
    let kind = parts.next()?;
    if !kind.eq_ignore_ascii_case("GOTOBUTTON") && !kind.eq_ignore_ascii_case("MACROBUTTON") {
        return None;
    }
    field_identifier_token(parts.next()?)?;
    let mut display_parts = Vec::new();
    let mut text_format = None;
    while let Some(part) = parts.next() {
        if part == "\\*" {
            if !accept_field_format_switch(parts.next()?, &mut text_format) {
                return None;
            }
            continue;
        }
        if let Some(format) = part.strip_prefix("\\*") {
            if !accept_field_format_switch(format, &mut text_format) {
                return None;
            }
            continue;
        }
        if part.starts_with('\\') {
            return None;
        }
        display_parts.push(part);
    }
    let display_text = display_parts.join(" ");
    let display_text = field_literal_token(&display_text)?.to_string();
    (!display_text.is_empty()).then_some(ActionInstruction {
        display_text,
        text_format,
    })
}

fn unquote_field_text(text: &str) -> String {
    let bytes = text.as_bytes();
    if bytes.len() >= 2 && bytes[0] == b'"' && *bytes.last().unwrap_or(&0) == b'"' {
        text[1..text.len() - 1].to_string()
    } else {
        text.to_string()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SymbolInstruction {
    code: u32,
    unicode: bool,
    font: Option<String>,
    text_format: Option<FieldTextFormat>,
}

pub(crate) fn computed_display_result(instruction: &str) -> Option<String> {
    computed_advance_result(instruction)
        .or_else(|| computed_eq_result(instruction))
        .or_else(|| computed_symbol_result(instruction))
}

fn computed_advance_result(instruction: &str) -> Option<String> {
    advance_instruction(instruction)?;
    Some(String::new())
}

fn advance_instruction(instruction: &str) -> Option<()> {
    let tokens = instruction_parts(instruction);
    let mut parts = tokens.iter().map(String::as_str);
    let kind = parts.next()?;
    if !kind.eq_ignore_ascii_case("ADVANCE") {
        return None;
    }
    let mut text_format = None;
    while let Some(part) = parts.next() {
        if part == "\\*" {
            if !accept_field_format_switch(parts.next()?, &mut text_format) {
                return None;
            }
            continue;
        }
        if let Some(format) = part.strip_prefix("\\*") {
            if accept_field_format_switch(format, &mut text_format) {
                continue;
            }
        }
        if accept_advance_switch(part, &mut parts).is_some() {
            continue;
        }
        return None;
    }
    Some(())
}

fn accept_advance_switch<'a>(part: &str, parts: &mut impl Iterator<Item = &'a str>) -> Option<()> {
    for switch in ["\\d", "\\u", "\\l", "\\r", "\\x", "\\y"] {
        if part.eq_ignore_ascii_case(switch) {
            parse_advance_points(parts.next()?)?;
            return Some(());
        }
        if let Some(value) = strip_ascii_switch_prefix(part, switch) {
            if value.is_empty() {
                return None;
            }
            parse_advance_points(value)?;
            return Some(());
        }
    }
    None
}

fn parse_advance_points(value: &str) -> Option<f32> {
    field_name_token(value)?
        .parse::<f32>()
        .ok()
        .filter(|value| value.is_finite())
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct EqInstruction {
    expression: String,
    text_format: Option<FieldTextFormat>,
}

fn computed_eq_result(instruction: &str) -> Option<String> {
    let spec = eq_instruction(instruction)?;
    Some(apply_field_text_format(
        eq_display_text(&spec.expression)?,
        spec.text_format,
    ))
}

fn eq_instruction(instruction: &str) -> Option<EqInstruction> {
    let tokens = instruction_parts(instruction);
    let mut parts = tokens.iter().map(String::as_str);
    let kind = parts.next()?;
    if !kind.eq_ignore_ascii_case("EQ") {
        return None;
    }
    let mut expression_parts = Vec::new();
    let mut text_format = None;
    while let Some(part) = parts.next() {
        if part == "\\*" {
            if !accept_field_format_switch(parts.next()?, &mut text_format) {
                return None;
            }
            continue;
        }
        if let Some(format) = part.strip_prefix("\\*") {
            if !accept_field_format_switch(format, &mut text_format) {
                return None;
            }
            continue;
        }
        expression_parts.push(part);
    }
    if expression_parts.is_empty() {
        return None;
    }
    Some(EqInstruction {
        expression: expression_parts.join(" "),
        text_format,
    })
}

fn eq_display_text(expression: &str) -> Option<String> {
    eq_displace_text(expression)
        .or_else(|| eq_array_text(expression))
        .or_else(|| eq_fraction_text(expression))
        .or_else(|| eq_radical_text(expression))
        .or_else(|| eq_script_text(expression))
        .or_else(|| eq_integral_text(expression))
        .or_else(|| eq_overstrike_text(expression))
        .or_else(|| eq_bracket_text(expression))
        .or_else(|| eq_box_text(expression))
        .or_else(|| eq_list_text(expression))
}

fn eq_displace_text(expression: &str) -> Option<String> {
    let mut body = strip_ascii_switch_prefix(expression, "\\d")?.trim_start();
    let mut has_option = false;
    loop {
        if body == "()" {
            return has_option.then_some(String::new());
        }
        if let Some((_value, rest)) = consume_eq_numeric_prefix_option(body, "\\fo")
            .or_else(|| consume_eq_numeric_prefix_option(body, "\\ba"))
        {
            has_option = true;
            body = rest.trim_start();
        } else if let Some(rest) = consume_eq_prefix_switch(body, "\\li") {
            has_option = true;
            body = rest.trim_start();
        } else {
            return None;
        }
    }
}

fn eq_array_text(expression: &str) -> Option<String> {
    let mut body = strip_ascii_switch_prefix(expression, "\\a")?.trim_start();
    let mut columns = 1usize;
    loop {
        if let Some(rest) = consume_eq_prefix_switch(body, "\\al")
            .or_else(|| consume_eq_prefix_switch(body, "\\ac"))
            .or_else(|| consume_eq_prefix_switch(body, "\\ar"))
        {
            body = rest.trim_start();
        } else if let Some((value, rest)) = consume_eq_numeric_prefix_option(body, "\\co") {
            columns = eq_column_count(value)?;
            body = rest.trim_start();
        } else if let Some((_value, rest)) = consume_eq_numeric_prefix_option(body, "\\vs")
            .or_else(|| consume_eq_numeric_prefix_option(body, "\\hs"))
        {
            body = rest.trim_start();
        } else {
            break;
        }
    }
    let inner = body.strip_prefix('(')?.strip_suffix(')')?;
    let operands = split_eq_list_operands(inner)?;
    let mut cells = Vec::with_capacity(operands.len());
    for operand in operands {
        cells.push(eq_operand_text(operand)?);
    }
    Some(
        cells
            .chunks(columns)
            .map(|row| row.join("\t"))
            .collect::<Vec<_>>()
            .join("\n"),
    )
}

fn eq_fraction_text(expression: &str) -> Option<String> {
    let body = strip_ascii_switch_prefix(expression, "\\f")?;
    let inner = body.strip_prefix('(')?.strip_suffix(')')?;
    let (numerator, denominator) = split_eq_fraction_operands(inner)?;
    Some(format!(
        "{}/{}",
        eq_operand_text(numerator)?,
        eq_operand_text(denominator)?
    ))
}

fn eq_radical_text(expression: &str) -> Option<String> {
    let body = strip_ascii_switch_prefix(expression, "\\r")?;
    let inner = body.strip_prefix('(')?.strip_suffix(')')?;
    let operands = split_eq_radical_operands(inner)?;
    let text = match operands {
        (radicand, None) => format!("√{}", eq_operand_text(radicand)?),
        (degree, Some(radicand)) => {
            format!(
                "{}√{}",
                eq_operand_text(degree)?,
                eq_operand_text(radicand)?
            )
        }
    };
    Some(text)
}

fn eq_script_text(expression: &str) -> Option<String> {
    let mut body = expression.trim_start();
    let mut text = String::new();
    loop {
        let rest = strip_ascii_switch_prefix(body, "\\s")?;
        let (segment, remaining) = eq_script_segment(rest.trim_start())?;
        text.push_str(&segment);
        body = remaining.trim_start();
        if body.is_empty() {
            return Some(text);
        }
    }
}

fn eq_script_segment(mut body: &str) -> Option<(String, &str)> {
    let mut text = String::new();
    let mut saw_option = false;
    loop {
        if body.is_empty() || consume_eq_prefix_switch(body, "\\s").is_some() {
            return saw_option.then_some((text, body));
        }
        if let Some((marker, segment, rest)) = consume_eq_script_visible_option(body, "\\up", '^')
            .or_else(|| consume_eq_script_visible_option(body, "\\do", '_'))
        {
            text.push_str(&eq_script_marker(marker, eq_operand_text(segment)?));
            body = rest.trim_start();
            saw_option = true;
            continue;
        }
        if let Some(rest) = consume_eq_script_empty_option(body, "\\ai")
            .or_else(|| consume_eq_script_empty_option(body, "\\di"))
        {
            saw_option = true;
            body = rest.trim_start();
            continue;
        }
        return None;
    }
}

fn consume_eq_script_visible_option<'a>(
    value: &'a str,
    option: &str,
    marker: char,
) -> Option<(char, &'a str, &'a str)> {
    let (_points, rest) = consume_eq_numeric_prefix_option(value, option)?;
    let (operand, rest) = take_eq_parenthesized_operand(rest)?;
    Some((marker, operand, rest))
}

fn consume_eq_script_empty_option<'a>(value: &'a str, option: &str) -> Option<&'a str> {
    let (_points, rest) = consume_eq_numeric_prefix_option(value, option)?;
    let (operand, rest) = take_eq_parenthesized_operand(rest)?;
    operand.trim().is_empty().then_some(rest)
}

fn eq_script_marker(marker: char, text: String) -> String {
    if text.chars().count() == 1 {
        format!("{marker}{text}")
    } else {
        format!("{marker}{{{text}}}")
    }
}

fn eq_integral_text(expression: &str) -> Option<String> {
    let mut body = strip_ascii_switch_prefix(expression, "\\i")?.trim_start();
    let mut symbol = '∫';
    loop {
        if let Some(rest) = consume_eq_prefix_switch(body, "\\su") {
            symbol = 'Σ';
            body = rest.trim_start();
        } else if let Some(rest) = consume_eq_prefix_switch(body, "\\pr") {
            symbol = 'Π';
            body = rest.trim_start();
        } else if let Some(rest) = consume_eq_prefix_switch(body, "\\in") {
            body = rest.trim_start();
        } else if let Some((ch, rest)) = consume_eq_bracket_option(body, "\\fc")
            .or_else(|| consume_eq_bracket_option(body, "\\vc"))
        {
            symbol = ch;
            body = rest.trim_start();
        } else {
            break;
        }
    }
    let inner = body.strip_prefix('(')?.strip_suffix(')')?;
    let operands = split_eq_list_operands(inner)?;
    if operands.len() != 3 {
        return None;
    }
    let lower = eq_operand_text(operands[0])?;
    let upper = eq_operand_text(operands[1])?;
    let integrand = eq_operand_text(operands[2])?;
    Some(format!(
        "{}_{}^{} {}",
        symbol,
        eq_limit_text(lower),
        eq_limit_text(upper),
        integrand
    ))
}

fn eq_limit_text(text: String) -> String {
    if text.chars().count() == 1 {
        text
    } else {
        format!("{{{text}}}")
    }
}

fn eq_overstrike_text(expression: &str) -> Option<String> {
    let mut body = strip_ascii_switch_prefix(expression, "\\o")?.trim_start();
    while let Some(rest) = consume_eq_prefix_switch(body, "\\al")
        .or_else(|| consume_eq_prefix_switch(body, "\\ac"))
        .or_else(|| consume_eq_prefix_switch(body, "\\ar"))
    {
        body = rest.trim_start();
    }
    let inner = body.strip_prefix('(')?.strip_suffix(')')?;
    let operands = split_eq_list_operands(inner)?;
    let mut text = String::new();
    for operand in operands {
        text.push_str(&eq_operand_text(operand)?);
    }
    (!text.is_empty()).then_some(text)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct EqBracketPair {
    left: char,
    right: char,
}

fn eq_bracket_text(expression: &str) -> Option<String> {
    let mut body = strip_ascii_switch_prefix(expression, "\\b")?.trim_start();
    let mut brackets = EqBracketPair {
        left: '(',
        right: ')',
    };
    loop {
        if let Some((ch, rest)) = consume_eq_bracket_option(body, "\\bc") {
            brackets = eq_matching_brackets(ch);
            body = rest.trim_start();
        } else if let Some((ch, rest)) = consume_eq_bracket_option(body, "\\lc") {
            brackets.left = ch;
            body = rest.trim_start();
        } else if let Some((ch, rest)) = consume_eq_bracket_option(body, "\\rc") {
            brackets.right = ch;
            body = rest.trim_start();
        } else {
            break;
        }
    }
    let inner = take_eq_enclosed_operand(body)?;
    Some(format!(
        "{}{}{}",
        brackets.left,
        eq_operand_text(inner)?,
        brackets.right
    ))
}

fn eq_box_text(expression: &str) -> Option<String> {
    let inner = eq_enclosed_operand_with_prefix_switches(
        expression,
        "\\x",
        &["\\to", "\\bo", "\\le", "\\ri"],
    )?;
    eq_operand_text(inner)
}

fn eq_list_text(expression: &str) -> Option<String> {
    let body = strip_ascii_switch_prefix(expression, "\\l")?.trim_start();
    let inner = body.strip_prefix('(')?.strip_suffix(')')?;
    let operands = split_eq_list_operands(inner)?;
    let mut parts = Vec::with_capacity(operands.len());
    for operand in operands {
        parts.push(eq_operand_text(operand)?);
    }
    (!parts.is_empty()).then_some(parts.join(","))
}

fn consume_eq_bracket_option<'a>(value: &'a str, option: &str) -> Option<(char, &'a str)> {
    let rest = strip_ascii_switch_prefix(value, option)?;
    consume_eq_bracket_char(rest)
}

fn consume_eq_bracket_char(value: &str) -> Option<(char, &str)> {
    let rest = value.trim_start();
    let rest = rest.strip_prefix('\\').unwrap_or(rest);
    let ch = rest.chars().next()?;
    if ch.is_whitespace() {
        return None;
    }
    Some((ch, &rest[ch.len_utf8()..]))
}

fn eq_matching_brackets(ch: char) -> EqBracketPair {
    match ch {
        '{' => EqBracketPair {
            left: '{',
            right: '}',
        },
        '[' => EqBracketPair {
            left: '[',
            right: ']',
        },
        '(' => EqBracketPair {
            left: '(',
            right: ')',
        },
        '<' => EqBracketPair {
            left: '<',
            right: '>',
        },
        _ => EqBracketPair {
            left: ch,
            right: ch,
        },
    }
}

fn eq_enclosed_operand_with_prefix_switches<'a>(
    expression: &'a str,
    switch: &str,
    options: &[&str],
) -> Option<&'a str> {
    let mut body = strip_ascii_switch_prefix(expression, switch)?.trim_start();
    loop {
        let mut consumed = false;
        for option in options {
            if let Some(rest) = consume_eq_prefix_switch(body, option) {
                body = rest.trim_start();
                consumed = true;
                break;
            }
        }
        if !consumed {
            break;
        }
    }
    take_eq_enclosed_operand(body)
}

fn consume_eq_prefix_switch<'a>(value: &'a str, switch: &str) -> Option<&'a str> {
    let rest = strip_ascii_switch_prefix(value, switch)?;
    if matches!(
        rest.chars().next(),
        Some(ch) if ch.is_ascii_alphabetic()
    ) {
        return None;
    }
    Some(rest)
}

fn consume_eq_numeric_prefix_option<'a>(value: &'a str, option: &str) -> Option<(f32, &'a str)> {
    let rest = strip_ascii_switch_prefix(value, option)?;
    if matches!(
        rest.chars().next(),
        Some(ch) if ch.is_ascii_alphabetic()
    ) {
        return None;
    }
    let rest = rest.trim_start();
    let mut end = 0usize;
    for (index, ch) in rest.char_indices() {
        if index == 0 && (ch == '-' || ch == '+') {
            end = ch.len_utf8();
            continue;
        }
        if !ch.is_ascii_digit() && ch != '.' && ch != 'e' && ch != 'E' && ch != '-' && ch != '+' {
            break;
        }
        end = index + ch.len_utf8();
    }
    if end == 0 || matches!(rest.get(..end), Some("+") | Some("-")) {
        return None;
    }
    let value = parse_advance_points(&rest[..end])?;
    Some((value, &rest[end..]))
}

fn eq_column_count(value: f32) -> Option<usize> {
    (value.fract() == 0.0 && value >= 1.0 && value <= usize::MAX as f32).then_some(value as usize)
}

fn take_eq_parenthesized_operand(value: &str) -> Option<(&str, &str)> {
    let value = value.trim_start();
    if !value.starts_with('(') {
        return None;
    }
    let mut depth = 0usize;
    let mut in_quotes = false;
    let mut escaped = false;
    for (index, ch) in value.char_indices().skip(1) {
        if escaped {
            escaped = false;
            continue;
        }
        match ch {
            '\\' => escaped = true,
            '"' => in_quotes = !in_quotes,
            '(' if !in_quotes => depth += 1,
            ')' if !in_quotes && depth == 0 => {
                return Some((&value[1..index], &value[index + 1..]))
            }
            ')' if !in_quotes => depth = depth.checked_sub(1)?,
            _ => {}
        }
    }
    None
}

fn take_eq_enclosed_operand(value: &str) -> Option<&str> {
    let (inner, rest) = take_eq_parenthesized_operand(value)?;
    rest.trim().is_empty().then_some(inner)
}

fn split_eq_fraction_operands(inner: &str) -> Option<(&str, &str)> {
    let mut depth = 0usize;
    let mut separator = None;
    let mut in_quotes = false;
    let mut escaped = false;
    for (index, ch) in inner.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        match ch {
            '\\' => escaped = true,
            '"' => in_quotes = !in_quotes,
            '(' if !in_quotes => depth += 1,
            ')' if !in_quotes => depth = depth.checked_sub(1)?,
            ',' | ';' if !in_quotes && depth == 0 && separator.replace(index).is_some() => {
                return None;
            }
            _ => {}
        }
    }
    if in_quotes || escaped || depth != 0 {
        return None;
    }
    let index = separator?;
    Some((&inner[..index], &inner[index + 1..]))
}

fn split_eq_radical_operands(inner: &str) -> Option<(&str, Option<&str>)> {
    let mut depth = 0usize;
    let mut separator = None;
    let mut in_quotes = false;
    let mut escaped = false;
    for (index, ch) in inner.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        match ch {
            '\\' => escaped = true,
            '"' => in_quotes = !in_quotes,
            '(' if !in_quotes => depth += 1,
            ')' if !in_quotes => depth = depth.checked_sub(1)?,
            ',' | ';' if !in_quotes && depth == 0 && separator.replace(index).is_some() => {
                return None;
            }
            _ => {}
        }
    }
    if in_quotes || escaped || depth != 0 {
        return None;
    }
    match separator {
        Some(index) => Some((&inner[..index], Some(&inner[index + 1..]))),
        None => Some((inner, None)),
    }
}

fn split_eq_list_operands(inner: &str) -> Option<Vec<&str>> {
    let mut depth = 0usize;
    let mut operands = Vec::new();
    let mut start = 0usize;
    let mut in_quotes = false;
    let mut escaped = false;
    for (index, ch) in inner.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        match ch {
            '\\' => escaped = true,
            '"' => in_quotes = !in_quotes,
            '(' if !in_quotes => depth += 1,
            ')' if !in_quotes => depth = depth.checked_sub(1)?,
            ',' | ';' if !in_quotes && depth == 0 => {
                let operand = &inner[start..index];
                if operand.trim().is_empty() {
                    return None;
                }
                operands.push(operand);
                start = index + ch.len_utf8();
            }
            _ => {}
        }
    }
    if in_quotes || escaped || depth != 0 {
        return None;
    }
    let operand = &inner[start..];
    if operand.trim().is_empty() {
        return None;
    }
    operands.push(operand);
    Some(operands)
}

fn eq_operand_text(operand: &str) -> Option<String> {
    let operand = operand.trim();
    if operand.is_empty() {
        return None;
    }
    let text = unquote_field_text(operand);
    if let Some(nested) = eq_display_text(&text) {
        return Some(format!("({nested})"));
    }
    let text = unescape_eq_literal_operand(&text)?;
    (!text.is_empty()).then_some(text)
}

fn unescape_eq_literal_operand(operand: &str) -> Option<String> {
    let mut out = String::with_capacity(operand.len());
    let mut chars = operand.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        match chars.next()? {
            ',' => out.push(','),
            ';' => out.push(';'),
            '(' => out.push('('),
            ')' => out.push(')'),
            '\\' => out.push('\\'),
            _ => return None,
        }
    }
    Some(out)
}

fn computed_symbol_result(instruction: &str) -> Option<String> {
    let spec = symbol_instruction(instruction)?;
    let text = if symbol_font_matches(spec.font.as_deref(), "symbol") {
        symbol_font_char(spec.code)?.to_string()
    } else if symbol_font_matches(spec.font.as_deref(), "wingdings") {
        wingdings_font_char(spec.code)?.to_string()
    } else if spec.unicode {
        char::from_u32(spec.code)?.to_string()
    } else {
        ansi_char(spec.code)?.to_string()
    };
    Some(apply_field_text_format(text, spec.text_format))
}

fn symbol_instruction(instruction: &str) -> Option<SymbolInstruction> {
    let tokens = instruction_parts(instruction);
    let mut parts = tokens.iter().map(String::as_str);
    let kind = parts.next()?;
    if !kind.eq_ignore_ascii_case("SYMBOL") {
        return None;
    }
    let code = parse_symbol_code(parts.next()?)?;
    let mut unicode = false;
    let mut font = None;
    let mut text_format = None;
    while let Some(part) = parts.next() {
        if part.eq_ignore_ascii_case("\\a") || part.eq_ignore_ascii_case("\\h") {
            continue;
        }
        if part.eq_ignore_ascii_case("\\u") {
            unicode = true;
            continue;
        }
        if part.eq_ignore_ascii_case("\\j") {
            return None;
        }
        if part.eq_ignore_ascii_case("\\f") {
            font = Some(field_name_token(parts.next()?)?.to_string());
            continue;
        }
        if let Some(value) = strip_ascii_switch_prefix(part, "\\f") {
            if value.is_empty() {
                return None;
            }
            font = Some(field_name_token(value)?.to_string());
            continue;
        }
        if part.eq_ignore_ascii_case("\\s") {
            parse_symbol_size(parts.next()?)?;
            continue;
        }
        if let Some(size) = strip_ascii_switch_prefix(part, "\\s") {
            if size.is_empty() {
                return None;
            }
            parse_symbol_size(size)?;
            continue;
        }
        if part == "\\*" {
            if !accept_field_format_switch(parts.next()?, &mut text_format) {
                return None;
            }
            continue;
        }
        if let Some(format) = part.strip_prefix("\\*") {
            if accept_field_format_switch(format, &mut text_format) {
                continue;
            }
        }
        return None;
    }
    Some(SymbolInstruction {
        code,
        unicode,
        font,
        text_format,
    })
}

fn parse_symbol_size(token: &str) -> Option<f32> {
    field_name_token(token)?
        .parse::<f32>()
        .ok()
        .filter(|value| value.is_finite() && *value > 0.0)
}

fn parse_symbol_code(token: &str) -> Option<u32> {
    let token = field_name_token(token)?;
    if let Some(hex) = token
        .strip_prefix("0x")
        .or_else(|| token.strip_prefix("0X"))
    {
        return u32::from_str_radix(hex, 16).ok();
    }
    if let Ok(code) = token.parse::<u32>() {
        return Some(code);
    }
    let mut chars = token.chars();
    let ch = chars.next()?;
    chars.next().is_none().then_some(ch as u32)
}

fn ansi_char(code: u32) -> Option<char> {
    match code {
        0x80 => Some('\u{20AC}'),
        0x82 => Some('\u{201A}'),
        0x83 => Some('\u{0192}'),
        0x84 => Some('\u{201E}'),
        0x85 => Some('\u{2026}'),
        0x86 => Some('\u{2020}'),
        0x87 => Some('\u{2021}'),
        0x88 => Some('\u{02C6}'),
        0x89 => Some('\u{2030}'),
        0x8A => Some('\u{0160}'),
        0x8B => Some('\u{2039}'),
        0x8C => Some('\u{0152}'),
        0x8E => Some('\u{017D}'),
        0x91 => Some('\u{2018}'),
        0x92 => Some('\u{2019}'),
        0x93 => Some('\u{201C}'),
        0x94 => Some('\u{201D}'),
        0x95 => Some('\u{2022}'),
        0x96 => Some('\u{2013}'),
        0x97 => Some('\u{2014}'),
        0x98 => Some('\u{02DC}'),
        0x99 => Some('\u{2122}'),
        0x9A => Some('\u{0161}'),
        0x9B => Some('\u{203A}'),
        0x9C => Some('\u{0153}'),
        0x9E => Some('\u{017E}'),
        0x9F => Some('\u{0178}'),
        _ => char::from_u32(code),
    }
}

fn symbol_font_matches(font: Option<&str>, expected: &str) -> bool {
    let Some(font) = font else {
        return false;
    };
    font.chars()
        .filter(|ch| !ch.is_whitespace() && *ch != '-' && *ch != '_')
        .flat_map(char::to_lowercase)
        .collect::<String>()
        == expected
}

fn symbol_font_char(code: u32) -> Option<char> {
    Some(match code {
        0x41 => '\u{0391}',
        0x42 => '\u{0392}',
        0x43 => '\u{03A7}',
        0x44 => '\u{0394}',
        0x45 => '\u{0395}',
        0x46 => '\u{03A6}',
        0x47 => '\u{0393}',
        0x48 => '\u{0397}',
        0x49 => '\u{0399}',
        0x4B => '\u{039A}',
        0x4C => '\u{039B}',
        0x4D => '\u{039C}',
        0x4E => '\u{039D}',
        0x4F => '\u{039F}',
        0x50 => '\u{03A0}',
        0x51 => '\u{0398}',
        0x52 => '\u{03A1}',
        0x53 => '\u{03A3}',
        0x54 => '\u{03A4}',
        0x55 => '\u{03A5}',
        0x57 => '\u{03A9}',
        0x58 => '\u{039E}',
        0x59 => '\u{03A8}',
        0x5A => '\u{0396}',
        0x61 => '\u{03B1}',
        0x62 => '\u{03B2}',
        0x63 => '\u{03C7}',
        0x64 => '\u{03B4}',
        0x65 => '\u{03B5}',
        0x66 => '\u{03C6}',
        0x67 => '\u{03B3}',
        0x68 => '\u{03B7}',
        0x69 => '\u{03B9}',
        0x6B => '\u{03BA}',
        0x6C => '\u{03BB}',
        0x6D => '\u{03BC}',
        0x6E => '\u{03BD}',
        0x6F => '\u{03BF}',
        0x70 => '\u{03C0}',
        0x71 => '\u{03B8}',
        0x72 => '\u{03C1}',
        0x73 => '\u{03C3}',
        0x74 => '\u{03C4}',
        0x75 => '\u{03C5}',
        0x77 => '\u{03C9}',
        0x78 => '\u{03BE}',
        0x79 => '\u{03C8}',
        0x7A => '\u{03B6}',
        0xB7 => '\u{2022}',
        0xD3 => '\u{00A9}',
        _ => return None,
    })
}

fn wingdings_font_char(code: u32) -> Option<char> {
    Some(match code {
        0xFC => '\u{2713}',
        0xFB => '\u{2611}',
        0xFE => '\u{2612}',
        0xA8 => '\u{25CA}',
        0xD8 => '\u{27A2}',
        0xE0 => '\u{2794}',
        0xE8 => '\u{27A3}',
        0x6C => '\u{25CF}',
        0x6E => '\u{25A0}',
        0x75 => '\u{25C6}',
        _ => return None,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct StyleRefInstruction {
    style_identifier: String,
    text_format: Option<FieldTextFormat>,
    result: StyleRefResult,
    suppress_non_numeric: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StyleRefResult {
    Text,
    ParagraphNumber,
    RelativeContextNumber,
    FullContextNumber,
    RelativePosition,
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

fn style_ref_instruction(instruction: &str) -> Option<StyleRefInstruction> {
    let tokens = instruction_parts(instruction);
    let mut parts = tokens.iter().map(String::as_str);
    let kind = parts.next()?;
    if !kind.eq_ignore_ascii_case("STYLEREF") {
        return None;
    }
    let mut style_identifier = None;
    let mut text_format = None;
    let mut result = StyleRefResult::Text;
    let mut suppress_non_numeric = false;
    while let Some(part) = parts.next() {
        if part == "\\*" {
            if !accept_field_format_switch(parts.next()?, &mut text_format) {
                return None;
            }
            continue;
        }
        if let Some(format) = part.strip_prefix("\\*") {
            if !accept_field_format_switch(format, &mut text_format) {
                return None;
            }
            continue;
        }
        if part.starts_with('\\') {
            if part.eq_ignore_ascii_case("\\t") {
                if suppress_non_numeric {
                    return None;
                }
                suppress_non_numeric = true;
                continue;
            }
            if part.eq_ignore_ascii_case("\\n") {
                if result != StyleRefResult::Text {
                    return None;
                }
                result = StyleRefResult::ParagraphNumber;
                continue;
            }
            if part.eq_ignore_ascii_case("\\r") {
                if result != StyleRefResult::Text {
                    return None;
                }
                result = StyleRefResult::RelativeContextNumber;
                continue;
            }
            if part.eq_ignore_ascii_case("\\w") {
                if result != StyleRefResult::Text {
                    return None;
                }
                result = StyleRefResult::FullContextNumber;
                continue;
            }
            if part.eq_ignore_ascii_case("\\p") {
                if result != StyleRefResult::Text {
                    return None;
                }
                result = StyleRefResult::RelativePosition;
                continue;
            }
            return None;
        }
        let candidate = field_name_token(part)?;
        if style_identifier.replace(candidate.to_string()).is_some() {
            return None;
        }
    }
    if suppress_non_numeric
        && !matches!(
            result,
            StyleRefResult::ParagraphNumber
                | StyleRefResult::RelativeContextNumber
                | StyleRefResult::FullContextNumber
        )
    {
        return None;
    }
    Some(StyleRefInstruction {
        style_identifier: style_identifier?,
        text_format,
        result,
        suppress_non_numeric,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PageInstruction {
    number_format: Option<PageNumberFormat>,
    text_format: Option<FieldTextFormat>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PageRefInstruction {
    target: String,
    number_format: Option<PageNumberFormat>,
    text_format: Option<FieldTextFormat>,
    relative: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PageNumberFormat {
    Arabic,
    ArabicDash,
    DecimalZero,
    DecimalFullWidth,
    DecimalHalfWidth,
    DecimalFullWidth2,
    DecimalEnclosedCircle,
    DecimalEnclosedFullstop,
    DecimalEnclosedParen,
    Ganada,
    Chosung,
    KoreanDigital,
    KoreanCounting,
    KoreanLegal,
    KoreanDigital2,
    AlphabeticLower,
    AlphabeticUpper,
    RomanLower,
    RomanUpper,
    Ordinal,
    CardText,
    OrdText,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SequenceAction {
    Next,
    Current,
    Reset(i64),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SequenceInstruction {
    identifier: String,
    action: SequenceAction,
    hidden: bool,
    number_format: Option<PageNumberFormat>,
}

fn sequence_instruction(instruction: &str) -> Option<SequenceInstruction> {
    let tokens = instruction_parts(instruction);
    let mut parts = tokens.iter().map(String::as_str);
    let kind = parts.next()?;
    if !kind.eq_ignore_ascii_case("SEQ") {
        return None;
    }
    let identifier = field_identifier_token(parts.next()?)?.to_string();
    let mut action = SequenceAction::Next;
    let mut action_seen = false;
    let mut hidden = false;
    let mut number_format = None;
    while let Some(part) = parts.next() {
        if part == "\\*" {
            if !accept_page_number_format_switch(parts.next()?, &mut number_format) {
                return None;
            }
            continue;
        }
        if let Some(format) = part.strip_prefix("\\*") {
            if !accept_page_number_format_switch(format, &mut number_format) {
                return None;
            }
            continue;
        }
        if part.eq_ignore_ascii_case("\\n") {
            if action_seen {
                return None;
            }
            action_seen = true;
            action = SequenceAction::Next;
            continue;
        }
        if part.eq_ignore_ascii_case("\\c") {
            if action_seen {
                return None;
            }
            action_seen = true;
            action = SequenceAction::Current;
            continue;
        }
        if part.eq_ignore_ascii_case("\\h") {
            if hidden {
                return None;
            }
            hidden = true;
            continue;
        }
        if part.eq_ignore_ascii_case("\\r") {
            if action_seen {
                return None;
            }
            let reset = field_name_token(parts.next()?)?.parse::<i64>().ok()?;
            action_seen = true;
            action = SequenceAction::Reset(nonnegative_sequence_reset(reset)?);
            continue;
        }
        if let Some(reset) = strip_ascii_switch_prefix(part, "\\r") {
            if reset.is_empty() || action_seen {
                return None;
            }
            action_seen = true;
            let reset = field_name_token(reset)?.parse::<i64>().ok()?;
            action = SequenceAction::Reset(nonnegative_sequence_reset(reset)?);
            continue;
        }
        return None;
    }
    Some(SequenceInstruction {
        identifier,
        action,
        hidden,
        number_format,
    })
}

fn nonnegative_sequence_reset(value: i64) -> Option<i64> {
    (value >= 0).then_some(value)
}

pub(crate) fn computed_sequence_result(
    instruction: &str,
    counters: &mut HashMap<String, i64>,
) -> Option<String> {
    let instruction = sequence_instruction(instruction)?;
    let value = match instruction.action {
        SequenceAction::Next => counters.get(&instruction.identifier).copied().unwrap_or(0) + 1,
        SequenceAction::Current => counters.get(&instruction.identifier).copied()?,
        SequenceAction::Reset(value) => value,
    };
    let text = if instruction.hidden {
        if value < 0 {
            return None;
        }
        String::new()
    } else {
        format_sequence_number(value, instruction.number_format)?
    };
    if !matches!(instruction.action, SequenceAction::Current) {
        counters.insert(instruction.identifier, value);
    }
    Some(text)
}

fn format_sequence_number(value: i64, format: Option<PageNumberFormat>) -> Option<String> {
    let value = usize::try_from(value).ok()?;
    format_page_number(value, format)
}

pub(crate) fn computed_numbering_result(
    instruction: &str,
    autonum_counter: &mut i64,
) -> Option<String> {
    let tokens = instruction_parts(instruction);
    let mut parts = tokens.iter().map(String::as_str);
    let kind = parts.next()?;
    if !kind.eq_ignore_ascii_case("AUTONUM")
        && !kind.eq_ignore_ascii_case("AUTONUMLGL")
        && !kind.eq_ignore_ascii_case("AUTONUMOUT")
    {
        return None;
    }
    let accepts_separator_switch = kind.eq_ignore_ascii_case("AUTONUM");
    let mut number_format = None;
    let mut separator = None;
    while let Some(part) = parts.next() {
        if part == "\\*" {
            let switch = parts.next()?;
            if is_neutral_field_format_switch(switch) {
                continue;
            }
            if !accept_page_number_format_switch(switch, &mut number_format) {
                return None;
            }
            continue;
        }
        if let Some(format) = part.strip_prefix("\\*") {
            if is_neutral_field_format_switch(format) {
                continue;
            }
            if !accept_page_number_format_switch(format, &mut number_format) {
                return None;
            }
            continue;
        }
        if accepts_separator_switch && part.eq_ignore_ascii_case("\\s") {
            accept_autonum_separator_switch(parts.next()?, &mut separator)?;
            continue;
        }
        if accepts_separator_switch {
            if let Some(value) = strip_ascii_switch_prefix(part, "\\s") {
                accept_autonum_separator_switch(value, &mut separator)?;
                continue;
            }
        }
        if strip_ascii_switch_prefix(part, "\\s").is_some() || part.eq_ignore_ascii_case("\\s") {
            return None;
        }
        return None;
    }
    let value = *autonum_counter + 1;
    let mut text = format_sequence_number(value, number_format)?;
    if let Some(separator) = separator {
        text.push(separator);
    }
    *autonum_counter = value;
    Some(text)
}

pub(crate) fn supports_numbering_field_syntax(instruction: &str) -> bool {
    let tokens = instruction_parts(instruction);
    let mut parts = tokens.iter().map(String::as_str);
    let Some(kind) = parts.next() else {
        return false;
    };
    if kind.eq_ignore_ascii_case("AUTONUM")
        || kind.eq_ignore_ascii_case("AUTONUMLGL")
        || kind.eq_ignore_ascii_case("AUTONUMOUT")
    {
        let mut counter = 0;
        return computed_numbering_result(instruction, &mut counter).is_some();
    }
    if kind.eq_ignore_ascii_case("LISTNUM") {
        let mut counter = 0;
        if computed_listnum_result(instruction, &mut counter).is_some() {
            return true;
        }
        return supports_listnum_field_syntax(parts);
    }
    kind.eq_ignore_ascii_case("BIDIOUTLINE")
}

pub(crate) fn computed_listnum_result(
    instruction: &str,
    listnum_counter: &mut i64,
) -> Option<String> {
    let tokens = instruction_parts(instruction);
    let mut parts = tokens.iter().map(String::as_str);
    let kind = parts.next()?;
    if !kind.eq_ignore_ascii_case("LISTNUM") {
        return None;
    }
    let mut list_name_seen = false;
    let mut level_seen = false;
    let mut reset_start = None;
    let mut number_format = None;
    while let Some(part) = parts.next() {
        if part == "\\*" {
            if !accept_page_number_format_switch(parts.next()?, &mut number_format) {
                return None;
            }
            continue;
        }
        if let Some(format) = part.strip_prefix("\\*") {
            if !accept_page_number_format_switch(format, &mut number_format) {
                return None;
            }
            continue;
        }
        if part.eq_ignore_ascii_case("\\l") {
            accept_listnum_level_switch(parts.next()?, &mut level_seen)?;
            continue;
        }
        if let Some(level) = strip_ascii_switch_prefix(part, "\\l") {
            if level.is_empty() {
                return None;
            }
            accept_listnum_level_switch(level, &mut level_seen)?;
            continue;
        }
        if part.eq_ignore_ascii_case("\\s") {
            accept_listnum_start_switch(parts.next()?, &mut reset_start)?;
            continue;
        }
        if let Some(start) = strip_ascii_switch_prefix(part, "\\s") {
            if start.is_empty() {
                return None;
            }
            accept_listnum_start_switch(start, &mut reset_start)?;
            continue;
        }
        if part.starts_with('\\') || list_name_seen {
            return None;
        }
        let list_name = unquote_field_text(part);
        if !list_name.eq_ignore_ascii_case("NumberDefault") {
            return None;
        }
        list_name_seen = true;
    }
    let value = reset_start.unwrap_or(*listnum_counter + 1);
    let text = format_sequence_number(value, number_format)?;
    *listnum_counter = value;
    Some(text)
}

fn supports_listnum_field_syntax<'a>(mut parts: impl Iterator<Item = &'a str>) -> bool {
    let mut list_name_seen = false;
    let mut level_seen = false;
    let mut reset_start = None;
    let mut number_format = None;
    while let Some(part) = parts.next() {
        if part == "\\*" {
            let Some(format) = parts.next() else {
                return false;
            };
            if !accept_page_number_format_switch(format, &mut number_format) {
                return false;
            }
            continue;
        }
        if let Some(format) = part.strip_prefix("\\*") {
            if !accept_page_number_format_switch(format, &mut number_format) {
                return false;
            }
            continue;
        }
        if part.eq_ignore_ascii_case("\\l") {
            let Some(level) = parts.next() else {
                return false;
            };
            if !accept_listnum_syntax_level_switch(level, &mut level_seen) {
                return false;
            }
            continue;
        }
        if let Some(level) = strip_ascii_switch_prefix(part, "\\l") {
            if level.is_empty() || !accept_listnum_syntax_level_switch(level, &mut level_seen) {
                return false;
            }
            continue;
        }
        if part.eq_ignore_ascii_case("\\s") {
            let Some(start) = parts.next() else {
                return false;
            };
            if accept_listnum_start_switch(start, &mut reset_start).is_none() {
                return false;
            }
            continue;
        }
        if let Some(start) = strip_ascii_switch_prefix(part, "\\s") {
            if start.is_empty() || accept_listnum_start_switch(start, &mut reset_start).is_none() {
                return false;
            }
            continue;
        }
        if part.starts_with('\\') || list_name_seen || field_name_token(part).is_none() {
            return false;
        }
        list_name_seen = true;
    }
    true
}

fn accept_listnum_level_switch(part: &str, level_seen: &mut bool) -> Option<()> {
    if *level_seen {
        return None;
    }
    let level = field_name_token(part)?.parse::<u8>().ok()?;
    if level != 1 {
        return None;
    }
    *level_seen = true;
    Some(())
}

fn accept_listnum_syntax_level_switch(part: &str, level_seen: &mut bool) -> bool {
    if *level_seen {
        return false;
    }
    let Some(level) = field_name_token(part).and_then(|part| part.parse::<u8>().ok()) else {
        return false;
    };
    if level == 0 {
        return false;
    }
    *level_seen = true;
    true
}

fn accept_listnum_start_switch(part: &str, reset_start: &mut Option<i64>) -> Option<()> {
    let start = field_name_token(part)?.parse::<i64>().ok()?;
    if start < 0 || reset_start.replace(start).is_some() {
        return None;
    }
    Some(())
}

fn accept_autonum_separator_switch(part: &str, separator: &mut Option<char>) -> Option<()> {
    let value = field_literal_token(part)?;
    let mut chars = value.chars();
    let ch = chars.next()?;
    if chars.next().is_some() || separator.replace(ch).is_some() {
        return None;
    }
    Some(())
}

fn page_instruction(instruction: &str) -> Option<PageInstruction> {
    let tokens = instruction_parts(instruction);
    let mut parts = tokens.iter().map(String::as_str);
    let kind = parts.next()?;
    if !kind.eq_ignore_ascii_case("PAGE") {
        return None;
    }
    let mut number_format = None;
    let mut text_format = None;
    while let Some(part) = parts.next() {
        if part == "\\*" {
            if !accept_page_field_format_switch(parts.next()?, &mut number_format, &mut text_format)
            {
                return None;
            }
            continue;
        }
        if let Some(format) = part.strip_prefix("\\*") {
            if !accept_page_field_format_switch(format, &mut number_format, &mut text_format) {
                return None;
            }
            continue;
        }
        return None;
    }
    Some(PageInstruction {
        number_format,
        text_format,
    })
}

fn page_ref_instruction(instruction: &str) -> Option<PageRefInstruction> {
    let tokens = instruction_parts(instruction);
    let mut parts = tokens.iter().map(String::as_str);
    let kind = parts.next()?;
    if !kind.eq_ignore_ascii_case("PAGEREF") {
        return None;
    }
    let mut target = None;
    let mut number_format = None;
    let mut text_format = None;
    let mut relative = false;
    while let Some(part) = parts.next() {
        if part == "\\*" {
            if !accept_page_field_format_switch(parts.next()?, &mut number_format, &mut text_format)
            {
                return None;
            }
            continue;
        }
        if let Some(format) = part.strip_prefix("\\*") {
            if !accept_page_field_format_switch(format, &mut number_format, &mut text_format) {
                return None;
            }
            continue;
        }
        if part.starts_with('\\') {
            if part.eq_ignore_ascii_case("\\h") {
                continue;
            }
            if part.eq_ignore_ascii_case("\\p") {
                if relative {
                    return None;
                }
                relative = true;
                continue;
            }
            return None;
        }
        let candidate = bookmark_target_identifier(part)?;
        if target.replace(candidate.to_string()).is_some() {
            return None;
        }
    }
    Some(PageRefInstruction {
        target: target?,
        number_format,
        text_format,
        relative,
    })
}

fn accept_page_field_format_switch(
    part: &str,
    number_format: &mut Option<PageNumberFormat>,
    text_format: &mut Option<FieldTextFormat>,
) -> bool {
    accept_page_number_format_switch(part, number_format)
        || accept_field_format_switch(part, text_format)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct NoteRefInstruction {
    target: String,
    text_format: Option<FieldTextFormat>,
    relative: bool,
}

fn note_ref_instruction(instruction: &str) -> Option<NoteRefInstruction> {
    let tokens = instruction_parts(instruction);
    let mut parts = tokens.iter().map(String::as_str);
    let kind = parts.next()?;
    if !is_note_ref_kind(kind) {
        return None;
    }
    let mut target = None;
    let mut text_format = None;
    let mut relative = false;
    let mut formatted = false;
    while let Some(part) = parts.next() {
        if part == "\\*" {
            if !accept_field_format_switch(parts.next()?, &mut text_format) {
                return None;
            }
            continue;
        }
        if let Some(format) = part.strip_prefix("\\*") {
            if !accept_field_format_switch(format, &mut text_format) {
                return None;
            }
            continue;
        }
        if part.starts_with('\\') {
            if part.eq_ignore_ascii_case("\\h") {
                continue;
            }
            if part.eq_ignore_ascii_case("\\f") {
                if formatted {
                    return None;
                }
                formatted = true;
                continue;
            }
            if part.eq_ignore_ascii_case("\\p") {
                if relative {
                    return None;
                }
                relative = true;
                continue;
            }
            return None;
        }
        let candidate = bookmark_target_identifier(part)?;
        if target.replace(candidate.to_string()).is_some() {
            return None;
        }
    }
    Some(NoteRefInstruction {
        target: target?,
        text_format,
        relative,
    })
}

fn is_note_ref_kind(kind: &str) -> bool {
    kind.eq_ignore_ascii_case("NOTEREF") || kind.eq_ignore_ascii_case("FTNREF")
}

fn is_neutral_field_format_switch(part: &str) -> bool {
    part.eq_ignore_ascii_case("MERGEFORMAT") || part.eq_ignore_ascii_case("CHARFORMAT")
}

fn accept_page_number_format_switch(
    part: &str,
    number_format: &mut Option<PageNumberFormat>,
) -> bool {
    if is_neutral_field_format_switch(part) {
        return true;
    }
    let format = match part {
        _ if part.eq_ignore_ascii_case("Arabic") => PageNumberFormat::Arabic,
        "alphabetic" => PageNumberFormat::AlphabeticLower,
        "ALPHABETIC" => PageNumberFormat::AlphabeticUpper,
        "roman" => PageNumberFormat::RomanLower,
        "ROMAN" => PageNumberFormat::RomanUpper,
        _ if part.eq_ignore_ascii_case("Ordinal") => PageNumberFormat::Ordinal,
        _ if part.eq_ignore_ascii_case("CardText") => PageNumberFormat::CardText,
        _ if part.eq_ignore_ascii_case("OrdText") => PageNumberFormat::OrdText,
        _ if part.eq_ignore_ascii_case("ArabicDash") => PageNumberFormat::ArabicDash,
        _ => return false,
    };
    number_format.replace(format).is_none()
}

fn format_page_number(page: usize, format: Option<PageNumberFormat>) -> Option<String> {
    match format.unwrap_or(PageNumberFormat::Arabic) {
        PageNumberFormat::Arabic => Some(page.to_string()),
        PageNumberFormat::ArabicDash => Some(format!("- {page} -")),
        PageNumberFormat::DecimalZero => Some(format!("{page:02}")),
        PageNumberFormat::DecimalFullWidth => u32::try_from(page)
            .ok()
            .map(|page| numfmt::format(page, 0x0E)),
        PageNumberFormat::DecimalHalfWidth => u32::try_from(page)
            .ok()
            .map(|page| numfmt::format(page, 0x0F)),
        PageNumberFormat::DecimalFullWidth2 => u32::try_from(page)
            .ok()
            .map(|page| numfmt::format(page, 0x13)),
        PageNumberFormat::DecimalEnclosedCircle => u32::try_from(page)
            .ok()
            .map(|page| numfmt::format(page, 0x12)),
        PageNumberFormat::DecimalEnclosedFullstop => u32::try_from(page)
            .ok()
            .map(|page| numfmt::format(page, 0x1A)),
        PageNumberFormat::DecimalEnclosedParen => u32::try_from(page)
            .ok()
            .map(|page| numfmt::format(page, 0x1B)),
        PageNumberFormat::Ganada => u32::try_from(page)
            .ok()
            .map(|page| numfmt::format(page, 0x18)),
        PageNumberFormat::Chosung => u32::try_from(page)
            .ok()
            .map(|page| numfmt::format(page, 0x19)),
        PageNumberFormat::KoreanDigital => u32::try_from(page)
            .ok()
            .map(|page| numfmt::format(page, 0x29)),
        PageNumberFormat::KoreanCounting => u32::try_from(page)
            .ok()
            .map(|page| numfmt::format(page, 0x2A)),
        PageNumberFormat::KoreanLegal => u32::try_from(page)
            .ok()
            .map(|page| numfmt::format(page, 0x2B)),
        PageNumberFormat::KoreanDigital2 => u32::try_from(page)
            .ok()
            .map(|page| numfmt::format(page, 0x2C)),
        PageNumberFormat::AlphabeticLower => alphabetic_page_number(page, false),
        PageNumberFormat::AlphabeticUpper => alphabetic_page_number(page, true),
        PageNumberFormat::RomanLower => roman_page_number(page).map(|value| value.to_lowercase()),
        PageNumberFormat::RomanUpper => roman_page_number(page),
        PageNumberFormat::Ordinal => Some(ordinal_page_number(page)),
        PageNumberFormat::CardText => cardinal_page_number_text(page),
        PageNumberFormat::OrdText => ordinal_page_number_text(page),
    }
}

fn format_page_ref_number(
    page: usize,
    field_format: Option<PageNumberFormat>,
    page_format: PageRefDisplayFormat,
) -> Option<String> {
    let format = match field_format {
        Some(format) => Some(format),
        None => match page_format {
            PageRefDisplayFormat::Known(format) => format,
            PageRefDisplayFormat::Unsupported => return None,
        },
    };
    format_page_number(page, format)
}

fn alphabetic_page_number(mut page: usize, uppercase: bool) -> Option<String> {
    if page == 0 {
        return None;
    }
    let base = if uppercase { b'A' } else { b'a' };
    let mut chars = Vec::new();
    while page > 0 {
        page -= 1;
        chars.push((base + (page % 26) as u8) as char);
        page /= 26;
    }
    chars.reverse();
    Some(chars.into_iter().collect())
}

fn roman_page_number(mut page: usize) -> Option<String> {
    if page == 0 || page > 3999 {
        return None;
    }
    let mut out = String::new();
    for (value, numeral) in [
        (1000, "M"),
        (900, "CM"),
        (500, "D"),
        (400, "CD"),
        (100, "C"),
        (90, "XC"),
        (50, "L"),
        (40, "XL"),
        (10, "X"),
        (9, "IX"),
        (5, "V"),
        (4, "IV"),
        (1, "I"),
    ] {
        while page >= value {
            out.push_str(numeral);
            page -= value;
        }
    }
    Some(out)
}

fn ordinal_page_number(page: usize) -> String {
    let suffix = if (11..=13).contains(&(page % 100)) {
        "th"
    } else {
        match page % 10 {
            1 => "st",
            2 => "nd",
            3 => "rd",
            _ => "th",
        }
    };
    format!("{page}{suffix}")
}

fn cardinal_page_number_text(page: usize) -> Option<String> {
    cardinal_number_text(page)
}

fn ordinal_page_number_text(page: usize) -> Option<String> {
    ordinal_number_text(page)
}

fn cardinal_number_text(number: usize) -> Option<String> {
    if number == 0 {
        return Some("zero".to_string());
    }
    cardinal_positive_number_text(number)
}

fn cardinal_positive_number_text(number: usize) -> Option<String> {
    const SCALES: &[(usize, &str)] = &[
        (1_000_000_000_000, "trillion"),
        (1_000_000_000, "billion"),
        (1_000_000, "million"),
        (1_000, "thousand"),
    ];
    if number < 20 {
        return Some(SMALL_NUMBER_WORDS[number].to_string());
    }
    if number < 100 {
        let tens = number / 10;
        let rest = number % 10;
        let tens_word = TENS_NUMBER_WORDS[tens];
        return Some(if rest == 0 {
            tens_word.to_string()
        } else {
            format!("{tens_word}-{}", SMALL_NUMBER_WORDS[rest])
        });
    }
    if number < 1_000 {
        let hundreds = number / 100;
        let rest = number % 100;
        let prefix = format!("{} hundred", SMALL_NUMBER_WORDS[hundreds]);
        return Some(if rest == 0 {
            prefix
        } else {
            format!("{prefix} {}", cardinal_positive_number_text(rest)?)
        });
    }
    for (value, name) in SCALES {
        if number >= *value {
            let major = number / *value;
            let rest = number % *value;
            let prefix = format!("{} {name}", cardinal_positive_number_text(major)?);
            return Some(if rest == 0 {
                prefix
            } else {
                format!("{prefix} {}", cardinal_positive_number_text(rest)?)
            });
        }
    }
    None
}

fn ordinal_number_text(number: usize) -> Option<String> {
    if number < 20 {
        return Some(SMALL_ORDINAL_WORDS[number].to_string());
    }
    if number < 100 {
        let tens = number / 10;
        let rest = number % 10;
        let tens_word = TENS_NUMBER_WORDS[tens];
        return Some(if rest == 0 {
            TENS_ORDINAL_WORDS[tens].to_string()
        } else {
            format!("{tens_word}-{}", ordinal_number_text(rest)?)
        });
    }
    if number < 1_000 {
        let hundreds = number / 100;
        let rest = number % 100;
        let prefix = format!("{} hundred", SMALL_NUMBER_WORDS[hundreds]);
        return Some(if rest == 0 {
            format!("{prefix}th")
        } else {
            format!("{prefix} {}", ordinal_number_text(rest)?)
        });
    }
    for (value, name) in [
        (1_000_000_000_000usize, "trillion"),
        (1_000_000_000, "billion"),
        (1_000_000, "million"),
        (1_000, "thousand"),
    ] {
        if number >= value {
            let major = number / value;
            let rest = number % value;
            let prefix = cardinal_positive_number_text(major)?;
            return Some(if rest == 0 {
                format!("{prefix} {name}th")
            } else {
                format!("{prefix} {name} {}", ordinal_number_text(rest)?)
            });
        }
    }
    None
}

const SMALL_NUMBER_WORDS: [&str; 20] = [
    "zero",
    "one",
    "two",
    "three",
    "four",
    "five",
    "six",
    "seven",
    "eight",
    "nine",
    "ten",
    "eleven",
    "twelve",
    "thirteen",
    "fourteen",
    "fifteen",
    "sixteen",
    "seventeen",
    "eighteen",
    "nineteen",
];

const SMALL_ORDINAL_WORDS: [&str; 20] = [
    "zeroth",
    "first",
    "second",
    "third",
    "fourth",
    "fifth",
    "sixth",
    "seventh",
    "eighth",
    "ninth",
    "tenth",
    "eleventh",
    "twelfth",
    "thirteenth",
    "fourteenth",
    "fifteenth",
    "sixteenth",
    "seventeenth",
    "eighteenth",
    "nineteenth",
];

const TENS_NUMBER_WORDS: [&str; 10] = [
    "", "", "twenty", "thirty", "forty", "fifty", "sixty", "seventy", "eighty", "ninety",
];

const TENS_ORDINAL_WORDS: [&str; 10] = [
    "",
    "",
    "twentieth",
    "thirtieth",
    "fortieth",
    "fiftieth",
    "sixtieth",
    "seventieth",
    "eightieth",
    "ninetieth",
];

#[derive(Debug, Clone, PartialEq, Eq)]
struct RefInstruction {
    target: String,
    text_format: Option<FieldTextFormat>,
    note_reference: bool,
    sequence_separator: bool,
    relative: bool,
    paragraph_number: bool,
    full_context_number: bool,
    relative_context_number: bool,
    suppress_non_numeric: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FieldTextFormat {
    Upper,
    Lower,
    Caps,
    FirstCap,
}

fn ref_instruction(instruction: &str) -> Option<RefInstruction> {
    let tokens = instruction_parts(instruction);
    let mut parts = tokens.iter().map(String::as_str);
    let kind = parts.next()?;
    if !kind.eq_ignore_ascii_case("REF") {
        return None;
    }
    parse_ref_instruction_parts(parts)
}

fn parse_ref_instruction_parts<'a>(
    mut parts: impl Iterator<Item = &'a str>,
) -> Option<RefInstruction> {
    let mut target = None;
    let mut text_format = None;
    let mut note_reference = false;
    let mut sequence_separator = false;
    let mut relative = false;
    let mut paragraph_number = false;
    let mut full_context_number = false;
    let mut relative_context_number = false;
    let mut suppress_non_numeric = false;
    while let Some(part) = parts.next() {
        if part == "\\*" {
            if !accept_field_format_switch(parts.next()?, &mut text_format) {
                return None;
            }
            continue;
        }
        if let Some(format) = part.strip_prefix("\\*") {
            if !accept_field_format_switch(format, &mut text_format) {
                return None;
            }
            continue;
        }
        if part.starts_with('\\') {
            if part.eq_ignore_ascii_case("\\t") {
                if suppress_non_numeric {
                    return None;
                }
                suppress_non_numeric = true;
                continue;
            }
            if part.eq_ignore_ascii_case("\\f") {
                if note_reference {
                    return None;
                }
                note_reference = true;
                continue;
            }
            if part.eq_ignore_ascii_case("\\d") {
                if sequence_separator {
                    return None;
                }
                let separator = field_literal_token(parts.next()?)?;
                if separator.is_empty() || separator.starts_with('\\') {
                    return None;
                }
                sequence_separator = true;
                continue;
            }
            if let Some(separator) = strip_ascii_switch_prefix(part, "\\d") {
                if sequence_separator {
                    return None;
                }
                let separator = field_literal_token(separator)?;
                if separator.is_empty() || separator.starts_with('\\') {
                    return None;
                }
                sequence_separator = true;
                continue;
            }
            if part.eq_ignore_ascii_case("\\n") {
                if paragraph_number || full_context_number || relative_context_number {
                    return None;
                }
                paragraph_number = true;
                continue;
            }
            if part.eq_ignore_ascii_case("\\w") {
                if full_context_number || paragraph_number || relative_context_number {
                    return None;
                }
                full_context_number = true;
                continue;
            }
            if part.eq_ignore_ascii_case("\\r") {
                if relative_context_number || paragraph_number || full_context_number {
                    return None;
                }
                relative_context_number = true;
                continue;
            }
            if part.eq_ignore_ascii_case("\\p") {
                if relative {
                    return None;
                }
                relative = true;
                continue;
            }
            if is_ref_value_neutral_switch(part) {
                continue;
            }
            return None;
        }
        let candidate = bookmark_target_identifier(part)?;
        if target.replace(candidate.to_string()).is_some() {
            return None;
        }
    }
    if suppress_non_numeric && !(paragraph_number || full_context_number || relative_context_number)
    {
        return None;
    }
    if note_reference
        && (relative
            || paragraph_number
            || full_context_number
            || relative_context_number
            || suppress_non_numeric
            || sequence_separator)
    {
        return None;
    }
    Some(RefInstruction {
        target: target?,
        text_format,
        note_reference,
        sequence_separator,
        relative,
        paragraph_number,
        full_context_number,
        relative_context_number,
        suppress_non_numeric,
    })
}

fn bookmark_target_identifier(value: &str) -> Option<&str> {
    field_identifier_token(value)
}

fn ref_note_field_target(instruction: &str) -> Option<String> {
    let spec =
        ref_instruction(instruction).or_else(|| direct_bookmark_ref_instruction(instruction))?;
    (spec.note_reference && !spec.sequence_separator).then_some(spec.target)
}

fn is_ref_value_neutral_switch(part: &str) -> bool {
    part.eq_ignore_ascii_case("\\h")
}

fn accept_field_format_switch(part: &str, text_format: &mut Option<FieldTextFormat>) -> bool {
    if part.eq_ignore_ascii_case("MERGEFORMAT") || part.eq_ignore_ascii_case("CHARFORMAT") {
        return true;
    }
    let format = if part.eq_ignore_ascii_case("Upper") {
        FieldTextFormat::Upper
    } else if part.eq_ignore_ascii_case("Lower") {
        FieldTextFormat::Lower
    } else if part.eq_ignore_ascii_case("Caps") {
        FieldTextFormat::Caps
    } else if part.eq_ignore_ascii_case("FirstCap") {
        FieldTextFormat::FirstCap
    } else {
        return false;
    };
    text_format.replace(format).is_none()
}

fn capitalize_first_word(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut changed = false;
    for ch in text.chars() {
        if !changed && ch.is_alphabetic() {
            out.extend(ch.to_uppercase());
            changed = true;
        } else {
            out.push(ch);
        }
    }
    out
}

fn capitalize_words(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut at_word_start = true;
    for ch in text.chars() {
        if ch.is_alphabetic() {
            if at_word_start {
                out.extend(ch.to_uppercase());
            } else {
                out.push(ch);
            }
            at_word_start = false;
        } else {
            out.push(ch);
            at_word_start = !ch.is_alphanumeric();
        }
    }
    out
}

pub(crate) fn computed_toc_result(instruction: &str, toc_entries: &[TocEntry]) -> Option<String> {
    let spec = toc_spec(instruction)?;
    let lines: Vec<_> = toc_entries
        .iter()
        .filter_map(|entry| toc_entry_for_spec(entry, &spec))
        .map(|(level, text)| {
            format!(
                "{}{}",
                "  ".repeat(level.saturating_sub(spec.start) as usize),
                text
            )
        })
        .collect();
    (!lines.is_empty()).then(|| apply_field_text_format(lines.join("\n"), spec.text_format))
}

fn apply_field_text_format(text: String, format: Option<FieldTextFormat>) -> String {
    match format {
        Some(FieldTextFormat::Upper) => text.to_uppercase(),
        Some(FieldTextFormat::Lower) => text.to_lowercase(),
        Some(FieldTextFormat::Caps) => capitalize_words(&text),
        Some(FieldTextFormat::FirstCap) => capitalize_first_word(&text),
        None => text,
    }
}

fn toc_entry_for_spec<'a>(entry: &'a TocEntry, spec: &TocSpec) -> Option<(u8, &'a str)> {
    if !spec.bookmark.as_ref().map_or(true, |bookmark| {
        entry.bookmarks.iter().any(|name| name == bookmark)
    }) {
        return None;
    }
    if entry.source == TocEntrySource::TcField {
        if spec.tc_filter.is_none() && spec.tc_level_range.is_none() {
            return None;
        }
        if let Some(TcFilter::EntryType(expected)) = &spec.tc_filter {
            if !entry
                .tc_type
                .as_ref()
                .is_some_and(|actual| actual.eq_ignore_ascii_case(expected))
            {
                return None;
            }
        }
        let (start, end) = spec.tc_level_range.unwrap_or((1, 9));
        return (start..=end)
            .contains(&entry.level)
            .then_some((entry.level, entry.text.as_str()));
    }
    if entry.source == TocEntrySource::SequenceField {
        let filter = spec.sequence_filter.as_ref()?;
        if !entry
            .sequence_identifier
            .as_ref()
            .is_some_and(|actual| actual.eq_ignore_ascii_case(filter.identifier()))
        {
            return None;
        }
        let text = match filter {
            TocSequenceFilter::FullCaption(_) => entry.text.as_str(),
            TocSequenceFilter::CaptionText(_) => entry.sequence_caption_text.as_deref()?,
        };
        return Some((1, text));
    }
    if spec.include_standard
        && (spec.start..=spec.end).contains(&entry.level)
        && (!spec.outline_only || entry.source == TocEntrySource::OutlineLevel)
    {
        return Some((entry.level, entry.text.as_str()));
    }
    spec.custom_styles
        .iter()
        .find(|style| toc_entry_matches_style(entry, &style.name))
        .map(|style| (style.level, entry.text.as_str()))
}

fn toc_entry_matches_style(entry: &TocEntry, name: &str) -> bool {
    entry
        .style_id
        .as_ref()
        .is_some_and(|style_id| style_id.eq_ignore_ascii_case(name))
        || entry
            .style_name
            .as_ref()
            .is_some_and(|style_name| style_name.eq_ignore_ascii_case(name))
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TocSpec {
    start: u8,
    end: u8,
    outline_only: bool,
    include_standard: bool,
    custom_styles: Vec<TocStyleSpec>,
    tc_filter: Option<TcFilter>,
    tc_level_range: Option<(u8, u8)>,
    sequence_filter: Option<TocSequenceFilter>,
    bookmark: Option<String>,
    text_format: Option<FieldTextFormat>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TocStyleSpec {
    name: String,
    level: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum TcFilter {
    All,
    EntryType(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum TocSequenceFilter {
    FullCaption(String),
    CaptionText(String),
}

impl TocSequenceFilter {
    fn identifier(&self) -> &str {
        match self {
            Self::FullCaption(identifier) | Self::CaptionText(identifier) => identifier,
        }
    }
}

fn toc_spec(instruction: &str) -> Option<TocSpec> {
    let tokens = instruction_parts(instruction);
    let mut parts = tokens.iter().map(String::as_str).peekable();
    let kind = parts.next()?;
    if !kind.eq_ignore_ascii_case("TOC") {
        return None;
    }
    let mut saw_switch = false;
    let mut outline_range = None;
    let mut saw_outline_switch = false;
    let mut bookmark = None;
    let mut custom_styles = Vec::new();
    let mut tc_filter = None;
    let mut tc_level_range = None;
    let mut sequence_filter = None;
    let mut text_format = None;
    let mut saw_page_number_sequence_prefix = false;
    let mut saw_default_toc_neutral_switch = false;
    while let Some(part) = parts.next() {
        saw_switch = true;
        if part == "\\*" {
            if !accept_field_format_switch(parts.next()?, &mut text_format) {
                return None;
            }
            saw_default_toc_neutral_switch = true;
            continue;
        }
        if let Some(format) = part.strip_prefix("\\*") {
            if !accept_field_format_switch(format, &mut text_format) {
                return None;
            }
            saw_default_toc_neutral_switch = true;
            continue;
        }
        if is_toc_value_neutral_switch(part) {
            saw_default_toc_neutral_switch = true;
            continue;
        }
        if part.eq_ignore_ascii_case("\\f") {
            let filter = match parts.next_if(|next| !next.starts_with('\\')) {
                Some(value) => TcFilter::EntryType(tc_type_identifier(value)?),
                None => TcFilter::All,
            };
            if !accept_tc_filter(&mut tc_filter, filter) {
                return None;
            }
            continue;
        }
        if let Some(value) = strip_ascii_switch_prefix(part, "\\f") {
            let filter = if value.is_empty() {
                TcFilter::All
            } else {
                TcFilter::EntryType(tc_type_identifier(value)?)
            };
            if !accept_tc_filter(&mut tc_filter, filter) {
                return None;
            }
            continue;
        }
        if part.eq_ignore_ascii_case("\\a") {
            let value = parts.next_if(|next| !next.starts_with('\\'))?;
            if !accept_sequence_filter(&mut sequence_filter, value, TocSequenceFilter::CaptionText)
            {
                return None;
            }
            continue;
        }
        if let Some(value) = strip_ascii_switch_prefix(part, "\\a") {
            if value.is_empty()
                || !accept_sequence_filter(
                    &mut sequence_filter,
                    value,
                    TocSequenceFilter::CaptionText,
                )
            {
                return None;
            }
            continue;
        }
        if part.eq_ignore_ascii_case("\\c") {
            let value = parts.next_if(|next| !next.starts_with('\\'))?;
            if !accept_sequence_filter(&mut sequence_filter, value, TocSequenceFilter::FullCaption)
            {
                return None;
            }
            continue;
        }
        if let Some(value) = strip_ascii_switch_prefix(part, "\\c") {
            if value.is_empty()
                || !accept_sequence_filter(
                    &mut sequence_filter,
                    value,
                    TocSequenceFilter::FullCaption,
                )
            {
                return None;
            }
            continue;
        }
        if part.eq_ignore_ascii_case("\\l") {
            let range = parts.next_if(|next| !next.starts_with('\\'))?;
            if tc_level_range
                .replace(parse_toc_outline_range(range)?)
                .is_some()
            {
                return None;
            }
            continue;
        }
        if let Some(range) = strip_ascii_switch_prefix(part, "\\l") {
            if range.is_empty()
                || tc_level_range
                    .replace(parse_toc_outline_range(range)?)
                    .is_some()
            {
                return None;
            }
            continue;
        }
        if part.eq_ignore_ascii_case("\\u") {
            saw_outline_switch = true;
            continue;
        }
        if part.eq_ignore_ascii_case("\\n") {
            if let Some(range) = parts.next_if(|next| !next.starts_with('\\')) {
                parse_toc_outline_range(range)?;
            }
            saw_default_toc_neutral_switch = true;
            continue;
        }
        if let Some(range) = strip_ascii_switch_prefix(part, "\\n") {
            if range.is_empty() {
                return None;
            }
            parse_toc_outline_range(range)?;
            saw_default_toc_neutral_switch = true;
            continue;
        }
        if part.eq_ignore_ascii_case("\\p") {
            field_literal_token(parts.next_if(|next| !next.starts_with('\\'))?)?;
            saw_default_toc_neutral_switch = true;
            continue;
        }
        if let Some(separator) = strip_ascii_switch_prefix(part, "\\p") {
            field_literal_token(separator)?;
            saw_default_toc_neutral_switch = true;
            continue;
        }
        if part.eq_ignore_ascii_case("\\d") {
            field_literal_token(parts.next_if(|next| !next.starts_with('\\'))?)?;
            saw_default_toc_neutral_switch = true;
            continue;
        }
        if let Some(separator) = strip_ascii_switch_prefix(part, "\\d") {
            field_literal_token(separator)?;
            saw_default_toc_neutral_switch = true;
            continue;
        }
        if part.eq_ignore_ascii_case("\\s") {
            let identifier = parts.next_if(|next| !next.starts_with('\\'))?;
            if saw_page_number_sequence_prefix || toc_sequence_identifier(identifier).is_none() {
                return None;
            }
            saw_page_number_sequence_prefix = true;
            saw_default_toc_neutral_switch = true;
            continue;
        }
        if let Some(identifier) = strip_ascii_switch_prefix(part, "\\s") {
            if saw_page_number_sequence_prefix || toc_sequence_identifier(identifier).is_none() {
                return None;
            }
            saw_page_number_sequence_prefix = true;
            saw_default_toc_neutral_switch = true;
            continue;
        }
        if part.eq_ignore_ascii_case("\\b") {
            let target = parts.next_if(|next| !next.starts_with('\\'))?;
            let target = field_identifier_token(target)?;
            if bookmark.replace(target.to_string()).is_some() {
                return None;
            }
            continue;
        }
        if let Some(target) = strip_ascii_switch_prefix(part, "\\b") {
            let target = field_identifier_token(target)?;
            if bookmark.replace(target.to_string()).is_some() {
                return None;
            }
            continue;
        }
        if part.eq_ignore_ascii_case("\\t") {
            let specs = parse_toc_style_specs(parts.next_if(|next| !next.starts_with('\\'))?)?;
            custom_styles.extend(specs);
            continue;
        }
        if let Some(value) = strip_ascii_switch_prefix(part, "\\t") {
            if value.is_empty() {
                return None;
            }
            custom_styles.extend(parse_toc_style_specs(value)?);
            continue;
        }
        let range = if part.eq_ignore_ascii_case("\\o") {
            match parts.next_if(|next| !next.starts_with('\\')) {
                Some(range) => range,
                None => {
                    if outline_range.replace((1, 9)).is_some() {
                        return None;
                    }
                    continue;
                }
            }
        } else {
            strip_ascii_switch_prefix(part, "\\o")?
        };
        if outline_range
            .replace(parse_toc_outline_range(range)?)
            .is_some()
        {
            return None;
        }
    }
    let (start, end, outline_only, include_standard) = if saw_switch {
        match outline_range {
            Some((start, end)) => (start, end, false, true),
            None if saw_outline_switch => (1, 9, true, true),
            None if !custom_styles.is_empty() => (1, 9, false, false),
            None if tc_filter.is_some() || tc_level_range.is_some() => {
                let (start, end) = tc_level_range.unwrap_or((1, 9));
                (start, end, false, false)
            }
            None if sequence_filter.is_some() => (1, 9, false, false),
            None if bookmark.is_some() => (1, 3, false, true),
            None if saw_default_toc_neutral_switch => (1, 3, false, true),
            None => return None,
        }
    } else {
        (1, 3, false, true)
    };
    Some(TocSpec {
        start,
        end,
        outline_only,
        include_standard,
        custom_styles,
        tc_filter,
        tc_level_range,
        sequence_filter,
        bookmark,
        text_format,
    })
}

fn accept_tc_filter(slot: &mut Option<TcFilter>, filter: TcFilter) -> bool {
    slot.replace(filter).is_none()
}

fn accept_sequence_filter(
    slot: &mut Option<TocSequenceFilter>,
    value: &str,
    filter: fn(String) -> TocSequenceFilter,
) -> bool {
    let Some(value) = toc_sequence_identifier(value) else {
        return false;
    };
    if slot.replace(filter(value.to_string())).is_some() {
        return false;
    }
    true
}

fn toc_sequence_identifier(value: &str) -> Option<&str> {
    field_identifier_token(value)
}

fn parse_toc_style_specs(value: &str) -> Option<Vec<TocStyleSpec>> {
    let value = value.trim();
    let value = match (value.starts_with('"'), value.ends_with('"')) {
        (true, true) if value.len() >= 2 => &value[1..value.len() - 1],
        (true, _) | (_, true) => return None,
        (false, false) => value,
    };
    let parts: Vec<_> = value.split(',').map(str::trim).collect();
    if parts.is_empty() || parts.len() % 2 != 0 {
        return None;
    }
    let mut specs = Vec::new();
    for pair in parts.chunks_exact(2) {
        let name = pair[0];
        let level = pair[1];
        if name.is_empty() || name.starts_with('\\') || name.contains('"') || level.contains('"') {
            return None;
        }
        let level = level.parse::<u8>().ok()?;
        if !(1..=9).contains(&level) {
            return None;
        }
        specs.push(TocStyleSpec {
            name: name.to_string(),
            level,
        });
    }
    Some(specs)
}

fn is_toc_value_neutral_switch(part: &str) -> bool {
    part.eq_ignore_ascii_case("\\h")
        || part.eq_ignore_ascii_case("\\z")
        || part.eq_ignore_ascii_case("\\w")
        || part.eq_ignore_ascii_case("\\x")
}

fn strip_ascii_switch_prefix<'a>(part: &'a str, switch: &str) -> Option<&'a str> {
    let prefix = part.get(..switch.len())?;
    prefix
        .eq_ignore_ascii_case(switch)
        .then_some(&part[switch.len()..])
}

fn parse_toc_outline_range(range: &str) -> Option<(u8, u8)> {
    let range = field_name_token(range)?;
    let (start, end) = range.split_once('-')?;
    let start = start.parse::<u8>().ok()?;
    let end = end.parse::<u8>().ok()?;
    ((1..=9).contains(&start) && start <= end && end <= 9).then_some((start, end))
}

fn normalize_instruction(s: &str) -> String {
    instruction_parts(s).join(" ")
}

fn instruction_parts(s: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    for ch in s.chars() {
        if ch == '"' {
            in_quotes = !in_quotes;
            current.push(ch);
        } else if ch.is_whitespace() && !in_quotes {
            if !current.is_empty() {
                parts.push(std::mem::take(&mut current));
            }
        } else {
            current.push(ch);
        }
    }
    if !current.is_empty() {
        parts.push(current);
    }
    parts
}

fn field_kind(instruction: &str) -> FieldKind {
    FieldKind::from_instruction(instruction)
}

fn read_text(r: &mut Xml<'_>) -> String {
    let mut s = String::new();
    loop {
        match r.read_event() {
            Ok(Event::Text(t)) => match t.unescape().ok().map(|c| c.into_owned()) {
                Some(c) => s.push_str(&c),
                None => s.push_str(&String::from_utf8_lossy(t.into_inner().as_ref())),
            },
            Ok(Event::CData(t)) => s.push_str(&String::from_utf8_lossy(t.into_inner().as_ref())),
            Ok(Event::End(_)) | Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::{
        cardinal_page_number_text, computed_action_result, computed_ask_result,
        computed_display_result, computed_dynamic_result, computed_listnum_result,
        computed_numbering_result, computed_reference_index_result, computed_sequence_result,
        computed_set_result, computed_toc_entry_result, direct_bookmark_ref_instruction,
        document_info_instruction, format_page_number, note_ref_instruction,
        ordinal_page_number_text, page_ref_instruction, ref_instruction,
        seq_identifier_from_instruction, style_ref_instruction, table_formula_context, toc_entries,
        toc_spec, PageNumberFormat, TocEntrySource,
    };
    use crate::docx::styles::Styles;
    use std::collections::HashMap;

    #[test]
    fn page_number_text_formats_cover_compound_values() {
        assert_eq!(
            cardinal_page_number_text(342).as_deref(),
            Some("three hundred forty-two")
        );
        assert_eq!(
            ordinal_page_number_text(321).as_deref(),
            Some("three hundred twenty-first")
        );
        assert_eq!(
            format_page_number(21, Some(PageNumberFormat::OrdText)).as_deref(),
            Some("twenty-first")
        );
        assert_eq!(
            format_page_number(21, Some(PageNumberFormat::ArabicDash)).as_deref(),
            Some("- 21 -")
        );
        assert_eq!(
            format_page_number(4, Some(PageNumberFormat::DecimalZero)).as_deref(),
            Some("04")
        );
        assert_eq!(
            format_page_number(1_005, Some(PageNumberFormat::CardText)).as_deref(),
            Some("one thousand five")
        );
    }

    #[test]
    fn ask_default_result_populates_field_bookmark() {
        let mut field_bookmarks = HashMap::new();

        assert_eq!(
            computed_ask_result(
                r#"ASK ClientCode "Client code?" \d "ac-42" \o"#,
                &mut field_bookmarks,
            )
            .as_deref(),
            Some("")
        );
        assert_eq!(
            field_bookmarks.get("ClientCode").map(String::as_str),
            Some("ac-42")
        );
        assert_eq!(
            computed_dynamic_result(r#"ASK ClientCode "Client code?" \d "ac-42" \o"#),
            None
        );
    }

    #[test]
    fn ask_accepts_field_text_format_switches_without_formatting_bookmark() {
        let mut field_bookmarks = HashMap::new();

        assert_eq!(
            computed_ask_result(
                r#"ASK ClientCode "Client code?" \d "ac-42" \* Upper"#,
                &mut field_bookmarks,
            )
            .as_deref(),
            Some("")
        );
        assert_eq!(
            field_bookmarks.get("ClientCode").map(String::as_str),
            Some("ac-42")
        );
    }

    #[test]
    fn ask_bookmarks_reject_unbalanced_and_embedded_quotes() {
        let mut field_bookmarks = HashMap::new();

        assert_eq!(
            computed_ask_result(
                r#"ASK Client"Code" "Client code?" \d "ac-42""#,
                &mut field_bookmarks,
            ),
            None
        );
        assert_eq!(
            computed_ask_result(
                r#"ASK "ClientCode"x "Client code?" \d "ac-42""#,
                &mut field_bookmarks,
            ),
            None
        );
        assert!(field_bookmarks.is_empty());
    }

    #[test]
    fn quoted_field_literals_reject_embedded_quotes() {
        assert_eq!(
            computed_dynamic_result(r#"FILLIN "Client?" \d "Acme""#).as_deref(),
            Some("Acme")
        );
        assert_eq!(
            computed_action_result(r#"PRINT "page \p""#).as_deref(),
            Some("")
        );
        assert_eq!(
            computed_dynamic_result(r#"FILLIN "Cli"ent?" \d "Acme""#),
            None
        );
        assert_eq!(
            computed_dynamic_result(r#"FILLIN "Client?" \d "Ac"me""#),
            None
        );
        assert_eq!(computed_action_result(r#"PRINT "page "p""#), None);
    }

    #[test]
    fn print_accepts_field_text_format_switches() {
        assert_eq!(
            computed_action_result(r#"PRINT "page \p" \* Upper"#).as_deref(),
            Some("")
        );
        assert_eq!(
            computed_action_result(r#"PRINT \p ReportBox "0 0 moveto" \*Lower"#).as_deref(),
            Some("")
        );
    }

    #[test]
    fn merge_controls_accept_field_text_format_switches() {
        assert_eq!(
            computed_dynamic_result(r#"NEXT \* Upper"#).as_deref(),
            Some("")
        );
        assert_eq!(
            computed_dynamic_result(r#"NEXTIF 1 = 1 \* FirstCap"#).as_deref(),
            Some("")
        );
        assert_eq!(
            computed_dynamic_result(r#"SKIPIF 1 = 0 \*Lower"#).as_deref(),
            Some("")
        );
    }

    #[test]
    fn set_accepts_field_text_format_switches_without_formatting_bookmark() {
        let mut field_bookmarks = HashMap::new();

        assert_eq!(
            computed_set_result(
                r#"SET ClientName "Acme Launch" \* Upper"#,
                &mut field_bookmarks,
            )
            .as_deref(),
            Some("")
        );
        assert_eq!(
            field_bookmarks.get("ClientName").map(String::as_str),
            Some("Acme Launch")
        );
    }

    #[test]
    fn set_bookmarks_reject_quoted_switch_names() {
        let mut field_bookmarks = HashMap::new();

        assert_eq!(
            computed_set_result(r#"SET " \r" "Acme""#, &mut field_bookmarks),
            None
        );
        assert!(field_bookmarks.is_empty());
    }

    #[test]
    fn set_values_reject_unbalanced_quotes() {
        let mut field_bookmarks = HashMap::new();

        assert_eq!(
            computed_set_result(r#"SET ClientName "Acme"#, &mut field_bookmarks),
            None
        );
        assert!(field_bookmarks.is_empty());
    }

    #[test]
    fn document_info_names_reject_malformed_quotes() {
        assert!(document_info_instruction(r#"DOCPROPERTY "Client Name""#).is_some());
        assert!(document_info_instruction(r#"DOCPROPERTY "Client Name"#).is_none());
        assert!(document_info_instruction(r#"INFO "Title"#).is_none());
        assert!(document_info_instruction(r#"DOCVARIABLE Client"Code""#).is_none());
    }

    #[test]
    fn document_info_date_formats_reject_malformed_quotes() {
        assert!(document_info_instruction(r#"CREATEDATE \@ "yyyy-MM-dd""#).is_some());
        assert!(document_info_instruction(r#"CREATEDATE \@"yyyy-MM-dd""#).is_some());
        assert!(document_info_instruction(r#"CREATEDATE \@ yyyy-MM-dd"#).is_some());
        assert!(document_info_instruction(r#"CREATEDATE \@ "yyyy-MM-dd"#).is_none());
        assert!(document_info_instruction(r#"CREATEDATE \@ yyyy-MM-dd""#).is_none());
        assert!(document_info_instruction(r#"CREATEDATE \@"yyyy-MM-dd"#).is_none());
    }

    #[test]
    fn style_ref_names_reject_malformed_quotes() {
        assert!(style_ref_instruction(r#"STYLEREF "Heading 1""#).is_some());
        assert!(style_ref_instruction(r#"STYLEREF "Heading 1"#).is_none());
        assert!(style_ref_instruction(r#"STYLEREF Heading"1""#).is_none());
        assert!(style_ref_instruction(r#"STYLEREF "\Heading 1""#).is_none());
    }

    #[test]
    fn compact_autonum_separator_switches_are_case_insensitive() {
        let mut counter = 0;

        assert_eq!(
            computed_numbering_result(r#"AUTONUM \S":" "#, &mut counter).as_deref(),
            Some("1:")
        );
        assert_eq!(counter, 1);
    }

    #[test]
    fn autonum_separator_switches_reject_malformed_quotes() {
        let mut counter = 0;
        assert_eq!(
            computed_numbering_result(r#"AUTONUM \s ")""#, &mut counter).as_deref(),
            Some("1)")
        );

        let mut counter = 0;
        assert_eq!(
            computed_numbering_result(r#"AUTONUM \s ""#, &mut counter),
            None
        );
        assert_eq!(counter, 0);

        let mut counter = 0;
        assert_eq!(
            computed_numbering_result(r#"AUTONUM \s"""#, &mut counter),
            None
        );
        assert_eq!(counter, 0);
    }

    #[test]
    fn listnum_numeric_switches_reject_malformed_quotes() {
        let mut counter = 0;
        assert_eq!(
            computed_listnum_result(r#"LISTNUM NumberDefault \l "1""#, &mut counter).as_deref(),
            Some("1")
        );

        let mut counter = 0;
        assert_eq!(
            computed_listnum_result(r#"LISTNUM NumberDefault \l1"#, &mut counter).as_deref(),
            Some("1")
        );

        let mut counter = 0;
        assert_eq!(
            computed_listnum_result(r#"LISTNUM NumberDefault \s "4""#, &mut counter).as_deref(),
            Some("4")
        );

        let mut counter = 0;
        assert_eq!(
            computed_listnum_result(r#"LISTNUM NumberDefault \s4"#, &mut counter).as_deref(),
            Some("4")
        );

        let mut counter = 0;
        assert_eq!(
            computed_listnum_result(r#"LISTNUM NumberDefault \l "1"#, &mut counter),
            None
        );
        assert_eq!(counter, 0);

        let mut counter = 0;
        assert_eq!(
            computed_listnum_result(r#"LISTNUM NumberDefault \s 4""#, &mut counter),
            None
        );
        assert_eq!(counter, 0);
    }

    #[test]
    fn table_formula_scan_keeps_outer_field_across_nested_result_field() {
        let xml = r#"<w:document>
            <w:body>
                <w:tbl>
                    <w:tr>
                        <w:tc><w:p><w:r><w:t>5</w:t></w:r></w:p></w:tc>
                    </w:tr>
                    <w:tr>
                        <w:tc><w:p>
                            <w:r><w:fldChar w:fldCharType="begin"/></w:r>
                            <w:r><w:instrText>= SUM(ABOVE)</w:instrText></w:r>
                            <w:r><w:fldChar w:fldCharType="separate"/></w:r>
                            <w:r><w:fldChar w:fldCharType="begin"/></w:r>
                            <w:r><w:instrText>PAGE</w:instrText></w:r>
                            <w:r><w:fldChar w:fldCharType="separate"/></w:r>
                            <w:r><w:t>1</w:t></w:r>
                            <w:r><w:fldChar w:fldCharType="end"/></w:r>
                            <w:r><w:fldChar w:fldCharType="end"/></w:r>
                        </w:p></w:tc>
                    </w:tr>
                </w:tbl>
            </w:body>
        </w:document>"#;

        assert_eq!(
            table_formula_context(xml).field_result(0).as_deref(),
            Some("5")
        );
    }

    #[test]
    fn toc_sequence_scan_keeps_outer_field_across_nested_result_field() {
        let xml = r#"<w:document>
            <w:body>
                <w:p>
                    <w:r><w:t>Figure </w:t></w:r>
                    <w:r><w:fldChar w:fldCharType="begin"/></w:r>
                    <w:r><w:instrText>SEQ Figure</w:instrText></w:r>
                    <w:r><w:fldChar w:fldCharType="separate"/></w:r>
                    <w:r><w:fldChar w:fldCharType="begin"/></w:r>
                    <w:r><w:instrText>PAGE</w:instrText></w:r>
                    <w:r><w:fldChar w:fldCharType="separate"/></w:r>
                    <w:r><w:t>1</w:t></w:r>
                    <w:r><w:fldChar w:fldCharType="end"/></w:r>
                    <w:r><w:fldChar w:fldCharType="end"/></w:r>
                    <w:r><w:t>: Nested</w:t></w:r>
                </w:p>
            </w:body>
        </w:document>"#;

        let entries = toc_entries(xml, &Styles::default());
        let sequence = entries
            .iter()
            .find(|entry| entry.source == TocEntrySource::SequenceField)
            .expect("sequence entry");

        assert_eq!(sequence.sequence_identifier.as_deref(), Some("Figure"));
        assert_eq!(sequence.text, "Figure 1: Nested");
        assert_eq!(sequence.sequence_caption_text.as_deref(), Some("Nested"));
    }

    #[test]
    fn sequence_identifiers_reject_quoted_switch_names() {
        let mut counters = HashMap::new();

        assert_eq!(
            computed_sequence_result(r#"SEQ " \r""#, &mut counters),
            None
        );
        assert!(counters.is_empty());
        assert_eq!(seq_identifier_from_instruction(Some(r#"SEQ " \r""#)), None);
    }

    #[test]
    fn sequence_identifiers_reject_whitespace_names() {
        let mut counters = HashMap::new();

        assert_eq!(
            computed_sequence_result(r#"SEQ "Figure List""#, &mut counters),
            None
        );
        assert!(counters.is_empty());
        assert_eq!(
            seq_identifier_from_instruction(Some(r#"SEQ "Figure List""#)),
            None
        );
    }

    #[test]
    fn sequence_identifiers_reject_unbalanced_quotes() {
        let mut counters = HashMap::new();

        assert_eq!(
            computed_sequence_result(r##"SEQ "Figure"##, &mut counters),
            None
        );
        assert!(counters.is_empty());
        assert_eq!(
            seq_identifier_from_instruction(Some(r##"SEQ "Figure"##)),
            None
        );
    }

    #[test]
    fn sequence_identifiers_reject_unsupported_tail_switches() {
        assert_eq!(
            seq_identifier_from_instruction(Some(r#"SEQ Figure \x"#)),
            None
        );
        assert_eq!(
            seq_identifier_from_instruction(Some(r#"SEQ Figure \* roman"#)).as_deref(),
            Some("Figure")
        );
    }

    #[test]
    fn sequence_identifiers_reject_unsupported_negative_resets() {
        assert_eq!(
            seq_identifier_from_instruction(Some(r#"SEQ Figure \r -1"#)),
            None
        );
    }

    #[test]
    fn sequence_resets_reject_malformed_quotes() {
        let mut counters = HashMap::new();
        assert_eq!(
            computed_sequence_result(r#"SEQ Figure \r "7""#, &mut counters).as_deref(),
            Some("7")
        );

        let mut counters = HashMap::new();
        assert_eq!(
            computed_sequence_result(r#"SEQ Figure \r 7"#, &mut counters).as_deref(),
            Some("7")
        );

        let mut counters = HashMap::new();
        assert_eq!(
            computed_sequence_result(r#"SEQ Figure \r "7"#, &mut counters),
            None
        );
        assert!(counters.is_empty());

        let mut counters = HashMap::new();
        assert_eq!(
            computed_sequence_result(r#"SEQ Figure \r 7""#, &mut counters),
            None
        );
        assert!(counters.is_empty());
    }

    #[test]
    fn reference_targets_reject_quoted_switch_names() {
        assert!(ref_instruction(r#"REF " \p""#).is_none());
        assert!(direct_bookmark_ref_instruction(r#"" \p""#).is_none());
        assert!(page_ref_instruction(r#"PAGEREF " \p""#).is_none());
        assert!(note_ref_instruction(r#"NOTEREF " \p""#).is_none());
    }

    #[test]
    fn reference_targets_reject_whitespace_names() {
        assert!(ref_instruction(r#"REF "Figure List""#).is_none());
        assert!(direct_bookmark_ref_instruction(r#""Figure List""#).is_none());
        assert!(page_ref_instruction(r#"PAGEREF "Figure List""#).is_none());
        assert!(note_ref_instruction(r#"NOTEREF "Figure List""#).is_none());
    }

    #[test]
    fn reference_targets_reject_unbalanced_quotes() {
        assert!(ref_instruction(r#"REF "Figure1"#).is_none());
        assert!(direct_bookmark_ref_instruction(r#""Figure1"#).is_none());
        assert!(page_ref_instruction(r#"PAGEREF "Figure1"#).is_none());
        assert!(note_ref_instruction(r#"NOTEREF "Figure1"#).is_none());
    }

    #[test]
    fn ref_sequence_separators_accept_compact_values() {
        assert!(ref_instruction(r#"REF Figure1 \d-"#).is_some());
        assert!(direct_bookmark_ref_instruction(r#"Figure1 \d-"#).is_some());
        assert!(ref_instruction(r#"REF Figure1 \d\p"#).is_none());
    }

    #[test]
    fn ref_sequence_separators_reject_malformed_quotes() {
        assert!(ref_instruction(r#"REF Figure1 \d "-""#).is_some());
        assert!(direct_bookmark_ref_instruction(r#"Figure1 \d-"#).is_some());
        assert!(ref_instruction(r#"REF Figure1 \d "-"#).is_none());
        assert!(ref_instruction(r#"REF Figure1 \d -""#).is_none());
        assert!(direct_bookmark_ref_instruction(r#"Figure1 \d"-"#).is_none());
    }

    #[test]
    fn reference_index_categories_reject_malformed_quotes() {
        assert!(computed_reference_index_result(r#"TA \l "Case" \c "1""#).is_some());
        assert!(computed_reference_index_result(r#"TA \l "Case" \c "1"#).is_none());
        assert!(computed_reference_index_result(r#"TA \l "Case" \c"1"#).is_none());
    }

    #[test]
    fn reference_index_markers_accept_field_text_format_switches() {
        assert_eq!(
            computed_reference_index_result(r#"RD "chapter2.docx" \* Upper"#).as_deref(),
            Some("")
        );
        assert_eq!(
            computed_reference_index_result(r#"TA \l "Case" \*Lower"#).as_deref(),
            Some("")
        );
        assert_eq!(
            computed_reference_index_result(r#"XE "Mercury" \t "See planets" \* FirstCap"#)
                .as_deref(),
            Some("")
        );
    }

    #[test]
    fn toc_bookmark_targets_reject_empty_and_quoted_switch_names() {
        assert!(toc_spec(r#"TOC \b """#).is_none());
        assert!(toc_spec(r#"TOC \b " \o""#).is_none());
        assert!(toc_spec(r#"TOC \b" \o""#).is_none());
    }

    #[test]
    fn toc_bookmark_targets_reject_unbalanced_quotes() {
        assert!(toc_spec(r#"TOC \b "ChapterList"#).is_none());
        assert!(toc_spec(r#"TOC \b"ChapterList"#).is_none());
    }

    #[test]
    fn toc_type_identifiers_reject_quoted_switch_names() {
        assert_eq!(computed_toc_entry_result(r#"TC "Entry" \f " \l""#), None);
        assert!(toc_spec(r#"TOC \f " \l""#).is_none());
        assert!(toc_spec(r#"TOC \f" \l""#).is_none());
    }

    #[test]
    fn toc_entry_text_rejects_malformed_quotes() {
        assert!(computed_toc_entry_result(r#"TC "Entry""#).is_some());
        assert!(computed_toc_entry_result(r#"TC Entry"#).is_some());
        assert!(computed_toc_entry_result(r#"TC "Entry"#).is_none());
        assert!(computed_toc_entry_result(r#"TC Entry""#).is_none());
        assert!(computed_toc_entry_result(r#"TC En"try""#).is_none());
    }

    #[test]
    fn toc_sequence_filters_reject_invalid_identifiers() {
        assert!(toc_spec(r#"TOC \c "Figure List""#).is_none());
        assert!(toc_spec(r#"TOC \a " \o""#).is_none());
        assert!(toc_spec(r#"TOC \c" \o""#).is_none());
    }

    #[test]
    fn toc_page_number_sequence_prefixes_reject_invalid_identifiers() {
        assert!(toc_spec(r#"TOC \o "1-2" \s "\o""#).is_none());
        assert!(toc_spec(r#"TOC \o "1-2" \s"\o""#).is_none());
    }

    #[test]
    fn toc_numeric_levels_reject_malformed_quotes() {
        assert!(computed_toc_entry_result(r#"TC "Entry" \l "2"#).is_none());
        assert!(toc_spec(r#"TOC \o "1-2"#).is_none());
        assert!(toc_spec(r#"TOC \o "1-2" \l "2-3"#).is_none());
    }

    #[test]
    fn toc_separator_switches_reject_malformed_quotes() {
        assert!(toc_spec(r#"TOC \o "1-2" \p """#).is_some());
        assert!(toc_spec(r#"TOC \o "1-2" \p ""#).is_none());
        assert!(toc_spec(r#"TOC \o "1-2" \d"-"#).is_none());
    }

    #[test]
    fn toc_style_specs_reject_malformed_style_names() {
        assert!(toc_spec(r#"TOC \o "1-2" \t "Custom Heading,2""#).is_some());
        assert!(toc_spec(r#"TOC \o "1-2" \t "Custom Heading,2"#).is_none());
        assert!(toc_spec(r#"TOC \o "1-2" \t "Custom"Heading,2""#).is_none());
        assert!(toc_spec(r#"TOC \o "1-2" \t "\Bad,2""#).is_none());
    }

    #[test]
    fn compact_if_comparisons_ignore_operators_inside_quotes() {
        assert_eq!(
            computed_dynamic_result(r#"IF "A=B"="A=B" "yes" "no""#).as_deref(),
            Some("yes")
        );
        assert_eq!(
            computed_dynamic_result(r#"COMPARE "A>B"<>"A?B""#).as_deref(),
            Some("0")
        );
    }

    #[test]
    fn text_comparisons_accept_wildcards_on_either_operand() {
        assert_eq!(
            computed_dynamic_result(r#"COMPARE "A*" = "AB""#).as_deref(),
            Some("1")
        );
    }

    #[test]
    fn if_result_text_rejects_malformed_quotes() {
        assert_eq!(
            computed_dynamic_result(r#"IF 1 = 1 "yes" "no""#).as_deref(),
            Some("yes")
        );
        assert_eq!(
            computed_dynamic_result(r#"IF 1 = 1 yes no"#).as_deref(),
            Some("yes")
        );
        assert_eq!(computed_dynamic_result(r#"IF 1 = 1 "yes no"#), None);
        assert_eq!(computed_dynamic_result(r#"IF 1 = 0 yes "no"#), None);
    }

    #[test]
    fn quote_text_rejects_malformed_quotes() {
        assert_eq!(
            computed_dynamic_result(r#"QUOTE "literal text""#).as_deref(),
            Some("literal text")
        );
        assert_eq!(
            computed_dynamic_result(r#"QUOTE plain words"#).as_deref(),
            Some("plain words")
        );
        assert_eq!(computed_dynamic_result(r#"QUOTE "literal text"#), None);
        assert_eq!(computed_dynamic_result(r#"QUOTE literal text""#), None);
    }

    #[test]
    fn eq_script_segments_accept_multiple_visible_and_empty_options() {
        assert_eq!(
            computed_display_result(r#"EQ \s\up8(UB)\ai4()\do8(2)\di3()"#).as_deref(),
            Some("^{UB}_2")
        );
    }

    #[test]
    fn eq_enclosed_operands_reject_malformed_quotes() {
        assert_eq!(
            computed_display_result(r#"EQ \b("Chapter One")"#).as_deref(),
            Some("(Chapter One)")
        );
        assert!(computed_display_result(r#"EQ \b("A"B")"#).is_none());
        assert!(computed_display_result(r#"EQ \b("Chapter One)"#).is_none());
        assert!(computed_display_result(r#"EQ \x \to("Chapter One)"#).is_none());
    }

    #[test]
    fn eq_literal_operands_accept_escaped_closing_parentheses() {
        assert_eq!(
            computed_display_result(r#"EQ \f(A\),B)"#).as_deref(),
            Some("A)/B")
        );
    }

    #[test]
    fn eq_literal_operands_accept_escaped_semicolons() {
        assert_eq!(
            computed_display_result(r#"EQ \f(A\;B;C)"#).as_deref(),
            Some("A;B/C")
        );
    }

    #[test]
    fn advance_points_reject_malformed_quotes() {
        assert_eq!(
            computed_display_result(r#"ADVANCE \r "2""#).as_deref(),
            Some("")
        );
        assert_eq!(
            computed_display_result(r#"ADVANCE \r2"#).as_deref(),
            Some("")
        );
        assert!(computed_display_result(r#"ADVANCE \r "2"#).is_none());
        assert!(computed_display_result(r#"ADVANCE \r2""#).is_none());
    }

    #[test]
    fn advance_accepts_field_text_format_switches() {
        assert_eq!(
            computed_display_result(r#"ADVANCE \r 2 \* Upper"#).as_deref(),
            Some("")
        );
    }

    #[test]
    fn symbol_values_reject_malformed_quotes() {
        assert_eq!(
            computed_display_result(r#"SYMBOL "65""#).as_deref(),
            Some("A")
        );
        assert!(computed_display_result(r#"SYMBOL "65"#).is_none());
        assert_eq!(
            computed_display_result(r#"SYMBOL 65 \s "12""#).as_deref(),
            Some("A")
        );
        assert!(computed_display_result(r#"SYMBOL 65 \s "12"#).is_none());
        assert!(computed_display_result(r#"SYMBOL 183 \f "Symbol""#).is_some());
        assert!(computed_display_result(r#"SYMBOL 183 \f "Symbol"#).is_none());
    }

    #[test]
    fn symbol_values_accept_field_text_format_switches() {
        assert_eq!(
            computed_display_result(r#"SYMBOL 0x0063 \u \* Upper"#).as_deref(),
            Some("C")
        );
        assert_eq!(
            computed_display_result(r#"SYMBOL 0x0064 \u \*Lower"#).as_deref(),
            Some("d")
        );
    }

    #[test]
    fn action_targets_reject_quoted_backslash_names() {
        assert_eq!(
            computed_action_result(r#"GOTOBUTTON "\BadTarget" "Jump""#),
            None
        );
        assert_eq!(
            computed_action_result(r#"GOTOBUTTON " \BadTarget" "Jump""#),
            None
        );
    }

    #[test]
    fn action_targets_reject_whitespace_names() {
        assert_eq!(
            computed_action_result(r#"GOTOBUTTON "Bad Target" "Jump""#),
            None
        );
        assert_eq!(
            computed_action_result(r#"MACROBUTTON "Run Report" "Run""#),
            None
        );
    }

    #[test]
    fn action_targets_reject_embedded_quotes() {
        assert_eq!(
            computed_action_result(r#"GOTOBUTTON ""BadTarget"" "Jump""#),
            None
        );
        assert_eq!(
            computed_action_result(r#"MACROBUTTON ""RunReport"" "Run""#),
            None
        );
    }

    #[test]
    fn action_display_text_rejects_malformed_quotes() {
        assert_eq!(
            computed_action_result(r#"GOTOBUTTON TargetBookmark "Jump Now""#).as_deref(),
            Some("Jump Now")
        );
        assert_eq!(
            computed_action_result(r#"MACROBUTTON RunReport Run Now"#).as_deref(),
            Some("Run Now")
        );
        assert_eq!(
            computed_action_result(r#"GOTOBUTTON TargetBookmark "Jump Now"#),
            None
        );
        assert_eq!(
            computed_action_result(r#"MACROBUTTON RunReport Run Now""#),
            None
        );
    }

    #[test]
    fn print_groups_reject_quoted_backslash_names() {
        assert_eq!(
            computed_action_result(r#"PRINT \p " \BadGroup" "0 0 moveto""#),
            None
        );
        assert_eq!(
            computed_action_result(r#"PRINT \p" \BadGroup" "0 0 moveto""#),
            None
        );
    }

    #[test]
    fn print_groups_reject_embedded_quotes() {
        assert_eq!(
            computed_action_result(r#"PRINT \p ""BadGroup"" "0 0 moveto""#),
            None
        );
        assert_eq!(
            computed_action_result(r#"PRINT \p""BadGroup"" "0 0 moveto""#),
            None
        );
    }

    #[test]
    fn print_groups_reject_whitespace_names() {
        assert_eq!(
            computed_action_result(r#"PRINT \p "Bad Group" "0 0 moveto""#),
            None
        );
        assert_eq!(
            computed_action_result(r#"PRINT \p"Bad Group" "0 0 moveto""#),
            None
        );
    }
}
