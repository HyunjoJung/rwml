"""Opt-in PDF parsing checks; arbitrary PDF bytes are parsed only in the worker."""

import base64
import io
import os
from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))
import libreoffice_container as runtime  # noqa: E402
import pdf_font_resources as resources  # noqa: E402
import pdf_font_worker as worker  # noqa: E402


class PDFFontIntegrationTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        value = os.environ.get("RWML_PYPDF_WHEEL")
        if not value:
            raise RuntimeError("RWML_PYPDF_WHEEL is required for PDF integration tests")
        cls.wheel = Path(value).resolve()
        resources.wheel_payload(cls.wheel)
        runtime.inspect_image(runtime.load_runtime_lock())
        sys.path.insert(0, str(cls.wheel))
        import pypdf

        if pypdf.__version__ != worker.WHEEL_VERSION:
            raise RuntimeError("fixture builder pypdf version differs")

    def fixture(self, mutation=None, *, kind="Type1"):
        from pypdf import PdfWriter
        from pypdf.generic import (
            ArrayObject,
            DecodedStreamObject,
            DictionaryObject,
            NameObject,
            NumberObject,
        )

        writer = PdfWriter()
        page = writer.add_blank_page(200, 200)
        name = NameObject

        def dictionary(**values):
            return DictionaryObject(
                {name("/" + key): value for key, value in values.items()}
            )

        def stream(payload, **values):
            value = DecodedStreamObject()
            value.set_data(payload)
            value.update(dictionary(**values))
            return writer._add_object(value.flate_encode())

        program = {
            "Type1": b"%!FontType1-fixture",
            "TrueType": b"\x00\x01\x00\x00fixture",
            "CIDFontType2": b"truefixture",
            "CIDFontType0": b"\x01\x00\x04\x04fixture",
        }[kind]
        key = {
            "Type1": "FontFile",
            "TrueType": "FontFile2",
            "CIDFontType2": "FontFile2",
            "CIDFontType0": "FontFile3",
        }[kind]
        program_ref = stream(
            program,
            **({"Subtype": name("/CIDFontType0C")} if kind == "CIDFontType0" else {}),
        )
        descriptor = dictionary(
            Type=name("/FontDescriptor"),
            FontName=name("/AAAAAA+Fixture"),
            **{key: program_ref},
        )
        descriptor_ref = writer._add_object(descriptor)
        font = dictionary(
            Type=name("/Font"),
            Subtype=name("/" + kind),
            BaseFont=name("/AAAAAA+Fixture"),
            FontDescriptor=descriptor_ref,
        )
        if kind.startswith("CIDFont"):
            descendant = writer._add_object(font)
            font = dictionary(
                Type=name("/Font"),
                Subtype=name("/Type0"),
                BaseFont=name("/AAAAAA+Fixture-Identity-H"),
                Encoding=name("/Identity-H"),
                DescendantFonts=ArrayObject([descendant]),
            )
        cmap = b"1 beginbfchar\n<0001> <0041>\nendbfchar\n"
        font[name("/ToUnicode")] = stream(cmap)
        font_ref = writer._add_object(font)
        page[name("/Resources")] = dictionary(Font=dictionary(F0=font_ref))
        state = {
            "writer": writer,
            "page": page,
            "font": font,
            "font_ref": font_ref,
            "descriptor": descriptor,
            "program_ref": program_ref,
            "program": program,
            "cmap": cmap,
            "name": name,
            "dictionary": dictionary,
            "stream": stream,
            "array": ArrayObject,
            "number": NumberObject,
        }
        if mutation:
            mutation(state)
        output = io.BytesIO()
        writer.write(output)
        return output.getvalue(), state

    def extract(self, mutation=None, *, kind="Type1"):
        pdf, _ = self.fixture(mutation, kind=kind)
        return resources.extract_pdf(pdf, self.wheel)

    def rejected(self, mutation, reason, *, kind="Type1"):
        with self.assertRaises(runtime.ProcessFailed) as caught:
            self.extract(mutation, kind=kind)
        self.assertIn(reason.encode(), caught.exception.stderr)

    def test_exact_resource_bytes_and_receipt_repeat(self):
        pdf, state = self.fixture()
        first = resources.extract_pdf(pdf, self.wheel)
        second = resources.extract_pdf(pdf, self.wheel)
        self.assertEqual(first, second)
        result = first["result"]
        self.assertEqual(len(result["fonts"]), 1)
        programs = {
            blob["kind"]: base64.b64decode(blob["base64"]) for blob in result["blobs"]
        }
        self.assertEqual(
            programs, {"type1-pfa": state["program"], "to-unicode": state["cmap"]}
        )
        resources.verify_receipt(resources.common.canonical(first), second)

    def test_all_observed_representations_are_extracted_not_interpreted(self):
        for kind, expected in (
            ("TrueType", "truetype"),
            ("CIDFontType2", "truetype"),
            ("CIDFontType0", "cid-cff"),
        ):
            with self.subTest(kind=kind):
                result = self.extract(kind=kind)["result"]
                self.assertEqual(result["blobs"][0]["kind"], expected)

    def test_inherited_forms_appearances_acroform_and_graphics_fonts_are_found(self):
        def mutate(s):
            d, n, a, w = s["dictionary"], s["name"], s["array"], s["writer"]

            def extra():
                # Distinct resources deliberately share the same BaseFont name.
                return w._add_object(
                    d(**{str(key)[1:]: value for key, value in s["font"].items()})
                )

            form = s["stream"](
                b"q Q",
                Type=n("/XObject"),
                Subtype=n("/Form"),
                Resources=d(Font=d(F1=extra())),
            )
            appearance = s["stream"](b"q Q", Resources=d(Font=d(F2=extra())))
            annotation = w._add_object(
                d(Type=n("/Annot"), Subtype=n("/Widget"), AP=d(N=appearance))
            )
            page = s["page"]
            page[n("/Resources")][n("/XObject")] = d(Form=form)
            page[n("/Annots")] = a([annotation])
            w.root_object[n("/AcroForm")] = d(DR=d(Font=d(F3=extra())))
            page[n("/Parent")][n("/Resources")] = d(Font=d(F4=extra()))
            graphics = w._add_object(
                d(Type=n("/ExtGState"), Font=a([extra(), s["number"](12)]))
            )
            page[n("/Resources")][n("/ExtGState")] = d(GS=graphics)
            # Cycles are valid graph edges; they must not hide another font.
            form.get_object()[n("/Resources")][n("/XObject")] = d(Self=form)

        result = self.extract(mutate)["result"]
        self.assertEqual(len(result["fonts"]), 6)
        self.assertEqual(len(result["blobs"]), 2)
        self.assertEqual(len({font["base_font"] for font in result["fonts"]}), 1)

    def test_repeated_reference_is_not_a_second_font(self):
        def mutate(s):
            s["page"]["/Resources"]["/Font"][s["name"]("/Alias")] = s["font_ref"]

        self.assertEqual(len(self.extract(mutate)["result"]["fonts"]), 1)

    def test_font_without_optional_type_is_found_from_resources(self):
        self.assertEqual(
            len(self.extract(lambda s: s["font"].pop("/Type"))["result"]["fonts"]), 1
        )

    def test_indirect_type_name_does_not_hide_a_reachable_font(self):
        def mutate(s):
            font = s["dictionary"](
                **{str(key)[1:]: value for key, value in s["font"].items()}
            )
            font[s["name"]("/Type")] = s["writer"]._add_object(s["name"]("/Font"))
            s["writer"].root_object[s["name"]("/ExtraFont")] = s["writer"]._add_object(
                font
            )

        self.assertEqual(len(self.extract(mutate)["result"]["fonts"]), 2)

    def test_absent_cmap_is_explicit_not_a_fabricated_mapping(self):
        result = self.extract(lambda s: s["font"].pop("/ToUnicode"))["result"]
        self.assertIsNone(result["fonts"][0]["to_unicode"])
        self.assertEqual(len(result["blobs"]), 1)

    def test_missing_or_duplicate_embedding_fails(self):
        self.rejected(
            lambda s: s["descriptor"].pop("/FontFile"), "pdf_embedded_program"
        )
        self.rejected(
            lambda s: s["descriptor"].update(
                {s["name"]("/FontFile2"): s["program_ref"]}
            ),
            "pdf_embedded_program",
        )

    def test_missing_font_reference_fails_without_emitting_pdf_text(self):
        from pypdf.generic import IndirectObject

        def mutate(s):
            s["page"]["/Resources"]["/Font"][s["name"]("/Missing")] = IndirectObject(
                99999, 0, s["writer"]
            )

        self.rejected(mutate, "pdf_parser_warning")

    def test_direct_font_resource_is_explicitly_unsupported(self):
        def mutate(s):
            s["page"]["/Resources"]["/Font"][s["name"]("/F0")] = s["font"]

        self.rejected(mutate, "pdf_direct_resource_unsupported")

    def test_type3_is_explicitly_unsupported(self):
        self.rejected(
            lambda s: s["font"].update({s["name"]("/Subtype"): s["name"]("/Type3")}),
            "pdf_font_kind",
        )

    def test_nonstream_cmap_fails(self):
        self.rejected(
            lambda s: s["font"].update(
                {s["name"]("/ToUnicode"): s["writer"]._add_object(s["dictionary"]())}
            ),
            "pdf_stream",
        )

    def test_unsupported_composite_encoding_fails(self):
        self.rejected(
            lambda s: s["font"].update(
                {s["name"]("/Encoding"): s["name"]("/Custom-CMap")}
            ),
            "pdf_encoding_unsupported",
            kind="CIDFontType0",
        )

    def test_multiple_composite_descendants_fail(self):
        self.rejected(
            lambda s: s["font"]["/DescendantFonts"].append(
                s["font"]["/DescendantFonts"][0]
            ),
            "pdf_descendants",
            kind="CIDFontType2",
        )

    def test_wrong_embedded_representation_fails(self):
        self.rejected(
            lambda s: s["descriptor"].update(
                {s["name"]("/FontFile"): s["stream"](b"not-a-font")}
            ),
            "pdf_program_kind",
        )

    def test_external_stream_fails(self):
        self.rejected(
            lambda s: s["program_ref"]
            .get_object()
            .update({s["name"]("/F"): s["name"]("/external")}),
            "pdf_external_stream_unsupported",
        )

    def test_unsupported_filter_fails(self):
        self.rejected(
            lambda s: s["program_ref"]
            .get_object()
            .update({s["name"]("/Filter"): s["name"]("/ASCII85Decode")}),
            "pdf_filter_unsupported",
        )

    def test_oversized_cmap_fails(self):
        self.rejected(
            lambda s: s["font"].update(
                {s["name"]("/ToUnicode"): s["stream"](b"x" * 65537)}
            ),
            "pdf_stream_bound",
        )

    def test_compressed_stream_bomb_fails_inside_worker(self):
        self.rejected(
            lambda s: s["descriptor"].update(
                {
                    s["name"]("/FontFile"): s["stream"](
                        b"%!FontType1-" + b"x" * (5 * 1024 * 1024)
                    )
                }
            ),
            "parser_rejected_input",
        )

    def test_depth_bound_fails(self):
        def mutate(s):
            value = s["dictionary"]()
            s["writer"].root_object[s["name"]("/Deep")] = value
            for _ in range(70):
                child = s["dictionary"]()
                value[s["name"]("/Next")] = child
                value = child

        self.rejected(mutate, "pdf_graph_depth")

    def test_font_count_bound_fails(self):
        def mutate(s):
            for index in range(65):
                font = s["dictionary"](
                    **{str(key)[1:]: value for key, value in s["font"].items()}
                )
                s["page"]["/Resources"]["/Font"][s["name"](f"/F{index + 1}")] = s[
                    "writer"
                ]._add_object(font)

        self.rejected(mutate, "pdf_resource_count")

    def test_graph_node_bound_fails(self):
        def mutate(s):
            s["writer"].root_object[s["name"]("/Many")] = s["array"](
                [s["dictionary"]() for _ in range(worker.PDF_LIMITS["graph_nodes"] + 1)]
            )

        self.rejected(mutate, "pdf_graph_nodes")

    def test_graph_edge_bound_fails(self):
        def mutate(s):
            s["writer"].root_object[s["name"]("/Many")] = s["array"](
                [s["number"](1)] * (worker.PDF_LIMITS["graph_edges"] + 1)
            )

        self.rejected(mutate, "pdf_graph_edges")

    def test_aggregate_decoded_bytes_are_bounded(self):
        def mutate(s):
            payload = b"%!FontType1-" + b"x" * (3 * 1024 * 1024)
            s["descriptor"][s["name"]("/FontFile")] = s["stream"](payload)
            descriptor = s["writer"]._add_object(
                s["dictionary"](
                    Type=s["name"]("/FontDescriptor"),
                    FontName=s["name"]("/Fixture"),
                    FontFile=s["stream"](payload),
                )
            )
            font = s["writer"]._add_object(
                s["dictionary"](
                    Type=s["name"]("/Font"),
                    Subtype=s["name"]("/Type1"),
                    BaseFont=s["name"]("/Fixture"),
                    FontDescriptor=descriptor,
                )
            )
            s["page"]["/Resources"]["/Font"][s["name"]("/Second")] = font

        self.rejected(mutate, "pdf_decoded_bound")

    def test_timeout_removes_the_owned_container_and_staging(self):
        from unittest import mock

        pdf, _ = self.fixture()
        identifier = resources.uuid.uuid4()
        name = "rwml-oracle-" + identifier.hex
        before = (
            set(resources.SCRATCH.iterdir()) if resources.SCRATCH.exists() else set()
        )
        with mock.patch.object(resources.uuid, "uuid4", return_value=identifier):
            with self.assertRaisesRegex(ValueError, "timed out"):
                resources.extract_pdf(pdf, self.wheel, timeout=0.001)
        self.assertEqual(set(resources.SCRATCH.iterdir()), before)
        remaining = runtime.run_bounded(
            ["docker", "ps", "-aq", "--filter", f"name=^{name}$"]
        )
        self.assertEqual(remaining.strip(), b"")

    def test_encrypted_pdf_fails(self):
        self.rejected(lambda s: s["writer"].encrypt("secret"), "pdf_encrypted")

    def test_damaged_pdf_fails_inside_worker(self):
        with self.assertRaises(runtime.ProcessFailed):
            resources.extract_pdf(b"%PDF-1.7\nnot a PDF\n%%EOF", self.wheel)


if __name__ == "__main__":
    unittest.main()
