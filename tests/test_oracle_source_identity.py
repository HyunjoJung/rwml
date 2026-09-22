import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest import mock

from scripts import render_validate, table_oracle_topology, word_oracle_capture


class OracleSourceIdentityTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.repo = Path(temporary.name)
        environment = {
            key: value for key, value in os.environ.items() if not key.startswith("GIT_")
        }
        environment.update(GIT_CONFIG_GLOBAL=os.devnull, GIT_CONFIG_NOSYSTEM="1")
        environment_patch = mock.patch.dict(os.environ, environment, clear=True)
        environment_patch.start()
        self.addCleanup(environment_patch.stop)
        self.git("init", "--quiet")
        self.git("config", "user.name", "Oracle Test")
        self.git("config", "user.email", "oracle@example.invalid")
        self.git("config", "commit.gpgsign", "false")
        self.git("config", "core.autocrlf", "false")
        self.source = self.repo / "source.txt"
        self.source.write_text("first\n", encoding="utf-8")
        self.git("add", "source.txt")
        self.git("commit", "--quiet", "-m", "First source")
        self.first = self.git("rev-parse", "HEAD")
        self.source.write_text("second\n", encoding="utf-8")
        self.git("commit", "--quiet", "-am", "Second source")
        self.head = self.git("rev-parse", "HEAD")

    def git(self, *arguments):
        return subprocess.run(
            ["git", *arguments], cwd=self.repo, check=True,
            capture_output=True, text=True,
        ).stdout.strip()

    def helpers(self):
        return (
            (render_validate, "REPO"),
            (table_oracle_topology, "ROOT"),
        )

    def test_implicit_and_explicit_head_identify_the_configured_checkout(self):
        for module, root_name in self.helpers():
            with self.subTest(module=module.__name__):
                with mock.patch.object(module, root_name, self.repo):
                    self.assertEqual(module._source_identity(None), (self.head, False))
                    self.assertEqual(module._source_identity(self.head), (self.head, False))

    def test_explicit_revision_cannot_relabel_a_different_checkout(self):
        for module, root_name in self.helpers():
            with self.subTest(module=module.__name__):
                with mock.patch.object(module, root_name, self.repo):
                    with self.assertRaisesRegex(ValueError, "checked-out commit"):
                        module._source_identity(self.first)

    def test_explicit_revision_rejects_empty_and_noncanonical_values(self):
        for module, root_name in self.helpers():
            for revision in ("", "HEAD", self.head[:12], "A" * 40, self.head + "\n"):
                with self.subTest(module=module.__name__, revision=revision):
                    with mock.patch.object(module, root_name, self.repo):
                        with self.assertRaisesRegex(ValueError, "source revision"):
                            module._source_identity(revision)

    def test_tracked_and_untracked_changes_remain_visible(self):
        for kind in ("tracked", "untracked"):
            path = self.source if kind == "tracked" else self.repo / "untracked.txt"
            path.write_text("changed\n", encoding="utf-8")
            try:
                for module, root_name in self.helpers():
                    with self.subTest(kind=kind, module=module.__name__):
                        with mock.patch.object(module, root_name, self.repo):
                            self.assertEqual(
                                module._source_identity(self.head), (self.head, True)
                            )
            finally:
                if kind == "tracked":
                    self.source.write_text("second\n", encoding="utf-8")
                else:
                    path.unlink()

    def test_detached_head_is_supported(self):
        self.git("switch", "--quiet", "--detach", self.head)
        for module, root_name in self.helpers():
            with self.subTest(module=module.__name__):
                with mock.patch.object(module, root_name, self.repo):
                    self.assertEqual(module._source_identity(self.head), (self.head, False))

    def test_unborn_checkout_cannot_use_an_explicit_revision(self):
        unborn = self.repo / "unborn"
        self.git("init", "--quiet", str(unborn))
        for module, root_name in self.helpers():
            with self.subTest(module=module.__name__):
                with mock.patch.object(module, root_name, unborn):
                    with self.assertRaisesRegex(
                        (ValueError, render_validate.RenderDependencyError),
                        "unavailable|identity command failed",
                    ):
                        module._source_identity(self.head)

    def test_word_capture_keeps_the_same_revision_assertion(self):
        with mock.patch.object(word_oracle_capture, "ROOT", self.repo):
            self.assertEqual(word_oracle_capture._git_revision(self.head), self.head)
            with self.assertRaisesRegex(ValueError, "checked-out commit"):
                word_oracle_capture._git_revision(self.first)


if __name__ == "__main__":
    unittest.main()
