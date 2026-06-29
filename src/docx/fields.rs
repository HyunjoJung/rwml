//! `.docx` field marker parsing.

use std::collections::{HashMap, HashSet};

use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;

use crate::annotation::{
    accept_field_number_format_switch,
    accept_field_text_format_switch as accept_field_format_switch, accept_general_format_switch,
    action_field_syntax, advance_field_syntax, compare_field_syntax, direct_ref_field_syntax,
    document_property_key, eq_enclosed_operand as take_eq_enclosed_operand,
    eq_fraction_operands as split_eq_fraction_operands, eq_list_operands as split_eq_list_operands,
    eq_numeric_prefix_option as consume_eq_numeric_prefix_option,
    eq_parenthesized_operand as take_eq_parenthesized_operand,
    eq_prefix_switch_tail as consume_eq_prefix_switch,
    eq_radical_operands as split_eq_radical_operands, field_identifier_token, field_literal_token,
    field_name_token, field_non_empty_literal_token, field_quoted_literal_token,
    filename_field_syntax, if_field_syntax, instruction_parts, is_neutral_field_format_switch,
    legacy_form_field_syntax, merge_control_field_syntax, note_ref_field_syntax,
    numbering_field_syntax, page_field_format_syntax_tail, page_ref_field_syntax,
    prompt_field_syntax, quote_field_syntax, ref_field_syntax, reference_index_category_token,
    reference_index_literal_token, reference_index_plain_value_token,
    revision_number_field_text_format, sequence_field_syntax, set_field_syntax,
    strip_ascii_switch_prefix, style_ref_field_syntax, symbol_field_syntax, toc_entry_field_syntax,
    toc_field_syntax, Field, FieldKind, FieldNumberFormat, FieldTextFormat, PromptFieldSyntax,
    StyleRefFieldSyntax, StyleRefResult, TocFieldSyntax as TocSpec, TocSequenceFilter,
    TocTcFilter as TcFilter,
};
use crate::model::{PageNumberFormat as ModelPageNumberFormat, SectionBreakKind};
use crate::{numfmt, CoreProperties};

use super::numbering::Numbering;
use super::styles::Styles;
use super::xml_text::{read_text, skip_subtree};
use super::{
    attr_local, attr_local_trimmed, attr_local_trimmed_preserve_empty, attr_u8, attr_usize,
    field_char_type, is_page_break_type, local, toggle_on,
};

type Xml<'a> = Reader<&'a [u8]>;

mod formula;
mod note_ref;
mod reference;
mod section;
mod style_ref;
mod table_formula;
mod toc;

#[cfg(test)]
use self::formula::computed_formula_result;
use self::formula::computed_formula_result_with_bookmarks;
pub(crate) use self::formula::supports_formula_field_syntax;
#[cfg(test)]
use self::note_ref::note_ref_instruction;
pub(crate) use self::note_ref::{
    computed_note_ref_result, note_ref_context, note_ref_target_names, NoteRefContext,
    NoteRefFieldPosition,
};
#[cfg(test)]
use self::reference::ref_instruction;
pub(crate) use self::reference::{
    computed_direct_bookmark_ref_result, computed_ref_result,
    is_direct_bookmark_ref_field_instruction, is_ref_position_field_instruction,
    ref_number_context, ref_position_context, ref_targets, RefFieldPosition, RefNumberContext,
    RefPositionContext, RefResultContext,
};
use self::reference::{
    computed_ref_instruction_result, direct_bookmark_ref_instruction, ref_instruction_target_known,
    ref_note_field_target, ref_numeric_paragraph_number, ref_paragraph_number,
    relative_context_ref_number,
};
pub(crate) use self::section::{
    computed_section_result, is_section_field_instruction, section_context, SectionContext,
};
#[cfg(test)]
use self::style_ref::style_ref_instruction;
#[allow(unused_imports)]
pub(crate) use self::style_ref::{
    computed_style_ref_result, is_style_ref_field_instruction, style_ref_context,
    supports_style_ref_field_syntax, StyleRefContext, StyleRefFieldPosition,
};
pub(crate) use self::table_formula::{table_formula_context, TableFormulaContext};
pub(crate) use self::toc::{
    computed_toc_entry_result, computed_toc_result, supports_toc_entry_field_syntax, toc_entries,
    TocEntry,
};
#[cfg(test)]
use self::toc::{seq_identifier_from_instruction, toc_spec, TocEntrySource};

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

pub(crate) fn parse(
    xml: &str,
    styles: &Styles,
    toc_entries: &[TocEntry],
    numbering: &Numbering,
    properties: FieldDocumentProperties<'_>,
    preserve_legacy_form_cache: bool,
) -> Vec<Field> {
    let bookmarks = ref_targets(xml);
    let ref_positions = ref_position_context(xml, numbering);
    let ref_numbers = ref_number_context(xml, numbering);
    let page_refs = page_ref_context(xml);
    let note_refs = note_ref_context(xml);
    let sections = section_context(xml);
    let style_refs = style_ref_context(xml, styles, numbering);
    let legacy_forms = legacy_form_context(xml, preserve_legacy_form_cache);
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
    match field_char_type(e).as_deref() {
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
    preserve_cached: bool,
}

impl LegacyFormContext {
    fn field_result(&self, index: usize) -> Option<String> {
        self.results.get(index).cloned().flatten()
    }

    fn preserves_cached(&self) -> bool {
        self.preserve_cached
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
    page_position: Option<PageRefPosition>,
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

impl From<SectionBreakKind> for PageRefSectionBreak {
    fn from(kind: SectionBreakKind) -> Self {
        match kind {
            SectionBreakKind::NextPage => PageRefSectionBreak::Next,
            SectionBreakKind::EvenPage => PageRefSectionBreak::Even,
            SectionBreakKind::OddPage => PageRefSectionBreak::Odd,
        }
    }
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
    display_only_restart_target: Option<PageRefTarget>,
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
            display_only_restart_target: None,
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
            self.display_only_restart_target = None;
        } else {
            if let Some(format) = page_number_format {
                self.rendered_display_format = format;
            }
            self.rendered_context_trusted = false;
            self.display_only_restart_target = page_number_start.map(|_| PageRefTarget {
                display_page: self.leading_display_page_number,
                display_format: self.leading_display_format,
            });
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
        self.display_only_restart_target = None;
        *source_order += 1;
    }

    fn advance_page_break_before(&mut self, source_order: &mut usize) {
        self.leading_page_number += 1;
        self.leading_display_page_number += 1;
        self.rendered_page_number += 1;
        self.rendered_display_page_number += 1;
        self.rendered_context_trusted = true;
        self.display_only_restart_target = None;
        *source_order += 1;
    }

    fn advance_last_rendered_page_break(&mut self, source_order: &mut usize) {
        self.rendered_page_number += 1;
        self.rendered_display_page_number += 1;
        self.rendered_context_trusted = true;
        self.display_only_restart_target = None;
        *source_order += 1;
    }

    fn note_visible_content(&mut self) {
        self.display_only_restart_target = None;
    }
}

pub(crate) fn legacy_form_context(xml: &str, preserve_cached: bool) -> LegacyFormContext {
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
                        let kind = field_char_type(&e);
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
                        field_char_type(&e).as_deref(),
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
    LegacyFormContext {
        results,
        preserve_cached,
    }
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
        b"checked" if in_checkbox => data.checkbox = Some(toggle_on(attr_local(e, b"val"))),
        b"default" if in_checkbox && data.checkbox.is_none() => {
            data.checkbox = Some(toggle_on(attr_local(e, b"val")));
        }
        b"default" if in_dropdown => data.dropdown_default = attr_usize(e, b"val"),
        b"default" if in_text_input => {
            data.text_default = attr_local(e, b"val");
        }
        b"result" if in_dropdown => data.dropdown_result = attr_usize(e, b"val"),
        b"listEntry" if in_dropdown => {
            if let Some(value) = attr_local(e, b"val") {
                data.dropdown_entries.push(value);
            }
        }
        _ => {}
    }
}

fn legacy_form_field_result(instruction: &str, data: &LegacyFormData) -> Option<String> {
    let spec = legacy_form_field_syntax(instruction)?;
    let text = match spec.kind.as_str() {
        "FORMCHECKBOX" => Some(
            if data.checkbox? {
                "\u{2612}"
            } else {
                "\u{2610}"
            }
            .to_string(),
        ),
        "FORMDROPDOWN" => {
            let result = data
                .dropdown_result
                .and_then(|index| data.dropdown_entries.get(index));
            let default = data
                .dropdown_default
                .and_then(|index| data.dropdown_entries.get(index));
            result.or(default).cloned()
        }
        "FORMTEXT" => data.text_default.clone(),
        _ => None,
    }?;
    Some(apply_field_text_format(text, spec.text_format))
}

pub(crate) fn computed_legacy_form_result(
    instruction: &str,
    current_result: &str,
    legacy_forms: &LegacyFormContext,
    field_index: usize,
) -> Option<String> {
    if legacy_forms.preserves_cached() {
        return None;
    }
    let spec = legacy_form_field_syntax(instruction)?;
    match spec.kind.as_str() {
        "FORMTEXT" if !current_result.is_empty() => Some(apply_field_text_format(
            current_result.to_string(),
            spec.text_format,
        )),
        _ => legacy_forms.field_result(field_index),
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
    let mut paragraph_depth = 0usize;
    let mut paragraph_properties_depth = 0usize;
    let mut section_properties_depth = 0usize;
    let mut section_type_seen = false;
    let mut section_is_paragraph_break = false;
    let mut section_break_pending = None;
    let mut paragraph_section_break_pending = None;
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
                    b"p" => paragraph_depth += 1,
                    b"pPr" => paragraph_properties_depth += 1,
                    b"pageBreakBefore"
                        if paragraph_properties_depth > 0 && page_ref_on_off_enabled(&e) =>
                    {
                        pages.advance_page_break_before(&mut source_order);
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
                        current_page_field_position(&pages, source_order),
                        &mut source_order,
                        &mut field_positions,
                        &mut page_field_positions,
                    ),
                    b"fldChar" => apply_page_ref_scan_fld_char(
                        &e,
                        current_page_ref_position(&pages, source_order),
                        current_page_field_position(&pages, source_order),
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
                    b"bookmarkStart" => record_page_ref_bookmark_start(
                        &e,
                        &pages,
                        saw_visible_content,
                        &mut source_order,
                        &mut targets,
                        &mut rendered_targets,
                        &mut target_positions,
                    ),
                    b"t" => {
                        let visible_text = !read_text(&mut r).is_empty();
                        consumed_element = true;
                        saw_visible_content |= visible_text;
                        if visible_text {
                            pages.note_visible_content();
                            source_order += 1;
                        }
                    }
                    b"br" if is_page_break_type(&e) => {
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
                    b"tab" | b"cr" | b"noBreakHyphen" | b"softHyphen" | b"drawing" | b"pict"
                    | b"object" => {
                        saw_visible_content = true;
                        pages.note_visible_content();
                        source_order += 1;
                    }
                    b"br" => {
                        saw_visible_content = true;
                        pages.note_visible_content();
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
                        pages.advance_page_break_before(&mut source_order);
                    }
                    b"sectPr" if paragraph_properties_depth > 0 => {
                        paragraph_section_break_pending =
                            Some((PageRefSectionBreak::Next, None, None));
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
                        current_page_field_position(&pages, source_order),
                        &mut source_order,
                        &mut field_positions,
                        &mut page_field_positions,
                    ),
                    b"fldChar" => apply_page_ref_scan_fld_char(
                        &e,
                        current_page_ref_position(&pages, source_order),
                        current_page_field_position(&pages, source_order),
                        &mut source_order,
                        &mut current,
                        &mut field_positions,
                        &mut page_field_positions,
                    ),
                    b"bookmarkStart" => record_page_ref_bookmark_start(
                        &e,
                        &pages,
                        saw_visible_content,
                        &mut source_order,
                        &mut targets,
                        &mut rendered_targets,
                        &mut target_positions,
                    ),
                    b"br" if is_page_break_type(&e) => {
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
                    b"tab" | b"br" | b"cr" | b"noBreakHyphen" | b"softHyphen" | b"drawing"
                    | b"pict" | b"object" => {
                        saw_visible_content = true;
                        pages.note_visible_content();
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
                                paragraph_section_break_pending = Some((
                                    section_break,
                                    section_page_number_start,
                                    section_page_number_format,
                                ));
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
                    b"p" => {
                        if paragraph_depth == 1 {
                            if let Some((section_break, page_number_start, page_number_format)) =
                                paragraph_section_break_pending.take()
                            {
                                pages.advance_section_break(
                                    section_break,
                                    saw_visible_content,
                                    saw_rendered_page_break,
                                    page_number_start,
                                    page_number_format,
                                    &mut source_order,
                                );
                            }
                        }
                        paragraph_depth = paragraph_depth.saturating_sub(1);
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
    toggle_on(attr_local(e, b"val"))
}

fn page_ref_section_break(e: &BytesStart<'_>) -> Option<PageRefSectionBreak> {
    match attr_local_trimmed_preserve_empty(e, b"val").as_deref() {
        None | Some("") | Some("nextPage") => Some(PageRefSectionBreak::Next),
        Some(value) => SectionBreakKind::from_wml_value(value).map(PageRefSectionBreak::from),
    }
}

fn page_ref_section_page_number_start(e: &BytesStart<'_>) -> Option<usize> {
    attr_usize(e, b"start").filter(|start| *start > 0)
}

fn page_ref_section_page_number_format(e: &BytesStart<'_>) -> Option<PageRefDisplayFormat> {
    let value = attr_local_trimmed_preserve_empty(e, b"fmt")?;
    if value.is_empty() {
        return Some(PageRefDisplayFormat::default());
    }
    Some(
        ModelPageNumberFormat::from_wml_value(&value)
            .map(|format| PageRefDisplayFormat::Known(Some(PageNumberFormat::from(format))))
            .unwrap_or(PageRefDisplayFormat::Unsupported),
    )
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

fn record_page_ref_bookmark_start(
    e: &BytesStart<'_>,
    pages: &PageRefPageState,
    saw_visible_content: bool,
    source_order: &mut usize,
    targets: &mut HashMap<String, PageRefTarget>,
    rendered_targets: &mut HashMap<String, PageRefPosition>,
    target_positions: &mut HashMap<String, PageRefPosition>,
) {
    let Some(name) = bookmark_name(e) else {
        return;
    };
    if pages.leading_page_number > 1 && !saw_visible_content {
        targets.entry(name.clone()).or_insert(PageRefTarget {
            display_page: pages.leading_display_page_number,
            display_format: pages.leading_display_format,
        });
        target_positions_insert(
            target_positions,
            name.clone(),
            PageRefPosition {
                physical_page: pages.leading_page_number,
                display_page: pages.leading_display_page_number,
                display_format: pages.leading_display_format,
                order: *source_order,
            },
        );
    }
    if pages.rendered_context_trusted {
        rendered_targets.entry(name).or_insert(PageRefPosition {
            physical_page: pages.rendered_page_number,
            display_page: pages.rendered_display_page_number,
            display_format: pages.rendered_display_format,
            order: *source_order,
        });
    } else if let Some(target) = pages.display_only_restart_target {
        targets.entry(name).or_insert(target);
    }
    *source_order += 1;
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

fn current_page_field_position(
    pages: &PageRefPageState,
    source_order: usize,
) -> Option<PageRefPosition> {
    current_page_ref_position(pages, source_order).or_else(|| {
        pages
            .display_only_restart_target
            .map(|target| PageRefPosition {
                physical_page: pages.leading_page_number,
                display_page: target.display_page,
                display_format: target.display_format,
                order: source_order,
            })
    })
}

fn record_page_ref_field_position(
    instruction: Option<&str>,
    page_ref_position: Option<PageRefPosition>,
    page_position: Option<PageRefPosition>,
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
            field_positions.push(page_ref_position);
            *source_order += 1;
        }
        Some(FieldKind::Page) => {
            page_field_positions.push(page_position);
            *source_order += 1;
        }
        _ => {}
    }
}

fn apply_page_ref_scan_fld_char(
    e: &BytesStart<'_>,
    page_ref_position: Option<PageRefPosition>,
    page_position: Option<PageRefPosition>,
    source_order: &mut usize,
    current: &mut Option<PageRefScanField>,
    field_positions: &mut Vec<Option<PageRefPosition>>,
    page_field_positions: &mut Vec<Option<PageRefPosition>>,
) {
    match field_char_type(e).as_deref() {
        Some("begin") => {
            *current = Some(PageRefScanField {
                instruction: String::new(),
                page_position,
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
                    page_ref_position,
                    field.page_position,
                    source_order,
                    field_positions,
                    page_field_positions,
                );
            }
        }
        _ => {}
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

fn bookmark_start(e: &BytesStart<'_>) -> Option<(String, String)> {
    Some((
        attr_local_trimmed(e, b"id")?,
        attr_local_trimmed(e, b"name")?,
    ))
}

fn bookmark_end_id(e: &BytesStart<'_>) -> Option<String> {
    attr_local_trimmed(e, b"id")
}

fn bookmark_name(e: &BytesStart<'_>) -> Option<String> {
    attr_local_trimmed(e, b"name")
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

fn push_unique(names: &mut Vec<String>, name: String) {
    if !names.iter().any(|existing| existing == &name) {
        names.push(name);
    }
}

fn normalize_toc_text(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn inline_marker_text(e: &BytesStart<'_>) -> Option<&'static str> {
    match local(e.name().as_ref()) {
        b"tab" => Some("\t"),
        b"br" => {
            if is_page_break_type(e) {
                Some("\u{000C}")
            } else {
                Some("\n")
            }
        }
        b"cr" => Some("\n"),
        b"noBreakHyphen" => Some("-"),
        b"softHyphen" => Some("\u{00ad}"),
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
                if is_section_field_instruction(&field.instruction) {
                    let position = ctx.sections.field_position(section_field_index);
                    section_field_index += 1;
                    computed_section_result(&field.instruction, position)
                } else {
                    None
                }
            }
            FieldKind::DocumentStructure(kind) if kind == "STYLEREF" => {
                let position = ctx.style_refs.field_position(style_ref_field_index);
                style_ref_field_index += 1;
                computed_style_ref_result(&field.instruction, ctx.style_refs, position)
            }
            FieldKind::Dynamic(kind) if kind == "=" => {
                let result = ctx.table_formulas.field_result(formula_field_index);
                formula_field_index += 1;
                result.or_else(|| {
                    computed_dynamic_result_with_bookmarks(&field.instruction, &field_bookmarks)
                })
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
                computed_dynamic_result_with_bookmarks(&field.instruction, &field_bookmarks)
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

pub(crate) fn supports_page_field_syntax(instruction: &str) -> bool {
    page_instruction(instruction).is_some()
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

#[cfg(test)]
pub(crate) fn computed_dynamic_result(instruction: &str) -> Option<String> {
    computed_formula_result(instruction)
        .or_else(|| computed_quote_result(instruction))
        .or_else(|| computed_fill_in_result(instruction))
        .or_else(|| computed_if_result(instruction))
        .or_else(|| computed_compare_result(instruction))
        .or_else(|| computed_merge_control_result(instruction))
}

pub(crate) fn computed_dynamic_result_with_bookmarks(
    instruction: &str,
    field_bookmarks: &HashMap<String, String>,
) -> Option<String> {
    computed_formula_result_with_bookmarks(instruction, Some(field_bookmarks))
        .or_else(|| computed_quote_result(instruction))
        .or_else(|| computed_fill_in_result(instruction))
        .or_else(|| computed_if_result_with_bookmarks(instruction, field_bookmarks))
        .or_else(|| computed_compare_result_with_bookmarks(instruction, field_bookmarks))
        .or_else(|| computed_merge_control_result_with_bookmarks(instruction, field_bookmarks))
}

#[cfg(test)]
fn computed_merge_control_result(instruction: &str) -> Option<String> {
    computed_merge_control_result_with_bookmarks(instruction, &HashMap::new())
}

fn computed_merge_control_result_with_bookmarks(
    instruction: &str,
    field_bookmarks: &HashMap<String, String>,
) -> Option<String> {
    merge_control_instruction(instruction, field_bookmarks)?;
    Some(String::new())
}

pub(crate) fn supports_filename_field_syntax(instruction: &str) -> bool {
    filename_field_syntax(instruction)
}

fn merge_control_instruction(
    instruction: &str,
    field_bookmarks: &HashMap<String, String>,
) -> Option<()> {
    let tokens = instruction_parts(instruction);
    let mut parts = tokens.iter().map(String::as_str);
    let kind = parts.next()?;
    if kind.eq_ignore_ascii_case("NEXT") {
        accept_field_format_tail(&mut parts)?;
        return Some(());
    }
    if kind.eq_ignore_ascii_case("NEXTIF") || kind.eq_ignore_ascii_case("SKIPIF") {
        let first = parts.next()?;
        let (left, operator, right) = comparison_operands(first, &mut parts, field_bookmarks)?;
        compare_if_operands(&left, operator, &right)?;
        accept_field_format_tail(&mut parts)?;
        return Some(());
    }
    None
}

pub(crate) fn supports_merge_control_field_syntax(instruction: &str) -> bool {
    merge_control_field_syntax(instruction)
}

fn field_format_tail<'a, I>(parts: &mut I) -> Option<Option<FieldTextFormat>>
where
    I: Iterator<Item = &'a str>,
{
    let mut text_format = None;
    while let Some(part) = parts.next() {
        if accept_field_format_switch_for_tail(part, parts, &mut text_format)? {
            continue;
        }
        return None;
    }
    Some(text_format)
}

fn accept_field_format_tail<'a, I>(parts: &mut I) -> Option<()>
where
    I: Iterator<Item = &'a str>,
{
    field_format_tail(parts).map(|_| ())
}

fn accept_field_format_switch_for_tail<'a>(
    part: &'a str,
    parts: &mut impl Iterator<Item = &'a str>,
    text_format: &mut Option<FieldTextFormat>,
) -> Option<bool> {
    accept_general_format_switch(part, parts, |format| {
        accept_field_format_switch(format, text_format)
    })
}

pub(super) fn accept_neutral_field_format_tail<'a, I>(parts: &mut I) -> Option<()>
where
    I: Iterator<Item = &'a str>,
{
    while let Some(part) = parts.next() {
        if accept_general_format_switch(part, parts, is_neutral_field_format_switch)? {
            continue;
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

pub(crate) fn supports_set_field_syntax(instruction: &str) -> bool {
    set_field_syntax(instruction)
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
        if accept_field_format_switch_for_tail(part, &mut parts, &mut text_format)? {
            continue;
        }
        if part.eq_ignore_ascii_case("\\@") {
            if date_format.is_some() {
                return None;
            }
            let format = field_non_empty_literal_token(parts.next()?)?;
            date_format = Some(format.to_string());
            continue;
        }
        if let Some(format) = strip_ascii_switch_prefix(part, "\\@") {
            if date_format.is_some() {
                return None;
            }
            let format = field_non_empty_literal_token(format)?;
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
            let value = field_quoted_literal_token(part)?.to_string();
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

pub(crate) fn computed_revision_number_result(
    instruction: &str,
    core_properties: &CoreProperties,
) -> Option<String> {
    let text_format = revision_number_field_text_format(instruction)?;
    let revision = core_properties.revision.clone()?;
    Some(apply_field_text_format(revision, text_format))
}

pub(crate) fn supports_revision_number_field_syntax(instruction: &str) -> bool {
    revision_number_field_text_format(instruction).is_some()
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

fn computed_quote_result(instruction: &str) -> Option<String> {
    let spec = quote_field_syntax(instruction)?;
    Some(apply_field_text_format(spec.text, spec.text_format))
}

pub(crate) fn supports_quote_field_syntax(instruction: &str) -> bool {
    quote_field_syntax(instruction).is_some()
}

fn computed_fill_in_result(instruction: &str) -> Option<String> {
    let spec = fill_in_instruction(instruction)?;
    let text = spec.default?;
    Some(apply_field_text_format(text, spec.text_format))
}

fn fill_in_instruction(instruction: &str) -> Option<FillInInstruction> {
    match prompt_field_syntax(instruction)? {
        PromptFieldSyntax::FillIn {
            default,
            text_format,
        } => Some(FillInInstruction {
            default,
            text_format,
        }),
        PromptFieldSyntax::Ask { .. } => None,
    }
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
    match prompt_field_syntax(instruction)? {
        PromptFieldSyntax::Ask { bookmark, default } => Some(AskInstruction { bookmark, default }),
        PromptFieldSyntax::FillIn { .. } => None,
    }
}

pub(super) fn quoted_literal_text(token: &str) -> Option<String> {
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

#[cfg(test)]
fn computed_if_result(instruction: &str) -> Option<String> {
    computed_if_result_with_bookmarks(instruction, &HashMap::new())
}

fn computed_if_result_with_bookmarks(
    instruction: &str,
    field_bookmarks: &HashMap<String, String>,
) -> Option<String> {
    let spec = if_instruction(instruction, field_bookmarks)?;
    let selected = if compare_if_operands(&spec.left, spec.operator, &spec.right)? {
        spec.true_text
    } else {
        spec.false_text
    };
    Some(apply_field_text_format(selected, spec.text_format))
}

fn if_instruction(
    instruction: &str,
    field_bookmarks: &HashMap<String, String>,
) -> Option<IfInstruction> {
    let tokens = instruction_parts(instruction);
    let mut parts = tokens.iter().map(String::as_str);
    let kind = parts.next()?;
    if !kind.eq_ignore_ascii_case("IF") {
        return None;
    }
    let first = parts.next()?;
    let (left, operator, right) = comparison_operands(first, &mut parts, field_bookmarks)?;
    let true_text = if_result_text(parts.next()?)?;
    let mut false_text = String::new();
    let mut text_format = None;
    if let Some(part) = parts.next() {
        if !accept_if_format_switch(part, &mut parts, &mut text_format)? {
            false_text = if_result_text(part)?;
        }
    }
    while let Some(part) = parts.next() {
        if !accept_if_format_switch(part, &mut parts, &mut text_format)? {
            return None;
        }
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
    if_field_syntax(instruction)
}

#[cfg(test)]
fn computed_compare_result(instruction: &str) -> Option<String> {
    computed_compare_result_with_bookmarks(instruction, &HashMap::new())
}

fn computed_compare_result_with_bookmarks(
    instruction: &str,
    field_bookmarks: &HashMap<String, String>,
) -> Option<String> {
    let spec = compare_instruction(instruction, field_bookmarks)?;
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
    compare_field_syntax(instruction)
}

fn compare_instruction(
    instruction: &str,
    field_bookmarks: &HashMap<String, String>,
) -> Option<IfInstruction> {
    let tokens = instruction_parts(instruction);
    let mut parts = tokens.iter().map(String::as_str);
    let kind = parts.next()?;
    if !kind.eq_ignore_ascii_case("COMPARE") {
        return None;
    }
    let first = parts.next()?;
    let (left, operator, right) = comparison_operands(first, &mut parts, field_bookmarks)?;
    let mut text_format = None;
    while let Some(part) = parts.next() {
        if !accept_if_format_switch(part, &mut parts, &mut text_format)? {
            return None;
        }
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
    field_bookmarks: &HashMap<String, String>,
) -> Option<(IfOperand, IfOperator, IfOperand)>
where
    I: Iterator<Item = &'a str>,
{
    if let Some(comparison) = compact_if_comparison(first, field_bookmarks) {
        Some(comparison)
    } else {
        Some((
            if_operand(first, field_bookmarks)?,
            if_operator(parts.next()?)?,
            if_operand(parts.next()?, field_bookmarks)?,
        ))
    }
}

fn if_operand(token: &str, field_bookmarks: &HashMap<String, String>) -> Option<IfOperand> {
    if let Some(text) = quoted_literal_text(token) {
        return Some(IfOperand::Text(text));
    }
    if let Some(value) = token.parse::<f64>().ok().filter(|value| value.is_finite()) {
        return Some(IfOperand::Number(value));
    }
    let name = field_name_token(token)?;
    field_bookmarks.get(name).cloned().map(IfOperand::Text)
}

fn compact_if_comparison(
    token: &str,
    field_bookmarks: &HashMap<String, String>,
) -> Option<(IfOperand, IfOperator, IfOperand)> {
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
            if_operand(left, field_bookmarks)?,
            if_operator(operator)?,
            if_operand(right, field_bookmarks)?,
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

pub(crate) fn supports_reference_index_marker_syntax(instruction: &str) -> bool {
    computed_reference_index_result(instruction).is_some()
}

fn reference_index_rd_instruction<'a>(mut parts: impl Iterator<Item = &'a str>) -> Option<()> {
    reference_index_literal_token(parts.next()?)?;
    let mut text_format = None;
    while let Some(part) = parts.next() {
        if part.eq_ignore_ascii_case("\\f") {
            continue;
        }
        if accept_reference_index_field_format(part, &mut parts, &mut text_format).is_some() {
            continue;
        }
        return None;
    }
    Some(())
}

fn reference_index_ta_instruction<'a>(mut parts: impl Iterator<Item = &'a str>) -> Option<()> {
    let mut has_entry_text = false;
    let mut text_format = None;
    while let Some(part) = parts.next() {
        if part.eq_ignore_ascii_case("\\l") || part.eq_ignore_ascii_case("\\s") {
            reference_index_literal_token(parts.next()?)?;
            has_entry_text = true;
            continue;
        }
        if let Some(value) = strip_ascii_switch_prefix(part, "\\l")
            .or_else(|| strip_ascii_switch_prefix(part, "\\s"))
        {
            if value.is_empty() {
                return None;
            }
            reference_index_literal_token(value)?;
            has_entry_text = true;
            continue;
        }
        if part.eq_ignore_ascii_case("\\c") {
            reference_index_category_token(parts.next()?)?;
            continue;
        }
        if let Some(category) = strip_ascii_switch_prefix(part, "\\c") {
            if category.is_empty() {
                return None;
            }
            reference_index_category_token(category)?;
            continue;
        }
        if accept_reference_index_field_format(part, &mut parts, &mut text_format).is_some() {
            continue;
        }
        return None;
    }
    has_entry_text.then_some(())
}

fn reference_index_xe_instruction<'a>(mut parts: impl Iterator<Item = &'a str>) -> Option<()> {
    reference_index_literal_token(parts.next()?)?;
    let mut text_format = None;
    while let Some(part) = parts.next() {
        if part.eq_ignore_ascii_case("\\b") || part.eq_ignore_ascii_case("\\i") {
            continue;
        }
        if part.eq_ignore_ascii_case("\\f") || part.eq_ignore_ascii_case("\\r") {
            reference_index_plain_value_token(parts.next()?)?;
            continue;
        }
        if let Some(value) = strip_ascii_switch_prefix(part, "\\f")
            .or_else(|| strip_ascii_switch_prefix(part, "\\r"))
        {
            if value.is_empty() {
                return None;
            }
            reference_index_plain_value_token(value)?;
            continue;
        }
        if part.eq_ignore_ascii_case("\\t") {
            reference_index_literal_token(parts.next()?)?;
            continue;
        }
        if let Some(value) = strip_ascii_switch_prefix(part, "\\t") {
            if value.is_empty() {
                return None;
            }
            reference_index_literal_token(value)?;
            continue;
        }
        if accept_reference_index_field_format(part, &mut parts, &mut text_format).is_some() {
            continue;
        }
        return None;
    }
    Some(())
}

fn accept_reference_index_field_format<'a>(
    part: &'a str,
    parts: &mut impl Iterator<Item = &'a str>,
    text_format: &mut Option<FieldTextFormat>,
) -> Option<()> {
    accept_field_format_switch_for_tail(part, parts, text_format)
        .and_then(|accepted| accepted.then_some(()))
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

pub(super) fn is_field_format_start(part: &str) -> bool {
    part == "\\*" || part.starts_with("\\*")
}

fn accept_if_format_switch<'a, I>(
    part: &'a str,
    parts: &mut I,
    text_format: &mut Option<FieldTextFormat>,
) -> Option<bool>
where
    I: Iterator<Item = &'a str>,
{
    accept_field_format_switch_for_tail(part, parts, text_format)
}

pub(crate) fn supports_action_field_syntax(instruction: &str) -> bool {
    action_field_syntax(instruction).is_some()
}

pub(crate) fn computed_action_result(instruction: &str) -> Option<String> {
    let spec = action_field_syntax(instruction)?;
    let text = spec.computed_text?;
    Some(apply_field_text_format(text, spec.text_format))
}

fn unquote_field_text(text: &str) -> String {
    let bytes = text.as_bytes();
    if bytes.len() >= 2 && bytes[0] == b'"' && *bytes.last().unwrap_or(&0) == b'"' {
        text[1..text.len() - 1].to_string()
    } else {
        text.to_string()
    }
}

pub(crate) fn computed_display_result(instruction: &str) -> Option<String> {
    computed_advance_result(instruction)
        .or_else(|| computed_eq_result(instruction))
        .or_else(|| computed_symbol_result(instruction))
}

pub(crate) fn supports_display_field_syntax(instruction: &str) -> bool {
    if computed_display_result(instruction).is_some() {
        return true;
    }
    if symbol_field_syntax(instruction).is_some() {
        return true;
    }
    let Some(spec) = eq_instruction(instruction) else {
        return false;
    };
    supports_eq_displace_syntax(&spec.expression) || supports_eq_script_syntax(&spec.expression)
}

fn computed_advance_result(instruction: &str) -> Option<String> {
    advance_field_syntax(instruction).then_some(())?;
    Some(String::new())
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
        if accept_field_format_switch_for_tail(part, &mut parts, &mut text_format)? {
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
        if let Some((_value, rest)) = consume_eq_numeric_prefix_option(body, "\\fo")
            .or_else(|| consume_eq_numeric_prefix_option(body, "\\ba"))
        {
            has_option = true;
            body = rest.trim_start();
        } else if let Some(rest) = consume_eq_prefix_switch(body, "\\li") {
            has_option = true;
            body = rest.trim_start();
        } else {
            break;
        }
    }
    if !has_option {
        return None;
    }
    let inner = take_eq_enclosed_operand(body)?;
    if inner.trim().is_empty() {
        Some(String::new())
    } else {
        eq_operand_text(inner)
    }
}

fn supports_eq_displace_syntax(expression: &str) -> bool {
    let Some(mut body) = strip_ascii_switch_prefix(expression, "\\d") else {
        return false;
    };
    body = body.trim_start();
    let mut has_option = false;
    loop {
        if let Some((_value, rest)) = consume_eq_numeric_prefix_option(body, "\\fo")
            .or_else(|| consume_eq_numeric_prefix_option(body, "\\ba"))
        {
            has_option = true;
            body = rest.trim_start();
        } else if let Some(rest) = consume_eq_prefix_switch(body, "\\li") {
            has_option = true;
            body = rest.trim_start();
        } else {
            break;
        }
    }
    let Some(inner) = take_eq_enclosed_operand(body) else {
        return false;
    };
    has_option && (inner.trim().is_empty() || eq_operand_text(inner).is_some())
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

fn supports_eq_script_syntax(expression: &str) -> bool {
    let mut body = expression.trim_start();
    loop {
        let Some(rest) = strip_ascii_switch_prefix(body, "\\s") else {
            return false;
        };
        let Some(remaining) = eq_script_syntax_segment(rest.trim_start()) else {
            return false;
        };
        body = remaining.trim_start();
        if body.is_empty() {
            return true;
        }
    }
}

fn eq_script_syntax_segment(mut body: &str) -> Option<&str> {
    let mut saw_option = false;
    loop {
        if body.is_empty() || consume_eq_prefix_switch(body, "\\s").is_some() {
            return saw_option.then_some(body);
        }
        if let Some(rest) = consume_eq_script_syntax_option(body, "\\up", false)
            .or_else(|| consume_eq_script_syntax_option(body, "\\do", false))
            .or_else(|| consume_eq_script_syntax_option(body, "\\ai", true))
            .or_else(|| consume_eq_script_syntax_option(body, "\\di", true))
        {
            body = rest.trim_start();
            saw_option = true;
            continue;
        }
        return None;
    }
}

fn consume_eq_script_syntax_option<'a>(
    value: &'a str,
    option: &str,
    allow_empty: bool,
) -> Option<&'a str> {
    let (_points, rest) = consume_eq_numeric_prefix_option(value, option)?;
    let (operand, rest) = take_eq_parenthesized_operand(rest)?;
    if operand.trim().is_empty() {
        return allow_empty.then_some(rest);
    }
    eq_operand_text(operand)?;
    Some(rest)
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

fn eq_column_count(value: f32) -> Option<usize> {
    (value.fract() == 0.0 && value >= 1.0 && value <= usize::MAX as f32).then_some(value as usize)
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
    let spec = symbol_field_syntax(instruction)?;
    let text = if spec.unicode {
        char::from_u32(spec.code)?.to_string()
    } else if symbol_font_matches(spec.font.as_deref(), "symbol") {
        symbol_font_char(spec.code)?.to_string()
    } else if symbol_font_matches(spec.font.as_deref(), "wingdings") {
        wingdings_font_char(spec.code)?.to_string()
    } else {
        ansi_char(spec.code)?.to_string()
    };
    Some(apply_field_text_format(text, spec.text_format))
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
        0x41 => '\u{270C}',
        0x4A => '\u{263A}',
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
    Hex,
    DollarText,
}

impl From<ModelPageNumberFormat> for PageNumberFormat {
    fn from(format: ModelPageNumberFormat) -> Self {
        match format {
            ModelPageNumberFormat::Decimal => PageNumberFormat::Arabic,
            ModelPageNumberFormat::DecimalZero => PageNumberFormat::DecimalZero,
            ModelPageNumberFormat::NumberInDash => PageNumberFormat::ArabicDash,
            ModelPageNumberFormat::DecimalFullWidth => PageNumberFormat::DecimalFullWidth,
            ModelPageNumberFormat::DecimalHalfWidth => PageNumberFormat::DecimalHalfWidth,
            ModelPageNumberFormat::DecimalFullWidth2 => PageNumberFormat::DecimalFullWidth2,
            ModelPageNumberFormat::DecimalEnclosedCircle => PageNumberFormat::DecimalEnclosedCircle,
            ModelPageNumberFormat::DecimalEnclosedFullstop => {
                PageNumberFormat::DecimalEnclosedFullstop
            }
            ModelPageNumberFormat::DecimalEnclosedParen => PageNumberFormat::DecimalEnclosedParen,
            ModelPageNumberFormat::Ganada => PageNumberFormat::Ganada,
            ModelPageNumberFormat::Chosung => PageNumberFormat::Chosung,
            ModelPageNumberFormat::KoreanDigital => PageNumberFormat::KoreanDigital,
            ModelPageNumberFormat::KoreanCounting => PageNumberFormat::KoreanCounting,
            ModelPageNumberFormat::KoreanLegal => PageNumberFormat::KoreanLegal,
            ModelPageNumberFormat::KoreanDigital2 => PageNumberFormat::KoreanDigital2,
            ModelPageNumberFormat::LowerLetter => PageNumberFormat::AlphabeticLower,
            ModelPageNumberFormat::UpperLetter => PageNumberFormat::AlphabeticUpper,
            ModelPageNumberFormat::LowerRoman => PageNumberFormat::RomanLower,
            ModelPageNumberFormat::UpperRoman => PageNumberFormat::RomanUpper,
            ModelPageNumberFormat::Ordinal => PageNumberFormat::Ordinal,
            ModelPageNumberFormat::CardinalText => PageNumberFormat::CardText,
            ModelPageNumberFormat::OrdinalText => PageNumberFormat::OrdText,
        }
    }
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
    text_format: Option<FieldTextFormat>,
}

fn sequence_instruction(instruction: &str) -> Option<SequenceInstruction> {
    sequence_instruction_with_reset_policy(instruction, false, false)
}

fn sequence_instruction_with_reset_policy(
    instruction: &str,
    allow_negative_reset: bool,
    allow_heading_reset: bool,
) -> Option<SequenceInstruction> {
    let tokens = instruction_parts(instruction);
    let mut parts = tokens.iter().map(String::as_str);
    let kind = parts.next()?;
    if !kind.eq_ignore_ascii_case("SEQ") {
        return None;
    }
    let identifier = field_identifier_token(parts.next()?)?.to_string();
    let mut action = SequenceAction::Next;
    let mut action_seen = false;
    let mut heading_reset_seen = false;
    let mut hidden = false;
    let mut number_format = None;
    let mut text_format = None;
    while let Some(part) = parts.next() {
        if accept_page_field_format_switch_for_tail(
            part,
            &mut parts,
            &mut number_format,
            &mut text_format,
        )? {
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
        if part.eq_ignore_ascii_case("\\s") {
            let level = parts.next()?;
            accept_sequence_heading_reset(level, &mut heading_reset_seen, allow_heading_reset)?;
            continue;
        }
        if let Some(level) = strip_ascii_switch_prefix(part, "\\s") {
            if level.is_empty() {
                return None;
            }
            accept_sequence_heading_reset(level, &mut heading_reset_seen, allow_heading_reset)?;
            continue;
        }
        if part.eq_ignore_ascii_case("\\r") {
            if action_seen {
                return None;
            }
            let reset = field_name_token(parts.next()?)?.parse::<i64>().ok()?;
            action_seen = true;
            action = SequenceAction::Reset(sequence_reset_value(reset, allow_negative_reset)?);
            continue;
        }
        if let Some(reset) = strip_ascii_switch_prefix(part, "\\r") {
            if reset.is_empty() || action_seen {
                return None;
            }
            action_seen = true;
            let reset = field_name_token(reset)?.parse::<i64>().ok()?;
            action = SequenceAction::Reset(sequence_reset_value(reset, allow_negative_reset)?);
            continue;
        }
        return None;
    }
    Some(SequenceInstruction {
        identifier,
        action,
        hidden,
        number_format,
        text_format,
    })
}

fn accept_sequence_heading_reset(
    value: &str,
    heading_reset_seen: &mut bool,
    allow_heading_reset: bool,
) -> Option<()> {
    if !allow_heading_reset || *heading_reset_seen {
        return None;
    }
    let level = field_name_token(value)?.parse::<u8>().ok()?;
    if !(1..=9).contains(&level) {
        return None;
    }
    *heading_reset_seen = true;
    Some(())
}

fn sequence_reset_value(value: i64, allow_negative: bool) -> Option<i64> {
    (allow_negative || value >= 0).then_some(value)
}

pub(crate) fn supports_sequence_field_syntax(instruction: &str) -> bool {
    sequence_field_syntax(instruction)
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
    Some(apply_field_text_format(text, instruction.text_format))
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
    let mut text_format = None;
    let mut separator = None;
    while let Some(part) = parts.next() {
        if accept_page_field_format_switch_for_tail(
            part,
            &mut parts,
            &mut number_format,
            &mut text_format,
        )? {
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
    Some(apply_field_text_format(text, text_format))
}

pub(crate) fn supports_numbering_field_syntax(instruction: &str) -> bool {
    numbering_field_syntax(instruction)
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
    let mut text_format = None;
    while let Some(part) = parts.next() {
        if accept_page_field_format_switch_for_tail(
            part,
            &mut parts,
            &mut number_format,
            &mut text_format,
        )? {
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
        if !list_name.eq_ignore_ascii_case("NumberDefault")
            && !list_name.eq_ignore_ascii_case("LegalDefault")
        {
            return None;
        }
        list_name_seen = true;
    }
    let value = reset_start.unwrap_or(*listnum_counter + 1);
    let text = format_sequence_number(value, number_format)?;
    *listnum_counter = value;
    Some(apply_field_text_format(text, text_format))
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
    let format = page_field_format_syntax_tail(&mut parts)?;
    Some(PageInstruction {
        number_format: format
            .number_format
            .map(page_number_format_from_field_format),
        text_format: format.text_format,
    })
}

fn page_ref_instruction(instruction: &str) -> Option<PageRefInstruction> {
    let syntax = page_ref_field_syntax(instruction)?;
    Some(PageRefInstruction {
        target: syntax.target,
        number_format: syntax
            .number_format
            .map(page_number_format_from_field_format),
        text_format: syntax.text_format,
        relative: syntax.relative,
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

fn accept_page_field_format_switch_for_tail<'a>(
    part: &'a str,
    parts: &mut impl Iterator<Item = &'a str>,
    number_format: &mut Option<PageNumberFormat>,
    text_format: &mut Option<FieldTextFormat>,
) -> Option<bool> {
    accept_general_format_switch(part, parts, |format| {
        accept_page_field_format_switch(format, number_format, text_format)
    })
}

fn accept_page_number_format_switch(
    part: &str,
    number_format: &mut Option<PageNumberFormat>,
) -> bool {
    let mut format = number_format.map(|_| FieldNumberFormat::Arabic);
    let accepted = accept_field_number_format_switch(part, &mut format);
    if accepted {
        *number_format = format.map(page_number_format_from_field_format);
    }
    accepted
}

fn page_number_format_from_field_format(format: FieldNumberFormat) -> PageNumberFormat {
    match format {
        FieldNumberFormat::Arabic => PageNumberFormat::Arabic,
        FieldNumberFormat::ArabicDash => PageNumberFormat::ArabicDash,
        FieldNumberFormat::AlphabeticLower => PageNumberFormat::AlphabeticLower,
        FieldNumberFormat::AlphabeticUpper => PageNumberFormat::AlphabeticUpper,
        FieldNumberFormat::RomanLower => PageNumberFormat::RomanLower,
        FieldNumberFormat::RomanUpper => PageNumberFormat::RomanUpper,
        FieldNumberFormat::Ordinal => PageNumberFormat::Ordinal,
        FieldNumberFormat::CardText => PageNumberFormat::CardText,
        FieldNumberFormat::OrdText => PageNumberFormat::OrdText,
        FieldNumberFormat::Hex => PageNumberFormat::Hex,
        FieldNumberFormat::DollarText => PageNumberFormat::DollarText,
    }
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
        PageNumberFormat::Hex => Some(format!("{page:X}")),
        PageNumberFormat::DollarText => dollar_page_number_text(page),
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

fn dollar_page_number_text(page: usize) -> Option<String> {
    Some(format!("{} and 00/100", cardinal_number_text(page)?))
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

fn apply_field_text_format(text: String, format: Option<FieldTextFormat>) -> String {
    match format {
        Some(FieldTextFormat::Upper) => text.to_uppercase(),
        Some(FieldTextFormat::Lower) => text.to_lowercase(),
        Some(FieldTextFormat::Caps) => capitalize_words(&text),
        Some(FieldTextFormat::FirstCap) => capitalize_first_word(&text),
        None => text,
    }
}

fn normalize_instruction(s: &str) -> String {
    instruction_parts(s).join(" ")
}

fn field_kind(instruction: &str) -> FieldKind {
    FieldKind::from_instruction(instruction)
}

#[cfg(test)]
mod tests {
    use super::{
        cardinal_page_number_text, computed_action_result, computed_ask_result,
        computed_display_result, computed_dynamic_result, computed_listnum_result,
        computed_numbering_result, computed_reference_index_result, computed_sequence_result,
        computed_set_result, computed_toc_entry_result, direct_bookmark_ref_instruction,
        document_info_instruction, format_page_number, note_ref_context, note_ref_instruction,
        ordinal_page_number_text, page_ref_context, page_ref_instruction, ref_instruction,
        ref_position_context, ref_targets, seq_identifier_from_instruction, style_ref_instruction,
        supports_action_field_syntax, supports_compare_field_syntax, supports_if_field_syntax,
        supports_merge_control_field_syntax, supports_prompt_field_syntax,
        supports_reference_index_marker_syntax, supports_toc_entry_field_syntax,
        table_formula_context, toc_entries, toc_spec, PageNumberFormat, TocEntrySource,
    };
    use crate::docx::numbering::Numbering;
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
    fn formula_numeric_pictures_use_common_neutral_format_tail() {
        assert_eq!(
            computed_dynamic_result(r#"= 10 / 4 \# "0.00" \* MERGEFORMAT"#).as_deref(),
            Some("2.50")
        );
        assert_eq!(
            computed_dynamic_result(r#"= 10 / 4 \#"0.0" \*CHARFORMAT"#).as_deref(),
            Some("2.5")
        );
        assert_eq!(
            computed_dynamic_result(r#"= 10 / 4 \# "0.00" \* MERGEFORMATINET"#).as_deref(),
            Some("2.50")
        );
        assert_eq!(
            computed_dynamic_result(r#"= 10 / 4 \# "0.00" \* Upper"#),
            None
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
    fn prompt_defaults_accept_unquoted_single_tokens() {
        assert_eq!(
            computed_dynamic_result(r#"FILLIN "Client?" \d Acme"#).as_deref(),
            Some("Acme")
        );
        assert_eq!(
            computed_dynamic_result(r#"FILLIN "Department?" \d ops \* Upper"#).as_deref(),
            Some("OPS")
        );

        let mut field_bookmarks = HashMap::new();
        assert_eq!(
            computed_ask_result(
                r#"ASK ClientCode "Client code?" \d ac-42"#,
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
    fn prompt_defaults_reject_switch_like_unquoted_tokens() {
        assert_eq!(computed_dynamic_result(r#"FILLIN "Client?" \d \o"#), None);
        assert!(!supports_prompt_field_syntax(r#"FILLIN "Client?" \d \o"#));

        let mut field_bookmarks = HashMap::new();
        assert_eq!(
            computed_ask_result(
                r#"ASK ClientCode "Client code?" \d \o"#,
                &mut field_bookmarks,
            ),
            None
        );
        assert!(field_bookmarks.is_empty());
    }

    #[test]
    fn prompt_text_accepts_unquoted_single_tokens() {
        assert_eq!(
            computed_dynamic_result(r#"FILLIN Client? \d Acme"#).as_deref(),
            Some("Acme")
        );

        let mut field_bookmarks = HashMap::new();
        assert_eq!(
            computed_ask_result(r#"ASK ClientCode Client? \d ac-42"#, &mut field_bookmarks)
                .as_deref(),
            Some("")
        );
        assert_eq!(
            field_bookmarks.get("ClientCode").map(String::as_str),
            Some("ac-42")
        );
    }

    #[test]
    fn prompt_text_rejects_switch_like_unquoted_tokens() {
        assert_eq!(computed_dynamic_result(r#"FILLIN \z \d Acme"#), None);
        assert!(!supports_prompt_field_syntax(r#"FILLIN \z \d Acme"#));

        let mut field_bookmarks = HashMap::new();
        assert_eq!(
            computed_ask_result(r#"ASK ClientCode \o \d ac-42"#, &mut field_bookmarks),
            None
        );
        assert!(field_bookmarks.is_empty());
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
        assert_eq!(
            computed_action_result(r#"PRINT \p ReportBox "\p literal code""#).as_deref(),
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
    fn table_formula_scan_trims_unit_gridspan() {
        let xml = r#"<w:document>
            <w:body>
                <w:tbl>
                    <w:tr>
                        <w:tc>
                            <w:tcPr><w:gridSpan w:val=" 1 "/></w:tcPr>
                            <w:p><w:r><w:t>5</w:t></w:r></w:p>
                        </w:tc>
                        <w:tc><w:p><w:fldSimple w:instr=" = SUM(LEFT) ">
                            <w:r><w:t>stale sum</w:t></w:r>
                        </w:fldSimple></w:p></w:tc>
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
    fn bookmark_scanners_trim_ids_and_names() {
        let xml = r#"<w:document>
            <w:body>
                <w:p><w:r><w:br w:type="page"/></w:r></w:p>
                <w:p>
                    <w:bookmarkStart w:id=" 7 " w:name=" Figure1 "/>
                    <w:r><w:t>Figure 1</w:t></w:r>
                    <w:bookmarkEnd w:id="7"/>
                </w:p>
                <w:p><w:r><w:t>Outside</w:t></w:r></w:p>
                <w:p>
                    <w:bookmarkStart w:id=" 8 " w:name=" ScopedToc "/>
                    <w:pPr><w:pStyle w:val="Heading1"/></w:pPr>
                    <w:r><w:t>Scoped Heading</w:t></w:r>
                    <w:bookmarkEnd w:id="8"/>
                </w:p>
                <w:p>
                    <w:bookmarkStart w:id=" 9 " w:name=" FootOne "/>
                    <w:r><w:footnoteReference w:id="1"/></w:r>
                    <w:bookmarkEnd w:id="9"/>
                </w:p>
            </w:body>
        </w:document>"#;

        assert_eq!(
            ref_targets(xml).get("Figure1").map(String::as_str),
            Some("Figure 1")
        );
        assert!(ref_position_context(xml, &Numbering::default())
            .target_position("Figure1")
            .is_some());
        assert!(page_ref_context(xml).target_position("Figure1").is_some());
        assert!(note_ref_context(xml).targets.contains_key("FootOne"));
        assert!(toc_entries(xml, &Styles::default()).iter().any(|entry| {
            entry.text == "Scoped Heading"
                && entry.bookmarks.len() == 1
                && entry.bookmarks[0] == "ScopedToc"
        }));
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
    fn reference_targets_reject_switch_first_names() {
        assert!(ref_instruction(r#"REF \h Figure1"#).is_none());
        assert!(direct_bookmark_ref_instruction(r#"\h Figure1"#).is_none());
        assert!(page_ref_instruction(r#"PAGEREF \p Figure1"#).is_none());
        assert!(note_ref_instruction(r#"NOTEREF \p FootOne"#).is_none());
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
    fn reference_index_syntax_support_matches_marker_forms() {
        assert!(supports_reference_index_marker_syntax(
            r#"RD "chapter2.docx" \* Upper"#
        ));
        assert!(supports_reference_index_marker_syntax(
            r#"TA \l "Case" \*Lower"#
        ));
        assert!(supports_reference_index_marker_syntax(
            r#"XE "Mercury" \t "See planets" \* FirstCap"#
        ));
        assert!(!supports_reference_index_marker_syntax(
            r#"TA \l "Case" \c"1"#
        ));
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
    fn toc_entry_syntax_support_matches_tc_forms() {
        assert!(supports_toc_entry_field_syntax(r#"TC "Entry" \f A \l 2"#));
        assert!(supports_toc_entry_field_syntax(r#"TC Entry \n"#));
        assert!(!supports_toc_entry_field_syntax(r#"TC "Entry" \l "2"#));
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
    fn if_and_compare_use_common_general_format_switches() {
        assert_eq!(
            computed_dynamic_result(r#"IF 1 = 1 "ship" "hold" \* Upper"#).as_deref(),
            Some("SHIP")
        );
        assert_eq!(
            computed_dynamic_result(r#"IF 1 = 1 "ship" \*Lower"#).as_deref(),
            Some("ship")
        );
        assert_eq!(
            computed_dynamic_result(r#"COMPARE "A*" = "AB" \* MERGEFORMAT"#).as_deref(),
            Some("1")
        );
    }

    #[test]
    fn compare_syntax_accepts_data_operands_without_computing_them() {
        assert!(supports_compare_field_syntax(
            r#"COMPARE CustomerTier = "Gold""#
        ));
        assert!(supports_compare_field_syntax(
            r#"COMPARE CustomerTier="Gold""#
        ));
        assert_eq!(
            computed_dynamic_result(r#"COMPARE CustomerTier = "Gold""#),
            None
        );
        assert!(!supports_compare_field_syntax(r#"COMPARE 1e309 > 0"#));
    }

    #[test]
    fn comparison_syntax_rejects_switch_like_unquoted_operands() {
        assert!(!supports_compare_field_syntax(r#"COMPARE \o = "Gold""#));
        assert!(!supports_if_field_syntax(r#"IF \o = "Gold" "ship" "hold""#));
        assert!(!supports_merge_control_field_syntax(
            r#"NEXTIF \o = "Gold""#
        ));
        assert!(supports_compare_field_syntax(r#"COMPARE "\o" = "Gold""#));
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
    fn eq_displacement_controls_preserve_literal_operand_text() {
        assert_eq!(
            computed_display_result(r#"EQ \d \fo10(A)"#).as_deref(),
            Some("A")
        );
        assert_eq!(
            computed_display_result(r#"EQ \d \ba2(\f(1,2))"#).as_deref(),
            Some("(1/2)")
        );
        assert_eq!(computed_display_result(r#"EQ \d(A)"#), None);
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
    fn action_syntax_support_matches_computed_action_forms() {
        assert!(supports_action_field_syntax(
            r#"GOTOBUTTON TargetBookmark "Jump Now""#
        ));
        assert!(supports_action_field_syntax(
            r#"PRINT \p ReportBox "0 0 moveto""#
        ));
        assert!(supports_action_field_syntax(
            r#"MACROBUTTON RunReport \* MERGEFORMAT"#
        ));
        assert_eq!(
            computed_action_result(r#"MACROBUTTON RunReport \* MERGEFORMAT"#),
            None
        );
        assert!(!supports_action_field_syntax(
            r#"MACROBUTTON RunReport Run \* Upper Again"#
        ));
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
