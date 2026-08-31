import base64
import contextlib
import copy
import io
import json
import os
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
            self.assertNotIn("--install", command)
            self.assertEqual(run.call_count, 2)

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

    def test_capture_cli_rejects_partial_and_conflicting_profiles_before_verification(
        self,
    ):
        manifest = capture.ROOT / "corpus/public/RENDER_SMOKE_ORACLE.json"
        base = [
            "render_validate.py",
            "--manifest",
            str(manifest),
            "--json",
            "--capture-dir",
            "capture",
        ]
        full = base + [
            "--shared-font-pack",
            "pack",
            "--fonttools-wheel",
            "ft",
            "--pypdf-wheel",
            "pp",
        ]
        for argv in (
            base,
            full + ["--system-fonts"],
            full + ["--verify-oracle"],
            full + ["--soffice", "docker"],
        ):
            with (
                self.subTest(argv=argv),
                mock.patch.object(sys, "argv", argv),
                mock.patch.object(
                    capture.render, "captured_validation_report"
                ) as verify,
                contextlib.redirect_stderr(io.StringIO()),
                self.assertRaises(SystemExit),
            ):
                capture.render.main()
            verify.assert_not_called()

    def test_shared_capture_metrics_require_a_distinct_schema_and_complete_binding(
        self,
    ):
        from test_render_oracle_contract import (
            valid_manifest,
            write_manifest,
            valid_environment,
            valid_core_report,
        )
        import render_oracle_contract as contract

        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            manifest = write_manifest(root, valid_manifest())
            corpus = contract.load_corpus_manifest(manifest)
            environment = valid_environment()
            environment["renderer"]["font_mode"] = "locked-shared-fonts"
            environment["oracle"]["mode"] = "locked-container"
            report = valid_core_report()
            report["visual_comparison"]["font_mode"] = "locked-shared-fonts"
            report["summary"]["reference_stable"] = None
            binding = {
                "schema": capture.SCHEMA,
                "sha256": "e" * 64,
                "environment_sha256": environment["oracle"]["identity_sha256"],
                "source_revision": environment["source_revision"],
                "campaign": corpus.identity(),
                "renderer_sha256": "f" * 64,
                "font_scope": "declared-font-resources",
                "cases": [
                    {
                        "case_id": doc.case_id,
                        "input_sha256": doc.sha256,
                        "native_pdf_sha256": "1" * 64,
                        "reference_pdf_sha256": "2" * 64,
                        "native_fonts_sha256": "3" * 64,
                        "reference_fonts_sha256": "4" * 64,
                    }
                    for doc in corpus.documents
                ],
            }
            evidence = contract.bind_evidence_report(
                report, corpus, environment, capture=binding
            )
            self.assertEqual(evidence["schema"], "rwml.render-oracle-evidence.v5")
            for mutation in (
                "missing",
                "duplicate",
                "input",
                "environment",
                "revision",
                "downgrade",
            ):
                changed = copy.deepcopy(evidence)
                if mutation == "missing":
                    changed["capture"]["cases"] = []
                elif mutation == "duplicate":
                    changed["capture"]["cases"] *= 2
                elif mutation == "input":
                    changed["capture"]["cases"][0]["input_sha256"] = "0" * 64
                elif mutation == "environment":
                    changed["capture"]["environment_sha256"] = "0" * 64
                elif mutation == "revision":
                    changed["capture"]["source_revision"] = "0" * 40
                else:
                    changed["schema"] = contract.EVIDENCE_SCHEMA
                    del changed["capture"]
                with self.subTest(mutation=mutation), self.assertRaises(ValueError):
                    contract.validate_evidence_report(changed, corpus)

    def test_native_command_preserves_locked_order_and_never_enables_fallback(self):
        paths = [Path("z.ttf"), Path("a.otf")]
        command = capture.native_command(
            Path("renderer"),
            Path("input.docx"),
            Path("out.pdf"),
            Path("report.json"),
            paths,
        )
        self.assertEqual(
            command,
            [
                "renderer",
                "input.docx",
                "out.pdf",
                "--report-json",
                "report.json",
                "--font",
                "z.ttf",
                "--font",
                "a.otf",
            ],
        )
        self.assertNotIn("cargo", command)
        self.assertNotIn("--fixed-fonts", command)
        with self.assertRaises(ValueError):
            capture.native_command(Path("x"), Path("i"), Path("o"), Path("r"), [])

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
        from test_render_oracle_contract import valid_manifest, write_manifest

        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source_root = root / "corpus"
            source_root.mkdir()
            manifest = write_manifest(source_root, valid_manifest())
            output = root / "capture"
            entry = font_entry()
            lock = SimpleNamespace(fonts=[entry])
            sources = {entry["name"]: b"source"}
            material = {
                "identity": "fixed",
                "analysis_tools": capture.table_capture.analysis_tools()
                if capture.render.fitz
                else {"python": "test", "pymupdf": "test", "pillow": "test"},
            }
            numpy = capture.render.integer_metric_numpy()
            if numpy is not None:
                material["analysis_tools"]["numpy"] = str(numpy.__version__)
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

            def build(path):
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
                self.assertEqual(kwargs["timeout"], 120)
                self.assertEqual(
                    command[-2:], ["--font", str(output / "fonts/Regular.ttf")]
                )
                Path(command[2]).write_bytes(b"%PDF-native")
                Path(command[4]).write_text('{"warnings": []}')
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
                result = capture.run(*args)
                self.assertEqual(len(result["rows"]), 1)
                self.assertEqual(builds.call_count, 1)
                self.assertEqual(conversions.call_count, 1)
                self.assertEqual(checks.call_count, 2)
                self.assertEqual(capture.run(*args, verify=True), result)
                self.assertEqual(builds.call_count, 2)
                self.assertEqual(conversions.call_count, 1)
                self.assertEqual(checks.call_count, 4)
                from test_render_oracle_contract import valid_core_report

                core = valid_core_report()
                values = core["rows"][0]
                visual = SimpleNamespace(
                    **{
                        name: values[name]
                        for name in (
                            "mean_page_ahash_similarity",
                            "foreground_ink_iou",
                            "compared_pages",
                            "unmatched_candidate_pages",
                            "unmatched_reference_pages",
                            "capped_matched_pages",
                            "integer_visual_metrics",
                            "pdf_point_geometry",
                            "semantic_text_metrics",
                            "text_geometry_metrics",
                        )
                    }
                )
                arguments = SimpleNamespace(
                    capture_dir=output,
                    shared_font_pack=args[2],
                    fonttools_wheel=args[3],
                    pypdf_wheel=args[4],
                    source_revision=None,
                    recall_min=0.97,
                )
                corpus = capture.load_corpus_manifest(manifest)
                with (
                    mock.patch.object(capture.render, "text_recall", return_value=1.0),
                    mock.patch.object(capture.render, "page_count", return_value=1),
                    mock.patch.object(
                        capture.render, "hash_similarity", return_value=1.0
                    ),
                    mock.patch.object(
                        capture.render, "compare_pdf_visuals", return_value=visual
                    ),
                    mock.patch.object(capture.render, "verify_campaign_inputs"),
                ):
                    metrics = capture.render.captured_validation_report(
                        arguments,
                        corpus,
                        {},
                        capture.render.validate_visual_settings(
                            {"font_mode": "locked-shared-fonts"}
                        ),
                    )
                self.assertEqual(metrics["schema"], "rwml.render-oracle-evidence.v5")
                self.assertEqual(metrics["summary"]["skipped"], 0)
                self.assertIsNone(metrics["summary"]["reference_stable"])
                self.assertEqual(
                    metrics["capture"]["cases"][0]["native_pdf_sha256"],
                    capture.digest(b"%PDF-native"),
                )
                before = (output / "CAPTURE.json").read_bytes()
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
