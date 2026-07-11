# Session handoff (continue on another machine)

A living continuity note so a fresh Claude Code session — e.g. on a faster machine — can pick the
work up immediately. **If you are a new session: read this, then `CLAUDE.md`, then
`docs/impl/08-nested-structs.md`.** Everything durable is in this repo; the conversation history and
Claude's per-machine memory do not travel with `git clone` (see "Memory" below).

_Last updated: 2026-07-11, sixth wave (**M13 OPENED — Slice 1 MERGED as #418**, symbol
internalization + constant hygiene: codegen previously set ZERO linkage; now the C entry `main`
(incl. the Result-main wrapper) is the SOLE external definition — `align_main` + all program fns +
lifted lambdas = `internal`, the four thunk classes (`$fnval`/`$clos`/`tramp$R`/`$parthunk`) =
`private`, `@str`/`@jfields`/`@jphf` constants = `private unnamed_addr constant`; runtime/
`extern "C"` declares stay external. Three documented free helpers in `align_codegen_llvm`
(no recursive-fn match arms — frame pitfall avoided). IR-shape test `link_hygiene.rs` (6 tests,
mutation-checked: neutered `mark_internal` fails 3, dropped `unnamed_addr` fails 2) pins the map;
size smoke `pipe.o` −33%, no regressions. Key fact recorded (roadmap + decision site): Align has
no separate compilation — one `Program` → one object, `pub` resolves at sema, so `emit-obj`
output is a whole-program object and internalizing is safe by construction; `unnamed_addr` safe
because string `==` is content compare (`align_rt_str_eq`), never address identity. Adversarial
gate SHIP zero CONFIRMED findings (completeness sweep over every `add_function`/`add_global`
site; FFI proven import-only; the multi-object C-helper link path still works). gemini: 1 medium
(explicit `'c` lifetime for sibling-helper consistency) verified and applied. `cargo test
--workspace` **1819 green** (1813 + 6), clippy `-D warnings` clean. **Next: M13 Slice 2**
(capability-based linking + runtime split — `fn main() -> i32 = 0` must link none of
z/zstd/crypto/ssl; `empty`/`hello` size benchmarks), then Slices 3/4/5/V per the roadmap M13
section. Earlier: fifth wave (**M12 COMPLETE — A5-SSE MERGED as #417**, the last M12
slice. `ctx.respond_stream(rb)` (header-only rb → abort, consumes both, auto TE:chunked for 1.1 /
close-delimited raw for 1.0 via a threaded version bool, single-sourced `http_serialize_head` with
`respond`) + Move `http_stream` (`send` = one length-driven chunk frame per write, `send("")` =
no-op, EPIPE → poison; `finish()` = the sole clean terminator `0\r\n\r\n`; Drop = close-only — the
settled amendment). Protocol-weighted adversarial gate SHIP zero defects (framing proven
length-driven by raw-socket decode — a payload that IS a fake `0\r\n\r\n` survives; hex lowercase;
poison/truncation/leak all confirmed). gemini's 3 mediums applied (target-independent hex buffer,
poison short-circuit placed after the empty-noop check to keep send("")=Ok, pre-sized send buffer).
Rebased past the day's docs commits (roadmap A8 record-and-close kept, A5→DONE). `cargo test
--workspace` **1813 green**. **M12 is now fully closed** (A4 #413 / A6 #414 / A7 #415 / A8 #416
below-gate / A5-SSE #417) — the align-LLM Phase-0→4 + gateway language prerequisites are all in
place. **Next: M13** (codegen quality & link hygiene — see below). Earlier: fourth wave (**the
roadmap beyond M12 is now PLANNED, 8a47cc0**: the
owner's out-of-repo GPT-5.6 optimization consultation was read in full by Fable, three claimed
gaps empirically confirmed (zero linkage settings in codegen; `emit-llvm` is pre-opt only; the
unconditional `-lz -lzstd -lcrypto -lssl …` link), and the whole thing triaged into an
open-questions adoption record (adopted / verify / consumer-gated / post-upgrade /
rejected-with-reasons). **New roadmap M13 = the pre-LLVM-upgrade codegen-quality wave**
(internalization + unnamed_addr; capability-based linking + runtime split; optimized-IR emission
+ remarks→Align translation; build profiles over stock `default<O*>`; internal ABI flatten +
argument attributes incl. broad `noundef`; verification bundle incl. the Cpu-feature test and
measured cold metadata). **The LLVM/inkwell upgrade checkpoint is sequenced AFTER M13** (M13's
shape/size/bench net is the upgrade gate); **ThinLTO → runtime bitcode → PGO → BOLT go after the
upgrade** (bitcode compat dictates the order). A8 closed earlier today as measured-below-gate
(#416, 1.06× < 1.15×; the 13.5× no-re-zero upper bound is the recorded follow-up). Owner
reaffirmed: pre-release breaking changes stay OK indefinitely. Earlier: third wave (**M12 Slice
A7 MERGED as #415** — streaming line reads:
`r.buffered()` (the read dual of the buffered writer; drain-before-fd interleaving contract) +
`r.read_line(mut buffer)` (body-with-`\r?\n`-stripped, consumed-incl-terminator, 0=EOF, 64 MiB
cap, split-CRLF-across-refill verified) + the generic validating view `bytes.as_str()`. Gate SHIP
zero soundness defects — the novel buffered-provenance set is over-strict on rebind/if-expr/
fn-return (recorded UX limitation) but NEVER unsound (runtime defensively upgrades). gemini's
null-guard medium rejected (the #413 class). `cargo test --workspace` **1799 green**.
**align-LLM Phase-2 language prerequisites are now COMPLETE** (read_line → as_str → json.decode →
array_builder). **A5-SSE design SETTLED** (3100742, http.md item 7 — send("")=no-op, Drop=
close-only amendment, finish() sole terminator, HTTP/1.0 close-delimited raw mode via threaded
version info) — implement next; then the A8 arena-checkpoint design round closes M12. Earlier:
second wave (**M12 Slice A6 MERGED as #414** — `array_builder<T>`,
the third grow-then-freeze member: annotation-inferred element type, Pure mut-receiver
`push`/`append`, consuming zero-copy `.build() -> array<T>` via the new `align_rt_realloc`;
elements v1 = Copy scalars + `string` (moves in, deep-freed on unfrozen drop — leak-guarded by a
cfg(test) live-counter after the gate's mutation survivor); recorded rejections enforced
fail-closed (Copy structs, string append, Move handles, str views — no arena-view laundering).
Gate SHIP zero defects (stride matrix all widths, 200k-iter double-free stress, ~40 probes);
gemini's 4 debug_assert mediums verified-true-then-applied. `cargo test --workspace` **1783
green**, clippy clean. The align-LLM Phase-2 accumulate side is ready; **next: A7 streaming
line reads** (design NOT yet settled — two-lens review first), then A8 arena checkpoint /
A5-SSE. Earlier: **M12 opened and Slice A4 MERGED as #413** — offset-addressed file
I/O: the `file` Move type, `fs.create_rw`/`open_rw` (CLOEXEC), `f.pread` (actual-count/EOF=0) /
`f.pwrite` (loops-to-full, sparse-verified past-EOF extension) / `f.len()` (live fstat); **no
seek** (hidden cursor) and **no read-only open** (reads stay reader|mmap) per the settled design;
negative offset aborts; `file` is a nameable surface type (the gate's one CONFIRMED finding —
the missing type-name arm — fixed pre-merge with threading tests). The align-LLM Phase-4
(alignpack relayout) enabler. gemini's null-guard medium rejected on the sibling-convention +
dead-guard grounds. `cargo test --workspace` **1762 green**, clippy clean. **M12 scoped in the
roadmap** (= runway remainder A4✓/A6/A7/A8/A5-SSE; A4+A6 designs settled by a two-lens review,
d10c627). **The owner's align-LLM spec is now v1.0 FINAL** (out-of-repo; runway record updated
4744a61 — A-list confirmed unchanged; watch item: flat-only `json.encode` vs nested gateway
payloads, deferred-until-consumer). **Next: A6 `array_builder<T>`** (settled design in roadmap
M12), then A7 streaming line reads / A8 arena checkpoint / A5-SSE. Earlier: 2026-07-10, sixth
wave (**#412 MERGED — std.http Slice 5, HTTPS/TLS client — and
M11 FORMALLY CLOSED**: `https://` works transparently through `cl.get/post/request/get_many` over
OpenSSL libssl; mandatory verification = system trust store + hostname binding (`SSL_set1_host` /
`set1_ip_asc` for IP literals, both mutation-pinned by negative tests incl. the gate-requested
wrong-DNS-host case); Denied/Code/Invalid taxonomy; `(scheme,host,port)` pool key with live `SSL*`
pooled; per-thread sigmask SIGPIPE. Security-weighted adversarial gate: SHIP, verification-bypass
hunt clean. gemini: its security-critical "double-close" claim was **empirically disproven**
(`SSL_set_fd` = BIO_NOCLOSE; `SSL_free` does NOT close the fd — the probe is now a permanent
regression test) and its two valid mediums applied (pooled-conn teardown moved outside the pool
Mutex). `cargo test --workspace` **1747 green**, clippy clean. **std.http COMPLETE, R1–R6 all met;
M11 closed in the roadmap + CLAUDE.md.** Known follow-up: ja/http.md needs the natural-JA
translation of the Slice-5 detail (status lines already synced). **Next: the align-LLM runway A4+**
(seek/pread, growable `array<T>`, streaming line reads, arena checkpoint — open-questions →
"align-LLM runway") **or M12 scoping.** Earlier same day, fifth wave (**#411 MERGED — `cl.get_many`
(R5) + the `array<response>`
opaque-Move-handle-array capability**; design settled same day by a second two-lens review
(record: http.md item 6, f04cb96) — input-order **all-or-Err** (per-slot Result is inexpressible:
`Result` is a `Ty`, never a `Scalar`), lowest-index error, run-to-completion, dedicated bounded
blocking-I/O claim-loop workers (the CPU-sized ParPool was proven the wrong shape for I/O overlap),
`rs[i]` borrow-in-receiver-position with per-element drop. Bench: **15.4× overlap** at degree 16
(12 ms injected latency, 32-core machine) + **1.01× of an equal-degree Rust pool** — **R6 now met
in full**. Salvage note: the implementer session died at its usage limit with everything
uncommitted; the orchestrator verified (1735 green), doc-finished, committed, and the gate ran as
usual (SHIP; its one test-gap finding — the batch-error free path survived mutation — was closed
with a cfg(test) live-response counter, mutation-verified). gemini: 1 valid duplicate-diagnostic
medium applied; 1 dead-null-guard high rejected (would have broken empty→Ok-empty). New
open-questions note: the pre-existing view-across-reassign dangle (Borrow-liveness entry).
**Slice 5 HTTPS/TLS is the LAST M11 item**, design settled in the same f04cb96 record. Earlier
same day, fourth wave — **#409 MERGED — std.http Slice 4, the server primitive**:
`http.serve`/`srv.accept`/`http.response(status)` → `response_builder`/`ctx.respond(rb)`
double-consume one-write; the surface was **settled the same day by a two-lens design review**
(record: http.md Signatures + slice-plan item 4, commit 3ee3ad7 + ja mirror 3524fde) after the
implementer correctly STOPped on the undesigned response surface. NEW `http_parse_request_head`
with the five inbound smuggling guards (+ gemini hardening: RFC-token header names, CR/NUL-clean
values, digits-only Content-Length, alloc-free status line); 3 new Move types full-twin-mirrored;
dogfood test = compiled Align client ↔ Align server over loopback; fd-leak-free proven via
/proc/self/fd; expr-depth 5/5 kept. v1 limits recorded: sequential accept loop
(Move-capture-into-spawn = the concurrency prerequisite), trusted-network caveat (no read
deadline → slow-loris), SSE = future sibling op `respond_stream` (runway A5 landing pinned).
The client-side Content-Length `+` hole flagged during that reflection is CLOSED too (**#410**
MERGED — digits-only pre-check in `http_parse_head`, mirror of the server guard; the one gemini
"critical" on it was a false positive — a neighboring same-theme test misread as a duplicate —
rejected with written proof). The ja http.md mirror is fully re-synced (5bf98e4: the stale R6
status + shipped Slice-4 state). `cargo test --workspace` **1720 green**, clippy clean.
Next std.http: Slice 5 (HTTPS/TLS) + `get_many` (R5). Earlier same
day, third wave — **#408 MERGED — `Ord(str)` + `else` on `Result`**, the last
2026-07-09 owed implementation delta: byte-lexicographic `<`/`<=`/`>`/`>=` on `str` via
`align_rt_str_cmp` + `sort_by_key` str keys (owned-`string` ordering and bare `sort()` on str arrays
stay deferred per the record); `else` accepts `Result<T,E>` — shared `ElseUnwrap` node so every
structural pass covers it by construction; Move **error** payload `else` rejected-with-diagnostic
(discard would leak; every builtin `Error` is Copy so the common case is whole). Adversarial gate
SHIP zero findings; gemini's two "won't compile" highs disproven-by-green-build and rejected with
written reasons. `cargo test --workspace` **1701 green**, clippy clean. **std.http Slice 4 (server
primitive) implementation is IN FLIGHT** on branch `http-slice4-server`. Earlier same day, second
wave — **soundness-triage wave DONE**: another session's three
open PRs triaged — **#405** MERGED (bare array literal as generic arg: ICE → sema diagnostic),
**#406** MERGED (closure capturing an arena view escapes → rejected; `Ty::Fn` now `tracks_region`
+ capture-region fold; retires the #399-gate UAF record), **#404** CLOSED-superseded by **#407**
MERGED (a `mut` binding's region is now **fixed at initialization** — region-changing reassign of
an owned Move local = sema error, view regions intersect; kills a confirmed double-free AND a
confirmed view-UAF on main; #404's intersect-everything fix traded the UB for an unbounded leak and
was rejected). `cargo test --workspace` **1687 green**, clippy clean. New Open items recorded:
wrapper-hidden local-slice escape through a fn return (`return Ok(xs[..])`, pre-existing UAF), and
the if-expression mixed-region leak (pre-existing, flow-sensitive-slice class). Root stray files
triaged + deleted. Earlier same day: **#402** the `loop` expression MERGED (full slice, lexer→MIR);
**#401** runway A2 binary decode/encode; **#400** lexer escape set (other session); **#399** runway
A1 `fs.read_bytes_view`; **#398** std.http Slice 3 keepalive pool + R6 bench; **#396**/**#397**
owed-delta wave; staleness sweep #395)._

## ▶ NEXT SESSION — start here

**Repo state (re-verified 2026-07-10):** `main` clean, no open PRs. Newest commit is the
**docs-only design wave committed directly to main on 2026-07-09** (see the three 2026-07-09
paragraphs below: `loop` settled, spec-vacuum sweep + 7 settlements total, align-LLM runway).
Two code merges landed between #392 and that wave, easy to miss: **#393** (editor support —
`editors/` Vim/Emacs/VS Code syntax + snippets, `.ctags`, `.vscode/tasks.json`,
`examples/playground.align`; the stray `.playground.align.swp` it committed was removed in the
docs-wave commit, blob stays in history) and **#394** (one-line codegen fix — the BufferBytes
alloca moved to the entry block, stopping a per-iteration stack overflow). `cargo test
--workspace` re-verified **1601 green** on 2026-07-10 (post-#394); clippy clean at `-D
warnings`. Housekeeping note: **8** stale local branches remain (`fix-*`/`*group-agg*` from the
squash-merged Gemini wave #358–#368; `handoff-http-slice1-inflight`, a pre-#391 stash; and
`fix-codegen-bufferbytes-alloca`, merged as #394) — squash merges hide them from `--merged`;
verify content landed (`git diff main...<br>`) then force-delete at leisure. Two stray untracked
files at root (`examples/double_free.align`, `wait_for_review.sh`) were left untouched — not this
session's work; triage or gitignore at leisure. **DONE 2026-07-10 (this wave): the two priority
owed deltas.** **#396** — struct/tuple/array/sum/owned-`string` `==` no longer ICEs in codegen;
sema enforces a fail-closed allow-list (equality = numeric+bool+char+str via
`Bound::Eq.satisfied_by`; ordering = numeric+char; owned-`string` compare deferred with a clear
message; `bool <` now rejected per spec; the generic `T: Eq`-instantiated-with-struct bypass
verified closed at two layers + regression-tested; the bound diagnostic's `struct#N` leak fixed
via `ty_display`). **#397** — no-shadowing enforced: `check_shadow` at all 5 user-named binding
sites (coverage proven complete — every local goes through `declare()`); sibling scopes/arms/
lambdas stay legal for free (`scope` is the live chain); module constants included; zero corpus
call sites needed fixing; single-level-capture coupling invariant documented in a comment, and
"locals may share a top-level fn name" recorded as a new Open item. Both adversarially gate-
reviewed (zero CONFIRMED findings), mutation-checked, gemini reflected (#396 zero findings; #397's
one medium — eager lookups in `check_shadow` — verified and applied). `cargo test --workspace`
**1628 green**; clippy clean at `-D warnings`. **std.http Slice 3 is now MERGED as #398**
(keepalive pool + R6 bench, both gates met — see the std.http Slice 3 paragraph below);
`cargo test --workspace` **1637 green** post-#398. **Next (updated after the 2026-07-10 waves —
the `loop` slice, lexer escapes, and runway A1/A2 are all DONE), pick one:**
(a) **std.http Slice 4** (the server primitive `serve`/`accept`, caller writes the response), then
Slice 5 (HTTPS/TLS) and `get_many` (R5) — the standing M11 plan, and Slice 4 doubles as the
align-LLM runway A5 substrate (http server+SSE); or (b) the last
**2026-07-09 owed implementation delta**: `Ord(str)` + `else`-on-`Result` implementation, then the
align-LLM runway A4+ (seek/pread, growable `array<T>`, streaming line reads, arena checkpoint).

**Design settled 2026-07-09: the `loop` expression** (docs-only, no code). One narrow sequential-control construct — `loop { ... break value }`, an expression; no `for`/`while`/`continue`/labels; recursion is explicitly not iteration (the spec now guarantees no TCO — scope-end drops and `?` kill tail position). The pipeline owns the data path; `loop` owns the control path. Updated: `draft.md` §4 "Loop" + §7, `language-spec.md`, `design-notes.md` → "The loop philosophy", `history.md`, `open-questions.md` (Settled → "Sequential control"), guide ch00/02/06/13/17, little-aligner ch11 **rewritten** as `11-do-it-until.md` (it taught recursion-as-iteration and overclaimed TCO), + `ja/` mirrors. Implementation is an unscheduled future slice (lexer/parser `loop`/`break`, break-type unification like match arms, per-iteration drops, block-value escape rule); the deferred M8 frequency lints gain their firing surface when it lands.

**Spec-vacuum sweep (2026-07-09, same session, docs-only):** a two-track audit closed the "guide
teaches what the spec never states" class. **Five settlements** written into the spec:
`print`/display contract, literals+escapes (single-line strings, `\u{...}`, unknown = error),
**`==` = scalars+strings only** (no structural equality), **no shadowing**, **floats = IEEE,
never abort** — full record in `open-questions.md` Settled → "Spec-vacuum sweep". The remainder
is recorded as Open → "Unrecorded spec vacuums — remainder" (assert, str char access, precedence
table, stack-overflow contract, main signature set, reserved words + ASCII-only-identifier lean,
C→Align: embedding = non-goal / callbacks deferred-with-trigger). **The two priority
implementation deltas are DONE (2026-07-10):** (1) struct `==` ICE → sema diagnostic shipped as
**#396**; (2) shadowing → compile error shipped as **#397** (details in the NEXT SESSION block
above). Still owed: the `loop` implementation slice itself, and the lexer escape-set gaps
(`\r \0 \u{}`, unknown-escape error, single-line enforcement). **Two owner-directed follow-up
settlements (same day):** `Ord(str)` — byte-lexicographic string comparison + `sort_by_key` string
keys — and **`else` on `Result`** (deliberate error-discarding fallback; overturns guide ch04's
old "Option-only" doctrine, guide rewritten en+ja). Record: `open-questions.md` Settled →
"`Ord(str)` + `else` on `Result`". Both unimplemented (sema + runtime compare; sema else-on-Result).

**align-LLM runway recorded (2026-07-09, docs-only — owner deferred all implementation on token
budget):** the owner's align-LLM inference-engine spec (v0.4, out-of-repo) is the planned killer
app; working backward from it, `open-questions.md` Open → "align-LLM runway" now records the M12
std-wave candidates (A1–A8: `fs.read_bytes_view` binary mmap, bytes/buffer binary decode+encode,
the `loop` implementation slice, seek/pread, http server+SSE slice, growable `array<T>`,
streaming line reads, arena checkpoint — the last three are general fast-systems needs, not
engine-specific), two design stances (async = task_group + blocking workers, NO async/await;
shared state lean = channels, atomics sealed), and two explicit non-adoption boundaries (no
MemoryTier/pinned/VRAM in the language core — std/pkg Move handles instead; f16/bf16 stays
Future). **A1 is DONE (merged as #399, 2026-07-10):** `fs.read_bytes_view(path) ->
Result<bytes, Error>` — arena-scoped binary mmap view, no UTF-8 validation; runtime shares
`fs_read_view_impl` with `read_file_view`. The compiler substance: the **first arena-backed
slice view** — new `Scalar::Slice(PrimScalar)` (twin-mirror swept: Copy, not on move/drop,
excluded from `==`/print/fn-values/box/array-literals) and the `region_bearing` predicate
(`tracks_region || ty_mentions_slice`, transparent through Result/Option/tuple **and struct
fields + array elements** — the gemini defensive hardening, both verified unreachable today)
replacing the `tracks_region` gate at every EscapeCheck site. Adversarial gate: ~25 escape-bypass
programs all rejected, no new hole; the one CONFIRMED finding is the **pre-existing**
closure-captured-arena-view UAF (also reproduces on M9 `read_file_view`), recorded in
open-questions under the escaping-fn-values deferral. `cargo test --workspace` **1647 green**.
**A2 is DONE (merged as #401, 2026-07-10):** binary codec — decode `bytes.u8/i8(off)` +
`{u16..u64,i16..i64,f32,f64}_{le,be}(off)` (18 offset-explicit Copy-scalar reads, **inline**
codegen: align-1 load + `llvm.bswap` for `_be` + float bit-cast — no FFI barrier in descriptor
loops); encode `buffer.put_*(v)` (same 18-name set) + `buffer.append(bytes/str)` on a `mut`
bound buffer (the deferred M9 buffer op set landing with its designed consumer). Settled and
recorded (draft.md §12 + open-questions): endianness always explicit (no hidden default);
out-of-range **aborts** like `slice[i]` (the `off+width` i64-overflow case provably caught by
the signed `start>end` arm); **copy-out/owned-bytes not needed, deferred**. Adversarial gate:
zero defects (par_map race structurally impossible — Move buffers can't be captured); gemini's
2 mediums (defensive bswap width guard, bulk BE append) applied. `cargo test --workspace`
**1661 green**. **A3 — the `loop` expression — is DONE (merged as #402, 2026-07-10):** full
ideal-form slice (lexer/parser/fmt/sema/MIR): `loop { ... break v }` expression, break unification
reusing the match running-unify, break-less loop diverges (exit = Unreachable), `for`/`while`/
`continue` get clear diagnostics; **two-pass fixpoint loop-back MoveCheck** (2nd-iteration
use-after-move caught; two passes proven sufficient by the adversarial gate); **per-iteration
drops** via sema-recorded loop-body LocalId ranges intersected with `drop_locals` (the gemini
review caught the original HIR-walk collector's fail-open wildcard leaking nested owned locals —
reproduced, redesigned, regression-tested); break value must be `Static` (conservative v1;
enclosing-arena views need `.clone()`); break from an arena/task_group nested in the loop =
rejected-with-diagnostic (region-unwind-on-break is a recorded future slice). Review side
discoveries recorded in open-questions: value-position array literals panic in lower_expr
(if/match arms, pre-existing), value-position block exprs miscompile (pre-existing).
`cargo test --workspace` **1680 green**. **A4+ (seek/pread, http server+SSE, growable array,
streaming line reads, arena checkpoint) are not started.**

**M11 is IN PROGRESS — `std.net` (#371–#374), `std.process` (#376–#378), `std.compress`
(#380–#381), and `std.crypto` (#383–#388) are DONE.** Full shipped-feature summaries + per-slice
decisions + deferral lists live in the roadmap's **M11 section** (`docs/impl/07-roadmap.md`) —
that is the record; don't duplicate it here. crypto headlines: engine = OpenSSL libcrypto (EVP,
floor ≥ 3.2, `-lcrypto` always-linked; settled by two independent design reviews, #383);
`constant_time_equal` branchless-**verified by disassembly** of both shipped profiles; AEAD open
is all-or-nothing with `OPENSSL_cleanse` confirmed live in the optimized artifact; all KATs
canonical (NIST / RFC 4231 / RFC 5869 / RFC 8439 / phc-winner-argon2); **`argon2_params` is the
language's first builtin struct** (reserved-name injection like `Error`); blake3 deferred (no
system engine). Two standing engineering conventions came out of this module: (1) recursive
compiler fns must not gain match arms with inline locals — `#[inline(never)]` free helpers + boxed
wide `Rvalue` payloads (the Slice-3 frame regression, root-caused and measured); (2) the
dangling-ptr+len-0 FFI convention is deliberate and formally defended (#387 gemini rejection).
The expr-depth cap (128) vs full-pipeline stack ceiling (~40) gap is recorded as a new Open item.

**`std.http` — the LAST M11 module — Slice 1 DONE (merged as #391).** Request/
response Move types + HTTP/1.1 serialize/parse, NO sockets. Full details + the eight key decisions
in the roadmap's std.http entry. Headlines: two Move types (`Ty::HttpRequest`/`HttpResponse` +
`Scalar::HttpResponse`, full twin-mirror sweep); `http.request`/`r.header`/`r.body`/`http.parse`/
`resp.{status,header,body}` exposed (Pure — sockets are Slice 2); **URL validation deferred to
serialize** (runtime URLs never abort the builder); **`http.parse` exposed** as the response
constructor+codec (Slice 2's client reuses the engine) while **serialize stays a runtime-internal
codec** (`align_rt_http_serialize`, unit-tested, one-buffer R4); P6 CR/LF/NUL header injection
**aborts**; auto Host+Content-Length, caller dup rejected; chunked → `Error.Invalid` (Content-Length
only, v1); caps 128 headers / 1 GiB; R1 zero-copy offset table, `memchr`-backed scan (R2). Tests:
`m11_http.rs` (14 driver) + 7 `align_runtime` units. **NOTE the `lower_expr` frame lesson bit again:**
adding 7 http arms tipped the default-env expr_depth ceiling (baseline 5/5 → 4/5 overflow at the
40-term full-pipeline chain); fixed by collapsing all 7 http ops into ONE `lower_expr` arm delegating
to a single `#[inline(never)] lower_http(b, e)` dispatcher — back to 5/5. **Adversarial review then
found + fixed 3 issues on-branch:** (1) a CONFIRMED use-after-free — `resp.header()`'s `Option<str>`
view escaped when unwrapped through a `match` arm (`Some(v) => v`), because `EscapeCheck` never
carried the scrutinee's region into arm-payload bindings (the codebase's first `Option<borrowed
view>`). Fixed the **general** gap: a `match`-arm binding now inherits the scrutinee's non-Static
region (like `LetTuple`), closing it for every future `Option<view>`/`Result<view>` — no regression
across cli/net/crypto view-escape suites. (2) serialize now validates method = RFC 7230 token +
authority/path have no CR/LF/NUL/SP (permanent-codec smuggling guard). (3) parse rejects a
conflicting duplicate Content-Length (RFC 7230 §3.3.3). `cargo test --workspace` 1584 green,
expr_depth 5/5 default env, clippy clean.

**`std.http` — Slice 3 DONE (merged as #398: keepalive connection pool + R6 benchmark).**
`Ty::HttpClient` goes from a ZST to a real Move type owning a keepalive pool
(`Mutex<HashMap<(host,port), Vec<IdleConn>>>`) — and this was a **pure runtime change**: the compiler
already treats `HttpClient` as an opaque handle pointer (codegen emits a pointer; Drop already calls
`align_rt_http_client_free`), so ZST→state is invisible to sema/MIR/codegen (the Slice-2 "ZST behind
the same FFI" design paid off — zero compiler edits). **R3 reuse by default:** consecutive
`get`/`post`/`request` to the same `(host,port)` reuse a live idle conn, zero opt-in, surface + FFI
unchanged. **Reuse verdict (correctness-critical — a dirty conn reused misframes the next response):**
pool a finished conn IFF keep-alive (HTTP/1.1 default; `Connection: close`/non-1.1 → no, via
`http_head_keep_alive`) AND Content-Length-framed (read-to-close → no) AND no leftover bytes beyond the
framed message. **Stale-conn retry:** a reused idle conn the server dropped fails before any response
byte → retried once on a fresh conn (idle-close race, request never processed); a fresh conn's failure
or a mid-response failure surfaces directly. **SIGPIPE:** client writes use `send(MSG_NOSIGNAL)`
(Linux) / `SO_NOSIGPIPE` (macOS) so writing a dropped conn returns `EPIPE` (→ retry), never kills the
process (no global handler). **Drop closes all pooled conns (P5).** **Bounds:** ≤ 8 idle/host;
idle-expiry reaps conns idle > 90 s on take. **I/O timeouts stay deferred** (ideal-form call: connect
timeout belongs to the net-rail non-blocking substrate; read timeout has no v1 config surface without
expanding frozen signatures — the pool's idle-expiry ≠ an I/O deadline; recorded in http.md "Known v1
limitations"). **R6 MET:** `bench/http_client/` (drives the shipped pool's C-ABI vs a plain-Rust
`std::net` baseline over a localhost server) records **2.86× keepalive speedup** (floor 1.48×) and
**parity with hand-written Rust** on the reuse path. Tests: `align_runtime` units (reuse across 3 gets;
`Connection: close` not pooled; stale-conn retry; `http_head_keep_alive` table) + a driver test (2 gets
reuse 1 conn, via the server's accept count). **Review wave (all reflected pre-merge):** the
adversarial gate CONFIRMED one defect — the stale-conn retry re-entered the pool (could grab a
second dead conn) instead of forcing a fresh connect — fixed + regression-tested; it also flagged
the bare leftover-bytes check, now mutation-verified by a dedicated test. gemini's 3 findings all
verified valid and applied: pool only after `http_parse_core` succeeds (poisoning guard; the one
reachable divergence was a >1 GiB body), `put_idle` reaps stale conns before the capacity check,
`take_idle` removes emptied buckets. `cargo test --workspace` **1637 green**; clippy `-D warnings`
clean. **Next: Slice 4** (server primitive `serve`/`accept`), then Slice 5 (HTTPS/TLS). `get_many`
(R5) also remains.

**`std.http` — Slice 2 DONE (merged as #392).**
The plaintext HTTP/1.1 client: one new Move type `Ty::HttpClient` (a ZST in v1 — no `Scalar`, never
rides an aggregate; full twin-mirror Gate-1 sweep). Surface behind `import std.http`, all **Impure**:
`http.client()`, `cl.get(url)` / `cl.post(url, body)` / `cl.request(req)` → `Result<response, Error>`
(bound-receiver gate; `cl` borrowed, `request` **consumes** its Move `req`, MIR nulls the slot). Each
request = ONE fresh net `tcp_conn` (connect via `align_rt_tcp_connect` → **TCP_NODELAY** → **one
write** of the Slice-1-serialized request → stream the response in 32 KiB reads to Content-Length →
`http_parse_core` → close); NO pool yet (Slice 3), but the FFI already takes `*mut HttpClient` so
Slice 3 adds keepalive behind the same surface. **R4 shipped, not aspirational.** P1: `https://` /
malformed URL → `Error.Invalid` at request time (never a silent downgrade). P2: 4xx/5xx = `Ok`. The
Slice-1 parser was refactored to an `Incomplete`/`Invalid` split so the streaming read distinguishes
"read more" from "malformed" over ONE shared decoder (`http_parse_head` frames without copying the
body). `http.client` is a **ZST** — the honest "no state yet" form (not a disabled half-pool);
Box round-trip sound, Drop a null-safe no-op until Slice 3 (P5). Reused `lower_http` dispatcher (no
new `lower_expr` frame arms — the #296 lesson). Tests: `m11_http.rs` +10 driver (get/post round-trips
vs an in-process server, 404-is-Ok, https/malformed error, request-consumes-req, unbound/array/import
gates, client-path body-view escape) + 7 `align_runtime` units. `cargo test --workspace` **1601 green**,
expr_depth **5/5 default env**, clippy `-D warnings` clean. **Next: Slice 3** (connection pool /
keepalive reuse — R3; the measured 1.48× floor). **The owner explicitly wants http FAST** — http.md's
R1–R6 are requirements; R6 benchmark-gating (`bench/http_client` vs a Rust baseline) still owed.
Slices 4–5 (server primitive, HTTPS/TLS) after.

**std.http Slice 2 process note (2026-07-08):** the slice-flow ran clean again. Adversarial gate:
**no code defects** (Gate-1 twin-mirror verified complete incl. the correct omissions; fd-close
proven on every error path); its three findings were docs-only and recorded on-branch — (1) no
read/connect timeout (inherited from the net rail's documented no-timeout behavior; now an explicit
"Known v1 limitations" section in http.md, follow-up tied to Slice 3), (2) `https://` maps to the
bare message-less `Error.Invalid` (P1 security intent met, "clear message" recorded as a v1 limit),
(3) R6 not yet satisfied — made explicit it gates *module* completion, bench lands with Slice 3.
gemini on #392: 1 high + 2 mediums; the high (`_request` leaked its moved-in `req` on the
defensive `out`-null early return) and one medium (multi-colon unbracketed authority parsed as
garbage host instead of rejecting — also closed the adversarial pass's bare-`::1` note) were
verified real and APPLIED; the other medium (`conn.is_null()` after `tcp_connect` returns 0) was
REJECTED with a written PR reason — the invariant (0 returned only after `*out = Box::into_raw`)
re-verified at the source, same provably-dead-guard class as the #364/#387 rejections.

**std.crypto process note (2026-07-07):** the slice-flow ran five more times, clean. Adversarial
gates: zero findings on all five slices (they also machine-code-verified the CT and cleanse
properties). gemini: #384/#386/#388 zero findings; #385's three "may not compile" highs and
#387's three dangling-ptr "UB" mediums were each verified against the code and **rejected with
written reasons** (PR comments) — the reflect-before-merge rule includes rejecting wrong findings,
not just applying right ones. One gemini review errored and was re-triggered with
`/gemini review` (#387). The Slice-3 stack regression was caught by the orchestrator's default-env
re-verify (the implementer had masked it with RUST_MIN_STACK — rejected), root-caused to
recursive-frame inflation, fixed, and measured back to exact parity with main.

**Slice-flow that worked (keep it):** deep-reasoner implements in an isolated worktree (one
slice per PR; tell it explicitly to never touch the shared checkout) → orchestrator re-verifies
test total + clippy on the branch (`cargo build` first — the driver link tests fail on a stale
runtime staticlib, and piping test output through `tail`/`awk` eats the exit code, so read the
totals) → an INDEPENDENT deep-reasoner adversarial gate review (skills/align-self-review Gates
1–4/6; twin-mirror diffing for sibling types) → fix/record findings → PR → gemini review
reflected (verify each finding against the code — #372's "high" was a false positive disproven
by an actual `alignc check` run; #376/#378's findings were real and applied) → squash-merge.
std.net slices 3/4 and std.process slice 3 came back zero-or-low-finding from the adversarial pass; the
process-slice-2 implementer self-caught a real double-reap hole via the Gate-1 sweep.

**Gemini fix wave #358–#368 audited (2026-07-06):** PRs #358–#368 were authored by **Gemini CLI**
(not Claude): runtime memory-safety hardening (`safe_len`/`safe_slice` FFI-boundary guards,
`checked_mul` size math, null checks, group_agg/group_io overflow fixes, buffer huge-capacity
panic), group_agg hot-loop optimization (`get_unchecked` + `HashMap::entry`), LLVM vector
reductions via `llvm.vector.reduce.*` (NaN-correct `fminimum`/`fmaximum`; strict-ordered `fadd`),
undef→poison, diagnostics stdout→stderr. A two-lens adversarial re-review (runtime soundness /
semantics+regression) found the work **SOUND**: every `get_unchecked` invariant holds, no
abort→silent-return policy downgrade (the "safe returns" only replaced previously-*unsafe* silent
`as usize` wraps), reductions semantics-preserving, tests 1349 green. Of Gemini's 30 inline review
findings, 26 were addressed within the wave; the 4 unaddressed (#364, same-pattern
`get_unchecked`/`get_unchecked_mut` double-index micro-perf nits) are **consciously rejected** —
LLVM folds the duplicate index; not worth the churn. Cleanup after the audit: **#369** removed an
8 MB compiled binary (`unsafe_raw`) + throwaway script accidentally committed by #365 and added
root-scratch `.gitignore` rules (`/scratch*`, `/test_*.sh`, `/*.o`; the blob stays in git history —
reclaiming it needs a history rewrite, owner's call); **#370** fixed the wave's clippy regressions
(re-attached the orphaned `# Safety` doc on `align_rt_str_clone`, removed redundant nested `unsafe`,
two pre-existing lints) and restored the deleted sema rollback-soundness comment — **clippy clean
again at `-D warnings`**. Process note: #364/#366 were merged before/seconds after the gemini
review landed (the reflect-before-merge rule lapsed for the Gemini-driven PRs); the findings were
recovered post-hoc this time, but hold the gate for future Gemini-authored PRs too.

**M10 is COMPLETE and formally closed** (Slice 1 `std.encoding` #346, Slice 2 `std.rand` #347,
Slice 3 `std.cli` #356 — see the roadmap's M10 section for the shipped-feature summary).
`cargo test --workspace` ≈ **1349 green**.

**std.cli slice 3 notes (2026-07-04, #356):** two new Move types `Ty::CliCommand`/`Ty::CliParsed` +
`Scalar::CliParsed` payload; `parse` borrows the command (usage callable after `Err`); `get_*`
total-or-abort (the #345 policy); `get_str` = Frame-capped view (#297 arm). Review-driven
refinement: cli method names live as **type-guarded arms in the tail method match** (`trim`/
`map_err` shape) — same-named methods on other types fall through to normal resolution (the rng
eager-error pre-block from #347 predates this and was left as-is; consider aligning it if it ever
matters). Deferred behind the same signatures: richer argv (`-x` clustering, `--`, positionals),
struct-decode (waits for derive).

**External design-note import COMPLETE (#355 + earlier same-day adoptions):** the out-of-repo memo
`~/prj/std_simd_design_notes.md` is now fully digested into `docs/open-questions.md` ("external
design-note review adoption" entries); the memo file is no longer load-bearing and may be discarded.

**Docs session 2026-07-04 (PRs #352, #353):** the tutorial `docs/guide/` was rewritten from 6 thin
chapters into a **full 18-chapter book** (The Book style; every example compiled+run against
`alignc`; unimplemented surfaces marked "implementation in progress"), and a second learning
track **`docs/little-aligner/`** was added — a *Little Schemer*-style Q&A drill book (11 chapters
+ Commandments) drilling pipelines/match/Option-Result/Move-arena/SoA/group_by/recursion. Both
bilingual (EN original + `ja/` mirror, deep-reasoner natural-JA translation); `docs/impl/
std-design/ja/` and `README.ja.md` retranslated to natural Japanese; English-side cross-links now
say "Japanese" (not 日本語). Implementation-behavior facts discovered while verifying examples
(worth knowing when writing docs/tests): `else`-unwrap is **Option-only** in the implementation (the spec now says Result too — settled 2026-07-09, unimplemented);
`to_soa()` requires an enclosing `arena {}`; string literals are single-line; `group_by(...).agg`
/`dict_encode` need a *dynamic* `array<Struct>` source (fixed-size literal arrays are rejected);
generic fn over generic struct (`fn f<T>(p: Pair<T>)`) not supported yet; `Result<buffer,_>`
can't be bound to a local (match it directly).

**PR #353 (same session):** `docs/impl/core-design/` — core library surface docs at std-design
depth (README + option-result / array-slice-pipeline / string / json / soa-groupby / vec-mask /
arena-heap / hash, EN + `ja/`). Records verified-implemented surface incl. `hash64`/`hash128`,
str `find`/`rfind`/`eq_ignore_ascii_case`, string range slicing, scalar math methods — and the
not-implemented set (`split`/`find_any`, `bitset`, `json.scan`/`validate`/`token`/`field_table`,
non-scalar box payloads). draft.md §18.1 points there for status; CLAUDE.md docs map + bilingual
rule updated; guide ch07 string-method list corrected (EN+JA).

**What landed the previous session (all merged):**
- M10 Slice 1 `std.encoding` (#346) + Slice 2 `std.rand` (#347) — the last implementation work.
- **Full std design specs** `docs/impl/std-design/{cli,net,http,process,compress,crypto}.md` (#348) —
  Opus-implementable depth; **these are the source of truth for implementing each std module.**
  Notable settled decisions inside: `process.exit` runs cleanup-then-exit (`process.abort()` is the
  hard-exit escape hatch); `child` Drop reaps via blocking `waitpid` (NOT `SA_NOCLDWAIT` — it breaks
  `wait()` with `ECHILD`); net's `get_many` lives in `std.http` (net→http would be a layering
  violation); http v1 is plaintext-only (HTTPS deferred, `https://` rejected not silently downgraded);
  crypto borrows a constant-time-audited engine, with `constant_time_equal` as the only
  self-hosted primitive.
- **Hands-on tutorial** `docs/guide/` 00–05 (#349) + **bilingual mirrors** `docs/guide/ja/` and
  `docs/impl/std-design/ja/` + `README.ja.md`, cross-linked (#350).
- **README streamlined + CLAUDE.md refreshed** (#350): the language rule now allows **bilingual
  user-facing docs** (guide + std-design keep an English original + a `ja/` mirror; core spec, code,
  identifiers, diagnostics, commits stay English — English is authoritative, `ja/` must not drift).
  CLAUDE.md's redundant M0–M4 blow-by-blow narrative was removed (roadmap is the milestone truth).

**Out-of-repo (NOT in git, on this machine only):** two Japanese learning docs the user asked for,
at `/home/hiro/prj/learning/` — `alignc-compiler-guide.md` (how this compiler is built, with general
compiler theory) and `rust-primer.md` (Rust basics oriented toward reading alignc). These are the
user's personal study material; they do not travel with `git clone`.

## M10 — std (encoding / rand / cli) — DONE (2026-07-04, formally closed)

**Slice 3 — std.cli — DONE (#356).** See "▶ NEXT SESSION" above for the slice notes; full record in
the roadmap's M10 section.

**Slice 2 — std.rand — DONE.** Copy `rng` ([`Ty::Rng`] = Xoshiro256++ `[4 x i64]`, value not Move —
never on the move/drop/escape path); `rand.seed()` (OS `getrandom`, abort on the rare failure) /
`rand.seed_with(s)` (SplitMix64 deterministic); `r.next()`/`r.range(lo,hi)` (Lemire, `lo>=hi` aborts)
/`r.shuffle(out xs)` (in-place Fisher-Yates) /`r.sample(xs,k)` (partial Fisher-Yates → owned
`array<T>`, `k<0`/`k>len` aborts) take a **mut** receiver. All rand nodes Impure (excluded from
`par_map`). New `Ty::Rng` swept Copy/`Static` through every pass; HIR `Rand*` + MIR `Rvalue`s +
`align_rt_rng_*`; `tests/m10_rand.rs` (12) + runtime units (5). Only Slice 3 (std.cli) remains.


**Slice 1 — std.encoding — DONE.** `encoding.base64_encode`/`base64_decode`/`base64url_encode`/
`base64url_decode`/`hex_encode`/`hex_decode`/`utf8_valid`: encode (byte view) -> owned `string`,
decode (`str`) -> `Result<buffer, Error>` (invalid -> `Error.Invalid`), `utf8_valid(bytes)` -> `bool`
reusing the #344 validator. New `Scalar::Buffer` owned-Move payload (reader/writer precedent) carries
the decoded `buffer`; scalar reference impl (SIMD later, same signatures). sema dispatch + MIR
`Encoding*`/`Utf8Valid` + `align_rt_*` runtime; `tests/m10_encoding.rs` (11) + runtime units (7).



**M10 std-2 design settled: `std.encoding` / `std.rand` / `std.cli`** — all three close over
existing mechanisms (`str`/`bytes`/`buffer`, `mut` slice, `main(args: array<str>)`'s `array<str>`)
with zero new Move types, zero new effects, and no FFI engine; `rand.seed`'s OS-getrandom call is
the only new runtime primitive. Full signatures in `draft.md` §18.2; slice breakdown + completion
conditions in `docs/impl/07-roadmap.md` M10; scope rationale in `docs/open-questions.md` Settled →
"M10 scope decision". **`std.net`/`std.http`/`std.process`/`std.compress`/`std.crypto` → explicitly
M11+** (each needs a new Move type, an FFI engine, or an unsettled design question — recorded per-
module in the roadmap's M10 deferral list); `process.exit`'s Drop/arena-cleanup semantics is a new
Open item to settle when `std.process` is designed. Implementation (M10 Slices 1–3) has not started.

## M9 — std (I/O, filesystem, path, env, time) — formally closed (2026-07-04)

**M0–M9 COMPLETE** (language core + tooling/FFI + std phase 1). All four M9 slices are done and
their completion conditions independently met (design settled #336; shipped #337–#340): **`std.io`**
core (`reader`/`writer`/`buffer` Move types + a shared errno→`Error` table), **`io.copy`** (a
non-consuming portable fixed-buffer transfer, `Result<i64, Error>`), **`std.fs`** complete
(`write_file`/`exists`/`remove`/`read_dir` plus the arena-scoped `mmap` view `read_file_view`), and
**`std.path`/`std.env`/`std.time`** (`path.join`/`normalize`/`base`/`dir`/`ext` views, `env.get`/
`env.set`, `time.now`/`instant`/`sleep`). See the M9 section of `docs/impl/07-roadmap.md` for the
full shipped-feature summary. **Not blockers, deferred as post-M9 backlog** (own labeled subsection
right after M9 in the roadmap): `io.copy` syscall fast paths (`sendfile`/`splice`/`io_uring`), no
`SIGBUS` handler on post-`mmap` truncation, non-UTF-8 filenames from `read_dir` (a raw-byte caveat
for `string` ops that assume UTF-8), dropping unbound Move temporaries (a one-shot
`io.stdout.write(x)?` leaks its writer handle today), streaming×pipeline integration, and the M10+
module set (`std.net`/`std.http`/`std.cli`/`std.process`/`std.encoding`/`std.compress`/`std.rand`/
`std.crypto`).

## M8 — Tooling and Quality — formally closed (2026-07-03)

All four completion conditions are met: the formatter (#233, `align_fmt`); `unsafe`/`raw.*`
(#262–264); `extern "C"` FFI v1 (#265–269) plus by-value struct passing shipped beyond the v1
boundary (x86-64 SysV only, #329); and the lint suite's full **profile-independent** slice —
unhandled-`Result` (#138), huge-struct-copy (#234), lossy-cast + wasteful-default-element (#313),
unnecessary-heap narrow form (#323) — five lints that never need runtime/profile evidence. See the
M8 section of the roadmap for the full shipped-feature summary. **Not blockers, deferred as post-M8
backlog** (own labeled section right after M8 in the roadmap, and `docs/open-questions.md` → "M8
lint candidates"): the frequency-dependent lints (allocation-in-loop, the broader
unnecessary-clone/unnecessary-heap forms, branch-in-hot-loop, string re-scan, implicit copy),
`prefer-pipeline-over-vecN` (no firing surface — no loop construct exists yet), and the hot/cold
field-split suggestion (heuristic design needed).

## M6 — SIMD / vec / mask — formally closed (2026-07-03)

Both completion conditions in `docs/impl/07-roadmap.md` are met and re-verified: `emit-llvm` on a
`vecN<T>` program shows real `<N x T>` IR, and `where(p).<reducer>()` is branch-free for every
reducer (`sum`/`count`/`min`/`max`/`any`/`all`/`reduce`), same as PR #303 established. See the M6
section of the roadmap for the full shipped-feature summary. **Not blockers, deferred as post-M6
backlog** (own labeled section right after M6 in the roadmap, and `docs/open-questions.md`): owned
SoA columns, `soa_slice<T>`, packed-bool columns, dynamic/arena over-aligned arrays.

## Internal review (2026-07-02)

A same-day multi-agent internal review (4 parallel deep-dive tracks — frontend soundness / MIR+LLVM
codegen / runtime+library / language-design evaluation — plus the design-evaluation question put to
Opus and Codex **independently**, which converged on the same conclusions). Distinct from, and no
overlap with, the external soundness audit below. Findings recorded in `docs/open-questions.md` under
"2026-07-02 internal review" (Open, near the external-audit section). **All bug findings were fixed
the same day, PRs #293–#297**: the `AssignField` MoveCheck gap + `arena_alloc` raw cast (#293), the
division-by-zero/`INT_MIN÷-1` LLVM-UB guard — zero aborts via `align_rt_div_fail`, `INT_MIN/-1`
wraps, constant divisors skip the guard (#294), `json.decode` out-of-range integers — sign bit added
to the field tag, an ABI change across codegen+runtime (#295), the expression-depth ICE — post-parse
`cap_expr_depths` with a measured 128 ceiling (#296), and the `chunks`-over-local-array region hole,
including the str-array storage-vs-element-region case Gemini caught on the PR (#297). **Still open:
the perf backlog** — led by missing LLVM no-alias metadata on fused loops and `task_group` spawning a
thread per task instead of reusing `ParPool` — and the design-decision Open items (out-of-range
literals, `main() -> Result<(), E>` exit mapping). The
design-facing conclusions from the same review (MIR must carry vectorization-enabling *properties*
and never bake in a fixed vector width — vector width stays a permanent backend decision; a two-tier
SIMD story where `vecN<T>`/`maskN<T>` stay the fixed-width kernel escape hatch and the pipeline is the
width-agnostic main path where scalable ISAs live; `str + str` is now a hard error, not a lint
candidate; unconstrained-literal defaults and `&&`/`||` short-circuit order are now explicit in the
spec) were landed the same day in `draft.md` / `docs/design-notes.md` / `docs/impl/*` and recorded as
Settled/Future entries in `docs/open-questions.md`. **The perf backlog from this review is now done
(PRs #300–#303, same day):** alias-scope metadata was investigated and **deferred with the sound
encoding documented** — the mechanism proven but no source construct generates an aliasing-ambiguous
loop today (belongs with the future `map_into(out)` slice); the investigation surfaced and #302
fixed a real soundness hole (`out` no-alias check blind to sub-slices); `task_group` now reuses
`ParPool` via a caller-participating claim loop (nesting-deadlock-free by construction, #301);
allocator declarations carry verified `noalias`/`nounwind`/split-`nofree` attributes and `emit-llvm`
output is self-describing (triple, #301); and the branchless identity-select `where` now covers
every reducer — the M6 completion criterion — with `where`+`min` demonstrably vectorizing (#303).
**Remaining from the review: the design-decision Open items** (out-of-range literals,
`main() -> Result<(), E>` exit mapping) and the deferred noalias emission gated on `map_into(out)`.

## Improvement wave (2026-07-02–03, PRs #306–#314)

A queue-driven improvement wave off the same review backlog, all merged same-day(-ish): **#306** closed
the json speculative-path duplicate-key gap (zero new state; cost confined to records with undeclared
colons); **#307** made default struct layout **unspecified order** — fields now reorder by descending
alignment (Rust-style) while `layout(C)` stays fixed, sema/codegen parity pinned by a mutation-checked
test; **#308** restricted `main`'s error type to the builtin `Error` and fixed a real ICE along the way
(user-`E` `main` was reaching codegen's hard-coded `Error` layout); **#309** made out-of-range integer
literals a hard error, including nested-negation effective values, while pattern literals keep wrap per
spec; **#310** confirmed `str` search was already memchr-SIMD (AVX2+NEON+scalar) and added a
differential SIMD-vs-scalar oracle; **#311** made json `u64` fields accept the full `u64` range (three
write sites unified through one dispatcher); **#312** evaluated `Option` niche optimization and deferred
it — no expressible target type today, since `Option` payloads are scalar-only — with the revisit plan
recorded; **#313** shipped M8 lints batch 1 (lossy `as` conversions, unconstrained-default large array
literals, both post-inference classification); **#314** was a clippy sweep, 50 warnings → 0 with zero
`allow`s, and unified the runtime's byte-copy loops on `copy_nonoverlapping`. **Held/deferred** (reasons
recorded in `docs/open-questions.md`): hot/cold field-split lint (heuristics need design), buffered
`print` (deliberate), escape→MIR dataflow + purity-as-effect-bit (structural, big), relative pointers
(no recursive types yet), `f16`/`bf16` (arithmetic semantics decision needed). Tests grew ~1047 → ~1103;
clippy is clean at `-D warnings`. **Next:** continue roadmap work (M6 is now formally closed, see
above; M8 remainders) — the queue is derived from `docs/impl/07-roadmap.md`.

## Roadmap-remainder wave (2026-07-03, PRs #316–#324)

Continuing the same queue, all merged with reviews reflected (#317 is absent below — closed
unmerged after a branch mishap and re-landed as #318): **#316** added dynamic `array<Struct>`
element-field write (`StoreElemFieldPtr`, the write dual of `IndexFieldPtr`); **#318** shipped
lane-wise `%` for `vecN<T>` and closed the unguarded vec integer-division UB residual alongside it (lane-wise
zero-abort + `INT_MIN`/`-1` wrap, plus a broadcast-constant fast path); **#319** gave over-aligned
struct arrays a padded stride (`round_up(size, align)`, C-style tail padding; dynamic
`array<align(N)S>` stays rejected pending aligned heap alloc); **#320** added the `align(N)` binding
form for numeric scalar arrays — the aligned-vector-load enabler (a proven-or-nothing aligned-load
switch); **#321** converged canonical hashing into a new `align_hash` crate (wyhash) so the JSON PHF
byte-match is now structural, with `group_by`/`dict_encode` 1.4–1.8× faster; **#322** shipped
str-bearing `soa` element writes (read columns had already landed; the write path was the real gap);
**#323** shipped M8 lint batch 2 (`unnecessary-heap`, narrow single-node form; `prefer-pipeline-over-vecN`
held — no loop syntax exists yet to fire on); **#324** fixed a **class-closing miscompile**: `check_expr`
now reconciles every value's type with its expected context at a single reconciliation point (found via
#323's side discovery), surfacing ~10 latent silent-truncation spots in the test corpus, now explicit
`as` casts. Tests grew ~1103 → ~1147. M6/M8 roadmap remainders are now essentially consumed;
deferred-with-record: owned `soa` columns, dynamic over-aligned arrays, cross-function aligned slice
loads, the `prefer-pipeline` lint (needs a kernel surface).

## Third wave (2026-07-03, PRs #326–#330)

**#326** extended the differential fuzzer to pipeline reducers and `vecN` lane arithmetic
(mutation-checked, no miscompiles found); **#327** formally closed M6 with both completion conditions
re-verified; **#328** shipped the `map_into(out)` pipeline terminal with the thrice-deferred
out-slice noalias emission — overlap guards went 3→0 at `-O2` — and the Claude-side fallback review
caught a CONFIRMED false-noalias miscompile (call-laundered aliasing args) pre-merge, fixed by
conservatizing the caller-side out-disjointness check; **#329** shipped FFI by-value structs
(x86-64 SysV, ≤16B register class) with clang-verified flattening, and the fallback review caught a
CONFIRMED SysV atomicity miscompile under register pressure, resolved by a compile-time GP/SSE budget
walk after aggregate-coerce was empirically disproven; **#330** decided the `soa_slice<T>` repr
(windowed 4-word soa view, unification not a new type) with implementation deferred pending a
concrete consumer. **Review process note:** `gemini-code-assist` ran out of daily quota mid-wave; per
owner instruction the review gate switched to Claude-side fallback reviews (deep-reasoner adversarial
pass + re-verification of fixes by the original reviewer). This caught two real miscompiles pre-merge
(#328/#329) — the gate is load-bearing. Tests grew ~1147 → ~1190.

## Latest (2026-07-02, PRs #262–#290)

Since the #183 snapshot below: **M8 unsafe/`raw.*` + `extern "C"` FFI v1 shipped** (#262–#269:
`extern "C"` decls + unsafe-gated foreign calls, `layout(C)` struct-by-pointer ABI, `str`/`slice`/
`bytes` → C data pointer, `link("name")`; deferred: by-value struct return, `bool`/`char` args,
`ptr_cast`). The **2026-07-02 external soundness audit is fully addressed** (#270–#277: escape/effect/
move coverage holes, `&&`/`||` short-circuit, arena double-free, negate-unsigned sign loss, parser/
diagnostic papercuts). Owned-array materialization `.to_array()` + a clean error for bare-literal-in-
owned-array-context (#283–#285). A **dependency-free fuzz + property suite** now locks the invariants
(#286–#290): `fuzz_frontend.rs` (front end never panics), `fuzz_fmt.rs` (formatter idempotent +
parse-preserving), `fuzz_differential.rs` (generate-program-with-oracle differential fuzzer over
scalars / all widths + casts / call ABI / struct+array aggregates — **no miscompile found**). `cargo
test` ≈ 990+ green. Remaining audit items are the structural refactors (escape→MIR-dataflow,
purity-as-effect-bit) tracked **open** in `docs/open-questions.md`. See memory
`fuzzing-infrastructure.md`, `audit-2026-07-02-fixes.md`, `m8-unsafe-raw-started.md`.

## Setup on the new machine

```bash
git clone https://github.com/sanohiro/align            # ideally into /home/<user>/prj/align
cd align
# Toolchain: Rust 1.96 + LLVM 19 (inkwell llvm19-1). Debian: apt install llvm-19 llvm-19-dev
# .cargo/config.toml already sets LLVM_SYS_191_PREFER_DYNAMIC=1 (Debian llvm-19 is shared-only).
cargo build && cargo test       # expect all green (1600+ tests)
```

The compiler is `./target/debug/alignc` (or `./target/release/alignc` after `--release`) — not on
`PATH`. `./target/debug/alignc run examples/min.align` compiles `.align` → native. Subcommands:
`check` / `emit-mir` / `emit-llvm` / `emit-obj` / `build` / `run`. (Or just drive it via `cargo run
-p align_driver -- run <file>`.)

## Where we are (as of main @ commit for PR #183)

The **language core is essentially complete**: types/struct/sum-type/tuple, if/match, Option/Result/
`?`, ownership (value/move/arena/box), strings/template/JSON, the data-oriented array/slice pipeline
(map/where/reduce/sum/scan/sort/partition/chunks), lambdas/closures, task_group/par_map, generics,
numeric casts, multi-file modules, named constants, bitwise/shift, LLVM -O2 (real SIMD). All run
end-to-end to native.

**M6 data-oriented perf is well underway and validated** (see `bench/`): `soa<T>` column scan beats
Rust ~8–10×; `group_by(.key).sum/min/max/.count()` beats the default `std::HashMap` 1.4–4.2×;
`par_map` uses a persistent worker pool; flat pipelines match idiomatic Rust (shared LLVM).

**Perf profiling snapshot (2026-06-29):** benchmark harnesses now support
`ALIGN_BENCH_PROFILE=1 .../run.sh native` decomposition output. The important measured bottlenecks:
JSON decode is parser/decoder-bound (`bench/json_decode`: 1M full decode-only ≈91 ms vs
decode+aggregate ≈92 ms); JSON→SoA **now beats serde** at 1M after the direct-decode work below
(`bench/json_soa`: ≈1.03× of serde); `group_by_reuse` now has a fused one-pass `a3` (below) that beats
the naive 4× group_by 3.2–3.7× but still trails smart single-pass Rust; `string_builder` is
call-count/itoa-bound, not capacity-bound (the `literal + int + literal` batch lowering below now
removes two of three per-row calls); cheap `par_map` loses to Align's own sequential/vectorized
`map().sum()` because every element crosses an indirect thunk. See `bench/README.md` and the
per-benchmark READMEs before changing perf code.

**Direct SoA JSON decode DONE (2026-06-29):** `json.decode → soa<Struct>` parses straight into
arena-allocated columns — no AoS intermediate, no transpose. New runtime `align_rt_json_decode_soa`
(count rows → arena-allocate columns via the `soa_column_offset` layout → fill in one value-parse
pass, sharing the AoS Mison speculation through a generic `FieldDst`); new `Rvalue::JsonDecodeSoa`;
`lower_json_decode_soa` rewritten (no more `transpose_to_soa` for json — `.to_soa()` still uses it).
At 1M rows the SoA path went ≈0.82× → **≈1.03× of serde_json** (~104 → ~83.5 ms), even edging the
AoS decode-only path (which still heap-materializes). See `bench/json_soa/README.md`.

**Fused multi-aggregate `group_by` DONE — first cut (2026-06-29):** `xs.group_by(.name).agg(sum(.a),
max(.b), count(), …)` over an AoS str key computes all K aggregates in **one pass** (intern key once,
fold K accumulators — the `HashMap<&str,[i64;K]>` shape), instead of one group_by per aggregate. New
surface (`.agg(...)`, sema `check_group_agg_multi` → `hir::ArrayGroupAggMulti`), MIR
`Rvalue::GroupAggMultiStr`, runtime `align_rt_group_multi_str` (with a fast FxHash-class hasher, not
SipHash). Bench `a3` beats naive `a1` 3.2–3.7× and beats `a2` (dict_encode reuse); still loses to smart
Rust 1.3–2.4×. **Measured (corrects an earlier guess):** right-sizing the output buffers is a *no-op*
(over-allocation is lazily paged); the real lever is the **hasher** (`ahash` moved `smart/a3` 0.77×→0.92×
at 632k), but it's a new runtime dependency; and the bench's smart baseline reads pre-extracted columns
(a3 reads AoS strided). Deferred: i64-key soa / `dict_encoded` sources. See
`bench/group_by_reuse/README.md` + `docs/open-questions.md`.

**Builder batch lowering DONE (2026-06-29):** the compiler lowers `b.write("lit"); b.write_int(x);
b.write("lit")` in a builder-reduce body to one `align_rt_builder_write_str_int_str` call — a MIR
peephole (`fuse_builder_writes` in `align_mir`), narrow to exactly the `str,int,str` shape on one
builder. Same-host before/after at 100k rows: generated `build` ~1.65 → ~1.30 ms (≈21%), within ~0.19
ms of the direct batch probe and now beating Rust `naive`. A general builder-chain batcher (other
shapes) is the recorded follow-up. See `bench/string_builder/README.md`.

**Active feature: nested struct fields** (`docs/impl/08-nested-structs.md`), the last big language gap:
- **Slice 1 DONE** (PR #182): plain-data (scalar-only, acyclic) nested struct fields — `Line { a: Point }`,
  depth-N read/write (`l.a.x`), nested-literal construction.
- **Slice 2 DONE** (PR #183): whole-struct value semantics (read `p := l.a`, struct-by-value
  params/returns, struct-to-struct assign) — was already working once Slice 1 generalized
  Field/Load/Store; locked in by `tests/struct_by_value.rs`.
- **Slice 3 DONE** (this branch): owned (`string`-bearing) struct fields → the struct becomes a
  **Move** type with a recursive **Drop**; whole-struct move (return/pass/assign) nulls the source.
  Closed the Move-vs-Copy soundness seams (array-of / Option-Result-enum-payload-of a Move struct
  rejected). `tests/owned_structs.rs`. Deferred: owned-field read-out (`u.name.len()`), `array<T>`
  fields, reassign-drops-old (a pre-existing gap for all owned types).

## Next action

**Recently DONE (perf):** builder batch lowering (`fuse_builder_writes`), direct SoA JSON decode
(`align_rt_json_decode_soa`), **and** the fused multi-aggregate `group_by(.key).agg(...)`
(`align_rt_group_multi_str`) — all in the snapshot above, all with new tests, `cargo test` green.

**Best next action: the remaining perf follow-ups**, in measured priority order: a **cross-cutting
"beat smart Rust" pass** (deferred on purpose — we trail smart in several benches, best decided once):
the hash strategy (`ahash` dep vs hand-rolled AES, applied across **all** str group paths incl.
`dict_encode`), an inline-value accumulator layout, and possibly a fair AoS-reading smart baseline — the
right-size-the-output-buffers idea was probed and is a **no-op** (lazy paging), so don't re-try it in
isolation. Also extend the fused `.agg(...)` to i64-key soa / `dict_encoded` sources. Then: cheap
`par_map` sequential fallback or thunk specialization; a SIMD/structural JSON parser (decode is still
value-parse-bound, the lever for both `json_decode` and `json_soa`). Smaller recorded follow-ups: a
general builder-chain batcher; fold
the SoA decode's count pass into the structural-index build. Re-run any perf change with:

```bash
ALIGN_BENCH_PROFILE=1 bench/json_soa/run.sh native
cargo test -q
```

Continue `docs/impl/08-nested-structs.md`:
- **Slice 4** — arrays/soa × nesting (`arr[i].a.x`, nested soa column) **and arrays of Move structs**
  (`[User{…}]` — needs per-element drop; Slice 3 rejects it for now). Risk: medium–high.
- **Slice 5 DONE** — cross-module field types (`f: geom.Point`): an imported `pub` type may be a
  struct field / enum payload / template member. `tests/cross_module_types.rs`.
- **Partial owned-field move DONE** — `n := u.name` (depth-1 `string` field) moves the buffer out,
  nulls the struct field, struct Drop frees null. Deeper paths / Move-struct fields still deferred.
- **Slice 4 `arr[i].a.x` read DONE** — nested field of a struct-array element (`ElemField.field` →
  `path`; first field loaded via the single-field seam, remainder projected from a temp slot — the
  pipeline seam untouched). Deferred: nested element *write* (`arr[i].a.x = v`), nested soa column,
  and **arrays of Move structs** (`[User{name}]`, per-element drop). `tests/struct_index.rs`.
- Smaller follow-up unblocked by Slice 3: owned `array<T>` struct fields.
- **DONE (this branch): borrowing an owned field out** — `u.name.len()` / `str` arg / `s: str :=
  u.name` now read a `string` field as a zero-copy `str` view (non-consuming, `Frame`-regioned so it
  can't escape the struct). Moving the field out stays deferred. `tests/owned_structs.rs`.
- **DONE (this branch): reassign-drops-old** — `mut s := …; s = …` no longer leaks the old buffer
  (all owned types). Sema's `MoveCheck` sets `Stmt::Assign::drop_old` (a `Cell<bool>`) iff the RHS
  doesn't move the old value out; MIR drops the slot before the store. No double-free (`s = f(s)`
  emits no drop). Still deferred: owned **field**/**element** reassign (`u.name = …`, `a[i] = …`).
  `crates/align_driver/tests/reassign_drop.rs`.

Or pause: this is a natural milestone (language core + S1/S2/S3 done, M6 perf validated).

## This session's PRs (#174–#183)

Gap A leak fix (#174); match-on-owned-payload double-free fix (#175); Gemini bench Part 3 record
(#176); builder itoa Gap D + string_builder bench (#177); `builder(capacity)` Gap C — measured *not*
the lever (#178); par_map persistent worker pool (#179); group_by table-interleave negative result
(#180); group_by min/max/count (#181); nested struct fields Slice 1 (#182); struct-by-value Slice 2
(#183).

## Process rules (do not skip — see `CLAUDE.md` + memory)

- **MANDATORY: reflect the `gemini-code-assist` PR review before merging any code PR** (until its
  2026-07-17 sunset). Open PR → poll until the review lands → scrutinize each finding (verify against
  code, don't blind-apply) → reflect valid ones / reject invalid with reason → merge. This lapsed
  once and the user called it out; do not repeat.
- **Benchmark-driven**: measure before claiming a win; if a change doesn't help (e.g. the group_by
  interleave, `builder(capacity)`), don't ship it — record the finding.
- **Ideal form, or defer**: ship only the ideal/unified form; defer rather than compromise.
- **English only** in the repo; **no backward-compat shims** (pre-release — change outright).

## Memory (does NOT travel with `git clone`)

Claude's cross-session memory lives at `~/.claude/projects/-home-hiro-prj-align/memory/` (10
files; see its `MEMORY.md` index). The
repo is self-sufficient without it, but to carry it over:

```bash
# old machine (note the leading ./ — the dir name starts with '-', which tar would else read as flags):
tar czf align-memory.tgz -C ~/.claude/projects ./-home-hiro-prj-align
# new machine:
tar xzf align-memory.tgz -C ~/.claude/projects
```
The project key (`-home-hiro-prj-align`) is derived from the clone path. Clone to the **same**
path (`/home/<user>/prj/align`) so it matches. If the new machine's user/path differs, the key
changes (e.g. `-home-bob-prj-align`) — rename the extracted folder to that new key, or Claude
Code won't pick the memory up.
