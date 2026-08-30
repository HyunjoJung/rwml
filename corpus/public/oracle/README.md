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

## Native fixed-font rendering

`scripts/render_validate.py` uses the `to_pdf` example's `--fixed-fonts` path.
It disables system fallback and rejects missing visible glyphs, missing glyph
artwork, and registered faces the PDF backend cannot embed. The supplied Noto
subsets cover bounded Korean/hanja, Arabic, and Hebrew text, not arbitrary CJK or
emoji. A coverage failure remains a failed native render; do not replace the
input or enable host fallback to make a fixed-font campaign succeed.

For local diagnostics with a separately verified font set, the example accepts
repeatable `--font` arguments. They select the same isolated PDF path and cannot
be combined with `--fixed-fonts`:

```sh
cargo run --features render --example to_pdf -- input.docx output.pdf \
  --font regular.ttf --font fallback.otf --report-json render.json
```

Files are loaded in argument order, with limits of 128 files, 64 MiB per file,
and 256 MiB total. Missing files, invalid arguments, and exceeded limits fail
before rendering. This local command does not attest font provenance or replace
the campaign's locked environment. Registered caller families also participate
in script and emoji fallback; system fallback remains disabled.

Native font isolation does not establish a common font set with LibreOffice.
Compare actual font selection and locked payloads before interpreting geometry
differences as renderer behavior. Oracle font validation remains independently
required for every primary and repeated reference PDF.

## Shared diagnostic font sources

`shared-font-lock.json` is an optional v2 source-pack contract. It binds the
unchanged eight-font v1 lock by SHA-256 and adds exact upstream Git commits,
paths, blob identities, SHA-256 digests, sizes, and license payloads for
[Noto Sans CJK KR](https://github.com/notofonts/noto-cjk/tree/523d033d6cb47f4a80c58a35753646f5c3608a78)
and [Noto Emoji](https://github.com/google/fonts/tree/b979dba422e445492b0eb9951ac52ee0b4d648c3/ofl/notoemoji).
Its `font_order` is the explicit native fallback order, not directory order.

Supply a directory containing exactly the ten named font files and a separate
directory containing the two additional license files, named
`NotoSansCJKkr-LICENSE.txt` and `NotoEmoji-OFL.txt`. Obtain those bytes from the
immutable source locations recorded in the locks. Preparation and verification
are offline; neither command downloads, installs, or discovers host fonts.

```sh
python3 scripts/shared_oracle_fonts.py prepare \
  --font-dir <exact-font-directory> \
  --license-dir <exact-license-directory> \
  --output target/render-oracle/shared-font-pack
python3 scripts/shared_oracle_fonts.py verify \
  --output target/render-oracle/shared-font-pack
```

Preparation requires a fresh output and validates all inputs before writing.
Verification rechecks every payload and recomputes the path-neutral manifest;
missing, extra, altered, symlinked, oversized, or identity-mismatched inputs and
receipts fail. Source metadata checks do not attest PDF Type 1/CFF subsets,
glyph shaping, or general variable-font fidelity. This pack is not yet wired
into general campaign acceptance and does not change the release gate or the
existing eight-font PDF attestation contract below.

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

## Digest-locked Linux regression capture

`scripts/libreoffice_table_capture.py` captures the 48 unequal-table inputs
through an isolated Linux amd64 Writer image. It uses the same exact
`NotoSans-Regular.ttf` payload as the Word diagnostic below. The image contains
no installed fonts; only that separately verified, read-only font is visible.
This path requires a POSIX Docker client, a Linux Docker daemon capable of
executing amd64 images, PyMuPDF 1.28.2, and Pillow 12.3.0. It is separate from
the existing local release-preflight oracle.

`libreoffice-container-lock.json` pins the upstream archive, base image,
BuildKit/Buildx versions, build recipe, profile, font configuration, image
manifest, config, and uncompressed layer digests. With Buildx 0.35.0 installed,
prepare a fresh build context from the official archive:

```sh
mkdir -p target/libreoffice-oracle
curl --fail --location --proto '=https' --proto-redir '=https' \
  --output target/libreoffice-oracle/libreoffice.tar.gz \
  https://downloadarchive.documentfoundation.org/libreoffice/old/26.2.3.2/deb/x86_64/LibreOffice_26.2.3.2_Linux_x86-64_deb.tar.gz
python3 scripts/libreoffice_container.py prepare \
  --archive target/libreoffice-oracle/libreoffice.tar.gz \
  --output target/libreoffice-oracle/context
docker buildx create --name rwml-lo-build --driver docker-container \
  --driver-opt image=docker.io/moby/buildkit:v0.31.2@sha256:2f5adac4ecd194d9f8c10b7b5d7bceb5186853db1b26e5abd3a657af0b7e26ec \
  --buildkitd-flags '--oci-worker-snapshotter=native' --bootstrap
docker buildx build --builder rwml-lo-build --platform linux/amd64 \
  --file target/libreoffice-oracle/context/Containerfile \
  --build-arg SOURCE_DATE_EPOCH=1783900800 \
  --no-cache --provenance=false --sbom=false \
  --output type=docker,dest=target/libreoffice-oracle/image.tar,rewrite-timestamp=true,oci-mediatypes=false \
  target/libreoffice-oracle/context
docker load --input target/libreoffice-oracle/image.tar
python3 scripts/libreoffice_container.py inspect
```

Use a fresh builder name when the example name is already occupied. Keep enough
free builder storage for installation and layer export; a failed export is not
a reproducibility result. The capture tool never pulls a floating image tag.
It accepts the locked image only after checking its platform, execution
configuration, and complete layer identity. The source archive is validated
before the build context is created. The Python tools do not download or install
the capture font.

From a clean source revision, run two complete captures with the exact font
identified by `word-font-lock.json`:

```sh
python3 scripts/libreoffice_table_capture.py capture \
  --font <path-to-NotoSans-Regular.ttf> \
  --output target/libreoffice-oracle/capture
python3 scripts/libreoffice_table_capture.py validate \
  --output target/libreoffice-oracle/capture
```

Each document gets a fresh profile and a read-only, non-root container with no
network, no capabilities, fixed CPU/memory/PID/file limits, bounded temporary
storage, and a 180-second deadline. Source and font bytes are checked before
and after conversion. Output must have exactly the expected regular-file
members; the verifier checks the producer, font name and SFNT revision, raw PDF
identity, every page's raster hash, and independently extracted table topology.
It rejects incomplete campaigns, stale source/harness/tool identities, altered
artifacts, and non-repeatable captures. Both normalized topology and 110-DPI
page pixels must repeat for all 48 documents. PDF byte identities are retained
but may differ because of producer metadata.

`CAPTURE.json` is written only after independent validation succeeds. Validation
does not need Docker or the original installed font: the retained verified font
and generated corpus are included in the capture directory. It does require
the recorded source revision and analysis-tool versions. A passing diagnostic
is not an authoritative Word comparison, release gate, or layout-parity claim.
General CJK/emoji font packs are not accepted by this single-font table path.

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
