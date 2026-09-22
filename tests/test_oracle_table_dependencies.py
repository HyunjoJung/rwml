"""Complete reference rasters and explicit empty metadata for table captures."""

import hashlib
import os
from pathlib import Path
import subprocess
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))
import render_oracle_contract as contract
import render_validate as render


class OracleTableDependencyTests(unittest.TestCase):
    def test_validator_remains_importable_as_a_namespace_module(self):
        result = subprocess.run(
            [sys.executable, "-B", "-c", "import scripts.render_validate"],
            cwd=Path(__file__).resolve().parents[1],
            capture_output=True,
            text=True,
            timeout=30,
        )
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_empty_metadata_requires_explicit_boolean_opt_in(self):
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "warmup.log"
            path.write_bytes(b"")
            with self.assertRaises(ValueError):
                contract._read_bounded_regular_file(path, 100)
            self.assertEqual(
                contract._read_bounded_regular_file(path, 100, allow_empty=True), b""
            )
            for invalid in (1, "yes", None):
                with self.subTest(value=invalid), self.assertRaises(ValueError):
                    contract._read_bounded_regular_file(path, 100, allow_empty=invalid)

    @unittest.skipUnless(hasattr(os, "mkfifo"), "FIFO boundary is POSIX-specific")
    def test_empty_metadata_opt_in_does_not_allow_fifo_or_symlink(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            fifo = root / "pipe"
            os.mkfifo(fifo)
            with self.assertRaisesRegex(ValueError, "regular file"):
                contract._read_bounded_regular_file(fifo, 100, allow_empty=True)
            empty = root / "empty"
            empty.write_bytes(b"")
            link = root / "link"
            link.symlink_to(empty)
            with self.assertRaisesRegex(ValueError, "symlink"):
                contract._read_bounded_regular_file(link, 100, allow_empty=True)

    def test_reference_rasters_reject_truncated_or_empty_page_sets(self):
        image = SimpleNamespace(width=2, height=3, mode="RGB", tobytes=lambda: b"x")
        for images, total in (([image], 2), ([], 0)):
            with self.subTest(total=total), mock.patch.object(
                render, "rasterize_pdf_pages", return_value=(images, total)
            ):
                self.assertIsNone(
                    render.reference_page_digests(Path("unused.pdf"), dpi=110, page_cap=1)
                )

    def test_reference_rasters_bind_dimensions_and_color_mode(self):
        digests = []
        for width, height, mode in ((2, 3, "RGB"), (3, 2, "RGB"), (2, 3, "L")):
            image = SimpleNamespace(
                width=width, height=height, mode=mode, tobytes=lambda: b"same samples"
            )
            with mock.patch.object(
                render, "rasterize_pdf_pages", return_value=([image], 1)
            ):
                digest = render.reference_page_digests(
                    Path("unused.pdf"), dpi=110, page_cap=1
                )
            expected = hashlib.sha256(
                width.to_bytes(8, "big") + height.to_bytes(8, "big")
                + mode.encode("ascii") + b"same samples"
            ).hexdigest()
            self.assertEqual(digest, [expected])
            digests.append(digest[0])
        self.assertEqual(len(set(digests)), 3)

    def test_pdf_font_metadata_reads_subset_revision_and_deduplicates_resources(self):
        payload = bytearray(64)
        payload[:4] = b"true"
        payload[4:6] = (1).to_bytes(2, "big")
        payload[12:16] = b"head"
        payload[20:24] = (32).to_bytes(4, "big")
        payload[24:28] = (16).to_bytes(4, "big")
        payload[36:40] = (132055).to_bytes(4, "big")
        font = (7, "ttf", "TrueType", "ABCDEF+NotoSans-Regular", "F1", "", 0)
        page = mock.Mock()
        page.get_fonts.return_value = [font, font]
        document = mock.MagicMock()
        document.__iter__.return_value = iter([page])
        document.extract_font.return_value = (font[3], "ttf", "TrueType", bytes(payload))
        fake_fitz = mock.Mock()
        fake_fitz.open.return_value = document
        with mock.patch.object(render, "fitz", fake_fitz):
            identities = render.reference_pdf_font_identities(Path("unused.pdf"))
        self.assertEqual(
            identities, [{"postscript_name": "NotoSans-Regular", "sfnt_revision": 132055}]
        )
        document.extract_font.assert_called_once_with(7)
        document.close.assert_called_once_with()

    def test_pdf_font_metadata_rejects_unavailable_program_and_closes_document(self):
        font = (7, "ttf", "TrueType", "ABCDEF+NotoSans-Regular", "F1", "", 0)
        for resource, program in ((font, None), (font, b"truncated"), ((True, *font[1:]), b"x")):
            with self.subTest(resource=resource, program=program):
                page = mock.Mock()
                page.get_fonts.return_value = [resource]
                document = mock.MagicMock()
                document.__iter__.return_value = iter([page])
                document.extract_font.return_value = (font[3], "ttf", "TrueType", program)
                fake_fitz = mock.Mock()
                fake_fitz.open.return_value = document
                with mock.patch.object(render, "fitz", fake_fitz), self.assertRaises(ValueError):
                    render.reference_pdf_font_identities(Path("unused.pdf"))
                document.close.assert_called_once_with()


if __name__ == "__main__":
    unittest.main()
