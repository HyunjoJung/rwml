//! Second pass: turn the piece table + character/paragraph properties + list
//! tables into the rich [`DocModel`].
//!
//! This never runs for the fast [`crate::Document::text`] path; it is built only
//! when a caller asks for the model or an exporter. It decodes the pieces a
//! second time into a CP-aligned `(units, fcs)` pair (each UTF-16 code unit
//! tagged with its source `WordDocument` byte offset) so character properties
//! (CHPX, keyed by FC) can be attached per run.

use std::collections::HashMap;

use encoding_rs::Encoding;

use crate::chpx::{Chp, ChpxTable};
use crate::clx::Piece;
use crate::fib::Fib;
use crate::list::Numberer;
use crate::model::{
    Align, Block, CharProps, DocMeta, DocModel, DocSetup, FieldRole, Image, ListInfo, ParaProps,
    Paragraph, SourceRegion, SourceRegionKind, Stats,
};
use crate::papx::PapxTable;
use crate::stsh::StyleSheet;
use crate::table::{self, RowBuild};
use crate::util::u32le;

/// Build the document model from the already-parsed structures.
#[allow(clippy::too_many_arguments)]
pub(crate) fn build_model(
    word: &[u8],
    table: &[u8],
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
    let (blocks, regions) = build_legacy_region_blocks(
        &units, &fcs, table, papx, chpx, stylesheet, data, fonts, numberer, fib,
    );
    let stats = compute_stats(&blocks);
    let setup = legacy_doc_setup_from_regions(&blocks, &regions);
    DocModel {
        blocks,
        regions,
        meta: DocMeta {
            codepage: fib.ansi_codepage(),
            lid: fib.lid,
            stats,
        },
        custom_properties: Default::default(),
        custom_xml_items: Vec::new(),
        setup,
    }
}

fn legacy_doc_setup_from_regions(blocks: &[Block], regions: &[SourceRegion]) -> DocSetup {
    let mut setup = DocSetup::default();
    if let Some(region) = regions.iter().find(|region| {
        region.kind == SourceRegionKind::HeaderFooter
            && region.block_start < region.block_end
            && legacy_header_footer_story_is_header(region.source_story_index).unwrap_or(true)
    }) {
        let start = region.block_start.min(blocks.len());
        let end = region.block_end.min(blocks.len());
        if start < end {
            setup.header = blocks[start..end].to_vec();
        }
    }
    setup
}

#[allow(clippy::too_many_arguments)]
fn build_legacy_region_blocks(
    units: &[u16],
    fcs: &[u32],
    table: &[u8],
    papx: &PapxTable,
    chpx: &ChpxTable,
    stylesheet: &StyleSheet,
    data: &[u8],
    fonts: &[String],
    numberer: &mut Numberer<'_>,
    fib: &Fib,
) -> (Vec<Block>, Vec<SourceRegion>) {
    let mut blocks = Vec::new();
    let mut regions = Vec::new();
    let mut source_start_cp = 0usize;
    let mut text_start = 0usize;
    let header_stories = header_footer_story_ranges(fib, table);

    for (kind, source_len_cp) in legacy_region_specs(fib) {
        if kind == SourceRegionKind::HeaderFooter && !header_stories.is_empty() {
            for story in header_stories
                .iter()
                .filter(|story| story.story_index >= HEADER_FOOTER_STORY_BASE)
            {
                push_legacy_region(
                    units,
                    fcs,
                    papx,
                    chpx,
                    stylesheet,
                    data,
                    fonts,
                    numberer,
                    &mut blocks,
                    &mut regions,
                    &mut text_start,
                    kind,
                    source_start_cp.saturating_add(story.start_cp),
                    story.end_cp.saturating_sub(story.start_cp),
                    Some(story.story_index),
                    false,
                );
            }
        } else {
            push_legacy_region(
                units,
                fcs,
                papx,
                chpx,
                stylesheet,
                data,
                fonts,
                numberer,
                &mut blocks,
                &mut regions,
                &mut text_start,
                kind,
                source_start_cp,
                source_len_cp,
                None,
                kind == SourceRegionKind::Main,
            );
        }

        source_start_cp = source_start_cp.saturating_add(source_len_cp);
    }

    (blocks, regions)
}

#[allow(clippy::too_many_arguments)]
fn push_legacy_region(
    units: &[u16],
    fcs: &[u32],
    papx: &PapxTable,
    chpx: &ChpxTable,
    stylesheet: &StyleSheet,
    data: &[u8],
    fonts: &[String],
    numberer: &mut Numberer<'_>,
    blocks: &mut Vec<Block>,
    regions: &mut Vec<SourceRegion>,
    text_start: &mut usize,
    kind: SourceRegionKind,
    source_start_cp: usize,
    source_len_cp: usize,
    source_story_index: Option<usize>,
    include_empty: bool,
) {
    let block_start = blocks.len();
    let actual_start = source_start_cp.min(units.len()).min(fcs.len());
    let actual_end = source_start_cp
        .saturating_add(source_len_cp)
        .min(units.len())
        .min(fcs.len());
    let mut region_blocks = if actual_start < actual_end {
        let mut asm = Asm::new(papx, chpx, stylesheet, data, fonts, numberer);
        asm.run(
            &units[actual_start..actual_end],
            &fcs[actual_start..actual_end],
        );
        asm.finish()
    } else {
        Vec::new()
    };
    let text_len = compute_stats(&region_blocks).text_chars;
    blocks.append(&mut region_blocks);
    let block_end = blocks.len();

    if source_len_cp > 0 || include_empty {
        regions.push(SourceRegion {
            kind,
            source_story_index,
            block_start,
            block_end,
            source_start_cp,
            source_len_cp,
            text_start: *text_start,
            text_len,
        });
    }

    *text_start = (*text_start).saturating_add(text_len);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct HeaderStoryRange {
    story_index: usize,
    start_cp: usize,
    end_cp: usize,
}

const HEADER_FOOTER_STORY_BASE: usize = 6;

fn legacy_header_footer_story_is_header(story_index: Option<usize>) -> Option<bool> {
    let story_index = story_index?;
    let position = story_index.checked_sub(HEADER_FOOTER_STORY_BASE)? % 6;
    match position {
        0 | 1 | 4 => Some(true),
        _ => Some(false),
    }
}

fn header_footer_story_ranges(fib: &Fib, table: &[u8]) -> Vec<HeaderStoryRange> {
    if fib.ccp_hdd == 0 || fib.lcb_plcf_hdd < 12 {
        return Vec::new();
    }
    let Some(slice) = table.get(fib.fc_plcf_hdd..fib.fc_plcf_hdd.saturating_add(fib.lcb_plcf_hdd))
    else {
        return Vec::new();
    };
    let cp_count = slice.len() / 4;
    if cp_count < 3 {
        return Vec::new();
    }
    let story_count = cp_count.saturating_sub(2);
    let hdd_len = fib.ccp_hdd as usize;
    let mut stories = Vec::new();
    for story_index in 0..story_count {
        let start = u32le(slice, story_index * 4).unwrap_or(0) as usize;
        let end = u32le(slice, (story_index + 1) * 4).unwrap_or(0) as usize;
        let start = start.min(hdd_len);
        let end = end.min(hdd_len);
        if start < end {
            stories.push(HeaderStoryRange {
                story_index,
                start_cp: start,
                end_cp: end,
            });
        }
    }
    stories
}

fn legacy_region_specs(fib: &Fib) -> [(SourceRegionKind, usize); 6] {
    [
        (SourceRegionKind::Main, fib.ccp_text as usize),
        (SourceRegionKind::Footnote, fib.ccp_ftn as usize),
        (SourceRegionKind::HeaderFooter, fib.ccp_hdd as usize),
        (SourceRegionKind::Annotation, fib.ccp_atn as usize),
        (SourceRegionKind::Endnote, fib.ccp_edn as usize),
        (SourceRegionKind::TextBox, fib.ccp_txbx as usize),
    ]
}

/// Decode every piece in CP order into UTF-16 code units, recording each unit's
/// source byte offset in the `WordDocument` stream (so CHPX/PAPX FC lookups land
/// on the right character).
fn decode_with_fc(word: &[u8], pieces: &[Piece], enc: &'static Encoding) -> (Vec<u16>, Vec<u32>) {
    let mut units: Vec<u16> = Vec::new();
    let mut fcs: Vec<u32> = Vec::new();
    // Bound cumulative decoded bytes (see `text::decode_pieces`): valid pieces partition the
    // stream (total ≤ word.len()), but overlapping pieces in a crafted piece table would
    // re-decode it per piece — a quadratic memory/CPU DoS. Stop once the budget is reached.
    let budget = word.len().saturating_add(16);
    let mut consumed = 0usize;
    for p in pieces {
        if p.cch == 0 {
            continue;
        }
        if consumed >= budget {
            break;
        }
        if p.compressed {
            let end = p.fc.saturating_add(p.cch).min(word.len());
            let Some(slice) = word.get(p.fc..end) else {
                continue;
            };
            consumed = consumed.saturating_add(slice.len());
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
            consumed = consumed.saturating_add(slice.len());
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

    // Current run being coalesced. `run_chp` is the (cheap, `Copy`) source the current
    // `run_props` was built from — comparing it per code unit avoids rebuilding the owned
    // `CharProps` (which clones the font name) and `FieldRole` (which clones the URL) for
    // every character. The URL is constant within a run because every `active_url` change
    // happens at a field mark, which flushes the run first.
    run_buf: Vec<u16>,
    run_chp: Chp,
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
    // separator and the instruction parsed at that point. Text is visible only
    // when *every* open field has seen its separator: if any enclosing field is
    // still in its instruction part, the text (even a nested field's result)
    // belongs to that instruction and is dropped. This makes a field with no
    // separator at all, and text after any field ends, correctly return to
    // visible-content mode — a plain bool could never be un-stuck and silently
    // swallowed all trailing text.
    field_stack: Vec<FieldState>,
    // Count of `field_stack` entries still in their instruction part (not yet
    // separated). `in_instruction()` is `unseparated != 0` — an O(1) replacement
    // for scanning `field_stack` per code unit, which a crafted run of N field
    // markers + N text chars turned into O(N²) work (CPU DoS via the model APIs).
    unseparated: usize,
    // Per-document inline-picture cache + byte budget. A crafted `.doc` can point
    // many picture runs (`0x01`) at the same `fcPic`, so without dedup the same
    // `Data` payload is rescanned and recopied per run — O(runs × payload). Cache
    // each `fcPic`'s extraction (scan once) and cap total materialized image bytes
    // (legit image bytes live once in `Data`, so ≤ ~2×data.len()).
    img_cache: HashMap<u32, Image>,
    img_budget: usize,
}

#[derive(Default)]
struct FieldState {
    separated: bool,
    instr_buf: Vec<u16>,
    role: FieldRole,
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
            run_chp: Chp::default(),
            run_props: CharProps::default(),
            run_field: FieldRole::None,
            para_runs: Vec::new(),
            cur_rows: Vec::new(),
            cur_row_cells: Vec::new(),
            cell_blocks: Vec::new(),
            field_stack: Vec::new(),
            unseparated: 0,
            img_cache: HashMap::new(),
            img_budget: data.len().saturating_mul(2).saturating_add(1 << 20),
        }
    }

    /// We are in field-instruction (drop) mode iff *any* open field has not yet
    /// passed its `0x14` separator — a nested field's result is still part of the
    /// enclosing field's instruction. Empty stack ⇒ visible body content. Tracked
    /// as a counter (not a per-call scan of `field_stack`) so this stays O(1).
    fn in_instruction(&self) -> bool {
        self.unseparated != 0
    }

    fn active_field_role(&self) -> FieldRole {
        if self.in_instruction() {
            return FieldRole::None;
        }
        self.field_stack
            .last()
            .map(|field| field.role.clone())
            .unwrap_or_default()
    }

    fn push_instruction_unit(&mut self, u: u16) {
        if let Some(field) = self
            .field_stack
            .iter_mut()
            .rev()
            .find(|field| !field.separated)
        {
            field.instr_buf.push(u);
        }
    }

    fn run(&mut self, units: &[u16], fcs: &[u32]) {
        for (i, &u) in units.iter().enumerate() {
            let fc = fcs.get(i).copied().unwrap_or(0);
            match u {
                FIELD_BEGIN => {
                    self.flush_run();
                    self.field_stack.push(FieldState::default());
                    self.unseparated += 1;
                }
                FIELD_SEP => {
                    // Mark the innermost field as separated → its result follows.
                    let n = self.field_stack.len();
                    if n > 0 && !self.field_stack[n - 1].separated {
                        self.field_stack[n - 1].separated = true;
                        self.unseparated -= 1;
                    }
                    if let Some(field) = self.field_stack.last_mut() {
                        let instr = String::from_utf16_lossy(&field.instr_buf);
                        field.role = field_role_from_instruction(&instr);
                    }
                    self.flush_run();
                }
                FIELD_END => {
                    self.flush_run();
                    if let Some(field) = self.field_stack.pop() {
                        if !field.separated {
                            self.unseparated -= 1;
                        }
                    }
                }
                _ if self.in_instruction() => self.push_instruction_unit(u),
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
        let img = self.extract_image(fc_pic);
        self.para_runs.push(Run_ {
            text: String::new(),
            props: CharProps::default(),
            field: FieldRole::None,
            field_dirty: false,
            image: Some(img),
            comment: None,
            revision: None,
            content_control: None,
            bookmark: None,
            note: None,
        });
    }

    /// Resolve the picture at `fc_pic`, scanning the `Data` stream at most once per
    /// location (cache) and bounding total materialized image bytes (budget). Once
    /// the budget is spent, further pictures become metadata-only placeholders — so
    /// a crafted `.doc` aliasing one payload across many runs stays O(input), not
    /// O(runs × payload), without dropping images in any real document.
    fn extract_image(&mut self, fc_pic: u32) -> Image {
        if !self.img_cache.contains_key(&fc_pic) {
            // PICF total size (lcb @ fcPic) bounds the scan; charge it before scanning
            // so even payloads with no recognizable raster cost the budget once.
            let lcb = crate::util::u32le(self.data, fc_pic as usize).unwrap_or(0) as usize;
            let img = if lcb == 0 || lcb > self.img_budget {
                Image::default()
            } else {
                self.img_budget = self.img_budget.saturating_sub(lcb);
                crate::image::extract(self.data, fc_pic)
            };
            self.img_cache.insert(fc_pic, img);
        }
        // Per-run copy: charge the emitted bytes; over budget ⇒ metadata-only
        // placeholder (no byte clone) so N references to one payload stay bounded.
        let n = self
            .img_cache
            .get(&fc_pic)
            .and_then(|i| i.bytes.as_ref())
            .map_or(0, |b| b.len());
        if n == 0 || n > self.img_budget {
            let c = self.img_cache.get(&fc_pic).expect("inserted above");
            return Image {
                alt: c.alt.clone(),
                bytes: None,
                mime: c.mime.clone(),
                width_px: c.width_px,
                height_px: c.height_px,
                rotation_degrees: c.rotation_degrees,
                floating_offset_emu: c.floating_offset_emu,
            };
        }
        self.img_budget -= n;
        self.img_cache.get(&fc_pic).cloned().unwrap_or_default()
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
        // Start a new run only when the (cheap) char properties change or after a flush
        // (e.g. a field mark, which is also the only place `active_url` changes). The owned
        // `CharProps`/`FieldRole` — which clone the font name and URL — are then built once
        // per run, not once per code unit (the latter was O(metadata × text) work).
        if self.run_buf.is_empty() || chp != self.run_chp {
            self.flush_run();
            self.run_chp = chp;
            self.run_props = CharProps {
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
            self.run_field = self.active_field_role();
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
            field_dirty: false,
            image: None,
            comment: None,
            revision: None,
            content_control: None,
            bookmark: None,
            note: None,
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

fn normalize_field_instruction(instr: &str) -> String {
    instr.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn field_role_from_instruction(instr: &str) -> FieldRole {
    if let Some(url) = parse_hyperlink(instr) {
        return FieldRole::Hyperlink { url };
    }
    let instruction = normalize_field_instruction(instr);
    if instruction.is_empty() {
        FieldRole::None
    } else {
        FieldRole::Simple { instruction }
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
            Block::Chart(_) | Block::PageBreak | Block::SectionBreak(_) => {}
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
    fn simple_field_result_keeps_instruction_on_result_run() {
        let mut units = vec![FIELD_BEGIN];
        units.extend(us(" PAGE "));
        units.push(FIELD_SEP);
        units.extend(us("7"));
        units.push(FIELD_END);
        units.extend(us(" tail"));
        units.push(PARA_MARK);
        let blocks = run_units(&units);
        let Block::Paragraph(p) = &blocks[0] else {
            panic!("expected paragraph");
        };

        assert_eq!(p.text(), "7 tail");
        let page = p.runs.iter().find(|r| r.text == "7").unwrap();
        assert_eq!(
            page.field,
            FieldRole::Simple {
                instruction: "PAGE".to_string()
            }
        );
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
    fn many_separated_fields_then_text_stays_linear_and_visible() {
        // Adversarial field shape: N [FIELD_BEGIN, FIELD_SEP] pairs leave N separated fields
        // on the stack, then N text chars + a paragraph mark. The old per-code-unit
        // `field_stack` scan made this O(N²); the `unseparated` counter keeps it O(N).
        // All fields are separated, so the trailing text is visible content.
        let n = 100_000;
        let mut units = Vec::with_capacity(n * 2 + n + 1);
        for _ in 0..n {
            units.push(FIELD_BEGIN);
            units.push(FIELD_SEP);
        }
        units.extend(std::iter::repeat(b'A' as u16).take(n));
        units.push(PARA_MARK);
        let text = all_text(&run_units(&units));
        assert_eq!(text.len(), n);
        assert!(text.chars().all(|c| c == 'A'));
    }

    #[test]
    fn repeated_picture_runs_are_deduped_and_byte_budget_bounds_total() {
        // Data stream: one PICF (cbHeader=8) + 33-byte blip header + a PNG signature
        // (mirrors image.rs::finds_png_after_blip_header).
        let png = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 1, 2, 3];
        let payload_len = 33 + png.len();
        let lcb = 8 + payload_len;
        let mut data = Vec::new();
        data.extend_from_slice(&(lcb as u32).to_le_bytes());
        data.extend_from_slice(&8u16.to_le_bytes());
        data.extend_from_slice(&[0u8; 2]); // pad to cbHeader = 8
        data.extend_from_slice(&[0u8; 33]); // blip header
        data.extend_from_slice(&png);

        let papx = PapxTable::default();
        let chpx = ChpxTable::default();
        let stsh = StyleSheet::default();
        let lists = Lists::default();
        let mut numberer = Numberer::new(&lists);
        let mut asm = Asm::new(&papx, &chpx, &stsh, &data, &[], &mut numberer);

        // Dedup: many runs at the same fcPic scan the Data once (one cache entry).
        let first = asm.extract_image(0);
        assert!(first.bytes.is_some(), "first extraction finds the PNG");
        for _ in 0..50 {
            let _ = asm.extract_image(0);
        }
        assert_eq!(asm.img_cache.len(), 1, "same fcPic scanned/cached once");

        // Byte budget bounds total materialized image bytes: once spent, further
        // picture runs become metadata-only placeholders instead of byte copies.
        let img_bytes = first.bytes.as_ref().unwrap().len();
        asm.img_budget = img_bytes; // room for exactly one more full copy
        assert!(asm.extract_image(0).bytes.is_some());
        let over = asm.extract_image(0);
        assert!(over.bytes.is_none(), "over-budget picture is a placeholder");
        assert_eq!(
            over.mime.as_deref(),
            Some("image/png"),
            "placeholder keeps mime"
        );
    }

    #[test]
    fn same_property_content_coalesces_into_one_run() {
        // Consecutive chars with identical (default) properties must coalesce into a single
        // run — confirming the per-run (not per-code-unit) property build still merges runs.
        let mut units = us("HELLOWORLD");
        units.push(PARA_MARK);
        let blocks = run_units(&units);
        let Block::Paragraph(p) = &blocks[0] else {
            panic!("expected paragraph");
        };
        assert_eq!(p.runs.len(), 1);
        assert_eq!(p.runs[0].text, "HELLOWORLD");
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
