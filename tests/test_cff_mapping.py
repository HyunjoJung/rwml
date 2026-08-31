from pathlib import Path
import sys
from types import SimpleNamespace as NS
import unittest
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))
import cff_mapping_worker as mapping  # noqa: E402


class Glyph:
    def __init__(self, endpoint, width=1000):
        self.endpoint, self.width = endpoint, width

    def draw(self, pen):
        pen.moveTo((0, 0))
        pen.lineTo((self.endpoint, 100))
        pen.closePath()


class CFFMappingTests(unittest.TestCase):
    def discover(
        self, *, text="4", edges=None, ligatures=None, source=None, endpoint=20
    ):
        source = source or {
            ".notdef": Glyph(0),
            **{f"cid{index:05}": Glyph(index * 10) for index in range(1, 7)},
        }
        subset = {".notdef": Glyph(0), "cid00001": Glyph(endpoint)}
        graph = mapping.CandidateGraph(edges or {}, ligatures or [])
        return mapping.discover(
            source,
            subset,
            {1: text},
            {ord("4"): "cid00001", ord("f"): "cid00003", ord("l"): "cid00004"},
            graph,
        )

    def test_default_unicode_glyph_is_not_assumed_to_be_the_match(self):
        result = self.discover(edges={"cid00001": {"cid00002"}})
        self.assertEqual(
            result["glyphs"], [[".notdef", ".notdef"], ["cid00001", "cid00002"]]
        )

    def test_ligature_uses_all_unicode_components(self):
        result = self.discover(
            text="fl", ligatures=[(("cid00003", "cid00004"), "cid00005")], endpoint=50
        )
        self.assertEqual(result["glyphs"][1], ["cid00001", "cid00005"])

    def test_cycles_terminate_without_duplicate_candidates(self):
        result = self.discover(
            edges={"cid00001": {"cid00002"}, "cid00002": {"cid00001"}}
        )
        self.assertEqual(result["glyphs"][1][1], "cid00002")

    def test_no_match_fails_instead_of_returning_partial_map(self):
        with self.assertRaisesRegex(ValueError, "unmatched"):
            self.discover()

    def test_equal_candidates_are_explicitly_ambiguous(self):
        source = {".notdef": Glyph(0), "cid00001": Glyph(20), "cid00002": Glyph(20)}
        with self.assertRaisesRegex(ValueError, "ambiguous"):
            self.discover(source=source, edges={"cid00001": {"cid00002"}})

    def test_candidate_closure_is_bounded(self):
        with mock.patch.dict(mapping.MAPPING_LIMITS, {"candidates_per_glyph": 1}):
            with self.assertRaisesRegex(ValueError, "candidate_bound"):
                self.discover(edges={"cid00001": {"cid00002"}})

    def test_hints_require_complete_nonzero_cids_and_scalar_text(self):
        for hints in (
            {},
            {0: "4", 1: "4"},
            {2: "4"},
            {1: ""},
            {1: "\ud800"},
            {1: "a" * 9},
        ):
            with self.subTest(hints=hints), self.assertRaises(ValueError):
                mapping.validate_hints(hints, 2)

    def test_graph_builder_supports_extensions_without_context_simulation(self):
        single = NS(mapping={"cid00001": "cid00002"})
        extension = NS(ExtensionLookupType=1, ExtSubTable=single)
        lookup = NS(LookupType=7, SubTable=[extension])
        graph = mapping.build_graph([lookup], {".notdef", "cid00001", "cid00002"})
        self.assertEqual(graph.closure("cid00001"), {"cid00001", "cid00002"})

    def test_outline_source_and_search_work_are_bounded(self):
        for key, limit, reason in (
            ("candidate_commands", 1, "outline_work_bound"),
            ("candidate_source_glyphs", 1, "source_bound"),
            ("candidate_search_steps", 0, "search_bound"),
        ):
            with (
                self.subTest(key=key),
                mock.patch.dict(mapping.MAPPING_LIMITS, {key: limit}),
            ):
                with self.assertRaisesRegex(ValueError, reason):
                    self.discover(edges={"cid00001": {"cid00002"}})

    def test_gsub_graph_sizes_and_invalid_targets_are_rejected(self):
        lookup = NS(LookupType=1, SubTable=[NS(mapping={"cid00001": "cid00002"})])
        names = {".notdef", "cid00001", "cid00002"}
        for key in ("gsub_lookups", "gsub_subtables", "gsub_edges"):
            with (
                self.subTest(key=key),
                mock.patch.dict(mapping.MAPPING_LIMITS, {key: 0}),
            ):
                with self.assertRaises(ValueError):
                    mapping.build_graph([lookup], names)
        with self.assertRaisesRegex(ValueError, "gsub_glyph"):
            mapping.build_graph([lookup], {".notdef", "cid00001"})

    def test_ligature_count_and_components_are_bounded(self):
        record = NS(Component=["cid00002"], CompCount=2, LigGlyph="cid00003")
        lookup = NS(LookupType=4, SubTable=[NS(ligatures={"cid00001": [record]})])
        names = {f"cid{index:05}" for index in range(1, 4)}
        with mock.patch.dict(mapping.MAPPING_LIMITS, {"ligature_records": 0}):
            with self.assertRaisesRegex(ValueError, "ligature_bound"):
                mapping.build_graph([lookup], names)
        record.CompCount = 3
        with self.assertRaisesRegex(ValueError, "ligature_components"):
            mapping.build_graph([lookup], names)

    def test_recursive_extension_is_not_traversed(self):
        extension = NS(ExtensionLookupType=7)
        extension.ExtSubTable = extension
        with self.assertRaisesRegex(ValueError, "extension"):
            mapping.build_graph([NS(LookupType=7, SubTable=[extension])], set())


if __name__ == "__main__":
    unittest.main()
