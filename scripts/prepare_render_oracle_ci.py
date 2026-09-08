#!/usr/bin/env python3
"""Download verified public prerequisites for diagnostic render CI.

Capture itself remains offline. This setup command accepts only sources from
the existing runtime, font, and wheel locks; it does not install host fonts.
"""

from __future__ import annotations

import argparse
import hashlib
from pathlib import Path
import shutil
import sys
import tempfile
import urllib.parse
import urllib.request
import zipfile

import font_subset_attestation as attestation
import libreoffice_container as runtime
import libreoffice_oracle_fonts as base_fonts
import pdf_font_resources as resources
import shared_oracle_fonts as shared
from render_oracle_contract import _load_json


def verify_payload(payload: bytes, expected: dict) -> None:
    if (
        len(payload) != expected["bytes"]
        or runtime.sha256(payload) != expected["sha256"]
    ):
        raise ValueError("downloaded source identity differs from its lock")


def download(cache: Path, url: str, expected: dict) -> Path:
    if urllib.parse.urlsplit(url).scheme != "https":
        raise ValueError("source download requires HTTPS")
    cache.mkdir(parents=True, exist_ok=True)
    destination = cache / expected["sha256"]
    if destination.exists() or destination.is_symlink():
        verify_payload(
            runtime.read_regular_file(destination, expected["bytes"]), expected
        )
        return destination
    temporary = None
    try:
        with urllib.request.urlopen(url, timeout=60) as response:
            if urllib.parse.urlsplit(response.geturl()).scheme != "https":
                raise ValueError("source download redirected away from HTTPS")
            with tempfile.NamedTemporaryFile(dir=cache, delete=False) as stream:
                temporary = Path(stream.name)
                count = 0
                digest = hashlib.sha256()
                while chunk := response.read(
                    min(1024 * 1024, expected["bytes"] - count + 1)
                ):
                    count += len(chunk)
                    if count > expected["bytes"]:
                        raise ValueError(
                            "source download exceeds its locked byte length"
                        )
                    digest.update(chunk)
                    stream.write(chunk)
        if count != expected["bytes"] or digest.hexdigest() != expected["sha256"]:
            raise ValueError("downloaded source identity differs from its lock")
        temporary.replace(destination)
        return destination
    finally:
        if temporary is not None:
            temporary.unlink(missing_ok=True)


def archive_member(archive: Path, name: str, maximum: int) -> bytes:
    with zipfile.ZipFile(archive) as source:
        matches = [entry for entry in source.infolist() if entry.filename == name]
        if len(matches) != 1 or matches[0].is_dir():
            raise ValueError("font archive member is missing or ambiguous")
        if matches[0].file_size > maximum:
            raise ValueError("font archive member exceeds its byte bound")
        # Read a named member only; upstream license paths may contain ../.
        with source.open(matches[0]) as stream:
            payload = stream.read(maximum + 1)
        if len(payload) > maximum:
            raise ValueError("font archive member exceeds its byte bound")
        return payload


def github_blob_url(source: dict, path: str) -> str:
    return (
        "https://raw.githubusercontent.com/"
        f"{source['repository']}/{source['target_commit']}/"
        + urllib.parse.quote(path, safe="/")
    )


def prepare(output: Path, cache: Path) -> dict:
    if output.exists() or output.is_symlink():
        raise ValueError("prepared output must be fresh")
    output, cache = output.resolve(), cache.resolve()
    if output.is_relative_to(cache) or cache.is_relative_to(output):
        raise ValueError("prepared output and download cache overlap")
    lock = shared.load_lock()
    base = base_fonts.load_font_lock()
    document, payload = _load_json(shared.DEFAULT_LOCK, base_fonts.MAX_LOCK_BYTES)
    if runtime.sha256(payload) != lock.sha256:
        raise ValueError("shared font lock changed during preparation")
    runtime_lock = runtime.load_runtime_lock()
    wheels = [attestation.tool_lock()["wheel"], resources.tool_lock()["wheel"]]
    output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(dir=output.parent) as temporary:
        staged = Path(temporary) / "prepared"
        staged.mkdir()
        fonts = staged / "font-inputs"
        licenses = staged / "license-inputs"
        notices = staged / "licenses"
        for directory in (fonts, licenses, notices):
            directory.mkdir()
        for family in base["families"]:
            source = family["source"]
            url = (
                f"https://github.com/{source['repository']}/releases/download/"
                f"{source['release_tag']}/{source['asset']['name']}"
            )
            archive = download(cache, url, source["asset"])
            for entry in family["files"]:
                payload = archive_member(archive, entry["asset_member"], entry["bytes"])
                verify_payload(payload, entry)
                (fonts / entry["name"]).write_bytes(payload)
            with zipfile.ZipFile(archive) as source_archive:
                names = set(source_archive.namelist())
            candidates = [name for name in ("OFL.txt", "../OFL.txt") if name in names]
            if len(candidates) != 1:
                raise ValueError("font archive license is missing or ambiguous")
            payload = archive_member(archive, candidates[0], shared.MAX_LICENSE_BYTES)
            (notices / f"{source['release_tag']}-OFL.txt").write_bytes(payload)
        for entry in document["additions"]:
            source = entry["source"]
            font = download(
                cache, github_blob_url(source, source["font"]["path"]), entry
            )
            shutil.copyfile(font, fonts / entry["name"])
            license_entry = source["license"]
            license_file = download(
                cache, github_blob_url(source, license_entry["path"]), license_entry
            )
            shutil.copyfile(license_file, licenses / license_entry["name"])
            shutil.copyfile(license_file, notices / license_entry["name"])
        receipt = shared.prepare_pack(
            fonts, licenses, staged / "shared-font-pack", lock
        )
        for wheel in wheels:
            shutil.copyfile(
                download(cache, wheel["url"], wheel), staged / wheel["name"]
            )
        archive = runtime_lock["archive"]
        runtime.prepare_build(
            download(cache, archive["url"], archive), staged / "context"
        )
        # Source staging is disposable; retain only the verified pack and notices.
        shutil.rmtree(fonts)
        shutil.rmtree(licenses)
        staged.rename(output)
    return receipt


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--cache", type=Path, required=True)
    args = parser.parse_args()
    try:
        receipt = prepare(args.output, args.cache)
    except (OSError, ValueError, zipfile.BadZipFile) as error:
        print(f"prepare_render_oracle_ci: {error}", file=sys.stderr)
        return 1
    print(shared.canonical_json(receipt).decode())
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
