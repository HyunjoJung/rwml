# Microsoft Word note-numbering probe

`scripts/word_note_numbering_capture.py` captures the unchanged
`pilot-notes-numbering` input from `render-pilot-v1.json` twice. This separate
one-document profile leaves the 48-document unequal-table and three-document
pagination contracts intact. It records Word's actual labels without requiring
either the native renderer's or another producer's numbering result.

The 5,856-byte input has SHA-256
`e3c2192b845dd27396fbd9e9709915578736e2d12433710a9a4e3be66dca8092`.
Its document settings request `lowerLetter` footnotes starting at 3 and
`lowerRoman` endnotes starting at 2. It contains two footnote references and one
endnote reference, corresponding to the labels `c`, `d`, and `ii` in those
settings. The original styles already specify Noto Sans 10pt in all four script
slots. The generator reconstructs the exact locked package without rewriting
numbering, styles, or any other part. No corpus lock is refreshed.

Generate the input without Word:

```sh
python scripts/word_note_numbering_capture.py generate --output target/word-note-input
```

Capture requires native Windows, desktop Word, Windows PowerShell, Git, a clean
checkout, PyMuPDF 1.28.2, and the exact installed font from `word-font-lock.json`.
Save and close existing Word work before capture. Use a fresh output directory:

```powershell
python scripts/word_note_numbering_capture.py capture `
  --output target/word-note-capture `
  --font "<installed-NotoSans-Regular.ttf>" `
  --source-revision (git rev-parse HEAD)
python scripts/word_note_numbering_capture.py verify --output target/word-note-capture
```

The two exports retain both PDFs, the exact input, source/harness identities,
Word executable/runtime/export metadata, the locked font identity, embedded
font-program hashes, text spans and baselines, page geometry, and 110-DPI raster
hashes. A successful `CAPTURE.json` requires identical producer identities and
page/text/font/raster observations across both runs, followed by independent PDF
reconstruction. Verification reconstructs every retained observation and receipt
field again. Raw PDF and embedded subset-program bytes may differ and remain
recorded. Embedded fonts must be named `NotoSans-Regular`; this diagnostic does
not prove font-outline equivalence or general layout parity.

The backend opens inputs read-only and uses the same fixed export options as
the other profiles. It does not change global Office security settings or the
user's PowerShell profile. If capture fails, preserve its partial output; a
missing receipt means it did not complete. Inspect ownership before cleaning up
any remaining Word process, and never terminate unrelated Word work. Transient
failed `job.json` files contain local paths and must not be published.
