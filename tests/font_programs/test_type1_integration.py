"""Explicit Docker/locked-wheel integration gate, separate from dependency-free tests."""

import io
import os
from pathlib import Path
import sys
import unittest

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "scripts"))
import font_subset_attestation as attestation  # noqa: E402
import font_subset_worker as worker  # noqa: E402
import libreoffice_container as runtime  # noqa: E402


class Type1IntegrationTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        path = os.environ.get("RWML_FONTTOOLS_WHEEL")
        if not path:
            raise RuntimeError("Set RWML_FONTTOOLS_WHEEL to the locked FontTools wheel")
        cls.wheel = Path(path).resolve()
        attestation.wheel_payload(cls.wheel)
        runtime.inspect_image(runtime.load_runtime_lock())
        sys.path.insert(0, str(cls.wheel))
        from fontTools.fontBuilder import FontBuilder
        from fontTools.pens.t2CharStringPen import T2CharStringPen

        builder = FontBuilder(1000, isTTF=False)
        names = [".notdef", "cid00001"]
        builder.setupGlyphOrder(names)
        builder.setupCharacterMap({0x4E00: "cid00001"})
        charstrings = {}
        for name in names:
            pen = T2CharStringPen(1000, None)
            pen.moveTo((0, 0))
            pen.lineTo((100, 200))
            pen.closePath()
            charstrings[name] = pen.getCharString()
        builder.setupCFF(
            "Locked-CJK",
            {"FullName": "Locked CJK", "FamilyName": "Locked CJK", "Weight": "Regular"},
            charstrings,
            {},
        )
        builder.setupHorizontalMetrics({name: (1000, 0) for name in names})
        builder.setupHorizontalHeader(ascent=800, descent=-200)
        builder.setupNameTable(
            {
                "familyName": "Locked CJK",
                "styleName": "Regular",
                "psName": "Locked-CJK",
                "fullName": "Locked CJK",
                "uniqueFontIdentifier": "Locked-CJK-1",
            }
        )
        builder.setupOS2(
            sTypoAscender=800, sTypoDescender=-200, usWinAscent=800, usWinDescent=200
        )
        builder.setupPost()
        builder.font["head"].created = 2082844800
        builder.font["head"].modified = 2082844800
        builder.font["head"].fontRevision = 1.0
        builder.font.recalcTimestamp = False
        output = io.BytesIO()
        builder.font.save(output)
        cls.source = output.getvalue()
        cls.entry = {
            "bytes": len(cls.source),
            "sha256": worker.digest(cls.source),
            "postscript_name": "Locked-CJK",
            "sfnt_revision": 65536,
        }

    def program(
        self,
        *,
        width=1000,
        endpoint=(100, 200),
        font_name="Locked-CJK",
        matrix="0.001 0 0 0.001 0 0",
        names=(".notdef", "cid1"),
        prefix="",
        long_outline=False,
    ):
        from fontTools.misc.eexec import encrypt
        from fontTools.misc.psCharStrings import T1CharString

        rows = []
        for name in names:
            commands = [0, width, "hsbw", 0, 0, "rmoveto"]
            if long_outline and name != ".notdef":
                commands += [1, 1, "rlineto"] * 8200
            else:
                commands += [*endpoint, "rlineto"]
            commands += ["closepath", "endchar"]
            charstring = T1CharString(program=commands)
            charstring.compile()
            encoded, _ = encrypt(b"abcd" + charstring.bytecode, 4330)
            rows.append(f"/{name} <{encoded.hex()}> def")
        header = (
            f"%!FontType1-1.0: {font_name} 1.0\n8 dict begin\n/FontName /{font_name} def\n"
            f"/FontType 1 def\n/PaintType 0 def\n/FontMatrix [{matrix}] def\n"
            "/FontBBox [0 0 100 200] def\n/Encoding StandardEncoding def\ncurrentfile eexec\n"
        ).encode()
        body = (
            f"{prefix}\n/Private 2 dict dup begin /lenIV 4 def /Subrs 0 array def end def\n"
            f"/CharStrings {len(rows)} dict dup begin\n"
            + "\n".join(rows)
            + f"\nend def\ncurrentdict end /{font_name} exch definefont pop\nmark\ncurrentfile closefile\n"
        ).encode()
        encrypted, _ = encrypt(b"abcd" + body, 55665)
        return (
            header + encrypted.hex().encode() + b"\n" + b"0" * 512 + b"\ncleartomark\n"
        )

    def attest(self, program=None, **kwargs):
        if program is None:
            program = self.program()
        return attestation.attest_program(
            program, self.source, self.entry, self.wheel, **kwargs
        )

    def rejected(self, program, reason):
        with self.assertRaises(runtime.ProcessFailed) as caught:
            self.attest(program)
        self.assertIn(reason.encode(), caught.exception.stderr)

    def test_exact_synthetic_program_repeats_without_metadata_variation(self):
        first, second = self.attest(), self.attest()
        self.assertEqual(first, second)
        self.assertEqual(first["result"]["proof"]["glyph_count"], 2)

    def test_changed_width_fails(self):
        self.rejected(self.program(width=999), "glyph_width_mismatch")

    def test_changed_outline_fails(self):
        self.rejected(self.program(endpoint=(100, 201)), "glyph_outline_mismatch")

    def test_changed_name_fails(self):
        self.rejected(self.program(font_name="Other-CJK"), "subset_font_identity")

    def test_changed_matrix_fails(self):
        self.rejected(
            self.program(matrix="0.002 0 0 0.001 0 0"), "font_matrix_mismatch"
        )

    def test_duplicate_cid_alias_fails(self):
        self.rejected(
            self.program(names=(".notdef", "cid1", "cid01")), "subset_glyph_mapping"
        )

    def test_unknown_cid_fails(self):
        self.rejected(self.program(names=(".notdef", "cid2")), "subset_glyph_mapping")

    def test_missing_notdef_fails(self):
        self.rejected(self.program(names=("cid1",)), "subset_glyph_count")

    def test_notdef_only_fails(self):
        self.rejected(self.program(names=(".notdef",)), "subset_glyph_count")

    def test_malformed_postscript_fails(self):
        self.rejected(
            b"%!FontType1-1.0: Invalid\ninvalid_operator\n", "parser_rejected_input"
        )

    def test_large_postscript_allocation_fails_within_the_worker_limit(self):
        self.rejected(
            self.program(prefix="1000000000 array pop"), "parser_rejected_input"
        )

    def test_glyph_operation_limit_is_enforced(self):
        self.rejected(self.program(long_outline=True), "outline_work_bound")

    def test_stdout_overflow_is_not_accepted_as_a_receipt(self):
        with self.assertRaisesRegex(ValueError, "output exceeded"):
            self.attest(self.program(prefix="(" + "x" * 600000 + ") print"))

    def test_timeout_removes_the_named_container(self):
        before = runtime.run_bounded(["docker", "ps", "-a", "--format", "{{.Names}}"])
        with self.assertRaisesRegex(ValueError, "timed out"):
            self.attest(self.program(prefix="0 1 100000000 {pop} for"), timeout=0.3)
        after = runtime.run_bounded(["docker", "ps", "-a", "--format", "{{.Names}}"])
        self.assertEqual(before, after)

    def test_native_cff_is_not_misclassified_as_type1(self):
        with self.assertRaisesRegex(ValueError, "Type 1"):
            self.attest(b"\x01\x00\x04\x04" + b"x" * 50)


if __name__ == "__main__":
    unittest.main()
