# Validated codec observations

Status: implemented local codec observation boundary.

## Existing contract and reproduced boundary

The authoritative [codec ledger](core-design/codec.md) makes `codec.open` the
validator for the complete borrowed envelope, including every offset, count,
name and string cell. Batch and column accessors reuse that proof without
revalidation. Their source lifetime does not prove that the bytes stayed valid.

At `abee27a4229e4dab1db837c894ad7c88275aff8d`, both source and per-unit checking
accept this compile-only negative witness:

```align
import core.codec
fn main() -> Result<(), Error> {
  encoder := codec.encoder(0)?
  finished := encoder.finish()
  mut bytes := finished.bytes()
  batch := codec.open(bytes)?
  bytes[16] = 255
  print(batch.rows())
  Ok(())
}
```

Never execute a negative witness. The mutation changes the validated row count;
other offsets can invalidate the native accessors' memory-safety prerequisites.
This is a local observation boundary, independent of hidden interprocedural writes.

Reuse [plan 57](57-validated-text-observation-plan.md)'s reviewed byte-backing
snapshot, generation recency, alias overlap and validity-only invalidation
strategy. The observation's kind distinguishes UTF-8 text from a codec envelope.
No source type, qualifier, allocation, copy, owner transfer, native validation,
IR variant, runtime ABI or persisted format changes. This implements the existing
codec promise; its English/Japanese public ledger does not change. No additional
benchmark is required because this repair makes no performance/resource promise.

## Implementation closure matrix

| Axis | Implementation and regression owner |
| --- | --- |
| Formation | `record_value_completion` creates one codec-validation observation of the completed `CodecOpen.input`, attached only to `ResultOk`. Error payloads carry none. Generalize the private byte-validation kind/backing record; reuse Current/Prior renaming. `validated_codec_observation_local_matrix` covers direct, retained Result, `?`, `else` and reopening controls. |
| Source provenance | Keep the input's ordinary lifetime roots and allocation generations. Codec batch/column/name/string-cell projections carry the observation. Scalar results completed before mutation do not. `validated_codec_observation_projection_matrix` crosses batch counts/kind/find, all four column kinds, names and string cells with local/field/Option carriers. |
| Every byte destination | Existing indexed writes, vector store, map_into, shuffle, native OutBytes and direct/indirect Out/BorrowMut call invalidators end both kinds of byte validation. `validated_codec_observation_sink_matrix` pairs alias writes with disjoint targets; reuse the original UTF-8 sink owner. |
| Value snapshots and control | Preserve completed/eager operands, source snapshots and projected siblings through if/match/else/?/map_err/loop/early returns. A later write cannot erase earlier completed scalar use. `validated_codec_observation_control_matrix` and `validated_codec_observation_completion_matrix` cover joins, revalidation at a repeated site, retained prior observations and late eager writes. Existing `validated_byte_observation_state_closure` is generalized across both kinds. |
| Reopening and copies | Reopening creates a new observation and does not revive an old batch. Copying input bytes before opening gives independent backing; copying a derived string to owned text detaches its dependency. Converting an already-completed text view to bytes checks it first, then removes validation-only observations while preserving lifetime and read-only permission. Raw byte reuse remains available. Local/projection owners include these controls. |
| Lifecycle and alias identity | Do not mint an owner or end the backing storage when only its validation expires. Preserve move-in/out, source nulling, Drop, replacement, return and source expiry through existing generation/borrow machinery. Existing `core_codec` region/mutation owner and `validated_text_observation_backing_identity_matrix` remain required; the codec local owner adds disjoint/copy/rebinding controls. |
| Whole/per-unit/generic/cache | Generalized facts are recomputed after generic substitution and checked-HIR replay. Existing argument-derived identity summaries preserve caller observations; public lifetime serialization expands them to underlying roots. `validated_codec_observation_whole_unit_parity` crosses local/imported generic bodies and identity transport with invalidation/copy controls. Existing compiler identity invalidates changed checker implementations; no new interface field exists. |
| Malformed checked HIR | `validated_codec_observation_checked_hir_replay` changes one accepted same-typed selected batch to a stale batch after a write; core shape stays valid, body facts and lowering reject. Read-only peers still pass. |
| Diagnostics | Distinguish modified validated codec bytes from expired reader observations and from UTF-8-only validation. Advise reopening the codec after mutation or copying the input before opening. All negative matrices require an invalidation diagnostic; explicit mutable lifetime endings retain their existing diagnostic precedence. |
| Native and allocation parity | Runtime/accessor code is unchanged. `validated_codec_observation_safe_execution` exercises saved scalar values, independent input copies, reopening and raw-byte reuse in both compilation modes. Existing core_codec round-trip and whole/per-unit owners retain the no-copy borrowed accessor contract. No new allocation/FFI test or benchmark is added solely for compiler-private bookkeeping. |
| Deferred boundary | Hidden ordinary callee writes and callee-created codec validation remain with interprocedural access. This local repair does not export a guessed validation effect or strengthen unsafe raw stores. |

## Author completion requirements

Before code review, map every applicable row to the diff and its owner result.
Sweep every private validation predicate, projection filter, pruning/renaming
path and diagnostic branch for both observation kinds. A missing validation
record stays unknown and cannot prove disjointness. Preserve the existing source
read-only permission and all UTF-8 observation owners while adding codec facts.

This follows the reviewed local observation strategy without changing ownership
or native safety strategy; the author matrix pass and one final independent code
review close the boundary. The fresh review includes the codec capability scope.
