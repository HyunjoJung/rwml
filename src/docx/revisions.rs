//! `.docx` tracked-change marker parsing.

use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;

use crate::annotation::{Revision, RevisionKind, RevisionView};
use crate::text;

use super::{attr_local_trimmed, local};

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
                } else if matches!(local(e.name().as_ref()), b"t" | b"delText") {
                    push_segment(&mut out, &read_text(&mut r));
                }
            }
            Ok(Event::Empty(e)) => {
                if let Some(kind) = revision_kind(local(e.name().as_ref())) {
                    push_revision_text(&mut out, view, kind, "");
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
            Ok(Event::Start(e)) if local(e.name().as_ref()) == b"t" => {
                text.push_str(&read_text(r));
            }
            Ok(Event::Start(e)) if local(e.name().as_ref()) == b"delText" => {
                text.push_str(&read_text(r));
            }
            Ok(Event::Start(e)) => {
                if let Some(marker) = inline_marker_text(&e) {
                    text.push_str(marker);
                    skip_subtree(r);
                }
            }
            Ok(Event::Empty(e)) => {
                if let Some(marker) = inline_marker_text(&e) {
                    text.push_str(marker);
                }
            }
            Ok(Event::End(e)) if local(e.name().as_ref()) == end_name => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    break;
                }
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

fn push_segment(out: &mut String, value: &str) {
    if value.is_empty() {
        return;
    }
    if !out.is_empty() {
        out.push(' ');
    }
    out.push_str(value);
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
