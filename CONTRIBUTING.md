# Contributing to rwml

Thanks for your interest in improving `rwml` — a native Rust toolkit for
Microsoft Word documents (**read**, **write**, **edit**, **render**) covering
both legacy `.doc` (Word 97–2003 binary, [MS-DOC]) and modern `.docx`
(OOXML WordprocessingML, [ECMA-376]).

## Ground rules

- **No `unsafe`.** The crate is `#![forbid(unsafe_code)]`. Parsing untrusted
  files must never crash a host process — every byte access is bounds-checked,
  and malformed or hostile input must surface as an `Error` value or read-only
  diagnostics, never a panic.
- **Preserve before interpreting.** `.docx` edits mutate live WordprocessingML
  element trees and reserialize **only** touched parts; a no-op open→save is
  part-payload byte-stable. Don't regress package preservation.
- **Deterministic output.** Field evaluation and rendering must be deterministic
  and source-order stable.
- **Document every public item.** The crate denies `missing_docs`.
- **Keep dependencies minimal.** The legacy-`.doc` core depends only on `cfb`,
  `encoding_rs`, and `thiserror`; the default `docx` feature adds `zip`,
  `quick-xml`, and `flate2`, and the opt-in `render` feature adds `parley` and
  `krilla` (raising the MSRV to 1.92). New dependencies need a strong
  justification.
- **Follow the spec.** `.doc` behaviour should trace to [MS-DOC] / [MS-CFB] and
  `.docx` behaviour to [ECMA-376]; cite the relevant section in comments when
  implementing format details.

## Pull request workflow

`main` is protected, including for maintainers. Make changes on a topic branch,
rebase it on the current `origin/main`, and open a pull request; do not push
implementation or documentation commits directly to `main`. Pull requests must
be up to date and pass the required CI jobs before squash or rebase merge.

Keep linked issues open until the implementing pull request merges. Use
`Closes #N` in the pull request body when the merged change fully satisfies an
issue, and describe any intentionally deferred ceiling instead of closing it
early.

## Before opening a PR

Run the local gate (all must pass clean). The default gate covers the `docx`
feature:

```sh
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
cargo doc --no-deps
```

If you touch the renderer, also run the `render`-gated build (MSRV 1.92):

```sh
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --features render
```

If you change the checked-in public corpus, also verify deterministic
regeneration and the release-mode corpus smoke:

```sh
python3 scripts/gen_public_corpus.py --check
cargo test --release --test performance --locked -- --ignored --nocapture
```

## Release validation

Release publishing is tag-bound. Create the version-matching `v*` tag only
after the protected-branch CI result is green; the release workflow rejects
branch dispatches, verifies that the tag points at the exact workflow revision,
and compares the package against the published `v0.1.1` API baseline. It then
generates strict render and extraction evidence, packages both crates, records
checksums, and attaches the exact packages and evidence to the GitHub Release.

The local evidence commands mirror the workflow:

```sh
python3 scripts/render_validate.py --json --manifest corpus/public/RENDER_MANIFEST.tsv \
  --recall-min 0.97 --min-mean-recall 0.90 --max-skipped 0 --verify-oracle \
  > dist/render-validation.json
cargo build --release --example extract --locked
python3 scripts/bench_vs_mature.py --corpus corpus/public/benchmark --json \
  --version "$VERSION" --git-rev "$(git rev-parse HEAD)" \
  --min-poi-recall-mean 0.95 --min-poi-f1-mean 0.95 --max-errors 0 --min-scored 1 \
  --output dist/extract-benchmark.json
python3 scripts/public_hygiene_audit.py --json > dist/public-hygiene.json
python3 scripts/release_manifest.py --version "$VERSION" \
  --git-rev "$(git rev-parse HEAD)" --release-policy public-release \
  --enforce-policy-inputs --hygiene-report dist/public-hygiene.json \
  --validation-report dist/render-validation.json \
  --benchmark-report dist/extract-benchmark.json \
  --corpus-manifest corpus/public/MANIFEST.tsv \
  --corpus-manifest corpus/public/RENDER_MANIFEST.tsv \
  --output dist/rwml-release-manifest.json \
  target/package/rwml-"$VERSION".crate rwml-fonts/target/package/rwml-fonts-"$VERSION".crate
```

## Tests

- `.doc` unit tests build minimal valid OLE2/`.doc` files in memory so the
  parser is exercised end-to-end without binary fixtures; `.docx` tests drive
  the public API (`Document::open`, `fields()`, `write_docx`, …) and the
  synthetic public corpus under `corpus/public/`.
- Use TDD (a red test first) for behaviour changes. When fixing a real-world
  file, add a focused regression test that captures the structural quirk (piece
  table shape, encoding, control marks, element layout) rather than committing a
  private or binary document. Use only synthetic or clearly public corpus files.

## Scope

`rwml` reads, writes, edits, and renders both `.doc` and `.docx` into one shared
`DocModel`. Most remaining work is deeper compatibility — field semantics,
layout/rendering fidelity, additional fixtures, and validation depth — rather
than new top-level APIs; see the roadmap in the [README](README.md#roadmap).
Preview-grade rendering is not a LibreOffice/Word replacement, and layout-exact
pagination is intentionally out of scope.

[MS-DOC]: https://learn.microsoft.com/en-us/openspecs/office_file_formats/ms-doc/
[MS-CFB]: https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-cfb/
[ECMA-376]: https://ecma-international.org/publications-and-standards/standards/ecma-376/
