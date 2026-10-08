#!/usr/bin/env python3
"""Actual-process owners for native probe wrappers, phases and cleanup."""
import os
import importlib.util
import shlex
from pathlib import Path
import shutil
import signal
import socket
import subprocess
import sys
import tempfile
import time
import unittest
from unittest.mock import patch

import native_probe


HERE = Path(__file__).resolve().parent


class RunnerOwnership(unittest.TestCase):
    def test_rejects_unknown_probe_before_work(self):
        for arguments in ([], ["unknown"], ["../utf8_lossy"], ["utf8_lossy", "extra"]):
            with self.subTest(arguments=arguments), patch.object(sys, "argv", ["probe", *arguments]), \
                    patch.object(native_probe.subprocess, "Popen") as spawn, \
                    patch.object(native_probe.tempfile, "mkdtemp") as acquire:
                with self.assertRaises(SystemExit):
                    native_probe.main()
                spawn.assert_not_called()
                acquire.assert_not_called()

    def test_refreshes_stale_published_runtime_without_changing_source(self):
        cargo = shutil.which("cargo")
        cc = shutil.which("cc")
        ar = shutil.which("ar")
        self.assertTrue(cargo and cc and ar, "the repository Rust/native toolchain is required")
        with tempfile.TemporaryDirectory(prefix="align-probe-producer-") as temporary:
            root = Path(temporary)
            runtime = root / "crates/align_runtime/src/lib.rs"
            runtime.parent.mkdir(parents=True)
            source = '#[unsafe(no_mangle)] pub extern "C" fn producer() -> i32 { 11 }\n'
            runtime.write_text(source)
            (root / "Cargo.toml").write_text(
                '[package]\nname = "align_runtime"\nversion = "0.0.0"\nedition = "2024"\n'
                '[lib]\npath = "crates/align_runtime/src/lib.rs"\ncrate-type = ["staticlib"]\n')
            (root / "scripts").mkdir()
            (root / "scripts/cargo.sh").write_text(f'exec {shlex.quote(cargo)} "$@"\n')
            directory = root / "bench/base64_decode"
            directory.mkdir(parents=True)
            (directory / "main.c").write_text('extern int producer(void); int main(void) { return producer() == 11 ? 0 : 2; }\n')
            runner = root / "bench/native_probe.py"
            shutil.copy(HERE / "native_probe.py", runner)
            spec = importlib.util.spec_from_file_location("owned_producer_probe", runner)
            module = importlib.util.module_from_spec(spec)
            spec.loader.exec_module(module)
            module.LIMITS = dict.fromkeys(module.LIMITS, 30)
            previous = Path.cwd()
            try:
                os.chdir(root)
                with patch.dict(os.environ, CARGO_TARGET_DIR=str(root / "target"), CC=cc), \
                        patch.object(sys, "argv", [str(runner), "base64_decode"]), \
                        patch.object(module.signal, "signal"):
                    module.main()
                    # Simulate another producer occupying the shared top-level archive
                    # while Cargo's successful fingerprint for this source stays intact.
                    (root / "other.c").write_text('int producer(void) { return 22; }\n')
                    module.run_phase("link", [cc, "-c", "other.c", "-o", "other.o"])
                    module.run_phase("link", [ar, "rcs", "other.a", "other.o"])
                    archives = [root / "target/release/libalign_runtime.a"]
                    cached = list((root / "target/release/deps").glob("libalign_runtime-*.a"))
                    self.assertTrue(cached, "Cargo must publish its fingerprinted archive")
                    for archive in [*archives, *cached]:
                        shutil.copyfile(root / "other.a", archive)
                    module.main()
                    self.assertEqual(runtime.read_text(), source)
            finally:
                os.chdir(previous)

    def test_phase_lifecycle(self):
        for benchmark in native_probe.PROBES:
            for phase in ("build", "link", "probe"):
                for ending in ("timeout", "exited-leader", "HUP", "INT", "TERM"):
                    with self.subTest(benchmark=benchmark, phase=phase, ending=ending), \
                            tempfile.TemporaryDirectory(prefix="align-probe-owner-") as temporary:
                        self.exercise(Path(temporary), benchmark, phase, ending)

    def exercise(self, root, benchmark, phase, ending):
        directory = root / "bench" / benchmark
        directory.mkdir(parents=True)
        shutil.copy(HERE / "native_probe.py", root / "bench/native_probe.py")
        shutil.copy(HERE / benchmark / "run.sh", directory / "run.sh")
        (directory / "main.c").touch()
        runtime = root / "crates/align_runtime/src/lib.rs"
        runtime.parent.mkdir(parents=True)
        runtime.write_text("// owned runtime fixture\n")
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
            "    assert pathlib.Path('bench/' + os.environ['OWNER_BENCH'] + '/main.c').is_file()\n"
            "    assert 'bench/' + os.environ['OWNER_BENCH'] + '/main.c' in sys.argv\n"
            "    archives = [pathlib.Path(arg).resolve() for arg in sys.argv if arg.endswith('/libalign_runtime.a')]\n"
            "    assert archives == [pathlib.Path('target/release/libalign_runtime.a').resolve()]\n"
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
            "    pathlib.Path(os.environ['OWNER_MARKER']).write_text(str(os.getpgrp()))\n"
            "    if os.environ['OWNER_ENDING'] != 'exited-leader': time.sleep(60)\n"
        )
        helper.chmod(0o700)
        (root / "scripts").mkdir()
        (root / "scripts/cargo.sh").write_text('exec "${CC:-cc}" build\n')
        (root / "bin").mkdir()
        (root / "bin/cc").symlink_to(helper)
        # The real shell wrapper dispatches to this interpreter shim. Only
        # phase budgets change; argument forwarding and source selection stay real.
        interpreter = root / "bin/python3"
        interpreter.write_text(
            f"#!{sys.executable}\n"
            "import pathlib, sys\n"
            "sys.dont_write_bytecode = True\n"
            "sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[1] / 'bench'))\n"
            "import native_probe\n"
            "native_probe.LIMITS = dict.fromkeys(native_probe.LIMITS, 1)\n"
            "sys.argv = sys.argv[2:]\n"
            "native_probe.main()\n"
        )
        interpreter.chmod(0o700)
        env = dict(os.environ, CC="" if ending == "exited-leader" else str(helper),
                   CARGO_TARGET_DIR="", TMPDIR=str(scratch),
                   PATH=str(root / "bin") + os.pathsep + os.environ["PATH"],
                   OWNER_PHASE=phase, OWNER_BENCH=benchmark,
                   OWNER_ENDING=ending, OWNER_SOCKET=str(address), OWNER_MARKER=str(marker))
        with socket.socket(socket.AF_UNIX) as listener:
            listener.bind(str(address))
            listener.listen(1)
            listener.settimeout(4)
            child = subprocess.Popen(["bash", str(directory / "run.sh")], cwd=root,
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
