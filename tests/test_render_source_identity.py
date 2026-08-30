import contextlib
import importlib.util
import pathlib
import subprocess
import sys
import tempfile
import unittest
from unittest import mock


ROOT = pathlib.Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location(
    "render_validate_source", ROOT / "scripts" / "render_validate.py"
)
render_validate = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
sys.modules[SPEC.name] = render_validate
SPEC.loader.exec_module(render_validate)


class RenderSourceIdentityTests(unittest.TestCase):
    def setUp(self):
        directory = self.enterContext(tempfile.TemporaryDirectory())
        self.repository = pathlib.Path(directory)
        self.git("init", "--quiet")
        self.hooks = self.repository / ".git" / "empty-hooks"
        self.hooks.mkdir()
        self.commit("first")
        self.previous = self.git("rev-parse", "HEAD")
        self.commit("second")
        self.head = self.git("rev-parse", "HEAD")
        self.enterContext(mock.patch.object(render_validate, "REPO", self.repository))

    def git(self, *args):
        return subprocess.run(
            ["git", *args], cwd=self.repository, check=True,
            capture_output=True, text=True,
        ).stdout.strip()

    def commit(self, message):
        self.git(
            "-c", "user.name=Fixture", "-c", "user.email=fixture@example.invalid",
            "-c", "commit.gpgsign=false", "-c", f"core.hooksPath={self.hooks}",
            "commit", "--allow-empty", "--quiet", "-m", message,
        )

    def test_default_identity_uses_current_head(self):
        self.assertEqual(render_validate._source_identity(None), (self.head, False))

    def test_explicit_identity_must_match_the_measured_checkout(self):
        self.assertEqual(render_validate._source_identity(self.head), (self.head, False))
        with self.assertRaisesRegex(ValueError, "does not match.*HEAD"):
            render_validate._source_identity(self.previous)

    def test_noncanonical_explicit_identity_is_rejected(self):
        for value in ("", self.head[:7], self.head.upper()):
            with self.subTest(revision=value):
                with self.assertRaisesRegex(ValueError, "full lowercase Git SHA"):
                    render_validate._source_identity(value)

    def test_dirty_checkout_is_never_reported_as_clean(self):
        (self.repository / "changed.txt").write_text("changed", encoding="utf-8")
        self.assertEqual(render_validate._source_identity(self.head), (self.head, True))

    def verify(self, *, dirty=False, harness=None, cargo=None, corpus=None):
        initial_corpus = mock.Mock()
        initial_corpus.path = pathlib.Path("RENDER_ORACLE.json")
        environment = {
            "source_revision": self.head,
            "source_dirty": dirty,
            "harness_sha256": "a" * 64,
            "cargo_lock_sha256": "b" * 64,
        }
        with contextlib.ExitStack() as stack:
            stack.enter_context(mock.patch.object(
                render_validate, "_harness_sha256", return_value=harness or "a" * 64
            ))
            stack.enter_context(mock.patch.object(
                render_validate, "_sha256_file", return_value=cargo or "b" * 64
            ))
            stack.enter_context(mock.patch.object(
                render_validate, "load_corpus_manifest",
                return_value=initial_corpus if corpus is None else corpus,
            ))
            render_validate.verify_campaign_inputs(initial_corpus, environment)

    def test_unchanged_campaign_inputs_are_accepted(self):
        self.verify()

    def test_head_change_during_campaign_is_rejected(self):
        self.commit("changed while rendering")
        with self.assertRaisesRegex(ValueError, "does not match.*HEAD"):
            self.verify()

    def test_dirty_state_change_during_campaign_is_rejected(self):
        (self.repository / "changed.txt").write_text("changed", encoding="utf-8")
        with self.assertRaisesRegex(ValueError, "source tree changed"):
            self.verify()

    def test_harness_dependency_and_corpus_drift_are_rejected(self):
        for changed, message in (
            ({"harness": "c" * 64}, "harness changed"),
            ({"cargo": "c" * 64}, "Cargo.lock changed"),
            ({"corpus": mock.Mock()}, "corpus changed"),
        ):
            with self.subTest(changed=message):
                with self.assertRaisesRegex(ValueError, message):
                    self.verify(**changed)


if __name__ == "__main__":
    unittest.main()
