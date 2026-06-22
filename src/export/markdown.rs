//! `.doc` → Markdown (GitHub-Flavored) exporter.
//!
//! Headings use `#`; bold/italic/strike use `**`/`*`/`~~`; hyperlinks use
//! `[text](url)`; lists use native `1.`/`-` with indentation by nesting level.
//! Rectangular, single-paragraph-cell tables become GFM pipe tables; tables with
//! merged cells or block content fall back to embedded HTML `<table>` (which GFM
//! permits), so nothing is silently lost.

use crate::model::{Block, DocModel, FieldRole, Paragraph, Run, Table};

pub(crate) fn render(doc: &DocModel) -> String {
    let mut out = String::new();
    let mut first = true;
    for block in &doc.blocks {
        let chunk = render_block(block);
        if chunk.is_empty() {
            continue;
        }
        if !first {
            out.push_str("\n\n");
        }
        out.push_str(&chunk);
        first = false;
    }
    out
}

fn render_block(block: &Block) -> String {
    match block {
        Block::Paragraph(p) => render_paragraph(p),
        Block::Table(t) => render_table(t),
        Block::Image(img) => format!("![{}]()", img.alt.as_deref().unwrap_or("image")),
    }
}

fn render_paragraph(p: &Paragraph) -> String {
    let inline = render_runs(&p.runs);
    if let Some(level) = p.props.heading_level {
        let hashes = "#".repeat(level.clamp(1, 6) as usize);
        return format!("{hashes} {inline}");
    }
    if let Some(list) = &p.props.list {
        let indent = "  ".repeat(list.level as usize);
        let marker = if list.ordered { "1. " } else { "- " };
        return format!("{indent}{marker}{inline}");
    }
    inline
}

fn render_runs(runs: &[Run]) -> String {
    let mut out = String::new();
    for run in runs {
        if run.image.is_some() {
            out.push_str("![image]()");
            continue;
        }
        if run.text.is_empty() {
            continue;
        }
        let escaped = escape(&run.text);
        // Don't wrap whitespace-only runs in emphasis (would emit `** **`).
        let styled = if run.text.trim().is_empty() {
            escaped
        } else {
            emphasis(escaped, run)
        };
        match &run.field {
            FieldRole::Hyperlink { url } => {
                out.push_str(&format!("[{}]({})", styled, escape_url(url)))
            }
            _ => out.push_str(&styled),
        }
    }
    out
}

fn emphasis(text: String, run: &Run) -> String {
    let mut t = text;
    if run.props.strike {
        t = format!("~~{t}~~");
    }
    if run.props.bold && run.props.italic {
        t = format!("***{t}***");
    } else if run.props.bold {
        t = format!("**{t}**");
    } else if run.props.italic {
        t = format!("*{t}*");
    }
    t
}

fn render_table(t: &Table) -> String {
    if !t.is_simple_grid() || t.rows.is_empty() {
        // Merges or block-content cells: GFM cannot express these losslessly.
        return super::html::render_table_fragment(t);
    }
    let ncols = t.rows.iter().map(|r| r.cells.len()).max().unwrap_or(0);
    if ncols == 0 {
        return String::new();
    }
    let cell_text = |row: &crate::model::Row, c: usize| -> String {
        row.cells
            .get(c)
            .map(|cell| cell.text().replace('|', "\\|").replace('\n', "<br>"))
            .unwrap_or_default()
    };
    let mut lines = Vec::new();
    // GFM requires a header row + separator; use the first row as the header.
    let header = &t.rows[0];
    lines.push(format!(
        "| {} |",
        (0..ncols)
            .map(|c| cell_text(header, c))
            .collect::<Vec<_>>()
            .join(" | ")
    ));
    lines.push(format!("| {} |", vec!["---"; ncols].join(" | ")));
    for row in &t.rows[1..] {
        lines.push(format!(
            "| {} |",
            (0..ncols)
                .map(|c| cell_text(row, c))
                .collect::<Vec<_>>()
                .join(" | ")
        ));
    }
    lines.join("\n")
}

/// Escape Markdown inline metacharacters in run text.
fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '\\' | '`' | '*' | '_' | '[' | ']' | '<' | '>' | '|' => {
                out.push('\\');
                out.push(ch);
            }
            _ => out.push(ch),
        }
    }
    out
}

fn escape_url(s: &str) -> String {
    s.replace(' ', "%20").replace(')', "%29")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::*;

    fn para(runs: Vec<Run>) -> Block {
        Block::Paragraph(Paragraph {
            props: ParaProps::default(),
            runs,
        })
    }
    fn run(text: &str, bold: bool, italic: bool) -> Run {
        Run {
            text: text.to_string(),
            props: CharProps {
                bold,
                italic,
                ..Default::default()
            },
            ..Default::default()
        }
    }

    #[test]
    fn bold_italic_and_heading() {
        let doc = DocModel {
            blocks: vec![
                Block::Paragraph(Paragraph {
                    props: ParaProps {
                        heading_level: Some(2),
                        ..Default::default()
                    },
                    runs: vec![run("제목", false, false)],
                }),
                para(vec![
                    run("bold", true, false),
                    run(" and ", false, false),
                    run("it", false, true),
                ]),
            ],
            meta: DocMeta::default(),
            ..Default::default()
        };
        let md = render(&doc);
        assert_eq!(md, "## 제목\n\n**bold** and *it*");
    }

    #[test]
    fn gfm_table() {
        let cell = |s: &str| Cell {
            blocks: vec![para(vec![run(s, false, false)])],
            ..Default::default()
        };
        let t = Table {
            rows: vec![
                Row {
                    cells: vec![cell("A"), cell("B")],
                },
                Row {
                    cells: vec![cell("1"), cell("2")],
                },
            ],
            header_rows: 0,
            ..Default::default()
        };
        let md = render(&DocModel {
            blocks: vec![Block::Table(t)],
            meta: DocMeta::default(),
            ..Default::default()
        });
        assert_eq!(md, "| A | B |\n| --- | --- |\n| 1 | 2 |");
    }

    #[test]
    fn hyperlink() {
        let r = Run {
            text: "site".to_string(),
            field: FieldRole::Hyperlink {
                url: "https://x.io".to_string(),
            },
            ..Default::default()
        };
        assert_eq!(
            render(&DocModel {
                blocks: vec![para(vec![r])],
                meta: DocMeta::default(),
                ..Default::default()
            }),
            "[site](https://x.io)"
        );
    }
}
