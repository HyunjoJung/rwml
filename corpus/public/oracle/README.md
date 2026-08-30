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
