import copy
import importlib
import io
import json
import os
import pathlib
import sys
import tarfile
import tempfile
import unittest
from unittest import mock

ROOT = pathlib.Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "scripts"))
oracle = importlib.import_module("libreoffice_container")


def sample_lock():
    return {
        "image": {
            "manifest_sha256": "a" * 64,
            "config_sha256": "b" * 64,
            "rootfs_sha256": ["c" * 64],
        },
    }


def sample_image():
    return {
        "Id": "sha256:" + "a" * 64,
        "Architecture": "amd64",
        "Os": "linux",
        "Descriptor": {"digest": "sha256:" + "a" * 64},
        "RootFS": {"Type": "layers", "Layers": ["sha256:" + "c" * 64]},
        "Config": {
            "User": "65534:65534",
            "Entrypoint": ["/opt/rwml-oracle/capture.sh"],
            "WorkingDir": "/oracle",
        },
    }


def archive(entries):
    output = io.BytesIO()
    with tarfile.open(fileobj=output, mode="w") as target:
        for name, payload, kind in entries:
            member = tarfile.TarInfo(name)
            member.size = len(payload)
            member.type = kind
            target.addfile(member, io.BytesIO(payload))
    return output.getvalue()


class LibreOfficeContainerTests(unittest.TestCase):
    def test_public_lock_binds_recipe_and_rejects_mutations(self):
        lock = oracle.load_runtime_lock()
        self.assertEqual(lock["build"]["platform"], "linux/amd64")
        self.assertEqual(set(lock["files"]), set(oracle.RECIPE_FILES))
        changes = []
        invalid = copy.deepcopy(lock)
        invalid["unknown"] = True
        changes.append(invalid)
        invalid = copy.deepcopy(lock)
        invalid["files"]["capture.sh"] = "f" * 64
        changes.append(invalid)
        invalid = copy.deepcopy(lock)
        invalid["image"]["manifest_sha256"] = "0" * 64
        changes.append(invalid)
        invalid = copy.deepcopy(lock)
        invalid["build"]["source_date_epoch"] = True
        changes.append(invalid)
        with tempfile.TemporaryDirectory() as temporary:
            path = pathlib.Path(temporary) / "lock.json"
            for invalid in changes:
                path.write_text(json.dumps(invalid))
                with self.assertRaises(ValueError):
                    oracle.load_runtime_lock(path)
            path.write_text('{"schema":"one","schema":"two"}')
            with self.assertRaises(ValueError):
                oracle.load_runtime_lock(path)

    def test_image_identity_accepts_manifest_and_classic_config_stores(self):
        manifest_image = sample_image()
        self.assertEqual(
            oracle.validate_image(manifest_image, sample_lock()), manifest_image["Id"]
        )
        classic = copy.deepcopy(manifest_image)
        classic.pop("Descriptor")
        classic["Id"] = "sha256:" + "b" * 64
        self.assertEqual(oracle.validate_image(classic, sample_lock()), classic["Id"])

    def test_image_identity_rejects_wrong_payload_platform_or_entrypoint(self):
        mutations = [
            ("Id", "sha256:" + "d" * 64),
            ("Architecture", "arm64"),
            ("Os", "windows"),
            ("Descriptor", {"digest": "sha256:" + "e" * 64}),
            ("RootFS", {"Type": "layers", "Layers": []}),
            ("Config", {"User": "0", "Entrypoint": ["sh"], "WorkingDir": "/oracle"}),
        ]
        for key, value in mutations:
            with self.subTest(key=key):
                info = sample_image()
                info[key] = value
                with self.assertRaises(ValueError):
                    oracle.validate_image(info, sample_lock())

    def test_create_command_has_fixed_isolation_and_read_only_mounts(self):
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            command = oracle.create_command(
                "sha256:" + "a" * 64, "rwml-oracle-" + "d" * 32, root, root
            )
        for option, value in [
            ("--pull", "never"),
            ("--network", "none"),
            ("--cap-drop", "ALL"),
            ("--user", "65534:65534"),
            ("--pids-limit", "128"),
            ("--memory", "2g"),
        ]:
            self.assertEqual(command[command.index(option) + 1], value)
        self.assertIn("--read-only", command)
        self.assertIn("no-new-privileges", command)
        self.assertEqual(command[-1], "sha256:" + "a" * 64)
        mounts = [
            command[i + 1] for i, value in enumerate(command) if value == "--mount"
        ]
        self.assertEqual(len(mounts), 2)
        self.assertTrue(all(value.endswith(",readonly") for value in mounts))

    def test_create_command_rejects_tags_and_mount_option_injection(self):
        for reference in ["lo-cli", "image:latest", "sha256:" + "A" * 64]:
            with self.assertRaises(ValueError):
                oracle.create_command(reference, "rwml-oracle-" + "d" * 32, ROOT, ROOT)
        with self.assertRaises(ValueError):
            oracle.create_command(
                "sha256:" + "a" * 64,
                "rwml-oracle-" + "d" * 32,
                pathlib.Path("/tmp/a,readonly=false"),
                ROOT,
            )

    @unittest.skipUnless(os.name == "posix", "POSIX capture process boundary")
    def test_bounded_process_succeeds_and_rejects_overflow_timeout_or_nonzero(self):
        result = oracle.run_bounded(
            [sys.executable, "-c", "print('ok')"], timeout=5, stdout_limit=100
        )
        self.assertEqual(result, b"ok\n")
        for source, options in [
            ("print('x'*10000)", {"stdout_limit": 20, "timeout": 5}),
            ("import time; time.sleep(5)", {"stdout_limit": 20, "timeout": 0.1}),
            ("raise SystemExit(3)", {"stdout_limit": 20, "timeout": 5}),
        ]:
            with self.subTest(source=source), self.assertRaises(ValueError):
                oracle.run_bounded([sys.executable, "-c", source], **options)

    def test_capture_archive_rejects_duplicates_links_traversal_and_missing_files(self):
        entries = [(name, b"x", tarfile.REGTYPE) for name in oracle.CAPTURE_MEMBERS]
        for invalid in [
            entries[:-1],
            entries + [entries[0]],
            entries + [("../escape", b"x", tarfile.REGTYPE)],
            [(name, data, tarfile.SYMTYPE) for name, data, _ in entries],
        ]:
            with self.assertRaises(ValueError):
                oracle.read_capture_archive(archive(invalid))
        self.assertEqual(
            set(oracle.read_capture_archive(archive(entries))), oracle.CAPTURE_MEMBERS
        )

    def test_create_failure_still_attempts_cleanup(self):
        with mock.patch.object(
            oracle, "run_bounded", side_effect=[ValueError("timed out"), b"removed"]
        ) as run:
            with self.assertRaisesRegex(ValueError, "timed out"):
                oracle.capture_document("sha256:" + "a" * 64, ROOT, ROOT)
        self.assertEqual(
            run.call_args_list[-1].args[0][:3], ["docker", "rm", "--force"]
        )

    def test_bounded_container_result_is_reusable_without_archive_parsing(self):
        name = "rwml-oracle-" + "a" * 32
        state = json.dumps(
            {"Running": False, "ExitCode": 0, "OOMKilled": False}
        ).encode()
        with mock.patch.object(
            oracle, "run_bounded", side_effect=[b"b" * 64, b"{}", state, b"removed"]
        ) as run:
            result = oracle.run_container(
                ["docker", "create", "--name", name],
                name,
                timeout=30,
                stdout_limit=1024,
            )
        self.assertEqual(result, b"{}")
        self.assertEqual(
            run.call_args_list[1].kwargs, {"timeout": 30, "stdout_limit": 1024}
        )
        self.assertEqual(
            run.call_args_list[-1].args[0], ["docker", "rm", "--force", name]
        )

    def test_bounded_container_rejects_oom_even_with_zero_exit(self):
        name = "rwml-oracle-" + "a" * 32
        state = json.dumps(
            {"Running": False, "ExitCode": 0, "OOMKilled": True}
        ).encode()
        with mock.patch.object(
            oracle, "run_bounded", side_effect=[b"b" * 64, b"{}", state, b"removed"]
        ):
            with self.assertRaisesRegex(ValueError, "complete"):
                oracle.run_container(
                    ["docker", "create", "--name", name],
                    name,
                    timeout=30,
                    stdout_limit=1024,
                )

    @unittest.skipUnless(os.name == "posix", "POSIX capture process boundary")
    def test_denied_group_cleanup_is_a_typed_failure_not_success(self):
        with mock.patch.object(
            oracle.os, "killpg", side_effect=PermissionError("denied")
        ):
            with self.assertRaisesRegex(ValueError, "cleanup failed"):
                oracle.run_bounded([sys.executable, "-c", "pass"], timeout=5)

    def test_cleanup_failure_is_not_silently_discarded(self):
        with mock.patch.object(
            oracle,
            "run_bounded",
            side_effect=[ValueError("timed out"), ValueError("daemon failed")],
        ):
            with self.assertRaisesRegex(ValueError, "cleanup failed"):
                oracle.capture_document("sha256:" + "a" * 64, ROOT, ROOT)

    def test_image_inspection_rejects_non_string_identifiers(self):
        info = sample_image()
        info["Id"] = []
        with self.assertRaises(ValueError):
            oracle.validate_image(info, sample_lock())

    @unittest.skipUnless(os.name == "posix", "POSIX capture process boundary")
    def test_bounded_process_rejects_stderr_overflow(self):
        with self.assertRaisesRegex(ValueError, "output exceeded"):
            oracle.run_bounded(
                [sys.executable, "-c", "import sys; sys.stderr.write('x'*70000)"],
                timeout=5,
            )


if __name__ == "__main__":
    unittest.main()
