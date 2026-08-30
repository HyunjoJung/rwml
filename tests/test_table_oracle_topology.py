import copy
import importlib.util
import io
import pathlib
import sys
import tempfile
import types
import unittest


ROOT = pathlib.Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "scripts" / "table_oracle_topology.py"


def load_module(name: str, path: pathlib.Path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    assert spec.loader is not None
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


topology = load_module("table_oracle_topology", SCRIPT)


def token(token_id, paint_order, x, y):
    return {
        "id": token_id,
        "paint_order": paint_order,
        "bbox_millipoints": [x, y, x + 22000, y + 9000],
    }


def vertical(axis, start, end):
    return {
        "axis_millipoints": axis,
        "start_millipoints": start,
        "end_millipoints": end,
        "thickness_millipoints": 1000,
    }


def horizontal(axis, start, end):
    return {
        "axis_millipoints": axis,
        "start_millipoints": start,
        "end_millipoints": end,
        "thickness_millipoints": 1000,
    }


def boundary_pages(case, *, shifted=0):
    pages = [
        {
            "number": 1,
            "width_millipoints": 360000,
            "height_millipoints": 360000,
            "tokens": [],
            "horizontal_borders": [],
            "vertical_borders": [],
        }
    ]
    tracks = [
        (1, 12, 36000 + shifted, 106000, 262000),
        (13, 20, 189000 + shifted, 36000, 150000),
    ]
    paint_order = 0
    for first, last, left, top, bottom in tracks:
        pages[0]["vertical_borders"].extend(
            vertical(edge, top, bottom)
            for edge in (left, left + 40000, left + 80000)
        )
        pages[0]["horizontal_borders"].extend(
            (horizontal(top, left, left + 80000), horizontal(bottom, left, left + 80000))
        )
        for unit in range(first, last + 1):
            y = top + (unit - first) * 12000 + 1000
            pages[0]["tokens"].append(
                token(f"T{case.index:02d}L{unit:02d}", paint_order, left + 1000, y)
            )
            paint_order += 1
            pages[0]["tokens"].append(
                token(f"T{case.index:02d}R{unit:02d}", paint_order, left + 41000, y)
            )
            paint_order += 1
    pages[0]["tokens"].sort(key=lambda item: item["id"])
    pages[0]["horizontal_borders"].sort(
        key=lambda item: (
            item["axis_millipoints"],
            item["start_millipoints"],
            item["end_millipoints"],
            item["thickness_millipoints"],
        )
    )
    pages[0]["vertical_borders"].sort(
        key=lambda item: (
            item["axis_millipoints"],
            item["start_millipoints"],
            item["end_millipoints"],
            item["thickness_millipoints"],
        )
    )
    return pages


class TableOracleTopologyTests(unittest.TestCase):
    def test_case_contract_has_exact_unique_tokens(self):
        self.assertEqual(len(topology.CASES), 48)
        all_tokens = set()
        for case in topology.CASES:
            expected = topology.expected_token_ids(case)
            count = 40 if case.fragment == "row-boundary" else 52
            self.assertEqual(len(expected), count)
            self.assertEqual(len(set(expected)), count)
            self.assertTrue(all_tokens.isdisjoint(expected))
            all_tokens.update(expected)
        self.assertEqual(len(all_tokens), 48 * 46)

    def test_derives_consecutive_table_segments_from_tokens_and_borders(self):
        case = topology.case_by_id("equal-auto-row-boundary-column")
        pages = boundary_pages(case)
        result = topology.derive_topology(case, pages)

        self.assertEqual(result["expected_tokens"], 40)
        self.assertEqual(result["observed_tokens"], 40)
        self.assertEqual(result["paired_units"], 20)
        self.assertEqual(result["pair_page_matches"], 20)
        self.assertEqual(result["pair_y_aligned"], 20)
        self.assertTrue(result["paint_sequence_exact"])
        self.assertEqual(
            result["segments"],
            [
                {
                    "first_unit": 1,
                    "last_unit": 12,
                    "page": 1,
                    "left_millipoints": 36000,
                    "divider_millipoints": 76000,
                    "right_millipoints": 116000,
                    "top_millipoints": 106000,
                    "bottom_millipoints": 262000,
                },
                {
                    "first_unit": 13,
                    "last_unit": 20,
                    "page": 1,
                    "left_millipoints": 189000,
                    "divider_millipoints": 229000,
                    "right_millipoints": 269000,
                    "top_millipoints": 36000,
                    "bottom_millipoints": 150000,
                },
            ],
        )

    def test_comparison_separates_partition_and_geometry(self):
        case = topology.case_by_id("equal-auto-row-boundary-column")
        reference = {
            "case_id": case.case_id,
            "pages": boundary_pages(case),
        }
        reference["topology"] = topology.derive_topology(case, reference["pages"])
        candidate = {
            "case_id": case.case_id,
            "pages": boundary_pages(case, shifted=1000),
        }
        candidate["topology"] = topology.derive_topology(case, candidate["pages"])

        result = topology.compare_document_capture(case, candidate, reference)
        self.assertEqual(result["candidate_pages"], 1)
        self.assertEqual(result["reference_pages"], 1)
        self.assertEqual(result["matched_tokens"], 40)
        self.assertEqual(result["token_page_matches"], 40)
        self.assertTrue(result["segment_partition_exact"])
        self.assertFalse(result["segment_geometry_exact"])
        self.assertEqual(result["max_token_bbox_delta_millipoints"], 1000)
        self.assertEqual(result["max_segment_edge_delta_millipoints"], 1000)

    def test_clipped_outer_border_is_recovered_before_overflowing_text(self):
        page = {
            "vertical_borders": [
                vertical(35500, 106000, 315000),
                vertical(95500, 107000, 314000),
                vertical(102000, 106000, 315000),
            ]
        }
        left = token("T28L01", 0, 36600, 107000)
        right = token("T28R01", 1, 96600, 107000)

        self.assertEqual(
            topology._unit_edges(page, left, right),
            (35500, 95500, 102000, 106000, 315000),
        )

    def test_border_normalization_accepts_gray_and_stroked_rectangles(self):
        rect = types.SimpleNamespace(x0=10.0, y0=20.0, x1=90.0, y1=60.0)
        drawings = [
            {
                "width": 1.0,
                "color": (0.0,),
                "fill": None,
                "items": [("re", rect, 1)],
            }
        ]
        horizontal_borders, vertical_borders = topology._extract_axis_borders(drawings)

        self.assertEqual(
            [
                (
                    row["axis_millipoints"],
                    row["start_millipoints"],
                    row["end_millipoints"],
                )
                for row in horizontal_borders
            ],
            [(20000, 10000, 90000), (60000, 10000, 90000)],
        )
        self.assertEqual(
            [
                (
                    row["axis_millipoints"],
                    row["start_millipoints"],
                    row["end_millipoints"],
                )
                for row in vertical_borders
            ],
            [(10000, 20000, 60000), (90000, 20000, 60000)],
        )
        self.assertTrue(topology._dark_color((0.0, 0.0, 0.0, 1.0)))
        self.assertFalse(topology._dark_color((0.0, 0.0, 0.0, 0.0)))

    def test_document_contract_recomputes_topology(self):
        case = topology.case_by_id("equal-auto-row-boundary-column")
        pages = boundary_pages(case)
        document = types.SimpleNamespace(input_bytes=7810, sha256="a" * 64)
        capture = {
            "case_id": case.case_id,
            "input_bytes": document.input_bytes,
            "input_sha256": document.sha256,
            "pdf": {"bytes": 1024, "sha256": "b" * 64, "pages": 1},
            "pages": pages,
            "topology": topology.derive_topology(case, pages),
        }
        topology._validate_document_capture(
            capture, document, case, topology._limits()
        )

        tampered = copy.deepcopy(capture)
        tampered["topology"]["segments"][0]["last_unit"] -= 1
        with self.assertRaisesRegex(ValueError, "topology is inconsistent"):
            topology._validate_document_capture(
                tampered, document, case, topology._limits()
            )

    @unittest.skipIf(topology.pymupdf is None, "PyMuPDF is not installed")
    def test_extracts_bounded_pdf_tokens_and_vector_borders(self):
        case = topology.case_by_id("equal-auto-row-boundary-column")
        document = topology.pymupdf.open()
        page = document.new_page(width=360, height=360)
        paint_order = 0
        for first, last, left, top in ((1, 12, 36, 106), (13, 20, 189, 36)):
            bottom = top + (last - first + 1) * 12
            for edge in (left, left + 40, left + 80):
                page.draw_line((edge, top), (edge, bottom), color=(0, 0, 0), width=1)
            for row in range(first, last + 1):
                y = top + (row - first) * 12 + 9
                for side, x in (("L", left + 1), ("R", left + 41)):
                    page.insert_text(
                        (x, y),
                        f"T{case.index:02d}{side}{row:02d}",
                        fontsize=8,
                        fontname="helv",
                    )
                    paint_order += 1
        payload = document.tobytes(garbage=4, deflate=True)
        document.close()

        with tempfile.TemporaryDirectory() as tmp:
            path = pathlib.Path(tmp) / f"{case.case_id}.pdf"
            path.write_bytes(payload)
            capture = topology.extract_pdf(path, case)

        self.assertEqual(capture["pdf"]["pages"], 1)
        self.assertEqual(len(capture["pages"][0]["tokens"]), 40)
        self.assertEqual(len(capture["topology"]["segments"]), 2)
        self.assertTrue(capture["topology"]["paint_sequence_exact"])

    def test_missing_or_duplicate_tokens_fail_closed(self):
        case = topology.case_by_id("equal-auto-row-boundary-column")
        pages = boundary_pages(case)
        pages[0]["tokens"].pop()
        with self.assertRaisesRegex(ValueError, "token coverage"):
            topology.derive_topology(case, pages)

        pages = boundary_pages(case)
        pages[0]["tokens"].append(dict(pages[0]["tokens"][0]))
        with self.assertRaisesRegex(ValueError, "duplicate token"):
            topology.derive_topology(case, pages)


if __name__ == "__main__":
    unittest.main()
