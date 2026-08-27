#!/usr/bin/env python3
"""Build and validate the exact immutable release-cache artifact graph."""

from __future__ import annotations

import argparse
from collections import Counter
from pathlib import Path
import re
import shutil
import struct


HEX128 = re.compile(r"^[0-9a-f]{32}$")
MANIFEST_MAX_BYTES = 64 * 1024 * 1024
CAS_MAX_BYTES = 256 * 1024 * 1024


def fail(message: str) -> None:
    raise SystemExit(f"prebuilt cache inventory: {message}")


def read_expected(path: Path) -> list[str]:
    units = [line.strip() for line in path.read_text(encoding="utf-8").splitlines() if line.strip()]
    if units != sorted(set(units)):
        fail("expected unit inventory must be sorted and unique")
    return units


def outcome_units(path: Path, suffix: str) -> dict[str, str]:
    outcomes: dict[str, str] = {}
    for line in path.read_text(encoding="utf-8").splitlines():
        prefix = "alignc: cache: "
        if not line.startswith(prefix):
            continue
        match = re.fullmatch(r"(.+?)( frontend)? (hit|miss)(?: .*)?", line[len(prefix) :])
        if match is None:
            continue
        unit, frontend, status = match.groups()
        if (suffix == "frontend") != (frontend is not None) or not unit or unit[0].isdigit():
            continue
        if unit in outcomes:
            fail(f"duplicate {suffix or 'codegen'} outcome for {unit}")
        outcomes[unit] = status
    return outcomes


def verify_outcomes(expected: list[str], path: Path, required: str | None) -> None:
    wanted = set(expected)
    for suffix in ["frontend", ""]:
        observed = outcome_units(path, suffix)
        if set(observed) != wanted:
            fail(
                f"{suffix or 'codegen'} outcomes differ: "
                f"missing={sorted(wanted - set(observed))}, extra={sorted(set(observed) - wanted)}"
            )
        if required is not None:
            wrong = sorted(unit for unit, status in observed.items() if status != required)
            if wrong:
                fail(f"{suffix or 'codegen'} outcomes are not all {required}: {wrong}")


def regular_files(directory: Path) -> list[Path]:
    if not directory.is_dir() or directory.is_symlink():
        fail(f"missing real directory {directory}")
    files: list[Path] = []
    for entry in sorted(directory.iterdir()):
        if entry.is_symlink() or not entry.is_file():
            fail(f"unexpected non-regular entry {entry}")
        if not HEX128.fullmatch(entry.name):
            fail(f"unexpected cache filename {entry}")
        files.append(entry)
    return files


def blob_digest_from_codegen_manifest(path: Path) -> str:
    data = path.read_bytes()
    if len(data) < 16:
        fail(f"truncated codegen manifest {path}")
    lo, hi = struct.unpack("<QQ", data[-16:])
    return f"{lo:016x}{hi:016x}"


def manifest_contents(paths: list[Path]) -> Counter[bytes]:
    return Counter(path.read_bytes() for path in paths)


def inventory(root: Path, expected_count: int) -> tuple[list[Path], list[Path], set[Path]]:
    if not root.is_dir() or root.is_symlink():
        fail(f"missing real cache root {root}")
    top = sorted(entry.name for entry in root.iterdir())
    if top != ["actions", "cas", "index"]:
        fail(f"root namespaces differ at {root}: {top}")
    for namespace in top:
        path = root / namespace
        if not path.is_dir() or path.is_symlink():
            fail(f"unexpected non-directory namespace {path}")
    for namespace in ["actions", "index"]:
        kinds = sorted(entry.name for entry in (root / namespace).iterdir())
        if kinds != ["codegen", "unit"]:
            fail(f"{namespace} namespaces differ: {kinds}")

    unit_actions = regular_files(root / "actions" / "unit")
    codegen_actions = regular_files(root / "actions" / "codegen")
    unit_indexes = regular_files(root / "index" / "unit")
    codegen_indexes = regular_files(root / "index" / "codegen")
    for label, files in [
        ("unit actions", unit_actions),
        ("codegen actions", codegen_actions),
        ("unit indexes", unit_indexes),
        ("codegen indexes", codegen_indexes),
    ]:
        if len(files) != expected_count:
            fail(f"{label}: expected {expected_count}, observed {len(files)}")
        oversized = [path for path in files if path.stat().st_size > MANIFEST_MAX_BYTES]
        if oversized:
            fail(f"{label} exceed the runtime manifest bound: {oversized}")

    if manifest_contents(unit_actions) != manifest_contents(unit_indexes):
        fail("unit action/index manifests differ")
    if manifest_contents(codegen_actions) != manifest_contents(codegen_indexes):
        fail("codegen action/index manifests differ")

    references = {blob_digest_from_codegen_manifest(path) for path in codegen_actions}
    blobs: set[Path] = set()
    cas = root / "cas"
    if not cas.is_dir() or cas.is_symlink():
        fail(f"missing real CAS directory {cas}")
    for shard in sorted(cas.iterdir()):
        if shard.is_symlink() or not shard.is_dir() or not re.fullmatch(r"[0-9a-f]{2}", shard.name):
            fail(f"unexpected CAS shard {shard}")
        for blob in regular_files(shard):
            if not blob.name.startswith(shard.name):
                fail(f"CAS blob is in the wrong shard: {blob}")
            if blob.stat().st_size > CAS_MAX_BYTES:
                fail(f"CAS blob exceeds the runtime object bound: {blob}")
            blobs.add(blob)
    observed = {path.name for path in blobs}
    if observed != references:
        fail(
            f"CAS graph differs: missing={sorted(references - observed)}, "
            f"unreferenced={sorted(observed - references)}"
        )
    return unit_actions, codegen_actions, blobs


def copy_bundle(source: Path, destination: Path, expected_count: int) -> None:
    unit_actions, codegen_actions, blobs = inventory(source, expected_count)
    if destination.exists():
        if not destination.is_dir() or destination.is_symlink() or any(destination.iterdir()):
            fail(f"destination is not an empty real directory: {destination}")
    for kind, files in [("unit", unit_actions), ("codegen", codegen_actions)]:
        for namespace in ["actions", "index"]:
            target = destination / namespace / kind
            target.mkdir(parents=True, exist_ok=True)
            sources = regular_files(source / namespace / kind)
            for path in sources:
                shutil.copy2(path, target / path.name, follow_symlinks=False)
    for blob in blobs:
        target = destination / "cas" / blob.parent.name
        target.mkdir(parents=True, exist_ok=True)
        shutil.copy2(blob, target / blob.name, follow_symlinks=False)
    inventory(destination, expected_count)


def main() -> None:
    parser = argparse.ArgumentParser()
    sub = parser.add_subparsers(dest="command", required=True)
    bundle = sub.add_parser("bundle")
    bundle.add_argument("--source", type=Path, required=True)
    bundle.add_argument("--destination", type=Path, required=True)
    bundle.add_argument("--expected", type=Path, required=True)
    bundle.add_argument("--outcomes", type=Path, required=True)
    verify = sub.add_parser("verify-outcomes")
    verify.add_argument("--expected", type=Path, required=True)
    verify.add_argument("--outcomes", type=Path, required=True)
    verify.add_argument("--require", choices=["hit", "miss"])
    tree = sub.add_parser("verify-tree")
    tree.add_argument("--root", type=Path, required=True)
    tree.add_argument("--expected", type=Path, required=True)
    args = parser.parse_args()

    expected = read_expected(args.expected)
    if args.command in {"bundle", "verify-outcomes"}:
        verify_outcomes(expected, args.outcomes, getattr(args, "require", None))
    if args.command == "bundle":
        copy_bundle(args.source, args.destination, len(expected))
    elif args.command == "verify-tree":
        inventory(args.root, len(expected))


if __name__ == "__main__":
    main()
