# Agent Instructions

Scope: the whole repository.

rdoc is a native Rust Microsoft Word engine. The public target is not a thin
wrapper: keep `.doc`, `.docx`, authoring, editing, diagnostics, and optional PDF
rendering moving toward one coherent Rust-native document model.

## Working Style

- Read the code and trace the flow before editing. A small diff in the wrong
  layer is still a bug.
- Prefer the smallest correct change, in this order: reuse existing repo
  helpers, use the Rust standard library, use already-installed dependencies,
  then write only the missing code.
- Do not add dependencies, abstractions, feature flags, fixtures, or docs unless
  they pay for the specific change.
- Fix root causes in shared scanners/helpers instead of patching one caller.
  Check sibling callers before declaring a parser or evaluator bug fixed.
- Respect dirty worktrees. Never revert or overwrite user changes; stage only
  files that belong to the task.
- Keep comments rare and useful. Mark intentional ceilings with a short comment
  that names the limit and the upgrade path.
- Avoid duplicate process ceremony. For roadmap work that already has PRD/TRD
  approval, skip fresh brainstorming/spec gates unless the user asks for them;
  keep minimal-diff discipline, TDD for behavior changes, and fresh verification.
- Use inventory-first roadmap batches. Before editing, list the bounded candidate
  gaps with evidence, risk, likely files, and focused verification; explicitly
  mark no-op or ambiguous candidates as skipped, then implement only the top
  verified slice.
- Keep inventory notes proportional. For a small deterministic parser/evaluator
  gap, first prove the gap locally with code evidence or a red test; write the
  longer outside-repo inventory entry after the selected slice is real, not
  before.
- Keep skill usage narrow. Select the smallest directly relevant skill set,
  avoid overlapping skills for the same decision, and reuse skill guidance that
  is already present in the current context instead of rereading adjacent docs.
- Keep agent-token output small: prefer `rg` plus narrow file windows, avoid full
  `git diff` dumps, cap broad search output, and summarize long test logs by
  exit status plus failures.
- Keep diffs small by closing focused, verified batches frequently. When asked
  to commit, stage only task-owned files and commit after the batch gate passes.

## Exploration Discipline

- Use a local-first path for narrow deterministic field/parser/report gaps:
  inspect the current code with `rg`, read the smallest relevant windows, add a
  focused red test, then edit. Do not start Spark just to find the next small
  syntax variant when the likely files are already known.
- Use Spark/Ultracode for high-uncertainty direction choices, cross-subsystem
  audits, R2-b layout work, R2-e legacy `.doc` work, or cases where independent
  read-only lanes can genuinely reduce search time.
- Before launching workers, define the stop condition. If local evidence or two
  worker lanes already identify the same low-risk candidate, start the local
  red-test loop instead of waiting for marginal extra scouting.
- Worker output is advisory. The parent must validate the chosen candidate
  locally before changing code, public docs, or outside inventories.
- When looking for an existing filtered test, use `cargo test <scope> -- --list`
  plus `rg` if the exact name is uncertain. A filtered command that runs zero
  tests is not verification.

## Ultracode / Spark

- For non-trivial codebase investigation, roadmap implementation, or independent
  fan-out/fan-in work, use Ultracode workers when the batch can benefit from
  parallel read-only lanes.
- Prefer Spark for heavyweight lanes by pinning the lowercase canonical model id
  `--model gpt-5.3-codex-spark --reasoning-effort high` when it is available
  and cost-appropriate. Lowercase matters for Codex app and worker tool calls;
  uppercase display names can be rejected.
- If Spark is capped, unavailable, or unnecessarily expensive for the lane,
  choose the cheapest adequate fallback worker model without asking for
  approval. Capture exact Spark errors when they happen, and state any fallback
  model used in the handoff or final summary.
- Keep worker lanes read-only for investigation unless the task explicitly needs
  isolated writable worktrees. The parent agent remains responsible for reading
  worker output, integrating changes, and running verification.
- Avoid duplicate local/worker inspection. Before launching a worker, give it a
  bounded question that does not overlap with the parent agent's active local
  search; if the parent can answer the question cheaply, skip the worker.
- Do not launch Ultracode for genuinely trivial edits, direct local commands, or
  small deterministic parser/evaluator gaps that can be proven faster with local
  `rg` plus one red test.

## Project Invariants

- Preserve package data by default. `.docx` edits should reserialize only touched
  parts and keep unmodeled package content byte-stable where possible.
- Malformed or hostile documents should return errors or read-only diagnostics,
  not panic.
- Computed field/render behavior must stay deterministic and source-order
  stable. If a field is unsupported, preserve cached display text and report the
  reason.
- WordprocessingML compatibility wrappers, tracked revisions, comments, notes,
  text boxes, floating shapes, fields, and header/footer regions should follow a
  consistent accepted-current view unless a test explicitly covers another view.
- Keep MSRV constraints intact: default features target Rust 1.74; `render`
  currently targets Rust 1.88.

## Public Hygiene

- Do not commit private documents, government/procurement source material,
  secrets, local paths, API keys, private emails beyond intended crate metadata,
  or AI planning artifacts.
- Use synthetic or clearly public corpus files only. When adding public corpus
  data, update manifests and document provenance.
- Before public/release work, run `python3 scripts/public_hygiene_audit.py`.
- Near-term release validation and manifest policy work is parked except for
  regressions, broken gates, or explicit user requests. Do not expand
  release-policy scope, thresholds, evidence formats, or render/benchmark
  normalization while the active engine push is trying to close reader,
  evaluator, layout, and legacy compatibility slices.

## Verification

Run the smallest command that proves the change, then broaden with risk. For
focused batches, prefer the narrowest relevant test binary, test name, or script
plus formatting/hygiene checks when they apply; do not run the full local gate by
default. Use the broader all-target/default/render gates only when the touched
surface or risk justifies them.

- Formatting: `cargo fmt --all -- --check`
- Focused Rust tests: `cargo test --test <name> <filter>` or
  `cargo test --lib <filter>`
- Test discovery: `cargo test --test <name> -- --list | rg '<pattern>'` when a
  filter is uncertain; rerun with the exact test name before claiming coverage.
- Default Rust gate: `cargo test --all-targets`
- Render changes: `cargo test --all-targets --features render`
- Public hygiene: `python3 scripts/public_hygiene_audit.py`
- Python tooling tests: `python3 -m unittest discover -s tests -p 'test_*.py'`
- Full local gate only for release-sized/high-risk changes:
  `cargo fmt --all -- --check`,
  `cargo clippy --all-targets -- -D warnings`,
  `cargo clippy --all-targets --all-features -- -D warnings`,
  `cargo test --all-targets`,
  `cargo test --all-targets --features render`,
  `cargo test --doc --all-features`, and
  `cargo doc --no-deps --all-features`.

If a check is skipped, say exactly why.

## Documentation

- Keep README, PRD, TRD, roadmap, tests, and diagnostics aligned. Do not claim
  support that lacks tests or deterministic behavior.
- Treat the roadmap's Near-Term Cut/R2 slice list as the canonical active
  backlog summary. README/PRD/TRD should stay user-facing and need not repeat
  every micro-slice if the existing wording remains true.
- For small semantic batches, update public docs only when the public support
  claim changes, becomes misleading, or needs a cached-vs-computed distinction.
  If a change only tightens already-documented behavior, record that in tests and
  the outside inventory instead of editing four long support paragraphs.
- Prefer short tables, bullets, or stable subsection anchors for support wording.
  Avoid broad patches against dense field-support paragraphs when a narrow exact
  clause or no doc change is enough.
- Write public docs as release notes for users, not internal agent logs.
- Avoid hype. State what works, what remains cached/unsupported, and which
  command verifies it.
