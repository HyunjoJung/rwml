---
name: Bug report
about: A read, write, edit, render, CLI, WASM, preservation, panic, or crash problem
title: ''
labels: bug
assignees: ''
---

## Before filing

This issue is public. **Do not attach confidential, private, or proprietary
documents, personal data, credentials, or files you cannot redistribute.** Use
a synthetic reproducer or clearly licensed public fixture. If this may be a
security vulnerability, stop here and use
[GitHub private vulnerability reporting](https://github.com/HyunjoJung/rwml/security/advisories/new).

Search open and closed issues before submitting a new report.

## What happened

<!-- Describe the failure and its user-visible impact. -->

## Affected path

- [ ] Read or export legacy `.doc`
- [ ] Read or export `.docx`
- [ ] Write a fresh `.docx`
- [ ] Edit or preserve an existing `.docx` package
- [ ] Render a native PDF preview
- [ ] CLI
- [ ] WASM or browser example
- [ ] Other public API

## Reproduction

1. Minimal Rust code or CLI command:
2. Minimal synthetic input or a description of the relevant document/package
   structure:
3. Error, panic, incorrect output, or changed package parts:

<!-- Attach a fixture only when it is safe and licensed for public redistribution. -->

## Expected behavior

<!-- State the expected text, model, package preservation, file output, or preview. -->

## Actual behavior

<!-- State what rwml produced. Include a short error or diagnostic excerpt. -->

## Compatibility boundary

<!--
Document producer/version, .doc or .docx, relevant view or feature, and comparison
with Word, LibreOffice, Apache POI, python-docx, or another oracle when applicable.
Call out any case that should remain unsupported.
-->

## Environment

- `rwml` version:
- Rust version (`rustc --version`):
- OS:
- Enabled Cargo features:
- Document producer/version, if known:

## Additional public context

<!-- Link public specifications, minimized fixtures, or related issues. -->
