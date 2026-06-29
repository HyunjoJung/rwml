use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;

use crate::annotation::FieldKind;

use super::super::xml_text::{read_text, skip_subtree};
use super::super::{attr_local, local};
use super::formula::{
    eval_formula_function, format_formula_number, formula_instruction, formula_number_text,
    FormulaNumberFormat, FormulaParser,
};
use super::{
    apply_complex_field_scan_fld_char, inline_marker_text, normalize_instruction,
    should_skip_alternate_branch, skip_element, AlternateContentBranchState, ComplexField,
    FieldPhase,
};

type Xml<'a> = Reader<&'a [u8]>;

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
                        has_span |= grid_span_exceeds_one(&e);
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
                        has_span |= grid_span_exceeds_one(&e);
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

fn grid_span_exceeds_one(e: &BytesStart<'_>) -> bool {
    attr_local(e, b"val").is_some_and(|value| value.trim() != "1")
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
    let mut parser = FormulaParser::new(&expression, None);
    let value = parser.parse()?;
    match spec.number_format {
        Some(FormulaNumberFormat::Picture(format)) => format_formula_number(value, &format),
        Some(FormulaNumberFormat::General(_)) => None,
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
        if let Some((end, target_row, target_col)) = table_formula_cell_reference_at(&chars, pos) {
            let value = table_formula_direct_cell_value(rows, row, col, target_row, target_col)?;
            output.push_str(&formula_number_text(value)?);
            pos = end;
            changed = true;
            continue;
        }
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

fn table_formula_cell_reference_at(chars: &[char], pos: usize) -> Option<(usize, usize, usize)> {
    if pos > 0 && chars.get(pos - 1) == Some(&':') {
        return None;
    }
    let mut end = pos;
    while chars
        .get(end)
        .is_some_and(|ch| ch.is_ascii_alphanumeric() || *ch == '_')
    {
        end += 1;
    }
    if end == pos || chars.get(end) == Some(&':') {
        return None;
    }
    let mut after = end;
    while chars.get(after).is_some_and(|ch| ch.is_whitespace()) {
        after += 1;
    }
    if chars.get(after) == Some(&'(') {
        return None;
    }
    let token: String = chars[pos..end].iter().collect();
    let (row, col) = table_formula_cell_reference(&token)?;
    Some((end, row, col))
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

fn table_formula_direct_cell_value(
    rows: &[Vec<TableFormulaCell>],
    row: usize,
    col: usize,
    target_row: usize,
    target_col: usize,
) -> Option<f64> {
    if row == target_row && col == target_col {
        return None;
    }
    let cell = rows.get(target_row)?.get(target_col)?;
    if cell.contains_formula {
        return None;
    }
    let value = cell.text.trim().parse::<f64>().ok()?;
    value.is_finite().then_some(value)
}
