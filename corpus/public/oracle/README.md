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

## Bounded CJK Type 1 subset checks

`scripts/font_subset_attestation.py` checks a LibreOffice-style Type 1/PFA
program against the shared pack's exact Noto Sans CJK KR OTF. It compares every
subset glyph, including `.notdef`, using CID names, widths, font matrix, and
exact outline commands. The default mode rejects missing or aliased CIDs,
changed geometry, unsupported commands, and raw CFF without an explicit map.
Hinting, PDF encodings,
text shaping, placement, and raster equivalence are not covered by this proof.

The parser runs only inside the digest-locked Linux image described below,
using its Python 3.12.13 and the exact pure-Python FontTools 4.63.0 wheel pinned
in `fonttools-lock.json`. Supply that wheel from its recorded immutable URL;
the tool neither downloads it nor installs a host dependency. Docker isolation
is retained: no network, read-only input, non-root execution, fixed cgroup
limits, bounded temporary storage, and forced container cleanup. Additional
worker limits are 512 MiB of data memory, 20 CPU seconds, 1,024 subset glyphs,
131,072 outline commands total, and 8,192 commands per glyph. The attached
worker has a 30-second timeout; Docker lifecycle operations have their own
bounded deadlines. Input limits are 64 MiB for the source, 4 MiB for the PFA,
and 512 KiB for the result.

With the image loaded and the shared font pack independently verified:

```sh
python3 scripts/font_subset_attestation.py \
  --font-pack target/render-oracle/shared-font-pack \
  --fonttools-wheel <locked-FontTools-wheel> \
  --program <subset.pfa> --output <fresh-receipt.json>
python3 scripts/font_subset_attestation.py \
  --font-pack target/render-oracle/shared-font-pack \
  --fonttools-wheel <locked-FontTools-wheel> \
  --program <subset.pfa> --verify <receipt.json>
```

Verification reruns the bounded parser on the original source and subset,
then compares the complete receipt with the independently recomputed result.
Receipts bind input bytes, worker code, runtime, tools, enforced limits, and
glyph proofs; stale or modified receipts fail even when their aggregate hash
has been repaired. Verification requires the original inputs and locked
runtime, not just a receipt. The input is an already extracted raw PFA program;
this command does not extract or validate an entire PDF and does not change
the default PDF verifier or campaign acceptance.

Ordinary Python tests cover pure comparison and receipt contracts without
FontTools or Docker. Run the synthetic parser and resource-failure integration
tests explicitly with the locked image and wheel available:

```sh
RWML_FONTTOOLS_WHEEL=<locked-FontTools-wheel> \
  python3 -m unittest discover -s tests/font_programs -p 'test_*.py' -v
```

The explicit integration gate fails rather than skips when its runtime or
wheel is unavailable. It is diagnostic tooling validation, not a release gate
or evidence of Word pagination parity.

### Native renumbered CFF

The same tool accepts standalone CID-keyed CFF only with an explicit
`--cff-glyph-map`. The native subsetter renumbers CIDs; they cannot be treated
as original source CIDs. Unicode cmap lookup alone is also insufficient when
shaping selects alternate source glyphs. The map is an untrusted lookup
witness, not an assertion the verifier accepts without checking outlines.

Supply a JSON object with this structure, replacing the illustrative hashes
and glyph pairs with the complete map for the actual program:

```json
{
  "schema": "rwml.cff-glyph-map.v1",
  "source_sha256": "<source-OTF-SHA-256>",
  "subset_sha256": "<raw-CFF-SHA-256>",
  "glyphs": [[".notdef", ".notdef"], ["cid00001", "cid63157"]]
}
```

Both hashes must be 64 lowercase hexadecimal characters. Glyph pairs must
cover the complete subset in consecutive CID order, beginning with `.notdef`.
Every source glyph must exist and be unique. The 64-KiB map and 1,024-glyph
limits apply before font parsing; every mapped source glyph is then independently
compared with the actual subset glyph. A plausible map with changed geometry
still fails. The source and subset top matrices must agree; absent or identity
Font DICT matrices are supported, while other transforms fail explicitly.

```sh
python3 scripts/font_subset_attestation.py \
  --font-pack target/render-oracle/shared-font-pack \
  --fonttools-wheel <locked-FontTools-wheel> \
  --program <subset.cff> --cff-glyph-map <glyph-map.json> \
  --output <fresh-receipt.json>
python3 scripts/font_subset_attestation.py \
  --font-pack target/render-oracle/shared-font-pack \
  --fonttools-wheel <locked-FontTools-wheel> \
  --program <subset.cff> --cff-glyph-map <glyph-map.json> \
  --verify <receipt.json>
```

The map is bound into the recomputed receipt. CFF2, non-CID programs,
nonconsecutive native CIDs, additional fonts, invalid selectors, and unsupported
transforms are rejected. Type 1 remains the default and still rejects raw CFF
without a map. Both modes share the same locked parser/resource boundary and
explicit integration gate above. Automatic mapping discovery, bounded PDF
extraction, and general campaign integration are not provided by this command.
An outline proof does not establish Unicode semantics, font selection parity,
shaping correctness, or Word layout fidelity.

## Bounded PDF font resources

`scripts/pdf_font_resources.py` independently extracts catalog-reachable,
declared font dictionaries and their decoded embedded programs. It follows
inherited page resources, Form XObjects, annotation appearances, AcroForm
resources, and ExtGState font references. Repeated references are deduplicated
by PDF object number and generation, not by font name or observed text. Unused
declared fonts are included; unreachable objects are not. An empty inventory
is explicit and does not prove that every text operator has a valid resource.

The opt-in tool uses the exact pure-Python pypdf 6.16.2 wheel recorded in
`pypdf-lock.json`. Its URL, byte length, and SHA-256 are checked before importing
a private wheel snapshot inside the existing isolated Linux image. Nothing is
downloaded automatically or installed in the host environment. The existing
container, 512-MiB data-memory limit, 20-second CPU limit, and 30-second attached
worker deadline are unchanged. PDF inputs are limited to 16 MiB, graph traversal
to 16,384 nodes, 65,536 edges and depth 64, font resources to 64, and aggregate
decoded font/CMap data to 4 MiB. Each ToUnicode stream is limited to 64 KiB;
JSON output is limited to 8 MiB. Parser warnings fail rather than being hidden.

```sh
python3 scripts/pdf_font_resources.py \
  --pdf <input.pdf> --pypdf-wheel <locked-pypdf-wheel> \
  --output <fresh-resource-receipt.json>
python3 scripts/pdf_font_resources.py \
  --pdf <input.pdf> --pypdf-wheel <locked-pypdf-wheel> \
  --verify <resource-receipt.json>
```

Receipts bind the original PDF digest, parser and worker identities, runtime,
limits, unique font references, exact decoded program bytes, and raw ToUnicode
bytes. Verification repeats extraction from the original PDF and compares the
complete receipt. Missing or changed inputs, duplicate or unresolved identities,
inconsistent embedded types, missing or multiple programs, unsupported filters,
and exceeded bounds fail. Missing ToUnicode is recorded as `null`, not replaced
with an inferred map. TrueType, Type 1/PFA, and composite CIDFontType0C are
supported extraction representations; composite encodings are restricted to
Identity-H/V. Direct font/stream resources, Type 3, encrypted PDFs, external
streams, and non-Flate compressed font/CMap streams remain unsupported.

This tool does not parse font outlines or validate CMap semantics, content
operators, glyph selection, shaping, placement, raster equivalence, or Word
fidelity. Raw program checks above remain separate; automatic CFF mapping and
general campaign integration are not implied. Default release validation and
renderer support claims are unchanged.

The ordinary Python suite covers receipt contracts without pypdf or Docker.
Run the isolated parser, nested-resource, and malformed-input checks separately:

```sh
RWML_PYPDF_WHEEL=<locked-pypdf-wheel> \
  python3 -m unittest discover -s tests/pdf_resources -p 'test_*.py' -v
```

Missing prerequisites fail this explicit gate; they are not skipped.

### Automatic native CFF witnesses

`scripts/native_cff_attestation.py` composes PDF resource extraction, bounded
source-glyph discovery, and the independent raw-CFF proof worker. It checks
every extracted CIDFontType0C resource against the shared pack's exact Noto Sans
CJK KR source. No hand-written map is needed. Other font representations stay
listed in `unverified_resources`; this is not a whole-PDF font-fidelity proof.
A PDF with no native CFF resources is an explicit error, not an empty success.

ToUnicode values provide candidate hints only. Source GSUB single, alternate,
and ligature substitutions, including extension lookups, widen those candidates.
The discovery worker requires one exact width/outline-fingerprint match per
subset glyph, including `.notdef`, and rejects ambiguity or missing coverage.
The generated complete map then goes through the independent worker's exact
matrix, width, and outline-command comparison. A discovery result alone is not
accepted as proof, and hints do not establish Unicode or shaping correctness.

```sh
python3 scripts/native_cff_attestation.py \
  --pdf <native.pdf> --font-pack target/render-oracle/shared-font-pack \
  --fonttools-wheel <locked-FontTools-wheel> \
  --pypdf-wheel <locked-pypdf-wheel> --output <fresh-cff-receipt.json>
python3 scripts/native_cff_attestation.py \
  --pdf <native.pdf> --font-pack target/render-oracle/shared-font-pack \
  --fonttools-wheel <locked-FontTools-wheel> \
  --pypdf-wheel <locked-pypdf-wheel> --verify <cff-receipt.json>
```

Verification repeats extraction, discovery, and proof from the original inputs.
Receipts bind both workers to the original PDF font references, embedded CFF and
ToUnicode bytes, locked source font, parser/tool identities, and runtime. Missing,
duplicate, extra, empty, oversized, or surrogate hints fail. Contextual shaping
and arbitrary multi-glyph transformations are not simulated. Hint sequences are
limited to eight Unicode scalars; candidate sets to 256 per glyph; source draws
to 4,096 glyphs; and candidate search and outline work to 131,072 steps/commands
each. GSUB construction is limited to 1,024 lookups, 4,096 subtables, 65,536
single/alternate edges, and 4,096 ligature records. The existing isolated parser
limits remain in force. A 120-second batch budget is checked between bounded
operations; cleanup and Docker lifecycle operations retain their own deadlines.
The existing 8-MiB receipt limit applies to the composed evidence.

```sh
RWML_FONTTOOLS_WHEEL=<locked-FontTools-wheel> \
RWML_PYPDF_WHEEL=<locked-pypdf-wheel> \
  python3 -m unittest discover -s tests/cff_discovery -p 'test_*.py' -v
```

This diagnostic gate is separate from the earlier font/PDF checks and the
release gate. It does not establish Word layout, content-operator validity,
font-selection parity, or general strict-campaign acceptance.

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
## Shared-font campaign capture

The diagnostic capture command composes the locked LibreOffice runtime, verified
shared font pack, native renderer, and declared PDF font-resource checks. It
requires a clean checkout and a strict corpus manifest; outputs must be fresh
and outside the input corpus and font pack. Build/load the locked container and
prepare the shared font pack and pinned wheels using the commands above first.

```sh
python3 scripts/render_campaign_capture.py capture \
  --manifest corpus/public/RENDER_SMOKE_ORACLE.json \
  --output target/render-oracle/shared-smoke-a \
  --font-pack target/shared-font-pack \
  --fonttools-wheel target/fonttools-4.63.0-py3-none-any.whl \
  --pypdf-wheel target/pypdf-6.16.2-py3-none-any.whl

python3 scripts/render_validate.py --json \
  --manifest corpus/public/RENDER_SMOKE_ORACLE.json \
  --capture-dir target/render-oracle/shared-smoke-a \
  --shared-font-pack target/shared-font-pack \
  --fonttools-wheel target/fonttools-4.63.0-py3-none-any.whl \
  --pypdf-wheel target/pypdf-6.16.2-py3-none-any.whl
```

Capture builds `to_pdf` once with Rust 1.92.0 and passes every shared font in lock
order without system fallback. Each DOCX is rendered by both engines; the input,
PDFs, native warning report, reference runtime/font-closure records, and complete
font-check receipts are retained. Any conversion or font-check failure prevents
the final `CAPTURE.json` receipt. The process, output, and campaign bounds are
enforced rather than treating timeouts or skipped cases as successful captures.

The measurement command first independently verifies the retained capture:
it rebuilds the native executable and repeats PDF extraction and applicable
font checks, then computes the existing visual/text/geometry metrics. Use
`render_campaign_capture.py verify` with the same capture arguments for this
verification without metric analysis. Verification does not rerender the
documents or establish authenticated producer provenance. Receipts bind observed
bytes and identities; two separately captured campaigns are still needed for
repeatability, and reviewed Word diagnostics remain a separate requirement.

Shared capture metrics use `rwml.render-oracle-evidence.v5`, with complete
per-case capture bindings. Existing local/legacy-container validation remains
v4. A single captured campaign leaves reference repeatability unverified;
`--verify-oracle`, system-font, and renderer overrides cannot be combined with
`--capture-dir`. The schema and command do not change release requirements.

Font results distinguish exact Type 1/native CFF glyph-outline checks from
TrueType descriptor-name/SFNT-revision metadata checks. Empty inventories are
explicit, and every declared resource is accounted for. Metadata agreement is
not outline equivalence, font-selection correctness, Unicode/shaping correctness,
or full PDF/Word fidelity. The current locked capture recipe accepts DOCX only.
