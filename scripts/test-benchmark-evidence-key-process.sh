#!/usr/bin/env bash
# Deterministic owner for the trusted host-side SSHSIG key process.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd -P)"
cd "$REPO_ROOT"
export PYTHONDONTWRITEBYTECODE=1

python3 - "$REPO_ROOT" <<'PY'
import base64
import hashlib
import os
import stat
import tempfile
from types import SimpleNamespace

from scripts.benchmark_evidence import canonical_json as cj
from scripts.benchmark_evidence import cleanup
from scripts.benchmark_evidence import native_host
from scripts.benchmark_evidence import native_signing
from scripts.benchmark_evidence import sshsig


H64 = "a" * 64
RAW_KEY = bytes(range(32))
KEY_BLOB = (
    len(sshsig.KEY_ALGORITHM).to_bytes(4, "big")
    + sshsig.KEY_ALGORITHM
    + len(RAW_KEY).to_bytes(4, "big")
    + RAW_KEY
)
FINGERPRINT = "SHA256:" + base64.b64encode(hashlib.sha256(KEY_BLOB).digest()).decode("ascii").rstrip("=")
MESSAGE = b'{"body":{},"body_sha256":"' + b"0" * 64 + b'"}\n'
SNAPSHOT = cleanup.CleanupSnapshot(0, 0, 0, 0, 0, True, True, True)


def O(*pairs):
    return cj.Object(pairs)


def expect_error(action, fragment):
    try:
        action()
    except (native_signing.KeyProcessError, native_host.NativeHostError) as exc:
        assert fragment in str(exc), (fragment, str(exc))
    else:
        raise AssertionError(f"accepted invalid operation: {fragment}")


profile = O(
    (
        "signing",
        O(
            ("key_type", "ssh-ed25519"),
            ("public_key_base64", base64.b64encode(RAW_KEY).decode("ascii")),
            ("fingerprint", FINGERPRINT),
            ("ssh_keygen_version", "OpenSSH_9.9"),
            ("ssh_keygen_executable_sha256", H64),
        ),
    )
)
config = native_signing.from_profile(profile)
assert config.public_key_blob == KEY_BLOB
assert config.ssh_keygen_sha256 == H64
bad_fingerprint = O(
    (
        "signing",
        O(
            ("key_type", "ssh-ed25519"),
            ("public_key_base64", base64.b64encode(RAW_KEY).decode("ascii")),
            ("fingerprint", "SHA256:" + "A" * 43),
            ("ssh_keygen_version", "OpenSSH_9.9"),
            ("ssh_keygen_executable_sha256", H64),
        ),
    )
)
expect_error(lambda: native_signing.from_profile(bad_fingerprint), "fingerprint")
expect_error(lambda: native_signing.from_profile(O(("signing", O(("key_type", "ssh-ed25519"),))),), "member order")

native_signing._validate_private_key_metadata(SimpleNamespace(st_mode=stat.S_IFREG | 0o600, st_uid=0))
expect_error(
    lambda: native_signing._validate_private_key_metadata(
        SimpleNamespace(st_mode=stat.S_IFREG | 0o640, st_uid=0)
    ),
    "group/other",
)
expect_error(
    lambda: native_signing._validate_private_key_metadata(
        SimpleNamespace(st_mode=stat.S_IFREG | 0o600, st_uid=1000)
    ),
    "root-owned",
)
expect_error(
    lambda: native_signing._validate_private_key_metadata(
        SimpleNamespace(st_mode=stat.S_IFDIR | 0o700, st_uid=0)
    ),
    "regular file",
)


original_root = native_signing.SIGNING_WORK_ROOT
original_validate_root = native_signing._validate_work_root
original_open_key = native_signing._open_private_key
try:
    with tempfile.TemporaryDirectory() as work_root:
        native_signing.SIGNING_WORK_ROOT = work_root
        native_signing._validate_work_root = lambda: None
        events = []

        def fake_open_key():
            descriptor = os.open(os.devnull, os.O_RDONLY)
            events.append(("key-open", descriptor))
            return descriptor

        native_signing._open_private_key = fake_open_key
        fixed_signature = sshsig.encode_armor(
            sshsig.Signature(KEY_BLOB, sshsig.REPORT_NAMESPACE, b"S" * 64)
        )

        def sign_runner(executable, commands, expected_hash, *, extra_fds):
            argv = commands[0]
            events.append(("sign-run", executable, argv, expected_hash, tuple(extra_fds)))
            assert executable == native_signing.SSH_KEYGEN
            assert expected_hash == H64
            assert len(tuple(extra_fds)) == 1
            key_fd = tuple(extra_fds)[0]
            assert argv[:4] == (native_signing.SSH_KEYGEN, "-Y", "sign", "-f")
            assert argv[4] == f"/proc/self/fd/{key_fd}"
            assert argv[5:7] == ("-n", sshsig.REPORT_NAMESPACE.decode("ascii"))
            assert native_signing.SIGNING_KEY_PATH not in argv
            with open(argv[-1] + ".sig", "wb") as stream:
                stream.write(fixed_signature)
            return ((b"",), H64)

        signed = native_signing.sign(
            config,
            MESSAGE,
            sshsig.REPORT_NAMESPACE,
            SNAPSHOT,
            runner=sign_runner,
        )
        assert signed == fixed_signature
        assert events[0][0] == "key-open"
        assert events[1][0] == "sign-run"
        assert os.listdir(work_root) == []

        forbidden_events = []

        def forbidden_key():
            forbidden_events.append("key")
            raise AssertionError("key opened before cleanup proof")

        native_signing._open_private_key = forbidden_key
        bad_snapshot = cleanup.CleanupSnapshot(1, 0, 0, 0, 0, True, True, True)
        expect_error(
            lambda: native_signing.sign(
                config,
                MESSAGE,
                sshsig.REPORT_NAMESPACE,
                bad_snapshot,
                runner=sign_runner,
            ),
            "resources to be gone",
        )
        assert forbidden_events == []
        native_signing._open_private_key = fake_open_key
        expect_error(
            lambda: native_signing.sign(
                config,
                MESSAGE,
                b"wrong-namespace",
                SNAPSHOT,
                runner=sign_runner,
            ),
            "declared evidence namespace",
        )
        expect_error(
            lambda: native_signing.sign(
                config,
                MESSAGE,
                sshsig.REPORT_NAMESPACE,
                SNAPSHOT,
                runner=lambda *args, **kwargs: (_ for _ in ()).throw(KeyboardInterrupt()),
            ),
            "runner failed",
        )
        assert os.listdir(work_root) == []

        verify_signature = sshsig.Signature(KEY_BLOB, sshsig.REPORT_NAMESPACE, b"V" * 64)
        verify_events = []

        def verify_runner(executable, commands, expected_hash, *, stdin_fd):
            argv = commands[0]
            verify_events.append((executable, argv, expected_hash, stdin_fd))
            assert argv[:4] == (native_signing.SSH_KEYGEN, "-Y", "verify", "-f")
            assert argv[5:9] == ("-I", native_signing.SIGNER_IDENTITY, "-n", sshsig.REPORT_NAMESPACE.decode("ascii"))
            assert argv[9] == "-s"
            with open(argv[4], "rb") as stream:
                assert stream.read() == native_signing._allowed_signers(KEY_BLOB)
            with open(argv[10], "rb") as stream:
                assert stream.read() == sshsig.encode_armor(verify_signature)
            os.lseek(stdin_fd, 0, os.SEEK_SET)
            assert os.read(stdin_fd, len(MESSAGE)) == MESSAGE
            return ((b"",), H64)

        assert native_signing.verify(config, MESSAGE, verify_signature, runner=verify_runner) is True
        assert len(verify_events) == 1
        assert os.listdir(work_root) == []

        def rejected_verify_runner(*_args, **_kwargs):
            raise native_host.NativeHostError("ssh-keygen exited nonzero")

        assert native_signing.verify(config, MESSAGE, verify_signature, runner=rejected_verify_runner) is False
        assert os.listdir(work_root) == []
finally:
    native_signing.SIGNING_WORK_ROOT = original_root
    native_signing._validate_work_root = original_validate_root
    native_signing._open_private_key = original_open_key


extra_fd = os.open(os.devnull, os.O_RDONLY)
original_open_executable = native_host._open_executable
original_hash_fd = native_host._hash_fd
original_run_command = native_host._run_command
events = []


def fake_open_executable(_path):
    descriptor = os.open(os.devnull, os.O_RDONLY)
    events.append(("open", descriptor))
    return descriptor


def fake_hash_fd(descriptor, path):
    events.append(("hash", descriptor, path))
    return H64


def fake_run_command(argv, *, executable_fd=None, extra_fds=()):
    events.append(("run", argv, executable_fd, tuple(extra_fds)))
    return b"ok"


native_host._open_executable = fake_open_executable
native_host._hash_fd = fake_hash_fd
native_host._run_command = fake_run_command
try:
    outputs, digest = native_host.run_pinned_commands(
        native_signing.SSH_KEYGEN,
        ((native_signing.SSH_KEYGEN, "-Y", "check-novalidate"),),
        H64,
        extra_fds=(extra_fd,),
    )
finally:
    native_host._open_executable = original_open_executable
    native_host._hash_fd = original_hash_fd
    native_host._run_command = original_run_command
    os.close(extra_fd)
assert outputs == (b"ok",)
assert digest == H64
assert [event[0] for event in events] == ["open", "hash", "run"]
assert events[-1][3] == (extra_fd,)
assert events[-1][2] == events[0][1]

print("key-process evidence checks passed")
PY
