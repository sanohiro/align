# std.log — public contract and implementation design

> 🌐 **English** · [Japanese](./ja/log.md)

> **Status:** designed. Implementation is the next post-`core.test` capability.

## Authoritative public-contract ledger

This table is the authority for the first `std.log` capability. Later prose may explain it but
must not widen it. `logger` below is written by its source name `log.logger`, and `level` by
`log.level`.

| Public surface | Exact inputs, defaults, validation, and evaluation | Exact result, errors, and effects | Ownership, lifetime, allocation, and cleanup | Compiler/runtime/package owner and identity | Acceptance owner |
|---|---|---|---|---|---|
| `log.level { Debug, Info, Warn, Error, Off }` | A closed builtin tag-only sum type. The exact tags are `Debug=0`, `Info=1`, `Warn=2`, `Error=3`, and `Off=4`; that order is the severity order. There is no numeric conversion, custom level, name parser, default, alias, or ambient override. A record level is enabled exactly when it is not `Off` and its tag is greater than or equal to the logger's minimum tag. A minimum of `Off` disables every record. | Copy and Pure. `log.level.Off` is valid only as a disabling value: `enabled(Off)` is false and `line(Off, ...)` emits nothing. No level operation returns `Error` or aborts for a source-formed value. | Inline `i32`; no borrow, allocation, or Drop. | `align_sema` owns the unique builtin enum definition and qualified type/variant resolution behind `import std.log`; HIR/MIR and native calls carry the exact `i32` tag. `align_interface` serializes the existing nominal named-type/enum form; no interface encoding tag or format-version bump is needed. | Import/type/variant positives; exact ordinal and threshold Cartesian product; Copy/move/print/equality negatives required by the ordinary enum rules; checked-HIR wrong-enum-id and bad-variant rejection. |
| `log.new(output: writer, minimum: log.level) -> log.logger` | Exactly two positional arguments and no defaults. Arguments evaluate left to right. `output` must be a complete owned writer source and is consumed only after both arguments check; its source slot is nulled on the success path. `minimum` is one of the five closed tags. There is no stdout/stderr/file constructor and no environment/config lookup. | Returns one initialized logger. Pure: it performs no write, flush, close, clock, process, thread, or other externally visible I/O. Source-formed construction has no recoverable error; terminal OOM follows the language-wide abort policy. | The returned Move handle owns one logger shell allocation and the consumed writer handle. The logger becomes the sole owner of the writer allocation and optional 64 KiB buffer, but it does not change descriptor provenance: an owning writer still closes its fd, a static standard-stream writer still borrows its process fd, and a connection-derived writer still borrows its `tcp_conn`. Exactly `region_of(LogNew) = region_of(output)` and `Ty::Logger` is region-tracked, so a borrowed-descriptor logger cannot outlive its writer's owner through a local, carrier, field, or return. No path/text view is retained. Logger Drop best-effort frees the underlying writer through its ordinary flush-then-close-if-owned path, then frees the logger shell. | `Ty::Logger`/`Scalar::Logger`, `ExprKind::LogNew` with the writer-derived region fact, MIR construction, and one keyed native constructor. The runtime shell is private and carries `{ writer, minimum, first_error }`; compiler provenance, not a second runtime owner, keeps a borrowed fd live. The public type is nominal, not structural. `log.logger` crosses an interface as the existing `IType::Named` spelling; its return-region summary, reachability, and `std.log` capability enter the existing interface/implementation hashes and compiler/runtime ABI fingerprints. | Arity/type/import/shadowing; every complete/incomplete static, owning, and connection-derived writer source; local/function/direct-imported/function-value/branch/loop/`?`/`map_err` construction, region propagation, escape rejection, and source nulling; tagged/field/return provenance; one-shell allocation/free count; no-I/O constructor; malformed checked-HIR; whole-program and per-unit imported signatures. |
| `l.enabled(level: log.level) -> bool` | `l` must be a bound initialized logger local or an admitted borrow receiver; it is borrowed read-only. The level argument is evaluated eagerly after the receiver. The result is false for `Off`, a tag below the minimum, or once any sink failure is latched; otherwise true. It reads no environment and does not probe the fd. | Total, Copy, and Pure. It never clears or changes the failure latch and returns no `Error`. | No move, retained borrow, allocation, I/O, or state mutation. The bool outlives the logger. | `ExprKind::LogEnabled` and a keyed native predicate over the private logger state. HIR validation requires the unique `log.level` enum and exact Bool result. | Five-by-five minimum/record matrix before failure and the five record levels after failure; borrowed receiver; no mutation/I/O/allocation; import and wrong-receiver/type negatives. |
| `l.line(level: log.level, message: str|string|builder) -> ()` | `l` must be a bound initialized logger and is mutably borrowed for the call; it is never consumed. Receiver, level, then message evaluate eagerly and exactly once. An owned `string` is auto-borrowed as `str`; a builder is borrowed and not consumed. Gating occurs inside the call after ordinary argument evaluation. Callers must use `if l.enabled(level) { ... }` when skipped message construction itself must be avoided. A disabled, `Off`, or already-failed call performs no sink I/O and does not mutate state. For an enabled call, the complete text view is validated before the first sink write. Text is UTF-8; embedded NUL and tabs are admitted. The byte transform is exact and allocation-free: `\` becomes `\\`, LF becomes `\n`, CR becomes `\r`, every other UTF-8 byte is unchanged. | Unit and Impure. The exact record is the level prefix (`[DEBUG] `, `[INFO] `, `[WARN] `, or `[ERROR] `), transformed message bytes, then one LF byte. The first nonzero writer status is latched and the remainder of that record is not attempted. The call itself never returns `Error`, aborts because the sink failed, retries the record, rolls back bytes, or writes a fallback destination. A partial record is possible. Later `line` calls perform no sink I/O until Drop; `flush` exposes the first failure. | The message is borrowed for the call only. `line` performs zero owned allocations and uses O(1) extra memory while scanning O(message bytes). Caller-created `string`, builder, or template allocation remains visible and outside the call. It reuses the writer's existing buffering and ownership. It gives no syscall-count, cross-thread/process atomicity, delivery, durability, ordering across different loggers, or terminal-safety guarantee. | Distinct str/string and builder HIR forms lower to keyed native text/builder entries. The native layer calls the existing writer mechanism and owns the prefix/escape scan and first-status latch. It returns `AL_INVALID` for a null logger before dereference. With a live logger, it returns an existing latch first, then validates the closed level; an invalid level latches/returns `AL_INVALID` only when no earlier failure exists. It next gates; a suppressed call returns zero without inspecting message bytes. Only an enabled call validates signed length, representability, nullness, then UTF-8, all before its first write. Zero length may use null; positive length may not. A detectable malformed enabled input latches/returns `AL_INVALID`; a dangling non-null logger/text pointer or builder violates the compiler-private pointer-range precondition. | Exact bytes for all levels, empty/non-ASCII/NUL/tab/backslash/LF/CR/mixed text, str/string/builder/template parity, disabled and post-failure no-write cases, eager evaluation and guarded allocation twins, first-piece/middle/newline/underlying-buffer failure injection, partial-output/no-fallback, zero-allocation owner, unbuffered/buffered writers, malformed ABI/checked-HIR, whole/per-unit codegen. |
| `l.flush() -> Result<(), Error>` | `l` must be a bound initialized logger and is mutably borrowed, not consumed. If a prior first error is latched, it is selected before touching the writer. Otherwise the underlying writer is flushed exactly once. No other logger state or ambient input participates. | Impure. Returns `Ok(())` when no error was latched and the underlying flush succeeds. A prior or newly returned nonzero status maps through the one fixed std errno/status table and returns `Err` with that exact first error; a new flush error is latched before return. Repeated flush after failure returns the same error without sink I/O. A successful flush does not disable the logger and later lines remain allowed. | No allocation, move, close, or retained borrow. Explicit flush is the only source-visible error-observation path. Logger Drop still invokes the underlying writer's best-effort flush/close-if-owned cleanup even after a latch; that final cleanup error is unobservable, exactly like writer Drop. | `ExprKind::LogFlush`, the existing status-to-`Error` MIR helper, and one keyed native flush entry. HIR validation pins `Result<(), builtin Error>`. | Empty/success/failure/repeated-failure/success-then-line cases; exact fixed status mapping; no-I/O replay of a latch; Drop after every outcome; early return/`?`/`map_err`; malformed HIR and runtime null owner. |

### Exact level/prefix/gate table

Rows are the logger minimum and columns the requested record level. `Y` writes the shown prefix;
`-` is disabled. A latched logger changes every `Y` to `-`.

| Minimum \ record | `Debug` (`[DEBUG] `) | `Info` (`[INFO] `) | `Warn` (`[WARN] `) | `Error` (`[ERROR] `) | `Off` |
|---|---:|---:|---:|---:|---:|
| `Debug` | Y | Y | Y | Y | - |
| `Info` | - | Y | Y | Y | - |
| `Warn` | - | - | Y | Y | - |
| `Error` | - | - | - | Y | - |
| `Off` | - | - | - | - | - |

There is no timestamp, source location, target, process/thread id, structured-field map, JSON mode,
multiline mode, terminal escaping, rotation, file opening, dynamic minimum setter, asynchronous
queue, fatal level, global/default logger, or logger-specific formatting API. Logging uses ordinary
eager Align expressions plus the shipped `template`/`builder` methods (`write`, `write_int`,
`write_bool`, `write_char`, and `write_float`). `write_hex` is not a shipped builder method and is
not introduced by this capability.

### Placement and transfer ledger

`log.logger` follows the ordinary bare Move-handle class rather than defining a second ownership
model.

| Position or transition | Contract | Required owner |
|---|---|---|
| Local; by-value, `borrow`, `borrow mut`, or `out` parameter; direct function return | Admitted under the existing Move rules. A by-value transfer nulls its complete source; a borrow never does. A logger constructed from a region-bound writer retains that region through every transfer and cannot escape its descriptor owner. | Type formation, call modes, MoveCheck, EscapeCheck, region/return summaries, return cleanup, interface round trip. |
| User struct field; direct builtin `Option`/`Result` or user-sum payload | Admitted where the existing single-owner handle grammar admits `writer`. The enclosing value becomes Move and recursively drops the active logger exactly once. Field move-in, move-out, replacement, consuming match, `else`, `?`, and `map_err` follow existing aggregate/tagged ownership, and every carrier keeps the logger's writer-derived region. | Field/payload formation, DropPlan, active-tag cleanup, region propagation/escape, complete-source nulling, replacement and branch/loop joins. |
| Array, slice, fixed array, tuple, `box`, builder element, parallel element/result, closure/task capture, global/constant, or user native/extern ABI | Rejected before MIR. These paths could copy, hide, parallelize, or externalize one opaque owner without the required ownership proof. | One diagnostic owner per rejected edge and a fail-closed new-type tripwire. |
| Method receiver | A bound initialized local or admitted borrow place only. An unbound owned temporary such as `log.new(io.stderr, log.level.Info).line(...)` is rejected; bind first. | Shared owned-handle receiver gate plus exact diagnostics. |
| `if`, `match`, `else`, `?`, `map_err`, block tail, loop-carried value, early return | Admitted only when the ordinary complete-source and path-local ownership facts prove one live owner at every join/exit. Rejected paths publish no runtime action. | Parameterized Move/Drop/control-flow matrix. |
| Drop, `process.exit`, abort, successful `process.exec` | Ordinary scope exit and `process.exit` run logger cleanup, which frees the writer before the logger shell. Existing immediate abort and successful exec skip cleanup; no special logging hook or global flush exists. | Existing lifecycle twins plus logger fd/allocation probes. |

### Native ABI delta

Implementation adds exactly six ordinary keyed runtime records. Their exact keys, symbols, LLVM
declarations, and Rust ABIs are:

| Runtime key | Symbol | Exact LLVM declaration | Exact Rust ABI |
|---|---|---|---|
| `LogNew` | `align_rt_log_new` | A114: `ptr @SYM(ptr, i32)` | `unsafe extern "C" fn(*mut Writer, i32) -> *mut Logger` |
| `LogEnabled` | `align_rt_log_enabled` | A115: `i32 @SYM(ptr, i32)` | `unsafe extern "C" fn(*mut Logger, i32) -> i32` |
| `LogLine` | `align_rt_log_line` | A116: `i32 @SYM(ptr, i32, ptr, i64)` | `unsafe extern "C" fn(*mut Logger, i32, *const u8, i64) -> i32` |
| `LogLineBuilder` | `align_rt_log_line_builder` | A117: `i32 @SYM(ptr, i32, ptr)` | `unsafe extern "C" fn(*mut Logger, i32, *mut Builder) -> i32` |
| `LogFlush` | `align_rt_log_flush` | A03: `i32 @SYM(ptr)` | `unsafe extern "C" fn(*mut Logger) -> i32` |
| `LogFree` | `align_rt_log_free` | A62: `void @SYM(ptr)` | `unsafe extern "C" fn(*mut Logger)` |

No curated return, parameter, or function attributes are promised for these rows. `LogFlush` and
`LogFree` reuse existing ABI shapes A03 and A62. The shipped compiler-private `core.test` rows own
A110 through A113; logging's other four exact declarations are new shapes A114 through A117. All
six keys, symbols, declarations, definitions, collision reservations,
export-parity rows, runtime ABI fingerprint, whole/per-unit selection, and checked-in declaration
golden activate atomically. That implementation changes the current exact inventories from
314/331/339 keyed/base/maximum records to 320/337/345 and extends the shape range through A117.
There is no optional feature or target-dependent row.

`LogNew` receives the only live writer pointer and a checked tag. A compiler-generated call always
supplies non-null provenance from an initialized writer and `0..=4`; construction transfers the
pointer into the returned non-null logger and the compiler nulls the source slot. A null writer or
invalid minimum returns null without allocation or consumption. A dangling non-null pointer or a
writer that is not uniquely owned is outside this compiler-private ownership ABI. Terminal
allocator failure aborts. Before dereference, `LogEnabled` returns zero for a null logger or invalid
level; `LogLine` and `LogLineBuilder` return `AL_INVALID` for a null logger; `LogFlush` returns
`AL_INVALID` for a null logger; and `LogFree` is null-safe. With a live logger, `LogLine` and
`LogLineBuilder` return zero after a successful or gated call, the stored status after an existing
latch, and the newly latched status on their first failure; MIR deliberately discards that value.
A builder pointer must be live and borrowed for the call. Detectable invalid text shape uses the
validation and latch order in the public row. The source checker and checked-HIR validator
reject bad tags, types, and moves before runtime selection, so source programs cannot manufacture
the foreign-precondition cases.

The private first-error field stores the exact nonzero `i32` writer status, never an `Error` tag or
formatted text. Zero means no failure. The runtime validates and latches before returning the status;
MIR discards `LogLine`/`LogLineBuilder` status for public Unit and maps `LogFlush` through the
existing single status decoder. No new wire format, persisted record, reflection table, environment
input, or package artifact is introduced.

## Implementation closure matrix

The implementation must close this matrix author-side before its one preflight review. One
parameterized owner may close multiple cells; a cell needs a new test only when existing regression
coverage would not fail for the defect.

| Axis | Required implementation closure | Exact regression owner |
|---|---|---|
| Type formation and import | Register `log.level`, `log.logger`, `std.log`, qualified type/variant lookup, spelling, shadowing guard, and builtin capability. Reject every unimported/wrong-arity/wrong-type/collision edge before HIR. | Sema unit matrix plus driver import/interface tests. |
| Construction and move-in | Check arguments left-to-right, require one complete writer source, form `LogNew`, copy the exact writer region to the logger, transfer only after both check, null exactly that source, and initialize one logger drop flag. Static, owning-fd, and connection-borrowed writers use the same operation without erasing provenance. | Direct local, param, function result, block, `if`/`match`, `else`, `?`, `map_err`, and loop-carried writer constructor matrix plus direct/imported/function-value connection-writer escape twins. |
| Move-out, replacement, return | Sweep Logger through canonical Move/region/drop predicates and all ownership facts. Cover local/field/tagged move-out, replacement cleanup, direct/Result/user-sum return, early return, branch/loop joins, and use-after-move. | `MOVE_HANDLE_TYPES` tripwire, handle-free-key sweep, MoveCheck/EscapeCheck/return cleanup tests, allocation/fd balance. |
| Drop and terminal paths | Map logger to `LogFree`; free underlying writer once before shell. Cover ordinary scope, partial initialization, moved source, active/inactive tag, replacement, function return, `process.exit`, abort, exec, and malformed cleanup facts. | Runtime allocation/fd probes, lifecycle twins, checked-HIR cleanup mutation matrix. |
| `enabled` | Preserve unique enum id, eager single evaluation, pure borrow, complete threshold table, latch suppression, and exact Bool result through replay/MIR/codegen. | 25 threshold cells plus post-latch row; effect, no-I/O, no-allocation, wrong-HIR tests. |
| `line` strings | Preserve receiver/level/message order; validate enabled text before I/O; emit prefix, escaped runs, and LF; stop and latch the first nonzero status; discard public status. | Runtime byte goldens, all validation faults before-side-effect, per-piece injected failures, buffered/unbuffered driver programs. |
| `line` builder/template | Borrow rather than consume the builder, read the same UTF-8 byte sequence, reuse the exact transform/status machine, and leave builder ownership unchanged on every logger outcome. | Str/builder/template parity, builder reuse after line, disabled guarded/eager allocation twins, failure injection. |
| `flush` and error mapping | Read an existing latch before writer I/O; otherwise call writer flush once and latch failure; map through the one MIR helper; preserve logger on Ok/Err/`?`/`map_err`. | Status Cartesian table, repeated-flush no-I/O probe, continued logging after success, owner-control-flow tests. |
| HIR/replay/validation | Add every new expression to depth, clone/replay, effects, ownership, borrow/region, finalization, traversal, cache/semantic projection, checked-HIR validation, and malformed-input fail-closed switches. `tracks_region(Logger)` and `region_of(LogNew)` must consume the exact writer fact through locals, carriers, fields, direct/imported/function-value calls, and returns. No wildcard may silently classify a new form. | Variant sweep tripwire; one-field mutation for child/result/enum id/effect/ownership/region fact; replay identity and borrowed-writer escape matrix. |
| MIR/runtime selection | Add typed MIR forms and all six `RuntimeKey` rows; select only reachable operations; preserve whole-program/per-unit parity; reject malformed types/tags before LLVM. | MIR validation mutation tests, runtime-key inventory/bijection, unused import/no-selection and each-operation selection tests. |
| LLVM/native ABI | Emit exact declarations/calls and opaque pointers; use i32 levels/status; keep all six rows in the typed registry and base exports. No hand-written declaration bypass. | Exact extern-type matrix, declaration golden, key/symbol reverse lookup, base/maximum export parity, rt-LTO on/off. |
| Interface/cache/generics | Serialize `log.logger` and `log.level` through existing nominal names, preserve parameter modes/effects/return-region/return cleanup, instantiate imported generic users without duplicating runtime ownership, and include capability/runtime fingerprints. FORMAT_VERSION remains 8 because the encoding grammar does not change. | Producer/consumer signature and return-region goldens, generic whole/per-unit borrowed/static parity, two-build determinism, interface hash changes on surface use and not on private span-only edits. |
| Allocation/resource promise | One logger shell allocation per successful `new`; zero owned allocations in `enabled`, `line`, and `flush`; O(1) line scratch; exactly one writer/logger free. Caller template/builder allocation is separately observable. | `alloc-count` deltas for str/builder, escaping/non-escaping/disabled paths and Drop; long-message bounded-RSS or allocation-count owner, not a timing benchmark. |
| Diagnostics and docs | Use exact source spellings `log.level`/`log.logger`, explain bound receivers and the `enabled` guard, and keep English/Japanese design mirrors plus normative summaries synchronized. Do not teach the API in end-user guides until implementation ships. | Diagnostic assertions; doc diff and mirror/anchor consistency; example syntax check. |

The implementation is one capability boundary. Its type owner, logger runtime state, calls, and Drop
are a strict producer-to-consumer chain with no independently useful dormant midpoint. Although it
crosses more than three compiler layers and may exceed roughly 1,000 hand-written lines, splitting
it would duplicate the Move-handle, checked-HIR, ABI, and allocation proof while leaving no usable
consumer. This matrix is therefore the required cross-cutting boundary explanation.

### Design-review finding-to-fix ledger

| Finding | Ledger decision and closure |
|---|---|
| P1: consuming a connection-derived writer could erase its borrowed-fd lifetime | Preserve descriptor provenance and define `region_of(LogNew) = region_of(output)` with `tracks_region(Logger)`. The public ledger, placement rules, construction/HIR/interface matrix, specification summaries, and direct/imported/function-value plus tagged/field/return owners now retain and test that region. |
| P1: proposed shapes A110–A113 were already occupied by `core.test` | Reserve the shipped child-control rows explicitly in the runtime ABI ledger and allocate the four new logging declarations as A114–A117. The exact logging range and inventory delta now agree with the registry. |
| P2: malformed null logger behavior contradicted the runtime acceptance owner | Define a result before dereference for every null-taking entry: `LogEnabled` returns zero, line/builder/flush return `AL_INVALID`, and free remains null-safe. `LogNew` likewise rejects null writer or invalid minimum without allocation or consumption. |

## Rationale and usage

The logger is explicit data, not process-global policy. The sink, minimum level, failure checkpoint,
and lifetime remain visible in source:

```align
import std.io
import std.log

fn run(count: i64) -> Result<(), Error> {
  out := io.stderr.buffered()
  l := log.new(out, log.level.Info)

  l.line(log.level.Info, "ready")
  if l.enabled(log.level.Debug) {
    l.line(log.level.Debug, template "items={count}")
  }

  l.flush()?
  return Ok(())
}
```

The `enabled` guard controls construction cost; it is not required for correctness. Without the
guard, ordinary eager evaluation builds the template before `line` sees that Debug is disabled.
The same logger can accept an explicitly assembled builder when formatting needs conditional pieces
or a shipped numeric formatter.

Escaping CR/LF and the escape marker itself gives every successful record exactly one physical LF
delimiter without allocating a replacement string. It does not claim terminal-safe or reversible
Unicode rendering beyond that three-byte-class transform: tabs, NUL, escape characters, bidi text,
and other Unicode controls remain caller data. A security-sensitive terminal or machine protocol
needs an explicit encoder or structured package above this primitive.

Best effort means normal work does not need to branch on every diagnostic write. It does not mean
silent success: callers that care select an explicit checkpoint with `flush()?`. Keeping the first
failure makes that checkpoint deterministic and prevents later writes from obscuring the original
cause. A caller that never flushes accepts the same unobservable Drop error as an ordinary writer.

Formatting stays one mechanism. Templates and builders already make allocation and conversion
visible, so `std.log` adds neither variadic arguments nor reflection-based field formatting. The
logger also deliberately omits time and source metadata: retrieving either would add hidden effects
or compiler injection to an otherwise explicit record.
