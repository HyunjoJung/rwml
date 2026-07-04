---
name: Bug report
about: Wrong/garbled text, a parse failure, a panic, or a crash
title: ''
labels: bug
assignees: ''
---

## What happened

A clear, concise description of the problem.

## Reproduction

1. The `.doc` file that triggers it (attach it if you can — even a minimal one)
2. The code / CLI you ran (e.g. `rwml::extract_text(&bytes)`)
3. The error, panic message, or wrong output

## Expected vs actual

- **Expected:** what the text should be (e.g. what Word / Apache POI produces)
- **Actual:** what `rwml` produced

## Environment

- `rwml` version:
- Rust version (`rustc --version`):
- OS:

## Additional context

Document origin (Word version, language), encryption, fast-save, etc. — anything
that helps narrow it down.
