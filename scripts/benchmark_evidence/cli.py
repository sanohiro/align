"""Strict command-line boundary for the benchmark evidence controller.

This module only parses the three reviewed public invocations.  It does not
open repositories or input files, resolve revisions, import installed code,
run children, or select ambient configuration.  Those operations belong to
the trusted controller and verifier layers that consume these immutable
invocation records.
"""

from __future__ import annotations

import os
import re
from dataclasses import dataclass
from typing import Sequence


class CliError(ValueError):
    """A command-line invocation violates the evidence boundary."""


_OID = re.compile(r"[0-9a-f]{40}\Z")

_RUN_OPTIONS = (
    "--repository",
    "--baseline",
    "--candidate",
    "--review-log",
    "--output-dir",
)
_VERIFY_OPTIONS = (
    "--report",
    "--signature",
    "--expected-baseline",
    "--expected-candidate",
    "--pr-body",
    "--review-attestation",
)
_VERIFY_MERGE_OPTIONS = (
    "--repository",
    "--report",
    "--signature",
    "--merge",
    "--output-dir",
)


@dataclass(frozen=True)
class RunInvocation:
    """The validated arguments for the producer's ``run`` mode."""

    repository: str
    baseline: str
    candidate: str
    review_log: str
    output_dir: str


@dataclass(frozen=True)
class VerifyInvocation:
    """The validated arguments for the diagnostic/CI ``verify`` mode."""

    report: str
    signature: str
    expected_baseline: str
    expected_candidate: str
    pr_body: str
    review_attestation: str


@dataclass(frozen=True)
class VerifyMergeInvocation:
    """The validated arguments for the post-merge verification mode."""

    repository: str
    report: str
    signature: str
    merge: str
    output_dir: str


Invocation = RunInvocation | VerifyInvocation | VerifyMergeInvocation


def _absolute_path(option: str, value: str) -> str:
    if "\x00" in value or not os.path.isabs(value):
        raise CliError(f"{option} must be an absolute path")
    return value


def _oid(option: str, value: str) -> str:
    if _OID.fullmatch(value) is None:
        raise CliError(f"{option} must be a lowercase 40-hex object ID")
    return value


def _new_output_dir(option: str, value: str) -> str:
    value = _absolute_path(option, value)
    if os.path.lexists(value):
        raise CliError(f"{option} must not already exist")
    return value


def _options(argv: Sequence[str], allowed: tuple[str, ...]) -> dict[str, str]:
    if not argv:
        raise CliError("a command mode is required")
    if any(not isinstance(token, str) for token in argv):
        raise CliError("arguments must be strings")

    values: dict[str, str] = {}
    index = 0
    while index < len(argv):
        option = argv[index]
        if option not in allowed:
            raise CliError(f"unknown or misplaced option: {option}")
        if option in values:
            raise CliError(f"repeated option: {option}")
        if index + 1 == len(argv):
            raise CliError(f"missing value for {option}")
        value = argv[index + 1]
        if value.startswith("-"):
            raise CliError(f"missing value for {option}")
        values[option] = value
        index += 2

    if set(values) != set(allowed):
        missing = [option for option in allowed if option not in values]
        raise CliError(f"missing option: {missing[0]}")
    return values


def parse(argv: Sequence[str]) -> Invocation:
    """Parse one exact public invocation without consulting ambient state."""

    if not argv or not isinstance(argv[0], str):
        raise CliError("a command mode is required")
    mode = argv[0]
    values = argv[1:]

    if mode == "run":
        parsed = _options(values, _RUN_OPTIONS)
        baseline = _oid("--baseline", parsed["--baseline"])
        candidate = _oid("--candidate", parsed["--candidate"])
        if baseline == candidate:
            raise CliError("--baseline and --candidate must differ")
        return RunInvocation(
            repository=_absolute_path("--repository", parsed["--repository"]),
            baseline=baseline,
            candidate=candidate,
            review_log=_absolute_path("--review-log", parsed["--review-log"]),
            output_dir=_new_output_dir("--output-dir", parsed["--output-dir"]),
        )

    if mode == "verify":
        parsed = _options(values, _VERIFY_OPTIONS)
        expected_baseline = _oid("--expected-baseline", parsed["--expected-baseline"])
        expected_candidate = _oid("--expected-candidate", parsed["--expected-candidate"])
        if expected_baseline == expected_candidate:
            raise CliError("--expected-baseline and --expected-candidate must differ")
        return VerifyInvocation(
            report=_absolute_path("--report", parsed["--report"]),
            signature=_absolute_path("--signature", parsed["--signature"]),
            expected_baseline=expected_baseline,
            expected_candidate=expected_candidate,
            pr_body=_absolute_path("--pr-body", parsed["--pr-body"]),
            review_attestation=_absolute_path(
                "--review-attestation",
                parsed["--review-attestation"],
            ),
        )

    if mode == "verify-merge":
        parsed = _options(values, _VERIFY_MERGE_OPTIONS)
        return VerifyMergeInvocation(
            repository=_absolute_path("--repository", parsed["--repository"]),
            report=_absolute_path("--report", parsed["--report"]),
            signature=_absolute_path("--signature", parsed["--signature"]),
            merge=_oid("--merge", parsed["--merge"]),
            output_dir=_new_output_dir("--output-dir", parsed["--output-dir"]),
        )

    raise CliError(f"unknown command mode: {mode}")
