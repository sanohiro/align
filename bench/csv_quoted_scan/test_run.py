#!/usr/bin/env python3
"""Actual-process owners for all CSV benchmark phases and scratch cleanup."""
import os
from pathlib import Path
import shutil
import signal
import socket
import subprocess
import sys
import tempfile
import time
import unittest


HERE = Path(__file__).resolve().parent


class RunnerOwnership(unittest.TestCase):
    def test_phase_lifecycle(self):
        for phase in ("build", "link", "probe"):
            for ending in ("timeout", "exited-leader", "HUP", "INT", "TERM"):
                with self.subTest(phase=phase, ending=ending), tempfile.TemporaryDirectory(
                    prefix="align-csv-owner-"
                ) as temporary:
                    self.exercise(Path(temporary), phase, ending)

    def exercise(self, root, phase, ending):
        directory = root / "bench/csv_quoted_scan"
        directory.mkdir(parents=True)
        shutil.copy(HERE / "run.py", directory / "run.py")
        scratch = root / "scratch"
        scratch.mkdir()
        marker = root / "leader"
        address = root / "socket"
        helper = root / "tool"
        helper.write_text(
            f"#!{sys.executable}\n"
            "import os, pathlib, signal, socket, sys, time\n"
            "phase = 'link' if '-o' in sys.argv else ('build' if len(sys.argv) > 1 else 'probe')\n"
            "if phase == 'link':\n"
            "    output = pathlib.Path(sys.argv[sys.argv.index('-o') + 1])\n"
            "    output.write_bytes(pathlib.Path(__file__).read_bytes())\n"
            "    output.chmod(0o700)\n"
            "if phase == os.environ['OWNER_PHASE']:\n"
            "    signal.signal(signal.SIGTERM, signal.SIG_IGN)\n"
            "    signal.signal(signal.SIGINT, signal.SIG_IGN)\n"
            "    signal.signal(signal.SIGHUP, signal.SIG_IGN)\n"
            "    reader, writer = os.pipe()\n"
            "    if os.fork() == 0:\n"
            "        os.close(reader)\n"
            "        witness = socket.socket(socket.AF_UNIX)\n"
            "        witness.connect(os.environ['OWNER_SOCKET'])\n"
            "        os.write(writer, b'R')\n"
            "        os.close(writer)\n"
            "        time.sleep(60)\n"
            "        os._exit(0)\n"
            "    os.close(writer)\n"
            "    assert os.read(reader, 1) == b'R'\n"
            "    pathlib.Path(os.environ['OWNER_MARKER']).write_text(str(os.getpid()))\n"
            "    if os.environ['OWNER_ENDING'] != 'exited-leader': time.sleep(60)\n"
        )
        helper.chmod(0o700)
        (root / "scripts").mkdir()
        (root / "scripts/cargo.sh").write_text('exec "$CC" build\n')
        env = dict(os.environ, CC=str(helper), TMPDIR=str(scratch), OWNER_PHASE=phase,
                   OWNER_ENDING=ending, OWNER_SOCKET=str(address), OWNER_MARKER=str(marker))
        # Exercise the actual runner, shortening only its fixed phase budgets.
        bootstrap = ("import run; run.LIMITS = dict.fromkeys(run.LIMITS, 1); run.main()")
        with socket.socket(socket.AF_UNIX) as listener:
            listener.bind(str(address))
            listener.listen(1)
            listener.settimeout(4)
            child = subprocess.Popen([sys.executable, "-B", "-c", bootstrap], cwd=directory,
                                     env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                     start_new_session=True)
            retired = False
            try:
                connection, _ = listener.accept()
                with connection:
                    connection.settimeout(4)
                    deadline = time.monotonic() + 2
                    while not marker.exists() and time.monotonic() < deadline:
                        time.sleep(0.01)
                    self.assertTrue(marker.exists(), "fixture did not publish its leader")
                    if ending in ("HUP", "INT", "TERM"):
                        child.send_signal(getattr(signal, "SIG" + ending))
                    stdout, stderr = child.communicate(timeout=4)
                    if ending == "exited-leader":
                        self.assertEqual(child.returncode, 0, stderr)
                    elif ending == "timeout":
                        self.assertNotEqual(child.returncode, 0)
                        self.assertIn(f"{phase} exceeded".encode(), stderr)
                    else:
                        self.assertEqual(child.returncode, 128 + getattr(signal, "SIG" + ending))
                    # EOF proves the descendant exited, rather than merely
                    # observing that its leader was reaped or a signal was sent.
                    self.assertEqual(connection.recv(1), b"")
                    retired = True
                    self.assertEqual(list(scratch.iterdir()), [])
                    self.assertEqual(stdout, b"")
            finally:
                # Bound negative controls even if the runner loses ownership.
                pids = [int(marker.read_text())] if marker.exists() and not retired else []
                if child.poll() is None:
                    pids.append(child.pid)
                for pid in pids:
                    try:
                        os.killpg(pid, signal.SIGKILL)
                    except (ProcessLookupError, PermissionError):
                        pass
                child.communicate(timeout=4)


if __name__ == "__main__":
    unittest.main()
