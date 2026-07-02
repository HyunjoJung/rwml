//! `.docx` tracked-change marker parsing.

use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;

use crate::annotation::{Revision, RevisionKind, RevisionView};
use crate::text;

use super::fields::{computed_quote_result, computed_run_symbol_char};
use super::xml_text::{
    inline_marker_text, read_text, skip_alternate_content_branch, skip_subtree,
    AlternateContentBranchState,
};
use super::{attr_local_trimmed, field_char_type, local};

type Xml<'a> = Reader<&'a [u8]>;

pub(crate) fn parse(xml: &str) -> Vec<Revision> {
    let mut r = Reader::from_str(xml);
    let mut revisions = Vec::new();
    let mut alternate_content_stack = Vec::new();
    loop {
        match r.read_event() {
            Ok(Event::Start(e))
                if skip_alternate_content_branch(
                    &mut alternate_content_stack,
                    local(e.name().as_ref()),
                ) =>
            {
                skip_subtree(&mut r);
            }
            Ok(Event::Empty(e))
                if skip_alternate_content_branch(
                    &mut alternate_content_stack,
                    local(e.name().as_ref()),
                ) => {}
            Ok(Event::Start(e)) if local(e.name().as_ref()) == b"AlternateContent" => {
                alternate_content_stack.push(AlternateContentBranchState::default());
            }
            Ok(Event::Start(e)) => {
                if let Some(kind) = revision_kind(local(e.name().as_ref())) {
                    revisions.push(read_revision(&mut r, &e, kind));
                }
            }
            Ok(Event::Empty(e)) => {
                if let Some(kind) = revision_kind(local(e.name().as_ref())) {
                    revisions.push(revision_shell(&e, kind, String::new()));
                }
            }
            Ok(Event::End(e)) if local(e.name().as_ref()) == b"AlternateContent" => {
                alternate_content_stack.pop();
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    revisions
}

pub(crate) fn main_text_with_view(xml: &str, view: RevisionView) -> String {
    let mut r = Reader::from_str(xml);
    let mut out = String::new();
    let mut alternate_content_stack = Vec::new();
    let mut inline_continuation = false;
    loop {
        match r.read_event() {
            Ok(Event::Start(e))
                if skip_alternate_content_branch(
                    &mut alternate_content_stack,
                    local(e.name().as_ref()),
                ) =>
            {
                skip_subtree(&mut r);
            }
            Ok(Event::Empty(e))
                if skip_alternate_content_branch(
                    &mut alternate_content_stack,
                    local(e.name().as_ref()),
                ) => {}
            Ok(Event::Start(e)) if local(e.name().as_ref()) == b"AlternateContent" => {
                alternate_content_stack.push(AlternateContentBranchState::default());
            }
            Ok(Event::Start(e)) => {
                if let Some(kind) = revision_kind(local(e.name().as_ref())) {
                    let rev_text = read_revision_text(&mut r, local(e.name().as_ref()));
                    push_revision_text(&mut out, view, kind, &rev_text);
                    inline_continuation = false;
                } else if matches!(local(e.name().as_ref()), b"t" | b"delText") {
                    push_text_segment(&mut out, &read_text(&mut r), &mut inline_continuation);
                } else if let Some(marker) = inline_marker_text(&e) {
                    push_inline_segment(&mut out, marker, &mut inline_continuation);
                    skip_subtree(&mut r);
                } else if local(e.name().as_ref()) == b"sym" {
                    if let Some(ch) = revision_symbol_char(&e) {
                        let text = ch.to_string();
                        push_segment(&mut out, &text);
                        inline_continuation = false;
                    }
                    skip_subtree(&mut r);
                }
            }
            Ok(Event::Empty(e)) => {
                if let Some(kind) = revision_kind(local(e.name().as_ref())) {
                    push_revision_text(&mut out, view, kind, "");
                    inline_continuation = false;
                } else if let Some(marker) = inline_marker_text(&e) {
                    push_inline_segment(&mut out, marker, &mut inline_continuation);
                } else if let Some(ch) = revision_symbol_char(&e) {
                    let text = ch.to_string();
                    push_segment(&mut out, &text);
                    inline_continuation = false;
                }
            }
            Ok(Event::End(e)) if local(e.name().as_ref()) == b"AlternateContent" => {
                alternate_content_stack.pop();
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    text::finalize(&out)
}

fn revision_kind(name: &[u8]) -> Option<RevisionKind> {
    match name {
        b"ins" => Some(RevisionKind::Insertion),
        b"del" => Some(RevisionKind::Deletion),
        b"moveFrom" => Some(RevisionKind::MoveFrom),
        b"moveTo" => Some(RevisionKind::MoveTo),
        _ => None,
    }
}

fn revision_shell(e: &BytesStart<'_>, kind: RevisionKind, text: String) -> Revision {
    Revision {
        kind,
        id: attr_local_trimmed(e, b"id"),
        author: attr_local_trimmed(e, b"author"),
        date: attr_local_trimmed(e, b"date"),
        text,
    }
}

fn read_revision(r: &mut Xml<'_>, start: &BytesStart<'_>, kind: RevisionKind) -> Revision {
    let end_name = local(start.name().as_ref()).to_vec();
    let text = read_revision_text(r, &end_name);
    revision_shell(start, kind, text)
}

fn read_revision_text(r: &mut Xml<'_>, end_name: &[u8]) -> String {
    let mut depth = 1usize;
    let mut text = String::new();
    let mut complex_field = RevisionComplexField::default();
    let mut embedded_body_depth = 0usize;
    let mut alternate_content_stack = Vec::new();
    loop {
        match r.read_event() {
            Ok(Event::Start(e))
                if skip_alternate_content_branch(
                    &mut alternate_content_stack,
                    local(e.name().as_ref()),
                ) =>
            {
                skip_subtree(r);
            }
            Ok(Event::Empty(e))
                if skip_alternate_content_branch(
                    &mut alternate_content_stack,
                    local(e.name().as_ref()),
                ) => {}
            Ok(Event::Start(e)) if local(e.name().as_ref()) == b"AlternateContent" => {
                alternate_content_stack.push(AlternateContentBranchState::default());
            }
            Ok(Event::Start(e)) if local(e.name().as_ref()) == end_name => {
                depth = depth.saturating_add(1);
            }
            Ok(Event::Start(e)) if is_revision_embedded_body(local(e.name().as_ref())) => {
                embedded_body_depth += 1;
            }
            Ok(Event::Start(e)) if local(e.name().as_ref()) == b"fldChar" => {
                if let Some(computed) = complex_field.apply_field_char(&e) {
                    text.push_str(&computed);
                }
                skip_subtree(r);
            }
            Ok(Event::Empty(e)) if local(e.name().as_ref()) == b"fldChar" => {
                if let Some(computed) = complex_field.apply_field_char(&e) {
                    text.push_str(&computed);
                }
            }
            Ok(Event::Start(e)) if local(e.name().as_ref()) == b"instrText" => {
                let instruction = read_text(r);
                complex_field.append_instruction_text(&instruction);
            }
            Ok(Event::Start(e)) if local(e.name().as_ref()) == b"p" && embedded_body_depth == 0 => {
                push_revision_paragraph_boundary(&mut text);
            }
            Ok(Event::Start(e)) if local(e.name().as_ref()) == b"fldSimple" => {
                if let Some(computed) = computed_revision_simple_field_text(&e) {
                    text.push_str(&computed);
                    skip_subtree(r);
                }
            }
            Ok(Event::Empty(e)) if local(e.name().as_ref()) == b"fldSimple" => {
                if let Some(computed) = computed_revision_simple_field_text(&e) {
                    text.push_str(&computed);
                }
            }
            Ok(Event::Start(e)) if local(e.name().as_ref()) == b"t" => {
                let value = read_text(r);
                if !complex_field.suppresses_result() {
                    text.push_str(&value);
                }
            }
            Ok(Event::Start(e)) if local(e.name().as_ref()) == b"delText" => {
                let value = read_text(r);
                if !complex_field.suppresses_result() {
                    text.push_str(&value);
                }
            }
            Ok(Event::Start(e)) if local(e.name().as_ref()) == b"sym" => {
                if !complex_field.suppresses_result() {
                    if let Some(ch) = revision_symbol_char(&e) {
                        text.push(ch);
                    }
                }
                skip_subtree(r);
            }
            Ok(Event::Start(e)) => {
                if !complex_field.suppresses_result() {
                    if let Some(marker) = inline_marker_text(&e) {
                        text.push_str(marker);
                        skip_subtree(r);
                    }
                }
            }
            Ok(Event::Empty(e)) => {
                if !complex_field.suppresses_result() {
                    if let Some(marker) = inline_marker_text(&e) {
                        text.push_str(marker);
                    } else if let Some(ch) = revision_symbol_char(&e) {
                        text.push(ch);
                    }
                }
            }
            Ok(Event::End(e)) if local(e.name().as_ref()) == end_name => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    break;
                }
            }
            Ok(Event::End(e)) if is_revision_embedded_body(local(e.name().as_ref())) => {
                embedded_body_depth = embedded_body_depth.saturating_sub(1);
            }
            Ok(Event::End(e)) if local(e.name().as_ref()) == b"AlternateContent" => {
                alternate_content_stack.pop();
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    text
}

fn is_revision_embedded_body(name: &[u8]) -> bool {
    matches!(name, b"drawing" | b"pict" | b"object" | b"txbxContent")
}

fn push_revision_paragraph_boundary(text: &mut String) {
    if !text.is_empty() {
        text.push('\n');
    }
}

fn revision_symbol_char(e: &BytesStart<'_>) -> Option<char> {
    let value = attr_local_trimmed(e, b"char")?;
    let font = attr_local_trimmed(e, b"font");
    computed_run_symbol_char(font.as_deref(), &value)
}

fn computed_revision_simple_field_text(e: &BytesStart<'_>) -> Option<String> {
    let instruction = attr_local_trimmed(e, b"instr")?;
    computed_quote_result(&instruction)
}

#[derive(Default)]
struct RevisionComplexField {
    depth: usize,
    instruction: String,
    phase: Option<RevisionComplexFieldPhase>,
    computed_result: Option<String>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RevisionComplexFieldPhase {
    Instruction,
    Result,
}

impl RevisionComplexField {
    fn apply_field_char(&mut self, e: &BytesStart<'_>) -> Option<String> {
        match field_char_type(e).as_deref() {
            Some("begin") => {
                if self.depth == 0 {
                    self.instruction.clear();
                    self.phase = Some(RevisionComplexFieldPhase::Instruction);
                    self.computed_result = None;
                }
                self.depth += 1;
                None
            }
            Some("separate")
                if self.depth == 1
                    && self.phase == Some(RevisionComplexFieldPhase::Instruction) =>
            {
                self.phase = Some(RevisionComplexFieldPhase::Result);
                self.computed_result = computed_quote_result(&self.instruction);
                self.computed_result.clone()
            }
            Some("end") => {
                if self.depth > 0 {
                    self.depth -= 1;
                    if self.depth == 0 {
                        self.instruction.clear();
                        self.phase = None;
                        self.computed_result = None;
                    }
                }
                None
            }
            _ => None,
        }
    }

    fn append_instruction_text(&mut self, text: &str) {
        if self.depth == 1 && self.phase == Some(RevisionComplexFieldPhase::Instruction) {
            self.instruction.push_str(text);
        }
    }

    fn suppresses_result(&self) -> bool {
        self.depth > 0
            && self.phase == Some(RevisionComplexFieldPhase::Result)
            && self.computed_result.is_some()
    }
}

fn push_revision_text(out: &mut String, view: RevisionView, kind: RevisionKind, value: &str) {
    match (view, kind) {
        (RevisionView::Accepted, RevisionKind::Insertion | RevisionKind::MoveTo)
        | (RevisionView::Original, RevisionKind::Deletion | RevisionKind::MoveFrom) => {
            push_segment(out, value);
        }
        (RevisionView::Annotated, RevisionKind::Insertion) => {
            push_marker(out, "[+", value, "]");
        }
        (RevisionView::Annotated, RevisionKind::Deletion) => {
            push_marker(out, "[-", value, "]");
        }
        (RevisionView::Annotated, RevisionKind::MoveFrom) => {
            push_marker(out, "[~", value, "->]");
        }
        (RevisionView::Annotated, RevisionKind::MoveTo) => {
            push_marker(out, "[~->", value, "]");
        }
        _ => {}
    }
}

fn push_marker(out: &mut String, prefix: &str, value: &str, suffix: &str) {
    if !out.is_empty() {
        out.push(' ');
    }
    out.push_str(prefix);
    out.push_str(value);
    out.push_str(suffix);
}

fn push_text_segment(out: &mut String, value: &str, inline_continuation: &mut bool) {
    if value.is_empty() {
        return;
    }
    if !out.is_empty() && !*inline_continuation {
        out.push(' ');
    }
    out.push_str(value);
    *inline_continuation = false;
}

fn push_inline_segment(out: &mut String, value: &str, inline_continuation: &mut bool) {
    if value.is_empty() {
        return;
    }
    out.push_str(value);
    *inline_continuation = true;
}

fn push_segment(out: &mut String, value: &str) {
    if value.is_empty() {
        return;
    }
    if !out.is_empty() {
        out.push(' ');
    }
    out.push_str(value);
}
