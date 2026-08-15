#!/usr/bin/env bash
# Deterministic owner for benchmark_evidence_cleanup_matrix.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd -P)"
cd "$REPO_ROOT"
export PYTHONDONTWRITEBYTECODE=1

python3 - <<'PY'
from scripts.benchmark_evidence import cleanup


def expect_error(call, fragment):
    try:
        call()
    except cleanup.CleanupError as exc:
        assert fragment in str(exc), (fragment, str(exc))
    else:
        raise AssertionError(f"accepted invalid cleanup transition: {fragment}")


def attach_all(tx):
    for kind in ("children", "containers", "mounts", "fds", "private_dirs"):
        tx.attach(kind, kind)


tx = cleanup.CleanupTransaction()
attach_all(tx)
assert tx.snapshot().children_remaining == 1
for kind in ("children", "containers", "mounts", "fds", "private_dirs"):
    tx.remove(kind, kind)
tx.stage_report()
assert tx.snapshot() == cleanup.CleanupSnapshot(0, 0, 0, 0, 0, True, True, True)
tx.create_reservation()
tx.release_lock()
tx.publish_output()
tx.remove_reservation()
result = tx.accept()
assert result.accepted
assert not result.fail_closed
assert result.output_present
assert not result.reservation_present

live = cleanup.CleanupTransaction()
live.attach("children", "child")
expect_error(lambda: live.stage_report(), "all children")
live.remove("children", "child")
live.set_manifest_state(source_unchanged=False, cache_unchanged=True)
expect_error(lambda: live.stage_report(), "manifests changed")

order = cleanup.CleanupTransaction()
expect_error(lambda: order.create_reservation(), "locked staging")
expect_error(lambda: order.release_lock(), "durable reservation")
order.stage_report()
expect_error(lambda: order.publish_output(), "locked-staging")
order.create_reservation()
expect_error(lambda: order.remove_reservation(), "published output")
order.release_lock()
order.publish_output()
order.remove_reservation()
order.accept()

failed_before_publish = cleanup.CleanupTransaction()
failed_before_publish.stage_report()
cleaned = failed_before_publish.abort(cleanup_succeeded=True)
assert not cleaned.accepted
assert not cleaned.fail_closed
assert not cleaned.staging_present
assert not cleaned.output_present

failed_publication = cleanup.CleanupTransaction()
failed_publication.stage_report()
failed_publication.create_reservation()
failed = failed_publication.abort(
    cleanup_succeeded=True, reservation_remove_succeeded=False
)
assert not failed.accepted
assert failed.fail_closed
assert failed.reservation_present
assert not failed.output_present

failed_live = cleanup.CleanupTransaction()
failed_live.attach("children", "term-ignoring-descendant")
failed_live_result = failed_live.abort(cleanup_succeeded=False)
assert not failed_live_result.accepted
assert failed_live_result.fail_closed
assert failed_live_result.staging_present is False
assert failed_live_result.reservation_present
assert not failed_live.snapshot().host_lock_held_for_signing

print("cleanup checks passed")
PY
