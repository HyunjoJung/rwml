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
as `corpus/public/RENDER_ORACLE.json`. The lock is an input identity contract, not a claim of
Word parity and not a release threshold.

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

`--source-revision` asserts the current extracting checkout's full Git SHA; it
cannot label newly extracted evidence with a different revision. The separate
producer metadata identifies the application that wrote the PDFs. Retained capture
reports keep their recorded source revisions when loaded for comparison.

## Shared diagnostic font sources

`shared-font-lock.json` extends the unchanged eight-font
`libreoffice-font-lock.json` by its exact SHA-256. It adds Noto Sans CJK KR
OpenType CFF and Noto Emoji variable TrueType sources, with immutable upstream
repository/commit/path/Git-blob identities, file sizes, SHA-256 digests, SFNT
revisions, and the two additional sources' license payloads. Its explicit
ten-font order is the intended native diagnostic order, not host font discovery.

`scripts/shared_oracle_fonts.py` prepares and verifies this source pack offline.
Obtain the eight base fonts from their existing pinned release assets and the
two additions and licenses from the commit paths in the shared lock. Retain
upstream license/provenance material; the pack does not replace the base assets'
distribution obligations. Stage exactly the ten named fonts in one directory
and the two additional license files, using their locked names, in another:

```sh
python3 -B scripts/shared_oracle_fonts.py prepare \
  --font-dir <exact-font-directory> \
  --license-dir <additional-license-directory> \
  --output target/shared-oracle-fonts
python3 -B scripts/shared_oracle_fonts.py verify \
  --output target/shared-oracle-fonts
```

Preparation requires a fresh output outside the input directories. It validates
the complete file sets before writing; verification rereads every font and
license independently and checks the exact path-neutral `MANIFEST.json`.
Missing, extra, modified, aliased, symlinked, nonregular, or oversized inputs
and inconsistent receipts are rejected. The commands do not download fonts,
install them on the host, or change an existing capture's inputs.

This establishes source identity and order only. It does not prove embedded
Type 1/CFF subset identity, variable-font instance fidelity, glyph shaping,
Word-compatible layout, or cross-producer equivalence. Existing PDF attestation
and release validation retain their separate contracts.

## Locked LibreOffice runtime

`scripts/libreoffice_container.py` prepares and verifies a fixed Linux amd64
LibreOffice Writer image. `libreoffice-container-lock.json` pins the upstream
archive, base image, BuildKit/Buildx versions, recipe, profile, font configuration,
image manifest, config, and uncompressed layer digests. The runtime requires a
POSIX Docker client and a Linux daemon capable of executing amd64 images; use
WSL for this path on Windows. It does not replace the local release-preflight
oracle or change release acceptance.

With Buildx 0.35.0 installed, prepare a fresh build context from the official
archive. The preparation command verifies the archive bytes before creating the
context and refuses an existing output directory:

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

Use a fresh builder name if the example name is occupied. Inspection accepts
both Docker image-store formats only after checking the locked manifest/config
identity, complete layer identity, platform, user, entrypoint, and working
directory. Capture uses the validated digest with `--pull never`; it never
substitutes a floating tag. The recipe removes installed fonts and a
host-dependent fontconfig installation log. Do not update the lock to accept a
different build output without investigating the difference.

The Python `capture_document` helper runs one staged DOCX with a separately
staged font directory. The source directory contains `input.docx` and
`SHA256SUMS`; the font directory contains the selected font files, `SHA256SUMS`,
and `expected-paths.txt` listing their exact `/oracle/fonts/` paths. The caller
must bind these staged bytes to its corpus and font locks. The container checks
both checksum sets before and after conversion and checks its complete visible
font list. This low-level runtime does not attest a campaign or select fonts.

Each call uses a fresh profile in a read-only, non-root container with no network
or capabilities, two CPUs, 2 GiB memory with no extra swap, 128 PIDs, bounded
temporary storage, and a 180-second attached-process deadline. Output is a
bounded archive with exactly six regular-file members: the PDF, version, font
list, PDF checksum, warmup log, and conversion log. Duplicate, missing, linked,
sparse, or oversized members are rejected. The task-owned container is removed
on success and failure, and cleanup failure is reported. Process deadlines must
be finite positive numbers and output limits must be bounded positive integers.

Runtime inspection and a successful conversion alone do not prove repeatability,
Word parity, or full-campaign acceptance.

## Repeated LibreOffice table capture

`scripts/libreoffice_table_capture.py` runs both complete exports of the 48-case
unequal-column table corpus and independently validates the retained results.
It uses the locked runtime above and the exact `NotoSans-Regular.ttf` bytes from
`word-font-lock.json`. PyMuPDF 1.28.2 and Pillow 12.3.0 are required. The font is
staged read-only; this command does not download it or install host fonts.

From a clean checkout and a stable Python environment, use a fresh output path:

```sh
python3 -B scripts/libreoffice_table_capture.py capture \
  --font <path-to-NotoSans-Regular.ttf> \
  --output target/libreoffice-oracle/table-capture
python3 -B scripts/libreoffice_table_capture.py validate \
  --output target/libreoffice-oracle/table-capture
```

Each capture records the checkout, Python executable and runtime payload,
imported library payloads, Docker executable/runtime, image/recipe, input, font,
and extraction-harness identities. The analysis identity includes installed
startup code and bytecode, not only version strings or installer receipts.
Keep the environment unchanged and use `-B` for capture and validation. A
symlinked or changing library payload is rejected rather than silently omitted.

The verifier checks both complete PDF/metadata sets, exact artifact checksums,
embedded font PostScript names and SFNT revisions, independently extracted table
topology, and every page's 110-DPI raster. Raster hashes bind width, height and
color mode; a truncated page set is not a complete stability observation.
Only metadata reads explicitly allow empty regular files, such as warmup logs;
empty inputs, links, FIFOs and changing files remain invalid. Font names and
revision metadata alone do not prove outline or Unicode correctness.

`CAPTURE.json` is written only after independent validation succeeds. All 48
normalized topologies and complete raster sets must repeat. PDF bytes are
retained but are not required to match, because producer metadata may differ.
Validation uses the retained font and regenerated input identities and does not
launch Docker or Word. It requires the recorded checkout, harness and analysis
environment; historical receipts must not be relabeled to another revision.

These are diagnostic captures, not an authoritative Word comparison or a
release gate. General font packs and the expanded native-versus-oracle campaigns
remain separate from this single-font table path. `libreoffice-font-lock.json`
also records the bounded OFL font provenance used by the shared font-inspection
helpers; it does not widen the table capture's accepted font set.

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
