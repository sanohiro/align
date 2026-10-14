#!/usr/bin/env python3
"""Actual-process lifecycle owner for the JSON key measurement entrypoint."""
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
MEASURE = Path(os.environ.get("ALIGN_KEYS_MEASURE_TEST_RUNNER", HERE / "measure.py"))


class MeasurementOwnership(unittest.TestCase):
    def test_compilation_and_measurement_lifecycle(self):
        for phase in ("build", "measure"):
            for ending in ("HUP", "INT", "TERM", "exited-leader", "failure"):
                with self.subTest(phase=phase, ending=ending), \
                        tempfile.TemporaryDirectory(prefix="align-keys-owner-", dir="/tmp") as temporary:
                    self.exercise(Path(temporary), phase, ending)

    def exercise(self, root, phase, ending):
        # Keep the AF_UNIX path within the native platform's small length limit.
        directory = root / "bench/keys"
        directory.mkdir(parents=True)
        shutil.copy(MEASURE, directory / "measure.py")
        shutil.copy(HERE.parent / "native_probe.py", root / "bench/native_probe.py")
        for name in ("key.align", "keys.align"):
            shutil.copy(HERE / name, directory / name)
        scratch = root / "scratch"
        scratch.mkdir()
        marker, address, output = root / "leader", root / "socket", root / "samples.json"
        helper = root / "alignc"
        helper.write_text(
            f"#!{sys.executable}\n"
            "import json, os, pathlib, signal, socket, sys, time\n"
            "phase = 'build' if sys.argv[1] == 'build' else 'measure'\n"
            "if phase == 'build':\n"
            "    output = pathlib.Path(pathlib.Path(sys.argv[2]).stem)\n"
            "    output.write_bytes(pathlib.Path(__file__).read_bytes())\n"
            "    output.chmod(0o700)\n"
            "else:\n"
            "    data = json.loads(pathlib.Path(sys.argv[1]).read_text())\n"
            "    print(sum(len(key) for key in data), flush=True)\n"
            "    print(123, flush=True)\n"
            "marker = pathlib.Path(os.environ['OWNER_MARKER'])\n"
            "if phase == os.environ['OWNER_PHASE'] and not marker.exists():\n"
            "    for sig in (signal.SIGHUP, signal.SIGINT, signal.SIGTERM):\n"
            "        signal.signal(sig, signal.SIG_IGN)\n"
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
            "    os.close(reader)\n"
            "    marker.write_text(str(os.getpgrp()))\n"
            "    ending = os.environ['OWNER_ENDING']\n"
            "    if ending == 'failure': sys.exit(7)\n"
            "    if ending != 'exited-leader': time.sleep(60)\n"
        )
        helper.chmod(0o700)
        (root / "libalign_runtime.a").write_bytes(b"owned hash fixture")
        env = dict(os.environ, TMPDIR=str(scratch), OWNER_MARKER=str(marker),
                   OWNER_SOCKET=str(address), OWNER_PHASE=phase, OWNER_ENDING=ending)
        with socket.socket(socket.AF_UNIX) as listener:
            listener.bind(str(address))
            listener.listen(1)
            listener.settimeout(4)
            child = subprocess.Popen(
                [sys.executable, str(directory / "measure.py"), str(helper), str(output),
                 "--source-id", "owned-fixture", "--repeats", "1"],
                cwd=root, env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
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
                    _, stderr = child.communicate(timeout=8)
                    # EOF observes actual descendant retirement, not signal delivery.
                    self.assertEqual(connection.recv(1), b"")
                    retired = True
                    self.assertEqual(list(scratch.iterdir()), [])
                    if ending == "exited-leader":
                        self.assertEqual(child.returncode, 0, stderr)
                        self.assertTrue(output.is_file())
                    elif ending == "failure":
                        self.assertNotEqual(child.returncode, 0)
                        self.assertFalse(output.exists())
                    else:
                        self.assertEqual(child.returncode, 128 + getattr(signal, "SIG" + ending), stderr)
                        self.assertFalse(output.exists())
            finally:
                # Bound failed controls even when the runner loses its child group.
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
