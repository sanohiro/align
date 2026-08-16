"""Execute one fixed prepared benchmark child through the native Docker boundary.

This module is the first executable performance-measurement rail.  The trusted
controller still owns the schedule cursor, workspace lifecycle, host monitor,
resource ledger, and eventual report.  This adapter only builds one fixed
container argv, invokes the pinned Docker client once, retains bounded stream
metadata, and parses the benchmark's exact native output.
"""

from __future__ import annotations

import hashlib
import re
import time
from dataclasses import dataclass
from types import MappingProxyType
from typing import Any, Callable, Mapping, Sequence

from . import container
from . import native_host
from . import schedule


class NativeMeasurementError(RuntimeError):
    """A prepared benchmark child cannot cross the measurement boundary."""


_HEX64 = re.compile(r"[0-9a-f]{64}\Z")
_HEX_BYTES = re.compile(r"(?:[0-9a-f]{2})*\Z")
_UINT = re.compile(r"(?:0|[1-9][0-9]*)\Z")
_MILLISECONDS = re.compile(r"(?:0|[1-9][0-9]*)\.[0-9]{3}\Z")
_RATIO = re.compile(r"(?:0|[1-9][0-9]*)\.[0-9]{2}x\Z")
_U64_MAX = (1 << 64) - 1
_MAX_OUTPUT = 64 << 10
_STDERR_TAIL_BYTES = 4096
_NANOSECONDS_PER_SECOND = 1_000_000_000
_PHASE_TIMEOUT_KEYS = {
    "prepare": "prepare_ns",
    "warmup": "warmup_ns",
    "sample": "sample_ns",
}

BENCHMARKS = schedule.BENCHMARKS
REVISIONS = schedule.REVISIONS
PRODUCTS = tuple((benchmark, revision) for benchmark in BENCHMARKS for revision in REVISIONS)
PREPARED_PATH = "/work/prepared/artifact-manifest.json"

DECODE_TITLE = (
    "JSON decode throughput — Align json.decode vs serde_json "
    "(both fold where(.active).pay.sum())"
)
SOA_TITLE = (
    "JSON decode + where(.active).pay.sum() — Align soa / Align AoS / Align proj "
    "(narrow soa) vs serde_json"
)
DECODE_HEADER = "{:>9} {:>8} | {:>9} {:>9} {:>9} | {:>9} {:>9} {:>9}".format(
    "records",
    "json KB",
    "A-full",
    "rs-full",
    "full×",
    "A-proj",
    "rs-proj",
    "proj×",
)
SOA_HEADER = "{:>9}  {:>8}  {:>9}  {:>9}  {:>9}  {:>9}  {:>8}  {:>8}  {:>9}".format(
    "records",
    "json KB",
    "soa ms",
    "aos ms",
    "proj ms",
    "rust ms",
    "soa/rust",
    "aos/rust",
    "proj/rustP",
)
_TITLES = {"json_decode": DECODE_TITLE, "json_soa": SOA_TITLE}
_HEADERS = {"json_decode": DECODE_HEADER, "json_soa": SOA_HEADER}
_COMMANDS = {
    benchmark: f"/src/bench/{benchmark}/run.sh" for benchmark in BENCHMARKS
}
_SELECTED_FIELDS = {
    "json_decode": ("A-full", "A-proj"),
    "json_soa": ("soa ms", "aos ms", "proj ms"),
}

Runner = Callable[..., tuple[tuple[native_host.CommandCapture, ...], str]]
Clock = Callable[[], int]


def _error(message: str) -> None:
    raise NativeMeasurementError(message)


def _hash(value: object, label: str) -> str:
    if not isinstance(value, str) or _HEX64.fullmatch(value) is None:
        _error(f"{label} must be lowercase SHA-256")
    return value


def _path(value: object, label: str) -> str:
    try:
        return container._host_path(value, label)
    except container.ContainerError as exc:
        raise NativeMeasurementError(str(exc)) from exc


def _validate_workspace_paths(paths: Sequence[str]) -> None:
    if len(set(paths)) != len(paths):
        _error("workspace paths must be distinct")
    for index, left in enumerate(paths):
        for right in paths[index + 1 :]:
            if right.startswith(left + "/") or left.startswith(right + "/"):
                _error("workspace paths must not nest")


def _profile_uint(profile: Mapping[str, Any], section: str, key: str, label: str) -> int:
    value = profile.get(section)
    if not isinstance(value, Mapping):
        _error(f"profile.{section} must be a mapping")
    number = value.get(key)
    if type(number) is not int or not 0 < number <= _U64_MAX:
        _error(f"profile.{section}.{key} must be a positive u64")
    return number


def _execution_limits(profile: Mapping[str, Any], phase: str) -> tuple[float, int, int, int]:
    """Return the fixed phase timeout and separate stream/tail limits."""

    timeout_key = _PHASE_TIMEOUT_KEYS.get(phase)
    if timeout_key is None:
        _error("child phase has no profile timeout")
    timeout_ns = _profile_uint(profile, "phase_timeouts", timeout_key, timeout_key)
    stdout_limit = _profile_uint(profile, "capture_limits", "stdout_max_bytes", "stdout_max_bytes")
    stderr_limit = _profile_uint(profile, "capture_limits", "stderr_max_bytes", "stderr_max_bytes")
    tail_limit = _profile_uint(profile, "capture_limits", "stderr_tail_max_bytes", "stderr_tail_max_bytes")
    if stdout_limit > _MAX_OUTPUT:
        _error("profile capture limit exceeds the native host ceiling")
    if tail_limit > _STDERR_TAIL_BYTES or tail_limit > stderr_limit:
        _error("profile stderr tail limit exceeds the native host tail ceiling")
    return timeout_ns / _NANOSECONDS_PER_SECOND, stdout_limit, stderr_limit, tail_limit


@dataclass(frozen=True)
class ChildWorkspace:
    """Trusted host paths and the prepare-time digest for one product."""

    source: str
    target: str
    work: str
    cargo_home: str
    toolchain: str
    artifact_manifest_sha256: str | None = None

    def __post_init__(self) -> None:
        paths = tuple(
            _path(value, label)
            for label, value in (
                ("workspace.source", self.source),
                ("workspace.target", self.target),
                ("workspace.work", self.work),
                ("workspace.cargo_home", self.cargo_home),
                ("workspace.toolchain", self.toolchain),
            )
        )
        _validate_workspace_paths(paths)
        if self.artifact_manifest_sha256 is not None:
            _hash(self.artifact_manifest_sha256, "workspace.artifact_manifest_sha256")
        object.__setattr__(self, "source", paths[0])
        object.__setattr__(self, "target", paths[1])
        object.__setattr__(self, "work", paths[2])
        object.__setattr__(self, "cargo_home", paths[3])
        object.__setattr__(self, "toolchain", paths[4])


@dataclass(frozen=True)
class NativeMeasurementConfig:
    """Immutable profile and complete four-product workspace inventory."""

    profile: Mapping[str, Any]
    docker_client_sha256: str
    workspaces: Mapping[tuple[str, str], ChildWorkspace]

    def __post_init__(self) -> None:
        if not isinstance(self.profile, Mapping):
            _error("measurement profile must be a mapping")
        try:
            container._profile_identity(self.profile)
        except container.ContainerError as exc:
            raise NativeMeasurementError(str(exc)) from exc
        for phase in _PHASE_TIMEOUT_KEYS:
            _execution_limits(self.profile, phase)
        _hash(self.docker_client_sha256, "docker_client_sha256")
        if not isinstance(self.workspaces, Mapping):
            _error("workspaces must be a mapping")
        workspaces = dict(self.workspaces)
        if set(workspaces) != set(PRODUCTS):
            _error("workspaces must contain exactly one entry for every benchmark/revision")
        if any(not isinstance(key, tuple) or len(key) != 2 for key in workspaces):
            _error("workspace keys must be benchmark/revision pairs")
        for key, workspace in workspaces.items():
            if key not in PRODUCTS:
                _error("workspace key is not in the fixed benchmark inventory")
            if not isinstance(workspace, ChildWorkspace):
                _error("workspace value has the wrong type")
        object.__setattr__(self, "workspaces", MappingProxyType(workspaces))


@dataclass(frozen=True)
class ParsedRow:
    """One fixed-size benchmark row after grammar validation."""

    records: int
    json_kb: int
    duration_us: tuple[int, ...]
    ratios: tuple[str, ...]


@dataclass(frozen=True)
class ParsedBenchmark:
    """The exact three-row output and the retained report fields."""

    benchmark: str
    rows: tuple[ParsedRow, ...]
    fields: tuple[tuple[str, int], ...]

    def __post_init__(self) -> None:
        if self.benchmark not in BENCHMARKS:
            _error("parsed benchmark is not in the fixed inventory")
        if self.rows != tuple(self.rows) or len(self.rows) != 3:
            _error("parsed benchmark must contain exactly three rows")
        expected_fields = _SELECTED_FIELDS[self.benchmark]
        if tuple(name for name, _value in self.fields) != expected_fields:
            _error("parsed benchmark fields are not in the fixed order")
        for name, value in self.fields:
            if not isinstance(name, str) or type(value) is not int or not 0 < value <= _U64_MAX:
                _error("parsed benchmark field value is not a positive u64")


@dataclass(frozen=True)
class ChildExecution:
    """Bounded result returned to the trusted controller after one child."""

    child_id: str
    phase: str
    benchmark: str
    revision: str
    artifact_manifest_sha256: str
    build_attempted: bool
    stdout_sha256: str
    stderr_sha256: str
    stderr_tail_hex: str
    elapsed_ns: int
    measurement: ParsedBenchmark | None

    def __post_init__(self) -> None:
        _hash(self.child_id, "child_id")
        if self.phase not in ("prepare", "warmup", "sample"):
            _error("child phase is not fixed")
        if self.benchmark not in BENCHMARKS or self.revision not in REVISIONS:
            _error("child product is not in the fixed inventory")
        _hash(self.artifact_manifest_sha256, "artifact_manifest_sha256")
        if type(self.build_attempted) is not bool or self.build_attempted != (self.phase == "prepare"):
            _error("build_attempted does not match the child phase")
        _hash(self.stdout_sha256, "stdout_sha256")
        _hash(self.stderr_sha256, "stderr_sha256")
        if not isinstance(self.stderr_tail_hex, str) or _HEX_BYTES.fullmatch(self.stderr_tail_hex) is None:
            _error("stderr_tail_hex is not canonical hex")
        if type(self.elapsed_ns) is not int or not 0 <= self.elapsed_ns <= _U64_MAX:
            _error("elapsed_ns is not a u64")
        if self.phase == "prepare":
            if self.measurement is not None:
                _error("prepare child cannot return a native measurement")
        elif not isinstance(self.measurement, ParsedBenchmark) or self.measurement.benchmark != self.benchmark:
            _error("native child must return its parsed benchmark")


def _split_lines(raw: bytes, label: str, *, ascii_only: bool = False) -> tuple[str, ...]:
    if type(raw) is not bytes or len(raw) > _MAX_OUTPUT:
        _error(f"{label} exceeds the fixed output limit")
    if not raw.endswith(b"\n"):
        _error(f"{label} must end with one LF")
    try:
        text = raw.decode("ascii" if ascii_only else "utf-8")
    except UnicodeDecodeError as exc:
        raise NativeMeasurementError(f"{label} is not valid {'ASCII' if ascii_only else 'UTF-8'}") from exc
    if "\r" in text or "\t" in text:
        _error(f"{label} contains a forbidden control character")
    lines = text.split("\n")
    if lines[-1] != "":
        _error(f"{label} has trailing bytes after its final LF")
    return tuple(lines[:-1])


def parse_prepare_output(raw: bytes) -> str:
    """Parse the exact two-line prepare result and return its sealed digest."""

    lines = _split_lines(raw, "prepare output", ascii_only=True)
    if len(lines) != 2 or lines[0] != PREPARED_PATH:
        _error("prepare output does not contain the fixed manifest path")
    prefix = "artifact-manifest-sha256: "
    if not lines[1].startswith(prefix):
        _error("prepare output does not contain the fixed manifest digest")
    return _hash(lines[1][len(prefix) :], "prepare artifact manifest")


def _uint(value: str, label: str, *, positive: bool = False) -> int:
    if _UINT.fullmatch(value) is None:
        _error(f"{label} is not a canonical unsigned integer")
    result = int(value)
    if result > _U64_MAX or (positive and result == 0):
        _error(f"{label} is outside the accepted u64 range")
    return result


def _milliseconds_to_us(value: str, label: str) -> int:
    if _MILLISECONDS.fullmatch(value) is None:
        _error(f"{label} is not a three-decimal millisecond token")
    whole, fraction = value.split(".")
    result = int(whole) * 1000 + int(fraction)
    if result <= 0 or result > _U64_MAX:
        _error(f"{label} is not a positive u64 microsecond value")
    return result


def _ratio(value: str, label: str) -> str:
    if _RATIO.fullmatch(value) is None:
        _error(f"{label} is not a two-decimal ratio token")
    whole, fraction = value[:-1].split(".")
    if int(whole) == 0 and int(fraction) == 0:
        _error(f"{label} must be positive")
    return value


def _row_tokens(segment: str, count: int, label: str) -> tuple[str, ...]:
    if not segment or not segment.isascii() or any(char not in " 0123456789.x" for char in segment):
        _error(f"{label} contains non-ASCII row syntax")
    tokens = tuple(token for token in segment.strip(" ").split(" ") if token)
    if len(tokens) != count:
        _error(f"{label} has the wrong field count")
    return tokens


def _parse_decode_row(line: str, expected_records: int, index: int) -> ParsedRow:
    segments = line.split("|")
    if len(segments) != 3:
        _error("json_decode row must contain exactly two separators")
    left = _row_tokens(segments[0], 2, "json_decode row identity")
    middle = _row_tokens(segments[1], 3, "json_decode row full fields")
    right = _row_tokens(segments[2], 3, "json_decode row projection fields")
    records = _uint(left[0], "json_decode records", positive=True)
    if records != expected_records:
        _error(f"json_decode row {index} has the wrong record count")
    json_kb = _uint(left[1], "json_decode json KB", positive=True)
    durations = tuple(
        _milliseconds_to_us(token, f"json_decode row {index} duration")
        for token in (*middle[:2], *right[:2])
    )
    ratios = tuple(
        _ratio(token, f"json_decode row {index} ratio")
        for token in (middle[2], right[2])
    )
    return ParsedRow(records, json_kb, durations, ratios)


def _parse_soa_row(line: str, expected_records: int, index: int) -> ParsedRow:
    tokens = _row_tokens(line, 9, "json_soa row")
    records = _uint(tokens[0], "json_soa records", positive=True)
    if records != expected_records:
        _error(f"json_soa row {index} has the wrong record count")
    json_kb = _uint(tokens[1], "json_soa json KB", positive=True)
    durations = tuple(
        _milliseconds_to_us(token, f"json_soa row {index} duration") for token in tokens[2:6]
    )
    ratios = tuple(_ratio(token, f"json_soa row {index} ratio") for token in tokens[6:])
    return ParsedRow(records, json_kb, durations, ratios)


def parse_native_output(raw: bytes, benchmark: str) -> ParsedBenchmark:
    """Parse one exact fixed native benchmark output without floating point."""

    if benchmark not in BENCHMARKS:
        _error("benchmark is not in the fixed inventory")
    lines = _split_lines(raw, f"{benchmark} output")
    if len(lines) != 6:
        _error(f"{benchmark} output must contain target, title, header, and three rows")
    if lines[:3] != ("target: native", _TITLES[benchmark], _HEADERS[benchmark]):
        _error(f"{benchmark} output has the wrong target, title, or header")
    expected_records = (10_000, 100_000, 1_000_000)
    parser = _parse_decode_row if benchmark == "json_decode" else _parse_soa_row
    rows = tuple(
        parser(line, records, index)
        for index, (line, records) in enumerate(zip(lines[3:], expected_records), start=1)
    )
    final = rows[-1]
    if benchmark == "json_decode":
        fields = (("A-full", final.duration_us[0]), ("A-proj", final.duration_us[2]))
    else:
        fields = (
            ("soa ms", final.duration_us[0]),
            ("aos ms", final.duration_us[1]),
            ("proj ms", final.duration_us[2]),
        )
    return ParsedBenchmark(benchmark, rows, fields)


def _clock_ns(clock: Clock) -> int:
    try:
        value = clock()
    except BaseException as exc:
        raise NativeMeasurementError("measurement clock failed") from exc
    if type(value) is not int or not 0 <= value <= _U64_MAX:
        _error("measurement clock returned a non-u64 value")
    return value


def _command_for(plan: schedule.ChildPlan) -> tuple[str, ...]:
    if plan not in schedule.full_schedule():
        _error("child plan is not the fixed evidence schedule")
    expected = "prepare" if plan.phase == "prepare" else "native"
    return (_COMMANDS[plan.benchmark], expected) + (("native",) if expected == "prepare" else ())


def _launch(config: NativeMeasurementConfig, plan: schedule.ChildPlan, child_id: str) -> container.ContainerLaunch:
    workspace = config.workspaces[(plan.benchmark, plan.revision)]
    return container.ContainerLaunch(
        child_id=child_id,
        source=workspace.source,
        target=workspace.target,
        work=workspace.work,
        cargo_home=workspace.cargo_home,
        toolchain=workspace.toolchain,
        command=_command_for(plan),
        artifact_manifest_sha256=workspace.artifact_manifest_sha256,
    )


def execute_child(
    config: NativeMeasurementConfig,
    plan: schedule.ChildPlan,
    child_id: str,
    *,
    runner: Runner = native_host.run_docker_commands_captured,
    clock: Clock = time.monotonic_ns,
) -> ChildExecution:
    """Execute exactly one fixed child and return bounded measurement facts.

    The surrounding :class:`schedule.ScheduleState` must call ``start`` before
    this function and ``finish`` only after it returns.  That state owns the
    no-overlap, order, and duplicate-ID checks; this function owns the single
    Docker invocation and never retries it.
    """

    if not isinstance(config, NativeMeasurementConfig):
        _error("measurement config has the wrong type")
    if not isinstance(plan, schedule.ChildPlan):
        _error("child plan has the wrong type")
    if not isinstance(child_id, str) or _HEX64.fullmatch(child_id) is None:
        _error("child_id must be lowercase 64-hex")
    if not callable(runner) or not callable(clock):
        _error("runner and clock must be callable")
    if plan not in schedule.full_schedule():
        _error("child plan is not the fixed evidence schedule")
    workspace = config.workspaces[(plan.benchmark, plan.revision)]
    if plan.phase == "prepare":
        if workspace.artifact_manifest_sha256 is not None:
            _error("prepare child already has an artifact manifest digest")
    elif workspace.artifact_manifest_sha256 is None:
        _error("native child requires the prepare-time artifact manifest digest")

    try:
        argv = container.build_argv(config.profile, _launch(config, plan, child_id))
    except container.ContainerError as exc:
        raise NativeMeasurementError(f"container launch rejected: {exc}") from exc

    timeout_seconds, stdout_limit, stderr_limit, stderr_tail_limit = _execution_limits(
        config.profile, plan.phase
    )
    started = _clock_ns(clock)
    try:
        outputs, actual_client_hash = runner(
            (argv,),
            config.docker_client_sha256,
            timeout_seconds=timeout_seconds,
            stdout_limit=stdout_limit,
            stderr_limit=stderr_limit,
        )
    except NativeMeasurementError:
        raise
    except native_host.NativeHostError as exc:
        raise NativeMeasurementError(f"native Docker command failed: {exc}") from exc
    except BaseException as exc:
        raise NativeMeasurementError("native Docker command failed before a child result") from exc
    ended = _clock_ns(clock)
    if ended < started:
        _error("measurement clock moved backwards")
    elapsed_ns = ended - started
    _hash(actual_client_hash, "actual Docker client digest")
    if actual_client_hash != config.docker_client_sha256:
        _error("Docker client digest changed during measurement")
    if type(outputs) is not tuple or len(outputs) != 1:
        _error("Docker runner must return exactly one command capture")
    capture = outputs[0]
    if not isinstance(capture, native_host.CommandCapture):
        _error("Docker runner returned the wrong capture type")
    if len(capture.stdout) > stdout_limit or len(capture.stderr) > stderr_limit:
        _error("child output exceeded the fixed limit")
    if capture.stderr:
        _error("child stderr is not empty")

    stdout_sha256 = hashlib.sha256(capture.stdout).hexdigest()
    stderr_sha256 = hashlib.sha256(capture.stderr).hexdigest()
    stderr_tail_hex = capture.stderr[-stderr_tail_limit:].hex()
    if plan.phase == "prepare":
        artifact_manifest_sha256 = parse_prepare_output(capture.stdout)
        measurement = None
    else:
        assert workspace.artifact_manifest_sha256 is not None
        artifact_manifest_sha256 = workspace.artifact_manifest_sha256
        measurement = parse_native_output(capture.stdout, plan.benchmark)

    return ChildExecution(
        child_id=child_id,
        phase=plan.phase,
        benchmark=plan.benchmark,
        revision=plan.revision,
        artifact_manifest_sha256=artifact_manifest_sha256,
        build_attempted=plan.phase == "prepare",
        stdout_sha256=stdout_sha256,
        stderr_sha256=stderr_sha256,
        stderr_tail_hex=stderr_tail_hex,
        elapsed_ns=elapsed_ns,
        measurement=measurement,
    )
