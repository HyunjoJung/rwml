//! `.docx` comments part parsing.

use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;

use std::collections::HashMap;

use crate::annotation::{Comment, TextAnchor};

use super::{attr_local, local};

type Xml<'a> = Reader<&'a [u8]>;

pub(crate) fn parse(xml: &str) -> Vec<Comment> {
    let mut r = Reader::from_str(xml);
    let mut comments = Vec::new();
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) if local(e.name().as_ref()) == b"comment" => {
                comments.push(read_comment(&mut r, &e));
            }
            Ok(Event::Empty(e)) if local(e.name().as_ref()) == b"comment" => {
                comments.push(comment_shell(&e));
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    comments
}

fn comment_shell(e: &BytesStart<'_>) -> Comment {
    Comment {
        id: attr_local(e, b"id").unwrap_or_default(),
        author: attr_local(e, b"author"),
        initials: attr_local(e, b"initials"),
        date: attr_local(e, b"date"),
        parent_comment_id: attr_local(e, b"parentId"),
        text: String::new(),
        anchor: None,
    }
}

pub(crate) fn parse_anchors(xml: &str) -> HashMap<String, TextAnchor> {
    let mut r = Reader::from_str(xml);
    let mut anchors: HashMap<String, TextAnchor> = HashMap::new();
    let mut active: Vec<(String, bool)> = Vec::new();
    let mut old_content_depth = 0usize;
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) if matches!(local(e.name().as_ref()), b"del" | b"moveFrom") => {
                old_content_depth += 1;
            }
            Ok(Event::Start(e)) | Ok(Event::Empty(e))
                if local(e.name().as_ref()) == b"commentRangeStart" =>
            {
                if let Some(id) = attr_local(&e, b"id") {
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
                if let Some(id) = attr_local(&e, b"id") {
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
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    anchors
}

fn read_comment(r: &mut Xml<'_>, start: &BytesStart<'_>) -> Comment {
    let mut c = comment_shell(start);
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) if local(e.name().as_ref()) == b"t" => {
                c.text.push_str(&read_text(r));
            }
            Ok(Event::Start(e)) if local(e.name().as_ref()) == b"delText" => {
                c.text.push_str(&read_text(r));
            }
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                if let Some(text) = inline_marker_text(&e) {
                    c.text.push_str(text);
                }
            }
            Ok(Event::End(e)) if local(e.name().as_ref()) == b"comment" => break,
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    c
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
