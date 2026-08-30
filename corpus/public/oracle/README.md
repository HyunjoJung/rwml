# Generated render-oracle campaigns

This directory stores public identity locks for diagnostic campaigns whose input
documents are generated on demand. The generated DOCX files do not join the ordinary
public release corpus until their external-oracle expectations and release value have
been reviewed independently.

`unequal-table-v1.json` binds a 48-case factorial campaign across four physical column
layouts, three table-width policies, two row-fragment classes, and two continuation
handoffs. It records the generator SHA-256 plus every output path, byte length, SHA-256,
scenario label, and expected native page count. No private document content or planning
artifact is involved.

Materialize and validate the exact campaign under the ignored `target/` directory:

```sh
python3 scripts/generate_unequal_table_oracle.py
python3 scripts/generate_unequal_table_oracle.py --check
python3 scripts/render_oracle_contract.py \
  target/render-oracle/unequal-table-v1/RENDER_ORACLE.json
```

The generated `RENDER_ORACLE.json` uses the same bounded, path-neutral corpus contract
as the release render campaign. The lock is an input identity contract, not a claim of
Word parity and not a release threshold.

## LibreOffice regression font lock

`libreoffice-font-lock.json` pins eight LibreOffice-bundled Noto Sans, Noto Sans
Arabic, and Noto Sans Hebrew style files to official upstream release archives,
commits, members, byte lengths, SHA-256 digests, PostScript names, and SFNT revisions.
The local regression profile maps the observed Office and platform fallback families
to those locked families. Before a strict campaign, `scripts/render_validate.py`
locates the fonts relative to the LibreOffice installation and verifies their exact
bytes without retaining installation paths. Every primary and repeat reference PDF is
then required to embed only a locked PostScript name at its locked SFNT revision.

This is a regression-oracle environment lock, not a claim that LibreOffice pagination
is authoritative for Word. Page-count and geometry differences remain explicit
diagnostic measurements unless separately accepted from Microsoft Word evidence.

`scripts/table_oracle_topology.py` reduces a complete directory of `<case-id>.pdf`
outputs to content-safe structural evidence. Its producer metadata input contains only
the producer name, canonical mode, version, identity SHA-256, and platform identity.
The report retains exact input/PDF identities, page geometry, the campaign's synthetic
cell-token boxes, normalized axis-aligned table borders, and consecutive continuation
segments; it never retains arbitrary document text or local paths. Capture reports can
then be compared without defining or weakening a fidelity threshold:

```sh
python3 scripts/table_oracle_topology.py extract \
  --manifest target/render-oracle/unequal-table-v1/RENDER_ORACLE.json \
  --pdf-dir <complete-pdf-directory> \
  --producer-metadata <producer-identity.json> \
  --source-revision <full-git-sha> \
  --output <capture.json>

python3 scripts/table_oracle_topology.py compare \
  --manifest target/render-oracle/unequal-table-v1/RENDER_ORACLE.json \
  --candidate <candidate-capture.json> \
  --reference <oracle-capture.json> \
  --output <comparison.json>
```

Use `--require-normalized-exact` only when comparing two independent captures from the
same producer. Cross-producer comparisons are diagnostic until authoritative Word
evidence is reviewed.

## Microsoft Word diagnostic capture

`word-font-lock.json` identifies the exact Noto Sans Regular font used by this
campaign. It pins the official
[`NotoSans-v2.015`](https://github.com/notofonts/latin-greek-cyrillic/releases/tag/NotoSans-v2.015)
release tag, target commit, release-archive byte length and SHA-256, archive member,
and extracted font identity. The font is licensed under SIL Open Font License 1.1,
but the font binary is not copied into this repository. A capture host must install
the exact locked file in the Windows system or per-user font directory; the harness
verifies its byte length and SHA-256 before opening Word and verifies the embedded PDF
PostScript font name after export.

The authoritative diagnostic backend requires desktop Microsoft Word on Windows,
Windows PowerShell, PyMuPDF, and a clean checkout at the full source revision being
captured. It disables macros and dialogs, opens every generated DOCX read-only, uses a
fixed `ExportAsFixedFormat` option set, records the Word executable and runtime
identity, and retains no local paths. It does not use the network.

Run the campaign from PowerShell with the exact installed font path:

```powershell
python scripts/word_oracle_capture.py capture `
  --font "$env:WINDIR\Fonts\NotoSans-Regular.ttf" `
  --source-revision (git rev-parse HEAD)
```

The command creates two fresh Word processes beneath the ignored `target/` tree,
validates all 48 PDF identities and embedded fonts, extracts both topology reports,
and requires all 48 normalized reports to match exactly. Transient jobs containing
local paths are deleted after successful runs. `CAPTURE.json`, each path-neutral
export metadata file, both topology captures, and `repeatability.json` retain the
evidence needed for review.

Microsoft Word evidence remains diagnostic until the captured topology has been
reviewed against the renderer and accepted publicly. A repeatable capture alone does
not define a parity threshold, change renderer behavior, or add a release gate.
