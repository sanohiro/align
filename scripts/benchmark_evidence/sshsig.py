"""Strict SSHSIG v1 framing for signed evidence artifacts.

This module only handles the pinned OpenSSH SSHSIG bytes and signing preimage.
Key management and ``ssh-keygen`` invocation belong to the native signing
adapter; report semantics and trust-policy decisions remain verifier
responsibilities.
"""

from __future__ import annotations

import base64
import hashlib
import struct
from dataclasses import dataclass


class SSHSigError(ValueError):
    """A signature does not satisfy the pinned SSHSIG representation."""


MAGIC = b"SSHSIG"
VERSION = 1
HASH_ALGORITHM = b"sha512"
KEY_ALGORITHM = b"ssh-ed25519"
REPORT_NAMESPACE = b"align-json-escape-benchmark-evidence-v1"
MERGE_NAMESPACE = b"align-json-escape-benchmark-merge-verification-v1"
_NAMESPACES = (REPORT_NAMESPACE, MERGE_NAMESPACE)
_BEGIN = b"-----BEGIN SSH SIGNATURE-----\n"
_END = b"-----END SSH SIGNATURE-----\n"
_LINE_WIDTH = 70
_ED25519_KEY_BYTES = 32
_ED25519_SIGNATURE_BYTES = 64


@dataclass(frozen=True)
class Signature:
    """Decoded SSHSIG fields after strict binary and armor validation."""

    public_key_blob: bytes
    namespace: bytes
    signature: bytes


def _error(message: str) -> None:
    raise SSHSigError(message)


def _require_bytes(value: object, label: str) -> bytes:
    if not isinstance(value, bytes):
        _error(f"{label} must be bytes")
    return value


def _ssh_string(value: bytes, label: str) -> bytes:
    if len(value) > 0xFFFFFFFF:
        _error(f"{label} is too long")
    return struct.pack(">I", len(value)) + value


def _read_ssh_string(raw: bytes, offset: int, label: str) -> tuple[bytes, int]:
    if offset < 0 or offset > len(raw) - 4:
        _error(f"{label} length is truncated")
    length = struct.unpack_from(">I", raw, offset)[0]
    start = offset + 4
    end = start + length
    if end > len(raw):
        _error(f"{label} is truncated")
    return raw[start:end], end


def _validate_key_blob(blob: object, label: str = "public_key_blob") -> bytes:
    blob = _require_bytes(blob, label)
    algorithm, offset = _read_ssh_string(blob, 0, f"{label}.algorithm")
    key, offset = _read_ssh_string(blob, offset, f"{label}.key")
    if offset != len(blob):
        _error(f"{label} has trailing bytes")
    if algorithm != KEY_ALGORITHM:
        _error(f"{label} has the wrong key algorithm")
    if len(key) != _ED25519_KEY_BYTES:
        _error(f"{label} has the wrong Ed25519 key length")
    return blob


def _validate_namespace(namespace: object) -> bytes:
    namespace = _require_bytes(namespace, "namespace")
    if namespace not in _NAMESPACES:
        _error("namespace is not a declared evidence namespace")
    return namespace


def _validate_signature_bytes(signature: object) -> bytes:
    signature = _require_bytes(signature, "signature")
    if len(signature) != _ED25519_SIGNATURE_BYTES:
        _error("signature has the wrong Ed25519 signature length")
    return signature


def _signature_blob(signature: bytes) -> bytes:
    return _ssh_string(KEY_ALGORITHM, "signature.algorithm") + _ssh_string(signature, "signature.bytes")


def encode_binary(signature: Signature) -> bytes:
    """Encode a validated signature into the exact SSHSIG binary record."""

    public_key_blob = _validate_key_blob(signature.public_key_blob)
    namespace = _validate_namespace(signature.namespace)
    signature_bytes = _validate_signature_bytes(signature.signature)
    return (
        MAGIC
        + struct.pack(">I", VERSION)
        + _ssh_string(public_key_blob, "public_key_blob")
        + _ssh_string(namespace, "namespace")
        + _ssh_string(b"", "reserved")
        + _ssh_string(HASH_ALGORITHM, "hash_algorithm")
        + _ssh_string(_signature_blob(signature_bytes), "signature_blob")
    )


def decode_binary(raw: bytes) -> Signature:
    """Decode one exact SSHSIG binary record and reject trailing bytes."""

    raw = _require_bytes(raw, "binary signature")
    if len(raw) < len(MAGIC) + 4 or raw[: len(MAGIC)] != MAGIC:
        _error("binary signature has the wrong magic")
    version = struct.unpack_from(">I", raw, len(MAGIC))[0]
    if version != VERSION:
        _error("binary signature has the wrong version")
    offset = len(MAGIC) + 4
    public_key_blob, offset = _read_ssh_string(raw, offset, "public_key_blob")
    namespace, offset = _read_ssh_string(raw, offset, "namespace")
    reserved, offset = _read_ssh_string(raw, offset, "reserved")
    hash_algorithm, offset = _read_ssh_string(raw, offset, "hash_algorithm")
    signature_blob, offset = _read_ssh_string(raw, offset, "signature_blob")
    if offset != len(raw):
        _error("binary signature has trailing bytes")
    _validate_key_blob(public_key_blob)
    _validate_namespace(namespace)
    if reserved != b"":
        _error("reserved field must be empty")
    if hash_algorithm != HASH_ALGORITHM:
        _error("binary signature has the wrong hash algorithm")
    algorithm, signature_bytes_offset = _read_ssh_string(signature_blob, 0, "signature.algorithm")
    signature_bytes, signature_bytes_offset = _read_ssh_string(
        signature_blob,
        signature_bytes_offset,
        "signature.bytes",
    )
    if signature_bytes_offset != len(signature_blob):
        _error("signature blob has trailing bytes")
    if algorithm != KEY_ALGORITHM:
        _error("signature has the wrong key algorithm")
    _validate_signature_bytes(signature_bytes)
    result = Signature(public_key_blob, namespace, signature_bytes)
    if encode_binary(result) != raw:
        _error("binary signature is not canonical")
    return result


def encode_armor(signature: Signature) -> bytes:
    """Encode one SSHSIG binary record using the pinned armor layout."""

    encoded = base64.b64encode(encode_binary(signature))
    lines = [encoded[start : start + _LINE_WIDTH] for start in range(0, len(encoded), _LINE_WIDTH)]
    return _BEGIN + b"\n".join(lines) + b"\n" + _END


def decode_armor(
    raw: bytes,
    *,
    expected_public_key_blob: bytes | None = None,
    expected_namespace: bytes | None = None,
) -> Signature:
    """Decode exact SSHSIG armor, optionally binding key and namespace."""

    raw = _require_bytes(raw, "armored signature")
    if not raw.startswith(_BEGIN) or not raw.endswith(_END):
        _error("armored signature has the wrong header or footer")
    content = raw[len(_BEGIN) : -len(_END)]
    if not content.endswith(b"\n"):
        _error("armored signature is missing the final base64 line LF")
    encoded = content[:-1]
    lines = encoded.split(b"\n")
    if not lines or any(not line for line in lines):
        _error("armored signature has an empty base64 line")
    if any(len(line) != _LINE_WIDTH for line in lines[:-1]):
        _error("armored signature has a non-final line with the wrong width")
    if len(lines[-1]) == 0 or len(lines[-1]) > _LINE_WIDTH:
        _error("armored signature final line has the wrong width")
    joined = b"".join(lines)
    try:
        binary = base64.b64decode(joined, validate=True)
    except (ValueError, base64.binascii.Error) as exc:
        raise SSHSigError(f"invalid SSHSIG base64: {exc}") from exc
    if base64.b64encode(binary) != joined:
        _error("armored signature base64 is not canonical")
    signature = decode_binary(binary)
    if encode_armor(signature) != raw:
        _error("armored signature is not canonical")
    if expected_public_key_blob is not None and signature.public_key_blob != expected_public_key_blob:
        _error("armored signature has the wrong public key")
    if expected_namespace is not None and signature.namespace != expected_namespace:
        _error("armored signature has the wrong namespace")
    return signature


def signing_preimage(message: bytes, namespace: bytes) -> bytes:
    """Return the exact SSHSIG Ed25519 preimage for one complete message."""

    message = _require_bytes(message, "message")
    namespace = _validate_namespace(namespace)
    return (
        MAGIC
        + _ssh_string(namespace, "namespace")
        + _ssh_string(b"", "reserved")
        + _ssh_string(HASH_ALGORITHM, "hash_algorithm")
        + _ssh_string(hashlib.sha512(message).digest(), "message_sha512")
    )


def sha256(raw: bytes) -> str:
    """Return the lowercase SHA-256 of complete canonical armor bytes."""

    return hashlib.sha256(_require_bytes(raw, "signature bytes")).hexdigest()
