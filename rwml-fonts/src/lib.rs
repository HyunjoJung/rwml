#![forbid(unsafe_code)]
//! Bundled OFL font subsets for rwml's PDF renderer.
//!
//! The bundled faces are regular-weight Noto Sans KR, Noto Sans Arabic, and
//! Noto Sans Hebrew. They are layout-derived subsets, not the full upstream
//! fonts.
//!
//! `noto_sans_kr_subset` is the slim subset: KS X 1001 wansung Hangul (2,350
//! syllables), Hangul compatibility jamo, Basic Latin, Latin-1, and common
//! punctuation.
//!
//! `noto_sans_kr_subset_with_hanja` adds KS X 1001 hanja coverage. It maps
//! 4,885 of the 4,888 KS X 1001 hanja characters; 3 compatibility ideographs
//! are absent from upstream Noto Sans KR itself.
//!
//! `noto_sans_arabic_subset` and `noto_sans_hebrew_subset` cover common RTL
//! letters, combining marks, digits, and punctuation. Both retain the OpenType
//! shaping tables required by a layout engine.

/// Return the bundled Noto Sans KR Regular subset bytes.
pub fn noto_sans_kr_subset() -> &'static [u8] {
    include_bytes!("../fonts/NotoSansKR-rwml-subset.ttf")
}

/// Return the bundled Noto Sans KR Regular subset bytes with KS X 1001 hanja.
pub fn noto_sans_kr_subset_with_hanja() -> &'static [u8] {
    include_bytes!("../fonts/NotoSansKR-rwml-subset-full.ttf")
}

/// Return the bundled Noto Sans Arabic Regular subset bytes.
pub fn noto_sans_arabic_subset() -> &'static [u8] {
    include_bytes!("../fonts/NotoSansArabic-rwml-subset.ttf")
}

/// Return the bundled Noto Sans Hebrew Regular subset bytes.
pub fn noto_sans_hebrew_subset() -> &'static [u8] {
    include_bytes!("../fonts/NotoSansHebrew-rwml-subset.ttf")
}

#[cfg(test)]
mod tests {
    const SFNT_CHECKSUM: u32 = 0xB1B0_AFBA;

    fn read_u16(bytes: &[u8], offset: usize) -> Option<u16> {
        let value = bytes.get(offset..offset.checked_add(2)?)?;
        Some(u16::from_be_bytes([value[0], value[1]]))
    }

    fn read_u32(bytes: &[u8], offset: usize) -> Option<u32> {
        let value = bytes.get(offset..offset.checked_add(4)?)?;
        Some(u32::from_be_bytes([value[0], value[1], value[2], value[3]]))
    }

    fn table<'a>(font: &'a [u8], tag: &[u8; 4]) -> Option<&'a [u8]> {
        let table_count = usize::from(read_u16(font, 4)?);
        for index in 0..table_count {
            let record = 12usize.checked_add(index.checked_mul(16)?)?;
            if font.get(record..record.checked_add(4)?)? != tag {
                continue;
            }
            let offset = usize::try_from(read_u32(font, record.checked_add(8)?)?).ok()?;
            let length = usize::try_from(read_u32(font, record.checked_add(12)?)?).ok()?;
            return font.get(offset..offset.checked_add(length)?);
        }
        None
    }

    fn format_4_contains(subtable: &[u8], codepoint: u32) -> Option<bool> {
        let codepoint = u16::try_from(codepoint).ok()?;
        let length = usize::from(read_u16(subtable, 2)?);
        let subtable = subtable.get(..length)?;
        let segment_count = usize::from(read_u16(subtable, 6)? / 2);
        let end_codes = 14usize;
        let start_codes = end_codes
            .checked_add(segment_count.checked_mul(2)?)?
            .checked_add(2)?;
        let deltas = start_codes.checked_add(segment_count.checked_mul(2)?)?;
        let range_offsets = deltas.checked_add(segment_count.checked_mul(2)?)?;

        for index in 0..segment_count {
            let entry_offset = index.checked_mul(2)?;
            let start = read_u16(subtable, start_codes.checked_add(entry_offset)?)?;
            let end = read_u16(subtable, end_codes.checked_add(entry_offset)?)?;
            if codepoint < start || codepoint > end {
                continue;
            }

            let delta = read_u16(subtable, deltas.checked_add(entry_offset)?)?;
            let range_offset_position = range_offsets.checked_add(entry_offset)?;
            let range_offset = usize::from(read_u16(subtable, range_offset_position)?);
            if range_offset == 0 {
                return Some(codepoint.wrapping_add(delta) != 0);
            }

            let codepoint_offset = usize::from(codepoint - start).checked_mul(2)?;
            let glyph_offset = range_offset_position
                .checked_add(range_offset)?
                .checked_add(codepoint_offset)?;
            let glyph = read_u16(subtable, glyph_offset)?;
            return Some(glyph != 0 && glyph.wrapping_add(delta) != 0);
        }
        Some(false)
    }

    fn format_12_contains(subtable: &[u8], codepoint: u32) -> Option<bool> {
        let length = usize::try_from(read_u32(subtable, 4)?).ok()?;
        let subtable = subtable.get(..length)?;
        let group_count = usize::try_from(read_u32(subtable, 12)?).ok()?;
        if group_count > subtable.len().saturating_sub(16) / 12 {
            return None;
        }

        for index in 0..group_count {
            let group = 16usize.checked_add(index.checked_mul(12)?)?;
            let start = read_u32(subtable, group)?;
            let end = read_u32(subtable, group.checked_add(4)?)?;
            if codepoint >= start && codepoint <= end {
                let first_glyph = read_u32(subtable, group.checked_add(8)?)?;
                return Some(first_glyph.checked_add(codepoint - start)? != 0);
            }
        }
        Some(false)
    }

    fn cmap_contains(font: &[u8], codepoint: u32) -> bool {
        let Some(cmap) = table(font, b"cmap") else {
            return false;
        };
        let Some(encoding_count) = read_u16(cmap, 2).map(usize::from) else {
            return false;
        };

        for index in 0..encoding_count {
            let Some(record) = 4usize.checked_add(index.saturating_mul(8)) else {
                continue;
            };
            let Some(offset) = read_u32(cmap, record.saturating_add(4))
                .and_then(|value| usize::try_from(value).ok())
            else {
                continue;
            };
            let Some(subtable) = cmap.get(offset..) else {
                continue;
            };
            let covered = match read_u16(subtable, 0) {
                Some(4) => format_4_contains(subtable, codepoint),
                Some(12) => format_12_contains(subtable, codepoint),
                _ => None,
            };
            if covered == Some(true) {
                return true;
            }
        }
        false
    }

    fn assert_valid_sfnt(font: &[u8]) {
        assert!(font.starts_with(&[0x00, 0x01, 0x00, 0x00]));
        assert!(read_u16(font, 4).is_some_and(|count| count > 0));
        assert!(table(font, b"cmap").is_some());
        assert_eq!(font.len() % 4, 0);

        let checksum = font.chunks_exact(4).fold(0u32, |sum, chunk| {
            sum.wrapping_add(u32::from_be_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
        });
        assert_eq!(checksum, SFNT_CHECKSUM);
    }

    #[test]
    fn noto_sans_kr_subset_bytes_are_non_empty() {
        assert!(!crate::noto_sans_kr_subset().is_empty());
    }

    #[test]
    fn noto_sans_kr_subset_starts_with_sfnt_magic() {
        assert!(crate::noto_sans_kr_subset().starts_with(&[0x00, 0x01, 0x00, 0x00]));
    }

    #[test]
    fn noto_sans_kr_subset_stays_under_package_ceiling() {
        assert!(crate::noto_sans_kr_subset().len() <= 600_000);
        assert!(crate::noto_sans_kr_subset_with_hanja().len() <= 2_200_000);
    }

    #[test]
    fn noto_sans_kr_subset_with_hanja_bytes_are_non_empty() {
        assert!(!crate::noto_sans_kr_subset_with_hanja().is_empty());
    }

    #[test]
    fn noto_sans_kr_subset_with_hanja_starts_with_sfnt_magic() {
        assert!(crate::noto_sans_kr_subset_with_hanja().starts_with(&[0x00, 0x01, 0x00, 0x00]));
    }

    #[test]
    fn rtl_subsets_are_valid_sfnt_fonts() {
        assert_valid_sfnt(crate::noto_sans_arabic_subset());
        assert_valid_sfnt(crate::noto_sans_hebrew_subset());
    }

    #[test]
    fn rtl_subsets_retain_required_shaping_tables() {
        for font in [
            crate::noto_sans_arabic_subset(),
            crate::noto_sans_hebrew_subset(),
        ] {
            assert!(table(font, b"GDEF").is_some());
            assert!(table(font, b"GPOS").is_some());
            assert!(table(font, b"GSUB").is_some());
        }
    }

    #[test]
    fn rtl_subsets_stay_under_package_ceilings() {
        assert!(crate::noto_sans_arabic_subset().len() <= 90_000);
        assert!(crate::noto_sans_hebrew_subset().len() <= 30_000);
    }

    #[test]
    fn arabic_subset_covers_representative_text() {
        let font = crate::noto_sans_arabic_subset();
        for codepoint in [
            0x0020, 0x007E, 0x060C, 0x0610, 0x061B, 0x061F, 0x0627, 0x0644, 0x064A, 0x064B, 0x0651,
            0x0660, 0x0669, 0x0670, 0x067E, 0x06D6, 0x06F0, 0x06F9, 0x0750, 0x08A0, 0x08F0, 0x200C,
            0x2014, 0x25CC,
        ] {
            assert!(cmap_contains(font, codepoint), "missing U+{codepoint:04X}");
        }
        assert!(!cmap_contains(font, 0xFB50));
    }

    #[test]
    fn hebrew_subset_covers_representative_text() {
        let font = crate::noto_sans_hebrew_subset();
        for codepoint in [
            0x0020, 0x007E, 0x0591, 0x05B0, 0x05BC, 0x05C1, 0x05C7, 0x05D0, 0x05DA, 0x05EA, 0x05EF,
            0x05F0, 0x05F3, 0x05F4, 0x200F, 0x2014, 0x25CC,
        ] {
            assert!(cmap_contains(font, codepoint), "missing U+{codepoint:04X}");
        }
    }

    #[test]
    fn ofl_license_text_is_packaged() {
        assert!(include_str!("../OFL.txt").contains("SIL OPEN FONT LICENSE Version 1.1"));
        assert!(
            include_str!("../OFL-NotoSansArabic.txt").contains("SIL OPEN FONT LICENSE Version 1.1")
        );
        assert!(
            include_str!("../OFL-NotoSansHebrew.txt").contains("SIL OPEN FONT LICENSE Version 1.1")
        );
    }
}
