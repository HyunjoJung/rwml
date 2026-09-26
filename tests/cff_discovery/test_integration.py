"""Explicit isolated CFF discovery and independent-proof integration tests."""

import io
import os
from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))
import font_subset_attestation as proof  # noqa: E402
import font_subset_worker as common  # noqa: E402
import libreoffice_container as runtime  # noqa: E402
import native_cff_attestation as native  # noqa: E402
import pdf_font_resources as resources  # noqa: E402


class CFFDiscoveryIntegrationTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        for name in ("RWML_FONTTOOLS_WHEEL", "RWML_PYPDF_WHEEL"):
            if not os.environ.get(name):
                raise RuntimeError(f"{name} is required for CFF discovery integration")
        cls.fonttools = Path(os.environ["RWML_FONTTOOLS_WHEEL"]).resolve()
        cls.pypdf = Path(os.environ["RWML_PYPDF_WHEEL"]).resolve()
        proof.wheel_payload(cls.fonttools)
        resources.wheel_payload(cls.pypdf)
        runtime.inspect_image(runtime.load_runtime_lock())
        sys.path[:0] = [str(cls.fonttools), str(cls.pypdf)]

    def fixture(
        self,
        *,
        text="4",
        selected=2,
        ambiguous=False,
        gsub=True,
        extension=False,
        matrix=None,
        extra_font=False,
        extra_cff=False,
        raw_hint=None,
    ):
        from fontTools.fontBuilder import FontBuilder
        from fontTools.pens.t2CharStringPen import T2CharStringPen
        from fontTools.feaLib.builder import addOpenTypeFeaturesFromString
        from fontTools.cffLib import CharStrings, FDArrayIndex, FDSelect, FontDict
        from fontTools.misc.psCharStrings import T2CharString
        from pypdf import PdfWriter
        from pypdf.generic import (
            ArrayObject,
            DecodedStreamObject,
            DictionaryObject,
            NameObject,
        )

        names = [".notdef", *[f"cid{index:05d}" for index in range(1, 7)]]
        builder = FontBuilder(1000, isTTF=False)
        builder.setupGlyphOrder(names)
        builder.setupCharacterMap(
            {ord("4"): names[1], ord("f"): names[3], ord("l"): names[4]}
        )
        charstrings = {}
        for index, name in enumerate(names):
            pen = T2CharStringPen(1000, None)
            pen.moveTo((0, 0))
            pen.lineTo((20 if ambiguous and index == 6 else index * 10, 100))
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
        if gsub:
            feature = (
                "lookup change "
                + ("useExtension " if extension else "")
                + "{ sub cid00001 by cid00002; } change; feature tnum { lookup change; } tnum; feature liga { sub cid00003 cid00004 by cid00005; } liga;"
            )
            if ambiguous:
                feature += (
                    " feature aalt { sub cid00001 from [cid00002 cid00006]; } aalt;"
                )
            addOpenTypeFeaturesFromString(builder.font, feature)
        builder.font["head"].created = 2082844800
        builder.font["head"].modified = 2082844800
        builder.font["head"].fontRevision = 1.0
        builder.font.recalcTimestamp = False
        output = io.BytesIO()
        builder.font.save(output)
        source = output.getvalue()
        entry = {
            "bytes": len(source),
            "sha256": common.digest(source),
            "postscript_name": "Locked-CJK",
            "sfnt_revision": 65536,
        }

        font = builder.font
        font.recalcBBoxes = False
        cff = font["CFF "].cff
        top = cff.topDictIndex[0]
        private = top.Private
        top.ROS = ("Adobe", "Identity", 0)
        top.CIDCount = 2
        top.FDArray = FDArrayIndex()
        fd = FontDict()
        fd.Private, fd.FontMatrix = private, [1, 0, 0, 1, 0, 0]
        top.FDArray.append(fd)
        top.FDSelect = FDSelect(format=0)
        top.FDSelect.gidArray = [0, 0]
        top.charset = [".notdef", "cid00001"]
        del top.Private
        top.CharStrings = CharStrings(
            None, None, cff.GlobalSubrs, private, top.FDSelect, top.FDArray
        )
        for index, name in enumerate(top.charset):
            glyph = T2CharString(
                program=[
                    1000,
                    0,
                    0,
                    "rmoveto",
                    selected * 10 if index else 0,
                    100,
                    "rlineto",
                    "endchar",
                ],
                private=private,
                globalSubrs=cff.GlobalSubrs,
            )
            glyph.fdSelectIndex = 0
            top.CharStrings[name] = glyph
        if matrix is not None:
            top.FontMatrix = matrix
        font.setGlyphOrder(top.charset)
        output = io.BytesIO()
        cff.compile(output, font, isCFF2=False)
        program = output.getvalue()
        hint = (
            b"1 beginbfchar\n<0001> <"
            + text.encode("utf-16-be", "surrogatepass").hex().encode()
            + b">\nendbfchar\n"
            if raw_hint is None
            else raw_hint
        )

        writer = PdfWriter()
        page = writer.add_blank_page(200, 200)

        def dictionary(**values):
            return DictionaryObject(
                {NameObject("/" + key): value for key, value in values.items()}
            )

        def stream(data, **values):
            value = DecodedStreamObject()
            value.set_data(data)
            value.update(dictionary(**values))
            return writer._add_object(value.flate_encode())

        descriptor = writer._add_object(
            dictionary(
                Type=NameObject("/FontDescriptor"),
                FontName=NameObject("/ABCDEF+Locked-CJK"),
                FontFile3=stream(program, Subtype=NameObject("/CIDFontType0C")),
            )
        )
        descendant = writer._add_object(
            dictionary(
                Type=NameObject("/Font"),
                Subtype=NameObject("/CIDFontType0"),
                BaseFont=NameObject("/ABCDEF+Locked-CJK"),
                FontDescriptor=descriptor,
            )
        )
        resource = writer._add_object(
            dictionary(
                Type=NameObject("/Font"),
                Subtype=NameObject("/Type0"),
                BaseFont=NameObject("/ABCDEF+Locked-CJK-Identity-H"),
                Encoding=NameObject("/Identity-H"),
                DescendantFonts=ArrayObject([descendant]),
                ToUnicode=stream(hint),
            )
        )
        fonts = dictionary(F0=resource)
        if extra_cff:
            clone = dictionary(
                **{str(key)[1:]: value for key, value in resource.get_object().items()}
            )
            fonts[NameObject("/SecondCFF")] = writer._add_object(clone)
        if extra_font:
            descriptor = writer._add_object(
                dictionary(
                    Type=NameObject("/FontDescriptor"),
                    FontName=NameObject("/Other-Font"),
                    FontFile2=stream(b"truefixture"),
                )
            )
            fonts[NameObject("/Other")] = writer._add_object(
                dictionary(
                    Type=NameObject("/Font"),
                    Subtype=NameObject("/TrueType"),
                    BaseFont=NameObject("/Other-Font"),
                    FontDescriptor=descriptor,
                )
            )
        page[NameObject("/Resources")] = dictionary(Font=fonts)
        output = io.BytesIO()
        writer.write(output)
        return {
            "pdf": output.getvalue(),
            "source": source,
            "entry": entry,
            "program": program,
            "hint": hint,
        }

    def discover(self, **kwargs):
        fixture = self.fixture(**kwargs)
        result = native.discover_program(
            fixture["program"],
            fixture["hint"],
            fixture["source"],
            fixture["entry"],
            self.fonttools,
            self.pypdf,
        )
        return fixture, result

    def attest(self, fixture):
        return native.attest_pdf(
            fixture["pdf"],
            fixture["source"],
            fixture["entry"],
            self.fonttools,
            self.pypdf,
        )

    def rejected(self, reason, **kwargs):
        with self.assertRaises(runtime.ProcessFailed) as caught:
            self.discover(**kwargs)
        self.assertIn(reason.encode(), caught.exception.stderr)

    def test_alternate_digit_is_discovered_and_independently_proved(self):
        fixture = self.fixture(extra_font=True)
        first, second = self.attest(fixture), self.attest(fixture)
        self.assertEqual(first, second)
        self.assertEqual(
            first["cff_resources"][0]["discovery"]["result"]["glyphs"][1],
            ["cid00001", "cid00002"],
        )
        self.assertEqual(
            first["cff_resources"][0]["proof"]["result"]["proof"]["glyph_count"], 2
        )
        self.assertEqual(len(first["unverified_resources"]), 1)
        resources.verify_receipt(common.canonical(first), second)

    def test_multiscalar_ligature_is_discovered_and_proved(self):
        result = self.attest(self.fixture(text="fl", selected=5))
        self.assertEqual(
            result["cff_resources"][0]["discovery"]["result"]["glyphs"][1],
            ["cid00001", "cid00005"],
        )

    def test_every_distinct_cff_resource_gets_a_proof(self):
        result = self.attest(self.fixture(extra_cff=True))
        self.assertEqual(len(result["cff_resources"]), 2)
        self.assertEqual(
            len({tuple(row["font_ref"]) for row in result["cff_resources"]}), 2
        )
        self.assertEqual(
            result["cff_resources"][0]["proof"], result["cff_resources"][1]["proof"]
        )

    def test_discovery_timeout_cleans_its_container_and_staging(self):
        from unittest import mock

        fixture = self.fixture()
        identifier = native.uuid.uuid4()
        name = "rwml-oracle-" + identifier.hex
        before = set(native.SCRATCH.iterdir()) if native.SCRATCH.exists() else set()
        with mock.patch.object(native.uuid, "uuid4", return_value=identifier):
            with self.assertRaisesRegex(ValueError, "timed out"):
                native.discover_program(
                    fixture["program"],
                    fixture["hint"],
                    fixture["source"],
                    fixture["entry"],
                    self.fonttools,
                    self.pypdf,
                    timeout=0.001,
                )
        self.assertEqual(set(native.SCRATCH.iterdir()), before)
        self.assertEqual(
            runtime.run_bounded(
                ["docker", "ps", "-aq", "--filter", f"name=^{name}$"]
            ).strip(),
            b"",
        )

    def test_real_extension_lookup_is_supported(self):
        _, result = self.discover(extension=True)
        self.assertEqual(result["result"]["glyphs"][1][1], "cid00002")

    def test_default_glyph_without_gsub_is_supported(self):
        _, result = self.discover(selected=1, gsub=False)
        self.assertEqual(result["result"]["glyphs"][1][1], "cid00001")

    def test_missing_substitution_does_not_assume_default_glyph(self):
        self.rejected("mapping_glyph_unmatched", gsub=False)

    def test_ambiguous_geometry_is_rejected(self):
        self.rejected("mapping_glyph_ambiguous", ambiguous=True)

    def test_duplicate_hint_cid_is_rejected(self):
        self.rejected(
            "mapping_hint_duplicates",
            raw_hint=b"2 beginbfchar\n<0001> <0034>\n<0001> <0034>\nendbfchar\n",
        )

    def test_missing_and_extra_hint_cids_are_rejected(self):
        for hint in (
            b"0 beginbfchar\nendbfchar\n",
            b"1 beginbfchar\n<0002> <0034>\nendbfchar\n",
        ):
            with self.subTest(hint=hint):
                self.rejected("mapping_hint_coverage", raw_hint=hint)

    def test_empty_oversized_or_surrogate_hints_are_rejected(self):
        for text in ("", "a" * 9, "\ud800"):
            with self.subTest(text=repr(text)):
                self.rejected("mapping_hint_text", text=text)

    def test_unknown_unicode_is_not_silently_dropped(self):
        self.rejected("mapping_source_cmap_missing", text="\u2603")

    def test_broken_hint_line_is_not_silent_recovery(self):
        self.rejected(
            "pdf_parser_warning", raw_hint=b"1 beginbfchar\n<0001> <GGGG>\nendbfchar\n"
        )

    def test_independent_proof_rejects_matrix_even_after_discovery(self):
        fixture, _ = self.discover(matrix=[0.002, 0, 0, 0.001, 0, 0])
        with self.assertRaises(runtime.ProcessFailed) as caught:
            self.attest(fixture)
        self.assertIn(b"font_matrix_mismatch", caught.exception.stderr)


if __name__ == "__main__":
    unittest.main()
