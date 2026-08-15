#!/usr/bin/env bash
# Deterministic owner for strict canonical evidence JSON primitives.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd -P)"
export PYTHONDONTWRITEBYTECODE=1

python3 - "$REPO_ROOT" <<'PY'
import sys

from scripts.benchmark_evidence import canonical_json as cj

root = sys.argv[1]
value = cj.Object((
    ("schema", "test/v1"),
    ("count", 42),
    ("items", ["A", "B"]),
))
expected = b'{"schema":"test/v1","count":42,"items":["A","B"]}\n'
assert cj.encode(value) == expected
assert cj.decode(expected) == value
assert cj.sha256(expected) == "10f1ae68885a38a46009f0a8b3c7f98ce52dcba12c7d59f1b29d97de50b8fb72"
cj.require_object(value, ("schema", "count", "items"), "value")
cj.require_string(value["schema"], "schema")
assert cj.require_uint(value["count"], "count") == 42
assert cj.require_uint(cj.MAX_U64, "max") == cj.MAX_U64
assert cj.require_uint(2**32 - 1, "u32", maximum=(2**32 - 1)) == 2**32 - 1
assert cj.decode(b'{"value":"a/b"}\n')["value"] == "a/b"
recursive = cj.Object((("outer", cj.Object((("inner", [0, 1, 2]),))),))
assert cj.decode(cj.encode(recursive)) == recursive


def rejected(label, action):
    try:
        action()
    except cj.CanonicalJsonError:
        return
    raise AssertionError(f"{label} was accepted")


rejected("whitespace", lambda: cj.decode(b'{ "schema":"test/v1","count":42,"items":[] }\n'))
rejected("trailing bytes", lambda: cj.decode(expected + b"x"))
rejected("missing final LF", lambda: cj.decode(expected[:-1]))
rejected("duplicate key", lambda: cj.decode(b'{"schema":"test/v1","schema":"test/v1"}\n'))
rejected("escaped canonical string", lambda: cj.decode(b'{"schema":"test\\u002Fv1","count":42,"items":["A","B"]}\n'))
rejected("escaped quote", lambda: cj.decode(b'{"value":"\\\""}\n'))
rejected("escaped backslash", lambda: cj.decode(b'{"value":"\\\\"}\n'))
rejected("invalid UTF-8", lambda: cj.decode(b'{"value":"\xff"}\n'))
rejected("unicode string", lambda: cj.encode(cj.Object((("value", "é"),))))
rejected("control string", lambda: cj.encode(cj.Object((("value", "line\nbreak"),))))
rejected("quote string", lambda: cj.encode(cj.Object((("value", '"'),))))
rejected("backslash string", lambda: cj.encode(cj.Object((("value", "\\"),))))
stale = cj.Object((("value", 1),))
stale["extra"] = 2
rejected("stale object mapping", lambda: cj.encode(stale))
stale_member = cj.Object((("value", 1),))
stale_member["value"] = 2
rejected("replaced object member", lambda: cj.require_object(stale_member, ("value",), "value"))
rejected("float", lambda: cj.decode(b'{"value":1.0}\n'))
rejected("oversized integer", lambda: cj.decode(b'{"value":' + b"9" * 5000 + b"}\n"))
rejected("deep JSON", lambda: cj.decode(b"[" * 2000 + b"0" + b"]" * 2000 + b"\n"))
rejected("negative", lambda: cj.decode(b'{"value":-1}\n'))
rejected("overflow", lambda: cj.decode(b'{"value":18446744073709551616}\n'))
rejected("null", lambda: cj.decode(b'{"value":null}\n'))
assert cj.decode(b'{"value":true}\n')["value"] is True
rejected("boolean", lambda: cj.require_uint(True, "value"))
rejected("u32 overflow", lambda: cj.require_uint(2**32, "u32", maximum=(2**32 - 1)))
rejected(
    "wrong member order",
    lambda: cj.require_object(
        cj.decode(b'{"count":42,"schema":"test/v1","items":[]}\n'),
        ("schema", "count", "items"),
        "value",
    ),
)
rejected(
    "wrong object shape",
    lambda: cj.require_object(cj.decode(b'{"schema":"test/v1"}\n'), ("schema", "count"), "value"),
)

try:
    cj.encode({"value": object()})
except cj.CanonicalJsonError:
    pass
else:
    raise AssertionError("unsupported scalar was accepted")

print("canonical evidence JSON checks passed")
PY
