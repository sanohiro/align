#!/usr/bin/env python3
"""Actual-process regression for benchmark descendant cleanup after leader exit."""
import errno
import os
from pathlib import Path
import signal
import socket
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

import measure


class ProcessOwnership(unittest.TestCase):
    def test_exited_leader_retires_descendants_and_reaps(self):
        for keep_pipes in (False, True):
            with self.subTest(keep_pipes=keep_pipes), tempfile.TemporaryDirectory(
                prefix="align-bench-owner-"
            ) as temporary:
                directory = Path(temporary)
                address = directory / "socket"
                helper = directory / "helper.py"
                helper.write_text(
                    "import os, socket, sys, time\n"
                    "reader, writer = os.pipe()\n"
                    "if os.fork() == 0:\n"
                    "    os.close(reader)\n"
                    "    witness = socket.socket(socket.AF_UNIX)\n"
                    "    witness.connect(sys.argv[1])\n"
                    + ("" if keep_pipes else "    os.close(1)\n    os.close(2)\n")
                    + "    os.write(writer, b'R')\n"
                    "    os.close(writer)\n"
                    "    time.sleep(60)\n"
                    "    os._exit(0)\n"
                    "os.close(writer)\n"
                    "assert os.read(reader, 1) == b'R'\n"
                    "os._exit(0)\n"
                )
                children = []
                original_popen = subprocess.Popen

                def spawn(*args, **kwargs):
                    child = original_popen(*args, **kwargs)
                    children.append(child)
                    return child

                with socket.socket(socket.AF_UNIX) as listener:
                    listener.bind(str(address))
                    listener.listen(1)
                    listener.settimeout(2)
                    try:
                        with patch.object(measure.subprocess, "Popen", spawn), patch.object(
                            measure, "WORK_TIMEOUT_SECONDS", 0.25
                        ):
                            if keep_pipes:
                                with self.assertRaisesRegex(RuntimeError, "budget"):
                                    measure.run(Path(sys.executable), str(helper), address)
                            else:
                                result = measure.run(Path(sys.executable), str(helper), address)
                                self.assertEqual(result[:3], (0, b"", b""))
                        self.assertEqual(len(children), 1)
                        # The descendant owns the only connected socket. EOF proves
                        # actual exit, including when the leader has already exited.
                        connection, _ = listener.accept()
                        with connection:
                            connection.settimeout(2)
                            self.assertEqual(connection.recv(1), b"")
                        with self.assertRaises(OSError) as caught:
                            os.waitpid(children[0].pid, os.WNOHANG)
                        self.assertEqual(caught.exception.errno, errno.ECHILD)
                    finally:
                        # Keep the negative control bounded when the old bug returns.
                        for child in children:
                            try:
                                os.killpg(child.pid, signal.SIGKILL)
                            except ProcessLookupError:
                                pass
                            child.communicate(timeout=2)


if __name__ == "__main__":
    unittest.main()
