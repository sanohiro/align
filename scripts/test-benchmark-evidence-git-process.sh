#!/usr/bin/env bash
# Deterministic owner for the pinned Git process/configuration boundary.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd -P)"
cd "$REPO_ROOT"
export PYTHONDONTWRITEBYTECODE=1

python3 - "$REPO_ROOT" <<'PY'
import os
import signal
import tempfile

from scripts.benchmark_evidence import git_process as gp


def expect_error(call, fragment):
    try:
        call()
    except gp.GitProcessError as exc:
        assert fragment in str(exc), str(exc)
    else:
        raise AssertionError("accepted invalid Git process input")


class FakeProcess:
    pid = 4242
    returncode = 0

    def communicate(self, timeout=None):
        assert timeout == 1.0 or timeout is None
        return b"", b""


def main(repo_root):
    with tempfile.TemporaryDirectory() as home:
        spec = gp.build_spec(repo_root, home)
        try:
            assert spec.repository == repo_root
            assert spec.home == home
            assert spec.repository_fd is not None
            assert os.fstat(spec.repository_fd).st_mode & 0o170000 == 0o040000
            assert spec.argv == (
                "/usr/bin/git",
                "--no-pager",
                "--no-optional-locks",
                "--no-lazy-fetch",
                "--no-replace-objects",
                "-c",
                "core.hooksPath=/dev/null",
                "-c",
                "core.attributesFile=/dev/null",
                "-c",
                "core.fsmonitor=false",
                "-c",
                "core.commitGraph=false",
                "-c",
                "fetch.recurseSubmodules=false",
                "-c",
                "protocol.file.allow=never",
                "cat-file",
                "--batch",
            )
            expected_env = {
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
            assert spec.env() == expected_env
            assert set(spec.env()) == set(expected_env)
            for forbidden in (
                "GIT_DIR",
                "GIT_WORK_TREE",
                "GIT_OBJECT_DIRECTORY",
                "GIT_ALTERNATE_OBJECT_DIRECTORIES",
                "GIT_SSH_COMMAND",
                "HTTP_PROXY",
                "HTTPS_PROXY",
                "PYTHONPATH",
            ):
                assert forbidden not in spec.env()
        finally:
            spec.close()

        calls = []
        original_popen = gp.subprocess.Popen
        process = gp.PinnedGitProcess(repo_root, home)
        repository_fd = process.spec.repository_fd
        assert repository_fd is not None
        expected_env = process.spec.env()

        def fake_popen(*args, **kwargs):
            calls.append((args, kwargs))
            return FakeProcess()

        gp.subprocess.Popen = fake_popen
        try:
            process.start().close()
        finally:
            gp.subprocess.Popen = original_popen
        assert calls == [
            (
                (process.spec.argv,),
                {
                    "cwd": f"/dev/fd/{repository_fd}",
                    "env": expected_env,
                    "stdin": gp.subprocess.PIPE,
                    "stdout": gp.subprocess.PIPE,
                    "stderr": gp.subprocess.PIPE,
                    "close_fds": True,
                    "pass_fds": (repository_fd,),
                    "start_new_session": True,
                    "text": False,
                    "shell": False,
                    "bufsize": 0,
                },
            )
        ]
        assert process.spec.repository_fd is None

        with gp.PinnedGitProcess(repo_root, home) as process:
            assert process.pid > 0

        with tempfile.TemporaryDirectory() as parent:
            temporary_repo = os.path.join(parent, "repo")
            gp.subprocess.run(
                [gp.GIT_PATH, "init", "--quiet", temporary_repo],
                cwd=parent,
                env={
                    "GIT_CONFIG_GLOBAL": "/dev/null",
                    "GIT_CONFIG_NOSYSTEM": "1",
                    "GIT_CONFIG_SYSTEM": "/dev/null",
                    "HOME": home,
                    "LC_ALL": "C",
                    "PATH": "/usr/bin:/bin",
                },
                check=True,
                stdin=gp.subprocess.DEVNULL,
                stdout=gp.subprocess.PIPE,
                stderr=gp.subprocess.PIPE,
            )
            moved_repo = os.path.join(parent, "moved-repo")
            process = gp.PinnedGitProcess(temporary_repo, home)
            os.rename(temporary_repo, moved_repo)
            os.mkdir(temporary_repo)
            with process:
                pass

        nonempty = os.path.join(home, "foreign")
        with open(nonempty, "wb") as stream:
            stream.write(b"foreign")
        expect_error(lambda: gp.build_spec(repo_root, home), "must be empty")
        os.unlink(nonempty)

        expect_error(lambda: gp.build_spec("relative", home), "absolute path")
        expect_error(lambda: gp.build_spec(repo_root + "\x00", home), "absolute path")
        expect_error(lambda: gp.build_spec(repo_root, os.path.join(home, "missing")), "existing")

        with tempfile.TemporaryDirectory() as outer:
            target = os.path.join(outer, "target")
            os.mkdir(target)
            linked = os.path.join(outer, "linked")
            os.symlink(target, linked)
            expect_error(lambda: gp.build_spec(linked, home), "existing")
            expect_error(lambda: gp.build_spec(target + "/", home), "canonical")
            expect_error(lambda: gp.build_spec(target + "/.", home), "canonical")
            child = os.path.join(target, "child")
            os.mkdir(child)
            expect_error(lambda: gp.build_spec(os.path.join(linked, "child"), home), "existing")

            linked_home = os.path.join(outer, "linked-home")
            os.symlink(home, linked_home)
            expect_error(lambda: gp.build_spec(repo_root, linked_home), "existing")

        original_handler = signal.getsignal(signal.SIGINT)
        original_popen = gp.subprocess.Popen
        launched = []

        def raising_handler(signum, frame):
            raise KeyboardInterrupt

        def signal_during_popen(*args, **kwargs):
            child = original_popen(*args, **kwargs)
            launched.append(child)
            os.kill(os.getpid(), signal.SIGINT)
            return child

        signal.signal(signal.SIGINT, raising_handler)
        gp.subprocess.Popen = signal_during_popen
        try:
            interrupted = gp.PinnedGitProcess(repo_root, home)
            try:
                interrupted.start()
            except KeyboardInterrupt:
                pass
            else:
                raise AssertionError("did not redeliver the deferred interrupt")
        finally:
            gp.subprocess.Popen = original_popen
            signal.signal(signal.SIGINT, original_handler)
        assert launched and launched[0].poll() is not None

    print("benchmark evidence Git process checks passed")


main(os.path.abspath(os.sys.argv[1]))
PY
