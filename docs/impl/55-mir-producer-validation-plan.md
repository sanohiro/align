# MIR producer validation ownership

## Boundary and reason

The remaining interprocedural view-access repair in plan 52 needs a backend-independent
producer check shared by interface publication and every emission entrypoint.
The existing typed producer, initialization and backing-access validator operates
entirely on MIR but currently lives in `align_codegen_llvm`. Move that existing
validator into `align_mir` before extending its access domain. Preserve its
validation order, diagnostics and acceptance exactly.

This capability has an immediate consumer: current interface publication, whole
program emission and ThinLTO emission use the MIR-owned validator. It publishes
no dormant access summary or new source write path. The later interprocedural
capability still requires its own completed record, codec and strategy review;
this extraction does not close ordinary parameter/result laundering or R46.

The move exceeds 1,000 handwritten lines because the existing producer graph and
its type/resource validators form one dependency closure. Moving the complete
closure once avoids duplicated initialization/access proofs and leaves LLVM
with lowering and target-specific checks. File movement is not a language or
safety-strategy change. Plans 52 and 53 remain the authority for the moved proof.

## Exact implementation contract

- `align_mir::producer` owns the existing producer graph, initialization solver,
  typed value/backing access relations, call-component certification, resource
  schema checks, tagged/function-type validation, slice-index validation and
  fixed-element nulling validation.
- Its error type owns the existing diagnostic string. LLVM converts that error
  to its existing `CodegenError::Lowering` only at the backend boundary. Driver
  publication calls the MIR API directly. No LLVM context, target machine,
  native library or interface-crate dependency enters `align_mir`.
- Publication retains its existing order and exact certified local-function set.
  Emission retains additional generated/parallel/ABI preflight at its existing
  action points; publication must not start running emission-only checks.
- The small callable shape/capture-summary helpers already shared with emission
  have one MIR owner. They do not mint new authority or relax compatibility.
- No HIR/MIR variant, interface field, canonical byte, cache version, source
  qualifier, runtime allocation, ownership transfer or Drop path changes.
- The panic ratchet moves four existing occurrences from codegen to MIR; its
  combined allowance is unchanged. This is a location change, not new panic
  permission.
- Existing malformed-producer tests continue exercising publication, ordinary
  emission and ThinLTO. Do not replace them with tests that merely assert the new
  module path. A focused MIR-only owner proves the validator is callable without
  linking the LLVM backend.

## Implementation closure matrix

| Axis | Implementation and owner |
| --- | --- |
| Formation / validation / malformed input | Move existing graph/type/schema validators without semantic edits. Existing codegen malformed-producer, tagged, slice-index and fixed-element owners retain rejection and diagnostic order. |
| Construction / move / source nulling / Drop / replacement | No runtime change. Existing initialized/uninitialized, owned/shared/exclusive, cleanup companion, borrowed-place and native-output mutation owners retain the same verdicts. |
| Return / branch / loop / early exit | Move the complete founded equation solver and call-component worklist. Existing call/store-cycle and seeded-join owners retain the same convergence and rejection. |
| Generic / imports / serialization | Current interface producers invoke the MIR API after lowering; generic revalidation, imported certification, canonical signatures and cache bytes remain unchanged. Existing return_provenance and imported_mutable_retention driver owners apply. |
| Whole / per-unit / ThinLTO | All existing publication/emission entrypoints keep their original checks in their original order. Existing three-entrypoint producer mutation helper is the owner. |
| Callback / capture / retained carriers | Move exact callable-flow, target-relative capture and buffer proof helpers together. Plan 53 shared-string and initialization-cycle owners remain authoritative; no new callback domain is admitted. |
| Layering / allocation / performance | `align_mir` has no LLVM/interface dependency. One MIR-only validator smoke owner plus workspace dependency/build checks. No performance promise or benchmark gate; no runtime allocation change. |

The author-side matrix-to-diff pass must compare the moved implementation with
the original after normalizing only module paths, visibility and error wrappers.
Any semantic edit found by that comparison is resolved before review. The fresh
preflight review inspects this dependency closure and entrypoint wiring against
the already-reviewed proof strategy; a separate new analysis strategy is not
being approved in this refactor.
