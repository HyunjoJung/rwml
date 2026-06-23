# Contributing to rdoc

Thanks for your interest in improving `rdoc` — a native Rust text extractor for
legacy Microsoft Word `.doc` files.

## Ground rules

- **No `unsafe`.** The crate is `#![forbid(unsafe_code)]`. Parsing untrusted
  files must never be able to crash a host process — every byte access is
  bounds-checked and malformed input must surface as an [`Error`], not a panic.
- **Document every public item.** The crate denies `missing_docs`.
- **Keep dependencies minimal.** The crate intentionally depends only on `cfb`,
  `encoding_rs`, and `thiserror`. New dependencies need a strong justification.
- **Follow the spec.** Behaviour should be traceable to [MS-DOC] / [MS-CFB].
  Cite the relevant section in comments when implementing format details.

## Before opening a PR

Run the full local gate (all must pass clean):

```sh
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo doc --no-deps
```

## Tests

- Unit tests build minimal valid `.doc` files in memory (see `src/lib.rs`
  `tests`) so the parser can be exercised end-to-end without binary fixtures.
- When fixing a real-world file, add a focused regression test that captures the
  structural quirk (piece-table shape, encoding, control marks) rather than
  committing the binary, where possible.

## Scope

`rdoc` currently targets **plain-text extraction** at parity with POI
`WordExtractor.getText()`. Larger features (table structure, full field
semantics, additional code pages, damaged-container recovery) are tracked in the
README roadmap and very welcome.

[MS-DOC]: https://learn.microsoft.com/en-us/openspecs/office_file_formats/ms-doc/
[MS-CFB]: https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-cfb/
