#!/usr/bin/env bash
# Local item-3 wall-time and stage-overlap measurement. Not a correctness/CI gate.
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
repo="$(cd "$here/../.." && pwd)"
baseline="${BASELINE_ALIGNC:-}"
pipelined="${PIPELINED_ALIGNC:-}"
samples="${ALIGN_PIPELINE_SAMPLES:-7}"

if [ -z "$baseline" ] || [ -z "$pipelined" ]; then
  echo "set BASELINE_ALIGNC and PIPELINED_ALIGNC to optimized alignc binaries" >&2
  exit 2
fi
case "$samples" in
  ''|*[!0-9]*) echo "ALIGN_PIPELINE_SAMPLES must be an integer >= 3" >&2; exit 2 ;;
esac
if [ "$samples" -lt 3 ]; then
  echo "ALIGN_PIPELINE_SAMPLES must be >= 3" >&2
  exit 2
fi

work="$(mktemp -d)"
cleanup() {
  rm -rf "$work"
}
trap cleanup EXIT

mkdir -p "$work/baseline" "$work/pipelined"
cp -R "$repo/apps/db/." "$work/baseline/"
cp -R "$repo/apps/db/." "$work/pipelined/"

# Runtime-only Linux images may ship libpq.so.5 without the development-package libpq.so linker
# name. The benchmark needs no headers or server, so provide that name inside the disposable work
# directory when ldconfig can resolve the installed ABI library.
if [ "$(uname -s)" = Linux ] && command -v ldconfig >/dev/null 2>&1 \
  && ! cc -print-file-name=libpq.so | grep -q '^/'; then
  pq_runtime="$(ldconfig -p 2>/dev/null | awk '/libpq\.so\.[0-9]+ / { print $NF; exit }')"
  if [ -n "$pq_runtime" ]; then
    mkdir -p "$work/lib"
    ln -s "$pq_runtime" "$work/lib/libpq.so"
    export LIBRARY_PATH="$work/lib${LIBRARY_PATH:+:$LIBRARY_PATH}"
  fi
fi

run_timed() {
  binary="$1"
  dir="$2"
  times="$3"
  timing="$work/time.txt"
  (
    cd "$dir"
    { time -p "$binary" build main.align -j 4 --profile release --no-rt-lto >/dev/null; } \
      2>"$timing"
  )
  awk '/^real / { value=$2 } END { if (value == "") exit 1; print value }' "$timing" >>"$times"
}

measure_work() {
  binary="$1"
  dir="$2"
  cache="$3"
  label="$4"
  log="$work/$label-work.log"
  (
    cd "$dir"
    ALIGNC_CACHE="$cache" "$binary" build main.align -j 4 --profile release --no-rt-lto \
      --cache-stats >/dev/null 2>"$log"
  )
  frontend_misses="$(awk '/^alignc: cache: .* frontend miss \(/ { count++ } END { print count + 0 }' "$log")"
  frontend_hits="$(awk '/^alignc: cache: .* frontend hit$/ { count++ } END { print count + 0 }' "$log")"
  codegen_misses="$(awk '/^alignc: cache: .* miss \(/ && $0 !~ / frontend miss \(/ { count++ } END { print count + 0 }' "$log")"
  codegen_hits="$(awk '/^alignc: cache: .* hit$/ && $0 !~ / frontend hit$/ { count++ } END { print count + 0 }' "$log")"
  if [ "$frontend_hits" -ne 0 ] || [ "$codegen_hits" -ne 0 ]; then
    echo "$label work audit unexpectedly hit a fresh cache" >&2
    sed -n '/^alignc: cache:/p' "$log" >&2
    exit 1
  fi
  echo "$frontend_misses $codegen_misses"
}

baseline_work="$(measure_work "$baseline" "$work/baseline" "$work/baseline-cache" baseline)"
pipelined_work="$(measure_work "$pipelined" "$work/pipelined" "$work/pipelined-cache" pipelined)"
if [ "$baseline_work" != "$pipelined_work" ]; then
  echo "frontend/codegen work differs: baseline $baseline_work, pipelined $pipelined_work" >&2
  exit 1
fi
set -- $baseline_work
frontend_invocations="$1"
codegen_invocations="$2"

export ALIGNC_CACHE=off
run_timed "$baseline" "$work/baseline" "$work/baseline-warmup.txt"
run_timed "$pipelined" "$work/pipelined" "$work/pipeline-warmup.txt"

i=1
while [ "$i" -le "$samples" ]; do
  if [ $((i % 2)) -eq 1 ]; then
    run_timed "$baseline" "$work/baseline" "$work/baseline-times.txt"
    run_timed "$pipelined" "$work/pipelined" "$work/pipeline-times.txt"
  else
    run_timed "$pipelined" "$work/pipelined" "$work/pipeline-times.txt"
    run_timed "$baseline" "$work/baseline" "$work/baseline-times.txt"
  fi
  i=$((i + 1))
done

cmp "$work/baseline/main" "$work/pipelined/main"
middle=$(((samples + 1) / 2))
baseline_median="$(sort -n "$work/baseline-times.txt" | sed -n "${middle}p")"
pipeline_median="$(sort -n "$work/pipeline-times.txt" | sed -n "${middle}p")"

echo "cold-cache frontend invocations per revision: $frontend_invocations"
echo "cold-cache codegen invocations per revision: $codegen_invocations"
echo "baseline seconds: $(tr '\n' ' ' <"$work/baseline-times.txt")"
echo "pipelined seconds: $(tr '\n' ' ' <"$work/pipeline-times.txt")"
echo "baseline median: $baseline_median s"
echo "pipelined median: $pipeline_median s"
echo "output bytes: identical"

# Linux-only direct observation: sample alignc's main-thread and worker-thread CPU ticks. A bucket
# where both advance is an observed frontend/coordinator + LLVM-worker overlap interval. The old
# two-phase driver waits while its codegen workers run, so its main thread does not advance there.
if [ "$(uname -s)" = Linux ] && [ -r /proc/self/stat ]; then
  prior_dir="$PWD"
  cd "$work/pipelined"
  "$pipelined" build main.align -j 4 --profile release --no-rt-lto >/dev/null 2>&1 &
  pid=$!
  cd "$prior_dir"
  previous_main=0
  previous_workers=0
  overlap_buckets=0
  total_buckets=0
  while kill -0 "$pid" 2>/dev/null; do
    main_ticks=0
    worker_ticks=0
    for stat in /proc/"$pid"/task/*/stat; do
      [ -r "$stat" ] || continue
      # alignc's thread names contain no spaces, so proc stat fields 14/15 remain shell fields 14/15.
      set -- $(sed -n '1p' "$stat")
      ticks=$((${14} + ${15}))
      tid="$(basename "$(dirname "$stat")")"
      if [ "$tid" = "$pid" ]; then
        main_ticks="$ticks"
      else
        worker_ticks=$((worker_ticks + ticks))
      fi
    done
    if [ "$main_ticks" -gt "$previous_main" ] && [ "$worker_ticks" -gt "$previous_workers" ]; then
      overlap_buckets=$((overlap_buckets + 1))
    fi
    previous_main="$main_ticks"
    previous_workers="$worker_ticks"
    total_buckets=$((total_buckets + 1))
    sleep 0.01
  done
  wait "$pid"
  echo "observed overlap: ${overlap_buckets} x 10ms sampling buckets (${total_buckets} total)"
else
  echo "observed overlap: /proc task sampling unavailable on this host"
fi
