# Microsoft Word pagination probes

`scripts/word_pilot_capture.py` is a separate, fixed diagnostic profile for two
pagination questions. It leaves the 48-document unequal-table campaign unchanged.
Its three inputs are synthetic, MIT-licensed, and bound by exact byte identities:

| Case | Input bytes | Purpose |
|---|---:|---|
| `pilot-unequal-table-continuation` | 18,300 | Existing pilot with unequal section columns and a continuing table |
| `synthetic-pagination-keep` | 4,507 | Original public pagination fixture, including its unspecified document font defaults |
| `synthetic-pagination-keep-noto11` | 4,701 | Original fixture with explicit Noto Sans 11pt document run defaults |

The control modifies only `word/styles.xml`, preserves package member order and
all other part payloads, and sets `ascii`, `hAnsi`, `eastAsia`, and `cs` font slots
plus `sz`/`szCs=22`. Its SHA-256 is
`f04c46ea3da6e4c1ea023fee71389602cf7c0091f351e8a950b61982bd035979`.
The existing pilot and public corpus locks are validated without refreshing them.
Generate inputs without Word with:

```sh
python scripts/word_pilot_capture.py generate --output target/word-pilot-inputs
```

Capture requires native Windows, desktop Word, Windows PowerShell, Git, a clean
checkout, PyMuPDF 1.28.2, and the exact font installed according to
`word-font-lock.json`. Save and close existing Word work before capture. The
backend opens inputs read-only, disables macros for its automation instance, and
uses the same fixed PDF export options as the table campaign. It does not change
the user's PowerShell profile or global Office security settings.

```powershell
python scripts/word_pilot_capture.py capture `
  --output target/word-pilot-capture `
  --font "<installed-NotoSans-Regular.ttf>" `
  --source-revision (git rev-parse HEAD)
python scripts/word_pilot_capture.py verify --output target/word-pilot-capture
```

Use a fresh output directory. The command exports all three documents twice;
retains six PDFs, producer/runtime/export metadata, exact inputs, font program
digests, text span baselines and sizes, page geometry, and 110-DPI raster hashes;
then independently extracts the PDFs again before writing `CAPTURE.json`.
Verification reconstructs all retained observations and receipt fields anew.
Both runs must have identical producer identities, page observations, and pixels;
raw PDF and embedded subset-program bytes may differ and remain recorded.

The original pagination document records its actual embedded fonts under the
explicit `observe-installed-defaults` policy. It does not claim the installed
Noto font was selected. The table and 11pt control require embedded
`NotoSans-Regular`; name and payload checks do not prove font-outline equivalence.
These observations establish neither expected Word page counts nor layout parity.

A timeout may leave the Word COM process running. Preserve partial output and
inspect process ownership before cleanup; never terminate unrelated Word work.
Missing `CAPTURE.json` means the capture did not complete successfully. Failed
transient `job.json` files contain local paths and must not be published.
