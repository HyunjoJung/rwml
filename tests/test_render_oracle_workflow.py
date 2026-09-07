from pathlib import Path
import unittest


WORKFLOW = Path(__file__).resolve().parents[1] / ".github/workflows/render-oracle.yml"


class OracleWorkflowTests(unittest.TestCase):
    def test_job_is_bounded_read_only_and_bound_to_the_checked_out_commit(self):
        text = WORKFLOW.read_text()
        for expected in (
            "pull_request:",
            "workflow_dispatch:",
            "contents: read",
            "timeout-minutes: 60",
            "cancel-in-progress: true",
            "persist-credentials: false",
            "ref: ${{ github.sha }}",
            "SOURCE_REVISION: ${{ github.sha }}",
            '--source-revision "$SOURCE_REVISION"',
            "retention-days: 14",
            "if-no-files-found: error",
        ):
            self.assertIn(expected, text)
        for forbidden in (
            "pull_request_target:",
            "continue-on-error:",
            "secrets.",
            "contents: write",
            "packages: write",
            "cargo publish",
            "git push",
        ):
            self.assertNotIn(forbidden, text)

    def test_ci_uses_locked_prerequisites_and_verifies_the_built_image(self):
        text = WORKFLOW.read_text()
        for expected in (
            "dtolnay/rust-toolchain@1.92.0",
            'python-version: "3.13"',
            "PyMuPDF==1.28.2 Pillow==12.3.0 numpy==2.4.4",
            "cargo fetch --locked",
            "scripts/prepare_render_oracle_ci.py",
            "runtime.load_runtime_lock()",
            "docker/setup-buildx-action@v4",
            "version: ${{ steps.lock.outputs.buildx }}",
            "image=${{ steps.lock.outputs.buildkit }}",
            "--oci-worker-snapshotter=native",
            "--provenance=false --sbom=false",
            "rewrite-timestamp=true,oci-mediatypes=false",
            "scripts/libreoffice_container.py inspect",
            "scripts/render_oracle_ci.py",
            "target/oracle-ci/evidence",
            "actions/upload-artifact@v7",
        ):
            self.assertIn(expected, text)
        self.assertLess(
            text.index("scripts/libreoffice_container.py inspect"),
            text.index("scripts/render_oracle_ci.py"),
        )


if __name__ == "__main__":
    unittest.main()
