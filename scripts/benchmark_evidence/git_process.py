"""Pinned Git process boundary for the benchmark evidence controller.

This module owns only process construction and lifecycle.  It never resolves
revisions, reads object responses, traverses a repository, or imports source
from one.  The later repository reader consumes the already-fixed
``git cat-file --batch`` process through :mod:`git_batch`.
"""

from __future__ import annotations

import os
import signal
import stat
import subprocess
from dataclasses import dataclass


class GitProcessError(RuntimeError):
    """A pinned Git process boundary cannot be constructed or closed."""


GIT_PATH = "/usr/bin/git"
_CONFIG = (
    "core.hooksPath=/dev/null",
    "core.attributesFile=/dev/null",
    "core.fsmonitor=false",
    "core.commitGraph=false",
    "fetch.recurseSubmodules=false",
    "protocol.file.allow=never",
)
_FIXED_ENV_KEYS = (
    "CARGO_NET_OFFLINE",
    "GIT_ATTR_NOSYSTEM",
    "GIT_CONFIG_COUNT",
    "GIT_CONFIG_GLOBAL",
    "GIT_CONFIG_NOSYSTEM",
    "GIT_CONFIG_SYSTEM",
    "GIT_NO_LAZY_FETCH",
    "GIT_NO_REPLACE_OBJECTS",
    "GIT_OPTIONAL_LOCKS",
    "GIT_PAGER",
    "GIT_TERMINAL_PROMPT",
    "HOME",
    "LC_ALL",
    "PATH",
    "TZ",
)
_STOP_TIMEOUT_SECONDS = 1.0


@dataclass(frozen=True)
class GitProcessSpec:
    """The complete immutable argv/environment contract for one Git child."""

    repository: str
    home: str
    argv: tuple[str, ...]
    environment: tuple[tuple[str, str], ...]

    def env(self) -> dict[str, str]:
        """Return a fresh environment mapping for ``subprocess.Popen``."""

        return dict(self.environment)


def _directory(path: object, label: str, *, require_empty: bool) -> str:
    if not isinstance(path, str) or not path or "\x00" in path or not os.path.isabs(path):
        raise GitProcessError(f"{label} must be an absolute path without NUL")
    try:
        metadata = os.lstat(path)
    except OSError as exc:
        raise GitProcessError(f"{label} is not an existing directory") from exc
    if stat.S_ISLNK(metadata.st_mode) or not stat.S_ISDIR(metadata.st_mode):
        raise GitProcessError(f"{label} must be a non-symlink directory")
    if require_empty:
        try:
            with os.scandir(path) as entries:
                if next(entries, None) is not None:
                    raise GitProcessError(f"{label} must be empty")
        except OSError as exc:
            raise GitProcessError(f"{label} cannot be inspected") from exc
    return path


def build_spec(repository: object, home: object) -> GitProcessSpec:
    """Build the fixed Git invocation without consulting ambient state."""

    repository = _directory(repository, "repository", require_empty=False)
    home = _directory(home, "home", require_empty=True)
    argv = [
        GIT_PATH,
        "--no-pager",
        "--no-optional-locks",
        "--no-lazy-fetch",
        "--no-replace-objects",
    ]
    for config in _CONFIG:
        argv.extend(("-c", config))
    argv.extend(("-C", repository, "cat-file", "--batch"))
    environment = {
        "CARGO_NET_OFFLINE": "true",
        "GIT_ATTR_NOSYSTEM": "1",
        "GIT_CONFIG_COUNT": "0",
        "GIT_CONFIG_GLOBAL": "/dev/null",
        "GIT_CONFIG_NOSYSTEM": "1",
        "GIT_CONFIG_SYSTEM": "/dev/null",
        "GIT_NO_LAZY_FETCH": "1",
        "GIT_NO_REPLACE_OBJECTS": "1",
        "GIT_OPTIONAL_LOCKS": "0",
        "GIT_PAGER": "cat",
        "GIT_TERMINAL_PROMPT": "0",
        "HOME": home,
        "LC_ALL": "C",
        "PATH": "/usr/bin:/bin",
        "TZ": "UTC",
    }
    assert tuple(sorted(environment)) == _FIXED_ENV_KEYS
    return GitProcessSpec(
        repository=repository,
        home=home,
        argv=tuple(argv),
        environment=tuple(sorted(environment.items())),
    )


class PinnedGitProcess:
    """Own one fixed ``git cat-file --batch`` child and its process group."""

    def __init__(self, repository: object, home: object):
        self.spec = build_spec(repository, home)
        self._process: subprocess.Popen[bytes] | None = None
        self._closed = False

    @property
    def pid(self) -> int:
        """Return the child PID after start."""

        if self._process is None:
            raise GitProcessError("Git process has not started")
        return self._process.pid

    def start(self) -> "PinnedGitProcess":
        """Start the fixed child with no shell and a new process group."""

        if self._closed:
            raise GitProcessError("Git process is already closed")
        if self._process is not None:
            raise GitProcessError("Git process has already started")
        try:
            self._process = subprocess.Popen(
                self.spec.argv,
                cwd=self.spec.repository,
                env=self.spec.env(),
                stdin=subprocess.PIPE,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                close_fds=True,
                start_new_session=True,
                text=False,
                shell=False,
                bufsize=0,
            )
        except OSError as exc:
            raise GitProcessError("failed to start pinned Git") from exc
        return self

    def close(self) -> None:
        """Close stdin, drain the child, and terminate its group if needed."""

        if self._closed:
            return
        self._closed = True
        process = self._process
        if process is None:
            return
        try:
            process.communicate(timeout=_STOP_TIMEOUT_SECONDS)
        except subprocess.TimeoutExpired:
            try:
                os.killpg(process.pid, signal.SIGTERM)
            except OSError:
                pass
            try:
                process.communicate(timeout=_STOP_TIMEOUT_SECONDS)
            except subprocess.TimeoutExpired:
                try:
                    os.killpg(process.pid, signal.SIGKILL)
                except OSError:
                    pass
                process.communicate()
        if process.returncode != 0:
            raise GitProcessError(f"pinned Git exited with status {process.returncode}")

    def __enter__(self) -> "PinnedGitProcess":
        return self.start()

    def __exit__(self, exc_type, exc_value, traceback) -> None:
        self.close()
