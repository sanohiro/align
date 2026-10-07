# Owned string keys for stable sorting

Status: implemented; capability owners pass.

This implements the explicitly deferred Category A capability in plan23:
`Ord` already includes `string`, but fused `sort_by_key` lacks per-key cleanup.
Request27 records the consumer sorting family. Direct Move string elements,
comparator overloads and K1 remain outside this boundary.

## Public contract ledger

- Surface: existing `source.sort_by_key(f) -> array<T>`; existing Copy scalar
  and str element domain. `f(T) -> K` admits the existing Ord domain including
  owned string. No arguments, defaults, operator or error family are added.
- Key evaluation: exactly once per surviving element in input-index order,
  after collection, with existing capture evaluation order. Empty input calls
  no key; singleton calls one. Named, lifted, generic/imported callables retain
  current by-value Copy input/capture admission. Invalid types/modes are static
  errors; allocation failure remains terminal; no unwind is introduced.
- Order: stable byte-lexicographic string ordering including NUL/prefix/empty
  keys; integer/float/char/str behavior is unchanged.
- Ownership/lifetime: each returned string keeps its ordinary dynamic cleanup
  bit. Sorting retains the key bytes until comparisons finish, then releases
  independently owned keys exactly once. A false cleanup bit never transfers key ownership to the sort. The returned array contains only original Copy elements,
  never keys; its current source/view lifetime rules are unchanged.
- Allocation: sort is the explicit materializing terminal. Owned-string keys
  add one transient heap owner column of N string headers. Existing sorted
  key and scratch columns hold Copy str views; no implicit key-byte clone.
  Key-function allocations remain explicit in its source. No performance
  improvement or throughput claim; existing comparison algorithm remains.
- Owners: sema key admission, checked-HIR callable validation, MIR named-call
  cleanup ABI and sort lowering. No runtime helper, ABI record, interface
  format or HIR/MIR variant. Compiler/build identity invalidates artifacts.
- Sources: draft.md, language-spec.md, design-notes.md, Settled sorting prose
  where normative, plan19 key/copy-position rows, plan23 Category A disposition,
  plan88 historical scope pointer and core array-slice-pipeline English/ja.

## Ownership strategy

Keep comparison keys Copy. For a string-returning key callable, invoke through
emit_named_call so CallWithCleanup produces the exact ordinary cleanup bit.
Create its str view for the sorted comparison column. A separate owner column
stays in original input order. For each key, reset one private String scratch
slot to null with DropFlagInit; only when the returned cleanup bit is true,
store the returned String header there. Store the selected header (owned string
or null) into the owner column. This normalizes conditional ownership at the
transfer boundary, not by inferring ownership from lifetime roots. The private
scratch slot has no independent Drop or cleanup owner; each header is transferred into exactly one column entry before the slot is reset. Never
sort or duplicate the owner column. At the shared post-decoration exit, shallow-drop
comparison columns and recursively DropValue the owner column once. Null entries
are harmless; externally owned keys are never freed by this sort.

Every key either returns normally or exits its own call/aborts. There is no
recoverable exceptional edge after partial decoration: the key type is String,
not Result; capture/control expressions finish before scratch acquisition.
Early ordered and tiny-input exits must converge on the same owner cleanup.
If inspection finds another normal post-allocation escape, add its partial
initialization ownership before admitting the capability rather than assuming
all entries are initialized.

## Implementation closure matrix

| Cell | Implementation and owner requirement |
| --- | --- |
| Formation/validation | Semantics admit only exact Ord String beyond old Copy output; checked-HIR uses an explicit output ownership policy for sort keys only. Other pipeline output Copy gates and input/capture/mode checks remain. `owned_string_sort_key_hir_rejects_forged_metadata` rejects wrong key/type/call/cleanup records. |
| Construction/move/storage/Drop | Ordinary dynamic return bit determines ownership in the unsorted nullable owner column. MIR `owned_sort_keys_preserve_cleanup_and_copy_comparisons` checks exact CallWithCleanup-to-bit-to-conditional-store linkage and one common deep cleanup; driver `owned_string_keys_order_effects_and_cleanup` supplies native allocation/free accounting and a missing-Drop mutation. |
| All exits/joins | Zero/one/32/33/multi-pass inputs, ordered/reverse/duplicates, filters and early source exits (`early_source` in the driver owner); captures are ordinary bound Copy values evaluated before scratch acquisition; callback branch/match/else/?/map_err/loop returns reach ordinary owned-return ABI. Abort/divergence promises no cleanup. No owning key may enter comparison scratch. |
| Lifetime/arena | Heap String keys over captured caller-region views, `owned_keys_preserve_element_lifetimes_and_copy_boundaries` owns expiry negatives; returned elements preserve original roots. No borrowed key becomes an owner. `mixed_return_cleanup_bits_leave_external_key_bytes_live` supplies separately retained false-bit keys at the MIR ABI seam. |
| Generic/interface/full/per-unit | `owned_string_keys_order_effects_and_cleanup` exercises an imported key, lifted capture and generic wrapper plus interface codec replay; both native compilation modes. No serialized new state. |
| Siblings/malformed | `sort_by_key`, `sort_strings`, `sort_merge`, and `sort_adaptive` own existing numeric/str paths; all other Copy-only pipeline positions retain source diagnostics and checked-HIR rejection. Direct Move elements remain excluded. |
| Lifecycle/resource owners | Exclusive ArtifactStage and bounded guarded child; explicit alloc-count feature with compile-time symbol reference and positive probe startup. Driver `sort_owned_keys::owned_string_keys_order_effects_and_cleanup` checks exact key count and balanced allocations/frees, including a missing-owner-Drop mutation. |

Current source String producers always return heap owners; `clone_in` returns
Str, including on String receivers. There is no admitted Copy-input/capture
source callable returning a false-bit String today. The mixed-bit native owner
therefore changes one producer to return a false bit after lowering and retains
those keys in a fixture-owned column, freed after the sort cleanup. Premature
sort cleanup double-frees; missing cleanup fails allocation parity. The fixture
still passes MIR producer validation. It is an ABI fixture, not
new source syntax or an arena-String producer. Source owners separately exercise
heap keys made from caller-region Str views and existing Str-key cleanup.

One capability contains admission plus ownership lowering plus all consumer
owners; a dormant admission would be unsound. The complete diff is approximately
1000 handwritten lines, predominantly the bounded native fixture and owners.
Keeping it together avoids duplicating the ownership proof or publishing an
unusable producer without cleanup; splitting the tests would remove the proof
from the capability that needs it. No performance claim or benchmark gate.
The strategy received one independent adversarial inspection before coding;
the author closure pass binds every applicable cell above to the implementation
and owners. Fresh full-diff review and final-SHA gates precede publication.
