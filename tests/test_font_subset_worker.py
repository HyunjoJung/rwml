import copy
import math
from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))
import font_subset_worker as worker  # noqa: E402


class Glyph:
    def __init__(self, width=1000, points=((0, 0), (100, 200))):
        self.width = width
        self.points = points

    def draw(self, pen):
        pen.moveTo(self.points[0])
        pen.lineTo(self.points[1])
        pen.closePath()


class FontSubsetWorkerTests(unittest.TestCase):
    def setUp(self):
        self.source = {".notdef": Glyph(), "cid00001": Glyph()}
        self.subset = {".notdef": Glyph(), "cid1": Glyph()}
        self.matrix = [0.001, 0, 0, 0.001, 0, 0]

    def compare(self, **kwargs):
        return worker.compare_glyphs(
            self.source, self.subset, self.matrix, self.matrix, **kwargs
        )

    def test_every_glyph_and_notdef_are_compared_exactly(self):
        result = self.compare()
        self.assertEqual(result["glyph_count"], 2)
        self.assertEqual(
            [row["source"] for row in result["glyphs"]], [".notdef", "cid00001"]
        )
        self.assertEqual(result, self.compare())

    def test_integer_and_exact_float_coordinates_have_one_identity(self):
        first = self.compare()
        self.subset["cid1"] = Glyph(1000.0, ((0.0, 0), (100, 200.0)))
        self.assertEqual(first, self.compare())

    def test_different_width_and_different_outline_are_rejected(self):
        self.subset["cid1"].width = 999
        with self.assertRaisesRegex(worker.SubsetError, "width"):
            self.compare()
        self.subset["cid1"] = Glyph(points=((0, 0), (100, 201)))
        with self.assertRaisesRegex(worker.SubsetError, "outline"):
            self.compare()

    def test_matrix_change_is_not_normalized_away(self):
        matrix = copy.deepcopy(self.matrix)
        matrix[0] = 0.002
        with self.assertRaisesRegex(worker.SubsetError, "matrix"):
            worker.compare_glyphs(self.source, self.subset, self.matrix, matrix)

    def test_empty_notdef_only_and_missing_notdef_subsets_fail(self):
        for subset in ({}, {".notdef": Glyph()}, {"cid1": Glyph()}):
            self.subset = subset
            with self.assertRaises(worker.SubsetError):
                self.compare()

    def test_cid_aliases_unknown_cids_and_noncanonical_names_fail(self):
        for name in ("cid01", "cid2", "cid-1", "uni4E00", "cid100000"):
            with self.subTest(name=name):
                self.subset = {".notdef": Glyph(), "cid1": Glyph(), name: Glyph()}
                with self.assertRaises(worker.SubsetError):
                    self.compare()

    def test_nonfinite_boolean_and_excessive_coordinates_fail(self):
        for value in (True, math.inf, math.nan, 2**40, 2**4096, 1e-200):
            with self.subTest(value=value):
                self.subset["cid1"] = Glyph(points=((0, 0), (value, 1)))
                with self.assertRaises(worker.SubsetError):
                    self.compare()

    def test_global_and_per_glyph_work_are_bounded(self):
        with self.assertRaisesRegex(worker.SubsetError, "work"):
            self.compare(command_limit=3)
        pen = worker.BoundedPen(worker.Budget(100), glyph_limit=1)
        pen.moveTo((0, 0))
        with self.assertRaisesRegex(worker.SubsetError, "work"):
            pen.lineTo((1, 1))

    def test_composite_and_quadratic_commands_are_not_silently_accepted(self):
        pen = worker.BoundedPen(worker.Budget(100))
        for operation in (
            lambda: pen.addComponent("a", (1, 0, 0, 1, 0, 0)),
            lambda: pen.qCurveTo((1, 1), (2, 2)),
            lambda: pen.curveTo((1, 1)),
        ):
            with self.assertRaises(worker.SubsetError):
                operation()

    def test_glyph_count_bound_precedes_drawing(self):
        self.subset.update({f"cid{index}": Glyph() for index in range(2, 1030)})
        with self.assertRaisesRegex(worker.SubsetError, "glyph_count"):
            self.compare()

    def test_request_requires_exact_nested_types_and_representation(self):
        request = {
            "schema": "rwml.font-subset-request.v1",
            "worker_sha256": "a" * 64,
            "source": {
                "bytes": 64,
                "sha256": "b" * 64,
                "postscript_name": "Locked-CJK",
                "sfnt_revision": 65536,
            },
            "subset": {
                "bytes": 128,
                "sha256": "c" * 64,
                "representation": "type1-pfa",
            },
        }
        worker.validate_request(request)
        for section, key, value in (
            ("source", "sfnt_revision", True),
            ("source", "sfnt_revision", 65536.0),
            ("source", "bytes", 0),
            ("source", "bytes", worker.MAX_SOURCE_BYTES + 1),
            ("source", "postscript_name", "not a name"),
            ("source", "sha256", "B" * 64),
            ("subset", "representation", "cff"),
            ("subset", "bytes", True),
            ("subset", "extra", 1),
        ):
            changed = copy.deepcopy(request)
            changed[section][key] = value
            with self.subTest(section=section, key=key, value=value):
                with self.assertRaises(worker.SubsetError):
                    worker.validate_request(changed)


if __name__ == "__main__":
    unittest.main()
