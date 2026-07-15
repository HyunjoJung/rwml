#!/usr/bin/env python3
"""Idempotently publish a crate while verifying immutable registry identity."""

from __future__ import annotations

import argparse
import hashlib
import json
import pathlib
import subprocess
import sys
import time
from typing import Optional
from urllib import error, parse, request


API_ROOT = "https://crates.io/api/v1/crates"
USER_AGENT = "rwml-release-ci (github actions)"


class PublishError(RuntimeError):
    pass


def sha256_file(path: pathlib.Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as artifact:
        for chunk in iter(lambda: artifact.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def registry_checksum(name: str, version: str) -> Optional[str]:
    url = f"{API_ROOT}/{parse.quote(name, safe='')}/{parse.quote(version, safe='')}"
    registry_request = request.Request(url, headers={"User-Agent": USER_AGENT})
    try:
        with request.urlopen(registry_request, timeout=15) as response:
            payload = json.loads(response.read())
    except error.HTTPError as exc:
        exc.close()
        if exc.code == 404:
            return None
        raise PublishError(f"crates.io returned HTTP {exc.code} for {name} {version}") from exc
    except (error.URLError, TimeoutError, json.JSONDecodeError) as exc:
        raise PublishError(f"failed to query crates.io for {name} {version}: {exc}") from exc

    checksum = payload.get("version", {}).get("checksum")
    if not isinstance(checksum, str) or len(checksum) != 64:
        raise PublishError(f"crates.io returned no valid checksum for {name} {version}")
    try:
        bytes.fromhex(checksum)
    except ValueError as exc:
        raise PublishError(f"crates.io returned an invalid checksum for {name} {version}") from exc
    return checksum.lower()


def require_matching_checksum(name: str, version: str, local: str, remote: str) -> None:
    if local != remote:
        raise PublishError(
            f"published {name} {version} checksum {remote} does not match local artifact {local}"
        )


def wait_for_matching_version(
    name: str,
    version: str,
    local_checksum: str,
    attempts: int,
    interval: float,
) -> None:
    last_error: Optional[PublishError] = None
    for attempt in range(attempts):
        try:
            checksum = registry_checksum(name, version)
            last_error = None
        except PublishError as exc:
            checksum = None
            last_error = exc
        if checksum is not None:
            require_matching_checksum(name, version, local_checksum, checksum)
            return
        if attempt + 1 < attempts:
            time.sleep(interval)

    if last_error is not None:
        raise PublishError(
            f"{name} {version} did not become verifiable after publication: {last_error}"
        ) from last_error
    raise PublishError(f"{name} {version} did not become visible after publication")


def ensure_published(
    name: str,
    version: str,
    artifact: pathlib.Path,
    manifest_path: Optional[pathlib.Path],
    *,
    poll_attempts: int = 12,
    poll_interval: float = 10,
    check_only: bool = False,
) -> str:
    if poll_attempts < 1 or poll_interval < 0:
        raise PublishError("poll attempts must be positive and interval must be nonnegative")
    if not artifact.is_file():
        raise PublishError(f"crate artifact does not exist: {artifact}")

    local_checksum = sha256_file(artifact)
    published = registry_checksum(name, version)
    if published is not None:
        require_matching_checksum(name, version, local_checksum, published)
        print(f"{name} {version} already published with matching checksum")
        return "already-published"
    if check_only:
        print(f"{name} {version} is not published; local artifact is ready")
        return "not-published"

    command = ["cargo", "publish"]
    if manifest_path is not None:
        command.extend(["--manifest-path", str(manifest_path)])
    result = subprocess.run(command, check=False)

    try:
        wait_for_matching_version(
            name,
            version,
            local_checksum,
            attempts=poll_attempts,
            interval=poll_interval,
        )
    except PublishError as exc:
        if result.returncode != 0:
            raise PublishError(
                f"cargo publish exited with {result.returncode} and registry recovery failed: {exc}"
            ) from exc
        raise

    if result.returncode == 0:
        print(f"published {name} {version} with verified checksum")
        return "published"
    print(f"recovered {name} {version} after cargo publish exited with {result.returncode}")
    return "recovered"


def parse_args(argv: Optional[list[str]] = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--name", required=True)
    parser.add_argument("--version", required=True)
    parser.add_argument("--artifact", required=True, type=pathlib.Path)
    parser.add_argument("--manifest-path", type=pathlib.Path)
    parser.add_argument("--poll-attempts", type=int, default=12)
    parser.add_argument("--poll-interval", type=float, default=10)
    parser.add_argument("--check-only", action="store_true")
    return parser.parse_args(argv)


def main(argv: Optional[list[str]] = None) -> int:
    args = parse_args(argv)
    try:
        ensure_published(
            args.name,
            args.version,
            args.artifact,
            args.manifest_path,
            poll_attempts=args.poll_attempts,
            poll_interval=args.poll_interval,
            check_only=args.check_only,
        )
    except PublishError as exc:
        print(f"publish_crate: {exc}", file=sys.stderr)
        return 2
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
