# Library boundary prerequisites

## Status

Design of record for the language/compiler work that must land before `pkg.db` implementation.
This is not a database-private escape hatch. It closes the general gap between ordinary Align
packages and native stateful libraries. `std.http`, `std.net`, `std.process`, and future
FFI-backed packages must be able to use the same ownership and borrow machinery.

Sections 7.5 and 7.6 are later, independently consumer-complete extensions for align-llm Requests
8 and 10. Both are implemented. Neither is a `pkg.db` prerequisite.

The implementation order in this document is mandatory. A database driver must not add another
closed `Ty`/HIR/MIR family that recognizes `pkg.db` names, and it must not expose `raw` handles or
manual close functions through its safe public API.

## Request 14 native filesystem boundary (design accepted; implementation pending)

The accepted `std.fs` publication extension is a consumer of the same general
native-boundary discipline recorded here, not a new package-private escape:

```text
fs.create_exclusive(path: str) -> Result<writer, Error>
fs.rename_no_replace(source: str, destination: str) -> Result<(), Error>
```

The implementation must form two distinct semantic/HIR/MIR operations and
thread them through checked-HIR validation, replay, generic rechecking, LLVM,
the typed runtime-key registry, and the native runtime. Path operands remain
borrowed `str` views; the existing nominal `writer` Move type and Drop path are
the only returned resource. The runtime boundary validates checkable length,
null, UTF-8, empty, and NUL conditions before side effects, constructs
ephemeral NUL-terminated path copies, and uses terminal OOM for actual
allocation failure. The create ABI clears its output slot before path
processing; rename marshals source before destination.

The native capability is deliberately direct: exclusive create and native
no-replace rename only. No ordinary replacing rename, existence-check race,
`link` plus remove, subprocess, filesystem-class query, path sandbox, or hidden
cleanup lock is permitted. The accepted Linux/macOS primitives, A08/A09 ABI
rows, whole/per-unit identity, and C6f2 consumer-owned trusted-path and
single-writer precondition are authoritative in
`docs/impl/27-fs-exclusive-publication-plan.md` and
`docs/impl/std-design/fs.md`. This section records the boundary before its
implementation activates any new `RuntimeKey` or validator variant.

## Asymmetric crypto native boundary (implemented 2026-08-30)

The post-pkg.db asymmetric signature suite is another consumer of this general boundary. Six
algorithm/class-specific builtin Move types share one payloaded `SignatureKey(kind)` compiler
representation and one runtime shell; the shell owns its repeated kind, private ordinary
`OSSL_LIB_CTX`, explicitly loaded built-in default provider, and provider-managed `EVP_PKEY`.
They are not package-defined resources and do not expose `raw`, a manual close, or a generic
algorithm selector. Constructors return a fresh
owner, ordinary moves null the source, aggregate replacement/Drop reaches the one null-safe
`align_rt_crypto_key_free`, and `borrow` sign/verify parameters retain neither key nor message.
One fail-closed structural carrier classifier admits local/by-value/return/shared-borrow and
recursive struct/sum/Option/Result ownership, including AoS Move-struct arrays, while rejecting
direct/tagged key collections, tuples/boxes, capture/parallel transport, mutable/out/global/native
exposure, and scalar observation. Bare key aliases need no import; only `crypto.*` spellings and
value operations require `std.crypto`.

Five generic operation shapes carry the closed `SignatureAlgorithm` discriminator through checked
HIR, MIR, canonical type identity, runtime-key selection, and LLVM: private PEM construction,
public PEM construction, decoded-JWK public construction, sign, and verify. The runtime repeats the
closed algorithm/class kind in every handle and rejects a mismatch before EVP. All output slots are
validated/alignment-checked and zeroed before work; every signed length and null/zero/positive input
view is validated before slice formation. Private PEM admits only canonical PKCS#8 v1
`PrivateKeyInfo` version zero through `d2i_PKCS8_PRIV_KEY_INFO` and `EVP_PKCS82PKEY_ex`; relabeled
legacy DER and `OneAsymmetricKey` reject. Base64 decodes into one exact `SensitiveDer` allocation,
and it plus every private re-encoding scratch is cleansed before free on all paths. Every
libctx/provider/context/key/PKCS#8/DER/BN/signature/shell allocation is released on failure, and no
owner or signature is published before complete validation. Each fallible OpenSSL call isolates its
thread-local error queue. Provider checks and verify exhaust a disjoint native-return ×
`Empty`/`InputOnly`/`CodeBearing` queue table: documented zero plus Empty/InputOnly is invalid data
or mismatch, CodeBearing dominates a zero, and negative/unsupported/unexpected returns are Code.
Decoder/import empty/unknown/resource/internal/fetch entries are Code. Every
decode/import/operation fetch uses exact `provider=default` in the private context,
and key/operation provider pointers must equal the shell's owned provider; global OpenSSL
configuration and providers are never consulted. Ed25519 PEM/JWK and private-derived public values
also pass wrapper-owned RFC 8032 canonical/on-curve/non-small-order validation independently of
provider `public_check`. Key import is trusted setup without a timing promise; fixed-public-length
signing uses no wrapper secret flow and relies explicitly on the pointer-verified built-in default
provider's constant-time primitive with RSA blinding retained.
The exact public surface, format/error/multi-invalid precedence, private-secret cleanup,
Move/control-path closure matrix,
stable discriminant bytes, A106–A109 ABI, interface/cache requirements, and one-capability-PR
boundary are authoritative in
`docs/impl/std-design/crypto.md` “Asymmetric signature suite”; the checked-HIR and runtime inventory
deltas are mirrored in `19-hir-validation-ledger.md` and `20-runtime-abi-ledger.md`.

## 1. Decisions

Seven prerequisites are accepted:

1. Recursive tagged Move payloads for ordinary structured `Option`/`Result`/sum errors and outputs.
2. Borrowed function parameters, including an invalidating mutable-borrow mode.
3. Package-defined opaque Move resources with exactly-once Drop.
4. Named arena bindings that expose a non-owning `region` capability to ordinary functions.
5. Deterministic static source inputs and per-unit query artifacts.
6. Region-backed `array_builder<PlainStruct>` with no hidden heap allocation.
7. Nested generic package APIs over the existing monomorphization model.

They preserve the existing model:

- ownership remains a property of the type;
- lifetimes and borrow roots remain inferred;
- there are no user-written lifetime parameters;
- there is no public trait hierarchy or allocator trait;
- unsafe/native construction stays visible in the declaring package;
- the compiler still has no build-script or macro language;
- MIR owns cleanup, borrow invalidation, and allocation semantics before LLVM lowering.

The dependency order is:

```text
recursive tagged Move payloads
  -> structured db.Error / compound Output in Result

borrowed parameters + interface summaries
  -> opaque resources + resource_ref + dependent resources
  -> named region capability
  -> region builder

deterministic static inputs
  -> static Query artifact

nested generic package APIs
  -> ordinary `db.query<P, R>` / `db.rows<P, R>` / `array<R>` package functions

all seven
  -> pkg.db SQLite/PostgreSQL vertical slices
```

## 2. Borrowed parameters

### 2.1 Surface

A parameter mode appears before the binding name:

```align
fn inspect(borrow c: Conn) -> i64
fn advance(borrow mut rows: Rows) -> Option<Row>
```

Call syntax does not change:

```align
n := inspect(conn)       // conn is not moved
row := advance(rows)     // rows is not moved; its previous borrow generation ends
```

`borrow` and `borrow mut` are contextual parameter-mode words, not types. They never appear in a
type argument, binding annotation, return type, struct field, or user-written lifetime. In
`borrow: T` the word is an ordinary parameter name; it is a mode only in `borrow name: T` or
`borrow mut name: T`. A normal by-value parameter retains the existing rule: passing a Move value
consumes it.

Shared `borrow` accepts a stable bound Copy or Move place. Copy already preserves caller ownership,
but explicit borrow avoids a structural by-value copy and uses the same non-null checked-place ABI.
`borrow mut` accepts either a Move type or a Copy type at a writable bound place: Copy preserves
ownership but does not propagate mutation, so exclusive in-place state updates are not redundant.

### 2.2 Shared borrow

For `borrow x: T`:

- a Move argument remains owned by the caller;
- the callee may inspect it but may not move, drop, replace, or consume it;
- a returned view may borrow from it;
- a borrow that does not reach the return value ends at the call;
- a returned borrow is tied to the exact caller-side owner generation;
- moving, replacing, or dropping that owner invalidates the returned value.

The first implementation accepts a bound local or a field place whose owning root is a bound local.
It rejects every unbound temporary or rvalue at a borrowed parameter because the call requires
stable caller storage and the language does not create a hidden addressable temporary.

### 2.3 Mutable borrow

For `borrow mut x: T`:

- the caller must provide a writable bound place;
- the call has exclusive access to that owner for the duration of the call;
- the call ends the owner's previous storage generation before its body executes;
- all older views rooted in that generation become invalid;
- a returned view belongs to the fresh post-call generation;
- the call does not consume the owner.

For a Copy aggregate, the same pointer-to-caller-storage ABI applies and field assignments remain
visible to the caller. Its old recursively view-bearing field provenance ends before the call, just
as for a Move owner. A scalar Copy value may use `borrow mut`, although an ordinary return is usually
clearer; the language does not add a shape-specific exception.

A recursively Move pointee remains owned by the caller and receives no callee function-exit Drop.
If the callee replaces it, the internal ABI also exposes the caller's cleanup-bit slot: the old
value's ordinary Drop plan runs before the store, then the slot receives the replacement's bit.
Leaving the pointee unchanged neither drops it nor changes that bit.

This deliberately uses one conservative rule. A mutable-borrow operation that happens not to
reallocate still ends the old generation. That may reject retaining an otherwise-safe view across
some mutations, but it cannot leave a package-defined mutation accidentally fail-open.

For every `borrow mut` argument, the checker recursively scans every other argument regardless of
whether that peer is `ByValue`, `Borrow`, `BorrowMut`, or `Out`. The call is rejected when the peer
place directly overlaps the mutable owner or when any provenance embedded in the peer value is
rooted in the owner generation being invalidated. This includes a Copy `str`, slice,
`resource_ref`, dependent resource, or either of two distinct aggregate holders containing such a
view. Two `borrow mut` arguments to the same owner and `borrow mut` plus overlapping `borrow`/`out`
are therefore ordinary cases of the same rule. Independently, an `out` slice may not overlap any
peer argument under any parameter mode.

The existing out-parameter no-alias proof and borrow-root analysis are the single implementation
mechanism for these checks. Argument expressions are evaluated before transfer, but generation
invalidation at call entry must not make another delivered argument dangling; such a call is
rejected rather than reordered or copied.

### 2.4 Effects and interfaces

Parameter modes are part of named-function signatures, function-value types, and interface hashes.
Concrete function values also carry the inferred return provenance of their target:

```text
Fn(
  [(ParamMode, Ty)],
  ReturnTy,
  Effect,
  ReturnBorrowSummary,
  ReturnRegionSummary,
  ReturnCleanupAbi,
)
ParamMode = ByValue | Out | Borrow | BorrowMut
ReturnCleanupAbi = None | DynamicBit
```

Binding or passing a named function preserves every mode. Indirect-call checking requires an exact
mode/type match and lowers `Borrow`/`BorrowMut` through the same pointer-to-caller-storage ABI as a
direct call. Mode erasure, implicit adaptation, and treating a borrow-mode target as by-value are
rejected. Binding preserves both return summaries and the return cleanup ABI. A mutable function
local or control-flow join unions the sorted parameter-index sets. Capture roots remain relative to
the selected function target and travel with that function value's environment; they are not
reinterpreted as slots in another target. The indirect result is tied to every owner/region that
the selected target may use. When its return type may carry borrow provenance, an unresolved
function-typed parameter conservatively includes every `Borrow`/`BorrowMut` input plus every
by-value/out input whose type recursively may carry a view, `resource_ref`, dependent resource, or
other embedded borrow — including Move inputs. Call transfer captures that embedded provenance
before nulling the moved source and attaches it to the indirect result. This does not legalize
returning a view of a consumed owner that dies in the callee; ordinary callee escape checking still
rejects that shape. It does preserve a moved dependent child or Move aggregate that is itself
returned. When the return may carry region provenance, the fallback includes every explicit
`region` input plus region provenance embedded in all compatible value inputs. Corrupt/out-of-range
summaries fail closed.

The inferred `Effect` and return summaries remain excluded from written source-level function-type
equality; parameter modes are not. They are concrete checked-HIR/interface facts consumed by
indirect calls. An outer public function that returns an indirect-call result exports its normally
inferred outer `ReturnBorrowSummary`/`ReturnRegionSummary`, so consumers never need the target body.

The checked-HIR analysis infers canonical return lifetime summaries:

```text
ReturnBorrowSummary
  None
  Roots {
    params: sorted parameter indices,
    captures: sorted target capture-slot indices,
  }

ReturnRegionSummary
  None
  Roots {
    params: sorted `region` parameter indices,
    captures: sorted target capture-slot indices,
  }
```

The summaries record which parameters or captured environment slots may back any view, dependent
resource, or region-owned value in the return. They are computed from the same exhaustive
provenance walk used by local borrow liveness. A named function has no capture slots; its parameter
sets are serialized in `IFnSig`, consumed during separate type checking, and included in
`interface_hash`. A concrete closure target stores its sorted capture-slot sets beside the closure
environment. An indirect call resolves those slots to the exact owner generations/regions held by
that selected function value. Moving the function value moves those roots with its environment;
the result may not outlive the environment or any captured owner. A join preserves the selected
target-relative capture metadata while conservatively unioning compatible parameter sets. An outer
named function that returns the result must resolve capture roots to its own parameters/captures or
reject the escape; an exported interface never contains an unbound capture slot.

`ReturnRegionSummary` records which explicit destination regions or captured regions may own the
returned value. A consumer therefore knows that `run(..., out)` cannot escape `out` without
importing the producer body. When several borrowed/region roots may contribute, the caller uses
their inferred shortest region.

Every recursively Move return has `ReturnCleanupAbi::DynamicBit`. Direct, indirect, and imported
calls return the value together with its path-selected cleanup bit. The caller stores that bit in
the result's path-local cleanup slot; tagged/aggregate Drop consults it. `Ok` may therefore return
arena-owned data with a clear bit while `Err` returns individually owned strings with a set bit,
without recomputing ownership from the joined region. `IFnSig`, `FnTy`, interface hashes, and ABI
fingerprints record that the extra bit exists, never its runtime value. Copy returns use
`ReturnCleanupAbi::None`. `ReturnRegionSummary` remains lifetime provenance and is not a substitute
for this dynamic ownership result. A later by-value call does not forward that bit as a parameter
ABI value: its callee assumes and drops free-standing storage. Escape analysis must therefore
reject a call result that may select an explicit caller region before moving it into an ordinary
by-value parameter, including after tagged success projection; a shared borrow remains legal.

The return-borrow summary is not limited to parameters spelled `borrow`. A by-value Copy view such
as `str`, `slice<T>`, `resource_ref<R>`, or a recursively view-bearing `db.exec` may back the
returned value and therefore appears in the same parameter-index summary. The `borrow` spelling is
needed to avoid consuming a Move owner or structurally copying stable Copy storage. Ordinary Copy
arguments remain by value unless the declaration explicitly chooses the no-copy borrow ABI.

`borrow mut` parameters are already explicit invalidation summaries. No second user annotation or
name-based effect table is allowed.

Mutation rooted exclusively in a `borrow mut` parameter is an explicit input effect and remains
`Pure` when the body performs no other Impure operation. It cannot touch a captured/global owner,
and call-site alias checking proves exclusivity. This is the same cost/parallel-safety category as
mutating an owned local or builder; it lets a deterministic row shaper update caller-owned state
without transferring an arena-owned Move value across a by-value call. Unsafe/FFI/database calls
remain Impure regardless of parameter mode.

A bound `array_builder<T>` may be passed to a `borrow mut` parameter and used as a mutable receiver
inside that call. The callee cannot consume it with `build()` or store/return it; ownership returns
to the same caller local. This does not make builders legal aggregate fields.

## 3. Package-defined opaque Move resources

### 3.1 Declaration

An opaque resource declaration names its exactly-once Drop hook:

```align
import pkg.db.internal.resource

pub resource conn = pkg.db.internal.resource.drop_conn
pub resource stmt<P, R> = pkg.db.internal.resource.drop_stmt
```

A resource declaration is not a data struct or sum type. It defines an opaque one-word native
owner. The type is always Move, non-null, non-Copy, non-printable, and non-comparable. Generic
parameters are phantom compile-time identity and are monomorphized; they do not change the one-word
runtime representation.

The Drop hook must resolve to a `pub` function in the declaring package's allowed `internal`
subtree and have this source shape:

```align
pub fn drop_conn(handle: raw) -> () {
  unsafe {
    native_conn_close(handle)
  }
}
```

Align has unsafe blocks, not unsafe-function declarations. The hook cannot return `Result`, capture
state, be generic, or be exported from the package as the public destruction operation. `pub` makes
it callable across modules only inside the package's `internal` import boundary.

Resolving a public resource declaration synthesizes a non-user-callable resource-drop thunk in the
resource-declaring producer. Its canonical symbol and ABI fingerprint are stored in resource type
metadata and the producer object is a link dependency of every consumer that may Drop the resource.
The thunk calls the internal hook; an importing consumer neither imports the internal module nor
resolves its source path. The thunk has hidden support-symbol visibility but ordinary external
linkability across compilation units. Align does not unwind; ordinary exits, `?`, value-carrying
control flow, reassignment, and scope cleanup call it exactly once. Process abort remains the
existing no-cleanup path.

The public resource interface serializes nominal resource identity, generic arity, representation
version, and thunk symbol/ABI fingerprint. It does not serialize the internal source path as an
importable API. Changing only the hook body changes producer implementation identity; changing the
thunk ABI/representation changes the public resource interface.

### 3.2 Native construction

`resource.borrow(owner)` is a public safe ownership operation wherever the opaque resource type is
visible. It returns a `resource_ref<R>` tied to the owner's current generation and exposes no native
representation. It is not representation-privileged.

Only the declaring module and its canonical descendant-module subtree may use
`resource.from_raw`, `resource.from_raw_borrowed`, `resource.view_from_raw`, `resource.raw`, and
`resource.into_raw`:

```text
resource.from_raw(handle) -> R
resource.raw(resource_ref<R>) -> raw
resource.into_raw(R) -> raw
```

These are independent signature forms, not one executable sequence. Each call occurs in `unsafe`.

Rules:

- `resource.from_raw` takes ownership of one non-null native handle; the expected result type selects
  the resource type, so there is no turbofish.
- `raw.is_null()` is the one check used before construction when a C API reports failure with null.
- `resource.borrow` is safe, available to ordinary consumers, and returns `resource_ref<R>` without
  granting representation access.
- `resource.raw` is unsafe and accepts `resource_ref<R>`, never an owning value.
- `resource.into_raw` is unsafe, consumes the resource, clears its cleanup flag, and transfers the
  native ownership to the caller.
- In v1 the `resource.into_raw` operand must be a standalone initialized resource root: a local or
  by-value resource parameter owned by the current function. A field, index, dereference,
  borrowed/out parameter, aggregate projection, or temporary is rejected. The root restriction
  keeps the existing one-cleanup-bit aggregate representation exact; raw transfer does not create
  field-level ownership state.
- Representation privilege is checked by canonical module-path prefix, not by an import cycle or a
  public constructor. For a resource declared in `pkg.db`, `pkg.db.sqlite`,
  `pkg.db.postgres`, and `pkg.db.internal.*` are privileged descendants; an importing application or
  another package is not.
- The `pub` Drop-hook module accepts only `raw` and need not import the resource-declaring module.
  The root may therefore reference `pkg.db.internal.resource.drop_conn`, while a driver descendant
  imports the root type and constructs it directly from an expected `db.conn`; `internal` never
  imports the root and the graph stays acyclic. Package `internal` visibility keeps the hook out of
  consumer source APIs; the generated root thunk supplies separate-compilation linkage.
- Module visibility is enforced even inside `unsafe`; unsafe does not bypass representation
  privilege.

`resource.from_raw` on null is an unsafe-precondition violation. Safe driver constructors must test
the native result and return a structured error before calling it.

### 3.3 Resource references

`resource_ref<R>` is a builtin Copy view:

- runtime representation: one native pointer;
- region/provenance: the exact generation of its resource owner;
- valid as a function parameter, return value, struct field, or sum payload;
- invalid after owner move, replacement, Drop, or a `borrow mut` operation on the owner;
- forbidden in FFI signatures and escaping task captures;
- raw extraction is restricted to the resource's declaring package.

The compiler propagates `resource_ref` provenance recursively through structs, tuples, sums,
`Option`, and `Result`. It must not add one-off region rules for each resource consumer.

### 3.4 Dependent resources and native views

Some owned native handles still borrow a parent native owner: a prepared SQLite statement depends
on its connection, and a row-result handle may depend on a statement or connection. The resource
itself therefore may carry inferred borrow provenance even though its runtime representation stays
one pointer:

```align
unsafe {
  stmt: stmt<P, R> := resource.from_raw_borrowed(native_stmt, resource.borrow(conn))
}
```

`from_raw_borrowed` takes exactly one `resource_ref<Parent>` dependency. Moving the returned
resource transfers that dependency; dropping it releases the dependency; the compiler rejects
moving/dropping/mutably borrowing the parent while the child resource is live. The Drop order is
therefore child before parent. A resource created by `from_raw` has no parent dependency.

Unsafe FFI wrappers also need to turn a native `(ptr, len)` into a zero-copy view with explicit
provenance:

```align
unsafe {
  text: str := resource.view_from_raw(resource.borrow(rows), ptr, len) else {
    return Err(native_shape_error())
  }
  data: slice<u8> := resource.view_from_raw(resource.borrow(rows), ptr, len) else {
    return Err(native_shape_error())
  }
}
```

The expected payload type selects `Option<str>` or `Option<slice<FFIScalar>>`. The operation returns
`None` for a negative/unrepresentable length, a non-empty null pointer, misalignment, or invalid
UTF-8 when `str` is requested. `len == 0` produces a valid empty view even when the native pointer
is null. The call remains `unsafe` because only the wrapper can prove that the foreign allocation
really covers the reported range and stays alive. It is restricted to the resource's declaring
declaring module subtree and ties `Some(view)` to the supplied owner generation. There is no
owner-free `raw.as_str`/`raw.as_slice` safe escape hatch.

These two operations are required general FFI boundary primitives. They are not SQLite/PostgreSQL
intrinsics.

### 3.5 Aggregate and thread rules

A resource may be a field of a one-owner Move struct or sum value. Recursive Drop follows the normal
aggregate order, and the aggregate has the existing single path-local cleanup bit. A resource cannot
be an element of a Copy fixed array or a pipeline element. A dynamic resource collection requires a
separate concrete consumer and is not part of this prerequisite.

A resource and any `resource_ref` are non-Send unless a future declaration form explicitly proves a
native type thread-safe. Version 1 has no `send` modifier. This keeps FFI library thread-safety
fail-closed.

## 4. Named region capability

### 4.1 Surface

An arena expression may bind its allocation capability:

```align
arena out {
  result := query.run(exec, params, out)?
  use(result)
}
```

The binding has builtin type `region`:

```align
fn build(rows: Rows, out: region) -> Output
```

Anonymous `arena { ... }` remains the short form when no ordinary function needs the capability.
Both forms open exactly the same runtime arena and `Region::Arena(id)` scope.

### 4.2 Ownership

`region` is a Copy, non-owning, scope-bound capability:

- it is created only by `arena name {}`;
- it cannot be returned, stored in an aggregate, placed in `Option`/`Result`, captured by or
  otherwise sent to any parallel worker (`spawn` or `par_map`), or passed through FFI;
- passing it to an ordinary function does not transfer ownership of the arena;
- every allocation performed through it receives that exact `Arena(id)` region;
- the existing escape analysis rejects a returned or stored value that would outlive that arena;
- arena cleanup remains owned by the lexical block, never by the `region` value.

The non-Send restriction is independent of inferred effect. A sequential direct call, local
closure, `map`, or `reduce` may capture a `region` under the ordinary lexical-region proof. The
implemented `pkg.csv` boundary gives every parallel worker one shared fail-closed worker-transfer
provenance gate before worker publication, MIR, generated identity, call, or allocation. The gate
consumes the existing finite `BorrowFact` trie and
`CallableProvenance`: it follows `ClosureTarget`/`ClosureCapture` paths through moves, assignments,
and control joins, recursively inspects nested function environments, and translates full same-
program direct/concrete-indirect helper summaries and imported `parallel_transfer_params` against
their completed actual arguments. An unresolved target or unavailable environment must select every
borrow-capable argument/capture; a named or lifted noncapturing function has an empty environment.

There is no allocator trait, allocator polymorphism, or user-created region. Cross-arena sharing is
not needed: a value belongs to its inferred shortest region, and an explicit copy is used when a
longer-lived destination is required.

### 4.3 Core operations

Core allocation operations that need a caller-selected destination accept `region` explicitly:

```text
array_builder(out)
value.clone_in(out)
```

`clone_in` is the one explicit copy into a region. For `str`/`bytes`, it copies the backing bytes and
returns a view tied to `out`. For a supported plain struct, it recursively copies view-bearing
fields. It rejects resources and independently owned Move fields.

An implementation must not use a thread-local "current arena" to make an ordinary package function
allocate into its caller. The region argument is the visible allocation authority.

## 5. Deterministic static source inputs

### 5.1 Build rule

The hermetic-build rule becomes:

> Normal `alignc build`/`check` reads reachable `.align` units plus static source inputs explicitly
> registered by a compiler-known constructor in those units. It never scans a directory, runs a
> script, reads a manifest language, contacts a service, or consults an environment variable to
> discover inputs. Explicit `alignc db` tool actions may consume explicitly named migration/database
> inputs under `pkg-design/db.md`; those are not normal-build discovery.

`db.query_file([])` and `db.command_file([])` are the first registered constructor family. Their
inline siblings register decoded static literal bytes with a tagged inline source identity rather
than pretending that a file exists. Driver-qualified variants select the same mechanism plus a
static driver restriction. This is compiler infrastructure, not a general compile-time evaluation
language.

The recognized arities are exact:

```text
db.query_file(common_query_options)
db.query_file(relative_path, common_query_options)
db.query(sql_literal, common_query_options)
driver.query_file(common_query_options, native_query_options)
driver.query_file(relative_path, common_query_options, native_query_options)
driver.query(sql_literal, common_query_options, native_query_options)

db.command_file(common_command_options)
db.command_file(relative_path, common_command_options)
db.command(sql_literal, common_command_options)
driver.command_file(common_command_options, native_command_options)
driver.command_file(relative_path, common_command_options, native_command_options)
driver.command(sql_literal, common_command_options, native_command_options)
```

Every option argument is the fixed literal sum list specified by `pkg-design/db.md` §13. There is
no omitted/default option argument; `[]` is the explicit empty list. The path/SQL and static option
expressions are definition-time inputs, not general runtime calls.

### 5.2 Resolution

For a constructor in `/root/db/queries/user.align`:

```text
db.query_file([])                 -> /root/db/queries/user.sql
db.query_file("shared.sql", [])   -> /root/db/queries/shared.sql
```

Rules:

- the path-free form replaces the defining `.align` extension;
- the explicit form is relative to the defining module's directory;
- only a compile-time string literal is accepted;
- absolute paths, lexical `..` escape, and symlink escape outside the project/package root are
  rejected;
- the logical path stored in artifacts is root-relative with `/` separators;
- the file must be valid UTF-8;
- Query/command SQL must contain no U+0000; screening reports its exact span before artifact
  generation so a length-aware source identity can never disagree with a NUL-terminated native API;
- the exact source bytes are the static-input bytes and receive a `source_sql_hash`; no newline
  normalization occurs;
- database wire bytes are a separate deterministic artifact field: SQLite uses the exact source
  bytes, while PostgreSQL replaces only recognized named-parameter token spans with `$n` and
  preserves every other byte;
- the file is registered in `SourceMap` so SQL diagnostics carry file/byte spans.

Inline `db.query(sql_literal, ...)`/`db.command(sql_literal, ...)` uses:

```text
SqlSourceIdentity
  File { root_relative_logical_path }
  Inline { query_id }
```

For `Inline`, `source_sql` is the exact UTF-8 string value after Align escape decoding. The artifact
stores a decoded-SQL-byte to defining-`.align`-literal span map for diagnostics; that diagnostic map
is not a filesystem identity. The defining `.align` file is already a normal unit input, while the
decoded bytes/source hash and `Inline { query_id }` participate in the Query artifact and producer
implementation identity. User-facing diagnostics name the Query item and its literal span, never a
synthetic `.sql` path.

### 5.3 Unit identity and cache keys

Each unit has a sorted static-input list:

```text
StaticInput
  source = File(root_relative_logical_path) | Inline(query_id)
  content_hash
  consumer_kind
  driver_restriction
```

Registration is based on the resolved callee identity, never a textual path match. A local `db`
binding, an unimported path, or a user function with the same spelling registers nothing and cannot
cause a file read or missing-file diagnostic.

Only `File` entries are read before a frontend-cache lookup. `Inline` bytes come from the already
parsed unit; they still use this tagged record for artifact/action-key canonicalization and can never
request filesystem I/O. Canonical list order is source tag (`File = 0`, `Inline = 1`), then UTF-8
payload bytes, then consumer kind, then descriptor ID as the tie-breaker for two descriptors sharing
one source; content hashes never decide ordering.

The producer action identity includes the unit source/import digest plus this list. At the shipped
cache boundary, name resolution/frontend still runs before the list is first produced, and
`impl_hash` includes every static input that changes generated MIR/data. This forces a producer
object miss and relink even when no `.align` byte changed.

To preserve an eventual pre-frontend no-op hit without guessing from syntax, the driver may persist
a versioned `StaticInputManifest` emitted by successful import/name resolution. It is keyed by the
exact source/import-resolution digest and records constructor identity, tagged source identity,
kind, and every derived checked-metadata dependency. For each descriptor and each permitted driver
the metadata entry is:

```text
CheckedMetadataInput {
  driver,
  logical_path,
  state: Missing | Present { content_hash, format_version },
}
```

The logical path is exactly
`.align-db/{sqlite|postgres}/{Hash128::of(descriptor_id.as_bytes()).to_hex()}.json` as fixed by DB
§16.3, where `descriptor_id` is the Query ID or command ID; no directory scan is allowed. Before a
frontend-cache lookup, a matching manifest re-stats and, when present, reads and hashes each exact
`File` and checked-metadata path. Creation, deletion, content change, or format version change
therefore changes the action key. `Inline` SQL never causes a source-file read, but
its derived checked-metadata entries are still validated. A missing or mismatched manifest runs
import/name resolution and regenerates it before any static file is treated as an input. The
manifest is a cache index, not a build manifest, and is never accepted across a
source/import/schema digest change. File and directory mtimes never participate.

## 6. Static Query artifacts and separate compilation

### 6.1 Query identity

A recognized static Query/command constructor is legal only as the complete single-expression body
of a named, zero-argument, non-generic descriptor function:

```align
pub fn query() -> db.query<Params, Row> =
  db.query_file("user_by_id.sql", [])
```

The body contains exactly one resolved recognized constructor call. It cannot be conditional,
repeated, placed in a block/loop, nested in another expression, or wrapped by a user helper.
Static constructors are rejected everywhere else. A descriptor may be private for same-module use;
`pub` is required to expose it through a module interface. This restriction gives each constructor
one stable item identity and prevents artifact/thunk slots from colliding.

A Query ID is:

```text
fully-qualified module path + descriptor function name
```

It never contains an absolute filesystem path. Renaming the module or descriptor creates a new
Query identity; moving only the SQL file through an explicit linkage does not.

### 6.2 Artifact split

The compiler emits a `StaticQueryArtifact` beside the normal interface/object artifacts:

```text
StaticQueryArtifact
  format_version
  unit
  item
  query_id
  Params canonical structural contract
  Row canonical structural contract
  Params canonical fingerprint
  Row canonical fingerprint
  binder ABI version
  decoder ABI version
  driver restriction
  static semantic options
  SQL source identity: File(logical path) | Inline(query_id)
  source SQL exact bytes
  source SQL hash
  per-allowed-driver wire SQL exact bytes/hash
  source-to-wire rewrite map and rewrite-format version
  parameter occurrence table
  per-driver binding plan and retention classes
  source-span map
  per-driver checked-metadata policy/state/digest
```

`StaticCommandArtifact` uses the same versioned envelope and discovery/cache mechanism, with these
exact differences:

```text
StaticCommandArtifact
  format_version
  unit, item, command_id
  Params canonical structural contract
  Params canonical fingerprint
  binder ABI version
  driver restriction
  static semantic options
  SQL source identity and exact bytes/hash
  per-allowed-driver wire SQL exact bytes/hash
  source-to-wire rewrite map and rewrite-format version
  parameter occurrence table and source-span map
  per-driver binding plan and retention classes
  per-driver checked-metadata policy/state/digest
```

The canonical artifact codec is:

```text
magic                ASCII "ALIGNQRY" | "ALIGNCMD" (8 bytes)
format_version       u32 little-endian
integer/ABI version  u32 little-endian; native type IDs use two's-complement i64 little-endian
enum/bool/tag         u8 using the explicit tags below
Hash128               lo u64 then hi u64, little-endian
string/byte field     u32 little-endian length, then exact bytes
Option                u8 0 | 1, then payload when 1
sequence              u32 little-endian count, then elements in semantic order
```

The complete v1 tags are:

```text
Driver                  SQLite = 0 | PostgreSQL = 1
DriverRestriction       AnySupportedDriver = 0 | SQLiteOnly = 1 | PostgreSQLOnly = 2
SqlSourceIdentity       File = 0 | Inline = 1
CheckPolicy             DeclaredOnly = 0 | CheckedOptional = 1 | CheckedRequired = 2
VerificationState       Declared = 0 | DatabaseChecked = 1
BindRetention           BindValue = 0 | BindCopy = 1
StaticOptionOwner       Common = 0 | SQLite = 1 | PostgreSQL = 2
CanonicalType           Named = 0 | Tuple = 1 | Fn = 2
CanonicalDefinition     Struct = 0 | Sum = 1
MetaStatementClass      Select = 0 | Dml = 1 | Ddl = 2 | Native = 3 | Unknown = 4
MetaNullability         Yes = 0 | No = 1 | Unknown = 2
```

`CanonicalType` uses the existing `align_interface` type-reference subencoding: `Named` is
`tag, path: string, args: sequence<CanonicalType>`; `Tuple` is
`tag, elems: sequence<CanonicalType>`; and `Fn` is
`tag, params: sequence<CanonicalType>, result: CanonicalType`. Params and Row roots are fully
substituted and contain no type parameter. A stored Params/Row contract is not only that nominal
root. Its exact structural encoding is:

```text
CanonicalContract
  root: CanonicalType
  definitions: sequence<CanonicalDefinition>

CanonicalDefinition
  path: string
  args: sequence<CanonicalType>
  kind: u8
  Struct => fields: sequence<{ name: string, type: CanonicalType }>
  Sum    => variants: sequence<{ name: string, payload: sequence<CanonicalType> }>
```

`definitions` contains every reachable user-defined instantiated type exactly once, including the
root definition, sorted by the complete encoded `(path, args)` key. Builtins have no definition
entry. Struct fields and sum variants/payloads use source declaration order. Recursive edges remain
`Named` references, so the graph encoding terminates. Two definition entries with the same key, a
missing reachable definition, an unreachable extra definition, or a non-substituted type parameter
is invalid. Params/Row fingerprints are `Hash128::of` over the complete stored
`CanonicalContract`, so field name, order, type, `Option` shape, variant, or reachable layout changes
the fingerprint even when the nominal path is unchanged.

The exact nested records are:

```text
Span
  start: u32                         # inclusive UTF-8 byte offset
  end: u32                           # exclusive UTF-8 byte offset

SqlSourceIdentity
  tag: u8
  File   => logical_path: string
  Inline => query_or_command_id: string

StaticOption
  owner: u8
  variant: u8
  payload:
    Common/0     => policy: CheckPolicy
    SQLite/0     => major: u32, minor: u32, patch: u32
    PostgreSQL/0 => parameter_name: string, canonical_type_name: string

ParameterOccurrence
  source_name: string
  source_span: Span
  protocol_ordinal: u32              # one-based

RewriteEntry
  source_span: Span
  wire_span: Span

BindingEntry
  params_field_ordinal: u32          # zero-based declaration order
  source_name: string
  protocol_ordinal: u32              # one-based
  field_type_fingerprint: Hash128
  retention: BindRetention

DeclaredParameterMeta
  ordinal: u32                       # one-based protocol ordinal
  source_name: string
  logical_type: string

DeclaredColumnMeta
  ordinal: u32                       # zero-based decoder ordinal
  source_alias: string
  logical_type: string

CheckedParameterMeta
  ordinal: u32
  native_type: Option<string>
  native_type_id: Option<i64>

CheckedColumnMeta
  ordinal: u32
  native_type: Option<string>
  native_type_id: Option<i64>
  origin_schema: Option<string>
  origin_table: Option<string>
  origin_column: Option<string>
  nullable: MetaNullability

CheckedQueryEvidence
  prepare_identity: string
  schema_identity: string
  server_identity: string
  parameters: sequence<CheckedParameterMeta>
  columns: sequence<CheckedColumnMeta>

QueryMetaPlan
  statement_class: MetaStatementClass
  parameters: sequence<DeclaredParameterMeta>
  columns: sequence<DeclaredColumnMeta>

DecodedSpanEntry
  decoded_span: Span
  defining_file_span: Span

CheckedMetadata
  policy: CheckPolicy
  state: VerificationState
  Declared        => no further fields
  DatabaseChecked => metadata_format_version: u32, metadata_digest: Hash128,
                     query_evidence: Option<CheckedQueryEvidence>

DriverEntry
  driver: Driver
  wire_sql: bytes
  wire_sql_hash: Hash128
  rewrite_format_version: u32
  rewrites: sequence<RewriteEntry>
  bindings: sequence<BindingEntry>
  checked_metadata: CheckedMetadata
```

The static option variants correspond exactly to `db.QueryOption.Check`/
`db.CommandOption.Check`, `sqlite.*Option.RequireVersionAtLeast`, and
`postgres.*Option.ParameterType`. A new static option variant is an artifact format change. Before
encoding, duplicates/conflicts are rejected and options are sorted by
`(owner, variant, complete payload bytes)`. `[]` is encoded as the single effective
`Common/0/DeclaredOnly` option, so an omitted default and its explicit spelling have one artifact
identity.

There is one `ParameterOccurrence` for every placeholder occurrence, ordered by
`source_span.start`; equal or overlapping source spans are invalid. Repeated source names repeat in
this table with the same `protocol_ordinal`. There is one `BindingEntry` for every Params field,
ordered by `params_field_ordinal`; the ordinals must be dense from zero, source names unique, and
protocol ordinals dense from one in first-source-occurrence order. The occurrence-name set and
binding-name set must be equal; an unused Params field or placeholder without a field is rejected
before encoding. `field_type_fingerprint` hashes the complete `CanonicalContract` rooted at that
fully substituted Params field type.

`QueryMetaPlan.parameters` and `.columns` use dense ordinals in their documented bases and contain
the declared names/types used by D12 at Summary/Full. Checked evidence uses the same ordinal sets
and order. It is `Some` for a DatabaseChecked Query and `None` for a DatabaseChecked command;
Declared entries have no evidence payload. The producer rejects checked evidence whose statement
class, ordinals, counts, or identities disagree with the declared plan.

There is one `RewriteEntry` for every placeholder occurrence, including identity rewrites, in the
same order as `occurrences`. Each pair names the complete source and wire placeholder spans.
Non-placeholder positions are translated by the cumulative length delta of preceding entries; a
position inside a replacement maps to its complete peer span. Source and wire spans must be
monotone, non-overlapping, and within their respective SQL byte lengths. This is the complete v1
source-to-wire map; an implementation may not invent an additional unstored mapping rule.

`DecodedSpanEntry` is empty for `File`. For `Inline`, entries are the maximal coalesced runs having
one affine decoded-to-file byte mapping, ordered by `decoded_span.start`; they are non-overlapping
and cover every decoded SQL byte exactly once. Escape expansions therefore form their own entries.
All strings, SQL, and mapped source bytes are UTF-8. Any byte length, element count, or offset above
`u32::MAX` is a compile error before artifact creation.

The top-level byte order is exact:

```text
StaticQueryArtifact
  magic = "ALIGNQRY"
  format_version: u32
  unit: string
  item: string
  query_id: string
  params_type: CanonicalContract
  row_type: CanonicalContract
  params_fingerprint: Hash128
  row_fingerprint: Hash128
  binder_abi_version: u32
  decoder_abi_version: u32
  driver_restriction: DriverRestriction
  static_options: sequence<StaticOption>
  source_identity: SqlSourceIdentity
  source_sql: bytes
  source_sql_hash: Hash128
  occurrences: sequence<ParameterOccurrence>
  driver_entries: sequence<DriverEntry>
  decoded_span_map: sequence<DecodedSpanEntry>
  query_meta_plan: QueryMetaPlan

StaticCommandArtifact
  magic = "ALIGNCMD"
  format_version: u32
  unit: string
  item: string
  command_id: string
  params_type: CanonicalContract
  params_fingerprint: Hash128
  binder_abi_version: u32
  driver_restriction: DriverRestriction
  static_options: sequence<StaticOption>
  source_identity: SqlSourceIdentity
  source_sql: bytes
  source_sql_hash: Hash128
  occurrences: sequence<ParameterOccurrence>
  driver_entries: sequence<DriverEntry>
  decoded_span_map: sequence<DecodedSpanEntry>
```

Version 1 encodes `format_version = 1`. `unit` is the canonical fully-qualified module path and
`item` is the unqualified descriptor function identifier. `query_id`/`command_id` is the exact
`unit + "." + item` byte string; an Inline source identity repeats that same ID.
Maps are never encoded by map iteration. `driver_entries` contains exactly the restriction's
permitted set in `SQLite`, then `PostgreSQL` order. `CheckedMetadata.policy` must equal the effective
common static `Check` option for every entry. Hash fields must equal `Hash128::of` over their named
exact byte field. IDs must equal the exact `unit + "." + item` identity rule. A decoder rejects
an unknown tag/version, duplicate/missing/unreachable type definition, duplicate or out-of-order
element, invalid span, fingerprint/hash mismatch, restriction/driver mismatch, policy/evidence
mismatch, ID mismatch, trailing byte, or truncated field.

Binder/decoder ABI versions are `u32`; changing a generated thunk calling/layout contract increments
the corresponding version even when the logical type is unchanged. The artifact digest is
`Hash128::of` over the complete bytes beginning at `magic`.

L5 checks in one Query and one command semantic fixture plus their exact
`crates/align_driver/tests/golden/static_{query,command}_v1.hex` bytes and sibling `.digest` files
containing 32-lowercase-hex `Hash128::to_hex()` values. The Query fixture is portable, uses repeated
named parameters, a nested structural Row definition, an inline escape, both retention classes,
mixed Declared/DatabaseChecked driver state, non-empty QueryMeta evidence, and non-empty rewrite/span
maps. The command fixture is PostgreSQL-pinned, file-backed, has
`ParameterType` and `CheckedRequired`, and proves the omitted Row/decoder fields. A standalone
test-only reference encoder implements the table above without calling the production artifact
codec and produces the reviewed goldens. Tests decode each golden, compare every semantic field,
re-encode byte-for-byte, and separately encode the semantic fixture and compare the checked-in
bytes and digest. Updating a golden requires an intentional `format_version` change or a
test-reviewed correction to this contract; round-trip success alone is not acceptance.

It has the same generated Params binder and source/wire/cache invalidation rules as a Query. It has
no Row contract, result-column metadata, or decode thunk. `command_id` uses the same fully-qualified
descriptor-item identity rule as `query_id`. A command SQL-only edit changes the producer
implementation/artifact and relink input without changing an unchanged `IStaticCommand`.

The public interface carries only facts needed to type-check a consumer:

```text
IStaticQuery
  item
  Params type
  Row type
  driver restriction
  static semantic options
```

The corresponding `IStaticCommand` omits Row. Both use the same interface/implementation split.

These facts participate in `interface_hash`. Source/wire SQL bytes and hashes, rewrite maps,
occurrence tables, checked metadata, and generated bind/decode thunks participate in the producer's
`impl_hash` and query artifact, not the consumer interface hash. Editing SQL without changing its
public typed contract recompiles and relinks the producer but does not recompile consumers.

Compiled-library distribution bundles query artifacts as a separate CAS-addressed part. It does not
smuggle SQL through generic function bodies or require source-path reconstruction.

### 6.3 Descriptor ABI

`db.query<P, R>` is a compiler-known Copy descriptor. Its runtime value points to immutable static
data owned by the producer unit:

```text
QueryStatic
  query ID/hash
  artifact digest
  driver mask
  source-SQL hash
  per-driver wire-SQL pointer/length/hash
  per-driver rewrite-format version
  static-option pointer
  per-driver binding-plan pointer/retention classes
  producer-owned QueryMetaPlan and per-driver checked-evidence pointers
  generated bind thunk
  generated decode thunk
  D12-generated QueryMeta materialization thunk
  per-driver checked-metadata state/fingerprint
```

The command sibling is:

```text
CommandStatic
  command ID/hash
  driver mask
  per-driver wire-SQL pointer/length/hash
  static-option pointer
  per-driver binding-plan pointer/retention classes
  generated bind thunk
  per-driver checked-metadata state/fingerprint
```

For Query, `P` and `R` are compile-time phantom contracts; for command, only `P` exists. The generated
binder reads known field offsets from `P`; the Query decoder writes known offsets in `R`. There is no
per-row field-name lookup, reflection, boxing, or map allocation.

The QueryMeta thunk is generated code, not reflection. Given the selected driver, `MetaDetail`, and
explicit output region, it materializes exactly the Summary/Parameter/Column rows from the
producer-owned plan and checked-evidence tables. It never reads source files, `.align-db`, interface
summaries, decoder code, or a database at runtime. D1 emits and tests the producer-owned plan for
Declared Queries; D3/D5 populate checked evidence; D12 introduces the exact thunk ABI, generated
code, descriptor-header version, and ordinary package call together with that thunk's first native
consumer. Commands carry no QueryMeta plan or thunk.

The producer's descriptor function returns the corresponding static pointer. When the item is
`pub`, its interface exports that descriptor contract. Generated thunks stay in the producer object
and are referenced by the static descriptor, so a consumer does not need the Query/command body to
call it.

The runtime selects the already-generated wire entry after checking the connection's driver mask
and sends those bytes exactly. A PostgreSQL rewrite never becomes the source/build identity. The
artifact maps each rewritten byte range back to its source parameter span so prepare/runtime
diagnostics can report the `.sql` source; positions the engine cannot map are reported at the Query
item. Checked metadata keys include driver, source hash, wire hash, rewrite-format version, and
static options.

## 7. Region-backed plain-struct builder

### 7.1 Surface

The existing builder grows one coherent surface:

```text
array_builder()       // individually owned heap storage; existing behavior
array_builder(out)    // region storage selected by an explicit region capability
push(value)
build() -> array<T>
```

The argument count makes the allocation home visible. There is no database-private vector.
Both forms remain one-owner mutable locals. A helper may push through a `borrow mut` builder
parameter, but a builder is not a `RegionPlain` value, cannot be stored in the shaping state, and is
consumed only by the caller's final `build()`.

### 7.2 Plain element class

The region form accepts `RegionPlain` elements:

- primitive scalars, `bool`, and `char`;
- fixed-size vectors/masks of supported scalar lanes;
- `str`/`bytes` whose inferred source region outlives or equals the destination region;
- `Option` and fixed arrays of `RegionPlain`;
- structs and sum payloads recursively composed of `RegionPlain`.

It rejects:

- `resource` and `resource_ref`;
- `raw` and function values;
- independently owned `string`, dynamic `array`, `box`, or another builder;
- a view whose owner generation ends before the destination region.

For streamed database text, the shaper must call `value.clone_in(out)` before `push`. Storing a
current-row view and then advancing the row stream remains a compile error.

### 7.3 Growth and freeze

A region builder grows geometrically in arena chunks. It never allocates a hidden heap vector.
`build()` allocates the exact contiguous result buffer in the same region and performs one compacting
element pass. The result is arena-owned (`cleanup bit = false`) and has the destination region.

The final pass is an explicit documented cost of the region builder. The spec does not promise an
impossible general combination of unknown length, bump allocation, contiguous output, and zero-copy
freeze. The existing heap form retains zero-copy freeze.

The implementation may elide the compacting pass only when it proves the current region allocation
is already the exact final contiguous buffer. This is an optimization, not a semantic branch.

### 7.4 Nested generic package APIs

Ordinary package code must be able to express the generic types used by its public functions:

```align
fn rows_stmt<P, R>(borrow c: conn, q: query<P, R>, p: P) -> Result<rows<R>, Error>
fn all<P, R: RegionPlain>(
  borrow c: conn,
  q: query<P, R>,
  p: P,
  out: region,
) -> Result<array<R>, Error>
```

L7 permits `Ty::Param` recursively beneath `Option`, `Result`, `array`, `slice`, and applications of
top-level generic struct, sum, and resource definitions in a generic function's parameters, locals,
and return type. It permits `query<P, R>`, `stmt<P, R>`, `rows<R>`, and comparable ordinary generic
definition applications inside a generic function. It does not declare a generic definition inside
another function, add written type arguments at calls, or admit a concrete container element that
the container already rejects.

`RegionPlain` is a closed builtin structural bound alongside the existing fixed bounds, not a
user-defined trait or public trait hierarchy. It grants only operations whose implementation is
defined for recursively `RegionPlain` values, including region-builder push/build and an
`array<R>` result. A concrete instantiation recursively validates the exact substituted type.
Resources, independently owned fields, raw values, functions, and builders fail the bound. The
abstract template checker may use only the operations granted by the written builtin bound.

Nested parameter matching and expected-type inference recurse through these type constructors.
Generic struct/sum/resource applications containing `Ty::Param` remain symbolic in the template and
are instantiated only after concrete arguments are known. Monomorphization still runs to a
fixpoint before MoveCheck, EscapeCheck, MIR, and ABI lowering; there are no runtime dictionaries,
virtual calls, user traits, turbofish, or runtime type reflection.

Public generic interfaces serialize the canonical nested type expression, builtin bounds, and body
template. A monomorphization key is the defining item identity plus the canonical fully substituted
type-argument tuple and compiler schema version. Whole-program and interface-only instantiation
must produce identical keys, `RegionPlain` decisions, Drop plans, diagnostics, and ABI.

A1/D13 adds one narrow completion without reopening L7's container surface. `soa<R>` may occur in a
generic template exactly when `R: SoaPlain`, where `SoaPlain` accepts a nonempty struct of
integer/float/`bool`/`char`/`str` fields. The public template interface serializes the canonical
symbolic `soa<Param(R)>` expression and bound so a separate consumer can reconstruct it. Concrete
instantiation reruns the ordinary SoA admission rule and substitutes `Ty::Soa(struct_id)` before
MoveCheck, EscapeCheck, and emitted HIR/MIR. No abstract SoA, generic thunk, dictionary, reflection,
or DB-named compiler operation reaches runtime code.

### 7.5 Individually owned heap-record builder

Status: implemented for align-llm Request 8 on 2026-08-14.

The heap constructor keeps the existing syntax and allocation visibility:

```text
array_builder<T>()       // T is inferred from the annotated binding
b.push(value: T)         // consumes value when T is Move
b.append(xs: slice<T>)   // remains Copy-scalar-only
b.build() -> array<T>    // consumes the builder; zero-copy freeze
```

This capability admits declared records to the existing individually owned heap form. It does not
add a constructor, type argument syntax, allocator input, ambient option, conversion, or second
collection abstraction. The explicit-region form in §§7.1–7.4 remains the only builder that accepts
borrowed views.

#### Public-contract ledger

| Field | Contract |
|---|---|
| Surface and owner | `core.array_builder`, owned by Sema formation/call checking, Move/Escape analysis, MIR cleanup, LLVM lowering, and the runtime allocation primitives. `array_builder()` infers `T` from `array_builder<T>` exactly as today. |
| Heap record predicate | `HeapRecord(S)` holds only for a nonempty, acyclic declared struct whose fields, recursively in source declaration order, are integer/float/`bool`/`char`, owned `string`, or another `HeapRecord`. The root and every nested struct use natural Align layout with alignment at most 8 and have neither `align(N)` nor `layout(C)`. Copy-scalar-only records are Copy; a reachable `string` makes the record Move. |
| Closed exclusions | Direct or nested `str`, `slice`, `resource_ref`, resource, `raw`, function, builder, dynamic/fixed array, `Option`, `Result`, sum/enum, tuple, box, independently owned non-string collection, empty struct, explicit layout/alignment, unknown definition, and inline cycle reject before builder construction. No exclusion is inferred from an opaque existing Drop plan. |
| Push and move | `push` mutably borrows the builder. A Copy record is copied once. A Move record transfers the complete value, and the source is nulled at the same move boundary before any later source Drop. There is no implicit clone, JSON path, per-element arena, or shallow-copy fallback. |
| Allocation mode | Every reachable `string` in a pushed Move record must be free-standing. Sema/MIR proves the complete value has individual ownership before the growth side effect; arena-owned, mixed, or path-dependent ownership rejects at the first source-order owner path. Copy fields contribute no allocation mode. Relocation preserves owner bytes without dropping the old byte positions. |
| Builder ownership | The record form follows the existing builder owner model: one Move handle, mutable local receiver, existing by-value function transfer and `borrow mut` rules, no aggregate/`Option`/`Result` storage, and no task/closure capture. A by-value call moves the builder; a `borrow mut` helper cannot consume or retain it. `build` consumes it. Scalar and `string` behavior is unchanged. |
| Append | `append` stays available only for existing Copy scalar elements. It never bulk-copies a record, `string`, view, or other Move value. |
| Growth and build | Heap growth remains amortized `align_rt_realloc` storage. Record relocation is bytewise because accepted records contain only scalars and external string owners, never self-relative views. `build` transfers the initialized buffer without a second element allocation and returns the ordinary AoS `array<S>`. |
| Drop and cleanup | The compile-time element type owns one canonical recursive `DropPlan`. Dropping an unfinished builder drains exactly its initialized prefix through the same array-element Drop plan, then frees storage/header. A successful build transfers that obligation to ordinary `array<S>` Drop. No runtime reflection, callback table, or self-describing element wire is added. |
| Failures | Type, placement, source-move, exact-type, and allocation-mode failures are compile diagnostics before constructor/push side effects. Capacity arithmetic and allocator failure retain the existing terminal-abort policy and never return partial success. Partial source-record construction is cleaned by the existing aggregate owner before `push` begins. |
| Validation order | Parser/import/arity and expected-type inference; recursive `HeapRecord` formation; builder placement/receiver; source move state and exact element type; recursive individual-allocation proof; build result escape/cleanup. Whole-program, interface-backed, and cache-replay paths use the same first failure. |
| Interfaces and identity | The canonical identity is the existing nominal producer item plus its serialized reachable struct-definition graph, layout, ownership facts, and compiler interface schema. `Ty::ArrayBuilder(Scalar::Struct(id))` and `Ty::DynStructArray(id, Aos)` remap that identity through the existing interface tables; two declarations with identical fields/layout remain distinct nominal specializations, and generic use is admitted only after concrete monomorphization. Existing interface and codegen-cache hashes change when a reachable definition changes and return to the old identity after an exact revert. |
| Persisted/wire format | N/A. A builder is not persisted or exchanged. There is deliberately no `RecordBuilderDescV1`: a second structural byte identity would conflict with Align's nominal type identity and duplicate the versioned interface graph. Persisted consumer artifacts remain consumer-owned. |
| Encoding and text | Field names in compiler metadata are UTF-8 source identifiers under existing declaration rules. Owned `string` contents remain valid UTF-8 and may contain embedded NUL; the builder performs no second encoding or validation. |
| Effects and overlap | Constructor, push, and build are Pure in-memory operations under existing allocation semantics. One mutable builder operation is representable at a time; two distinct builders and independent processes share no mutable builder state. |
| Metric | N/A. This is a correctness/ownership capability and makes no performance threshold claim. Zero-copy heap freeze is preserved and checked structurally; a later optimization claim requires its own workload and baseline. |

The independently owned heap and explicit-region predicates intentionally overlap on scalar-only
records but differ in ownership: `array_builder()` may retain free-standing `string` owners and no
views, while `array_builder(out)` may retain region-valid views and no independently owned field.
The constructor argument makes that choice visible. Neither predicate is widened to approximate the
other.

#### Implementation closure matrix

| Closure cell | Required implementation closure | Owner evidence |
|---|---|---|
| Formation and deterministic exclusion | Add one cycle-safe, source-order `HeapRecord` classifier and select it only for `array_builder()` after expected-type inference. Reject every closed exclusion, empty/explicit-layout/over-aligned records, and malformed imported definitions before HIR publication. | `align_sema::heap_record_predicate_is_closed_source_ordered_and_cycle_safe`; `m12_array_builder::record_builder_closed_shape_and_append_rejections`; whole/per-unit producer validation |
| Concrete generic substitution | A generic struct application may become a heap record only after complete concrete substitution; no abstract `Param`, new bound, or unresolved nominal enters emitted HIR. | `generics::record_builder_generic_instantiation`; checked-HIR abstract-template rejection twins |
| Copy construction, push, growth, and build | Preserve the exact struct layout in zero/one/many/reallocating pushes and return the ordinary AoS dynamic struct array with exact values. | `m12_array_builder::copy_record_push_build_zero_one_many_and_realloc`; MIR/LLVM element-layout assertions |
| Move-in source forms and source nulling | Apply one parameterized owner to every complete Move-record rvalue: bound local, fresh literal, by-value function result, transparent block tail, value-carrying `if`/`match`/`else`, and a successful `?` unwrap including `map_err(...)?` are supported and transfer/null the selected complete source once. A borrowed record, Move field projection, type-divergent join, already-consumed arm, or arena/mixed/path-dependent selected owner rejects before push; a non-fallthrough arm emits no push. | `m12_array_builder::record_builder_move_source_matrix`; MIR source-nulling and selected-arm cleanup assertions |
| Individual allocation proof | Accept only fully free-standing reachable string owners. Reject arena, mixed, and path-dependent owner modes before the runtime push call. | `align_sema::heap_record_builder_owned_leaves_are_proved_individual_before_push`; the closed predicate makes an arena-bearing source value unformable today, while EscapeCheck retains the explicit fail-closed guard for imported/future owner states |
| Relocation | Reallocation copies initialized record bytes without transient Drop and preserves every nested owner for later exactly-once cleanup. | `m12_array_builder::nested_move_record_reallocation_and_reassignment_drop_once`; `align_runtime::array_builder_record_sized_push_realloc_and_instances_are_independent` |
| Unfinished cleanup in both header modes | Normal exit, return, `?`, `map_err`, branch/match/else joins, loop back-edge/break, reassignment, and malformed-input exit drain every initialized element and storage once. Prove this for the stack-local header and for the boxed header after by-value forwarding/return; both use the same compile-time recursive element Drop and differ only in header disposal. | `m12_array_builder::record_builder_abandonment_all_exit_kinds`, `nested_move_record_reallocation_and_reassignment_drop_once`; `align_codegen_llvm::unfinished_heap_record_builders_reuse_array_drop_for_stack_and_boxed_headers` |
| Partial value and enclosing aggregate | A failure while constructing the next record remains source-aggregate cleanup and never increments builder length; failure after placing a built array in an enclosing record drops that array and its initialized elements once. | `m12_array_builder::record_builder_partial_element_and_enclosing_record_cleanup`; existing aggregate partial-construction owners |
| Build transfer in both header modes | Heap build consumes either stack-local or boxed header state, transfers the same element buffer, disposes only the applicable header, and leaves the result's recursive array Drop as sole owner. Unused, returned, and consumed results have no duplicate builder cleanup. | `m12_array_builder::copy_record_push_build_zero_one_many_and_realloc`, `move_record_push_nulls_source_and_build_deep_drops`, and `record_builder_by_value_parameter_return_and_borrow_mut`; `align_codegen_llvm::unfinished_heap_record_builders_reuse_array_drop_for_stack_and_boxed_headers` |
| Function and borrow boundary | Preserve existing scalar/string builder transfer behavior and apply the same typed by-value/`borrow mut` rules to records. Reject aggregate storage, capture, task transfer, and consuming a borrowed builder. | `m12_array_builder::record_builder_by_value_parameter_return_and_borrow_mut`, `record_builder_invalid_storage_capture_and_borrowed_consumption_rejected`; existing boxed-header/capture owners |
| Interface, nominal remap, and cache | Serialize/remap the existing builder element type and complete reachable struct definition; whole/per-unit and cold/cache-replay paths agree, two same-shape declarations remain distinct, malformed producer definitions reject, and edit/revert changes/restores cache identity. | `per_unit::record_builder_imported_interface_graph`; `m12_array_builder::record_builder_nominal_twins_remain_distinct`; `cache_codegen::record_builder_nominal_identity_and_definition_edit_revert`; existing interface graph corruption owners |
| Checked-HIR formation, push, and build | Extend the authoritative checked-HIR ledger in `19-hir-validation-ledger.md`: heap `ArrayBuilderNew` accepts exactly `HeapRecord`, record `ArrayBuilderPush.moves_value` equals the canonical element Drop requirement, and `ArrayBuilderBuild` preserves the same struct id/AoS result. Valid producer rows and malformed element-id/predicate/move-bit/result mutations fail closed before MIR allocation. | `align_mir::validate_hir_tests::heap_record_array_builder_rows_match_the_producer`; parameterized valid/malformed New/Push/Build rows |
| Runtime terminal boundaries | Checked capacity math and allocation failure abort without a successful partial array; separate builders/processes remain independent. | `align_runtime::array_builder_record_capacity_arithmetic_failures_are_terminal`, `array_builder_record_sized_push_realloc_and_instances_are_independent`, and the shared allocator-family OOM contract/owners |
| Compatibility | Existing scalar/string heap builders, region aggregate builders, JSON struct arrays, deep array Drop, and bounded PR tests remain green. | existing `m12_array_builder`, region-builder, `m5` JSON-array, DropPlan, `scripts/test-pr.sh`, and applicable Clippy owners |

The comprehensive design review at `cac65c77` found five valid closure gaps. This revision extends
the checked-HIR New/Push/Build ledger; reconciles the align-llm request with existing by-value and
nominal-interface rules; adds the same-shape nominal twin; enumerates every supported/rejected Move
record push source through one parameterized owner; and proves abandonment/build cleanup in both
stack-local and boxed-header modes. Those repairs refine the settled one-way typed-builder strategy
and introduce no second identity or runtime descriptor.

This is one implementation capability. Formation without recursive cleanup would admit an unsafe
type; runtime storage without the Sema allocation-mode proof could retain an arena child; build
without ordinary array Drop would transfer a leak. Intermediate commits may be compiling owner-test
checkpoints, but the public merge includes the complete formation/push/cleanup/build consumer. The
hand-written diff exceeds roughly 1,000 lines because splitting this strict producer-to-cleanup
chain would either publish an unsafe record type or duplicate the same nominal-layout, ownership,
and Drop proof across dormant boundaries; one consumer-complete capability has lower integration
risk and less duplicated evidence.

### 7.6 Recursive owned heap-record builder

Status: implemented 2026-08-14 for align-llm Request 10.

This extension keeps the Request 8 syntax, owner model, nominal identity, and runtime allocation
ABI. It widens only the admitted record graph needed by the named C6 evaluator records. There is no
second builder, allocator selector, dynamic value tree, collection conversion, or implicit clone.

#### Public-contract ledger

| Field | Contract |
|---|---|
| Surface and owner | The surface remains `array_builder()` under an expected `array_builder<S>` type, `b.push(value: S)`, and consuming `b.build() -> array<S>`. Sema owns formation and allocation-mode proof; Move/Escape analysis owns transfer; MIR owns initialized-prefix cleanup; LLVM owns recursive element Drop; the runtime retains only raw checked byte growth. |
| Recursive element predicate | `HeapTreeRecord(S)` requires the Request 8 root invariants: a nonempty, acyclic declared struct, natural alignment at most 8, and no `align(N)` or `layout(C)`. Each field is a Copy integer/float/`bool`/`char`, owned `string`, another `HeapTreeRecord`, `Option<T>` where `T` recursively satisfies this field grammar, or `array<E>` where `E` is a Copy scalar, owned `string`, or `HeapTreeRecord`. Options may nest; an array element is never another Option or dynamic array. |
| Exact consumer closure | The grammar covers the exact C6 `SnapshotRequest`, `PromptEvaluationTask`, `PromptTaskRow`, `TaskAggregate`, `CorpusAggregate`, `RegressionReason`, `RunSnapshotAttestation`, `SnapshotResult`, and `TaskInputSnapshot` construction graphs recorded below: optional scalars/records/arrays, arrays of strings, and arrays of recursively owned records. One checked-in Align fixture copies this complete graph and the projected result root exactly; a reduced representative shape is not acceptance evidence. |
| Closed exclusions | Direct or nested `str`, `slice`, `resource_ref`, resource, raw, function, builder, Result, user sum/enum, tuple, box, fixed array, response array, region-owned aggregate, and every other view or Move handle reject before construction. `array<Option<T>>`, `array<array<T>>`, and other composite-element dynamic arrays remain unrepresentable and outside this request. Unknown definitions, inline cycles, empty records, and explicit/over-aligned layouts fail closed. |
| Type formation outside the builder | A direct `array<string>` record field becomes an ordinary valid Move field, matching the already valid `Option<array<string>>` path. Its element-wise string Drop is the existing standalone `array<string>` Drop. Existing Option, Result, and user-sum payload grammars may therefore carry a newly valid record containing that field; their syntax and tag ABI do not widen, but their construction, extraction, partial cleanup, and tag-switched Drop must close over it. This Request 10 boundary did not itself change JSON decode/encode modes, fixed arrays, indexing, pipelines, or literal producers; the later settled Request 9 direct-owned JSON route now owns its one flat JSON producer. |
| Construction and move | `push` evaluates one complete record rvalue, verifies its exact nominal type and allocation mode, then transfers the entire value and zeros the selected source before growth. `None` owns no payload; `Some` transfers its one complete payload. Empty arrays use `{null,0}`; nonempty arrays transfer their existing buffer. No child is cloned or rebuilt by `push`. |
| Allocation mode | Every reachable owned `string` and dynamic-array buffer, including array elements' recursive owners, must be uniformly free-standing at the selected push path. Arena-owned, mixed, unknown, or path-dependent ownership rejects in source field/index order before the runtime growth call. Copy fields and `None` contribute no owner. |
| Growth and representation | The outer builder remains one amortized `align_rt_realloc` byte buffer with the compiler-computed stride of `S`. Reallocation relocates tags, `{ptr,len}` headers, and nested record bytes without running transient Drop. Children remain separate allocations owned by their relocated headers. `build` is the existing zero-copy outer-buffer transfer to AoS `array<S>`. |
| Drop and cleanup | One compiler-owned recursive Drop plan is used for a complete source, an unfinished builder prefix, and the built array. Option Drop visits only `Some`; `array<string>` drops every initialized string then its buffer; `array<Move-record>` drops each element recursively then its buffer; Copy arrays free only their buffer. Dropping an unfinished stack or boxed builder consumes its header into `{ptr,len}` and invokes this ordinary array Drop once. |
| Partial states | A failure while constructing the next record remains the source aggregate's responsibility and does not increment builder length. After `push` begins, capacity overflow and allocator failure are terminal and cannot expose a partial success. Replacement, return, `?`, `map_err`, branch/match/else joins, loop back-edges/breaks, early exit, and enclosing-record failure each leave exactly one cleanup owner. |
| Builder ownership | The existing single-owner rules remain: typed by-value transfer and return are allowed; `borrow mut` is non-consuming and cannot escape; aggregate/Option/Result storage and task/closure capture are rejected. `append` remains available only for the pre-existing Copy scalar forms and never shallow-copies any record or Move element. |
| Validation order | Parser/import/arity and expected-type inference; recursive type formation and representability; builder placement/receiver; source move state and exact nominal type; depth-first allocation-mode proof in declaration/index order; build result escape and cleanup. Whole-program, imported/per-unit, checked-HIR, and cache-replay paths preserve the same first failure and perform no allocation before their applicable validation completes. |
| Interfaces and identity | Existing nominal producer identity plus the complete serialized reachable struct/tagged/array definition graph remains canonical. Concrete generic substitution finishes before admission. Same-shape declarations remain distinct; field/option/array/drop/layout edits invalidate interface and codegen caches, and an exact revert restores the prior identity. No `RecordBuilderDescV2`, runtime dictionary, reflection table, callback, or source/artifact read is added. |
| Encoding and persistence | N/A. The builder has no wire form. Field names retain existing UTF-8 identifier semantics; owned string bytes retain the language string contract, including embedded NUL. JSON Requests 9 and 13 remain separate operation-specific materialization/codec work. |
| Effects and overlap | Constructor, push, and build remain Pure in-memory operations. One mutable operation can target one builder at a time. Distinct builders and processes share no mutable builder state. Failure restoration is ordinary owner cleanup; no connection-global or process-global state exists. |
| Allocation accounting | For already constructed children, each push adds no child allocation; only outer-buffer growth may allocate. Build adds none. A value containing `k` separately built arrays and their string elements retains exactly those owners plus the one outer builder buffer. Instrumented owner tests, not a benchmark, prove this correctness accounting because no latency or throughput threshold is promised. |
| Prerequisite and adoption | Request 8 at `029e27465d79e24cd36d374aae41dca0ec7e6979` is the implementation base. The Align implementation must merge before align-llm runs `c6c2-request10-adoption`; C6f2 later runs its own `c6f2-array-builder-adoption`. Neither consumer checkpoint is part of Align's implementation gate. |

#### Exact C6 gate graph

This graph is copied from align-llm's
`docs/specs/c6-prompt-context-optimizer.md` blob
`58ac4e9064dad392cd99f2ae4bef5fcd77c54b6c`. In the Align fixture, every persisted textual,
path, digest, status, and discriminator field is an owned `string`; every schema version, count, index, byte
size, duration, seed, and ppm field is `i64`; and every predicate field is `bool`. The source's
`array<str>` and `Option<str>` spellings are deliberately materialized as `array<string>` and
`Option<string>` because the evaluator result owns its decoded construction graph.

| Named consumer | Exact ordered fields and Align types |
|---|---|
| `SnapshotRequest` | `schema_version:i64`; `artifact_kind:string`; `task_id:string`; `project_root:string`; `repo_path:string`; `repo_revision:string`; `require_clean_repo:bool`; `static_expectations:array<ArtifactExpectation>`; `additional_files:array<string>`; `workspace_path:string`; `allowed_workspace_entries:array<string>`; `content_sha256:string` |
| `PromptEvaluationTask` | `schema_version:i64`; `artifact_kind:string`; `task_id:string`; `repo_id:string`; `repo_revision:string`; `repo_path:string`; `require_clean_repo:bool`; `cmd:string`; `argv:array<string>`; `snapshot_cmd:string`; `snapshot_argv:array<string>`; `measurement_adapter_runtime:string`; `snapshot_helper_runtime:string`; `cwd:string`; `timeout_ns:i64`; `task_prompt_path:string`; `context_sources_path:string`; `generation_policy_path:string`; `provider_control_path:string`; `environment_policy_path:string`; `artifacts:array<ArtifactExpectation>`; `regression_limits:RegressionLimits`; `content_sha256:string` |
| `PromptTaskRow` | `schema_version:i64`; `artifact_kind:string`; `evaluation_id:string`; `task_id:string`; `sample_index:i64`; `variant:string`; `variant_id:string`; `variant_sha256:string`; `prompt_preparation_ns:i64`; `time_to_passing_patch_ns:Option<i64>`; `evaluation_input:EvaluationInputIdentity`; `measurement:TaskMeasurement`; `content_sha256:string` |
| `TaskAggregate` | `task_id:string`; `parent_pass_count:i64`; `candidate_pass_count:i64`; `parent_repair_loop_count:i64`; `candidate_repair_loop_count:i64`; `paired_pass_count:i64`; `parent_paired_median_time_ns:Option<i64>`; `candidate_paired_median_time_ns:Option<i64>`; `time_improvement_ppm:Option<i64>`; `time_regression_ppm:Option<i64>` |
| `CorpusAggregate` | `task_count:i64`; `sample_count:i64`; `parent_pass_count:i64`; `candidate_pass_count:i64`; `parent_repair_loop_count:i64`; `candidate_repair_loop_count:i64`; `paired_pass_count:i64`; `parent_paired_median_time_ns:Option<i64>`; `candidate_paired_median_time_ns:Option<i64>`; `completion_gain_count:i64`; `time_improvement_ppm:Option<i64>`; `time_regression_ppm:Option<i64>`; `repair_loop_regression_count:i64` |
| `RegressionReason` | `task_id:string`; `sample_index:i64`; `code:string`; `parent_value:string`; `candidate_value:string`; `limit:string` |
| `RunSnapshotAttestation` | `schema_version:i64`; `artifact_kind:string`; `task_id:string`; `sample_index:i64`; `variant:string`; `status:string`; `error_code:string`; `error:string`; `snapshot_request_sha256:string`; `before_snapshot_result_sha256:string`; `after_snapshot_result_sha256:Option<string>`; `before_input_snapshot_sha256:Option<string>`; `after_input_snapshot_sha256:Option<string>`; `content_sha256:string` |
| `SnapshotResult` | `schema_version:i64`; `artifact_kind:string`; `task_id:string`; `status:string`; `error_code:string`; `error:string`; `environment_probe:Option<EnvironmentProbe>`; `artifact_digests:array<ArtifactDigest>`; `content_sha256:string` |
| `TaskInputSnapshot` | `schema_version:i64`; `artifact_kind:string`; `task_id:string`; `task_manifest_sha256:string`; `artifact_digests:array<ArtifactDigest>`; `environment_sha256:string`; `content_sha256:string` |

The supporting records in that reachable graph are exact too:

| Supporting record | Exact ordered fields and Align types |
|---|---|
| `ArtifactExpectation` | `path:string`; `kind:string`; `expected_sha256:string` |
| `ArtifactDigest` | `path:string`; `mode:string`; `byte_count:i64`; `sha256:string` |
| `RegressionLimits` | `maximum_unrelated_diff_count:i64`; `maximum_patch_size_bytes:i64`; `maximum_public_api_change_count:i64`; `maximum_repair_loops:i64`; `maximum_benchmark_regression_ppm:Option<i64>` |
| `EnvironmentProbe` | `schema_version:i64`; `artifact_kind:string`; `producer:string`; `os:string`; `os_release:string`; `architecture:string`; `cpu:string`; `logical_cpu_count:Option<i64>`; `gpu:string`; `runtime_identity:string`; `content_sha256:string` |
| `EvaluationInputIdentity` | `schema_version:i64`; `artifact_kind:string`; `task_id:string`; `task_input_snapshot_sha256:string`; `parent_variant_sha256:string`; `candidate_variant_sha256:string`; `task_prompt_sha256:string`; `context_sources_sha256:string`; `generation_policy_sha256:string`; `generation_request_sha256:string`; `adapter_request_sha256:string`; `environment_policy_sha256:string`; `environment_sha256:string`; `sample_index:i64`; `paired_seed:i64`; `content_sha256:string` |
| `GenerationRequestIdentity` | `schema_version:i64`; `artifact_kind:string`; `rendered_prompt_sha256:string`; `system_text_sha256:string`; `user_text_sha256:string`; `generation_policy_sha256:string`; `provider_control_sha256:string`; `environment_policy_sha256:string`; `max_tokens:i64`; `temperature_micros:i64`; `paired_seed:i64`; `provider_request_sha256:string`; `seed_attestation_sha256:string`; `content_sha256:string` |
| `SeedCapabilityAttestation` | `schema_version:i64`; `artifact_kind:string`; `provider_kind:string`; `provider_model:string`; `requested_seed:i64`; `result:string`; `applied_seed:Option<i64>`; `provider_request_sha256:string`; `content_sha256:string` |
| `TaskMeasurement` | `schema_version:i64`; `artifact_kind:string`; `status:string`; `failure_kind:string`; `build_status:string`; `test_status:string`; `repair_loop_count:i64`; `unrelated_diff_count:i64`; `patch_size_bytes:i64`; `public_api_change_count:i64`; `policy_violation_count:i64`; `cleanup_passed:bool`; `containment_passed:bool`; `benchmark_regression_ppm:Option<i64>`; `generation_to_passing_patch_ns:Option<i64>`; `rendered_prompt_sha256:string`; `generation_request:GenerationRequestIdentity`; `environment_probe:EnvironmentProbe`; `seed_attestation:SeedCapabilityAttestation`; `diagnostic_summary:string`; `diagnostic_stdout:string`; `diagnostic_stderr:string`; `content_sha256:string` |

One synthetic `Request10ConsumerRoot` fixture owns, in order,
`tasks:array<PromptEvaluationTask>`, `snapshot_requests:array<SnapshotRequest>`,
`snapshot_results:array<SnapshotResult>`, `input_snapshots:array<TaskInputSnapshot>`,
`snapshot_attestations:array<RunSnapshotAttestation>`, `rows:array<PromptTaskRow>`,
`task_aggregates:array<TaskAggregate>`, `corpus_aggregate:Option<CorpusAggregate>`, and
`serious_regression_reasons:array<RegressionReason>`. The owner test constructs nonempty values for
every field and also mutates each field path to its applicable empty/`None` state. A generated
field-vector assertion compares these ledger rows with the checked-in fixture so deleting,
reordering, or retyping a field fails the Align gate before align-llm adoption.

The recursive grammar is intentionally not the unrestricted mathematical closure of `Option` and
`array`. The named consumers need arrays of strings and records whose fields may themselves contain
options and arrays. They do not need arrays whose elements are options or arrays. Those shapes need
a new composite-element array representation and per-element move-out/indexing contract, so adding
them here would cross a distinct language and ABI boundary without a consumer.

#### Deterministic field and owner order

The classifier and allocation-mode proof both use depth-first source order. An Option contributes
its payload at the field position. An array contributes its buffer owner first, then element owners
in increasing index order for runtime cleanup; static admission validates the element type at the
field position. For multiple invalid fields, the first declaration-order path wins. For a selected
Move source, type/source-state errors precede allocation-mode errors, and all precede growth.

#### Implementation closure matrix

| Closure cell | Required implementation closure | Owner evidence |
|---|---|---|
| Formation and closed grammar | Replace the Request 8 leaf-only walk with one cycle-safe, source-order `HeapTreeRecord` classifier over records, nested Options, and the exact dynamic-array element set. Reject every excluded leaf and malformed id before HIR publication. | Sema predicate unit matrix; `m12_array_builder` accepted/rejected graph and precedence matrix; whole/per-unit malformed producer twins |
| Exact consumer graph | Check in the complete field-for-field graph above plus `Request10ConsumerRoot`; construct every nonempty path and each applicable empty/`None` path. A field-vector assertion pins names, order, and types to the ledger rather than accepting a smaller lookalike. | `m12_array_builder::request10_exact_c6_consumer_graph` plus its generated field-vector assertion |
| Direct `array<string>` field | Remove only the direct-field prohibition after proving the existing deep string-array Drop is used through direct, Option-wrapped, nested-record, replacement, and partial-construction paths. Preserve the Request 10 JSON/fixed-array/indexing restrictions; the separately reviewed Request 9 route may replace only its flat direct-record JSON rejection owner. | declaration owner plus direct/nested `array<string>` construction/drop tests; scoped existing-route JSON rejection owners |
| Existing tagged payload closure | A record containing direct `array<string>` remains valid everywhere the existing Option, Result, and user-sum payload grammar already accepts that record. Cover construction, move-in/out, `match`, `else`, `?`, `map_err`, replacement, permitted return, rejected call-boundary positions, later-sibling failure, active-tag Drop, and inactive payload bytes without widening which payload categories are nameable. | one parameterized tagged-wrapper source/MIR/LLVM runtime owner over Option/Result/user sum, with None/Ok/Err/each user variant and partial-error twins |
| Concrete generic substitution | Admit a generic record only after every Option/array/record parameter is concrete and satisfies the same graph. No new bound or unresolved Param reaches checked HIR. | generic direct/nested positive and excluded composite-element negative; interface template mutation |
| Copy/Move construction | Cover Copy-only records, `None`, `Some` scalars/strings/records/arrays, empty/nonempty string arrays, and arrays of Copy/Move records. Complete literals, locals, function results, transparent blocks, `if`/`match`/`else`, `?`, and `map_err(...)?` transfer once. | one parameterized `m12_array_builder` construction/source matrix with source-use-after-push negatives |
| Allocation-mode proof | Walk every reachable string and array owner in deterministic field/index order; accept only uniform free-standing ownership and reject arena, mixed, unknown, and path-dependent paths before push. | Sema unit owner plus runtime-call-absence driver assertions for direct, Option, nested array, and array-element paths |
| Relocation | Force outer reallocations around records containing `None`, `Some`, empty/nonempty arrays, nested Move records, and arrays of Move records; no transient child Drop occurs. | driver allocation/free instrumentation and runtime exact-size growth owner |
| Unfinished cleanup | Normal exit, return, `?`, `map_err`, all joins, loop continue-by-back-edge/break, reassignment, and malformed-input exit drain every initialized element exactly once in stack and boxed header modes. | parameterized abandonment matrix plus LLVM structural stack/boxed owner using ordinary array Drop |
| Recursive Drop | Close `None`/`Some`, direct and Option-wrapped `array<string>`, Copy arrays, `array<Move-record>`, arrays of records that themselves contain Options/arrays, and repeated nominal subgraphs through the canonical iterative Drop dispatcher. | Sema DropPlan graph tests; LLVM IR/runtime exact-free owners; deep finite graph non-recursion owner |
| Partial and enclosing aggregates | Failure constructing the next nested Option/array record leaves builder length unchanged; failure after a built array enters an enclosing Move record drops the array and later sibling state exactly once. | driver partial-element and enclosing-record failure owners |
| Build transfer | Stack and boxed builds transfer the same outer buffer and recursive obligation, dispose only the applicable header, and suppress later builder cleanup. Empty and nonempty results use ordinary AoS array Drop. | driver build/use-after-build/return owners plus LLVM header-mode structural owner |
| Function and borrow boundary | Preserve typed by-value transfer/return and non-consuming `borrow mut`; reject aggregate storage, capture, task transfer, alias escape, and consuming a borrowed builder for the widened graph. | existing Request 8 boundary owner parameterized with a recursive record |
| Interface, nominal remap, and cache | Serialize/remap every reachable Option/array/record definition; whole/per-unit and cold/cache-replay decisions agree; same-shape nominal twins differ; edit/revert restores identity. | generics, per-unit, interface corruption, nominal twin, and cache edit/revert owners |
| Checked HIR | Rename/extend the shared `HR` predicate and require New/Push/Build to use it, exact move bits, exact root id, and AoS result. Wrong tagged id, array element, move bit, root/result id, or malformed reachable definition fails before MIR. | `19-hir-validation-ledger.md` rows and parameterized valid/malformed HIR mutations |
| Runtime terminal boundaries | Raw push keeps checked `len * stride`, capacity, and allocation arithmetic; overflow/OOM remain terminal; distinct builders/processes remain independent. No runtime ABI symbol changes. | existing Request 8 runtime owners plus widened exact-stride and terminal-child cases |
| Allocation parity | Count that push/build add no child allocation or clone and that every pre-existing child plus the outer buffer frees once on build/abandonment. | driver/runtime allocation instrumentation; no benchmark |
| Compatibility | Scalar/string and Request 8 record builders, region builders, ordinary Option/array Drop, JSON routes, sum payload restrictions, bounded gate, and Clippy remain green. | existing owner suites and `scripts/test-pr.sh` |

#### Request 10 design-review finding-to-fix ledger

| Finding | Closure |
|---|---|
| P2: named consumers were represented only by equivalent shapes, so Align's gate could omit a real C6 field path | Pin the source blob, copy every named and supporting field with exact order/type into the ledger, and require a checked-in field-for-field fixture plus projected root and field-vector assertion. |
| P2: lifting direct `array<string>` fields indirectly admits that record through existing tagged payload grammars without owning their cleanup paths | Add the Option/Result/user-sum construction, transfer, partial failure, permitted/rejected boundary, and active-tag Drop matrix cell; synchronize the memory model, HIR ledger, specifications, and JSON non-widening boundary. |

This is one consumer-complete implementation boundary. Admission without direct string-array Drop,
allocation-mode proof, recursive unfinished cleanup, checked-HIR closure, and build transfer would
publish an unsafe shallow-copy or leak. The expected hand-written diff may exceed roughly 1,000
lines because the producer-to-cleanup chain crosses Sema, HIR validation, MIR, LLVM, interfaces,
and owner tests; splitting it would publish no useful safe intermediate consumer and duplicate the
same graph proof.

### 7.7 Bounded canonical JSON encoding

Status: implemented for align-llm Request 12 on 2026-08-14.

The evaluator must persist typed artifacts without first materializing an unbounded string. The
one public addition is the fallible, individually owned sibling of `json.encode`:

```align
json.encode_bounded(value, max_bytes) -> Result<string, Error>
```

This is not a second JSON format, dynamic JSON tree, writer interface, size-estimation pass, or
builder surface. It uses the exact existing typed encode plan and formatters, with one inclusive
byte ceiling on their shared destination.

#### Public-contract ledger

| Field | Contract |
|---|---|
| Surface and owner | `core.json` owns `json.encode_bounded(value, max_bytes: i64) -> Result<string, Error>`. Sema owns import/arity/type/schema checking and constructs the same ordered encode parts as `json.encode`; checked HIR and MIR own the new fallible envelope; LLVM owns result construction and cleanup; the runtime owns bounded builder storage and the final status/out-slot ABI. |
| Input and schema | `value` is borrowed and must satisfy exactly the same `json.encode` input predicate at every implementation head, including its local-binding restriction, nested records, Options, arrays, unions, field order, and closed exclusions. Request 12 did not itself admit a shape rejected by `json.encode`; accepted Request 13 replaces both owned encode plans from one V2 graph when its implementation lands. `max_bytes` is exactly `i64`; no integer coercion or default exists. |
| Canonical bytes | On `Ok`, the returned UTF-8 bytes are byte-for-byte equal to `json.encode(value)` for the same typed value and compiler. Declaration-order object keys, existing numeric rendering, JSON string escaping, omission of `None`, empty collection/object spelling, union payload spelling, and unknown-field behavior are unchanged. “Canonical” means this one compiler-owned typed encoding, not RFC 8785 key sorting or a schema-unknown dynamic canonicalizer. The bounded path reuses the same HIR encode-part constructor, MIR `TemplatePiece` values, and runtime scalar/object/array/union writers; it may not carry a copied formatter. |
| Limit | `max_bytes` is an inclusive UTF-8 byte ceiling on the complete encoded result. `0` is valid but no currently encodable JSON value is empty. If the next emitted byte would make the length exceed the ceiling, encoding becomes failed before any capacity growth or byte write beyond the ceiling. A multi-byte UTF-8 scalar or JSON escape counts by its emitted bytes. Exact fit succeeds; one byte over fails. |
| Result and error | Success returns one free-standing owned `string`. A negative ceiling or an encoded result larger than the ceiling returns `Err(Error.Invalid)`. Those two public-value failures are deliberately one existing resource/input category; no new `Error` variant or numeric code is introduced. No failure exposes a partial string, byte count, prefix, or reusable builder. |
| Evaluation and validation order | Resolve `core.json`, arity, and exact argument types; validate the value's complete encode schema in its existing deterministic field/variant order; validate the result envelope; then evaluate the borrowed value expression and `max_bytes` left-to-right. At runtime a negative limit fails before builder allocation or encode traversal. For a nonnegative limit, encoding observes the first would-exceed write. Compile diagnostics therefore precede all runtime limit results, and the existing first schema diagnostic is unchanged. |
| Allocation and ownership | The bounded builder starts empty and grows in the existing C allocator family, never reserving or retaining capacity above `max_bytes`. Its growth candidate is `min(max_bytes, max(required, doubled, min(8, max_bytes)))`, after proving `required <= max_bytes`; this deliberately replaces ordinary `BuilderBuf`'s unconditional 8-byte minimum for the bounded mode. Success transfers its allocation directly into the `string` Ok payload. Negative-limit and over-limit paths free the complete builder buffer and leave the output slot zeroed before constructing Err. The borrowed source is never consumed or mutated. Result `match`, `?`, `else`, return, reassignment, and unused-value cleanup use the existing recursive tagged-payload Drop. |
| Overflow and OOM | Length addition, growth doubling, and capacity conversion are checked before allocation. A length that would exceed the caller ceiling is `Error.Invalid`, not an allocator call or abort. On supported 64-bit targets every nonnegative `i64` ceiling is representable as `usize`/`isize`. Actual allocator failure retains Align's existing terminal-abort policy; it is not misreported as a caller limit and cannot return a partial Result. |
| Runtime state and early stop | The builder carries one internal `failed_limit` bit and the inclusive ceiling. Once set, all central writes are no-ops and descriptor loops stop at their next bounded-writer check; `pop_comma` is a no-op. Finishing consumes the header exactly once: it transfers bytes only when healthy, otherwise frees them. Independent calls/builders/processes share no mutable limit state. |
| Runtime ABI | Add `align_rt_builder_init_bounded_stack(out: ptr, max_bytes: i64) -> ptr` and `align_rt_builder_finish_bounded_stack(builder: ptr, out_string: ptr) -> i32`. Finish returns `0` with `{ptr,len}` written on success or `AL_INVALID` with a zeroed out slot on limit failure, and consumes the stack header in both cases. Existing builder and `json.encode` symbols and layouts retain their behavior; the conservative 64-byte/16-align stack-header assertion remains the ABI guard. |
| HIR/MIR contract | Add a dedicated `JsonEncodeBounded { base, parts, max_bytes }` HIR expression with stored result `Result<string,builtin Error>` and a matching MIR operation/control-flow lowering. `base` is the checked source local whose complete reachable schema determines the canonical plan. Its `parts` obey the existing `TemplatePart` rendering contract, and checked HIR reconstructs the producer plan from `base` to require every static token, field ordinal, access path, name, descriptor identity, and array element in order. The new envelope also adds exact limit type, result type, borrow, and failure cleanup. Ordinary `Template` remains `str` and unchanged. No unchecked HIR can select an arbitrary formatter or claim an owned result without the fallible finish. |
| Descriptor integrity | All struct/array/union ids and descriptor directions are compiler-owned metadata validated before MIR and native allocation. A malformed checked-HIR id, reachable definition, part/access pairing, or imported interface rejects compilation; it is not reclassified as a runtime limit error. Runtime null/defensive descriptor branches remain unreachable from producer-valid code and must stay memory-safe in direct ABI tests. |
| Interface and cache identity | The public core symbol, exact signature, new HIR/MIR discriminators, complete reachable typed encode schema, runtime ABI rows, compiler interface schema, and compiler build identity form the capability identity. Whole-program, per-unit/imported, cold, and cache-replay compilation produce the same parts and bytes. Editing a reachable record/union definition changes identity; exact revert restores it. Existing `json.encode` source retains its surface and encode plan. |
| Request 13 composition | Request 12 accepts only shapes `json.encode` accepts at the same implementation head. Request 13's recursive owned graph is accepted in [`25-recursive-owned-json-plan.md`](25-recursive-owned-json-plan.md) and implementation remains pending. That implementation atomically replaces the shared flat owned plan for both `encode` and `encode_bounded` with one V2 graph and owns the exact pinned C6 byte/cap vectors. There is no bounded-only schema and no revision to this limit/result ABI. |
| Persisted format | N/A in Align. The function returns bytes but does not name an artifact schema, path, digest, or durability protocol. align-llm owns its versioned artifact ledger and may persist only an `Ok` value. Align guarantees only the byte identity above. |
| Effects and overlap | Pure in-memory computation under the existing allocation model. Each call owns its builder and output. Re-entrant/nested calls and parallel calls are independent; no ambient process limit or mutable global is consulted. |
| Acceptance and metric | Correctness is exact byte parity plus the memory ceiling and cleanup, not a latency claim. Owner evidence covers every named boundary below. Any future performance claim needs a separate reproducible benchmark against `json.encode` and the consumer's time-to-passing-patch metric. |

#### Implementation closure matrix

| Closure cell | Required implementation closure | Owner evidence |
|---|---|---|
| Surface and schema parity | Register only `encode_bounded`, require `import core.json`, exact arity and `i64` limit, and invoke the same encode-schema/part constructor as `json.encode`. Preserve the local-binding and unsupported-shape diagnostics. | Sema unit matrix plus driver compile-fail twins comparing `encode`/`encode_bounded` diagnostics for every accepted/rejected root shape |
| Byte parity | For every currently admitted encode shape — scalars within records, nested records, required/optional fields, all-`None`, strings requiring every escape class and multibyte UTF-8, scalar/record arrays, unions, and empty values — exact-fit bounded output equals ordinary encode byte-for-byte. Request 13 extends this same owner with the accepted pinned C6 graph only when its V2 implementation lands. | parameterized `m5` golden/parity owner over the current encode matrix; Request 13's recursive parity matrix for V2 |
| Limit boundaries | Cover negative, every cap `0..=8`, `exact - 1`, exact, `exact + 1`, the two-byte all-optional `{}` result, a breach in static text, escape expansion, numeric formatting, nested object, array separator, and closing delimiter. Assert each reserve candidate and observed peak capacity are at most the cap; a first write larger than the cap allocates zero; no partial Ok is constructible. | runtime builder failpoint/capacity/allocation instrumentation plus end-to-end result owners |
| Central writer closure | Route raw, int, float, bool, char, JSON-string, optional-field comma repair, object, scalar-array, struct-array, and union writes through the same bounded `BuilderBuf`; each recursive/iterative encoder observes the sticky failure and exits without later source reads where practical. No formatter clone or unbounded temporary exists. | runtime per-writer table, source inventory assertion, direct-ABI null defensive cases |
| Success ownership | Finish transfers the one builder allocation into the Ok `string`; bind, consume, return, `match`, `else`, `?`, `map_err`, reassign, and unused results free it exactly once. The borrowed source remains usable. | driver tagged-Move result matrix and allocation/free counters |
| Failure cleanup | Negative limit allocates nothing. Breach at every writer class frees the partial buffer and zeroes the out slot. Failure inside nested object/array/union traversal, after an omitted Option, and after capacity growth leaks no allocation and drops no borrowed source owner. | runtime failpoint matrix plus LLVM/MIR branch/drop assertions |
| Checked arithmetic and terminal allocation | Checked `len + additional`, bounded doubling, and `i64` conversion distinguish caller-cap rejection from allocator failure. Limit failures return `AL_INVALID`; injected OOM/host-size failure follows the terminal allocator owner and never returns Err or Ok. | runtime arithmetic boundary tests and child-process terminal OOM owner |
| HIR validation | Add the exact `JsonEncodeBounded` ledger row: source `base` is one visible local of an admitted root type; its schema reconstructs exactly the ordered encode parts; `max_bytes` is exactly `i64`; the result is exactly `Result<string,builtin Error>`; and every part access is borrowed. Mutate the base, part kind, static key, access root/path, field order, schema ids, limit type, error enum, Ok type, result type, and malformed reachable definitions; reject before MIR/runtime allocation. | whole/per-unit producer twins and parameterized `validate_hir_tests` mutations |
| MIR/LLVM/ABI | Lower one bounded stack header, emit the shared pieces, call the fallible consuming finish once, and build the existing builtin Error from status. Verify the two exact new ABI declarations and export presence; no ordinary template/json.encode call site changes symbols. | MIR shape, LLVM IR call/order/result-branch tests, runtime ABI ledger/test, native link smoke |
| Interface and cache | Export/import the symbol and reachable schema; whole/per-unit, generic concrete instantiations, cold/cache replay, same-shape nominal twins, edit/revert, and malformed imported HIR agree on admission and bytes. | interface/generics/cache owner matrix and checked-HIR imported corruption twins |
| Accepted Request 13 composition | Keep schema selection and plan construction single-sourced between ordinary and bounded encode. A source-inventory/negative owner proves no bounded-only field predicate exists. The accepted Request 13 implementation replaces both flat owned plans atomically and owns the exact recursive C6 graph parity and limit vectors; until then, shipped flat behavior remains authoritative. | shared-plan call graph assertion, current flat `string`/`array<string>` twins for both surfaces, then Request 13's V2 recursive parity matrix |
| Concurrency and compatibility | Parallel independent bounded encodes with different caps cannot share sticky state. Existing templates, `builder`, `json.encode`, decode/scan/doc, Request 8/10 owners, native supported targets, bounded gate, and Clippy remain green. | runtime concurrent-call owner, existing JSON/builder suites, `scripts/test-pr.sh`, applicable native profile checks |

#### Request 12 design-review finding-to-fix ledger

| Finding | Closure |
|---|---|
| P2: the existing builder's unconditional minimum capacity of 8 would allocate above a valid `2..=7` byte caller ceiling | Specify the bounded-only cap-clamped growth formula, including `min(8, max_bytes)`, and require capacity/allocation owners for every cap `0..=8`, exact `{}`, and a first write larger than the cap. Ordinary unbounded builder growth remains unchanged. |
| P2: two authoritative whole-surface summaries still omitted the new operation | Synchronize the struct-schema summary, language-spec completeness paragraph, settled open-question catalog, and English/Japanese core-design indexes with the five-operation catalog and mark the new signature pending until implementation. |

This must land as one consumer-usable capability. A surface without the central memory guard would
violate the request; a guard without fallible ownership cleanup would leak or expose partial bytes;
a separate formatter would make canonical parity an assertion rather than a construction. The
implementation therefore crosses Sema, checked HIR, MIR, LLVM, runtime ABI, and owner tests in one
reviewed change.

## 8. Recursive tagged Move payloads

### 8.1 Required surface

Ordinary structured errors and compound outputs must compose with the one `Option`/`Result`/sum
model:

```align
NativeError {
  code: Option<string>,
  message: string,
}

DbError {
  Native(NativeError),
  Decode(string),
}

fn run(...) -> Result<Option<Output>, DbError>
```

This currently crosses several implementation restrictions: an `Option<Move>` field needs
conditional Drop, a Move struct/sum needs a recursive tagged payload plan, and a Move sum used as a
`Result` error must propagate/drop through `?`, `else`, and `match`. The database design must not
replace these values with codes, empty-string sentinels, an opaque error allocation, or a
database-private Result.

The implementation split is exact: L1a admits only the required `Option<string>` struct-field leaf
and establishes the recursive plan framework; L1b admits Move struct/sum payloads in
Option/Result/user sums, including `Option<MoveStruct>`, and completes tagged control flow.

The settled rule is:

- `Option<T>`, `Result<T,E>`, and user sum payloads may contain any finite, non-recursive type with
  a compiler-known Drop plan;
- the enclosing value is Move iff any live payload is Move;
- Drop first tests the active tag and recursively drops only the live payload;
- construction moves the payload and clears its source ownership;
- owning-place extraction through `match`, `else`, or `?` moves the live payload and clears the
  container; an admitted borrowed-place `match` reads the active payload in place and leaves the
  source owner unchanged;
- early return, branch/loop join, reassignment, and partially initialized construction retain the
  existing path-local cleanup discipline;
- borrowed payload provenance remains recursive and independent of the cleanup plan;
- recursive types remain rejected; this prerequisite does not add tracing or heap-indirect enums;
- arrays/collections of arbitrary Move elements are separate data-layout work and are not implied.

This is one recursive `DropPlan`/`MovePlan` classification shared by structs, sums, `Option`, and
`Result`. A table-free helper that treats an enum/struct payload as Copy is prohibited.

### 8.2 ABI and interface

The ABI stays the existing tagged aggregate shape. A user sum remains the current non-union
aggregate `{ tag, every variant payload flattened in declaration order }`; `Option` and `Result`
retain their current tagged aggregate layouts. The extension is semantic cleanup and move-out, not
pointer boxing or a union-layout change. `DropPlan` is derived from canonical type definitions after
generic monomorphization. Public interfaces already carry the structural type definitions; their
canonical hash changes when a payload type or ownership shape changes. A corrupt or cyclic imported
definition fails closed before a plan is built.

Success paths do not allocate merely because the error type is Move. Owned strings are allocated
only when the program constructs the error payload. Returning `Ok` initializes no error-owned
field, and Drop consults the tag before touching payload storage.

### 8.3 L1b implementation closure matrix

This matrix is the implementation gate for L1b. L1b is delivered as three independently correct
vertical PRs, each expected to stay under 1,000 changed hand-written lines:

```text
L1b-a  one direct existing-Scalar Move payload per tagged arm
L1b-b  multiple Move payloads, partial construction, and uniform ownership mode
L1b-c  tagged-in-tagged type representation and exact Result<Option<Output>, DbError>
```

L1b-a established direct recursively Move payloads. L1b-b admits multiple Move payloads only after
partial construction and mixed-provenance rules close. L1b-c admits nested `Option`/`Result`
payloads after type representation, generic substitution, HIR/MIR, LLVM layout, interface
round-trip, and malformed-interface validation agree.
Arbitrary new Move-element collection layouts, resources, and borrowed-parameter modes remain in
their later milestones. L1b-a may slightly exceed 1,000 total added-plus-removed lines because the
safe vertical unit must remove obsolete sema rejection paths, add MIR ownership, add LLVM recursive
Drop, and convert the same owner tests together; splitting type admission from cleanup would create
an unsound intermediate commit. Its net new hand-written surface is approximately 1,000 lines,
including the closure-matrix regressions and benchmark rows.

| Contract path | Required implementation | Owner regression |
|---|---|---|
| type formation | L1b-a admits direct Move struct/sum/string payloads already representable by `Scalar`; L1b-b admits multiple Move payloads. Combined struct/sum inline cycles, nested tagged payloads, and unsupported Move-element collections fail closed until their owning slices. | sema declaration and generic-monomorph tests |
| classification | Derive one finite recursive `DropPlan`; container is Move iff an active payload may be Move | sema DropPlan unit tests |
| leaf closure | One dispatcher covers owned string, flat allocation, opaque Move handle, nested Move struct, nested Move enum, and every already-supported deep collection leaf; any unimplemented leaf is rejected during type formation | dispatcher unit tests plus one runtime representative per leaf class |
| construction / move-in | Move one payload into `Some`, `Ok`, `Err`, or a user variant and clear the source cleanup bit | MIR source-nulling tests plus runtime use-after-move cases |
| multiple payload construction (L1b-b) | Keep each earlier Move payload owned while evaluating later payloads; clean it on `?`/`return`; accept all-heap/all-arena and deterministically reject mixed heap/arena ownership | early-exit, all-heap, all-arena, and mixed-mode diagnostics |
| Drop | Switch on `Option`, `Result`, or user-sum tag and recursively drop only the active payload | LLVM tag-guard assertions and allocation/free parity |
| `match` move-out | Transfer every bound live payload, clear the container, and drop a fresh discarded active payload before an arm may diverge | bound/fresh/wildcard/or-pattern and L1b-b multiple-binding runtime cases |
| `else` move-out/discard | Transfer `Some`/`Ok`; recursively drop an active discarded `Err` before evaluating fallback | success, error, and diverging-fallback runtime cases |
| `?` propagation | Transfer `Ok` forward or `Err` into the returned `Result`; clear the consumed source before exit cleanup | bound/fresh success/error and early-exit cases |
| `map_err` | Pass `Ok` ownership unchanged; transfer old `Err` exactly once into the mapper and clear its container. A receiver already evaluated before a mapper expression that returns or propagates remains owned and is cleaned. | fresh/bound both-arm cases, mapper-expression `return`/`?`, mapper call, and arena-owned call-boundary rejection |
| replacement | Drop the old tagged value once and install the selected cleanup bit | all tag transitions |
| return / call boundary | L1b accepts only results proven free-standing by the current ABI rules and rejects arena/path-dependent cleanup-bit returns. L2 owns dynamic path-selected return cleanup bits. | direct/imported free-standing parity plus arena/path-dependent rejection |
| branch / loop joins | Carry the selected path-local cleanup bit without deriving it from the static type or region | `if`, `match`, and value-carrying `loop` cases |
| borrow provenance | Preserve existing recursive region roots independently from cleanup ownership | borrowed payload invalidation tests |
| generic / interface | Recompute the same plan after monomorphization; whole-program and per-unit builds agree | generic payload plus two-unit executable parity |
| malformed interface | Reject unresolved type references, struct↔enum and enum↔enum cycles, illegal payload shapes, generic-substitution cycles, hash mismatch, and truncation before MIR/codegen | interface decoder corruption tests |
| allocation contract | `Ok`/`None` hot paths allocate no error payload; every constructed owned payload is freed exactly once | benchmark rows for all acceptance edges |

Author-side closure requires every applicable row to point to both code and a passing test before
preflight. A review finding reopens its entire row and all other consumers of the same ownership
fact; it is not closed by patching only the reported expression.

### 8.4 L1b-c tagged type representation and author plan

L1b-c keeps `Ty` and `Scalar` compact and `Copy`. It does not make either recursively heap-owned.
The exact internal record is:

```text
TaggedTypeId = u32

TaggedType
  Option => payload: Scalar
  Result => ok: Scalar, err: Scalar

Scalar::Tagged(TaggedTypeId)
Ty::Tagged(TaggedTypeId)

HIR Program
  tagged_types: sequence<TaggedType>

MIR Program
  tagged_types: sequence<TaggedType>  # concrete, reachable, structurally ordered
```

`Ty::Option` and `Ty::Result` remain the ordinary outer type forms. `Scalar::Tagged(id)` is used
only when one of those forms is itself a scalar payload. `Ty::Tagged(id)` is the reversible
scalar-to-type view of that payload; semantic comparison and display expand it through the owning
program's `tagged_types` table.

There is exactly one structural tagged interner in one sema universe. It admits no duplicate entry,
so raw `TaggedTypeId` equality is semantic equality only inside that universe; the existing derived
`Ty`/`Scalar` equality remains valid there. No raw id is compared across whole-program, producer, or
consumer universes. Cross-universe identity is always the existing id-free recursive `IType`.
Display, diagnostics, and monomorph symbol keys are table-aware and render a complete structural
type; they never print or mangle a raw tagged id.

Generic templates may temporarily reference `Scalar::Param` inside a tagged entry. Substitution
recursively substitutes the complete reachable tagged shape, interns the concrete result, and
leaves no `Param` or template-only tagged entry reachable from a monomorphized signature or HIR
expression. Before MIR lowering, the compiler computes the closure of tagged entries reachable from
every `Ty`/`Scalar` occurrence in concrete function signatures, locals, expressions and their
type-bearing child records, captures, stages, struct/sum/tuple/function types, extern/imported
declarations, and other HIR program fields. It rejects a reachable `Param`, missing id, or cycle;
derives an injective id-free ADT key for every reachable entry; sorts by that key; and remaps every
reachable `Scalar::Tagged`/`Ty::Tagged` reference into one compact concrete MIR table. The key uses
fixed tags for builtin leaves and fully qualified nominal identities plus recursively encoded
generic arguments for struct/sum leaves; it never uses display text, mangled text, or local nominal
ids. Unreachable template entries are not copied to MIR and therefore cannot affect layout or cache
identity. Programs with the same complete reachable tagged set produce the same MIR table and ids
regardless of generic instantiation or source-resolution order.

The interface format remains its existing recursive, id-free `IType` tree. A producer never writes
`TaggedTypeId` into an interface. A consumer reparses the public source-shaped type and rebuilds its
local sema table. Whole-program and per-unit compilations need not assign the same raw id when their
complete reachable sets differ; they must reconstruct the same id-free structural type, LLVM
layout, ABI, and behavior for their shared public surface. MIR's structural codegen-input hash
includes its own complete canonical table in order; an omitted or changed entry cannot hit a stale
object.

LLVM predeclares one opaque struct per distinct tagged body, then assigns the existing Option or
Result body using the already-created struct, enum, and tagged type tables. This preserves the
current Option/Result field order, tag width, alignment, and by-value ABI; it does not disguise a
nested tagged value as a user sum or change the user-sum non-union layout.

That identified struct is the *only* lowering of its Align type. `Scalar::Tagged(id)` reaches it,
and so does the source-shaped `Ty::Option`/`Ty::Result` spelling of the same type in a local,
parameter, or return position: sema treats the two spellings as one type, so a value crosses
between them and a second, structurally-equal-but-distinct LLVM type would make `insertvalue` and
`ret` ill-formed. For the same reason the entry-to-struct map is many-to-one. The MIR table is
keyed on type ids, which are finer than LLVM type identity — two origin-specific generic instances
keep distinct ids while sharing one nominal LLVM type — so entries whose bodies lower identically
share one identified struct, exactly as those instances already share one struct or sum type.

The exact acceptance fixture is:

```align
Output {
  text: string,
  note: Option<string>,
}

NativeError {
  code: Option<string>,
  message: string,
}

DbError {
  Native(NativeError),
  Decode(string),
}

fn run(...) -> Result<Option<Output>, DbError>
```

`Ok(None)` allocates no output or error payload. `Ok(Some(Output { ... }))` owns the two live
`string` leaves selected by the fixture, and `Err` owns only the active `DbError` arm. Heap-owned,
arena-owned non-escaping, and path-dependent ownership cases use this same shape.

The L1b-c author pass closes this matrix before preflight:

| Consumer | Required L1b-c change | Evidence |
|---|---|---|
| type resolution | intern nested `Option`/`Result` payloads; preserve the exact written type and deterministic diagnostics | declaration/signature tests for nested Option, Result, and both arms |
| equality, inference, display, and symbol identity | canonical interning makes raw ids equal only within one sema universe; render and mangle expanded structural shapes; never expose `tagged#N` | same shape reached in different resolution orders, function-type interning, generic mono symbols, and mismatch diagnostics |
| generic substitution | recursively substitute and re-intern tagged entries; reject reachable unresolved parameters before MIR | used and unused generic nested tags, different instantiation orders, generic function, and generic sum tests |
| concrete MIR closure | scan every HIR type-bearing node including expression-only temporaries, collect reachable concrete entries, injective structural-key sort, compact/remap every occurrence, and omit unused template entries | exact table assertions, expression-only nested tags, order-independence, and structural codegen-hash tests |
| cycle and table validation | reject source cycles, missing/out-of-range ids, tagged cycles, reachable unresolved parameters, duplicate/noncanonical entries, and unreachable MIR entries before codegen | sema graph tests and malformed-MIR/codegen tests |
| ownership classification | recurse through tagged entries for Move, Drop, and borrow provenance | DropPlan and region-analysis tests |
| ownership mode | carry definite/maybe individual allocation through every nested tag; preserve all-heap/all-arena behavior and reject nested path-dependent return ownership at the L2 boundary | heap, arena, and path-dependent `Result<Option<Output>, DbError>` cases |
| HIR/MIR construction and transfer | preserve the existing source-nulling, partial construction, `match`, `else`, `?`, and `map_err` rules through nested tags | exact `Result<Option<Output>, DbError>` runtime cases and MIR assertions |
| LLVM layout and Drop | use the existing Option/Result ABI at every level and tag-test every nested Drop | LLVM layout/tag assertions and allocation/free parity |
| physical-classifier closure | handle tagged types explicitly in LLVM scalar/ABI/layout, size/alignment, field permutation, capture/env ABI, allocation provenance, and cleanup-bit classification; keep FFI, box, array, JSON, print, sort, and hash boundaries fail-closed; no catch-all may lower Tagged to i32 | classifier unit tests plus malformed/out-of-range CodegenError tests |
| interface and cache identity | rebuild ids from id-free interface types; reject bad nested arity/name/cycles before MIR; include canonical tagged definitions in structural MIR identity | whole/per-unit executable parity with an unrelated extra tagged type in another unit, interface-only corruption/rebuild, and hash-change tests |
| stale assumptions | remove the L1b-c rejection and sweep old tests, diagnostics, comments, and plans that say nested tagged payloads are unsupported | repository search recorded in the PR |
| scope boundary | retain rejection of recursive inline types, arbitrary Move-element collections, and L2 dynamic path-selected return ownership | negative owner regressions |

This vertical is allowed to exceed the usual 1,000 changed-hand-written-line expectation only if
the final diff shows that separating the table from its validation, structural cache identity, or
LLVM/Drop consumers would create an accepted-but-unsound intermediate compiler. If the author pass
cannot close all rows in one bounded change, revise this section and split L1b-c before admitting
the source type.

## 9. MIR and ABI ownership

The new semantics must be explicit before LLVM:

- HIR/MIR types have one recursive tagged Move/Drop plan for struct/sum/Option/Result payloads.
- HIR function parameters and function-value parameter entries carry
  `ByValue | Out | Borrow | BorrowMut`.
- named/interface signatures and concrete function values carry return-borrow/region summaries;
  concrete closure targets additionally carry capture-slot roots, function-value joins preserve
  selected target-relative capture metadata, and unresolved higher-order parameters use the
  fail-closed all-compatible-input summary.
- recursively Move returns use one dynamic cleanup-bit result in direct, indirect, and imported
  ABIs; callers store the returned bit beside the value and never derive it from region provenance.
- HIR resource types carry declaration identity and the producer-owned Drop-thunk identity/ABI
  fingerprint.
- MIR locals keep the existing path-local cleanup bit for resources and Move aggregates.
- MIR has generic `ResourceFromRaw`, dependent
  `ResourceFromRawBorrowed { resource_def, raw, parent_ref }`, borrow,
  `ResourceViewFromRaw { resource_def, owner_ref, ptr, len, view_kind, validation_plan }`, raw
  extraction, ownership transfer, and Drop operations;
  there is no `DbConnDrop`/`HttpServerDrop` family.
- dependent construction carries the parent generation through HIR/MIR, while raw-view construction
  carries explicit checked size/null/alignment/UTF-8 validation and its successful owner
  generation; LLVM only lowers those facts.
- a `BorrowMut` call checks every peer parameter mode for direct or recursively embedded overlap,
  then ends the owner generation in checked-HIR borrow state before the call.
- borrowed pointees receive no function-exit cleanup in the callee, but replacement through
  `BorrowMut` emits the ordinary old-value Drop plan before the store and updates the caller's
  cleanup bit exactly once.
- direct and indirect calls share the same parameter-mode ABI and alias/provenance checks, including
  the indirect target's joined return summaries.
- named arena lowering reuses `ArenaBegin`/`ArenaEnd` and merely binds the handle as `region`.
- static Query construction lowers to immutable data plus generated binder/decoder functions.
- nested generic package types are monomorphized before this MIR is constructed; MIR never contains
  an unsubstituted `Ty::Param` or a runtime generic dictionary.
- LLVM performs pure representation lowering and does not decide ownership, invalidation, or Query
  semantics.

Every new HIR/MIR/type variant must be wired through the exhaustive region, move, escape, effect,
interface, print, codec, ABI, and codegen classifications. Catch-all defaults for a resource or
borrow-bearing type are prohibited.

## 10. Implementation capability plan

The labels below own exact closure cells and verification, not one mandatory PR
each. Completed historical checkpoints retain their original evidence. Remaining
cells are grouped into the capability waves recorded in `HANDOFF.md`; a wave may
land as one independently correct PR when its complete owner matrix closes.
Any retained cell prose that names an old line target, per-cell PR, post-open
review, pre/post attestation pair, or mandatory benchmark is historical boundary
evidence. It does not override the capability waves, one stable-candidate review
with one coherent finding closure, focused owner checks, local-measurement policy,
or pre-PR attestation defined by `CLAUDE.md` and `HANDOFF.md`.

### L1a — recursive DropPlan framework and `Option<string>` fields

**Status: complete.** This first implementation slice:

- introduce one canonical recursive owned-value/Drop-plan classifier after all struct/enum
  definitions are resolved;
- make `Option<string>` a legal struct field and use it as the first conditional owned leaf;
- permit a recursive Move `Result` Ok producer only when an immediate `?` consumes the tagged
  aggregate; the Err payload must remain shallow because `?` propagates it, and retaining or
  returning the raw tagged value—or declaring it in a parameter/return type—remains an explicit
  L1b diagnostic;
- mark the enclosing struct Move;
- emit tag-tested Drop for the field on normal/early cleanup and drop-old reassignment;
- move/null the live payload on whole-struct moves and supported field extraction;
- keep `Option<MoveStruct>`, Move sum/Result payloads, recursive types, nested partial moves not
  covered by the existing place machinery, and arbitrary Move collection elements rejected with
  explicit diagnostics until their owning slices;
- keep partial replacement fail-closed unless its exact old-value Drop lowering exists: direct
  struct fields support `string` and `Option<string>`, fixed-array element fields support `string`,
  and larger Move leaves require whole-struct or whole-element replacement;
- keep fixed-array element-field reads Copy-only except that `string` is exposed as a borrowed
  `str`; every other Move leaf is an explicit diagnostic;
- add no database, resource, borrow syntax, or runtime library dependency.

Planned changed files:

```text
crates/align_sema/src/lib.rs
crates/align_mir/src/lib.rs
crates/align_codegen_llvm/src/lib.rs
crates/align_driver/tests/owned_tagged_payloads.rs
crates/align_driver/tests/analysis_coverage.rs
bench/owned_tagged_payload/README.md
bench/owned_tagged_payload/.gitignore
bench/owned_tagged_payload/Cargo.lock
bench/owned_tagged_payload/Cargo.toml
bench/owned_tagged_payload/build.rs
bench/owned_tagged_payload/kernel.align
bench/owned_tagged_payload/run.sh
bench/owned_tagged_payload/src/main.rs
docs/impl/07-roadmap.md
docs/impl/17-library-boundary-prerequisites.md
HANDOFF.md
```

Acceptance:

- `struct { detail: Option<string> }` constructs/drops `None` and `Some` without leak or
  double-free;
- whole-struct return/pass/reassignment and `if`/`match`/`loop` joins preserve one live owner;
- replacing `Some(old)` drops old before installing new; `Some -> None` drops old; `None -> Some`
  does not touch uninitialized payload;
- `?` during construction drops every already-initialized owned field exactly once;
- `?` over a bound Result transfers a shallow Move Err before exit cleanup, with allocation/free
  parity at the caller;
- an arena-owned shallow Move Err is rejected at `?` because exit cleanup would end its arena
  before propagation;
- `Option<MoveStruct>` still receives the explicit L1b-not-yet-supported diagnostic;
- invalid recursive and unsupported deep partial-move cases fail with diagnostics, never panic;
- emitted LLVM has one tag branch on Drop and no allocation on the `None`/unrelated success path;
- `scripts/test-pr.sh` and workspace Clippy pass after the focused test.

The benchmark compares a scalar struct and `Option<string>` struct for construct/pass/drop in
always-`None`, always-`Some`, and 1%-`Some` mixes. It records wall time, allocations/frees, and
generated LLVM branch count; it is a regression record, not permission to remove required safety.

The PR acceptance commands are exactly:

```text
cargo test -p align_driver --test owned_tagged_payloads
cargo test -p align_driver --test analysis_coverage
scripts/test-pr.sh
cargo clippy --workspace --lib --bins --locked -- -D warnings
bench/owned_tagged_payload/run.sh
```

The focused test runs successful programs through the normal double-free-sensitive runtime path and
compile-fail programs through the normal diagnostic path. The manual alloc-count benchmark is the
leak-sensitive gate, including field tag transitions and early-`?` construction cleanup; it does
not become an ordinary CI gate.

Recorded native Apple Silicon evidence:

```text
scalar:         0.781 ms  alloc=       0 free=       0
None:           1.644 ms  alloc=       0 free=       0
Some:          37.199 ms  alloc= 1000000 free= 1000000
1%-Some:        2.180 ms  alloc=   10000 free=   10000
replacement:   57.641 ms  alloc= 3000000 free= 3000000
conditional:   25.566 ms  alloc= 1500000 free= 1500000
match/loop:    50.572 ms  alloc= 3000000 free= 3000000
early `?`:     17.223 ms  alloc= 1000000 free= 1000000
raw LLVM Option Drop tag branches: 38
```

The counts are the correctness gate; timings are one manual regression record and remain
host-sensitive.

### L1b — Move sum/Option/Result payload completion

Ship this milestone as the L1b-a/L1b-b/L1b-c sequence fixed in §8.3; do not combine the three
closure boundaries into one review diff.

Milestone scope:

- allow Move structs and Move sums as user-sum, `Option`, and `Result` payloads;
- recursively classify the enclosing tagged value as Move;
- implement tag-switched Drop, move-in/null-source, and move-out/null-container for construction,
  `match`, `else`, `?`, `map_err`, return, reassignment, and branch/loop joins;
- add whole-program/per-unit parity and malformed-interface fail-closed tests.

Acceptance includes the exact `NativeError`/`DbError`/`Result<Option<Output>,DbError>` shape from
§8.1, with allocation counters proving cleanup on every success/error control-flow edge and no
error allocation on an `Ok` hot path. It also includes `Option<MoveStruct>` with nested owned fields,
proving recursive tag-driven Drop and move/null behavior; this case is not an L1a acceptance test.

### L2 — borrowed parameters and interface summaries

Scope:

- contextual parse/check of `borrow`, `borrow mut`, and `out`, including function-type modes;
- `BorrowMut` on writable Copy/Move places and exact function-value mode preservation;
- local no-move/exclusive-alias rules;
- effect inference that treats mutation rooted only in an explicit `borrow mut` parameter as Pure;
- return-borrow inference;
- interface codec/hash support;
- per-unit parity.

The clean reviewed am-r ledger merged in #678 originally described the L2
acceptance cells below. Later size-trigger splits incorrectly turned many
strictly dependent dormant cells into separate PRs and expanded the plan to
thirty-six implementation PRs. That PR count is retired. The rows remain the
authoritative closure checklist, not mandatory PR boundaries. Am-r itself is a
design-only gate. L2b-a2
first isolates product, MIR
action-continuation, and global type-domain validation. Placement, nominal, callable namespace,
declaration/header, and four body-validator construction/activation verticals follow the explicit
am-r design gate below. A PR may
add dormant representation or tighten existing provenance, but it must
not accept source syntax whose complete safety contract belongs to a later milestone.

| Slice | Exact closure | Public exposure at merge | Required gate |
|---|---|---|---|
| L2a | Replace `is_out`/bare parameter-type lists with `ParamMode`; add span-free return-borrow and return-region records to `FnTy`, named/imported signatures, HIR/MIR, interface codecs, hashes, and ABI fingerprints | Existing `ByValue` and `Out` behavior only; `borrow` and `borrow mut` remain identifiers outside parameter-mode lookahead and are rejected as modes | codec byte/hash goldens, corrupt-tag rejection, whole/per-unit identity, and an exhaustive consumer audit |
| L2b-a1 | Infer parameter roots for named functions and preserve conservative flattened roots across recursion, direct/imported calls, control flow, and interfaces | No new borrow mode; aggregate projections and indirect calls retain all-compatible-input unions | scalar direct/imported matrix, semantic interface validation, and summary-inference size/time evidence |
| L2b-a2-s | Add the projection fact and refine named summaries through structs, tuples, block/`if`/loop, field assignment, and destructuring | No new borrow mode; array, pipeline, tagged/control residuals, and indirect calls retain the L2b-a1 all-compatible-input fallback | direct/imported product-view projection matrix and per-unit parity |
| L2b-a2-ac | Close MIR fallthrough propagation after every terminating eager expression child, including enclosing consumers and later siblings | No new borrow mode or provenance precision; source semantics are unchanged; the normal driver still supplies semantically checked HIR | exhaustive recursive-call-site classification, representative family-level no-action assertions, runtime twins, and whole/per-unit continuation parity |
| L2b-a2-am-g-t | Validate concrete roots through every global type table before MIR construction | No source, semantic, HIR, MIR, interface, or ABI change; direct handcrafted-HIR lowering returns a canonical empty program only for an invalid global type domain while every placement, nominal, namespace, declaration/header, and body predicate remains the semantic-checker contract | exhaustive type-domain/root/reference/cycle mutation matrix across all lowering entrypoints and unchanged valid-program MIR |
| L2b-a2-am-r | Design gate completed in #678 and amended by the mandatory am-c pre-implementation review: the public-contract ledger below isolates five producer corrections and one checked-HIR depth-safety closure, then enumerates per-position type admissibility, nominal/link metadata, declarations/headers, body validation, and dependent callable acceptance cells | This row authorizes no implementation itself. The cells are grouped into the capability waves below; they are not a fixed PR count | completed #678 adversarial ledger review plus the am-c matrix review and finding-to-ledger closure covering exact runtime, codec, callable applicability, collision, precedence, and capability boundaries |
| L2b-a2-am-d | Make the fixed conservative checked-HIR record ceiling and the unbounded valid type-DAG domain stack-safe end to end | Preserve every parser-valid source, including diagnosed HIR before producer finalization, while rejecting handcrafted HIR deeper than the fixed 259 ceiling before semantic consumption; every depth-259 body and every finite am-g-t-valid acyclic-inline/header-mediated type DAG at a producer-valid root remains stack-safe from HIR entry through LLVM verification on the 2 MiB test stack | exhaustive constructor-expansion ceiling proof, complete recursive body/type-consumer inventory, common iterative traversal closure, 258/259/260 body cases, deep valid/malformed type-DAG roots, and whole/per-unit MIR/LLVM parity |
| L2b-a2-am-e | Implemented: make the entry producer and backend ABI exact so no-arg `main` returns only Unit, exact signed i32, or `Result<Unit,builtin Error>` and argv main remains the exact Result form | The compiler rejects previously accepted non-C-ABI entry returns with one source diagnostic; Unit/Result wrappers and direct i32 entry behavior remain exact | completed sema signature matrix, whole/per-unit Unit/i32/Result exit behavior, every rejected graph-valid return category, LLVM signature/link/ThinLTO parity |
| L2b-a2-am-f | Implemented: make function completion exact before a non-Unit body reaches HIR/MIR | Bare return and reachable absent tail are valid only for Unit; every non-Unit path returns a typed value or is proven non-fallthrough | completed bare/value return, tail/absent tail, every-control-family, whole/per-unit MIR/LLVM verifier matrix |
| L2b-a2-am-w | Make successful task-wait dominance path-complete before malformed-HIR validation consumes it | Reject a `TaskGet` unless its exact active group proves the Task's born generation valid and the current generation completed, with every earlier drained fallible Wait resolved Ok; carry Wait proofs and Move Task origin proofs through exact transparent local/control flow without a type or runtime ABI change | straight-line/reset, infallible/fallible, stored/copied/reassigned/map_err Result, Task move/reassignment/control origin, if/match/else, loop-break, early-exit, stale Wait alias after Spawn, unresolved/failed first Wait plus empty second Wait, inner-Wait/outer-Task isolation, outer proof handled inside inner group, exited-inner proof clearing, repeated primitive get, whole/per-unit task result matrix |
| L2b-a2-am-v | Require each native output `Buffer` to be a bound `mut` local before the runtime can write through it | Reject temporary and immutable output buffers at ReaderRead, ReaderReadLine, FilePread, UdpRecvFrom, and CryptoRandom; every other native handle and accepted buffer use is unchanged | five-site local/mut/type/diagnostic-order matrix, accepted runtime/allocation twins, whole/per-unit parity |
| L2b-a2-am-u | Make foreign invocation permission lexical and non-escaping (shipped 2026-08-01) | Reject extern function-value formation; direct extern calls and named extern pipeline/reducer/sort callbacks require their invocation expression inside `unsafe`; safe user/imported callable behavior and native RuntimeKey calls are unchanged | direct/callback/FnValue/unsafe-depth matrix, resolver diagnostic order, whole/per-unit extern ABI parity |
| L2b-a2-am-p | Validate every body-independent type placement against its exact sema producer predicate | No source acceptance change; placement-invalid handcrafted HIR becomes canonical-empty | producer/placement Cartesian matrix and valid graph-but-invalid-position twins |
| L2b-a2-am-n | Validate nominal/source identities, complete structural equality, enum/table ordinals, alignment, and link libraries | No source, ABI, or artifact change on valid input | exact-byte/NUL/collision/shape/base/alignment/library matrix |
| L2b-a2-am-h | Validate extern/import/stored/main/local body-independent declarations and headers, and retain normalized imported-effect facts in checked HIR | Header-invalid handcrafted HIR becomes canonical-empty; source/interface behavior is unchanged | mode/signature/summary/imported-effect/main/local/drop-set structural matrix |
| L2b-a2-am-b1 | Build the dormant total-validator core for statements, ordinary expressions, calls, aggregates, tagged values, and structured control | No public entrypoint activation | exhaustive direct discriminator/field unit owners |
| L2b-a2-am-b2a | Extend the dormant validator through fixed/constant array and vector records on the existing am-b1 worklist | No public entrypoint activation | exhaustive direct literal/zip/vector unit owners |
| L2b-a2-am-b2b | Split the same dormant validator after b2a: am-b2b1 closes pipeline terminals/views, while am-b2b2 closes templates, JSON, and group/dictionary records | No general public entrypoint activation; Request 6's scanner Copy predicate is the explicitly named narrow pre-lowering safety exception | exhaustive direct b2b1 pipeline/view owners plus exhaustive b2b2 template/JSON/group owners and the active scanner gate |
| L2b-a2-am-b3 | Extend the dormant validator through every native/runtime family and generated-callable body fact | No public entrypoint activation | exhaustive direct native/helper/generated metadata unit owners |
| L2b-a2-am-b4 | Correlate body-derived ownership and effect facts and activate the complete body validator in every lowering entrypoint | Any invalid body makes the whole MIR program canonical-empty; valid HIR stays byte-identical | full inventory assertion, ownership/effect-cell mutations, parallel-effect twins, depth bound, whole/per-unit identity and benchmark |
| L2b-a2-am-c1 | Make the fixed typed native ABI registry authoritative for declarations, attributes, export verification, compatible extern reuse, and dedicated native consumers while preserving all legacy program/generated string lookups until c3 | Normal MIR/interface/C-runtime ABI, emitted symbol spellings, and legacy mixed-map resolution stay fixed; keyed native LLVM declarations normalize once from hand-written source order to alphabetical `RuntimeKey::ALL` order, changing raw LLVM order and cache identity but not linkage; object-byte equality is not promised either way; an extern that claims a 286-row fixed native symbol with an incompatible source-derived LLVM function type now fails before LLVM, while an exact compatible keyed extern plus builtin or unkeyed extern plus needed wrapper newly shares one declaration and links; a program definition/import with a native physical spelling retains program-before-native uniquification but no longer receives the native row's attributes by prefix accident; dedicated keyed consumers use `RuntimeKey`, and the two main-wrapper consumers use typed unkeyed handles | exact 281-key/286-base-row declarations/order, physical/logical collision order and attribute ownership, export-set, dedicated-consumer, compatible/incompatible extern, eight-specialized/seven-legacy direct parity, and runtime-cost parity |
| L2b-a2-am-c2a1 | Land `FunctionTypeDef`, the private semantic error identity, the one `pub(super)` five-variant node identity relocated without behavior change from `validate_hir`, checked field helpers including parameter-mode encoding, and the one closed primitive/scalar/root encoder used by every later graph path | No `CanonicalTypeView`, graph traversal, `Program` field, public codec, canonical partition/bytes, hash, or emitted-symbol change | private 57-root/34-scalar/6-primitive tag/width/mode matrix, exact semantic-error mapping, and unchanged existing HIR validator owners |
| L2b-a2-am-c2a2a | Atomically move the existing private am-n comparator behind the exact borrowed `SourceShapeView` and preserve its HIR caller, collection classes, cache/restart, bijection, and every `Ty`/`Scalar` result | No observer, complexity instrumentation, benchmark row, canonical view, new traversal, semantic bytes, public surface, or valid/invalid HIR behavior change | existing am-n suite plus wildcard-free 57-`Ty`/34-`Scalar`, five-node projection, minimal-view, cache-extension, and source-inventory parity |
| L2b-a2-am-c2a2b | Add only the zero-sized production observer seam, exact sequence-wide `V/E/P/Q` collector owners, sharing/cycle/restart adversaries, and compiler-only benchmark around c2a2a's unchanged comparator | No comparator semantic/caller/collection change, canonical view, new traversal, semantic bytes, public surface, or valid/invalid HIR behavior change | exact alias-fanout/degree/unique/shared-depth metrics, both-adapter topology/cache owners, unchanged am-n suite, and `canonical-source-shape-comparison` benchmark |
| L2b-a2-am-c2a3 | Land the private borrowed `CanonicalTypeView`, node encoder, and sole `ValidatedGraph::new` raw traversal with field ordinals, reachable/am-n validation, and the shared comparator | No second raw traversal/raw-byte path, `Program` field, public codec, canonical partition/bytes, hash, or emitted-symbol change | five-node/member/reference/am-n/error-precedence/deep traversal matrix and direct transient Fn-table/Fn-root owner |
| L2b-a2-am-c2a4 | Accept only c2a3's `ValidatedGraph` to land the private greatest-fixed-point partition and canonical semantic-to-byte path | No raw view/root entry, `Program` field, public codec, callable activation, hash, or emitted-symbol change | anonymous/nominal equivalence matrix, DFS-first canonical bytes, direct recursive Fn root, and semantic-byte goldens |
| L2b-a2-am-c2b | Retain the compact effect-free MIR function-type table, consume c2a4 to sort/equate it, remap every function-type id, and change structural MIR/cache identity once | No public codec/callable activation or emitted-symbol change; interface bytes/hashes stay fixed | table compactness, every type-bearing field/remap, whole/per-unit MIR/hash parity |
| L2b-a2-am-c2c | Expose dormant `ProgramCall`, canonical type/function wrappers, and the complete public decoder/error surface over c2a4's engine and c2b's retained canonical `Program` table | No generated record, MIR call-field, interface, emitted-symbol, or runtime change | public type/Fn semantic↔byte goldens including `from_program` Fn roots, every malformed field/error-precedence row, deep decode owners |
| L2b-a2-am-c2d | Expose dormant `GeneratedId`/parallel records and record-local codecs over c2c without LLVM naming or collection publication | No MIR call-field, collection pairing, interface, emitted-symbol, or runtime change | generated family/record-local semantic↔byte and malformed owners; collection pairing remains c3 |
| L2b-a2-am-c3 | Separate program/runtime direct targets, type every program declaration/callback, activate encoded Align symbols and generated identities, and preflight every collision before LLVM construction | Existing source spellings remain accepted; internal MIR and LLVM/object program/generated symbol bytes change together, while interface and C/runtime ABI stay fixed | callable applicability matrix, generated-family/pair/probe matrix, external-identity collision/precedence matrix, whole/per-unit/ThinLTO link parity |
| L2b-a2-af | Extend the projection fact through validated fixed arrays and exact/dynamic element reads/writes | No new borrow mode; pipeline, tagged/control, non-fixed collection, and indirect-call residuals retain the L2b-a1 all-compatible-input fallback | direct/imported fixed-array projection matrix and per-unit parity |
| L2b-a2-ar | Close eager retained-storage lifetime for non-fixed `Index`, `ElemField`, `SliceRange`, `ArrayChunks`, and `HttpRespHeader`; make non-fixed `ElemField` receiver-first | No new borrow mode or projection precision; non-fixed results remain flattened | invalidated eager-action matrix, terminating-operand twins, runtime source-order checks, malformed-HIR rejection, and per-unit parity |
| L2b-a2-ap | Extend the projection fact through pipeline `Project`/`WhereField` and terminal formation | No new borrow mode; tagged/control and indirect calls retain the L2b-a1 all-compatible-input fallback; unsupported stages and terminals widen explicitly | direct/imported pipeline-view projection matrix and per-unit parity |
| L2b-a2-t | Complete user-sum/`Option`/`Result`, `match`, `else`, `?`, and `map_err` projection, including the read-only projection of an admitted tagged payload through an already-supported `borrow`/`borrow mut` place | Complete L2b-a2 behavior; no new borrow mode or interface body record; indirect calls retain the pre-L2b fallback. The exact-place predicate and admitted payload grammar are owned by `impl/26-borrowed-sum-projection-plan.md`; this row consumes the existing L2d/L2e place ABI rather than scheduling a later borrow capability | direct/imported tagged-view projection matrix, exact-root mixed-provenance negatives, admitted/unsupported payload matrix, and per-unit parity |
| L2b-a2-ta | Extend the same read-only tagged projection through ordinary dynamic scalar/AoS-record arrays, preserve ordinary Copy-view element regions from direct/field/projected bases, and admit an indexed Move element only at an explicit shared-`borrow` call | No new syntax, borrow mode, mutable element, partial move, interface field, or runtime ABI. The exact grammar, Copy-view `Index`/`ElemField` region mapping, stable indexed base, direct/imported/indirect target modes, eager same-root invalidation exclusion from index formation through every later argument and the call action, terminating-index and terminating-later-argument behavior, return and `borrow mut` retention substitution, MIR-owned argument-position bounds CFG and action-time pointer formation, lifetime, validation order, and exclusions are owned by `impl/28-borrowed-dynamic-aggregate-projection-plan.md`; the two producer-to-consumer halves land atomically because neither is a useful stable consumer alone | direct/nested/imported/generic array-bearing sum matrix; direct/field/projected `array<str>` and AoS Copy-record direct/nested `str`/`slice<T>` return/retention/escape owners plus an admitted-region-bearing-Copy classifier sweep; direct/imported/function-value indexed shared-borrow evaluation/bounds/return-and-retention lifetime matrix; field-complete `BorrowedElementBase.owner_fact` mutation rejection; terminating-index no-action and same-root index/later-argument invalidation negatives; terminating-later-argument no-pointer/no-call twins; malformed HIR/MIR and LLVM no-copy owners; cache parity; and exact align-llm result/evidence adoption |
| L2b-a2-tas | Map ordinary dynamic `array<string>` indexing to the canonical non-consuming `str` view | No new syntax, reference type, whole Move-record value, clone, mutable element, partial move, interface field, or runtime ABI. Existing receiver/index/termination/bounds order, storage generations, contained region roots, temporary-owner handling, and `Index`/`SliceIndex` lowering apply; plan 30 owns the one physical-`String`/logical-`Str` checked-HIR rule and all exclusions | exact String/Str/response/other-Move type matrix; direct/field/projected/temporary lifetime and invalidation owners; every value-carrying control wrapper and all five borrow-transparent receiver scopes; malformed-MIR physical/result mismatch rejection; termination/bounds/MIR no-copy assertions; whole/per-unit/cache parity; all three registered Request 22 adoption targets |
| L2b-b | Extend the same inference to capture roots, closures, function-value joins/moves, direct/indirect targets, and unresolved higher-order fallback | Complete L2b behavior; no borrow mode | indirect/captured/joined nested-view matrix, malformed capture-domain rejection, and indirect-return evidence |
| L2c | Add `ReturnCleanupAbi` to function and interface identity and implement `DynamicBit` for every recursively Move direct, indirect, and imported return; forward the selected bit on every return edge and store it in the caller slot | No borrow syntax; metadata and physical ABI land atomically before borrowed mutation can construct path-selected values | codec/hash goldens, `Result<Option<MoveStruct>, Error>` None/Some/Err matrix, ABI mismatch rejection, per-unit parity, and return-cost evidence |
| L2d | Contextually accept shared `borrow`, preserve the mode in function types/interfaces, pass non-null caller storage, prohibit callee move/drop, and apply the completed return-root summaries | Shared borrow only; `borrow mut` remains unavailable | reusable Copy/Move place, no-copy ABI, move-from-borrow rejection, returned-view invalidation, function-value/import parity |
| L2e | Contextually accept `borrow mut`; complete existing `Out` and new `BorrowMut` under one all-peer recursive exclusivity engine; implement generation invalidation, writable Copy/Move replacement, drop-old/cleanup-bit update, and Pure exclusive-state shaping. For same-program direct bodies, infer the exact parameter roots retained into each mutable destination; imported, indirect, missing-body, malformed, and unresolved calls keep the all-compatible-view fallback | Full L2 surface | all-peer alias matrix, exact direct-retention and conservative fallback matrix, stale-view rejection, changed/unchanged pointee Drop counts, effect matrix, and per-unit parity |

L2a is one intentionally unsplit vertical PR even when its hand-written diff exceeds roughly 1,000
lines. Parameter modes and both summary records participate in one function-signature identity and
must change atomically across AST/HIR/MIR, whole-program and per-unit lowering, interface
serialization, mangling, LLVM validation, and malformed-input tests. Interface and physical ABI
fingerprints are id-free and structural. The internal semantic monomorph key additionally retains a
concrete function-value origin discriminator: same-signature values have independent inferred
effect cells, and deduplicating a Pure and Impure origin would make the selected generic
struct/sum/function HIR read the wrong cell. A generic struct/sum therefore records two names:
the origin-aware internal analysis name and an id-free source nominal name. Source equality,
diagnostics, and LLVM named-type reuse use the latter; generic-function analysis keys use the
former. Every pair sharing a source nominal name must have the same recursively id-free ABI shape
or codegen rejects it before creating LLVM types. This prevents an inferred
`Holder { callback: f }` from becoming incompatible with `Holder<fn(T) -> R>` while retaining
separate Pure/Impure cells. Reassignment, field replacement, control-flow joins, and
source-compatible fixed or dynamic struct-array formation and materialization into an origin-aware
generic aggregate join the affected private effect cells. An explicitly annotated source aggregate
has no intrinsic concrete target: private function parameter, return, local, and loop-result
boundaries therefore use function- or expression-owned projection cells and join every reachable
same-program producer. A closed-world boundary may become `Pure` only after all of those producers
are known. An exportable callback-bearing parameter remains seeded `Unknown` while building its
interface effect summary, regardless of provider-local call sites, because a dependent unit may pass
an unseen callback. Each export is solved independently so its Unknown input cannot contaminate an
unrelated Pure export through a shared private helper; producer-owned exported returns may retain
their inferred origin. Splitting any
one layer or either summary into a separately mergeable PR would temporarily permit two incompatible
identities or require a compatibility path that this pre-release repository forbids. Every callable
signature consumer—including indirect calls, named stages, and terminals—compares recursive
source-visible identity rather than origin-specific internal ids. Pipeline stage and terminal
boundaries join the effective element, accumulator, mapped-result, and capture producers into the
same per-function parameter cells as an explicit call. A direct `Result.map_err` likewise transfers
the error producer into the converter parameter and its converter result back into the mapped error
origin. Static indirect and `map_err` targets also enter the named call graph, so open-world
reachability validates any parallel boundary inside them. L2a does not retain a named target after
a function value is bound, moved, or joined:
an indirect call or `map_err` with a callback-bearing actual remains legal in sequential code but
fails closed when its enclosing function must prove `Pure`. The same unresolved dispatch is rejected
when reachable from an exportable callback-bearing root if it carries a callback-bearing actual or
the erased target can be an internally constructed function value **and the unit contains at least
one `par_map` site**. The purity obligation this protects belongs to `par_map`, whose callables must
be `Pure`; a spawned task deliberately may be `Impure`, so a unit with no `par_map` has no
obligation for an erased target to launder and the dispatch is accepted (2026-08-11 — `pkg.web`
routes middleware and stream pumps exactly this way). Whole-program checking sees every `par_map`
in the program while per-unit checking sees only its own unit's, so a program whose `par_map` and
whose callback-dispatching library live in different units is accepted per-unit and rejected
whole-program; both directions stay conservative with respect to the obligation. The latter covers zero-argument
closures whose captures feed an internal parallel boundary even when unrelated side effects make
both the closed and open-world effects `Impure`; a direct external callback parameter or parameter
field call remains legal. L2b replaces those conservative boundaries with recursive target-relative
provenance through function-value joins.

The amended am-r ledger fixes the independently checkable L2b acceptance cells; the nine added
boundaries are c1 plus the eight dormant c2a1/c2a2a/c2a2b/c2a3/c2a4/c2b/c2c/c2d prerequisites for the final am-c3 activation. L2b-a1 owns named/direct/imported
parameter-root inference, semantic interface validation, and whole/per-unit parity while retaining
flattened all-compatible-input unions for aggregates, indirect calls, and unanalyzed extern
targets. L2b-a2-s adds the projection fact and closes struct/tuple construction, selection,
replacement, destructuring, and ordinary block/branch/loop flow while retaining conservative
array, pipeline, and tagged residuals. L2b-a2-ac next closes MIR continuation after every
terminating eager expression child for semantically checked HIR. L2b-a2-am-g-t then closes only the
global type domain for direct lowering. L2b-a2-am-r is the authoring and review gate that closed in
#678 after covering five producer corrections, checked-HIR depth safety, and the remaining
placement, nominal/link, declaration/header, body, and dependent callable surfaces; it authorizes the fourteen
implementation verticals below.
Am-d first makes every checked-HIR body consumer stack-safe through the fixed conservative producer
ceiling. Unbounded eager-dispatch and type-DAG recursion use explicit work items; structured MIR
control uses small out-of-line continuation frames whose native depth is bounded by that checked-HIR
ceiling and proved on the 2 MiB owner stack. Every finite am-g-t-valid type DAG uses the common
iterative traversal contract. Am-e then closes the C-entry signature hole, am-f closes non-Unit return completeness, am-w
closes task-result wait dominance, am-v closes native output-buffer place/mutability, and am-u
closes escaping or non-lexical extern invocation. Later am-p/am-n/am-h/am-b1–b4/am-c owners inherit that type-DAG contract
before malformed-HIR validation begins. L2b-a2-af closes
validated fixed-array formation and
element reads/writes while retaining conservative non-fixed and pipeline residuals. L2b-a2-ar
closes the affected non-fixed index/range, chunks, and response-header retained-storage actions.
L2b-a2-ap closes pipeline `Project`/`WhereField` propagation and terminal formation. L2b-a2-t
closes tagged values, `match`, `else`, `?`, and `map_err`
without weakening the indirect/unanalyzed fallback. L2b-b finally adds
target-relative capture roots and function-value target sets, validates the explicit-parameter and
capture domains in MIR, removes the now-obsolete unresolved-internal-target effect restriction, and
closes the indirect/captured benchmark row. This split keeps each hand-written diff near the
repository review bound; none of these PRs exposes `borrow` syntax or depends on an incomplete
physical ABI.
L2b-a1 cannot split producer inference from interface validation, direct-call consumption, and
whole/per-unit parity: shipping a non-empty public summary without any one of those seams would
either ignore the fact in one compilation mode or trust an unvalidated artifact.
Its semantic-import correction may exceed the usual 1,000 changed-hand-written-line expectation:
the exact local-definition index, complete shape/header validation, borrow and growth fixed points,
weighted-cycle rejection, closure-matrix owner tests, and import-validation benchmark are one
fail-closed vertical. Splitting before the weighted-cycle gate would accept a malformed or
non-terminating public type graph; splitting the gate from its shape/fixed-point prerequisites would
reintroduce the same false accepts and false rejections that reopened this matrix.

L2b-a2 has one implementation closure matrix. It refines the analysis-local value fact before that
fact is flattened into the already-shipped parameter-root interface summary; it does not add a
serialized projection path. A source parameter seeds one root at the whole-value boundary, so any
nested selection from that single aggregate still names the same parameter. A value assembled
inside the function instead records roots at its exact member/tag path, so selecting one member does
not retain roots that exist only in a sibling. Direct/imported callers consume the resulting
canonical parameter set exactly as in L2b-a1. An imported function that returns a scalar or one
selected nested view is precise at parameter-root granularity: selection from a value assembled
from distinct parameters can remove unselected parameter roots, but selection within one aggregate
actual still names that whole parameter and therefore every caller owner embedded in that actual.
An aggregate returned across the interface likewise remains conservatively rooted in every
parameter represented by that aggregate. L2b-b later applies the same projection algebra to
target-relative captures and indirect calls.

### L2b-a2-ac MIR continuation closure matrix

L2b-a2-ac is a prerequisite implementation slice, not a provenance extension. Its input contract is
the semantically checked HIR supplied by the normal driver. MIR currently uses
an `Operand::Const(Const::Unit)` placeholder when a nested expression has already terminated its
current continuation. Several eager parents consume that placeholder, lower a later sibling, or
append an action because they do not re-check the builder after the child returns. The fix is one
fallthrough protocol across every eager child site. It lands before fixed-array receiver reordering
and before eager retained-storage actions rely on terminating-operand behavior.

`lower_expr` remains typed as returning `Operand`; changing it and every helper to a second public
result algebra would create a larger parallel lowering path without changing HIR or MIR. Instead,
the existing internal `lowering_continues` seam is applied immediately after exactly one child; it
reports whether the builder still has a reachable current continuation and converts an
unterminated zero-predecessor join to `Term::Unreachable`. Every eager parent must stop immediately
on the negative case. A direct tail delegation may return the child's placeholder unchanged only when it performs
no later child, statement, block construction, owner transfer, or action; its first non-tail parent
must apply the required-child check. Statement/function boundaries separately stop before using
the placeholder. The placeholder is never stored, passed, cast, compared, returned as a typed
source value, or used to construct control flow. The check is an in-place post-call macro or direct
branch in the existing caller, never a wrapper that calls `lower_expr`: the latter would add a
second recursive frame per nesting level and violate the measured `expr_depth` stack headroom.
The canonical `lower_required!(builder, child, fallback)` macro expands in the caller to one
`lower_expr` call followed by `lowering_continues`; `fallback` is the enclosing helper's existing
unreachable return shape (`Operand::Const(Const::Unit)`, `None`, `false`, or `()`).

| MIR continuation cell | Required closure | Exact owner evidence |
|---|---|---|
| required-child protocol | Lower one HIR child once, then call `lowering_continues` in the same caller frame before any post-child parent work. Fallthrough returns the exact operand. An unterminated join with no entry predecessor becomes `Unreachable`; all other termination propagates immediately through every enclosing eager parent. A direct tail delegation is the only unchecked form. `BuilderCtx` maintains one reachability bit per block: `new_block` starts false, the function entry starts true, and `terminate` marks `Goto`/`Branch` successors reachable only when it successfully installs the first terminator of a reachable current block. A duplicate terminator debug-asserts and returns without marking any successor. Structured-control lowering must emit every possible predecessor before selecting an unterminated join as current; marking a previously unreachable, already terminated block reachable debug-asserts, so a one-bit state cannot hide a late predecessor. `current_is_reachable` and every required-child check are therefore O(1); no per-child CFG allocation or scan is permitted. `Builder::push` debug-asserts that its current block has no terminator, making a missed same-block action fail in focused tests instead of silently appending before a stored terminator. No helper may wrap and recursively call `lower_expr`. | helper unit assertions for terminated, reachable, zero-predecessor, forward-join, branch, loop-backedge, unreachable-predecessor, ignored duplicate-terminator phantom edge, and rejected late reachability blocks; source audit classifying every recursive child-lowering entrypoint as required child, explicit control continuation, or tail delegation; debug assertions exercised by the exhaustive matrix; passing `within_limit_chain_compiles_and_runs` MIR/codegen depth owner plus a debug `lower_expr` frame no larger than the base-commit measurement; high-CFG MIR-lowering benchmark |
| allowed pre-child preparation | A parent may allocate compile-time MIR slots/values, register a synthetic owner or cleanup bit needed by the child's own early-exit cleanup, or begin an explicit region whose child termination emits the matching cleanup before lowering the child. Completed earlier source operands keep their already-required temporary owners. Infallible type/layout facts already guaranteed by semantic checking may be derived before or between children, including vector lanes, scalar result types, capture-type lists, and argument-type lists; deriving such facts emits no MIR and does not inspect a fallible compiler-owned table lookup. Any fallible parent-result/action metadata lookup—including function-signature table access and element-field-path traversal—and every parent action remains deferred until every required child falls through. These preparations are not evidence of child fallthrough. After a child fails `lowering_continues`, the parent may pop or restore only compile-time lexical bookkeeping—arena/task-group/loop/control frames and debug/span stacks—needed before lowering a sibling CFG arm; that restoration emits no MIR and transfers or disarms nothing. Otherwise the parent may perform only cleanup already owned by the terminating edge. It may not transfer/disarm an owner, mark a destination live, allocate runtime storage, restore runtime/action state, or emit the parent action. | representative `lower_borrowed_owned`, `lower_consumed_call_arg`, aggregate, arena, and task-group termination owners plus the recursive-call-site source audit; exact synthetic-owner/drop-flag state remains covered by the cumulative ownership tests |
| statement and function boundary | `Let`, `LetTuple`, `Assign`, `AssignField`, `AssignIndex`, `AssignElemField`, `AssignElem`, `AssignVecLane`, expression statements, `return`, `break`, tuple destructuring, and function/block tails use only operands from a live continuation. A terminating initializer/index/RHS/value emits no binding store, replacement Drop, destination null/store, tuple extraction, outer return/break edge, implicit Unit return, or later statement/tail. | representative initializer/index/RHS/value twins plus the exhaustive recursive-call-site classification; existing return, break, process, ownership, and pipeline termination tests remain cumulative |
| strict scalar and vector parents | Unary, non-no-op cast, non-short-circuit binary, checked/saturating/wrapping arithmetic, integer/vector division guards, math operations, vector construction/select/shuffle/extract/insert/load/store, raw pointer offset/load/store, and alignment/vector memory actions stop after the first terminating operand in written order. No later operand, divisor/bounds/alignment guard, `Rvalue`, store, or helper CFG is built. A no-op cast may tail-delegate either a fallthrough operand or a terminating placeholder because it performs no later work; the first non-tail boundary still guards it. | unary/cast/binary later-sibling matrix; division and vector-memory twins inspect statements and block count; runtime side-effect counters prove written order and no later action |
| aggregate, capture, and call formation | Fixed/dynamic array, struct, tuple, user-sum, `Option`, `Result`, closure/capture aggregate, generic aggregate, direct call, indirect call, named-call argument list, and callable/capture preparation stop at the first terminating element, field, payload, callee, argument, or capture. No later child, allocation/materialization, call, aggregate `Rvalue`, ownership registration, or destination store is emitted. | first/middle/last aggregate and call operands; named/indirect/captured twins; owned aggregate Drop-count checks; whole/per-unit MIR parity |
| template and string-builder formation | `Template` may register its hidden owned-string cleanup before holes, then lowers text, primitive/string/JSON holes, option/struct/array access, comma control, and union values in written order. The first terminating hole stops every later part and emits no `Rvalue::Template`, uninitialized result use, owner disarm, or parent action; its already-registered hidden owner remains correctly false/cleaned on the terminating edge. `BuilderNew`, every builder write kind, and finish apply the same rule to capacity, builder, and argument operands. | representative first/middle/last hole and builder-operand twins plus the recursive-call-site classification; exact Template absence and cumulative hidden-owner/Drop owners |
| storage, view, and collection read | Every ordinary `Index` discriminator, fixed/dynamic `ElemField`, `SliceRange`, `ArrayChunks` direct/materialized actions, `ArrayToSlice`, `ArrayToSoa`, field/nested-field read, string/bytes view, dict/struct-array access, and buffer operation stops before the next bound/index/value or read action. Fixed scalar `Index`, whole-element fixed `StructArray` `Index`, and fixed `ElemField` receiver/index twins are cumulative prerequisites for af. Ac changes no shipped receiver order. | exact MIR no-action assertions for fixed/non-fixed receivers, bounds, loads, owner inheritance, and later children; constant `IndexField` recorded as having no eager child; dynamic/SoA order parity |
| native and runtime action | JSON, I/O, filesystem, path, socket/network, process, environment/CLI, encoding/compression/crypto, random/time, regex, HTTP/client/server, task, and unsafe/native helpers apply the same required-child protocol to every source-level operand before allocation, native call, state change, or helper CFG. Existing operation-specific validation order is unchanged because ac runs only after checked HIR formation. | representative single- and multi-operand native helpers plus the exhaustive recursive-call-site classification; focused existing family tests stay cumulative |
| structured control continuation | `if`, `match`, `else`, `?`, `map_err`, short-circuit boolean, loop, arena, task-group, unsafe, and nested block helpers distinguish a terminated arm from an explicitly created reachable join. They may switch `Builder.cur` only to a block with a real predecessor or an operation-defined early-return edge. A fully terminating construct propagates termination; a mixed construct yields only its fallthrough alternatives; no placeholder supplies a join value. | fully terminating/mixed/all-fallthrough triples for each control family; exact predecessor, phi/store, cleanup, and result assertions; nested eager parent around each triple |
| pipeline and callback action | Existing source/stage/terminal continuation gates remain authoritative. Required-child checks cover source, stage operand/capture, terminal argument/capture, initializer, reducer, destination, and JSON-scanner callbacks before allocation, loop state, callback call, source nulling, or cleanup transfer. Ac does not reorder a pipeline operand or change effect/provenance inference. | cumulative `terminating_pipeline_operand_emits_no_terminal_state`, capture-order, source-shape, driver runtime, and effect-source-order matrices, each nested under a strict eager parent |
| owner, cleanup, and allocation parity | A terminating child owns the cleanup and control edge it already emitted. Its parent performs no Drop, drop-flag write, source nulling, cleanup transfer, allocation, owner inheritance, or action-side restoration. Completed earlier operands retain only cleanup required on the terminating edge. Fallthrough allocation and Drop order are byte-for-byte unchanged. | owned earlier-operand + terminating-later-operand Drop-count twins; MIR drop-flag/null/transfer assertions; allocation counter parity on all-fallthrough twins |
| narrow malformed-HIR defense | Ac may replace a direct index or shape assumption touched by its continuation edits with a checked lookup, but this is defense in depth rather than a complete handcrafted-HIR contract. Missing indirect-function type metadata and an invalid element-field path terminate before the parent action. L2b-a2-am-g-t owns only the global type domain; the completed am-r ledger assigns the remaining structural boundary to am-p/am-n/am-h/am-b1–b4/am-c. | `malformed_hir_continuation_metadata_fails_closed` covers exactly the indirect-function signature and fixed/dynamic/SoA element-field-path cases, including no dynamic/SoA length action before rejection; no broader malformed-HIR claim is attached to ac |
| public and artifact boundary | No AST/HIR/MIR/LLVM/interface type, tag, codec, fingerprint, cache identity, source syntax, ownership rule, or runtime ABI changes. Whole-program and per-unit lowering call the same internal continuation implementation. | interface/hash goldens remain cumulative; focused whole-program and per-unit runtime twins agree, the existing single-unit gate retains MIR/object identity, and the high-CFG lowering row records continuation cost |

The author-side matrix-to-diff pass must account for every recursive child-lowering entrypoint after
the change: direct `lower_expr`, `lower_expr_for_borrow`, `lower_block`,
`lower_block_for_borrow`, `lower_borrowed_owned`, `lower_consumed_call_arg`, and any helper that
delegates to them. Each call points to one row above and is either guarded at the immediate
required-child boundary, part of an explicit structured-control continuation with predecessor
evidence, or a side-effect-free tail delegation. Iterator-based eager lowering is converted to
written-order loops so it can stop at the first terminating child. A helper that creates blocks,
pushes statements, allocates slots, mutates cleanup state, or lowers another child is never a tail
delegation.

The implemented slice is approximately 1,700 changed hand-written lines. It cannot split safely by
expression family: leaving any eager parent unchecked would still allow a typed placeholder or
later sibling to escape through an otherwise fixed child, while downstream af/ar termination
claims would depend on that gap. Reachability state, the caller-local guard, every recursive parent
family, and their whole/per-unit owners are therefore one compatibility boundary. A fresh
adversarial preflight found that the original matrix also attached a much broader handcrafted-HIR
validation contract to this slice. That contract is independently mergeable and would require
checks across every HIR family, so it is not part of ac.

### L2b-a2-am-g-t immediate closure and completed am-r design gate

At the am-g-t checkpoint, only am-g-t was authorized for implementation by this section. Normal
compiler input is unchanged:
semantic checking still owns user diagnostics and supplies valid HIR. Direct `lower_program`
callers, tests, and future tooling may construct an invalid global type graph, so am-g-t validates
the complete type domain before any MIR record is copied or any function is lowered. Any am-g-t
failure returns the canonical empty MIR `Program` with every vector empty. Valid HIR produces
byte-for-byte identical MIR.

The former combined am-g/am-b design is reopened after am-g-t. Its implementation checkpoint mixed
type-domain validation with nominal/link validation and reached 1,535 changed hand-written lines.
A fresh boundary review proved those phases have no atomic dependency. Later post-open reviews also
found that the broader matrix omitted exact per-position producer type admissibility and the full
callable namespace: compiler runtime lookup keys, exact emitted identities, and body-generated
`$fnval`, `$clos`, task-trampoline, and parallel-kernel names. Rejecting source-accepted exact
compiler/runtime spellings as malformed HIR would itself be a hidden semantic change. Am-r
therefore had to publish and pass a new public-contract ledger before any placement, nominal/link,
namespace, declaration/header, or body implementation began. That ledger merged in #678 and fixes
the exact remaining closure cells. Its original per-cell PR split and count are retired by the
capability-wave plan above.

| Slice / malformed-HIR cell | Required closure | Exact owner evidence |
|---|---|---|
| am-g-t concrete type roots and total type domain | Before copying any record, validate tuples, structs, enums, and every type reachable from those tables, an `extern`/imported declaration, or a stored-function header. Every stored struct, enum, tuple, tagged-type, and function-type entry is a concrete root unless it belongs to an abstract template graph: a node that contains `Scalar::Param`/`Ty::Param` or transitively depends on such a node. An unreachable abstract template graph is permitted because the producer retains generic-template interner entries that MIR omits; a concrete root that reaches one rejects. Every non-template entry remains a root even when otherwise unreachable, so a missing id, inline cycle, or invalid concrete scalar cannot hide in discarded state. Every reference must be in range even inside a permitted template graph. Traverse with an explicit enter/exit worklist and visit colors rather than native recursion. `IntTy.bits` is exactly `8`, `16`, `32`, or `64`; `FloatTy.bits` is exactly `32` or `64`; the same widths apply inside every `PrimScalar`. `Vec`/`Mask` accepts only an integer/float scalar and exactly `2`, `4`, `8`, or `16` lanes. Every `Struct`/`Enum`/`Tuple`/`Tagged`/`Fn` discriminator resolves to its matching table, every struct-bearing collection resolves a struct, and `DictEncoded(id, field)` resolves an in-range `str` key field. `Ty::IntVar`, `Ty::FloatVar`, `Ty::Error`, and HIR-reachable `Ty::StrFinder` reject. Fixed arrays, tuples, structs, enums, `Option`, `Result`, and nested tagged payloads extend the active inline-layout path and reject an inline cycle. `Box`, slices, dynamic arrays, `ArrayBuilder`, `Task`, dynamic struct arrays, SoA, scanners, dictionary headers, and function closures validate their referenced entries but break that inline path; header-mediated nominal recursion is valid. Am-g-t validates graph formation only: it does not claim that every valid type is admissible in every field, payload, tuple element, parameter, return, local, or body position. | one mutation for every `Ty`, `Scalar`, and `PrimScalar` discriminator; every width/lane boundary; missing/wrong-kind table id and dictionary field; inline-cycle rejection and `Box`/dynamic-array/task/function-header positive cycle twins; reachable/unreachable `Param` nominal, tagged, and function-type twins; unused malformed non-template entries; first/middle/final concrete roots; placement-invalid but graph-valid positive twins remain unchanged for am-r; invalid results have every vector empty in all four entrypoints |
| am-r completed remainder | #678 records the exact producer predicate for every field, payload, tuple element, parameter, return, local, and body position; nominal/link identities and validation order; logical callable keys; stored/imported/extern emitted identities; compiler/native declarations and compatible reuse; every `$fnval`, `$clos`, task-trampoline, and parallel-kernel identity; source encoding/NUL rules; ownership/allocation; cache and ABI effects; error precedence; and every valid producer twin. It preserves current source acceptance except for the five explicit producer corrections. | one owner row per placement and generated-symbol family; exact compiler/runtime spelling positives; duplicate/cross-class/multi-invalid precedence; whole/per-unit/cache identity; completed fresh independent adversarial review of the ledger and proposed PR boundaries |

### L2b-a2-am-r public-contract ledger

This ledger closes the reopened row above. Apart from the exact am-e, am-f, am-w, am-v, and am-u producer
corrections recorded below, it preserves every currently accepted source spelling and type
placement. Validation is an internal HIR-to-MIR precondition, not a new language diagnostic or
artifact format. Every invalid placement, global identity, callable identity, declaration/header,
or body returns the canonical empty MIR `Program`; body rejection is deliberately program-wide
rather than per-function. A valid caller cannot retain a call to a rejected body, and no partial
MIR, native declaration, Align-program/runtime/native/artifact/cache allocation, ownership
transfer, or cacheable object survives rejection. Compiler-owned validation worklists may allocate
and are released before return.
All four whole-program/per-unit lowering entrypoints run the same phases before copying a HIR
record:

```text
am-g-t type graph
am-d   iterative checked-HIR depth closure
am-e   exact source-entry and C-entry ABI
am-f   non-Unit return and fallthrough completeness
am-w   outcome-sensitive successful task-wait dominance
am-v   native output Buffer local/mutability
am-u   lexical non-escaping extern invocation
am-p   placement predicates
am-n   nominal and link metadata
am-h   declarations and body-independent headers
am-b4  activated total body validator assembled by am-b1/am-b2/am-b3
am-c   typed callable targets and injective emitted identities, consuming am-b4-valid body facts
```

The exact public records are:

| Entry | Inputs and defaults | Result, ownership, allocation, and artifact effect |
|---|---|---|
| `lower_program(&hir::Program) -> mir::Program` | shared HIR borrow; no ambient/default input | valid input returns the existing owned whole-program MIR; invalid input returns a newly owned `mir::Program` whose every vector is empty |
| `lower_program_located(&hir::Program, &SourceMap) -> mir::Program` | shared HIR and source-map borrows; no ambient/default input | same validation/result; invalid input does not read source text or construct line tables |
| `lower_program_per_unit(&hir::Program) -> mir::Program` | shared per-unit HIR borrow; no ambient/default input | valid input preserves imported declarations/exportability; invalid input returns the same all-empty record |
| `lower_program_per_unit_located(&hir::Program, &SourceMap) -> mir::Program` | shared per-unit HIR/source-map borrows; no ambient/default input | combines the preceding two contracts |

No entry adds a public error or diagnostic result. The caller owns the returned MIR and drops it
normally. Validation allocation is compiler-only and is absent from MIR, interface, object, and
package artifacts. Every valid-input identity owner compares the complete returned MIR with the
pre-validation internal lowering path, including located lines and whole/per-unit distinctions.

The first failing phase wins. Within a phase, vectors use stored order and bodies use function then
statement/expression stored order. Each body record validates its envelope fields in the exact
field order recorded by the body ledger, then its child expressions left-to-right to completion,
then cross-field and derived-result facts. Thus an invalid parent id/discriminator beats every
child failure, the first invalid child beats a later child, and a child failure beats an invalid
derived `Expr.ty`. Validators return only success/failure;
they allocate compiler-owned sets, maps, and explicit worklists with lifetime limited to the
lowering call. They neither retain HIR text nor allocate runtime/package state. Rust `String`
already guarantees valid UTF-8. Identity compares exact UTF-8 bytes without normalization.
U+0000 is permitted in source string literals and rejected in every identifier, nominal, callable,
extern, generated-name input, and link record before it reaches LLVM, C, or a linker. These checks
run before any side effect.

#### Type-placement ledger

The following producer sets are exact:

- `ty-scalar` is `ty_to_scalar`: integer, float, bool, char, unit, struct,
  owned string, primitive-element owned array, AoS struct array, response array, `str`,
  primitive-element slice, reader, writer, regex, captures, file, parsed CLI, TCP connection,
  TCP listener, UDP socket, child, HTTP response, HTTP server, HTTP request context, response
  builder, HTTP stream, run output, SoA, JSON document, enum, concrete tagged value, or a template
  parameter. It also includes `Buffer`; `CliCommand`, `HttpRequest`, `HttpClient`, `Command`,
  `HttpHeaders`, and every non-scalar composite are absent.
- `payload-scalar` is `scalar_arg(..., allow_param=true)`: `ty-scalar` minus `Buffer`, with nested
  `Option`/`Result` interned as `Tagged`. `Param` is legal only in an abstract template.
- `collection-scalar` is `scalar_arg(..., allow_param=false)` plus `Fn`: `ty-scalar` minus
  `Buffer`, `Reader`, `Writer`, `Regex`, `Captures`, `CliParsed`, `TcpConn`, `TcpListener`,
  `UdpSocket`, `Child`, `File`, `HttpResponse`, `HttpServer`, `HttpRequestCtx`, `HttpStream`,
  `ResponseBuilder`, and `RunOutput`, then plus `Fn`. Owned I/O handles are never collection
  elements because element reads copy the handle without a move-out and the generic array Drop path
  cannot release one handle per element.
  The resolver rejects a nested owned `array<array<T>>`: `ty_to_scalar` requires the inner owned
  array element to be primitive, so `Ty::DynArray(Scalar::DynArray(…))` is graph-valid HIR but not
  producer-valid. Am-p rejects that shape before MIR, while preserving the separate body-level
  indexing rejection for any other Move element that the producer admits.
- `fn-scalar` is `ty-scalar` without `Slice`. A first-class callable return is `fn-scalar` or
  `Result`; an annotated `FnTy` parameter uses `ty-scalar`, preserving the currently accepted
  slice annotation, while an actual named/lifted function value uses `fn-scalar`.

| Position, in validation order | Exact accepted producer contract | Required valid twin and invalid owner |
|---|---|---|
| struct field | `Int`, `Float`, `Bool`, `Char`, `Str`, `String`, `Struct`, `Enum`, every `is_move_handle` leaf, `HttpHeaders`, `Slice`, `Fn`, recursively admissible `Option`/`Result`/`Tagged`, `DynArray`, and `DynStructArray`. A direct `DynArray(String)` field rejects; the recursive `Option`/`Result`/`Tagged` field predicate follows the current sema producer and therefore admits a nested `DynArray(String)` payload. Every inline struct reached through a direct/tagged/enum field is acyclic and has no `align(N)`. `layout(C)` narrows the field to integer or float. | one positive per arm, direct/tagged nesting including nested `DynArray(String)`, Move enum/struct-array, and `http_headers`/function/slice fields; one wrong placement per arm and every `layout(C)`/alignment/cycle edge |
| concrete enum payload | Integer, float, bool, char, `Str`, `String`, `Struct`, `Enum`, `Fn`, `ResponseBuilder`, recursively concrete `Option`/`Result`/`Tagged`, `DynArray` except `DynArray(String)`, and `DynStructArray` whose element struct is non-Move. Inline struct payloads must be acyclic and have no `align(N)`; inline struct/enum/tagged cycles reject. | positive direct Move struct/enum, function, builder, tagged, scalar array, and non-Move struct-array payloads; owned-element, over-aligned-inline-struct, and recursive negatives |
| generic enum template and monomorph | A template first uses `scalar_arg(..., allow_param=true)`: `payload-scalar`, including `Param`, nested tagged parameters, and `ResponseBuilder`, but not the concrete-only `Fn` extension. A monomorph substitutes every parameter, then applies the same `enum_payload_ok` predicate and rechecks graph-dependent struct/struct-array ownership after all definitions resolve. The producer predicate preserves `ResponseBuilder` through a generic substitution, while a concrete `Fn` payload remains a direct-only extension; the validator accepts only the union actually emitted by these two paths and does not widen the template path to the concrete path. | abstract unused template twins, a `ResponseBuilder` concrete substitution, and concrete `Fn`/builder positives versus generic-monomorph negatives |
| tuple element | Exactly integer, float, bool, char, `Str`, `String`, `DynArray`, or `DynStructArray`; order is significant and duplicate tuple element lists are one interned identity. A Move tuple Drop recursively dispatches each owned element through its concrete type, including deep `array<string>` and `array<Move-struct>` elements. | one positive per kind, deep tuple-drop owner coverage, and all other graph-valid scalar/composite negatives |
| `Option`/`Result` payload | `scalar_arg(..., allow_param=true)`: `payload-scalar`, with nested `Option`/`Result` interned as `Tagged`; abstract `Param` is template-only. | every payload kind, nested tagged values, and excluded buffer/builder/header/composite twins |
| box type argument | `scalar_arg(..., allow_param=false)`, then reject `Struct`, `Enum`, every `Scalar::is_move`, and `Str`. The admitted type-formation remainder is integer, float, bool, char, unit, primitive `Slice`, SoA, JSON document, and a concrete non-Move `Tagged` value. This is deliberately broader than value construction: `heap.new` additionally rejects `Slice`, whose borrowed view cannot be stored as an owned box payload. | one type-formation positive for every admitted remainder including `Slice`/SoA/JSON/tagged; `heap.new(Slice)` body negative; struct/enum/owned/`Str`/parameter negatives |
| slice/dynamic-array/builder type argument | `collection-scalar` for slices and established scalar dynamic arrays. A dynamic struct array instead records its exact struct id and rejects an over-aligned element. Every owned I/O handle, including `File`, is rejected because the generic array Drop path cannot release one handle per element. SoA separately requires a non-empty struct containing only integer, float, bool, char, or `Str` fields. `ArrayBuilder` records either an exact scalar descriptor or one of the closed vector, mask, fixed-scalar-array, and fixed-struct-array aggregate descriptors; constructor validation narrows the heap form to primitive Copy scalars/String or the closed `HeapRecord` predicate and the explicit-region form to recursively `RegionPlain` concrete types. | one positive per type-argument family including `Fn`; every explicitly excluded handle/File/nested/over-aligned/SoA-field/builder negative; every builder descriptor positive plus invalid lane, length, scalar, nominal-id, `HeapRecord`, and region/heap predicate twins |
| fixed-array literal element | Body-owned, not am-p-owned. A fixed struct array admits an over-aligned struct and records the padded/aligned slot contract. A scalar literal rejects every owned handle including `File`, every slice-bearing non-struct, and a Move enum; all elements have one checked type, `ArrayLit.elem` matches it, and the length fits the stored type. | over-aligned fixed-struct positive; `File` type-formation-positive/literal-negative twin; handle/slice/Move-enum/type/length/pooled-state matrix in am-b2 |
| vector and mask element | Integer or float with exactly 2, 4, 8, or 16 lanes. | every width/lane endpoint and bool/char/aggregate negatives |
| annotated `FnTy` type positions | Each parameter is `ty-scalar`. The return is any graph-valid non-`Error` type currently produced by `resolve_type`; the body/call validator separately requires each actual callable origin to satisfy `fn-scalar` parameters and a `fn-scalar`/`Result` return. Mode cardinality/class and summaries belong only to am-h. Imported effect transport belongs to am-h; body-correlated effect cells and parallel eligibility belong only to am-b4. | slice- and buffer-parameter annotation positives, actual fn-value slice negative, Result-return handler, and one type-position mutation per branch |
| stored source function or monomorph type positions | Each parameter and return is a concrete `resolve_type` result. A parameter is not `Box`; a return is neither `Box` nor `Fn`. A monomorph contains no reachable `Param`. Modes, `main`, summaries, and local records belong only to am-h. | every source-nameable parameter/return family, Box/Fn boundary negatives, generic substitution twins |
| imported function type positions | Same source-function type-position contract as its producer, plus id-free structural ABI type identity and no abstract/private type identity. Modes, summary equality, and interface header facts belong only to am-h. | whole/per-unit identical type twins and one type-position corruption at a time |
| extern parameter and return type positions | Parameters are integer, float, raw, `Str`, numeric `Slice`, or a non-empty `layout(C)` struct. Returns are unit, integer, float, raw, or a non-empty `layout(C)` struct. Target-specific SysV size/register rejection remains codegen-owned after this target-independent validation. Modes and summaries belong only to am-h. | scalar/view/C-struct positives; empty/non-C/wrong field/view-return type negatives |
| local, expression, statement, and block-tail position | A local may carry any concrete graph-valid type actually produced at that body point, including compiler-only task, dictionary, scanner, builder, and handle types. There is no global local allowlist. Am-b derives every expression result, requires exact equality with `Expr.ty`, then requires initializer, assignment, return, break, argument, capture, stage, and tail positions to equal their declared producer type and ownership facts. | valid producer twin for all 240 `ExprKind` variants; wrong `Expr.ty` and wrong consumer position for every family |

Am-p owns this table and nothing else. It validates global/table/header placements whose producer is
body-independent. The body-correlated final row is specified here but implemented only by am-b.
This keeps `am-p` independent and prevents it from guessing whether a graph-valid local type was
actually produced by its initializer.

#### Am-p review-finding closure matrix

This matrix is authoritative for the post-preflight am-p correction. Every negative fixture first
proves the global type graph is valid, then proves the placement predicate rejects it. Producer
changes and validator changes share one row so a placement predicate cannot silently drift from its
sema producer.

| Cell | Required closure | Exact owner evidence |
|---|---|---|
| recursive field placement | Reject graph-invalid nested owned-array shapes and File collection elements while preserving producer-valid `Option`/`Result`/`Tagged` nesting. A direct `array<string>` field becomes producer-valid only through the closed §7.6 capability; before that implementation lands, the producer rejection remains authoritative. | `valid_hir_type_placement_preflight_is_mir_identity`, §7.6 direct `array<string>` positive plus excluded composite-array twins, graph-invalid nested-array and File-collection negatives |
| generic sum producer | `enum_payload_ok` and the placement predicate both admit `ResponseBuilder` after generic substitution; concrete `Fn` remains direct-only. | `generic_enum_response_builder_monomorph_is_producer_valid`, concrete/generic builder and `Fn` twins |
| header type formation | Header returns/parameters use the exact `resolve_type` nameable set; body-only `CliParsed`, HTTP request/response/client/server, command, and run-output types reject. | `body_only_header_types_fail_placement_closed`, source/imported/FnTy header twins |
| abstract box | `box` payload formation never admits `Param`, including an unreachable abstract `FnTy` node. | `abstract_box_param_fails_placement_closed` |
| shared tagged DAG | Tagged validation memoizes completed nodes per placement mode, including the inline-struct alignment walk, rejects active cycles, and remains linear per reachable edge rather than exponentially revisiting shared subgraphs. | `deep_hir_type_dag_placement_is_stack_bounded` reaches the DAG through an imported header and an inline struct field; shared tagged-DAG owner |
| inline enum alignment | Reject an `align(N)` struct nested in an enum payload or an outer struct's enum field because enum payload storage is inline and the LLVM type cannot carry the custom member alignment. | `malformed_hir_type_placement_fails_closed` graph-valid over-aligned enum payload and `Outer { e: EnumWithAlignedPayload }` negatives |
| tuple deep ownership | Every accepted Move tuple element reaches the same recursive destructor as a standalone value; an outer collection free must not skip nested string or Move-struct element storage. | `tuple_drop_uses_recursive_element_destructor` plus the tuple element producer/placement matrix |
| owner isolation | Graph-valid placement negatives reject through all four lowering entrypoints without publishing partial MIR; graph-invalid fixtures remain owned by am-g-t. | graph-valid placement-negative matrix plus existing malformed global matrix |

#### Nominal, link, and callable identity ledger

Am-n visits structs, enums, tuples, then link libraries. Struct/enum `name` and `source_name` are
non-empty exact UTF-8 without U+0000. Internal names are unique across the combined nominal
namespace. A repeated `source_name` is legal only for the same nominal kind and identical complete,
recursively id-free shape: declaration/member order, field/variant names, type graph, alignment,
`layout(C)`, function modes, summaries, and callable ABI; the private function-effect origin is
excluded. Field and variant names match `[A-Za-z_][A-Za-z0-9_]*` and are unique in their
declaration. Tuple element vectors are unique. Alignment is `None` or a power of two in
`1..=2^29`. Enum `field_base` starts at 1 and each next value is the checked preceding base plus
payload length; the flattened count fits `u32`. Link libraries are unique, non-empty, do not start
with `-`, and contain only ASCII alphanumeric bytes or `._+-`.

Source-name preservation has positive owner evidence, not only rejection mutations. Two generic
nominal instances that differ only in private function-effect origin retain distinct `name` values
but the same producer `source_name` and identical complete source shape; am-n accepts both and the
canonical source ABI bytes agree. Two modules declaring the same bare type spelling retain their
distinct producer-mangled `source_name` values and canonical bytes differ. Mutating only
`source_name` while keeping the private name and shape fixed also changes the canonical bytes.
These twins prove that validation neither substitutes private `name` for `source_name` nor erases
the source-visible nominal identity.

Am-c replaces the ambiguous string-only callable table with three typed registries:

```text
ProgramCall(exact HIR logical name) -> Align definition/import/extern FunctionValue
RuntimeCall(RuntimeKey)             -> exact align_rt_* FunctionValue
GeneratedCall(GeneratedId)          -> private compiler helper FunctionValue
```

The concrete Rust representation is:

```rust
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ProgramCall(Box<str>);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProgramCallError {
    Empty,
    EmbeddedNul,
    TooLong,
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct CanonicalTy(Box<[u8]>);

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct CanonicalFnAbi(Box<[u8]>);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CanonicalCodecError {
    Truncated,
    TrailingBytes,
    UnsupportedVersion,
    UnknownTag,
    InvalidBool,
    InvalidUtf8,
    EmbeddedNul,
    InvalidWidth,
    InvalidCount,
    MissingReference,
    DuplicateMember,
    NonCanonicalOrder,
    InvalidSummary,
    InvalidGraph,
}

#[derive(Clone, Debug)]
pub struct FunctionTypeDef {
    pub params: Vec<(ParamMode, Scalar)>,
    pub ret: Ty,
    pub return_borrow: ReturnBorrowSummary,
    pub return_region: ReturnRegionSummary,
}

struct CanonicalTypeView<'a> {
    structs: &'a [StructDef],
    enums: &'a [EnumDef],
    tuples: &'a [TupleDef],
    tagged_types: &'a [hir::TaggedType],
    fn_types: &'a [FunctionTypeDef],
}

#[derive(Clone, Debug)]
pub struct ProgramExtern {
    pub name: ProgramCall,
    pub params: Vec<Ty>,
    pub param_modes: Vec<ParamMode>,
    pub ret: Ty,
    pub return_borrow: ReturnBorrowSummary,
    pub return_region: ReturnRegionSummary,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[repr(u16)]
pub enum RuntimeKey { /* the 281 variants below, in that exact order */ }

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[repr(u8)]
pub enum UnkeyedRuntimeKey {
    ReportError = 0,
    ArgsBuild = 1,
    ArenaReset = 2,
    Realloc = 3,
    HttpSerialize = 4,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum RuntimeAbiId {
    Keyed(RuntimeKey),
    Unkeyed(UnkeyedRuntimeKey),
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum DirectCall {
    Program(ProgramCall),
    Runtime(RuntimeKey),
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum GeneratedId {
    FnValue {
        target: ProgramCall,
        signature: CanonicalFnAbi,
    },
    Closure {
        lifted: ProgramCall,
        explicit_signature: CanonicalFnAbi,
        captures: Vec<CanonicalTy>,
    },
    Task {
        fallible: bool,
        result: CanonicalTy,
    },
    Parallel(ParallelGeneratedId),
}
```

`ProgramCall`, `ProgramCallError`, `CanonicalTy`, `CanonicalFnAbi`, `CanonicalCodecError`,
`FunctionTypeDef`, `ProgramExtern`, `RuntimeKey`, `DirectCall`, `GeneratedId`, and the parallel
identity records are public types in `align_mir`.
`RuntimeKey` lands in c1. C2a1 lands the public effect-free `FunctionTypeDef` plus the private
semantic error/node identities and closed primitive/scalar/root field encoder. C2a2a extracts the
existing private am-n sharing-preserving comparison behind one typed `SourceShapeView` while
preserving HIR validation exactly. C2a2b adds only observation, exact work owners, and the
compiler-only benchmark. After that mandatory checkpoint, C2a3 lands the private borrowed `CanonicalTypeView`, node
encoder, and sole `ValidatedGraph::new` traversal with reachable/am-n validation; the view is
borrowed for one engine call and stores/hashes nothing. C2a4 accepts only `ValidatedGraph` and adds
greatest-fixed-point equivalence plus canonical semantic-to-byte encoding. C2b
forms that exact record vector from validated HIR, passes it with the existing MIR
tables through the view, then retains the remapped vector as `Program.fn_types`. `ProgramCall`/errors and canonical wrappers/errors/decoders
land in c2c; dormant generated identity records/codecs land in c2d;
`ProgramExtern`, `DirectCall`, and every callable-field conversion activate in c3.
`CanonicalTypeView` remains private to `align_mir`; all other types named public above retain that
visibility in their landing slice. `RuntimeKey` contains only the backend-agnostic semantic target and exact logical MIR spelling;
it contains no LLVM type, attribute, or physical-symbol policy. `UnkeyedRuntimeKey`,
`RuntimeAbiId`, the LLVM-only `RuntimeAbi` row, and all declaration/attribute materialization remain
private to `align_codegen_llvm`; no MIR, package, or runtime API exposes them. The code snippets use
`pub` on the two unkeyed identity enums only within that private codegen module.

`ProgramCall::try_from_logical(&str) -> Result<ProgramCall, ProgramCallError>` is public because
hand-built MIR and the separate codegen crate must be able to form declaration keys. It accepts
only a non-empty, NUL-free exact UTF-8 logical name whose byte length fits `u32`; the error is
exactly `Empty`, `EmbeddedNul`, or `TooLong`, checked in that order. Registry membership is not a
constructor precondition: whole-record validation later proves that each call target is present in
the already-formed declaration registry. It owns one boxed copy; that
storage and its clones are ordinary compiler allocations. The allocation itself never reaches an
artifact, while the semantic name bytes participate in the structural MIR hash, encoded
`align_fn$...` identity, `GeneratedId` canonical bytes/stems, LLVM/object symbols, and debug output
exactly as specified below.
`ProgramCall::as_str(&self) -> &str` and `ProgramCall::as_bytes(&self) -> &[u8]` publicly borrow
that exact owned UTF-8 allocation; neither normalizes, copies, nor appends a terminator. Codegen
uses only these accessors for encoded symbols and generated readable stems.
`RuntimeKey::ALL: [RuntimeKey; 281]` lists declaration order, and
`RuntimeKey::logical_name(self) -> &'static str` returns the exact snake-case MIR alias from the
key list below (`Print` returns `"print"`). Each public Rust variant is formed mechanically by
splitting that listed ASCII name on `_`, uppercasing only the first ASCII letter of each nonempty
segment, preserving every remaining letter and digit byte exactly, then concatenating the segments:
`utf8_valid -> Utf8Valid`, `base64url_decode -> Base64urlDecode`,
`crypto_aes_gcm_open -> CryptoAesGcmOpen`, and `print_f32 -> PrintF32`. This rule plus the exact
snake-case list is the complete variant table. Neither API exposes or derives the physical native
symbol; the backend-private exhaustive `RuntimeAbi` match owns that mapping and its three
exceptions.
`RuntimeKey`, `UnkeyedRuntimeKey`, and `RuntimeAbiId` are `Copy`. `DirectCall` and `GeneratedId` are compiler-owned values with lowering-call
or codegen-module lifetime; no runtime allocation or Drop contract is introduced.

The exact MIR field change is one-for-one:

| Current field | Am-c field |
|---|---|
| `Function.name: String` | `Function.name: ProgramCall` |
| `ImportedFn.name: String` | `ImportedFn.name: ProgramCall` |
| `Program.externs: Vec<hir::ExternFn>` | `Program.externs: Vec<ProgramExtern>` |
| discarded `hir::Program::fn_types` | c2b: `Program.fn_types: Vec<FunctionTypeDef>`: compact, effect-free, canonical, and the owner of every retained `Ty::Fn`/`Scalar::Fn` id |
| `Rvalue::Call(String, Vec<Operand>)` | `Rvalue::Call(DirectCall, Vec<Operand>)` |
| `Rvalue::FnAddr { name: String, signature }` | `Rvalue::FnAddr { target: ProgramCall, signature }` |
| `Rvalue::Closure { lifted: String, captures, capture_tys, signature }` | `Rvalue::Closure { lifted: ProgramCall, captures, capture_tys, signature }` |
| `ParMapStage.func: Option<String>` | `ParMapStage.func: Option<ProgramCall>` |
| `Rvalue::ParMapParallel.func: String` | `Rvalue::ParMapParallel.func: ProgramCall` |
| `Rvalue::ParMapReduce.func: String` | `Rvalue::ParMapReduce.func: ProgramCall` |
| internal `Reducer::{Fold,AnyAll}.func`, `PreparedCollectKind::Scan.func`, and `SortKey.func` strings | the same fields as `ProgramCall`; no helper converts back to a logical string |

HIR remains string-bearing. Am-b3/b4 first proves every HIR callable/body relation from
[`19-hir-validation-ledger.md`](19-hir-validation-ledger.md). Am-c then converts a validated named
target to `ProgramCall`, and converts a compiler builtin/native selection directly to
`RuntimeKey`; it never classifies a program target by spelling. Codegen accepts `DirectCall` and
cannot perform a string lookup that crosses the two classes.

Declaration-registry construction visits stored functions, imports, then externs in vector order.
The typed declaration fields above let it include uncalled declarations, exports, and main without
reconstructing a key from a call site. HIR-to-MIR conversion first forms `FunctionTypeDef` records
by dropping only `FnEffect`. It then computes the canonical definition equivalence described below,
removes unreachable and equivalent function definitions, sorts the surviving equivalence classes
by their canonical root bytes, and remaps every `Ty::Fn` and `Scalar::Fn` in headers,
nominal/tuple/tagged/function tables, slots, values, and embedded MIR fields to the new `u32`
ordinal. The stored table is compact and canonical before `impl_hash` or codegen; whole-program and
per-unit lowering produce the same table for the same retained type roots. A missing/out-of-range
function reference, a non-compact table, a duplicate canonical definition, an unreachable retained
definition, or any table/reference semantic mismatch makes hand-built MIR invalid before registry
or cache publication. Producer owners prove that every old id is remapped; an in-range numeric id
carries no provenance, so the consumer does not claim to distinguish a stale old ordinal from the
same valid new ordinal. The effect-free `FunctionTypeDef` cannot represent an effect-bearing replay
record; effect removal is instead proved by HIR-to-MIR construction and the record's closed field
shape.

`CanonicalFnAbi` is the ordered parameter list of `{ mode, CanonicalTy }`, the return
`CanonicalTy`, and exact borrow and region summaries. It has no return-cleanup field in am-c:
`ReturnCleanupAbi` does not exist until later L2c, which must extend this record and its byte
encoding atomically when it lands. It excludes effect because effect changes call legality, not the
physical thunk signature.

The two canonical wrappers own already-validated canonical bytes. Their exact API is:

```rust
impl CanonicalTy {
    pub fn from_program(root: Ty, program: &Program) -> Result<Self, CanonicalCodecError>;
    pub fn decode(bytes: &[u8]) -> Result<Self, CanonicalCodecError>;
    pub fn as_bytes(&self) -> &[u8];
}

impl CanonicalFnAbi {
    pub fn from_parts(
        params: &[(ParamMode, Ty)],
        ret: Ty,
        borrow: &ReturnBorrowSummary,
        region: &ReturnRegionSummary,
        program: &Program,
    ) -> Result<Self, CanonicalCodecError>;
    pub fn decode(bytes: &[u8]) -> Result<Self, CanonicalCodecError>;
    pub fn as_bytes(&self) -> &[u8];
}
```

Each successful `CanonicalTy::from_program`, `CanonicalFnAbi::from_parts`, and `decode` retains
exactly one caller-owned `Box<[u8]>` containing the canonical bytes. Decode never borrows into or
retains its input: it validates and re-encodes/copies into that one box. An error returns no retained
allocation. The private c2a2a comparator retains exactly its existing caller cache, queue, seen set,
and two bijection maps; its borrowed node projection allocates nothing. C2a2b's zero-sized
production observer also allocates nothing. The c2a3/c2a4 graph engine may use transient compiler-owned vectors/worklists and one
growable output buffer within the exact complexity bounds below; all are
released before either return, and
`into_boxed_slice` transfers that output allocation as the one retained box on success.
`as_bytes` borrows the box for `&self`'s lifetime and allocates/copies nothing. Derived `Clone`
deep-copies the canonical bytes into exactly one new box and shares no mutable storage; dropping
either clone releases only its own box. None of these allocations is an Align/runtime/artifact
allocation. `ProgramCall::clone` follows the same one-new-box deep-copy rule for its UTF-8 bytes.

The closed public `CanonicalCodecError` mapping is exact:

| First failing condition | Error |
|---|---|
| input ends before the current fixed-width scalar, declared text bytes, vector element, node, or nested record completes, including a decoded count whose promised elements are absent | `Truncated` |
| bytes remain after the one requested top-level record | `TrailingBytes` |
| top-level version byte is not `1` | `UnsupportedVersion` |
| any otherwise complete discriminator, parameter mode, or enum tag is outside its listed set | `UnknownTag` |
| a boolean byte is not `0` or `1` | `InvalidBool` |
| a complete text payload is not UTF-8 | `InvalidUtf8` |
| a complete UTF-8 callable, nominal, or member text contains NUL | `EmbeddedNul` |
| a concrete integer/float width or vector lane is outside the am-g-t-admitted set | `InvalidWidth` |
| an encoder-side text, vector, or node count does not fit `u32`; no decoder-side `u32` value itself overflows | `InvalidCount` |
| the first definition reference in field order is out of range or does not name the required node kind | `MissingReference` |
| the first repeated field/variant name or second serialized node in one canonical equivalence class | `DuplicateMember` |
| all semantic fields are valid but root/node/member/reference order or re-encoded bytes are noncanonical | `NonCanonicalOrder` |
| the first summary has an empty explicit root set, unsorted/duplicate/out-of-range index, nonempty capture before enabled, or borrow/region disagreement | `InvalidSummary` |
| the first remaining record-local invariant fails: an empty decoded `ProgramCall`; a serialized nominal/member invariant listed below; invalid `work_weight`; impossible generated mode/stage relation; or another explicitly listed record-local semantic rule | `InvalidGraph` |

Decode checks fields from left to right. It reports an incomplete field before inspecting later
bytes; otherwise it validates that field's version/tag/bool/text/NUL/width constraint immediately.
Every semantic constraint is charged to the earliest canonical field whose value can violate it,
even if a reference or cross-node shape can be decided only after the complete graph is parsed.
After parsing, the decoder returns the failure with the lowest field encounter ordinal. At the same
field the tie order is scalar syntax/UTF-8/NUL/width, local `InvalidGraph`, `DuplicateMember`,
`MissingReference`, then `InvalidSummary`. A cross-node nominal/equivalence failure is charged to
the end of the second node; an overall generated mode/stage relation is charged after the last
stage, then `work_weight` is charged at its later stored field. Only when every record field is
semantically valid does trailing input yield `TrailingBytes`; canonical re-encoding/order is the
final check and yields `NonCanonicalOrder`. Encoding uses the same field ordinals and tie order,
with `InvalidCount` checked before allocating output.

Canonical node decoding replays every am-n invariant that is representable in these bytes. In node
ordinal and stored member order, `InvalidGraph` applies immediately to an empty `source_name`; a
present struct alignment that is not a power of two in `1..=2^29`; a field/variant name that does
not match `[A-Za-z_][A-Za-z0-9_]*` (including empty); an enum whose first `field_base` is not `1`,
whose later base is not the checked preceding base plus preceding payload length, or whose flattened
payload count overflows `u32`; and the second nominal node with the same `source_name` but a
different kind or different complete recursively id-free shape. NUL text still fails earlier as
`EmbeddedNul`; a repeated valid member/variant name, repeated anonymous tuple vector, or second
same-kind/same-source/same-shape nominal equivalence class is `DuplicateMember`; a missing/wrong-kind
reference remains `MissingReference`. At a single member, text/NUL/identifier checks precede its
duplicate-name check, then its type/reference. An earlier member reference therefore beats a later
member's invalid identifier/base, while an identifier or duplicate on the same member beats that
member's missing reference. At a nominal collision, the failure is charged after the second node's
full shape. Internal nominal names and link libraries are not serialized and
therefore remain enclosing `Program` validation, not decoder claims.
`CanonicalTy::decode` proves only closed graph validity. Position-specific am-p/am-h rules are
checked by the enclosing function ABI, task, parallel, declaration, or MIR-field validator after
decode; a standalone type decoder does not claim a parameter/return/capture placement.
There is no encoded canonical-type artifact or cache ingress in am-c. Decode is the independently
tested inverse and the required gate for any future ingress; current registries construct semantic
records and encode them before cache publication.

The parallel record is closed:

```rust
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[repr(u8)]
pub enum ParallelKernelMode {
    Materialize = 0,
    Reduce = 1,
    FilterCount = 2,
    FilterScatter = 3,
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum ParallelStageId {
    Map {
        target: ProgramCall,
        abi: CanonicalFnAbi,
        input: CanonicalTy,
        output: CanonicalTy,
        captures: Vec<CanonicalTy>,
    },
    Filter {
        target: ProgramCall,
        abi: CanonicalFnAbi,
        input: CanonicalTy,
        output: CanonicalTy,
        captures: Vec<CanonicalTy>,
    },
    FilterStrContains {
        input: CanonicalTy,
        output: CanonicalTy,
        needle: CanonicalTy,
    },
    Project {
        input: CanonicalTy,
        output: CanonicalTy,
        field: u32,
    },
    FilterField {
        input: CanonicalTy,
        output: CanonicalTy,
        field: u32,
    },
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ParallelGeneratedId {
    pub mode: ParallelKernelMode,
    pub source: CanonicalTy,
    pub terminal_input: CanonicalTy,
    pub terminal_output: CanonicalTy,
    pub terminal: ProgramCall,
    pub terminal_abi: CanonicalFnAbi,
    pub terminal_captures: Vec<CanonicalTy>,
    pub stages: Vec<ParallelStageId>,
    pub work_weight: u8,
}
```

`source` is the exact source-container type, not only its element type. `input`/`output` are the
stored stage types; their canonical reachable nominal definitions supply field layout identity.
`Project.field` and `FilterField.field` are logical field ordinals. A string-filter needle is
runtime data, so identity carries its type but not its bytes. Capture values likewise do not affect
generated code; their ordered types do. `work_weight` is exactly `1`, `2`, or `4`.
`Materialize` requires no filtering stage, `Reduce` requires the stage-free integer reduction form,
and a chain containing any Filter/FilterStrContains/FilterField produces one `FilterCount` and one
`FilterScatter` identity with otherwise identical fields. No raw nominal, tuple, tagged, or
function-table id is present.

Canonical bytes use this closed codec. Every integer is unsigned little-endian. Every count and
byte length is `u32`; values that do not fit reject. A boolean is one byte, exactly `0` or `1`.
UTF-8 text is `u32 byte_length || bytes`, with no normalization; callable/nominal/member text also
rejects embedded NUL before encoding. Decoders reject unknown tags, invalid booleans, overflow,
truncation, trailing bytes, duplicate definition fields/variants, out-of-range definition
references, non-canonical root/definition order, every serialized am-n invariant above, and any
graph the am-g-t contract rejects.
Every version, definition/root/scalar/primitive/generated/stage tag, parameter mode, layout,
optional-alignment discriminator, summary discriminator, kernel mode, and boolean occupies exactly
one `u8`. Integer and float bit widths are one `u8`; only the already-valid concrete widths admitted
by am-g-t encode. Definition references, field ordinals/bases, vector lanes, fixed-array lengths,
summary indices, counts, and text lengths are `u32`. No native `usize`, signed integer, enum memory
layout, padding, or host endianness enters the bytes.

`CanonicalTy` is `version=3:u8 || node_count:u32 || nodes || root_type`. Nodes are assigned ordinals
by first visit in a depth-first walk from the root; struct fields, enum variants/payloads, tuple
elements, tagged payloads, and function parameters are visited in stored declaration order.
Repeated and recursive references emit the first assigned `u32` ordinal. The node tags and payloads
are:

| Tag | Definition node payload |
|---|---|
| 0 | struct: source-name, optional alignment (`0` or `1 || u32`), C-layout bool, field count, then each field name and type |
| 1 | enum: source-name, variant count, then each variant name, `field_base:u32`, payload count, and payload scalars |
| 2 | tuple: element count and scalar elements |
| 3 | tagged: `0 || option scalar` or `1 || ok scalar || err scalar` |
| 4 | function: parameter count, each mode and scalar, return type, borrow summary, region summary, cleanup ABI; no effect or raw fn-table id |
| 5 | resource: source name, internal name, declaring module, Drop hook, Drop thunk, representation version, 16-byte Drop-ABI fingerprint, and generic arity |

Struct/enum nodes include nominal `source_name` and their complete reachable shape. They exclude
origin-aware private `name`. The fingerprint is nominal plus structural: different public nominals
never merge, and the same source nominal with a different reachable graph never shares a helper or
cache key. Cycles through box, function, or another header-mediated edge terminate through node
references rather than truncation.

“Visited” means a canonical definition-equivalence class, never a raw table id. Struct and enum ids
are equivalent exactly when kind and `source_name` agree; am-n has already proved that every such
alias has the same complete reachable source shape, and a mismatch is invalid rather than a second
class. Tuple, tagged, and effect-free function nodes use the greatest fixed-point structural
equivalence over kind, every non-reference field, ordered member/parameter sequence, summary, and
the current equivalence classes of referenced nodes; nominal references contribute their
kind/`source_name` class. Implementations compute that partition iteratively to stability with the
initial class keyed by kind plus non-reference fields, never with recursion or source id. Equal
anonymous/function definitions therefore merge even when HIR assigned fresh ids; unequal reachable
graphs never merge. Encoding walks the root's stable equivalence classes depth-first and assigns
ordinals on first visit. Decoding rebuilds the same partition, rejects two serialized nodes in one
equivalence class as `DuplicateMember`, and rejects any node/ordinal order different from the
depth-first first-visit re-encoding as `NonCanonicalOrder`.

The current `root_type` tags `0..=59`, in exact order, are:

```text
Int Float Bool Char Option Result Tagged Box Array Vec Mask StructArray DynStructArray Slice Soa
DynSliceArray DynArray DynResponseArray Str String ArenaHandle Raw Builder Writer Reader Buffer
ArrayBuilder StrFinder File Rng Regex Captures CliCommand CliParsed TcpConn TcpListener UdpSocket
Child Command RunOutput HttpRequest HttpResponse HttpClient HttpServer HttpRequestCtx
ResponseBuilder HttpStream HttpHeaders JsonDoc JsonScanner Struct Tuple Fn Enum Task DictEncoded Unit
Resource ResourceRef DynAggregateArray
```

`Int` is `signed:bool || bits:u8`; `Float` is `bits:u8`. `Bool`, `Char`, the closed handles, `Str`,
`String`, `Raw`, and `Unit` have no payload. `Option`, `Result`, `Box`, `Array`, `Vec`, `Mask`,
`Slice`, `DynArray`, and `Task` encode their scalar(s), then any `u32` length/lane.
`DynSliceArray` encodes a primitive scalar. `StructArray` encodes a struct-node reference and
length; `DynStructArray` encodes a struct-node reference and layout (`0=Aos`, `1=Soa`); `Soa`,
`JsonScanner`, and `Struct` encode a struct-node reference. `Tagged`, `Tuple`, `Fn`, and `Enum`
encode the matching node reference. `DictEncoded` encodes a struct-node reference then field
ordinal. `Resource` and `ResourceRef` encode a resource-node reference. `DynResponseArray` has no
payload. `ArrayBuilder` encodes `0 || scalar` or `1 || aggregate-element`; `DynAggregateArray`
encodes an aggregate element directly. Aggregate-element tags are exactly `0=Vec`, `1=Mask`,
`2=FixedArray`, and `3=FixedStructArray`; vector/mask records encode scalar then lanes, fixed scalar
arrays encode scalar then length, and fixed struct arrays encode a struct-node reference then
length. All lengths and lane counts are `u32`.

Valid scalar tags `0..=35`, in order, are:

```text
Int Float Bool Char Unit Struct String DynArray DynStructArray DynResponseArray Str Slice Enum
Tagged Soa JsonDoc Reader Writer Buffer Regex Captures CliParsed TcpConn TcpListener UdpSocket
Child File HttpResponse HttpServer HttpRequestCtx ResponseBuilder HttpStream RunOutput Fn Resource
ResourceRef
```

Scalar `Int`/`Float` use the same width payloads; `Struct`/`DynStructArray`/`Soa` use a struct-node
reference; `Enum`, `Tagged`, and `Fn` use their matching node reference; `DynArray` and `Slice`
contain one primitive scalar. Primitive scalar tags are exactly `0=Int`, `1=Float`, `2=Bool`,
`3=Char`, `4=Str`, `5=String`, with width payloads on Int/Float. `Param`, `IntVar`, `FloatVar`,
`Error`, a missing definition, and a raw table id have no encoding.

Parameter modes are `0=ByValue`, `1=Out`, `2=Borrow`, `3=BorrowMut`; am-c input still rejects the
later two modes. A summary is `0=None` or `1 || param_count || params || capture_count || captures`;
indices are `u32`, strictly increasing in each vector. In am-c every capture vector is empty.
`CanonicalFnAbi` is
`version=1 || param_count || (mode || CanonicalTy)* || return CanonicalTy || borrow || region`.

The public cross-crate codec surface is exact:

```rust
impl GeneratedId {
    pub fn to_canonical_bytes(&self) -> Result<Box<[u8]>, CanonicalCodecError>;
    pub fn decode(bytes: &[u8]) -> Result<Self, CanonicalCodecError>;
}
```

`to_canonical_bytes` validates every record-local tag, width, count, and mode/stage relation,
allocates exactly one caller-owned boxed result, and returns no partial bytes on error. The
enclosing preflight separately validates program-target membership/signature, capture relations,
and collection pairing before it calls this method; this argument-free codec does not claim access
to a declaration registry or sibling record. Callers retain that box as the borrowed byte
view for sorting, deduplication, cache lookup, and printed-stem construction; `GeneratedId` does not
hide a second cache or allocation. `decode` owns all nested values in the returned record and
borrows the input only for the call. Both use only the closed `CanonicalCodecError` enum and the
same left-to-right field, record-local semantic, and trailing-byte precedence specified above.

`GeneratedId` is `version=1` followed by `0=FnValue`, `1=Closure`, `2=Task`, or `3=Parallel`, then
the fields in the Rust declaration order above. `ProgramCall` is encoded as its length-prefixed
UTF-8 bytes. A vector is `u32 count` then elements. `ParallelGeneratedId` encodes its nine fields
in declaration order. `ParallelKernelMode` is its explicit `repr(u8)` value.
`ParallelStageId` is `0=Map`, `1=Filter`, `2=FilterStrContains`, `3=Project`, or
`4=FilterField`, followed by that variant's fields in declaration order. Decoding repeats the
semantic validity relations above and rejects a mode/stage combination that no producer emits.
All tags in these records are `u8`; all vector and text lengths are `u32`. Individual
`GeneratedId::decode` validates one record only. Collection validation separately requires every
`FilterCount` record to have exactly one otherwise-byte-identical `FilterScatter` record and vice
versa after equal-record deduplication; a missing or non-identical twin is `InvalidGraph` before
name reservation. No individual decoder claims that its collection twin exists.
Semantic-to-byte and independent byte-to-semantic goldens are:

| Semantic value | Canonical hex |
|---|---|
| `CanonicalTy::Unit` | `010000000038` |
| `CanonicalTy::Bool` | `010000000002` |
| signed `CanonicalTy::i64` | `0100000000000140` |
| `CanonicalFnAbi { params: [], ret: Unit, borrow: None, region: None }` | `01000000000100000000380000` |
| `GeneratedId::FnValue { target: "f", signature: preceding ABI }` | `0100010000006601000000000100000000380000` |
| `GeneratedId::Closure { lifted: "l", explicit_signature: preceding empty ABI, captures: [Bool] }` | `0101010000006c0100000000010000000038000001000000010000000002` |
| `GeneratedId::Task { fallible: false, result: Unit }` | `010200010000000038` |
| `GeneratedId::Task { fallible: true, result: i64 }` | `0102010100000000000140` |
| `GeneratedId::Parallel { mode: Materialize, source: slice<i64>, terminal_input: i64, terminal_output: i64, terminal: "f", terminal_abi: fn(ByValue i64) -> i64 with None/None, terminal_captures: [], stages: [], work_weight: 1 }` | `01030001000000000d000140010000000000014001000000000001400100000066010100000000010000000000014001000000000001400000000000000000000001` |

Each golden is encoded from the semantic record and decoded from the literal bytes in separate
tests. Malformed goldens flip version/tag/bool, truncate each scalar width, add one trailing byte,
and replace a definition reference with `u32::MAX`.

HIR-to-MIR lowering tags compiler-native calls as `RuntimeCall`; a user call with the same bytes
remains `ProgramCall`. This is an internal MIR representation change and lands atomically across
HIR lowering, MIR, whole/per-unit codegen, serialization-free consumers, tests, and benchmarks.
It does not reserve a new source spelling. Non-exported whole-program Align definitions and
per-unit Align definitions/imports use the injective emitted identity
`align_fn$<UTF-8-byte-length>$<lowercase-hex-bytes>`; the same function gets the same identity in
producer and consumer units. The length is canonical base-10 ASCII with no leading zero, and each
raw logical-name UTF-8 byte contributes exactly two lowercase hexadecimal digits; the length prefix
itself is not hex-encoded. Explicit `--export` roots retain their requested exact external
identity. Extern C symbols retain their exact identifier. Direct-i32 `main` emits `main`; Unit or
`Result` main emits the encoded Align identity plus the generated external `main` wrapper.
Explicit exports, extern definitions, and the external `main` identity are pairwise distinct. An
extern declaration may reuse an exact fixed-base native emitted identity only when its complete
LLVM parameter/return ABI is identical to that native registry entry; both logical targets then
point to the one declaration. The eight verification-only probe spellings are not native registry
members and follow the ordinary program/extern/export collision rules in normal builds.
A mismatched native redeclaration, an explicitly exported Align definition using a native identity,
or any other external emitted collision rejects before LLVM construction. A non-exported Align
definition whose logical bytes equal a native symbol or generated stem is encoded and remains
valid. Thus an accepted
extern declaration of `align_rt_print_i64` with one `i64` parameter remains accepted, while a
second definition cannot collide with the linked runtime. The encoded per-unit symbol scheme changes
object bytes once. The typed call target changes each affected unit's structural MIR `impl_hash`,
and the compiler binary change changes `compiler_build_id`; both are already codegen-cache key
fields, so no old producer or consumer object can hit. Interface bytes/hashes, source semantics,
the C/runtime ABI, and structural type fingerprints do not change.

ThinLTO consumes the same final emitted registry. In per-unit mode a derived-exportable source
function and its imported declaration use the identical encoded Align identity and remain external;
whole-program non-exported definitions and every generated helper remain internal. Explicit export,
extern, native, and external `main` roots retain their exact external identities. ThinLTO
internalization may not reclassify a function from logical-name spelling or reconstruct a generated
identity from its readable stem. Owner builds run ThinLTO off/on for whole-program and two-unit
inputs, compare link success and behavior, and assert that the implementation/object cache misses
once for the representation/compiler-build change and then hits on an unchanged rebuilt input.

The collision matrix is exhaustive:

| Input pair | Result |
|---|---|
| two stored program definitions with the same logical name | am-n/am-h reject |
| stored definition and import with the same logical name | exact compatible declaration/definition pair only; otherwise reject |
| stored definition and extern declaration with the same logical name | reject regardless of ABI; source definition and foreign C declaration are distinct callable classes |
| imported declaration and extern declaration with the same logical name | reject regardless of ABI; imported Align and foreign C declarations never dedupe |
| repeated compatible declarations within the imported class or within the extern class | dedupe to one program registry entry |
| incompatible repeated extern declaration | reject |
| non-exported program logical name equal to a RuntimeKey, native symbol, `align_gen$...` stem, or encoded-name-looking string | accept; emit the length-plus-hex `align_fn$...` identity |
| encoded identity of a stored definition or import equals a different-logical-name extern's exact symbol | reject before LLVM; neither declaration wins |
| encoded identity of a stored definition/import or wrapped-main body equals a different function's exact explicit-export identity | reject before LLVM; neither claimant wins |
| encoded identity of a wrapped-main body equals an extern's exact symbol | reject before LLVM; neither claimant wins while external `main` remains separately reserved |
| compatible extern logical/emitted name equal to one of the 286 fixed base native symbols | accept and reuse the one native declaration |
| extern or export equal to one of the eight verification-only probe spellings | accept under the ordinary program/external rules in a normal build; probe-feature runtime fixtures never link user artifacts |
| incompatible extern equal to a fixed base native symbol | reject |
| repeated identical `--export` root for the same function and exact external identity | idempotent dedupe at the first CLI occurrence; cache identity remains sorted/deduped |
| same explicit-export external identity claimed by a different function or claimant class | reject at the later CLI occurrence |
| explicit export equal to a native symbol, extern emitted identity, or external `main` | reject |
| direct-i32 `main` plus any other claimant of external `main` | reject; otherwise direct main alone emits `main` |
| wrapped main plus any other claimant of external `main` | reject; otherwise wrapper alone emits `main` and Align body remains encoded |
| two equal `GeneratedId` values | dedupe |
| unequal valid generated values with the same readable stem | unavailable: family prefixes and complete canonical identity make stems injective; conflicting FnValue/Closure metadata rejects before naming |
| generated candidate equal to an extern exact symbol or explicit-export external identity beginning `align_gen$` | probe; never reject the source spelling |
| generated candidate equal to an encoded non-exported program identity, fixed native identity, or external `main` | unavailable: their prefixes/exact spelling are respectively `align_fn$`, `align_rt_`, and `main` |

Owner tests include separate positives for a non-exported runtime-key-equal logical name, a
non-exported native-symbol-equal name, and a generated-stem-equal name; compatible and incompatible
native externs; stored-definition/extern and imported/extern cross-class rejection even when unused;
ordinary extern/export use of every probe spelling in a normal build; explicit export collision;
repeated identical export-root idempotence and a later different-claimant twin;
direct and wrapped main; duplicate equal generated records; semantic rejection of conflicting
FnValue/Closure records before naming; generated candidates with one/two candidates occupied by
extern or explicit-export global-registry identities; and unavailable fixed-native/encoded-program/
`main` generated-collision assertions.

The exact `RuntimeKey` set is:

```text
alloc alloc_size_fail arena_alloc arena_begin arena_end
array_builder_append array_builder_build array_builder_build_stack array_builder_free
array_builder_free_stack array_builder_free_strings array_builder_free_strings_stack
array_builder_init_stack array_builder_new array_builder_new_in array_builder_push
array_builder_push_bytes array_builder_push_str
base64_decode base64_encode base64url_decode base64url_encode bounds_fail
buffer_append buffer_bytes buffer_free buffer_len buffer_new buffer_put
builder_finish builder_finish_stack builder_free builder_free_stack builder_init_stack
builder_into_string builder_into_string_stack builder_new builder_pop_comma builder_write
builder_write_bool builder_write_char builder_write_f32 builder_write_f64 builder_write_int
builder_write_json_str builder_write_str_int_str bytes_as_str child_free child_kill child_wait
chunks cli_command cli_command_free cli_flag_bool cli_flag_i64 cli_flag_str cli_get_bool
cli_get_i64 cli_get_str cli_parse cli_parsed_free cli_usage command_cwd command_env
command_env_clear command_free command_new command_run command_timeout
compress_gzip_compress compress_gzip_decompress compress_zstd_compress compress_zstd_decompress
crypto_aes_gcm_open crypto_aes_gcm_seal crypto_argon2id
crypto_chacha20_poly1305_open crypto_chacha20_poly1305_seal
crypto_ct_equal crypto_hkdf_sha256 crypto_hmac_sha256 crypto_random
crypto_sha256 crypto_sha512 dict_encode_str dict_lookup div_fail dns_resolve env_get env_set
form_decode form_encode free free_response_array free_string_array fs_exists fs_read_bytes_view
fs_read_dir fs_read_file fs_read_file_view fs_remove fs_write_file fs_write_file_builder
gather_i64 group_count_i64 group_count_str group_count_str_cols group_max_i64 group_max_str
group_max_str_cols group_min_i64 group_min_str group_min_str_cols group_multi_str group_sum_i64
group_sum_str group_sum_str_cols hash128 hash64 hex_decode hex_encode html_escape
http_accept http_body http_client_free http_client_get http_client_new http_client_post
http_client_request http_client_timeout http_ctx_body http_ctx_free http_ctx_header
http_ctx_method http_ctx_path http_get_many http_header http_parse http_rb_body http_rb_header
http_request http_request_free http_resp_body http_resp_free http_resp_header http_resp_status
http_respond http_respond_stream http_response_free http_response_new http_serve
http_serve_shared http_server_free http_stream_finish http_stream_free http_stream_reject
http_stream_send http_stream_send_event http_timeout io_copy io_file_create io_file_free
io_file_len io_file_open io_file_pread io_file_pwrite io_reader_buffered io_reader_free
io_reader_open io_reader_read io_reader_read_line io_reader_stdin io_writer_create
io_writer_flush io_writer_free io_writer_std io_writer_write io_writer_write_builder
json_decode json_decode_array json_decode_scalar json_decode_soa json_decode_struct_array
json_decode_union json_doc_as_bool json_doc_as_f64 json_doc_as_i64 json_doc_as_str json_doc_at
json_doc_elems json_doc_get json_doc_key json_doc_kind json_doc_len json_doc_parse
json_encode_object json_encode_scalar_array json_encode_struct_array json_encode_union
json_scan_next len_mismatch_fail par_map par_map_filter par_map_reduce path_base path_dir
path_ext path_join path_normalize percent_decode percent_encode print print_bool print_char
print_f32 print_f64 print_str process_abort process_cpu_count process_exec process_exit
process_spawn range_fail regex_captures regex_captures_free regex_captures_group regex_compile
regex_find regex_find_all regex_free regex_group_count regex_group_index regex_is_match
regex_replace regex_split rng_next rng_range rng_sample rng_seed_os rng_seed_with rng_shuffle
run_output_code run_output_free run_output_stderr run_output_stdout str_clone str_cmp
str_contains str_ends_with str_eq str_eq_ignore_case str_find str_finder_find str_finder_free
str_finder_new str_rfind str_starts_with str_trim str_trim_end str_trim_start tcp_accept
tcp_conn_free tcp_conn_reader tcp_conn_writer tcp_connect tcp_listen tcp_listener_free
tcp_read_timeout tcp_write_timeout tg_alloc tg_begin tg_end tg_register tg_wait time_instant
time_now time_sleep udp_bind udp_recv_from udp_send_to udp_socket_free utf8_boundary_fail
utf8_valid
```

Every key emits `align_rt_<key>` except `print -> align_rt_print_i64`,
`cli_command -> align_rt_cli_command_new`, and
`http_request -> align_rt_http_request_new`. The main wrapper additionally declares
`align_rt_report_error` and, only for argv main, `align_rt_args_build`; neither has a MIR lookup
key. Eleven further always-built runtime exports are unkeyed base rows: the three existing
runtime-internal helpers and eight package-internal PostgreSQL codec helpers. Four `alloc-count` probes
and four distinct `par-map-probe` exports are verification-only runtime-fixture records; their
names remain ordinary program/extern/export spellings. `task-group-probe` adds no unmangled export. The
four AEAD cross-product symbols are ordinary keys
rather than a codegen-side string match.
[`20-runtime-abi-ledger.md`](20-runtime-abi-ledger.md) owns all 285 keyed symbol/type/attribute
records, the thirteen always-built unkeyed records, and the eight verification-only probe records.
The compiler registry is fixed at 298 base records with no feature or ambient input. The eight
probe rows extend only the verification-time maximum runtime-export table to 306; they are never a
RuntimeKey, callable declaration, collision reservation, or compatible-extern reuse target.
Probe-feature runtime builds never link user artifacts. Runtime feature selection affects only
export-set verification and changes no source acceptance or MIR/interface/artifact/cache identity.
Declaration, runtime lookup, base extern-compatible reuse, ABI goldens, link-identity collision
checks, and native-call body validation consume those fixed rows. Adding a native call without its
key or ABI row is a compile error rather than a drifting second list.

Generated identities are typed, injective, and deterministic. After collecting and validating all
requests, sort unique `GeneratedId` records by their canonical bytes. For each record, try the
printed stem below with `$0`, then `$1`, and so on, selecting the first emitted name absent from the
global registry. The checked increment rejects on integer exhaustion. This exact probe rule keeps
every program/extern/export spelling accepted instead of reserving `align_gen$...`; probe choice is
object identity but never ABI or interface identity.

The `ProgramCall hex` in a readable stem is exactly two lowercase hexadecimal digits per raw
logical-name UTF-8 byte; it excludes the canonical `u32` text-length prefix. Fallibility is the
single ASCII digit `0` or `1`; kernel mode is the single ASCII digit `0` through `3`; every
`complete structural-key hex` is two lowercase digits per complete canonical byte. The probe
counter is `u64`, rendered as canonical base-10 ASCII without leading zero. Probing begins at zero;
if candidate `$18446744073709551615` is occupied, the request fails with
`CodegenError::Lowering("generated name exhausted:<complete occupied candidate ASCII hex>")`
before LLVM construction. The owner injects a starting counter to test the
maximum/occupied case without iterating through earlier values.

| Generated family | Exact identity and cache dimensions | Owner evidence |
|---|---|---|
| named function-value thunk | stem `align_gen$fnval$<target ProgramCall hex>`; one per exact target used by `FnAddr`. Signature is target ABI prefixed by the environment pointer. | same runtime-key/generated-stem spelling as a user function remains a distinct positive; duplicate uses dedupe; unknown target and signature mismatch reject |
| lifted closure thunk | stem `align_gen$clos$<lifted ProgramCall hex>`; capture count/types, explicit signature, and lifted target must agree at every body occurrence before the single thunk is registered. | zero/many captures, repeated equal metadata, conflicting metadata, and target collision/probe twins |
| task trampoline | stem `align_gen$tramp$<fallibility>$<complete structural result-type hex>`; the type key includes widths, signs, nominal structural fingerprints, tagged payloads, and no raw table id. | every supported result/fallibility pair, formerly colliding unsupported `x` types, whole/per-unit identity |
| parallel kernel | stem `align_gen$par$<mode>$<complete structural-key hex>`. The key contains MIR/LLVM input and output types, terminal callable and exact signature, terminal capture types, every ordered stage kind/callable/signature/input/output/capture/path, field layout identity, and count/scatter mode. Cache lookup uses `GeneratedId`, never readable LLVM name. | existing structural-key matrix plus callable-name separator collisions, field-layout twins, count/scatter, generated-stem probe, and malformed-before-cache-reuse tests |

Generated symbols are private and do not enter interface or ABI fingerprints. Their deterministic
names affect only LLVM/object bytes and debug output. Program/native/generated registry construction,
all compatible duplicate reuse, and all collision checks finish before any function body or helper
CFG is emitted.

#### Declaration/header and total-body ledger

[`19-hir-validation-ledger.md`](19-hir-validation-ledger.md) is the authoritative per-record body
ledger. The inventory and PR ranges below are routing information only; they do not replace its
envelope order, child order, result/type relation, ownership correlation, or owner tests.

Am-h validates, in order, externs, imports, function-value header records, stored functions, then
locals:

- modes are parallel to parameters; extern modes are `ByValue`; stored/import modes are
  `ByValue` or `Out`, with `Out` only on `Slice`; `Borrow`/`BorrowMut` remain disabled;
- function-value `FnTy` records retain the graph-valid/type-placement contract already owned by
  am-g-t/am-p; am-h owns their mode cardinality/class and return-borrow/region summary shape;
- return borrow and region summaries are identical. `None` is canonical for no roots.
  `Roots { params, captures }` has a non-empty, strictly increasing, in-range `params` vector whose
  referenced parameter types are borrow-capable, and an empty `captures` vector before L2b-b;
- imported HIR carries `return_provenance_known: bool`. `true` means the producer received the
  validated external record and may trust an explicit `None`; `false` means the compatibility API
  omitted the record and replay must retain the all-compatible-input fallback. This is an am-b4
  replay field only and is stripped before MIR construction; it is not an interface or ABI field;
- every imported non-generic public function carries one required `effect: FnEffect` header fact.
  The semantic producer copies the exact `external_effects[canonical_name]` value when present and
  normalizes an absent compatibility-map entry to `Impure`, matching the current fail-closed
  analysis in which absence is both impure and unknown and impurity has diagnostic/classification
  precedence. Whole-program HIR has no imported declarations. This fact is internal checked-HIR
  transport, not a new interface field: the interface already stores the same three-valued effect.
  Am-h validates and preserves the field but does not infer a stored function body from it;

  ```text
  FnEffect = Pure | Impure | Unknown

  hir::ImportedFn {
    name: String,
    params: Vec<Ty>,
    param_modes: Vec<ParamMode>,
    ret: Ty,
    return_provenance_known: bool,
    return_borrow: ReturnBorrowSummary,
    return_region: ReturnRegionSummary,
    effect: FnEffect,
  }

  mir::ImportedFn {
    name: String,
    params: Vec<Ty>,
    param_modes: Vec<ParamMode>,
    ret: Ty,
    return_borrow: ReturnBorrowSummary,
    return_region: ReturnRegionSummary,
  }
  ```

  `return_provenance_known` and `effect` are not part of the Align call ABI or `CanonicalFnAbi`;
  they are validation-only HIR facts. The presence bit selects the producer-compatible provenance
  fallback, while `effect` is the imported semantic seed
  consumed by the existing effect fixed point. Am-h replaces MIR's current
  `Vec<hir::ImportedFn>` with `Vec<mir::ImportedFn>` in the exact existing field order and strips
  both validation-only fields only after validation. Both structs derive the same `ImportedFn { ... }` structural
  Debug bytes for those six fields, so validated per-unit codegen input has the same six-field
  imported-record identity and no new effect byte can enter MIR. This slice does not independently
  re-measure interface-summary bytes, `impl_hash`, link/cache keys, or cache hit behavior; those
  remain the existing interface/hash/cache owner tests' responsibility. The am-h owner test
  compares the validated MIR rendering and six-field imported record for Pure, Impure, and Unknown
  imports;
- `params` and `param_modes` have equal length. Each parameter id is unique, in range, and refers to
  the local with that id and the signature type. A source function or generic monomorph marks those
  locals `is_param: true`; a lifted function deliberately marks both explicit and trailing captured
  parameter locals `is_param: false`, because `is_param` records the source-level no-alias
  privilege rather than ABI parameter placement;
- am-h replaces the ambiguous pair `lifted_capture_count: Option<usize>` plus `exportable: bool`
  with one required origin record:

  ```text
  FnOrigin =
    Source { is_entry: bool, is_public: bool }
    | Monomorph
    | Lifted { capture_count: u32 }
  ```

  `Source` is used for every emitted non-generic source declaration. `is_entry` is the producer's
  compilation-unit role, and `is_public` is the declaration visibility even though an entry-unit
  declaration is never externally visible. `Monomorph` is used only for a concrete generic
  instantiation. It deliberately stores no template name or argument vector. A concrete
  `Call.type_args` record is correlated only through one shared compiler-owned
  `mangle_mono_suffix(type_args)` encoder (the current `mangle_mono("", type_args)` bytes), a
  non-empty base prefix, the target's `Monomorph` origin, and its concrete signature/result. Sema
  production and am-b validation call that same encoder; no second mangle grammar is permitted.
  The discarded template and its bounds are not reconstructible at the HIR boundary and are not
  claimed by am-h/am-b. `Lifted` is used only for a lambda helper,
  including `capture_count == 0`, and
  requires `capture_count as usize <= params.len()`, every mode `ByValue`, and both summaries
  `None`. The stored HIR no longer carries an independently forgeable `exportable` bit:
  `FnOrigin::is_exportable()` is true exactly for
  `Source { is_entry: false, is_public: true }`. Whole-program MIR still forces internal linkage;
  per-unit MIR copies only that derived value into its existing `mir::Fn.exportable` field;
- `main` first obeys the ordinary source-function header rules. A `Result` return, with or without
  argv, is exactly `Result<Unit, builtin Error>`. Only a
  `Source { is_entry: true, .. }` function whose exact logical name is `main` may be the main
  producer; `Monomorph` and `Lifted` never are. A main with parameters has exactly one
  `ByValue DynArray(Str)` parameter and that Result return. After am-e, a no-argument main returns
  exactly Unit, signed 32-bit `i32`, or that Result; every other graph-valid return has already
  received the source diagnostic and is not producer-valid checked HIR. Unit and Result bodies use
  an encoded internal Align identity plus the generated external
  `i32 @main()`/`i32 @main(i32,ptr)` wrapper; exact i32 uses external `i32 @main()` directly. The
  builtin error declaration is
  `Error { NotFound, Invalid, Denied, Timeout, Code(i32) }` with exact identity, order, payload,
  and field bases;
- local id equals ordinal; name is non-empty and NUL-free; a parameter name is an ASCII source
  identifier. Local alignment is `None` or a power of two in `1..=2^29`, and only a non-parameter
  fixed integer/float array may carry it;
- `drop_locals` and `drop_individual_locals` are sorted unique in-range sets and the latter is a
  subset. Exact membership and `drop_individual_exprs` remain body-derived am-b facts.

Am-b uses one explicit enter/exit worklist and a total `ExprKind` match. Am-b1, am-b2, and am-b3
merge dormant validator modules plus exhaustive direct unit tests; they do not call the public
lowering entrypoint and make no malformed-HIR promise. Am-b4 completes ownership correlation,
effect correlation, proves the total discriminator inventory, activates the assembled validator in
every entrypoint, and adds valid-HIR byte identity, canonical-empty rejection, whole/per-unit
parity, and cost evidence. Its final body pass also replays lexical local visibility and definite
initialization: parameters enter at function entry, initializers precede `Let`/`LetTuple` bindings,
match payloads are arm-local, and block/arm exit removes bindings. Every local read and mutable
place root must therefore be visible on that source path; a function-wide local table is not enough.
The same replay tracks non-discard visible names: duplicate parameters, same-scope rebinding, and
inner-scope shadowing reject, while a block or match-arm exit releases its names so disjoint siblings
may reuse them. Sema and the validator share these exact pure classification seams:

```text
tuple_drop_local_name(ordinal: usize) -> String
tuple_discard_needs_hidden_local(
  ty: Ty,
  structs: &[StructDef],
  enums: &[hir::EnumDef],
  tagged_types: &[hir::TaggedType],
) -> bool
```

The first returns the canonical compiler-reserved `$tuple_drop<ordinal>` bytes; `$` is outside the
source identifier grammar. The second is the exact producer predicate currently named
`is_owned_droppable`; producer and validator call it directly, rather than relying on
`needs_drop_flag` or an unstated scalar-domain equivalence. Only both shared predicates succeeding
at the matching `LetTuple` ordinal excludes a local from source-visible names. The local remains
id-visible and initialized until scope exit so the existing Drop analysis and lowering own cleanup.
Repeated owned discards and a preceding user `_drop0` are producer-valid positives. This is one
shared spelling/classification seam, not a new HIR field or discriminator, so the bounded fix remains
inside sema production and MIR validation without changing HIR/MIR shape, serialized schema, ABI,
generated-program/runtime allocation, or runtime cleanup. The helper's owned `String` result is one
compiler-owned temporary allocation per formation/comparison, and hidden-name HIR debug/value bytes
intentionally change from `_dropN` to `$tuple_dropN`. The existing
AM-B4 follow-up remains the smallest independently correct vertical because changing the producer
spelling without the validator would reject producer-valid HIR, while changing the validator alone
would preserve the ambiguity. `malformed_hir_visible_local_name_collisions_fail_closed` crosses parameter,
`Let`, `LetTuple`, nested-block, and match-payload collisions against sibling-block/arm positives in
all four lowering entrypoints, exact hidden-discard positives, and every near-spelling negative.

The hidden-name classification owner is the following closed product. For every name-visible cell,
no prior visible collision accepts and an otherwise identical prior collision rejects. A hidden cell
accepts either collision state for name purposes, while duplicate ids/bindings still reject through
their existing owners.

| Binding kind | Spelling at ordinal `i` | hidden-type predicate | Name classification |
|---|---|---:|---|
| `LetTuple` | exact `$tuple_drop<i>` | true | hidden |
| `LetTuple` | exact `$tuple_drop<i>` | false | visible |
| `LetTuple` | source `_drop<i>` | false/true | visible |
| `LetTuple` | leading-zero `$tuple_drop0<i>` | false/true | visible |
| `LetTuple` | canonical spelling for any wrong ordinal | false/true | visible |
| ordinary `Let` | canonical or any near spelling | false/true | visible |

The same owner separately proves the hidden-id lifecycle: a hidden local read in its initializer
rejects before activation; a direct validator fixture observes it initialized after binding; a read
after block exit rejects; producer HIR lists each hidden id exactly once in sorted `drop_locals` and
`drop_individual_locals`; and whole/per-unit plus located/non-located MIR each emit one scope-exit
`Drop` for each hidden string slot in the straight-line fixture. These cells prevent an implementation
from satisfying name tests by skipping hidden-local activation or cleanup.
Value joins use the structural body-type relation, including recursively matching fresh `FnTy`
cells rather than requiring ordinal identity. The checker-owned `Break.accepted` fact is replayed in
both directions: it is true exactly for an innermost loop target at the same arena/task depth, while
targetless and nested-region recovery breaks remain false. The owner matrix pairs a forged
same-region rejection with accepted and rejected arena/task crossings so neither truth direction can
silently drift. Effect correlation replays the existing source-order effect fixed point
using stored imported effects as the only cross-unit seeds, implicit `Impure` for every extern, and
body-derived effects for stored source/monomorph/lifted functions. It requires every concrete
function-value `FnTy.effect` cell, aggregate projection/join cell, result boundary, and parallel
callable eligibility decision to equal that replay. Annotation-only unused `FnTy` entries remain
`Unknown`. A forged `Pure` imported seed is no more independently authenticatable at this internal
boundary than a forged imported signature, but every body fact derived from the seed must be
internally consistent. Activation is all-or-nothing; no partially validated expression family is
exposed.

Am-b1 owns every `Stmt`, local/place and `ExprKind::Unit` through
`ExprKind::BuilderToString` in declaration order, plus `BuilderWriteKind`, `StrPredKind`, and
`StrTrimKind`. The am-b2 range is delivered as contiguous, reviewable verticals on the same
dormant validator state machine: am-b2a owns `ExprKind::ArrayLit` through `ExprKind::VecLit`;
am-b2b1 owns `ExprKind::ArraySum` through `ExprKind::ElemField` plus every `StageKind`; and
am-b2b2 owns `ExprKind::Template` through `ExprKind::ArrayDictEncode` plus every `TemplatePart`,
`GroupSource`, `GroupAgg1`, and `GroupOp`. Am-b3 owns
`ExprKind::FsReadFile` through `ExprKind::CryptoArgon2` in declaration order, plus `AeadCipher`,
`AeadDir`, `HashAlgo`, `CliFlagKind`, `EncodingKind`, `CompressKind`, and `PathComponentKind`.
Those three closed ranges assign every discriminator to exactly one construction PR. The exact
`ExprKind` inventory is:

```text
Unit Int Float Char Str Bool Local Unary Cast Binary IntArith MathOp FnValue Closure
CallFnValue TaskGroup EnumValue Match ResultMapErr Spawn TaskGet Wait Call If StructLit Field
SoaColumn Tuple TupleIndex IndexField Block OptionSome OptionNone ElseUnwrap ResultOk ResultErr
Try Loop Arena Unsafe RawAlloc RawFree RawLoad RawStore RawOffset HeapNew BoxGet BoxClone
StrClone StrPredicate StrTrim StrBorrow BuilderNew BuilderWrite BuilderToString ArrayLit
ConstArray ArrayZip Select VecSumWhere VecDot VecMinMax VecSum VecLoad VecStore VecLit
ArraySum ArrayCount ArrayAnyAll ArrayMinMax ArrayReduce ArrayScan ArrayDot ArraySort ArraySortBy
ArrayToArray ArrayToSoa ArrayMapInto ArrayPartition ArrayParMap ArrayChunks ArrayToSlice Len Index
SliceRange ElemField Template JsonDecode JsonDecodeArray JsonDecodeScalar JsonDecodeStructArray
JsonDecodeSoa JsonDecodeUnion JsonDoc JsonDocKind JsonDocGet JsonDocAt JsonDocAsStr
JsonDocAsScalar JsonDocLen JsonDocKey JsonDocElems JsonScan ArrayGroupAgg ArrayGroupAggMulti
ArrayDictEncode FsReadFile ReaderStdin ReaderOpen WriterStd WriterCreate ReaderRead
ReaderBuffered ReaderReadLine BytesAsStr WriterWrite WriterFlush IoCopy FileCreateRw FileOpenRw
FilePread FilePwrite FileLen BufferNew BufferBytes StrBytes BufferLen BytesRead BufferPut
BufferAppend ArrayBuilderNew ArrayBuilderPush ArrayBuilderAppend ArrayBuilderBuild FsWriteFile
FsExists FsRemove FsReadDir DnsResolve TcpConnect ConnReader ConnWriter TcpReadTimeout
TcpWriteTimeout TcpListen TcpAccept UdpBind UdpSendTo UdpRecvFrom FsReadFileView
FsReadBytesView PathJoin PathComponent PathNormalize EnvGet EnvSet TimeNow TimeInstant
ProcessCpuCount TimeSleep ProcessExit ProcessAbort ProcessSpawn ChildWait ChildKill ProcessExec
ProcessCommand CommandCwd CommandTimeout CommandEnv CommandEnvClear CommandRun RunOutputCode
RunOutputStdout RunOutputStderr EncodingEncode EncodingDecode Utf8Valid Compress Decompress
RandSeed RandSeedWith RandNext RandRange RandShuffle RandSample RegexCompile RegexIsMatch
RegexFind RegexFindAll RegexSplit RegexReplace RegexCaptures RegexGroupCount RegexGroupIndex
CapturesGroup CliCommand CliFlag CliParse CliGetBool CliGetI64 CliGetStr CliUsage HttpRequest
HttpHeader HttpBody HttpRequestTimeout HttpParse HttpRespStatus HttpRespHeader HttpRespBody
HttpClient HttpClientTimeout HttpClientGet HttpClientPost HttpClientRequest HttpGetMany HttpServe
HttpAccept HttpCtxMethod HttpCtxPath HttpCtxHeaders HttpCtxHeader HttpCtxBody HttpResponseBuilder
HttpRbHeader HttpRbBody HttpRespond HttpRespondStream HttpStreamSend HttpStreamFinish
HttpStreamReject CryptoCtEqual CryptoRandom CryptoHash CryptoHmac CryptoHkdf CryptoAead
CryptoArgon2
```

The same exhaustiveness assertion covers `Stmt::{Let, LetTuple, Assign, AssignIndex,
AssignVecLane, AssignField, AssignElemField, AssignElem, Return, Break, Expr}`,
`StageKind::{Map, Where, WhereField, WhereStrContains, Project}`, every `TemplatePart`,
`AeadCipher`, `AeadDir`, `HashAlgo`, `CliFlagKind`, `EncodingKind`, `CompressKind`,
`PathComponentKind`, `GroupSource`, `GroupOp`, `BuilderWriteKind`, `StrPredKind`, and
`StrTrimKind`. For each discriminator the validator checks its non-child envelope fields in its
ledger order, then child expressions left-to-right, then exact result and operand
types, every relational id/path/ordinal/arity/mode/capture, operation-specific wrapper/ABI record, and
body-derived ownership bit before advancing. One valid producer twin and one mutation of every
stored field are owner tests; multi-invalid precedence follows the universal order above.

#### Fixed implementation split, acceptance, and public effects

| Slice | Independently complete closure | Focused owner and benchmark |
|---|---|---|
| L2b-a2-am-d | Fix the conservative checked-HIR producer ceiling at 259, convert every body semantic replay and MIR-lowering consumer to explicit bounded frames, and expose one common explicit-worklist traversal for the finite but depth-unbounded type DAG already accepted by am-g-t. | `checked_hir_depth_closure_matrix`, 258/259/260 whole/per-unit MIR/LLVM owners, and deep valid/malformed type-DAG twins; no runtime benchmark |
| L2b-a2-am-e | Restrict the source/checker entry producer to Unit, exact i32, or `Result<Unit,builtin Error>` and make every admitted form's C ABI/link identity exact in whole/per-unit and ThinLTO paths. | `main_signature_matrix`, whole/per-unit Unit/i32/Result exit-code and LLVM-signature owners; no benchmark row |
| L2b-a2-am-f | Reject bare return and reachable absent body tail in every non-Unit function while preserving typed tail/return and proven non-fallthrough paths. | `function_return_completeness_matrix`, whole/per-unit MIR/LLVM verifier owners; no benchmark row |
| L2b-a2-am-w | Replace traversal-order task wait state with group/epoch-scoped outcome-sensitive successful-wait dominance across Spawn, transparent Result local/copy/reassignment/`map_err` flow, transparent Move Task-handle origin flow, Try, Match, Else, nested groups, and every control join. | The existing `crates/align_driver/tests/task_group.rs` matrix owns the shipped successful-wait/task-get behavior; the sema owners below add stable identity, malformed replay, depth, fixed-point, and unresolved-loop coverage. |
| L2b-a2-am-v | Require an exact bound `mut Buffer` local at the five native output positions without applying source `mut` to native objects that mutate only interior runtime state. | `native_output_buffer_requires_mut_local`, five runtime/allocation twins, whole/per-unit parity; no benchmark row |
| L2b-a2-am-u | Reject extern `FnValue`; require lexical `unsafe` at direct extern Call and every named extern pipeline/reducer/sort invocation, with inference-only signature peeks remaining silent. | `extern_invocation_permission_matrix`, direct/callback/FnValue whole/per-unit owners; no benchmark row |
| L2b-a2-am-p | Activate all body-independent placement predicates; invalid placement is canonical-empty while graph-valid but position-invalid twins distinguish every producer set. | `malformed_hir_type_placement_fails_closed`, `valid_hir_type_placement_preflight_is_mir_identity`, deep valid placement and deep malformed later-sibling precedence; `mir-type-placement-validation` |
| L2b-a2-am-n | Activate exact nominal/source-shape, tuple, alignment, enum-base, and link validation. | `malformed_hir_nominal_link_metadata_fails_closed`, shallow/deep equal-shape origin twins, and deep malformed later-sibling precedence; `mir-nominal-link-validation` |
| L2b-a2-am-h | Activate extern/import/stored/main/local body-independent header validation, including normalized imported-effect transport. | `malformed_hir_declaration_header_metadata_fails_closed`, `valid_hir_declaration_header_preflight_is_mir_identity`, imported Pure/Impure/Unknown/absent-normalization twins, and deep signature/summary valid/malformed twins; `mir-header-validation` |
| L2b-a2-am-b1 | Dormant total-validator core: every statement, local/place, ordinary expression/call/aggregate, tagged value, and structured-control record. | direct table-driven unit owner plus deep valid/malformed ordinary-body type relation; no public benchmark row yet |
| L2b-a2-am-b2 | Dormant storage/vector/array/pipeline/template/JSON validator, including every stage, terminal, capture, and descriptor record. | direct table-driven unit owner plus deep valid/malformed storage/pipeline type relation; no public benchmark row yet |
| L2b-a2-am-b3 | Dormant native/runtime validator plus generated-callable body facts for all I/O through crypto families. | direct table-driven unit owner plus deep valid/malformed native/generated signature relation; no public benchmark row yet |
| L2b-a2-am-b4 | Add body-derived Drop/ownership and effect correlation, assert the full inventory, activate all body validation globally, replay lexical visibility/definite initialization, use structural value joins for fresh callable cells, and prove valid identity/four-entrypoint parity. | `malformed_hir_body_metadata_fails_closed`, `checked_hir_body_fact_replay_covers_cleanup_and_function_effects`, `valid_hir_body_preflight_is_mir_identity`, `body_contract_function_return_none`, `body_contract_function_root_completion`, `hir_body_validator_accepts_structural_function_value_match_join`, `hir_body_validator_rejects_out_of_scope_local_use`, `checked_hir_depth_closure_matrix`, `deep_hir_body_core_type_dag_is_stack_bounded`, whole/per-unit codegen; `mir-body-validation` |
| L2b-a2-am-c1 | Land the fixed typed runtime ABI registry and migrate declarations, attributes, export verification, compatible native extern reuse, and every dedicated native LLVM consumer without activating typed MIR calls or changing symbols. | 281-key/286-base-row bidirectional ABI/export goldens, each probe-feature export set, every dedicated native consumer, compatible/incompatible extern rows; unchanged runtime-call cost |
| L2b-a2-am-c2a1 | Land `FunctionTypeDef`, the private semantic error identity, the one shared `pub(super)` node identity relocated from `validate_hir`, checked count/text/width/parameter-mode helpers, and the one primitive/scalar/root field encoder. No view or graph traversal lands. | every root/scalar/primitive tag, payload, width/lane, mode/error mapping, checked-count boundary, and unchanged HIR validation parity; no public runtime benchmark row |
| L2b-a2-am-c2a2a | Atomically move the existing private am-n comparator behind the exact borrowed `SourceShapeView`. Preserve the HIR caller, results, order, stack behavior, cache/restart, bijection, and collection classes exactly; add no observer, complexity collector/claim, benchmark, canonical graph view, or byte path. | unchanged am-n suite plus wildcard-free 57-`Ty`/34-`Scalar`, five-node/minimal-view parity, cache-extension, and collection-inventory owners |
| L2b-a2-am-c2a2b | Add only the zero-sized production observer seam and exact sequence-wide complexity/topology evidence around c2a2a's unchanged comparator. | exact `V/E/P/Q`, alias-fanout/degree/unique/shared-depth, sharing/cycle/cache/restart, production-zero-observer inventory, and compiler-only `canonical-source-shape-comparison` benchmark |
| L2b-a2-am-c2a3 | Land the private borrowed `CanonicalTypeView`, five-node encoder, and sole `ValidatedGraph::new(root, view)` raw traversal. It assigns every reachable field an encounter ordinal, collects rather than prematurely returns realizable semantic failures, then attaches exact tuple/nominal failures to the second node's end ordinal and selects the lowest `(ordinal, tie-rank)`. A referenced descendant or later sibling therefore cannot outrank the completed second serialized node. After mandatory c2a2b, it calls c2a2a's cached sharing-preserving bijective comparator and retains no raw bytes. A direct fixture passes transient `fn_types`. | every node/member/reference/am-n invariant, exact cross-node/later-sibling/referenced-descendant precedence, unreachable isolation, linear raw-scan/stack bounds, and direct Fn root; no public runtime benchmark row |
| L2b-a2-am-c2a4 | Accept only `&ValidatedGraph` and reuse c2a1/c2a3 encoders/order to add greatest-fixed-point tuple/tagged/function equivalence and DFS-first canonical semantic bytes. It adds no second validator, raw view/root entry, or traversal authority. | semantic→bytes goldens, direct recursive Fn reference, anonymous/nominal equivalence/non-equivalence, topology/declaration permutations, refinement-round bound, and deep canonical traversal; `bench/library_boundary/run.sh provenance`: compiler-only `canonical-type-graph` |
| L2b-a2-am-c2b | Retain the compact effect-free MIR function-type table, use only c2a4 canonical bytes/equivalence to remove/sort definitions, and remap every raw `Ty::Fn`/`Scalar::Fn` occurrence. | definition-equivalence/remap owners, table compactness, every type-bearing field, whole/per-unit MIR/hash parity; no public runtime benchmark row |
| L2b-a2-am-c2c | Expose `ProgramCall`, `CanonicalTy`, `CanonicalFnAbi`, `ProgramCallError`, and `CanonicalCodecError`, plus the complete independent canonical decoder over c2a4. `from_program`/`from_parts` construct the private view from c2b's already-canonical `Program` tables, including `Program.fn_types`; they do not rebuild or effect-strip a second function graph. No generated identity type lands. | independent bytes→semantic type/Fn goldens, `from_program` and `from_parts` Fn-root positives, every malformed field and exact error-precedence row, deep truncation owners, proof that no LLVM/symbol/runtime path consumes the dormant wrappers; no public runtime benchmark row |
| L2b-a2-am-c2d | Expose `GeneratedId` and parallel identity records plus record-local encode/decode over c2c. No collection validator, name reservation, MIR field, or codegen consumer lands. | generated-family semantic↔byte goldens, every record-local malformed/error row, deep nested decode, proof that no LLVM/symbol/runtime path consumes the dormant records; no public runtime benchmark row |
| L2b-a2-am-c3 | Land typed program declarations/call targets, encoded program symbols, generated identities, collection pairing/probing, and all name/collision validation atomically across MIR/codegen and whole/per-unit paths. | callable applicability/unavailable cells, external collision/precedence and golden-symbol suites, generated family/pair/probe owners, per-unit/ThinLTO link parity; `mir-callable-namespace-validation` plus unchanged continuation cost |

#### Am-b2a implementation closure matrix

This matrix is authoritative before the first am-b2 vertical is implemented. Am-b2a extends the
same crate-private `body_core_metadata_is_valid(&hir::Program) -> bool` boundary and the same
`BodyValidator` explicit worklist introduced by am-b1; it adds no public entrypoint, MIR shape,
interface byte, ABI, cache, or runtime behavior. The slice owns `ExprKind::ArrayLit` through
`ExprKind::VecLit` in the closed ledger order: fixed/constant literals and zip sources plus vector
records. Pipeline terminals, array materialization/views, length/index/range, struct-array element
fields, templates, JSON, group aggregation, and dictionary encoding remain rejected by the dormant
boundary until am-b2b.
This cell reuses one worklist/flow table and one source-order validator across all storage families.
It stays atomic inside the canonical callable capability wave because splitting by array family
would duplicate the source-flow and stored-result gate or publish a parallel validator path. Line
count alone neither forces nor forbids a wave split.

| Cell | Required am-b2a closure | Exact owner evidence |
|---|---|---|
| entry and shared state | Reuse the am-b1 entry, immutable program reference, function/lexical context, pointer-keyed flow tables, and child-first enter/exit worklist. Add no second validator entry and do not activate the public HIR preflight. | `hir_body_validator_storage_vector_array` calls the same dormant helper; existing am-b1 valid/malformed and identity owners remain unchanged. |
| universal order and unsupported boundary | For every new expression, validate the variant envelope and span, then children in ledger order, then stored ids/fields/types and relational postconditions. Retained children after a non-fallthrough child remain structurally checked. Any `Template`-through-`ArrayDictEncode` expression remains fail-closed in b2a and cannot contribute a flow join. | The expression inventory owner mutates one envelope, child, relational, and stored-result field for each b2a family; a b2b sentinel proves the dormant boundary is not accidentally activated. |
| literals and zip | Validate `ArrayLit`, `ConstArray`, and `ArrayZip` lengths, element discriminators, fixed struct/primitive placement, pooled flag shape, tuple ids/arity, Copy-source constraints, and exact child element types. Reject owned handles, slice-bearing non-struct elements, Move enums, invalid lengths, and unequal zip source contracts. | `hir_body_validator_storage_vector_array` has valid primitive/struct/constant/zip fixtures and one-field mutations for element, length, pooled, tuple, source, and stored-result relations. |
| vector records | Validate `Select`, `VecSumWhere`, `VecDot`, `VecMinMax`, `VecSum`, `VecLoad`, `VecStore`, and `VecLit` lane counts, numeric scalar domains, mask/value pairing, index type, writable slice place, exact vector/result types, and operand order. | The same owner has positive and negative lane/scalar/mask/index/place/result twins; no vector operation reads deferred ownership/effect facts. |
| deferred b2b boundary | Keep every `ArraySum` through `ArrayDictEncode` expression, every `StageKind`, `TemplatePart`, `GroupSource`, `GroupAgg1`, and `GroupOp` fail-closed in b2a. The b2a helper must still structurally visit retained children only when they are reached through an owned b2a record; it must not read b2b allocation, region, alias, purity, JSON, or grouping facts. | `hir_body_validator_storage_vector_array_deferred_b2b` mutates one sentinel b2b discriminator and proves the dormant helper remains false; b2b owns the later positive and malformed matrix. |
| flow and evaluation order | Propagate strict-child fallthrough and accepted loop breaks through every new record, preserve source-order capture/operand evaluation, and keep stored-result polymorphism only for a genuinely non-fallthrough expression. No allocation, Drop, region, borrow, effect, or alias fact is inferred here. | `hir_body_validator_storage_vector_array_control_flow` covers a terminating child followed by malformed retained syntax, branch/loop joins, and exact stored results; the existing deferred-facts owner remains green. |
| type graph and malformed safety | Reuse the iterative type/name/mangle helpers and fail closed on every invalid id, scalar width, lane count, ordinal, path, length, and enum/struct reference without indexing or shifting from unchecked input. No new recursion over HIR/type graphs is introduced. | `deep_hir_body_storage_type_dag_is_stack_bounded` runs the direct helper on a 2 MiB stack for valid and malformed later-sibling graphs; focused malformed mutations assert `false` rather than panic. |
| ownership and activation | Do not read or mutate `drop_*`, `Assign` cleanup cells, region/borrow/escape state, allocation flags, effects, task proofs, or runtime/native objects. Valid b2a HIR remains dormant and public lowering remains byte-identical; b2b and b4 own the remaining records and activation. | `hir_body_validator_storage_vector_array_deferred_facts_are_not_consumed`, existing four-entrypoint identity owners, and repository search for the helper caller. |
| source propagation and review gate | Keep this matrix, the am-b2a ledger range, the implementation test table, and `HANDOFF.md` synchronized. This is one storage/vector/array closure cell inside the canonical callable wave and must not claim b2b, b3, b4, or public activation early. | Fresh independent matrix review before coding; final wave matrix-to-diff pass; focused owners, `scripts/test-pr.sh`, applicable Clippy, and the single stable-candidate preflight review/attestation. |

#### Am-b2b implementation closure matrix

This matrix is authoritative before the b2b implementation begins. Am-b2b1 and am-b2b2 reuse the
same crate-private `body_core_metadata_is_valid(&hir::Program) -> bool` boundary and the same
explicit enter/exit worklist. Neither slice activates public HIR validation generally; Request 6's
scanner predicate is the named narrow pre-lowering exception. Apart from that predicate, the
slices do not consume Drop, effect, allocation, region, borrow, escape, alias, JSON, or grouping
facts. Am-b2b1 owns the
closed declaration range `ExprKind::ArraySum` through `ExprKind::ElemField` and every `StageKind`;
am-b2b2 owns `Template` through `ArrayDictEncode` and every template/JSON/group nested record.
The b2b1 checkpoint deliberately leaves b2b2 records fail-closed so pipeline validation cannot
silently depend on a later JSON or grouping contract.

| Cell | Required am-b2b1 closure | Exact owner evidence |
|---|---|---|
| entry and shared state | Reuse the b2a body validator entry, function/lexical context, pointer-keyed flow tables, and child-first worklist. Add only source/stage context helpers; do not add a second validator or activate a public caller. | `hir_body_validator_pipeline_array_views` calls the same dormant helper; existing b1/b2a owners and four-entrypoint identity owners remain unchanged. |
| source and universal order | Validate each pipeline envelope before source, stages, explicit terminal arguments, and captures in ledger order. Source element type is threaded through every stage; every stored `Stage.out_ty` is checked against the next input and terminal result. Retained children after a non-fallthrough child remain structurally checked. | Positive source/stage/terminal fixtures plus one envelope, child, stage, capture, and stored-result mutation; `hir_body_validator_pipeline_control_flow` covers retained malformed syntax and joins. |
| stage records | Validate `Map`, `Where`, `WhereField`, `WhereStrContains`, and `Project`; resolve named signatures, capture arity/types, parameter modes, return types, field ordinals, bool predicates, and source-order needle children. Reject extern/borrow callback shapes and Move element/result/capture contracts where the producer forbids them. | `hir_body_validator_pipeline_stage_records` has one valid twin and one mutation per stage discriminator, callable field, capture, field ordinal, out type, and needle. |
| fused terminals | Validate `ArraySum`, `ArrayCount`, `ArrayAnyAll`, `ArrayMinMax`, `ArrayReduce`, `ArrayScan`, `ArrayDot`, `ArraySort`, `ArraySortBy`, `ArrayToArray`, `ArrayMapInto`, `ArrayPartition`, and `ArrayParMap` exact source/stage/callable/init/destination/result relations. Scanner sources remain deferred to b2b2 JSON ownership; b2b1 accepts only non-scanner supported sources. `ArrayParMap` callable signature/capture/result facts are structural here; complete reachable `Pure` is an am-b4 producer fact and is deliberately not consumed at b2b1. | `hir_body_validator_pipeline_terminals` covers positive reductions/materializers and one-field mutations for function names, captures, init/destination, element/key types, stages, and result tuple/dynamic-array types, including an explicit `JsonScanner` rejection twin and the deferred-Pure `ArrayParMap` positive. |
| views and chunks | Validate `ArrayToSoa`, `ArrayChunks`, `ArrayToSlice`, `Len`, `Index`, `SliceRange`, and `ElemField` source type, index/range operands, fixed-length/source-place restrictions, struct field paths, exact result/view type, and writable/arena-independent structural relations. | `hir_body_validator_array_views` covers fixed/dynamic/slice/chunk/struct/SoA positives and malformed source, path, index, scalar, length, and result twins. |
| deferred b2b2 boundary | Keep `Template` through `ArrayDictEncode`, all `TemplatePart`, `GroupSource`, `GroupAgg1`, and `GroupOp` records fail-closed. Do not read JSON descriptor, template ownership, grouping source, dictionary, or nested-record facts in b2b1. | `hir_body_validator_pipeline_deferred_b2b2` enumerates every deferred expression and varies each nested discriminator/field state one at a time while proving rejection; b2b2 owns all later positive and malformed matrices. |
| control flow and malformed safety | Propagate strict-child fallthrough and loop breaks through every b2b1 record; preserve source-order evaluation and fail closed on invalid IDs, widths, lanes, lengths, tuple/callable arity, modes, fields, and paths without unchecked indexing or new recursive HIR/type traversal. | Branch/loop/retained-child owner plus `deep_hir_body_pipeline_type_dag_is_stack_bounded`; focused malformed mutations assert `false` rather than panic. |
| ownership and activation | Do not inspect or mutate Drop sets, cleanup cells, region/borrow/escape facts, effects (including `Pure` for `ArrayParMap`), task proofs, allocation identity, runtime/native state, or public lowerers. Valid b2b1 HIR remains dormant and b2b2/b3/b4 own the remaining records and activation. | `hir_body_validator_pipeline_deferred_facts_are_not_consumed`, a valid `ArrayParMap` whose effect cell is mutated without changing the dormant verdict, existing identity owners, and repository search for the helper caller. |
| source propagation and review gate | Keep this matrix, the am-b2 ledger range, owner test names, and `HANDOFF.md` synchronized. The b2b1 cell must not claim b2b2, b3, b4, public activation, or JSON/group completion early. | Fresh independent matrix review before coding; final wave matrix-to-diff pass; focused owners, applicable Clippy, and the single stable-candidate preflight review/attestation. |

#### Am-b2b2 implementation closure matrix

This matrix is authoritative for the second b2b vertical. Am-b2b2 reuses the same dormant
`body_core_metadata_is_valid(&hir::Program) -> bool` entry, `BodyValidator` tables, and explicit
child-first worklist already closed by b2b1. It adds no second public caller and no general body
activation. Request 6's scanner predicate is the explicitly named narrow exception: the existing
  active `align_mir::hir_program_is_valid` gate invokes
  `validate_hir::json_scan_copy_rows_are_valid` before lowering. `JsonScan` is the explicit exception
  to the universal stored-field-before-`Span` rule. Its active scanner order is `Expr.span`, exact
  `Expr.ty`, existing row id, `input.ty == Ty::Str`, Decode schema, and canonical recursive Copy;
  the reason-valued `json_scan_validation_reason` seam makes the winner testable while the production
  boolean gate remains unchanged. Request 6 covers ordinary generic calls only when the
scanner row is concrete before call checking; an unresolved `json.scanner<Row<T>>` type argument
remains deferred to a separate Align prerequisite and keeps the existing resolver diagnostic.
The slice consumes no Drop, effect, allocation, region, borrow, escape, alias, or runtime/native
fact. The closed range is
`ExprKind::Template` through `ExprKind::ArrayDictEncode`, plus every `TemplatePart`,
`GroupSource`, `GroupAgg1`, and `GroupOp` discriminator. b3 remains the owner of the following
native/runtime records, and b4 remains the owner of activation and body-derived ownership/effect
replay.

| Cell | Required am-b2b2 closure | Exact owner evidence |
|---|---|---|
| Request 6 active scanner refinement | The active exception is `align_mir::hir_program_is_valid` calling `validate_hir::json_scan_copy_rows_are_valid` at all four MIR lowering entrypoints. `JsonScan` explicitly overrides the general stored-field-before-`Span` rule: validate `Expr.span`, exact `Expr.ty == Ty::JsonScanner(struct_id)`, existing row id, `input.ty == Ty::Str`, Decode schema, and canonical recursive Copy. The crate-private reason-valued `validate_hir::json_scan_validation_reason` returns `InvalidSpan`, `StoredType`, `UnknownRow`, `InputType`, `Schema`, or `Copy` for the first winner; production lowering consumes `.is_ok()` through the boolean gate. A precedence matrix covers malformed span against stored type, row id, input, schema, and Copy; a malformed span wins all five, while a valid span uses that listed order. Request 6 supports ordinary generic calls only when the scanner row is concrete before call checking; a concrete expected scanner return seeds bare substitution before arguments, substitutes bound parameters into each argument's expected type, arguments unify in source order, wrapper and multi-argument propagation are covered, numeric `IntVar`/`FloatVar` use existing `i64`/`f64` defaults, and the concrete instantiation reruns schema/Copy. An unresolved `json.scanner<Row<T>>` remains a separate Align prerequisite; for `Row<T>` the exact existing resolver diagnostic is `instantiating a generic struct with a type parameter ('Row<…>' inside a generic function) is not supported yet`. Missing context retains `cannot infer the scan row type; annotate the binding, e.g. \`rows: json.scanner<Row> := json.scan(d)\``; unresolved bare slots retain `cannot infer type parameter ...; annotate the call's context`, and conflicts retain the first existing `type mismatch: ...` in deterministic expected-context/argument order. No `ExprKind::JsonScan` HIR node or artifact is published after any failed step. | `hir_program_json_scan_envelope_mismatch`, `hir_program_json_scan_envelope_precedence_matrix`, `m5::json_scan_generic_return_context_ownership`, `m5::json_scan_generic_return_context_wrapper_matrix`, `m5::json_scan_generic_return_context_argument_order_matrix`, `m5::json_scan_generic_return_context_numeric_default`, `m5::json_scan_generic_return_context_inference_matrix`, `modules::json_scan_imported_generic_return_context_ownership`, `cache_codegen::json_scan_per_unit_interface_row_ownership`, `cache_codegen::json_scan_generic_return_context_no_publication`, and `m5::json_scan_copy_composite_runtime_matrix`. The implementation-time `json_scan_cross_compiler_identity` evidence is replayed by `scripts/compare-json-scan-identity.sh` from fixed baseline `576e57307fe4ef34e74566f5e389a2f0e2a04acd` and implementation `aa5bb7d66d0436c2d9ebf89f252b0ba5d528c2a8`; it is historical evidence, not a current-tree test. The pinned owner compares serialized interface and `InterfaceSummary.interface_hash` separately from the complete actual `CodegenKey` fields, plus complete codegen-input MIR, raw LLVM, and release baseline object bytes with `cmp` and no normalization; the expected `compiler_build_id` difference is recorded as `FirstDiff::CompilerBuildId` and no cache object is shared across compiler builds. |
| entry and shared state | Reuse the b2b1 worklist and pointer-keyed flow tables. Add only b2b2 envelope, child, descriptor, document, and grouping helpers. No second body validator or new public preflight caller, MIR action, allocation, or native registration is permitted; the existing active gate may invoke the pure Request 6 scanner predicate. | `hir_body_validator_pipeline_template_json_group` and `hir_body_validator_pipeline_template_json_group_control_flow` invoke the dormant helper; `hir_program_json_scan_copy_row`, `hir_program_json_scan_envelope_mismatch`, and `hir_program_json_scan_envelope_precedence_matrix` exercise the existing active gate; existing b1/b2a/b2b1 owners and four-entrypoint identity owners remain unchanged. |
| universal order and deferred facts | Validate every b2b2 envelope and span before children, visit retained children in written/source order, then apply the exact stored result and relational rule. A terminating child cannot authorize later template/group work. For active Request 6 `JsonScan`, the explicit exception order is `Span`, stored type, existing row id, input type, schema, and Copy; the reason-valued seam owns invalid span versus each later error. No b2b2 row reads ownership, effect, region, alias, task, allocation, or runtime facts except the `JsonScan` row's explicitly named canonical Copy/DropPlan precondition. | `hir_body_validator_pipeline_template_json_group` covers positive template/JSON/group fixtures and envelope/child/discriminator/descriptor/stored-result mutations; `hir_body_validator_pipeline_template_json_group_control_flow` covers a retained malformed child after a diverging template hole; deferred-facts mutation remains verdict-invariant; `hir_program_json_scan_envelope_mismatch` and `hir_program_json_scan_envelope_precedence_matrix` own the active envelope order and reason values. |
| template and nested parts | `Template` is non-empty, each `TemplatePart` is exhaustively checked, `Text` is valid stored text, holes are printable, `JsonStr` is `Str`, option fields have the exact Option payload and NUL-free names, nested/array/union parts have exact descriptor ids and access types, and `PopComma` is admitted only by the producer form: an active object opened by an exact `Text("{")`, at least one option part in that object since its last `PopComma`, then one `PopComma` before the matching exact `Text("}")`; nested object state is independent. Result is exactly `Str`; part order and strict child flow are preserved. TemplatePart has no span, so its envelope errors are attributed to the enclosing `Expr.span`; only the parent span is checked. | The owner covers every `TemplatePart` variant, positive mixed/nested templates, printable-type and field/name/id/access mutations, empty/template-result twins, malformed PopComma position/nesting twins, and a terminating retained-part twin. |
| JSON decode and scanner | Keep separate iterative descriptor walks for Decode and Encode. Decode admits only the sema Decode field shapes; Encode additionally admits `Option<enum>` and is used only by template parts. Require the unique builtin `Error` result identity. Validate flat/nested JSON struct descriptors and shape-directed union descriptors with no missing ids, unsupported fields, self-cycle, duplicate union shape class, multi-payload/tag-only union, or malformed scalar domain. `JsonDecodeArray` is primitive int/float/bool; `JsonDecodeScalar` is int/float/bool; `JsonDecodeStructArray` uses the Decode descriptor; `JsonDecodeSoa` is non-empty primitive/char/str-field SoA and requires active arena; `JsonScan` uses the Decode descriptor **and the canonical recursive Copy/DropPlan predicate over the complete row graph**, yielding the exact `JsonScanner(row)` from `Str`. The source sema gate emits the public diagnostic before HIR construction; for imported/per-unit consumers, interface/import reconstruction first materializes checked HIR and the active `align_mir::hir_program_is_valid` pre-lowering gate then rechecks the row graph fail-closed before MIR/runtime lowering, without reconstructing source spelling. The dormant b2b2 body validator may share pure helpers but is not the safety gate. A scanner is a pipeline source only: b2b2 extends b2b1's source helper so five HIR expression variants (`ArraySum`, `ArrayCount`, `ArrayReduce`, `ArrayAnyAll`, `ArrayMinMax`) expose all seven public methods (`sum`, `count`, `reduce`, `any`, `all`, `min`, `max`), each with the exact `Result<scalar, Error>` terminal type, while every materializing/non-streaming terminal rejects it. | One positive owner row per decode/scanner variant, separate Encode/Decode nested valid and malformed descriptor DAGs, every scalar/struct/union discriminator mutation, duplicate/missing/error-id twins, arena/no-arena SoA twins, scanner source/result/input mutations, direct/transitive Move-row negatives, imported/generic Copy/Move rows, checked-HIR Copy/Move mutation twins, all five accepted scanner HIR variants and all seven public methods, and one negative for every rejected terminal, plus the `Span`/type/row-id/input/schema/Copy reason-valued precedence matrix. The `hir_program_json_scan_copy_row` test must exercise the active pre-lowering route, not only `body_core_metadata_is_valid`. |
| JSON document views | `JsonDoc` requires `Result<JsonDoc, builtin Error>`, `Str` input, and active arena. `JsonDocKind` requires the exact builtin `Enum(json.kind)` result; `JsonDocGet` and `JsonDocAt` require `JsonDoc` and return `JsonDoc`; `JsonDocAsStr` and `JsonDocKey` return `Option<Str>`; `JsonDocAsScalar` accepts exactly `i64`, `f64`, or `Bool` and returns the matching `Option`; `JsonDocLen` returns `i64`; `JsonDocElems` requires an active arena and returns `Slice(JsonDoc)`. `At`/`Key` indices are `i64`. | Positive chained document owner plus one mutation per receiver, key/index, scalar target, builtin kind id, arena, and stored result; chained-view provenance is structural only and no region facts are consumed. |
| grouping and dictionary | Validate the base local, exact struct id, key/value ordinals, source discriminator, field types, operation/value-field pairing, non-empty multi-aggregate list, and exact interned tuple shape. Single `ArrayGroupAgg` supports `SoaI64` (`Soa`, i64 key), `SoaStr` (`Soa`, Str key), `AosStr` (`DynStructArray(_, Aos)`, Str key), and `Encoded` (`DictEncoded(struct,key)`, matching key). Its result is `(array<i64>,array<i64>)` for `SoaI64`, otherwise `(array<str>,array<i64>)`. `ArrayGroupAggMulti` is the sema first cut only: `AosStr` with an AoS dynamic struct-array, Str key, and one or more aggregates; its result is `array<str>` followed by one `array<i64>` per aggregate. `ArrayDictEncode` requires an AoS struct-array base and Str key field, returning the exact `DictEncoded` type. | Separate single-source and multi-Aos positive owners cover each supported op/value-field pairing and exact tuple arities/types; each base/id/ordinal/source/op/value-field/aggregate mutation is rejected, including invalid dictionary identity and non-local base. |
| type graph and malformed safety | Reuse checked `.get`/iterative walks for every struct, enum, tuple, builtin enum, field, and descriptor id. Reject invalid lengths, empty aggregate lists, NUL names, duplicate shape classes, unsupported nested types, and wrong result payloads without panic or unchecked user-derived indexing. TemplatePart/GroupSource/GroupAgg1/GroupOp carry no span; only their enclosing expression span participates in the universal span rule. | `deep_hir_body_pipeline_b2b2_type_dag_is_stack_bounded` validates a 512-node nested JSON descriptor on a 2 MiB stack; the direct positive owner covers the stored/discriminator mutations and later-sibling precedence. |
| control flow, arena, and source order | Schedule template part children, JSON operands, and document/group records in producer order under the LIFO worklist. Strict flows propagate through all b2b2 records, and `Arena(Block)` context is inherited by every nested template/JSON child and both branch arms exactly as existing body traversal does; arena-only rows reject outside the active context even when a sibling diverges. Group records have no expression children but still validate their base local before postconditions. | `hir_body_validator_pipeline_template_json_group_control_flow` covers a diverging template hole with a retained malformed part, arena nesting, both branch arms, and the no-arena document twin; the positive owner covers mixed part/source order. |
| ownership and activation | Do not replay body Drop sets, cleanup cells, allocation identity, region/borrow/escape provenance, effects, task proofs, or native state. The one structural exception is `JsonScan`'s canonical recursive Copy/DropPlan precondition, which is required to prevent a checked-HIR scanner row from reaching MIR; it performs no move, cleanup, or runtime action. The general b2b2 body validator remains dormant, while all four MIR lowering entrypoints invoke the active `align_mir::hir_program_is_valid` gate and its scanner predicate. | `hir_body_validator_json_scan_copy_row` covers the shared structural predicate and one direct/transitive Move mutation; `hir_program_json_scan_copy_row` and `hir_program_json_scan_envelope_mismatch` prove the active gate rejects malformed rows; `hir_body_validator_pipeline_deferred_facts_are_not_consumed` remains the owner for unrelated deferred facts, and repository search confirms the active gate is the only production consumer. |
| source propagation and review gate | Synchronize this matrix, the am-b2 ledger rows, exact owner names, the historical identity replay contract, and `HANDOFF.md`. The b2b2 cell claims only Template/JSON/group/dictionary validation plus the narrow Request 6 scanner safety gate; it must not claim b3, b4, general body activation, unresolved generic row-type support, or a public database surface early. | Fresh independent matrix review before coding; final wave matrix-to-diff pass; current focused owners, applicable Clippy, and the single stable-candidate preflight review/attestation. The fixed `json_scan_cross_compiler_identity` pair is replayable implementation-time evidence and is not rerun against later compiler heads. |

#### Am-b3 implementation closure matrix

This matrix is authoritative before the am-b3 native/runtime and generated-callable body slice is
implemented. Am-b3 reuses the one dormant `body_core_metadata_is_valid(&hir::Program) -> bool`
entry, `BodyValidator` state, pointer-keyed flow tables, and explicit child-first worklist closed
by am-b1 through am-b2b2. It owns `ExprKind::FsReadFile` through `ExprKind::CryptoArgon2` in
ledger order, plus `AeadCipher`, `AeadDir`, `HashAlgo`, `CliFlagKind`, `EncodingKind`,
`CompressKind`, and `PathComponentKind`. It adds no public caller, MIR/native registry entry,
runtime allocation, interface field, ABI change, cache identity, resource operation, effect replay,
Drop/ownership fact, or database-named compiler path. Am-b4 remains the sole body activation and
the owner of body-derived ownership/effect correlation; am-c remains the owner of typed callable
namespace identities and generated symbol encoding.

| Cell | Required am-b3 closure | Exact owner evidence |
|---|---|---|
| entry and universal order | Reuse the existing dormant entry and worklist. For every native/generated expression, validate the discriminator envelope and span, then schedule children in the ledger order, then apply stable-place, helper-enum, exact operand, callable-origin, and stored-result relations. Retained children after a terminating child remain structurally checked and never join a fallthrough/result state. | `hir_body_validator_native` calls the same helper; one envelope, span, child, helper, stable-place, callable, and result mutation per family; a retained malformed child after `ProcessExit`/`ProcessAbort`/`ProcessExec` proves order and non-fallthrough. |
| file, reader, writer, and copy | Validate `FsReadFile`, `ReaderStdin`, `ReaderOpen`, `WriterStd`, `WriterCreate`, `ReaderRead`, `ReaderBuffered`, `ReaderReadLine`, `BytesAsStr`, `WriterWrite`, `WriterFlush`, and `IoCopy` against exact `Str`/`bytes`/`Reader`/`Writer`/`Buffer` shapes, builtin `Error`, the ledger's exact `ReaderPlace`, `WriterPlace`, and `SourceMutLocal(Buffer, ...)` predicates, builder/byte-view discriminators, and result types. `ReaderReadLine` additionally requires the producer-recorded buffered-reader local; `ReaderBuffered` is the listed `consume-any` exception. No general mutable-handle rule is introduced. | Positive producer twins for every row; wrong type, fd/buffering, exact receiver place, exact mutable-buffer place, builder flag, and stored result mutations; source-local reuse after borrowed calls and `ReaderBuffered` consume-any are structural-only checks. |
| file, buffer, builder, and filesystem | Validate `FileCreateRw`, `FileOpenRw`, `FilePread`, `FilePwrite`, `FileLen`, `BufferNew`, `BufferBytes`, `StrBytes`, `BufferLen`, `BytesRead`, `BufferPut`, `BufferAppend`, `ArrayBuilderNew`, `ArrayBuilderPush`, `ArrayBuilderAppend`, `ArrayBuilderBuild`, `FsWriteFile`, `FsExists`, `FsRemove`, and `FsReadDir`. Enforce scalar widths/byte order, exact builder element/move flags, byte-view versus builder forms, and `SourceMutLocal(Buffer/ArrayBuilder, ...)` only where the ledger names it. Heap record rows delegate to the canonical `HeapRecord`/recursive ownership predicates. | Table-driven valid/malformed rows mutate every envelope bit, scalar/width, builder mode, receiver/place, child, stored result, record id/predicate, and record move bit. Producer-valid Copy/Move record and `String` push/build plus `DynArray(String)`/deep-owned filesystem results are covered; record formation/Drop facts use the shared producer helper rather than a private am-b3 list. |
| networking and process | Validate `DnsResolve`, `TcpConnect`, `ConnReader`, `ConnWriter`, `TcpReadTimeout`, `TcpWriteTimeout`, `TcpListen`, `TcpAccept`, `UdpBind`, `UdpSendTo`, `UdpRecvFrom`, `TimeNow`, `TimeInstant`, `ProcessCpuCount`, `TimeSleep`, `ProcessExit`, `ProcessAbort`, `ProcessSpawn`, `ChildWait`, `ChildKill`, `ProcessExec`, `ProcessCommand`, `CommandCwd`, `CommandTimeout`, `CommandEnv`, `CommandEnvClear`, `CommandRun`, `RunOutputCode`, `RunOutputStdout`, and `RunOutputStderr`. Enforce the ledger's exact `LocalHandle` predicate for every stable handle, `SourceMutLocal(Buffer, ...)` only for `UdpRecvFrom.buffer`, `argv = DynArray(Str)|Slice(Str)`, non-fallthrough process forms, and native state mutation without applying source `mut` to interior-mutated handles. | Positive rows and one mutation per host/port/timeout, handle kind/place, argv, command field, result, and termination relation; invalid later children after process termination remain rejected. No network/process syscall or runtime object is created by validation. |
| views, paths, environment, encoding, and compression | Validate `FsReadFileView`, `FsReadBytesView`, `PathJoin`, `PathComponent`, `PathNormalize`, `EnvGet`, `EnvSet`, `EncodingEncode`, `EncodingDecode`, `Utf8Valid`, `Compress`, and `Decompress`. Require arena context for file views, exact `PathComponentKind`/`EncodingKind`/`CompressKind`, byte-view and level relations, and fresh owned result types. | Arena/no-arena, helper-enum, input/result, level, and retained-child mutations; `Html` decode rejection and all exact encode/decode/compress helper pairs; no region/escape fact is inferred beyond the structural arena-depth gate. |
| random, regex, CLI, HTTP, and crypto | Validate `RandSeed`, `RandSeedWith`, `RandNext`, `RandRange`, `RandShuffle`, `RandSample`, every regex/capture row, every CLI row, every HTTP row, and every crypto row through `CryptoArgon2`. Enforce admitted scalar/struct ids, the ledger's exact `SourceMutLocal(Rng, ...)` and writable-slice predicates, exact `LocalHandle`/`HttpResponsePlace`/`HttpRequestCtxPlace` exceptions, byte-view forms, optional `RegexFind` start, exact HTTP result/resource shapes, crypto parameter structs, and `AeadCipher`/`AeadDir`/hash output contracts. No grouped row may replace those predicates with a general `Local` or `is_mut` rule. | `hir_body_validator_native` covers each discriminator and each stored field; `hir_body_validator_generated_callables` covers native-generated callback/signature relations separately. Every invalid id, field, helper enum, child type, option presence, exact place/mutability predicate, and result has a one-field negative. |
| generated callable facts | Reconcile `FnValue`, `Closure`, `CallFnValue`, direct `Call`, `ResultMapErr`, and every native/pipeline callable target against one exact stored/imported/extern signature and in-range `FnTy`. Preserve `FnOrigin::{Source,Monomorph,Lifted}` rules, non-exportable captured closures, exact capture count/trailing parameters, and native/generated helper signature rows. Per am-u, an extern is permitted only for the owning direct `Call` or named non-escaping stage/terminal invocation at `unsafe_depth > 0`; `FnValue`, `Closure`, `CallFnValue`, and function-value mapper forms never resolve an extern, even inside `Unsafe`. Do not infer/replay `FnEffect`, Drop, borrow, region, or ownership facts; those belong to am-b4/am-c. | `hir_body_validator_generated_callables` mutates origin class, capture count/order/type, exact `FnTy`, imported/extern/stored target, monomorph suffix, helper signature, and callable result. Direct and indirect, named and captured, whole/per-unit-shaped HIR twins remain dormant and do not publish a registry identity; extern direct/callback permission twins preserve am-u's exact lexical rule. |
| control, source order, and malformed safety | Native children are evaluated in the exact ledger order; strict flow carries `falls`/breaks through every row, arena depth is inherited for file views, `unsafe_depth` remains lexical, and `ProcessExit`/`ProcessAbort`/successful `ProcessExec` are non-fallthrough according to the producer. All ids, lengths, enum tags, widths, fields, paths, helper discriminators, and option shapes use checked access; no native recursion is added. | `hir_body_validator_native_control_flow` covers branch/loop/arena/unsafe/early-exit and retained malformed child precedence; `deep_hir_body_native_type_dag_is_stack_bounded` runs a deep valid and malformed later-sibling graph on the 2 MiB stack and asserts no panic. |
| ownership, cleanup, allocation, and FFI | Native/generated validation is compiler-only. It must not read or mutate Drop sets, cleanup cells, `FnTy.effect`, TaskProof/WaitProof, region/borrow/escape state, native/runtime tables, or allocation identity; it must not null, move, Drop, register, or call an Align/runtime/native object. Accepted HIR and all four lowering entrypoints remain byte-identical. | Deferred-facts mutation twins, accepted-HIR identity checks, repository search for the sole dormant caller, and no-action assertions for malformed native/generated records. |
| source propagation and review gate | Synchronize this matrix, the am-b3 ledger/inventory rows, exact owner names, and `HANDOFF.md`. The cell claims only native/runtime/generated-callable validation; it must not claim am-b4 activation, effect/ownership replay, am-c callable namespaces, L3 resources, or a public database surface early. | Fresh independent matrix review before coding; final wave matrix-to-diff pass; focused am-b3 owners, `scripts/test-pr.sh`, applicable Clippy, and the single stable-candidate preflight review/attestation. |

The measured am-b3 implementation and owner-test size is historical planning evidence, not a
current split or review trigger. It remains one closure cell because every native family shares
the same envelope gate, producer-order child worklist, lexical arena/unsafe context, flow join, and
dormant stored-result check; splitting those families would either duplicate that state machine or
leave an intermediate validator with a partial `ExprKind` domain and a second temporary path. The
boundary therefore lands the complete native/generated structural slice together, while am-b4 and
am-c remain separate activation and callable-identity boundaries.

#### Am-b4 implementation closure matrix

This matrix is authoritative for the am-b4 implementation and activation. The validator reuses the
producer's sema MoveCheck, EscapeCheck, effect fixed point, and path-complete `task_wait` replay on
a compiler-owned clone of the already structurally checked HIR. It resets every producer-owned fact
on that clone before replay, compares the replayed facts with the original HIR, and never mutates
the caller's HIR. Am-b4 is deliberately ordered as three independently correct verticals: first
close the task-wait replay identity/stack contract, then add the sema ownership/effect fact replay,
then activate `body_core` plus fact replay at all four MIR entries. The task-wait and sema replay
prerequisites are landed; this activation vertical closes the canonical-empty and valid-identity
owners. The structural body envelope includes the declaration-free core calls `print`, `hash64`,
and `hash128` with their producer-level borrowed-string representation, and compares a fresh local
`FnTy` cell by its callable shape rather than by its compiler-local ordinal. A producer-only change
would leave malformed HIR accepted, a replay-only change would
remain uncalled, and an entrypoint-only change would publish unchecked body facts.

| Cell | Required am-b4 closure | Exact owner evidence |
|---|---|---|
| structural activation order | The shared MIR gate must short-circuit in this order: checked-HIR depth, global type/placement/nominal/header validators, the active Request 6 `json.scan` envelope/Copy validator, the body-only structural record validator (`body_core::validate`), then sema body-fact replay. A malformed scanner/local/type/callable record must never reach MoveCheck/EscapeCheck/effect/task-wait replay. Every nonparameter local must have exactly one `Let`, `LetTuple`, or match-payload binding; even an unused orphan local-table record fails closed. The child worklist must visit every source, stage capture, terminal operand, and terminal capture later consumed by derivation; no producer-valid captured terminal may fail because its flow record was omitted. Declaration-free `print`, `hash64`, and `hash128` calls are validated by their exact builtin contracts before replay; source `String` operands must already be represented by `StrBorrow`/`Str`. | The Request 6 scanner precedence matrix pins its earlier gate. A body-structure plus body-fact simultaneous mutation proves the body-core failure wins; all four entrypoints return canonical-empty without replay side effects. `malformed_hir_unused_local_record_fails_closed`, `capturing_partition_and_par_map_reach_all_lowerers`, `hir_body_validator_accepts_builtin_display_and_hash_calls`, `hir_body_validator_rejects_unborrowed_builtin_string`, `hir_body_validator_accepts_nested_tagged_payload_construction`, and the body owner matrix close the binding, child-worklist, builtin, callable-shape, and nested-payload inventory. |
| replay entry and reset | Use one public sema replay predicate, `checked_hir_body_facts_are_valid(&hir::Program) -> bool`, that clones HIR, clears function return summaries, parallel-transfer summaries, Drop vectors/map, assignment cleanup cells, and all `FnTy.effect` cells through a bounded immutable event walk, then reruns the producer analyses in their existing order. No raw-pointer dereference or unsafe metadata mutation is permitted. Recompute imported return provenance, effect seeds, and parallel-transfer seeds only from validated HIR declarations; `ImportedFn.return_provenance_known` preserves the distinction between an explicit external `None` and a compatibility-API omission, whose all-compatible-input fallback must be replayed, while a compatibility omission of transfer roots selects every borrow-capable imported parameter. Do not infer facts from a missing body. Direct and concrete function-value calls translate exact transfer roots; unresolved indirect targets conservatively select every compatible argument/capture. A direct malformed-HIR call also fails closed if a legacy producer analysis reaches an unchecked ordinal; the predicate is not a replacement for the structural envelope validators. | The sema replay owners are `checked_hir_body_fact_replay_rejects_stale_producer_facts`, `checked_hir_body_fact_replay_covers_cleanup_and_function_effects`, `checked_hir_body_fact_replay_preserves_imported_fact_presence`, the imported/direct/indirect parallel-transfer parity owner, and the malformed-local-ordinal twin; this activation vertical adds `malformed_hir_body_metadata_fails_closed` and the canonical-empty/four-entrypoint owners. Valid producer HIR remains accepted; replay preserves input HIR, `fn_types.len`, all local `Ty::Fn` ids, and no MIR/native/artifact/cache state is allocated. |
| replay clone stack safety | The compiler-owned replay clone must not call derived recursive `Clone` for `Expr`, `Block`, `Stmt`, `MatchArm`, `Stage`, or `TemplatePart`. A child-first body-record worklist clones every expression exactly once, reconstructs each parent from already-cloned children, then consumes those children while rebuilding blocks/statements/stages/templates. The top-level metadata clone is field-explicit; assignment cleanup cells copy their current booleans and are reset before replay. No raw-pointer dereference, unsafe aliasing, depth-dependent acceptance shortcut, or larger worker-stack workaround is permitted. | `checked_hir_depth_closure_matrix` owns producer-valid depths 258 and 259 on the 2 MiB owner thread; the four-entrypoint identity test proves the rebuilt body is semantically identical; stale-fact, cleanup/effect, and imported-presence replay tests prove every rebuilt fact path remains covered; `git diff`/author matrix pass confirms every current `ExprKind` child-bearing variant is present in the explicit reconstruction match, and malformed child lookup fails closed without a panic. |
| ownership and return facts | Re-run `infer_return_provenance`, MoveCheck, and EscapeCheck with their existing explicit flow/worklist state. Compare every stored function's return-borrow/region summary, ascending `drop_locals`, ascending `drop_individual_locals`, and exact source-order `drop_individual_exprs` key/value map. A replay diagnostic rejects the body before comparison. | Body fixtures cover direct Move, borrowed views, branch/loop joins, early exits, retained dead children, arena/task-group regions, and malformed summary/Drop/map mutations; `malformed_hir_body_metadata_fails_closed` verifies first invalid body produces canonical-empty. |
| cleanup-cell inventory | Recompute `Assign.drop_old` and `Assign.drop_new` after all children, including every nested statement/control path. The traversal inventory must visit `Assign`, indexed/field/element replacement relations, return, break, match, loop, `?`, `else`, and retained dead children in producer order; no cleanup cell is accepted merely because its default is false. | Table-driven assignment and replacement twins mutate each `Cell<bool>` independently and in branch/loop joins; owner assertions cover Move RHS transfer, old-value Drop, arena-vs-heap replacement, and no-cell Copy paths. |
| effect fixed point and snapshots | Reset all stored effect cells to the producer's annotation baseline (`Unknown`) without allocating fresh local function types, then rerun closed-world and exportable open-world effect solving on the existing type topology. Recreate the deterministic compiler-owned boundary cells for every concrete local/aggregate/tuple/tagged/array projection, expression result, return, and capture; those ephemeral cells have no persisted producer snapshot, so replay diagnostics and complete parallel-eligibility checks are their equality evidence. Unused annotation-only cells remain `Unknown`, and imported effects remain the am-h value. | `checked_hir_body_fact_replay_covers_cleanup_and_function_effects` mutates concrete function-value effects and assignment cleanup cells; `checked_hir_body_fact_replay_rejects_stale_producer_facts` covers the annotation-only `Unknown` baseline; `valid_hir_body_preflight_is_mir_identity` proves effect facts are validation-only; replay-before/after checks prove no topology or local `Ty::Fn` id changes. |
| imported facts | Per-unit replay reconstructs `ExternalReturnProvenance` only for imported declarations whose HIR presence bit says the validated record was supplied; an omitted compatibility-API record retains the producer's all-compatible-input fallback, while an explicit `None` is exact. `external_effects` remains the imported declaration's `Pure`/`Unknown`/`Impure` seed, with no body inference. The validated interface-v6 `parallel_transfer_params` set is the third imported seed; a compatibility omission selects every borrow-capable parameter. The return-provenance-presence × effect × transfer-root Cartesian product is preserved through interface construction, HIR import construction, whole/per-unit replay, and exportable open-world callback solving. | `checked_hir_body_fact_replay_preserves_imported_fact_presence` covers absent versus explicit `None`/`Roots` across all three effects and empty/nonempty/compatibility-absent transfer roots; imported return-provenance × effect × transfer-root twins, absent normalization, and open-world callback owners cover every combination before MIR identity checks. |
| parallel eligibility (`pkg.csv` implemented) | Before any MIR lowering or generated identity, the shipped gate requires every `ArrayParMap` and widened parallel-stage callable to resolve to the replayed complete `Pure` fact. It independently consumes the completed worker-transfer provenance of every staged or terminal capture through the same authority used by `spawn`: direct `Ty::ArenaHandle`, a region behind one or more concrete `ClosureTarget`/`ClosureCapture` pairs, and unavailable callable provenance fail closed. Moves, reassignment, `if`/`match`/`else`/loop joins, direct/imported/indirect helper summaries, and whole/per-unit calls preserve the may-union; a known noncapturing function has an empty environment. Externs remain Impure. Checked HIR owns the transitive proof before MIR; codegen defensively rejects a direct handcrafted-MIR `ArenaHandle` before capture-context layout or kernel/runtime publication and does not guess through an opaque MIR function value. | Existing effect owners cover positive/negative `ArrayParMap` and widened-stage direct, indirect, closure, aggregate projection, imported, and open-world callback targets. The `pkg.csv` worker-send owner covers direct and function-value-wrapped regions across spawn and par-map, while malformed `ParMapParallel`/`ParMapReduce` and `SpawnTask` MIR `ArenaHandle` captures fail in codegen. Whole/per-unit and concrete-monomorph parity, the existing spawn negative, noncapturing/capturing-without-region function values, and sequential closure/map/reduce region positives remain. Every failing checked path rejects before generated identity, context layout, kernel/runtime call, or allocation. |
| wait dominance prerequisite | The am-b4 replay consumes a prerequisite am-w parity vertical. `task_wait` must use stable body-preorder identities rather than duplicateable Spans, explicit bounded frames, and a convergent loop-header fixed point; proof tokens remain analysis-local and are never serialized. Am-b4 then reruns that closed analyzer for every stored body and rejects any diagnostic. | `task_wait_duplicate_span_identity`, `task_wait_duplicate_span_all_identity_kinds`, `task_wait_duplicate_span_gets_report_separately`, `task_wait_missing_node_fails_closed`, `task_wait_token_exhaustion_fails_closed`, `task_wait_empty_body_has_replay_budget`, `task_wait_depth_is_stack_bounded`, `task_wait_loop_fixed_point_guard_is_depth_derived`, `task_wait_loop_unresolved_wait_reaches_later_break`, plus `cargo test -p align_driver --test task_group --test per_unit_codegen` land before activation. |
| function-root completion | Correlate each stored function root with the same source-order completion walk: `Return(None)` only for Unit, reachable absent tail only for Unit, and a non-Unit root must have a typed tail or non-fallthrough completion. | `body_contract_function_return_none` and `body_contract_function_root_completion` mutate return type, tail presence, and reachable/non-reachable predecessors; all four entrypoints fail closed. |
| global activation and empty result | Add the body-fact predicate to the one shared `hir_program_is_valid` gate used by `lower_program`, located lowering, per-unit lowering, and located per-unit lowering. Any invalid body returns the canonical `Program` with every vector empty before `lower_program_unchecked`; valid HIR retains byte-identical MIR and per-unit/whole parity. | `malformed_hir_body_metadata_fails_closed`, `malformed_hir_body_structure_precedes_fact_replay`, `valid_hir_body_preflight_is_mir_identity`, `checked_hir_depth_closure_matrix`, `deep_hir_body_core_type_dag_is_stack_bounded`, and `deep_type_consumer_closure_matrix`; all four entrypoints are asserted where the owner covers activation, including located source maps. The body owner matrix also covers builtin calls, unborrowed strings, fresh local `FnTy` shape matching, nested tagged payloads, reachable loop breaks, and cross-region break rejection. |
| depth and type graph | Keep the checked-HIR record ceiling at 259 and use bounded producer replay/worklist paths for body records. Accept 258/259 valid bodies, reject 260 before replay/lowering, and accept deep finite header-mediated type DAGs while rejecting malformed later siblings without process-stack recursion. | `checked_hir_depth_closure_matrix`, `deep_hir_body_core_type_dag_is_stack_bounded`, `deep_type_consumer_closure_matrix`, `cargo test -p align_mir`, and the existing whole/per-unit LLVM verification rows. |
| interface/HIR construction, stripping, downstream parity, and benchmark | Interface v6 adds the canonical public-function `parallel_transfer_params` field and per-unit HIR adds the matching validation-only imported field plus stored source-function summary. Interface decode authenticates the set before HIR construction; replay consumes only that validated seed, compares stored descriptor facts, and strips imported/summary metadata before the unchanged six-field MIR imported record. Whole-program construction has no imported seed. No runtime ABI field, native call, allocation, registry, or artifact/cache publication occurs before validation; the interface version/hash intentionally changes while accepted whole/per-unit and located/non-located MIR identity remains unchanged. Record `mir-body-validation` plus unchanged continuation cost. | Interface-v6 semantic/byte/hash goldens and malformed root matrix; imported construction/compatibility-absence owners; stored-summary mutation and descriptor mismatch owners through all four lowerers; six-field MIR structural identity; `cargo test -p align_driver --test expr_depth within_limit_chain_compiles_and_runs`; `cargo test -p align_driver --test per_unit_codegen`; `bench/library_boundary/run.sh provenance`: `mir-body-validation`, `mir-continuation-lowering`. |
| checkpoint, source propagation, and review gate | Build the prerequisite task-wait parity, sema replay, and MIR activation verticals as compile- and owner-test-backed intermediate commits inside the canonical callable wave; do not keep a legacy/new parallel validator path. The activation checkpoint includes the explicit replay clone because the shared gate cannot safely activate while replay still uses derived recursive `Clone`. Synchronize this matrix, the am-b4 rules in `docs/impl/19-hir-validation-ledger.md`, owner names, and `HANDOFF.md`. Am-c callable identities, L3 resources, and `pkg.db` remain separate capabilities. | One fresh independent matrix review before coding; author matrix-to-diff passes at intermediate checkpoints; exact owner commands, `scripts/test-pr.sh`, applicable Clippy, and one stable-candidate full-diff review/finding closure for the complete wave before its pre-PR attestation. |

#### Am-b1 implementation closure matrix

This matrix is authoritative before the dormant am-b1 validator core is implemented. The public
boundary is intentionally narrow: `body_core_metadata_is_valid(&hir::Program) -> bool` is a
crate-private owner helper used only by am-b1 tests. It is not called by `hir_program_is_valid` or
any of the four public MIR lowerers; malformed body HIR therefore remains unactivated until am-b4.
The helper assumes that the am-h declaration/header preflight will run before activation and does
not validate `Fn.params`, `Fn.param_modes`, `Local.is_param`, stored signature roles, imported or
extern headers, Drop sets, effects, or wait proofs. In particular, b1 does not use
`Local.is_param` to classify a body binding; b1 only rejects duplicate `Let`/`LetTuple`/match
binding declarations and parameter IDs rebound by a body declaration. Body-produced locals whose
initializing discriminator belongs to am-b2 or am-b3 may remain unbound in the dormant b1 slice;
their producer and the final am-b4 activation close that relation.
The measured implementation/test size is historical planning evidence, not a current split or
review trigger. The b1 cell stays atomic inside the capability wave because every discriminator
shares the same child-first explicit worklist, pointer-keyed flow tables, lexical context stack,
fallthrough/break join, and stored-result gate. Splitting by syntax family would either duplicate
that order/state machine or leave a temporary validator that rejects a producer-valid b1 body and
requires a second legacy/new path; both violate the single dormant entry contract. The matrix,
owner tests, and final matrix-to-diff pass preserve this rationale without using line count as a
progress or PR boundary.
The validator owns structural and type relations for the am-b1 ledger range only: all statements,
local/place records, ordinary expressions through `BuilderToString`, tagged values, calls,
aggregates, vector arithmetic/lane/SoA records, and structured control. Storage, vector-storage,
array, pipeline, template, and JSON expressions belong to am-b2; native/runtime/generated-callable expressions belong to am-b3; body-derived ownership,
Drop, effect, successful-wait, and global activation belong to am-b4.

| Cell | Required am-b1 closure | Exact owner evidence |
|---|---|---|
| construction and entry | Add one body-core validator entrypoint and one `BodyValidator` state containing the immutable HIR program, current function, current lexical `unsafe_depth`, and the active loop target stack. Visit stored functions in declaration order and each body block in source order. Do not mutate HIR, allocate runtime/native/artifact/cache state, or change any public lowering result. | `hir_body_validator_core` calls the helper directly; a valid source-produced body is accepted; `lower_program`, located lowering, per-unit lowering, and located per-unit lowering remain byte-identical because none invokes the dormant helper. |
| universal record order | For every record, validate the variant tag, all non-expression envelope fields in ledger order, then the enclosing `Span` (`lo <= hi`), then children left-to-right, then relational/type postconditions, then stored `Expr.ty`. Am-b1 stops at that stored-result check: the final body-derived records named by the full ledger are not visited until am-b4 activation. Retained children after a non-fallthrough child are still structurally validated; they never contribute a fallthrough/result join. First invalid envelope, span, child, relation, or stored result wins in that order, except for the explicit Request 6 `JsonScan` order recorded above. | The expression inventory covers every b1 discriminator and representative envelope/type mutation; statement and control owners cover the stored field families. The implementation preserves the universal order structurally; diagnostic precedence is not externally observable from this boolean owner helper and is closed by the ledger-to-code audit. |
| locals and places | `Local(id)` and every statement place require an in-range ordinal local; mutable places require `is_mut`; field paths are non-empty, in range, intermediate `Struct`, and produce the exact leaf type; tuple/enum/tagged/function ids, variants, fields, and ordinals are in range before use. Assignment, indexed assignment, vector lanes, field/element stores, and tuple bindings correlate every stored id/path/arity with the child types. Local type forms also retain am-p placement restrictions: dynamic arrays cannot use `Struct` scalar elements, and AoS dynamic struct arrays reject over-aligned element structs. Parameter-role facts and initialization/ownership completeness remain am-h/am-b4 facts. | `hir_body_validator_statements` covers the local/place and child-type families that b1 owns; `hir_body_validator_local_type_placement_is_fail_closed` closes the two local placement negatives and one valid twin; `hir_body_validator_deferred_facts_are_not_consumed` proves Drop cells and `Local.is_param` do not affect the dormant body result. |
| statements | Validate `Let`, `LetTuple`, `Assign`, `AssignIndex`, `AssignVecLane`, `AssignField`, `AssignElemField`, `AssignElem`, `Return`, `Break`, and `Expr` in declaration order. Check local/type/mode/arity/target relations and function return/loop-target type relations. `accepted=false` breaks remain non-fallthrough but still validate their retained payload structurally. `Let` ownership, assignment `drop_old`/`drop_new`, replacement Drop facts, return-root/region correlation, and loop iteration Drop sets are stored body-derived facts and are explicitly deferred to am-b4. | `hir_body_validator_statement_inventory` has a valid fixture for all 11 statement discriminators and one independent mutation for every b1-owned envelope/child field; `hir_body_validator_deferred_facts_are_not_consumed` mutates the am-b4-owned Drop cells and header-role bit and proves b1 acceptance is unchanged. |
| b1 expressions | Exhaustively match `Unit`, `Int`, `Float`, `Char`, `Str`, `Bool`, `Local`, `Unary`, `Cast`, `Binary`, `IntArith`, `MathOp`, `FnValue`, `Closure`, `CallFnValue`, `TaskGroup`, `EnumValue`, `Match`, `ResultMapErr`, `Spawn`, `TaskGet`, `Wait`, `Call`, `If`, `StructLit`, `Field`, `SoaColumn`, `Tuple`, `TupleIndex`, `IndexField`, `Block`, `OptionSome`, `OptionNone`, `ElseUnwrap`, `ResultOk`, `ResultErr`, `Try`, `Loop`, `Arena`, `Unsafe`, `RawAlloc`, `RawFree`, `RawLoad`, `RawStore`, `RawOffset`, `HeapNew`, `BoxGet`, `BoxClone`, `StrClone`, `StrPredicate`, `StrTrim`, `StrBorrow`, `BuilderNew`, `BuilderWrite`, and `BuilderToString`. Each row follows the exact am-b1 ledger relation and rejects every b2/b3 discriminator in the dormant core. | `hir_body_validator_expression_inventory` has a valid owner case for every listed discriminator plus a negative stored-result mutation; the remaining per-field mutation Cartesian product is tracked in the implementation ledger rather than claimed by this single representative owner. |
| callable and sum relations | Resolve `FnValue`/`Closure`/`CallFnValue`/`Call` against one exact stored/imported/extern signature and in-range `FnTy`; reject extern `FnValue`, invalid generic suffix shape, disabled modes, and mismatched arguments. A monomorph call strips only the exact encoded `$...` suffix, preserving any module `$` in the base name. Validate `EnumValue`, `Match`, `Option`, `Result`, `Else`, `Try`, and `ResultMapErr` against exact enum/tagged tables, payload/arm coverage, and fallthrough result types. Am-b1 consumes the am-h-validated `FnTy` return-provenance fields only as part of the canonical function-type mangle; it does not infer or replay return provenance, effect, or callable-origin facts owned by am-h/am-b3/am-b4. | `hir_body_validator_expression_inventory` and `hir_body_validator_accepts_module_monomorph_call_name` provide the representative callable/sum cases; `hir_body_type_mangle_golden_vectors` closes the function-type provenance encoding; the remaining per-field mutation Cartesian product is tracked by the owning ledger. |
| structured control | Validate `TaskGroup`, `If`, `Block`, `Loop`, `Arena`, and `Unsafe` child structure, divergence/result compatibility, loop-local range shape, and lexical unsafe depth. `TaskGet` validates its exact `Task(T)` primitive-Copy type and child/result relation; `Wait` validates its task-group context and exact Unit/`Result<Unit,Error>` result shape. The sema `TaskProof`/`WaitProof` group, generation, completion, and successful-wait dominance state is not stored HIR and is deferred to am-w/am-b4. | Control-family table rows, nested unsafe-depth restoration, divergent branch/tail, loop break/result, and valid `TaskGet`/`Wait` fixtures; no task proof or effect cell is consulted. Negative `TaskGet`/`Wait` type/context mutations and retained-dead-child precedence twins remain tracked by the owning ledger. |
| type graph and stack | Reuse the am-d common explicit type traversal through every b1 type relation: function parameters/returns, locals, statement targets, struct field paths, tuple elements, enum/tagged payloads and matches, callable signatures, projections, and control-result joins. Accept a deep finite am-g-t-valid acyclic inline-struct graph rooted through a function parameter/return and reject a malformed later sibling deterministically without process-stack recursion. The body record depth ceiling remains am-d-owned; am-b1 adds no type-depth cap. | `deep_hir_body_core_type_dag_is_stack_bounded` exercises the deep parameter/return root with one valid fixture and one malformed later sibling via the direct helper on a 2 MiB stack; competing-invalid precedence remains owned by the common type/record validation suites. |
| ownership, cleanup, effects, and allocation | Do not read, recompute, or compare `drop_locals`, `drop_individual_locals`, `drop_individual_exprs`, `Assign` drop cells, `FnTy.effect`, `TaskProof`/`WaitProof` analysis state, or allocation flags. Function-type return provenance is different: am-h validates it as a header/type-identity fact, and b1 reads it only through the producer-compatible type mangle; b1 does not infer or replay call/return provenance. Do not null, move, Drop, replace, return, or register any Align/runtime/native object. The body-derived relations remain am-b4-owned; b1 only validates the type/control envelope needed to host them. | `hir_body_validator_deferred_facts_are_not_consumed` covers representative Drop-cell and `Local.is_param` independence; `hir_body_type_mangle_golden_vectors` covers the header-owned provenance mangle; per-field am-b4 mutation and replay remain deferred to their owner. |
| activation and downstream parity | No `hir_program_is_valid` call, MIR shape, interface byte, ABI, link/cache identity, benchmark row, or codegen path changes in am-b1. Am-b2 and am-b3 consume the same dormant validator state only after their own discriminators land; am-b4 owns the single global activation and canonical-empty behavior. | Existing whole/per-unit identity and codegen suites remain unchanged; `git diff` and repository search prove the helper has no public-lowering caller; no public benchmark row is added. |
| source-of-truth propagation | Keep this matrix, the am-b1 row in the public ledger, the am-b1 expression/statement rows in `docs/impl/19-hir-validation-ledger.md`, and `HANDOFF.md` consistent. Do not mark am-b2/b3/b4 or body activation complete early. | Author matrix-to-diff pass, exact owner test names, and the final capability-wave HANDOFF checkpoint. |

#### Am-h implementation closure matrix

This matrix is authoritative before the am-h declaration/header validator and its normalized
imported-effect transport are implemented. The cell is one atomic vertical inside the capability
wave: it publishes no
body-derived ownership or effect facts, and it does not activate the dormant body validators owned
by am-b1 through am-b4. A producer-only split would require a
temporary legacy `Fn`/imported-record representation or would publish an unvalidated effect field;
a validator-only split cannot distinguish source, monomorph, and lifted headers while the current
overloaded pair remains; and a MIR-only split cannot prove the validation-only effect is stripped.
The final author matrix-to-diff pass preserves this atomic-boundary rationale; line count is not a
separate PR or review trigger.

| Cell | Required am-h closure | Exact owner evidence |
|---|---|---|
| formation and validation order | The common HIR preflight validates extern declarations first, imported declarations second, function-value header records third, stored function headers fourth, and locals/header-adjacent sets fifth. Each record is checked in ledger order: identity/name, arity and ids, modes, producer-owned type placement, summaries, origin/role, and structural sets. On the first invalid field it returns the canonical empty MIR program; no later record is inspected for a competing diagnostic. Every malformed mutation is tested alone; later-sibling precedence is covered separately. | `malformed_hir_declaration_header_metadata_fails_closed` mutates each owned header field one at a time and verifies canonical-empty parity through all four entrypoints; `main_header_abi_matrix_is_exhaustive` closes the main/error Cartesian cells; `deep_hir_header_type_dag_is_stack_bounded` covers malformed later-sibling rejection. |
| extern headers | Extern names are nonempty, NUL-free, unique in the existing extern namespace, and not the logical `main` name. `params` and `param_modes` have equal length; modes are exactly `ByValue`; parameter and return types must satisfy the existing C-boundary placement validator owned by am-p; return-borrow and return-region summaries are canonical `None` for the current ABI. No native side effect or link action occurs during validation. | Extern name/arity/mode/summary/duplicate mutations; extern-`main` and non-`ByValue` twins; am-p's C-boundary type-placement owner; all four lowering entrypoints and per-unit/whole MIR identity checks. |
| imported headers, effect, and parallel-transfer transport | Every imported non-generic public function has the exact HIR transport `name, params, param_modes, ret, return_provenance_known, return_borrow, return_region, effect, parallel_transfer_params`, and an exact `main` name is not an imported logical entry. `return_provenance_known` is am-b4-owned replay metadata: `false` preserves the compatibility-API omission fallback, while `true` makes an explicit `None` exact; it is stripped before MIR. `parallel_transfer_params` is the interface-v6 canonical strictly increasing unique set of in-range borrow-capable parameter roots whose contained values may reach `spawn`/`par_map`; this class includes `ArenaHandle`, `Fn`, and every aggregate with a reachable borrow-capable field. Absence from a compatibility producer conservatively becomes every borrow-capable parameter before HIR publication. Parameter/return type formation and placement remain am-p-owned; am-h applies the source-signature, mode, return-summary, and transfer-root boundary to those types. The producer copies `external_effects[canonical_name]` when present and normalizes an absent compatibility-map entry to `Impure`; `Pure`, `Impure`, and `Unknown` remain distinct. The typed compatibility map has no fourth valid value: the interface decoder's `read_effect` rejects an invalid tag before sema, while a handcrafted HIR can only carry the three enum variants. Imported headers cannot carry generic/source-body/origin records. Validation authenticates normalized effect and transfer roots but does not infer an imported body. | `imported_effect_facts_are_normalized_and_stripped` covers present `Pure`/`Impure`/`Unknown` and absent→`Impure`; the imported-transfer owner covers empty/nonempty/absent plus order/range/borrow-capability mutations and proves stripping before MIR; `checked_hir_body_fact_replay_preserves_imported_fact_presence` covers omitted versus explicit provenance presence across all three effect seeds; `invalid_effect_tag_is_rejected_before_sema` rejects the fourth value; interface-v6 byte/hash owners reject malformed roots; generic-import rejection and malformed name/mode/summary/root mutations cover the header; am-p's type-placement owner and interface producer/consumer parity cover the canonical key. |
| MIR imported record and artifact identity | After validation, MIR stores a distinct six-field `mir::ImportedFn` (`name, params, param_modes, ret, return_borrow, return_region`) without the validation-only `effect` or `parallel_transfer_params`. The am-h owner guarantees the same six-field structural Debug/record identity and per-unit imported declaration ordering; it does not independently re-measure interface-summary bytes, `impl_hash`, link/cache keys, or cache hit behavior. Whole-program MIR keeps its existing empty imported list; per-unit MIR copies the validated declarations in source/interface order. Existing interface/hash/cache owners remain responsible for those artifact identities. | `valid_hir_declaration_header_preflight_is_mir_identity` compares the six-field MIR record and structural Debug across effect and transfer-root variants; existing downstream interface summary/hash and per-unit cache owners cover their artifact identities. |
| stored function header and `FnOrigin` | Replace the overloaded `lifted_capture_count`/`exportable` pair with `FnOrigin::{Source { is_entry, is_public }, Monomorph, Lifted { capture_count }}`. Source declarations set entry and visibility explicitly; concrete generic instantiations are `Monomorph`; every lambda is `Lifted`, including capture count zero. A lifted count is at most `params.len()`, all lifted parameter modes are `ByValue`, and lifted return summaries are `None`. Exportability is derived exactly from `Source { is_entry: false, is_public: true }`; it is not an independent mutable fact. `FnOrigin` is HIR-only metadata: it is not serialized into interface records, MIR structural Debug, `impl_hash`, object-cache keys, or runtime ABI. The existing derived MIR `Function.exportable` bit remains the only per-unit linkage input. | `lifted_function_origin_metadata_is_explicit` covers entry-unit source visibility, generic monomorph, and zero/nonzero-capture lambdas; `non_entry_public_function_origin_is_exportable` covers the actual external-unit producer; `valid_hir_declaration_header_preflight_is_mir_identity` proves the origin is absent from the MIR imported record. |
| main header and ABI role | Only a non-generic `FnOrigin::Source { is_entry: true, ... }` named `main` may be the logical entry. Its parameters are either empty or exactly one `ByValue array<str>`; its return is exactly `Unit`, signed `i32`, or `Result<Unit,builtin Error>`, with the argv form restricted to the Result form. The builtin `Error` name/source-name, variant names/order, field bases, and payload shape are exact. Monomorphs, lifted functions, imported functions, and externs cannot become main. | No-arg/argv × Unit/i32/Result matrix, every parameter/return/Error identity and shape mutation, generic/lifted/imported/extern main twins, exact diagnostic order, and whole/per-unit ABI/codegen owners. |
| parameters, locals, and summaries | Parameter vectors and modes have equal length. Every parameter id is unique and in range, points to a local with the same id and signature type, and has the source/monomorph `is_param` role; lifted capture locals remain non-parameters. Local ids are ordinal, names are nonempty and NUL-free, parameter names are ASCII source identifiers, and non-parameter fixed int/float array alignment is either absent or a power of two no greater than `2^29`. Return-borrow/region summaries are canonical `None` or have sorted unique in-range roots that reference borrow-capable parameter types; captures are empty before am-b. FnTy type placements are owned by am-p; FnTy modes and summaries use the same header rules. | Deep valid/malformed signature and summary twins; duplicate/out-of-range/id-type/name/ASCII/alignment/root-order/root-duplicate/root-range/non-borrowable-root/capture mutations; FnTy mode/summary mutations; all four entrypoints and source/monomorph/lifted producers. |
| drop-set structure | `drop_locals` and `drop_individual_locals` are sorted, unique, in range, and the individual set is a subset of the broad set. am-h deliberately does not read `drop_individual_exprs` at all: its `HashMap<Span,bool>` keys, bool values, duplicate-span insertion semantics, and correlation to expression ownership are wholly owned by am-b4, whose exact rule is in `docs/impl/19-hir-validation-ledger.md` §Universal record order and ownership correlation. No am-h path creates, removes, replaces, moves, nulls, or Drops a source value. | `malformed_hir_declaration_header_metadata_fails_closed` mutates sortedness, duplicates, range, and subset; `valid_header_does_not_consume_body_facts` proves an arbitrary `drop_individual_exprs` entry and compiler-only local type do not change am-h acceptance. |
| body/control boundary and precedence | am-h does not traverse or reinterpret statements, expressions, or control joins. `if`, `match`, `else`, `?`, `map_err`, branch joins, loop joins, early exits, and malformed body paths remain dormant until am-b1 through am-b4; stored-body effect, return-provenance, and exact drop-expression facts are never read by this pass. The common sequence is depth/global/placement/nominal validation, then am-h header order, then (only after a successful header pass) am-b4 body replay. Thus an earlier graph/placement/nominal error wins over an am-h error; an am-h header error returns canonical-empty before any body validator can report; a valid header does not claim to accept malformed body metadata until am-b4 activates. | `valid_header_does_not_consume_body_facts` proves the deferred body boundary; the existing global/placement/nominal malformed-HIR suites prove earlier-preflight precedence, and the deferred am-b1–b4 owner names remain unchanged. |
| generic, interface, whole-program, and per-unit construction | Source declarations, generic declarations, monomorphs, imported declarations, and externs follow their existing producer paths with the new origin/effect/provenance-presence fields. Whole-program HIR contains no imported declarations; interface-only/per-unit HIR carries imported headers, presence bits, and normalized effects. All four public lowerers—`lower_program`, `lower_program_located`, `lower_program_per_unit`, and `lower_program_per_unit_located`—run the same validation order and return either the existing owned MIR or canonical all-empty MIR. | Generic/monomorph source twins, interface import twins, whole/per-unit and located/non-located malformed/valid matrices, and continuation codegen tests with unchanged output. |
| ownership, cleanup, and allocation | Validation uses compiler-only worklist/map allocation and borrows the input HIR; it adds no runtime allocation, FFI call, link action, source nulling, Drop, cleanup, or artifact side effect. The direct four-entrypoint observation is an all-empty MIR result with no body/codegen call. Accepted per-unit inputs reach codegen with the same six-field MIR record; `return_provenance_known`, `FnEffect`, and `FnOrigin` are Copy metadata. MIR strips the validation-only imported presence/effect fields and otherwise preserves ownership/cleanup records. Artifact/cache side-effect observation remains owned by the existing driver per-unit tests. | `valid_hir_declaration_header_preflight_is_mir_identity`, the four-entrypoint canonical-empty assertions in `malformed_hir_declaration_header_metadata_fails_closed`, existing per-unit artifact/cache tests, and the downstream Move/Copy/drop-set identity checks. |
| implementation consumers | Every former `lifted_capture_count` and `exportable` consumer is replaced or intentionally derived from `FnOrigin`: source construction, lambda construction, monomorph construction, return-provenance inference, open-world effect analysis, MIR exportability, and interface/per-unit lowering. No compatibility alias or parallel old/new path remains. | Repository search with zero stale field consumers; source/monomorph/lifted effect and return-provenance owners; per-unit export and whole-program internalization parity. |
| deep graph and malformed precedence | Header signatures, parameter types, and borrow/region summaries may form finite but depth-unbounded type DAGs. The prior global and placement walks are explicit and stack-bounded; am-h's summary check reuses the cycle-safe borrow classifier. A later imported sibling is rejected after the malformed header record, without publishing partial MIR. | `deep_hir_header_type_dag_is_stack_bounded` covers a valid 4,096-node imported signature and a malformed later sibling; `ty_may_borrow_is_cycle_safe_for_header_mediated_nominals` covers legal header-mediated cycles. |
| field-to-owner closure | The malformed matrix covers every am-h-owned extern field (name, duplicate/name, arity/modes, summaries), imported field (including duplicate/name, signature boundary, return summary/presence, effect transport, and `parallel_transfer_params` order/range/borrow capability), stored `Fn` field (name, duplicate/name, origin, params, modes, return placement delegated to am-p, return and parallel-transfer summaries, span, and drop sets), local id/name/parameter-bit/alignment, every main ABI/Error identity and shape discriminator, and the FnTy mode/summary records. Local type graph/placement validity remains am-g-t/am-p-owned; local mutability and expression ownership remain body-owned. Each am-h mutation is paired with canonical-empty/identity outcome and entrypoint parity. | `malformed_hir_declaration_header_metadata_fails_closed`, `main_header_abi_matrix_is_exhaustive`, `lifted_function_origin_metadata_is_explicit`, `non_entry_public_function_origin_is_exportable`, `imported_effect_facts_are_normalized_and_stripped`, imported/stored parallel-transfer mutation owners, `valid_header_does_not_consume_body_facts`, `deep_hir_header_type_dag_is_stack_bounded`, and `valid_hir_declaration_header_preflight_is_mir_identity` own the named submatrices; am-p owns the complete extern/import/local type-placement cells. |
| benchmark and regression boundary | Add the `mir-header-validation` benchmark row for valid and malformed header preflight. The benchmark measures compiler-side validation only and does not change runtime behavior or add a new persisted artifact field. The existing placement and nominal/link benchmark rows and all continuation rows remain unchanged. | `bench/library_boundary` header fixture and README row; benchmark build/run when `llvm-config-22` is available; focused owner suite plus unchanged downstream codegen/runtime rows. |
| review and atomic-closure gate | The matrix receives one fresh independent adversarial review before coding. The cell atomically contains the producer `FnOrigin`/import-effect migration, every sema consumer, the distinct MIR imported record, the common header validator, focused owner tests, and its local measurement row. The no-legacy/no-parallel-path rule forbids a producer-only intermediate that exposes an unvalidated imported effect or retains the overloaded origin pair. | Matrix-review log bound to the pre-implementation base; final capability-wave matrix-to-diff pass; focused owners, `scripts/test-pr.sh`, applicable Clippy, and one stable-candidate full-diff review/finding closure before pre-PR attestation. |

#### Am-f implementation closure matrix

This matrix is authoritative before the return-completeness producer correction begins.

| Cell | Required am-f closure | Exact owner evidence |
|---|---|---|
| formation and validation | A bare `return` is accepted exactly when the active function return is Unit; otherwise it emits exactly `return without a value is only valid in a function returning (); this function returns <type>` at the containing block span. After the checked body is built, a non-Unit block body with no tail is accepted only when the existing source-order control-flow analysis proves its end unreachable; otherwise it emits exactly `function returning <type> has a reachable path without a return value` at the body span. A present tail is checked against the declared return as today. These checks run after return-type formation but before HIR publication; an ill-typed present value retains the existing type diagnostic before completion. | Unit/non-Unit × bare/value return × present/absent tail; wrong value plus missing path diagnostic order; exact spans/messages |
| construction and control | Every reachable path of a non-Unit function ends in `Return(Some(value))`, a typed tail converted by existing lowering, `Try` error propagation, a direct process completion expression or non-fallthrough process statement, or a proven diverging loop/control expression. ProcessExit/ProcessAbort remain exact Unit HIR and do not become a general `Never` coercion through eager value relations. If/Match/Else join only reachable fallthroughs; loop exits use accepted Break paths; nested blocks, Arena, Unsafe, TaskGroup, and lifted lambda bodies retain their own active function return. Dead statements are still structurally checked but cannot create a fallthrough edge. The function root HIR body has an absent reachable tail only for Unit. | straight-line, if/match/else, loop/break, Try, direct and statement process exit/abort completion, rejected named/generic/binary `Never`-coercion attempts, nested region/unsafe/task group, lifted lambda, and dead-code mutations |
| ownership, Drop, and allocation | Rejected functions publish no HIR/MIR, allocation, move, Drop, cleanup, or cacheable artifact. Accepted typed-return and non-fallthrough paths keep existing return move/nulling, arena/task cleanup, and allocation behavior. | Copy/Move `function_return_completeness_matrix` returns plus the cumulative `owned_structs`, `tuples`, and `bench/owned_tagged_payload` move/null/allocation-count owners; `m11_process::{exit_flushes_pending_buffered_writer_output,abort_skips_cleanup_and_loses_buffered_output,exit_inside_arena_runs_the_pending_arena_end}` cleanup twins; rejected-before-interface/cache assertion |
| generic, interface, whole/per-unit, and cache | Generic source bodies and each monomorph are checked under their concrete return; imported/extern bodyless declarations are unaffected. Whole/per-unit compilation applies the same local-body rule. Accepted interface/source/MIR/impl hashes are unchanged; the compiler-build change alone invalidates cached objects. | generic/monomorph return twins, interface goldens, whole/per-unit and cache miss-then-hit |
| native/ABI and benchmark | No non-Unit function can reach MIR/LLVM with `Return(None)` or a reachable absent tail, so LLVM never emits `ret void` under a value-returning signature. The checks are linear in the already-bounded body walk and add no runtime work. | exact MIR terminators; raw/optimized LLVM verification; whole/per-unit runtime results for typed/if/match/else/Try/region/Move families; `m11_process` runtime exit/abort/cleanup rows; unreachable terminators for non-terminating loops; compile-time no-regression owner |

#### Am-e/am-w implementation closure matrix

This matrix is authoritative before either producer correction begins. Both corrections precede
am-h/am-b4 so total HIR validation accepts exactly the corrected semantic producer. Its placement
before the am-d matrix is topical, not sequential: am-d implements the depth/type preflight first,
and every later producer/body pass enters only after that guard.

| Cell | Am-e entry closure | Am-w task-wait closure | Exact owner evidence |
|---|---|---|---|
| formation and validation | Source `main` is non-generic and has either no parameters or exact `ByValue array<str>`. No-arg return is Unit, exact signed i32, or `Result<Unit,builtin Error>`; argv requires that Result. After ordinary type formation, main return checks run before the parameter-shape check. An otherwise-valid non-Result return outside the admitted set emits exactly `main returns only (), i32, or Result<(), Error>; got <type>` at the return span. Existing wrong-Result Ok/Error diagnostics retain their order, followed by the existing argv/parameter diagnostic on a multi-invalid declaration. | Each active group owns compiler-only `group`, abstract `current_generation`/`proof_epoch` tokens, optional `completed_generation`, `valid_from`, and a sparse ordered unresolved-Wait set. Every Spawn advances to stable syntax-site/incoming-state tokens, staling all old WaitProofs; completion then differs from current. With an unresolved Wait it also clears that set and advances `valid_from` to invalidate old Tasks; otherwise old Tasks remain eligible for reauthorization. A fallible Wait produces `WaitProof { group, proof_epoch, wait, covers_through }`; Spawn produces `TaskProof { group, born_generation }`. Ok resolves only its id and sets completion to its covered generation only after every earlier id in that epoch is Ok. Err advances proof epoch, clears completion, and advances `valid_from`, poisoning all covered Tasks/Waits. Infallible Wait completes the current generation. A later no-task Wait does not revoke completion. TaskGet requires `born_generation >= valid_from` and `completed_generation == Some(current_generation)`. | all accepted signatures plus every other graph-valid return/parameter mutation and multi-invalid diagnostic order; straight-line infallible/fallible/reset/generation-join, stable token reuse, stale resolved-Wait alias after Spawn, unresolved-first Wait plus successful empty second Wait, first successful Wait plus later unhandled empty Wait, failed-first Wait plus `wait()?` on an empty queue, missing/wrong/stale TaskProof, and nested-group mutations |
| construction and control | Exact i32 emits external `i32 @main()` directly. Unit/Result emit an internal Align body plus an external i32 wrapper; argv wrapper alone accepts `(i32,ptr)`. No other entry ABI is constructible. | A fallible-Wait Result carries exact group/proof-epoch/wait/coverage provenance. Bare local binding, copy/reassignment, block tail, `ResultMapErr`, and value-producing if/match/else/loop preserve it only when every reachable result predecessor has the same proof; unrelated overwrite clears it. Move Task handles transfer `TaskProof { group, born_generation }` through transparent local binding, move/reassignment, block tail, and value-producing control flow; there is no Task-copy path. Try, Result Match, and Result Else resolve or poison the exact still-active proof even while a nested group is active. Multiple aliases resolve idempotently; a stale proof has no effect. Passing a Copy Wait proof leaves the caller local intact but no opaque boundary inherits either proof. Every Spawn/Wait/Err transfer interns its token by syntax site plus incoming group tokens. A byte-identical group-state join retains state verbatim; every differing join reuses its one syntactic-site `join_generation`/`join_proof_epoch`, assigns them to the current generation/proof epoch, clears WaitProofs/unresolved state, sets completion true iff all predecessors completed their current generation, and remaps each Task-valued local/result to the join generation iff every predecessor has a valid same-group Task proof and either completed its current generation or has no unresolved Wait covering the Task; predecessor handles may differ. A loop joins entry with reachable body fallthrough at its stable header site to a byte-identical fixed point, then computes its exit only from accepted breaks. Thus a post-join Wait registers under the join proof epoch, TaskGet checks the join generation, an earlier-iteration unresolved/failed Wait reaches later breaks, completed Spawn+Wait/no-Spawn joins remain readable, incomplete joins require a later Wait, and a drained unresolved Wait cannot hide. A later no-task Wait Result left unhandled does not revoke already-established completion, including across branch and loop joins. Entering a nested group retains outer state; exit removes all proofs naming the inner group. TaskGet checks its originating group/generation bounds, never the innermost position. Return, Try Err, process termination, and a diverging loop have no continuation. Lambdas/functions start empty. | whole/per-unit raw/optimized LLVM exact signatures and link/run exit behavior; direct and stored/copied/reassigned/map_err Wait proof; transparent Task move/reassignment/control proof and rejected Task-copy premise; completed and incomplete asymmetric Spawn/Wait/no-Spawn joins followed by Wait and TaskGet; branch-selected distinct Task handles; unresolved first Wait plus second empty Ok remains unreadable; first Ok then later unhandled empty Wait remains readable straight-line and across branch/loop joins; first Err invalidates the task and every second proof; multi-iteration loop with first-iteration unresolved/failed Wait, later break, post-loop empty Wait, and rejected get; all-success multi-iteration acceptance; stable-token fixed-point convergence; Spawn with an unresolved Wait invalidates the old task generation; successful Wait→Spawn→successful Wait authorizes old and new handles; unrelated overwrite and opaque-boundary nontransport; nested-group, originating-diagnostic, exact/wildcard/or Result arm, terminating Else fallback, branch/loop/early-exit/lambda matrix |
| stable identity and replay order | Task-group, Spawn, Wait, join, Err, and loop-header identities are compiler-owned preorder `NodeId`s assigned once from the explicit checked-HIR body event stream. They never use `Span`, so distinct records with the same span remain distinct. The analysis replays the same source-order child/statement/arm order on every branch and fixed-point iteration; diagnostic deduplication uses the structural TaskGet `NodeId` while the emitted location remains the original source span. | `task_wait_duplicate_span_identity` proves source-order Wait ids and tokens differ; `task_wait_duplicate_span_all_identity_kinds` covers duplicate-span group, Spawn, Err, join, and loop-header token sites and compares each loop-header site to its structural id; `task_wait_duplicate_span_gets_report_separately` proves distinct invalid TaskGet nodes at one span both report; source-order and duplicate-span mutation pairs retain the first-invalid precedence. |
| bounded frames and malformed safety | The replay uses explicit enter/exit work items for blocks, statements, expressions, branches, and loop iterations. The work stack is bounded by the `MAX_CHECKED_HIR_DEPTH`-derived frame limit, and dispatcher work-item steps are bounded by checked-HIR record count × that depth-derived step limit (with one minimum budget for an empty body). Retained sibling vectors and semantic-state clone/scan costs remain bounded by the checked-HIR record count and live proof entries, but are not claimed to be constant-factor total CPU. Token exhaustion, an exhausted dispatcher budget, or a structurally unindexed body fails closed before a proof can authorize `TaskGet`. No raw pointer is used to reset semantic state and no user-derived index is unchecked. | `task_wait_empty_body_has_replay_budget` covers the zero-record root; `task_wait_depth_is_stack_bounded` accepts checked-HIR depth 259 on a 2 MiB stack and rejects depth 260 before replay; `task_wait_missing_node_fails_closed` proves an unindexed operation returns no flow and no proof without a panic; `task_wait_token_exhaustion_fails_closed` proves token allocation cannot alias after `u32::MAX`. |
| loop fixed point | Loop headers start from the entry state and are recomputed until the complete canonical semantic state is equal. Stable token interning makes revisits reuse the same transfer/join ids; a bounded worklist guard is derived from the checked-HIR ceiling rather than a fixed visit count, and exhaustion is a fail-closed validation error. Only reachable fallthroughs return to the header; only accepted breaks join the exit. | `task_wait_loop_fixed_point_guard_is_depth_derived` pins the guard to the checked-HIR ceiling, observes distinct incoming tokens for the state-changing loop Spawn, and checks the loop-header join site; `task_wait_loop_unresolved_wait_reaches_later_break` proves a pending/failed earlier iteration cannot be hidden by a later empty Wait. |
| review and atomic-closure gate | The entry producer remains the existing am-e vertical. | Keep stable NodeId identity, explicit work items, and fixed-point replay in one atomic cell inside the capability wave: a NodeId-only intermediate leaves recursive overflow, a worklist-only intermediate leaves duplicate-Span aliasing, and a fixed-point-only intermediate leaves identity/order unsound. No temporary parallel proof path is permitted; the historical size estimate is not a split trigger. | One author matrix-to-diff pass for the cell, one stable-candidate adversarial review/finding closure for the complete wave, focused owner checks, and the single pre-PR attestation. |
| state equality terminology | In the am-w rows, `byte-identical` means equality of the complete canonical semantic `State` record, including ordered group fields and the full local proof maps; it does not claim a serialized byte fingerprint or depend on `HashMap` iteration order. | The implementation uses complete `State` equality, whose `HashMap` fields compare mappings semantically. |
| ownership, Drop, and allocation | Rejected headers construct no HIR/MIR/runtime state. Accepted Unit/i32/Result paths keep existing body ownership and wrapper allocation behavior. | The ambient, WaitProof, and TaskProof maps are compiler-only state. Propagating or clearing them follows existing Copy Result and Move Task evaluation but creates no task, join, read, Align allocation, source nulling, Drop, or cleanup action. Current Task results are primitive Copy values: `TaskGet` is a non-consuming read, preserves the Move handle and its TaskProof, and repeated get is producer-valid. Group cleanup remains byte-identical on accepted input. Owned Task results and their consuming-get/Drop contract remain a separate future slice. | rejected-before-MIR tests; Copy Wait and Move Task local/control bookkeeping twins; repeated primitive get; no source-nulling/Drop/allocation change and MIR equality |
| generic, interface, whole/per-unit, and cache | `main` remains entry-unit-only and non-generic. Interface summaries never export it. Whole/per-unit and ThinLTO preserve exactly external `main`; the compiler-build change invalidates old cached objects, while source/interface hashes for accepted input stay unchanged. | No interface or ABI field is added. Whole/per-unit semantic HIR agrees. Accepted-source MIR/impl hashes stay unchanged; rejected unsafe sources produce no artifact. | interface-byte/hash goldens for accepted entries; ThinLTO off/on; cache miss once then hit; whole/per-unit task twins |
| benchmark | No persistent validation pass is added; one constant-time header predicate replaces invalid backend construction. | Generation replacement clears the active sparse wait/proof entries in O(live wait/proof entries). Stable site-token interning is amortized O(1). The loop worklist is monotone: each header completion or local proof fact can be cleared once, and every differing header state canonicalizes to its one site token. Wait resolution and joins therefore examine only sparse live unresolved/proof entries in O(control edges × live proof entries examined); there is no runtime work. | compile-time task-count × unresolved-Wait × branch-count × loop-backedge-count × live-proof-alias scale owner, explicit stable-token convergence bound, plus no regression in existing `mir-header-validation` and task-group rows; no new runtime benchmark |

#### Am-v implementation closure matrix

This matrix is authoritative before the native output-buffer producer correction begins. Am-v
implements the `mut buffer` surface already promised by the language and library documentation; it
does not add a new source construct.

| Cell | Required am-v closure | Exact owner evidence |
|---|---|---|
| formation and validation | After each operation's existing receiver, arity, child-expression, and exact `Buffer` type checks, `ReaderRead.buffer`, `ReaderReadLine.buffer`, `FilePread.buffer`, `UdpRecvFrom.buffer`, and `CryptoRandom.out` must be a bare source local with `is_mut == true`. A non-local reports exactly `'<operation>' needs a mut buffer local (bind it first, then pass that local)`; an immutable local reports exactly `cannot fill immutable buffer '<name>' in '<operation>' (declare with mut)`. The operation labels are `.read()`, `.read_line()`, `.pread()`, `.recv_from()`, and `crypto.random`. Existing earlier diagnostics retain precedence on a multi-invalid call. | five operations × wrong type/non-local/immutable/valid local, plus receiver/arity-and-place multi-invalid order |
| construction, move, and control | Rejection constructs no native HIR row. An accepted output local is borrowed and mutated in place; it is neither moved nor nulled and retains its existing Drop. If/match/else/loop/early-exit behavior is ordinary expression behavior and no new compiler state joins. | HIR absence on reject; accepted HIR equality apart from source id; post-call reuse and exactly-once Drop across every control family |
| ownership, lifetime, allocation, and FFI | The output buffer owns its existing allocation for the whole call and the runtime writes only through that stable local's buffer window. Am-v creates no allocation and changes no native key, C ABI, runtime allocation provenance, success/error result, or handle ownership. | five runtime success/error twins, allocation-count parity, unchanged 281 keyed native declarations and feature export sets |
| generic, interface, whole/per-unit, and cache | `Buffer` is concrete and non-generic. No HIR/MIR/interface field or codec changes. Whole/per-unit compilation accepts and rejects the same sources; accepted MIR/impl/interface hashes remain byte-identical. The compiler-build change alone invalidates old cached objects. | whole/per-unit accepted/rejected twins, interface/hash goldens, one build-id cache miss then hit |
| benchmark | Five constant-time AST-local/mutability predicates run inside existing semantic checks; no persistent pass or runtime work is added. | compile-time no-regression owner; no new runtime benchmark |

#### Am-u implementation closure matrix

This matrix is authoritative for the shipped extern-call producer correction (2026-08-01).
`unsafe` remains a lexical invocation permission; am-u does not add an unsafe-callable function
type.

| Cell | Required am-u closure | Exact owner evidence |
|---|---|---|
| formation and validation | `check_named_fn_value` rejects `FnSig.is_extern` before scalar-signature eligibility with exactly `extern function '<source name>' cannot be used as a function value; call it directly inside unsafe`. `check_named_call`, `resolve_stage_fn`, and `resolve_fn` accept an extern only when the owning Call, stage pipeline, reducer/partition/any/all/par_map/scan, or sort-by-key expression is at `unsafe_depth > 0`; otherwise they emit exactly `calling extern function '<source name>' requires an unsafe { } block`. `named_sig`, `named_param_hint`, and `resolve_named_fn_quiet` remain inference-only and never authorize or diagnose. Lookup/undefined, callable shape, and argument diagnostics retain their existing order around that one permission check. | bare/qualified direct calls; every stage and terminal resolver consumer; extern FnValue in/outside Unsafe; undefined/shape/permission multi-invalid pairs; exact one-diagnostic owner |
| construction and control | Accepted direct extern HIR remains `Call`; accepted named higher-order HIR keeps its current target name at the non-escaping owning expression. Am-b1 requires lexical Unsafe for `Call` when `SIG(func).is_extern`; am-b2 requires it for every stored extern stage/terminal target. An extern never produces `FnValue`, `Closure`, `CallFnValue`, stored aggregate callable, return, capture, or task environment. Unsafe block entry/exit, branch, loop, early exit, and nested function/lambda bodies use their own lexical depth; permission never flows as data. | HIR presence/absence, direct and each callback family inside/outside/nested Unsafe, branch/loop/lambda scope matrix |
| ownership, lifetime, allocation, and FFI | Permission is compiler-only lexical state. It creates no Align value, ownership transfer, Drop, allocation, cleanup, or provenance. Accepted foreign calls preserve their existing argument modes, FFI coercions, native allocation behavior, external declaration ABI, and link libraries. | accepted HIR/MIR/runtime equality, view/raw/layout(C) ABI owners, allocation and link parity |
| generic, interface, whole/per-unit, and cache | Extern declarations remain bodyless, non-generic interface signatures. The source/interface codecs and hashes do not gain a permission field. Whole/per-unit compilation enforces the same lexical rule. Accepted MIR/impl/interface bytes are unchanged; the compiler-build change alone invalidates cached objects. | interface/hash goldens, whole/per-unit accepted/rejected twins, one build-id cache miss then hit |
| benchmark | Permission checks are constant-time predicates inside existing callable resolution and HIR validation; no persistent pass or runtime work is added. | compile-time no-regression owner; no new runtime benchmark |

#### Am-d implementation closure matrix

This matrix is authoritative before the stack-safety closure begins. The checked-HIR record
ceiling is fixed by the exhaustive conservative producer proof in
[`19-hir-validation-ledger.md`](19-hir-validation-ledger.md). It is separate from the global type
domain: am-g-t deliberately accepts every finite header-mediated nominal/tagged/function DAG, so
am-d must not impose an ambient or raw type-depth cap.

| Cell | Required am-d closure | Exact owner evidence |
|---|---|---|
| formation and validation | The constructor inventory classifies every AST-to-HIR creation site and charges principal records, structural helpers, non-stacking `StrBorrow`/`ArrayToSlice` wrappers, bounded leaf-only expansions, parser recursion guards, and the synthetic expression-function root to derive the fixed conservative producer ceiling of 259. It makes no unsupported claim that 259 is the minimum or source-reachable maximum. A common explicit enter/exit worklist measures a handcrafted HIR body before any recursive semantic consumer runs. Record depth 260 rejects to canonical-empty in all four lowering entrypoints; every depth-259 body continues. Separately, am-d factors the am-g-t type visitor into one common explicit-worklist graph traversal with stable root, field, element, parameter, return, and reference order, visit colors, and no finite valid depth limit. | complete constructor-site exhaustiveness assertion; raw, coercion-expanded, nested Block/Stmt/MatchArm/Stage/TemplatePart 258/259/260 fixtures, including diagnosed HIR before finalization; deep valid nominal/tagged/function DAG and deep malformed missing-reference/wrong-kind sibling-precedence twins |
| construction and control | Producer HIR finalization, lint, region, borrow, Move, Escape, effect, and am-b4 replay walks use explicit enter/exit work items. Strict-child divergence propagates through transparent stage/template records before later siblings are scheduled, while conditional branches remain alternatives and `process.exit`/`process.abort` remain non-fallthrough leaves. MIR strict eager spines whose giant dispatcher frame cannot safely recurse use one heterogeneous child-first worklist from every whole/per-unit located/unlocated entrypoint. Multi-child parents retain the immediate child→owner/action→next-child protocol. Structured `if`, `match`, `else`, short-circuit, loop, arena/task-group, block, and template lowering and the existing specialized file, reader, array-builder, process-command, path, regex, and HTTP groups use out-of-line helpers reached before the giant dispatcher frame is retained. Native helper depth is permitted only because the checked-HIR ceiling bounds it at 259; accepted-boundary owners must prove each recursive family on the ordinary 2 MiB stack. Every accepted-boundary fixture is itself producer-valid: its return cannot outlive a hidden or synthetic owner, every owned temporary has the same individual-allocation fact sema would publish, every Move parameter has the corresponding Drop metadata, and exact Result Ok/Error payloads agree through construction, inspection, and unwrap destinations. These mechanisms preserve the ledger's envelope-before-child, child source order, post-relation, result-type, and body-fact order across `if`, `match`, `else`, `?`, `map_err`, branch and loop joins, early exits, dead retained nodes, stages, templates, strict string wrappers, and specialized operation groups. No rejected body starts a later phase. The am-d inventory also enumerates every recursive type edge reachable in these phases and in codegen; each edge is migrated to the common worklist or classified with an owner-backed non-recursive/indirection-leaf proof. | exact first-error and reachable-state identity against shallow twins for every control family, including transitively diverging first template holes and stage captures followed by unreachable impure siblings, plus process-termination branch joins; complete recursive-call-site inventory classifying eager-worklist, bounded out-of-line control/specialized operation, immediate required-child, and tail edges; all four entrypoints on the 2 MiB test thread with operation-specific MIR assertions for mixed unary/cast/binary/call, string trim, alternating `str.borrow`/path components/normalization/join ending in owned `string` with per-expression individual-allocation facts, alternating `str.borrow`/regex replacement ending in owned `string` with per-expression individual-allocation facts, self-buffering reader, a producer-valid `Result<str, Error>` `StrBytes`/`BytesAsStr`/`Try` cycle, template spines whose hidden views are cloned to an owned return, file create over a deep path, array-builder push over a deep value, process-command construction over a deep command, HTTP request construction over a deep method, block/statement sentinel reachability, proportional `if` and wildcard/binary `match` CFG evidence, independently counted `else` and short-circuit branches, loop, and arena/task-group roots; exact Result Ok/Error operand, construction destination, `IsOk` bool destination, unwrap destination, and builtin Error declaration checks; final MIR and raw/optimized LLVM verification at the accepted boundary. The producer-validity and exact-result requirements are one inseparable test-closure correction rather than a new implementation slice: they change no compiler path and the boundary fixture cannot prove either invariant independently. |
| ownership, Drop, allocation, and return | Iterative replay preserves construction, move-in/out, source nulling, Drop, replacement, return, loop cleanup, and allocation order exactly. `drop_plan_rec`, recursively Move classification, borrow capability, region/escape, and ownership predicates accept deep acyclic inline/header-mediated graphs without process-stack recursion. An over-bound body returns canonical-empty before any Align-program/runtime/native/artifact/cache allocation, ownership action, native registration, source-map read, or cache publication. Only compiler-owned validation worklist allocation may occur; it is released on success or rejection. | deep Copy/Move local/return/aggregate and Drop-plan roots; deep borrow/region/escape roots; replacement, branch/loop/early-return, arena/task cleanup, allocation-count, and canonical-empty no-action twins |
| generic, interface, whole/per-unit, and cache | Every stored, monomorphized, and lifted function is an independent body-depth root. Whole/per-unit compilation uses the same body preflight. Interfaces and canonical type fingerprints retain the am-g-t finite type domain without a new depth field or limit. MIR type conversion and LLVM struct body/layout helpers accept deep graphs at stored header, parameter, local, return, and aggregate-field roots through raw and optimized verification. Accepted source/HIR/MIR/interface/impl hashes remain unchanged and only the compiler build id invalidates cached objects. | source/monomorph/lifted boundary fixtures, two-unit deep type-DAG import, deep MIR/LLVM parameter/local/return/field layout and executable twins, interface/hash goldens, and one build-id miss then hit |
| current/future type consumers and benchmark | Before merge, am-d closes every currently active HIR→MIR→LLVM recursive type consumer, including `drop_plan_rec`, `ty_is_move`/`struct_is_move`, `ty_may_borrow`, slice/region/escape/ownership predicates, type/layout lowering, and LLVM struct-body/layout construction. Am-p placement, am-n complete source-shape comparison, am-h signature/summary correlation, am-b1–b4 body type relations, and am-c canonical encode/decode then inherit the common traversal. Each owning slice has a deep valid acyclic-inline and header-mediated DAG plus a deep malformed later-sibling case proving diagnostic precedence. Am-c canonical semantic-to-bytes and bytes-to-semantic traversal is depth-first first-visit order implemented with explicit work items; malformed deep references and truncation reject without using the process stack. Ordinary body/type and c2a3 raw-validation traversals are linear in visited records/edges. C2a2a owns the comparator semantics without a new complexity claim. For one complete c2a2b-observed first-representative registration sequence and shared completed cache, `V/E` count distinct observed raw nodes and their edges, `P` counts distinct compared pairs including cache-free restarts, and `Q` counts every fixed comparison, sequence element, summary element, and compared text byte. Comparison takes expected-amortized `O(V + E + P + Q)` time and transient space. Both `P` and `Q` can exceed the raw input size, so this is deliberately not an input-linear promise. C2a4 refinement takes at most `R <= A + 1` rounds for `A` reachable anonymous nodes and is `O(R * ((V + E) log V))`, including repeated lexicographic comparison of variable-length signatures. Compiler-owned worklists are bounded by those explicit measures, and MIR structured-control native frames are bounded only by the fixed checked-HIR ceiling and the 2 MiB owner proof rather than by an ambient process-stack assumption. | am-d `deep_type_consumer_closure_matrix` across sema/MIR/codegen/driver roots; cumulative deep-DAG owner in am-p/am-n/am-h/am-b1–b4/am-c; c2a2b pair/scalar-work adversary and compiler-only `canonical-source-shape-comparison` benchmark; c2a4 refinement-round and wide/common-prefix signature-sort adversaries, compiler-only `canonical-type-graph` benchmark, deep semantic→bytes→semantic golden, and malformed deep-reference/truncation twins; unchanged `mir-global-type-validation`, later validation rows, and `mir-continuation-lowering` |

#### Am-h/am-b4/am-c implementation closure matrix

This matrix is authoritative before either internal representation change begins. The am-b
ownership/control matrix is the per-record ledger in
[`19-hir-validation-ledger.md`](19-hir-validation-ledger.md); am-c consumes it only after am-b4.

| Cell | Required implementation closure | Exact owner evidence |
|---|---|---|
| formation and validation | Am-h forms exactly one `FnOrigin` for every stored function, derives exportability only through `is_exportable()`, and carries normalized `FnEffect` plus canonical interface-v6 `parallel_transfer_params` on every imported HIR declaration before converting to the unchanged six-field MIR declaration after header validation. Am-b4 replays stored-body/cross-unit effect and parallel-transfer inference before am-c can consume a callable fact. Am-c forms `ProgramCall`, `RuntimeKey`, `CanonicalFnAbi`, `CanonicalTy`, and each `GeneratedId` only after am-b4 validation; canonical decoders reject before registry/cache publication. All nested type formation/comparison/canonicalization uses am-d's common explicit-worklist traversal and accepts every finite am-g-t-valid header-mediated DAG. | every origin/flag/count mutation; imported Pure/Impure/Unknown/absent-normalization and empty/nonempty/compatibility-absent transfer twins; effect/transfer-stripped MIR Debug and impl-hash identity; interface-v6 byte/hash goldens; every stored/projection/join effect-cell and parallel-transfer mutation plus parallel eligibility twin; every canonical tag/width/reference/order mutation; shallow and deep semantic↔byte goldens; deep malformed reference/truncation rejection without process-stack recursion |
| construction | Source declarations record entry/public flags, monomorph worklist outputs record `Monomorph`, every lifted lambda records its exact `u32` capture count, and interface-only declarations copy their exact external effect plus authenticated transfer roots, normalizing compatibility omissions conservatively. Direct calls, function addresses, closures, tasks, and all four parallel kernel modes construct the exact typed target/identity at their current single construction sites. | private/public entry/non-entry, mono, zero/positive capture, imported effect/transfer states; direct/native/fn-value/closure/task/materialize/reduce/count/scatter construction owners |
| move-in, move-out, source nulling, Drop, replacement, and return | `FnOrigin`, `RuntimeKey`, and kernel modes are Copy compiler metadata; boxed calls, canonical records, and generated ids use ordinary Rust ownership. They introduce no Align value, source nulling, Drop plan, replacement, return cleanup bit, runtime allocation, or allocation provenance. Existing callable operands/captures retain the am-b4-proved Move/Drop behavior byte-for-byte. | MIR equality excluding the typed metadata field, existing closure/task/parallel Drop and allocation-count owners, and explicit N/A assertions for new metadata |
| body and control paths | HIR `Call`, `FnValue`, `Closure`, `Spawn`, every stage/terminal callable, `ResultMapErr`, and indirect-call signature/effect correlation are validated before conversion. `if`, `match`, `else`, `?`, `map_err`, loop/branch joins, early exits, and malformed input never create or publish a typed target or effect join after a rejected child/body. | all corresponding am-b owner ids, stored/local/projection/result effect-cell mutations, Pure/Unknown/Impure parallel twins, plus malformed-before-registry/cache tests and canonical-empty four-entrypoint parity |
| generic and interface | Concrete generic instances record `Monomorph`; generic templates remain discarded. Imported declarations carry interface effect and v6 transfer-root facts only inside checked HIR, convert to the effect/transfer-free six-field MIR declaration, and later convert to `ProgramCall` using their exact producer identity. Interface serialization/hash and source ABI fingerprints still omit `FnOrigin`, `DirectCall`, and `GeneratedId`; format v6 intentionally adds only canonical `parallel_transfer_params`, changing the interface hash whenever those roots change. | generic source/mono name-equality twins, two-unit import/call/link, imported effect/transfer parity, effect/transfer-stripped MIR/impl-hash identity, interface-v6 unchanged-root and changed-root byte/hash goldens |
| whole-program and per-unit | Whole-program lowering derives internal linkage; per-unit lowering derives external linkage only for `Source { is_entry: false, is_public: true }`. Producer definitions and consumer imports encode the same program identity; direct/wrapped main and explicit exports follow the collision matrix. Before rt-LTO linking, codegen requires every guarded logical symbol to have its exact registry function type and a body; any missing, declaration-only, or wrong-type row loudly falls back and re-curates all guarded declarations without merging. It then renames every valid incoming guarded definition from its logical runtime symbol to the typed declaration's captured physical LLVM name, so a preceding same-spelled program/import claimant cannot receive or conflict with the runtime body. | whole/per-unit MIR, LLVM, executable, export, main, guarded artifact completeness/type/body/physical-collision, and ThinLTO off/on parity |
| native/FFI and allocation parity | The compiler registry is the fixed 286-row base surface. The eight probe rows are verification-only runtime exports: never RuntimeKeys, callable declarations, collision reservations, compatible-extern reuse targets, compiler inputs, or cache identity. Probe-feature runtime fixtures never link user artifacts. A source extern is compatible with a base row exactly when its source-derived LLVM function type equals the row type; it supplies no curated attributes. The reused native row then supplies all return/parameter/function attributes and rt-LTO policy. A type-incompatible fixed-base external claim rejects before LLVM; attribute mutations are registry/runtime-fixture self-validation failures, not source-extern mismatch inputs. Native calls, runtime ownership provenance, and success/error allocation counts are unchanged. | base/`alloc-count`/`par-map-probe`/all-feature bidirectional export sets, fixed 281 declaration ABI rows, base compatible/incompatible-type extern twins, compatible attributed-row reuse, every registry attribute mutation, eight ordinary probe-spelling extern/export positives under a normal runtime, cumulative native allocation owners |
| cache and monomorphization | Typed target bytes participate in structural MIR `impl_hash`; the compiler build changes `compiler_build_id`. Monomorph keys remain unchanged. Interface-v6 transfer roots participate in `interface_hash` and dependent unit/object cache keys, while their validated stripping leaves MIR `impl_hash` unchanged for otherwise identical accepted MIR. No generated lookup uses a printed stem or raw table id. | transfer-root interface/dependent-cache miss then unchanged hit, MIR impl-hash identity after stripping, monomorph identity twins, generated collision/probe matrix |
| benchmark | Am-h and am-c retain linear validation/registry construction and do no artifact or runtime allocation during validation. Canonical encoding and decoding preserve depth-first first-visit bytes through explicit enter/exit work items rather than native recursion. | `mir-header-validation`, `mir-callable-namespace-validation`, deep semantic→bytes→semantic and malformed-decode rows, unchanged runtime-call and continuation rows |

#### Am-c1 implementation closure matrix

Am-c1 is the next implementation checkpoint. It consumes the already-authoritative 281-key/286-base
row ABI ledger in [`20-runtime-abi-ledger.md`](20-runtime-abi-ledger.md) and does not depend on the
dormant c2a1–c2d prerequisites or c3 MIR activation.

| Cell | Required am-c1 closure | Exact owner evidence |
|---|---|---|
| formation and validation | One semantic declaration in `align_mir` forms every `RuntimeKey` variant, its exact logical spelling, and `RuntimeKey::ALL`; compile-time cardinality pins 281. One wildcard-free exhaustive `match RuntimeKey` in `align_codegen_llvm` supplies exactly one backend-private `RuntimeAbi` row per variant, so adding a semantic key without a physical row fails compilation without placing LLVM facts in MIR. Five explicit unkeyed rows make 286 fixed base rows. Registry preflight proves key and symbol uniqueness and the three exceptional key-to-symbol mappings. No environment, target, runtime feature, linked artifact, or MIR string changes membership. | every key/logical spelling/symbol/type/return/parameter/attribute/rt-LTO-policy mutation; missing keyed row compile failure; duplicate key/symbol and base-cardinality test failures; normalized compiled-Rust signature parity for all 286 rows; exact default/feature export-set comparison |
| declaration and legacy string seams | Registry and extern-compatibility preflight finish before LLVM construction but create no declaration. C1 fixes the post-change LLVM construction order exactly: stored definitions in vector order; non-shadowed imports in vector order; externs in vector order; keyed native rows in alphabetical `RuntimeKey::ALL` order; then existing generated helpers in their existing order. This intentionally normalizes only the relative keyed-native declaration order; all keyed physical symbols are unique, and every program/import/extern claimant still precedes them. A compatible extern claiming a fixed native physical symbol creates/reuses that exact row declaration during the extern phase; the later keyed row reuses the same handle. A same-physical-name stored definition/import is not compatible native reuse: the later native row retains current program-before-native LLVM uniquification behavior until c3 encodes program symbols. Main-wrapper emission later reuses a compatible `ReportError` extern or adds its row, and adds the wrapper-only `ArgsBuild` row when argv marshalling is required; no source-valid extern return lowers to `ArgsBuild`'s native `{ptr, i64}` view. A colliding stored/imported program symbol retains program-before-wrapper uniquification. Curated attributes attach through the selected typed row handle, never by scanning a symbol prefix that could hit the program claimant; this intentionally removes the current accidental native attributes from a program claimant and applies them to the possibly-suffixed native declaration. Separately, c1 does not classify generic old `Rvalue::Call(String, ..)`: the temporary mixed legacy map preserves the same stored/import/extern/keyed-runtime/generated alias insertion and overwrite behavior, with keyed aliases now inserted in `RuntimeKey::ALL` order. Because keyed aliases are mutually unique, their relative order changes no final binding. The map remains the string-indexed owner for generic direct calls, FnAddr/closure targets, generated helpers, and program/parallel signatures; only dedicated runtime consumers leave it in c1. C3 deletes all these program/generated/direct seams. The exact compiler-produced direct-runtime semantic set is 15 keys. Eight are already specialized codegen branches and become typed dedicated choices in c1: `Print`, `PrintStr`, `PrintBool`, `PrintChar`, `PrintF32`, `PrintF64`, `Hash64`, and `Hash128`. Seven still traverse the generic legacy direct seam: `ProcessExit`, `ProcessAbort`, `DivFail`, `BoundsFail`, `RangeFail`, `Utf8BoundaryFail`, and `LenMismatchFail`. `AllocSizeFail` is a separate dedicated consumer and `error(code)` is the existing non-runtime identity case. | exact post-c1 declaration/reuse order and one checked pre-c1→post-c1 native-order golden; stored/imported physical-native-symbol collision keeps spelling/program-before-native uniquification while program/native attribute ownership becomes correct; compatible keyed extern+builtin, four source-reachable unkeyed rows, and exact wrapper-only `ArgsBuild` source rejection; exact 281 legacy-alias insertions and final-binding parity; each eight-key specialized typed choice and seven-key legacy generic direct call; every program-name/runtime-alias collision preserves baseline resolution/result; unknown/error cases; declaration golden |
| dedicated native consumers and control | Every specialized MIR native/drop/allocation/cleanup path outside the seven-key generic legacy direct seam indexes the typed keyed runtime function registry with a `RuntimeKey`, including dynamic choices represented as typed key matches. This explicitly includes the six `print` type choices and two `hash` width choices. When a main wrapper needs them, it obtains `align_rt_report_error` and conditional `align_rt_args_build` only through `UnkeyedRuntimeKey::{ReportError, ArgsBuild}` handles formed from their two declaration-policy rows. The other three unkeyed rows create no unconditional dedicated handle; an exact compatible extern may still cause ordinary declaration/reuse through its row. Every success, error, early exit, branch/loop cleanup, arena/task cleanup, and main-wrapper path preserves call order and result handling. No dedicated consumer indexes a runtime function by `&str`; an exhaustive source assertion inventories the keyed and two unkeyed consumers separately. | exact keyed consumer inventory including six print/two hash choice rows; exact two-wrapper-handle inventory and three-unkeyed-no-unconditional-declaration negatives; compatible-extern positives for the four source-reachable unkeyed rows plus exact wrapper-only `ArgsBuild` source rejection; representative and cumulative existing runtime behavior/cleanup/allocation owners; main Unit/Result/argv owners |
| ownership, Drop, allocation, and FFI | Keys/ABI rows are compiler metadata and allocate no Align value. Existing native ownership provenance, argument coercion, return reconstruction, Drop/null-safety, success/error allocation counts, and runtime ABI are unchanged. Compatible extern reuse compares only the complete source-derived LLVM function type against the fixed base row, then applies that row's curated return/parameter/function attributes; an incompatible function type rejects before LLVM. | one exact 286-identity `RuntimeAbiId`-keyed row iterator; all 286 exact registry types plus one return and every parameter-ordinal mutation through the production predicate; source-valid compatible reuse representatives for all five attribute classes; exact `ArgsBuild` `str` rejection and closest source-valid `layout(C) { u64, i64 }` mismatch; registry attribute mutation owners; allocation-count and ownership owners; view/layout(C)/raw FFI parity |
| whole/per-unit, ThinLTO, interface, and cache | Whole/per-unit/located/unlocated/diagnostic/object/ThinLTO paths share the same registry builder. rt-LTO policy comes from each row. Guarded baked definitions must be present, exact-typed, body-bearing, external, and C-convention before merge; every captured handle must still be a body-bearing external C definition after merge before internalization. Program/generated/native symbol spellings, MIR print/structural bytes, interface bytes/hash, source ABI, and runtime ABI remain unchanged; alphabetical keyed-native declaration order changes textual/raw LLVM once. Object bytes may change or remain equal, so no byte-equality promise is made. Compiler build id changes cache identity, causing one miss and then a hit on unchanged rebuilt input. Probe rows remain verification-only and never enter user module collision/reuse. | raw whole/per-unit-shaped alphabetical declaration identity, object link/behavior without a byte-equality assertion, rt-LTO off/on guarded attributes plus missing/declaration/wrong-type/linkage/CCC/post-link body/linkage/CCC negatives, unchanged MIR/interface/impl hashes, build-id miss then hit, all eight ordinary probe-spelling extern/program positives |
| benchmark and PR boundary | Registry construction and lookup are linear/constant-time compiler work; emitted calls and runtime cost are unchanged. C1 may exceed 1,000 hand-written lines because the exact 286 rows, their attributes, every declaration producer, and all dedicated consumers must move to one ABI authority in one pass. The temporary legacy alias seam contains handles only, is populated from that authority, and preserves old direct-call resolution until c3; it cannot declare a runtime ABI. A table-only PR would create two drifting ABI authorities, while a partial dedicated-consumer migration would retain an untyped lookup outside the explicitly deferred seam. | unchanged runtime-call benchmark, author table-to-declaration/consumer pass, focused owners, full codegen owners and applicable Clippy |

#### Am-c2a1/c2a2a/c2a2b/c2a3/c2a4 implementation closure matrix

Am-c2a1 is the next independently dormant checkpoint after c1. It adds one public effect-free MIR
record plus the private closed field-codec primitives. Am-c2a2a extracts the existing private am-n
sharing-preserving comparator behind one typed view without changing HIR validation. Am-c2a2b adds
only observation/complexity closure around that unchanged comparator. Am-c2a3 adds
the private borrowed canonical graph view and sole validated raw traversal. Am-c2a4 accepts only
that validated token and adds the stable partition plus canonical semantic-to-byte result. No slice
adds a lowering entrypoint, stored `Program` field, public codec/wrapper/error, interface, hash,
symbol, LLVM, runtime, Align allocation, or package consumer.

The private c2a1 field encoders append to a caller-owned `Vec<u8>` and return
`Result<(), CanonicalGraphError>`. The private, test-visible error is exactly `EmbeddedNul`,
`InvalidWidth`, `InvalidCount`, `MissingReference`, `DuplicateMember`, `InvalidSummary`, or
`InvalidGraph`; c2c maps this semantic-input subset one-for-one to the settled public errors. C2a1's
helpers are transactional relative to the caller's entry length: success preserves the existing
prefix and appends one complete field, while error truncates every addition made by that helper and
leaves the prefix byte-for-byte unchanged. C2a1's
exact mode seam is
`encode_param_mode(out: &mut Vec<u8>, mode: ParamMode) -> Result<(), CanonicalGraphError>`;
`ByValue`/`Out` append `0`/`1`, while `Borrow`/`BorrowMut` return `InvalidGraph`.

C2a1 moves the existing `validate_hir` five-variant `Node` identity without changing its variants or
consumers into the canonical module as the one `pub(super)` graph identity; `validate_hir` imports
that identity rather than retaining a duplicate. C2a2a introduces exactly this private typed seam:

```rust
pub(super) enum SourceShapeNode<'a> {
    Struct {
        source_name: &'a str,
        align: &'a Option<u32>,
        c_repr: &'a bool,
        fields: &'a [hir::FieldDef],
    },
    Enum {
        source_name: &'a str,
        variants: &'a [hir::EnumVariant],
    },
    Tuple {
        elems: &'a [Scalar],
    },
    Tagged(&'a hir::TaggedType),
    Function {
        params: &'a [(ParamMode, Scalar)],
        ret: &'a Ty,
        return_borrow: &'a hir::ReturnBorrowSummary,
        return_region: &'a hir::ReturnRegionSummary,
    },
}

pub(super) trait SourceShapeView {
    fn source_shape_node(&self, node: Node) -> Option<SourceShapeNode<'_>>;
}
```

The enum has no derives. Both the HIR adapter in c2a2a and canonical adapter in c2a3 construct only
borrowed projections into `self`: no normalized node, string, field, parameter, or summary is
allocated or copied. Internal struct/enum names and HIR function effect are unrepresentable in the
projection. C2a2a changes the comparator entry to
`source_shape_equal<V: SourceShapeView + ?Sized>(view: &V, left: Node, right: Node, known_shapes: &mut HashSet<(Node, Node)>) -> bool`.
`SourceShapeView`, `SourceShapeNode`, and `source_shape_equal` are `pub(super)`, not public crate
surface. `SourceShapeNode` is the closed borrowed struct/enum/tuple/tagged/function projection
needed by the current comparison; its function projection contains parameters, return, borrow, and
region but deliberately omits HIR effect exactly as the current comparator does. The HIR adapter is
the sole caller in c2a2a/c2a2b, and c2a3 adds only the canonical-view adapter. C2a2a compares aliases
with the first representative, shares one completed-pair cache, and preserves the current
sharing-sensitive bijection. C2a2b makes the private generic core accept a `SourceShapeObserver`; the production wrapper passes
the zero-sized `()` implementation, so release builds add no global state, dynamic dispatch,
observer allocation, or observer branch. A test collector may span the complete ordered
first-representative registration sequence and its shared completed-pair cache. Across that one
sequence, `V` counts distinct raw node identities observed on either side, `E` their reference-edge
occurrences counted once per distinct raw node, `P` distinct compared pairs across every comparator
call and cache-free restart, and `Q` every comparison work unit, including fixed fields,
discriminators, numeric payloads, reference occurrences, parameter/field/variant/summary-vector
elements, and bytes of source/member/variant names. The expected-amortized hash-table bound is
`O(V + E + P + Q)` time and transient space. Both `P` and `Q` can exceed the raw input size, so
neither the plan nor benchmark calls it input-linear.

C2a3's only graph constructor is
`ValidatedGraph::new(root: Ty, view: CanonicalTypeView<'a>) -> Result<ValidatedGraph<'a>, CanonicalGraphError>`.
Its private fields are exactly `root`, `view`, and the validated DFS-first raw-node order. No raw
view/root encoder is callable after this boundary. C2a4's only semantic entry is
`canonical_type_bytes(graph: &ValidatedGraph<'_>) -> Result<Vec<u8>, CanonicalGraphError>`.
C2a3 charges an error to the first reachable root/node/member/reference field in canonical
encounter order; at one field the order is NUL, width, local `InvalidGraph`, duplicate member,
missing reference, then summary. `Param`, `IntVar`, `FloatVar`, `Error`, `Borrow`, and `BorrowMut`
are typed but outside am-c's closed semantic domain and each returns `InvalidGraph`; they are not
decoder-only `UnknownTag` cases. No partial output is returned. The traversal records all
realizable `(ordinal, tie-rank, error)` candidates before choosing the minimum. When a raw node is
popped, all ordinals for that node's own serialized fields plus its record-end ordinal are assigned
before any referenced node definition is processed; a reference field therefore precedes the
referenced definition's fields. Exact raw tuple
duplicates and cross-node nominal/source-shape failures are charged at the end ordinal of the
second node's complete shape, so an earlier bad field inside that node wins while that completed
second-node failure beats a later sibling or referenced descendant definition error. Invalid
UTF-8, unknown discriminators, invalid encoded booleans, declared byte counts, truncation, trailing
bytes, and noncanonical serialized order are unrepresentable through the typed semantic view and
remain c2c decoder owners.

Before c2a2a/c2a2b coding, their exact owner-test closure checklist is:

| C2a2a/C2a2b acceptance cell | Required direct evidence |
|---|---|
| c2a2a typed seam and HIR parity | `SourceShapeView` has the one settled lookup signature; `SourceShapeNode` projects all five node kinds and every currently compared field. The production `hir::Program` adapter is the unchanged validator caller. The complete am-n suite pins source-registration order, first-representative selection, accepted/rejected HIR, malformed-reference failure, and stack-bounded deep comparison. |
| closed node semantics | `canonical_source_shape_comparator` changes each struct source name/alignment/C-layout/field name/type, enum source name/variant name/base/payload, tuple element, tagged discriminator/payload, and function parameter mode/type/return/borrow/region in isolation. A function effect-only twin remains equal. Missing and cross-kind nodes reject. |
| sharing and cycles | The same owner runs equal and unequal DAGs and cycles through both the production HIR adapter and an independently implemented minimal fixture view. It pins left-to-right and right-to-left bijections, first mismatch, and cycle termination. |
| completed-pair cache | The owner pins a fresh cached root skip, successful cache extension only after a complete comparison, no cache extension after failure, and the sibling-context restart that clears local mappings and rechecks from the root without cache. |
| c2a2b exact work measures | One non-global test collector is shared across the complete ordered first-representative registration sequence and its completed-pair cache. It records exact `V/E/P/Q` separately for Struct, Enum, Tuple, Tagged, and Function projections, including every relevant source/member/variant text byte, field/variant/parameter/payload/summary element, discriminator, fixed payload, reference edge, and restart rescan. Function effect is absent from both comparison and metric work. The production generic instantiation uses the zero-sized `()` observer and adds no allocation, dynamic dispatch, global state, or observer branch. |
| complete type matcher | One compact wildcard-free table drives equal twins and one discriminator/payload mutation for every 57 `Ty` and 34 `Scalar` constructors, including signedness, width, length, lanes, layout, dictionary field ordinal, parameter/inference ids, and every node-reference kind. The five node projections then cross every node-only field. A future type variant fails compilation until this owner and comparator are extended together. |
| c2a2b complexity families | `canonical_source_shape_complexity` varies alias fan-out, node degree, unique depth, and shared depth independently; records exact `V/E/P/Q`; proves the cache/restart adversary is charged to `P + Q`; and makes no assertion that `P` or `Q` is bounded by `V + E`. |
| allocation, benchmark, and boundary | C2a2a source inventory pins exactly the existing caller-owned completed-pair `HashSet` plus comparator `VecDeque`, seen `HashSet`, and two bijection `HashMap`s; its borrowed projection adds no collection or per-node allocation. C2a2b's production `()` observer adds none. C2a2b `provenance` reports compiler-only `canonical-source-shape-comparison` for a deterministic accepted sharing workload. Both inventories prove no canonical view, validated graph, canonical partition/bytes, public codec/error, stored MIR field, artifact/cache identity, runtime call, or package consumer has landed. |

| Cell | Required split closure | Exact owner evidence |
|---|---|---|
| c2a1 public formation and closed field codec | Land exactly `FunctionTypeDef { params: Vec<(ParamMode, Scalar)>, ret: Ty, return_borrow: ReturnBorrowSummary, return_region: ReturnRegionSummary }`, plus private error identity and one `pub(super)` node identity moved from `validate_hir` without representation or behavior change, and checked count/text/width/parameter-mode helpers. One wildcard-free root encoder covers the 57 graph-valid `Ty` tags `0..=56`; one scalar encoder covers 34 tags `0..=33`; one primitive encoder covers tags `0..=5`. `Param`, `IntVar`, `FloatVar`, `Error`, invalid widths/lanes, `Borrow`, and `BorrowMut` reject with the recorded errors. Both `ByValue` and `Out` are graph-valid. Every helper appends transactionally relative to the caller's existing prefix. No view, node-table traversal, or top-level byte result lands. | `canonical_field_codec` pins the complete bytes for every tag/payload, every `u8` width and lane boundary, both admitted modes, exact disabled-state errors, success-prefix preservation/error rollback, checked-count overflow, semantic versus decoder-only cases, and compile-time `FunctionTypeDef` formation; existing `validate_hir` owners prove the node-identity relocation is behavior-neutral; source inventory proves no graph/public/consumer surface |
| c2a2a shared source-shape comparator | Genericize only the existing am-n comparator through the exact borrowed `SourceShapeView`; keep the HIR adapter, first-representative alias policy, completed-pair cache, collection classes, bijection maps, restart-without-cache behavior, diagnostic result, and caller order exact. Add no observer, complexity collector/claim, benchmark, `CanonicalTypeView`, new graph traversal, codec, or byte result. | unchanged complete am-n owner suite; wildcard-free 57-`Ty`/34-`Scalar` parity matrix; direct all-five projection and minimal fixture-view equal/unequal owners; cache success/failure inventory; exact collection-source inventory |
| c2a2b comparator observation and complexity | Add only the generic observer/core seam around c2a2a's unchanged matching, queueing, cache, and restart decisions. The honest expected-amortized bound is `O(V + E + P + Q)` under the exact scalar-work definitions above; no raw-input-linear claim remains. | `canonical_source_shape_comparator` crosses sharing-preserving equal/unequal DAGs and cycles through both adapters and pins fresh-root skip plus sibling-context restart; `canonical_source_shape_complexity` pins exact `V/E/P/Q` for each of the five projection kinds, then varies alias fan-out, node degree, and unique/shared depth; production-zero-observer inventory; compiler-only benchmark |
| c2a3 graph formation and am-n invariants | `CanonicalTypeView` contains exactly borrowed struct, enum, tuple, tagged, and transient function tables. `ValidatedGraph::new` is the only raw root/view validation traversal. It assigns monotonically increasing field ordinals, collects candidate errors, and selects the minimum only after tuple/nominal checks. After c2a2b, same-source aliases use c2a2a's comparator. The raw scan itself is `O(V + E)` time/space, stores no per-nominal raw byte vector, emits no raw bytes, and performs no second raw validation DFS. Function summaries retain structural checks only; am-h keeps placement/capability. Unreachable records are ignored. | `canonical_graph_validation` crosses all five nodes, member/reference/am-n/summary cells, NUL-versus-width, second-node missing-reference-versus-shape, second-node shape-versus-later sibling and referenced descendant errors, tuple duplicate-versus-later sibling and referenced descendant errors, and unreachable twins; `canonical_graph_function_root_validation`; deep chain/shared/cyclic owners instrument exact raw `V/E`, prove no retained raw bytes, and assert stack bounds |
| c2a4 greatest-fixed-point equivalence | The input is only `&ValidatedGraph`. Struct and enum classes use kind plus source name after c2a3's bijective validation. Tuple, tagged, and function nodes seed by kind, non-reference fields, and referenced kind/nominal class, then refine prior anonymous classes to stability. Distinct raw anonymous records may merge only after child-class substitution. For `A` reachable anonymous nodes, refinement terminates in at most `R <= A + 1` rounds and costs `O(R * ((V + E) log V))`, including variable-length signature sorting; this is not called input-linear. | `canonical_graph_equivalence` crosses nominal and anonymous equal/unequal nodes, every non-reference/child ordinal, repeated/shared graphs, permutations, fresh Fn ids, and anonymous self-cycle versus bisimilar multi-node cycle plus deepest-label split; an adversarial chain records `R` and owns the round bound; `canonical_graph_signature_sort_bound` varies width and long common prefixes and records signature bytes, comparison count, and compared bytes; compiler-only `canonical-type-graph` benchmark |
| c2a4 canonical traversal and bytes | Assign class ordinals by a canonical class-graph walk using only `ValidatedGraph`'s root/view/order and c2a1/c2a3 encoders. Encode `version=1 || node_count:u32 || nodes || root_type`; repeated/recursive edges use their first assigned ordinal. The stable-partition class walk is linear after refinement. This is not a second raw validation traversal: no raw view/root overload or validator exists. | semantic-to-byte goldens for Unit, Bool, signed i64, all five node kinds, repeated reference, declaration permutation, recursive Fn root, and deep canonical chain/shared DAG/cycle |
| ownership, control, and allocation | The record, comparator, and engine are compiler metadata. They may allocate transient Rust vectors/maps/worklists and c2a4 one output vector, all released on return. They create no Align value, source nulling, Drop plan, replacement, return-cleanup fact, runtime/native object, artifact, cache entry, or process-global state. Clean and already-invalid caller tables remain byte-for-byte unchanged on success or error. | before/after table equality for valid and malformed inputs; success/error/drop/replacement/return N/A assertions; source inventory for absent non-MIR consumers; compiler-only complexity benchmarks above |
| PR boundary and closure | The 407-line compressed checkpoint formatted to 777 source lines. The rejected two-way and three-way cuts lacked a typed seam or still mixed comparison with graph validation. Final c2a1 measures 338 ordinary rustfmt production lines plus exactly 260 test lines. The reviewed c2a2 cap then failed during implementation measurement: the observer-bearing module reached 457 production lines before the benchmark while required owners reached 243 lines, beyond 460/180. Coding stopped and split again. The first c2a2a exact-diff preflight then found four mandatory owner gaps in the 149-line compact suite; their complete compile-closed matcher, independent function-parameter, node-boundary, and source-inventory evidence plus the bounded final-SHA `Scalar::Param` ID closure measure 210 lines, so coding stopped at the 150-line cap. These counts are historical evidence, not future caps. C2a1, c2a2a, c2a2b, c2a3, and c2a4 remain exact closure cells, but the remaining cells land in the complete capability waves below. No cell may introduce a second graph authority or bypass `ValidatedGraph`. | c2a1 field-codec owners; c2a2a existing am-n plus exact matcher/projection parity; c2a2b topology/cache/complexity/benchmark owners; c2a3 validation/function/deep owners; c2a4 equivalence/golden/function/deep/refinement owners; full `align_mir`, applicable Clippy, and one author matrix-to-diff pass for the complete capability wave |

The am-c author-side construction/consumption inventory is exact for the current tree:

| Class | Producers that must change together | Consumers that must change together |
|---|---|---|
| program | validated stored functions, per-unit imports, extern declarations, HIR `Call`, `FnValue`, lifted `Closure`, every scalar/AoS pipeline stage, `reduce`/`any`/`all`, `scan`, `partition`, `sort_by_key`, and parallel terminal/stage callables | MIR print/debug, work-weight scan, tagged-type remap/embedded-type scan, LLVM definition/import/extern declaration registry, direct-call lowering, extern coercion, function-value and closure thunk discovery/lowering, parallel signature checks, whole/per-unit symbol/linkage, explicit exports, and main wrapping |
| runtime | the 15 compiler-produced direct semantic keys split into eight specialized choices (`Print`, `PrintStr`, `PrintBool`, `PrintChar`, `PrintF32`, `PrintF64`, `Hash64`, `Hash128`) and seven generic legacy-map calls (`ProcessExit`, `ProcessAbort`, `DivFail`, `BoundsFail`, `RangeFail`, `Utf8BoundaryFail`, `LenMismatchFail`); every other dedicated MIR native node remains an exact `RuntimeKey` consumer in LLVM lowering | the fixed 285 keyed declarations and their typed dedicated consumers; the legacy alias seam populated from those declarations for unchanged seven-key generic direct calls and deferred program/generated consumers; two typed unkeyed wrapper handles; contract attributes, ThinLTO guarded rows, runtime export verification, compatible-extern reuse, and allocation/cleanup calls; `AllocSizeFail` is dedicated, while `error(code)` is not a RuntimeKey and lowers to the existing MIR identity value instead of surviving as a call |
| generated | every distinct `FnAddr`, capturing `Closure`, `SpawnTask` result/fallibility pair, `ParMapParallel` materialize/filter count/filter scatter request, and `ParMapReduce` request | pre-body collection/validation, canonical byte sorting/deduplication, global-name reservation/probing, helper declaration/body emission, call-site pointer selection, debug names, and malformed-before-publication rejection |
| symbol/cache | stored and imported Align definitions, extern C declarations, explicit exports, direct/wrapped main, 298 fixed native base rows, and generated requests | encoded `align_fn$<length>$<hex>` definition/import lookup, exact extern/native reuse, external-identity collision rejection, deterministic generated probing, ThinLTO internalization roots, structural MIR `impl_hash`, compiler-build cache identity, and unchanged interface/source-ABI hashes |

The callable applicability matrix is exhaustive; “unavailable” is an invalid hand-built MIR cell,
not a missing positive owner:

| MIR/callable form | Program definition | Program import | Program extern | RuntimeKey | GeneratedId |
|---|---|---|---|---|---|
| direct `Rvalue::Call` | `DirectCall::Program` | `DirectCall::Program` | `DirectCall::Program`, with existing FFI coercion | `DirectCall::Runtime`, exact fixed `RuntimeAbi` | unavailable |
| `FnAddr` | allowed | allowed | unavailable by am-u | unavailable | produces one `FnValue` request |
| lifted `Closure` | exact stored lifted target only | unavailable | unavailable | unavailable | produces one `Closure` request |
| task spawn | indirect closure operand, no named target | unavailable | unavailable | `tg_*` only through dedicated MIR nodes | produces one `Task` request |
| sequential map/filter/reducer/scan/partition/sort target | allowed | allowed | allowed only with am-u lexical permission already proved in HIR | unavailable | unavailable |
| parallel terminal or named stage | allowed only when proved Pure | allowed only when proved Pure | unavailable because extern effect is not Pure | runtime orchestration uses dedicated `par_map*` nodes only | produces the applicable `Parallel` request(s) |

Each program target keeps its one validated logical signature; owners cover Unit, scalar, aggregate,
and existing extern-coercion signatures on targets that admit them. Each `RuntimeKey` has exactly
the single return/parameter ABI in its `RuntimeAbi` row, so no test crosses a key with invented
Unit/scalar/aggregate alternatives. A program callable whose bytes equal a RuntimeKey remains
`Program`; an unregistered or wrong-class target rejects before MIR/codegen registry publication.
Every applicable cell runs whole/per-unit; definition/import/external cells also run ThinLTO off/on.

The parallel mode/stage/collection matrix is exact:

| Ordered stage list | Materialize | Reduce | FilterCount | FilterScatter | Collection rule |
|---|---|---|---|---|---|
| empty | valid one record | valid only for the stage-free integer reduction form | unavailable | unavailable | no pair |
| one or more `Map`/`Project`, no filter-kind stage | valid one record | unavailable | unavailable | unavailable | no pair |
| contains `Filter`, `FilterStrContains`, or `FilterField`, optionally interleaved with `Map`/`Project` | unavailable | unavailable | valid individual record | valid individual record | exactly one otherwise-identical count/scatter pair after dedupe |
| any unknown stage, invalid ordinal/type/signature, invalid work weight, or other mode/stage combination | reject | reject | reject | reject | reject before reservation |

Canonical-type owners cross every current root tag `0..=59`, scalar tag `0..=35`, primitive tag `0..=5`,
definition tag `0..=5`, parameter mode, summary state, equivalence/non-equivalence class, repeated and
recursive reference, shallow/deep graph, and encode/decode direction. Malformed owners mutate one
version, tag, boolean, width, count, UTF-8/NUL, reference/order, duplicate member/equivalence class,
empty nominal source, member identifier byte, alignment presence/power/range, enum first/later
`field_base`/flattened-count overflow, same-source nominal kind/complete-shape equality, repeated
anonymous tuple vector, summary index/order, truncation, or trailing byte at a time. Multi-invalid
owners cross invalid-text-before-duplicate/reference, earlier-node-before-later-node, and
same-member duplicate-before-missing-reference, earlier-member missing-reference-before-later
identifier/`field_base`, and field-base-before-shape/equivalence precedence. Generated owners then cross family,
equal dedupe/unequal identity, signature/capture/result shape, applicable parallel rows, stage order,
field ordinal, source/terminal type, weight `1/2/4`, zero/one/two occupied candidates, and injected
`u64::MAX` exhaustion. Ownership/control owners retain the am-b4 clean/already-invalid,
fallthrough/divergence, branch/loop/early-exit, moved/captured-value, and heap/arena/task provenance
matrix; metadata has explicit no-Align-allocation/source-nulling/Drop/replacement/return-cleanup rows.

Validation and error precedence is one pre-LLVM sequence: (1) fixed native-registry self-check;
(2) MIR type/header/`ProgramCall`/reference validation in program/table order; (3) stored, imported,
then extern declaration-class compatibility in vector order; (4) extern ABI and exact native-reuse
validation in extern order; (5) every direct/function-value/closure/pipeline/parallel program target
membership, signature, capture metadata, and claimant-class relation in function/block/statement
field order; a repeated FnValue/Closure target with different metadata necessarily fails here at
the first mismatching occurrence because each program target has one exact declaration; (6)
encoded/exact external-identity reservation for stored definitions,
imports, externs, explicit exports in CLI order (an identical same-function/same-identity repeat is
idempotent at its first occurrence; any different claimant at that identity fails at the later
occurrence), then direct/wrapped main; (7) generated requests in function/block/statement encounter
order: equal records dedupe while retaining the first ordinal, then collection closure scans the
retained records in encounter order and reports the first count/scatter record without its
otherwise-identical twin;
(8) canonical
byte sort/equal dedupe and deterministic name probing. The first failing phase wins; within a phase
the stated order wins. Malformed canonical metadata therefore precedes duplicate/conflicting
generated requests; declaration conflict precedes native ABI mismatch, which precedes target
invalid, which precedes external collision; external collision precedes
generated pair/probe failure; pair failure precedes probe exhaustion. No phase publishes a partial
registry, cache key, LLVM declaration, helper, or artifact.

These pre-LLVM failures preserve the existing public `CodegenError` enum and return
`CodegenError::Lowering(String)`; no new public error variant lands. The inner string is exact, and
the existing `Display` prefix remains `lowering failed: `. `<hex>` below is two lowercase
hexadecimal digits per raw identity/canonical byte with no prefix or separators. The closed mapping
is:

| First failing condition | Exact `Lowering` inner string |
|---|---|
| keyed/base cardinality differs from 281/286 | `runtime ABI registry invariant: key-count` or `runtime ABI registry invariant: base-count` |
| duplicate semantic key | `runtime ABI registry invariant: duplicate-key:<logical-name hex>` |
| duplicate fixed physical symbol | `runtime ABI registry invariant: duplicate-symbol:<symbol hex>` |
| one of the three exceptional key/symbol mappings is wrong | `runtime ABI registry invariant: key-symbol:<logical-name hex>` |
| source extern claims a fixed native symbol with a different source-derived LLVM function type | `native extern ABI mismatch:<symbol hex>` |
| public canonical decoder/record-local validation fails | `callable metadata invalid:<CanonicalCodecError variant name>` |
| stored/imported/extern class relation conflicts for one logical program name | `callable declaration conflict:<ProgramCall raw UTF-8 hex>` |
| a typed program target is absent or has a mismatched signature/class | `callable target invalid:<ProgramCall raw UTF-8 hex>` |
| two different claimants reserve one exact external identity | `callable external identity collision:<identity raw-byte hex>` |
| one count/scatter record lacks its otherwise-identical twin | `generated count-scatter pair mismatch:<GeneratedId canonical hex>` |
| the occupied `$18446744073709551615` candidate exhausts probing | `generated name exhausted:<complete occupied candidate ASCII hex>` |

`CanonicalCodecError variant name` is exactly the Rust identifier declared above. Registry
attribute/ABI-row mutations that cannot arise from compiler input are compile-time or owner-test
failures and do not invent a public runtime error. The eight validation phases and their within-phase
orders above select which one string is returned for multi-invalid input; owners assert both the
`CodegenError::Lowering` variant and exact inner bytes. Differing repeated FnValue/Closure metadata
owners expect `callable target invalid` at the first mismatching occurrence. Generated
multi-invalid owners include two such target mismatches in opposite encounter order, two missing
pairs in opposite encounter order, and a target mismatch plus missing pair proving phase 5 wins.
Cross-phase owners combine declaration conflict, native ABI mismatch, invalid target,
external-identity collision, generated conflict, missing pair, and occupied-maximum probe in one
fixture, then remove the earliest invalidity one at a time to prove that exact phase sequence.

Am-c has ten acceptance cells. C1, c2a1, and c2a2a are already merged. The
remaining cells land as capability waves rather than one dormant PR per row.
Am-c1 is the closed fixed-native-ABI
`RuntimeKey`/`RuntimeAbi` vertical owned by doc 20. It rejects an extern that claims a fixed native
symbol with an incompatible source-derived LLVM function type and newly makes a compatible keyed
extern+builtin or compatible unkeyed extern+wrapper share one declaration and link. Otherwise MIR
call strings, final mixed-map bindings, and symbol spellings remain unchanged; keyed-native
declarations normalize to alphabetical `RuntimeKey::ALL` order and change raw LLVM plus cache
identity once; object bytes have no equality promise. Typed handle application also deliberately
moves native attributes off a same-spelled program claimant and onto the actual native declaration.
C1 neither claims to repair nor newly changes the
spelling ambiguity that c3's typed discriminant owns. Am-c2a1 lands the effect-free record plus only
private field-codec primitives. Am-c2a2a extracts the typed sharing-preserving comparator without
changing its existing HIR consumer. Am-c2a2b adds only its observer, complexity owners, and
benchmark. Am-c2a3 lands the sole borrowed-view-to-`ValidatedGraph`
traversal and calls that comparator. Am-c2a4 accepts only that token to add equivalence and canonical
bytes. Am-c2b consumes c2a4 to retain, sort, and remap the
effect-free function table, changing structural MIR/compiler-build cache identity once. Am-c2c
exposes the dormant canonical type/function wrappers and complete decoder/error API. Am-c2d exposes
only generated/parallel identity records and their record-local codecs; collection validation and
LLVM consumption remain c3. None can drift into a second graph implementation.

The author estimate, measured against the existing 273-line tagged canonicalizer, roughly 372-line
formatted am-n comparator, and 340-line codegen type-graph validator while reusing am-d traversal,
is: c2a1 400 source + 260 field-codec tests = 660; c2a2a 440 implementation source + 210 compact
parity/inventory tests = 650; c2a2b 80 implementation/benchmark source + 150 topology/complexity tests = 230;
c2a3 380 source + 320 validation tests = 700; c2a4 170
implementation/benchmark source + 220 equivalence/golden tests = 390;
c2b 210 source + 260 tests = 470; c2c 360 source + 420
malformed/golden tests = 780;
c2d 270 source + 330 tests = 600 hand-written changed lines. Generated fixtures/hex tables are
counted as hand-written. These are owner-inventory estimates, not PR caps or
progress units.
C2a2a alone may exceed 1,000 total changed diff lines because the existing comparator deletion and
typed replacement necessarily move together; splitting them would leave no validator or two
drifting comparator authorities.
C2a1, c2a2a, c2a2b, c2a3, and c2a4 are independently dormant; c2b cannot retain/remap before c2a4's stable
equivalence/bytes exist; c2c calls c2a4's engine over c2b's canonical retained table; c2d calls c2c. Their focused owners
invoke each boundary directly.
Am-c3 activates typed declarations/direct calls, encoded program identities, every generated
family, collection pairing, collision preflight, and whole/per-unit consumers together. C3 may
exceed 1,000 hand-written lines: splitting its MIR producer from codegen leaves an unreadable target,
splitting definition/import symbols breaks producer-consumer linkage, and splitting any generated
family leaves accepted extern/export spellings able to collide with an unprobed helper.

The remaining L2 execution waves are:

```text
C-A canonical callable closure  c2a2b + c2a3 + c2a4 + c2b + c2c + c2d + c3
C-B borrow/ownership closure    af + ar + ap + t + b + L2c + L2d + L2e
```

C-B combines the former return-provenance and cleanup/borrow waves because
they are one public capability: a reusable owner must behave identically for
direct, indirect, imported, shared-borrow, exclusive-borrow, replacement, and
recursively Move return paths. Landing provenance without its cleanup and
borrow consumers would create another dormant boundary and repeat the same
matrix/review/gate cost.

Each wave may use intermediate commits and focused owner checkpoints, but it
gets one closure matrix, one stable-candidate full-diff review and coherent
finding closure before the draft PR opens, and one selected final verification
cycle. Do not review the unchanged diff again after opening the PR; require a
new independent review only for the repository's high-risk triggers. Split a
wave only when implementation evidence reveals genuinely independent failure
domains; line count, test count,
or a dormant internal seam is not sufficient. A wave above roughly 1,000
changed hand-written lines may record useful sizing evidence, but size alone is
not a split or review trigger.

The acceptance rows describe coverage, not one fixture or command per row.
Reuse cumulative and parameterized owners wherever they detect the regression;
add only missing discriminating cases, then run the selected owner set once on
the final wave tree.

Am-d is one cross-cutting vertical even if that exceeds roughly 1,000 hand-written changed lines:
splitting the producer/replay/lowering conversion would merge a state in which an accepted
producer-depth body can still overflow a remaining recursive consumer, while splitting the common
type visitor would leave a later phase free to reintroduce the same failure for a valid deep DAG.
The exact body preflight, every current recursive body consumer, the common type traversal, and
their boundary/deep-graph owners therefore land atomically.
Am-c3 remains cross-cutting by necessity for the exact activation subset stated above. Do not
activate a partial validator or publish a second graph/identity authority.

Am-e deliberately narrows one unsound source corner: a no-argument `main` with any return other
than Unit, exact i32, or `Result<Unit,builtin Error>` receives a source diagnostic before HIR.
Those forms were checker-accepted but had no valid C entry ABI; retaining them would publish an
external `main` with a non-C return type. `draft.md`, `docs/language-spec.md`,
`docs/design-notes.md`, `docs/open-questions.md`, and `docs/impl/07-roadmap.md` record the corrected
entry contract. Am-f enforces the settled every-path return obligation, preventing a reachable
bare return or absent body tail from producing `ret void` under a non-Unit LLVM function. Am-w
enforces the already-settled successful-wait dominance rule across every
control path and every handled fallible-Wait success outcome; it changes no language design, but
rejects the currently unsafe match/loop traversal holes before an uninitialized task slot can be
read. Am-v enforces the already-documented `mut buffer` requirement at the five native output
positions whose checkers currently accept an equal-typed temporary or immutable buffer. Am-u makes
the settled unsafe-FFI boundary exact: a direct or non-escaping named callback invocation is
lexically inside `unsafe`, while an extern can never escape as a first-class `Fn` value.

Apart from those five explicit corrections, there is no source syntax, source acceptance, public
diagnostic, interface byte/hash, structural type fingerprint, ownership rule, package API, runtime
ABI, or C ABI change. Am-h replaces two internal
checked-HIR origin/linkage fields with `FnOrigin`; no HIR is persisted or exchanged. Am-c changes
internal MIR call representation and deterministic LLVM/object symbol bytes; its per-unit
producer/consumer symbols change together, and the existing MIR `impl_hash` plus compiler-build
cache identity invalidates every affected cached object. No language mirror covers these
compiler-entry or internal-HIR contracts. Acceptance tests and benchmark rows above become
cumulative after their owning slice.

Am-g-t is independently complete and activates only its global type-domain all-empty rejection. It
is linear in stored type metadata and allocates only compiler-owned validation worklists and visit
maps. `valid_hir_global_type_preflight_is_mir_identity` compares the validated and internal
unchecked lowering paths. The complete `align_mir` owner suite and whole/per-unit codegen suite
remain clean, and `mir-global-type-validation` records the phase beside the existing
`mir-continuation-lowering` benchmark row.

L2b-a2-s owns the base fact shape, parameter/local formation, struct/tuple
construction/selection/replacement, destructuring, ordinary block/`if`/loop flow, liveness parity,
and the product half of the public boundary. L2b-a2-ac first closes the MIR continuation
prerequisite above without changing a projection fact. L2b-a2-am-g-t closes only direct
handcrafted-HIR global type-domain validation without changing valid HIR. The completed am-r
ledger inserts am-d, am-e, am-f, am-w, am-v, am-u, am-p, am-n, am-h, am-b1–b4, and am-c1/c2a1/c2a2a/c2a2b/c2a3/c2a4/c2b/c2c/c2d/c3 before
L2b-a2-af adds validated fixed-array paths
and exact/dynamic element selection and replacement. L2b-a2-ar closes eager retained-storage
actions for non-fixed reads. L2b-a2-ap adds pipeline `Project`/`WhereField` and terminal formation.
L2b-a2-t owns tagged construction/binding, `match`, `else`, `?`, `map_err`, and the final
public/projection-malformed-boundary pass. Every extending projection PR must add malformed
type/path/ordinal fallback owners for its new projection kinds and selected/unselected liveness
owners to the shared focused targets. Every implementation slice retains the scope-boundary row.

| L2b-a2 path | Exact analysis contract | Owner evidence |
|---|---|---|
| fact shape and flattening | A finite projection trie distinguishes struct fields, tuple elements, fixed-array elements, user-sum `(variant, payload)` slots, `Option.Some`, `Result.Ok`, and `Result.Err`. Each node may also carry a whole-value root. Projection inherits whole-value roots and selects only matching descendants; final return/interface formation flattens the selected trie to canonical parameter roots. A known inactive tag/payload projects to empty. A path/type disagreement, missing definition/id, or out-of-range ordinal instead returns the complete flattened fact at the current value, so malformed checked HIR can add conservative roots but cannot drop one. | direct checked-summary assertions; sema malformed-HIR projection tests for wrong kind, missing id, and out-of-range ordinal; whole/per-unit interface parity |
| parameter and local formation | A recursively borrow-capable parameter seeds its whole-value parameter root. `let`, whole-local assignment, generic monomorph locals, and direct named/imported call results replace the destination fact after RHS evaluation; branch/loop joins union matching nodes. | parameter aggregate, local replacement, generic, direct, and imported fixtures |
| aggregate construction and selection | Struct/tuple/fixed-array literals place each reachable child fact under its exact ordinal path. Every eager child/argument fact is captured immediately after that expression falls through, before any later sibling can mutate its source locals; precise product formation, residual aggregate fallback, and named-summary argument mapping all consume those completion-time facts. A snapshot is keyed by checked-expression identity, not source span, because synthetic view wrappers share their child's span. It participates in the same branch/loop invalidation state as a local: if a later sibling ends an owner generation, an ended-root marker remains at the exact projection path and invalidates the eventual destination. Every eager operation, including one whose result cannot borrow, validates its completed operand snapshots at the action boundary; a terminating later operand has no enclosing action and performs no validation. Loop probes isolate diagnostic-dedup state so only the real pass records that validation. Field, nested-field, tuple-index, fixed-array index, element-field, and pipeline selectors read the corresponding path. An index is exact only when checked HIR contains an in-range `ExprKind::Int`; no separate constant folding is performed. Every other index unions all reachable elements. `StageKind::Project { field }` selects that element-struct field, while `WhereField` preserves the complete incoming element fact. Receiver/source evaluation precedes index or stage action; a terminating predecessor produces no result fact. | nested struct, tuple, product/residual-array/named-call child-source-reassignment, direct and non-borrowing-result action use, later-sibling owner invalidation, loop-probe/real-pass diagnostic parity, terminating-later-operand negative twin, same-span array-to-slice wrapper, imported selected-owner liveness, fixed-array literal/dynamic index, element-field, `Project`, `WhereField`, and terminating receiver/index fixtures |
| aggregate replacement and destructuring | A whole-local write replaces the complete fact. A struct-field or exact fixed-array/element-field write kills only the exact destination subtree and installs the RHS fact there after RHS evaluation. A dynamic-index write cannot identify the replaced slot, so every possible destination retains its old fact and joins the RHS fact; no old root is killed. Whole-element writes use the same rule before nested selection. Root/index/RHS evaluation follows HIR source order and performs no install when an earlier child terminates. Exact self-assignment preserves the old fact. Tuple destructuring projects each present binding's exact element after one successful initializer evaluation; an omitted binding receives nothing. This slice does not widen the tuple element types accepted by type formation. | field/nested-field replacement, exact/dynamic element replacement, whole-element replacement, self-assignment, terminating index/RHS, control-produced tuple, and tuple-destructure-with-hole fixtures |
| tagged construction and pattern binding | User-sum, `Option`, and `Result` constructors place facts only below the active tag/payload path. A simple `match` binding selects its exact variant/payload ordinal; wildcard/or-pattern binds nothing. Branch arms start from the same evaluated scrutinee state and only fallthrough arms join. | user-sum, `Option`, `Result`, wildcard/or-pattern, mixed/all-diverging match fixtures |
| `else` and `?` | `else` success selects `Option.Some` or `Result.Ok` and joins only a fallthrough fallback. `?` continues with only `Result.Ok`; its implicit early-return edge contributes only `Result.Err`, and only when the enclosing return can carry a borrow. The operand is evaluated once and a terminating operand contributes neither edge. | success/fallback, Ok/Err, terminating/mixed control, and direct/imported summary fixtures |
| `map_err` | The output `Ok` projection preserves only the receiver's `Result.Ok`. For a statically named mapper, only the receiver's `Result.Err` is mapped through the mapper's settled parameter-root summary into the output `Result.Err`; mapper captures and unresolved function-value targets retain the L2b-a1 all-compatible fallback until L2b-b. Formation order and terminating receiver/mapper behavior remain the L2b-a1 contract. | named mapper fixed/identity, unresolved fallback, terminating mapper, and whole/per-unit fixtures |
| branch, block, and loop result | Transparent block/arena/task-group/unsafe tails preserve the complete selected fact. `if` and `match` union only fallthrough alternatives. Each accepted `break value` contributes its complete fact to the target loop result; rejected, unreachable, or payload-terminating breaks contribute nothing. Loop backedge/local assignment state reaches the existing finite fixpoint without widening a selected member to its siblings. | block/branch/match/loop twins, mixed termination, reassignment fixpoint, and rejected-break regressions |
| liveness and ownership parity | Existing invalidation continues to use the flattened owner-root set. Projection refinement may remove a sibling root but may never remove the selected value's owner, hidden temporary, or parameter root. Move/null, Drop, cleanup-bit, pipeline-source snapshot, escape, and effect behavior do not change in this slice. | owner-use diagnostics for selected versus unselected siblings plus the cumulative L2b-a1 gates |
| public and malformed boundary | `ReturnBorrowSummary` and `ReturnRegionSummary` remain the L2a codec and hash shape and remain equal in L2b-a2. Semantic import keeps the L2b-a1 validation order. No projection trie, local id, span, raw nominal id, or control-state bit is serialized. Because the codec carries parameter indices only, an imported aggregate result and any later projection from one aggregate actual deliberately retain that actual's complete compatible owner set. | unchanged codec/hash goldens, interface corruption suite, aggregate-actual precision-limit fixture, and summary byte-size benchmark row |
| scope boundary | Indirect calls, closure captures, function-value joins/moves, target-relative capture slots, and direct calls without a settled named/imported summary—including unanalyzed extern targets—retain the documented all-compatible-input fallback. No `borrow`, `borrow mut`, cleanup ABI, resource, region, or database surface is enabled. | existing deferred-function-value and compatibility/extern fixtures plus disabled-mode regressions |

L2b-a2-s, L2b-a2-ac, L2b-a2-am-g-t, the am-r design gate, and am-d through am-c2a2a are completed.
The remaining acceptance order is am-c2a2b, am-c2a3, am-c2a4, am-c2b, am-c2c, am-c2d, am-c3,
af, ar, ap, t, and b. It is executed in the capability waves above rather than as one PR per cell.
The first historical L2b-a2 PR
publishes an exact product summary while array, pipeline, and tagged/control forms deliberately
retain the shipped flattened result. It must include product construction, reads, partial writes,
destructuring, ordinary control joins, direct/imported consumption, and whole/per-unit parity
together: omitting a writer or join can under-approximate the same public product fact. The second
PR closes the general MIR continuation invariant for checked HIR. The third adds only global
type-domain validation. The completed am-r implementation cells applied stack-safety before the
five producer corrections, followed by placement, nominal/link, header, total body metadata, the
c1 runtime registry, c2a1 private field codec, and c2a2a shared source-shape comparator. The
remaining canonical callable capability wave comes next; the provenance capability wave follows.
Cell dependency order remains part of each wave, but it is no longer a mandatory PR sequence.
Am-c1/c2a1/c2a2a/c2a2b/c2a3/c2a4/c2b/c2c/c2d/c3 follow am-b4 because their typed/generated identities
consume already validated body callable facts; it must not duplicate or anticipate the b3/b4 body
contract. The tagged slice still
replaces its explicit and implicit `Result` fallbacks atomically. A capability wave expected to
exceed roughly 1,000 changed hand-written lines records why keeping its strict producer/consumer
chain together avoids duplicated proof or an unusable intermediate state.
The final L2b-a2-s diff is approximately 1,900 changed hand-written lines after adversarial review
required fail-closed constructor/read/write validation, common eager-child source-order snapshots,
snapshot-generation invalidation, checked-expression identity, action-boundary validation, and
discriminating residual/liveness owners.
Those checks cannot form a later PR: publishing the product fact first would let malformed checked
HIR discard a root, while omitting the residual-write owner would let a deferred array write publish
`None`; omitting snapshot invalidation or expression identity would respectively accept a dangling
earlier child or drop a synthetic wrapper's temporary owner. Product formation, mutation, control
joins, malformed fallback, source-order value lifetime, and their owner evidence therefore remain
one independently sound vertical.

Through L2b-a1, a `?` occurrence conservatively joins its flattened operand roots into the
enclosing function only when that function's return type can recursively carry a borrow; its `Ok`
projection continues through the ordinary expression and explicit/implicit return paths. L2b-a2
replaces that early-edge union with only the operand's `Result.Err` projection at the enclosing
function's `Result.Err` return boundary.
Pattern bindings select the corresponding source projection: user sums use their variant/payload
ordinal, while builtin `Option.Some`, `Result.Ok`, and `Result.Err` use their distinct tagged
projections. Pure and Impure origins therefore survive the same match-binding path.

The field-presence rule is exhaustive: L2a records both provenance summaries for every named,
imported, and function-value signature even when their values are `None`; L2c then records the
cleanup ABI for every such signature in the same change that implements it. Interface decode rejects
unknown mode/summary/cleanup tags, unsorted or duplicate root indices, capture roots in exported
named signatures, out-of-range roots, and a cleanup ABI inconsistent with the resolved return type.
Through L2b-a1, borrow and region summaries are the same canonical parameter-root set because both
facts come from the same flattened provenance walker; interface import and MIR validation reject a
disagreement, a root whose parameter type cannot borrow, or provenance on a non-borrowing return
type. A foreign qualified nominal never resolves by bare-name suffix to a definition in the current
interface; local generic definitions substitute every actual argument before recursive capability
checking, including same-name nested parameters. Unresolved imported or generic nominals remain
conservatively borrow-capable. An imported exact `None` is trusted only when the caller supplies a
validated external-provenance record; compatibility-API omissions and unanalyzed extern targets
retain the all-compatible-input fallback.
Before L2d, semantic import rejects a decoded `Borrow` mode; before L2e it rejects `BorrowMut`.
Recognizing a codec tag does not enable its source or imported-call semantics. No slice reconstructs
ownership from region provenance.

The L2b-a1 convergence review exposed one missed matrix axis: physical-layout recursion and
borrow-capability reachability are distinct graphs. A pointer/header wrapper (`DynArray`,
`DynStructArray`, or `Task`) breaks the LLVM inline-layout path but not the semantic path to a
borrow-bearing element. The capability classifier therefore uses an iterative visited-ID traversal
across structs, tuples, sums, and tagged records. A revisited header-mediated cycle contributes no
new edge, while any reachable borrowing leaf still makes the root borrow-capable.
The interface-side equivalent does not recursively instantiate concrete generic types. Semantic
import first builds one structured `(kind, exact local name)` definition index; rendered type strings
are never graph keys. The index rejects duplicate or ambiguous struct/enum names only among local
definitions. A compiler-produced public definition may share the spelling of a source builtin:
this is not a duplicate definition. Type resolution preserves sema's exact precedence:

1. every generic function, struct, and sum type validates its declared type-parameter list after
   the complete local type table exists. Stored parameter order is authoritative: a repeated
   parameter reports `DuplicateTypeParameter`; if the duplicate pass fails, that item does not
   enter the shadowing pass. Otherwise, the first parameter that shares a local declared-type name
   reports `TypeParameterShadowsLocalType`;
2. the qualified source builtins `json.doc`, `json.kind`, and `json.scanner<...>`, plus the closed
   nominal-alias spellings `core.Error`, `crypto.argon2_params`, `regex.regex_match`, and — when the
   accepted asymmetric suite activates — the six
   `crypto.{rs256,es256,ed25519}_{private,public}_key` forms, resolve before every qualified user
   definition. The `crypto` and `regex` forms require the matching std capability in producer
   source; interface reconstruction derives the same imports from those structured public type
   paths rather than serializing a second identity;
3. another declared type parameter wins only when used bare and without arguments;
4. every bare name first resolves through the local-definition index;
5. only after a local miss, another bare source-builtin spelling resolves to that builtin; and
6. exact `summary.unit.Name` resolves locally unless rule 2 applies. Another qualified name,
   including `summary.unit.child.Name`, is a conservative foreign leaf and never prefix- or
   suffix-resolves locally.

If a non-shadowing type-parameter spelling is used with arguments, rules 2, 4, 5, and 6 still get
their ordinary chance to resolve it. A generic function
`fn f<Option>(value: Option<str>) -> Option<str>` in a unit with no local `Option` therefore uses the
builtin `Option`; only an otherwise unresolved parameter spelling with arguments returns
`TypeParameterWithArguments`. A type parameter may not use a local generic's spelling to reach
that definition because producer sema rejects the shadowing declaration first. The same duplicate
and shadowing checks apply to generic struct and sum-type parameters; importer validation never
rejects a compiler-produced declaration that producer validation admitted.
`Task` is an internal HIR type and is not a source-nameable interface builtin in L2b-a1, so a local
definition named `Task` resolves locally. Before capability analysis, a complete semantic type walk
rejects duplicate or ambiguous local struct/enum names, duplicate type-parameter names, a type
parameter shadowing a local definition, an otherwise unresolved parameter used with arguments,
wrong local or source-builtin arity, and an unresolved bare name.
The walk validates children of every named, tuple, and function type even when capability evaluation
later treats the outer type as opaque.

Semantic import then solves two parametric summaries for each local definition over separate finite
lattices:

- the **borrow summary** is the least fixed point of an intrinsic-borrow bit plus the exact
  type-parameter positions on which borrow capability depends; and
- the **growth-transport summary** is the greatest fixed point of the parameter positions whose
  whole actual can re-enter capability traversal, seeded at every direct parameter leaf and retained
  through self or mutual local-reference cycles.

The two monotone worklists are distinct. Borrow-free `A<T> { next: A<Option<T>> }` has an empty
borrow summary but retains `T` in growth transport; `Id<U> { value: U }` retains `U`, while
`Sink<U> { value: i64 }` removes it. Applying a completed borrow summary to a root evaluates finite
type syntax only. This distinguishes `B<i64>` from `B<str>` while sharing all definition work across
signature types, public struct fields, sum payloads, constant annotations, and nested function
signatures. Function types are intrinsic opaque borrow-capability leaves; their parameter and
return types still enter the complete semantic walk and their summaries are independently validated.

The completed growth-transport summaries own the termination proof. Semantic import builds a
declaration-level dependency graph whose nodes are `(definition kind, name, type-parameter index)`.
Growth transport and dependency-edge measurement are separate operations. Computing whether an
enclosing definition transports one of its parameters evaluates each target-exposed actual through
the ordinary capability-transparent rules: transparent builtins and completed local growth
positions continue, while `box`, function types, and every other opaque constructor stop. After a
local reference itself has been reached, dependency construction records every source-parameter
occurrence in each of that reference's direct actuals, including occurrences below an opaque
constructor; it then discovers further local references inside only those actual positions exposed
by the target's completed growth summary, using the same opaque stops. Thus direct
`A<T> -> A<box<T>>` records a positive edge and rejects, but
`Shield<T> { value: Id<box<T>> }` does not expose `T` to an enclosing consumer and discovery inside
`Id<box<A<T>>>` does not reach `A`.
An edge records whether the source parameter is the whole direct target actual (weight zero) or
occurs below one or more type constructors (positive weight). A positive edge in a dependency cycle
is generative (`A<T>` to `A<Option<T>>`) and rejects. Positive acyclic edges and zero-weight cycles
are finite.
`Converge<T, U>` to `Converge<Option<U>, str>` converges because the wrapped `U` moves to `T` and
the next transition replaces it. `Id<U> { value: U }` exposes an actual and therefore reveals
growth hidden through `Id<A<Option<T>>>`; `Sink<U> { value: i64 }` does not expose it.
Parallel zero and positive edges are preserved; a positive edge may not be deduplicated behind a
zero edge between the same nodes.

Empty provenance summaries and definitions not referenced by a function receive the same
validation without per-root definition expansion. Multi-invalid precedence is fixed: codec and
canonical-summary decode errors precede import; import then checks the definition index and complete
type shape, enabled parameter modes plus generic-body/nested-summary shape, the parametric capability
solve, generative dependency cycles, and finally return capability, capture roots, and
parameter-root capability. Within a group it uses stored function, struct, enum, and constant order,
field/variant/payload order, depth-first type order, and canonical borrow-summary then region-summary
root order; hash-map iteration never selects an error. With `S` interface type-syntax nodes, `R`
local-definition references, `F` possible intrinsic/parameter summary facts, and `E` emitted
weighted dependency edges including parallel edges, the finite scan-based solve is bounded by
`O(S × F + S × R + E)` time and `O(S + F + E)` memory. Every fact changes at most once in its
lattice direction, direct-actual measurement is independent of nested-reference discovery, each
local-reference actual is scanned at most once per containing reference during dependency
construction, and no definition is recursively instantiated. Each shape failure maps to one public
import error: `DuplicateLocalType`, `DuplicateTypeParameter`,
`TypeParameterShadowsLocalType`, `TypeParameterWithArguments`, `InvalidTypeArity`, or
`UnresolvedBareType`. After the complete structured shape succeeds, a transported generic
declaration that does not parse as its specified single fragment returns `GenericBodySyntax`; one
that parses but disagrees with its structured record returns `GenericBodyMismatch`. A structured
generic `layout(C)` struct, which producer sema cannot emit before a concrete instantiation has one
C type, returns `GenericCLayoutUnsupported`. Stored function, struct, then sum-type order selects
the first body error; within a struct the C-layout gate precedes syntax, and syntax precedes
mismatch. All three precede enabled-mode and return-summary header validation. The generativity
gate alone returns `ReturnSummaryGenerativeCapabilityGraph`; it never substitutes for a preceding
shape error. Codegen likewise retains one
type-graph validator and its completed-node sets across every retained definition and signature root
in the program. It preserves the existing stored root order and depth-first child order: an Enter
validates the current id and active-path state, marks the node active, then pushes its Exit followed
by its children in reverse stored order so the LIFO walk observes fields, variants/payloads, tuple
elements, and tagged `Result` `Ok` then `Err` in stored order. Exit removes the active mark and adds
the completed mark. Header/pointer references validate their id at the current position without an
Enter. Per-kind active sets therefore distinguish a real inline cycle from a previously validated
DAG without consuming the compiler call stack. The MIR canonical-table reachability/key walkers and
the source-ABI key walker over the same tagged and tuple syntax are iterative as well; validation
does not hand a deep accepted graph to a recursive post-check. The first `CodegenError` for
multi-invalid MIR is independent of hash-set iteration.

Named-return inference likewise uses the checked-HIR direct-call graph rather than repeated
whole-program rescans. Every function is analyzed once initially; when its monotone parameter-root
summary grows, only its direct callers are queued again. A call chain therefore advances by
dependency worklist edges rather than one whole-program round per link. Checked HIR records lifted
origin with the required `FnOrigin` record defined by am-h above. L2b-a1 skips only
`FnOrigin::Lifted`; it never classifies origin from a mangled-name suffix. This also fixes the exact
explicit-parameter/capture boundary that L2b-b will consume. Lifted lambdas and function-value
targets otherwise remain deferred exactly as above. Owner tests place callers before callees, cover
a mutually recursive pair, and include an ordinary dependency function whose legal source name is
`lambda0`, so correctness and convergence cannot come from declaration order, an in-place single
pass, or synthetic-name guessing; the benchmark chain uses the same caller-before-callee order.
Direct checked-HIR owner tests cover all origin records: private and public declarations in entry
and non-entry units, a generic monomorph, `Lifted { capture_count: 0 }`, and
`Lifted { capture_count: n }` with the exact positive count. They also mutate each source flag and
origin discriminator while holding the name constant, proving that neither visibility nor origin
is inferred from spelling.

The reopened-review corrections stay in L2b-a1 as one parity follow-up. Splitting any of them into a
later PR would leave the current producer able to emit an interface that its consumer rejects,
allow a forged generic fragment to reconstruct a different consumer declaration, or publish
unreachable provenance that rejects a legal caller. The follow-up closes the shared
producer/importer generic-parameter and local-name validation, semantic interface-name resolution,
generic-fragment verification, checked-HIR function-origin metadata, and source-order return-flow
reachability seams plus their owner tests. It does not widen L2b-a1 into function-value or
capture-root inference.

The second reopened review found that accepted-edge identity alone did not close termination inside
the accepted payload. A payload can emit an inner `break`, explicit `return`, process termination,
or another diverging control-flow expression before the enclosing `break` reaches its own edge.
L2b-a1 therefore also closes MIR payload termination and effect-boundary source-order traversal in
this PR. These are the same already-shipped control-flow surface, not a later provenance feature:
deferring either would let sema publish a false Impure callback or let MIR overwrite a typed loop
result and emit cleanup after a terminated block. The correction is approximately 1,300
added-plus-removed hand-written lines across the source-order walker, MIR continuation gate,
matrix, and owner tests. It cannot split the walker’s child traversal from each enclosing call,
impurity, and boundary action: either intermediate would publish false effect state for a reachable
subset of the same expression variants. It stays in this already-open vertical because both
consumers must obey the same checked-HIR fallthrough contract; merging only MIR would retain false
purity rejection, while merging only effect inference would retain typed-slot corruption and double
cleanup.

The third reopened review found one remaining source-order disagreement and one evidence overclaim.
For a written pipeline terminal, Align evaluates the receiver/source first, then every explicit
stage operand in written stage order, then the terminal's explicit arguments in written argument
order, and finally enters the terminal operation. For `reduce(init, reducer)` and
`scan(init, reducer)`, the reducer's lifted captures are part of entering the terminal operation
after `init`; they are not evaluated before the source, stage operands, or `init`. The effect walker
already visited `source` before `init` but visited `init` before stage operands, while MIR lowered
`init` before both. That mismatch could hide an executed effect in `init` when the source terminated
in the effect walk, or execute an `init` effect that the published summary omitted. L2b-a1 closes
the whole shared seam in one vertical, with structural and reachable-state duties kept distinct.
AST-to-HIR checking preserves deterministic written diagnostic precedence: type hints may be read
from a named stage or reducer signature without evaluating an expression, then the first invalid
source, stage operand, `init`, or reducer is reported in that order and suppresses diagnostics from
later operands of the same terminal. Pipeline collection records malformed stages without emitting
them before source checking. Once a pipeline has formed checked HIR, finalization and lints still
traverse every child, including control-flow-dead syntax, so every retained type is concrete and
every retained diagnostic remains available. EscapeCheck likewise lowers every
child into its diagnostic CFG, but a non-fallthrough source/stage/`init` leaves every later child in
predecessor-less state that cannot join reachable escape provenance. EffectScan and
MoveCheck/return provenance stop reachable-state formation after the first non-fallthrough operand.
Every stage-bearing array terminal and streaming JSON-scanner reducer MIR emits only the reachable
prefix.

Pipeline lambda capture remains the settled by-value-at-creation model. After the source falls
through, MIR snapshots every stage lambda's Copy captures once at that stage's written position;
after every stage operand falls through, it evaluates the terminal's explicit arguments and then
snapshots terminal callable captures. In particular, `reduce`/`scan` evaluate `init` between stage
and reducer snapshots, while `map_into` evaluates `dst` after stage snapshots. The fused loop reuses
those operands and never reloads an enclosing local per iteration. Capture formation is not callback
execution: EffectScan records reachable stage operand/capture formation before terminal arguments,
but joins no stage call/dependency, callback parameter/result boundary, or terminal action until all
terminal arguments and terminal-capture formation fall through and the operation is entered. Thus
an argument mutation cannot retroactively change a stage capture, while a later terminal capture
observes that mutation; a terminating argument retains the earlier snapshot/use diagnostics but
makes every callback action dead. A terminating source/stage/argument creates no later snapshot. No
accumulator/destination store, finder plan, output allocation, loop state, callback call, source
cleanup transfer, or terminal result is formed after that boundary. JSON-scanner `scan` is not a
supported surface and is not added by this correction.

The pipeline source is also a preheader snapshot. MoveCheck records its owner roots at source
formation and revalidates them after an intervening terminal argument, alongside already-formed
stage-view captures. Direct, zipped, JSON-scanner, transparent-block, and recursively
borrow-preserving `if`/`match` sources therefore retain the union of every reachable selected owner
at arbitrary control-flow depth and reject before action when `init` replaces an owned source or an
owner behind a view. `else` retains its fallback
owner and retains success-side provenance only for a Copy borrow; a Move success payload transfers
out of and nulls its Option/Result container. A value-producing loop likewise transfers each
accepted `break` payload into its result slot, so its moved-and-nulled source place is not an owner
dependency of the enclosing pipeline. A terminal argument that returns or breaks before action
destroys the analysis snapshot in both the current state and every saved loop-break state; a dead
snapshot cannot survive a loop join or fixpoint.
Function-value effect state remains deliberately monotone across assignments: a later operand can
retain or strengthen an earlier Impure/Unknown capture, but can never turn it Pure.

This third correction is not a separable prerequisite PR. At the reopened gate,
`main...HEAD` already contains 9,549 added-plus-removed lines across the independently reviewed
L2b-a1 producer/importer/provenance vertical. The pipeline-order correction is expected to add
roughly 1,200–1,600 hand-written lines across checker recovery, the shared analysis walkers, every
stage-bearing array/scanner terminal, documentation, and owners. Merging the existing branch first
would knowingly publish
an unsound Pure summary and runtime order; merging checker/analysis first would leave MIR executing
a different program, while merging MIR/capture snapshots first would leave sema certifying dead
callbacks and reporting diagnostics in the wrong order. Capture formation and callback action also
cannot split: the intermediate would either reload captures per iteration or join an action that
`init` prevents. The smallest independently correct boundary is therefore the complete
source→stage-formation→terminal-arguments→terminal-capture→action vertical on the existing L2b-a1
PR.

The earlier text also described one sema test and two MIR tests as though they directly inspected
every cell in the complete Cartesian product. They did not: the sema owner directly proved the
nested-accepted-break function-value boundary, and the MIR owners directly proved representative
terminating and mixed payloads. The tables below remain the normative implementation inventory, but
the evidence ledger now states only what a named test directly observes. L2b-a1 adds focused
pipeline-terminal owners for the newly exposed source/stage/init seam. Broader expression-family
coverage remains cumulative through the existing sema and driver suites; it is not represented as
an internal call-set or allocation-count assertion unless the named owner actually makes that
assertion.

The following closure matrix is authoritative for implementation and review. “All call forms” means
same-unit named, imported, bound function value, indirect call, and generic monomorph where the
current type restrictions admit that form.

| Surface | Owner | Required positive closure | Required negative/fail-closed closure | Later extension |
|---|---|---|---|---|
| Signature formation | L2a | `ByValue` and existing `Out` are preserved in AST-to-HIR, named/imported signatures, `FnTy`, MIR, rendering, source equality, id-free ABI/interface fingerprints, and monomorph keys combining the structural signature with concrete effect-origin identity | unknown modes, arity mismatch, and mode/type disagreement never default to `ByValue` | L2d admits `Borrow`; L2e admits `BorrowMut` |
| Provenance record formation | L2a | every named/imported/function-value signature contains canonical sorted parameter-root borrow and region summaries, including explicit `None`; L2b-a1 requires the two records to agree; named-return inference uses a reverse direct-call worklist so a changed summary reprocesses only its callers; am-h replaces the ambiguous checked-HIR `lifted_capture_count`/`exportable` pair with the single required `FnOrigin` record and no name spelling decides whether inference or linkage applies | duplicate, unsorted, out-of-range, exported capture roots, borrow/region disagreement, roots inconsistent with resolved parameter/return types, an entry declaration whose unmangled canonical name collides with `Error`, `argon2_params`, or `regex_match`, duplicate/ambiguous local definitions or type parameters, a function/struct/sum type parameter shadowing a local definition, an otherwise-unresolved parameter-with-arguments, wrong local/source-builtin arity, unresolved bare names, recursive generic-capability bindings, an exposure-aware positive constructor-growth edge in a declaration-parameter dependency cycle, generic-body/type-parameter shape disagreement, and every missing or recursive nominal/tuple/tagged id reachable through any by-value `Ty`/`Scalar` wrapper reject before consumer-visible side effects in the stated total order; imported units are non-entry and their same-spelled local definitions are ordinary nominal types; `generic_body` is precisely the producer's item-span fragment: it starts at `fn` for a function or the declared type name for a struct/sum, omits `pub` and every struct `align`/`layout` prefix, and contains exactly that full declaration/body; validation reconstructs `pub` plus canonical `align(N)` then `layout(C)` prefixes from the structured record, rejects a module/import, extra item, trailing non-END token, or syntax error, parses exactly one declaration, and compares its kind, name, ordered type parameters/bounds, ordered function parameter modes/types and return type, ordered struct fields plus reconstructed layout attributes, or ordered sum variants against the structured record; an extra `pub` in the fragment is a syntax error because visibility is reconstructed rather than compared; function parameter names and generic function implementation expressions are deliberately transported but are not separate structured interface fields; a structured generic `layout(C)` struct returns `GenericCLayoutUnsupported`, within a struct that gate precedes `GenericBodySyntax`, and syntax precedes `GenericBodyMismatch`; all three precede header validation; producer and importer both validate generic parameter lists in stored declaration/parameter order with duplicate-before-shadow precedence; local definitions win bare lookup over the three nominal aliases, and non-shadowing type-parameter, qualified builtin, bare builtin fallback, exact local, unit-prefix foreign, and other foreign resolution follows the recorded sema precedence; positive acyclic transformations and zero-weight cycles remain valid and parallel zero/positive edges remain distinct; non-empty generic-template and nested function-value summaries reject until their consumer-side transports exist; interface analysis uses one structured definition index, a least-fixed-point `{intrinsic borrow, dependent parameter positions}` summary and a separate greatest-fixed-point growth-transport summary per local definition across all public roots, with capability-aware opaque stops for transport, complete direct-actual measurement, and no recursive instantiation; layout validation shares completed nodes across the program and uses an iterative enter/exit traversal; both layout and borrow-capability traversal through header-mediated nominal cycles are cycle-safe and never overflow the compiler stack | L2b computes non-empty roots |
| Interface codec/hash | L2a | mode plus borrow/region summaries have independent byte/hash goldens and producer/consumer parity | truncated, trailing, unknown-tag, unsupported-known-mode, and semantic inconsistency cases reject | L2c adds cleanup ABI atomically |
| Existing return provenance | L2b-a1/a2 | a1 preserves conservative flattened parameter roots through recursion, assignment, control flow, explicit/implicit/early return, and direct/imported calls; only reachable explicit returns, loop breaks, and trailing values contribute roots or loop post-state: eager children follow source order and stop after the first non-fallthrough child; `&&`/`||`, `if`/`match` arms, and `else` fallback fork from their common incoming state, retain every reachable dependency/return edge, exclude a diverging alternative from post-state, and join only fallthrough alternatives; `?` evaluates its operand once, contributes its reachable implicit error-return roots only when the enclosing return can borrow, and continues post-state only through the success edge; a loop builds its back-edge only from body fallthrough, its post-state only from reachable breaks, and is non-fallthrough when none exist; checker-owned evidence records the exact statement span of each `break` accepted for its target loop after the target/lambda and newly nested `arena`/`task_group` gates but before payload validation, then a post-check source-order classifier counts only reachable spans from that per-loop set and consumes the separately recorded fallthrough result of each nested loop; HIR carries the same accepted-edge bit on every checked `break`, and effect inference, EscapeCheck, MoveCheck/return-provenance, and MIR lowering may form a loop-result join, escape edge, move/borrow post-state, provenance root, or loop-exit terminator only from an accepted edge; a region-rejected `break` emits its region diagnostic first, checks and preserves its payload only for nested type/effect/ownership/escape diagnostics, records no accepted edge, remains non-fallthrough for recovery in every consumer, lowers fail-closed to `Unreachable` if malformed HIR is forced into MIR, and can neither satisfy an assertion nor combine with an unreachable accepted break to make the loop fall through; statements and tails after a non-fallthrough statement are never visited, so no dead edge can taint a summary or caller liveness; a2 recursively refines struct, tuple, fixed-array, tagged, `else`, `?`, `map_err`, and branch/loop projections | indirect/unresolved higher-order targets retain all compatible roots; incompatible joins reject; semantic import rejects provenance on every compiler-known non-borrowing builtin (`Error`, `argon2_params`, and `regex_match`) before per-unit checking | L2b-b adds function-value/capture roots; L3 adds resource/dependent roots; L4 adds explicit region owners |
| Effect source-order closure | L2b-a1 | each structural pass visits every reachable eager child once in language order; loop refinement may repeat that pass but every call, impurity flag, and boundary join is monotone and idempotent across fixpoint passes; block traversal stops after the first non-fallthrough statement and visits a tail only when the block falls through; an accepted `break value` visits the reachable effects inside `value` but joins its function-value/concrete effect into the target loop result only when `value` itself falls through to that break edge; ordinary fallthrough accepted breaks still join; written pipelines evaluate source, stage operands, terminal arguments, then terminal captures/action; `if`, `match`, `else`, short-circuit, `?`, `map_err`, nested blocks/regions, loops, explicit return, inner break, calls, aggregates, assignments, captures, pipelines, and process termination use the exhaustive product below and the same fallthrough contract as return provenance | no dead eager sibling, statement, tail, operation, branch-result, terminal argument, stage, or outer break whose payload already terminated can taint a local/result/expression boundary, named-call dependency, direct/indirect impurity, unresolved dispatch, parallel-callback purity, or fixpoint; a rejected break still visits reachable payload diagnostics but never joins a loop result; projection queries cannot reintroduce a dead tail; no conservative default may turn a proven non-fallthrough payload into a result edge | L2b-b extends the same source-order walker to function-value/capture roots |
| Pipeline terminal formation and MIR closure | L2b-a1 | type formation may inspect named stage/terminal signatures for hints without evaluating an expression, then validates source, stage operands, terminal arguments, and terminal callable in written order; the first invalid operand is reported and later operands of that terminal are not checked; finalization/lints still visit every child of successfully formed HIR, including control-flow-dead syntax; EscapeCheck isolates later syntax in predecessor-less diagnostic CFG state after termination; EffectScan and MoveCheck form state only from the reachable prefix; EffectScan separates stage capture/operand formation from callback action, and joins stage/terminal calls plus callback boundaries only after every pre-terminal operand and terminal capture falls through; function-value effect state joins assignments monotonically, so a later operand cannot make an earlier Impure/Unknown capture Pure; MIR snapshots each stage capture once after the source and at that stage's written position, evaluates explicit terminal arguments, then snapshots terminal callable captures once; the loop reuses those captured operands; MoveCheck snapshots the source's owner roots at source formation and revalidates them after terminal arguments, alongside already-formed stage-view captures; direct, zip, JSON-scanner, and control-flow-selected sources retain every reachable selected owner; return/break before action removes the analysis snapshot from current and saved loop-break states; `sum`, `count`, `any`, `all`, `min`, `max`, `sort`, `sort_by_key`, `to_array`, `map_into`, `partition`, `par_map`, `reduce`, and `scan` share that formation/action boundary; `map_into(dst)` evaluates `dst` after stage snapshots but before any stage action; `reduce`/`scan` evaluate `init` between stage and reducer snapshots; an accepted `break value` lowers `value` once and, only if the selected continuation has at least one reachable predecessor, reads the target loop frame, stores the result, nulls a moved source, emits iteration drops, transfers cleanup, and jumps to that loop's exit; a mixed `if`/`match`/`else`/`?`/short-circuit payload keeps the outer edge only for its fallthrough alternatives, and a nested loop's own break may yield a payload that then reaches the outer edge; when every reachable payload path terminates, the inner terminating construct owns the only result/return/process edge and its required cleanup; fixed, dynamic, and zipped sources share the same order; every JSON-scanner reducer follows it | multi-invalid terminal precedence is source before stage before terminal argument before terminal callable, with only the earliest invalid operand diagnosed; checked-HIR dead syntax still finalizes and lints; no dead child joins reachable effect, move, return, borrow, or escape state; a terminating terminal argument retains earlier stage-operand state but adds no stage/terminal action, call dependency, or callback boundary; capture loads are neither repeated per iteration nor moved across a later terminal argument; an owner invalidated after direct/zip/scanner/control-flow source formation rejects before action; no analysis snapshot survives a terminating return/break; an un-terminated zero-predecessor join is not fallthrough; after payload termination MIR emits no outer result store, Unit fallback, source nulling, iteration Drop, cleanup transfer, loop-frame lookup, or outer exit edge; after pipeline source/stage/terminal-argument termination MIR emits no later operand, capture snapshot, accumulator/output allocation or store, loop/control state, callback call, source cleanup transfer, or result; nested accepted break, explicit return, `process.exit`, `process.abort`, and fully diverging nested block/`if`/`match`/loop payloads preserve their typed result and cannot be overwritten or double-cleaned; malformed HIR remains fail-closed without panic; JSON-scanner `scan` remains rejected | L2c reuses the same post-lowering continuation gate before cleanup-bit transfer |
| Closure/function-value provenance | L2b-b | zero-argument and parameterized closures, synthetic selectors, target joins, environment moves, direct and indirect calls retain selected target-relative roots | environment/owner death, stale generation, out-of-range capture slot, and interface capture root reject | L3/L4 extend the same walker with their types |
| Cleanup ABI formation | L2c | Copy returns record `None`; every recursively Move return records `DynamicBit` in `FnTy`, named/imported signatures, MIR, interface, mangling, cache identity, and LLVM ABI | metadata/type disagreement, missing bit, extra bit, unknown tag, and caller/callee fingerprint mismatch reject | none |
| Cleanup-bit production | L2c | normal expression return, explicit return, `if`, `match`, `else`, `?`, `map_err`, branch/loop join, and early exit forward the selected path-local bit and clear a moved source exactly once | malformed MIR bit source/destination, missing local, invalid tag, and uninitialized/duplicate transfer reject without panic | L4 adds explicit-region clear-bit values |
| Cleanup-bit consumption | L2c | all call forms store the returned bit in the caller result slot; move-out/null, reassignment drop-old, wildcard discard, and scope/early cleanup consult that bit exactly once | no caller may infer the bit from type, tag, or region; ABI mismatch fails before call emission | L2e reuses the same slot through mutable replacement |
| Shared-borrow formation | L2d | contextual `borrow name: T` works for named functions and function types; `borrow: T` and `out: region` remain parameter names; stable addressable immutable or mutable local/field places of Copy or Move type whose root is a bound local are accepted | temporary/rvalue, moved place, mode mismatch, move/drop/replace through callee binding, and unbound storage reject | L3 admits resource owners |
| Shared-borrow calls/results | L2d | all call forms pass non-null caller storage without ownership transfer; caller owner remains usable; completed summaries attach returned views to the exact owner generation | use after owner move/drop, wrong indirect mode, stale returned view, corrupt imported summary, any ByValue peer that moves/consumes the same root, and any overlapping existing `Out` peer reject identically in either argument order, including rooted fields and aggregate holders | none |
| Exclusive-borrow formation | L2e | contextual `borrow mut`, existing `Out`, writable Copy/Move local and field places, and function-value modes share one place classifier | immutable, temporary/rvalue, moved, overlapping field/whole-place, unbound storage, wrong mode, and unsupported partial Move leaf reject | L3 admits resource owners |
| Exclusive alias/invalidation | L2e | recursively scan every `ByValue`/`Borrow`/`BorrowMut`/`Out` peer, including distinct aggregate holders; end the old generation at the call; preserve branch/loop state | any direct or nested overlap and any older view use reject before callee effects, with identical local/imported diagnostics | L3 adds resource/dependent overlap classes |
| Exclusive replacement/effect | L2e | changed owned pointee runs guarded drop-old once, stores value and cleanup bit, and later caller Drop sees only the replacement; unchanged pointee emits no callee-exit cleanup; exclusive-input-only mutation is Pure. A reverse direct-call worklist computes a least fixed point from each same-program `borrow mut` destination to only the parameter roots stored by reachable whole/field/element replacement, builder push/append, or transitive direct calls; `clone_in(out)` contributes `out`, not its source. This fact is analysis-local and is recomputed for checked-HIR replay | imported, indirect, missing-body, malformed, and unresolved calls retain all compatible view-bearing arguments; a malformed destination/source index fails closed; no exact summary is serialized into HIR, MIR, interfaces, or ABI | none |
| End-to-end parity | each slice | focused owner tests, whole/per-unit builds, direct/indirect/imported behavior, generic/interface/cache identity, runtime provenance, Drop/allocation counts, and the slice benchmark agree | malformed interfaces/MIR fail closed and no disabled later mode is accepted | later rows add cases without weakening earlier gates |

Each closure-matrix row is owned by the following exact focused targets. New targets are created by
their first owning slice and remain cumulative gates afterward.

| Slice | Exact owner tests | Exact benchmark command and required rows |
|---|---|---|
| L2a | `cargo test -p align_interface --test summary`; `cargo test -p align_driver --test fn_values --test out_params --test interface_param_modes` | `bench/library_boundary/run.sh interface`: `interface-size`, `decode-throughput` |
| L2b-a1 | `cargo test -p align_interface --test summary`; `cargo test -p align_sema ty_may_borrow_is_cycle_safe_for_header_mediated_nominals`; `cargo test -p align_sema lifted_function_origin_metadata_is_explicit`; `cargo test -p align_sema checked_break_acceptance_is_preserved_in_hir`; `cargo test -p align_sema rejected_break_effect_payload_is_visited_without_loop_result_join`; `cargo test -p align_sema effect_source_order_closure_matrix`; `cargo test -p align_sema pipeline_terminal_snapshot_action_order_matrix`; `cargo test -p align_sema pipeline_terminal_diagnostic_order`; `cargo test -p align_sema pipeline_terminal_dead_state_isolated_across_analyses`; `cargo test -p align_sema pipeline_terminal_dead_hir_is_finalized_and_linted`; `cargo test -p align_sema pipeline_capture_owner_invalidation_is_rejected`; `cargo test -p align_sema pipeline_source_snapshot_owner_invalidation_matrix`; `cargo test -p align_codegen_llvm malformed_mir_type_graphs_fail_before_llvm_construction`; `cargo test -p align_mir rejected_checked_break_lowers_to_unreachable`; `cargo test -p align_mir terminating_break_payload_emits_no_outer_edge`; `cargo test -p align_mir mixed_break_payload_preserves_outer_edge`; `cargo test -p align_mir terminating_pipeline_operand_emits_no_terminal_state`; `cargo test -p align_mir pipeline_terminal_snapshot_action_order_matrix`; `cargo test -p align_mir pipeline_terminal_source_shape_parity`; `cargo test -p align_driver --test return_provenance --test analysis_coverage --test interface_param_modes --test per_unit`; `cargo test -p align_driver --test m5 json_scan_reduce_fold`; `cargo test -p align_driver --test zip_pipeline pipeline_terminal_source_order` | `bench/library_boundary/run.sh provenance`: `summary-inference`, `import-validation` |
| L2b-a2-s | `cargo test -p align_sema projected_return_provenance_fails_closed`; `cargo test -p align_driver --test return_provenance --test per_unit` | `bench/library_boundary/run.sh provenance`: `summary-inference` |
| L2b-a2-ac | `cargo test -p align_mir eager_expression_termination_matrix`; `cargo test -p align_mir malformed_hir_continuation_metadata_fails_closed`; `cargo test -p align_driver --test mir_continuation`; `cargo test -p align_driver --test expr_depth within_limit_chain_compiles_and_runs`; `cargo test -p align_driver --test per_unit_codegen eager_expression_termination` | `bench/library_boundary/run.sh provenance`: `mir-continuation-lowering` |
| L2b-a2-am-g-t | `cargo test -p align_mir malformed_hir_global_type_metadata_fails_closed`; `cargo test -p align_mir valid_hir_global_type_preflight_is_mir_identity`; `cargo test -p align_mir`; `cargo test -p align_driver --test per_unit_codegen` | `bench/library_boundary/run.sh provenance`: `mir-global-type-validation`, `mir-continuation-lowering` |
| L2b-a2-am-r | design-only completed-ledger consistency check and fresh adversarial review; no implementation target | none |
| L2b-a2-am-d | `cargo test -p align_sema checked_hir_depth_closure_matrix`; `cargo test -p align_sema deep_type_consumer_closure_matrix`; `cargo test -p align_mir checked_hir_depth_closure_matrix`; `cargo test -p align_mir deep_type_consumer_closure_matrix`; `cargo test -p align_codegen_llvm checked_hir_depth_closure_matrix`; `cargo test -p align_codegen_llvm deep_type_consumer_closure_matrix`; `cargo test -p align_driver --test expr_depth --test per_unit_codegen --test deep_type_graphs` | existing compile/MIR/codegen and `mir-global-type-validation` rows only; no new runtime benchmark |
| L2b-a2-am-e | `cargo test -p align_sema main_signature_matrix`; `cargo test -p align_codegen_llvm main_abi_matrix`; `cargo test -p align_driver --test main_abi --test unit_main_exit_code --test per_unit_codegen --test thin_lto` | existing compile/codegen rows only; no new benchmark |
| L2b-a2-am-f | `cargo test -p align_sema function_return_completeness_matrix`; `cargo test -p align_mir function_return_completeness_matrix`; `cargo test -p align_codegen_llvm function_return_completeness_matrix`; `cargo test -p align_driver --test value_control_flow --test analysis_coverage --test per_unit_codegen --test m11_process` | existing compile/control/allocation rows only; no new benchmark |
| L2b-a2-am-w | `cargo test -p align_driver --test task_group --test per_unit_codegen`; `cargo test -p align_sema task_wait_duplicate_span_identity`; `cargo test -p align_sema task_wait_duplicate_span_all_identity_kinds`; `cargo test -p align_sema task_wait_duplicate_span_gets_report_separately`; `cargo test -p align_sema task_wait_missing_node_fails_closed`; `cargo test -p align_sema task_wait_token_exhaustion_fails_closed`; `cargo test -p align_sema task_wait_empty_body_has_replay_budget`; `cargo test -p align_sema task_wait_depth_is_stack_bounded`; `cargo test -p align_sema task_wait_loop_fixed_point_guard_is_depth_derived`; `cargo test -p align_sema task_wait_loop_unresolved_wait_reaches_later_break` | `bench/library_boundary/run.sh provenance`: `task-wait-proof-flow`; no new runtime benchmark |
| L2b-a2-am-v | `cargo test -p align_sema native_output_buffer_requires_mut_local`; `cargo test -p align_driver --test m9_io --test m12_file_io --test m11_net --test m11_crypto --test per_unit_codegen` | existing native I/O rows only; no new benchmark |
| L2b-a2-am-u | `cargo test -p align_sema extern_invocation_permission_matrix`; `cargo test -p align_driver --test ffi --test ffi_views --test ffi_link --test fn_values --test m5 --test per_unit_codegen` | existing extern/callable rows only; no new benchmark |
| L2b-a2-am-p | `cargo test -p align_sema generic_enum_response_builder_monomorph_is_producer_valid`; `cargo test -p align_mir malformed_hir_type_placement_fails_closed`; `cargo test -p align_mir valid_hir_type_placement_preflight_is_mir_identity`; `cargo test -p align_mir body_only_header_types_fail_placement_closed`; `cargo test -p align_mir abstract_box_param_fails_placement_closed`; `cargo test -p align_mir deep_hir_type_dag_placement_is_stack_bounded`; `cargo test -p align_mir`; `cargo test -p align_driver --test per_unit_codegen` | `bench/library_boundary/run.sh provenance`: `mir-type-placement-validation`, `mir-continuation-lowering` |
| L2b-a2-am-n | `cargo test -p align_mir malformed_hir_nominal_link_metadata_fails_closed`; `cargo test -p align_mir valid_hir_nominal_link_preflight_is_mir_identity`; `cargo test -p align_mir deep_hir_source_shape_is_stack_bounded`; `cargo test -p align_mir`; `cargo test -p align_driver --test per_unit_codegen` | `bench/library_boundary/run.sh provenance`: `mir-nominal-link-validation`, `mir-continuation-lowering` |
| L2b-a2-am-h | `cargo test -p align_mir malformed_hir_declaration_header_metadata_fails_closed`; `cargo test -p align_mir valid_hir_declaration_header_preflight_is_mir_identity`; `cargo test -p align_mir deep_hir_header_type_dag_is_stack_bounded`; `cargo test -p align_mir`; `cargo test -p align_driver --test per_unit_codegen` | `bench/library_boundary/run.sh provenance`: `mir-header-validation`, `mir-continuation-lowering` |
| L2b-a2-am-b1 | `cargo test -p align_mir hir_body_validator_core`; `cargo test -p align_mir hir_body_validator_statements`; `cargo test -p align_mir hir_body_validator_statement_inventory`; `cargo test -p align_mir hir_body_validator_accepts_module_monomorph_call_name`; `cargo test -p align_mir hir_body_type_mangle_golden_vectors`; `cargo test -p align_mir deep_hir_body_core_type_dag_is_stack_bounded`; no public-entrypoint activation | none |
| L2b-a2-am-b2a | `cargo test -p align_mir hir_body_validator_storage_vector_array`; `cargo test -p align_mir deep_hir_body_storage_type_dag_is_stack_bounded`; no public-entrypoint activation | none |
| L2b-a2-am-b2b1 | `cargo test -p align_mir hir_body_validator_pipeline_array_views`; `cargo test -p align_mir hir_body_validator_pipeline_stage_records`; `cargo test -p align_mir hir_body_validator_pipeline_terminals`; `cargo test -p align_mir hir_body_validator_pipeline_control_flow`; `cargo test -p align_mir hir_body_validator_pipeline_deferred_facts_are_not_consumed`; `cargo test -p align_mir deep_hir_body_pipeline_type_dag_is_stack_bounded`; no public-entrypoint activation | none |
| L2b-a2-am-b2b2 | `cargo test -p align_mir hir_body_validator_pipeline_template_json_group`; `cargo test -p align_mir hir_body_validator_pipeline_template_json_group_control_flow`; `cargo test -p align_mir deep_hir_body_pipeline_b2b2_type_dag_is_stack_bounded`; `cargo test -p align_mir hir_body_validator_pipeline_deferred_b2b2`; `cargo test -p align_mir hir_body_validator_json_scan_copy_row`; `cargo test -p align_mir hir_program_json_scan_copy_row`; `cargo test -p align_mir hir_program_json_scan_envelope_mismatch`; `cargo test -p align_mir hir_program_json_scan_envelope_precedence_matrix`; `cargo test -p align_driver --test m5 json_scan_copy_`; `cargo test -p align_driver --test m5 json_scan_generic_return_context_`; `cargo test -p align_driver --test modules json_scan_imported_`; `cargo test -p align_driver --test cache_codegen json_scan_row_schema_rejection`; `cargo test -p align_driver --test cache_codegen json_scan_per_unit_interface_row_ownership`; `cargo test -p align_driver --test cache_codegen json_scan_generic_return_context_no_publication`; no general public-entrypoint activation | Historical implementation-time evidence: `scripts/compare-json-scan-identity.sh` replays pinned owner sources under `rustc 1.96.1`, `llvm-config-22 22.1.8`, `cc`, and no custom `RUSTFLAGS` between baseline `576e57307fe4ef34e74566f5e389a2f0e2a04acd` and implementation `aa5bb7d66d0436c2d9ebf89f252b0ba5d528c2a8`; compare exact interface bytes and `InterfaceSummary.interface_hash`, all actual `CodegenKey` fields except the intentional `compiler_build_id` difference (recorded as `FirstDiff::CompilerBuildId`), codegen-input MIR, raw LLVM, and release object bytes with `cmp` and no normalization; fail on any other cache-key difference and share no cache object across compiler builds; later compiler heads are not inputs; no new runtime benchmark |
| L2b-a2-am-b3 | `cargo test -p align_mir hir_body_validator_native`; `cargo test -p align_mir hir_body_validator_generated_callables`; `cargo test -p align_mir hir_body_validator_native_control_flow`; `cargo test -p align_mir deep_hir_body_native_type_dag_is_stack_bounded`; no public-entrypoint activation | none |
| L2b-a2-am-b4 | `cargo test -p align_mir malformed_hir_body_metadata_fails_closed`; `cargo test -p align_mir checked_hir_body_fact_replay_covers_cleanup_and_function_effects`; `cargo test -p align_mir valid_hir_body_preflight_is_mir_identity`; `cargo test -p align_mir body_contract_function_return_none`; `cargo test -p align_mir body_contract_function_root_completion`; `cargo test -p align_mir checked_hir_depth_closure_matrix`; `cargo test -p align_mir deep_hir_body_core_type_dag_is_stack_bounded`; `cargo test -p align_sema replay_clone`; `cargo test -p align_mir`; `cargo test -p align_driver --test expr_depth within_limit_chain_compiles_and_runs`; `cargo test -p align_driver --test per_unit_codegen` | `bench/library_boundary/run.sh provenance`: `mir-body-validation`, `mir-continuation-lowering` |
| L2b-a2-am-c1 | `cargo test -p align_codegen_llvm runtime_abi`; `cargo test -p align_runtime runtime_export_source_inventory_matches_registry`; `scripts/test-runtime-abi-exports.sh`; `cargo test -p align_codegen_llvm` | `bench/library_boundary/run.sh provenance`: unchanged runtime-call cost |
| L2b-a2-am-c2a1 | `cargo test -p align_mir canonical_field_codec`; `cargo test -p align_mir` | no public runtime benchmark row |
| L2b-a2-am-c2a2a | `cargo test -p align_mir malformed_hir_nominal_link_metadata_fails_closed`; `cargo test -p align_mir valid_hir_nominal_link_preflight_is_mir_identity`; `cargo test -p align_mir nominal_source_shape_preserves_shared_node_correspondence`; `cargo test -p align_mir deep_hir_source_shape_is_stack_bounded`; `cargo test -p align_mir canonical_source_shape_comparator`; `cargo test -p align_mir canonical_field_codec`; `cargo test -p align_mir` | no benchmark row |
| L2b-a2-am-c2a2b | `cargo test -p align_mir canonical_source_shape_comparator`; `cargo test -p align_mir canonical_source_shape_complexity`; `cargo test -p align_mir` | `bench/library_boundary/run.sh provenance`: compiler-only `canonical-source-shape-comparison` |
| L2b-a2-am-c2a3 | `cargo test -p align_mir canonical_graph_validation`; `cargo test -p align_mir canonical_graph_function_root_validation`; `cargo test -p align_mir deep_canonical_graph_validation_is_stack_bounded`; `cargo test -p align_mir canonical_graph_validation_raw_scan_is_linear`; `cargo test -p align_mir` | no public runtime benchmark row |
| L2b-a2-am-c2a4 | `cargo test -p align_mir canonical_graph_engine`; `cargo test -p align_mir canonical_graph_equivalence`; `cargo test -p align_mir canonical_graph_function_root`; `cargo test -p align_mir canonical_graph_refinement_round_bound`; `cargo test -p align_mir canonical_graph_signature_sort_bound`; `cargo test -p align_mir deep_canonical_graph_is_stack_bounded`; `cargo test -p align_mir` | `bench/library_boundary/run.sh provenance`: compiler-only `canonical-type-graph` |
| L2b-a2-am-c2b | `cargo test -p align_mir canonical_function_type_remap`; `cargo test -p align_driver --test per_unit_codegen gate_f_impl_hash`; `cargo test -p align_mir` | no public runtime benchmark row |
| L2b-a2-am-c2c | `cargo test -p align_mir canonical_type_codec`; `cargo test -p align_mir canonical_type_codec_function_root`; `cargo test -p align_mir canonical_codec_error_precedence`; `cargo test -p align_mir deep_canonical_type_codec_is_stack_bounded`; `cargo test -p align_mir` | no public runtime benchmark row |
| L2b-a2-am-c2d | `cargo test -p align_mir generated_identity_codec`; `cargo test -p align_mir generated_identity_error_precedence`; `cargo test -p align_mir deep_generated_identity_codec_is_stack_bounded`; `cargo test -p align_mir` | no public runtime benchmark row |
| L2b-a2-am-c3 | `cargo test -p align_mir malformed_hir_callable_namespace_fails_closed`; `cargo test -p align_codegen_llvm callable_namespace`; `cargo test -p align_codegen_llvm generated_identity_collection`; `cargo test -p align_driver --test per_unit_codegen --test export_roots --test thin_lto`; `cargo test -p align_mir`; `cargo test -p align_codegen_llvm` | `bench/library_boundary/run.sh provenance`: `mir-callable-namespace-validation`, `mir-continuation-lowering` |
| L2b-a2-af | `cargo test -p align_sema projected_return_provenance_fails_closed`; `cargo test -p align_mir eager_expression_termination_matrix`; `cargo test -p align_driver --test return_provenance --test per_unit` | `bench/library_boundary/run.sh provenance`: `summary-inference` |
| L2b-a2-ar | `cargo test -p align_mir eager_expression_termination_matrix`; `cargo test -p align_driver --test return_provenance --test borrow_liveness --test struct_index --test chunks --test soa --test m11_http --test m11_http_get_many` | `bench/library_boundary/run.sh provenance`: `summary-inference` |
| L2b-a2-ap | `cargo test -p align_sema projected_return_provenance_fails_closed`; `cargo test -p align_mir eager_expression_termination_matrix`; `cargo test -p align_driver --test return_provenance --test per_unit` | `bench/library_boundary/run.sh provenance`: `summary-inference` |
| L2b-a2-t | `cargo test -p align_sema projected_return_provenance_fails_closed`; `cargo test -p align_driver --test return_provenance --test per_unit` | `bench/library_boundary/run.sh provenance`: `summary-inference` |
| L2b-b | `cargo test -p align_driver --test return_provenance --test fn_values --test per_unit` | `bench/library_boundary/run.sh provenance`: `summary-inference`, `indirect-return` |
| L2c | `cargo test -p align_driver --test move_return_cleanup --test owned_tagged_payloads --test per_unit_codegen` | `bench/library_boundary/run.sh move-return`: `copy-return-control`, `move-return-none`, `move-return-some`, `move-return-err` |
| L2d | `cargo test -p align_driver --test borrowed_params shared_`; `cargo test -p align_driver --test return_provenance` | `bench/library_boundary/run.sh shared-borrow`: `by-value-call-control`, `shared-borrow-call`, `copy-aggregate-value-control`, `copy-aggregate-shared-borrow` |
| L2e | `cargo test -p align_driver --test borrowed_params exclusive_`; `cargo test -p align_driver --test out_params --test analysis_coverage` | `bench/library_boundary/run.sh exclusive-borrow`: `exclusive-copy-control`, `exclusive-copy-call`, `exclusive-move-replace` |

The following table is the normative L2b-a1 effect-evaluation inventory, not a claim that one test
directly exposes every private EffectScan cell. Every row requires fully terminating, mixed, and
all-fallthrough behavior where the syntax admits them. A fully terminating case retains effects and
diagnostics produced before termination, excludes the listed dead state, and reports
non-fallthrough. A mixed branch visits every statically reachable alternative, excludes
produced-value state from its terminating alternatives, and retains the continuation and
produced-value state of every fallthrough alternative. The same-shape all-fallthrough twin retains
all listed state and reports fallthrough. `effect_source_order_closure_matrix` directly inspects the
nested accepted-break function-value boundary.
`pipeline_terminal_snapshot_action_order_matrix` checks a terminating-`init`/all-fallthrough
`reduce` pair through the final inferred Pure/Impure result: the stopped twin excludes its impure
stage action while the live twin retains it. The shared formation/action helpers and exhaustive
terminal match route every stage-bearing terminal through the same ordering seam.
`analysis_coverage` separately proves the corresponding final Pure/Impure `par_map` decision.
Other rows are observed through their existing focused sema/driver tests and the exhaustive match;
no test claims direct inspection of private EffectScan cells.

| Effect evaluation site | Terminating discriminator | Dead state excluded / fallthrough state retained |
|---|---|---|
| block statement and tail | an earlier statement returns, breaks, exits, aborts, or evaluates a diverging expression | every later statement and the tail, including their call and boundary state |
| `Let`, `LetTuple`, `Assign`, and field/element assignment | initializer, earlier tuple/member/index, or assigned value terminates | later child evaluation, destination local/concrete boundary, and the assignment itself |
| index/element/vector write | receiver/index/value terminates in source order | later operands and view-write impurity; the completed twin records the write |
| direct call and arguments | an earlier argument terminates | later arguments, named-call dependency or `print` impurity, parameter/concrete boundary joins, and call result state |
| indirect call and arguments | callee or an earlier argument terminates | later arguments, `consume_fn_value`, target dependency, unresolved-dispatch/Unknown, parameter joins, and call result state |
| aggregate, constructor, index, range, builder, raw, and I/O expressions | an earlier eager child terminates | every later child and the enclosing operation's own call, impurity, or boundary state |
| closure, node, reducer, and stage captures | an earlier capture terminates | later captures, lifted/stage dependency, capture join, and node result state |
| pipeline source, stage formation/action, and terminal | the source, an earlier stage operand/capture, or an earlier terminal argument terminates; an intervening argument invalidates the owner of an already-formed source or view capture | later stage/terminal operands and snapshots plus terminal state are excluded; stage and terminal action, `parmaps`, named dependencies, callback-origin joins, and result boundary join only after all pre-terminal operands fall through; source/view owner invalidation rejects before action for direct, zip, and scanner shapes; function-value captures cannot become more Pure after formation because assignment joins effect state monotonically; the direct stopped/live pair covers `reduce(init, f)`, while the shared helpers and exhaustive match cover `sum`/`count`/`any`/`all`/`min`/`max`/`sort`/`sort_by_key`/`to_array`/`map_into`/`partition`/`par_map`/`scan` |
| explicit `return value` | `value` terminates before the return edge | return boundary join; reachable payload effect remains |
| accepted and rejected `break value` | the payload terminates before the break edge | accepted outer loop-result join; a rejected break never joins in either case but still visits its reachable payload |
| `if` | condition terminates, both arms diverge, or exactly one arm diverges | a dead condition excludes both arms; both-diverging excludes a result; mixed arms retain only the fallthrough result |
| `match` | scrutinee terminates, all arms diverge, or only some arms diverge | a dead scrutinee excludes bindings/arms; all-diverging excludes a result; mixed arms retain only fallthrough results |
| `else` unwrap | operand terminates or fallback diverges | a dead operand excludes both edges; a diverging fallback remains diagnostically visited but only the success edge contributes a result |
| short-circuit `&&`/`||` | LHS terminates or the conditional RHS diverges | a dead LHS excludes RHS; a diverging RHS contributes reachable effects while the short path still falls through |
| `?` | operand terminates or its Err edge returns | a dead operand excludes both edges; otherwise Err joins the implicit return effect and only Ok continues |
| `map_err` | receiver expression or mapper-value expression terminates | later mapper/call/boundary state is excluded in evaluation order; after both expressions fall through, unchanged Ok and mapped Err result joins remain conservatively reachable because L2b-a1 has no callee-divergence summary |
| loop and projection queries | body statement/payload diverges, the loop has no reachable break, or a block/branch tail is dead | no dead backedge/break join or `fn_value_effect`/`projected_fn_effect` resurrection; reachable accepted breaks converge monotonically and repeated fixpoint passes are idempotent |

The L2b-a1 MIR evidence ledger states the direct assertions made by each named owner:

| MIR payload product | Direct MIR assertion | Driver/LLVM observation |
|---|---|---|
| same-target nested accepted break; explicit `return`; `process.exit`; `process.abort`; fully diverging transparent block/`unsafe`/`arena`/`task_group`; all-diverging `if` and `match`; diverging loop | `terminating_break_payload_emits_no_outer_edge` proves no outer store, Unit fallback, source null, iteration Drop, cleanup transfer, frame read, or goto | a Copy `str` case proves LLVM never stores Unit into the typed loop-result slot |
| one terminating and one fallthrough `if` arm; the same `match` product | `mixed_break_payload_preserves_outer_edge` proves the join exists iff it has a reachable predecessor and only the fallthrough path emits the outer edge | both runtime selections return the typed value selected by the inner or outer edge |
| inner loop break yields the outer payload; terminating `else` fallback with fallthrough success; returning `?` Err with fallthrough Ok; diverging short-circuit RHS with fallthrough short path | the same mixed owner proves the outer edge survives every positive continuation | runtime success/short selections return the outer value and return/termination selections preserve their inner edge |
| owned `string` selected by a mixed payload with a live loop-iteration owner | the mixed MIR owner proves one result transfer, one source clear, one reachable iteration cleanup, and no cleanup statement after a terminated block | `return_provenance` observes the selected owned values through their returned lengths and successful process exit; it makes no allocator/leak-count claim |
| stage-bearing array terminals and JSON-scanner reducers sharing the source/stage/terminal preparation seams | `terminating_pipeline_operand_emits_no_terminal_state` proves a terminating `reduce` init emits no reducer action/loop and that an already completed owned source is still cleaned; `pipeline_terminal_snapshot_action_order_matrix` proves the stage and reducer captures are distinct preheader SSA snapshots and neither is reloaded in the loop; `pipeline_terminal_source_shape_parity` applies the preheader/action classification to fixed, dynamic, zipped, and JSON-scanner sources | the sema stopped/live pair proves the corresponding effect boundary; `return_provenance` observes array reduce/scan values, and `m5::json_scan_reduce_fold` observes the scanner reduce value; no runtime allocation/drop-count claim is made |
| source-owner snapshots across terminal arguments | `pipeline_source_snapshot_owner_invalidation_matrix` covers a direct owned source, a zipped constituent, a JSON-scanner backing owner, and an `if`-selected owner; each includes mixed-branch invalidation, while a Move-`else` success-container reassignment proves transfer does not create a false owner, and return- and outer-break-terminating argument twins prove no later action-boundary diagnostic or saved snapshot is manufactured | MoveCheck's analysis-only snapshot entries participate in the same invalidation, borrow-preserving control-result root union, branch joins, saved loop-break states, and fixpoints as local borrowers; Move `else` success and loop-result transfers deliberately contribute no old-container/source root; MIR source-shape parity alone cannot close this analysis invariant |
| mutable local captured by a stage before `reduce`/`scan` `init`, and by the terminal callable afterward | the MIR snapshot test proves the stage snapshot precedes the terminal-argument mutation, the terminal snapshot follows it, and neither load appears in the loop body | `lambda::pipeline_captures_snapshot_in_written_operand_order` executes reduce/scan twins and observes the pre-argument stage value plus post-argument terminal value; the existing Copy-only pipeline-capture restriction means no hidden allocation or Drop is introduced |

The remaining analysis cells have direct owners rather than being inferred from the MIR/runtime
rows. `pipeline_terminal_dead_state_isolated_across_analyses` places an owner invalidation and a
later reducer capture behind a terminating `init`, then proves the dead capture does not join
reachable borrow state. `pipeline_terminal_dead_hir_is_finalized_and_linted` places the
unnecessary-heap pattern in a dead `init` tail and proves finalization still emits the lint without
contributing reachable flow state.
`pipeline_capture_owner_invalidation_is_rejected` snapshots a borrowed `str` stage capture and
replaces its owned `string` source in `init`; the exact borrow-owner diagnostic proves the snapshot
cannot dangle before action. `pipeline_terminal_source_shape_parity` directly constructs fixed,
dynamic, zipped, and JSON-scanner-reduce HIR twins and applies the same snapshot/action
classification. `pipeline_terminal_snapshot_action_order_matrix` exercises the common sequential
stage/reducer call-argument seam, so a helper that reloads either capture in the loop fails directly.
`zip_pipeline::pipeline_terminal_source_order` and `m5::json_scan_reduce_fold` are the successful
runtime observations for their source classes.

Acceptance:

- interface import rejects direct, mutual, permuted, and `Id`-exposed positive growth cycles; accepts
  the `Sink` twin, the documented convergent transform, constant replacement, a whole local nominal
  actual, and zero-weight permutation/duplication;
- direct `A<box<T>>` identity growth rejects while local nominals below an exposed opaque actual do
  not create dependency edges; composed `Id<box<T>>` and `Id<fn(T) -> T>` wrappers do not expose
  `T` to an enclosing local consumer; parallel zero and positive edges still reject;
- a deep acyclic MIR type chain validates and a deep malformed inline cycle rejects with
  `CodegenError`, both through the iterative graph walker rather than the process stack; a
  multi-invalid graph reports the first stored child before later siblings;
- caller-before-callee chains and mutual named recursion converge to the same canonical return roots
  in whole-program and per-unit checking, only changed callees requeue their direct callers, and a
  dependency's ordinary exported `lambda0` function is inferred as named rather than skipped as a
  lifted lambda;
- duplicate local/type-parameter keys, function/struct/sum type-parameter local-definition
  shadowing, duplicate-before-shadow multi-invalid precedence, unresolved
  parameter-with-arguments, wrong local/source-builtin arity, unresolved bare names,
  exact qualified-local names,
  unit-prefix-but-foreign names such as `foo.bar.Type`, other foreign-qualified leaves, malformed
  nested function types, and multi-invalid precedence have exact semantic-import tests;
- compiler-produced public `Option` and `Task` definitions validate for import; a bare
  source-builtin spelling and an exact qualified local spelling resolve with the same precedence as
  sema, while `Task` remains a local source name rather than an invented interface builtin;
- public definitions named `Error`, `argon2_params`, or `regex_match` validate as ordinary
  non-entry local nominals; same-unit bare references resolve to them, exact unit-qualified
  references resolve to them for consumers, and a bare reference after a local miss resolves to the
  builtin alias. Explicit `core.Error`, `crypto.argon2_params`, and `regex.regex_match` references
  retain builtin capability classification. Producer-only entry declarations of the three names
  reject before interface publication, so semantic import has no `ReservedLocalType` failure;
- the asymmetric implementation extends that same parameterized rule to its six key names:
  same-module locals win bare lookup, bare misses need no import, `crypto.*` forms require
  `std.crypto`, and entry collisions reject; its crypto carrier/interface owner lands atomically
  with the new spellings;
- a unit named `json` with a public local `doc` definition still resolves qualified `json.doc`,
  `json.kind`, and `json.scanner<...>` as source builtins; a non-shadowing type-parameter spelling
  reused by a resolvable builtin application follows that builtin, a type parameter matching a
  local declared type rejects as forbidden shadowing, and a truly unresolved parameter application
  reports `TypeParameterWithArguments`;
- producer sema and semantic import both reject duplicate and local-shadowing type parameters on
  generic functions, structs, and sum types in stored order; when one occurrence is both duplicate
  and shadowing, `DuplicateTypeParameter` wins and no second shadow diagnostic is emitted for it;
- semantic import rejects a generic body whose declaration kind, name, ordered type
  parameters or bounds, function parameter modes/types, return type, struct fields/layout
  attributes, or sum variants differs from its structured record, before rendering consumer source;
- compiler-produced generic function, sum type, and over-aligned struct fragments all validate and
  round-trip; a forged structured generic `layout(C)` struct rejects with
  `GenericCLayoutUnsupported`; fragments containing an extra `pub`, module/import, second item,
  trailing token, wrong declaration kind, or malformed syntax reject with the recorded
  C-layout-before-syntax-before-mismatch precedence;
- checked HIR records source declarations, generic monomorphs, and lifted lambdas with distinct
  `FnOrigin` variants; lifted counts are exact, and source entry/public flags determine exportability
  without a parallel stored boolean;
- a reachable fixed return or loop break followed by an unreachable parameter-returning exit
  produces `None`, keeps the caller owner usable, and contributes no dead loop post-state in either
  whole-program or per-unit checking;
- a diverging first child followed by an unreachable parameter-returning operand, call argument,
  aggregate member, bound, or index contributes no parameter root, and a `return` followed by a
  dead `break` does not make its loop fall through;
- a reachable conditional return or direct-call dependency remains in the inferred summary; when
  one short-circuit/`if`/`match`/`else` alternative diverges and its sibling falls through, only the
  sibling post-state survives; a conditional reachable `break` contributes loop value/post-state
  while a dead break contributes neither;
- a syntactically reachable `break` rejected inside an `arena` or `task_group` nested in its loop
  reports the region-scoped-break diagnostic without a debug assertion or panic and does not make
  that loop produce a value; a same-function unreachable accepted break cannot combine with the
  rejected edge to manufacture fallthrough. The exact recovery matrix also covers a nested loop
  whose break belongs only to that inner loop, a rejected region-nested break that prevents a later
  outer break from becoming reachable, a loop created inside an already-active arena/task-group
  baseline, accepted breaks through a plain block and `unsafe`, lambda isolation, an accepted
  control edge with an invalid payload, region-diagnostic-before-nested-payload-diagnostic ordering,
  and a diverging break payload that never reaches its enclosing break. A rejected `break value`
  additionally records `accepted == false` in HIR, contributes no effect-result join, EscapeCheck
  loop-exit edge or break-escape diagnostic, MoveCheck loop post-state, return-provenance root, or
  MIR loop-exit edge, leaves later syntax unreachable in every analysis, and still visits the
  payload for independent nested diagnostics. The direct checked-HIR owner records payload identity
  and `accepted == true` for an ordinary break plus payload identity and `accepted == false` for
  region-, lambda-, and outside-loop-rejected breaks; type finalization and MIR statement-span
  selection preserve that payload for either bit, `UnnecessaryHeapScan` visits it for either bit,
  and both `hir_stmt_diverges` and MoveCheck's walked-statement classifier report non-fallthrough
  for either bit. The effect owner proves that a rejected payload's own nested effect violation is
  still visited while its function-value effect does not join the loop-result boundary. Driver
  recovery fixtures separately retain a nested type error, a nested effect error, a payload-internal
  ownership/use-after-move error, and a payload-internal escape error; they also prove that no outer
  break-escape diagnostic, loop move/borrow post-state, post-loop use-after-move diagnostic, or
  return root is manufactured. The direct malformed-HIR MIR owner lowers a rejected break without a
  loop frame to exactly `Term::Unreachable`, emits no payload runtime statement or side effect,
  result store, moved-source nulling, iteration drop, cleanup transfer, or exit edge, and does not
  lower a following HIR statement. The accepted-break MIR owner separately proves that a payload
  terminating through an inner accepted break, explicit return, `process.exit`, `process.abort`, or
  a diverging nested block/`if`/`match`/loop emits no enclosing result store, Unit overwrite,
  moved-source nulling, iteration drop, cleanup transfer, loop-frame lookup, or outer exit edge;
  direct MIR assertions and a Copy `str` LLVM case prove that the typed result slot is never
  overwritten by Unit. A separate mixed-path owned `string` runtime case observes the selected
  values through their returned lengths and successful process exit. Its direct MIR owner proves
  one selected result transfer, source clear, reachable iteration cleanup, and no cleanup statement
  after a terminated block; no allocator/leak-count claim is made. The effect owner visits
  reachable effects inside a terminating payload, stops before all later statements and the dead
  tail, and joins no outer loop function-value boundary; an end-to-end `analysis_coverage` case
  accepts the resulting Pure callback at `par_map`, while a paired ordinary accepted-break case
  still joins and rejects an actually Impure callback;
- a Move owner remains usable after a shared borrow;
- moving from a borrowed binding is rejected;
- a returned view dies when the caller owner moves/drops;
- `borrow mut` rejects later use of an older view;
- a call rejects an overlapping by-value `str`/slice or recursively view-bearing aggregate beside
  `borrow mut` of its owner; L3 applies the same completed alias engine to `resource_ref`;
- the same recursive rejection covers every peer mode (`ByValue`, `Borrow`, `BorrowMut`, and
  `Out`), including distinct aggregate holders rooted in the invalidated generation;
- shared borrowing a Copy aggregate reads through caller storage without a structural copy, and
  mutable borrowing a writable Copy aggregate updates caller state;
- `borrow: T` and `out: region` remain legal parameter names through contextual lookahead;
- function-value binding and indirect calls retain all four parameter modes exactly;
- borrow-returning function-value joins union return-borrow/region summaries, and an unresolved
  higher-order parameter uses the all-compatible-input summary;
- a zero-argument capturing closure may return a captured `str` or slice only while its environment
  and captured owner live; direct/indirect calls, target joins, and moved function values preserve
  those exact roots. L3 adds `resource_ref` and L4 adds explicitly region-owned values to this
  already-shipped capture-root engine;
- a synthetic field selector returning a struct, tuple, fixed array, or sum that contains a nested
  view records its receiver parameter root recursively rather than treating the outer non-view type
  as owner-free;
- direct, indirect, and imported `Result<Option<MoveStruct>, Error>` returns preserve the selected
  dynamic cleanup bit on `Ok(None)`, `Ok(Some(...))`, and owned `Err` paths; L4 adds caller-selected
  region-owned success values without changing this ABI;
- replacing an owned pointee through `borrow mut` drops the old value exactly once and installs the
  new cleanup bit; leaving an unchanged pointee does not drop it in the callee;
- an indirect identity over a by-value recursively view-bearing Copy aggregate transfers its
  embedded owner provenance to the result; the owner cannot drop early. L3 extends the same summary
  to dependent children and Move view aggregates once those types exist;
- a deterministic exclusive-state shaper is Pure, while captured mutation and unsafe/I/O remain
  Impure;
- imported and same-unit functions produce identical diagnostics;
- corrupt interface summaries fail closed.

### L3 — opaque resource and `resource_ref`

Scope:

- resource declarations and generic identity;
- `pub` internal Drop-hook validation and producer-owned hidden thunk/interface linkage;
- construction/dependent-construction/borrow/raw-view/raw/transfer intrinsics;
- recursive resource/ref type classes;
- exactly-once MIR cleanup.

Acceptance:

- normal return, `?`, branch, loop, reassignment, and aggregate cleanup call Drop once;
- the hook is a `pub` function with an `unsafe {}` body in an allowed `internal` module;
- interface-only cleanup links through the generated hidden support thunk without importing the
  hook module;
- `into_raw` on a standalone resource root suppresses Drop; a field, element, projection,
  borrowed/out parameter, or temporary is rejected;
- null/native failure is handled before construction;
- a ref cannot survive owner move/drop/mutable borrow;
- a dependent resource prevents parent move/drop/mutable borrow until the child drops;
- `resource_ref` hidden recursively in each `ByValue`/`Borrow`/`BorrowMut`/`Out` peer or in a
  distinct aggregate holder rejects overlap with mutable borrowing of its owner;
- a captured `resource_ref` remains tied to its owner generation through direct/indirect calls,
  target joins, and moved function values;
- an indirect identity over a dependent child or Move view aggregate transfers the embedded parent
  provenance to its result and prevents early parent move/drop;
- a raw-derived `str`/slice cannot outlive the supplied resource generation;
- emitted MIR contains `ResourceFromRawBorrowed` with the exact parent generation and
  `ResourceViewFromRaw` with the complete validation plan; no generic raw cast substitutes for
  either operation;
- invalid native pointer/length pairs never become safe views;
- another package cannot construct or extract the resource, including in `unsafe`;
- a root resource, raw-only internal Drop hook, and driver descendant type-check with an acyclic
  import graph; the internal hook module cannot import the root;
- no resource enters `spawn`.

The F-A implementation closure matrix is authoritative while L3 is built. It follows the already
settled public contract above; it does not introduce a second resource strategy or a database-named
compiler path.

| Closure cell | Required implementation closure | Owner evidence |
|---|---|---|
| declaration and type formation | Parse contextual `resource Name[<P...>] = fully.qualified.hook`; resolve one nominal id and generic arity per canonical module path; represent owning resources and `resource_ref<R>` explicitly in HIR, MIR, source/interface types, canonical graphs, mangling, printing, layout, and LLVM pointer ABI | parser contextual-word matrix; `resource_ownership` declaration/type positives and duplicate, arity, unresolved-path, reserved-shape negatives; interface byte/hash golden |
| hook validation and thunk identity | Resolve only a `pub fn(raw) -> ()` in the declaring package's canonical `internal` subtree; reject generic, capturing, result-returning, private, foreign-package, root-import-cycle, and non-unsafe-body hooks before codegen; synthesize one producer-owned support thunk with canonical symbol, representation version, and ABI fingerprint | `resource_ownership` hook diagnostic precedence; interface round trip/corruption; LLVM definition/import declaration and linker parity |
| construction and privilege | `from_raw` requires an expected concrete resource, unsafe context, declaring-module descendant privilege, and a non-null contract; `from_raw_borrowed` additionally snapshots exactly one `resource_ref` generation; another package cannot construct even in unsafe | direct/imported privilege matrix, expected-type and null-precheck diagnostics, exact MIR `ResourceFromRaw`/`ResourceFromRawBorrowed` records |
| ownership transfer | Move-in, move-out, return, assignment, branch/match/else/`?`/`map_err`, loop joins, and aggregate construction use the existing path-local cleanup bit; `into_raw` accepts only a standalone initialized owned local or by-value parameter, nulls its source, and suppresses Drop | MIR cleanup assertions and runtime drop counter across all completion kinds; projection/field/index/temporary/borrowed/out/uninitialized negatives |
| exactly-once Drop and replacement | Every resource leaf contributes a thunk-backed DropPlan leaf; normal exit, early exit, discard, reassignment, aggregate recursion, and imported cleanup call the producer thunk once; replacement drops the old live value before installing the new pointer/bit | `resource_ownership` drop-count matrix; MIR order; whole/per-unit executable parity; malformed resource/thunk ids fail before LLVM emission |
| resource reference provenance | `resource.borrow` is safe and public, produces one-pointer Copy `resource_ref<R>`, and records the exact owner generation recursively through struct/tuple/sum/Option/Result, function values, captures, joins, direct/imported/indirect calls, and monomorphization | reusable-ref positives; owner move/drop/replacement/mutable-borrow stale negatives; captured/joined/moved function-value and whole/per-unit parity |
| dependent resource provenance | A child created by `from_raw_borrowed` is Move, carries its parent generation across move/return/aggregate/function-value paths, blocks parent move/Drop/`borrow mut` while live, and releases the dependency only after child Drop | child-before-parent runtime order, direct/indirect/imported identity, branch/loop/early-exit matrix, parent overlap negatives |
| all-peer exclusivity | Extend the shipped `ByValue`/`Borrow`/`BorrowMut`/`Out` recursive provenance walker to resource refs and dependent children, including distinct aggregate holders; diagnostics remain order-independent between same-unit/imported calls | parameterized all-peer alias owners for direct and nested roots in both argument orders |
| raw extraction and checked native views | `raw` accepts only `resource_ref<R>` under unsafe descendant privilege; `view_from_raw` emits typed MIR with resource id, owner generation, view kind, and exact null/length/alignment/UTF-8 plan, returning `Option<str>` or `Option<slice<FFIScalar>>` | exact MIR and LLVM shape; empty-null success plus negative/unrepresentable length, non-empty null, misalignment, invalid UTF-8; raw-owner escape negatives |
| non-Send and excluded shapes | Resource/resource-ref types are rejected in `spawn`, Copy fixed-array elements, pipelines, FFI signatures, print/equality/order/hash, and unsupported dynamic collections; one-owner struct/sum fields remain legal and recursive | structural classifier owners and fail-closed malformed HIR/MIR tests; no catch-all classification |
| interface, cache, and separate compilation | Serialize nominal identity, generic arity, representation version, thunk symbol, and ABI fingerprint; rebuild ids independent of declaration order; include producer object linkage for any consumer Drop; reject duplicate/noncanonical/malformed metadata before side effects | codec bytes/digest goldens, declaration-order determinism, corrupted metadata precedence, whole/per-unit object/link/run parity |
| end-to-end and measurement | Whole-program and per-unit execution agree for construction, borrow, dependent child, transfer, and cleanup; emitted calls and native views add no hidden allocation or reflection | focused L3 owners, `scripts/test-pr.sh`, applicable Clippy, resource/ref/view microbenchmarks, and LLVM IR inspection |

F-A is intentionally one consumer-complete capability even when it exceeds roughly 1,000
hand-written changed lines. Splitting the type/interface producer from the linkable thunk leaves
consumer Drop uncallable; splitting provenance from construction admits dangling refs or dependent
children; splitting MIR cleanup from LLVM lowering leaks or double-drops an already accepted Move
type. Intermediate commits therefore remain compiling owner-backed checkpoints on one branch, not
publishable partial resource semantics.

### L4 — named region capability

Scope:

- parse `arena name {}`;
- builtin `region` type and restrictions;
- function argument propagation;
- `clone_in`.

Acceptance:

- ordinary functions allocate into the exact caller-selected arena;
- region/result escapes are rejected across direct, branch, loop, `?`, closure, and module paths;
- a captured explicitly region-owned value remains tied to that region through direct/indirect
  calls, target joins, and moved function values;
- the shipped worker-sendability gate rejects the region capability for `spawn`, `par_map`, and
  nested callable environments independently of purity and
  before worker publication or allocation;
- anonymous and named arena cleanup are byte-identical except for the bound handle;
- no thread-local ambient allocator is introduced.

### L5 — deterministic static inputs and Query/command artifacts

Scope:

- recognized-constructor discovery;
- whole-body descriptor placement and unique item identity;
- safe path resolution and SourceMap registration;
- frontend/impl cache keys;
- versioned Query/command artifacts and interface summaries;
- generated `QueryStatic`/`CommandStatic` data skeletons, including per-driver bind plans and checked
  state.
- structural Params/Row contracts/fingerprints and QueryMeta plan data, plus binder/decoder ABI
  versions in artifact bytes,
  reproducibility checks, and cache keys.

Acceptance:

- changing only `.sql` misses the producer object cache;
- a descriptor accepts exactly one whole-body static constructor; nested, conditional, multiple,
  generic, argument-taking, and helper-wrapped forms fail before artifact creation;
- two descriptor functions in one module receive distinct Query/command IDs and artifact/thunk slots;
- Query/command semantic fixtures match checked-in byte and digest goldens, and those artifacts
  decode and round-trip byte-identically with the exact magic, endian, top-level/nested field order,
  fingerprints, ABI versions, option payloads, spans, and permitted-driver order from §6.2;
- unchanged consumers still hit when the public Query/command contract is unchanged;
- public Params/Row/restriction changes invalidate Query consumers, and public
  Params/restriction changes invalidate command consumers;
- a same-path Params/Row field name/order/type/Option/reachable-definition edit changes its
  structural fingerprint, artifact digest, checked-metadata match, and generated thunk plan;
- absolute/escaping/symlink paths fail;
- a U+0000 byte in file or inline SQL fails at its exact source span before artifact generation;
- a shadowing local or same-spelled user function does not read/register a file;
- a stale `StaticInputManifest` is rejected after source/import/schema identity changes;
- creating, changing, or deleting the exact per-driver checked-metadata path invalidates a matching
  manifest/cache entry for both CheckedOptional and CheckedRequired; an Any descriptor tracks both
  drivers without scanning a directory;
- source SQL hash stays stable across driver selection, while PostgreSQL wire hash/rewrite map
  changes deterministically with placeholder ordinals;
- inline SQL uses `Inline { query_id }`, decoded literal bytes, and a decoded-byte-to-`.align` span
  map; no fake filesystem path enters identity or diagnostics;
- Query and command both retain source/wire/occurrence/bind/checked/cache identity; command omits
  only Row/result/decode, and command bind never uses reflection;
- D1 keeps a separately compiled Query's Declared QueryMeta plan available without
  source/interface/artifact file I/O; D12 owns the corresponding checked-plan materializer and
  cross-unit row test;
- CheckedRequired validates every permitted driver, while CheckedOptional preserves an explicit
  mixed per-driver state;
- artifact bytes are reproducible across checkout roots and process runs.

#### L5a artifact model/codec implementation closure matrix

L5 is delivered in consumer-complete checkpoints. L5a owns the producer-independent semantic
artifact model and its complete v1 byte boundary; constructor discovery, filesystem registration,
and cache/manifest integration remain the next L5 checkpoint and do not widen this codec PR.
The implementation is intentionally above the ordinary 1,000-line hand-written threshold: the
single versioned wire contract must carry both Query and command envelopes plus every nested
record, validation rule, and independent reference encoder. Splitting those producers would
duplicate the same byte-order proof and publish no useful consumer between them; keeping the
semantic model, decoder, goldens, and corruption owners together gives D1 one stable artifact
boundary and one invalid-input failure domain.

| Closure cell | Required implementation closure | Owner evidence |
|---|---|---|
| semantic formation | Define the Query/command envelope, canonical Params/Row contracts, static options, source identity, occurrences, rewrites, bindings, checked metadata, driver entries, QueryMeta plan, and all explicit tags/versions. Query carries Row/decoder/QueryMeta; command omits exactly those fields. | Rust formation tests construct both complete fixtures and assert the command omission and permitted-driver ordering before encoding |
| canonical bytes | Encode every scalar, enum, option, sequence, structural definition, hash, span, SQL byte field, and nested record in the exact §6.2 order with little-endian fixed widths and checked `u32` lengths. Never use map iteration or host layout. | independent reference encoder and checked-in Query/command v1 byte goldens; byte-for-byte re-encode assertions |
| decode and validation | Decode untrusted bytes without panics; reject unknown versions/tags, truncation/trailing bytes, invalid UTF-8, duplicate/out-of-order definitions and rows, non-dense ordinals, invalid spans, ID/hash/fingerprint mismatch, driver/restriction mismatch, policy/evidence mismatch, and malformed canonical contracts. | corruption matrix mutates each validation class and asserts a stable fail-closed error; valid goldens decode to the exact semantic fixture |
| identity and digest | Enforce `query_id/command_id = unit + "." + item`, Inline identity equality, exact source/wire hashes, structural Params/Row fingerprints, and artifact digest over bytes beginning at magic. | identity/hash mutation owners plus independent `.digest` goldens using `Hash128::to_hex()` |
| ownership and allocation | Codec owns only transient Rust metadata/byte buffers; it creates no Align value, runtime/native object, source registration, cache entry, or process-global state. Reader never trusts lengths for unchecked allocation and never indexes untrusted input directly. | source inventory and malformed large-count/truncation owners; no runtime/FFI calls in the crate diff |
| ABI and consumers | Export one stable public module for D1 producer/driver code; preserve the existing interface codec/hash types and avoid compiler-known DB syntax in this checkpoint. | `align_interface` unit/integration tests and a compile-time public API smoke test; discovery/cache/descriptor cells explicitly deferred to the next L5 checkpoint |

The bounded review of candidate `e3e77bc4` found six validation gaps; the coherent fix closes them
at this same boundary. Database-checked Query entries now require evidence and Hash128 identities,
source-to-wire bytes and rewrite spans are reconstructed from the occurrence table, QueryMeta columns
and checked nullability are correlated with the Row contract, and PostgreSQL parameter options must
name a declared Params field. `review_findings_are_closed_at_the_artifact_boundary` owns the six
regressions; the existing independent byte encoder and goldens remain the canonical-byte owners.

#### L5b static-input registration/manifest implementation closure matrix

L5b consumes the L5a artifact model at the driver boundary. It owns only the deterministic input
registration and cache-index substrate; parser/sema constructor recognition remains the next
consumer that supplies resolved descriptor identities. The public driver API is still useful before
that consumer exists: a future frontend can register a resolved `File` or decoded `Inline` record,
derive the exact checked-metadata paths, and pass the manifest digest into the existing codegen-key
builder without a second cache or filesystem policy.

The capability intentionally keeps its formatted implementation above the ordinary 1,000-line
hand-written threshold: path/root policy, metadata dependency identity, the canonical manifest
codec, revalidation, and the single codegen-key consumer share one failure boundary and one owner
suite. Splitting any of those pieces would duplicate source/metadata identity and malformed-byte
proof while leaving no stable consumer between the producer and cache-index seams.

| Closure cell | Required implementation closure | Owner evidence |
|---|---|---|
| descriptor/source formation | Validate non-empty descriptor IDs, `Query`/`Command` kind, tagged `File`/`Inline` identity, producer-derived content hashes, bounded decoded inline SQL, NUL-free logical identities, descriptor-ID uniqueness independent of source/kind ordering, and canonical source/kind/descriptor ordering. Inline registration consumes decoded bytes only and never accepts a filesystem path; constructor identity discovery remains deferred. | `inline_does_not_resolve_a_file_and_identity_is_descriptor_bound`, oversized-inline owner, `manifest_codec_is_canonical_and_fail_closed`, NUL-bearing file identity rejection, independent duplicate-descriptor rejection, unsorted/duplicate decode rejection, and source inventory for the deferred constructor consumer |
| path resolution and ownership | Resolve path-free sibling `.sql` or an explicit literal relative to the defining `.align` module's lexical directory; canonicalize the defining and selected files only to validate regular-file status and project-root containment. Reject absolute paths, NUL/backslash/lexical `..`, non-regular files, missing files, and canonical symlink escapes outside the project root. Return exact bytes with no newline normalization. | `resolves_sibling_and_registers_root_relative_source`, `in_root_defining_symlink_uses_lexical_module_sibling`, plus `explicit_path_rejects_root_escape_and_symlink_escape`; the source inventory shows no ambient env/scan and the resolver maps read failures before publication |
| text and diagnostics | Validate UTF-8 and reject the first embedded NUL before artifact generation; register valid file bytes in `SourceMap` under root-relative `/` spelling and return the byte offset for diagnostics. | `invalid_text_reports_utf8_and_first_nul`, `resolves_sibling_and_registers_root_relative_source`, and the invalid-descriptor no-partial-SourceMap assertion |
| metadata dependency | Derive exactly one `.align-db/{sqlite|postgres}/{Hash128::of(descriptor_id.as_bytes()).to_hex()}.json` path per permitted driver; never scan the directory. Snapshot `Missing` or `Present(content_hash, format_version)` only after one bounded canonical-JSON parser consumes the complete v1 record (including exact top-level and nested key order, duplicate/unknown-key rejection, exact nested tags/types/ordinals, no trailing bytes, and descriptor/driver identity equality). The bounded reader rejects oversized files before allocation, and the parser enforces the exact control-character escape forms. Require the manifest's entries to cover the exact `driver_restriction` set in driver order, and validate each entry against its descriptor/driver-derived path. | `metadata_paths_are_exact_and_checkout_root_independent`, `metadata_snapshot_and_revalidation_track_missing_present_and_change`, `metadata_parser_consumes_complete_canonical_v1_record`, malformed nested source/search/parameter/column owners, `oversized_static_file_is_rejected_before_reading_contents`, `metadata_parent_symlink_cannot_escape_project_root`, permitted-driver omission/order owners, and the manifest path validator; directory scanning is absent by source inventory |
| manifest bytes | Encode magic `ALIGNINP`, version, source/import resolution digest, sorted static inputs, content hashes, and sorted checked-metadata states with bounded little-endian length prefixes. Decode untrusted bytes fail-closed on bad magic/version/tag, truncation, invalid UTF-8, NUL-bearing identities, independent duplicate descriptor IDs, duplicate/order, derived metadata-path mismatch, and trailing bytes. Content hashes are producer-derived at registration and opaque in the manifest record. | test-only reference encoder plus semantic↔byte round trip; corruption matrix and bounded-length owner |
| revalidation and action identity | Revalidate every exact `File` and metadata path before a pre-frontend hit; `Inline` never reads a file. Creation/deletion/content/format changes return a stale result. Manifest canonical digest composes with the existing codegen key through one helper, so checkout-root spelling and filesystem mtimes do not enter identity. | `file_deletion_is_a_manifest_stale_result`, `metadata_snapshot_and_revalidation_track_missing_present_and_change`, `equivalent_checkout_roots_have_identical_manifest_identity`, `codegen_identity_includes_static_inputs_without_path_or_mtime`, and the cache first-diff owner |
| allocation/side effects | Resolver owns only bounded byte buffers and caller-provided SourceMap entries; no process-global state, runtime/native call, directory enumeration, or partial manifest publication. Errors occur before a consumer can publish an artifact. | source safety sweep, malformed-length tests, no-FFI inventory, and failure-path no-write owner |

#### L5b candidate review finding-to-fix ledger

| Finding | Closure |
|---|---|
| P1: a manifest could omit a permitted driver's checked-metadata state because the descriptor's driver set was not represented | Add `driver_restriction` to `StaticInput`, encode/decode it, require exactly that restriction's driver list in canonical order, and cover omission/order plus round-trip owners. |
| P2: a missing metadata file below an escaping parent symlink was recorded as `Missing` before containment was checked | Walk to the nearest existing metadata parent, canonicalize it, and enforce project-root containment for both snapshot and revalidation; cover present and missing outside-parent symlink owners. |
| P1: checked metadata accepted a prefix-only JSON/version probe and therefore malformed or v2 files as `Present` | Replace the probe with one bounded canonical-v1 JSON parser that consumes the complete object, rejects unknown/duplicate/out-of-order top-level keys, and checks exact `format_version: 1`; cover malformed suffix, v2, and canonical complete-record owners in snapshot and revalidation. |
| P1: a metadata file could claim a different descriptor or selected driver at its hash-derived path | Decode the canonical top-level `descriptor_id` and `driver` strings and require equality with the requested descriptor/driver before recording `Present`; cover identity mismatch in the metadata snapshot owner. |
| P2: metadata and SQL reads allocated the complete file before enforcing the field bound | Open each canonical regular file through one bounded reader that checks the file length and caps the read at `MAX_FIELD_BYTES + 1` before publication; cover oversized metadata and preserve the same rule for static SQL. |
| P2: canonical metadata accepted `\\u000a` where the v1 codec requires `\\n` | Reject `\\u` forms for the five named control escapes while retaining lowercase `\\u00xx` for the remaining controls; the canonical parser owner covers the escape form. |
| P2: static-input callers could not name the public `Driver` argument type through `align_driver` | Re-export `Driver` beside `DriverRestriction` and retain the public API owner compile path. |
| P2: a decoded `File` identity could contain U+0000 | Apply the NUL-free text identity rule to every manifest string before path/key use; cover a decoded NUL-bearing file path. |
| P2: duplicate descriptor IDs were only rejected when source/kind/order also matched | Add an independent descriptor-ID uniqueness pass before canonical ordering and cover same-ID/different-source and same-ID/different-kind twins. |
| P1: the canonical metadata parser accepted syntactically valid but schema-invalid nested values | Replace generic nested JSON skipping with exact source-identity, enum, hash, option, array, nested-object, and dense-ordinal validation; enforce command/query and driver-specific semantic constraints, and add malformed source/search/parameter/column owners. |
| P0 (verified false positive): by-value matching of `MetadataState` was reported as a compile error | Run the owner `cargo check` and static-input tests against the exact candidate; Rust's place-pattern match compiles for this field and no production change is required. |
| P2: inline SQL bypassed the static-input field bound before cloning | Validate the descriptor and decoded inline byte length before allocating the owned byte buffer; cover oversized inline input and preserve the same bounded reader for file inputs. |
| P2: canonicalizing the defining `.align` changed the sibling base directory for an in-root symlink | Retain the lexical defining path for sibling/explicit candidate construction, while canonicalizing only for containment and regular-file validation; cover an in-root defining symlink whose sibling differs from the target path. |

#### L5c resolved-constructor discovery implementation closure matrix

L5c connects the checked frontend to L5b without yet inventing the D1 `pkg.db` type surface or
generated descriptor ABI. It observes only calls whose ordinary module/function resolution has
already produced one of the exact `pkg.db`, `pkg.db.sqlite`, or `pkg.db.postgres` constructor
identities. The package declaration still supplies the callable signature in this checkpoint; D1
owns compiler-known generic descriptor type formation, option semantics, SQL scanning, artifact
production, and generated `QueryStatic`/`CommandStatic` data. L5c therefore changes no HIR/MIR
variant and publishes no executable database API.

This capability is slightly above the approximate 1,000-line review threshold because discovery,
whole/per-unit publication, and the L5b bridge form one strict producer-to-consumer chain. Splitting
them would leave a dormant descriptor inventory or an unowned bridge and would duplicate the same
identity, failure-atomicity, and parity proof across two PRs; the combined boundary has one semantic
failure domain and one end-to-end owner matrix.

| Closure cell | Required implementation closure | Owner evidence |
|---|---|---|
| resolved identity | Recognize only the twelve exact common/SQLite/PostgreSQL Query/command file/inline callees after normal import, visibility, and local-shadow resolution. Same-spelled user functions, an unimported module, and a local shadow register nothing. | semantic owner with exact-target positives plus same-spelled, unimported, and shadowed negatives |
| descriptor placement | Inspect every structurally checked source-body occurrence, including unreachable alternatives and lifted closures. Admit exactly one call only when it is the complete `= expr` body of a named zero-argument non-generic function. Reject block, nested, conditional, repeated, argument-taking, generic, and lambda forms before publishing a descriptor. | one parameterized placement owner crossing body form, occurrence count, params, generics, nesting, control flow, and lifted closure cells |
| constructor inputs | Enforce the exact common/driver and file/inline arities independent of the package declaration. File accepts an absent path or one decoded string literal; inline requires one decoded string literal; every common/native option position is an explicit array literal. Record the defining file, literal span, decoded bytes/path, consumer kind, and driver restriction without reading a file. | arity/source/option-literal owner for all twelve identities, path-free/explicit file, decoded inline, and non-literal rejection |
| identity and publication | Derive `descriptor_id` only as canonical module path plus item name, retain private/public item status, reject publication from an already-invalid function, and sort the successful descriptor inventory by ID. Two descriptor functions in one module must receive distinct IDs and slots. | two-item identity owner, invalid-subtree no-publication owner, and deterministic ordering assertion |
| whole/per-unit parity | Return the same descriptor record from whole-program checking and from the real producer unit in interface-backed per-unit checking. Interface-only dependency bodies are never rediscovered by a consumer. | whole/per-unit semantic parity owner with one imported public descriptor |
| L5b bridge and side effects | Resolve every published File/Inline request through the existing L5b API, snapshot exactly the permitted drivers' metadata paths, construct one canonical manifest, and only then add deduplicated file bytes to `SourceMap`. Any failure returns the descriptor ID/span and publishes neither a manifest nor partial SQL source entries. | end-to-end sibling/explicit/inline manifest owner, two-descriptors-one-file SourceMap dedup owner, metadata-driver coverage, and late-failure no-partial-registration owner |

L5c review finding-to-fix ledger:

| Finding | Class-wide fix and owner closure |
|---|---|
| P2: a signature-invalid descriptor function could look clean when only the body-check diagnostic delta was inspected | Gate publication on every earlier error whose span is contained by the declaration as well as on body-check errors; cover an unknown return type whose `Ty::Error` absorbs the body constraint. |
| P2: two reads of one logical SQL file could disagree while both descriptors received one shared `SourceMap` file | Record the first byte snapshot for every logical file and reject any later descriptor read whose bytes differ before manifest or `SourceMap` publication; cover a deterministic replacement between two reads. |

The next L5 checkpoint consumes these records to form structural Params/Row contracts, scan SQL,
emit the versioned L5a artifact, and replace the descriptor function body with its producer-owned
static data/thunk reference. Until that checkpoint, L5c is an additive frontend/driver boundary and
does not claim that `pkg.db` exists or that a discovered constructor can be lowered and linked.

### L6 — region plain-struct builder

Scope:

- `RegionPlain` recursive classification;
- chunked region growth;
- compacting build;
- borrow provenance through pushed elements;
- cleanup on early return and build failure.

Acceptance:

- scalar/Option/plain-struct arrays build correctly;
- `all<R>` rejects non-`RegionPlain` Row contracts before execution;
- current-row views cannot be retained across `next`;
- `clone_in` values can be retained;
- a Pure helper may push through `borrow mut builder`, but cannot build/store/return it;
- no heap allocation occurs in the region form;
- exactly one compacting element pass occurs;
- resources/owned heap fields receive compile diagnostics.

The F-B implementation closure matrix is authoritative while L4 and L6 are built. It implements
the already settled region contract without introducing an ambient allocator or a database-named
compiler path. Symbolic generic `RegionPlain` bounds remain owned by L7; F-B closes the concrete
recursive classifier and every runtime/materialization path that L7 will later select after
monomorphization.

The concrete builder element record is one non-recursive compiler descriptor, not a widening of
the general `Scalar` payload class. It has exactly these shapes: `Scalar(Scalar)`,
`Vec(Scalar, lanes)`, `Mask(Scalar, lanes)`, `FixedArray(Scalar, length)`, and
`FixedStructArray(struct_id, length)`. The last four shapes freeze to one dedicated dynamic
aggregate-array type carrying the same descriptor; scalar and struct elements retain the existing
`DynArray` and AoS `DynStructArray` result types. Formation converts the resolved concrete `Ty` once,
and push, build, indexing, type display, Drop/region analysis, HIR validation, MIR remapping,
interface reconstruction, and LLVM layout consume that same record. The descriptor preserves
nominal struct/tagged ids for canonical remapping and is rejected before MIR when a lane scalar,
width, length, struct id, or result correlation is malformed. Direct source formation closes the
currently spellable vector/mask shapes; fixed-array descriptors also close monomorphized and
hand-built-HIR consumers without inventing a second fixed-array surface spelling before L7.

| Closure cell | Required implementation closure | Owner evidence |
|---|---|---|
| syntax, binding, and type formation | Parse both `arena {}` and `arena name {}`; bind `name` as the builtin Copy `region` type only for the block; reject construction, mutation, shadowing, storage, unsupported aggregates, FFI, every `spawn`/`par_map` worker transfer, and return | parser/formatter round trips; named/anonymous scoping positives; formation and escape diagnostic matrix |
| parallel-worker capture (`pkg.csv` implemented) | Effect and sendability are distinct facts. One fail-closed source/HIR authority consumes the existing `BorrowFact`/`CallableProvenance` graph for every `spawn`, staged `ArrayParMap` callable, and terminal callable. It rejects a direct region or one reachable through nested function-value environments before lifted-worker publication, MIR, generated identity, runtime call, or allocation; an explicit parameter root becomes a summary `params` ordinal, while a lifted environment slot becomes `captures`, and callers translate both. Only public explicit parameter ordinals serialize as interface-v6 `parallel_transfer_params`. Unknown target/environment facts conservatively select every compatible input, while a known empty environment passes. Codegen defensively rejects a direct `ArenaHandle` in handcrafted parallel MIR; checked HIR owns transitive opaque function environments. Sequential direct calls, local closures, `map`, and `reduce` continue to use the ordinary lexical region proof. | Existing spawned-region diagnostic plus `pkg.csv` direct and nested-closure spawn/par-map captures; direct malformed `SpawnTask`, `ParMapParallel`, and `ParMapReduce` MIR `ArenaHandle` captures; whole-program/per-unit/concrete-monomorph parity; known noncapturing and region-free capturing function positives; sequential Pure closure/map/reduce positives; no-publication assertions through checked and backend owners. |
| exact region identity | Give every named arena and `region` parameter a stable semantic identity; inside a callee keep each caller-owned region or borrowed builder symbolic and distinct from both `Static` and callee-local frame/arena storage, then discharge relationships between distinct symbolic parameters at each concrete call site; preserve returned and captured region ownership through direct/imported/indirect calls, function-value target joins, moved function values, captures, and monomorphization without collapsing distinct caller regions | sema provenance owners for direct, branch, loop, `?`, closure, imported, and function-value return paths; nested-callee-arena rejection for incoming regions/builders; canonical interface and whole/per-unit parity |
| explicit allocation and `clone_in` | Lower every region allocation with the exact capability operand; `clone_in` copies `str`/`bytes` backing storage and recursively copies view-bearing fields of a `RegionPlain` struct into that region, returns a value tied to `out`, validates each view size before allocation, and performs no heap allocation | exact HIR/MIR operand assertions; scalar/bytes/struct runtime content and lifetime positives; wrong-region, owned-field, and post-region escape negatives; LLVM call inspection |
| cleanup and exits | Begin each arena once and end it once on every returning completion path, including normal completion, return, `?`, branch, and loop exit; allocation/overflow hard errors remain process-terminating; named and anonymous cleanup are byte-identical apart from storing the bound handle; a borrowed incoming region is never ended by its callee | named/anonymous LLVM cleanup-shape comparison plus the existing arena completion-path owners; whole/per-unit executable parity |
| concrete `RegionPlain` classification | Recursively accept scalars, `Option`, fixed vectors/masks, fixed arrays, plain structs, and region-valid `str`/`bytes` views; reject resources, refs, raw, functions, builders, independently owned heap fields, and recursive unsupported shapes before execution. Convert every admitted top-level shape to the one exact builder-element descriptor before HIR; never truncate it through `Scalar` | table-driven classifier tests for nested positive/negative shapes and every descriptor discriminator; deterministic first-invalid-field diagnostics; source `vec`/`mask` formation positives; malformed HIR descriptor/result-correlation rejection |
| region builder formation and ownership | `array_builder<T>(out)` records its exact region and concrete element descriptor, is Move and bound to one mutable local, may be passed only as `borrow mut`, and cannot be stored, returned, captured, moved into a task, or built through an alias. A `borrow mut` builder parameter keeps symbolic caller provenance and the heap-only, region-only, or constructor-dependent allocation bound implied by `T`, so callee-local frame/arena views cannot be retained and a nested helper cannot assume heap ownership. A same-program direct body retains in the caller's builder only the exact reachable parameter roots inferred from its stores; recursive and wrapped direct helpers reach the same least fixed point, so a stored `clone_in(out)` retains `out` rather than the cloned source. Imported, indirect, missing-body, malformed, and unresolved calls conservatively retain every compatible view-bearing argument root, so an unavailable body cannot erase newly stored provenance. Builder-parameter function values remain outside the existing scalar-only first-class signature surface. | constructor/receiver/mode diagnostic matrix across scalar/vector/mask/fixed-array descriptors; exact direct/wrapped/recursive/control-flow store and imported/indirect fallback owners; clone/raw-source, callee-local nested-arena, nested-helper allocation-mode, and call-site wrong-region negatives; move/alias/capture/store/build negatives |
| chunked growth and push provenance | Builder headers and growth chunks allocate only from the selected arena; scalar/Option/plain-struct pushes copy exactly one initialized element; pushed views retain their source provenance, so a current-row view cannot survive `next`, while `clone_in(out)` can | runtime allocation counters and chunk-boundary data checks; sema current-row/clone provenance tests; exact element-layout MIR/LLVM assertions |
| compacting build | `build` consumes the owned builder, allocates one final contiguous result in the same region, performs exactly one element compaction pass, invalidates the builder, and returns the correctly typed region-tied array | runtime pass counter and 0/1/multi-chunk result tests; move/use-after-build checks; MIR source-nulling and returned-region assertions |
| failure and early cleanup | Invalid native layouts and overflow are rejected before allocation or copy; allocation exhaustion follows the existing hard-error arena contract; early return, `?`, branch, loop, and unfinished-builder exits leave no independently owned storage, never end a borrowed region, and let the enclosing arena reclaim all chunks | invalid-layout/overflow runtime owners plus the existing arena MIR cleanup-path suite; nested named arenas and helper-call coverage |
| interfaces, ABI, and cache identity | Serialize `region` parameters, exact return-region summaries, region-builder forms, and every concrete builder/dynamic-result element discriminator canonically; remap embedded nominal ids once; reject malformed metadata and builder/result mismatches before MIR/codegen; keep whole-program and per-unit ABI byte-equivalent | interface codec/hash goldens and corruption tests for each descriptor; declaration-order determinism; whole/per-unit object/link/run parity |
| end-to-end resource promises | Materialize scalar, Option, vector, mask, and plain-struct arrays through ordinary functions, including recursively plain fields and fixed-array append sources; prove no heap calls in the region form and one compacting element pass without weakening anonymous arena behavior | focused F-B driver suite, applicable runtime/interface/MIR owners, LLVM IR inspection, allocation/pass-count measurement, `scripts/test-pr.sh`, and applicable Clippy |

The `transitive-send-canonical-hash` review reopens the parallel-worker cell with this focused
implementation closure matrix. It changes no public syntax, HIR/interface record shape, MIR ABI,
runtime ABI, allocation, or cleanup rule; it completes consumption of provenance already present in
the checked compiler graph.

| Worker-transfer axis | Exact closure | Owner evidence |
|---|---|---|
| fact formation | Reuse the one `BorrowFact` trie and `CallableProvenance` target set. A direct region contributes its local/parameter root; each closure prefixes completed capture facts by exact `ClosureTarget` then `ClosureCapture`; a named/lifted noncapturing function contributes an empty environment. An absent target, malformed ordinal, or unavailable environment is unknown rather than empty. | direct region, region-free closure, noncapturing named/lifted function, one- and two-level region closure, missing-target and malformed-ordinal twins |
| construction, movement, and joins | Function-value binding, local move-in/move-out, reassignment/replacement, and every currently admitted `if`/`match`/`else`/`?`/`map_err`/loop result preserve the may-union of target-relative environment roots; a diverging arm contributes no value. Currently admitted aggregate function fields/arrays are Static/noncapturing and therefore carry an empty environment. Returning a function value or storing/escaping a capture-bearing environment through an aggregate remains rejected by the existing deferred first-class-closure boundary and cannot become a laundering route. Existing environment allocation/Drop ownership is unchanged. | parameterized local/control matrix with one unsafe reaching arm and one diverging arm; moved/reassigned nested closure; Static aggregate controls; unchanged rejection for capture-bearing return/aggregate escape; unchanged environment allocation and cleanup assertions |
| worker sinks | `spawn`, every staged `ArrayParMap` callable, and its terminal callable consume the same completed fact. A resolved local region rejects immediately; a current-function explicit parameter enters summary `params`, and a lifted environment parameter enters summary `captures`. The explicit `par_map` surface checks before choosing parallel versus sequential backend support. | spawn plus staged/terminal par-map, supported range and sequential-fallback shapes, direct and nested region negatives, explicit-param/lifted-capture summary twins, region-free positives |
| helpers, monomorphs, and units | Direct and concrete indirect calls translate the selected `parallel_transfer` params/captures through completed callee and argument facts before later mutable-argument effects. Imported interface-v6 `parallel_transfer_params` uses the same ordinal meaning; compatibility omission or unresolved indirect target selects every borrow-capable argument/environment. Concrete generic instances rerun the same analysis after substitution; no generic-lambda surface is invented. | direct/indirect/imported/compatibility-absent helpers; whole/per-unit equality; concrete `CsvDecode` monomorph inside the captured callable; unknown-target fail-closed control |
| checked-HIR replay | Source production and am-b4 replay use the same finite fixed point and compare the stored summary. Mutating a closure target, capture projection, local/parameter type, stored transfer root, imported root set, or effect rejects through all four lowerers before MIR. No second sendability bit or trusted producer assertion exists. | field-complete malformed-HIR mutations; replay/source diagnostic equality; canonical-empty program through whole/located/per-unit/located-per-unit entries |
| MIR and LLVM boundary | Valid checked HIR alone forms MIR. Analysis-local target/environment facts and transfer summaries are stripped, preserving MIR/cache identity. Codegen rejects a direct self-describing `ArenaHandle` capture in handcrafted parallel MIR before context/kernel/runtime publication; it does not infer an opaque `Fn` environment from MIR. | valid pre/post MIR structural identity; direct `ParMapParallel`/`ParMapReduce` malformed-MIR negatives; no context/global/declaration/call/allocation publication |
| failure and parity | Every rejection precedes lifted-worker identity, MIR, generated capability/kernel identity, context layout, runtime declaration/call, and arena/heap allocation. Sequential direct calls, closures, `map`, and `reduce` retain ordinary lexical region behavior. Effect replay remains an independent requirement. | no-publication counters/empty vectors at each boundary; sequential direct/closure/map/reduce positives; Pure/Impure × Send/non-Send Cartesian owners |

F-B is intentionally one consumer-complete capability even when it exceeds roughly 1,000
hand-written changed lines. Splitting named-region formation from its first allocator consumer would
publish a dormant capability; splitting the builder runtime from provenance would allow accepted
views to dangle; and splitting compacting build from cleanup would leave no safe, usable result.
Intermediate commits therefore remain compiling owner-backed checkpoints on one branch rather than
publishable partial region semantics.

### L7 — nested generic package APIs and `RegionPlain` bound

Scope:

- recursively represent/infer/substitute `Ty::Param` under `array`, `slice`, `Option`, `Result`,
  and top-level generic struct/sum/resource applications;
- permit those applications in generic function parameters, locals, and returns;
- add the closed structural `RegionPlain` builtin bound and abstract operation gating;
- canonical interface serialization and monomorphization keys for nested applications/bounds;
- preserve monomorphization-before-analysis/MIR and reject unsupported concrete container elements.

Acceptance:

- the `rows_stmt<P, R>` and `all<P, R: RegionPlain>` signatures in §7.4 compile as ordinary package
  functions without a compiler-known DB API;
- `query<P, R>`, `stmt<P, R>`, `rows<R>`, `slice<R>`, and `array<R>` substitute to concrete types
  before MoveCheck/EscapeCheck/MIR;
- interface-only and whole-program instantiation produce byte-identical canonical mono keys and
  equivalent diagnostics;
- a recursively plain Row satisfies `RegionPlain`; a resource, owned heap field, function, raw
  value, or builder fails with a bound diagnostic before codegen;
- no runtime dictionary, reflection table, trait object, or extra indirect call is emitted;
- generic recursion, declarations nested inside functions, explicit call-site type arguments, and
  newly unsupported concrete collection elements remain rejected without compiler panic.

The implementation dependency is a DAG, not a mandatory serial PR list:

```text
                         +-> L3 resource ---------+
L2 complete -------------+-> L4 region -> L6 -----+-> L7 generic integration
                         +-> L5 static artifacts --+-> prerequisite gate
```

L3, L4, and L5 may be implemented concurrently after L2 because none consumes
another's implementation. Plan them as three consumer-complete streams:

```text
F-A native resources        L3
F-B region materialization  L4 + L6
F-C static artifacts        L5
F-D package integration     L7 after F-A/F-B; prerequisite gate also waits F-C
```

F-B keeps named regions with the first region-backed builder consumer rather
than landing L4 as an isolated compiler seam. L7 closes
the generic integration after the resource and `RegionPlain` types exist. D0
native feasibility probes may run at any time. Only after the complete L1a–L7
gate is shipped may the first safe SQLite runtime/Query vertical slice begin.

#### L7 package-integration implementation closure matrix

L7 is one compiler capability because its first stable consumers are ordinary package functions
whose signatures combine nested generic collections, nominal package types, and the closed
`RegionPlain` bound. Splitting abstract signature formation from concrete substitution would leave
unpublishable `Ty::Param` state at the HIR boundary; splitting the bound from `array_builder<R>`
would make the canonical `all<P, R: RegionPlain>` helper either unusable or unsound. The
implementation therefore keeps every unresolved application in sema-only template state and
publishes only concrete monomorphs to analysis, MIR, interfaces, and codegen.

| Closure cell | Required implementation closure | Owner evidence |
|---|---|---|
| abstract type formation | Admit a direct type parameter under `array`, `slice`, `Option`, and `Result`, and as an argument of a top-level generic struct, sum, or resource application. Retain the exact nominal constructor plus ordered argument pattern in sema-only template metadata. `array_builder<T>` is admitted abstractly only for a `T: RegionPlain` template. A generic nominal nested under a builtin or another nominal remains outside L7 and rejects. No unresolved application becomes a runtime/HIR type of an emitted function. | `generic_owned_array_parameter_and_return`, `generic_slice_parameter_and_return`, `option_return_position`, `option_param_position`, `result_return_and_question_mark`, `generic_struct_with_type_param_argument`, `generic_sum_application_in_generic_signature`, `generic_resource_application_in_generic_signature`, and `deeper_abstract_nominal_applications_are_rejected`; `valid_hir_global_type_preflight_is_mir_identity` permits unreachable abstract templates but rejects a concrete root that reaches one |
| inference and substitution | Match expected return first, then arguments in source order, through every admitted constructor. Repeated parameters unify once; concrete leaves and nominal constructors must match exactly. Substitute the complete application before argument checking when known, and materialize the exact dynamic-array/slice/builder or nominal monomorph after inference. Collection substitution reuses the source collection element class and preserves the canonical `DynSliceArray` form of `array<slice<P>>`. | existing generic repeated/conflict/arity/uninferable owners plus `phantom_package_nominal_infers_from_expected_return`, `generic_owned_array_parameter_and_return`, `generic_slice_parameter_and_return`, `generic_function_value_slice_preserves_collection_scalar_semantics`, `generic_owned_array_preserves_chunk_array_representation`, and `nested_package_generics_match_whole_and_per_unit_compilation` |
| nominal applications | Intern abstract and concrete instances by canonical producer type identity plus ordered structural arguments for generic structs, sums, and resources. Abstract instances may exist only as unreachable template records during checking; final HIR compaction removes them and remaps every concrete nominal reference. Concrete instances reuse the existing declaration field/payload/drop rules and monomorph tables. Same-spelled private or unimported types never alias a package type. | `generic_struct_with_type_param_argument`, `phantom_package_nominal_infers_from_expected_return`, `generic_sum_application_in_generic_signature`, `generic_resource_application_in_generic_signature`, `region_plain_builder_remaps_a_concrete_generic_struct_element`, and the existing visibility/arity/recursion owners |
| `RegionPlain` bound | Add the exact builtin spelling `RegionPlain` to parsing, diagnostics, interface serialization/reconstruction, and generic bound identity. Abstract operations are limited to the region-backed `array_builder<T>` constructor, `push`, and consuming `build` path needed by `all`; concrete instantiation reruns the canonical recursive classifier before HIR publication. `RegionPlain` grants no equality, ordering, or arithmetic operation. | `region_plain_bound_builds_a_generic_region_array`, `region_plain_bound_rejects_owned_heap_fields`, `region_plain_builder_remaps_a_concrete_generic_struct_element`, and `region_plain_does_not_grant_equality` |
| monomorphization and interfaces | Whole-program and interface-backed consumers derive the same concrete package behavior from exact nested source types and the serialized bound spelling. Implementation-only instantiations do not widen a public contract. | `nested_package_generics_match_whole_and_per_unit_compilation` compares whole/per-unit acceptance and executable result and requires non-empty consumer MIR; `generic_resource_application_in_generic_signature` does the same for owning and borrowed generic resources |
| fail-closed boundary and runtime cost | Reject uninferable, unsupported nested nominal, invalid bound, collection-forbidden single-owner or over-aligned elements, and Move-element copy states without panic. Every emitted HIR type is concrete before MoveCheck/EscapeCheck/effect analysis; MIR/codegen gain no generic variant, dictionary, reflection table, trait object, or extra indirect call. | existing generic rejection owners, `deeper_abstract_nominal_applications_are_rejected`, `generic_fixed_array_rejects_copying_a_move_struct_value`, `generic_collection_substitution_rejects_single_owner_handles`, `generic_collection_substitution_rejects_overaligned_struct_elements`, the HIR validator abstract-template twins, and the repository source inventory for no MIR/backend generic additions |

The author-side matrix-to-diff pass must point every admitted abstract constructor to both its
concrete substitution owner and its no-publication owner. A reviewer finding that permits an
unresolved type to cross the HIR boundary reopens this matrix as a soundness-class defect.

### D14-A2 — static scalar-callback descriptors and generated SQLite C trampolines

The SQLite callback rail consumes existing first-class function signatures, inferred effects,
return provenance/cleanup ABI, generated-symbol identity, and static-descriptor validation, but it
does not expose an Align function value to C. `pkg.db.sqlite.function` is a trusted compile-time
producer. It accepts one exact named or noncapturing lifted target and emits the 32-byte v1
descriptor fixed by `pkg-design/db.md` §23 plus one generated C-ABI
trampoline. Capturing closures, externs, dynamically selected/open target sets, ordinary fieldless
descriptor construction, and incomplete imported effect/provenance facts reject before HIR
publication. This is a general generated-callback mechanism selected by an exact package producer;
it adds no `pkg.db` ownership exception, unsafe-callable source type, function-value ABI change, or
runtime callback registry.

The cross-cutting implementation closure matrix is:

| Closure cell | Exact compiler/package relation | Required owner |
|---|---|---|
| formation and type proof | Recognize only the exact scalar producer identity after ordinary import/visibility and argument formation. Resolve one direct source/imported/lifted target; require the exact signature, no captures/extern/open dispatch, and complete effect/return provenance/cleanup. Preserve source-order diagnostics and emit no partial descriptor HIR on failure. | direct/imported/lambda positives; capture/dynamic/extern/unknown-effect/signature/multi-invalid negatives; whole/per-unit source parity |
| checked HIR and MIR | Add one typed scalar-callback-descriptor formation record carrying exact `ProgramCall`, descriptor id, signature, effect, return summaries, parallel-transfer roots, and generated-family version. The HIR validator re-derives all fields from stored/imported facts, rejects invocation root 0 before publication, and strips the validated transfer metadata before MIR keeps its corresponding static-descriptor rvalue with no function-value environment. Every lowering entrypoint fails closed on a field mutation. | valid scalar record; mutation of every scalar/id/signature/effect/provenance/cleanup/transfer field; direct/imported/returned-view/concrete-or-unresolved-fn-value transfer negatives; source/monomorph/imported target; empty-program and checked-error paths |
| descriptor and identity | Serialize the exact 32-byte v1 record and NUL-terminated canonical identity; identity includes target, signature/modes, effect, return facts, the validated empty callback transfer fact, descriptor kind/version, and generated-family version. Correlate descriptor, relocation, callback target, transfer fact, and generated trampoline before LLVM construction. | semantic-byte goldens including the canonical empty-transfer marker; canonical graph/interface/cache keys; equal dedup and unequal noncollision; splice/length/NUL/relocation/transfer negatives |
| scalar C trampoline | Generate exact C `void(ptr,i32,ptr)` ABI, nounwind, fixed 127-value stack scratch, null-context/database-handle hard abort, byte-count-before-final-pointer native validation, immediate errcode-on-null-Text OOM handling, and stable non-null normalization for empty views. Then call one direct Align target, consume its dynamic Move-return cleanup, and emit exactly one SQLite result/error path for non-null context. No callback frame view, source owner, or native pointer escapes. | LLVM ABI/body golden; null-context/database-handle subprocess owners; exact accessor traces; empty null/non-null pointer normalization; argc/argv/value/result/error/Text-conversion-OOM mutation owners; return-provenance and cleanup-bit matrix; ordinary Err/hard-abort twins |
| native registration bridge | Guard every descriptor accessor behind the complete v1 validator and exact kind. Package registration receives only the generated C pointer; application data/destructor are null. No C callback declaration enters the extern-function-value path, native RuntimeKey table, or ordinary Align callable namespace. | guarded/unguarded/wrong-kind/splice rejection; exact SQLite extern signature/link closure; runtime-key/export inventories unchanged |
| control, ownership, and build parity | Callback inputs are invocation-scoped views and scalar result provenance is consumed before C return. Hard termination emits no fallthrough cleanup edge. Whole/per-unit/ThinLTO produce one canonical trampoline and matching descriptor; malformed HIR creates neither. | if/match/else/`?`/early-return/hard-abort callback bodies; view/capture/task escape negatives; whole/per-unit/ThinLTO symbols and executable parity; no-partial-emission assertions |

Before implementation, the package ledger receives one fresh adversarial review. Before code
review, every applicable row above points to both implementation and discriminating owner evidence.
A P1 or equivalent finding in callback capture/effect, C ABI, return cleanup, or native-view
lifetime reopens this matrix rather than starting another local patch round.

## 11. Required verification

Each compiler capability PR runs its focused owner suite, `scripts/test-pr.sh`, applicable
Clippy, the `align-self-review` gate, and the repository's one-review/finding-closure flow.
Acceptance labels do not each force a separate broad test or review cycle.

Local measurement inventory:

- tagged Move payload Drop/propagation cost and no-allocation `Ok` path;
- borrowed-call overhead versus the corresponding current builtin handle operation, including
  all-peer alias scanning, captured-root transfer, and dynamic Move-return cleanup-bit cost;
- resource construction/Drop overhead and generated LLVM shape;
- compile-time and interface-size cost of parameter/capture return summaries and cleanup ABI;
- warm-cache behavior for unchanged, private-SQL-only, public-contract, and checked-metadata
  create/change/delete Query changes;
- region builder push/freeze throughput, bytes allocated, and exact copy count;
- no hidden heap allocation in the region builder path;
- nested-generic inference/monomorph compile time, interface/mono-key size, emitted code size, and
  proof of no runtime dictionary/extra indirect call.

These measurements run locally when their named path first lands or materially changes. They are
not ordinary regression tests and are not PR, release, or milestone gates. A previously recorded
measurement is not rerun for an unrelated compiler or package change.

The goal is not zero instructions for safety. It is one general, statically checked mechanism whose
cost and invalidation behavior remain visible and predictable.
