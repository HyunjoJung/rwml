import copy
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import struct
import sys
import tempfile
import unittest
from unittest import mock

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "scripts"))
import shared_oracle_fonts as fonts  # noqa: E402


def sha(payload):
    return hashlib.sha256(payload).hexdigest()


def blob(payload):
    return hashlib.sha1(
        b"blob " + str(len(payload)).encode() + b"\0" + payload
    ).hexdigest()


def sfnt(signature=b"\x00\x01\x00\x00"):
    payload = bytearray(64)
    payload[:4] = signature
    struct.pack_into(">H", payload, 4, 1)
    payload[12:16] = b"head"
    struct.pack_into(">II", payload, 20, 32, 16)
    struct.pack_into(">I", payload, 36, 65536)
    return bytes(payload)


class SharedOracleFontTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.fonts = self.root / "input-fonts"
        self.licenses = self.root / "input-licenses"
        self.fonts.mkdir()
        self.licenses.mkdir()
        payload = sfnt()
        entry = {
            "asset_member": "Locked/Locked-Regular.ttf",
            "bytes": len(payload),
            "name": "Locked-Regular.ttf",
            "postscript_name": "Locked-Regular",
            "sfnt_revision": 65536,
            "sha256": sha(payload),
            "style": "Regular",
        }
        self.base = self.root / "base.json"
        self.base.write_text(
            json.dumps(
                {
                    "schema": "rwml.libreoffice-oracle-font-lock.v1",
                    "license": "SIL-OFL-1.1",
                    "families": [
                        {
                            "family": "Locked",
                            "files": [entry],
                            "source": {
                                "repository": "example/fonts",
                                "release_tag": "v1",
                                "target_commit": "1" * 40,
                                "asset": {
                                    "name": "fonts.zip",
                                    "bytes": 100,
                                    "sha256": "a" * 64,
                                },
                            },
                        }
                    ],
                }
            )
        )
        (self.fonts / entry["name"]).write_bytes(payload)
        license_bytes = b"synthetic OFL fixture\n"
        addition = {
            "family": "Extra",
            "name": "Extra-Regular.ttf",
            "style": "Regular",
            "postscript_name": "Extra-Regular",
            "sfnt_revision": 65536,
            "bytes": len(payload),
            "sha256": sha(payload),
            "format": "truetype",
            "source": {
                "kind": "github-blob",
                "repository": "example/extra",
                "target_commit": "2" * 40,
                "font": {
                    "path": "fonts/Extra-Regular.ttf",
                    "git_blob_sha1": blob(payload),
                },
                "license": {
                    "path": "OFL.txt",
                    "name": "Extra-OFL.txt",
                    "bytes": len(license_bytes),
                    "sha256": sha(license_bytes),
                    "git_blob_sha1": blob(license_bytes),
                },
            },
        }
        (self.fonts / addition["name"]).write_bytes(payload)
        (self.licenses / "Extra-OFL.txt").write_bytes(license_bytes)
        self.document = {
            "schema": "rwml.libreoffice-oracle-font-lock.v2",
            "license": "SIL-OFL-1.1",
            "base_lock": {"name": "base.json", "sha256": sha(self.base.read_bytes())},
            "additions": [addition],
            "font_order": [entry["name"], addition["name"]],
        }
        self.lock_path = self.root / "shared.json"

    def load(self):
        self.lock_path.write_text(json.dumps(self.document))
        return fonts.load_lock(self.lock_path, self.base)

    def prepare(self):
        return fonts.prepare_pack(
            self.fonts, self.licenses, self.root / "pack", self.load()
        )

    def test_public_lock_extends_eight_fonts_without_replacing_v1(self):
        lock = fonts.load_lock()
        self.assertEqual(len(lock.fonts), 10)
        self.assertEqual(len(lock.licenses), 2)
        self.assertEqual(lock.fonts[0]["name"], "NotoSans-Regular.ttf")
        self.assertEqual(lock.fonts[-1]["name"], "NotoEmoji[wght].ttf")
        self.assertEqual(
            {entry["format"] for entry in lock.fonts},
            {"truetype", "opentype-cff", "truetype-variable"},
        )

    def test_prepared_pack_is_independently_verifiable_and_path_neutral(self):
        first = self.prepare()
        second = fonts.verify_pack(self.root / "pack", self.load())
        self.assertEqual(first, second)
        self.assertNotIn(str(self.root), json.dumps(second))
        self.assertEqual(
            [entry["name"] for entry in second["fonts"]], self.document["font_order"]
        )

    def test_source_lock_and_font_order_are_exact(self):
        original = copy.deepcopy(self.document)
        for mutate in (
            lambda x: x["base_lock"].update(sha256="0" * 64),
            lambda x: x["base_lock"].update(name="other.json"),
            lambda x: x.update(font_order=x["font_order"][:1]),
            lambda x: x.update(font_order=x["font_order"] * 2),
            lambda x: x.update(font_order=["unknown.ttf", "Extra-Regular.ttf"]),
        ):
            with self.subTest(mutate=mutate):
                self.document = copy.deepcopy(original)
                mutate(self.document)
                with self.assertRaises(ValueError):
                    self.load()

    def test_source_paths_and_blob_identities_fail_closed(self):
        for path in (
            "../Extra-Regular.ttf",
            "/Extra-Regular.ttf",
            "a//Extra-Regular.ttf",
            "./Extra-Regular.ttf",
            "a\\Extra-Regular.ttf",
            "a/Other.ttf",
        ):
            with self.subTest(path=path):
                self.document["additions"][0]["source"]["font"]["path"] = path
                with self.assertRaises(ValueError):
                    self.load()
        self.document["additions"][0]["source"]["font"]["path"] = (
            "fonts/Extra-Regular.ttf"
        )
        self.document["additions"][0]["source"]["font"]["git_blob_sha1"] = "0" * 40
        with self.assertRaisesRegex(ValueError, "blob"):
            self.prepare()

    def test_ambiguous_names_and_non_integer_sizes_are_rejected(self):
        original = copy.deepcopy(self.document)
        for change in (
            {"bytes": True},
            {"sfnt_revision": 65536.0},
            {"bytes": -1},
            {"name": "locked-regular.ttf"},
            {"postscript_name": "Locked-Regular"},
            {"unknown": 1},
            {"format": "opentype-cff"},
        ):
            with self.subTest(change=change):
                self.document = copy.deepcopy(original)
                self.document["additions"][0].update(change)
                with self.assertRaises(ValueError):
                    self.load()

    def test_prepare_rejects_changed_fonts_and_license_without_creating_output(self):
        for path in (self.fonts / "Extra-Regular.ttf", self.licenses / "Extra-OFL.txt"):
            original = path.read_bytes()
            path.write_bytes(original + b"changed")
            with self.assertRaises(ValueError):
                self.prepare()
            self.assertFalse((self.root / "pack").exists())
            path.write_bytes(original)

    def test_prepare_rejects_extra_files_and_existing_outputs(self):
        extra = self.fonts / "untracked.ttf"
        extra.write_bytes(b"extra")
        with self.assertRaisesRegex(ValueError, "file set"):
            self.prepare()
        extra.unlink()
        self.prepare()
        with self.assertRaisesRegex(ValueError, "fresh"):
            fonts.prepare_pack(
                self.fonts, self.licenses, self.root / "pack", self.load()
            )

    def test_verify_rejects_forged_receipts_and_missing_payloads(self):
        self.prepare()
        receipt_path = self.root / "pack/MANIFEST.json"
        original = receipt_path.read_bytes()
        receipt = json.loads(original)
        receipt["fonts"][0]["sfnt_revision"] = 65536.0
        receipt_path.write_text(json.dumps(receipt))
        with self.assertRaises(ValueError):
            fonts.verify_pack(self.root / "pack", self.load())
        receipt_path.write_bytes(original)
        (self.root / "pack/fonts/Extra-Regular.ttf").unlink()
        with self.assertRaisesRegex(ValueError, "file set"):
            fonts.verify_pack(self.root / "pack", self.load())

    def test_verify_rejects_symlinked_font_and_directory(self):
        self.prepare()
        font = self.root / "pack/fonts/Extra-Regular.ttf"
        font.unlink()
        font.symlink_to(self.fonts / font.name)
        with self.assertRaises(ValueError):
            fonts.verify_pack(self.root / "pack", self.load())
        alias = self.root / "alias"
        alias.symlink_to(self.root / "pack", target_is_directory=True)
        with self.assertRaises(ValueError):
            fonts.verify_pack(alias, self.load())

    @unittest.skipUnless(hasattr(os, "mkfifo"), "FIFO boundary is POSIX-specific")
    def test_input_fifo_is_rejected_without_blocking(self):
        path = self.fonts / "Extra-Regular.ttf"
        path.unlink()
        os.mkfifo(path)
        with self.assertRaises(ValueError):
            self.prepare()

    def test_duplicate_json_keys_are_rejected(self):
        self.load()
        self.lock_path.write_text('{"schema":"a","schema":"b"}')
        with self.assertRaises(ValueError):
            fonts.load_lock(self.lock_path, self.base)

    def test_declared_source_format_must_match_metadata(self):
        with self.assertRaisesRegex(ValueError, "CFF"):
            fonts._source_revision(sfnt(b"OTTO"), "opentype-cff")
        with self.assertRaisesRegex(ValueError, "variable"):
            fonts._source_revision(sfnt(), "truetype-variable")
        with self.assertRaises(ValueError):
            fonts._source_revision(b"%!FontType1", "truetype")
        with self.assertRaises(ValueError):
            fonts.sfnt_revision(sfnt(b"OTTO"))

    def test_aggregate_size_bound_is_checked_before_payload_reads(self):
        with mock.patch.object(fonts, "MAX_TOTAL_BYTES", 1):
            with self.assertRaisesRegex(ValueError, "aggregate"):
                self.load()

    def test_license_blob_and_source_revision_are_independently_checked(self):
        original = copy.deepcopy(self.document)
        self.document["additions"][0]["source"]["license"]["git_blob_sha1"] = "0" * 40
        with self.assertRaisesRegex(ValueError, "blob"):
            self.prepare()
        self.document = original
        self.document["additions"][0]["sfnt_revision"] = 2
        with self.assertRaisesRegex(ValueError, "revision"):
            self.prepare()

    def test_source_repository_commit_and_license_paths_are_strict(self):
        original = copy.deepcopy(self.document)
        for change in (
            {"repository": "../fonts"},
            {"repository": "https://github.com/a/b"},
            {"target_commit": "main"},
            {"kind": "url"},
            {"font": []},
            {"license": None},
        ):
            with self.subTest(change=change):
                self.document = copy.deepcopy(original)
                self.document["additions"][0]["source"].update(change)
                with self.assertRaises(ValueError):
                    self.load()
        for change in (
            {"path": "../OFL.txt"},
            {"name": "../OFL.txt"},
            {"bytes": True},
            {"bytes": 0},
            {"git_blob_sha1": False},
        ):
            with self.subTest(change=change):
                self.document = copy.deepcopy(original)
                self.document["additions"][0]["source"]["license"].update(change)
                with self.assertRaises(ValueError):
                    self.load()

    def test_unknown_pack_members_and_symlinked_license_directories_fail(self):
        self.prepare()
        extra = self.root / "pack/extra.json"
        extra.write_text("{}")
        with self.assertRaisesRegex(ValueError, "file set"):
            fonts.verify_pack(self.root / "pack", self.load())
        extra.unlink()
        directory = self.root / "pack/licenses"
        for path in directory.iterdir():
            path.unlink()
        directory.rmdir()
        directory.symlink_to(self.licenses, target_is_directory=True)
        with self.assertRaisesRegex(ValueError, "symlinked"):
            fonts.verify_pack(self.root / "pack", self.load())

    def test_prepare_does_not_write_inside_inputs(self):
        with self.assertRaisesRegex(ValueError, "overlap"):
            fonts.prepare_pack(
                self.fonts, self.licenses, self.fonts / "output", self.load()
            )
        self.assertFalse((self.fonts / "output").exists())

    def test_source_license_sha_and_font_sha_are_both_required(self):
        original = copy.deepcopy(self.document)
        self.document["additions"][0]["sha256"] = "0" * 64
        with self.assertRaisesRegex(ValueError, "identity"):
            self.prepare()
        self.document = original
        self.document["additions"][0]["source"]["license"]["sha256"] = "0" * 64
        with self.assertRaisesRegex(ValueError, "identity"):
            self.prepare()

    def test_changed_lock_cannot_validate_an_older_receipt(self):
        self.prepare()
        self.document["font_order"].reverse()
        with self.assertRaisesRegex(ValueError, "receipt"):
            fonts.verify_pack(self.root / "pack", self.load())

    def test_bounded_lock_loader_rejects_oversized_files(self):
        self.lock_path.write_bytes(b" " * (fonts.MAX_LOCK_BYTES + 1))
        with self.assertRaises(ValueError):
            fonts.load_lock(self.lock_path, self.base)

    def test_inherited_font_paths_are_revalidated_for_portable_staging(self):
        base = json.loads(self.base.read_text())
        entry = base["families"][0]["files"][0]
        entry["name"] = "directory\\Locked-Regular.ttf"
        entry["asset_member"] = "Locked/" + entry["name"]
        self.base.write_text(json.dumps(base))
        self.document["base_lock"]["sha256"] = sha(self.base.read_bytes())
        self.document["font_order"][0] = entry["name"]
        with self.assertRaises(ValueError):
            self.load()

    def test_receipt_metadata_cannot_contain_local_paths(self):
        self.document["additions"][0]["style"] = str(
            PurePosixPath("/") / "Users" / "example" / "fonts"
        )
        with self.assertRaisesRegex(ValueError, "path-neutral"):
            self.load()

    def test_windows_device_and_invalid_paths_are_rejected(self):
        for name in (
            "CON.ttf",
            "AUX.txt",
            "COM1.otf",
            "LPT9.ttf",
            "a?.ttf",
            "a*.ttf",
            "a|b.ttf",
            'a"b.ttf',
        ):
            with self.subTest(name=name):
                with self.assertRaises(ValueError):
                    fonts._path(name, "file", filename=True)


if __name__ == "__main__":
    unittest.main()
