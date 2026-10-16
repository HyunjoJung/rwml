#!/usr/bin/env python3
"""Check public-API compatibility against a released tag with its locked dependencies.

`cargo semver-checks --baseline-rev` (and `--baseline-version`) builds the
baseline inside a fresh placeholder project and re-resolves its dependencies,
so a release whose dependencies were later yanked can no longer serve as a
baseline. This script exports the tagged tree together with its committed
`Cargo.lock`, generates rustdoc JSON for the baseline and the current checkout
with cargo-semver-checks' own rustdoc flags and `--locked` resolution, and
compares the two documents for each checked feature set. Rustdoc JSON carries no
Cargo features, so the script also checks the manifests directly for removed
features and features that stopped being enabled by default.
"""

from __future__ import annotations

import argparse
import io
import json
import os
import pathlib
import re
import shutil
import subprocess
import sys
import tarfile
import tomllib


ROOT = pathlib.Path(__file__).resolve().parents[1]
CRATE = "rwml"
# The rustdoc flags cargo-semver-checks 0.48.0 passes for its own builds.
RUSTDOC_FLAGS = (
    "-Z unstable-options --document-private-items --document-hidden-items "
    "--output-format=json --cap-lints=allow"
)
FEATURE_SETS = {
    "default-features": [],
    "all-features": ["--all-features"],
}


def public_features(manifest: pathlib.Path) -> dict[str, list[str]]:
    """Cargo features of a manifest, including implicit optional-dependency features."""
    data = tomllib.loads(manifest.read_text(encoding="utf-8"))
    features = {name: list(members) for name, members in data.get("features", {}).items()}
    dep_refs = {
        member[len("dep:") :]
        for members in features.values()
        for member in members
        if member.startswith("dep:")
    }
    tables = [data.get("dependencies", {})] + [
        target.get("dependencies", {}) for target in data.get("target", {}).values()
    ]
    for table in tables:
        for name, spec in table.items():
            if isinstance(spec, dict) and spec.get("optional") and name not in dep_refs:
                features.setdefault(name, [f"dep:{name}"])
    return features


def feature_problems(baseline: pathlib.Path, current: pathlib.Path) -> list[str]:
    """Breaking Cargo feature changes that cargo-semver-checks reports from manifests."""
    old = public_features(baseline)
    new = public_features(current)
    problems = [f"feature `{name}` was removed" for name in sorted(old) if name not in new]
    new_defaults = set(new.get("default", []))
    problems += [
        f"feature `{member}` is no longer enabled by default"
        for member in old.get("default", [])
        if member not in new_defaults
    ]
    return problems


def run(command: list[str], *, env: dict[str, str] | None = None) -> None:
    print("+ " + " ".join(command), flush=True)
    subprocess.run(command, check=True, env=env)


def cargo(toolchain: str | None) -> list[str]:
    return ["cargo", f"+{toolchain}"] if toolchain else ["cargo"]


def export_baseline(rev: str, dest: pathlib.Path) -> pathlib.Path:
    """Extract the committed tree of `rev`, including its Cargo.lock."""
    if dest.exists():
        shutil.rmtree(dest)
    dest.mkdir(parents=True)
    archive = subprocess.run(
        ["git", "-C", str(ROOT), "archive", "--format=tar", rev],
        check=True,
        capture_output=True,
    ).stdout
    with tarfile.open(fileobj=io.BytesIO(archive)) as tar:
        tar.extractall(dest, filter="data")
    if not (dest / "Cargo.lock").is_file():
        raise SystemExit(f"baseline {rev} has no committed Cargo.lock")
    return dest


def rustdoc_json(
    manifest_dir: pathlib.Path,
    features: list[str],
    target_dir: pathlib.Path,
    output: pathlib.Path,
    toolchain: str | None,
) -> pathlib.Path:
    """Generate one rustdoc JSON document with locked dependencies."""
    generated = target_dir / "doc" / f"{CRATE}.json"
    generated.unlink(missing_ok=True)
    env = dict(os.environ, RUSTC_BOOTSTRAP="1", RUSTDOCFLAGS=RUSTDOC_FLAGS)
    run(
        cargo(toolchain)
        + [
            "doc",
            "--manifest-path",
            str(manifest_dir / "Cargo.toml"),
            "--locked",
            "--no-deps",
            "--lib",
            "--target-dir",
            str(target_dir),
            *features,
        ],
        env=env,
    )
    if not generated.is_file():
        raise SystemExit(f"rustdoc did not produce {generated}")
    json.loads(generated.read_text(encoding="utf-8"))
    output.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(generated, output)
    return output


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--baseline-rev", required=True, help="released tag, e.g. v0.1.4")
    parser.add_argument("--release-type", default="patch", choices=["patch", "minor", "major"])
    parser.add_argument(
        "--output-dir",
        type=pathlib.Path,
        default=ROOT / "target" / "semver-locked",
        help="scratch directory for the exported baseline, target dirs, and JSON",
    )
    parser.add_argument("--toolchain", help="rustup toolchain for rustdoc, e.g. 1.92.0")
    args = parser.parse_args(argv)

    out = args.output_dir.resolve()
    safe_rev = re.sub(r"[^A-Za-z0-9._-]", "_", args.baseline_rev)
    baseline = export_baseline(args.baseline_rev, out / f"baseline-{safe_rev}")

    problems = feature_problems(baseline / "Cargo.toml", ROOT / "Cargo.toml")
    if problems:
        listed = "".join(f"\n- {problem}" for problem in problems)
        raise SystemExit(f"Cargo features changed incompatibly since {args.baseline_rev}:{listed}")
    print(f"== Cargo features: compatible with {args.baseline_rev}", flush=True)

    for name, features in FEATURE_SETS.items():
        print(f"== {name}: {args.baseline_rev} -> current", flush=True)
        baseline_json = rustdoc_json(
            baseline,
            features,
            out / "target" / f"baseline-{name}",
            out / "rustdoc" / f"baseline-{name}.json",
            args.toolchain,
        )
        current_json = rustdoc_json(
            ROOT,
            features,
            out / "target" / f"current-{name}",
            out / "rustdoc" / f"current-{name}.json",
            args.toolchain,
        )
        run(
            cargo(args.toolchain)
            + [
                "semver-checks",
                "check-release",
                "--baseline-rustdoc",
                str(baseline_json),
                "--current-rustdoc",
                str(current_json),
                "--release-type",
                args.release_type,
            ]
        )
    return 0


if __name__ == "__main__":
    sys.exit(main())
