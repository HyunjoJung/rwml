#![forbid(unsafe_code)]
//! Bundled OFL font subset for rdoc's PDF renderer.
//!
//! The bundled face is Noto Sans KR Regular 400 with family name `Noto Sans KR`.
//! It is a layout-derived subset, not the full upstream font. Coverage includes
//! the KS X 1001 wansung set (2,350 Hangul syllables), Hangul compatibility
//! jamo, Basic Latin, Latin-1, and common punctuation.

/// Return the bundled Noto Sans KR Regular subset bytes.
pub fn noto_sans_kr_subset() -> &'static [u8] {
    include_bytes!("../fonts/NotoSansKR-rdoc-subset.ttf")
}

#[cfg(test)]
mod tests {
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
    }

    #[test]
    fn ofl_license_text_is_packaged() {
        assert!(include_str!("../OFL.txt").contains("SIL OPEN FONT LICENSE Version 1.1"));
    }
}
