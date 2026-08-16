"""Trusted controller ordering for the evidence boundary.

The controller is intentionally an orchestration core.  Privileged adapters
provide the installed-tree lease, host/image/source/review gates, child
executor, report producer, verifier, and publication operation.  This module
fixes their order and owns the fail-closed transition; it does not inspect a
host, invoke Docker, query a provider, or import candidate code.
"""

from __future__ import annotations

import hashlib
import re
from dataclasses import dataclass
from typing import Callable, Mapping

from . import cleanup
from . import cli
from . import report_schema
from . import schedule
from . import verifier


class ControllerError(ValueError):
    """A trusted controller port or transition is invalid."""


_HEX64 = re.compile(r"[0-9a-f]{64}\Z")
_NAME = re.compile(r"[A-Za-z0-9._/:+=@-]{1,255}\Z")
_IMAGE_DIGEST = re.compile(r"sha256:[0-9a-f]{64}\Z")
_RESOURCE_KINDS = ("children", "containers", "mounts", "fds", "private_dirs")
_GATE_NAMES = ("bootstrap", "host", "image", "source", "review")


def _error(message: str) -> None:
    raise ControllerError(message)


def _string(value: object, pattern: re.Pattern[str], label: str) -> str:
    if not isinstance(value, str) or pattern.fullmatch(value) is None:
        _error(f"{label} has the wrong grammar")
    return value


@dataclass(frozen=True)
class ControllerConfig:
    """Profile-pinned identities supplied by the trusted installation."""

    profile_id: str
    profile_sha256: str
    controller_sha256: str
    verifier_sha256: str
    monitor_sha256: str
    host_id: str
    image_digest: str
    identities: verifier.TrustedIdentities
    target_ref: str = "refs/heads/main"

    def __post_init__(self) -> None:
        _string(self.profile_id, _NAME, "profile_id")
        for label in (
            "profile_sha256",
            "controller_sha256",
            "verifier_sha256",
            "monitor_sha256",
        ):
            _string(getattr(self, label), _HEX64, label)
        _string(self.host_id, _NAME, "host_id")
        _string(self.image_digest, _IMAGE_DIGEST, "image_digest")
        if not isinstance(self.identities, verifier.TrustedIdentities):
            _error("identities has the wrong type")
        if self.identities.profile_id != self.profile_id:
            _error("identities.profile_id does not match profile_id")
        if self.identities.producer_executable_sha256 != self.controller_sha256:
            _error("producer identity does not match controller_sha256")
        if self.identities.verifier_executable_sha256 != self.verifier_sha256:
            _error("verifier identity does not match verifier_sha256")
        if self.identities.monitor_executable_sha256 != self.monitor_sha256:
            _error("monitor identity does not match monitor_sha256")
        if self.identities.execution_host_id != self.host_id:
            _error("execution identity does not match host_id")
        if self.identities.execution_image_digest != self.image_digest:
            _error("execution identity does not match image_digest")
        if self.target_ref != "refs/heads/main":
            _error("target_ref must be refs/heads/main")


@dataclass(frozen=True)
class OwnedResource:
    """One resource token returned by a trusted child executor."""

    kind: str
    token: str

    def __post_init__(self) -> None:
        if not isinstance(self.kind, str) or self.kind not in _RESOURCE_KINDS:
            _error("resource kind is not owned by the controller")
        if not isinstance(self.token, str) or not self.token or "\x00" in self.token:
            _error("resource token is invalid")


@dataclass(frozen=True)
class ChildResult:
    """The bounded result of one child invocation."""

    child_id: str
    artifact_manifest_sha256: str
    build_attempted: bool
    exit_code: int = 0
    signal: int | None = None
    timed_out: bool = False
    truncated: bool = False
    resources: tuple[OwnedResource, ...] = ()

    def __post_init__(self) -> None:
        _string(self.child_id, _HEX64, "child_id")
        _string(self.artifact_manifest_sha256, _HEX64, "artifact_manifest_sha256")
        if type(self.build_attempted) is not bool:
            _error("build_attempted must be a boolean")
        if type(self.exit_code) is not int or not 0 <= self.exit_code <= 255:
            _error("exit_code must be an unsigned byte")
        if self.signal is not None and (type(self.signal) is not int or not 1 <= self.signal <= 255):
            _error("signal must be a positive signal number")
        for label, value in (("timed_out", self.timed_out), ("truncated", self.truncated)):
            if type(value) is not bool:
                _error(f"{label} must be a boolean")
        if not isinstance(self.resources, tuple):
            _error("resources must preserve tuple order")
        tokens = set()
        for resource in self.resources:
            if not isinstance(resource, OwnedResource):
                _error("resources must contain OwnedResource values")
            key = (resource.kind, resource.token)
            if key in tokens:
                _error("a child may not return the same resource twice")
            tokens.add(key)


@dataclass(frozen=True)
class ControllerResult:
    """A terminal controller result, including cleanup evidence."""

    state: str
    phases: tuple[str, ...]
    cleanup: cleanup.CleanupResult
    verified: verifier.VerifiedEvidence | None
    error: str | None

    @property
    def accepted(self) -> bool:
        return self.state == "accepted"


Gate = Callable[[cli.RunInvocation, ControllerConfig], None]
ChildIdFactory = Callable[[schedule.ChildPlan], str]
Executor = Callable[[schedule.ChildPlan, str], ChildResult]
ManifestCheck = Callable[[cli.RunInvocation], tuple[bool, bool]]
ArtifactProducer = Callable[
    [cli.RunInvocation, Mapping[tuple[str, str], str], tuple[str, ...]],
    verifier.EvidenceArtifact,
]
ArtifactVerifier = Callable[[verifier.EvidenceArtifact], verifier.VerifiedEvidence]
Publisher = Callable[[verifier.VerifiedEvidence, verifier.EvidenceArtifact, str], None]


@dataclass(frozen=True)
class ControllerHooks:
    """Trusted ports supplied by the installed controller adapters."""

    gates: tuple[tuple[str, Gate], ...]
    child_id: ChildIdFactory
    execute: Executor
    manifests: ManifestCheck
    produce_artifact: ArtifactProducer
    verify_artifact: ArtifactVerifier
    publish: Publisher

    def __post_init__(self) -> None:
        if not isinstance(self.gates, tuple):
            _error("gates must preserve the declared tuple order")
        if any(
            not isinstance(item, tuple) or len(item) != 2
            for item in self.gates
        ):
            _error("each gate must be a name/callable pair")
        names = tuple(name for name, _gate in self.gates)
        if names != _GATE_NAMES:
            _error("gates must be exactly bootstrap, host, image, source, review")
        if any(not isinstance(name, str) or not callable(gate) for name, gate in self.gates):
            _error("each gate must be a callable named trusted port")
        for label, operation in (
            ("child_id", self.child_id),
            ("execute", self.execute),
            ("manifests", self.manifests),
            ("produce_artifact", self.produce_artifact),
            ("verify_artifact", self.verify_artifact),
            ("publish", self.publish),
        ):
            if not callable(operation):
                _error(f"{label} must be callable")


def _run_id(invocation: cli.RunInvocation, config: ControllerConfig) -> str:
    raw = (
        b"align-json-escape-evidence-controller-run-v1\0"
        + config.profile_sha256.encode("ascii")
        + b"\0"
        + invocation.baseline.encode("ascii")
        + b"\0"
        + invocation.candidate.encode("ascii")
    )
    return hashlib.sha256(raw).hexdigest()


def _fallback_cleanup(*, fail_closed: bool) -> cleanup.CleanupResult:
    return cleanup.CleanupResult(
        accepted=False,
        fail_closed=fail_closed,
        staging_present=fail_closed,
        output_present=False,
        reservation_present=fail_closed,
    )


def _verify_artifact_manifests(
    report_bytes: bytes,
    expected: Mapping[tuple[str, str], str],
) -> None:
    """Bind report preparation manifests to the controller-owned schedule."""

    try:
        report = report_schema.decode_report(report_bytes)
    except report_schema.ReportSchemaError as exc:
        raise ControllerError(f"published report schema rejected: {exc}") from exc
    actual: dict[tuple[str, str], str] = {}
    for benchmark in report["body"]["benchmarks"]:
        for preparation in benchmark["preparations"]:
            key = (benchmark["name"], preparation["revision"])
            if key in actual:
                _error("report repeats a preparation manifest")
            actual[key] = preparation["artifact_manifest_sha256"]
    if actual != dict(expected):
        _error("report preparation manifests do not match executed children")


class Controller:
    """Drive one exact run from the exclusive lease to accepted publication."""

    def __init__(
        self,
        config: ControllerConfig,
        hooks: ControllerHooks,
        lease: object,
    ) -> None:
        if not isinstance(config, ControllerConfig):
            _error("controller config has the wrong type")
        if not isinstance(hooks, ControllerHooks):
            _error("controller hooks have the wrong type")
        required = (
            "acquire",
            "create_reservation",
            "release_lock_for_publication",
            "mark_published",
            "finalize_publication",
            "abort",
        )
        if any(not callable(getattr(lease, name, None)) for name in required):
            _error("lease does not implement the exclusive publication port")
        self.config = config
        self.hooks = hooks
        self.lease = lease

    def _failure(
        self,
        *,
        phases: list[str],
        transaction: cleanup.CleanupTransaction,
        acquired: bool,
        run_id: str | None,
        output_dir: str | None,
        reservation_attempted: bool,
        uncertain: bool,
        error: Exception,
    ) -> ControllerResult:
        fail_closed = uncertain
        if acquired and uncertain and not reservation_attempted and run_id is not None and output_dir is not None:
            try:
                # A child or publication failure may happen before the normal
                # reservation phase.  Install the same durable reservation
                # before dropping the host lock so a later process cannot run.
                self.lease.create_reservation(run_id, output_dir)
            except Exception:
                fail_closed = True
        try:
            cleanup_result = transaction.abort(
                cleanup_succeeded=not uncertain,
                reservation_remove_succeeded=not uncertain,
            )
            fail_closed = fail_closed or cleanup_result.fail_closed
        except Exception:
            cleanup_result = _fallback_cleanup(fail_closed=True)
            fail_closed = True
        if acquired:
            try:
                # An uncertain child/publication leaves the durable reservation
                # for administrator recovery.  A known pre-publication failure
                # may remove an unused reservation and release the lock.
                self.lease.abort(remove_reservation=not uncertain)
            except Exception:
                fail_closed = True
        return ControllerResult(
            state="fail-closed" if fail_closed else "rejected",
            phases=tuple(phases),
            cleanup=cleanup_result,
            verified=None,
            error=f"{type(error).__name__}: {error}",
        )

    def run(self, invocation: cli.RunInvocation) -> ControllerResult:
        """Run one parsed invocation and return exactly one terminal result."""

        transaction = cleanup.CleanupTransaction()
        phases: list[str] = []
        acquired = False
        uncertain = False
        active_child = False
        reservation_attempted = False
        lease_reservation = False
        verified: verifier.VerifiedEvidence | None = None
        run_id: str | None = None

        try:
            if not isinstance(invocation, cli.RunInvocation):
                _error("controller accepts only cli.RunInvocation")
            run_id = _run_id(invocation, self.config)

            phases.append("exclusive")
            self.lease.acquire()
            acquired = True
            phases.append("reserve")
            reservation_attempted = True
            self.lease.create_reservation(run_id, invocation.output_dir)
            lease_reservation = True

            for name, gate in self.hooks.gates:
                phases.append(name)
                gate(invocation, self.config)

            state = schedule.ScheduleState()
            phases.append("schedule")
            for plan in state.plans:
                child_id = self.hooks.child_id(plan)
                state.start(plan, child_id)
                active_child = True
                result = self.hooks.execute(plan, child_id)
                if not isinstance(result, ChildResult):
                    _error("child executor returned the wrong result type")
                if result.child_id != child_id:
                    _error("child executor changed the controller-assigned child ID")
                for resource in result.resources:
                    transaction.attach(resource.kind, resource.token)
                state.finish(
                    exit_code=result.exit_code,
                    signal=result.signal,
                    timed_out=result.timed_out,
                    truncated=result.truncated,
                    artifact_manifest_sha256=result.artifact_manifest_sha256,
                    build_attempted=result.build_attempted,
                )
                for resource in result.resources:
                    transaction.remove(resource.kind, resource.token)
                active_child = False
            state.finish_all()

            phases.append("manifests")
            manifest_state = self.hooks.manifests(invocation)
            if (
                not isinstance(manifest_state, tuple)
                or len(manifest_state) != 2
                or any(type(value) is not bool for value in manifest_state)
            ):
                _error("manifest check must return two booleans")
            transaction.set_manifest_state(
                source_unchanged=manifest_state[0],
                cache_unchanged=manifest_state[1],
            )
            if manifest_state != (True, True):
                _error("source or cache manifest changed")

            phases.append("report")
            artifact = self.hooks.produce_artifact(
                invocation,
                schedule.manifest_map(state),
                tuple(phases),
            )
            if not isinstance(artifact, verifier.EvidenceArtifact):
                _error("artifact producer returned the wrong type")
            if (
                artifact.expectations.baseline != invocation.baseline
                or artifact.expectations.candidate != invocation.candidate
                or artifact.expectations.profile_sha256 != self.config.profile_sha256
                or artifact.expectations.identities != self.config.identities
            ):
                _error("artifact expectations are not bound to this invocation and profile identities")

            phases.append("verify")
            verified = self.hooks.verify_artifact(artifact)
            if not isinstance(verified, verifier.VerifiedEvidence):
                _error("artifact verifier returned the wrong type")
            if verified.artifact is not artifact:
                _error("artifact verifier did not return the checked artifact")
            if (
                verified.baseline != invocation.baseline
                or verified.candidate != invocation.candidate
                or verified.profile_sha256 != self.config.profile_sha256
            ):
                _error("verified artifact is not bound to this invocation and profile")
            if verified.verdict not in ("pass", "regression"):
                _error("verified artifact has an unknown verdict")
            if artifact.expectations.require_pass and verified.verdict != "pass":
                _error("a required-pass invocation cannot publish a regression")
            _verify_artifact_manifests(verified.artifact.report, schedule.manifest_map(state))

            phases.append("stage")
            transaction.stage_report()
            transaction.create_reservation()
            phases.append("unlock")
            self.lease.release_lock_for_publication()
            transaction.release_lock()

            phases.append("publish")
            uncertain = True
            self.hooks.publish(verified, verified.artifact, invocation.output_dir)
            transaction.publish_output()
            self.lease.mark_published()
            phases.append("finalize")
            self.lease.finalize_publication()
            lease_reservation = False
            transaction.remove_reservation()
            final_state = "regression" if verified.verdict == "regression" else "accepted"
            phases.append(final_state)
            cleanup_result = transaction.accept()
            return ControllerResult(
                state=final_state,
                phases=tuple(phases),
                cleanup=cleanup_result,
                verified=verified,
                error=None,
            )
        except Exception as exc:
            uncertain = uncertain or active_child or any(
                phase in phases for phase in ("unlock", "publish", "finalize")
            ) or (reservation_attempted and not lease_reservation)
            return self._failure(
                phases=phases,
                transaction=transaction,
                acquired=acquired,
                run_id=run_id,
                output_dir=invocation.output_dir if isinstance(invocation, cli.RunInvocation) else None,
                reservation_attempted=reservation_attempted,
                uncertain=uncertain,
                error=exc,
            )
