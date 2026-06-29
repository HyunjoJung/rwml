//! Shared inline text helpers for `.docx` side-channel parts.

use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;

use super::local;

#[derive(Default)]
pub(crate) struct AlternateContentBranchState {
    took_branch: bool,
}

pub(crate) fn skip_alternate_content_branch(
    stack: &mut [AlternateContentBranchState],
    name: &[u8],
) -> bool {
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

pub(crate) fn skip_subtree(r: &mut Reader<&[u8]>) {
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

pub(crate) fn read_text(r: &mut Reader<&[u8]>) -> String {
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

pub(crate) fn inline_marker_text(e: &BytesStart<'_>) -> Option<&'static str> {
    match local(e.name().as_ref()) {
        b"tab" => Some("\t"),
        b"br" | b"cr" => Some("\n"),
        b"noBreakHyphen" => Some("-"),
        b"softHyphen" => Some("\u{00ad}"),
        _ => None,
    }
}
