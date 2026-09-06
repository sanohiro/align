#!/usr/bin/env bash
set -u

usage() {
  echo "usage: bench/startup/run.sh --observer ABS --observer-sha256 SHA256 --work-dir ABS --provenance ABS" >&2
  exit 2
}

observer=
observer_sha256=
work_dir=
provenance=
seen_observer=0
seen_observer_sha256=0
seen_work_dir=0
seen_provenance=0

# Public shape validation uses shell builtins only and precedes every mutation
# and child creation.
while [ "$#" -gt 0 ]; do
  [ "$#" -ge 2 ] || usage
  option=$1
  value=$2
  case "$option" in
    --observer)
      [ "$seen_observer" -eq 0 ] || usage
      seen_observer=1
      observer=$value
      ;;
    --observer-sha256)
      [ "$seen_observer_sha256" -eq 0 ] || usage
      seen_observer_sha256=1
      observer_sha256=$value
      ;;
    --work-dir)
      [ "$seen_work_dir" -eq 0 ] || usage
      seen_work_dir=1
      work_dir=$value
      ;;
    --provenance)
      [ "$seen_provenance" -eq 0 ] || usage
      seen_provenance=1
      provenance=$value
      ;;
    *) usage ;;
  esac
  shift 2
done

[ "$seen_observer" -eq 1 ] || usage
[ "$seen_observer_sha256" -eq 1 ] || usage
[ "$seen_work_dir" -eq 1 ] || usage
[ "$seen_provenance" -eq 1 ] || usage
case "$observer" in /*) ;; *) usage ;; esac
case "$work_dir" in /*) ;; *) usage ;; esac
case "$provenance" in /*) ;; *) usage ;; esac
case "$observer_sha256" in
  *[!0-9a-f]*|'') usage ;;
esac
[ "${#observer_sha256}" -eq 64 ] || usage

# External acquisition helpers run under one fixed platform path and locale.
# Clear startup hooks and exported helper functions before the first child or
# filesystem mutation; the final observer exec still receives no environment.
PATH=/usr/bin:/bin
LANG=C
LC_ALL=C
TZ=UTC
export PATH LANG LC_ALL TZ
unset CDPATH ENV BASH_ENV
unset -f sh ps sleep dirname mkdir mktemp cp chmod id uname stat rm rmdir 2>/dev/null || :
readonly PATH LANG LC_ALL TZ
# Noninteractive callers can still have a controlling terminal. The worker is
# deliberately a background job-control group, so it must not be suspended by
# terminal job-control signals while the outer shell waits for it.
trap '' TSTP TTIN TTOU

private_exec() {
outer_pgid=$1
outer_pid=$2
worker_pgid=$(sh -c 'ps -o pgid= -p "$PPID"') || exit 1
worker_pgid=${worker_pgid//[[:space:]]/}
case "$worker_pgid" in *[!0-9]*|'') exit 1 ;; esac
[ "$worker_pgid" != "$outer_pgid" ] || exit 1

# This watchdog remains outside the worker's job-control process group when
# Bash supplies one. Its signal dispositions are also safe if a platform puts
# it in the same group. USR1 is the sole successful disarm path.
watch_preexec() {
  trap 'exit 0' USR1
  trap '' HUP INT TERM
  elapsed=0
  while [ "$elapsed" -lt 20 ]; do
    sleep 1 || :
    elapsed=$((elapsed + 1))
  done
  kill -TERM -- "-$worker_pgid" 2>/dev/null || exit 0
  elapsed=0
  while [ "$elapsed" -lt 5 ]; do
    sleep 1 || :
    elapsed=$((elapsed + 1))
  done
  kill -KILL -- "-$worker_pgid" 2>/dev/null || :
  exit 1
}
watch_preexec &
watchdog_pid=$!

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd -P) || exit 1
private_parent="$script_dir/../../target/startup-observer"
old_umask=$(umask)
umask 077
mkdir -p -- "$private_parent" || exit 1
[ ! -L "$private_parent" ] && [ -d "$private_parent" ] || exit 1
private_dir=$(mktemp -d "$private_parent/run.XXXXXX") || exit 1
private_image="$private_dir/observer"
cleanup_armed=1
cleanup() {
  status=$?
  trap - EXIT HUP INT TERM
  if [ "$cleanup_armed" -eq 1 ]; then
    rm -f -- "$private_image"
    rmdir -- "$private_dir" 2>/dev/null || :
  fi
  umask "$old_umask"
  kill -USR1 "$watchdog_pid" 2>/dev/null || :
  wait "$watchdog_pid" 2>/dev/null || :
  exit "$status"
}
trap cleanup EXIT HUP INT TERM

cp -- "$observer" "$private_image" || exit 1
chmod 0500 "$private_image" || exit 1
[ ! -L "$private_dir" ] && [ -d "$private_dir" ] || exit 1
[ ! -L "$private_image" ] && [ -f "$private_image" ] || exit 1
exec 9<"$private_image" || exit 1

effective_uid=$(id -u) || exit 1
case "$(uname -s)" in
  Linux)
    dir_state=$(stat -Lc '%a:%u:%F' "$private_dir") || exit 1
    path_state=$(stat -Lc '%d:%i:%a:%u:%h:%F' "$private_image") || exit 1
    descriptor_state=$(stat -Lc '%d:%i:%a:%u:%h:%F' /dev/fd/9) || exit 1
    ;;
  Darwin)
    dir_state=$(stat -Lf '%Lp:%u:%HT' "$private_dir") || exit 1
    path_state=$(stat -Lf '%d:%i:%Lp:%u:%l:%HT' "$private_image") || exit 1
    descriptor_state=$(stat -Lf '%d:%i:%Lp:%u:%l:%HT' /dev/fd/9) || exit 1
    ;;
  *) exit 1 ;;
esac
case "$dir_state" in
  "700:$effective_uid:directory"|"700:$effective_uid:Directory") ;;
  *) exit 1 ;;
esac
case "$path_state" in
  *":500:$effective_uid:1:regular file"|*":500:$effective_uid:1:Regular File") ;;
  *) exit 1 ;;
esac
[ "$path_state" = "$descriptor_state" ] || exit 1

# The observer validates descriptor identity, ownership, modes, link count and
# digest. Unlinking before exec removes the last pathname alias.
rm -f -- "$private_image" || exit 1
rmdir -- "$private_dir" || exit 1
cleanup_armed=0
trap - EXIT HUP INT TERM
umask "$old_umask"

# No watchdog process may survive into the observer lifetime.
kill -USR1 "$watchdog_pid" 2>/dev/null || exit 1
wait "$watchdog_pid" || exit 1
kill -USR1 "$outer_pid" 2>/dev/null || exit 1
trap - TSTP TTIN TTOU
exec -c /dev/fd/9 --adopt-fd 9 --expected-sha256 "$observer_sha256" \
  --observer "$observer" --observer-sha256 "$observer_sha256" \
  --work-dir "$work_dir" --provenance "$provenance"
exit 1
}

outer_pgid=$(sh -c 'ps -o pgid= -p "$PPID"') || exit 1
outer_pgid=${outer_pgid//[[:space:]]/}
case "$outer_pgid" in *[!0-9]*|'') exit 1 ;; esac
outer_pid=$$
observer_started=0
observer_ready() {
  observer_started=1
  trap '' HUP INT TERM
}
trap observer_ready USR1
set -m
private_exec "$outer_pgid" "$outer_pid" &
worker_pid=$!
set +m
forward_signal() {
  trap - HUP INT TERM
  kill -TERM -- "-$worker_pid" 2>/dev/null || :
  wait "$worker_pid" 2>/dev/null || :
  exit 1
}
trap forward_signal HUP INT TERM
while :; do
  wait "$worker_pid"
  status=$?
  kill -0 "$worker_pid" 2>/dev/null || break
done
trap - HUP INT TERM USR1
if [ "$observer_started" -eq 1 ]; then
  exit "$status"
fi
exit 1
