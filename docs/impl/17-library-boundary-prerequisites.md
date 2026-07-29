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
for this dynamic ownership result.

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
  generated QueryMeta materialization thunk
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
summaries, decoder code, or a database at runtime. D1 emits and tests the table/thunk skeleton for
Declared Queries; D3/D5 populate checked evidence; D12 exposes the ordinary package call. Commands
carry no QueryMeta plan or thunk.

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
- extraction through `match`, `else`, or `?` moves the live payload and clears the container;
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

LLVM predeclares one opaque struct per tagged entry, then assigns the existing Option or Result body
using the already-created struct, enum, and tagged type tables. `Scalar::Tagged(id)` lowers to that
identified struct. This preserves the current Option/Result field order, tag width, alignment, and
by-value ABI; it does not disguise a nested tagged value as a user sum or change the user-sum
non-union layout.

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

## 10. Implementation PR sequence

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
cargo clippy --workspace --all-targets -- -D warnings
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

L2 ships through seven closed implementation slices. A slice may add dormant representation or
tighten existing provenance, but it must not accept source syntax whose complete safety contract
belongs to a later slice.

| Slice | Exact closure | Public exposure at merge | Required gate |
|---|---|---|---|
| L2a | Replace `is_out`/bare parameter-type lists with `ParamMode`; add span-free return-borrow and return-region records to `FnTy`, named/imported signatures, HIR/MIR, interface codecs, hashes, and ABI fingerprints | Existing `ByValue` and `Out` behavior only; `borrow` and `borrow mut` remain identifiers outside parameter-mode lookahead and are rejected as modes | codec byte/hash goldens, corrupt-tag rejection, whole/per-unit identity, and an exhaustive consumer audit |
| L2b-a1 | Infer parameter roots for named functions and preserve conservative flattened roots across recursion, direct/imported calls, control flow, and interfaces | No new borrow mode; aggregate projections and indirect calls retain all-compatible-input unions | scalar direct/imported matrix, semantic interface validation, and summary-inference size/time evidence |
| L2b-a2 | Refine named summaries through struct, tuple, fixed-array, tagged, `else`, `?`, `map_err`, branch/loop, and synthetic-selector projections | No new borrow mode; indirect calls retain the pre-L2b all-compatible-input fallback | direct/imported nested-view projection matrix and per-unit parity |
| L2b-b | Extend the same inference to capture roots, closures, function-value joins/moves, direct/indirect targets, and unresolved higher-order fallback | Complete L2b behavior; no borrow mode | indirect/captured/joined nested-view matrix, malformed capture-domain rejection, and indirect-return evidence |
| L2c | Add `ReturnCleanupAbi` to function and interface identity and implement `DynamicBit` for every recursively Move direct, indirect, and imported return; forward the selected bit on every return edge and store it in the caller slot | No borrow syntax; metadata and physical ABI land atomically before borrowed mutation can construct path-selected values | codec/hash goldens, `Result<Option<MoveStruct>, Error>` None/Some/Err matrix, ABI mismatch rejection, per-unit parity, and return-cost evidence |
| L2d | Contextually accept shared `borrow`, preserve the mode in function types/interfaces, pass non-null caller storage, prohibit callee move/drop, and apply the completed return-root summaries | Shared borrow only; `borrow mut` remains unavailable and shared borrowing Copy is rejected as redundant | reusable Move owner, move-from-borrow rejection, returned-view invalidation, function-value/import parity |
| L2e | Contextually accept `borrow mut`; complete existing `Out` and new `BorrowMut` under one all-peer recursive exclusivity engine; implement generation invalidation, writable Copy/Move replacement, drop-old/cleanup-bit update, and Pure exclusive-state shaping | Full L2 surface | all-peer alias matrix, stale-view rejection, changed/unchanged pointee Drop counts, effect matrix, and per-unit parity |

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
the erased target can be an internally constructed function value. The latter covers zero-argument
closures whose captures feed an internal parallel boundary even when unrelated side effects make
both the closed and open-world effects `Impure`; a direct external callback parameter or parameter
field call remains legal. L2b replaces those conservative boundaries with recursive target-relative
provenance through function-value joins.

L2b is implemented as three independently sound vertical PRs. L2b-a1 owns named/direct/imported
parameter-root inference, semantic interface validation, and whole/per-unit parity while retaining
flattened all-compatible-input unions for aggregates, indirect calls, and unanalyzed extern
targets. L2b-a2 refines named
aggregate/control projections without weakening that fallback. L2b-b then adds
target-relative capture roots and function-value target sets, validates the explicit-parameter and
capture domains in MIR, removes the now-obsolete unresolved-internal-target effect restriction, and
closes the indirect/captured benchmark row. This split keeps each hand-written diff near the
repository review bound; none of these PRs exposes `borrow` syntax or depends on an incomplete
physical ABI.
L2b-a1 cannot split producer inference from interface validation, direct-call consumption, and
whole/per-unit parity: shipping a non-empty public summary without any one of those seams would
either ignore the fact in one compilation mode or trust an unvalidated artifact.
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

The following closure matrix is authoritative for implementation and review. “All call forms” means
same-unit named, imported, bound function value, indirect call, and generic monomorph where the
current type restrictions admit that form.

| Surface | Owner | Required positive closure | Required negative/fail-closed closure | Later extension |
|---|---|---|---|---|
| Signature formation | L2a | `ByValue` and existing `Out` are preserved in AST-to-HIR, named/imported signatures, `FnTy`, MIR, rendering, source equality, id-free ABI/interface fingerprints, and monomorph keys combining the structural signature with concrete effect-origin identity | unknown modes, arity mismatch, and mode/type disagreement never default to `ByValue` | L2d admits `Borrow`; L2e admits `BorrowMut` |
| Provenance record formation | L2a | every named/imported/function-value signature contains canonical sorted parameter-root borrow and region summaries, including explicit `None`; L2b-a1 requires the two records to agree | duplicate, unsorted, out-of-range, exported capture roots, borrow/region disagreement, roots inconsistent with resolved parameter/return types, recursive or unresolved generic-capability bindings, generic-body/type-parameter shape disagreement, and every missing or recursive nominal/tuple/tagged id reachable through any by-value `Ty`/`Scalar` wrapper reject before consumer-visible side effects; non-empty generic-template and nested function-value summaries reject until their consumer-side transports exist; validation traversal is cycle-safe and never overflows the compiler stack | L2b computes non-empty roots |
| Interface codec/hash | L2a | mode plus borrow/region summaries have independent byte/hash goldens and producer/consumer parity | truncated, trailing, unknown-tag, unsupported-known-mode, and semantic inconsistency cases reject | L2c adds cleanup ABI atomically |
| Existing return provenance | L2b-a1/a2 | a1 preserves conservative flattened parameter roots through recursion, assignment, control flow, explicit/implicit/early return, and direct/imported calls; an a1 `?` edge may union the flattened operand roots only when the enclosing return type can recursively carry a borrow, so a borrow-capable `Ok` payload cannot put provenance on a scalar/error-only return; a2 recursively refines struct, tuple, fixed-array, tagged, `else`, `?`, `map_err`, and branch/loop projections | indirect/unresolved higher-order targets retain all compatible roots; incompatible joins reject; semantic import rejects provenance on every compiler-known non-borrowing builtin (`Error`, `argon2_params`, and `regex_match`) before per-unit checking | L2b-b adds function-value/capture roots; L3 adds resource/dependent roots; L4 adds explicit region owners |
| Closure/function-value provenance | L2b-b | zero-argument and parameterized closures, synthetic selectors, target joins, environment moves, direct and indirect calls retain selected target-relative roots | environment/owner death, stale generation, out-of-range capture slot, and interface capture root reject | L3/L4 extend the same walker with their types |
| Cleanup ABI formation | L2c | Copy returns record `None`; every recursively Move return records `DynamicBit` in `FnTy`, named/imported signatures, MIR, interface, mangling, cache identity, and LLVM ABI | metadata/type disagreement, missing bit, extra bit, unknown tag, and caller/callee fingerprint mismatch reject | none |
| Cleanup-bit production | L2c | normal expression return, explicit return, `if`, `match`, `else`, `?`, `map_err`, branch/loop join, and early exit forward the selected path-local bit and clear a moved source exactly once | malformed MIR bit source/destination, missing local, invalid tag, and uninitialized/duplicate transfer reject without panic | L4 adds explicit-region clear-bit values |
| Cleanup-bit consumption | L2c | all call forms store the returned bit in the caller result slot; move-out/null, reassignment drop-old, wildcard discard, and scope/early cleanup consult that bit exactly once | no caller may infer the bit from type, tag, or region; ABI mismatch fails before call emission | L2e reuses the same slot through mutable replacement |
| Shared-borrow formation | L2d | contextual `borrow name: T` works for named functions and function types; `borrow: T` and `out: region` remain parameter names; stable addressable immutable or mutable local/field places of Move type whose root is a bound local are accepted | temporary/rvalue, moved place, shared Copy, mode mismatch, move/drop/replace through callee binding, and unbound storage reject | L3 admits resource owners |
| Shared-borrow calls/results | L2d | all call forms pass non-null caller storage without ownership transfer; caller owner remains usable; completed summaries attach returned views to the exact owner generation | use after owner move/drop, wrong indirect mode, stale returned view, corrupt imported summary, any ByValue peer that moves/consumes the same root, and any overlapping existing `Out` peer reject identically in either argument order, including rooted fields and aggregate holders | none |
| Exclusive-borrow formation | L2e | contextual `borrow mut`, existing `Out`, writable Copy/Move local and field places, and function-value modes share one place classifier | immutable, temporary/rvalue, moved, overlapping field/whole-place, unbound storage, wrong mode, and unsupported partial Move leaf reject | L3 admits resource owners |
| Exclusive alias/invalidation | L2e | recursively scan every `ByValue`/`Borrow`/`BorrowMut`/`Out` peer, including distinct aggregate holders; end the old generation at the call; preserve branch/loop state | any direct or nested overlap and any older view use reject before callee effects, with identical local/imported diagnostics | L3 adds resource/dependent overlap classes |
| Exclusive replacement/effect | L2e | changed owned pointee runs guarded drop-old once, stores value and cleanup bit, and later caller Drop sees only the replacement; unchanged pointee emits no callee-exit cleanup; exclusive-input-only mutation is Pure | double Drop, callee function-exit Drop of unchanged pointee, captured/global mutation classified Pure, and unsafe/I/O classified Pure all reject | none |
| End-to-end parity | each slice | focused owner tests, whole/per-unit builds, direct/indirect/imported behavior, generic/interface/cache identity, runtime provenance, Drop/allocation counts, and the slice benchmark agree | malformed interfaces/MIR fail closed and no disabled later mode is accepted | later rows add cases without weakening earlier gates |

Each closure-matrix row is owned by the following exact focused targets. New targets are created by
their first owning slice and remain cumulative gates afterward.

| Slice | Exact owner tests | Exact benchmark command and required rows |
|---|---|---|
| L2a | `cargo test -p align_interface --test summary`; `cargo test -p align_driver --test fn_values --test out_params --test interface_param_modes` | `bench/library_boundary/run.sh interface`: `interface-size`, `decode-throughput` |
| L2b-a1 | `cargo test -p align_interface --test summary`; `cargo test -p align_codegen_llvm malformed_mir_type_graphs_fail_before_llvm_construction`; `cargo test -p align_driver --test return_provenance --test interface_param_modes --test per_unit` | `bench/library_boundary/run.sh provenance`: `summary-inference` |
| L2b-a2 | `cargo test -p align_driver --test return_provenance --test per_unit` | `bench/library_boundary/run.sh provenance`: `summary-inference` |
| L2b-b | `cargo test -p align_driver --test return_provenance --test fn_values --test per_unit` | `bench/library_boundary/run.sh provenance`: `summary-inference`, `indirect-return` |
| L2c | `cargo test -p align_driver --test move_return_cleanup --test owned_tagged_payloads --test per_unit_codegen` | `bench/library_boundary/run.sh move-return`: `copy-return-control`, `move-return-none`, `move-return-some`, `move-return-err` |
| L2d | `cargo test -p align_driver --test borrowed_params shared_`; `cargo test -p align_driver --test return_provenance` | `bench/library_boundary/run.sh shared-borrow`: `by-value-call-control`, `shared-borrow-call` |
| L2e | `cargo test -p align_driver --test borrowed_params exclusive_`; `cargo test -p align_driver --test out_params --test analysis_coverage` | `bench/library_boundary/run.sh exclusive-borrow`: `exclusive-copy-control`, `exclusive-copy-call`, `exclusive-move-replace` |

Acceptance:

- a Move owner remains usable after a shared borrow;
- moving from a borrowed binding is rejected;
- a returned view dies when the caller owner moves/drops;
- `borrow mut` rejects later use of an older view;
- a call rejects an overlapping by-value `str`/slice or recursively view-bearing aggregate beside
  `borrow mut` of its owner; L3 applies the same completed alias engine to `resource_ref`;
- the same recursive rejection covers every peer mode (`ByValue`, `Borrow`, `BorrowMut`, and
  `Out`), including distinct aggregate holders rooted in the invalidated generation;
- mutable borrowing a writable Copy aggregate updates caller state; shared borrowing Copy is
  rejected as redundant;
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
- structural Params/Row contracts/fingerprints, QueryMeta plan/thunk data, plus binder/decoder ABI
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
- a separately compiled Query's Declared and checked QueryMeta rows come only from its static
  plan/materialization thunk and remain available without source/interface/artifact file I/O;
- CheckedRequired validates every permitted driver, while CheckedOptional preserves an explicit
  mixed per-driver state;
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

Only after L1a–L7 are shipped may the first SQLite runtime/Query vertical slice begin.

## 11. Required verification

Each compiler PR runs its focused regression suite, `scripts/test-pr.sh`, Clippy, the
`align-self-review` gate, and the repository pre/post-review flow.

Required benchmarks:

- tagged Move payload Drop/propagation cost and no-allocation `Ok` path;
- borrowed-call overhead versus the corresponding current builtin handle operation, including
  all-peer alias scanning, captured-root transfer, and dynamic Move-return cleanup-bit cost;
- resource construction/Drop overhead and generated LLVM shape;
- compile-time and interface-size cost of parameter/capture return summaries and cleanup ABI;
- warm-cache behavior for unchanged, private-SQL-only, public-contract, and checked-metadata
  create/change/delete Query changes;
- region builder push/freeze throughput, bytes allocated, and exact copy count;
- no hidden heap allocation in the region builder path.
- nested-generic inference/monomorph compile time, interface/mono-key size, emitted code size, and
  proof of no runtime dictionary/extra indirect call.

The goal is not zero instructions for safety. It is one general, statically checked mechanism whose
cost and invalidation behavior remain visible and predictable.
