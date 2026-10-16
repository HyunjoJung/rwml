import importlib.util
import pathlib
import tempfile
import unittest
from unittest import mock


ROOT = pathlib.Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "scripts" / "semver_check.py"


def load_script():
    spec = importlib.util.spec_from_file_location("semver_check", SCRIPT)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class SemverCheckTests(unittest.TestCase):
    def setUp(self):
        self.module = load_script()

    def test_rustdoc_matches_cargo_semver_checks_and_stays_locked(self):
        text = SCRIPT.read_text(encoding="utf-8")

        self.assertEqual(
            self.module.RUSTDOC_FLAGS,
            "-Z unstable-options --document-private-items --document-hidden-items "
            "--output-format=json --cap-lints=allow",
        )
        self.assertEqual(
            self.module.FEATURE_SETS,
            {"default-features": [], "all-features": ["--all-features"]},
        )
        self.assertIn('"--locked"', text)
        self.assertIn('"--no-deps"', text)
        self.assertIn('RUSTC_BOOTSTRAP="1"', text)
        self.assertIn('"git", "-C", str(ROOT), "archive"', text)
        self.assertIn("has no committed Cargo.lock", text)

    def test_each_feature_set_compares_locked_baseline_with_current(self):
        calls = []
        generated = []

        def fake_rustdoc(manifest_dir, features, target_dir, output, toolchain):
            generated.append((manifest_dir, tuple(features), target_dir.name, toolchain))
            return output

        with tempfile.TemporaryDirectory() as tmp:
            baseline = pathlib.Path(tmp) / "baseline-v0.1.4"
            with mock.patch.object(
                self.module, "export_baseline", return_value=baseline
            ) as export, mock.patch.object(
                self.module, "feature_problems", return_value=[]
            ), mock.patch.object(
                self.module, "rustdoc_json", side_effect=fake_rustdoc
            ), mock.patch.object(
                self.module, "run", side_effect=lambda cmd, env=None: calls.append(cmd)
            ):
                status = self.module.main(
                    [
                        "--baseline-rev",
                        "v0.1.4",
                        "--output-dir",
                        tmp,
                        "--toolchain",
                        "1.92.0",
                    ]
                )

        self.assertEqual(status, 0)
        export.assert_called_once()
        self.assertEqual(export.call_args.args[0], "v0.1.4")
        self.assertEqual(
            generated,
            [
                (baseline, (), "baseline-default-features", "1.92.0"),
                (self.module.ROOT, (), "current-default-features", "1.92.0"),
                (baseline, ("--all-features",), "baseline-all-features", "1.92.0"),
                (self.module.ROOT, ("--all-features",), "current-all-features", "1.92.0"),
            ],
        )
        self.assertEqual(len(calls), 2)
        for command, name in zip(calls, ["default-features", "all-features"]):
            self.assertEqual(command[:4], ["cargo", "+1.92.0", "semver-checks", "check-release"])
            self.assertIn("--baseline-rustdoc", command)
            self.assertIn("--current-rustdoc", command)
            self.assertTrue(command[command.index("--baseline-rustdoc") + 1].endswith(f"baseline-{name}.json"))
            self.assertTrue(command[command.index("--current-rustdoc") + 1].endswith(f"current-{name}.json"))
            self.assertEqual(command[command.index("--release-type") + 1], "patch")

    def write_manifest(self, directory, text):
        path = pathlib.Path(directory) / "Cargo.toml"
        path.write_text(text, encoding="utf-8")
        return path

    def test_feature_check_reports_removed_and_non_default_features(self):
        baseline = """
[dependencies]
zip = { version = "2", optional = true }
legacy = { version = "1", optional = true }

[features]
default = ["docx"]
docx = ["dep:zip"]
render = []
"""
        current = """
[dependencies]
zip = { version = "9", optional = true }
legacy = { version = "1", optional = true }

[features]
default = []
docx = ["dep:zip"]
other = ["dep:legacy"]
"""
        with tempfile.TemporaryDirectory() as old, tempfile.TemporaryDirectory() as new:
            problems = self.module.feature_problems(
                self.write_manifest(old, baseline), self.write_manifest(new, current)
            )

        self.assertEqual(
            problems,
            [
                "feature `legacy` was removed",
                "feature `render` was removed",
                "feature `docx` is no longer enabled by default",
            ],
        )

    def test_feature_check_accepts_identical_and_additive_features(self):
        baseline = """
[features]
default = ["docx"]
docx = []

[target.'cfg(target_arch = "wasm32")'.dependencies]
wasm-bindgen = { version = "0.2", optional = true }
"""
        current = baseline.replace("docx = []\n", "docx = []\nrender = []\n")
        with tempfile.TemporaryDirectory() as old, tempfile.TemporaryDirectory() as new:
            self.assertEqual(
                self.module.feature_problems(
                    self.write_manifest(old, baseline), self.write_manifest(new, current)
                ),
                [],
            )

    def test_main_stops_before_rustdoc_when_features_break(self):
        with tempfile.TemporaryDirectory() as tmp, mock.patch.object(
            self.module, "export_baseline", return_value=pathlib.Path(tmp)
        ), mock.patch.object(
            self.module, "feature_problems", return_value=["feature `docx` was removed"]
        ), mock.patch.object(self.module, "rustdoc_json") as rustdoc:
            with self.assertRaises(SystemExit) as raised:
                self.module.main(["--baseline-rev", "v0.1.4", "--output-dir", tmp])

        self.assertIn("feature `docx` was removed", str(raised.exception))
        rustdoc.assert_not_called()

    def test_cargo_uses_the_default_toolchain_unless_one_is_named(self):
        self.assertEqual(self.module.cargo(None), ["cargo"])
        self.assertEqual(self.module.cargo("1.92.0"), ["cargo", "+1.92.0"])


if __name__ == "__main__":
    unittest.main()
