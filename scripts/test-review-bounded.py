#!/usr/bin/env python3
"""Exercise real Git/process boundaries with deterministic provider fixtures."""

import importlib.util
import json
import os
import shutil
import signal
import subprocess
import sys
import tempfile
import time
import unittest
from pathlib import Path

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parent.parent
WRAPPER = ROOT / "scripts/review-bounded.sh"
AGENT = Path(".agents/agents/align-reviewer/agent.md")
REAL_GIT = shutil.which("git")
SPEC = importlib.util.spec_from_file_location("agy_result", ROOT / "scripts/review-agy-result.py")
PARSER = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(PARSER)


def events(root):
    return [
        {"event": "init", "conversation_id": "fresh-review-1", "init": {
            "cwd": str(root), "agent": "align-reviewer", "model": "gemini-3.8-flash-high",
            "permission_mode": "request-review", "tools": ["view_file", "grep_search"]}},
        {"event": "step_update", "step_update": {
            "conversation_id": "fresh-review-1", "step_type": "tool", "state": "DONE",
            "tool_name": "view_file", "tool_info": {"name": "view_file"}}},
        {"event": "result", "result": {
            "conversation_id": "fresh-review-1", "status": "SUCCESS", "num_turns": 1,
            "response": "No actionable findings.\nALIGN_REVIEW_VERDICT=CLEAN\n"}},
    ]


STUB = r'''#!/usr/bin/env python3
import json, os, signal, subprocess, sys, time
from pathlib import Path
mode = os.environ.get('REVIEW_TEST_MODE', 'clean')
args = Path(os.environ['REVIEW_TEST_ARGS'])
args.write_text(json.dumps(sys.argv[1:]))
provider = Path(sys.argv[0]).name
if mode.startswith('resistant'):
    child = subprocess.Popen([sys.executable, '-c',
        'import os,signal,time; from pathlib import Path; '
        'signal.signal(signal.SIGTERM,signal.SIG_IGN); '
        'Path(os.environ["REVIEW_TEST_PID"]).write_text(str(os.getpid())); time.sleep(60)'])
    while not Path(os.environ['REVIEW_TEST_PID']).exists(): time.sleep(.02)
if mode in ('resistant-stall', 'stall'):
    signal.signal(signal.SIGTERM, signal.SIG_IGN)
    time.sleep(60)
if mode == 'dirty': Path('unexpected.txt').write_text('mutation')
if mode == 'head': subprocess.run(['git', 'commit', '--allow-empty', '-qm', 'drift'], check=True)
if mode == 'base': subprocess.run(['git', 'update-ref', 'refs/heads/main', 'HEAD'], check=True)
if mode == 'git-failure': Path(os.environ['REVIEW_TEST_GIT_FAIL']).touch()
if provider == 'codex':
    print('codex', flush=True)
    if mode == 'progress':
        for i in range(4): print('phase', i, flush=True); time.sleep(.8)
    if mode == 'findings': print('- [P1] Defect in sample.py:2\nALIGN_REVIEW_VERDICT=FINDINGS')
    elif mode == 'native': print('No findings.')
    elif mode == 'trailing': print('ALIGN_REVIEW_VERDICT=CLEAN\nunfinished')
    else: print('ALIGN_REVIEW_VERDICT=CLEAN')
else:
    e = json.loads(os.environ['REVIEW_TEST_EVENTS'])
    if mode == 'findings': e[-1]['result']['response'] = '- [P1] Defect in sample.py:2\nALIGN_REVIEW_VERDICT=FINDINGS'
    if mode == 'error': e[-1]['result']['status'] = 'ERROR'
    if mode == 'partial': e = e[:-1]
    if mode == 'stderr': print('permission denied', file=sys.stderr)
    for row in e: print(json.dumps(row), flush=True)
sys.exit(7 if mode == 'failed-clean' else 0)
'''


def git(repo, *args):
    return subprocess.check_output([REAL_GIT, "-C", str(repo), *args], text=True,
                                   stderr=subprocess.PIPE).strip()


def fixture(parent):
    repo = parent / "repo"
    repo.mkdir()
    git(repo, "init", "-q", "-b", "main")
    for key, value in (("user.name", "review-test"), ("user.email", "review@example.invalid"),
                       ("commit.gpgsign", "false")):
        git(repo, "config", key, value)
    (repo / "CLAUDE.md").write_text(
        "# Canonical review fixture instructions\n"
        "Read HANDOFF.md first. Inspect only; do not modify files or execute commands.\n"
        "seconds_to_ms takes an integer count of seconds and must return that duration in milliseconds.\n"
        "Report actionable defects with file and line, then exactly one unfenced final line:\n"
        "ALIGN_REVIEW_VERDICT=CLEAN or ALIGN_REVIEW_VERDICT=FINDINGS.\n")
    (repo / "AGENTS.md").symlink_to("CLAUDE.md")
    (repo / "HANDOFF.md").write_text("Review the committed duration conversion change.\n")
    (repo / AGENT).parent.mkdir(parents=True)
    shutil.copyfile(ROOT / AGENT, repo / AGENT)
    (repo / "sample.py").write_text("def seconds_to_ms(seconds: int) -> int:\n    return seconds * 1000\n")
    git(repo, "add", ".")
    git(repo, "commit", "-qm", "baseline")
    git(repo, "switch", "-qc", "candidate")
    (repo / "sample.py").write_text("def seconds_to_ms(seconds: int) -> int:\n    return seconds\n")
    git(repo, "commit", "-qam", "change conversion")
    return repo


class StreamOwner(unittest.TestCase):
    def test_complete_and_findings(self):
        for verdict in ("CLEAN", "FINDINGS"):
            e = events("/repo")
            e[-1]["result"]["response"] = "Inspection complete.\nALIGN_REVIEW_VERDICT=" + verdict
            self.assertEqual(PARSER.validate(e, "/repo")[0], "fresh-review-1")

    def test_invalid_terminal_results(self):
        cases = [("status", value) for value in (
            "ERROR", "CANCELED", "INTERRUPTED", "INVALID", "WAITING", "RUNNING", None)]
        cases += [("num_turns", value) for value in (0, 2, True, "1", None)]
        cases += [("error", "failed"), ("denied_actions", ["view_file"]),
                  ("response", ""), ("conversation_id", "other")]
        for key, value in cases:
            with self.subTest(key=key, value=value):
                e = events("/repo")
                e[-1]["result"][key] = value
                with self.assertRaises(ValueError): PARSER.validate(e, "/repo")

    def test_invalid_initial_identity(self):
        for key, value in (("model", "gemini-3.8-flash-medium"), ("agent", "default"),
                           ("cwd", "/other"), ("permission_mode", "always-proceed")):
            with self.subTest(key=key):
                e = events("/repo")
                e[0]["init"][key] = value
                with self.assertRaises(ValueError): PARSER.validate(e, "/repo")

    def test_stream_shape(self):
        original = events("/repo")
        for e in ([], original[:-1], original[1:], original[::-1],
                  original + [original[-1]], [original[0]] + original,
                  original[:2] + [{"event": "unknown"}] + original[2:]):
            with self.subTest(events=e), self.assertRaises(ValueError): PARSER.validate(e, "/repo")
        with self.assertRaises(ValueError):
            json.loads('{"status":"ERROR","status":"SUCCESS"}', object_pairs_hook=PARSER.unique_object)

    def test_tool_authority(self):
        for tool in ("run_command", "write_to_file", "call_mcp_tool", "invoke_subagent"):
            e = events("/repo")
            e[1]["step_update"]["tool_name"] = tool
            with self.subTest(tool=tool), self.assertRaises(ValueError): PARSER.validate(e, "/repo")
        for field, value in (("conversation_id", "other"), ("subagent_info", {}), ("state", "ERROR"),
                             ("tool_info", {"name": "run_command"}),
                             ("tool_info", {"error": {"type": "permission_denied"}})):
            e = events("/repo")
            e[1]["step_update"][field] = value
            with self.subTest(field=field), self.assertRaises(ValueError): PARSER.validate(e, "/repo")

    def test_verdict_syntax(self):
        marker = "ALIGN_REVIEW_VERDICT=CLEAN"
        for response in ("No findings.", marker + "\nstill reviewing", marker + "\n" + marker,
                         "ALIGN_REVIEW_VERDICT=INCOMPLETE", "```\n" + marker,
                         "~~~text\n" + marker, "    " + marker, "> " + marker,
                         "````\n```\n" + marker, "```\n" + marker + "\n```"):
            e = events("/repo")
            e[-1]["result"]["response"] = response
            with self.subTest(response=response), self.assertRaises(ValueError): PARSER.validate(e, "/repo")
        e = events("/repo")
        e[-1]["result"]["response"] = "```python\nreturn 0\n```\n" + marker + "\n\n"
        PARSER.validate(e, "/repo")


class WrapperOwner(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="align-review-test-")
        self.addCleanup(self.temp.cleanup)
        self.parent = Path(self.temp.name).resolve()
        self.repo = fixture(self.parent)
        self.head = git(self.repo, "rev-parse", "HEAD")
        self.base = git(self.repo, "rev-parse", "main")
        self.bin = self.parent / "bin"
        self.bin.mkdir()
        for provider in ("agy", "codex"):
            path = self.bin / provider
            path.write_text(STUB)
            path.chmod(0o700)
        fake_git = self.bin / "git"
        fake_git.write_text("#!/usr/bin/env python3\nimport os,sys\nfrom pathlib import Path\n"
                            "if sys.argv[1:]==['status','--porcelain'] and "
                            "Path(os.environ['REVIEW_TEST_GIT_FAIL']).exists(): sys.exit(71)\n"
                            f"os.execv({REAL_GIT!r}, [{REAL_GIT!r}] + sys.argv[1:])\n")
        fake_git.chmod(0o700)
        self.env = dict(os.environ, PATH=str(self.bin) + os.pathsep + os.environ["PATH"],
                        REVIEW_TEST_ARGS=str(self.parent / "args"),
                        REVIEW_TEST_PID=str(self.parent / "child-pid"),
                        REVIEW_TEST_GIT_FAIL=str(self.parent / "git-fail"),
                        REVIEW_TEST_EVENTS=json.dumps(events(self.repo)),
                        ALIGN_REVIEW_STALL_SECONDS="10", ALIGN_REVIEW_PROGRESS_INTERVAL_SECONDS="1",
                        ALIGN_REVIEW_MAX_SECONDS="0")
        self.log = self.repo / ".git" / f"align-review-{self.head}.log"
        self.cycle = self.repo / ".git" / f"align-review-cycle-{self.head}"

    def run_review(self, mode="clean", provider="agy", args=(), cwd=None, expected=0):
        result = subprocess.run(["bash", str(WRAPPER), "--provider", provider, "--base", "main", *args],
                                cwd=cwd or self.repo, env=dict(self.env, REVIEW_TEST_MODE=mode),
                                text=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, timeout=20)
        self.assertEqual(result.returncode, expected, result.stdout)
        return result

    def assert_incomplete(self):
        for path in (self.log, self.cycle):
            self.assertTrue(path.read_text().rstrip().endswith("ALIGN_REVIEW_VERDICT=INCOMPLETE"), path)

    def test_launch_and_binding(self):
        self.run_review()
        argv = json.loads((self.parent / "args").read_text())
        for flag, value in (("--model", "gemini-3.8-flash-high"), ("--agent", "align-reviewer"),
                            ("--effort", "high"), ("--output-format", "stream-json"),
                            ("--print-timeout", "2562047h")):
            self.assertEqual(argv[argv.index(flag) + 1], value)
        for forbidden in ("--continue", "-c", "--conversation", "--dangerously-skip-permissions"):
            self.assertNotIn(forbidden, argv)
        self.assertIn("--disable-slash-commands", argv)
        self.assertIn("--sandbox", argv)
        log = self.log.read_text()
        self.assertIn(f"ALIGN_REVIEW_BASE={self.base}", log)
        self.assertIn(f"ALIGN_REVIEW_HEAD={self.head}", log)
        directory = Path(next(line.split("=", 1)[1] for line in log.splitlines()
                              if line.startswith("ALIGN_REVIEW_EVIDENCE=")))
        patch = (directory / "diff.patch").read_text()
        self.assertIn("-    return seconds * 1000", patch)
        self.assertIn("+    return seconds", patch)
        self.assertEqual(os.readlink(self.repo / "AGENTS.md"), "CLAUDE.md")

    def test_failed_runs_never_attest(self):
        for provider in ("agy", "codex"):
            with self.subTest(provider=provider):
                self.run_review("failed-clean", provider, expected=7)
                self.assert_incomplete()
        for mode in ("error", "partial", "stderr"):
            with self.subTest(mode=mode):
                self.run_review(mode, expected=3)
                self.assert_incomplete()

    def test_codex_native_result_and_findings(self):
        self.run_review("native", "codex")
        self.run_review("findings", "codex", expected=2)
        self.run_review("trailing", "codex", expected=3)
        self.assert_incomplete()
        self.run_review("findings", expected=2)

    def test_git_status_failure(self):
        self.run_review("git-failure", expected=71)
        self.assert_incomplete()
        (self.parent / "args").unlink()
        self.run_review(expected=71)
        self.assertFalse((self.parent / "args").exists())

    def test_head_drift(self):
        self.run_review("head", expected=1)
        self.assert_incomplete()

    def test_base_drift(self):
        self.run_review("base", expected=1)
        self.assert_incomplete()

    def test_worktree_drift(self):
        self.run_review("dirty", expected=1)
        self.assert_incomplete()
        self.assertTrue((self.repo / "unexpected.txt").exists())

    def test_dirty_work_is_preserved(self):
        (self.repo / "uncommitted.txt").write_text("preserve me")
        self.run_review(expected=1)
        self.assertEqual((self.repo / "uncommitted.txt").read_text(), "preserve me")
        self.assertFalse((self.parent / "args").exists())

    def advance(self):
        with (self.repo / "HANDOFF.md").open("a") as file: file.write("A coherent finding fix.\n")
        git(self.repo, "commit", "-qam", "fix: close findings")

    def test_cross_provider_review_cycle(self):
        self.run_review()
        self.advance()
        self.run_review(provider="codex", expected=1)
        self.run_review(provider="codex", args=("--changed-since", self.head))
        argv = json.loads((self.parent / "args").read_text())
        self.assertIn(f"git diff {self.head}..{git(self.repo, 'rev-parse', 'HEAD')}", argv[-1])

    def test_incomplete_ancestor_cannot_authorize_slice(self):
        self.run_review("failed-clean", expected=7)
        self.advance()
        self.run_review(provider="codex", args=("--changed-since", self.head), expected=1)

    def test_subdirectory_and_advancing_base(self):
        git(self.repo, "switch", "-q", "main")
        (self.repo / "unrelated.md").write_text("unrelated")
        git(self.repo, "add", "unrelated.md")
        git(self.repo, "commit", "-qm", "advance main")
        git(self.repo, "switch", "-q", "candidate")
        self.run_review(cwd=self.repo / ".agents")
        self.assertIn(f"ALIGN_REVIEW_BASE={self.base}", self.log.read_text())

    def test_linked_worktree(self):
        linked = self.parent / "linked"
        git(self.repo, "worktree", "add", "-qb", "linked", str(linked))
        self.env["REVIEW_TEST_EVENTS"] = json.dumps(events(linked))
        self.run_review(cwd=linked)
        path = Path(git(linked, "rev-parse", "--git-path", f"align-review-{self.head}.log"))
        self.assertTrue(path.read_text().rstrip().endswith("ALIGN_REVIEW_VERDICT=CLEAN"))
        self.assertEqual(git(linked, "status", "--porcelain"), "")

    def test_caller_relative_output(self):
        for provider in ("agy", "codex"):
            with self.subTest(provider=provider):
                self.run_review(provider=provider, cwd=self.repo / ".agents",
                                args=("--output", "../.git/custom-review.log"))
                log = (self.repo / ".git/custom-review.log").read_text()
                self.assertIn(f"ALIGN_REVIEW_HEAD={self.head}", log)
                self.assertTrue(log.rstrip().endswith("ALIGN_REVIEW_VERDICT=CLEAN"))
                self.assertEqual(git(self.repo, "status", "--porcelain"), "")

    def child_running(self):
        pid = int((self.parent / "child-pid").read_text())
        result = subprocess.run(["ps", "-p", str(pid), "-o", "stat="], text=True,
                                stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)
        return bool(result.stdout.strip()) and not result.stdout.lstrip().startswith("Z")

    def test_term_resistant_helper_after_success(self):
        self.run_review("resistant-success")
        self.assertFalse(self.child_running())

    def test_stall_and_explicit_maximum(self):
        self.env["ALIGN_REVIEW_STALL_SECONDS"] = "2"
        self.run_review("resistant-stall", expected=124)
        self.assert_incomplete()
        self.assertFalse(self.child_running())
        (self.parent / "child-pid").unlink()
        self.env.update(ALIGN_REVIEW_STALL_SECONDS="60", ALIGN_REVIEW_MAX_SECONDS="1",
                        ALIGN_REVIEW_PROGRESS_INTERVAL_SECONDS="30")
        started = time.monotonic()
        self.run_review("resistant-stall", expected=124)
        self.assertLess(time.monotonic() - started, 12)
        self.assertFalse(self.child_running())
        self.assert_incomplete()

    def test_progress(self):
        self.env["ALIGN_REVIEW_STALL_SECONDS"] = "3"
        self.run_review("progress", "codex")

    def test_signals_and_concurrent_run(self):
        for sig in (signal.SIGINT, signal.SIGTERM, signal.SIGHUP):
            with self.subTest(signal=sig):
                pid_file = self.parent / "child-pid"
                if pid_file.exists(): pid_file.unlink()
                with (self.parent / "signal-log").open("w") as output:
                    process = subprocess.Popen(["bash", str(WRAPPER), "--provider", "agy", "--base", "main"],
                        cwd=self.repo, env=dict(self.env, REVIEW_TEST_MODE="resistant-stall"),
                        stdout=output, stderr=subprocess.STDOUT, start_new_session=True)
                    try:
                        deadline = time.monotonic() + 10
                        while not pid_file.exists() and process.poll() is None and time.monotonic() < deadline:
                            time.sleep(.05)
                        self.assertTrue(pid_file.exists(), (self.parent / "signal-log").read_text())
                        self.run_review(provider="codex", expected=1)
                        process.send_signal(sig)
                        self.assertEqual(process.wait(timeout=10), 128 + sig)
                        self.assertFalse(self.child_running())
                        self.assert_incomplete()
                    finally:
                        if process.poll() is None: process.kill(); process.wait()
                        if pid_file.exists() and self.child_running():
                            os.killpg(os.getpgid(int(pid_file.read_text())), signal.SIGKILL)


def live():
    common = Path(git(ROOT, "rev-parse", "--git-common-dir"))
    if not common.is_absolute(): common = ROOT / common
    parent = Path(tempfile.mkdtemp(prefix="align-agy-live-", dir=common)).resolve()
    repo = fixture(parent)
    print(f"Live agy qualification evidence: {parent}", flush=True)
    result = subprocess.run(["bash", str(WRAPPER), "--provider", "agy", "--base", "main"], cwd=repo)
    require_live = result.returncode == 2 and git(repo, "status", "--porcelain") == ""
    head = git(repo, "rev-parse", "HEAD")
    log = (repo / ".git" / f"align-review-{head}.log").read_text()
    require_live = require_live and "sample.py" in log and "ALIGN_REVIEW_CONVERSATION=" in log
    if not require_live:
        raise SystemExit(f"Live agy qualification failed (exit {result.returncode}); evidence preserved at {parent}")
    print("Live agy qualification PASS: fresh high-model review found the committed conversion defect; source unchanged.")


if __name__ == "__main__":
    if sys.argv[1:] == ["--live"]:
        live()
    else:
        unittest.main()
