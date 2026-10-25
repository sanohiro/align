# Builder validation live-state projection

## Measured defect and boundary

R37's authenticated inline-builder module reaches the supervised 3 GiB RSS
limit during both per-unit checking and MIR emission. A fresh post-Plan246
trace shows that MoveCheck's loops converge with small finite facts; a sample
taken at the subsequent memory growth places it in
`validate_builder_read_reservations`, specifically its exact CFG-state inventory.
Earlier startup samples of MoveCheck did not identify this peak phase. The
shared-helper module passes. A separate long-running native producer access
analysis remains a distinct performance boundary.

The reservation validator keeps full vectors for builder identities and boolean
facts, including SSA definitions whose final use has passed. Its visited key
therefore distinguishes histories that no later transfer can observe. Each read
also explores the whole CFG independently. Keep that exact transfer/validation
model in this capability; do not replace it with a widened ownership solver,
erase a reachable path, introduce a state budget that accepts incomplete proof,
or restrict the language.

Selected correction: project only incoming `values`, `value_bools` and
`slot_bools` onto conservative backward may-liveness sets before canonical
identity renaming and visited-state lookup. Preserve every builder-identity
slot, argument and reservation bit unchanged. Ownership-slot projection is deferred.
Use the validator's actual reads and unconditional overwrites as the authority.
Overapproximate successor reachability and uncertain dependencies. Two states
may merge only when every component observable by a future transfer before its
overwrite agrees. Unreferenced identities can disappear without reviving an
alias: every remaining identity is still renamed as an equality class, and a
new allocation remains fresh relative to every live alias.

## Implementation closure matrix

| Axis | Required proof and owner |
| --- | --- |
| Inputs and malformed graphs | Preserve the existing structural producer preflight. Checked accesses and invalid CFG/definition references fail closed; liveness never authenticates an otherwise invalid operation. Existing forged-MIR builder owners remain required. |
| State reads | Keep every SSA operand read by the validator: Use, Store, DropValue, builder observations/mutations/finish, every call family and Branch, including the read's owner snapshot. Eligible Boolean Loads retain their slot fact. Identity Loads and exclusive caller-place checks keep their existing owner-slot facts. Retire observes its operand and every subsequently observable alias. Unknown/unclassified dependencies remain live. |
| State overwrites | Kill the exact SSA id at its unconditional Let definition or the Boolean fact at an unconditional Bool Store, then add that statement's operand uses. Conditional retirement cannot kill aliases statically. Address-exposed/native-written Bool eligibility stays owned by `builder_boolean_slots`; do not turn an unknown Bool into a constant. |
| Sparse representation | Store only nonzero identity entries and known Boolean entries in ordered maps. Absent means exactly the old zero/None default; false remains explicitly present. Setter, retirement, projection and canonical renaming maintain that invariant. Iterate slots/values/arguments in the same ascending-index order, preserving equality classes and the exact state quotient. This changes storage cost, not the proof strategy or state-space limit. |
| Control and joins | Solve may-liveness over all structural successors, including loops and backedges. Project before equality/normalization at each block entry. No branch or diagnostic is pruned by the liveness solver. Existing boolean path pruning remains unchanged. Cover one/zero/repeated loops, branches, early exits and absent paths. |
| Construction, moves and source nulling | Keep New/call fresh identities, Load/Use/Store alias equality, null moves and fresh replacement unchanged. A dead name may be removed only when every path overwrites it before the next validator read. Cover live forwarded aliases across blocks and backedges. |
| Drop and mutation | Preserve Drop/build/DropValue retirement, mutation during index evaluation, transferred-owner invalidation and exclusive replacement. A real stale SSA or alternate live alias must still reject. Parameterize snapshot/read type siblings and valid post-call reloads. |
| Reservation | Preserve the pending snapshot's equality with its exact owner and keep the reservation bit unchanged. A future read must retain its snapshot SSA across every predecessor/backedge until the snapshot's next definition. Keep nested shared reads and independent mutations valid. |
| Artifacts and runtime | No HIR/MIR variant, public summary, cache schema, ABI, emitted code, runtime allocation or language change. Whole/partition/LLVM producer validation must retain the same verdict. Existing source/import/cache/native builder owners close the consumer boundary. |
| Work and resource evidence | A synthetic independent fixture with increasing dead boolean/owner histories must keep actual visited-state work bounded by its live frontier; compare exact validation verdicts with small unprojected controls. Replay the authenticated inline module and shared-helper control under the same 3 GiB/240 s supervisor after removing probes. Record compiler/source/phase and measured work/RSS, not an unsupported global asymptotic promise. |

The implementation should reuse one shared transfer-read inventory for liveness
and validation where practical, rather than add a parallel generic MIR visitor.
A private visited-state count may support the deterministic work owner; it is
not a user-facing metric or a production acceptance limit. Public/source
semantics remain the reviewed Plans209/210 contracts. No consumer source or pin
changes are authorized. If projection alone does not close the measured defect,
retain its evidence and reconsider this boundary before adding another model.

## Validation and measured result

`builder_reservation_live_state_work` compares projected/unprojected verdicts and
actual visited work over independent dead SSA/slot histories, both builder
families, live cross-block snapshots and injected retirement.
`builder_reservation_live_boolean_edges` covers a frozen SSA value after its
source-slot overwrite and a live backedge slot. Existing codegen `builder_*`
owners retain false/unknown/mutated Boolean, exclusive replacement, alias,
malformed producer and native-layout coverage. Driver `builder_copy_reads`,
`builder_byte_views`, `array_builder_transfer` and `text_builder_params` own
source/import/cache/native behavior. The last owner's stale diagnostic assertion
now accepts the existing earlier borrowed-place invalidation as well as snapshot
invalidation; both reject the same unsafe receiver/argument expression before
MIR production.

SSA-only projection and the independently reviewed Boolean-slot extension each
remained above the private RSS bound. The combined liveness and sparse-state
representation closes the authenticated module witness under the same
3 GiB/240 s supervisor: inline per-unit check completes in 33.828 s at sampled
peak 1,312,848 KiB; MIR emission completes in 31.180 s at 1,312,832 KiB. Both
helper controls complete near 3.07 s and 115 MiB. These are local macOS arm64
opt-level-1 observations, not an asymptotic bound or a cross-machine speedup.
The private binding/result manifests remain under `.git/handoff/r37-sparse-state`.

Per-read CFG slicing and ownership-slot projection remain deferred. The separate
whole-program native-producer CPU witness is outside this capability.
