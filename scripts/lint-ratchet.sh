#!/usr/bin/env bash
# Ratchet gate for panic-source and lossy-cast counts in the crates where they
# are a recurring review-finding class (align-self-review Gates 2 and 3).
#
# A full `deny` needs a 1,000+ item burn-down, so instead the current counts
# are pinned in scripts/lint-ratchet-baseline.txt and this gate fails when a
# count RISES. New code is therefore effectively lint-enforced immediately,
# and the baseline only ever moves down. Run `scripts/lint-ratchet.sh --update`
# after intentionally reducing a count.
set -euo pipefail

cd "$(dirname "$0")/.."
baseline_file="scripts/lint-ratchet-baseline.txt"

# count CRATE KIND — occurrences outside pure comment lines. The patterns
# approximate the align-self-review Gate 2/3 greps. grep -r emits
# `path:content`, so the comment filter must skip past the filename prefix.
count() {
  local dir="crates/$1/src" pattern n
  case "$2" in
    panics) pattern='\.unwrap\(\)|\.expect\(|unreachable!' ;;
    casts) pattern=' as usize| as u32| as i32' ;;
    # A test that compiles a `.align` source through alignc must read it at RUNTIME
    # (common::fixture), never include_str! it: baking the source in makes every `.align`
    # edit rebuild+relink the whole test crate (measured ~6x slower edit->test on pkg.db).
    # Scanned across every crate's tests/ so a new bake is caught wherever it lands.
    fixture-bake)
      n=$(grep -rE 'include_str!\("[^"]*\.align"' crates/*/tests --include='*.rs' 2>/dev/null \
        | grep -cv '^[^:]*:[[:space:]]*//') || n=0
      echo "$n"
      return 0
      ;;
    *) echo "unknown ratchet kind: $2" >&2; exit 2 ;;
  esac
  [[ -d "$dir" ]] || {
    echo "missing $dir (crate moved or renamed? fix the rows in scripts/lint-ratchet.sh)" >&2
    exit 2
  }
  n=$(grep -rE "$pattern" "$dir" --include='*.rs' | grep -cv '^[^:]*:[[:space:]]*//') || n=0
  echo "$n"
}

rows="align_mir panics
align_codegen_llvm panics
align_runtime casts
workspace fixture-bake"

if [[ "${1:-}" == "--update" ]]; then
  {
    echo "# crate kind count — maintained by scripts/lint-ratchet.sh --update"
    while IFS=' ' read -r crate kind; do
      echo "$crate $kind $(count "$crate" "$kind")"
    done <<<"$rows"
  } > "$baseline_file"
  echo "baseline updated:"
  cat "$baseline_file"
  exit 0
fi

[[ -f "$baseline_file" ]] || { echo "missing $baseline_file (run --update once)" >&2; exit 2; }

status=0
while IFS=' ' read -r crate kind; do
  current=$(count "$crate" "$kind")
  pinned=$(awk -v c="$crate" -v k="$kind" '$1 == c && $2 == k { print $3 }' "$baseline_file")
  [[ -n "$pinned" ]] || { echo "no baseline row for $crate $kind (run --update)" >&2; status=1; continue; }
  if [[ "$current" -gt "$pinned" ]]; then
    echo "RATCHET: $crate $kind rose $pinned -> $current." >&2
    case "$kind" in
      panics) echo "  New unwrap()/expect()/unreachable! in $crate. User-input-reachable code must diagnose, never panic (Gate 3): return an error via .get()/.ok_or_else()/checked_sub instead. For a genuine internal invariant, justify it in a comment at the call site — then run scripts/lint-ratchet.sh --update in the same commit." >&2 ;;
      casts) echo "  New 'as usize/u32/i32' in $crate. Incoming lengths/counts must use usize::try_from + checked_mul/checked_add (Gate 2). For a proven-lossless cast, justify it in a comment — then run scripts/lint-ratchet.sh --update in the same commit." >&2 ;;
      fixture-bake) echo "  New include_str! of a .align source in a test. Read it at RUNTIME via common::fixture(\"path/from/repo/root.align\") instead — baking it in makes every .align edit rebuild+relink the whole test crate (~6x slower edit->test, measured on pkg.db)." >&2 ;;
    esac
    status=1
  elif [[ "$current" -lt "$pinned" ]]; then
    echo "RATCHET: $crate $kind improved $pinned -> $current — run scripts/lint-ratchet.sh --update to lock in the reduction." >&2
    status=1
  fi
done <<<"$rows"

[[ "$status" -eq 0 ]] && echo "lint ratchet OK"
exit "$status"
