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

The clean reviewed am-r ledger merged in #678 and fixes twenty-seven L2 implementation PRs. Am-r
itself is a design-only gate and is not one of those implementation PRs. L2b-a2
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
| L2b-a2-am-r | Design gate completed in #678: the public-contract ledger below isolates five producer corrections and one checked-HIR depth-safety closure, then splits per-position type admissibility, nominal/link metadata, declarations/headers, body validation, and the dependent callable representation change into fourteen independently correct verticals | This row authorizes no implementation itself. Its clean reviewed merge fixes L2b at twenty-three and L2 at twenty-seven implementation PRs and authorizes am-d | completed fresh independent adversarial ledger review covering the entry ABI, return completeness, task-wait dominance, native output buffers, extern unsafe-callability, the proved conservative HIR depth ceiling and body/type-consumer safety, every producer placement predicate, exact body discriminator record, runtime key and native ABI row, compiler/generated emitted identity, validation order, valid producer twin, and owner test |
| L2b-a2-am-d | Make the fixed conservative checked-HIR record ceiling and the unbounded valid type-DAG domain stack-safe end to end | Preserve every parser-valid source, including diagnosed HIR before producer finalization, while rejecting handcrafted HIR deeper than the fixed 259 ceiling before semantic consumption; every depth-259 body and every finite am-g-t-valid acyclic-inline/header-mediated type DAG at a producer-valid root remains stack-safe from HIR entry through LLVM verification on the 2 MiB test stack | exhaustive constructor-expansion ceiling proof, complete recursive body/type-consumer inventory, common iterative traversal closure, 258/259/260 body cases, deep valid/malformed type-DAG roots, and whole/per-unit MIR/LLVM parity |
| L2b-a2-am-e | Make the entry producer and backend ABI exact: no-arg `main` returns only Unit, exact signed i32, or `Result<Unit,builtin Error>`; argv main remains the exact Result form | The compiler rejects previously accepted non-C-ABI entry returns with one source diagnostic; Unit/Result wrappers and direct i32 entry behavior remain exact | sema signature matrix, whole/per-unit Unit/i32/Result exit behavior, every rejected graph-valid return, LLVM signature/link/ThinLTO parity |
| L2b-a2-am-f | Make function completion exact before a non-Unit body reaches HIR/MIR | Bare return and reachable absent tail are valid only for Unit; every non-Unit path returns a typed value or is proven non-fallthrough | bare/value return, tail/absent tail, every control family, whole/per-unit MIR/LLVM verifier matrix |
| L2b-a2-am-w | Make successful task-wait dominance path-complete before malformed-HIR validation consumes it | Reject a `TaskGet` unless its exact active group proves the Task's born generation valid and the current generation completed, with every earlier drained fallible Wait resolved Ok; carry Wait proofs and Move Task origin proofs through exact transparent local/control flow without a type or runtime ABI change | straight-line/reset, infallible/fallible, stored/copied/reassigned/map_err Result, Task move/reassignment/control origin, if/match/else, loop-break, early-exit, stale Wait alias after Spawn, unresolved/failed first Wait plus empty second Wait, inner-Wait/outer-Task isolation, outer proof handled inside inner group, exited-inner proof clearing, repeated primitive get, whole/per-unit task result matrix |
| L2b-a2-am-v | Require each native output `Buffer` to be a bound `mut` local before the runtime can write through it | Reject temporary and immutable output buffers at ReaderRead, ReaderReadLine, FilePread, UdpRecvFrom, and CryptoRandom; every other native handle and accepted buffer use is unchanged | five-site local/mut/type/diagnostic-order matrix, accepted runtime/allocation twins, whole/per-unit parity |
| L2b-a2-am-u | Make foreign invocation permission lexical and non-escaping | Reject extern function-value formation; direct extern calls and named extern pipeline/reducer/sort callbacks require their invocation expression inside `unsafe`; safe user/imported callable behavior and native RuntimeKey calls are unchanged | direct/callback/FnValue/unsafe-depth matrix, resolver diagnostic order, whole/per-unit extern ABI parity |
| L2b-a2-am-p | Validate every body-independent type placement against its exact sema producer predicate | No source acceptance change; placement-invalid handcrafted HIR becomes canonical-empty | producer/placement Cartesian matrix and valid graph-but-invalid-position twins |
| L2b-a2-am-n | Validate nominal/source identities, complete structural equality, enum/table ordinals, alignment, and link libraries | No source, ABI, or artifact change on valid input | exact-byte/NUL/collision/shape/base/alignment/library matrix |
| L2b-a2-am-h | Validate extern/import/stored/main/local body-independent declarations and headers, and retain normalized imported-effect facts in checked HIR | Header-invalid handcrafted HIR becomes canonical-empty; source/interface behavior is unchanged | mode/signature/summary/imported-effect/main/local/drop-set structural matrix |
| L2b-a2-am-b1 | Build the dormant total-validator core for statements, ordinary expressions, calls, aggregates, tagged values, and structured control | No public entrypoint activation | exhaustive direct discriminator/field unit owners |
| L2b-a2-am-b2 | Extend the dormant validator through storage, views, vectors, arrays, pipelines, templates, and JSON | No public entrypoint activation | exhaustive direct storage/stage/terminal/descriptor unit owners |
| L2b-a2-am-b3 | Extend the dormant validator through every native/runtime family and generated-callable body fact | No public entrypoint activation | exhaustive direct native/helper/generated metadata unit owners |
| L2b-a2-am-b4 | Correlate body-derived ownership and effect facts and activate the complete body validator in every lowering entrypoint | Any invalid body makes the whole MIR program canonical-empty; valid HIR stays byte-identical | full inventory assertion, ownership/effect-cell mutations, parallel-effect twins, depth bound, whole/per-unit identity and benchmark |
| L2b-a2-am-c | After am-b4, separate program, runtime, and generated call targets and give Align/generated symbols injective compiler-owned identities | Existing source spellings remain accepted; internal MIR and LLVM/object symbol bytes change atomically, while interface and C/runtime ABI stay fixed | complete runtime-key/native-symbol registry, generated-family collision matrix, whole/per-unit link parity |
| L2b-a2-af | Extend the projection fact through validated fixed arrays and exact/dynamic element reads/writes | No new borrow mode; pipeline, tagged/control, non-fixed collection, and indirect-call residuals retain the L2b-a1 all-compatible-input fallback | direct/imported fixed-array projection matrix and per-unit parity |
| L2b-a2-ar | Close eager retained-storage lifetime for non-fixed `Index`, `ElemField`, `SliceRange`, `ArrayChunks`, and `HttpRespHeader`; make non-fixed `ElemField` receiver-first | No new borrow mode or projection precision; non-fixed results remain flattened | invalidated eager-action matrix, terminating-operand twins, runtime source-order checks, malformed-HIR rejection, and per-unit parity |
| L2b-a2-ap | Extend the projection fact through pipeline `Project`/`WhereField` and terminal formation | No new borrow mode; tagged/control and indirect calls retain the L2b-a1 all-compatible-input fallback; unsupported stages and terminals widen explicitly | direct/imported pipeline-view projection matrix and per-unit parity |
| L2b-a2-t | Complete user-sum/`Option`/`Result`, `match`, `else`, `?`, and `map_err` projection | Complete L2b-a2 behavior; no new borrow mode; indirect calls retain the pre-L2b fallback | direct/imported tagged-view projection matrix and per-unit parity |
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

The clean reviewed am-r ledger in #678 fixes twenty-three independently sound L2b implementation
PRs. L2b-a1 owns named/direct/imported
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
the exact remaining PR split and count.

| Slice / malformed-HIR cell | Required closure | Exact owner evidence |
|---|---|---|
| am-g-t concrete type roots and total type domain | Before copying any record, validate tuples, structs, enums, and every type reachable from those tables, an `extern`/imported declaration, or a stored-function header. Every stored struct, enum, and tuple is a concrete root. Every stored tagged-type and function-type entry is also a concrete root unless it belongs to an abstract template graph: a node that contains `Scalar::Param`/`Ty::Param` or transitively depends on such a node. An unreachable abstract template graph is permitted because the producer retains generic-template interner entries that MIR omits; a concrete root that reaches one rejects. Every non-template tagged/function entry remains a root even when otherwise unreachable, so a missing id, inline cycle, or invalid concrete scalar cannot hide in discarded state. Every tagged reference and every function-type reference must be in range even inside a permitted template graph. Traverse with an explicit enter/exit worklist and visit colors rather than native recursion. `IntTy.bits` is exactly `8`, `16`, `32`, or `64`; `FloatTy.bits` is exactly `32` or `64`; the same widths apply inside every `PrimScalar`. `Vec`/`Mask` accepts only an integer/float scalar and exactly `2`, `4`, `8`, or `16` lanes. Every `Struct`/`Enum`/`Tuple`/`Tagged`/`Fn` discriminator resolves to its matching table, every struct-bearing collection resolves a struct, and `DictEncoded(id, field)` resolves an in-range `str` key field. `Ty::IntVar`, `Ty::FloatVar`, `Ty::Error`, and HIR-reachable `Ty::StrFinder` reject. Fixed arrays, tuples, structs, enums, `Option`, `Result`, and nested tagged payloads extend the active inline-layout path and reject an inline cycle. `Box`, slices, dynamic arrays, `ArrayBuilder`, `Task`, dynamic struct arrays, SoA, scanners, dictionary headers, and function closures validate their referenced entries but break that inline path; header-mediated nominal recursion is valid. Am-g-t validates graph formation only: it does not claim that every valid type is admissible in every field, payload, tuple element, parameter, return, local, or body position. | one mutation for every `Ty`, `Scalar`, and `PrimScalar` discriminator; every width/lane boundary; missing/wrong-kind table id and dictionary field; inline-cycle rejection and `Box`/dynamic-array/task/function-header positive cycle twins; reachable/unreachable `Param` tagged and function-type twins; unused malformed non-template tagged and function-type entries; first/middle/final concrete roots; placement-invalid but graph-valid positive twins remain unchanged for am-r; invalid results have every vector empty in all four entrypoints |
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
  `UdpSocket`, `Child`, `HttpResponse`, `HttpServer`, `HttpRequestCtx`, `HttpStream`,
  `ResponseBuilder`, and `RunOutput`, then plus `Fn`. `File` remains accepted because that is the
  current producer predicate; am-p does not silently narrow it.
  A dynamic scalar array must still reduce to one `PrimScalar`; nested arrays do not become an
  element scalar.
- `fn-scalar` is `ty-scalar` without `Slice`. A first-class callable return is `fn-scalar` or
  `Result`; an annotated `FnTy` parameter uses `ty-scalar`, preserving the currently accepted
  slice annotation, while an actual named/lifted function value uses `fn-scalar`.

| Position, in validation order | Exact accepted producer contract | Required valid twin and invalid owner |
|---|---|---|
| struct field | `Int`, `Float`, `Bool`, `Char`, `Str`, `String`, `Struct`, `Enum`, every `is_move_handle` leaf, `HttpHeaders`, `Slice`, `Fn`, recursively admissible `Option`/`Result`/`Tagged`, `DynArray`, and `DynStructArray`. `DynArray(String)` rejects. Every inline struct reached through a direct/tagged field is acyclic and has no `align(N)`. `layout(C)` narrows the field to integer or float. | one positive per arm, direct/tagged nesting, Move enum/struct-array, and `http_headers`/function/slice fields; one wrong placement per arm and every `layout(C)`/alignment/cycle edge |
| concrete enum payload | Integer, float, bool, char, `Str`, `String`, `Struct`, `Enum`, `Fn`, `ResponseBuilder`, recursively concrete `Option`/`Result`/`Tagged`, `DynArray` except `DynArray(String)`, and `DynStructArray` whose element struct is non-Move. Inline struct/enum/tagged cycles reject. | positive direct Move struct/enum, function, builder, tagged, scalar array, and non-Move struct-array payloads; owned-element and recursive negatives |
| generic enum template and monomorph | A template first uses `scalar_arg(..., allow_param=true)`: `payload-scalar`, including `Param`, nested tagged parameters, and `ResponseBuilder`, but not the concrete-only `Fn` extension. A monomorph substitutes every parameter, then applies `enum_payload_ok`; this rejects `ResponseBuilder` in an emitted generic monomorph and rechecks graph-dependent struct/struct-array ownership after all definitions resolve. The validator accepts only the union actually emitted by these two paths and does not widen the template path to the concrete path. | abstract unused template twins, each concrete substitution, and concrete `Fn`/builder positives versus generic-monomorph negatives |
| tuple element | Exactly integer, float, bool, char, `Str`, `String`, `DynArray`, or `DynStructArray`; order is significant and duplicate tuple element lists are one interned identity. | one positive per kind and all other graph-valid scalar/composite negatives |
| `Option`/`Result` payload | `scalar_arg(..., allow_param=true)`: `payload-scalar`, with nested `Option`/`Result` interned as `Tagged`; abstract `Param` is template-only. | every payload kind, nested tagged values, and excluded buffer/builder/header/composite twins |
| box type argument | `scalar_arg(..., allow_param=false)`, then reject `Struct`, `Enum`, every `Scalar::is_move`, and `Str`. The admitted type-formation remainder is integer, float, bool, char, unit, primitive `Slice`, SoA, JSON document, and a concrete non-Move `Tagged` value. This is deliberately broader than value construction: `heap.new` additionally rejects `Slice`, whose borrowed view cannot be stored as an owned box payload. | one type-formation positive for every admitted remainder including `Slice`/SoA/JSON/tagged; `heap.new(Slice)` body negative; struct/enum/owned/`Str`/parameter negatives |
| slice/dynamic-array type argument | `collection-scalar`. A dynamic struct array instead records its exact struct id and rejects an over-aligned element. `File` is admitted here by the current type producer even though no array-literal producer admits it. SoA separately requires a non-empty struct containing only integer, float, bool, char, or `Str` fields. `ArrayBuilder` accepts only integer, float, bool, char, or `String`. | one positive per type-argument family including `File` and `Fn`; every explicitly excluded handle/nested/over-aligned/SoA-field/builder negative |
| fixed-array literal element | Body-owned, not am-p-owned. A fixed struct array admits an over-aligned struct and records the padded/aligned slot contract. A scalar literal rejects every owned handle including `File`, every slice-bearing non-struct, and a Move enum; all elements have one checked type, `ArrayLit.elem` matches it, and the length fits the stored type. | over-aligned fixed-struct positive; `File` type-formation-positive/literal-negative twin; handle/slice/Move-enum/type/length/pooled-state matrix in am-b2 |
| vector and mask element | Integer or float with exactly 2, 4, 8, or 16 lanes. | every width/lane endpoint and bool/char/aggregate negatives |
| annotated `FnTy` type positions | Each parameter is `ty-scalar`. The return is any graph-valid non-`Error` type currently produced by `resolve_type`; the body/call validator separately requires each actual callable origin to satisfy `fn-scalar` parameters and a `fn-scalar`/`Result` return. Mode cardinality/class and summaries belong only to am-h. Imported effect transport belongs to am-h; body-correlated effect cells and parallel eligibility belong only to am-b4. | slice- and buffer-parameter annotation positives, actual fn-value slice negative, Result-return handler, and one type-position mutation per branch |
| stored source function or monomorph type positions | Each parameter and return is a concrete `resolve_type` result. A parameter is not `Box`; a return is neither `Box` nor `Fn`. A monomorph contains no reachable `Param`. Modes, `main`, summaries, and local records belong only to am-h. | every source-nameable parameter/return family, Box/Fn boundary negatives, generic substitution twins |
| imported function type positions | Same source-function type-position contract as its producer, plus id-free structural ABI type identity and no abstract/private type identity. Modes, summary equality, and interface header facts belong only to am-h. | whole/per-unit identical type twins and one type-position corruption at a time |
| extern parameter and return type positions | Parameters are integer, float, raw, `Str`, numeric `Slice`, or a non-empty `layout(C)` struct. Returns are unit, integer, float, raw, or a non-empty `layout(C)` struct. Target-specific SysV size/register rejection remains codegen-owned after this target-independent validation. Modes and summaries belong only to am-h. | scalar/view/C-struct positives; empty/non-C/wrong field/view-return type negatives |
| local, expression, statement, and block-tail position | A local may carry any concrete graph-valid type actually produced at that body point, including compiler-only task, dictionary, scanner, builder, and handle types. There is no global local allowlist. Am-b derives every expression result, requires exact equality with `Expr.ty`, then requires initializer, assignment, return, break, argument, capture, stage, and tail positions to equal their declared producer type and ownership facts. | valid producer twin for all 239 `ExprKind` variants; wrong `Expr.ty` and wrong consumer position for every family |

Am-p owns this table and nothing else. It validates global/table/header placements whose producer is
body-independent. The body-correlated final row is specified here but implemented only by am-b.
This keeps `am-p` independent and prevents it from guessing whether a graph-valid local type was
actually produced by its initializer.

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

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[repr(u16)]
pub enum RuntimeKey { /* the 281 variants below, in that exact order */ }

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

`ProgramCall::new` is crate-private and accepts only a non-empty, NUL-free exact UTF-8 logical name
already present in the validated program/import/extern registry. It owns one boxed copy; that
storage and its clones are ordinary compiler allocations. The allocation itself never reaches an
artifact, while the semantic name bytes participate in the structural MIR hash, encoded
`align_fn$...` identity, `GeneratedId` canonical bytes/stems, LLVM/object symbols, and debug output
exactly as specified below.
`RuntimeKey` is `Copy`. `DirectCall` and `GeneratedId` are compiler-owned values with lowering-call
or codegen-module lifetime; no runtime allocation or Drop contract is introduced.

The exact MIR field change is one-for-one:

| Current field | Am-c field |
|---|---|
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

`CanonicalFnAbi` is the ordered parameter list of `{ mode, CanonicalTy }`, the return
`CanonicalTy`, and exact borrow and region summaries. It has no return-cleanup field in am-c:
`ReturnCleanupAbi` does not exist until later L2c, which must extend this record and its byte
encoding atomically when it lands. It excludes effect because effect changes call legality, not the
physical thunk signature.

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
references, non-canonical root/definition order, and any type the am-g-t/am-p/am-h contract rejects.

`CanonicalTy` is `version=1:u8 || node_count:u32 || nodes || root_type`. Nodes are assigned ordinals
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
| 4 | function: parameter count, each mode and scalar, return type, borrow summary, region summary; no effect or raw fn-table id |

Struct/enum nodes include nominal `source_name` and their complete reachable shape. They exclude
origin-aware private `name`. The fingerprint is nominal plus structural: different public nominals
never merge, and the same source nominal with a different reachable graph never shares a helper or
cache key. Cycles through box, function, or another header-mediated edge terminate through node
references rather than truncation.

The `root_type` tags `0..=56`, in exact order, are:

```text
Int Float Bool Char Option Result Tagged Box Array Vec Mask StructArray DynStructArray Slice Soa
DynSliceArray DynArray DynResponseArray Str String ArenaHandle Raw Builder Writer Reader Buffer
ArrayBuilder StrFinder File Rng Regex Captures CliCommand CliParsed TcpConn TcpListener UdpSocket
Child Command RunOutput HttpRequest HttpResponse HttpClient HttpServer HttpRequestCtx
ResponseBuilder HttpStream HttpHeaders JsonDoc JsonScanner Struct Tuple Fn Enum Task DictEncoded Unit
```

`Int` is `signed:bool || bits:u8`; `Float` is `bits:u8`. `Bool`, `Char`, the closed handles, `Str`,
`String`, `Raw`, and `Unit` have no payload. `Option`, `Result`, `Box`, `Array`, `Vec`, `Mask`,
`Slice`, `DynArray`, `ArrayBuilder`, and `Task` encode their scalar(s), then any `u32` length/lane.
`DynSliceArray` encodes a primitive scalar. `StructArray` encodes a struct-node reference and
length; `DynStructArray` encodes a struct-node reference and layout (`0=Aos`, `1=Soa`); `Soa`,
`JsonScanner`, and `Struct` encode a struct-node reference. `Tagged`, `Tuple`, `Fn`, and `Enum`
encode the matching node reference. `DictEncoded` encodes a struct-node reference then field
ordinal. `DynResponseArray` has no payload.

Valid scalar tags `0..=33`, in order, are:

```text
Int Float Bool Char Unit Struct String DynArray DynStructArray DynResponseArray Str Slice Enum
Tagged Soa JsonDoc Reader Writer Buffer Regex Captures CliParsed TcpConn TcpListener UdpSocket
Child File HttpResponse HttpServer HttpRequestCtx ResponseBuilder HttpStream RunOutput Fn
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

`GeneratedId` is `version=1` followed by `0=FnValue`, `1=Closure`, `2=Task`, or `3=Parallel`, then
the fields in the Rust declaration order above. `ProgramCall` is encoded as its length-prefixed
UTF-8 bytes. A vector is `u32 count` then elements. `ParallelGeneratedId` encodes its nine fields
in declaration order. `ParallelKernelMode` is its explicit `repr(u8)` value.
`ParallelStageId` is `0=Map`, `1=Filter`, `2=FilterStrContains`, `3=Project`, or
`4=FilterField`, followed by that variant's fields in declaration order. Decoding repeats the
semantic validity relations above and rejects a mode/stage combination that no producer emits.
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
producer and consumer units. Explicit `--export` roots retain their requested exact external
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
| compatible extern logical/emitted name equal to one of the 286 fixed base native symbols | accept and reuse the one native declaration |
| extern or export equal to one of the eight verification-only probe spellings | accept under the ordinary program/external rules in a normal build; probe-feature runtime fixtures never link user artifacts |
| incompatible extern equal to a fixed base native symbol | reject |
| explicit export equal to a native symbol, extern emitted identity, or external `main` | reject |
| direct-i32 `main` plus any other claimant of external `main` | reject; otherwise direct main alone emits `main` |
| wrapped main plus any other claimant of external `main` | reject; otherwise wrapper alone emits `main` and Align body remains encoded |
| two equal `GeneratedId` values | dedupe |
| unequal generated values with the same readable stem | deterministic `$0`, `$1`, … probe |
| generated candidate equal to any program/native/external identity | probe; never reject the source spelling |

Owner tests include separate positives for a non-exported runtime-key-equal logical name, a
non-exported native-symbol-equal name, and a generated-stem-equal name; compatible and incompatible
native externs; stored-definition/extern and imported/extern cross-class rejection even when unused;
ordinary extern/export use of every probe spelling in a normal build; explicit export collision;
direct and wrapped main; duplicate equal generated records; and unequal generated records with
one/two occupied probe candidates.

The exact `RuntimeKey` set is:

```text
alloc alloc_size_fail arena_alloc arena_begin arena_end
array_builder_append array_builder_build array_builder_build_stack array_builder_free
array_builder_free_stack array_builder_free_strings array_builder_free_strings_stack
array_builder_init_stack array_builder_new array_builder_push array_builder_push_str
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
key. Three further always-built runtime exports are unkeyed base rows. Four `alloc-count` probes
and four distinct `par-map-probe` exports are verification-only runtime-fixture records; their
names remain ordinary program/extern/export spellings. `task-group-probe` adds no unmangled export. The
four AEAD cross-product symbols are ordinary keys
rather than a codegen-side string match.
[`20-runtime-abi-ledger.md`](20-runtime-abi-ledger.md) owns all 281 keyed symbol/type/attribute
records, the five always-built unkeyed records, and the eight verification-only probe records.
The compiler registry is fixed at 286 base records with no feature or ambient input. The eight
probe rows extend only the verification-time maximum runtime-export table to 294; they are never a
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

Am-h validates, in order, externs, imports, stored functions, then locals:

- modes are parallel to parameters; extern modes are `ByValue`; stored/import modes are
  `ByValue` or `Out`, with `Out` only on `Slice`; `Borrow`/`BorrowMut` remain disabled;
- return borrow and region summaries are identical. `None` is canonical for no roots.
  `Roots { params, captures }` has a non-empty, strictly increasing, in-range `params` vector whose
  referenced parameter types are borrow-capable, and an empty `captures` vector before L2b-b;
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

  `effect` is not part of the Align call ABI or `CanonicalFnAbi`; it is the imported semantic seed
  consumed by the existing effect fixed point. Am-h replaces MIR's current
  `Vec<hir::ImportedFn>` with `Vec<mir::ImportedFn>` in the exact existing field order and strips
  `effect` only after validation. Both structs derive the same `ImportedFn { ... }` structural
  Debug bytes for those six fields, so valid per-unit codegen input, `impl_hash`, interface-summary
  bytes/hash, and cache behavior remain byte-identical. An owner test compares the old unchecked
  MIR rendering/hash and new validated MIR rendering/hash for Pure, Impure, and Unknown imports;
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
parity, and cost evidence. Effect correlation replays the existing source-order effect fixed point
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
`StrTrimKind`. Am-b2 owns `ExprKind::ArrayLit` through `ExprKind::ArrayDictEncode` in declaration
order, plus every `StageKind`, `TemplatePart`, `GroupSource`, `GroupAgg1`, and `GroupOp`. Am-b3 owns
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
| L2b-a2-am-w | Replace traversal-order task wait state with group/epoch-scoped outcome-sensitive successful-wait dominance across Spawn, transparent Result local/copy/reassignment/`map_err` flow, transparent Move Task-handle origin flow, Try, Match, Else, nested groups, and every control join. | `task_get_successful_wait_dominance_matrix`, stored-result and Spawn-alias-invalidated twins, inner-Wait/outer-Task rejection, already-waited outer positive, outer stored-Wait proof handled inside inner positive, inner proof leaves outer false twin, inner Wait Result handled after group exit remains unproven, repeated primitive get, whole/per-unit task-group runtime owners; compile-time proof-alias scale row only |
| L2b-a2-am-v | Require an exact bound `mut Buffer` local at the five native output positions without applying source `mut` to native objects that mutate only interior runtime state. | `native_output_buffer_requires_mut_local`, five runtime/allocation twins, whole/per-unit parity; no benchmark row |
| L2b-a2-am-u | Reject extern `FnValue`; require lexical `unsafe` at direct extern Call and every named extern pipeline/reducer/sort invocation, with inference-only signature peeks remaining silent. | `extern_invocation_permission_matrix`, direct/callback/FnValue whole/per-unit owners; no benchmark row |
| L2b-a2-am-p | Activate all body-independent placement predicates; invalid placement is canonical-empty while graph-valid but position-invalid twins distinguish every producer set. | `malformed_hir_type_placement_fails_closed`, `valid_hir_type_placement_preflight_is_mir_identity`, deep valid placement and deep malformed later-sibling precedence; `mir-type-placement-validation` |
| L2b-a2-am-n | Activate exact nominal/source-shape, tuple, alignment, enum-base, and link validation. | `malformed_hir_nominal_link_metadata_fails_closed`, shallow/deep equal-shape origin twins, and deep malformed later-sibling precedence; `mir-nominal-link-validation` |
| L2b-a2-am-h | Activate extern/import/stored/main/local body-independent header validation, including normalized imported-effect transport. | `malformed_hir_declaration_header_metadata_fails_closed`, `valid_hir_declaration_header_preflight_is_mir_identity`, imported Pure/Impure/Unknown/absent-normalization twins, and deep signature/summary valid/malformed twins; `mir-header-validation` |
| L2b-a2-am-b1 | Dormant total-validator core: every statement, local/place, ordinary expression/call/aggregate, tagged value, and structured-control record. | direct table-driven unit owner plus deep valid/malformed ordinary-body type relation; no public benchmark row yet |
| L2b-a2-am-b2 | Dormant storage/vector/array/pipeline/template/JSON validator, including every stage, terminal, capture, and descriptor record. | direct table-driven unit owner plus deep valid/malformed storage/pipeline type relation; no public benchmark row yet |
| L2b-a2-am-b3 | Dormant native/runtime validator plus generated-callable body facts for all I/O through crypto families. | direct table-driven unit owner plus deep valid/malformed native/generated signature relation; no public benchmark row yet |
| L2b-a2-am-b4 | Add body-derived Drop/ownership and effect correlation, assert the full inventory, activate all body validation globally, and prove valid identity/four-entrypoint parity. | `malformed_hir_body_metadata_fails_closed`, `malformed_hir_effect_metadata_fails_closed`, `valid_hir_body_preflight_is_mir_identity`, maximum-depth and deep type-DAG stack owners, whole/per-unit codegen; `mir-body-validation` |
| L2b-a2-am-c | Land typed program/runtime/generated call targets, encoded Align symbols, native registry, generated identities, and all name/collision validation atomically across sema/MIR/codegen and whole/per-unit paths. | callable namespace/collision/golden-symbol suites, deep semantic→bytes→semantic plus malformed deep-reference/truncation owners, and per-unit link parity; `mir-callable-namespace-validation` plus unchanged runtime-call cost |

#### Am-f implementation closure matrix

This matrix is authoritative before the return-completeness producer correction begins.

| Cell | Required am-f closure | Exact owner evidence |
|---|---|---|
| formation and validation | A bare `return` is accepted exactly when the active function return is Unit; otherwise it emits exactly `return without a value is only valid in a function returning (); this function returns <type>` at the containing block span. After the checked body is built, a non-Unit block body with no tail is accepted only when the existing source-order control-flow analysis proves its end unreachable; otherwise it emits exactly `function returning <type> has a reachable path without a return value` at the body span. A present tail is checked against the declared return as today. These checks run after return-type formation but before HIR publication; an ill-typed present value retains the existing type diagnostic before completion. | Unit/non-Unit × bare/value return × present/absent tail; wrong value plus missing path diagnostic order; exact spans/messages |
| construction and control | Every reachable path of a non-Unit function ends in `Return(Some(value))`, a typed tail converted by existing lowering, `Try` error propagation, process termination, or a proven diverging loop/control expression. If/Match/Else join only reachable fallthroughs; loop exits use accepted Break paths; nested blocks, Arena, Unsafe, TaskGroup, and lifted lambda bodies retain their own active function return. Dead statements are still structurally checked but cannot create a fallthrough edge. The function root HIR body has an absent reachable tail only for Unit. | straight-line, if/match/else, loop/break, Try, process exit, nested region/unsafe/task group, lifted lambda, and dead-code mutations |
| ownership, Drop, and allocation | Rejected functions publish no HIR/MIR, allocation, move, Drop, cleanup, or cacheable artifact. Accepted typed-return and non-fallthrough paths keep existing return move/nulling, arena/task cleanup, and allocation behavior. | Move/Copy return and cleanup parity, rejected-before-HIR, allocation-count twins |
| generic, interface, whole/per-unit, and cache | Generic source bodies and each monomorph are checked under their concrete return; imported/extern bodyless declarations are unaffected. Whole/per-unit compilation applies the same local-body rule. Accepted interface/source/MIR/impl hashes are unchanged; the compiler-build change alone invalidates cached objects. | generic/monomorph return twins, interface goldens, whole/per-unit and cache miss-then-hit |
| native/ABI and benchmark | No non-Unit function can reach MIR/LLVM with `Return(None)` or a reachable absent tail, so LLVM never emits `ret void` under a value-returning signature. The checks are linear in the already-bounded body walk and add no runtime work. | exact MIR terminators, raw/optimized LLVM verifier and executable results for every admitted return family; compile-time no-regression owner |

#### Am-e/am-w implementation closure matrix

This matrix is authoritative before either producer correction begins. Both corrections precede
am-h/am-b4 so total HIR validation accepts exactly the corrected semantic producer. Its placement
before the am-d matrix is topical, not sequential: am-d implements the depth/type preflight first,
and every later producer/body pass enters only after that guard.

| Cell | Am-e entry closure | Am-w task-wait closure | Exact owner evidence |
|---|---|---|---|
| formation and validation | Source `main` is non-generic and has either no parameters or exact `ByValue array<str>`. No-arg return is Unit, exact signed i32, or `Result<Unit,builtin Error>`; argv requires that Result. After ordinary type formation, main return checks run before the parameter-shape check. An otherwise-valid non-Result return outside the admitted set emits exactly `main returns only (), i32, or Result<(), Error>; got <type>` at the return span. Existing wrong-Result Ok/Error diagnostics retain their order, followed by the existing argv/parameter diagnostic on a multi-invalid declaration. | Each active group owns compiler-only `group`, abstract `current_generation`/`proof_epoch` tokens, optional `completed_generation`, `valid_from`, and a sparse ordered unresolved-Wait set. Every Spawn advances to stable syntax-site/incoming-state tokens, staling all old WaitProofs; completion then differs from current. With an unresolved Wait it also clears that set and advances `valid_from` to invalidate old Tasks; otherwise old Tasks remain eligible for reauthorization. A fallible Wait produces `WaitProof { group, proof_epoch, wait, covers_through }`; Spawn produces `TaskProof { group, born_generation }`. Ok resolves only its id and sets completion to its covered generation only after every earlier id in that epoch is Ok. Err advances proof epoch, clears completion, and advances `valid_from`, poisoning all covered Tasks/Waits. Infallible Wait completes the current generation. A later no-task Wait does not revoke completion. TaskGet requires `born_generation >= valid_from` and `completed_generation == Some(current_generation)`. | all accepted signatures plus every other graph-valid return/parameter mutation and multi-invalid diagnostic order; straight-line infallible/fallible/reset/generation-join, stable token reuse, stale resolved-Wait alias after Spawn, unresolved-first Wait plus successful empty second Wait, first successful Wait plus later unhandled empty Wait, failed-first Wait plus `wait()?` on an empty queue, missing/wrong/stale TaskProof, and nested-group mutations |
| construction and control | Exact i32 emits external `i32 @main()` directly. Unit/Result emit an internal Align body plus an external i32 wrapper; argv wrapper alone accepts `(i32,ptr)`. No other entry ABI is constructible. | A fallible-Wait Result carries exact group/proof-epoch/wait/coverage provenance. Bare local binding, copy/reassignment, block tail, `ResultMapErr`, and value-producing if/match/else/loop preserve it only when every reachable result predecessor has the same proof; unrelated overwrite clears it. Move Task handles transfer `TaskProof { group, born_generation }` through transparent local binding, move/reassignment, block tail, and value-producing control flow; there is no Task-copy path. Try, Result Match, and Result Else resolve or poison the exact still-active proof even while a nested group is active. Multiple aliases resolve idempotently; a stale proof has no effect. Passing a Copy Wait proof leaves the caller local intact but no opaque boundary inherits either proof. Every Spawn/Wait/Err transfer interns its token by syntax site plus incoming group tokens. A byte-identical group-state join retains state verbatim; every differing join reuses its one syntactic-site `join_generation`/`join_proof_epoch`, assigns them to the current generation/proof epoch, clears WaitProofs/unresolved state, sets completion true iff all predecessors completed their current generation, and remaps each Task-valued local/result to the join generation iff every predecessor has a valid same-group Task proof and either completed its current generation or has no unresolved Wait covering the Task; predecessor handles may differ. A loop joins entry with reachable body fallthrough at its stable header site to a byte-identical fixed point, then computes its exit only from accepted breaks. Thus a post-join Wait registers under the join proof epoch, TaskGet checks the join generation, an earlier-iteration unresolved/failed Wait reaches later breaks, completed Spawn+Wait/no-Spawn joins remain readable, incomplete joins require a later Wait, and a drained unresolved Wait cannot hide. A later no-task Wait Result left unhandled does not revoke already-established completion, including across branch and loop joins. Entering a nested group retains outer state; exit removes all proofs naming the inner group. TaskGet checks its originating group/generation bounds, never the innermost position. Return, Try Err, process termination, and a diverging loop have no continuation. Lambdas/functions start empty. | whole/per-unit raw/optimized LLVM exact signatures and link/run exit behavior; direct and stored/copied/reassigned/map_err Wait proof; transparent Task move/reassignment/control proof and rejected Task-copy premise; completed and incomplete asymmetric Spawn/Wait/no-Spawn joins followed by Wait and TaskGet; branch-selected distinct Task handles; unresolved first Wait plus second empty Ok remains unreadable; first Ok then later unhandled empty Wait remains readable straight-line and across branch/loop joins; first Err invalidates the task and every second proof; multi-iteration loop with first-iteration unresolved/failed Wait, later break, post-loop empty Wait, and rejected get; all-success multi-iteration acceptance; stable-token fixed-point convergence; Spawn with an unresolved Wait invalidates the old task generation; successful Wait→Spawn→successful Wait authorizes old and new handles; unrelated overwrite and opaque-boundary nontransport; nested-group, originating-diagnostic, exact/wildcard/or Result arm, terminating Else fallback, branch/loop/early-exit/lambda matrix |
| ownership, Drop, and allocation | Rejected headers construct no HIR/MIR/runtime state. Accepted Unit/i32/Result paths keep existing body ownership and wrapper allocation behavior. | The ambient, WaitProof, and TaskProof maps are compiler-only state. Propagating or clearing them follows existing Copy Result and Move Task evaluation but creates no task, join, read, Align allocation, source nulling, Drop, or cleanup action. Current Task results are primitive Copy values: `TaskGet` is a non-consuming read, preserves the Move handle and its TaskProof, and repeated get is producer-valid. Group cleanup remains byte-identical on accepted input. Owned Task results and their consuming-get/Drop contract remain a separate future slice. | rejected-before-MIR tests; Copy Wait and Move Task local/control bookkeeping twins; repeated primitive get; no source-nulling/Drop/allocation change and MIR equality |
| generic, interface, whole/per-unit, and cache | `main` remains entry-unit-only and non-generic. Interface summaries never export it. Whole/per-unit and ThinLTO preserve exactly external `main`; the compiler-build change invalidates old cached objects, while source/interface hashes for accepted input stay unchanged. | No interface or ABI field is added. Whole/per-unit semantic HIR agrees. Accepted-source MIR/impl hashes stay unchanged; rejected unsafe sources produce no artifact. | interface-byte/hash goldens for accepted entries; ThinLTO off/on; cache miss once then hit; whole/per-unit task twins |
| benchmark | No persistent validation pass is added; one constant-time header predicate replaces invalid backend construction. | Generation replacement invalidates a failed or unresolved batch in O(1). Stable site-token interning is amortized O(1). The loop worklist is monotone: each header completion or local proof fact can be cleared once, and every differing header state canonicalizes to its one site token. Wait resolution and joins therefore examine only sparse live unresolved/proof entries in O(control edges × live proof entries examined); there is no runtime work. | compile-time task-count × unresolved-Wait × branch-count × loop-backedge-count × live-proof-alias scale owner, explicit stable-token convergence bound, plus no regression in existing `mir-header-validation` and task-group rows; no new runtime benchmark |

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

This matrix is authoritative before the extern-call producer correction begins. `unsafe` remains a
lexical invocation permission; am-u does not add an unsafe-callable function type.

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
| construction and control | Producer HIR finalization, lint, region, borrow, Move, Escape, effect, and am-b4 replay walks use explicit enter/exit work items. Strict-child divergence propagates through transparent stage/template records before later siblings are scheduled, while conditional branches remain alternatives and `process.exit`/`process.abort` remain non-fallthrough leaves. MIR strict eager spines whose giant dispatcher frame cannot safely recurse use one heterogeneous child-first worklist from every whole/per-unit located/unlocated entrypoint. Multi-child parents retain the immediate child→owner/action→next-child protocol. Structured `if`, `match`, `else`, short-circuit, loop, arena/task-group, block, and template lowering and the existing specialized file, reader, array-builder, process-command, path, regex, and HTTP groups use out-of-line helpers reached before the giant dispatcher frame is retained. Native helper depth is permitted only because the checked-HIR ceiling bounds it at 259; accepted-boundary owners must prove each recursive family on the ordinary 2 MiB stack. These mechanisms preserve the ledger's envelope-before-child, child source order, post-relation, result-type, and body-fact order across `if`, `match`, `else`, `?`, `map_err`, branch and loop joins, early exits, dead retained nodes, stages, templates, strict string wrappers, and specialized operation groups. No rejected body starts a later phase. The am-d inventory also enumerates every recursive type edge reachable in these phases and in codegen; each edge is migrated to the common worklist or classified with an owner-backed non-recursive/indirection-leaf proof. | exact first-error and reachable-state identity against shallow twins for every control family, including transitively diverging first template holes and stage captures followed by unreachable impure siblings, plus process-termination branch joins; complete recursive-call-site inventory classifying eager-worklist, bounded out-of-line control/specialized operation, immediate required-child, and tail edges; all four entrypoints on the 2 MiB test thread with operation-specific MIR assertions for mixed unary/cast/binary/call, string trim, alternating `str.borrow`/path components/normalization/join, alternating `str.borrow`/regex replacement, self-buffering reader, a producer-valid `Result<str, Error>` `StrBytes`/`BytesAsStr`/`Try` cycle, file create over a deep path, array-builder push over a deep value, process-command construction over a deep command, HTTP request construction over a deep method, block/template, `if`, binary/wildcard `match`, `else`, short-circuit, loop, and arena/task-group roots; Result construction/inspection MIR type-contract checks; final MIR and raw/optimized LLVM verification at the accepted boundary |
| ownership, Drop, allocation, and return | Iterative replay preserves construction, move-in/out, source nulling, Drop, replacement, return, loop cleanup, and allocation order exactly. `drop_plan_rec`, recursively Move classification, borrow capability, region/escape, and ownership predicates accept deep acyclic inline/header-mediated graphs without process-stack recursion. An over-bound body returns canonical-empty before any Align-program/runtime/native/artifact/cache allocation, ownership action, native registration, source-map read, or cache publication. Only compiler-owned validation worklist allocation may occur; it is released on success or rejection. | deep Copy/Move local/return/aggregate and Drop-plan roots; deep borrow/region/escape roots; replacement, branch/loop/early-return, arena/task cleanup, allocation-count, and canonical-empty no-action twins |
| generic, interface, whole/per-unit, and cache | Every stored, monomorphized, and lifted function is an independent body-depth root. Whole/per-unit compilation uses the same body preflight. Interfaces and canonical type fingerprints retain the am-g-t finite type domain without a new depth field or limit. MIR type conversion and LLVM struct body/layout helpers accept deep graphs at stored header, parameter, local, return, and aggregate-field roots through raw and optimized verification. Accepted source/HIR/MIR/interface/impl hashes remain unchanged and only the compiler build id invalidates cached objects. | source/monomorph/lifted boundary fixtures, two-unit deep type-DAG import, deep MIR/LLVM parameter/local/return/field layout and executable twins, interface/hash goldens, and one build-id miss then hit |
| current/future type consumers and benchmark | Before merge, am-d closes every currently active HIR→MIR→LLVM recursive type consumer, including `drop_plan_rec`, `ty_is_move`/`struct_is_move`, `ty_may_borrow`, slice/region/escape/ownership predicates, type/layout lowering, and LLVM struct-body/layout construction. Am-p placement, am-n complete source-shape comparison, am-h signature/summary correlation, am-b1–b4 body type relations, and am-c canonical encode/decode then inherit the common traversal. Each owning slice has a deep valid acyclic-inline and header-mediated DAG plus a deep malformed later-sibling case proving diagnostic precedence. Am-c canonical semantic-to-bytes and bytes-to-semantic traversal is depth-first first-visit order implemented with explicit work items; malformed deep references and truncation reject without using the process stack. Body and type traversal are linear in visited records/edges; compiler-owned worklists are bounded by input size, and MIR structured-control native frames are bounded only by the fixed checked-HIR ceiling and the 2 MiB owner proof rather than by an ambient process-stack assumption. | am-d `deep_type_consumer_closure_matrix` across sema/MIR/codegen/driver roots; cumulative deep-DAG owner in am-p/am-n/am-h/am-b1–b4/am-c; am-c deep semantic→bytes→semantic golden and malformed deep-reference/truncation twins; unchanged `mir-global-type-validation`, later validation rows, and `mir-continuation-lowering` |

#### Am-h/am-b4/am-c implementation closure matrix

This matrix is authoritative before either internal representation change begins. The am-b
ownership/control matrix is the per-record ledger in
[`19-hir-validation-ledger.md`](19-hir-validation-ledger.md); am-c consumes it only after am-b4.

| Cell | Required implementation closure | Exact owner evidence |
|---|---|---|
| formation and validation | Am-h forms exactly one `FnOrigin` for every stored function, derives exportability only through `is_exportable()`, carries one normalized `FnEffect` on every imported HIR declaration, and converts to the six-field effect-free MIR declaration only after header validation. Am-b4 replays stored-body/cross-unit effect inference before am-c can consume a callable fact. Am-c forms `ProgramCall`, `RuntimeKey`, `CanonicalFnAbi`, `CanonicalTy`, and each `GeneratedId` only after am-b4 validation; canonical decoders reject before registry/cache publication. All nested type formation/comparison/canonicalization uses am-d's common explicit-worklist traversal and accepts every finite am-g-t-valid header-mediated DAG. | every origin/flag/count mutation; imported Pure/Impure/Unknown/absent-normalization twins; effect-stripped MIR Debug/impl-hash/interface-byte identity; every stored/projection/join effect-cell mutation and parallel eligibility twin; every canonical tag/width/reference/order mutation; shallow and deep semantic↔byte goldens; deep malformed reference/truncation rejection without process-stack recursion |
| construction | Source declarations record entry/public flags, monomorph worklist outputs record `Monomorph`, every lifted lambda records its exact `u32` capture count, and interface-only declarations copy their exact external effect or normalize absence to `Impure`. Direct calls, function addresses, closures, tasks, and all four parallel kernel modes construct the exact typed target/identity at their current single construction sites. | private/public entry/non-entry, mono, zero/positive capture, imported effect states; direct/native/fn-value/closure/task/materialize/reduce/count/scatter construction owners |
| move-in, move-out, source nulling, Drop, replacement, and return | `FnOrigin`, `RuntimeKey`, and kernel modes are Copy compiler metadata; boxed calls, canonical records, and generated ids use ordinary Rust ownership. They introduce no Align value, source nulling, Drop plan, replacement, return cleanup bit, runtime allocation, or allocation provenance. Existing callable operands/captures retain the am-b4-proved Move/Drop behavior byte-for-byte. | MIR equality excluding the typed metadata field, existing closure/task/parallel Drop and allocation-count owners, and explicit N/A assertions for new metadata |
| body and control paths | HIR `Call`, `FnValue`, `Closure`, `Spawn`, every stage/terminal callable, `ResultMapErr`, and indirect-call signature/effect correlation are validated before conversion. `if`, `match`, `else`, `?`, `map_err`, loop/branch joins, early exits, and malformed input never create or publish a typed target or effect join after a rejected child/body. | all corresponding am-b owner ids, stored/local/projection/result effect-cell mutations, Pure/Unknown/Impure parallel twins, plus malformed-before-registry/cache tests and canonical-empty four-entrypoint parity |
| generic and interface | Concrete generic instances record `Monomorph`; generic templates remain discarded. Imported declarations carry the already-existing interface effect fact only inside checked HIR, convert to the effect-free MIR declaration, and later convert to `ProgramCall` using their exact producer identity. Interface serialization/hash and source ABI fingerprints do not add `FnOrigin`, imported-effect transport, `DirectCall`, or `GeneratedId` and remain byte-identical. | generic source/mono name-equality twins, two-unit import/call/link, imported effect parity, effect-stripped MIR/impl-hash identity, unchanged interface/hash goldens |
| whole-program and per-unit | Whole-program lowering derives internal linkage; per-unit lowering derives external linkage only for `Source { is_entry: false, is_public: true }`. Producer definitions and consumer imports encode the same program identity; direct/wrapped main and explicit exports follow the collision matrix. | whole/per-unit MIR, LLVM, executable, export, main, and ThinLTO off/on parity |
| native/FFI and allocation parity | The compiler registry is the fixed 286-row base surface. The eight probe rows are verification-only runtime exports: never RuntimeKeys, callable declarations, collision reservations, compatible-extern reuse targets, compiler inputs, or cache identity. Probe-feature runtime fixtures never link user artifacts. A compatible extern may reuse only an exact base row; incompatible ABI/attribute or fixed-base external-name collisions reject before LLVM. Native calls, runtime ownership provenance, and success/error allocation counts are unchanged. | base/`alloc-count`/`par-map-probe`/all-feature bidirectional export sets, fixed 281 declaration ABI rows, base compatible/incompatible extern twins, eight ordinary probe-spelling extern/export positives under a normal runtime, cumulative native allocation owners |
| cache and monomorphization | Typed target bytes participate in structural MIR `impl_hash`; the compiler build changes `compiler_build_id`. Monomorph keys and interface hashes remain unchanged. No generated lookup uses a printed stem or raw table id. | one representation-change miss then unchanged hit, monomorph identity twins, generated collision/probe matrix |
| benchmark | Am-h and am-c retain linear validation/registry construction and do no artifact or runtime allocation during validation. Canonical encoding and decoding preserve depth-first first-visit bytes through explicit enter/exit work items rather than native recursion. | `mir-header-validation`, `mir-callable-namespace-validation`, deep semantic→bytes→semantic and malformed-decode rows, unchanged runtime-call and continuation rows |

The clean reviewed #678 approval of these fourteen implementation boundaries fixes L2b at
twenty-three implementation PRs and L2 at twenty-seven. Target the repository's
500-line implementation checkpoint. If an implementation PR is expected to exceed roughly 1,000
changed hand-written lines, record why it cannot split safely before coding.
Am-d is one cross-cutting vertical even if that exceeds roughly 1,000 hand-written changed lines:
splitting the producer/replay/lowering conversion would merge a state in which an accepted
producer-depth body can still overflow a remaining recursive consumer, while splitting the common
type visitor would leave a later phase free to reintroduce the same failure for a valid deep DAG.
The exact body preflight, every current recursive body consumer, the common type traversal, and
their boundary/deep-graph owners therefore land atomically.
Am-c is cross-cutting by necessity because an untagged call string cannot distinguish a
source-accepted user spelling from its runtime key; its closure matrix and complete source/MIR/
codegen owner set land in the same PR. If any body construction PR approaches the bound, split its
dormant inventory row before coding and update this ledger; do not activate a partial validator.

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
ledger inserts am-d, am-e, am-f, am-w, am-v, am-u, am-p, am-n, am-h, am-b1–b4, and am-c before
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

L2b-a2-s, L2b-a2-ac, L2b-a2-am-g-t, and the am-r design gate are fixed completed verticals. Draft
PR #679 implements am-d. After it, the mandatory remaining sequence is am-e, am-f, am-w, am-v,
am-u, am-p, am-n, am-h, am-b1, am-b2, am-b3, am-b4, am-c, af, ar, ap, and t. The first PR
publishes an exact product summary while array, pipeline, and tagged/control forms deliberately
retain the shipped flattened result. It must include product construction, reads, partial writes,
destructuring, ordinary control joins, direct/imported consumption, and whole/per-unit parity
together: omitting a writer or join can under-approximate the same public product fact. The second
PR closes the general MIR continuation invariant for checked HIR. The third adds only global
type-domain validation. The fourteen am-r implementation verticals then apply stack-safety before
the five producer corrections, followed by placement, nominal/link, header, total body metadata, and finally
the callable representation change; af, ar, ap, and t retain their existing dependency order.
Am-c follows am-b4 because its typed/generated identities
consume already validated body callable facts; it must not duplicate or anticipate the b3/b4 body
contract. The tagged slice still
replaces its explicit and implicit `Result` fallbacks atomically. A PR expected to exceed roughly
1,000 changed hand-written lines must first record in this matrix why no narrower safe boundary
exists.
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
2. the qualified source builtins `json.doc`, `json.kind`, and `json.scanner<...>` resolve before
   every qualified user definition, including definitions in a unit literally named `json`;
3. another declared type parameter wins only when used bare and without arguments;
4. another bare source-builtin spelling resolves to that builtin;
5. every remaining bare name resolves through the local-definition index; and
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
import error: `ReservedLocalType`, `DuplicateLocalType`, `DuplicateTypeParameter`,
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
| Provenance record formation | L2a | every named/imported/function-value signature contains canonical sorted parameter-root borrow and region summaries, including explicit `None`; L2b-a1 requires the two records to agree; named-return inference uses a reverse direct-call worklist so a changed summary reprocesses only its callers; am-h replaces the ambiguous checked-HIR `lifted_capture_count`/`exportable` pair with the single required `FnOrigin` record and no name spelling decides whether inference or linkage applies | duplicate, unsorted, out-of-range, exported capture roots, borrow/region disagreement, roots inconsistent with resolved parameter/return types, a local definition using the producer-reserved exact name `Error`, `argon2_params`, or `regex_match`, duplicate/ambiguous local definitions or type parameters, a function/struct/sum type parameter shadowing a local definition, an otherwise-unresolved parameter-with-arguments, wrong local/source-builtin arity, unresolved bare names, recursive generic-capability bindings, an exposure-aware positive constructor-growth edge in a declaration-parameter dependency cycle, generic-body/type-parameter shape disagreement, and every missing or recursive nominal/tuple/tagged id reachable through any by-value `Ty`/`Scalar` wrapper reject before consumer-visible side effects in the stated total order; the complete struct/sum definition set is scanned for reserved names before duplicate detection; `generic_body` is precisely the producer's item-span fragment: it starts at `fn` for a function or the declared type name for a struct/sum, omits `pub` and every struct `align`/`layout` prefix, and contains exactly that full declaration/body; validation reconstructs `pub` plus canonical `align(N)` then `layout(C)` prefixes from the structured record, rejects a module/import, extra item, trailing non-END token, or syntax error, parses exactly one declaration, and compares its kind, name, ordered type parameters/bounds, ordered function parameter modes/types and return type, ordered struct fields plus reconstructed layout attributes, or ordered sum variants against the structured record; an extra `pub` in the fragment is a syntax error because visibility is reconstructed rather than compared; function parameter names and generic function implementation expressions are deliberately transported but are not separate structured interface fields; a structured generic `layout(C)` struct returns `GenericCLayoutUnsupported`, within a struct that gate precedes `GenericBodySyntax`, and syntax precedes `GenericBodyMismatch`; all three precede header validation; reserved-local-name rejection precedes duplicate-local-definition rejection and has the exact `ReservedLocalType(name)` import error; producer and importer both validate generic parameter lists in stored declaration/parameter order with duplicate-before-shadow precedence; a local definition sharing any other source-builtin spelling is not a duplicate, and non-shadowing type-parameter, qualified `json.*` builtin, bare builtin, exact local, unit-prefix foreign, and other foreign resolution follows the recorded sema precedence; positive acyclic transformations and zero-weight cycles remain valid and parallel zero/positive edges remain distinct; non-empty generic-template and nested function-value summaries reject until their consumer-side transports exist; interface analysis uses one structured definition index, a least-fixed-point `{intrinsic borrow, dependent parameter positions}` summary and a separate greatest-fixed-point growth-transport summary per local definition across all public roots, with capability-aware opaque stops for transport, complete direct-actual measurement for edge weight, and no recursive instantiation; layout validation shares completed nodes across the program and uses an iterative enter/exit traversal; both layout and borrow-capability traversal through header-mediated nominal cycles are cycle-safe and never overflow the compiler stack | L2b computes non-empty roots |
| Interface codec/hash | L2a | mode plus borrow/region summaries have independent byte/hash goldens and producer/consumer parity | truncated, trailing, unknown-tag, unsupported-known-mode, and semantic inconsistency cases reject | L2c adds cleanup ABI atomically |
| Existing return provenance | L2b-a1/a2 | a1 preserves conservative flattened parameter roots through recursion, assignment, control flow, explicit/implicit/early return, and direct/imported calls; only reachable explicit returns, loop breaks, and trailing values contribute roots or loop post-state: eager children follow source order and stop after the first non-fallthrough child; `&&`/`||`, `if`/`match` arms, and `else` fallback fork from their common incoming state, retain every reachable dependency/return edge, exclude a diverging alternative from post-state, and join only fallthrough alternatives; `?` evaluates its operand once, contributes its reachable implicit error-return roots only when the enclosing return can borrow, and continues post-state only through the success edge; a loop builds its back-edge only from body fallthrough, its post-state only from reachable breaks, and is non-fallthrough when none exist; checker-owned evidence records the exact statement span of each `break` accepted for its target loop after the target/lambda and newly nested `arena`/`task_group` gates but before payload validation, then a post-check source-order classifier counts only reachable spans from that per-loop set and consumes the separately recorded fallthrough result of each nested loop; HIR carries the same accepted-edge bit on every checked `break`, and effect inference, EscapeCheck, MoveCheck/return-provenance, and MIR lowering may form a loop-result join, escape edge, move/borrow post-state, provenance root, or loop-exit terminator only from an accepted edge; a region-rejected `break` emits its region diagnostic first, checks and preserves its payload only for nested type/effect/ownership/escape diagnostics, records no accepted edge, remains non-fallthrough for recovery in every consumer, lowers fail-closed to `Unreachable` if malformed HIR is forced into MIR, and can neither satisfy an assertion nor combine with an unreachable accepted break to make the loop fall through; statements and tails after a non-fallthrough statement are never visited, so no dead edge can taint a summary or caller liveness; a2 recursively refines struct, tuple, fixed-array, tagged, `else`, `?`, `map_err`, and branch/loop projections | indirect/unresolved higher-order targets retain all compatible roots; incompatible joins reject; semantic import rejects provenance on every compiler-known non-borrowing builtin (`Error`, `argon2_params`, and `regex_match`) before per-unit checking | L2b-b adds function-value/capture roots; L3 adds resource/dependent roots; L4 adds explicit region owners |
| Effect source-order closure | L2b-a1 | each structural pass visits every reachable eager child once in language order; loop refinement may repeat that pass but every call, impurity flag, and boundary join is monotone and idempotent across fixpoint passes; block traversal stops after the first non-fallthrough statement and visits a tail only when the block falls through; an accepted `break value` visits the reachable effects inside `value` but joins its function-value/concrete effect into the target loop result only when `value` itself falls through to that break edge; ordinary fallthrough accepted breaks still join; written pipelines evaluate source, stage operands, terminal arguments, then terminal captures/action; `if`, `match`, `else`, short-circuit, `?`, `map_err`, nested blocks/regions, loops, explicit return, inner break, calls, aggregates, assignments, captures, pipelines, and process termination use the exhaustive product below and the same fallthrough contract as return provenance | no dead eager sibling, statement, tail, operation, branch-result, terminal argument, stage, or outer break whose payload already terminated can taint a local/result/expression boundary, named-call dependency, direct/indirect impurity, unresolved dispatch, parallel-callback purity, or fixpoint; a rejected break still visits reachable payload diagnostics but never joins a loop result; projection queries cannot reintroduce a dead tail; no conservative default may turn a proven non-fallthrough payload into a result edge | L2b-b extends the same source-order walker to function-value/capture roots |
| Pipeline terminal formation and MIR closure | L2b-a1 | type formation may inspect named stage/terminal signatures for hints without evaluating an expression, then validates source, stage operands, terminal arguments, and terminal callable in written order; the first invalid operand is reported and later operands of that terminal are not checked; finalization/lints still visit every child of successfully formed HIR, including control-flow-dead syntax; EscapeCheck isolates later syntax in predecessor-less diagnostic CFG state after termination; EffectScan and MoveCheck form state only from the reachable prefix; EffectScan separates stage capture/operand formation from callback action, and joins stage/terminal calls plus callback boundaries only after every pre-terminal operand and terminal capture falls through; function-value effect state joins assignments monotonically, so a later operand cannot make an earlier Impure/Unknown capture Pure; MIR snapshots each stage capture once after the source and at that stage's written position, evaluates explicit terminal arguments, then snapshots terminal callable captures once; the loop reuses those captured operands; MoveCheck snapshots the source's owner roots at source formation and revalidates them after terminal arguments, alongside already-formed stage-view captures; direct, zip, JSON-scanner, and control-flow-selected sources retain every reachable selected owner; return/break before action removes the analysis snapshot from current and saved loop-break states; `sum`, `count`, `any`, `all`, `min`, `max`, `sort`, `sort_by_key`, `to_array`, `map_into`, `partition`, `par_map`, `reduce`, and `scan` share that formation/action boundary; `map_into(dst)` evaluates `dst` after stage snapshots but before any stage action; `reduce`/`scan` evaluate `init` between stage and reducer snapshots; an accepted `break value` lowers `value` once and, only if the selected continuation has at least one reachable predecessor, reads the target loop frame, stores the result, nulls a moved source, emits iteration drops, transfers cleanup, and jumps to that loop's exit; a mixed `if`/`match`/`else`/`?`/short-circuit payload keeps the outer edge only for its fallthrough alternatives, and a nested loop's own break may yield a payload that then reaches the outer edge; when every reachable payload path terminates, the inner terminating construct owns the only result/return/process edge and its required cleanup; fixed, dynamic, and zipped sources share the same order; every JSON-scanner reducer follows it | multi-invalid terminal precedence is source before stage before terminal argument before terminal callable, with only the earliest invalid operand diagnosed; checked-HIR dead syntax still finalizes and lints; no dead child joins reachable effect, move, return, borrow, or escape state; a terminating terminal argument retains earlier stage-operand state but adds no stage/terminal action, call dependency, or callback boundary; capture loads are neither repeated per iteration nor moved across a later terminal argument; an owner invalidated after direct/zip/scanner/control-flow source formation rejects before action; no analysis snapshot survives a terminating return/break; an un-terminated zero-predecessor join is not fallthrough; after payload termination MIR emits no outer result store, Unit fallback, source nulling, iteration Drop, cleanup transfer, loop-frame lookup, or outer exit edge; after pipeline source/stage/terminal-argument termination MIR emits no later operand, capture snapshot, accumulator/output allocation or store, loop/control state, callback call, source cleanup transfer, or result; nested accepted break, explicit return, `process.exit`, `process.abort`, and fully diverging nested block/`if`/`match`/loop payloads preserve their typed result and cannot be overwritten or double-cleaned; malformed HIR remains fail-closed without panic; JSON-scanner `scan` remains rejected | L2c reuses the same post-lowering continuation gate before cleanup-bit transfer |
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
| L2b-a1 | `cargo test -p align_interface --test summary`; `cargo test -p align_sema ty_may_borrow_is_cycle_safe_for_header_mediated_nominals`; `cargo test -p align_sema lifted_function_origin_metadata_is_explicit`; `cargo test -p align_sema checked_break_acceptance_is_preserved_in_hir`; `cargo test -p align_sema rejected_break_effect_payload_is_visited_without_loop_result_join`; `cargo test -p align_sema effect_source_order_closure_matrix`; `cargo test -p align_sema pipeline_terminal_snapshot_action_order_matrix`; `cargo test -p align_sema pipeline_terminal_diagnostic_order`; `cargo test -p align_sema pipeline_terminal_dead_state_isolated_across_analyses`; `cargo test -p align_sema pipeline_terminal_dead_hir_is_finalized_and_linted`; `cargo test -p align_sema pipeline_capture_owner_invalidation_is_rejected`; `cargo test -p align_sema pipeline_source_snapshot_owner_invalidation_matrix`; `cargo test -p align_codegen_llvm malformed_mir_type_graphs_fail_before_llvm_construction`; `cargo test -p align_mir rejected_checked_break_lowers_to_unreachable`; `cargo test -p align_mir terminating_break_payload_emits_no_outer_edge`; `cargo test -p align_mir mixed_break_payload_preserves_outer_edge`; `cargo test -p align_mir terminating_pipeline_operand_emits_no_terminal_state`; `cargo test -p align_mir pipeline_terminal_snapshot_action_order_matrix`; `cargo test -p align_mir pipeline_terminal_source_shape_parity`; `cargo test -p align_driver --test return_provenance --test analysis_coverage --test interface_param_modes --test per_unit`; `cargo test -p align_driver --test m5 json_scan_reduce_fold`; `cargo test -p align_driver --test zip_pipeline pipeline_terminal_source_order` | `bench/library_boundary/run.sh provenance`: `summary-inference`, `import-validation` |
| L2b-a2-s | `cargo test -p align_sema projected_return_provenance_fails_closed`; `cargo test -p align_driver --test return_provenance --test per_unit` | `bench/library_boundary/run.sh provenance`: `summary-inference` |
| L2b-a2-ac | `cargo test -p align_mir eager_expression_termination_matrix`; `cargo test -p align_mir malformed_hir_continuation_metadata_fails_closed`; `cargo test -p align_driver --test mir_continuation`; `cargo test -p align_driver --test expr_depth within_limit_chain_compiles_and_runs`; `cargo test -p align_driver --test per_unit_codegen eager_expression_termination` | `bench/library_boundary/run.sh provenance`: `mir-continuation-lowering` |
| L2b-a2-am-g-t | `cargo test -p align_mir malformed_hir_global_type_metadata_fails_closed`; `cargo test -p align_mir valid_hir_global_type_preflight_is_mir_identity`; `cargo test -p align_mir`; `cargo test -p align_driver --test per_unit_codegen` | `bench/library_boundary/run.sh provenance`: `mir-global-type-validation`, `mir-continuation-lowering` |
| L2b-a2-am-r | design-only completed-ledger consistency check and fresh adversarial review; no implementation target | none |
| L2b-a2-am-d | `cargo test -p align_sema checked_hir_depth_closure_matrix`; `cargo test -p align_sema deep_type_consumer_closure_matrix`; `cargo test -p align_mir checked_hir_depth_closure_matrix`; `cargo test -p align_mir deep_type_consumer_closure_matrix`; `cargo test -p align_codegen_llvm checked_hir_depth_closure_matrix`; `cargo test -p align_codegen_llvm deep_type_consumer_closure_matrix`; `cargo test -p align_driver --test expr_depth --test per_unit_codegen --test deep_type_graphs` | existing compile/MIR/codegen and `mir-global-type-validation` rows only; no new runtime benchmark |
| L2b-a2-am-e | `cargo test -p align_sema main_signature_matrix`; `cargo test -p align_codegen_llvm main_abi_matrix`; `cargo test -p align_driver --test main_abi --test unit_main_exit_code --test per_unit_codegen --test thin_lto` | existing compile/codegen rows only; no new benchmark |
| L2b-a2-am-f | `cargo test -p align_sema function_return_completeness_matrix`; `cargo test -p align_mir function_return_completeness_matrix`; `cargo test -p align_codegen_llvm function_return_completeness_matrix`; `cargo test -p align_driver --test value_control_flow --test analysis_coverage --test per_unit_codegen` | existing compile/control rows only; no new benchmark |
| L2b-a2-am-w | `cargo test -p align_sema task_get_successful_wait_dominance_matrix`; `cargo test -p align_sema task_wait_proof_alias_scale`; `cargo test -p align_driver --test task_group --test per_unit_codegen` | `bench/library_boundary/run.sh provenance`: `task-wait-proof-flow`; no new runtime benchmark |
| L2b-a2-am-v | `cargo test -p align_sema native_output_buffer_requires_mut_local`; `cargo test -p align_driver --test m9_io --test m12_file_io --test m11_net --test m11_crypto --test per_unit_codegen` | existing native I/O rows only; no new benchmark |
| L2b-a2-am-u | `cargo test -p align_sema extern_invocation_permission_matrix`; `cargo test -p align_driver --test ffi --test ffi_views --test ffi_link --test fn_values --test m5 --test per_unit_codegen` | existing extern/callable rows only; no new benchmark |
| L2b-a2-am-p | `cargo test -p align_mir malformed_hir_type_placement_fails_closed`; `cargo test -p align_mir valid_hir_type_placement_preflight_is_mir_identity`; `cargo test -p align_mir deep_hir_type_dag_placement_is_stack_bounded`; `cargo test -p align_mir`; `cargo test -p align_driver --test per_unit_codegen` | `bench/library_boundary/run.sh provenance`: `mir-type-placement-validation`, `mir-continuation-lowering` |
| L2b-a2-am-n | `cargo test -p align_mir malformed_hir_nominal_link_metadata_fails_closed`; `cargo test -p align_mir valid_hir_nominal_link_preflight_is_mir_identity`; `cargo test -p align_mir deep_hir_source_shape_is_stack_bounded`; `cargo test -p align_mir`; `cargo test -p align_driver --test per_unit_codegen` | `bench/library_boundary/run.sh provenance`: `mir-nominal-link-validation`, `mir-continuation-lowering` |
| L2b-a2-am-h | `cargo test -p align_mir malformed_hir_declaration_header_metadata_fails_closed`; `cargo test -p align_mir valid_hir_declaration_header_preflight_is_mir_identity`; `cargo test -p align_mir deep_hir_header_type_dag_is_stack_bounded`; `cargo test -p align_mir`; `cargo test -p align_driver --test per_unit_codegen` | `bench/library_boundary/run.sh provenance`: `mir-header-validation`, `mir-continuation-lowering` |
| L2b-a2-am-b1 | `cargo test -p align_mir hir_body_validator_core`; `cargo test -p align_mir hir_body_validator_statements`; `cargo test -p align_mir deep_hir_body_core_type_dag_is_stack_bounded`; no public-entrypoint activation | none |
| L2b-a2-am-b2 | `cargo test -p align_mir hir_body_validator_storage_pipeline_json`; `cargo test -p align_mir deep_hir_body_storage_type_dag_is_stack_bounded`; no public-entrypoint activation | none |
| L2b-a2-am-b3 | `cargo test -p align_mir hir_body_validator_native`; `cargo test -p align_mir hir_body_validator_generated_callables`; `cargo test -p align_mir deep_hir_body_native_type_dag_is_stack_bounded`; no public-entrypoint activation | none |
| L2b-a2-am-b4 | `cargo test -p align_mir malformed_hir_body_metadata_fails_closed`; `cargo test -p align_mir valid_hir_body_preflight_is_mir_identity`; `cargo test -p align_mir max_checked_hir_depth_body_preflight_is_stack_bounded`; `cargo test -p align_mir deep_hir_body_activation_type_dag_is_stack_bounded`; `cargo test -p align_mir`; `cargo test -p align_driver --test expr_depth within_limit_chain_compiles_and_runs`; `cargo test -p align_driver --test per_unit_codegen` | `bench/library_boundary/run.sh provenance`: `mir-body-validation`, `mir-continuation-lowering` |
| L2b-a2-am-c | `cargo test -p align_mir malformed_hir_callable_namespace_fails_closed`; `cargo test -p align_mir deep_canonical_type_codec_is_stack_bounded`; `cargo test -p align_codegen_llvm callable_namespace`; `cargo test -p align_driver --test per_unit_codegen --test exports`; `cargo test -p align_mir`; `cargo test -p align_codegen_llvm` | `bench/library_boundary/run.sh provenance`: `mir-callable-namespace-validation`, `mir-continuation-lowering` |
| L2b-a2-af | `cargo test -p align_sema projected_return_provenance_fails_closed`; `cargo test -p align_mir eager_expression_termination_matrix`; `cargo test -p align_driver --test return_provenance --test per_unit` | `bench/library_boundary/run.sh provenance`: `summary-inference` |
| L2b-a2-ar | `cargo test -p align_mir eager_expression_termination_matrix`; `cargo test -p align_driver --test return_provenance --test borrow_liveness --test struct_index --test chunks --test soa --test m11_http --test m11_http_get_many` | `bench/library_boundary/run.sh provenance`: `summary-inference` |
| L2b-a2-ap | `cargo test -p align_sema projected_return_provenance_fails_closed`; `cargo test -p align_mir eager_expression_termination_matrix`; `cargo test -p align_driver --test return_provenance --test per_unit` | `bench/library_boundary/run.sh provenance`: `summary-inference` |
| L2b-a2-t | `cargo test -p align_sema projected_return_provenance_fails_closed`; `cargo test -p align_driver --test return_provenance --test per_unit` | `bench/library_boundary/run.sh provenance`: `summary-inference` |
| L2b-b | `cargo test -p align_driver --test return_provenance --test fn_values --test per_unit` | `bench/library_boundary/run.sh provenance`: `summary-inference`, `indirect-return` |
| L2c | `cargo test -p align_driver --test move_return_cleanup --test owned_tagged_payloads --test per_unit_codegen` | `bench/library_boundary/run.sh move-return`: `copy-return-control`, `move-return-none`, `move-return-some`, `move-return-err` |
| L2d | `cargo test -p align_driver --test borrowed_params shared_`; `cargo test -p align_driver --test return_provenance` | `bench/library_boundary/run.sh shared-borrow`: `by-value-call-control`, `shared-borrow-call` |
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
- forged public definitions named `Error`, `argon2_params`, or `regex_match` reject with
  `ReservedLocalType` before duplicate-local and type-shape validation, matching the producer's exact
  reserved set without rejecting any other source-builtin spelling; the precedence owner fixture
  places an earlier same-kind duplicate before a later cross-kind reserved definition, so a
  per-definition interleaved scan cannot pass;
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
