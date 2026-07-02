//! `.docx` comments part parsing.

use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;

use std::collections::HashMap;

use crate::annotation::{Comment, TextAnchor};

use super::fields::{computed_dynamic_result_with_bookmarks, computed_run_symbol_char};
use super::xml_text::{
    inline_marker_text, read_text, skip_alternate_content_branch, skip_subtree,
    AlternateContentBranchState,
};
use super::{attr_local_trimmed, field_char_type, local};

type Xml<'a> = Reader<&'a [u8]>;

pub(crate) fn parse(xml: &str) -> Vec<Comment> {
    let mut r = Reader::from_str(xml);
    let mut comments = Vec::new();
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
            Ok(Event::Start(e)) if local(e.name().as_ref()) == b"comment" => {
                if let Some(comment) = read_comment(&mut r, &e) {
                    comments.push(comment);
                }
            }
            Ok(Event::Empty(e)) if local(e.name().as_ref()) == b"comment" => {
                if let Some(comment) = comment_shell(&e) {
                    comments.push(comment);
                }
            }
            Ok(Event::End(e)) if local(e.name().as_ref()) == b"AlternateContent" => {
                alternate_content_stack.pop();
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    comments
}

pub(crate) fn apply_extended_parent_ids(
    comments: &mut [Comment],
    comments_xml: &str,
    ex_xml: &str,
) {
    let id_to_para = comment_para_ids(comments_xml);
    if id_to_para.is_empty() {
        return;
    }
    let para_to_id: HashMap<String, String> = id_to_para
        .iter()
        .map(|(id, para_id)| (para_id.clone(), id.clone()))
        .collect();
    let metadata = extended_comment_metadata(ex_xml);
    for comment in comments {
        let Some(child_para_id) = id_to_para.get(&comment.id) else {
            continue;
        };
        let Some(meta) = metadata.get(child_para_id) else {
            continue;
        };
        if let Some(done) = meta.done {
            comment.resolved = Some(done);
        }
        if comment.parent_comment_id.is_some() {
            continue;
        }
        let Some(parent_para_id) = meta.parent_para_id.as_ref() else {
            continue;
        };
        if let Some(parent_id) = para_to_id.get(parent_para_id) {
            comment.parent_comment_id = Some(parent_id.clone());
        }
    }
}

fn comment_shell(e: &BytesStart<'_>) -> Option<Comment> {
    Some(Comment {
        id: attr_local_trimmed(e, b"id")?,
        author: attr_local_trimmed(e, b"author"),
        initials: attr_local_trimmed(e, b"initials"),
        date: attr_local_trimmed(e, b"date"),
        parent_comment_id: attr_local_trimmed(e, b"parentId"),
        text: String::new(),
        anchor: None,
        resolved: None,
    })
}

fn comment_para_ids(xml: &str) -> HashMap<String, String> {
    let mut r = Reader::from_str(xml);
    let mut current_comment_id: Option<String> = None;
    let mut ids = HashMap::new();
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
            Ok(Event::Start(e)) if local(e.name().as_ref()) == b"comment" => {
                current_comment_id = attr_local_trimmed(&e, b"id");
            }
            Ok(Event::Empty(e)) if local(e.name().as_ref()) == b"comment" => {
                current_comment_id = None;
            }
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) if local(e.name().as_ref()) == b"p" => {
                if let (Some(comment_id), Some(para_id)) = (
                    current_comment_id.as_ref(),
                    attr_local_trimmed(&e, b"paraId"),
                ) {
                    ids.insert(comment_id.clone(), para_id);
                }
            }
            Ok(Event::End(e)) if local(e.name().as_ref()) == b"comment" => {
                current_comment_id = None;
            }
            Ok(Event::End(e)) if local(e.name().as_ref()) == b"AlternateContent" => {
                alternate_content_stack.pop();
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    ids
}

#[derive(Default)]
struct CommentExMetadata {
    parent_para_id: Option<String>,
    done: Option<bool>,
}

fn extended_comment_metadata(xml: &str) -> HashMap<String, CommentExMetadata> {
    let mut r = Reader::from_str(xml);
    let mut ids: HashMap<String, CommentExMetadata> = HashMap::new();
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
            Ok(Event::Start(e)) | Ok(Event::Empty(e))
                if local(e.name().as_ref()) == b"commentEx" =>
            {
                if let Some(para_id) = attr_local_trimmed(&e, b"paraId") {
                    ids.insert(
                        para_id,
                        CommentExMetadata {
                            parent_para_id: attr_local_trimmed(&e, b"paraIdParent"),
                            done: attr_local_trimmed(&e, b"done")
                                .map(|v| super::toggle_on(Some(v))),
                        },
                    );
                }
            }
            Ok(Event::End(e)) if local(e.name().as_ref()) == b"AlternateContent" => {
                alternate_content_stack.pop();
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    ids
}

pub(crate) fn parse_anchors(xml: &str) -> HashMap<String, TextAnchor> {
    let mut r = Reader::from_str(xml);
    let mut anchors: HashMap<String, TextAnchor> = HashMap::new();
    let mut active: Vec<(String, bool)> = Vec::new();
    let mut complex_field = CommentComplexField::default();
    let mut old_content_depth = 0usize;
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
            Ok(Event::Start(e)) if matches!(local(e.name().as_ref()), b"del" | b"moveFrom") => {
                old_content_depth += 1;
            }
            Ok(Event::Start(e)) if is_comment_anchor_embedded_body(local(e.name().as_ref())) => {
                embedded_body_depth += 1;
            }
            Ok(Event::Start(e)) if local(e.name().as_ref()) == b"fldChar" => {
                if old_content_depth == 0 && (!active.is_empty() || complex_field.is_active()) {
                    if let Some(text) = complex_field.apply_field_char(&e) {
                        push_anchor_text(&active, &mut anchors, &text);
                    }
                }
                skip_subtree(&mut r);
            }
            Ok(Event::Empty(e)) if local(e.name().as_ref()) == b"fldChar" => {
                if old_content_depth == 0 && (!active.is_empty() || complex_field.is_active()) {
                    if let Some(text) = complex_field.apply_field_char(&e) {
                        push_anchor_text(&active, &mut anchors, &text);
                    }
                }
            }
            Ok(Event::Start(e)) if local(e.name().as_ref()) == b"instrText" => {
                let text = read_text(&mut r);
                if old_content_depth == 0 && (!active.is_empty() || complex_field.is_active()) {
                    complex_field.append_instruction_text(&text);
                }
            }
            Ok(Event::Start(e))
                if local(e.name().as_ref()) == b"p"
                    && old_content_depth == 0
                    && embedded_body_depth == 0 =>
            {
                push_anchor_paragraph_boundary(&active, &mut anchors);
            }
            Ok(Event::Start(e)) | Ok(Event::Empty(e))
                if local(e.name().as_ref()) == b"commentRangeStart" =>
            {
                if let Some(id) = attr_local_trimmed(&e, b"id") {
                    let visible = old_content_depth == 0;
                    if visible {
                        anchors.entry(id.clone()).or_insert_with(|| TextAnchor {
                            id: id.clone(),
                            text: String::new(),
                        });
                    }
                    active.push((id, visible));
                }
            }
            Ok(Event::Start(e)) if local(e.name().as_ref()) == b"fldSimple" => {
                if old_content_depth == 0 {
                    if let Some(text) = computed_comment_simple_field_text(&e) {
                        push_anchor_text(&active, &mut anchors, &text);
                        skip_subtree(&mut r);
                    }
                }
            }
            Ok(Event::Empty(e)) if local(e.name().as_ref()) == b"fldSimple" => {
                if old_content_depth == 0 {
                    if let Some(text) = computed_comment_simple_field_text(&e) {
                        push_anchor_text(&active, &mut anchors, &text);
                    }
                }
            }
            Ok(Event::Start(e)) if local(e.name().as_ref()) == b"t" => {
                let text = read_text(&mut r);
                if old_content_depth == 0 && !complex_field.suppresses_result() {
                    push_anchor_text(&active, &mut anchors, &text);
                }
            }
            Ok(Event::Start(e)) if local(e.name().as_ref()) == b"delText" => {
                let text = read_text(&mut r);
                if old_content_depth == 0 && !complex_field.suppresses_result() {
                    push_anchor_text(&active, &mut anchors, &text);
                }
            }
            Ok(Event::Start(e)) | Ok(Event::Empty(e))
                if local(e.name().as_ref()) == b"commentRangeEnd" =>
            {
                if let Some(id) = attr_local_trimmed(&e, b"id") {
                    if let Some(pos) = active.iter().rposition(|(active_id, _)| active_id == &id) {
                        active.remove(pos);
                    }
                }
            }
            Ok(Event::Start(e)) | Ok(Event::Empty(e))
                if old_content_depth == 0 && !complex_field.suppresses_result() =>
            {
                if let Some(text) = inline_marker_text(&e) {
                    push_anchor_text(&active, &mut anchors, text);
                } else if let Some(ch) = comment_symbol_char(&e) {
                    let text = ch.to_string();
                    push_anchor_text(&active, &mut anchors, &text);
                }
            }
            Ok(Event::End(e)) if matches!(local(e.name().as_ref()), b"del" | b"moveFrom") => {
                old_content_depth = old_content_depth.saturating_sub(1);
            }
            Ok(Event::End(e)) if is_comment_anchor_embedded_body(local(e.name().as_ref())) => {
                embedded_body_depth = embedded_body_depth.saturating_sub(1);
            }
            Ok(Event::End(e)) if local(e.name().as_ref()) == b"AlternateContent" => {
                alternate_content_stack.pop();
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    anchors
}

fn read_comment(r: &mut Xml<'_>, start: &BytesStart<'_>) -> Option<Comment> {
    let mut c = comment_shell(start);
    let mut complex_field = CommentComplexField::default();
    let mut old_content_depth = 0usize;
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
            Ok(Event::Start(e)) if matches!(local(e.name().as_ref()), b"del" | b"moveFrom") => {
                old_content_depth += 1;
            }
            Ok(Event::Start(e)) if is_comment_anchor_embedded_body(local(e.name().as_ref())) => {
                embedded_body_depth += 1;
            }
            Ok(Event::Start(e)) if local(e.name().as_ref()) == b"fldChar" => {
                if old_content_depth == 0 {
                    if let Some(text) = complex_field.apply_field_char(&e) {
                        if let Some(c) = c.as_mut() {
                            c.text.push_str(&text);
                        }
                    }
                }
                skip_subtree(r);
            }
            Ok(Event::Empty(e)) if local(e.name().as_ref()) == b"fldChar" => {
                if old_content_depth == 0 {
                    if let Some(text) = complex_field.apply_field_char(&e) {
                        if let Some(c) = c.as_mut() {
                            c.text.push_str(&text);
                        }
                    }
                }
            }
            Ok(Event::Start(e)) if local(e.name().as_ref()) == b"instrText" => {
                let text = read_text(r);
                if old_content_depth == 0 {
                    complex_field.append_instruction_text(&text);
                }
            }
            Ok(Event::Start(e))
                if local(e.name().as_ref()) == b"p"
                    && old_content_depth == 0
                    && embedded_body_depth == 0 =>
            {
                if let Some(c) = c.as_mut() {
                    push_comment_paragraph_boundary(&mut c.text);
                }
            }
            Ok(Event::Start(e)) if local(e.name().as_ref()) == b"fldSimple" => {
                if old_content_depth == 0 {
                    if let Some(text) = computed_comment_simple_field_text(&e) {
                        if let Some(c) = c.as_mut() {
                            c.text.push_str(&text);
                        }
                        skip_subtree(r);
                    }
                }
            }
            Ok(Event::Empty(e)) if local(e.name().as_ref()) == b"fldSimple" => {
                if old_content_depth == 0 {
                    if let Some(text) = computed_comment_simple_field_text(&e) {
                        if let Some(c) = c.as_mut() {
                            c.text.push_str(&text);
                        }
                    }
                }
            }
            Ok(Event::Start(e)) if matches!(local(e.name().as_ref()), b"t" | b"delText") => {
                let text = read_text(r);
                if old_content_depth == 0 && !complex_field.suppresses_result() {
                    if let Some(c) = c.as_mut() {
                        c.text.push_str(&text);
                    }
                }
            }
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                if old_content_depth == 0 && !complex_field.suppresses_result() {
                    if let Some(c) = c.as_mut() {
                        if let Some(text) = inline_marker_text(&e) {
                            c.text.push_str(text);
                        } else if let Some(ch) = comment_symbol_char(&e) {
                            c.text.push(ch);
                        }
                    }
                }
            }
            Ok(Event::End(e)) if matches!(local(e.name().as_ref()), b"del" | b"moveFrom") => {
                old_content_depth = old_content_depth.saturating_sub(1);
            }
            Ok(Event::End(e)) if is_comment_anchor_embedded_body(local(e.name().as_ref())) => {
                embedded_body_depth = embedded_body_depth.saturating_sub(1);
            }
            Ok(Event::End(e)) if local(e.name().as_ref()) == b"AlternateContent" => {
                alternate_content_stack.pop();
            }
            Ok(Event::End(e)) if local(e.name().as_ref()) == b"comment" => break,
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    c
}

fn comment_symbol_char(e: &BytesStart<'_>) -> Option<char> {
    let value = attr_local_trimmed(e, b"char")?;
    let font = attr_local_trimmed(e, b"font");
    computed_run_symbol_char(font.as_deref(), &value)
}

fn computed_comment_simple_field_text(e: &BytesStart<'_>) -> Option<String> {
    let instruction = attr_local_trimmed(e, b"instr")?;
    computed_comment_field_text(&instruction)
}

fn computed_comment_field_text(instruction: &str) -> Option<String> {
    computed_dynamic_result_with_bookmarks(instruction, &HashMap::new())
}

#[derive(Default)]
struct CommentComplexField {
    depth: usize,
    instruction: String,
    phase: Option<CommentComplexFieldPhase>,
    computed_result: Option<String>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CommentComplexFieldPhase {
    Instruction,
    Result,
}

impl CommentComplexField {
    fn is_active(&self) -> bool {
        self.depth > 0
    }

    fn apply_field_char(&mut self, e: &BytesStart<'_>) -> Option<String> {
        match field_char_type(e).as_deref() {
            Some("begin") => {
                if self.depth == 0 {
                    self.instruction.clear();
                    self.phase = Some(CommentComplexFieldPhase::Instruction);
                    self.computed_result = None;
                }
                self.depth += 1;
                None
            }
            Some("separate")
                if self.depth == 1 && self.phase == Some(CommentComplexFieldPhase::Instruction) =>
            {
                self.phase = Some(CommentComplexFieldPhase::Result);
                self.computed_result = computed_comment_field_text(&self.instruction);
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
        if self.depth == 1 && self.phase == Some(CommentComplexFieldPhase::Instruction) {
            self.instruction.push_str(text);
        }
    }

    fn suppresses_result(&self) -> bool {
        self.depth > 0
            && self.phase == Some(CommentComplexFieldPhase::Result)
            && self.computed_result.is_some()
    }
}

fn is_comment_anchor_embedded_body(name: &[u8]) -> bool {
    matches!(name, b"drawing" | b"pict" | b"object" | b"txbxContent")
}

fn push_comment_paragraph_boundary(text: &mut String) {
    if !text.is_empty() {
        text.push('\n');
    }
}

fn push_anchor_paragraph_boundary(
    active: &[(String, bool)],
    anchors: &mut HashMap<String, TextAnchor>,
) {
    for (id, visible) in active {
        if !visible {
            continue;
        }
        if let Some(anchor) = anchors.get_mut(id) {
            push_comment_paragraph_boundary(&mut anchor.text);
        }
    }
}

fn push_anchor_text(
    active: &[(String, bool)],
    anchors: &mut HashMap<String, TextAnchor>,
    text: &str,
) {
    for (id, visible) in active {
        if !visible {
            continue;
        }
        if let Some(anchor) = anchors.get_mut(id) {
            anchor.text.push_str(text);
        }
    }
}
