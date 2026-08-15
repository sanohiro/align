#!/usr/bin/env bash
# Deterministic owner for the git cat-file batch response boundary.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd -P)"
cd "$REPO_ROOT"
export PYTHONDONTWRITEBYTECODE=1

python3 - "$REPO_ROOT" <<'PY'
from scripts.benchmark_evidence import git_batch as gb
from scripts.benchmark_evidence import git_objects as go


OID = "ce013625030ba8dba906f756967f9e9ca394464a"
EMPTY_TREE_OID = "4b825dc642cb6eb9a060e54bf8d69288fbee4904"
OTHER_OID = EMPTY_TREE_OID


def expect_error(call, fragment):
    try:
        call()
    except gb.GitBatchError as exc:
        assert fragment in str(exc), str(exc)
    else:
        raise AssertionError("accepted invalid Git batch response")


def main():
    valid = OID.encode("ascii") + b" blob 6\nhello\n\n"
    result = gb.parse(OID, valid)
    assert result.oid == OID
    assert result.object == go.VerifiedObject(
        kind="blob",
        oid=OID,
        raw_sha256="2cf8d83d9ee29543b34a87727421fdecb7e3f3a183d337639025de576db9ebb4",
        payload=b"hello\n",
    )

    empty = EMPTY_TREE_OID.encode("ascii") + b" tree 0\n\n"
    empty_result = gb.parse(EMPTY_TREE_OID, empty)
    assert empty_result.object == go.VerifiedObject(
        kind="tree",
        oid=EMPTY_TREE_OID,
        raw_sha256="6ef19b41225c5369f1c104d45d8d85efa9b057b53b14b4b9b939dd74decc5321",
        payload=b"",
    )

    missing = OID.encode("ascii") + b" missing\n"
    assert gb.parse(OID, missing) == gb.BatchResult(oid=OID, object=None)

    expect_error(lambda: gb.parse("bad", valid), "40 lowercase")
    expect_error(lambda: gb.parse(OID, b""), "no LF")
    expect_error(lambda: gb.parse(OID, OTHER_OID.encode("ascii") + b" missing\n"), "wrong shape")
    expect_error(lambda: gb.parse(OID, OTHER_OID.encode("ascii") + b" blob 6\nhello\n\n"), "does not match")
    expect_error(lambda: gb.parse(OID, OID.encode("ascii") + b" blob 5\nhello"), "separator LF")
    expect_error(lambda: gb.parse(OID, OID.encode("ascii") + b" blob 7\nhello\n\n"), "payload length")
    expect_error(lambda: gb.parse(OID, OID.encode("ascii") + b" blob 00\n\n"), "canonical decimal")
    expect_error(lambda: gb.parse(OID, OID.encode("ascii") + b" blob " + b"9" * 5000 + b"\n\n"), "payload length")
    expect_error(lambda: gb.parse(OID, missing + b"extra"), "trailing bytes")
    expect_error(lambda: gb.parse(OID, OID.encode("ascii") + b" weird 0\n\n"), "supported Git kind")
    expect_error(lambda: gb.parse(OID, bytearray(valid)), "response must be bytes")

    print("benchmark evidence Git batch checks passed")


main()
PY
