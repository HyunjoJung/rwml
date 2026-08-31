#!/usr/bin/env python3
"""Extract declared PDF font resources only inside the isolated Linux worker."""

from __future__ import annotations

import base64
import io
import logging
from pathlib import Path
import re
import sys

if __name__ == "__main__":
    sys.path.insert(0, str(Path(__file__).parent))
import font_subset_worker as common

WHEEL_VERSION = "6.16.2"
WHEEL_BYTES = 385060
WHEEL_SHA256 = "c8b09a59399062fb45a1b8156c18a787a10a3dae03ac9674397a226712c94604"
MAX_PDF_BYTES = 16 * 1024 * 1024
MAX_RESULT_BYTES = 8 * 1024 * 1024
PDF_LIMITS = {
    "pdf_bytes": MAX_PDF_BYTES,
    "graph_nodes": 16384,
    "graph_edges": 65536,
    "graph_depth": 64,
    "font_resources": 64,
    "decoded_bytes": 4 * 1024 * 1024,
    "cmap_bytes": 65536,
    "result_bytes": MAX_RESULT_BYTES,
}
FONT_KEYS = {
    "ref",
    "subtype",
    "base_font",
    "descriptor_font",
    "descendant_ref",
    "descendant_subtype",
    "encoding_kind",
    "program",
    "to_unicode",
}
BLOB_KEYS = {"ref", "kind", "bytes", "sha256", "base64"}
FONT_KINDS = {
    ("Type1", None): "type1-pfa",
    ("TrueType", None): "truetype",
    ("Type0", "CIDFontType0"): "cid-cff",
    ("Type0", "CIDFontType2"): "truetype",
}


def require(condition: bool, reason: str) -> None:
    if not condition:
        raise common.SubsetError(reason)


def sha256(value: object) -> bool:
    return isinstance(value, str) and re.fullmatch(r"[0-9a-f]{64}", value) is not None


def reference(value: object) -> tuple[int, int]:
    require(
        isinstance(value, list)
        and len(value) == 2
        and all(type(number) is int for number in value)
        and 0 < value[0] <= 0x7FFFFFFF
        and 0 <= value[1] <= 65535,
        "pdf_reference",
    )
    return tuple(value)


def font_name(value: object) -> str:
    require(
        isinstance(value, str)
        and re.fullmatch(r"[A-Za-z0-9_.+-]{1,255}", value) is not None,
        "pdf_font_name",
    )
    return value


def validate_request(request: object) -> None:
    require(
        isinstance(request, dict)
        and set(request) == {"schema", "pdf", "worker_sha256", "helper_sha256"}
        and request["schema"] == "rwml.pdf-font-request.v1",
        "request_schema",
    )
    pdf = request["pdf"]
    require(
        isinstance(pdf, dict)
        and set(pdf) == {"bytes", "sha256"}
        and type(pdf["bytes"]) is int
        and 0 < pdf["bytes"] <= MAX_PDF_BYTES
        and sha256(pdf["sha256"])
        and sha256(request["worker_sha256"])
        and sha256(request["helper_sha256"]),
        "request_identity",
    )


def validate_inventory(fonts: object, blobs: object) -> None:
    require(
        isinstance(fonts, list)
        and len(fonts) <= PDF_LIMITS["font_resources"]
        and isinstance(blobs, list)
        and len(blobs) <= 2 * PDF_LIMITS["font_resources"],
        "pdf_resource_count",
    )
    seen, total = {}, 0
    for blob in blobs:
        require(isinstance(blob, dict) and set(blob) == BLOB_KEYS, "pdf_blob_schema")
        identity = reference(blob["ref"])
        kind = blob["kind"]
        require(
            isinstance(kind, str) and kind in {*FONT_KINDS.values(), "to-unicode"},
            "pdf_blob_kind",
        )
        maximum = (
            PDF_LIMITS["cmap_bytes"]
            if kind == "to-unicode"
            else common.MAX_SUBSET_BYTES
        )
        require(
            type(blob["bytes"]) is int
            and 0 < blob["bytes"] <= maximum
            and sha256(blob["sha256"])
            and isinstance(blob["base64"], str)
            and len(blob["base64"]) <= (maximum + 2) // 3 * 4,
            "pdf_blob_bound",
        )
        payload = base64.b64decode(blob["base64"], validate=True)
        require(
            len(payload) == blob["bytes"]
            and common.digest(payload) == blob["sha256"]
            and base64.b64encode(payload).decode() == blob["base64"],
            "pdf_blob_identity",
        )
        prefixes = {
            "type1-pfa": (b"%!FontType1-",),
            "truetype": (b"\x00\x01\x00\x00", b"true"),
            "cid-cff": (b"\x01\x00",),
        }
        require(
            kind == "to-unicode" or payload.startswith(prefixes[kind]),
            "pdf_program_kind",
        )
        require(identity not in seen, "pdf_blob_alias")
        seen[identity] = kind
        total += len(payload)
        require(total <= PDF_LIMITS["decoded_bytes"], "pdf_decoded_bound")
    require(list(seen) == sorted(seen), "pdf_blob_order")
    font_refs, descendant_refs, used = [], set(), set()
    for font in fonts:
        require(isinstance(font, dict) and set(font) == FONT_KEYS, "pdf_font_schema")
        font_refs.append(reference(font["ref"]))
        font_name(font["base_font"])
        font_name(font["descriptor_font"])
        require(
            isinstance(font["subtype"], str)
            and (
                font["descendant_subtype"] is None
                or isinstance(font["descendant_subtype"], str)
            ),
            "pdf_font_kind",
        )
        kind = FONT_KINDS.get((font["subtype"], font["descendant_subtype"]))
        require(kind is not None, "pdf_font_kind")
        encoding = font["encoding_kind"]
        require(isinstance(encoding, str), "pdf_encoding_kind")
        if font["subtype"] == "Type0":
            descendant_refs.add(reference(font["descendant_ref"]))
            require(encoding in ("Identity-H", "Identity-V"), "pdf_encoding_kind")
        else:
            require(
                font["descendant_ref"] is None
                and encoding in ("absent", "name", "dictionary"),
                "pdf_encoding_kind",
            )
        program = reference(font["program"])
        require(seen.get(program) == kind, "pdf_program_reference")
        used.add(program)
        if font["to_unicode"] is not None:
            cmap = reference(font["to_unicode"])
            require(seen.get(cmap) == "to-unicode", "pdf_cmap_reference")
            used.add(cmap)
    require(font_refs == sorted(set(font_refs)), "pdf_font_alias")
    require(
        not (
            set(font_refs) & descendant_refs
            or (set(font_refs) | descendant_refs) & set(seen)
        ),
        "pdf_resource_alias",
    )
    require(used == set(seen), "pdf_unreferenced_blob")


class RejectParserWarning(logging.Handler):
    def emit(self, record):
        raise common.SubsetError("pdf_parser_warning")


def extract_resources(payload: bytes) -> tuple[list, list]:
    from pypdf import PdfReader
    from pypdf.generic import (
        ArrayObject,
        DictionaryObject,
        IndirectObject,
        NameObject,
        NullObject,
        StreamObject,
    )

    reader = PdfReader(io.BytesIO(payload), strict=True)
    require(not reader.is_encrypted, "pdf_encrypted")
    resolved, visited, fonts, descendants, blobs = {}, set(), {}, set(), {}
    total_bytes = 0

    def identity(raw):
        require(isinstance(raw, IndirectObject), "pdf_direct_resource_unsupported")
        return reference([raw.idnum, raw.generation])

    def resolve(raw):
        if not isinstance(raw, IndirectObject):
            return raw
        ref = identity(raw)
        if ref not in resolved:
            require(len(resolved) < PDF_LIMITS["graph_nodes"], "pdf_graph_nodes")
            resolved[ref] = raw.get_object()
            require(
                not isinstance(resolved[ref], (IndirectObject, NullObject)),
                "pdf_unresolved_object",
            )
        return resolved[ref]

    def dictionary(raw):
        value = resolve(raw)
        require(
            isinstance(value, DictionaryObject) and not isinstance(value, StreamObject),
            "pdf_dictionary",
        )
        return value

    def add_font(raw):
        ref = identity(raw)
        value = dictionary(raw)
        require(resolve(value.get("/Type")) in (None, "/Font"), "pdf_font_type")
        require(
            resolve(value.get("/Subtype")) in ("/Type0", "/Type1", "/TrueType"),
            "pdf_font_kind",
        )
        fonts[ref] = value
        require(len(fonts) <= PDF_LIMITS["font_resources"], "pdf_resource_count")

    root = reader.trailer.get("/Root")
    require(resolve(dictionary(root).get("/Type")) == "/Catalog", "pdf_catalog")
    stack, edges = [(root, 0)], 0
    while stack:
        raw, depth = stack.pop()
        value = resolve(raw)
        if not isinstance(value, (DictionaryObject, ArrayObject)):
            continue
        key = (
            ("ref", *identity(raw))
            if isinstance(raw, IndirectObject)
            else ("direct", id(value))
        )
        if key in visited:
            continue
        visited.add(key)
        require(len(visited) <= PDF_LIMITS["graph_nodes"], "pdf_graph_nodes")
        require(depth <= PDF_LIMITS["graph_depth"], "pdf_graph_depth")
        if isinstance(value, DictionaryObject):
            if resolve(value.get("/Type")) == "/Font":
                if resolve(value.get("/Subtype")) in ("/CIDFontType0", "/CIDFontType2"):
                    descendants.add(identity(raw))
                else:
                    add_font(raw)
            if "/Font" in value:
                font_resources = resolve(value.get("/Font"))
                if isinstance(font_resources, ArrayObject):
                    # ExtGState also has a /Font entry: [font size], not a resource map.
                    require(len(font_resources) == 2, "pdf_graphics_font")
                    add_font(font_resources[0])
                else:
                    for _, font in sorted(dictionary(font_resources).items()):
                        add_font(font)
            children = [child for _, child in sorted(value.items())]
        else:
            children = value
        edges += len(children)
        require(edges <= PDF_LIMITS["graph_edges"], "pdf_graph_edges")
        stack.extend((child, depth + 1) for child in reversed(children))

    def blob(raw, kind):
        nonlocal total_bytes
        ref = identity(raw)
        if ref in blobs:
            require(blobs[ref]["kind"] == kind, "pdf_blob_alias")
            return list(ref)
        value = resolve(raw)
        require(isinstance(value, StreamObject), "pdf_stream")
        filter_value = resolve(value.get("/Filter"))
        require(
            filter_value in (None, "/FlateDecode")
            or isinstance(filter_value, ArrayObject)
            and list(filter_value) == ["/FlateDecode"],
            "pdf_filter_unsupported",
        )
        parameters = resolve(value.get("/DecodeParms"))
        require(
            parameters is None
            or isinstance(parameters, NullObject)
            or isinstance(parameters, DictionaryObject)
            and not parameters,
            "pdf_decode_parameters_unsupported",
        )
        require(
            not any(key in value for key in ("/F", "/FFilter", "/FDecodeParms")),
            "pdf_external_stream_unsupported",
        )
        if kind == "cid-cff":
            require(
                resolve(value.get("/Subtype")) == "/CIDFontType0C", "pdf_program_kind"
            )
        elif kind != "to-unicode":
            require("/Subtype" not in value, "pdf_program_kind")
        decoded = value.get_data()
        maximum = (
            PDF_LIMITS["cmap_bytes"]
            if kind == "to-unicode"
            else common.MAX_SUBSET_BYTES
        )
        require(
            isinstance(decoded, bytes) and 0 < len(decoded) <= maximum,
            "pdf_stream_bound",
        )
        total_bytes += len(decoded)
        require(total_bytes <= PDF_LIMITS["decoded_bytes"], "pdf_decoded_bound")
        blobs[ref] = {
            "ref": list(ref),
            "kind": kind,
            "bytes": len(decoded),
            "sha256": common.digest(decoded),
            "base64": base64.b64encode(decoded).decode(),
        }
        return list(ref)

    def name(value):
        value = resolve(value)
        require(isinstance(value, NameObject), "pdf_font_name")
        return font_name(str(value)[1:])

    rows, consumed_descendants = [], set()
    for ref, font in sorted(fonts.items()):
        subtype = name(font.get("/Subtype"))
        descendant_ref, descendant_subtype = None, None
        target = font
        encoding = resolve(font.get("/Encoding"))
        if subtype == "Type0":
            require(
                encoding in ("/Identity-H", "/Identity-V"), "pdf_encoding_unsupported"
            )
            encoding_kind = name(encoding)
            array = resolve(font.get("/DescendantFonts"))
            require(
                isinstance(array, ArrayObject) and len(array) == 1, "pdf_descendants"
            )
            descendant_ref = list(identity(array[0]))
            consumed_descendants.add(tuple(descendant_ref))
            target = dictionary(array[0])
            require(resolve(target.get("/Type")) == "/Font", "pdf_font_type")
            descendant_subtype = name(target.get("/Subtype"))
        elif encoding is None:
            encoding_kind = "absent"
        elif isinstance(encoding, NameObject):
            encoding_kind = "name"
        else:
            dictionary(encoding)
            encoding_kind = "dictionary"
        kind = FONT_KINDS.get((subtype, descendant_subtype))
        require(kind is not None, "pdf_font_kind")
        descriptor = dictionary(target.get("/FontDescriptor"))
        require(
            resolve(descriptor.get("/Type")) in (None, "/FontDescriptor"),
            "pdf_descriptor_type",
        )
        keys = [
            key
            for key in ("/FontFile", "/FontFile2", "/FontFile3")
            if key in descriptor
        ]
        expected_key = {
            "type1-pfa": "/FontFile",
            "truetype": "/FontFile2",
            "cid-cff": "/FontFile3",
        }[kind]
        require(keys == [expected_key], "pdf_embedded_program")
        rows.append(
            {
                "ref": list(ref),
                "subtype": subtype,
                "base_font": name(font.get("/BaseFont")),
                "descriptor_font": name(descriptor.get("/FontName")),
                "descendant_ref": descendant_ref,
                "descendant_subtype": descendant_subtype,
                "encoding_kind": encoding_kind,
                "program": blob(descriptor.get(expected_key), kind),
                "to_unicode": blob(font.get("/ToUnicode"), "to-unicode")
                if "/ToUnicode" in font
                else None,
            }
        )
    require(descendants == consumed_descendants, "pdf_orphan_descendant")
    blob_rows = [value for _, value in sorted(blobs.items())]
    validate_inventory(rows, blob_rows)
    reader.close()
    return rows, blob_rows


def run_worker(directory: Path, output: Path) -> dict:
    common.resource_limits()
    request = common.strict_json(common.read_bounded(directory / "request.json", 65536))
    validate_request(request)
    code = common.read_bounded(Path(__file__), 1024 * 1024)
    helper = common.read_bounded(Path(common.__file__), 1024 * 1024)
    payload = common.read_bounded(directory / "input.pdf", MAX_PDF_BYTES)
    wheel = common.read_bounded(directory / "pypdf.whl", WHEEL_BYTES)
    require(
        common.digest(code) == request["worker_sha256"]
        and common.digest(helper) == request["helper_sha256"],
        "worker_identity",
    )
    require(
        len(wheel) == WHEEL_BYTES and common.digest(wheel) == WHEEL_SHA256,
        "wheel_identity",
    )
    require(
        {"bytes": len(payload), "sha256": common.digest(payload)} == request["pdf"],
        "pdf_input_identity",
    )
    require(payload.startswith(b"%PDF-"), "pdf_header")
    snapshot = output / "pypdf.whl"
    with snapshot.open("xb") as stream:
        stream.write(wheel)
    python_root = "/opt/libreoffice26.2/program/python-core-3.12.13/lib"
    sys.path = [
        str(snapshot),
        *[
            path
            for path in sys.path
            if path.startswith(python_root) and "site-packages" not in path
        ],
    ]
    logger = logging.getLogger("pypdf")
    logger.handlers = [RejectParserWarning(logging.WARNING)]
    logger.setLevel(logging.WARNING)
    logger.propagate = False
    import pypdf
    from pypdf import filters

    require(pypdf.__version__ == WHEEL_VERSION, "parser_version")
    filters.ZLIB_MAX_OUTPUT_LENGTH = PDF_LIMITS["decoded_bytes"]
    fonts, blobs = extract_resources(payload)
    return {
        **request,
        "schema": "rwml.pdf-font-worker.v1",
        "parser_version": WHEEL_VERSION,
        "wheel_sha256": WHEEL_SHA256,
        "python": common.PYTHON_VERSION,
        "limits": {**common.LIMITS, **PDF_LIMITS},
        "fonts": fonts,
        "blobs": blobs,
    }


def main() -> int:
    try:
        result = run_worker(Path("/oracle/source"), Path("/oracle/output"))
        payload = common.canonical(result) + b"\n"
        require(len(payload) <= MAX_RESULT_BYTES, "result_size")
        sys.stdout.buffer.write(payload)
        return 0
    except common.SubsetError as error:
        print(f"pdf_font_worker: {error}", file=sys.stderr)
    except Exception:
        print("pdf_font_worker: parser_rejected_input", file=sys.stderr)
    return 1


if __name__ == "__main__":
    raise SystemExit(main())
