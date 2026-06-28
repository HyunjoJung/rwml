import pathlib
import unittest


WORKFLOW = (
    pathlib.Path(__file__).resolve().parents[1]
    / ".github"
    / "workflows"
    / "release.yml"
)


class ReleaseWorkflowTests(unittest.TestCase):
    def test_release_workflow_publishes_manifest_artifact(self):
        text = WORKFLOW.read_text(encoding="utf-8")

        self.assertIn("python3 scripts/public_hygiene_audit.py", text)
        self.assertIn("python3 scripts/public_hygiene_audit.py --json", text)
        self.assertIn("cargo fmt --all -- --check", text)
        self.assertIn("cargo clippy --all-targets -- -D warnings", text)
        self.assertIn("scripts/release_manifest.py", text)
        self.assertIn("--git-rev \"$GITHUB_SHA\"", text)
        self.assertIn("--release-policy public-release", text)
        self.assertNotIn("--enforce-policy-inputs", text)
        self.assertIn("--hygiene-report dist/public-hygiene.json", text)
        self.assertIn("--corpus-manifest corpus/public/MANIFEST.tsv", text)
        self.assertIn("--corpus-manifest corpus/public/RENDER_MANIFEST.tsv", text)
        self.assertIn("cargo test --all-targets --features render", text)
        self.assertIn("dist/public-hygiene.json", text)
        self.assertIn("dist/rdoc-release-manifest.json", text)
        self.assertIn("target/package/rdoc-*.crate", text)
        self.assertIn("actions/upload-artifact@v4", text)


if __name__ == "__main__":
    unittest.main()
