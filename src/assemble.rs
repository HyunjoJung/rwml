//! Second pass: turn the piece table + character/paragraph properties + list
//! tables into the rich [`DocModel`].
//!
//! This never runs for the fast [`crate::Document::text`] path; it is built only
//! when a caller asks for the model or an exporter. It decodes the pieces a
//! second time into a CP-aligned `(units, fcs)` pair (each UTF-16 code unit
//! tagged with its source `WordDocument` byte offset) so character properties
//! (CHPX, keyed by FC) can be attached per run.

use encoding_rs::Encoding;

use crate::chpx::ChpxTable;
use crate::clx::Piece;
use crate::fib::Fib;
use crate::list::Numberer;
use crate::model::{
    Align, Block, CharProps, DocMeta, DocModel, FieldRole, ListInfo, ParaProps, Paragraph, Stats,
};
use crate::papx::PapxTable;
use crate::stsh::StyleSheet;
use crate::table::{self, RowBuild};

/// Build the document model from the already-parsed structures.
#[allow(clippy::too_many_arguments)]
pub(crate) fn build_model(
    word: &[u8],
    pieces: &[Piece],
    enc: &'static Encoding,
    papx: &PapxTable,
    chpx: &ChpxTable,
    stylesheet: &StyleSheet,
    data: &[u8],
    fonts: &[String],
    numberer: &mut Numberer<'_>,
    fib: &Fib,
) -> DocModel {
    let (units, fcs) = decode_with_fc(word, pieces, enc);
    let mut asm = Asm::new(papx, chpx, stylesheet, data, fonts, numberer);
    asm.run(&units, &fcs);
    let blocks = asm.finish();
    let stats = compute_stats(&blocks);
    DocModel {
        blocks,
        meta: DocMeta {
            codepage: fib.ansi_codepage(),
            lid: fib.lid,
            stats,
        },
        setup: crate::model::DocSetup::default(),
    }
}

/// Decode every piece in CP order into UTF-16 code units, recording each unit's
/// source byte offset in the `WordDocument` stream (so CHPX/PAPX FC lookups land
/// on the right character).
fn decode_with_fc(word: &[u8], pieces: &[Piece], enc: &'static Encoding) -> (Vec<u16>, Vec<u32>) {
    let mut units: Vec<u16> = Vec::new();
    let mut fcs: Vec<u32> = Vec::new();
    for p in pieces {
        if p.cch == 0 {
            continue;
        }
        if p.compressed {
            let end = p.fc.saturating_add(p.cch).min(word.len());
            let Some(slice) = word.get(p.fc..end) else {
                continue;
            };
            // Decode the whole 8-bit slice (handles multi-byte cp949/cp932), then
            // assign each char its source FC by re-encoding to count its bytes.
            let text = enc.decode(slice).0;
            let mut fc = p.fc as u32;
            let mut tmp = [0u8; 4];
            let mut ubuf = [0u16; 2];
            for ch in text.chars() {
                let chs = ch.encode_utf8(&mut tmp);
                // Re-encode to recover the source byte width. An undecodable byte
                // decodes to U+FFFD, which `encode` would turn into a multi-byte
                // numeric character reference (`&#65533;`) — that would over-count
                // and shift every following FC, misattributing CHPX runs. Guard
                // it: on a round-trip error the source was a single bad byte, and
                // no char in any supported ANSI codepage is wider than 2 bytes.
                let (eb, _, had_err) = enc.encode(chs);
                let blen = if had_err { 1 } else { eb.len().clamp(1, 2) } as u32;
                for u in ch.encode_utf16(&mut ubuf) {
                    units.push(*u);
                    fcs.push(fc);
                }
                fc = fc.saturating_add(blen);
            }
        } else {
            let byte_len = p.cch.saturating_mul(2);
            let end = p.fc.saturating_add(byte_len).min(word.len());
            let Some(slice) = word.get(p.fc..end) else {
                continue;
            };
            for (i, c) in slice.chunks_exact(2).enumerate() {
                units.push(u16::from_le_bytes([c[0], c[1]]));
                fcs.push((p.fc + i * 2) as u32);
            }
        }
    }
    (units, fcs)
}

// Word control characters.
const CELL_MARK: u16 = 0x07;
const PARA_MARK: u16 = 0x0D;
const FIELD_BEGIN: u16 = 0x13;
const FIELD_SEP: u16 = 0x14;
const FIELD_END: u16 = 0x15;

/// Streaming assembler over the `(units, fcs)` stream.
struct Asm<'a, 'l> {
    papx: &'a PapxTable,
    chpx: &'a ChpxTable,
    stylesheet: &'a StyleSheet,
    data: &'a [u8],
    fonts: &'a [String],
    numberer: &'a mut Numberer<'l>,

    blocks: Vec<Block>,

    // Current run being coalesced.
    run_buf: Vec<u16>,
    run_props: CharProps,
    run_field: FieldRole,

    // Current paragraph's runs.
    para_runs: Vec<Run_>,

    // Table-building state.
    cur_rows: Vec<RowBuild>,
    cur_row_cells: Vec<Vec<Block>>,
    cell_blocks: Vec<Block>,

    // Field state. `field_stack` holds one entry per currently-open field
    // (`0x13`..`0x15`), each recording whether that field has passed its `0x14`
    // separator. Text is visible only when *every* open field has seen its
    // separator: if any enclosing field is still in its instruction part, the
    // text (even a nested field's result) belongs to that instruction and is
    // dropped. This makes a field with no separator at all, and text after any
    // field ends, correctly return to visible-content mode — a plain bool could
    // never be un-stuck and silently swallowed all trailing text.
    field_stack: Vec<bool>,
    instr_buf: Vec<u16>,
    active_url: Option<String>,
}

// Local alias to the model Run (avoid a name clash with the field below).
use crate::model::Run as Run_;

impl<'a, 'l> Asm<'a, 'l> {
    fn new(
        papx: &'a PapxTable,
        chpx: &'a ChpxTable,
        stylesheet: &'a StyleSheet,
        data: &'a [u8],
        fonts: &'a [String],
        numberer: &'a mut Numberer<'l>,
    ) -> Self {
        Asm {
            papx,
            chpx,
            stylesheet,
            data,
            fonts,
            numberer,
            blocks: Vec::new(),
            run_buf: Vec::new(),
            run_props: CharProps::default(),
            run_field: FieldRole::None,
            para_runs: Vec::new(),
            cur_rows: Vec::new(),
            cur_row_cells: Vec::new(),
            cell_blocks: Vec::new(),
            field_stack: Vec::new(),
            instr_buf: Vec::new(),
            active_url: None,
        }
    }

    /// We are in field-instruction (drop) mode iff *any* open field has not yet
    /// passed its `0x14` separator — a nested field's result is still part of the
    /// enclosing field's instruction. Empty stack ⇒ visible body content.
    fn in_instruction(&self) -> bool {
        self.field_stack.iter().any(|&seen_sep| !seen_sep)
    }

    fn run(&mut self, units: &[u16], fcs: &[u32]) {
        for (i, &u) in units.iter().enumerate() {
            let fc = fcs.get(i).copied().unwrap_or(0);
            match u {
                FIELD_BEGIN => {
                    self.flush_run();
                    self.field_stack.push(false);
                    self.instr_buf.clear();
                }
                FIELD_SEP => {
                    // Mark the innermost field as separated → its result follows.
                    if let Some(top) = self.field_stack.last_mut() {
                        *top = true;
                    }
                    let instr = String::from_utf16_lossy(&self.instr_buf);
                    if let Some(url) = parse_hyperlink(&instr) {
                        self.active_url = Some(url);
                    }
                    self.flush_run();
                }
                FIELD_END => {
                    self.flush_run();
                    self.field_stack.pop();
                    if self.field_stack.is_empty() {
                        self.active_url = None;
                    }
                }
                _ if self.in_instruction() => self.instr_buf.push(u),
                PARA_MARK => self.end_paragraph(fc, false),
                CELL_MARK => self.end_paragraph(fc, true),
                0x0001 => self.picture(fc),
                _ => self.push_content(u, fc),
            }
        }
    }

    /// An inline picture special char (`0x01`): if the run is a real picture
    /// (`fSpec` + `sprmCPicLocation`), extract it into an image run; otherwise
    /// (embedded OLE object, form field) drop it.
    fn picture(&mut self, fc: u32) {
        let Some(fc_pic) = self.chpx.pic_at(fc) else {
            return;
        };
        self.flush_run();
        let img = crate::image::extract(self.data, fc_pic);
        self.para_runs.push(Run_ {
            text: String::new(),
            props: CharProps::default(),
            field: FieldRole::None,
            image: Some(img),
        });
    }

    /// Append a content code unit to the current run, splitting the run when the
    /// character properties or field role change.
    fn push_content(&mut self, u: u16, fc: u32) {
        // Map Word control characters to plain text; drop the unrenderable ones.
        let mapped: Option<u16> = match u {
            0x0B | 0x0C | 0x0E => Some(0x000A), // line / page / column break → newline
            0x1E => Some(0x002D),               // non-breaking hyphen → '-'
            0xA0 => Some(0x0020),               // non-breaking space → ' '
            0x1F => None,                       // optional hyphen → drop
            0x01 | 0x02 | 0x08 => None,         // picture / footnote / object anchors (Slice 5)
            c if c < 0x20 && c != b'\t' as u16 => None, // other C0 controls
            c => Some(c),
        };
        let Some(unit) = mapped else { return };

        let chp = self.chpx.chp_at(fc);
        let props = CharProps {
            bold: chp.bold,
            italic: chp.italic,
            underline: chp.underline,
            strike: chp.strike,
            hidden: chp.hidden,
            size_half_pt: chp.size_half_pt,
            color: chp.color,
            font: chp.ftc.and_then(|ftc| crate::ffn::name_of(self.fonts, ftc)),
            ..Default::default()
        };
        let field = match &self.active_url {
            Some(url) => FieldRole::Hyperlink { url: url.clone() },
            None => FieldRole::None,
        };
        if props != self.run_props || field != self.run_field {
            self.flush_run();
            self.run_props = props;
            self.run_field = field;
        }
        self.run_buf.push(unit);
    }

    fn flush_run(&mut self) {
        if self.run_buf.is_empty() {
            return;
        }
        let text = String::from_utf16_lossy(&self.run_buf);
        self.run_buf.clear();
        self.para_runs.push(Run_ {
            text,
            props: self.run_props.clone(),
            field: self.run_field.clone(),
            image: None,
        });
    }

    /// Finalize the runs collected so far into a [`Paragraph`] with list info.
    fn take_paragraph(&mut self, fc: u32) -> Paragraph {
        self.flush_run();
        let runs = std::mem::take(&mut self.para_runs);
        let (ilfo, ilvl) = self.papx.list_at(fc);
        let list = if ilfo > 0 {
            self.numberer.label(ilfo, ilvl).map(|label| ListInfo {
                level: ilvl,
                ordered: !label.trim().is_empty(),
                label,
            })
        } else {
            None
        };
        // Heading level: an explicit outline level on the paragraph wins
        // (0..8 → h1..h9, 9 → body); otherwise the paragraph style decides.
        let (istd, outlvl, jc) = self.papx.style_at(fc);
        let heading_level = match outlvl {
            Some(o) if o <= 8 => Some(o + 1),
            Some(_) => None,
            None => self.stylesheet.heading_level(istd),
        };
        let align = match jc {
            1 => Align::Center,
            2 => Align::Right,
            3 | 4 => Align::Justify,
            _ => Align::Left,
        };
        let style_name = self.stylesheet.name(istd).map(str::to_string);
        // A heading takes precedence over list-item rendering.
        let list = if heading_level.is_some() { None } else { list };
        Paragraph {
            props: ParaProps {
                style_name,
                heading_level,
                align,
                outline_level: outlvl,
                list,
                ..Default::default()
            },
            runs,
        }
    }

    /// Handle a paragraph (`0x0D`) or cell (`0x07`) mark: finalize the paragraph
    /// and route it into the body or the current table.
    fn end_paragraph(&mut self, fc: u32, is_cell_mark: bool) {
        let (in_table, ttp) = self.papx.at(fc);
        let para = self.take_paragraph(fc);

        if !in_table {
            self.flush_table();
            if !para.is_blank() {
                self.blocks.push(Block::Paragraph(para));
            }
            return;
        }

        // A 0x0D inside a table starts a new paragraph within the SAME cell; a
        // 0x07 closes the cell (and, when it is the row terminator, the row).
        if !is_cell_mark {
            self.cell_blocks.push(Block::Paragraph(para));
            return;
        }
        // The row-terminating paragraph (`fTtp`) is an empty marker, not a real
        // cell — don't emit it as a phantom trailing column.
        let blank_terminator = ttp && para.is_blank() && self.cell_blocks.is_empty();
        if !blank_terminator {
            self.cell_blocks.push(Block::Paragraph(para));
            self.cur_row_cells
                .push(std::mem::take(&mut self.cell_blocks));
        } else {
            self.cell_blocks.clear();
        }
        if ttp {
            // The row definition (column geometry + merge flags) is carried on the
            // TTP paragraph's grpprl.
            let def = self.papx.table_def_at(fc).cloned();
            let header = self.papx.table_header_at(fc);
            self.cur_rows.push(RowBuild {
                cells: std::mem::take(&mut self.cur_row_cells),
                def,
                header,
            });
        }
    }

    /// Emit any in-progress table as a block, resolving cell merges.
    fn flush_table(&mut self) {
        // A dangling row (no row terminator) is still real tabular data.
        if !self.cur_row_cells.is_empty() {
            self.cur_rows.push(RowBuild {
                cells: std::mem::take(&mut self.cur_row_cells),
                def: None,
                header: false,
            });
        }
        self.cell_blocks.clear();
        if !self.cur_rows.is_empty() {
            let t = table::build(std::mem::take(&mut self.cur_rows));
            if !t.rows.is_empty() {
                self.blocks.push(Block::Table(t));
            }
        }
    }

    /// Flush trailing paragraph/table state at end of stream.
    fn finish(mut self) -> Vec<Block> {
        // A trailing paragraph with no final mark.
        if !self.para_runs.is_empty() || !self.run_buf.is_empty() {
            let para = self.take_paragraph(u32::MAX);
            if !para.is_blank() {
                self.blocks.push(Block::Paragraph(para));
            }
        }
        self.flush_table();
        self.blocks
    }
}

/// Extract the target URL from a `HYPERLINK` field instruction, e.g.
/// `HYPERLINK "https://example.com" \o "tooltip"` → `https://example.com`.
fn parse_hyperlink(instr: &str) -> Option<String> {
    let s = instr.trim();
    let after = s.find("HYPERLINK").map(|i| &s[i + "HYPERLINK".len()..])?;
    let start = after.find('"')?;
    let rest = &after[start + 1..];
    let end = rest.find('"')?;
    let url = rest[..end].trim();
    if url.is_empty() {
        None
    } else {
        Some(url.to_string())
    }
}

/// Aggregate paragraph/table/figure/character counts over a block tree. Shared
/// with the `.docx` path so both backends report stats identically.
pub(crate) fn compute_stats(blocks: &[Block]) -> Stats {
    let mut s = Stats::default();
    count_blocks(blocks, &mut s);
    s
}

fn count_blocks(blocks: &[Block], s: &mut Stats) {
    for b in blocks {
        match b {
            Block::Paragraph(p) => {
                s.paragraphs = s.paragraphs.saturating_add(1);
                s.text_chars += p.text().chars().count();
                for r in &p.runs {
                    if r.image.is_some() {
                        s.figures = s.figures.saturating_add(1);
                    }
                }
            }
            Block::Image(_) => s.figures = s.figures.saturating_add(1),
            Block::Table(t) => {
                s.tables = s.tables.saturating_add(1);
                for row in &t.rows {
                    for cell in &row.cells {
                        count_blocks(&cell.blocks, s);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::list::Lists;

    /// Run the assembler over a bare unit stream (FCs = 1:1 with index, no
    /// styling/list tables) and return the resulting blocks.
    fn run_units(units: &[u16]) -> Vec<Block> {
        let fcs: Vec<u32> = (0..units.len() as u32).collect();
        let papx = PapxTable::default();
        let chpx = ChpxTable::default();
        let stsh = StyleSheet::default();
        let lists = Lists::default();
        let mut numberer = Numberer::new(&lists);
        let mut asm = Asm::new(&papx, &chpx, &stsh, &[], &[], &mut numberer);
        asm.run(units, &fcs);
        asm.finish()
    }

    fn all_text(blocks: &[Block]) -> String {
        blocks
            .iter()
            .filter_map(|b| match b {
                Block::Paragraph(p) => Some(p.text()),
                _ => None,
            })
            .collect()
    }

    fn us(s: &str) -> Vec<u16> {
        s.encode_utf16().collect()
    }

    #[test]
    fn field_without_separator_does_not_swallow_following_text() {
        // 0x13 "AB" 0x15 (field begin, instruction, end — NO 0x14 separator),
        // then body "CD". The body must survive: a single in-instruction bool
        // would stay stuck after 0x15 and drop everything after it.
        let mut units = vec![FIELD_BEGIN];
        units.extend(us("AB"));
        units.push(FIELD_END);
        units.extend(us("CD"));
        units.push(PARA_MARK);
        assert_eq!(all_text(&run_units(&units)), "CD");
    }

    #[test]
    fn hyperlink_field_result_is_kept_and_linked() {
        // 0x13 ` HYPERLINK "http://x" ` 0x14 `link` 0x15, then body.
        let mut units = vec![FIELD_BEGIN];
        units.extend(us(" HYPERLINK \"http://x\" "));
        units.push(FIELD_SEP);
        units.extend(us("link"));
        units.push(FIELD_END);
        units.extend(us(" tail"));
        units.push(PARA_MARK);
        let blocks = run_units(&units);
        let Block::Paragraph(p) = &blocks[0] else {
            panic!("expected paragraph");
        };
        // The HYPERLINK instruction is dropped; only the result text + tail remain.
        assert_eq!(p.text(), "link tail");
        let linked = p
            .runs
            .iter()
            .find(|r| matches!(&r.field, FieldRole::Hyperlink { .. }));
        match linked.map(|r| (&r.text, &r.field)) {
            Some((t, FieldRole::Hyperlink { url })) => {
                assert_eq!(t, "link");
                assert_eq!(url, "http://x");
            }
            other => panic!("expected linked result run, got {other:?}"),
        }
        // The url does not leak onto the post-field tail.
        let tail = p.runs.iter().find(|r| r.text == " tail").unwrap();
        assert_eq!(tail.field, FieldRole::None);
    }

    #[test]
    fn nested_field_returns_to_outer_instruction_then_result() {
        // Outer field whose instruction itself contains a nested field:
        // 0x13 "A" 0x13 "B" 0x14 "C" 0x15 "D" 0x14 "RESULT" 0x15.
        // "A".."D" are all outer-instruction (dropped); only "RESULT" shows.
        let mut units = vec![FIELD_BEGIN];
        units.extend(us("A"));
        units.push(FIELD_BEGIN);
        units.extend(us("B"));
        units.push(FIELD_SEP);
        units.extend(us("C"));
        units.push(FIELD_END);
        units.extend(us("D"));
        units.push(FIELD_SEP);
        units.extend(us("RESULT"));
        units.push(FIELD_END);
        units.push(PARA_MARK);
        assert_eq!(all_text(&run_units(&units)), "RESULT");
    }

    #[test]
    fn decode_with_fc_keeps_fc_aligned_past_an_undecodable_byte() {
        use crate::clx::Piece;
        // cp1252 piece: 'A', 0x81 (undefined → U+FFFD), 'B'. Each source byte is
        // one char, so FCs must be base, base+1, base+2 — not blown out by the
        // U+FFFD re-encoding into a numeric character reference.
        let base = 0x200usize;
        let mut word = vec![0u8; base];
        word.extend_from_slice(&[b'A', 0x81, b'B']);
        let pieces = [Piece {
            cch: 3,
            fc: base,
            compressed: true,
        }];
        let (units, fcs) = decode_with_fc(&word, &pieces, encoding_rs::WINDOWS_1252);
        assert_eq!(units.len(), 3);
        assert_eq!(fcs, vec![base as u32, base as u32 + 1, base as u32 + 2]);
        assert_eq!(units[0], b'A' as u16);
        assert_eq!(units[2], b'B' as u16);
    }

    #[test]
    fn hyperlink_instruction_parsing() {
        assert_eq!(
            parse_hyperlink(" HYPERLINK \"https://example.com\" \\o \"tip\" ").as_deref(),
            Some("https://example.com")
        );
        assert_eq!(parse_hyperlink(" PAGE "), None);
        assert_eq!(
            parse_hyperlink(" HYPERLINK \\l \"anchor\" ").as_deref(),
            Some("anchor")
        );
    }
}
