#!/usr/bin/env python3
"""Discover bounded CFF source witnesses; the independent proof worker verifies them."""

from __future__ import annotations

import io
import logging
from pathlib import Path
import re
import sys

if __name__ == "__main__":
    sys.path.insert(0, str(Path(__file__).parent))
import font_subset_worker as common
import pdf_font_worker as pdf

MAPPING_LIMITS = {
    "gsub_lookups": 1024,
    "gsub_subtables": 4096,
    "gsub_edges": 65536,
    "ligature_records": 4096,
    "hint_scalars": 8,
    "candidates_per_glyph": 256,
    "candidate_source_glyphs": 4096,
    "candidate_commands": 131072,
    "candidate_search_steps": 131072,
}
HELPERS = ("font_subset_worker.py", "pdf_font_worker.py")
require = pdf.require


class CandidateGraph:
    def __init__(self, edges: dict, ligatures: list):
        self.edges = edges
        self.ligatures = ligatures
        self.search = common.Budget(MAPPING_LIMITS["candidate_search_steps"])

    def step(self):
        self.search.remaining -= 1
        require(self.search.remaining >= 0, "mapping_search_bound")

    def closure(self, name: str) -> set[str]:
        found, pending = {name}, [name]
        while pending:
            for other in sorted(self.edges.get(pending.pop(), ())):
                self.step()
                if other not in found:
                    found.add(other)
                    pending.append(other)
                require(
                    len(found) <= MAPPING_LIMITS["candidates_per_glyph"],
                    "mapping_candidate_bound",
                )
        return found

    def candidates(self, text: str, cmap: dict) -> set[str]:
        require(
            all(ord(character) in cmap for character in text),
            "mapping_source_cmap_missing",
        )
        components = [self.closure(cmap[ord(character)]) for character in text]
        if len(components) == 1:
            return components[0]
        found = set()
        for sequence, target in self.ligatures:
            self.step()
            if len(sequence) != len(components):
                continue
            matches = True
            for name, allowed in zip(sequence, components):
                self.step()
                if name not in allowed:
                    matches = False
                    break
            if matches:
                found.update(self.closure(target))
            require(
                len(found) <= MAPPING_LIMITS["candidates_per_glyph"],
                "mapping_candidate_bound",
            )
        return found


def build_graph(lookups: list, glyph_names: set) -> CandidateGraph:
    require(
        isinstance(lookups, list) and len(lookups) <= MAPPING_LIMITS["gsub_lookups"],
        "mapping_lookup_bound",
    )
    edges, ligatures, edge_count, subtable_count = {}, [], 0, 0

    def glyph(name):
        require(isinstance(name, str) and name in glyph_names, "mapping_gsub_glyph")
        return name

    for lookup in lookups:
        require(
            type(lookup.LookupType) is int
            and 1 <= lookup.LookupType <= 8
            and isinstance(lookup.SubTable, list),
            "mapping_gsub_structure",
        )
        subtable_count += len(lookup.SubTable)
        require(
            subtable_count <= MAPPING_LIMITS["gsub_subtables"], "mapping_subtable_bound"
        )
        for subtable in lookup.SubTable:
            kind = lookup.LookupType
            if kind == 7:
                kind = subtable.ExtensionLookupType
                require(
                    type(kind) is int and 1 <= kind <= 8 and kind != 7,
                    "mapping_extension",
                )
                subtable = subtable.ExtSubTable
            # These are candidates, not script/feature/context shaping decisions.
            if kind in (1, 3):
                values = subtable.mapping if kind == 1 else subtable.alternates
                require(isinstance(values, dict), "mapping_gsub_structure")
                for name, targets in values.items():
                    glyph(name)
                    targets = [targets] if kind == 1 else targets
                    require(isinstance(targets, list), "mapping_gsub_structure")
                    edge_count += len(targets)
                    require(
                        edge_count <= MAPPING_LIMITS["gsub_edges"], "mapping_edge_bound"
                    )
                    edges.setdefault(name, set()).update(
                        glyph(target) for target in targets
                    )
            elif kind == 4:
                require(isinstance(subtable.ligatures, dict), "mapping_gsub_structure")
                for first, records in subtable.ligatures.items():
                    glyph(first)
                    require(isinstance(records, list), "mapping_gsub_structure")
                    for record in records:
                        sequence = (first, *record.Component)
                        require(
                            2 <= len(sequence) <= MAPPING_LIMITS["hint_scalars"]
                            and record.CompCount == len(sequence),
                            "mapping_ligature_components",
                        )
                        ligatures.append(
                            (
                                tuple(glyph(name) for name in sequence),
                                glyph(record.LigGlyph),
                            )
                        )
                        require(
                            len(ligatures) <= MAPPING_LIMITS["ligature_records"],
                            "mapping_ligature_bound",
                        )
    return CandidateGraph(edges, sorted(set(ligatures)))


def validate_hints(hints: object, count: int) -> None:
    require(
        isinstance(hints, dict) and 2 <= count <= common.LIMITS["max_glyphs"],
        "mapping_hint_count",
    )
    require(
        all(type(key) is int for key in hints) and set(hints) == set(range(1, count)),
        "mapping_hint_coverage",
    )
    for text in hints.values():
        require(
            isinstance(text, str)
            and 1 <= len(text) <= MAPPING_LIMITS["hint_scalars"]
            and not any(0xD800 <= ord(character) <= 0xDFFF for character in text),
            "mapping_hint_text",
        )


def parse_hints(payload: bytes, count: int) -> dict:
    from pypdf import _cmap
    from pypdf.generic import DecodedStreamObject, DictionaryObject, NameObject

    _cmap.MAPPING_DICTIONARY_SIZE_LIMIT = common.LIMITS["max_glyphs"]
    stream = DecodedStreamObject()
    stream.set_data(payload)
    parsed, entries = _cmap._parse_to_unicode(
        DictionaryObject({NameObject("/ToUnicode"): stream})
    )
    require(len(entries) == len(set(entries)) == len(parsed), "mapping_hint_duplicates")
    require(
        all(isinstance(key, str) and len(key) == 1 for key in parsed),
        "mapping_hint_code",
    )
    hints = {ord(key): text for key, text in parsed.items()}
    require(set(entries) == set(hints), "mapping_hint_code")
    validate_hints(hints, count)
    return hints


def discover(source, subset, hints: dict, cmap: dict, graph: CandidateGraph) -> dict:
    validate_hints(hints, len(subset))
    names = [".notdef", *[f"cid{index:05d}" for index in range(1, len(subset))]]
    require(sorted(subset) == names, "mapping_subset_charset")
    budget = common.Budget(MAPPING_LIMITS["candidate_commands"])
    cache, rows = {}, []

    def signature(glyph):
        pen = common.BoundedPen(budget)
        glyph.draw(pen)
        return common.number(glyph.width), common.digest(common.canonical(pen.commands))

    for index, name in enumerate(names):
        expected = signature(subset[name])
        candidates = {".notdef"} if index == 0 else graph.candidates(hints[index], cmap)
        matches = []
        for candidate in sorted(candidates):
            require(candidate in source, "mapping_source_glyph")
            if candidate not in cache:
                require(
                    len(cache) < MAPPING_LIMITS["candidate_source_glyphs"],
                    "mapping_source_bound",
                )
                cache[candidate] = signature(source[candidate])
            if cache[candidate] == expected:
                matches.append(candidate)
        require(matches, "mapping_glyph_unmatched")
        require(len(matches) == 1, "mapping_glyph_ambiguous")
        rows.append([name, matches[0]])
    common.validate_cff_map(rows)
    return {
        "glyphs": rows,
        "stats": {
            "source_glyphs": len(cache),
            "outline_commands": MAPPING_LIMITS["candidate_commands"] - budget.remaining,
            "search_steps": MAPPING_LIMITS["candidate_search_steps"]
            - graph.search.remaining,
        },
    }


def validate_request(request: object) -> None:
    require(
        isinstance(request, dict)
        and set(request)
        == {"schema", "source", "program", "cmap", "worker_sha256", "helpers"}
        and request["schema"] == "rwml.cff-discovery-request.v1",
        "mapping_request_schema",
    )
    for name, maximum in (
        ("source", common.MAX_SOURCE_BYTES),
        ("program", common.MAX_SUBSET_BYTES),
        ("cmap", pdf.PDF_LIMITS["cmap_bytes"]),
    ):
        entry = request[name]
        keys = {"bytes", "sha256"} | (
            {"postscript_name", "sfnt_revision"} if name == "source" else set()
        )
        require(
            isinstance(entry, dict)
            and set(entry) == keys
            and type(entry["bytes"]) is int
            and 0 < entry["bytes"] <= maximum
            and pdf.sha256(entry["sha256"]),
            "mapping_request_input",
        )
    source = request["source"]
    require(
        isinstance(source["postscript_name"], str)
        and re.fullmatch(r"[A-Za-z0-9_.-]{1,127}", source["postscript_name"])
        is not None
        and type(source["sfnt_revision"]) is int
        and 0 < source["sfnt_revision"] <= 0xFFFFFFFF,
        "mapping_source_identity",
    )
    require(
        pdf.sha256(request["worker_sha256"])
        and isinstance(request["helpers"], dict)
        and set(request["helpers"]) == set(HELPERS)
        and all(pdf.sha256(value) for value in request["helpers"].values()),
        "mapping_worker_identity",
    )


def run_worker(directory: Path, output: Path) -> dict:
    common.resource_limits()
    request = common.strict_json(common.read_bounded(directory / "request.json", 65536))
    validate_request(request)
    require(
        common.digest(common.read_bounded(Path(__file__), 1024 * 1024))
        == request["worker_sha256"],
        "mapping_worker_identity",
    )
    for name in HELPERS:
        require(
            common.digest(common.read_bounded(directory / name, 1024 * 1024))
            == request["helpers"][name],
            "mapping_helper_identity",
        )
    inputs = {}
    for name, filename in (
        ("source", "source.otf"),
        ("program", "subset.cff"),
        ("cmap", "unicode.cmap"),
    ):
        entry = request[name]
        value = common.read_bounded(directory / filename, entry["bytes"])
        require(
            len(value) == entry["bytes"] and common.digest(value) == entry["sha256"],
            "mapping_input_identity",
        )
        inputs[name] = value
    require(
        inputs["source"].startswith(b"OTTO")
        and inputs["program"].startswith(b"\x01\x00"),
        "mapping_font_representation",
    )
    for name, size, digest in (
        ("fonttools", common.WHEEL_BYTES, common.WHEEL_SHA256),
        ("pypdf", pdf.WHEEL_BYTES, pdf.WHEEL_SHA256),
    ):
        payload = common.read_bounded(directory / f"{name}.whl", size)
        require(
            len(payload) == size and common.digest(payload) == digest,
            "mapping_wheel_identity",
        )
        with (output / f"{name}.whl").open("xb") as stream:
            stream.write(payload)
    python_root = "/opt/libreoffice26.2/program/python-core-3.12.13/lib"
    sys.path = [
        str(output / "fonttools.whl"),
        str(output / "pypdf.whl"),
        *[
            path
            for path in sys.path
            if path.startswith(python_root) and "site-packages" not in path
        ],
    ]
    logger = logging.getLogger("pypdf")
    logger.handlers = [pdf.RejectParserWarning(logging.WARNING)]
    logger.setLevel(logging.WARNING)
    logger.propagate = False
    import fontTools
    import pypdf
    from fontTools.ttLib import TTFont

    require(
        fontTools.version == common.WHEEL_VERSION
        and pypdf.__version__ == pdf.WHEEL_VERSION,
        "mapping_tool_version",
    )
    font = TTFont(io.BytesIO(inputs["source"]), lazy=True)
    require(
        "CFF " in font and "fvar" not in font and len(font.getGlyphOrder()) <= 65536,
        "mapping_source_format",
    )
    name = request["source"]["postscript_name"]
    require(
        font["name"].getDebugName(6) == name
        and round(font["head"].fontRevision * 65536)
        == request["source"]["sfnt_revision"],
        "mapping_source_identity",
    )
    common.require_identity_fd_matrices(font["CFF "].cff.topDictIndex[0])
    subset, _ = common.read_cff_subset(inputs["program"], name)
    hints = parse_hints(inputs["cmap"], len(subset))
    lookups = font["GSUB"].table.LookupList.Lookup if "GSUB" in font else []
    graph = build_graph(lookups, set(font.getGlyphOrder()))
    result = discover(
        font.getGlyphSet(), subset, hints, font.getBestCmap() or {}, graph
    )
    font.close()
    return {
        **request,
        "schema": "rwml.cff-discovery-worker.v1",
        "fonttools_version": common.WHEEL_VERSION,
        "fonttools_sha256": common.WHEEL_SHA256,
        "pypdf_version": pdf.WHEEL_VERSION,
        "pypdf_sha256": pdf.WHEEL_SHA256,
        "python": common.PYTHON_VERSION,
        "limits": {**common.LIMITS, **MAPPING_LIMITS},
        **result,
    }


def main() -> int:
    try:
        result = run_worker(Path("/oracle/source"), Path("/oracle/output"))
        payload = common.canonical(result) + b"\n"
        require(len(payload) <= common.MAX_RESULT_BYTES, "mapping_result_bound")
        sys.stdout.buffer.write(payload)
        return 0
    except common.SubsetError as error:
        print(f"cff_mapping_worker: {error}", file=sys.stderr)
    except Exception:
        print("cff_mapping_worker: parser_rejected_input", file=sys.stderr)
    return 1


if __name__ == "__main__":
    raise SystemExit(main())
