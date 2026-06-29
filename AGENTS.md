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
- Keep skill usage narrow. Select the smallest directly relevant skill set,
  avoid overlapping skills for the same decision, and reuse skill guidance that
  is already present in the current context instead of rereading adjacent docs.
- Keep agent-token output small: prefer `rg` plus narrow file windows, avoid full
  `git diff` dumps, cap broad search output, and summarize long test logs by
  exit status plus failures.
- Keep diffs small by closing focused, verified batches frequently. When asked
  to commit, stage only task-owned files and commit after the batch gate passes.

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
- Do not launch Ultracode for genuinely trivial edits or direct local commands.

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

## Verification

Run the smallest command that proves the change, then broaden with risk. For
focused batches, prefer the narrowest relevant test binary, test name, or script
plus formatting/hygiene checks when they apply; do not run the full local gate by
default. Use the broader all-target/default/render gates only when the touched
surface or risk justifies them.

- Formatting: `cargo fmt --all -- --check`
- Focused Rust tests: `cargo test --test <name> <filter>` or
  `cargo test --lib <filter>`
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
- Write public docs as release notes for users, not internal agent logs.
- Avoid hype. State what works, what remains cached/unsupported, and which
  command verifies it.
