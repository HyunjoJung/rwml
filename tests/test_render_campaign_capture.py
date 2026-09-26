import base64
import contextlib
import copy
import io
import json
import math
import os
import subprocess
from pathlib import Path
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))
import render_campaign_capture as capture  # noqa: E402


def font_entry(name="Regular", format_name="truetype"):
    return {
        "name": name + ".ttf",
        "postscript_name": name,
        "format": format_name,
        "sfnt_revision": 65536,
        "bytes": 6,
        "sha256": capture.digest(b"source"),
    }


def extraction(kind="truetype", name="ABCDEF+Regular"):
    program = b"font"
    return {
        "runtime_lock_sha256": "a" * 64,
        "image_manifest_sha256": "b" * 64,
        "result": {
            "fonts": [
                {
                    "ref": [5, 0],
                    "program": [6, 0],
                    "descriptor_font": name,
                    "to_unicode": None,
                }
            ],
            "blobs": [
                {
                    "ref": [6, 0],
                    "kind": kind,
                    "base64": base64.b64encode(program).decode(),
                }
            ],
        },
    }


class CaptureTests(unittest.TestCase):
    def setUp(self):
        # CI's warning policy is not an input to the mocked capture compiler.
        flags = mock.patch.dict(os.environ, {"RUSTFLAGS": ""})
        flags.start()
        self.addCleanup(flags.stop)
        disk = mock.patch(
            "shutil.disk_usage",
            return_value=SimpleNamespace(free=8 * 1024 * 1024 * 1024),
        )
        disk.start()
        self.addCleanup(disk.stop)

    def test_prepare_environment_binds_numpy_in_analysis_identity(self):
        numpy = SimpleNamespace(__version__="2.4.4")
        lock = SimpleNamespace(fonts=[])
        with (
            mock.patch.object(capture, "native_build_environment"),
            mock.patch.object(capture.shared, "load_lock", return_value=lock),
            mock.patch.object(capture.shared, "verify_pack", return_value={}),
            mock.patch.object(capture.shared, "_read_inputs", return_value={}),
            mock.patch.object(capture.runtime, "load_runtime_lock", return_value={}),
            mock.patch.object(capture.runtime, "inspect_image", return_value="image"),
            mock.patch.object(capture.attestation, "wheel_payload"),
            mock.patch.object(capture.resources, "wheel_payload"),
            mock.patch.object(capture.attestation, "tool_lock", return_value={}),
            mock.patch.object(capture.resources, "tool_lock", return_value={}),
            mock.patch.object(capture, "harness_identity", return_value={}),
            mock.patch.object(
                capture.table_capture, "execution_identity", return_value={}
            ),
            mock.patch.object(
                capture.render, "integer_metric_numpy", return_value=numpy
            ),
            mock.patch.object(
                capture.table_capture,
                "analysis_tools",
                return_value={"identity": "bound"},
            ) as analysis_tools,
        ):
            material, _, _, execution = capture.prepare_environment(
                Path("pack"), Path("fonttools.whl"), Path("pypdf.whl")
            )
        analysis_tools.assert_called_once_with({"numpy": ("numpy", "2.4.4", numpy)})
        self.assertEqual(material["analysis_tools"], {"identity": "bound"})
        self.assertEqual(
            material["native_execution"], capture.native_resource_profile()
        )
        self.assertEqual(execution, {"image": "image"})

    def test_native_build_is_offline_and_does_not_install_a_toolchain(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            compiled = root / "compiled"
            compiled.write_bytes(b"renderer")
            artifact = {
                "reason": "compiler-artifact",
                "target": {"name": "to_pdf"},
                "executable": str(compiled),
            }
            with mock.patch.object(
                capture.runtime,
                "run_bounded",
                side_effect=[json.dumps(artifact).encode(), b"rustc 1.92.0"],
            ) as run:
                capture.build_renderer(root / "retained")
            command = run.call_args_list[0].args[0]
            self.assertEqual(command[:5], ["rustup", "run", "1.92.0", "cargo", "build"])
            self.assertIn("--offline", command)
            self.assertIn("--locked", command)
            self.assertIn("--message-format=json-render-diagnostics", command)
            self.assertNotIn("--install", command)
            self.assertEqual(run.call_count, 2)

    def test_disk_check_uses_existing_ancestor_without_creating_output(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary).resolve()
            output = root / "new" / "capture"
            with mock.patch(
                "shutil.disk_usage", return_value=SimpleNamespace(free=100)
            ) as usage:
                capture.require_free_space(output, 100)
                usage.assert_called_once_with(root)
                with self.assertRaisesRegex(ValueError, "insufficient free disk space"):
                    capture.require_free_space(output, 101)
            self.assertFalse(output.parent.exists())

    def test_capture_rejects_low_disk_before_environment_or_output_creation(self):
        manifest = capture.ROOT / "corpus/public/RENDER_ORACLE.json"
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            output = root / "capture"
            with (
                mock.patch.object(
                    capture.table_capture, "source_revision", return_value="a" * 40
                ),
                mock.patch("shutil.disk_usage", return_value=SimpleNamespace(free=0)),
                mock.patch.object(capture, "prepare_environment") as prepare,
                mock.patch.object(capture, "build_renderer") as build,
            ):
                with self.assertRaisesRegex(ValueError, "insufficient free disk space"):
                    capture.run(
                        manifest, output, root / "pack", root / "ft", root / "pp"
                    )
                prepare.assert_not_called()
                build.assert_not_called()
            self.assertFalse(output.exists())

    def test_native_build_rejects_low_disk_before_compiling(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            output = root / "renderer"
            with (
                mock.patch.object(capture, "ROOT", root),
                mock.patch("shutil.disk_usage", return_value=SimpleNamespace(free=0)),
                mock.patch.object(capture.runtime, "run_bounded") as run,
            ):
                with self.assertRaisesRegex(ValueError, "insufficient free disk space"):
                    capture.build_renderer(output)
                run.assert_not_called()
            self.assertFalse((root / "target").exists())
            self.assertFalse(output.exists())

    def test_native_build_pins_reproducibility_settings_and_uses_fresh_targets(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            compiled = root / "compiled"
            compiled.write_bytes(b"renderer")
            artifact = {
                "reason": "compiler-artifact",
                "target": {"name": "to_pdf"},
                "executable": str(compiled),
            }
            builds = []

            def execute(command, **kwargs):
                if "build" not in command:
                    return b"rustc 1.92.0"
                target = Path(command[command.index("--target-dir") + 1])
                self.assertEqual(
                    target.parent, capture.ROOT / "target/render-oracle/native-builds"
                )
                self.assertTrue(target.is_dir())
                for key, value in {
                    "CARGO_INCREMENTAL": "0",
                    "CARGO_PROFILE_DEV_DEBUG": "0",
                    "CARGO_PROFILE_DEV_STRIP": "debuginfo",
                }.items():
                    self.assertEqual(kwargs["env"].get(key), value)
                builds.append(target)
                return json.dumps(artifact).encode()

            with (
                mock.patch.dict(os.environ, capture.NATIVE_BUILD_ENV),
                mock.patch.object(capture.runtime, "run_bounded", side_effect=execute),
            ):
                first = capture.build_renderer(root / "first")
                second = capture.build_renderer(root / "second")
            self.assertEqual(first, second)
            self.assertEqual(len(builds), 2)
            self.assertNotEqual(builds[0], builds[1])
            self.assertTrue(all(not target.exists() for target in builds))
            self.assertEqual((root / "first").read_bytes(), b"renderer")
            self.assertEqual((root / "second").read_bytes(), b"renderer")

    def test_native_build_cleans_its_target_after_failure(self):
        with tempfile.TemporaryDirectory() as temporary:
            targets = []

            def fail(command, **kwargs):
                target = Path(command[command.index("--target-dir") + 1])
                self.assertEqual(
                    target.parent, capture.ROOT / "target/render-oracle/native-builds"
                )
                targets.append(target)
                raise ValueError("compiler failed")

            output = Path(temporary) / "renderer"
            with mock.patch.object(capture.runtime, "run_bounded", side_effect=fail):
                with self.assertRaisesRegex(ValueError, "compiler failed"):
                    capture.build_renderer(output)
            self.assertEqual(len(targets), 1)
            self.assertFalse(targets[0].exists())
            self.assertFalse(output.exists())

    def test_native_build_failure_reports_bounded_compiler_stderr(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            error = capture.runtime.ProcessFailed(101, b"error: missing library\xff")
            with (
                mock.patch.object(capture, "ROOT", root),
                mock.patch.object(capture.runtime, "run_bounded", side_effect=error),
            ):
                with self.assertRaisesRegex(
                    ValueError, "native renderer build failed.*101.*missing library"
                ):
                    capture.build_renderer(root / "renderer")
            self.assertFalse((root / "renderer").exists())
            self.assertEqual(
                list((root / "target/render-oracle/native-builds").iterdir()), []
            )

    def test_native_build_rejects_unbound_compiler_overrides(self):
        for variable in (
            "RUSTFLAGS",
            "CARGO_ENCODED_RUSTFLAGS",
            "RUSTC",
            "RUSTC_WRAPPER",
            "RUSTC_WORKSPACE_WRAPPER",
            "CARGO_BUILD_TARGET",
            "CARGO_BUILD_RUSTFLAGS",
            "CARGO_PROFILE_DEV_OPT_LEVEL",
            "CARGO_PROFILE_DEV_DEBUG",
            "CARGO_PROFILE_DEV_STRIP",
            "CARGO_TARGET_AARCH64_APPLE_DARWIN_RUSTFLAGS",
        ):
            with (
                self.subTest(variable=variable),
                tempfile.TemporaryDirectory() as temporary,
                mock.patch.dict(os.environ, {variable: "custom-override"}),
                mock.patch.object(capture.runtime, "run_bounded") as run,
            ):
                with self.assertRaisesRegex(ValueError, variable):
                    capture.build_renderer(Path(temporary) / "renderer")
                run.assert_not_called()

    def test_native_command_preserves_locked_order_and_never_enables_fallback(self):
        paths = [Path("z.ttf"), Path("a.otf")]
        command = capture.native_command(
            Path("renderer"),
            Path("input.docx"),
            Path("out.pdf"),
            Path("report.json"),
            paths,
        )
        separator = command.index("--")
        self.assertEqual(
            command[:2],
            [sys.executable, str(capture.NATIVE_RESOURCE_LAUNCHER)],
        )
        self.assertEqual(
            command[separator + 1 :],
            [
                str(Path("renderer").absolute()),
                str(Path("input.docx").absolute()),
                str(Path("out.pdf").absolute()),
                "--report-json",
                str(Path("report.json").absolute()),
                "--font",
                str(Path("z.ttf").absolute()),
                "--font",
                str(Path("a.otf").absolute()),
            ],
        )
        for value in ("120", "16777216", "256", "64", "0"):
            self.assertIn(value, command[:separator])
        self.assertNotIn("cargo", command)
        self.assertNotIn("--fixed-fonts", command)
        with self.assertRaises(ValueError):
            capture.native_command(Path("x"), Path("i"), Path("o"), Path("r"), [])

    def test_native_resource_profile_is_explicit_about_platform_memory_limit(self):
        with mock.patch.object(capture.sys, "platform", "darwin"):
            darwin = capture.native_resource_profile()
        with mock.patch.object(capture.sys, "platform", "linux"):
            linux = capture.native_resource_profile()
        self.assertEqual(
            darwin["address_space"], {"mode": "unsupported", "bytes": None}
        )
        self.assertEqual(
            linux["address_space"],
            {"mode": "rlimit-as", "bytes": 4 * 1024 * 1024 * 1024},
        )
        self.assertEqual(darwin["file_bytes"], capture.resources.worker.MAX_PDF_BYTES)

    def test_staged_font_closure_has_sorted_container_paths_but_retains_native_order(
        self,
    ):
        entries = [font_entry("Zed"), font_entry("Alpha")]
        files = capture.font_files(
            entries, {item["name"]: b"source" for item in entries}
        )
        self.assertEqual(
            files["expected-paths.txt"],
            b"/oracle/fonts/Alpha.ttf\n/oracle/fonts/Zed.ttf\n",
        )
        self.assertEqual([item["name"] for item in entries], ["Zed.ttf", "Alpha.ttf"])

    def test_capture_metadata_rejects_changed_version_font_closure_and_pdf_digest(self):
        pdf = b"%PDF-fixture"
        fonts = b"/oracle/fonts/Regular.ttf\n"
        entries = {name: b"" for name in capture.runtime.CAPTURE_MEMBERS}
        entries.update(
            {
                "output.pdf": pdf,
                "fonts.txt": fonts,
                "version.txt": capture.runtime.VERSION_LINE.encode() + b"\n",
                "sha256.txt": (capture.digest(pdf) + "  output.pdf\n").encode(),
            }
        )
        capture.validate_capture(entries, fonts)
        for name, value in (
            ("version.txt", b"old"),
            ("fonts.txt", b"other"),
            ("sha256.txt", b"fixed"),
            ("output.pdf", b"broken"),
        ):
            with self.subTest(name=name), self.assertRaises(ValueError):
                capture.validate_capture({**entries, name: value}, fonts)

    def test_true_type_is_metadata_only_and_resource_coverage_is_complete(self):
        data = extraction()
        data["result"]["fonts"].append({**data["result"]["fonts"][0], "ref": [7, 0]})
        with (
            mock.patch.object(capture.resources, "extract_pdf", return_value=data),
            mock.patch.object(capture, "sfnt_revision", return_value=65536),
        ):
            result = capture.check_fonts(
                b"pdf", [font_entry()], {}, Path("ft"), Path("pp")
            )
        self.assertEqual(len(result["resources"]), 2)
        self.assertEqual(
            [row["font_ref"] for row in result["resources"]], [[5, 0], [7, 0]]
        )
        self.assertTrue(
            all(
                row["check"] == "postscript-and-revision-only"
                for row in result["resources"]
            )
        )

    def test_namespace_import_has_no_scripts_path_dependency(self):
        result = subprocess.run(
            [sys.executable, "-I", "-B", "-c",
             "import sys; sys.path.insert(0, sys.argv[1]); "
             "from scripts import render_campaign_capture; "
             "from scripts import posix_resource_exec",
             str(capture.ROOT)],
            capture_output=True, text=True, check=False, timeout=30,
        )
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_font_check_rejects_an_expired_final_operation_or_empty_result(self):
        for kind in ("empty", "truetype", "type1-pfa"):
            now = [0]
            data = extraction(kind)
            if kind == "empty":
                data["result"] = {"fonts": [], "blobs": []}
            entry = font_entry(format_name="opentype-cff" if kind == "type1-pfa" else "truetype")

            def extract(*args, **kwargs):
                now[0] = 181 if kind == "empty" else 170
                return data

            def late(*args, **kwargs):
                now[0] = 181
                return 65536 if kind == "truetype" else {
                    key: data[key] for key in ("runtime_lock_sha256", "image_manifest_sha256")
                }

            with (
                self.subTest(kind=kind),
                mock.patch.object(capture.time, "monotonic", side_effect=lambda: now[0]),
                mock.patch.object(capture.resources, "extract_pdf", side_effect=extract),
                mock.patch.object(capture, "sfnt_revision", side_effect=late),
                mock.patch.object(capture.attestation, "attest_program", side_effect=late),
                self.assertRaisesRegex(ValueError, "timed out"),
            ):
                capture.check_fonts(b"pdf", [entry], {entry["name"]: b"source"}, Path("ft"), Path("pp"))

    def test_type1_worker_receives_remaining_font_batch_time(self):
        now = [0]
        data = extraction("type1-pfa")
        entry = font_entry(format_name="opentype-cff")

        def extract(*args, **kwargs):
            now[0] = 175
            return data

        proof = {key: data[key] for key in ("runtime_lock_sha256", "image_manifest_sha256")}
        with (
            mock.patch.object(capture.time, "monotonic", side_effect=lambda: now[0]),
            mock.patch.object(capture.resources, "extract_pdf", side_effect=extract) as extract_pdf,
            mock.patch.object(capture.attestation, "attest_program", return_value=proof) as attest,
        ):
            capture.check_fonts(b"pdf", [entry], {entry["name"]: b"source"}, Path("ft"), Path("pp"))
        self.assertEqual(extract_pdf.call_args.kwargs.get("timeout"), 30)
        self.assertEqual(attest.call_args.kwargs.get("timeout"), 5)

    def test_font_check_timeout_is_typed_and_checked_before_workers(self):
        for timeout in (True, None, "1", 1j, 0, -1, math.inf, math.nan, 181, 2**4096):
            with self.subTest(timeout=repr(timeout)[:32]), mock.patch.object(capture.resources, "extract_pdf") as extract:
                with self.assertRaises(ValueError):
                    capture.check_fonts(b"pdf", [], {}, Path("ft"), Path("pp"), timeout=timeout)
                extract.assert_not_called()

    def test_cff_worker_receives_remaining_font_batch_time(self):
        now = [0]
        data = extraction("cid-cff")
        entry = font_entry(format_name="opentype-cff")
        cff = {"extraction": data, "cff_resources": [{"font_ref": [5, 0]}]}

        def extract(*args, **kwargs):
            now[0] = 175
            return data

        with (
            mock.patch.object(capture.time, "monotonic", side_effect=lambda: now[0]),
            mock.patch.object(capture.resources, "extract_pdf", side_effect=extract),
            mock.patch.object(capture.native, "attest_pdf", return_value=cff) as attest,
        ):
            capture.check_fonts(b"pdf", [entry], {entry["name"]: b"source"}, Path("ft"), Path("pp"))
        self.assertEqual(attest.call_args.kwargs["timeout"], 5)

    def test_native_replay_rejects_expired_budget_before_build(self):
        with (
            mock.patch.object(capture.time, "monotonic", return_value=10),
            mock.patch.object(capture, "build_renderer") as build,
        ):
            with self.assertRaisesRegex(ValueError, "timed out"):
                capture.verify_native_outputs(Path("unused"), None, {}, None, 10)
            build.assert_not_called()

    def test_pdf_font_check_receives_remaining_campaign_budget(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            pdf = root / "input.pdf"
            pdf.write_bytes(b"%PDF-fixture")
            with (
                mock.patch.object(capture.time, "monotonic", return_value=10),
                mock.patch.object(
                    capture, "check_fonts", return_value={"resources": []},
                ) as check,
            ):
                capture.pdf_record(
                    pdf, root / "fonts.json", {}, SimpleNamespace(fonts=[]),
                    root / "ft", root / "pp", verify=False, deadline=15,
                )
            self.assertEqual(check.call_args.kwargs["timeout"], 5)

    def test_native_build_timeout_is_typed_before_environment_or_output(self):
        for timeout in (True, None, "1", 0, -1, math.inf, math.nan, 901, 2**4096):
            with (
                self.subTest(timeout=repr(timeout)[:32]),
                mock.patch.object(capture, "native_build_environment") as environment,
                mock.patch.object(capture.runtime, "run_bounded") as execute,
            ):
                with self.assertRaises(ValueError):
                    capture.build_renderer(Path("unused"), timeout=timeout)
                environment.assert_not_called()
                execute.assert_not_called()

    def test_unknown_font_or_wrong_true_type_revision_fails(self):
        for data, revision in ((extraction(name="Unknown"), 65536), (extraction(), 1)):
            with (
                mock.patch.object(capture.resources, "extract_pdf", return_value=data),
                mock.patch.object(capture, "sfnt_revision", return_value=revision),
            ):
                with self.assertRaises(ValueError):
                    capture.check_fonts(
                        b"pdf", [font_entry()], {}, Path("ft"), Path("pp")
                    )

    def test_cff_requires_independent_proof_and_matching_extraction(self):
        data = extraction("cid-cff")
        entry = font_entry(format_name="opentype-cff")
        cff = {"extraction": data, "cff_resources": [{"font_ref": [5, 0], "proof": {}}]}
        with (
            mock.patch.object(capture.resources, "extract_pdf", return_value=data),
            mock.patch.object(capture.native, "attest_pdf", return_value=cff),
        ):
            result = capture.check_fonts(
                b"pdf", [entry], {entry["name"]: b"source"}, Path("ft"), Path("pp")
            )
            self.assertEqual(result["resources"][0]["check"], "exact-glyph-outlines")
            changed = copy.deepcopy(cff)
            changed["extraction"]["image_manifest_sha256"] = "c" * 64
            with mock.patch.object(capture.native, "attest_pdf", return_value=changed):
                with self.assertRaisesRegex(ValueError, "extraction"):
                    capture.check_fonts(
                        b"pdf",
                        [entry],
                        {entry["name"]: b"source"},
                        Path("ft"),
                        Path("pp"),
                    )

    def test_type1_proof_must_match_extraction_runtime(self):
        data = extraction("type1-pfa")
        entry = font_entry(format_name="opentype-cff")
        proof = {"runtime_lock_sha256": "c" * 64, "image_manifest_sha256": "b" * 64}
        with (
            mock.patch.object(capture.resources, "extract_pdf", return_value=data),
            mock.patch.object(
                capture.attestation, "attest_program", return_value=proof
            ),
        ):
            with self.assertRaisesRegex(ValueError, "runtime"):
                capture.check_fonts(
                    b"pdf", [entry], {entry["name"]: b"source"}, Path("ft"), Path("pp")
                )

    def test_no_font_resource_is_an_explicit_empty_inventory_not_a_glyph_proof(self):
        data = extraction()
        data["result"] = {"fonts": [], "blobs": []}
        with mock.patch.object(capture.resources, "extract_pdf", return_value=data):
            result = capture.check_fonts(b"pdf", [], {}, Path("ft"), Path("pp"))
        self.assertEqual(result["resources"], [])
        self.assertEqual(result["scope"], "declared-font-resources")

    def test_verify_files_rejects_missing_extra_symlink_and_modified_artifact(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root / "pdf").write_bytes(b"original")
            capture.verify_files(root, {"pdf": b"original"})
            for expected in (
                {},
                {"pdf": b"changed"},
                {"pdf": b"original", "missing": b"x"},
            ):
                with self.assertRaises(ValueError):
                    capture.verify_files(root, expected)
            (root / "link").symlink_to(root / "pdf")
            with self.assertRaises(ValueError):
                capture.verify_files(root, {"pdf": b"original", "link": b"original"})

    def test_source_payload_must_match_manifest_before_any_render(self):
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "source.docx"
            path.write_bytes(b"original")
            document = SimpleNamespace(
                path=path, input_bytes=8, sha256=capture.digest(b"original")
            )
            self.assertEqual(capture.source_payload(document), b"original")
            path.write_bytes(b"modified")
            with self.assertRaises(ValueError):
                capture.source_payload(document)

    def test_capture_and_verify_compose_every_case_and_reject_repaired_receipts(self):
        self._exercise_capture_roundtrip("docx")

    def test_legacy_doc_capture_keeps_source_bytes_and_manifest_format(self):
        self._exercise_capture_roundtrip("doc")

    def _exercise_capture_roundtrip(self, format_name):
        from test_render_oracle_contract import valid_manifest, write_manifest

        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source_root = root / "corpus"
            source_root.mkdir()
            document_bytes = b"fixture"
            if format_name == "doc":
                document_bytes = bytes.fromhex("d0cf11e0a1b11ae1") + document_bytes
            data = valid_manifest(document_bytes)
            data["documents"][0]["format"] = format_name
            data["documents"][0]["path"] = f"synthetic/fixture.{format_name}"
            manifest = write_manifest(source_root, data, document_bytes)
            if format_name == "doc":
                (source_root / "synthetic/fixture.docx").rename(
                    source_root / "synthetic/fixture.doc"
                )
            output = root / "capture"
            entry = font_entry()
            lock = SimpleNamespace(fonts=[entry])
            sources = {entry["name"]: b"source"}
            material = {
                "identity": "fixed",
                "analysis_tools": {"identity": "fixed"},
            }
            expected_fonts = capture.font_files(lock.fonts, sources)[
                "expected-paths.txt"
            ]
            captured = {name: b"" for name in capture.runtime.CAPTURE_MEMBERS}
            captured.update(
                {
                    "version.txt": capture.runtime.VERSION_LINE.encode(),
                    "fonts.txt": expected_fonts,
                    "output.pdf": b"%PDF-reference",
                    "sha256.txt": (
                        capture.digest(b"%PDF-reference") + "  output.pdf\n"
                    ).encode(),
                }
            )

            def build(path, **kwargs):
                path.write_bytes(b"renderer")
                return {
                    **capture.identity(b"renderer"),
                    "rustc": "rustc 1.92.0",
                    "features": ["render"],
                    "profile": "dev",
                    "cargo_lock_sha256": capture.digest(
                        (capture.ROOT / "Cargo.lock").read_bytes()
                    ),
                }

            def execute(command, **kwargs):
                self.assertEqual(kwargs["timeout"], capture.NATIVE_WALL_SECONDS)
                payload = command[command.index("--") + 1 :]
                if (output / "CAPTURE.json").exists():
                    self.assertNotEqual(Path(payload[0]), output / "renderer")
                    self.assertEqual(Path(payload[0]).read_bytes(), b"renderer")
                    self.assertNotEqual(Path(payload[1]), output / "cases/fixture-basic/input.docx")
                self.assertEqual(
                    payload[-2:],
                    ["--font", str((output / "fonts/Regular.ttf").absolute())],
                )
                Path(payload[2]).write_bytes(b"%PDF-native")
                Path(payload[4]).write_text('{"warnings": []}')
                return b""

            check = {
                "resources": [
                    {
                        "font_ref": [5, 0],
                        "source": "Regular.ttf",
                        "kind": "truetype",
                        "check": "postscript-and-revision-only",
                    }
                ]
            }
            args = (manifest, output, root / "pack", root / "ft", root / "pp")
            with (
                mock.patch.object(
                    capture.table_capture, "source_revision", return_value="a" * 40
                ),
                mock.patch.object(
                    capture,
                    "prepare_environment",
                    return_value=(material, sources, lock, {"image": "fixed"}),
                ),
                mock.patch.object(
                    capture, "build_renderer", side_effect=build
                ) as builds,
                mock.patch.object(
                    capture.runtime, "capture_document", return_value=captured
                ) as conversions,
                mock.patch.object(capture.runtime, "run_bounded", side_effect=execute),
                mock.patch.object(capture, "check_fonts", return_value=check) as checks,
                contextlib.redirect_stderr(io.StringIO()),
            ):
                failed_output = root / "disk-exhausted"
                with mock.patch(
                    "shutil.disk_usage",
                    side_effect=[
                        SimpleNamespace(free=capture.MAX_CAMPAIGN_BYTES),
                        SimpleNamespace(free=0),
                    ],
                ):
                    with self.assertRaisesRegex(
                        ValueError, "insufficient free disk space"
                    ):
                        capture.run(manifest, failed_output, *args[2:])
                conversions.assert_not_called()
                checks.assert_not_called()
                self.assertEqual(list((failed_output / "cases").iterdir()), [])
                self.assertEqual((failed_output / "renderer").read_bytes(), b"renderer")
                self.assertFalse((failed_output / "CAPTURE.json").exists())
                builds.reset_mock()
                result = capture.run(*args)
                self.assertEqual(
                    (output / "cases/fixture-basic/input.docx").read_bytes(),
                    document_bytes,
                )
                self.assertEqual(
                    capture.load_corpus_manifest(manifest).documents[0].format,
                    format_name,
                )
                self.assertEqual(len(result["rows"]), 1)
                self.assertEqual(builds.call_count, 1)
                self.assertEqual(conversions.call_count, 1)
                self.assertEqual(checks.call_count, 2)
                with mock.patch(
                    "shutil.disk_usage", return_value=SimpleNamespace(free=0)
                ):
                    # Only the independently rebuilt renderer needs writable space.
                    self.assertEqual(capture.run(*args, verify=True), result)
                self.assertEqual(builds.call_count, 2)
                self.assertEqual(conversions.call_count, 1)
                self.assertEqual(checks.call_count, 4)
                self.assertEqual(
                    list((capture.ROOT / "target/render-oracle/capture-build-verification").iterdir()),
                    [],
                )
                before = (output / "CAPTURE.json").read_bytes()
                with mock.patch.object(
                    capture.runtime, "run_bounded", side_effect=ValueError("replay process failed"),
                ):
                    with self.assertRaisesRegex(ValueError, "replay process failed"):
                        capture.run(*args, verify=True)
                self.assertEqual((output / "CAPTURE.json").read_bytes(), before)
                self.assertEqual(
                    list((capture.ROOT / "target/render-oracle/capture-build-verification").iterdir()),
                    [],
                )
                for key, value in (
                    ("source_revision", "b" * 40),
                    ("environment", {"identity": "wrong runtime"}),
                    ("campaign", {"identity": "wrong input"}),
                    ("rows", result["rows"] * 2),
                    ("rows", []),
                    ("renderer", {**result["renderer"], "profile": "release"}),
                    ("renderer", {**result["renderer"], "features": ["bundled-fonts"]}),
                ):
                    altered = {**result, key: value}
                    (output / "CAPTURE.json").write_bytes(capture.canonical(altered))
                    with self.subTest(key=key, value=value), self.assertRaises(ValueError):
                        capture.run(*args, verify=True)
                    self.assertEqual(
                        (output / "CAPTURE.json").read_bytes(), capture.canonical(altered),
                    )
                    (output / "CAPTURE.json").write_bytes(before)
                case = output / "cases/fixture-basic"
                moved = output / "cases/undeclared"
                case.rename(moved)
                with self.assertRaisesRegex(ValueError, "coverage"):
                    capture.run(*args, verify=True)
                moved.rename(case)
                (output / "extra").write_bytes(b"extra")
                with self.assertRaisesRegex(ValueError, "coverage"):
                    capture.run(*args, verify=True)
                (output / "extra").unlink()
                for name, value in (
                    ("native.pdf", b"%PDF-tampered"),
                    ("native-report.json", b'{"warnings": [], "pages": 100}'),
                ):
                    path = output / "cases/fixture-basic" / name
                    original = path.read_bytes()
                    altered = copy.deepcopy(result)
                    path.write_bytes(value)
                    if name == "native.pdf":
                        altered["rows"][0]["native"]["pdf"] = capture.identity(value)
                    else:
                        altered["rows"][0]["native_report"] = capture.identity(value)
                    (output / "CAPTURE.json").write_bytes(capture.canonical(altered))
                    with self.subTest(name=name), self.assertRaisesRegex(ValueError, "native .* replay"):
                        capture.run(*args, verify=True)
                    self.assertEqual(path.read_bytes(), value)
                    path.write_bytes(original)
                    (output / "CAPTURE.json").write_bytes(before)
                with self.assertRaisesRegex(ValueError, "fresh"):
                    capture.run(*args)
                self.assertEqual((output / "CAPTURE.json").read_bytes(), before)
                changed_binary = b"different renderer"
                altered = copy.deepcopy(result)
                altered["renderer"].update(capture.identity(changed_binary))
                (output / "renderer").write_bytes(changed_binary)
                (output / "CAPTURE.json").write_text(json.dumps(altered))
                with self.assertRaisesRegex(ValueError, "independently rebuilt"):
                    capture.run(*args, verify=True)
                (output / "renderer").write_bytes(b"renderer")
                (output / "CAPTURE.json").write_bytes(before)
                receipt = output / "cases/fixture-basic/native-fonts.json"
                changed = copy.deepcopy(check)
                changed["resources"][0]["source"] = "Other.ttf"
                payload = capture.canonical(changed) + b"\n"
                receipt.write_bytes(payload)
                altered = copy.deepcopy(result)
                altered["rows"][0]["native"]["font_checks"] = capture.identity(payload)
                altered["rows"][0]["native"]["resources"] = changed["resources"]
                (output / "CAPTURE.json").write_text(json.dumps(altered))
                with self.assertRaisesRegex(ValueError, "recomputed"):
                    capture.run(*args, verify=True)
                self.assertEqual(receipt.read_bytes(), payload)

    @unittest.skipUnless(os.name == "posix", "process-group runner requires POSIX")
    def test_bounded_process_accepts_explicit_working_directory_and_environment(self):
        with tempfile.TemporaryDirectory() as temporary:
            result = capture.runtime.run_bounded(
                [
                    sys.executable,
                    "-c",
                    "import os; print(os.environ['CAPTURE_TEST']); print(os.getcwd())",
                ],
                cwd=Path(temporary),
                env={"CAPTURE_TEST": "isolated"},
            )
            self.assertEqual(
                result.decode().splitlines(),
                ["isolated", str(Path(temporary).resolve())],
            )


if __name__ == "__main__":
    unittest.main()
