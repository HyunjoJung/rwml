import copy
import importlib
from pathlib import Path, PurePosixPath
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest import mock


ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "scripts"))
identity = importlib.import_module("analysis_tool_identity")


class FakeDistribution:
    def __init__(self, root: Path, name: str = "Demo-Tool", version: str = "1.2.3"):
        self.root = root
        self.metadata = {"Name": name}
        self.version = version
        self.files = [
            PurePosixPath("demo/__init__.py"),
            PurePosixPath("demo_tool-1.2.3.dist-info/METADATA"),
            PurePosixPath("demo_tool-1.2.3.dist-info/RECORD"),
            PurePosixPath("../../../bin/demo-tool"),
        ]

    def locate_file(self, path):
        return self.root / Path(str(path))


def write_fake_distribution(root: Path, *, marker: bytes = b"extra") -> None:
    package = root / "demo"
    metadata = root / "demo_tool-1.2.3.dist-info"
    package.mkdir(parents=True)
    metadata.mkdir()
    (package / "__init__.py").write_bytes(b"VERSION = '1.2.3'\n")
    # This file is deliberately absent from RECORD. The installed import tree,
    # not the installer receipt, owns the identity.
    (package / "extra.py").write_bytes(marker)
    (metadata / "METADATA").write_bytes(b"Name: Demo-Tool\nVersion: 1.2.3\n")
    (metadata / "RECORD").write_text("path-dependent installer receipt\n")
    (metadata / "INSTALLER").write_text("pip\n")
    (metadata / "REQUESTED").write_bytes(b"")
    (metadata / "direct_url.json").write_text('{"url":"file:///private/path"}\n')
    cache = package / "__pycache__"
    cache.mkdir()
    (cache / "extra.cpython-313.pyc").write_bytes(b"path-dependent cache")


class AnalysisToolIdentityTests(unittest.TestCase):
    def test_python_identity_binds_unowned_site_packages_startup_code(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            executable = root / "bin/python"
            stdlib = root / "lib/python"
            platform_stdlib = root / "platform-lib/python"
            site_packages = root / "venv/site-packages"
            platform_site_packages = root / "venv/platform-site-packages"
            library = root / "lib/libpython.so"
            executable.parent.mkdir(parents=True)
            stdlib.mkdir(parents=True)
            platform_stdlib.mkdir(parents=True)
            site_packages.mkdir(parents=True)
            platform_site_packages.mkdir(parents=True)
            executable.write_bytes(b"python executable")
            library.write_bytes(b"python library")
            (stdlib / "os.py").write_bytes(b"stdlib")
            (platform_stdlib / "extension.so").write_bytes(b"extension")
            startup = site_packages / "sitecustomize.py"
            startup.write_bytes(b"first")
            platform_startup = platform_site_packages / "platform_hook.pth"
            platform_startup.write_bytes(b"first platform hook")

            def get_path(name):
                return str(
                    {
                        "stdlib": stdlib,
                        "platstdlib": platform_stdlib,
                        "purelib": site_packages,
                        "platlib": platform_site_packages,
                    }[name]
                )

            with (
                mock.patch.object(identity.sys, "executable", str(executable)),
                mock.patch.object(identity.sysconfig, "get_path", side_effect=get_path),
                mock.patch.object(identity, "_python_library", return_value=library),
            ):
                first = identity.python_identity()
                startup.write_bytes(b"second")
                second = identity.python_identity()
                startup.write_bytes(b"first")
                platform_startup.write_bytes(b"second platform hook")
                third = identity.python_identity()
            self.assertNotEqual(first["sha256"], second["sha256"])
            self.assertNotEqual(first["sha256"], third["sha256"])
            self.assertNotIn(str(root), str(first))

    def test_distribution_identity_is_path_neutral_and_covers_unowned_files(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            first_root = root / "first/site-packages"
            second_root = root / "elsewhere/site-packages"
            write_fake_distribution(first_root)
            write_fake_distribution(second_root)
            (second_root / "demo_tool-1.2.3.dist-info/RECORD").write_text(
                "different absolute installation path\n"
            )

            with mock.patch.object(
                identity.metadata,
                "distribution",
                side_effect=[
                    FakeDistribution(first_root),
                    FakeDistribution(second_root),
                    FakeDistribution(second_root),
                    FakeDistribution(second_root),
                ],
            ):
                first = identity.distribution_identity(
                    "demo",
                    "Demo-Tool",
                    "1.2.3",
                    SimpleNamespace(__file__=first_root / "demo/__init__.py"),
                )
                second = identity.distribution_identity(
                    "demo",
                    "Demo-Tool",
                    "1.2.3",
                    SimpleNamespace(__file__=second_root / "demo/__init__.py"),
                )
                self.assertEqual(first, second)
                self.assertEqual(first["name"], "demo")
                self.assertEqual(first["files"], 4)
                self.assertNotIn(str(root), str(first))

                cache = second_root / "demo/__pycache__/extra.cpython-313.pyc"
                cache.write_bytes(b"changed bytecode")
                bytecode_changed = identity.distribution_identity(
                    "demo",
                    "Demo-Tool",
                    "1.2.3",
                    SimpleNamespace(__file__=second_root / "demo/__init__.py"),
                )
                self.assertNotEqual(first["sha256"], bytecode_changed["sha256"])
                cache.write_bytes(b"path-dependent cache")
                (second_root / "demo/extra.py").write_bytes(b"changed")
                changed = identity.distribution_identity(
                    "demo",
                    "Demo-Tool",
                    "1.2.3",
                    SimpleNamespace(__file__=second_root / "demo/__init__.py"),
                )
                self.assertNotEqual(first["sha256"], changed["sha256"])

    def test_distribution_identity_rejects_symlinks_and_version_drift(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary) / "site-packages"
            write_fake_distribution(root)
            (root / "demo/linked.py").symlink_to(root / "demo/extra.py")
            with mock.patch.object(
                identity.metadata, "distribution", return_value=FakeDistribution(root)
            ):
                with self.assertRaisesRegex(ValueError, "symlink"):
                    identity.distribution_identity(
                        "demo",
                        "Demo-Tool",
                        "1.2.3",
                        SimpleNamespace(__file__=root / "demo/__init__.py"),
                    )
            (root / "demo/linked.py").unlink()
            with mock.patch.object(
                identity.metadata, "distribution", return_value=FakeDistribution(root)
            ):
                with self.assertRaisesRegex(ValueError, "version"):
                    identity.distribution_identity(
                        "demo",
                        "Demo-Tool",
                        "9.9.9",
                        SimpleNamespace(__file__=root / "demo/__init__.py"),
                    )

    def test_distribution_identity_rejects_excessive_tree_depth(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary) / "site-packages"
            write_fake_distribution(root)
            nested = root / "demo"
            for index in range(identity.MAX_DEPTH + 1):
                nested = nested / f"d{index}"
                nested.mkdir()
            with mock.patch.object(
                identity.metadata,
                "distribution",
                return_value=FakeDistribution(root),
            ):
                with self.assertRaisesRegex(ValueError, "depth"):
                    identity.distribution_identity(
                        "demo",
                        "Demo-Tool",
                        "1.2.3",
                        SimpleNamespace(__file__=root / "demo/__init__.py"),
                    )

    def test_distribution_identity_rejects_shadow_import_outside_payload(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            installed = root / "site-packages"
            write_fake_distribution(installed)
            shadow = root / "shadow/demo.py"
            shadow.parent.mkdir()
            shadow.write_bytes(b"VERSION = '1.2.3'\n")
            with mock.patch.object(
                identity.metadata,
                "distribution",
                return_value=FakeDistribution(installed),
            ):
                with self.assertRaisesRegex(ValueError, "import origin"):
                    identity.distribution_identity(
                        "demo",
                        "Demo-Tool",
                        "1.2.3",
                        SimpleNamespace(__file__=shadow),
                    )

    def test_distribution_identity_rejects_change_after_file_was_read(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary) / "site-packages"
            write_fake_distribution(root)
            original = identity._read_regular_file

            def change_after_read(path, maximum, **kwargs):
                payload = original(path, maximum, **kwargs)
                if path.name == "__init__.py":
                    path.write_bytes(b"changed after read")
                return payload

            with (
                mock.patch.object(
                    identity.metadata,
                    "distribution",
                    return_value=FakeDistribution(root),
                ),
                mock.patch.object(
                    identity, "_read_regular_file", side_effect=change_after_read
                ),
            ):
                with self.assertRaisesRegex(ValueError, "changed while"):
                    identity.distribution_identity(
                        "demo",
                        "Demo-Tool",
                        "1.2.3",
                        SimpleNamespace(__file__=root / "demo/__init__.py"),
                    )

    def test_analysis_identity_is_sorted_validated_and_digest_bound(self):
        python = {
            "implementation": "CPython",
            "version": "3.13.13",
            "cache_tag": "cpython-313",
            "abi": "cpython-313-test",
            "platform": "test-platform",
            "flags": {name: 0 for name in identity.PYTHON_FLAG_NAMES},
            "files": 3,
            "bytes": 12,
            "sha256": "a" * 64,
        }
        packages = {
            "pymupdf": ("PyMuPDF", "1.28.2", object()),
            "pillow": ("Pillow", "12.3.0", object()),
        }

        def package(name, _distribution, version, _module):
            return {
                "name": name,
                "version": version,
                "files": 2,
                "bytes": 8,
                "sha256": ("b" if name == "pillow" else "c") * 64,
            }

        with (
            mock.patch.object(identity, "python_identity", return_value=python),
            mock.patch.object(identity, "distribution_identity", side_effect=package),
        ):
            value = identity.analysis_identity(packages)

        self.assertEqual(
            [item["name"] for item in value["distributions"]],
            ["pillow", "pymupdf"],
        )
        self.assertEqual(
            identity.tool_versions(value),
            {"pillow": "12.3.0", "pymupdf": "1.28.2", "python": "3.13.13"},
        )
        identity.validate_analysis_identity(value)
        for mutation in ("identity", "files", "order"):
            invalid = copy.deepcopy(value)
            if mutation == "identity":
                invalid["identity_sha256"] = "0" * 64
            elif mutation == "files":
                invalid["distributions"][0]["files"] = True
            else:
                invalid["distributions"].reverse()
            with self.subTest(mutation=mutation), self.assertRaises(ValueError):
                identity.validate_analysis_identity(invalid)


if __name__ == "__main__":
    unittest.main()
