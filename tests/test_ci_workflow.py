import pathlib
import unittest


WORKFLOW = (
    pathlib.Path(__file__).resolve().parents[1] / ".github" / "workflows" / "ci.yml"
)


class CiWorkflowTests(unittest.TestCase):
    def test_ci_workflow_runs_public_hygiene_audit(self):
        text = WORKFLOW.read_text(encoding="utf-8")

        self.assertIn("python3 scripts/public_hygiene_audit.py", text)

    def test_ci_workflow_runs_no_default_gate(self):
        text = WORKFLOW.read_text(encoding="utf-8")

        self.assertIn("cargo test --all-targets", text)
        self.assertIn("cargo test --all-targets --no-default-features", text)
        self.assertIn("cargo test --all-targets --features render", text)

    def test_ci_workflow_runs_python_tooling_tests(self):
        text = WORKFLOW.read_text(encoding="utf-8")

        self.assertIn(
            "python3 -m unittest discover -s tests -p 'test_*.py'", text
        )

    def test_ci_workflow_runs_bundled_font_gate(self):
        text = WORKFLOW.read_text(encoding="utf-8")

        self.assertIn(
            "cargo test --test bundled_fonts --all-features --locked", text
        )

    def test_ci_workflow_runs_release_mode_performance_gate(self):
        text = WORKFLOW.read_text(encoding="utf-8")

        self.assertIn(
            "cargo test --release --test performance --locked -- --ignored --nocapture",
            text,
        )

    def test_ci_workflow_builds_and_executes_wasm_bindings(self):
        text = WORKFLOW.read_text(encoding="utf-8")

        for command in [
            "cargo install wasm-bindgen-cli --version 0.2.126 --locked",
            "cargo build --lib --target wasm32-unknown-unknown --locked",
            "wasm-bindgen --target nodejs --out-dir target/wasm-node",
            "node tests/wasm_node_smoke.cjs target/wasm-node corpus/public/synthetic/comments.docx",
            "node tests/wasm_demo_report_format.mjs",
        ]:
            self.assertIn(command, text)

    def test_ci_workflow_checks_fuzz_targets_and_public_corpus_determinism(self):
        text = WORKFLOW.read_text(encoding="utf-8")

        self.assertIn(
            "cargo check --manifest-path fuzz/Cargo.toml --all-targets --locked",
            text,
        )
        self.assertIn("python3 scripts/gen_public_corpus.py --check", text)

    def test_ci_workflow_builds_no_default_msrv_surface(self):
        text = WORKFLOW.read_text(encoding="utf-8")

        self.assertIn("dtolnay/rust-toolchain@1.85.0", text)
        self.assertIn("dtolnay/rust-toolchain@1.92.0", text)
        self.assertIn("cargo build --no-default-features", text)

    def test_ci_workflow_checks_patch_compatible_public_api(self):
        text = WORKFLOW.read_text(encoding="utf-8")

        self.assertIn("fetch-depth: 0", text)
        self.assertIn(
            "cargo install cargo-semver-checks --version 0.48.0 --locked", text
        )
        self.assertIn(
            "cargo semver-checks check-release --baseline-rev v0.1.0 "
            "--release-type patch --default-features",
            text,
        )
        self.assertIn(
            "cargo semver-checks check-release --baseline-rev v0.1.0 "
            "--release-type patch --all-features",
            text,
        )


if __name__ == "__main__":
    unittest.main()
