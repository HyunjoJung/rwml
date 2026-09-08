#!/usr/bin/env python3
"""Describe file and metadata differences in a locally built Docker image archive.

This diagnostic reads archive members without extracting paths or accepting an
image as the locked runtime. Use it on archives produced by the oracle build.
"""

import argparse
import hashlib
import json
from pathlib import Path
import sys
import tarfile

MAX_ARCHIVE_BYTES = 2 * 1024**3
MAX_JSON_BYTES = 2 * 1024**2
MAX_LAYERS = 32
MAX_ENTRIES = 100000
MAX_CONTENT_BYTES = 8 * 1024**3


def read_json(archive, name):
    member = archive.getmember(name)
    if not member.isfile() or not 0 < member.size <= MAX_JSON_BYTES:
        raise ValueError("image JSON member is outside its bound")
    return json.load(archive.extractfile(member))


def inspect_archive(path: Path) -> dict:
    if not 0 < path.stat().st_size <= MAX_ARCHIVE_BYTES:
        raise ValueError("image archive size is outside its bound")
    entries, content_bytes = 0, 0
    with tarfile.open(path, mode="r:") as archive:
        manifest = read_json(archive, "manifest.json")
        if not isinstance(manifest, list) or len(manifest) != 1:
            raise ValueError("image archive must contain exactly one image")
        image = manifest[0]
        names = image["Layers"]
        if not isinstance(names, list) or not 1 <= len(names) <= MAX_LAYERS:
            raise ValueError("image layer count is outside its bound")
        result = {
            "scope": "image-build-diagnostic-not-runtime-acceptance",
            "config": read_json(archive, image["Config"]),
            "layers": [],
        }
        for name in names:
            member = archive.getmember(name)
            if not member.isfile() or not 0 < member.size <= MAX_ARCHIVE_BYTES:
                raise ValueError("image layer member is outside its bound")
            rows = []
            with tarfile.open(fileobj=archive.extractfile(member), mode="r|*") as layer:
                for entry in layer:
                    entries += 1
                    content_bytes += entry.size
                    if entries > MAX_ENTRIES:
                        raise ValueError("image entry count is outside its bound")
                    if content_bytes > MAX_CONTENT_BYTES:
                        raise ValueError("image content size is outside its bound")
                    row = {
                        key: getattr(entry, key)
                        for key in (
                            "name",
                            "mode",
                            "uid",
                            "gid",
                            "size",
                            "mtime",
                            "linkname",
                            "uname",
                            "gname",
                            "pax_headers",
                        )
                    }
                    row["type"] = entry.type.decode("ascii")
                    if entry.isfile():
                        digest = hashlib.sha256()
                        stream = layer.extractfile(entry)
                        while chunk := stream.read(1024 * 1024):
                            digest.update(chunk)
                        row["sha256"] = digest.hexdigest()
                    rows.append(row)
            result["layers"].append({"blob": name, "entries": rows})
    return result


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--archive", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    try:
        result = inspect_archive(args.archive)
        with args.output.open("x", encoding="utf-8", newline="\n") as stream:
            json.dump(result, stream, sort_keys=True, separators=(",", ":"))
            stream.write("\n")
    except (OSError, ValueError, KeyError, TypeError, tarfile.TarError) as error:
        print(f"inspect_oracle_image: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
