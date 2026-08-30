"""Explicit locked-runtime CFF checks, including source-map and parser failures."""

import io
import unittest

import test_type1_integration as type1
import font_subset_attestation as attestation
import libreoffice_container as runtime


class CFFIntegrationTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        type1.Type1IntegrationTests.setUpClass()
        cls.source = type1.Type1IntegrationTests.source
        cls.entry = type1.Type1IntegrationTests.entry
        cls.wheel = type1.Type1IntegrationTests.wheel
        cls.mapping = [[".notdef", ".notdef"], ["cid00001", "cid00001"]]

    def program(
        self,
        *,
        width=1000,
        endpoint=(100, 200),
        matrix=None,
        fd_matrix=None,
        names=None,
        cid=True,
        name="Locked-CJK",
        long_outline=False,
        recursive=False,
        multiple=False,
        omit_fd_matrix=False,
        invalid_fd=False,
        cid_count=2,
    ):
        from fontTools.cffLib import (
            CharStrings,
            FDArrayIndex,
            FDSelect,
            FontDict,
            SubrsIndex,
        )
        from fontTools.misc.psCharStrings import T2CharString
        from fontTools.ttLib import TTFont

        font = TTFont(io.BytesIO(self.source))
        font.recalcBBoxes = False
        cff = font["CFF "].cff
        top = cff.topDictIndex[0]
        cff.fontNames = [name]
        old = top.Private
        top.ROS = ("Adobe", "Identity", 0)
        top.CIDCount = cid_count
        top.FDArray = FDArrayIndex()
        fd = FontDict()
        fd.Private = old
        fd.FontMatrix = [1, 0, 0, 1, 0, 0] if fd_matrix is None else fd_matrix
        if omit_fd_matrix:
            del fd.FontMatrix
        top.FDArray.append(fd)
        top.FDSelect = FDSelect(format=0)
        top.FDSelect.gidArray = [0, 0]
        top.charset = [".notdef", "cid00001"] if names is None else names
        del top.Private
        top.CharStrings = CharStrings(
            None, None, cff.GlobalSubrs, old, top.FDSelect, top.FDArray
        )
        for index, glyph_name in enumerate(top.charset):
            commands = [width, 0, 0, "rmoveto"]
            commands += (
                [1, 1, "rlineto"] * 8200
                if long_outline and index
                else [*endpoint, "rlineto"]
            )
            commands += ["endchar"]
            if recursive and index:
                old.Subrs = SubrsIndex()
                old.Subrs.append(
                    T2CharString(
                        program=[-107, "callsubr", "return"],
                        private=old,
                        globalSubrs=cff.GlobalSubrs,
                    )
                )
                commands = [-107, "callsubr", "endchar"]
            glyph = T2CharString(
                program=commands, private=old, globalSubrs=cff.GlobalSubrs
            )
            glyph.fdSelectIndex = 0
            top.CharStrings[glyph_name] = glyph
        if matrix is not None:
            top.FontMatrix = matrix
        if not cid:
            del top.ROS
        if multiple:
            cff.fontNames.append("Other-CJK")
            cff.topDictIndex.append(top)
        if invalid_fd:
            top.FDSelect.gidArray[1] = 255
        output = io.BytesIO()
        cff.compile(output, font, isCFF2=False)
        return output.getvalue()

    def attest(self, program=None, mapping=None):
        return attestation.attest_program(
            self.program() if program is None else program,
            self.source,
            self.entry,
            self.wheel,
            glyph_map=self.mapping if mapping is None else mapping,
        )

    def rejected(self, program, reason):
        with self.assertRaises(runtime.ProcessFailed) as caught:
            self.attest(program)
        self.assertIn(reason.encode(), caught.exception.stderr)

    def test_cff_exact_program_and_receipt_repeat(self):
        first, second = self.attest(), self.attest()
        self.assertEqual(first, second)
        self.assertEqual(first["result"]["subset"]["representation"], "cid-cff")
        self.assertEqual(first["result"]["proof"]["glyph_count"], 2)

    def test_cff_width_change_fails(self):
        self.rejected(self.program(width=999), "glyph_width_mismatch")

    def test_cff_outline_change_fails(self):
        self.rejected(self.program(endpoint=(101, 200)), "glyph_outline_mismatch")

    def test_cff_top_matrix_change_fails(self):
        self.rejected(
            self.program(matrix=[0.002, 0, 0, 0.001, 0, 0]), "font_matrix_mismatch"
        )

    def test_cff_nonidentity_fd_matrix_fails(self):
        self.rejected(
            self.program(fd_matrix=[1, 0, 0, 2, 0, 0]), "cff_fd_matrix_unsupported"
        )

    def test_cff_missing_fd_matrix_has_the_same_glyph_proof(self):
        first = self.attest()
        second = self.attest(self.program(omit_fd_matrix=True))
        self.assertEqual(first["result"]["proof"], second["result"]["proof"])

    def test_cff_non_cid_font_is_not_accepted(self):
        self.rejected(self.program(cid=False), "cff_font_kind")

    def test_cff_font_name_change_fails(self):
        self.rejected(self.program(name="Other-CJK"), "cff_font_identity")

    def test_cff_nonidentity_charset_fails(self):
        self.rejected(self.program(names=[".notdef", "cid00002"]), "cff_charset")

    def test_cff_unknown_source_map_fails(self):
        with self.assertRaises(runtime.ProcessFailed) as caught:
            self.attest(mapping=[[".notdef", ".notdef"], ["cid00001", "cid00200"]])
        self.assertIn(b"cff_mapping_coverage", caught.exception.stderr)

    def test_cff_multiple_fonts_fail(self):
        self.rejected(self.program(multiple=True), "cff_font_identity")

    def test_cff_invalid_fd_selector_fails(self):
        self.rejected(self.program(invalid_fd=True), "cff_structure")

    def test_cff_cid_count_smaller_than_charset_fails(self):
        self.rejected(self.program(cid_count=1), "cff_structure")

    def test_cff_malformed_offsets_fail(self):
        self.rejected(b"\x01\x00\x04\x04\xff\xff\x04\xff\xff", "parser_rejected_input")

    def test_cff_excessive_outline_work_fails(self):
        self.rejected(self.program(long_outline=True), "outline_work_bound")

    def test_cff_recursive_subroutine_fails(self):
        self.rejected(self.program(recursive=True), "parser_rejected_input")


if __name__ == "__main__":
    unittest.main()
