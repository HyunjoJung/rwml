use crate::annotation::FieldNumberFormat;
use crate::model::PageNumberFormat as ModelPageNumberFormat;
use crate::numfmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PageNumberFormat {
    Arabic,
    ArabicDash,
    DecimalZero,
    DecimalFullWidth,
    DecimalHalfWidth,
    DecimalFullWidth2,
    DecimalEnclosedCircle,
    DecimalEnclosedFullstop,
    DecimalEnclosedParen,
    Ganada,
    Chosung,
    KoreanDigital,
    KoreanCounting,
    KoreanLegal,
    KoreanDigital2,
    AlphabeticLower,
    AlphabeticUpper,
    RomanLower,
    RomanUpper,
    Ordinal,
    CardText,
    OrdText,
    Hex,
    DollarText,
}

impl From<ModelPageNumberFormat> for PageNumberFormat {
    fn from(format: ModelPageNumberFormat) -> Self {
        match format {
            ModelPageNumberFormat::Decimal => PageNumberFormat::Arabic,
            ModelPageNumberFormat::DecimalZero => PageNumberFormat::DecimalZero,
            ModelPageNumberFormat::NumberInDash => PageNumberFormat::ArabicDash,
            ModelPageNumberFormat::DecimalFullWidth => PageNumberFormat::DecimalFullWidth,
            ModelPageNumberFormat::DecimalHalfWidth => PageNumberFormat::DecimalHalfWidth,
            ModelPageNumberFormat::DecimalFullWidth2 => PageNumberFormat::DecimalFullWidth2,
            ModelPageNumberFormat::DecimalEnclosedCircle => PageNumberFormat::DecimalEnclosedCircle,
            ModelPageNumberFormat::DecimalEnclosedFullstop => {
                PageNumberFormat::DecimalEnclosedFullstop
            }
            ModelPageNumberFormat::DecimalEnclosedParen => PageNumberFormat::DecimalEnclosedParen,
            ModelPageNumberFormat::Ganada => PageNumberFormat::Ganada,
            ModelPageNumberFormat::Chosung => PageNumberFormat::Chosung,
            ModelPageNumberFormat::KoreanDigital => PageNumberFormat::KoreanDigital,
            ModelPageNumberFormat::KoreanCounting => PageNumberFormat::KoreanCounting,
            ModelPageNumberFormat::KoreanLegal => PageNumberFormat::KoreanLegal,
            ModelPageNumberFormat::KoreanDigital2 => PageNumberFormat::KoreanDigital2,
            ModelPageNumberFormat::LowerLetter => PageNumberFormat::AlphabeticLower,
            ModelPageNumberFormat::UpperLetter => PageNumberFormat::AlphabeticUpper,
            ModelPageNumberFormat::LowerRoman => PageNumberFormat::RomanLower,
            ModelPageNumberFormat::UpperRoman => PageNumberFormat::RomanUpper,
            ModelPageNumberFormat::Ordinal => PageNumberFormat::Ordinal,
            ModelPageNumberFormat::CardinalText => PageNumberFormat::CardText,
            ModelPageNumberFormat::OrdinalText => PageNumberFormat::OrdText,
        }
    }
}

impl From<FieldNumberFormat> for PageNumberFormat {
    fn from(format: FieldNumberFormat) -> Self {
        match format {
            FieldNumberFormat::Arabic => PageNumberFormat::Arabic,
            FieldNumberFormat::ArabicDash => PageNumberFormat::ArabicDash,
            FieldNumberFormat::AlphabeticLower => PageNumberFormat::AlphabeticLower,
            FieldNumberFormat::AlphabeticUpper => PageNumberFormat::AlphabeticUpper,
            FieldNumberFormat::RomanLower => PageNumberFormat::RomanLower,
            FieldNumberFormat::RomanUpper => PageNumberFormat::RomanUpper,
            FieldNumberFormat::Ordinal => PageNumberFormat::Ordinal,
            FieldNumberFormat::CardText => PageNumberFormat::CardText,
            FieldNumberFormat::OrdText => PageNumberFormat::OrdText,
            FieldNumberFormat::Hex => PageNumberFormat::Hex,
            FieldNumberFormat::DollarText => PageNumberFormat::DollarText,
        }
    }
}

pub(crate) fn format_page_number(page: usize, format: Option<PageNumberFormat>) -> Option<String> {
    match format.unwrap_or(PageNumberFormat::Arabic) {
        PageNumberFormat::Arabic => Some(page.to_string()),
        PageNumberFormat::ArabicDash => Some(format!("- {page} -")),
        PageNumberFormat::DecimalZero => Some(format!("{page:02}")),
        PageNumberFormat::DecimalFullWidth => u32::try_from(page)
            .ok()
            .map(|page| numfmt::format(page, 0x0E)),
        PageNumberFormat::DecimalHalfWidth => u32::try_from(page)
            .ok()
            .map(|page| numfmt::format(page, 0x0F)),
        PageNumberFormat::DecimalFullWidth2 => u32::try_from(page)
            .ok()
            .map(|page| numfmt::format(page, 0x13)),
        PageNumberFormat::DecimalEnclosedCircle => u32::try_from(page)
            .ok()
            .map(|page| numfmt::format(page, 0x12)),
        PageNumberFormat::DecimalEnclosedFullstop => u32::try_from(page)
            .ok()
            .map(|page| numfmt::format(page, 0x1A)),
        PageNumberFormat::DecimalEnclosedParen => u32::try_from(page)
            .ok()
            .map(|page| numfmt::format(page, 0x1B)),
        PageNumberFormat::Ganada => u32::try_from(page)
            .ok()
            .map(|page| numfmt::format(page, 0x18)),
        PageNumberFormat::Chosung => u32::try_from(page)
            .ok()
            .map(|page| numfmt::format(page, 0x19)),
        PageNumberFormat::KoreanDigital => u32::try_from(page)
            .ok()
            .map(|page| numfmt::format(page, 0x29)),
        PageNumberFormat::KoreanCounting => u32::try_from(page)
            .ok()
            .map(|page| numfmt::format(page, 0x2A)),
        PageNumberFormat::KoreanLegal => u32::try_from(page)
            .ok()
            .map(|page| numfmt::format(page, 0x2B)),
        PageNumberFormat::KoreanDigital2 => u32::try_from(page)
            .ok()
            .map(|page| numfmt::format(page, 0x2C)),
        PageNumberFormat::AlphabeticLower => alphabetic_page_number(page, false),
        PageNumberFormat::AlphabeticUpper => alphabetic_page_number(page, true),
        PageNumberFormat::RomanLower => roman_page_number(page).map(|value| value.to_lowercase()),
        PageNumberFormat::RomanUpper => roman_page_number(page),
        PageNumberFormat::Ordinal => Some(ordinal_page_number(page)),
        PageNumberFormat::CardText => cardinal_page_number_text(page),
        PageNumberFormat::OrdText => ordinal_page_number_text(page),
        PageNumberFormat::Hex => Some(format!("{page:X}")),
        PageNumberFormat::DollarText => dollar_page_number_text(page),
    }
}

fn alphabetic_page_number(mut page: usize, uppercase: bool) -> Option<String> {
    if page == 0 {
        return None;
    }
    let base = if uppercase { b'A' } else { b'a' };
    let mut chars = Vec::new();
    while page > 0 {
        page -= 1;
        chars.push((base + (page % 26) as u8) as char);
        page /= 26;
    }
    chars.reverse();
    Some(chars.into_iter().collect())
}

fn roman_page_number(mut page: usize) -> Option<String> {
    if page == 0 || page > 3999 {
        return None;
    }
    let mut out = String::new();
    for (value, numeral) in [
        (1000, "M"),
        (900, "CM"),
        (500, "D"),
        (400, "CD"),
        (100, "C"),
        (90, "XC"),
        (50, "L"),
        (40, "XL"),
        (10, "X"),
        (9, "IX"),
        (5, "V"),
        (4, "IV"),
        (1, "I"),
    ] {
        while page >= value {
            out.push_str(numeral);
            page -= value;
        }
    }
    Some(out)
}

fn ordinal_page_number(page: usize) -> String {
    let suffix = if (11..=13).contains(&(page % 100)) {
        "th"
    } else {
        match page % 10 {
            1 => "st",
            2 => "nd",
            3 => "rd",
            _ => "th",
        }
    };
    format!("{page}{suffix}")
}

pub(crate) fn cardinal_page_number_text(page: usize) -> Option<String> {
    cardinal_number_text(page)
}

pub(crate) fn ordinal_page_number_text(page: usize) -> Option<String> {
    ordinal_number_text(page)
}

fn dollar_page_number_text(page: usize) -> Option<String> {
    Some(format!("{} and 00/100", cardinal_number_text(page)?))
}

fn cardinal_number_text(number: usize) -> Option<String> {
    if number == 0 {
        return Some("zero".to_string());
    }
    cardinal_positive_number_text(number as u64)
}

fn cardinal_positive_number_text(number: u64) -> Option<String> {
    const SCALES: &[(u64, &str)] = &[
        (1_000_000_000_000, "trillion"),
        (1_000_000_000, "billion"),
        (1_000_000, "million"),
        (1_000, "thousand"),
    ];
    if number < 20 {
        return Some(SMALL_NUMBER_WORDS[number as usize].to_string());
    }
    if number < 100 {
        let tens = number / 10;
        let rest = number % 10;
        let tens_word = TENS_NUMBER_WORDS[tens as usize];
        return Some(if rest == 0 {
            tens_word.to_string()
        } else {
            format!("{tens_word}-{}", SMALL_NUMBER_WORDS[rest as usize])
        });
    }
    if number < 1_000 {
        let hundreds = number / 100;
        let rest = number % 100;
        let prefix = format!("{} hundred", SMALL_NUMBER_WORDS[hundreds as usize]);
        return Some(if rest == 0 {
            prefix
        } else {
            format!("{prefix} {}", cardinal_positive_number_text(rest)?)
        });
    }
    for (value, name) in SCALES {
        if number >= *value {
            let major = number / *value;
            let rest = number % *value;
            let prefix = format!("{} {name}", cardinal_positive_number_text(major)?);
            return Some(if rest == 0 {
                prefix
            } else {
                format!("{prefix} {}", cardinal_positive_number_text(rest)?)
            });
        }
    }
    None
}

fn ordinal_number_text(number: usize) -> Option<String> {
    ordinal_positive_number_text(number as u64)
}

fn ordinal_positive_number_text(number: u64) -> Option<String> {
    if number < 20 {
        return Some(SMALL_ORDINAL_WORDS[number as usize].to_string());
    }
    if number < 100 {
        let tens = number / 10;
        let rest = number % 10;
        let tens_word = TENS_NUMBER_WORDS[tens as usize];
        return Some(if rest == 0 {
            TENS_ORDINAL_WORDS[tens as usize].to_string()
        } else {
            format!("{tens_word}-{}", ordinal_positive_number_text(rest)?)
        });
    }
    if number < 1_000 {
        let hundreds = number / 100;
        let rest = number % 100;
        let prefix = format!("{} hundred", SMALL_NUMBER_WORDS[hundreds as usize]);
        return Some(if rest == 0 {
            format!("{prefix}th")
        } else {
            format!("{prefix} {}", ordinal_positive_number_text(rest)?)
        });
    }
    for (value, name) in [
        (1_000_000_000_000u64, "trillion"),
        (1_000_000_000, "billion"),
        (1_000_000, "million"),
        (1_000, "thousand"),
    ] {
        if number >= value {
            let major = number / value;
            let rest = number % value;
            let prefix = cardinal_positive_number_text(major)?;
            return Some(if rest == 0 {
                format!("{prefix} {name}th")
            } else {
                format!("{prefix} {name} {}", ordinal_positive_number_text(rest)?)
            });
        }
    }
    None
}

const SMALL_NUMBER_WORDS: [&str; 20] = [
    "zero",
    "one",
    "two",
    "three",
    "four",
    "five",
    "six",
    "seven",
    "eight",
    "nine",
    "ten",
    "eleven",
    "twelve",
    "thirteen",
    "fourteen",
    "fifteen",
    "sixteen",
    "seventeen",
    "eighteen",
    "nineteen",
];

const SMALL_ORDINAL_WORDS: [&str; 20] = [
    "zeroth",
    "first",
    "second",
    "third",
    "fourth",
    "fifth",
    "sixth",
    "seventh",
    "eighth",
    "ninth",
    "tenth",
    "eleventh",
    "twelfth",
    "thirteenth",
    "fourteenth",
    "fifteenth",
    "sixteenth",
    "seventeenth",
    "eighteenth",
    "nineteenth",
];

const TENS_NUMBER_WORDS: [&str; 10] = [
    "", "", "twenty", "thirty", "forty", "fifty", "sixty", "seventy", "eighty", "ninety",
];

const TENS_ORDINAL_WORDS: [&str; 10] = [
    "",
    "",
    "twentieth",
    "thirtieth",
    "fortieth",
    "fiftieth",
    "sixtieth",
    "seventieth",
    "eightieth",
    "ninetieth",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_and_field_formats_share_one_formatter() {
        assert_eq!(
            format_page_number(7, Some(ModelPageNumberFormat::UpperRoman.into())).as_deref(),
            Some("VII")
        );
        assert_eq!(
            format_page_number(7, Some(FieldNumberFormat::CardText.into())).as_deref(),
            Some("seven")
        );
        assert_eq!(
            format_page_number(
                12,
                Some(ModelPageNumberFormat::DecimalEnclosedCircle.into())
            )
            .as_deref(),
            Some("⑫")
        );
    }
}
