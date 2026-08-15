#!/usr/bin/env bash
# Deterministic owner for the benchmark-evidence public CLI boundary.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd -P)"
cd "$REPO_ROOT"
export PYTHONDONTWRITEBYTECODE=1

python3 - "$REPO_ROOT" <<'PY'
import os
import sys
import tempfile

from scripts.benchmark_evidence import cli


OID = "0123456789abcdef0123456789abcdef01234567"
OTHER_OID = "fedcba9876543210fedcba9876543210fedcba98"


def expect_error(argv, fragment):
    try:
        cli.parse(argv)
    except cli.CliError as exc:
        assert fragment in str(exc), (argv, str(exc))
    else:
        raise AssertionError(f"accepted invalid invocation: {argv}")


def main():
    with tempfile.TemporaryDirectory() as root:
        root = os.path.abspath(root)
        output = os.path.join(root, "new-output")
        run = cli.parse(
            [
                "run",
                "--repository",
                os.path.join(root, "repo"),
                "--baseline",
                OID,
                "--candidate",
                OTHER_OID,
                "--review-log",
                os.path.join(root, "review.log"),
                "--output-dir",
                output,
            ]
        )
        assert isinstance(run, cli.RunInvocation)
        assert run.repository == os.path.join(root, "repo")
        assert run.baseline == OID
        assert run.candidate == OTHER_OID
        assert run.output_dir == output

        verify = cli.parse(
            [
                "verify",
                "--report",
                os.path.join(root, "report.json"),
                "--signature",
                os.path.join(root, "report.json.sig"),
                "--expected-baseline",
                OID,
                "--expected-candidate",
                OTHER_OID,
                "--pr-body",
                os.path.join(root, "pr-body.txt"),
                "--review-attestation",
                os.path.join(root, "review-attestation.json"),
            ]
        )
        assert isinstance(verify, cli.VerifyInvocation)
        assert verify.expected_baseline == OID
        assert verify.expected_candidate == OTHER_OID

        merge = cli.parse(
            [
                "verify-merge",
                "--repository",
                os.path.join(root, "repo"),
                "--report",
                os.path.join(root, "report.json"),
                "--signature",
                os.path.join(root, "report.json.sig"),
                "--merge",
                OID,
                "--output-dir",
                os.path.join(root, "merge-output"),
            ]
        )
        assert isinstance(merge, cli.VerifyMergeInvocation)
        assert merge.merge == OID

        existing = os.path.join(root, "existing")
        os.mkdir(existing)
        expect_error(
            [
                "run",
                "--repository",
                os.path.join(root, "repo"),
                "--baseline",
                OID,
                "--candidate",
                OTHER_OID,
                "--review-log",
                os.path.join(root, "review.log"),
                "--output-dir",
                existing,
            ],
            "must not already exist",
        )

        linked = os.path.join(root, "linked-output")
        os.symlink(os.path.join(root, "missing-target"), linked)
        expect_error(
            [
                "verify-merge",
                "--repository",
                os.path.join(root, "repo"),
                "--report",
                os.path.join(root, "report.json"),
                "--signature",
                os.path.join(root, "report.json.sig"),
                "--merge",
                OID,
                "--output-dir",
                linked,
            ],
            "must not already exist",
        )

    base_run = [
        "run",
        "--repository",
        "/repo",
        "--baseline",
        OID,
        "--candidate",
        OTHER_OID,
        "--review-log",
        "/review.log",
        "--output-dir",
        "/new-output",
    ]
    expect_error([], "command mode")
    expect_error(["unknown"], "unknown command mode")
    expect_error(base_run[:-1], "missing value")
    expect_error(base_run + ["--extra", "value"], "unknown or misplaced")
    expect_error(base_run[:3] + ["--repository", "/other"] + base_run[3:], "repeated option")
    expect_error(base_run[:3] + ["--repository=/repo"] + base_run[3:], "unknown or misplaced")
    expect_error(base_run[:3] + ["relative"] + base_run[4:], "unknown or misplaced")
    expect_error(
        [*base_run[:4], "not-an-oid", *base_run[5:]],
        "lowercase 40-hex",
    )
    expect_error(
        [*base_run[:4], OID, "--candidate", OID, *base_run[7:]],
        "must differ",
    )
    expect_error(
        [*base_run[:2], "relative", *base_run[3:]],
        "absolute path",
    )

    print("benchmark evidence CLI checks passed")


main()
PY
