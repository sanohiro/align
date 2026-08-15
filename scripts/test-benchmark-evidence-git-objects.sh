#!/usr/bin/env bash
# Deterministic owner for raw Git object identity framing.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd -P)"
cd "$REPO_ROOT"
export PYTHONDONTWRITEBYTECODE=1

python3 - "$REPO_ROOT" <<'PY'
from scripts.benchmark_evidence import git_objects as go


def expect_error(call, fragment):
    try:
        call()
    except go.GitObjectError as exc:
        assert fragment in str(exc), str(exc)
    else:
        raise AssertionError("accepted invalid raw Git object")


def main():
    raw = go.encode("blob", b"hello\n")
    assert raw == b"blob 6\x00hello\n"
    assert go.parse(raw) == go.ParsedObject("blob", b"hello\n")
    verified = go.verify(
        "ce013625030ba8dba906f756967f9e9ca394464a",
        raw,
    )
    assert verified == go.VerifiedObject(
        kind="blob",
        oid="ce013625030ba8dba906f756967f9e9ca394464a",
        raw_sha256="2cf8d83d9ee29543b34a87727421fdecb7e3f3a183d337639025de576db9ebb4",
        payload=b"hello\n",
    )

    commit_payload = (
        b"tree "
        + b"a" * 40
        + b"\nparent "
        + b"b" * 40
        + b"\nauthor Example <example@example.com> 0 +0000\n"
        + b"committer Example <example@example.com> 0 +0000\n\nmessage\n"
    )
    commit_raw = go.encode("commit", commit_payload)
    assert go.parse(commit_raw).kind == "commit"
    assert go.parse(commit_raw).payload == commit_payload

    expect_error(lambda: go.encode("unknown", b""), "supported Git kind")
    expect_error(lambda: go.encode("blob", bytearray()), "payload must be bytes")
    expect_error(lambda: go.parse(b"blob 0"), "NUL terminator")
    expect_error(lambda: go.parse(b"blob 0\x00x"), "payload length")
    expect_error(lambda: go.parse(b"blob 00\x00"), "canonical decimal")
    expect_error(lambda: go.parse(b"blob +0\x00"), "canonical decimal")
    expect_error(lambda: go.parse(b"blob 0 0\x00"), "wrong shape")
    expect_error(lambda: go.parse(b"bl\xffb 0\x00"), "ASCII")
    expect_error(lambda: go.parse(b"blob 0\x00trailing"), "payload length")
    expect_error(lambda: go.parse(b"blob " + b"9" * 5000 + b"\x00"), "payload length")
    expect_error(lambda: go.verify("CE013625030BA8DBA906F756967F9E9CA394464A", raw), "40 lowercase")
    expect_error(lambda: go.verify("0" * 39, raw), "40 lowercase")
    expect_error(lambda: go.verify("0" * 40, raw), "does not match")
    expect_error(
        lambda: go.verify("ce013625030ba8dba906f756967f9e9ca394464a", b"blob 7\x00hello\n"),
        "does not match",
    )

    print("benchmark evidence raw Git object checks passed")


main()
PY
