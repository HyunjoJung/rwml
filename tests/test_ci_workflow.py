import pathlib
import unittest


WORKFLOW = (
    pathlib.Path(__file__).resolve().parents[1] / ".github" / "workflows" / "ci.yml"
)


class CiWorkflowTests(unittest.TestCase):
    def test_ci_workflow_runs_no_default_gate(self):
        text = WORKFLOW.read_text(encoding="utf-8")

        self.assertIn("cargo test --all-targets", text)
        self.assertIn("cargo test --no-default-features", text)
        self.assertIn("cargo test --all-targets --features render", text)


if __name__ == "__main__":
    unittest.main()
