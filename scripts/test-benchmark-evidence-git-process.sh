#!/usr/bin/env bash
# Deterministic owner for the pinned Git process/configuration boundary.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd -P)"
cd "$REPO_ROOT"
export PYTHONDONTWRITEBYTECODE=1

python3 - "$REPO_ROOT" <<'PY'
import os
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
        assert spec.repository == repo_root
        assert spec.home == home
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
            "-C",
            repo_root,
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

        calls = []
        original_popen = gp.subprocess.Popen

        def fake_popen(*args, **kwargs):
            calls.append((args, kwargs))
            return FakeProcess()

        gp.subprocess.Popen = fake_popen
        try:
            process = gp.PinnedGitProcess(repo_root, home).start()
            process.close()
        finally:
            gp.subprocess.Popen = original_popen
        assert calls == [
            (
                (spec.argv,),
                {
                    "cwd": repo_root,
                    "env": expected_env,
                    "stdin": gp.subprocess.PIPE,
                    "stdout": gp.subprocess.PIPE,
                    "stderr": gp.subprocess.PIPE,
                    "close_fds": True,
                    "start_new_session": True,
                    "text": False,
                    "shell": False,
                    "bufsize": 0,
                },
            )
        ]

        with gp.PinnedGitProcess(repo_root, home) as process:
            assert process.pid > 0

        nonempty = os.path.join(home, "foreign")
        with open(nonempty, "wb") as stream:
            stream.write(b"foreign")
        expect_error(lambda: gp.build_spec(repo_root, home), "must be empty")

        expect_error(lambda: gp.build_spec("relative", home), "absolute path")
        expect_error(lambda: gp.build_spec(repo_root + "\x00", home), "absolute path")
        expect_error(lambda: gp.build_spec(repo_root, os.path.join(home, "missing")), "existing directory")

        with tempfile.TemporaryDirectory() as outer:
            linked = os.path.join(outer, "linked")
            os.symlink(home, linked)
            expect_error(lambda: gp.build_spec(repo_root, linked), "non-symlink directory")

    print("benchmark evidence Git process checks passed")


main(os.path.abspath(os.sys.argv[1]))
PY
