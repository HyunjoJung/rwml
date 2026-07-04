#![forbid(unsafe_code)]
//! Bundled OFL font subset for rwml's PDF renderer.
//!
//! The bundled faces are Noto Sans KR Regular 400 with family name `Noto Sans
//! KR`. They are layout-derived subsets, not the full upstream font.
//!
//! `noto_sans_kr_subset` is the slim subset: KS X 1001 wansung Hangul (2,350
//! syllables), Hangul compatibility jamo, Basic Latin, Latin-1, and common
//! punctuation.
//!
//! `noto_sans_kr_subset_with_hanja` adds KS X 1001 hanja coverage. It maps
//! 4,885 of the 4,888 KS X 1001 hanja characters; 3 compatibility ideographs
//! are absent from upstream Noto Sans KR itself.

/// Return the bundled Noto Sans KR Regular subset bytes.
pub fn noto_sans_kr_subset() -> &'static [u8] {
    include_bytes!("../fonts/NotoSansKR-rwml-subset.ttf")
}

/// Return the bundled Noto Sans KR Regular subset bytes with KS X 1001 hanja.
pub fn noto_sans_kr_subset_with_hanja() -> &'static [u8] {
    include_bytes!("../fonts/NotoSansKR-rwml-subset-full.ttf")
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
    fn ofl_license_text_is_packaged() {
        assert!(include_str!("../OFL.txt").contains("SIL OPEN FONT LICENSE Version 1.1"));
    }
}
