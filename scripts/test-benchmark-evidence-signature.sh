#!/usr/bin/env bash
# Deterministic owner for the pinned SSHSIG v1 representation.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd -P)"
cd "$REPO_ROOT"
export PYTHONDONTWRITEBYTECODE=1

python3 - "$REPO_ROOT" <<'PY'
import base64
import hashlib
import struct
import sys

from scripts.benchmark_evidence import sshsig


def ssh(value):
    return struct.pack(">I", len(value)) + value


def binary_record(
    *,
    version=sshsig.VERSION,
    key_blob=None,
    namespace=sshsig.REPORT_NAMESPACE,
    reserved=b"",
    hash_algorithm=sshsig.HASH_ALGORITHM,
    signature_algorithm=sshsig.KEY_ALGORITHM,
    signature=bytes(range(64)),
    signature_suffix=b"",
    trailing=b"",
):
    if key_blob is None:
        key_blob = ssh(sshsig.KEY_ALGORITHM) + ssh(b"K" * 32)
    signature_blob = ssh(signature_algorithm) + ssh(signature) + signature_suffix
    return (
        sshsig.MAGIC
        + struct.pack(">I", version)
        + ssh(key_blob)
        + ssh(namespace)
        + ssh(reserved)
        + ssh(hash_algorithm)
        + ssh(signature_blob)
        + trailing
    )


def rejected(label, action):
    try:
        action()
    except sshsig.SSHSigError:
        return
    raise AssertionError(f"{label} was accepted")


key_blob = ssh(sshsig.KEY_ALGORITHM) + ssh(b"K" * 32)
record = sshsig.Signature(key_blob, sshsig.REPORT_NAMESPACE, bytes(range(64)))
binary = sshsig.encode_binary(record)
armor = sshsig.encode_armor(record)
assert sshsig.decode_binary(binary) == record
assert sshsig.decode_armor(armor) == record
assert sshsig.decode_armor(armor, expected_public_key_blob=key_blob, expected_namespace=sshsig.REPORT_NAMESPACE) == record
assert armor.startswith(b"-----BEGIN SSH SIGNATURE-----\n")
assert armor.endswith(b"-----END SSH SIGNATURE-----\n")
base64_lines = armor[len(b"-----BEGIN SSH SIGNATURE-----\n") : -len(b"-----END SSH SIGNATURE-----\n")].splitlines()
assert all(len(line) == 70 for line in base64_lines[:-1])
assert 1 <= len(base64_lines[-1]) <= 70
assert sshsig.sha256(armor) == "794581e2699e8bf3412301e9ea70173ae72b49bb9a5947e6a46f75b7fd1fe0ee"
assert hashlib.sha256(sshsig.signing_preimage(b"report\n", sshsig.REPORT_NAMESPACE)).hexdigest() == "4a3bed86ced51db799d037d860484b9857854c3a43ac7d33aeb20fa5f2111d23"

rejected("non-bytes binary", lambda: sshsig.decode_binary("text"))
rejected("bad magic", lambda: sshsig.decode_binary(b"BAD" + binary[3:]))
rejected("bad version", lambda: sshsig.decode_binary(binary_record(version=2)))
rejected("trailing binary", lambda: sshsig.decode_binary(binary + b"x"))
rejected("truncated binary", lambda: sshsig.decode_binary(binary[:-1]))
rejected("bad key algorithm", lambda: sshsig.decode_binary(binary_record(key_blob=ssh(b"ssh-rsa") + ssh(b"K" * 32))))
rejected("short Ed25519 key", lambda: sshsig.decode_binary(binary_record(key_blob=ssh(sshsig.KEY_ALGORITHM) + ssh(b"K" * 31))))
rejected("key trailing bytes", lambda: sshsig.decode_binary(binary_record(key_blob=key_blob + b"x")))
rejected("unknown namespace", lambda: sshsig.decode_binary(binary_record(namespace=b"unknown")))
rejected("nonempty reserved", lambda: sshsig.decode_binary(binary_record(reserved=b"reserved")))
rejected("wrong hash", lambda: sshsig.decode_binary(binary_record(hash_algorithm=b"sha256")))
rejected("bad nested algorithm", lambda: sshsig.decode_binary(binary_record(signature_algorithm=b"ssh-rsa")))
rejected("short signature", lambda: sshsig.decode_binary(binary_record(signature=b"S" * 63)))
rejected("nested trailing bytes", lambda: sshsig.decode_binary(binary_record(signature_suffix=b"x")))
rejected("wrong expected key", lambda: sshsig.decode_armor(armor, expected_public_key_blob=ssh(sshsig.KEY_ALGORITHM) + ssh(b"L" * 32)))
rejected("wrong expected namespace", lambda: sshsig.decode_armor(armor, expected_namespace=sshsig.MERGE_NAMESPACE))
rejected("bad header", lambda: sshsig.decode_armor(b"X" + armor[1:]))
rejected("bad footer", lambda: sshsig.decode_armor(armor[:-2] + b"x\n"))
rejected("missing armor LF", lambda: sshsig.decode_armor(armor[:-1]))
rejected("CR in armor", lambda: sshsig.decode_armor(armor.replace(b"\n", b"\r\n", 1)))
rejected("empty armor line", lambda: sshsig.decode_armor(armor.replace(b"\n", b"\n\n", 1)))
rejected("wrong line width", lambda: sshsig.decode_armor(armor.replace(b"\n", b"\nA", 1)))
rejected("invalid base64", lambda: sshsig.decode_armor(armor.replace(b"A", b"!", 1)))
rejected("noncanonical base64 padding", lambda: sshsig.decode_armor(armor.replace(b"=", b"", 1)))

merge = sshsig.Signature(key_blob, sshsig.MERGE_NAMESPACE, b"M" * 64)
assert sshsig.decode_armor(sshsig.encode_armor(merge)) == merge
print("SSHSIG evidence checks passed")
PY
