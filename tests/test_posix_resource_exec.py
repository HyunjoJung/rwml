import importlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest import mock


ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "scripts"))
launcher = importlib.import_module("posix_resource_exec")


@unittest.skipUnless(os.name == "posix", "POSIX resource boundary")
class PosixResourceExecTests(unittest.TestCase):
    def test_apply_limits_sets_and_verifies_exact_kernel_values(self):
        current = (10_000_000_000, 10_000_000_000)
        observed = {}

        def set_limit(key, value):
            observed[key] = value

        def get_limit(key):
            return observed.get(key, current)

        with (
            mock.patch.object(launcher.resource, "setrlimit", side_effect=set_limit),
            mock.patch.object(launcher.resource, "getrlimit", side_effect=get_limit),
        ):
            launcher.apply_limits(
                cpu_seconds=120,
                file_bytes=16 * 1024 * 1024,
                open_files=256,
                processes=64,
                core_bytes=0,
                address_space_bytes=4 * 1024 * 1024 * 1024,
            )
        self.assertEqual(
            observed,
            {
                launcher.resource.RLIMIT_CORE: (0, 0),
                launcher.resource.RLIMIT_CPU: (120, 120),
                launcher.resource.RLIMIT_FSIZE: (16 * 1024 * 1024,) * 2,
                launcher.resource.RLIMIT_NOFILE: (256, 256),
                launcher.resource.RLIMIT_NPROC: (64, 64),
                launcher.resource.RLIMIT_AS: (4 * 1024 * 1024 * 1024,) * 2,
            },
        )

    def test_main_applies_limits_before_exact_exec(self):
        command = ["/absolute/renderer", "input.docx", "output.pdf"]
        with (
            mock.patch.object(launcher, "apply_limits") as apply,
            mock.patch.object(launcher.os, "execv") as execute,
        ):
            self.assertEqual(
                launcher.main(
                    [
                        "--cpu-seconds",
                        "120",
                        "--file-bytes",
                        "16777216",
                        "--open-files",
                        "256",
                        "--processes",
                        "64",
                        "--core-bytes",
                        "0",
                        "--",
                        *command,
                    ]
                ),
                0,
            )
        apply.assert_called_once_with(
            cpu_seconds=120,
            file_bytes=16 * 1024 * 1024,
            open_files=256,
            processes=64,
            core_bytes=0,
            address_space_bytes=None,
        )
        execute.assert_called_once_with(command[0], command)

    def test_file_size_limit_stops_child_output_growth(self):
        with tempfile.TemporaryDirectory() as temporary:
            output = Path(temporary) / "oversized.bin"
            command = [
                sys.executable,
                str(ROOT / "scripts/posix_resource_exec.py"),
                "--cpu-seconds",
                "5",
                "--file-bytes",
                "1024",
                "--open-files",
                "64",
                "--processes",
                "64",
                "--core-bytes",
                "0",
                "--",
                sys.executable,
                "-c",
                (
                    "import sys; stream=open(sys.argv[1],'wb'); "
                    "stream.write(b'x'*4096); stream.flush()"
                ),
                str(output),
            ]
            result = subprocess.run(command, capture_output=True, check=False)
            self.assertNotEqual(result.returncode, 0)
            self.assertLessEqual(output.stat().st_size, 1024)

    def test_exec_child_observes_exact_kernel_limits(self):
        source = (
            "import json,resource; "
            "names=('RLIMIT_CORE','RLIMIT_CPU','RLIMIT_FSIZE','RLIMIT_NOFILE','RLIMIT_NPROC'); "
            "print(json.dumps({name:list(resource.getrlimit(getattr(resource,name))) "
            "for name in names},sort_keys=True))"
        )
        command = [
            sys.executable,
            str(ROOT / "scripts/posix_resource_exec.py"),
            "--cpu-seconds",
            "5",
            "--file-bytes",
            "4096",
            "--open-files",
            "64",
            "--processes",
            "64",
            "--core-bytes",
            "0",
            "--",
            sys.executable,
            "-c",
            source,
        ]
        result = subprocess.run(command, capture_output=True, check=True)
        self.assertEqual(
            json.loads(result.stdout),
            {
                "RLIMIT_CORE": [0, 0],
                "RLIMIT_CPU": [5, 5],
                "RLIMIT_FSIZE": [4096, 4096],
                "RLIMIT_NOFILE": [64, 64],
                "RLIMIT_NPROC": [64, 64],
            },
        )

    @unittest.skipUnless(sys.platform.startswith("linux"), "Linux address-space limit")
    def test_linux_child_observes_address_space_limit(self):
        command = [
            sys.executable,
            str(ROOT / "scripts/posix_resource_exec.py"),
            "--cpu-seconds",
            "5",
            "--file-bytes",
            "4096",
            "--open-files",
            "64",
            "--processes",
            "64",
            "--core-bytes",
            "0",
            "--address-space-bytes",
            str(4 * 1024 * 1024 * 1024),
            "--",
            sys.executable,
            "-c",
            "import json,resource; print(json.dumps(resource.getrlimit(resource.RLIMIT_AS)))",
        ]
        result = subprocess.run(command, capture_output=True, check=True)
        self.assertEqual(
            json.loads(result.stdout),
            [4 * 1024 * 1024 * 1024, 4 * 1024 * 1024 * 1024],
        )


if __name__ == "__main__":
    unittest.main()
