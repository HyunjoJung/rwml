//! Render any Word file (legacy `.doc` or modern `.docx`) to a native A4 PDF
//! through the shared document model. Requires the `render` feature.
//!
//! ```text
//! cargo run --example to_pdf --features render -- input.doc [output.pdf]
//! cargo run --example to_pdf --features render -- input.doc output.pdf --report-json report.json
//! cargo run --example to_pdf --features render -- input.doc output.pdf --fixed-fonts
//! cargo run --example to_pdf --features render -- input.doc output.pdf --font regular.ttf --font fallback.otf
//! ```

use std::ffi::OsString;
use std::fs::File;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const USAGE: &str = "usage: to_pdf <input.doc|.docx> [output.pdf] [--report-json report.json] [--fixed-fonts | --font file ...]";
const MAX_FONT_FILES: usize = 128;
const MAX_FONT_BYTES: u64 = 64 * 1024 * 1024;
const MAX_TOTAL_FONT_BYTES: u64 = 256 * 1024 * 1024;

#[derive(Debug)]
struct Options {
    input: PathBuf,
    output: PathBuf,
    report_json: Option<PathBuf>,
    fixed_fonts: bool,
    font_paths: Vec<PathBuf>,
}

fn option_path(args: &mut impl Iterator<Item = OsString>, option: &str) -> Result<PathBuf, String> {
    let path = args
        .next()
        .filter(|value| !value.is_empty())
        .filter(|value| !value.to_str().is_some_and(|value| value.starts_with('-')))
        .ok_or_else(|| format!("{option} requires a path"))?;
    Ok(PathBuf::from(path))
}

fn parse_args(args: impl IntoIterator<Item = OsString>) -> Result<Options, String> {
    let mut args = args.into_iter();
    let mut positional = Vec::new();
    let mut report_json = None;
    let mut fixed_fonts = false;
    let mut font_paths = Vec::new();
    let mut paths_only = false;
    while let Some(arg) = args.next() {
        if paths_only {
            positional.push(PathBuf::from(arg));
            continue;
        }
        match arg.to_str() {
            Some("--") => paths_only = true,
            Some("--report-json") => {
                if report_json.is_some() {
                    return Err("--report-json may only be supplied once".into());
                }
                report_json = Some(option_path(&mut args, "--report-json")?);
            }
            Some("--fixed-fonts") => {
                if fixed_fonts {
                    return Err("--fixed-fonts may only be supplied once".into());
                }
                fixed_fonts = true;
            }
            Some("--font") => {
                if font_paths.len() == MAX_FONT_FILES {
                    return Err(format!("at most {MAX_FONT_FILES} font files are allowed"));
                }
                font_paths.push(option_path(&mut args, "--font")?);
            }
            Some(value) if value.starts_with('-') => {
                return Err(format!("unknown option: {value}"));
            }
            _ => positional.push(PathBuf::from(arg)),
        }
    }
    if positional.is_empty() || positional.len() > 2 {
        return Err("expected one input and an optional output path".into());
    }
    if fixed_fonts && !font_paths.is_empty() {
        return Err("--fixed-fonts cannot be combined with --font".into());
    }
    let input = positional[0].clone();
    let output = positional.get(1).cloned().unwrap_or_else(|| {
        let mut p = input.clone();
        p.set_extension("pdf");
        p
    });
    Ok(Options {
        input,
        output,
        report_json,
        fixed_fonts,
        font_paths,
    })
}

fn read_font_bytes(reader: impl Read, limit: u64) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader.take(limit + 1).read_to_end(&mut bytes)?;
    if bytes.is_empty() || bytes.len() as u64 > limit {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "font is empty or exceeds the byte limit",
        ));
    }
    Ok(bytes)
}

fn read_font_file(path: &Path, limit: u64) -> io::Result<Vec<u8>> {
    if !path.metadata()?.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "font must be a regular file",
        ));
    }
    let file = File::open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > limit {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "font must be a nonempty regular file within the byte limit",
        ));
    }
    read_font_bytes(file, limit)
}

fn load_font_files(
    paths: &[PathBuf],
    file_limit: u64,
    total_limit: u64,
) -> Result<Vec<Vec<u8>>, String> {
    if paths.is_empty() || paths.len() > MAX_FONT_FILES {
        return Err(format!("expected 1 to {MAX_FONT_FILES} font files"));
    }
    let mut remaining = total_limit.min(MAX_TOTAL_FONT_BYTES);
    let mut fonts = Vec::new();
    for path in paths {
        let limit = file_limit.min(MAX_FONT_BYTES).min(remaining);
        let bytes = read_font_file(path, limit)
            .map_err(|error| format!("read font {}: {error}", path.display()))?;
        remaining -= bytes.len() as u64;
        fonts.push(bytes);
    }
    Ok(fonts)
}

fn main() -> ExitCode {
    let options = match parse_args(std::env::args_os().skip(1)) {
        Ok(options) => options,
        Err(error) => {
            eprintln!("{error}\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    let fixed_fonts = if options.fixed_fonts {
        Some(vec![
            rwml_fonts::noto_sans_kr_subset_with_hanja().to_vec(),
            rwml_fonts::noto_sans_arabic_subset().to_vec(),
            rwml_fonts::noto_sans_hebrew_subset().to_vec(),
        ])
    } else if options.font_paths.is_empty() {
        None
    } else {
        match load_font_files(&options.font_paths, MAX_FONT_BYTES, MAX_TOTAL_FONT_BYTES) {
            Ok(fonts) => Some(fonts),
            Err(error) => {
                eprintln!("{error}");
                return ExitCode::FAILURE;
            }
        }
    };
    let input = options.input.display();
    let bytes = match std::fs::read(&options.input) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("read {input}: {e}");
            return ExitCode::FAILURE;
        }
    };
    let doc = match rwml::Document::open(&bytes) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("parse {input}: {e}");
            return ExitCode::FAILURE;
        }
    };
    let (pdf, report) = if options.report_json.is_some() {
        let rendered = match &fixed_fonts {
            Some(fonts) => doc.try_to_pdf_with_fixed_fonts_and_report(fonts),
            None => doc.try_to_pdf_with_report(),
        };
        match rendered {
            Ok(rendered) => (rendered.pdf, Some(rendered.report)),
            Err(e) => {
                eprintln!("render {input}: {e}");
                return ExitCode::FAILURE;
            }
        }
    } else {
        let rendered = match &fixed_fonts {
            Some(fonts) => doc
                .try_to_pdf_with_fixed_fonts_and_report(fonts)
                .map(|rendered| rendered.pdf),
            None => doc.try_to_pdf(),
        };
        match rendered {
            Ok(pdf) => (pdf, None),
            Err(e) => {
                eprintln!("render {input}: {e}");
                return ExitCode::FAILURE;
            }
        }
    };
    let out = options.output;
    if let Err(e) = std::fs::write(&out, &pdf) {
        eprintln!("write {}: {e}", out.display());
        return ExitCode::FAILURE;
    }
    if let (Some(path), Some(report)) = (options.report_json, report) {
        if let Err(e) = std::fs::write(&path, report.to_json()) {
            eprintln!("write {}: {e}", path.display());
            return ExitCode::FAILURE;
        }
    }
    eprintln!("wrote {} ({} bytes)", out.display(), pdf.len());
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn parse(args: &[&str]) -> Result<Options, String> {
        parse_args(args.iter().map(OsString::from))
    }

    fn font_path(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("rwml-fonts/fonts")
            .join(name)
    }

    #[test]
    fn default_mode_and_output_are_unchanged() {
        let options = parse(&["input.docx"]).unwrap();
        assert_eq!(options.input, Path::new("input.docx"));
        assert_eq!(options.output, Path::new("input.pdf"));
        assert!(options.report_json.is_none());
        assert!(!options.fixed_fonts);
        assert!(options.font_paths.is_empty());
    }

    #[test]
    fn bundled_mode_and_report_are_preserved() {
        let options = parse(&[
            "--fixed-fonts",
            "input.doc",
            "output.pdf",
            "--report-json",
            "report.json",
        ])
        .unwrap();
        assert!(options.fixed_fonts);
        assert!(options.font_paths.is_empty());
        assert_eq!(options.output, Path::new("output.pdf"));
        assert_eq!(options.report_json.unwrap(), Path::new("report.json"));
    }

    #[test]
    fn explicit_fonts_preserve_order_and_report() {
        let options = parse(&[
            "--font",
            "regular.ttf",
            "input.docx",
            "output.pdf",
            "--font",
            "fallback.otf",
            "--report-json",
            "report.json",
        ])
        .unwrap();
        assert!(!options.fixed_fonts);
        assert_eq!(
            options.font_paths,
            vec![PathBuf::from("regular.ttf"), PathBuf::from("fallback.otf")]
        );
        assert_eq!(options.report_json.unwrap(), Path::new("report.json"));
    }

    #[test]
    fn invalid_and_ambiguous_arguments_are_errors() {
        for args in [
            vec![],
            vec!["input.docx", "output.pdf", "extra.pdf"],
            vec!["input.docx", "--unknown"],
            vec!["input.docx", "--font"],
            vec!["input.docx", "--font", ""],
            vec!["input.docx", "--font", "--fixed-fonts"],
            vec!["input.docx", "--report-json"],
            vec!["input.docx", "--report-json", "--fixed-fonts"],
            vec!["input.docx", "--fixed-fonts", "--fixed-fonts"],
            vec!["input.docx", "--fixed-fonts", "--font", "a.ttf"],
            vec!["input.docx", "--font", "a.ttf", "--fixed-fonts"],
            vec![
                "input.docx",
                "--report-json",
                "a.json",
                "--report-json",
                "b.json",
            ],
        ] {
            assert!(parse(&args).is_err(), "accepted {args:?}");
        }
    }

    #[test]
    fn option_terminator_allows_dash_prefixed_paths() {
        let options = parse(&["--", "-input.docx", "-output.pdf"]).unwrap();
        assert_eq!(options.input, Path::new("-input.docx"));
        assert_eq!(options.output, Path::new("-output.pdf"));
        assert!(parse(&["--", "a", "b", "c"]).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn paths_need_not_be_utf8() {
        use std::os::unix::ffi::OsStringExt;
        let path = OsString::from_vec(b"font\xff.ttf".to_vec());
        let options = parse_args([
            OsString::from("input.docx"),
            OsString::from("--font"),
            path.clone(),
        ])
        .unwrap();
        assert_eq!(options.font_paths, vec![PathBuf::from(path)]);
    }

    #[test]
    fn explicit_font_count_is_bounded() {
        let mut args = vec!["input.docx"];
        for _ in 0..MAX_FONT_FILES {
            args.extend(["--font", "a.ttf"]);
        }
        assert_eq!(parse(&args).unwrap().font_paths.len(), MAX_FONT_FILES);
        args.extend(["--font", "a.ttf"]);
        assert!(parse(&args).is_err());
    }

    #[test]
    fn font_reader_rejects_empty_and_over_limit_streams() {
        assert!(read_font_bytes(Cursor::new([]), 8).is_err());
        assert!(read_font_bytes(Cursor::new([1; 9]), 8).is_err());
        assert_eq!(read_font_bytes(Cursor::new([1; 8]), 8).unwrap(), [1; 8]);
    }

    #[test]
    fn font_reader_does_not_read_past_the_limit_probe() {
        let mut reader = Cursor::new([1; 100]);
        assert!(read_font_bytes(&mut reader, 8).is_err());
        assert_eq!(reader.position(), 9);
    }

    #[test]
    fn explicit_files_load_exact_payloads_in_order() {
        let kr = font_path("NotoSansKR-rwml-subset-full.ttf");
        let arabic = font_path("NotoSansArabic-rwml-subset.ttf");
        let fonts = load_font_files(&[kr, arabic], MAX_FONT_BYTES, MAX_TOTAL_FONT_BYTES).unwrap();
        assert_eq!(fonts[0], rwml_fonts::noto_sans_kr_subset_with_hanja());
        assert_eq!(fonts[1], rwml_fonts::noto_sans_arabic_subset());
    }

    #[test]
    fn missing_and_non_file_font_paths_fail() {
        let missing = font_path("missing-test-font.ttf");
        assert!(!missing.exists());
        for path in [missing, PathBuf::from(env!("CARGO_MANIFEST_DIR"))] {
            assert!(load_font_files(&[path], MAX_FONT_BYTES, MAX_TOTAL_FONT_BYTES).is_err());
        }
    }

    #[test]
    fn file_and_total_font_bytes_are_bounded() {
        let path = font_path("NotoSansKR-rwml-subset-full.ttf");
        let size = path.metadata().unwrap().len();
        let paths = [path.clone(), path];
        assert!(load_font_files(&paths[..1], size - 1, size).is_err());
        assert!(load_font_files(&paths, size, size * 2 - 1).is_err());
        assert_eq!(load_font_files(&paths, size, size * 2).unwrap().len(), 2);
    }

    #[test]
    fn loader_rejects_empty_or_excessive_file_lists() {
        assert!(load_font_files(&[], MAX_FONT_BYTES, MAX_TOTAL_FONT_BYTES).is_err());
        assert!(load_font_files(
            &vec![PathBuf::from("unused.ttf"); MAX_FONT_FILES + 1],
            MAX_FONT_BYTES,
            MAX_TOTAL_FONT_BYTES,
        )
        .is_err());
    }
}
