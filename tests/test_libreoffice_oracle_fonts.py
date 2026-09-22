import hashlib
import importlib.util
import os
import pathlib
import struct
import sys
import tempfile
import unittest


ROOT = pathlib.Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "scripts" / "libreoffice_oracle_fonts.py"
SPEC = importlib.util.spec_from_file_location("libreoffice_oracle_fonts", SCRIPT)
libreoffice_oracle_fonts = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
sys.modules[SPEC.name] = libreoffice_oracle_fonts
SPEC.loader.exec_module(libreoffice_oracle_fonts)


class LibreOfficeOracleFontTests(unittest.TestCase):
    @unittest.skipUnless(hasattr(os, "mkfifo"), "FIFO boundary is POSIX-specific")
    def test_metadata_reader_rejects_fifo_without_blocking(self):
        with tempfile.TemporaryDirectory() as temporary:
            path = pathlib.Path(temporary) / "pipe"
            os.mkfifo(path)
            with self.assertRaisesRegex(ValueError, "regular file"):
                libreoffice_oracle_fonts._read_regular_file(path, 1024, allow_empty=True)

    def test_empty_regular_file_requires_explicit_metadata_opt_in(self):
        with tempfile.TemporaryDirectory() as temporary:
            path = pathlib.Path(temporary) / "empty.log"
            path.write_bytes(b"")
            with self.assertRaises(ValueError):
                libreoffice_oracle_fonts._read_regular_file(path, 1024)
            self.assertEqual(
                libreoffice_oracle_fonts._read_regular_file(path, 1024, allow_empty=True),
                b"",
            )
            alias = pathlib.Path(temporary) / "alias.log"
            alias.symlink_to(path)
            with self.assertRaises(ValueError):
                libreoffice_oracle_fonts._read_regular_file(alias, 1024, allow_empty=True)

    def test_public_lock_pins_expected_upstream_files(self):
        lock = libreoffice_oracle_fonts.load_font_lock()
        files = libreoffice_oracle_fonts.font_files(lock)

        self.assertEqual(lock["license"], "SIL-OFL-1.1")
        self.assertEqual(len(files), 8)
        self.assertEqual(
            {entry["postscript_name"] for entry in files},
            {
                "NotoSans-Bold",
                "NotoSans-BoldItalic",
                "NotoSans-Italic",
                "NotoSans-Regular",
                "NotoSansArabic-Bold",
                "NotoSansArabic-Regular",
                "NotoSansHebrew-Bold",
                "NotoSansHebrew-Regular",
            },
        )

    def test_installation_identity_is_path_neutral_and_digest_locked(self):
        payload = b"locked font bytes"
        entry = {
            "asset_member": "family/full/Locked-Regular.ttf",
            "bytes": len(payload),
            "name": "Locked-Regular.ttf",
            "postscript_name": "Locked-Regular",
            "sfnt_revision": 65536,
            "sha256": hashlib.sha256(payload).hexdigest(),
            "style": "Regular",
        }
        lock = {"families": [{"files": [entry]}]}
        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)
            executable = root / "26.2.3" / "soffice.wrapper.sh"
            executable.parent.mkdir()
            executable.write_bytes(b"soffice")
            font = (
                executable.parent
                / "LibreOffice.app"
                / "Contents"
                / "Resources"
                / "fonts"
                / entry["name"]
            )
            font.parent.mkdir(parents=True)
            font.write_bytes(payload)

            identity = libreoffice_oracle_fonts.installation_font_identity(
                executable, lock
            )

        self.assertEqual(
            identity,
            [
                {
                    "bytes": len(payload),
                    "name": "Locked-Regular.ttf",
                    "postscript_name": "Locked-Regular",
                    "sfnt_revision": 65536,
                    "sha256": hashlib.sha256(payload).hexdigest(),
                }
            ],
        )
        self.assertNotIn(str(root), str(identity))

    def test_sfnt_revision_reads_bounded_head_table(self):
        payload = bytearray(64)
        payload[:4] = b"true"
        payload[4:6] = struct.pack(">H", 1)
        payload[12:16] = b"head"
        payload[20:24] = struct.pack(">I", 32)
        payload[24:28] = struct.pack(">I", 16)
        payload[36:40] = struct.pack(">I", 196674)

        self.assertEqual(
            libreoffice_oracle_fonts.sfnt_revision(bytes(payload)),
            196674,
        )
        with self.assertRaisesRegex(ValueError, "head table"):
            libreoffice_oracle_fonts.sfnt_revision(b"true" + bytes(20))

    def test_pdf_identity_rejects_unknown_or_wrong_revision_fonts(self):
        lock = libreoffice_oracle_fonts.load_font_lock()
        valid = [{"postscript_name": "NotoSans-Regular", "sfnt_revision": 132055}]

        libreoffice_oracle_fonts.validate_pdf_font_identities(valid, lock)
        with self.assertRaisesRegex(ValueError, "no embedded fonts"):
            libreoffice_oracle_fonts.validate_pdf_font_identities([], lock)
        libreoffice_oracle_fonts.validate_pdf_font_identities(
            [], lock, allow_empty=True
        )
        with self.assertRaisesRegex(ValueError, "not locked"):
            libreoffice_oracle_fonts.validate_pdf_font_identities(
                [{"postscript_name": "GeezaPro", "sfnt_revision": 65536}],
                lock,
            )
        with self.assertRaisesRegex(ValueError, "revision"):
            libreoffice_oracle_fonts.validate_pdf_font_identities(
                [{"postscript_name": "NotoSans-Regular", "sfnt_revision": 1}],
                lock,
            )


if __name__ == "__main__":
    unittest.main()
