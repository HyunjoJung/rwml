use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TocEntry {
    pub level: u8,
    pub text: String,
    pub(crate) source: TocEntrySource,
    pub(crate) tc_type: Option<String>,
    pub(crate) sequence_identifier: Option<String>,
    pub(crate) sequence_caption_text: Option<String>,
    pub(crate) bookmarks: Vec<String>,
    pub(crate) style_id: Option<String>,
    pub(crate) style_name: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TocEntrySource {
    HeadingStyle,
    OutlineLevel,
    StyledParagraph,
    TcField,
    SequenceField,
}

pub(crate) fn toc_entries(xml: &str, styles: &Styles) -> Vec<TocEntry> {
    let mut r = Reader::from_str(xml);
    let mut entries = Vec::new();
    let mut active_bookmarks = Vec::new();
    let mut sequence_counters = HashMap::new();
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
                        read_toc_paragraph(
                            &mut r,
                            styles,
                            &mut active_bookmarks,
                            &mut sequence_counters,
                            &mut entries,
                        );
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
    sequence_counters: &mut HashMap<String, i64>,
    entries: &mut Vec<TocEntry>,
) {
    let mut style_id: Option<String> = None;
    let mut outline: Option<u8> = None;
    let mut text = String::new();
    let mut bookmarks = active_bookmark_names(active_bookmarks);
    let mut current = Vec::new();
    let mut result_starts: Vec<Option<usize>> = Vec::new();
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
                        } else if push_computed_toc_sequence_result(
                            instruction.as_deref(),
                            sequence_counters,
                            &mut sequence_identifiers,
                            &mut text,
                        ) {
                            skip_element(r, b"fldSimple");
                            consumed_element = true;
                        } else {
                            text.push_str(&read_toc_simple_field_result(
                                r,
                                instruction.as_deref(),
                                active_bookmarks,
                                &mut bookmarks,
                                sequence_counters,
                                &mut sequence_identifiers,
                                entries,
                            ));
                            consumed_element = true;
                        }
                    }
                    b"fldChar" => {
                        apply_toc_fld_char(
                            &e,
                            &mut current,
                            &mut result_starts,
                            &bookmarks,
                            sequence_counters,
                            &mut sequence_identifiers,
                            &mut text,
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
                        let hidden_result = toc_source_hidden_field_result(&current);
                        if !hidden_result {
                            text.push_str(&run_text);
                        }
                    }
                    b"sym" => {
                        if !toc_source_hidden_field_result(&current) {
                            if let Some(ch) = toc_symbol_char(&e) {
                                text.push(ch);
                            }
                        }
                    }
                    b"tab" | b"br" | b"cr" => text.push(' '),
                    b"noBreakHyphen" => text.push('-'),
                    b"softHyphen" => text.push('\u{00ad}'),
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
                            && !push_computed_toc_sequence_result(
                                instruction.as_deref(),
                                sequence_counters,
                                &mut sequence_identifiers,
                                &mut text,
                            )
                        {
                            if let Some(computed) =
                                computed_toc_source_field_result(instruction.as_deref())
                            {
                                text.push_str(&computed);
                            }
                        }
                    }
                    b"fldChar" => {
                        apply_toc_fld_char(
                            &e,
                            &mut current,
                            &mut result_starts,
                            &bookmarks,
                            sequence_counters,
                            &mut sequence_identifiers,
                            &mut text,
                            entries,
                        );
                    }
                    b"sym" => {
                        if !toc_source_hidden_field_result(&current) {
                            if let Some(ch) = toc_symbol_char(&e) {
                                text.push(ch);
                            }
                        }
                    }
                    b"tab" | b"br" | b"cr" => text.push(' '),
                    b"noBreakHyphen" => text.push('-'),
                    b"softHyphen" => text.push('\u{00ad}'),
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

fn read_toc_simple_field_result(
    r: &mut Xml<'_>,
    instruction: Option<&str>,
    active_bookmarks: &mut Vec<(String, String)>,
    bookmarks: &mut Vec<String>,
    sequence_counters: &mut HashMap<String, i64>,
    sequence_identifiers: &mut Vec<String>,
    entries: &mut Vec<TocEntry>,
) -> String {
    let mut text = String::new();
    let mut current = Vec::new();
    let mut result_starts: Vec<Option<usize>> = Vec::new();
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
                    b"fldSimple" => {
                        let nested_instruction = attr_local(&e, b"instr");
                        if push_tc_entry_from_instruction(
                            nested_instruction.clone(),
                            bookmarks.as_slice(),
                            entries,
                        ) {
                            skip_element(r, b"fldSimple");
                        } else if push_computed_toc_sequence_result(
                            nested_instruction.as_deref(),
                            sequence_counters,
                            sequence_identifiers,
                            &mut text,
                        ) {
                            skip_element(r, b"fldSimple");
                        } else {
                            text.push_str(&read_toc_simple_field_result(
                                r,
                                nested_instruction.as_deref(),
                                active_bookmarks,
                                bookmarks,
                                sequence_counters,
                                sequence_identifiers,
                                entries,
                            ));
                        }
                        consumed_element = true;
                    }
                    b"fldChar" => {
                        apply_toc_fld_char(
                            &e,
                            &mut current,
                            &mut result_starts,
                            bookmarks,
                            sequence_counters,
                            sequence_identifiers,
                            &mut text,
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
                        if !toc_source_hidden_field_result(&current) {
                            text.push_str(&run_text);
                        }
                    }
                    b"sym" => {
                        if !toc_source_hidden_field_result(&current) {
                            if let Some(ch) = toc_symbol_char(&e) {
                                text.push(ch);
                            }
                        }
                    }
                    b"tab" | b"br" | b"cr" => text.push(' '),
                    b"noBreakHyphen" => text.push('-'),
                    b"softHyphen" => text.push('\u{00ad}'),
                    b"bookmarkStart" => {
                        if let Some(name) = push_active_bookmark(active_bookmarks, &e) {
                            push_unique(bookmarks, name);
                        }
                    }
                    b"bookmarkEnd" => remove_active_bookmark(active_bookmarks, &e),
                    _ => {
                        if let Some(marker) = inline_marker_text(&e) {
                            if !toc_source_hidden_field_result(&current) {
                                text.push_str(marker);
                            }
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
                    b"fldSimple" => {
                        let nested_instruction = attr_local(&e, b"instr");
                        if !push_tc_entry_from_instruction(
                            nested_instruction.clone(),
                            bookmarks.as_slice(),
                            entries,
                        ) && !push_computed_toc_sequence_result(
                            nested_instruction.as_deref(),
                            sequence_counters,
                            sequence_identifiers,
                            &mut text,
                        ) {
                            if let Some(computed) =
                                computed_toc_source_field_result(nested_instruction.as_deref())
                            {
                                text.push_str(&computed);
                            }
                        }
                    }
                    b"fldChar" => {
                        apply_toc_fld_char(
                            &e,
                            &mut current,
                            &mut result_starts,
                            bookmarks,
                            sequence_counters,
                            sequence_identifiers,
                            &mut text,
                            entries,
                        );
                    }
                    b"sym" => {
                        if !toc_source_hidden_field_result(&current) {
                            if let Some(ch) = toc_symbol_char(&e) {
                                text.push(ch);
                            }
                        }
                    }
                    b"tab" | b"br" | b"cr" => text.push(' '),
                    b"noBreakHyphen" => text.push('-'),
                    b"softHyphen" => text.push('\u{00ad}'),
                    b"bookmarkStart" => {
                        if let Some(name) = push_active_bookmark(active_bookmarks, &e) {
                            push_unique(bookmarks, name);
                        }
                    }
                    b"bookmarkEnd" => remove_active_bookmark(active_bookmarks, &e),
                    _ => {
                        if let Some(marker) = inline_marker_text(&e) {
                            if !toc_source_hidden_field_result(&current) {
                                text.push_str(marker);
                            }
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
    computed_toc_source_field_result(instruction).unwrap_or(text)
}

fn toc_source_hidden_field_result(current: &[ComplexField]) -> bool {
    current.iter().rev().any(|field| {
        field.phase == FieldPhase::Result
            && (toc_entry_field_syntax(&field.instruction).is_some()
                || seq_identifier_from_instruction(Some(&field.instruction)).is_some())
    })
}

fn toc_symbol_char(e: &BytesStart<'_>) -> Option<char> {
    let value = attr_local_trimmed(e, b"char")?;
    let font = attr_local_trimmed(e, b"font");
    computed_run_symbol_char(font.as_deref(), &value)
}

fn apply_toc_fld_char(
    e: &BytesStart<'_>,
    current: &mut Vec<ComplexField>,
    result_starts: &mut Vec<Option<usize>>,
    bookmarks: &[String],
    sequence_counters: &mut HashMap<String, i64>,
    sequence_identifiers: &mut Vec<String>,
    text: &mut String,
    entries: &mut Vec<TocEntry>,
) {
    // Track where each complex field's result text begins in `text`, so a
    // deterministic source-field result can overwrite the leaked cached runs on
    // `end` — mirroring the STYLEREF/REF/table-formula complex-field sinks.
    let fld_type = field_char_type(e);
    match fld_type.as_deref() {
        Some("begin") => result_starts.push(None),
        Some("separate") => {
            if let Some(start) = result_starts.last_mut() {
                *start = Some(text.len());
            }
        }
        _ => {}
    }
    let mut computed_source = None;
    apply_complex_field_scan_fld_char(e, current, |field| {
        if push_tc_entry(&field.instruction, bookmarks, entries)
            || push_computed_toc_sequence_result(
                Some(&field.instruction),
                sequence_counters,
                sequence_identifiers,
                text,
            )
        {
            return;
        }
        if let Some(identifier) = seq_identifier_from_instruction(Some(&field.instruction)) {
            push_unique(sequence_identifiers, identifier);
            return;
        }
        computed_source = computed_toc_source_field_result(Some(&field.instruction));
    });
    if fld_type.as_deref() == Some("end") {
        if let (Some(Some(start)), Some(computed)) = (result_starts.pop(), computed_source) {
            if start <= text.len() {
                text.replace_range(start.., &computed);
            }
        }
    }
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
    if let Some(spec) = toc_entry_field_syntax(instruction) {
        entries.push(TocEntry {
            level: spec.level,
            text: apply_field_text_format(spec.text, spec.text_format),
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
    toc_entry_field_syntax(instruction)?;
    Some(String::new())
}

pub(crate) fn supports_toc_entry_field_syntax(instruction: &str) -> bool {
    toc_entry_field_syntax(instruction).is_some()
}

fn is_tc_instruction(instruction: &str) -> bool {
    instruction_parts(instruction)
        .first()
        .is_some_and(|kind| kind.eq_ignore_ascii_case("TC"))
}

fn push_computed_toc_sequence_result(
    instruction: Option<&str>,
    sequence_counters: &mut HashMap<String, i64>,
    sequence_identifiers: &mut Vec<String>,
    text: &mut String,
) -> bool {
    let Some(instruction) = instruction else {
        return false;
    };
    let Some(identifier) = seq_identifier_from_instruction(Some(instruction)) else {
        return false;
    };
    let Some(result) = computed_sequence_result(instruction, sequence_counters) else {
        return false;
    };
    push_unique(sequence_identifiers, identifier);
    text.push_str(&result);
    true
}

fn computed_toc_source_field_result(instruction: Option<&str>) -> Option<String> {
    let instruction = normalize_instruction(instruction?);
    if is_tc_instruction(&instruction)
        || seq_identifier_from_instruction(Some(&instruction)).is_some()
    {
        return None;
    }
    let mut field_bookmarks = HashMap::new();
    match FieldKind::from_instruction(&instruction) {
        FieldKind::Dynamic(kind) if kind == "SET" => {
            return super::computed_set_result(&instruction, &mut field_bookmarks);
        }
        FieldKind::Dynamic(kind) if kind == "ASK" => {
            return super::computed_ask_result(&instruction, &mut field_bookmarks);
        }
        _ => {}
    }
    super::computed_quote_result(&instruction)
        .or_else(|| super::computed_fill_in_result(&instruction))
        .or_else(|| super::computed_if_result_with_bookmarks(&instruction, &field_bookmarks))
        .or_else(|| super::computed_compare_result_with_bookmarks(&instruction, &field_bookmarks))
        .or_else(|| {
            super::computed_merge_control_result_with_bookmarks(&instruction, &field_bookmarks)
        })
        .or_else(|| computed_display_result(&instruction))
        .or_else(|| computed_action_result(&instruction))
        .or_else(|| computed_reference_index_result(&instruction))
}

pub(crate) fn seq_identifier_from_instruction(instruction: Option<&str>) -> Option<String> {
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
    let (id, name) = bookmark_start(e)?;
    if !active_bookmarks
        .iter()
        .any(|(active_id, _)| active_id == &id)
    {
        active_bookmarks.push((id, name.clone()));
    }
    Some(name)
}

fn remove_active_bookmark(active_bookmarks: &mut Vec<(String, String)>, e: &BytesStart<'_>) {
    if let Some(id) = bookmark_end_id(e) {
        active_bookmarks.retain(|(active_id, _)| active_id != &id);
    }
}

fn read_toc_ppr(r: &mut Xml<'_>, style_id: &mut Option<String>, outline: &mut Option<u8>) {
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) if local(e.name().as_ref()) == b"pPrChange" => skip_subtree(r),
            Ok(Event::Start(e)) => match local(e.name().as_ref()) {
                b"pStyle" => *style_id = attr_local_trimmed(&e, b"val"),
                b"outlineLvl" => *outline = attr_u8(&e, b"val"),
                _ => {}
            },
            Ok(Event::Empty(e)) => match local(e.name().as_ref()) {
                b"pStyle" => *style_id = attr_local_trimmed(&e, b"val"),
                b"outlineLvl" => *outline = attr_u8(&e, b"val"),
                _ => {}
            },
            Ok(Event::End(e)) if local(e.name().as_ref()) == b"pPr" => break,
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
}

pub(crate) fn computed_toc_result(
    instruction: &str,
    toc_entries: &[TocEntry],
    bookmark_names: &HashSet<String>,
) -> Option<String> {
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
    if lines.is_empty() {
        if spec
            .bookmark
            .as_ref()
            .is_some_and(|bookmark| bookmark_names.contains(bookmark))
        {
            Some(apply_field_text_format(String::new(), spec.text_format))
        } else {
            None
        }
    } else {
        Some(apply_field_text_format(lines.join("\n"), spec.text_format))
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

pub(crate) fn toc_spec(instruction: &str) -> Option<TocSpec> {
    toc_field_syntax(instruction)
}
