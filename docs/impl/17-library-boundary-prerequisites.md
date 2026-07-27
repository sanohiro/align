# Library boundary prerequisites

## Status

Design of record for the language/compiler work that must land before `pkg.db` implementation.
This is not a database-private escape hatch. It closes the general gap between ordinary Align
packages and native stateful libraries. `std.http`, `std.net`, `std.process`, and future
FFI-backed packages must be able to use the same ownership and borrow machinery.

The implementation order in this document is mandatory. A database driver must not add another
closed `Ty`/HIR/MIR family that recognizes `pkg.db` names, and it must not expose `raw` handles or
manual close functions through its safe public API.

## 1. Decisions

Six prerequisites are accepted:

1. Recursive tagged Move payloads for ordinary structured `Option`/`Result`/sum errors and outputs.
2. Borrowed function parameters, including an invalidating mutable-borrow mode.
3. Package-defined opaque Move resources with exactly-once Drop.
4. Named arena bindings that expose a non-owning `region` capability to ordinary functions.
5. Deterministic static source inputs and per-unit query artifacts.
6. Region-backed `array_builder<PlainStruct>` with no hidden heap allocation.

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

all six
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

Shared `borrow` requires a Move parameter type because Copy already preserves caller ownership.
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
It rejects an unbound Move temporary at a borrowed parameter because the owner would otherwise need
a second hidden lifetime-extension rule.

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

This deliberately uses one conservative rule. A mutable-borrow operation that happens not to
reallocate still ends the old generation. That may reject retaining an otherwise-safe view across
some mutations, but it cannot leave a package-defined mutation accidentally fail-open.

At one call site, aliases of the same owner may not appear as:

- two `borrow mut` arguments;
- one `borrow mut` and any overlapping `borrow`;
- an `out` slice and an overlapping `borrow`/`borrow mut`.

The existing out-parameter no-alias proof and borrow-root analysis are the single implementation
mechanism for these checks.

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
)
ParamMode = ByValue | Out | Borrow | BorrowMut
```

Binding or passing a named function preserves every mode. Indirect-call checking requires an exact
mode/type match and lowers `Borrow`/`BorrowMut` through the same pointer-to-caller-storage ABI as a
direct call. Mode erasure, implicit adaptation, and treating a borrow-mode target as by-value are
rejected. Binding preserves both return summaries; a mutable function local or control-flow join
unions the sorted parameter-index sets, so the indirect result is tied to every owner/region that
any reachable target may use. When its return type may carry borrow provenance, an unresolved
function-typed parameter conservatively includes every `Borrow`/`BorrowMut` input plus every
by-value/out Copy input whose type recursively carries a view or `resource_ref`; it does not make a
consumed by-value Move owner returnable. When the return may carry region provenance, it includes
every `region` input. Corrupt/out-of-range summaries fail closed.

The inferred `Effect` and return summaries remain excluded from written source-level function-type
equality; parameter modes are not. They are concrete checked-HIR/interface facts consumed by
indirect calls. An outer public function that returns an indirect-call result exports its normally
inferred outer `ReturnBorrowSummary`/`ReturnRegionSummary`, so consumers never need the target body.

The checked-HIR analysis infers canonical return lifetime summaries:

```text
ReturnBorrowSummary
  None
  Params(sorted parameter indices)

ReturnRegionSummary
  None
  Params(sorted `region` parameter indices)
```

The summary records which parameters may back any view or dependent resource in the return value.
It is computed from the same exhaustive provenance walk used by local borrow liveness. It is serialized in
`IFnSig`, consumed during separate type checking, and included in `interface_hash`.
`ReturnRegionSummary` records which explicit destination regions may own the returned value. A
consumer therefore knows that `run(..., out)` cannot escape `out` without importing the producer
body. When several borrowed/region parameters may contribute, the caller uses their inferred
shortest region.

The return-borrow summary is not limited to parameters spelled `borrow`. A by-value Copy view such
as `str`, `slice<T>`, `resource_ref<R>`, or a recursively view-bearing `db.exec` may back the
returned value and therefore appears in the same parameter-index summary. The `borrow` spelling is
needed specifically to avoid consuming a Move owner.

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
pub resource conn = internal.drop_conn
pub resource stmt<P, R> = internal.drop_stmt
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

Only the declaring module and its canonical descendant-module subtree may use the resource
representation intrinsics:

```align
unsafe {
  c: conn := resource.from_raw(handle)
  ref := resource.borrow(c)
  handle2 := resource.raw(ref)
  transferred := resource.into_raw(c)
}
```

Rules:

- `resource.from_raw` takes ownership of one non-null native handle; the expected result type selects
  the resource type, so there is no turbofish.
- `raw.is_null()` is the one check used before construction when a C API reports failure with null.
- `resource.borrow` is safe and returns `resource_ref<R>`.
- `resource.raw` is unsafe and accepts `resource_ref<R>`, never an owning value.
- `resource.into_raw` is unsafe, consumes the resource, clears its cleanup flag, and transfers the
  native ownership to the caller.
- Representation privilege is checked by canonical module-path prefix, not by an import cycle or a
  public constructor. For a resource declared in `pkg.db`, `pkg.db.sqlite`,
  `pkg.db.postgres`, and `pkg.db.internal.*` are privileged descendants; an importing application or
  another package is not.
- The `pub` Drop-hook module accepts only `raw` and need not import the resource-declaring module.
  The root may therefore reference `pkg.db.internal.drop_conn`, while a driver descendant imports
  the root type and constructs it directly from an expected `db.conn`; `internal` never imports the
  root and the graph stays acyclic. Package `internal` visibility keeps the hook out of consumer
  source APIs; the generated root thunk supplies separate-compilation linkage.
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
- it cannot be returned, stored in an aggregate, placed in `Option`/`Result`, sent to a task, or
  passed through FFI;
- passing it to an ordinary function does not transfer ownership of the arena;
- every allocation performed through it receives that exact `Arena(id)` region;
- the existing escape analysis rejects a returned or stored value that would outlive that arena;
- arena cleanup remains owned by the lexical block, never by the `region` value.

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
```

Registration is based on the resolved callee identity, never a textual path match. A local `db`
binding, an unimported path, or a user function with the same spelling registers nothing and cannot
cause a file read or missing-file diagnostic.

Only `File` entries are read before a frontend-cache lookup. `Inline` bytes come from the already
parsed unit; they still use this tagged record for artifact/action-key canonicalization and can never
request filesystem I/O. Canonical list order is source tag (`File = 0`, `Inline = 1`), then UTF-8
payload bytes, then consumer kind; content hashes never decide ordering.

The producer action identity includes the unit source/import digest plus this list. At the shipped
cache boundary, name resolution/frontend still runs before the list is first produced, and
`impl_hash` includes every static input that changes generated MIR/data. This forces a producer
object miss and relink even when no `.align` byte changed.

To preserve an eventual pre-frontend no-op hit without guessing from syntax, the driver may persist
a versioned `StaticInputManifest` emitted by successful import/name resolution. It is keyed by the
exact source/import-resolution digest and records constructor identity, tagged source identity, and
kind. A matching manifest lets the driver validate/read/hash only its exact `File` paths before a
frontend-cache lookup; `Inline` never causes a read. A missing or mismatched manifest runs
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
  Params canonical type
  Row canonical type
  driver restriction
  static semantic options
  SQL source identity: File(logical path) | Inline(query_id)
  source SQL exact bytes
  source SQL hash
  per-allowed-driver wire SQL exact bytes/hash
  source-to-wire rewrite map and rewrite-format version
  parameter occurrence table
  source-span map
  checked-metadata policy and digest
```

`StaticCommandArtifact` is the same versioned envelope with `statement_kind = Command`, a Params
type, and no Row/decode thunk. It is not a second discovery/cache mechanism.

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
  driver mask
  per-driver wire-SQL pointer/length/hash
  static-option pointer
  generated bind thunk
  generated decode thunk
  checked-metadata fingerprint
```

`P` and `R` are compile-time phantom contracts. The generated binder reads known field offsets from
`P`; the decoder writes known offsets in `R`. There is no per-row field-name lookup, reflection,
boxing, or map allocation.

The producer's descriptor function returns the static pointer. When the item is `pub`, its interface
exports that descriptor contract. Generated thunks stay in the producer object and are referenced
by the static descriptor, so a consumer does not need the Query body to call it.

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

The settled rule is:

- `Option<T>`, `Result<T,E>`, and user sum payloads may contain any finite, non-recursive type with
  a compiler-known Drop plan;
- the enclosing value is Move iff any live payload is Move;
- Drop first tests the active tag and recursively drops only the live payload;
- construction moves the payload and clears its source ownership;
- extraction through `match`, `else`, or `?` moves the live payload and clears the container;
- early return, branch/loop join, reassignment, and partially initialized construction retain the
  existing path-local cleanup discipline;
- borrowed payload provenance remains recursive and independent of the cleanup plan;
- recursive types remain rejected; this prerequisite does not add tracing or heap-indirect enums;
- arrays/collections of arbitrary Move elements are separate data-layout work and are not implied.

This is one recursive `DropPlan`/`MovePlan` classification shared by structs, sums, `Option`, and
`Result`. A table-free helper that treats an enum/struct payload as Copy is prohibited.

### 8.2 ABI and interface

The ABI stays the existing tagged aggregate shape. A payload's physical fields are unioned/aligned
as today; the extension is semantic cleanup and move-out, not pointer boxing. `DropPlan` is derived
from canonical type definitions after generic monomorphization. Public interfaces already carry
the structural type definitions; their canonical hash changes when a payload type or ownership
shape changes. A corrupt or cyclic imported definition fails closed before a plan is built.

Success paths do not allocate merely because the error type is Move. Owned strings are allocated
only when the program constructs the error payload. Returning `Ok` initializes no error-owned
field, and Drop consults the tag before touching payload storage.

## 9. MIR and ABI ownership

The new semantics must be explicit before LLVM:

- HIR/MIR types have one recursive tagged Move/Drop plan for struct/sum/Option/Result payloads.
- HIR function parameters and function-value parameter entries carry
  `ByValue | Out | Borrow | BorrowMut`.
- named/interface signatures and concrete function values carry return-borrow/region summaries;
  function-value joins union them and unresolved higher-order parameters use the fail-closed
  all-compatible-input summary.
- HIR resource types carry declaration identity and the producer-owned Drop-thunk identity/ABI
  fingerprint.
- MIR locals keep the existing path-local cleanup bit for resources and Move aggregates.
- MIR has generic resource construction, dependent construction, borrow, raw-view construction,
  raw extraction, ownership transfer, and Drop;
  there is no `DbConnDrop`/`HttpServerDrop` family.
- a `BorrowMut` call ends the owner generation in checked-HIR borrow state before the call.
- direct and indirect calls share the same parameter-mode ABI and alias/provenance checks, including
  the indirect target's joined return summaries.
- named arena lowering reuses `ArenaBegin`/`ArenaEnd` and merely binds the handle as `region`.
- static Query construction lowers to immutable data plus generated binder/decoder functions.
- LLVM performs pure representation lowering and does not decide ownership, invalidation, or Query
  semantics.

Every new HIR/MIR/type variant must be wired through the exhaustive region, move, escape, effect,
interface, print, codec, ABI, and codegen classifications. Catch-all defaults for a resource or
borrow-bearing type are prohibited.

## 10. Implementation PR sequence

### L1a — recursive DropPlan and `Option<Move>` fields

This is the first implementation PR. Its exact scope is:

- introduce one canonical recursive owned-value/Drop-plan classifier after all struct/enum
  definitions are resolved;
- make `Option<Move>` a legal struct field;
- mark the enclosing struct Move;
- emit tag-tested Drop for the field on normal/early cleanup and drop-old reassignment;
- move/null the live payload on whole-struct moves and supported field extraction;
- keep recursive types, nested partial moves not covered by the existing place machinery, and
  arbitrary Move collection elements rejected with explicit diagnostics;
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
- a nested owned struct inside `Option` recurses through the same plan;
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
cargo clippy --workspace --all-targets -- -D warnings
bench/owned_tagged_payload/run.sh
```

The focused test invokes successful programs under the repository's double-free/leak-sensitive
allocation harness and compile-fail programs through the normal diagnostic path. The benchmark is
manual evidence recorded in the PR; it does not become an ordinary CI gate.

### L1b — Move sum/Option/Result payload completion

Scope:

- allow Move structs and Move sums as user-sum, `Option`, and `Result` payloads;
- recursively classify the enclosing tagged value as Move;
- implement tag-switched Drop, move-in/null-source, and move-out/null-container for construction,
  `match`, `else`, `?`, `map_err`, return, reassignment, and branch/loop joins;
- add whole-program/per-unit parity and malformed-interface fail-closed tests.

Acceptance includes the exact `NativeError`/`DbError`/`Result<Option<Output>,DbError>` shape from
§8.1, with allocation counters proving cleanup on every success/error control-flow edge and no
error allocation on an `Ok` hot path.

### L2 — borrowed parameters and interface summaries

Scope:

- contextual parse/check of `borrow`, `borrow mut`, and `out`, including function-type modes;
- `BorrowMut` on writable Copy/Move places and exact function-value mode preservation;
- local no-move/exclusive-alias rules;
- effect inference that treats mutation rooted only in an explicit `borrow mut` parameter as Pure;
- return-borrow inference;
- interface codec/hash support;
- per-unit parity.

Acceptance:

- a Move owner remains usable after a shared borrow;
- moving from a borrowed binding is rejected;
- a returned view dies when the caller owner moves/drops;
- `borrow mut` rejects later use of an older view;
- mutable borrowing a writable Copy aggregate updates caller state; shared borrowing Copy is
  rejected as redundant;
- `borrow: T` and `out: region` remain legal parameter names through contextual lookahead;
- function-value binding and indirect calls retain all four parameter modes exactly;
- borrow-returning function-value joins union return-borrow/region summaries, and an unresolved
  higher-order parameter uses the all-compatible-input summary;
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
- `into_raw` suppresses Drop;
- null/native failure is handled before construction;
- a ref cannot survive owner move/drop/mutable borrow;
- a dependent resource prevents parent move/drop/mutable borrow until the child drops;
- a raw-derived `str`/slice cannot outlive the supplied resource generation;
- invalid native pointer/length pairs never become safe views;
- another package cannot construct or extract the resource, including in `unsafe`;
- a root resource, raw-only internal Drop hook, and driver descendant type-check with an acyclic
  import graph; the internal hook module cannot import the root;
- no resource enters `spawn`.

### L4 — named region capability

Scope:

- parse `arena name {}`;
- builtin `region` type and restrictions;
- function argument propagation;
- `clone_in`.

Acceptance:

- ordinary functions allocate into the exact caller-selected arena;
- region/result escapes are rejected across direct, branch, loop, `?`, closure, and module paths;
- anonymous and named arena cleanup are byte-identical except for the bound handle;
- no thread-local ambient allocator is introduced.

### L5 — deterministic static inputs and Query artifacts

Scope:

- recognized-constructor discovery;
- whole-body descriptor placement and unique item identity;
- safe path resolution and SourceMap registration;
- frontend/impl cache keys;
- versioned query artifact and interface summary;
- generated descriptor data skeleton.

Acceptance:

- changing only `.sql` misses the producer object cache;
- a descriptor accepts exactly one whole-body static constructor; nested, conditional, multiple,
  generic, argument-taking, and helper-wrapped forms fail before artifact creation;
- two descriptor functions in one module receive distinct Query IDs and artifact/thunk slots;
- unchanged consumers still hit when the public Query contract is unchanged;
- public Params/Row/restriction changes invalidate consumers;
- absolute/escaping/symlink paths fail;
- a shadowing local or same-spelled user function does not read/register a file;
- a stale `StaticInputManifest` is rejected after source/import/schema identity changes;
- source SQL hash stays stable across driver selection, while PostgreSQL wire hash/rewrite map
  changes deterministically with placeholder ordinals;
- inline SQL uses `Inline { query_id }`, decoded literal bytes, and a decoded-byte-to-`.align` span
  map; no fake filesystem path enters identity or diagnostics;
- artifact bytes are reproducible across checkout roots and process runs.

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

Only after L1a–L6 are shipped may the first SQLite runtime/Query vertical slice begin.

## 11. Required verification

Each compiler PR runs its focused regression suite, `scripts/test-pr.sh`, Clippy, the
`align-self-review` gate, and the repository pre/post-review flow.

Required benchmarks:

- tagged Move payload Drop/propagation cost and no-allocation `Ok` path;
- borrowed-call overhead versus the corresponding current builtin handle operation;
- resource construction/Drop overhead and generated LLVM shape;
- compile-time and interface-size cost of return-borrow summaries;
- warm-cache behavior for unchanged, private-SQL-only, and public-contract Query changes;
- region builder push/freeze throughput, bytes allocated, and exact copy count;
- no hidden heap allocation in the region builder path.

The goal is not zero instructions for safety. It is one general, statically checked mechanism whose
cost and invalidation behavior remain visible and predictable.
