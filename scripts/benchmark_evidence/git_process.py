"""Pinned Git process boundary for the benchmark evidence controller.

This module owns only process construction and lifecycle.  It never resolves
revisions, reads object responses, traverses a repository, or imports source
from one.  The later repository reader consumes the already-fixed
``git cat-file --batch`` process through :mod:`git_batch`.
"""

from __future__ import annotations

import os
import signal
import subprocess
from contextlib import contextmanager
from dataclasses import dataclass
from typing import BinaryIO, Iterator


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
_FD_PATH_ROOT = "/dev/fd"
try:
    _DIRECTORY_FLAGS = (
        os.O_RDONLY | os.O_DIRECTORY | os.O_CLOEXEC | os.O_NOFOLLOW
    )
except AttributeError as exc:  # pragma: no cover - unsupported non-POSIX host
    raise RuntimeError("pinned Git requires no-follow directory open flags") from exc


@dataclass
class GitProcessSpec:
    """The argv/environment contract and owned repository directory FD."""

    repository: str
    home: str
    argv: tuple[str, ...]
    environment: tuple[tuple[str, str], ...]
    repository_fd: int | None

    def env(self) -> dict[str, str]:
        """Return a fresh environment mapping for ``subprocess.Popen``."""

        return dict(self.environment)

    def close(self) -> None:
        """Close the repository FD exactly once."""

        if self.repository_fd is None:
            return
        fd = self.repository_fd
        self.repository_fd = None
        try:
            os.close(fd)
        except OSError as exc:
            raise GitProcessError("failed to close the repository directory") from exc


def _components(path: object, label: str) -> tuple[str, ...]:
    if not isinstance(path, str) or not path or "\x00" in path or not os.path.isabs(path):
        raise GitProcessError(f"{label} must be an absolute path without NUL")
    parts = tuple(path.split("/")[1:])
    if not parts or any(part in ("", ".", "..") for part in parts):
        raise GitProcessError(f"{label} must use canonical absolute path components")
    return parts


def _open_directory(path: object, label: str) -> int:
    """Open every absolute path component with no-follow directory semantics."""

    parts = _components(path, label)
    fd: int | None = None
    try:
        fd = os.open("/", _DIRECTORY_FLAGS)
        for part in parts:
            next_fd = os.open(part, _DIRECTORY_FLAGS, dir_fd=fd)
            os.close(fd)
            fd = next_fd
        return fd
    except OSError as exc:
        if fd is not None:
            try:
                os.close(fd)
            except OSError:
                pass
        raise GitProcessError(f"{label} must be an existing non-symlink directory") from exc


def _open_empty_directory(path: object, label: str) -> int:
    fd = _open_directory(path, label)
    try:
        if os.listdir(fd):
            raise GitProcessError(f"{label} must be empty")
        return fd
    except BaseException:
        try:
            os.close(fd)
        except OSError:
            pass
        raise


@contextmanager
def _defer_spawn_signals() -> Iterator[list[tuple[int, object]]]:
    """Record process signals while Popen establishes child ownership."""

    signals = tuple(
        signum
        for signum in (signal.SIGINT, signal.SIGTERM, getattr(signal, "SIGHUP", None))
        if signum is not None
    )
    previous: dict[int, object] = {}
    deferred: list[tuple[int, object]] = []

    def defer(signum, _frame):
        deferred.append((signum, previous[signum]))

    installed: list[int] = []
    try:
        for signum in signals:
            previous[signum] = signal.getsignal(signum)
            signal.signal(signum, defer)
            installed.append(signum)
    except ValueError:
        for signum in reversed(installed):
            signal.signal(signum, previous[signum])
        yield deferred
        return

    try:
        yield deferred
    finally:
        for signum in reversed(installed):
            signal.signal(signum, previous[signum])


def _deliver_deferred(deferred: list[tuple[int, object]]) -> None:
    for signum, handler in deferred:
        if handler == signal.SIG_IGN:
            continue
        if callable(handler):
            handler(signum, None)
        else:
            os.kill(os.getpid(), signum)


def build_spec(repository: object, home: object) -> GitProcessSpec:
    """Build the fixed Git invocation without consulting ambient state."""

    _components(repository, "repository")
    _components(home, "home")
    repository_fd = _open_directory(repository, "repository")
    try:
        if not os.path.isdir(_FD_PATH_ROOT):
            raise GitProcessError("the descriptor-relative cwd path is unavailable")
        home_fd = _open_empty_directory(home, "home")
        os.close(home_fd)
    except BaseException:
        try:
            os.close(repository_fd)
        except OSError:
            pass
        raise
    argv = [
        GIT_PATH,
        "--no-pager",
        "--no-optional-locks",
        "--no-lazy-fetch",
        "--no-replace-objects",
    ]
    for config in _CONFIG:
        argv.extend(("-c", config))
    argv.extend(("cat-file", "--batch"))
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
        repository_fd=repository_fd,
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

    @property
    def stdin(self) -> BinaryIO:
        """Return the owned child's binary stdin after start."""

        if self._process is None or self._process.stdin is None:
            raise GitProcessError("Git process stdin is unavailable")
        return self._process.stdin

    @property
    def stdout(self) -> BinaryIO:
        """Return the owned child's binary stdout after start."""

        if self._process is None or self._process.stdout is None:
            raise GitProcessError("Git process stdout is unavailable")
        return self._process.stdout

    def start(self) -> "PinnedGitProcess":
        """Start the fixed child with no shell and a new process group."""

        if self._closed:
            raise GitProcessError("Git process is already closed")
        if self._process is not None:
            raise GitProcessError("Git process has already started")
        repository_fd = self.spec.repository_fd
        if repository_fd is None:
            raise GitProcessError("repository directory is already closed")
        process = None
        with _defer_spawn_signals() as deferred:
            try:
                process = subprocess.Popen(
                    self.spec.argv,
                    cwd=f"{_FD_PATH_ROOT}/{repository_fd}",
                    env=self.spec.env(),
                    stdin=subprocess.PIPE,
                    stdout=subprocess.PIPE,
                    stderr=subprocess.PIPE,
                    close_fds=True,
                    pass_fds=(repository_fd,),
                    start_new_session=True,
                    text=False,
                    shell=False,
                    bufsize=0,
                )
                self._process = process
                self.spec.close()
            except BaseException:
                if process is not None:
                    self._process = process
                    self.close()
                else:
                    self.spec.close()
                raise
        if deferred:
            self.close()
            _deliver_deferred(deferred)
        return self

    def close(self) -> None:
        """Close stdin, drain the child, and terminate its group if needed."""

        if self._closed:
            return
        self._closed = True
        process = self._process
        try:
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
        finally:
            self.spec.close()

    def __enter__(self) -> "PinnedGitProcess":
        return self.start()

    def __exit__(self, exc_type, exc_value, traceback) -> None:
        self.close()
