"""Run the trusted host-side SSHSIG signing and verification processes.

The report and :mod:`sshsig` modules own bytes and framing.  This adapter owns
the narrow privileged boundary around the administrator-provisioned private
key and the pinned host ``ssh-keygen`` executable.  It never enters a
measurement container, receives candidate paths, or returns private-key
bytes.
"""

from __future__ import annotations

import base64
import binascii
import hashlib
import os
import stat
import tempfile
from dataclasses import dataclass
from typing import Any, Callable, Mapping, Sequence

from . import cleanup
from . import native_host
from . import sshsig


class KeyProcessError(RuntimeError):
    """A signing or verification process cannot cross the trusted boundary."""


SSH_KEYGEN = "/usr/bin/ssh-keygen"
SIGNING_KEY_PATH = "/etc/align-evidence/signing-key"
SIGNING_WORK_ROOT = "/run/align-evidence/signing"
SIGNER_IDENTITY = "align-evidence"
MAX_MESSAGE_BYTES = 64 << 20
MAX_PROCESS_OUTPUT_BYTES = 64 << 10

_SIGNING_KEYS = (
    "key_type",
    "public_key_base64",
    "fingerprint",
    "ssh_keygen_version",
    "ssh_keygen_executable_sha256",
)
_HEX64 = frozenset("0123456789abcdef")
_FINGERPRINT_PREFIX = "SHA256:"
_KEY_ALGORITHM = sshsig.KEY_ALGORITHM
_PRIVATE_FILES = ("message", "message.sig", "allowed-signers")

Runner = Callable[..., tuple[tuple[bytes, ...], str]]


def _error(message: str) -> None:
    raise KeyProcessError(message)


def _hash(value: object, label: str) -> str:
    if not isinstance(value, str) or len(value) != 64 or any(char not in _HEX64 for char in value):
        _error(f"{label} is not lowercase SHA-256")
    return value


def _name(value: object, label: str) -> str:
    if (
        not isinstance(value, str)
        or not value
        or len(value) > 255
        or any(
            not (char.isascii() and (char.isalnum() or char in "._/:+=@-"))
            for char in value
        )
    ):
        _error(f"{label} has invalid name grammar")
    return value


def _object(value: object, keys: Sequence[str], label: str) -> Mapping[str, Any]:
    if not isinstance(value, Mapping) or tuple(value) != tuple(keys):
        _error(f"{label} has the wrong member order")
    return value


def _public_key_blob(raw_key: bytes) -> bytes:
    if not isinstance(raw_key, bytes) or len(raw_key) != 32:
        _error("profile signing key must contain exactly 32 raw Ed25519 bytes")
    return len(_KEY_ALGORITHM).to_bytes(4, "big") + _KEY_ALGORITHM + len(raw_key).to_bytes(4, "big") + raw_key


def _fingerprint(public_key_blob: bytes) -> str:
    encoded = base64.b64encode(hashlib.sha256(public_key_blob).digest()).rstrip(b"=")
    return _FINGERPRINT_PREFIX + encoded.decode("ascii")


@dataclass(frozen=True)
class KeyProcessConfig:
    """Profile-owned public identity for the host key-process adapter."""

    ssh_keygen_sha256: str
    public_key_blob: bytes

    def __post_init__(self) -> None:
        _hash(self.ssh_keygen_sha256, "ssh_keygen_sha256")
        try:
            sshsig.encode_binary(
                sshsig.Signature(
                    self.public_key_blob,
                    sshsig.REPORT_NAMESPACE,
                    b"\0" * 64,
                )
            )
        except sshsig.SSHSigError as exc:
            raise KeyProcessError(f"public key blob is invalid: {exc}") from exc


def from_profile(profile: Mapping[str, Any]) -> KeyProcessConfig:
    """Construct the host signer identity from one validated profile object."""

    if not isinstance(profile, Mapping):
        _error("profile must be an object")
    signing = _object(profile.get("signing"), _SIGNING_KEYS, "profile.signing")
    if signing["key_type"] != "ssh-ed25519":
        _error("profile.signing.key_type must be ssh-ed25519")
    public_key_base64 = signing["public_key_base64"]
    if not isinstance(public_key_base64, str):
        _error("profile.signing.public_key_base64 must be text")
    try:
        raw_key = base64.b64decode(public_key_base64.encode("ascii"), validate=True)
    except (UnicodeEncodeError, binascii.Error, ValueError) as exc:
        raise KeyProcessError("profile signing public key is not canonical base64") from exc
    public_key_blob = _public_key_blob(raw_key)
    fingerprint = signing["fingerprint"]
    if not isinstance(fingerprint, str) or fingerprint != _fingerprint(public_key_blob):
        _error("profile signing fingerprint does not match the public key")
    _name(signing["ssh_keygen_version"], "profile.signing.ssh_keygen_version")
    return KeyProcessConfig(
        ssh_keygen_sha256=_hash(
            signing["ssh_keygen_executable_sha256"],
            "profile.signing.ssh_keygen_executable_sha256",
        ),
        public_key_blob=public_key_blob,
    )


def _namespace(namespace: object) -> bytes:
    if namespace not in (sshsig.REPORT_NAMESPACE, sshsig.MERGE_NAMESPACE):
        _error("namespace is not a declared evidence namespace")
    return namespace


def _message(message: object) -> bytes:
    if not isinstance(message, bytes) or len(message) > MAX_MESSAGE_BYTES:
        _error("message exceeds the fixed signing limit")
    return message


def _require_signing_cleanup(snapshot: cleanup.CleanupSnapshot) -> None:
    if not isinstance(snapshot, cleanup.CleanupSnapshot):
        _error("signing requires a cleanup snapshot")
    if any(
        value != 0
        for value in (
            snapshot.children_remaining,
            snapshot.containers_remaining,
            snapshot.mounts_remaining,
            snapshot.fds_remaining,
            snapshot.private_dirs_remaining,
        )
    ):
        _error("signing requires all measurement resources to be gone")
    if not snapshot.host_lock_held_for_signing:
        _error("signing requires the host lock")
    if not snapshot.source_manifests_unchanged or not snapshot.cache_manifests_unchanged:
        _error("signing requires unchanged source and cache manifests")


def _validate_private_key_metadata(metadata: os.stat_result) -> None:
    mode = stat.S_IMODE(metadata.st_mode)
    if not stat.S_ISREG(metadata.st_mode):
        _error("signing key is not a regular file")
    if metadata.st_uid != 0:
        _error("signing key is not root-owned")
    if mode & 0o077:
        _error("signing key is readable or writable by group/other")
    if not mode & 0o400 or mode & 0o111:
        _error("signing key permissions are not a private readable file")


def _validate_directory_metadata(metadata: os.stat_result, label: str) -> None:
    if not stat.S_ISDIR(metadata.st_mode):
        _error(f"{label} is not a directory")
    if metadata.st_uid != 0 or stat.S_IMODE(metadata.st_mode) & 0o022:
        _error(f"{label} is not root-owned and benchmark-account unwritable")


def _open_private_key() -> int:
    """Open the fixed key through no-follow descriptors and retain no path trust."""

    components = SIGNING_KEY_PATH.split("/")
    if len(components) < 3 or components[0] != "" or any(not component for component in components[1:]):
        _error("signing key path is not canonical")
    directory_flags = native_host._fixed_open_flags(directory=True)
    file_flags = native_host._fixed_open_flags()
    directories: list[int] = []
    key_fd: int | None = None
    ownership_transferred = False
    cleanup_errors: list[OSError] = []
    try:
        current = os.open("/", directory_flags)
        directories.append(current)
        for component in components[1:-1]:
            _validate_directory_metadata(os.fstat(current), "signing key parent")
            current = os.open(component, directory_flags, dir_fd=current)
            directories.append(current)
        _validate_directory_metadata(os.fstat(current), "signing key parent")
        key_fd = os.open(components[-1], file_flags, dir_fd=current)
        _validate_private_key_metadata(os.fstat(key_fd))
        while directories:
            descriptor = directories[-1]
            os.close(descriptor)
            directories.pop()
        ownership_transferred = True
        return key_fd
    except OSError as exc:
        if key_fd is None:
            raise KeyProcessError("cannot open signing key") from exc
        raise KeyProcessError("cannot inspect signing key") from exc
    finally:
        if key_fd is not None and not ownership_transferred:
            try:
                os.close(key_fd)
            except OSError as exc:
                cleanup_errors.append(exc)
        while directories:
            descriptor = directories.pop()
            try:
                os.close(descriptor)
            except OSError as exc:
                cleanup_errors.append(exc)
        if cleanup_errors:
            raise KeyProcessError("signing key descriptor cleanup is uncertain") from cleanup_errors[0]


def _validate_work_root() -> None:
    try:
        native_host._validate_docker_config_dir(SIGNING_WORK_ROOT)
    except native_host.NativeHostError as exc:
        raise KeyProcessError(f"signing work root is not trusted: {exc}") from exc
    descriptor: int | None = None
    try:
        descriptor = os.open(SIGNING_WORK_ROOT, native_host._fixed_open_flags(directory=True))
        metadata = os.fstat(descriptor)
        if stat.S_IMODE(metadata.st_mode) != 0o700:
            _error("signing work root mode is not 0700")
    except OSError as exc:
        raise KeyProcessError("cannot inspect signing work root") from exc
    finally:
        if descriptor is not None:
            try:
                os.close(descriptor)
            except OSError as exc:
                raise KeyProcessError("signing work root close failed") from exc


def _write_private(path: str, value: bytes) -> None:
    flags = os.O_WRONLY | os.O_CREAT | os.O_EXCL
    for name in ("O_CLOEXEC", "O_NOFOLLOW"):
        if not hasattr(os, name):
            _error("signing temporary files require no-follow close-on-exec flags")
        flags |= getattr(os, name)
    descriptor: int | None = None
    try:
        descriptor = os.open(path, flags, 0o600)
        offset = 0
        while offset < len(value):
            written = os.write(descriptor, value[offset:])
            if written <= 0:
                _error("signing temporary file made no progress")
            offset += written
        os.fsync(descriptor)
    except OSError as exc:
        raise KeyProcessError("signing temporary file write failed") from exc
    finally:
        if descriptor is not None:
            try:
                os.close(descriptor)
            except OSError as exc:
                raise KeyProcessError("signing temporary file close failed") from exc


def _read_private(path: str) -> bytes:
    try:
        value = native_host.read_no_follow(path, limit=MAX_PROCESS_OUTPUT_BYTES)
    except native_host.NativeHostError as exc:
        raise KeyProcessError("signing process output cannot be read") from exc
    if not value:
        _error("signing process produced empty output")
    return value


def _new_workspace() -> str:
    try:
        return tempfile.mkdtemp(prefix=".align-sign-", dir=SIGNING_WORK_ROOT)
    except OSError as exc:
        raise KeyProcessError("cannot create private signing workspace") from exc


def _remove_workspace(workspace: str) -> None:
    errors: list[BaseException] = []
    for name in _PRIVATE_FILES:
        path = os.path.join(workspace, name)
        try:
            os.unlink(path)
        except FileNotFoundError:
            pass
        except OSError as exc:
            errors.append(exc)
    try:
        if os.listdir(workspace):
            errors.append(KeyProcessError("private signing workspace contains unexpected entries"))
    except OSError as exc:
        errors.append(exc)
    try:
        os.rmdir(workspace)
    except OSError as exc:
        errors.append(exc)
    if errors:
        raise KeyProcessError("private signing workspace cleanup is uncertain") from errors[0]


def _runner_result(
    runner: Runner,
    argv: tuple[str, ...],
    config: KeyProcessConfig,
    *,
    extra_fds: Sequence[int] = (),
    stdin_fd: int | None = None,
) -> tuple[bytes, ...]:
    if not callable(runner):
        _error("key-process runner is not callable")
    try:
        runner_kwargs: dict[str, Any] = {}
        if extra_fds:
            runner_kwargs["extra_fds"] = tuple(extra_fds)
        if stdin_fd is not None:
            runner_kwargs["stdin_fd"] = stdin_fd
        outputs, actual_hash = runner(
            SSH_KEYGEN,
            (argv,),
            config.ssh_keygen_sha256,
            **runner_kwargs,
        )
    except native_host.NativeHostError:
        raise
    except BaseException as exc:
        raise KeyProcessError("key-process runner failed") from exc
    if not isinstance(outputs, tuple) or len(outputs) != 1 or not isinstance(outputs[0], bytes):
        _error("key-process runner returned the wrong output")
    if actual_hash != config.ssh_keygen_sha256:
        _error("key-process executable hash changed across the boundary")
    return outputs


def sign(
    config: KeyProcessConfig,
    message: bytes,
    namespace: bytes,
    cleanup_snapshot: cleanup.CleanupSnapshot,
    *,
    runner: Runner = native_host.run_pinned_commands,
) -> bytes:
    """Sign one complete message after trusted measurement cleanup."""

    if not isinstance(config, KeyProcessConfig):
        _error("key-process config has the wrong type")
    message = _message(message)
    namespace = _namespace(namespace)
    _require_signing_cleanup(cleanup_snapshot)
    workspace: str | None = None
    key_fd: int | None = None
    try:
        _validate_work_root()
        workspace = _new_workspace()
        message_path = os.path.join(workspace, "message")
        _write_private(message_path, message)
        key_fd = _open_private_key()
        argv = (
            SSH_KEYGEN,
            "-Y",
            "sign",
            "-f",
            f"/proc/self/fd/{key_fd}",
            "-n",
            namespace.decode("ascii"),
            message_path,
        )
        outputs = _runner_result(runner, argv, config, extra_fds=(key_fd,))
        if outputs[0]:
            _error("signing process wrote unexpected stdout")
        try:
            os.close(key_fd)
        except OSError as exc:
            raise KeyProcessError("signing key descriptor close failed") from exc
        key_fd = None
        signature_bytes = _read_private(message_path + ".sig")
        try:
            signature = sshsig.decode_armor(
                signature_bytes,
                expected_public_key_blob=config.public_key_blob,
                expected_namespace=namespace,
            )
            if sshsig.encode_armor(signature) != signature_bytes:
                _error("signing process produced noncanonical armor")
        except sshsig.SSHSigError as exc:
            raise KeyProcessError(f"signing process produced invalid SSHSIG: {exc}") from exc
        return signature_bytes
    except KeyProcessError:
        raise
    except native_host.NativeHostError as exc:
        raise KeyProcessError("signing process failed") from exc
    except BaseException as exc:
        raise KeyProcessError("signing operation failed") from exc
    finally:
        close_error: BaseException | None = None
        if key_fd is not None:
            try:
                os.close(key_fd)
            except OSError as exc:
                close_error = exc
        if workspace is not None:
            try:
                _remove_workspace(workspace)
            except KeyProcessError as exc:
                close_error = close_error or exc
        if close_error is not None:
            raise KeyProcessError("signing cleanup is uncertain") from close_error


def _allowed_signers(public_key_blob: bytes) -> bytes:
    encoded = base64.b64encode(public_key_blob).decode("ascii")
    return f"{SIGNER_IDENTITY} ssh-ed25519 {encoded}\n".encode("ascii")


def verify(
    config: KeyProcessConfig,
    message: bytes,
    signature: sshsig.Signature,
    *,
    runner: Runner = native_host.run_pinned_commands,
) -> bool:
    """Verify one message/signature pair with the pinned host process."""

    if not isinstance(config, KeyProcessConfig):
        _error("key-process config has the wrong type")
    message = _message(message)
    if not isinstance(signature, sshsig.Signature):
        _error("signature has the wrong type")
    try:
        signature_bytes = sshsig.encode_armor(signature)
        decoded = sshsig.decode_armor(
            signature_bytes,
            expected_public_key_blob=config.public_key_blob,
            expected_namespace=signature.namespace,
        )
        namespace = _namespace(decoded.namespace)
    except sshsig.SSHSigError as exc:
        raise KeyProcessError(f"signature framing is invalid: {exc}") from exc
    workspace: str | None = None
    stdin_fd: int | None = None
    try:
        _validate_work_root()
        workspace = _new_workspace()
        message_path = os.path.join(workspace, "message")
        signature_path = os.path.join(workspace, "message.sig")
        allowed_path = os.path.join(workspace, "allowed-signers")
        _write_private(message_path, message)
        _write_private(signature_path, signature_bytes)
        _write_private(allowed_path, _allowed_signers(config.public_key_blob))
        try:
            stdin_fd = os.open(message_path, native_host._fixed_open_flags())
        except OSError as exc:
            raise KeyProcessError("cannot open verification message") from exc
        argv = (
            SSH_KEYGEN,
            "-Y",
            "verify",
            "-f",
            allowed_path,
            "-I",
            SIGNER_IDENTITY,
            "-n",
            namespace.decode("ascii"),
            "-s",
            signature_path,
        )
        try:
            _runner_result(runner, argv, config, stdin_fd=stdin_fd)
        except native_host.NativeHostError:
            return False
        return True
    except KeyProcessError:
        raise
    except BaseException as exc:
        raise KeyProcessError("verification operation failed") from exc
    finally:
        close_error: BaseException | None = None
        if stdin_fd is not None:
            try:
                os.close(stdin_fd)
            except OSError as exc:
                close_error = exc
        if workspace is not None:
            try:
                _remove_workspace(workspace)
            except KeyProcessError as exc:
                close_error = close_error or exc
        if close_error is not None:
            raise KeyProcessError("verification cleanup is uncertain") from close_error
