//! `.docx` comments part parsing.

use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;

use std::collections::HashMap;

use crate::annotation::{Comment, TextAnchor};

use super::{attr_local_trimmed, local};

type Xml<'a> = Reader<&'a [u8]>;

pub(crate) fn parse(xml: &str) -> Vec<Comment> {
    let mut r = Reader::from_str(xml);
    let mut comments = Vec::new();
    loop {
        match r.read_event() {
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
    let parent_para_by_child_para = extended_parent_para_ids(ex_xml);
    for comment in comments {
        if comment.parent_comment_id.is_some() {
            continue;
        }
        let Some(child_para_id) = id_to_para.get(&comment.id) else {
            continue;
        };
        let Some(parent_para_id) = parent_para_by_child_para.get(child_para_id) else {
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
    })
}

fn comment_para_ids(xml: &str) -> HashMap<String, String> {
    let mut r = Reader::from_str(xml);
    let mut current_comment_id: Option<String> = None;
    let mut ids = HashMap::new();
    loop {
        match r.read_event() {
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
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    ids
}

fn extended_parent_para_ids(xml: &str) -> HashMap<String, String> {
    let mut r = Reader::from_str(xml);
    let mut ids = HashMap::new();
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) | Ok(Event::Empty(e))
                if local(e.name().as_ref()) == b"commentEx" =>
            {
                if let (Some(para_id), Some(parent_para_id)) = (
                    attr_local_trimmed(&e, b"paraId"),
                    attr_local_trimmed(&e, b"paraIdParent"),
                ) {
                    ids.insert(para_id, parent_para_id);
                }
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
    let mut old_content_depth = 0usize;
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
            Ok(Event::Start(e)) if local(e.name().as_ref()) == b"t" => {
                let text = read_text(&mut r);
                if old_content_depth == 0 {
                    push_anchor_text(&active, &mut anchors, &text);
                }
            }
            Ok(Event::Start(e)) if local(e.name().as_ref()) == b"delText" => {
                let text = read_text(&mut r);
                if old_content_depth == 0 {
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
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) if old_content_depth == 0 => {
                if let Some(text) = inline_marker_text(&e) {
                    push_anchor_text(&active, &mut anchors, text);
                }
            }
            Ok(Event::End(e)) if matches!(local(e.name().as_ref()), b"del" | b"moveFrom") => {
                old_content_depth = old_content_depth.saturating_sub(1);
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
    let mut old_content_depth = 0usize;
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
            Ok(Event::Start(e)) if matches!(local(e.name().as_ref()), b"t" | b"delText") => {
                let text = read_text(r);
                if old_content_depth == 0 {
                    if let Some(c) = c.as_mut() {
                        c.text.push_str(&text);
                    }
                }
            }
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                if old_content_depth == 0 {
                    if let (Some(c), Some(text)) = (c.as_mut(), inline_marker_text(&e)) {
                        c.text.push_str(text);
                    }
                }
            }
            Ok(Event::End(e)) if matches!(local(e.name().as_ref()), b"del" | b"moveFrom") => {
                old_content_depth = old_content_depth.saturating_sub(1);
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

#[derive(Default)]
struct AlternateContentBranchState {
    took_branch: bool,
}

fn skip_alternate_content_branch(stack: &mut [AlternateContentBranchState], name: &[u8]) -> bool {
    if !matches!(name, b"Choice" | b"Fallback") {
        return false;
    }
    let Some(state) = stack.last_mut() else {
        return false;
    };
    if state.took_branch {
        true
    } else {
        state.took_branch = true;
        false
    }
}

fn skip_subtree(r: &mut Xml<'_>) {
    let mut depth = 1usize;
    loop {
        match r.read_event() {
            Ok(Event::Start(_)) => depth += 1,
            Ok(Event::End(_)) => {
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

fn inline_marker_text(e: &BytesStart<'_>) -> Option<&'static str> {
    match local(e.name().as_ref()) {
        b"tab" => Some("\t"),
        b"br" | b"cr" => Some("\n"),
        b"noBreakHyphen" => Some("-"),
        _ => None,
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
