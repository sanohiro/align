#!/usr/bin/env python3
"""Restore tracked-file mtimes from Git history so Cargo's fingerprint cache
survives a fresh checkout (actions/checkout stamps every file with `now`)."""

import argparse
import os
import subprocess
import sys
import time


def run(args):
    return subprocess.run(args, stdout=subprocess.PIPE, stderr=subprocess.PIPE)


def tracked_files():
    proc = run(["git", "ls-files", "-z"])
    if proc.returncode != 0:
        sys.stderr.write(proc.stderr.decode("utf-8", "replace"))
        sys.exit(1)
    text = proc.stdout.decode("utf-8", "surrogateescape")
    return {p for p in text.split("\x00") if p}


def set_mtime(path, when):
    """Set path's own mtime (never a symlink target's) if it still exists."""
    try:
        os.utime(path, (when, when), follow_symlinks=False)
        return True
    except NotImplementedError:
        # A platform without lutimes(): fall back to following the symlink.
        try:
            os.utime(path, (when, when))
            return True
        except OSError:
            return False
    except OSError:
        return False


def restore_from_history(tracked):
    """Assign each tracked path the commit time of the most recent commit
    that touched it, stopping the log walk as soon as every path is found."""
    remaining = set(tracked)
    assigned = {}
    proc = subprocess.Popen(
        ["git", "log", "--pretty=format:%x00%ct", "--name-only"],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    ct = None
    try:
        for raw in proc.stdout:
            line = raw.decode("utf-8", "surrogateescape").rstrip("\n")
            if line.startswith("\x00"):
                ct = int(line[1:])
                continue
            path = line
            if not path or ct is None:
                continue
            if path in remaining:
                assigned[path] = ct
                remaining.discard(path)
                if not remaining:
                    break
    finally:
        proc.stdout.close()
        if proc.poll() is None:
            proc.terminate()
        stderr = proc.stderr.read()
        proc.wait()
        proc.stderr.close()

    # A positive exit code is a genuine git failure. A path left unassigned
    # because we stopped the walk early (or because it truly has no history,
    # which cannot happen for a committed tracked file) is not an error by
    # itself; only pair it with a real failed exit.
    if remaining and proc.returncode and proc.returncode > 0:
        sys.stderr.write(stderr.decode("utf-8", "replace"))
        sys.exit(1)

    restored = 0
    for path, when in assigned.items():
        if set_mtime(path, when):
            restored += 1
    return restored


def touch_changed(base, head):
    """Set mtime = now for every path the change touched, so a file whose
    commit predates the cached build (an old author date, a rebase) is never
    mistaken for unchanged."""
    proc = run(["git", "diff", "--no-renames", "--name-only", f"{base}...{head}"])
    if proc.returncode != 0:
        proc = run(["git", "diff", "--no-renames", "--name-only", f"{base}..{head}"])
        if proc.returncode != 0:
            sys.stderr.write(proc.stderr.decode("utf-8", "replace"))
            sys.exit(1)
    now = time.time()
    touched = 0
    for path in proc.stdout.decode("utf-8", "surrogateescape").splitlines():
        if path and set_mtime(path, now):
            touched += 1
    return touched


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--changed", nargs=2, metavar=("BASE", "HEAD"))
    args = parser.parse_args()

    inside = run(["git", "rev-parse", "--is-inside-work-tree"])
    if inside.returncode != 0 or inside.stdout.decode().strip() != "true":
        sys.stderr.write("restore-mtime: not inside a git worktree\n")
        sys.exit(1)

    tracked = tracked_files()
    restored = restore_from_history(tracked)

    touched = 0
    if args.changed:
        touched = touch_changed(args.changed[0], args.changed[1])

    print(f"restore-mtime: {restored} files restored, {touched} changed files touched")


if __name__ == "__main__":
    main()
