# Session handoff (continue on another machine)

A living continuity note so a fresh Claude Code session — e.g. on a faster machine — can pick the
work up immediately. **If you are a new session: read this, then `CLAUDE.md`, then
`docs/impl/source-correctness-fixes-2026-07-13.md`, then
`docs/impl/13-string-array-allocation-short-input-audit.md`, then
`docs/impl/12-pipeline-closure-memory-io-simd-audit.md`, then `docs/impl/08-nested-structs.md`.**
Everything durable is in this repo; the conversation history and
Claude's per-machine memory do not travel with `git clone` (see "Memory" below).

_Last updated: 2026-07-23, **v0.2.0 RELEASE PREP: the workspace version is bumped to 0.2.0 and
`RELEASE_NOTES_0.2.0.md` is written (headline: `pkg.web`, the zero-copy REST framework — 1.31x Go
Fiber-prefork; plus the JSON/`std.http` waves and the three soundness fixes). Tagging `v0.2.0` and
pushing the tag — which is what triggers the public release build — is the OWNER's step, deliberately
not automated here.** Before that, **the Gate-3 codegen hardening #620 started is FINISHED (NEXT item 0b).**
`gen_print` now dispatches on the SAME `align_sema::print_kind` classification the template holes
use and reads its operand through the same `display_int` / `display_float` / `display_view`
accessors, so its catch-all integer tail is gone; `Operand::Value(id)` / `Operand::Arg(i)` are
fallible lookups instead of a direct map index and an `expect`, which closes the one unprintable
shape (`()`) that escaped even the template path. The sweep found the class was **live, not
theoretical**: `json.encode` of an `Option<enum>` / `Option<char>` field, an `array<char>` field, or
an `array<enum>` field reached codegen with **no sema diagnostic at all**, and the last two killed
the compiler outright. `json.encode` now names its own encodable domain in sema
(`json_encodable_scalar`, one predicate for both an `Option` payload and an `array` element; an
`array<Struct>` element is validated like a decode target), and the descriptor builders behind that
gate (`json_payload_tag_sub` / `emit_json_subtable` / `emit_json_union` / `emit_desc_table`) return
`CodegenError` instead of aborting. The encode gate is **direction-aware** — the descriptor walk is
shared with decode but the domains differ by `Option<enum>` (the encoder renders it, the decoder has
no rule), so validating encode with the decode domain silently narrows it; the walk now takes a
`JsonDir`, which also stops an encode-only program reporting `'json.decode' field '…'`. That fix is
a net widening: `S { a: Option<N> }` with `N { c: Option<C> }` was REJECTED on `main` and now runs.
Verdicts over all 116 repo `.align` files are identical to `main` under `ALIGNC_CACHE=off`, and the
emitted LLVM IR is identical modulo SSA temp NAMES (5 of 108 files differ, only at the `print(str)`
site, now `%p`/`%l` from the shared accessor). Three defects found and deliberately NOT fixed here,
recorded as items 0c/0d/0e: `structArray.to_array()` panics `align_mir`; a `()`-valued binding
(`x := u()`) is a MIR lowering hole that now surfaces as a span-less codegen error instead of an
abort; and the codegen cache can mask a cold-build failure, so any main-vs-branch comparison must
use `ALIGNC_CACHE=off`.
Before that, **the `template` borrow-provenance hole was FIXED (NEXT item 0a, PR #621),
and with it the fail-open tail that hid it.** A `str` bound from a `template` inside a `loop` used to
escape the hidden owner's per-iteration free with `alignc check` reporting ok — `template` is the one
expression whose value (`str`) views storage the expression itself allocates (a hidden owned
`string`), so the existing hidden-owner machinery, which keys on the value's OWN type being
droppable, was structurally blind to it. That fact is now single-sourced in sema's
`owns_hidden_string(e, in_arena)`, which MIR's lowering calls too, and `MoveCheck` mirrors MIR's
arena stack so the idiomatic **arena-scoped** accumulator stays legal. Closing the
`_ => BorrowRoots::new()` tail that hid it — `borrow_sources_inner` is now exhaustive over all 216
`ExprKind` variants, no wildcard — surfaced **two more holes of the same class, neither previously
reported**: `EnumValue` (the one aggregate constructor that did not forward its payload's
provenance, so `keep = C.Text(view)` outlived the view's source) and `RandSample` (a sampled
`array<str>` holds views into its source). MIR is byte-identical over all 213 repo `.align` files;
the whole workspace suite is green with zero pre-existing tests flipped. Details, mutation evidence,
and the pinned legal-cases battery under NEXT item 0a. Before that, **the owned-`string` template
defect was FIXED (NEXT item 0, PR #620):
interpolating an owned `string` (`template "{h}"`) no longer panics codegen — it borrows the buffer
to a `str` view (sema's existing `StrBorrow`, the same borrow `b.write(s)` / a `str` argument
applies), so there is ONE string display and the `string` is neither moved nor freed by the
interpolation. `print`'s argument now takes the identical borrow (its lowered MIR is byte-identical,
verified), and the fail-open MIR catch-all that caused the bug (`_ => IntHole`) is replaced by
sema's single `print_kind` classification, which both stages now share. Every remaining unprintable
hole — array / slice / buffer / builder / array_builder / reader / struct / tuple / Option / enum /
`()` / fn-value — is a sema DIAGNOSTIC, swept by a 16-case matrix, and the codegen display paths
return a compiler error instead of an inkwell panic if a shape ever mismatches.** Before that,
**the REST-completeness backlog's last `TODO` was closed (#619):
`pkg.web.multipart` walks an RFC 7578 upload body with zero allocation and zero copy — `boundary()`
reads the delimiter out of the Content-Type, and `next(body, bd, from)` hands back
`Found(Part)` / `Done` / `Invalid` where every name, filename, content-type and data run is a view
into the caller's own bytes. A PUBLIC sibling module (`Part`/`Step` are named by applications), NOT
wired into the core surface: multipart is a body codec, not a routing concern, so the module imports
nothing.** Before that, **W4 was COMPLETE (#617): handler failures no longer vanish — unary
handlers and stream pumps log one best-effort stderr line with the real request method/path and the
complete builtin `Error`, while preserving the fixed 500/stream-close behavior and keeping the serve
loop alive.** Before that, **the W4 malformed-request matrix was DONE (#616): pkg.web now drives
request-line, target-form, header-syntax, Transfer-Encoding, and conflicting-Content-Length faults
through real sockets, proving after every class that only the bad connection closes and the serve
loop remains alive.** Before that, **the W4 route-tree edge matrix landed (#615): production dispatch is
now pinned at 64 path segments (past the linear oracle's safe score width), with 4 KiB static and
captured parameter segments, and across every empty-table query helper.** Before that, **the
web-router scaling row + CI gate landed (#614): identical runtime
paths at identical depths now cross the 6- and 128-route tables at both sibling-chain ends, using
median adjacent AB/BA ratios. Local head is 1.04–1.13× and the honest remaining tail slope is
1.84–2.17×; the first hosted Linux x86_64 run was 1.14× / 2.23×. CI pins head <= 1.35× and every
shape <= 2.75×.** Before that, **the HTTP
request serialization-buffer reuse landed (#613):
each client lends bounded scratch storage to an in-flight serializer and gets it back by RAII on
every success/error path; concurrent workers lease distinct buffers, and retention is capped at
eight buffers of at most 64 KiB. The common path falls 3 → 2 allocations/request and 263 → 213 fresh
bytes with CPU flat at ~3.1 µs above the syscall floor.** Before that, **the HTTP borrowed-host pool
lookup landed (#612):
authority splitting now returns a URL-backed host slice, and the pool is keyed by host first so
`HashMap<String, ...>` can query it with `&str`; `(scheme, port)` remains the inner endpoint key.
The common path falls 4 → 3 allocations/request and 272 → 263 fresh bytes with CPU flat at ~3.2 µs
above the syscall floor.** Before that, **the HTTP response header-span inline cut landed (#611):
the common `Content-Type` + `Content-Length` pair now lives inside the opaque response
handle, while a third header spills losslessly to the existing heap representation. The common path
falls 5 → 4 allocations/request and 352 → 272 fresh bytes with CPU flat at ~3.1 µs above the syscall
floor.** Before that, **the HTTP idle-bucket reuse cut landed (#610):
the pool retains an emptied bucket only while its checked-out request is in flight, then either
reuses it on put or removes it on every terminal no-put path. The common path falls 6 → 5
allocations/request (480 → 352 fresh bytes) with CPU flat at ~3.3 µs above the syscall floor; a
conditional cleanup leaves any concurrently refilled bucket untouched.** Before that, **the HTTP
pool-key ownership cut landed (#609):
the completed request moves its now-dead `(scheme, host, port)` key back into the idle map instead of
cloning the host `String`, taking the common path 7 → 6 allocations/request (489 → 480 fresh bytes)
with CPU flat at ~3.4 µs above the syscall floor.** Before that, **the body-size-aware HTTP client
read landed (#608): a 200 KiB response
falls from 6330 → 3693 ns/request above the syscall floor (median,
−42%), while 13 B and 8 KiB stay flat. Only the large body's middle reads directly into the returned
buffer; the 32 KiB first read remains, growth is geometric rather than trusting Content-Length up
front, and the last read stays unclamped to preserve dirty-connection detection.** Before that,
**the HTTP client allocation slice landed (#607):
`http.get` is 14 → 7 allocations/request and ~4.4 → ~3.4 µs above its syscall floor. Convenience
GET/POST borrow their ABI inputs, request serialization reserves its exact wire size, and the socket
exchange moves its first parsed head + receive buffer straight into the response instead of parsing
and copying both twice. The Codex review also closed a response-head cap bypass: a head whose final
read crossed 256 KiB and included the terminator used to be accepted.** Before that, the
protocol-path perf work gained an instrument
(`bench/http_path`, #601) and three wins (#602: the 32 KiB per-request memset is gone, ~−640 ns;
#603: the response is serialized in ONE right-sized allocation — 14 → 9 allocations/request,
~−468 ns; #604: the inbound head parse resumes across reads — a 206 KiB head in 100-byte pieces goes
56 ms → 0.24 ms, closing a slowloris lever); before that, the
scope-end-drop borrow hole is CLOSED — borrow liveness now ends at
the owner's DROP, not only at its move (see "DONE 2026-07-22" under NEXT). **That entry's closing
claim — "the language no longer accepts a program it must reject" — was FALSE, and item 0a below
records how: the drop-edge rule was right, but three expressions never handed it any provenance to
act on. They are fixed now; the honest statement is that the classification those rules read is now
compiler-forced to be complete, so the next such gap cannot be silent.** Before that, 2026-07-21: **pkg.web: CONCURRENT SERVE (PREFORK) + SERVER KEEP-ALIVE COMPLETE
(#595) — the hard streaming ordering constraint is LIFTED** — plus **#597, `accept`'s
transient-errno classification: a worker no longer dies over a dropped connection, a pending network
error, or a full fd table (see "DONE 2026-07-21" under NEXT).** The three designed slices all landed in one arc:
① std.http `http.serve_shared` (`SO_REUSEPORT` sibling op; `ExprKind`/`Rvalue::HttpServe` gained a
`shared: bool` FIELD, not a variant, so no analysis pass changed), ② std.http keep-alive entirely
inside `accept`/`respond` (one `Arc<Mutex<ParkSlot>>` per server handle; eligibility = 1.1 + no
`Connection: close` + no residual; `poll({parked, listener})` preferring parked; no `Connection`
header on a persistent response; `HttpServer::drop` marks the cell `Dead`), and ③ pkg.web prefork —
`serve(host, port, routes, workers)` with `workers == 1` inline (strict bind, zero threads) and
`>= 2` spawning `task_group` workers that each bind their own listener. **No compiler enabler was
needed**, exactly as probed: the worker's bind is an ordinary value-carrying
`if shared { http.serve_shared(…)? } else { http.serve(…)? }`. **Behavioral consequence for every
caller/test:** an eligible 1.1 request now leaves the connection OPEN, so a read-to-EOF client must
send `Connection: close` (driver tests share `common::one_shot`) or frame by `Content-Length`.
**Four corrections fell out of implementation + the adversarial review, all in this PR:**
① `respond` used to emit `Content-Length` only when a body was SET, so a bodiless `200` was framed
"until close" — un-keep-alive-able and indistinguishable from a truncated stream. Every response
whose status may carry a body is now framed either way (the set length, or `0`); `1xx`/`204`/`304`
get neither the header NOR any body bytes the builder holds (those would be read as the next
response). ② The park slot became a bounded SET (256, LRU eviction): with one slot every new
connection evicted the previous one, so a client just told "persistent" lost its next request —
keep-alive was useless past one client, which would also have made the W5 bench meaningless.
③ `serve` now aborts when `workers > process.cpu_count()`: `task_group` dispatches onto a pool sized
by the available parallelism, so never-returning workers past that count SILENTLY never start (the
`serve(..., 4)` = "four loops" promise was false above cores+1). That required the std addition
**`process.cpu_count()`** — which also makes the documented `workers = cores` sizing writable at
all. ④ `poll` now watches `POLLNVAL` (an invalid fd would otherwise spin the accept loop at 100%).
**A SECOND adversarial round then found seven more, all fixed here:** ⑤ **a malformed request killed
the server** — `accept` surfaced a per-request parse fault (bare-LF, a scanner, TLS to the plaintext
port) as an `Err`, so `srv.accept()?` returned and, with prefork, every worker died in turn; it now
closes that connection and keeps waiting, exactly as the parked path already did, and only a real
`accept(2)` failure returns. (Pre-existing since M11 — prefork is what made it systemic.) ⑥ the
readiness scan rotates, because "parked first" let busy keep-alive clients starve the listener
outright (its `SO_REUSEPORT` queue has no sibling to drain it). ⑦ a capacity valve on the listener
path, so idle keep-alive clients cannot pin every slot until `EMFILE` — **removed again in #597**,
which showed the park-time valve already bounds the set and this one only ever killed warm
connections for requests that never join it (see the #597 entry below). ⑧ the worker cap is
`cpu_count() + 1` (the pool PLUS the caller) — and the prefork tests, which hardcoded 4 workers,
would have aborted on any CI runner with fewer cores. ⑨ `respond_stream` rejects the bodiless
statuses too. ⑩ a body set on a bodiless status is now `Err`, not silently dropped (the same
treatment a caller-set `Content-Length` gets). ⑪ `accept`'s poll array is reused instead of two
allocations per request. Plus the doc rot and a vacuous eviction test the round-1 change left behind,
and the prefork tests are serialized (`SO_REUSEPORT` makes a port collision SILENT).
Tests: 6 runtime keep-alive units + the `serve_shared` double-bind unit, driver `m11_http_server.rs`
(serve_shared E2E + gates), `apps_web_root.rs` (keep-alive × the pkg.web loop), and the new
`apps_web_prefork.rs` (16 concurrent clients over 4 workers; a held-open SSE stream occupying ONE
worker while the others answer — the property the sequential loop could not have; `workers < 1`
abort).

**NEXT (recommended order — W5 and W7 are both DONE; details of each below and in the bench
READMEs):**

1. **Keep going down the protocol path with the dedicated in-process instruments
   (`bench/http_path` for the server, `bench/http_client_path` for the client), not `web_e2e`.** The
   server budget is **~2.5 µs of CPU above the poll floor** (the keep-alive `poll` is a further ~0.9
   µs and is not CPU). Measured targets, in order of what is known about them:
   - ~~apply #602's read-into-the-buffer to the CLIENT~~ — **tried, measured, DISCARDED (#606).**
     The memset it targets does not exist there (`objdump`: zero `memset` calls — LLVM elides it in
     that inlining context), and reading into the buffer forces a size decision before the framing is
     known: a 2 KiB start costs an extra `read` syscall for every response past 2 KiB (**+1200 ns at
     8 KiB, 3/3**) and starting bigger is not available because that buffer IS the returned response
     body. Full numbers + the two lessons in `bench/http_client_path/README.md`.
   - **DONE (#609) — move the completed request's pool key:** the success path always
     returns, so its owned `(scheme, host, port)` key is dead after returning the conn. Moving it into
     the idle map instead of cloning its host `String` makes the common path **7 → 6 allocations**
     and fresh bytes **489 → 480 B/request**; CPU remains ~3.4 µs above the floor (three 100k runs
     3219/3418/3575 ns). The benchmark now pins 6; the 200 KiB arm likewise falls 10 → 9 allocations.
   - **DONE (#610) — retain the idle bucket across its in-flight request:** taking the
     last conn used to remove and free the bucket's `Vec<IdleConn>`, then every successful put built
     the same 128-byte allocation again. The empty bucket now survives only until that request puts
     or terminates; no-put paths remove it only if another request has not refilled it. This makes the
     common path **6 → 5 allocations** and fresh bytes **480 → 352 B/request**; CPU stays flat (three
     100k runs 3267/3181/3451 ns above the floor). The benchmark pins 5, and 200 KiB is 9 → 8.
   - **DONE (#611) — inline two response header spans:** the dominant response carries
     exactly `Content-Type` + `Content-Length`, so those spans now live in the opaque response handle;
     a third header spills to a `Vec` with order, lookup, and the 128-header cap preserved. This makes
     the common path **5 → 4 allocations** and fresh bytes **352 → 272 B/request**; CPU stays flat
     (three 100k runs 3113/3363/3146 ns above the floor). The benchmark pins 4; 200 KiB is 8 → 7.
   - **DONE (#612) — borrow host through pool lookup:** `http_split_authority` now
     returns a host slice into the URL, and the idle pool's outer `String` host key supports borrowed
     lookup; `(scheme, port)` is an inner map so schemes and ports still cannot cross. Host ownership
     is allocated only when a new host bucket is first created, not per request. This makes the common
     path **4 → 3 allocations** and fresh bytes **272 → 263 B/request**; CPU stays flat (three 100k
     runs 3054/3468/3229 ns above the floor). The benchmark pins 3; 200 KiB is 7 → 6.
   - **DONE (#613) — reuse request serialization buffers:** each client lends one scratch `Vec` to
     every in-flight serializer and takes it back through an RAII lease on success, validation,
     connect, TLS, socket, and retry exits. `get_many` workers lease distinct buffers; retention is
     bounded to eight buffers of at most 64 KiB, while larger requests still work and discard their
     allocation afterward. The common path is **3 → 2 allocations** and **263 → 213 fresh
     B/request**; CPU stays flat (three 100k runs 2995/3482/3118 ns, median 3118). The benchmark pins
     2, a disabled-reuse mutation restores 3 and trips the gate, and 200 KiB is 6 → 5.
   - **DONE (#608) — BODY-SIZE-AWARE response reads:** keep the 32 KiB first read,
     direct-read only a large framed body's middle into the response buffer, then leave the last
     32767 bytes to the original unclamped read. This keeps 13 B / 8 KiB flat, makes 200 KiB
     **6330 → 3693 ns/request above the floor (median, −42%)**, and reduces retained growth
     **229376 → 172116 B/request**. Capacity grows geometrically as bytes arrive, so a lying huge
     Content-Length does not allocate its whole claim up front. The one-byte final margin preserves
     the dirty-conn property; a new large-body socket regression pins the returned bytes and idle
     pool, alongside `http_client_does_not_pool_leftover_arriving_after_the_framing`.
   - **DONE (#607) — the obvious client ownership/allocation batch:** convenience
     GET/POST now borrow their ABI inputs; request serialization reserves once at its exact size;
     the first parsed response head and socket receive buffer move into the response rather than
     being rebuilt/copied. `bench/http_client_path`: **14 → 7 allocations**, growth **56 → 0
     B/request**, CPU **~4.4 → ~3.4 µs** above the floor (three runs 3362/3429/3489 ns). The bench
     pins 7 as a regression ceiling.
   - **Codex review correction:** the response-head cap was checked only while parsing returned
     `Incomplete`; an oversized head whose cap-crossing read also carried the final blank line
     returned `Ok` and bypassed it. The successful-head path now checks `body_start <= 256 KiB`,
     with a mutation-checked socket regression.
   - The remaining two common-path allocations are the response `Box` (120 B, including two inline
     spans) and response bytes (93 B here). Combining them needs a custom one-allocation response
     representation and a careful opaque-handle/free boundary. Builder calls also retain two
     `String`s per caller header.
2. **DONE (#614) — `bench/web_router` scaling row + CI gate:** the old row mixed a two-segment
   small path with a three-segment large path and sampled unrelated chain positions. Both tables now
   contain the exact same paths at the exact same depths; static/param are reported at chain head and
   tail, and ratios are medians of adjacent counterbalanced pairs. Six native/baseline 200k runs put
   head at 1.04–1.07× and worst at 2.08–2.11×; three CI-sized baseline runs put head at 1.09–1.13×
   and worst at 2.15–2.17×. The first hosted run was 1.14× / 2.23×. Linux x86_64 CI gates head <=
   1.35× and every shape <= 2.75×. Restoring the old depth mismatch reaches 1.59× and
   mutation-fails the head gate.
3. **DONE (#615) — W4 route-tree edge matrix:** absolute production-tree expectations cover a
   64-segment static and param path (the base-3 linear oracle can wrap beyond ~39), byte-exact 4 KiB
   static hit/miss, a 4 KiB zero-copy param capture, same-claim GET/POST dispatch + 405 `Allow`, and
   empty-table dispatch / 405 / `Allow` / validation. No implementation bug surfaced.
4. **DONE (#616) — W4 malformed-request matrix:** five real-socket rejection classes cover the
   request line, target form, header syntax, and framing; a valid routed request after every case
   proves the worker loop survives. The runtime parser's exhaustive guard + incremental/one-shot
   differential suites remain the lower-layer coverage.
5. **DONE (#617) — W4 handler-`Err` logging:** unary handlers and stream pumps emit one stable
   stderr line with method, path, and the full builtin `Error`; E2E pins every Error variant, unary
   500 + survival, and stream close + survival. **W4 is complete.**
6. **DONE (#618) — W6 middleware-lite:** `web.group` / `group_with`, homogeneous named-function
   middleware lists, zero-copy prefix-aware radix routing, ordered proceed/short-circuit/failure
   verdicts, logged 500s, and loop survival are E2E-pinned. The implementation also landed the two
   narrow language enablers the design required (function-value slices and Copy-struct
   `.to_array()`). Next: multipart.
7. **DONE (#619) — `pkg.web.multipart`, the REST-completeness backlog's last `TODO`:** an RFC 7578
   upload walk with **zero allocation and zero copy** — `boundary(content_type) -> str` (or `""`,
   the single "not a walkable multipart form" answer) and `next(body, bd, from) -> Found(Part) |
   Done | Invalid`, where `Part`'s name / filename / content_type / data are all views into the
   caller's own bytes and iteration state is one `i64` the caller owns (`Part.next`). A **public
   sibling** module like `cookie`/`cors` — `Part`/`Step` are named by applications — and
   deliberately **not** wired into the core surface, which keeps it import-free: a body codec is not
   a routing concern. Design decisions worth remembering: `Step` is a three-way or-kind because
   collapsing `Done` into `Invalid` would make a TRUNCATED body read as a well-formed end of
   iteration; a `\` quoted-pair in a quoted `filename`/`boundary` is REFUSED rather than returned
   still-escaped (un-escaping would allocate, and the escaped bytes are the wrong filename); and a
   delimiter is checked as a whole LINE (RFC 2046 §5.1.1), so a data line that merely starts with
   the boundary (`--SEPARATE` for boundary `SEP`) stays inside the part. **Review round two added
   the ambiguity rules:** a duplicate is refused, never resolved (two `Content-Disposition` /
   `Content-Type` lines in one part, or one parameter attribute twice — first-wins and last-wins are
   both guesses that disagree, and the parser in front of us may have made the other one), a BARE
   parameter value must be an RFC 9110 `token` (`boundary=A,B` / `boundary=abc def` are refused, as
   Go's `mime` refuses them), and a boundary may not end in SP/HTAB (§5.1.1 `bcharsnospace` —
   transport padding follows a delimiter). It also pinned the security note the docs were missing:
   **`filename` is advisory and unsanitized** — `../` and absolute paths come back verbatim, and an
   application must validate before using it as a path. No compiler enabler was needed.
   `crates/align_driver/tests/apps_web_multipart.rs` drives the shipped source (`include_str!`)
   through 16 cases, one of which EXTRACTS the documented `upload` handler out of
   `pkg-design/web.md` and compiles it against the real `apps/web/pkg/**`, so a doc example that
   stops compiling fails the suite. **§2b of the plan now has no `TODO` rows left.**

**DONE 2026-07-22 — borrow liveness ends at the owner's DROP, not only at its MOVE.** (**Correction,
2026-07-23:** this entry claimed the last program-the-language-must-reject was closed. It was not —
the *rule* was complete, but `borrow_sources_inner`'s fail-open `_` tail meant three expressions
handed it no provenance at all, so the rule never fired for them. Fixed in the entry below, item
0a.) `MoveCheck` invalidated a view
when its source was moved or reassigned; it never noticed the source being **freed**, so a view
assigned out of a loop body to a longer-lived local read the previous iteration's freed buffer. No
`unsafe`, no std handle: `mut keep: str := "start"` + `loop { print(keep); owned := mk("hello"); keep
= owned }` printed heap garbage on `main`.
- **The fix is a mirror of MIR, not a new policy** — and an iteration frees **two** kinds of
  storage, which is where the first cut of this fix was wrong (see the correction below). Named:
  `drop_locals ∩ body_locals`. Anonymous: `Builder::new_synthetic_owner` pushes the hidden owner of
  every **unbound Move temporary** into the innermost loop frame too, and `lower_loop` re-reads the
  final frame at the back-edge for exactly that reason. Both are freed at the **back-edge** and at
  **every `break`**. (The other `emit_drop_if_live` sites are function exit — nothing follows; an
  `Assign`'s drop-of-old — already the move path's; and a hidden owner after a *scalar-only*
  consumer — a view-returning consumer keeps its owner alive.) `MoveCheck` now ends both:
  `BorrowRoots` is a set of `BorrowRoot::Local(id) | IterTemp(depth)`, the named half from
  `iteration_drops` via the shared `needs_drop_flag` predicate behind `Fn::drop_locals`, the
  anonymous half from `temp_owner_root` via MIR's own materialization condition (`needs_drop_flag` +
  `may_need_synthetic_owner`, the latter **moved into sema** so the two stages share one definition).
  Depth is all the identity a temporary needs — every temporary at a depth dies on that loop's two
  edges; outside a loop the hidden owner lives to function exit, so nothing is recorded.
- `BorrowState::invalid` records **how** each generation ended (`Consumed` / `Dropped`) so the
  diagnostic names the right cause, and names the source when it *has* a name (a temporary's message
  instead names the three ways to give the storage a longer life — see #621); the join merges per root by
  `Ord`, keeping it commutative for the fixpoint.
- **Where the temp root is ATTRIBUTED matters as much as the condition.** MIR mints the hidden owner
  only where a fresh Move value is *borrowed* (`lower_borrowed_owned`); a value **moved** into a
  local that owns it transfers its storage to that named local, and nothing joins the loop's drop
  set. The first attempt added the root in `borrow_sources` — which every consumer reaches — and so
  rejected the ordinary `names = src.map(up).to_array()` rebuild-each-pass idiom. It now comes from
  `storage_roots`, which *is* the borrowing position (every borrow producer routes its operand
  through it; a materializer recurses through `borrow_sources` and gets nothing).
- **A wrapper must not launder a borrow — the third round's find, and the sharpest one.**
  `may_need_synthetic_owner` is transparent through `{ }` / `unsafe { }`; `storage_roots` was not, so
  a block recorded NO root at all — not `IterTemp` (correctly not a temporary) and not the place's
  `Local` (the fallback short-circuits on an owned type). `keep = { inner }` walked past the entire
  rule and printed freed heap, two characters from the rejected `keep = inner`, and it had slipped
  every earlier revision of this fix. The decision is now **single-sourced in
  `borrow_transparent_value`**, which both consumers call, so adding a wrapper updates them together
  rather than relying on a comment to keep them agreeing. Both still end in a `_` catch-all, so a
  future variant that *should* be transparent would compile while being neither — compile-forcing
  that (an exhaustive, wildcard-free place dispatch) is recorded with the structural follow-up.
- **A third over-rejection, pinned** (`over_rejects_a_control_flow_borrow_over_outer_bound_places`):
  `keep = if c { a } else { b }` over sources declared OUTSIDE the loop is rejected, because
  `may_need_synthetic_owner` is conservatively `true` for `if`/`match`/`else`-unwrap/`arena`/
  `task_group`, whose runtime value can still be a bound place. `emit-mir` shows the owner's
  temporary flag stored `false` on every bound-arm path — no drop at either edge. Same family as the
  arena and chunks pins: a static shape predicate against a per-path runtime flag sema cannot see.
  Workaround (borrow the arms as `str` views first) is accepted and runs.
- **This fix shipped a false claim TWICE, and both times the adversarial reviewer killed it.** Round
  1's commit message asserted "MIR emits exactly one class of early drop" and that hidden owners are
  safe "by construction" — both wrong, and `keep = "AAAA…".clone()` in a loop still printed freed
  heap bytes. Round 2's placement of the fix then over-rejected a common idiom. **A drop-site
  enumeration is a claim to verify against the emitter, not to reason out**, and a new root's
  *attribution point* needs the same treatment. The mandatory independent adversarial pass is what
  caught both; do not skip it.
- **Two claims in the original write-up were wrong, and that is the reusable lesson** (same shape as
  the #597 test-hang guess): ① the `arena {}` variant is **not an instance of this bug** — a
  heap-owned local bound inside `arena {}` is dropped at *function* exit (`emit-mir` shows the drop
  after `arena_end`, and the shape prints the correct string, i.e. nothing was freed), while storage
  that genuinely is arena-allocated is already rejected by the region rule's `decl_depth` check.
  ② `http_headers` is not reliably louder than `str`; both are shape- and allocator-dependent UB.
  Verify a UB claim by running it, not by reasoning from the type.
- **The one cost, pinned as a test rather than hidden.** The rule keys on the *type* predicate
  `needs_drop_flag`, because `MoveCheck` runs BEFORE `EscapeCheck` and cannot see the
  individual-vs-arena ownership bit. An array allocated inside an enclosing `arena {}` is
  arena-owned — flag never set, back-edge drop folded away, nothing freed until `arena_end` — yet a
  view of it assigned out of the loop is now rejected. The same shape with a heap-owned source (a
  `string`, malloc'd even inside an arena) is a real use-after-free that must stay rejected, and the
  two are indistinguishable to a type-level predicate. Conservative is the right side (inconvenience,
  not breakage). The real fix is the recorded structural follow-up — borrow liveness belongs in the
  checked-HIR escape CFG (#461–#464), which already has regions, provenance, and loop fixpoints.
  Pinned as `over_rejects_a_view_of_an_arena_allocated_loop_local`.
- **A second, pre-existing over-rejection the loop rule widened**, also pinned: `ch[0]` views the
  *source* array, not the chunks header, yet `local_owns_view_storage` counts a `DynSliceArray` local
  as owning its elements' storage — so ending the header's generation kills the element views. It is
  rejected on plain reassignment with no loop in sight (verified), so the loop rule only made the
  common shape hit it; the fix belongs with `local_owns_view_storage`.
  Pinned as `over_rejects_a_view_into_the_source_of_a_dropped_chunks_header`.
- Tests: the flipped `a_view_of_a_handle_dropped_at_the_end_of_an_iteration_is_rejected` (was
  `known_hole_scope_end_drop_…`), plus thirteen in `tests/borrow_liveness.rs` — back-edge, `break`
  edge, all four temporary shapes, a `json` view over a temporary and over a dropped loop-body
  input, six block-laundering shapes, and the controls that keep the rule from over-rejecting: a
  fresh value **moved into an owning local** (array and Move-struct forms), a block over a source
  that outlives the loop, same-iteration use of a local and of a temporary, a temporary outside any
  loop, a source declared outside the loop, an inner `break` dropping only the inner body's locals,
  and an owned local **moved out** by `break`. Mutation-checked in six places (the local set,
  `temp_owner_root`, the `IterTemp` edge arm, the `storage_roots` attribution, the block-transparency
  arm, and *restoring* the attribution to `borrow_sources`), each failing exactly its own tests.
  Whole workspace green (2605 passed), clippy clean; **zero false positives** — the only pre-existing
  test the whole arc broke was the pinned known hole.

**DONE 2026-07-22 — `bench/http_path` (#601), and the correction that the roadmap's measurement plan
did not work.** The plan said to price the 4.1 µs path's allocations with `bench/web_e2e` at
`CONNS=1`. It cannot: that figure is the *difference of two ~70 µs measurements*, so it carries both
their noises — three adjacent baseline runs gave **3.3 / 3.9 / 4.8 µs**, a spread larger than the
whole allocation budget. `bench/http_path` prices the same path in-process on an exact allocation
count (**14.00 per request** then, 9.00 after #603; zero noise) plus the server thread's
`CLOCK_THREAD_CPUTIME_ID`.
- **Two floors, because Align does one syscall more.** The keep-alive `poll` costs **~0.9 µs** — 21%
  of the naive difference, and not CPU work. Reporting only against a plain read/write floor charged
  it to Align. The honest CPU budget is **~3.5 µs**.
- **Three of the first version's headline claims were wrong** and the adversarial rounds killed each:
  the floor was missing that `poll`; "±1.3%" was a 3-sample fluke (11 samples gave σ 3.5%, and the
  residual is *between-run drift*, which iterations cannot fix); and the bench built the runtime with
  LTO while the compiler links a default-release archive across a C ABI (2.8% bias). Then a second
  round found the **slot position itself is biased** — three *identical* floors read slot 2 ~115 ns
  above slot 1 — so the arm order is now counterbalanced, which moved the reported `poll` cost from
  1022 to 858 ns against an independent 913 ns.
- Lessons promoted to `bench/README.md`: a floor must do the same syscalls or its difference is not
  the thing you named; iterations do not cure drift (alternate, counterbalance, take the median); and
  min is the wrong statistic for a *difference of two* arms (median σ 88/173/99 vs min 146/214/127).

**DONE 2026-07-22 — the inbound head parse RESUMES across reads (#604): 56 ms → 0.24 ms, the
slowloris lever closed.** `http_read_request` re-parsed the whole buffer after every read, so the
cost of one request was quadratic in **how finely the client chose to split its head** — nothing to
do with volume. Measured on a 206 KiB head (120 headers, under the 128 cap): **56.4 ms** of one
worker's CPU at 100-byte pieces, 5.2 ms at 1 KiB, 0.67 ms at 8 KiB — the shape of an O(n²). Prefork
makes it systemic: a handful of such connections pins every worker. Now **0.243 / 0.153 / 0.062 ms**
for the same three splits (**232× at 100 bytes**), and the bytes the `\n` search touches are
**exactly the head's size at every split** — 1.00×, invariant.
- **The fix is the parser, not a cache in front of it.** `HttpRequestHeadScan` holds `pos` (the next
  unparsed line), `scanned` (how far into the current line the `\n` search already got), the
  validated request line, the header spans, and the `Content-Length` — all offsets into a buffer
  that only ever grows, so a span accepted in an earlier call stays valid in a later one. `advance`
  continues; it never starts over. `scanned` is what bounds a **single** pathological header line
  (up to the 256 KiB block cap) to O(its length) rather than O(length × reads) — without it the
  quadratic survives inside one line.
- **The one-shot `http_parse_request_head` is now `#[cfg(test)]`** — the same scan advanced once. Two
  new tests tie the forms together: a **differential oracle** re-feeding a 22-case corpus (well-formed
  shapes, valid-so-far prefixes, and every smuggling guard) at 1/2/3/7/64/4096-byte pieces and
  asserting the incremental verdict and every span match the one-shot; and a linearity test asserting
  the *measured* search work stays under 2× the head.
- **A spent scan used to return a header-less `Ok`** — the adversarial review's one finding, and the
  comment that claimed to guard it was factually wrong. `advance` moves only the header list out, and
  the terminating `break` deliberately skips `commit_line`, so `pos` still points AT the blank line: a
  second call re-found it and returned `Ok` **with the framing intact** (`content_length` still set,
  so a body would still be consumed) and **every header gone** — which would make
  `http_request_wants_close` see an empty list and read a `Connection: close` request as keep-alive.
  Not reachable (`http_read_request` builds a fresh scan per request and stops advancing once the head
  is `Some`), so it was a trap for the next caller rather than a live bug. A `spent` latch now fails
  closed with `Invalid` — `Incomplete` would have spun the read loop — and a test pins it.
  The review's 400k-input × 7-chunking differential fuzz found **no** parse disagreement, no panic
  (debug build, including buffers truncated below `pos`), and no residual quadratic (1.00× at every
  split, including a single 200 KiB header line); its two coverage notes are also applied — the
  `HTTP_MAX_HEADERS` cliff itself (126/128/129) is now in the differential corpus, and the linearity
  test records that `piece=100` is the row carrying its assertion (a regression costs only ~1.2× at
  8192 and would slip under the 2× bar there).
- **The instrument was wrong before the mutations fixed it.** The first linearity assertion read the
  cursor's own bookkeeping, so removing the resume entirely (`from = pos`) left it green — it
  measured what the code intended, not what it did. It now counts what `memchr` actually looked at
  (a `#[cfg(test)]` counter, charging `rel + 1` on a hit — charging the slice length instead read
  3.34× and looked like a failure). Four mutations now fail the right test: no-resume and
  scanned-never-advances fail linearity, scanned-not-reset fails both, and a weakened bare-LF guard
  fails the existing smuggling suite (a differential test cannot catch that one — it mutates both
  sides equally, which is worth remembering).

**DONE 2026-07-22 — one right-sized allocation per response (#603), the second win on that budget.**
`http_serialize_head` started from `Vec::new()` and doubled 8→16→32→64→128 on the way to a plaintext
response, and `http_serialize_response_inner` then rendered the `Content-Length` value through
`to_string()` — a `String` allocated and dropped per response. Both are gone: the head serializer now
takes an **`extra`** parameter (what the caller will append — its framing header, the blank line, the
body) and reserves `http_head_len(rb, persistent) + extra` up front, so the whole message is written
into ONE exactly-sized buffer; `http_push_decimal` renders the length from the stack, the sibling of
the chunked path's `http_push_chunk_size_hex`. **14 → 9 allocations/request** and **−537 / −478 /
−421 / −434 ns across four adjacent A/B pairs, 4/4 the same sign** (`bench/http_path`); the poll-floor
budget is now ~2.5 µs. `respond_stream` reserves its `Transfer-Encoding` line the same way.
- **The size function is the risk, and the assertion has to be the right one.** `http_head_len` +
  `http_response_extra` mirror the writer, so any drift (a separator, a prefix, the blank line)
  silently costs a `realloc` — the very thing removed. The test asserts **`out.len()` == what the
  size functions say**, which depends on no allocator behaviour, over a matrix (no headers / many
  headers / bodied / set-but-empty body / HEAD-suppressed / persistent / the three bodiless statuses
  / an unknown status's empty reason phrase). `len() == capacity()` follows as a *corollary* — on
  its own it is weaker than it looks, since `RawVec` grows to `max(cap*2, required)` and a
  half-sized reservation would satisfy it after a real growth. The five written literals are
  `const`s shared by writer and sizer, so they cannot drift at all.
- **The adversarial review found a realloc this pass ADDED, and a test that pinned nothing.**
  ① `stream_finish` extends the *stored head* with the 5-byte chunked terminator, so an exactly
  sized head guaranteed a `realloc` + copy on every zero-event SSE response (on `main` the head's
  slack absorbed it). `http_stream_head_extra` now reserves those 5 bytes; when a `send` comes first
  the head is copied out and dropped, so they are merely unused. ② The streaming reservation had
  **no test at all** — the first version re-typed the formula in the test and asserted on its own
  copy, so mutating the production line to a 999-byte over-reserve left all 292 tests green (nothing
  about over/under-reserving changes a wire byte). It now goes through the real
  `align_rt_http_respond_stream` and asserts the stored head's spare capacity is *exactly* the
  terminator's room. Both mutations, and a body-forgetting response sizer, now fail.
- The wrapper was **not** kept: `http_serialize_head` itself gained the parameter (one mechanism, per
  the repo's no-compat rule), and its three call sites each pass what they append.

**DONE 2026-07-22 — the 32 KiB memset on every request (#602), the first real win on that budget.**
`http_read_request` read into a `let mut chunk = [0u8; 32 * 1024]` and then `extend_from_slice`d it
into the buffer that keeps the request. The array is zero-initialised and LLVM does not elide it, so
the shipped object carried a **32 KiB `memset` plus an eight-page stack probe per request** — larger
than all fourteen heap allocations together — plus a per-byte copy. Reading straight into the
buffer's uninitialised spare capacity removes all three. **Median −640 ns** (reviewer, 8/8 negative),
−404 ns on the final commit here; **−16% on a 200 KiB body**, because the memset is per-request but
the copy is per-byte.
- **The first cut introduced an attacker-controlled 2× peak buffer** and the review caught it:
  reserving a flat chunk before the final short read let `Vec`'s amortized growth double, and
  `truncate` never gives capacity back — 2 GiB per connection at `HTTP_MAX_BODY`. Reads are now
  clamped to the framed remainder, with doubling bounded by the framed total: capacity/total is
  **1.00× where main is 2.00×**, at 1/8/64 MiB, with reallocs still O(log n).
- Behaviour change, benign and now documented: a client pipelining behind a *bodied* request is no
  longer closed on — reads can no longer overshoot the framing, so the next request is simply re-read
  on the next `accept`. No byte can be lost (`truncate` discards only when `residual` is already set).

**DONE 2026-07-22 — a test-harness exe race that turned `main` red (#600), diagnosed not guessed.**
`response_builder_payload.rs`'s `run_server` took a `name` and threw it away (`let _ = name;`),
building every server as `srv-rb`. `build_exe` derives the executable path from that name plus the
pid, and libtest runs the two server tests concurrently in one binary — so both built, spawned, and
deleted the SAME file: `ETXTBSY` when one spawned while the other was still linking it, `NotFound`
when the other's `TempArtifacts` drop had already removed it. Both appeared in one run.
**Measured A/B** (four concurrent runs of the test binary x six rounds): **2/24 failed with the
shared name, 0/24 with a per-test one**; serially it is 0/15 either way, which is why it reads as a
one-off flake — the reproduction needs concurrent load. Swept the suite for the same hazard: no other
file uses a duplicate literal `build_exe` name, and the other computed-name callers already thread
theirs through.

**DONE 2026-07-21 — `web.header(c, name)`, on the std.http enabler `ctx.headers()`
(`std-design/http.md` item 10, now SHIPPED).** The detached view won: `ctx.headers() ->
http_headers`, a Copy, region-bound, non-owning view **whose representation is the ctx pointer**, so
`hs.get(name)` lowers to the runtime lookup that already existed — **the enabler added no runtime
code at all**. `ctx.header(name)` is REPLACED by `ctx.headers().get(name)` (there were no Align call
sites, so one spelling cost nothing); `Ctx` carries the view as one more field and `web.header`
forwards. pkg.web ships no lookup of its own.
- **The region rule SPLITS**, which is the whole design: `ctx.headers()` keeps the `Frame` cap
  (a view minted from a local handle stays in the frame — no return, no `break`, no surviving a
  serve iteration) and `hs.get(name)` **inherits** `region_of(hs)`, so through a parameter it is
  `Static` and the pkg.web wrapper compiles. Re-capping the lookup was mutation-checked: it breaks
  every pkg.web E2E.
- **Where the design's own fail-open account was wrong (recorded in item 10 "What actually
  shipped"):** `region_of` and `slice_is_local` are exhaustive over `ExprKind`, so the region rule
  is fail-CLOSED, not fail-open; the real fail-open set is `ty_may_borrow`, `scalar_type`'s pointer
  arm, and `borrow_sources_inner`. All three mutation-checked against the test matrix. Two smaller
  finds: there are TWO `payload_scalar`s and the `Checker` one mislabelled every rejection as an
  "Option payload" (now takes the position it checks); and the ctx-method dispatch arm is
  name-guarded, so `ctx.header(x)` never reaches the "try method / path / …" suggestion string —
  which is why the removed name now has its own arm that ERRORS with the replacement spelled out
  (a diagnostic; it resolves nothing).
- `Ty::HttpRequestCtx`'s 16-vs-8 `ty_size_align` over-report was fixed alongside, and
  `sema_and_codegen_struct_layout_agree` gained the rows it never had for a Move-handle field, a
  `Ty::Fn` field, a `slice<T>` field, the new view field, and the pkg.web `Ctx` shape.
- Tests: `crates/align_driver/tests/http_headers_view.rs` (the full item-10 matrix) +
  `apps_web_root.rs::web_header_reads_the_request_header_table` (case-insensitive hit / folded hit /
  absent-is-`None` / present-but-empty-is-`Some("")`).
- **A PRE-EXISTING soundness hole surfaced by the adversarial pass, deliberately not fixed here —
  FIXED the next day, see "DONE 2026-07-22" above:** `MoveCheck` ended a borrow generation when the
  owner was **moved or reassigned**, never when it was **dropped at the end of a loop iteration**, so
  a view assigned out of a loop body whose Move handle merely drops survived it and read freed
  memory. General to every view over a Move handle: reproduced identically on a plain `str` from
  `ctx.path()` and on a plain `string`. Every shipped `serve` loop was safe because `ctx.respond(rb)`
  MOVES the handle each pass, which the existing move path rejects. Two claims made here were wrong
  and are corrected in the 2026-07-22 entry: the `arena {}` block does **not** do it too, and this
  type is not reliably louder than a `str`.

**DONE 2026-07-21 — `accept`'s transient-errno classification (#597).** `http_accept_conn` used to
return ANY `accept(2)` failure and `pkg.web`'s `srv.accept()?` ended the worker on it. One decision
function (`classify_accept_error`) now owns the whole policy — `Again` / `NoFds` / `Fatal` — and the
noise half (`accept_errno_is_noise`) is shared with std.net's `tcp_accept`, which had the identical
hole. **No errno reaches Align's `Error`**: the classification lives entirely under `accept`, so no
language surface changed. **The adversarial round is where this got its real shape — read the fixes,
not just the feature:**
- **The retry must be a WAIT, not a re-`accept` in place.** The first cut looped inside
  `http_accept_conn`. On a blocking listener that parks the thread in `accept` — and the parked
  keep-alive connections share that one `poll`, so a single client connecting and resetting would
  have stalled every warm client until an unrelated new connection arrived. `http_accept_conn` now
  does exactly ONE `accept`; `Again` returns to the caller's wait loop.
- **`ECONNABORTED` is the wrong errno to lean on for Linux.** Linux usually completes the handshake
  and reports a reset later; what it DOES hand back from `accept` is the connection's
  already-pending network error (`ENETDOWN`, `EPROTO`, `ENOPROTOOPT`, `EHOSTDOWN`, `ENONET`,
  `EHOSTUNREACH`, `EOPNOTSUPP`, `ENETUNREACH` — accept(2) says to treat them like `EAGAIN`). All
  eight are now noise; without them the fix mostly covered a BSD event.
- **Which connection exhaustion spends, and how fast.** The reclaim reads this wait's `revents` and
  takes the coldest parked connection *with no readable request* (closing one whose next request has
  already arrived drops it silently), falling back to the coldest when all are readable. And it is
  **paced at one per 10 ms of waiting** (state per `accept` call — the only place a burn-down could
  run): prefork workers share the descriptor table but each owns a separate parked set, so the fds a
  worker lacks are usually a sibling's — unpaced, one worker would burn its whole warm set in a
  tight loop over pressure it did not cause.
- **The accept-time capacity valve is GONE** (round 3). Moving it after the accept — the round-2 fix
  — left it with no upside: the parked set is bounded where it GROWS (`respond` evicts the coldest
  when parking into a full set), so what an accept-side valve still did was kill a warm connection
  for a request that never joins the set (`Connection: close`, malformed, a client that vanishes
  after the handshake), permanently shrinking the warm set. Round 3 also showed **no test observed
  it at all** — deleting it outright left the suite green — so the removal ships with a regression
  test that pins it (a one-shot request at capacity costs nobody their connection), and that test
  fails FAST rather than hanging, which the first version of it did.
  **Two things the removal does cost, recorded because round 4 caught the claim being absolute:**
  ① the valve was also the only path that reaped a **zombie parked connection** — a peer that
  vanished without a FIN is silent, so `poll` never reports it and no eviction fires while traffic
  stays one-shot. `accept` now sets **`SO_KEEPALIVE`** on every connection (parity with the net
  rail), so the kernel's probes turn such a connection into a hangup this loop closes — slow (hours,
  by system default) but bounded, and `NoFds` reclaims immediately under real pressure. ② the
  worst-case descriptor count per worker goes from MAX to **MAX + 1** (parked MAX plus the in-flight
  connection), i.e. 257 in production, +1 per worker. #595's stated reason for the valve (`EMFILE`
  from idle clients pinning every slot) was never its own: the park-time valve bounds the set at the
  same MAX either way.
- **The child-process E2E was fail-open**: `libtest` exits 0 when a filter matches nothing, so a
  rename would have left the child running no test with the parent still green. It now pipes the
  child's output and asserts the harness's own "1 passed" (and prints both streams on failure).

Tests: the classification table (incl. the pending-network family), the reclaim-choice + pacing
unit, and the out-of-process E2E under a lowered `RLIMIT_NOFILE`. Both the E2E and the fail-open
guard are mutation-checked (reclassifying `EMFILE` as fatal fails the E2E; a renamed test trips the
guard).

**A pre-existing test hang, diagnosed and fixed here (and a lesson about recording a guess).** A
runtime-suite run wedged during this arc; the first write-up in this file called it unexplained,
unreproducible in 13 runs, and "most likely the known port/`SO_REUSEPORT` collision class". **All
three were wrong** — the third adversarial round reproduced it 4 times in 15 runs **on `main`** and
found the actual cause: `http_server_rejects_malformed_then_keeps_serving` spawned its malformed
client and its good client as two threads, so the SYN arrival order was a race; when the good one
landed first, the test's single `accept` served it and the malformed connection was never accepted
or closed, leaving that client's `read_to_end` blocked forever. Fixed at the root: `raw_http_client`
now connects and sends **on the calling thread** (a loopback `connect` completes the handshake
before returning, so arrival order follows call order) and only the read waits on a thread. Measured
A/B on this box, running the test binary directly with the `http_` filter 15 times each: **5/15 runs
hung with the old helper, 0/15 with the new one** (an independent reproduction on the same tree hung
4/15 — same conclusion, different sample). A guess written into the record is worse than no record — it
costs the next reader a wrong prior.

- **The 4.1 µs protocol path — first attempt made, REVERTED, and the method matters more than the
  attempt.** Align does exactly one syscall more than the floor (`poll` before the read), so the
  first cut was a speculative non-blocking `recv` on the most-recently-served connection, falling
  back to `poll` on a miss. It cannot win at `CONNS=1` by construction (a synchronous client has not
  sent yet, so the read always misses and costs an extra syscall), and under load the harness could
  not resolve it: adjacent A/B at 32 workers gave with 371.8k, without 392.5k, with-again 437.9k —
  **the same build varies 18% run to run**, which swamps a 5% effect. Reverted rather than shipped
  unproven; the negative result and both lessons are in `bench/web_e2e/README.md`. **The lessons:**
  (a) `CONNS=1` ping-pong is the tool for protocol-path work on this box — stable to ~1% where
  throughput moves 18%; (b) what is actually left in the 4.1 µs is ALLOCATION, not the syscall —
  `http_read_request` starts a fresh `Vec` per request, the header spans another, the builder holds
  `String`s, and the response serializes into a third. Four-plus allocations on a 4.1 µs budget, and
  the CONNS=1 harness can price each.
- **W7: DONE.** Same box, same generator, same request, **both sides prefork** (Fiber's own
  throughput recommendation and the analogue of `serve(..., workers)`): **pkg.web 491,505 req/s vs
  Fiber-prefork 374,393 — 1.31×**, 1.19× on single-connection latency (70.2 vs 81.2 µs), 2.5× at the
  p99 tail (210 vs 533 µs); 5.9× Go `net/http`. In protocol-path terms against the minimal-Rust
  floor: **Align 4.1 µs/req, Fiber 17.9 µs, net/http 37.7 µs.** Fiber's DEFAULT (non-prefork) mode
  is 4.3× slower than its own prefork mode — worth knowing before reading anyone's Fiber numbers.
  Caveats live with the numbers in `bench/web_e2e/README.md`: our own generator (no `wrk`/`oha` on
  this box, held identically against all five servers), WSL2 loopback inflating the 63 µs floor,
  32 connections is a small load, neither side tuned. Go 1.26.5 / Fiber v2 (`bench/web_e2e/go`,
  `-stdlib` and `-prefork` switch the mode).

(#596 merged: the hoist, the dispatch chains and `bench/web_router`.) (`bench/web_router` + `bench/web_e2e`, keep-alive'd,
`workers = process.cpu_count()`), then W7 Fiber. **W5 IS ALREADY PAYING OFF — a first measurement
taken 2026-07-21 says the dispatch path violates performance-contract item 3 and must be fixed
BEFORE the bench is meaningful:**

- **Measured, then FIXED (the hoist is DONE — 2026-07-21).** Before: 1319 ns/op over a 6-route
  table, 708 ns/op over a 2-route one (release runtime, best of 7 × 200k dispatches) — cost scaling
  with the number of ROUTES, the signature of the per-call radix build. After hoisting the build
  out of the request path: **1319 → 57 ns/op**, a **23× drop**. Then the two O(table) scans inside
  dispatch went too (per-node edge chains + a same-CLAIM route chain, both built once). **Item 3's
  ideal is still NOT met**, but `bench/web_router` now reports and gates the slope honestly (6-route
  vs 128-route). **Read its README before touching this.** The old row conflated three variables:
  the small table's static path was 2 segments and the large one's 3, and each shape landed at a
  different chain position. #614 replaced it with identical runtime paths at identical depths,
  reporting static/param at both chain ends and using median adjacent AB/BA ratios. Local head is
  1.04–1.13×; the honest remaining lever is the **per-node sibling scan** at local 1.84–2.17× on the tail:
  a node's static edges are a linked chain
  with a string compare each, so a miss on a flat 128-route namespace still costs ~0.4 µs and the
  per-route slope is UNCHANGED by the chains (the chain IS the node's children). Wants a first-byte
  bucket or a sorted edge run. Second lever: the per-edge `Route` struct copy forced by
  `routes[i].pattern` being rejected through a `slice<struct>` (compiler-side). **One regression was
  caught there and fixed:** edges were appended at the chain HEAD, making first-registered routes
  the LAST candidate — `/r0` in a flat 128-route namespace went 23.7 → 394.5 ns/op (16.6× slower)
  before the tail-append fix. Shape: `router.build_tree(routes)` returns ONE flat
  `array<i64>` (header + ELEVEN contiguous columns — the soa/tape/offset-table move),
  `worker` builds it once before the accept loop, and `dispatch_routes` / `method_not_allowed` /
  `allow_methods` / `tree_best_path` all take that borrowed tree. `best_path_route(routes, path)`
  survives as the build-and-match convenience the differential oracle tests use.
- **The bench shape is probed and works** — this is the awkward part, so don't re-derive it:
  `dispatch_routes` is an INTERNAL module, so neither `main` nor a bench module outside `pkg.web`
  may import it (D7). The working arrangement is a **window module in the BENCH TREE ONLY**,
  `pkg/web/bench.align` (`module pkg.web.bench`, one `pub fn dispatch(routes, method, path)`
  forwarding to `router.dispatch_routes`) — legal because D7 admits any importer under `pkg.web.`
  — plus a `main`-unit kernel that builds the table with the public constructors and loops.
  `alignc emit-obj kernel.align --export dispatch_bench` then writes one object per unit
  (`--export` is entry-unit only, which is why the kernel, not the window module, holds the
  exported fn); link all six objects + `libalign_runtime.so`. The shipped package stays clean —
  `run.sh` copies `apps/web/pkg/**` into the bench tree and drops the window module in.
  A scratch reproduction lived at `scratchpad/wr/` (gone with the session; the recipe above is
  the durable part). **Process note worth keeping:** the arc was
reviewed THREE times (the pre-PR checklist, then two independent adversarial passes) and each pass
found real defects in the previous one's output — round 2 found seven, including a pre-existing
`accept` contract bug that let one malformed request kill a pkg.web server. Review the FIXES, not
just the feature. The route-tree edge matrix shipped in #615 and malformed-request matrix in #616;
handler-`Err` logging then closed W4 in #617. Earlier context follows._

_Previously: **pkg.web: F1 + F0 + W1 COMPLETE, W2 ROUTING COMPLETE; streaming
ENABLERS 1–5 ALL COMPLETE (#593); W4 HARDENING SLICE 1 COMPLETE (#594).** #593 = the pkg.web
streaming wiring (`Handler` Respond/Stream sum type in the ONE route table, `web.sse`/`web.stream`,
serve's stream arm, std.http `s.send_event` — surface revised from the sketched
`web.send_event(s, …)` free fn, which cannot borrow a Move handle) + TWO paired MoveCheck fixes
(the loop-back-edge false positive on match-arm bindings, and the adversarial-review hole it
unshielded: extracting a Move payload now marks the Local scrutinee moved). #594 = startup
route-table validation (10 abort diagnoses incl. the Stream-row empty-content-type invariant
guard), RFC-9110 §9.3.2 HEAD (std.http respond suppresses the body keeping CL; serve routes
HEAD→GET for Respond rows), and the fixed minimal JSON 404/405/500 bodies. **NEXT: concurrent serve + server-side keepalive —
DESIGNED 2026-07-21 (this session), implementation is the next session's work.** The design:
`pkg-design/web.md` → "Concurrent serve (prefork) + connection keep-alive" and
`std-design/http.md` item 9. Shape: `serve(host, port, routes, workers)` (outright signature
change; `workers == 1` = today's loop inline, `>= 2` = `task_group`-spawned prefork workers,
EACH owning its own `SO_REUSEPORT` listener — no shared Move handle, no locks); keep-alive lives
entirely in std.http (`accept`/`respond` semantics: per-server parked slot, poll prefer-parked
with new-traffic eviction so NO idle timeout, eligibility = 1.1 + no `Connection: close` + no
residual bytes, Arc'd slot for ctx/server drop-order safety; `respond_stream`/`reject` keep
close-always). **PROBED: the pkg-side prefork shape needs NO compiler enabler** — Impure spawn
bodies, loop-spawned workers capturing the `slice<Route>` view + fn-field indirect calls on real
threads all run correctly today. **Implementation order: ① `http.serve_shared` (sibling op) →
② std.http keep-alive (testable on the sequential loop) → ③ pkg.web prefork (serve signature,
all call sites outright) → ④ W5 bench gate (keep-alive'd, `workers = cores`) → Fiber (W7).**
The route-tree edge matrix shipped in #615 and malformed-request matrix in #616.
(Housekeeping, found while mirroring: `ja/http.md` has pre-existing drift — item 7's SSE
streaming body was never mirrored and the recent items use an unnumbered-bold style; items 8–9
followed the local style. A ja/http.md re-sync pass is owed, separate from any feature work.) Earlier context: #589 (`http_stream`
nameable, fn value as enum variant payload via new `Scalar::Fn`, indirect-call Move-param
soundness; + a capturing-closure frame-escape UAF fixed at `region_of(Closure)`) and the std.http
`respond_stream` rework (http.md item 8 ①–③)._

## Where pkg.web stands

Shipped Align source (all compiled by driver tests via `include_str!`, so the repo files ARE the
tested source):

```text
apps/web/pkg/web/types.align            Ctx (Move, owns http_request_ctx) + Route (Copy: method,
                                        pattern, fn(Ctx) -> Result<(), Error>). A dependency-free
                                        PUBLIC leaf — the internal router needs Route and the root
                                        needs both, so neither can define them (import cycle), and
                                        Align has no re-exports (D9).
apps/web/pkg/web/internal/router.align  seg_end / match_score / dispatch (linear oracle) /
                                        best_path_route (flat SoA radix tree built over the ROUTE
                                        TABLE + backtracking tree_walk, differential-tested) /
                                        param_value (zero-copy :param + *wildcard capture) /
                                        dispatch_routes + method_not_allowed (method-aware, 404 vs 405)
apps/web/pkg/web/internal/query.align   raw / has / key_matches — zero-alloc query lookup; an ESCAPED
                                        key matches by decoding as the comparison walks
apps/web/pkg/web/cookie.align           get / build (+CookieOpts) — RFC 6265; build REJECTS header &
                                        attribute injection (Result)
apps/web/pkg/web/cors.align             CorsPolicy / valid / allow_origin / vary_origin /
                                        method_allowed — exact-origin match, wildcard+credentials
                                        rejected
apps/web/pkg/web/multipart.align        boundary / next (+Part, Step) — RFC 7578 upload walk, zero
                                        allocation and zero copy; a PUBLIC sibling module, unwired
                                        from the core surface (a body codec, not routing)
apps/jwt/pkg/jwt.align                  encode_hs256 / decode_hs256 / time_claims_valid — alg-pinned,
                                        constant-time compare, byte-identical to the jwt.io vector
```

Driver tests: `apps_web_router.rs` (7), `apps_web_serve.rs` (3, real socket), `apps_web_query.rs` (3),
`apps_web_cookie.rs` (3), `apps_web_cors.rs` (3), `apps_web_multipart.rs` (16), `apps_jwt.rs` (3),
`pkg_foundation.rs` (6),
`struct_handle_fields.rs` (9), `struct_slice_fields.rs` (5), `fn_values.rs` (21).

**The designed handler contract runs end-to-end over a real socket** (`apps_web_serve.rs`): radix
dispatch -> `Ctx` owning the request handle -> `param_value` zero-copy capture -> the handler invoked
THROUGH the `Route.handler` fn-value field -> a responder consuming the handle out of that field.
Nothing there stands in for a missing compiler feature.

## NEXT (in order)

0. **DONE (2026-07-22, PR #620) — the owned-`string` template defect** (found while writing
   `pkg.web.multipart`; it needed its own PR). `template "{h}"` on an owned `string` panicked
   codegen with `Found StructValue { … llvm_type: "{ ptr, i64 }" } but expected the IntValue
   variant`; a `str` interpolated fine.

   **Root cause:** MIR's template-hole lowering classified the hole with its own catch-all —
   `Ty::Str => StrHole, Ty::Bool => …, _ => IntHole` — while sema's `is_printable` accepted
   `Ty::String`. The one printable type the match forgot therefore became "an integer", and codegen
   asked a `{ptr,i64}` aggregate for its `IntValue`. Two independent classifications of the same
   fact, and the wildcard failed OPEN. (The lowering never even reached an ownership question.)

   **Fix — the ideal form, one string display, not two:** an owned `string` is *borrowed* to a `str`
   view in sema (`check_print_operand` → the existing `ExprKind::StrBorrow`, the same borrow
   `b.write(s)` / a `str` argument / `s == t` already apply), so `template "{h}"` neither moves nor
   frees `h` — `h` stays usable and is dropped exactly once, at scope end; an interpolated *temporary*
   gets MIR's hidden owner and is freed on every loop edge. `print`'s argument takes the identical
   borrow, so the two display sites share one mechanism (its lowered MIR is byte-identical — verified
   by diffing `emit-mir` before/after). The fail-open catch-all is gone: sema now exposes a single
   `print_kind(ty) -> Option<PrintKind>` classification, `is_printable` is defined from it, and MIR
   picks the `TemplatePiece` from it — adding a printable type forces every consumer to handle it.
   Codegen's display accessors (`display_int` / `display_float` / `display_view`, plus the
   `json.encode` `OptionField` payload arms) now return a `CodegenError` instead of an inkwell panic
   on a shape mismatch.

   **Sibling sweep:** every other Move/aggregate kind that can reach a hole — `array<T>`, `slice<T>`,
   `buffer`, `builder`, `array_builder<T>`, `reader`, struct, tuple, `Option<T>`, `enum`, `()`,
   fn-value — is a real sema diagnostic ("a template hole must be an int, float, str, bool, or
   char, got X"), never a codegen panic, and `print`'s path is gated by the same classification.
   `json.encode` of a `string` field / an `Option<Move>` field was already rejected at the
   struct-field boundary. Tests: `m5.rs` (`template_hole_takes_an_owned_string`,
   `owned_string_interpolates_in_every_position` — direct `print(template …)`, a bound
   `let s := template …`, `builder.write(template …)`, inside a `loop`, inside a `match` arm, and
   inside functions that afterwards move / return the interpolated value) and `template_ownership.rs`
   (borrow-and-drop-once MIR evidence, per-iteration temporary free, the 16-case
   diagnostic-not-panic matrix). **Mutation-checked, with the adversarial review's corrections:**
   reverting sema + MIR together fails all four owned-`string` rows (with the codegen hardening
   still in, as `CodegenError: an int template hole expects an integer operand, got StructValue(…)`;
   revert that hunk too and it is the original `expected the IntValue variant` panic). Reverting
   **only** the sema borrow fails exactly one row workspace-wide —
   `an_owned_string_hole_temporary_is_freed_each_iteration` — because MIR then classifies the raw
   `Ty::String` as a byte view (no panic) but no hidden owner is minted and the interpolated
   temporary leaks. Reverting **only** the MIR half is NOT test-visible: sema hands MIR a `Ty::Str`,
   so the old `_ => IntHole` catch-all never sees a `string` again. Its value is a **compile-time**
   guarantee, not a runtime-pinned one: adding a `PrintKind` variant breaks the MIR match, which is
   exactly the fail-open this defect came from. The independent adversarial review measured the
   borrow non-consuming across temporaries, moves, loops, struct fields, array elements, `match`
   arms, arenas, `?` paths, drop-flag branches, `par_map` and `spawn` (LD_PRELOAD malloc counter at
   two iteration counts), and confirmed `emit-mir` over all 213 repo `.align` files is byte-identical
   to `main`.

   **Two follow-ups this PR does NOT fix (separate work, both confirmed by that review):**

   0a. **DONE (2026-07-23, PR #621) — SOUNDNESS, pre-existing on `main`, HIGH: a `str` bound from a
   `template` escaped the hidden owner's loop-edge free, and sema accepted it — and the fail-open
   tail that let it through hid TWO MORE of the same bug.** Repro: `mut acc: str := "start"` then
   `loop { acc = template "{acc}-{c}"; c = c + 1; if c >= 5 { break } }` then `print(acc)` →
   `alignc check` reported ok, and the binary read freed memory (the debug runtime trips
   `ptr::copy_nonoverlapping`'s precondition; a release build printed garbage).

   **Confirmed cause, and the part the original write-up got only half right.** The reported half is
   real: `borrow_sources_inner` had no `ExprKind::Template` arm and a fail-open
   `_ => BorrowRoots::new()` tail. But adding an arm is not enough to *know* what to put in it, and
   the deeper reason the machinery could not see a template is that **`template` is the one
   expression whose value views storage the expression itself allocates**: MIR mints a hidden owned
   `string` at the node (`b.arenas.is_empty().then(|| b.new_synthetic_owner(Ty::String))`) while the
   value's own type is `str`. `MoveCheck::temp_owner_root` keys on `needs_drop_flag(e.ty)` — and a
   `str` is not droppable — so the existing hidden-owner machinery was structurally blind to it. It
   is also the one hidden owner minted **unconditionally** rather than only in a borrowing position,
   which is why the root belongs in `borrow_sources` and not in `storage_roots` (where every other
   temp root lives): plainly assigning the `str` already depends on it.

   **Fix.** A shared `owns_hidden_string(e, in_arena)` in sema is now the single definition of that
   fact, and MIR's Template lowering calls it — the same single-sourcing pattern as
   `may_need_synthetic_owner` / `borrow_transparent_value` / `print_kind`. `MoveCheck` grew an
   `arena_depth` counter mirroring MIR's `arenas` stack, because inside an `arena {}` no owner is
   minted (the bytes are bump-allocated and outlive the loop) — **without that mirror the idiomatic
   arena-scoped accumulator, which is the correct way to write the rejected loop, would have been
   rejected too.** Diagnostics are the existing ones verbatim, no new wording.

   **The sweep found two more holes, both fixed and pinned here.** ① **`ExprKind::EnumValue`** — the
   exact sibling of `StructLit` / `Tuple` / `OptionSome`, all of which forward their operands'
   provenance. It was the one aggregate constructor the `_` tail swallowed, so `keep = C.Text(h.trim())`
   over a loop-local `string` `h` laundered the borrow and printed garbage. ② **`ExprKind::RandSample`**
   — `r.sample(xs, k)` copies element *values* into a fresh owned array, and with `slice<str>` elements
   those values are views; it is the `.to_array()` shape, which does forward, and it did not.
   `json.encode` needed no rule of its own: it desugars to `Template`, so it inherits the fix (pinned).

   **The tail itself is closed, not just its instances.** `borrow_sources_inner` is now **exhaustive
   over all 216 `ExprKind` variants with no `_` arm** — verified compiler-forced by deleting one
   variant from the list and getting `E0004 non-exhaustive patterns`. The 145 previously-swallowed
   variants are classified in two commented groups: borrow-capable results whose storage this frame
   cannot free early (rodata `Str`/`ConstArray`, an environment-less `FnValue`, a `Reader`/`Writer`
   that owns its fd, `fs.read_*_view`'s arena-bound mmap, `OptionNone`, and `Loop` — whose value is
   provably `Static` because `check_break_escape` rejects breaking a view of local storage out of a
   loop, the reason `Loop` was NOT a fourth hole), and results whose type never borrows at all. Two
   are called out as one-rule-change away from becoming holes: `ArrayBuilderBuild` (sema restricts
   `array_builder<T>` to Copy scalars and owned `string`) and `RandShuffle`.

   **Over-rejection risk, and how it was bounded.** The only widening is "a `str` from a template
   inside a loop dies on that loop's edges", so the exposure is templates, sum-type constructors and
   `rand.sample`. Bounded three ways: (a) `emit-mir` over all 213 repo `.align` files is
   **byte-identical** to `main` and `alignc check` over the same corpus is verdict- and
   message-identical; (b) a legal-cases battery of eight shapes is pinned as a test — same-iteration
   bind-and-use, outside any loop, literal-only holes, `t := template "{h}"` followed by `h = …` (a
   template is a real COPY of its holes, so this stays legal), written into a builder, an outer
   loop's template read by an inner loop, an arena-scoped accumulator, an arena opened inside the
   loop — plus outer-source controls for the enum and sample rows and a runtime assertion that the
   arena accumulator really produces `start-0-1-2-3-4`; (c) the whole workspace suite is green
   (2658 passed, 0 failed) with zero pre-existing tests flipped.

   **Mutation evidence** (each reverted alone, rebuilt, run): dropping the `Template` arm fails
   exactly the three template/`json.encode` rows; dropping the `EnumValue` arm fails exactly the
   sum-type row; dropping the `RandSample` arm fails exactly the sample row; **dropping the arena
   half of `owns_hidden_string` fails the legal-cases row**, which is what pins the guard against
   over-rejection. Tests live with the other borrow-liveness rows in
   `crates/align_driver/tests/borrow_liveness.rs` (45 there now).

   **Blast radius, wider than the repro suggests.** Because the provenance is on the *node*, every
   way of laundering that `str` onward inherits it — a tuple, `Some`/`Ok`, an `if`/`match` value, a
   match-payload binding, a struct literal or a later field / tuple-index read of one,
   destructuring, `else`-unwrap, a nested template, `.trim()`, slicing, `.bytes()`. All of those
   checked ok on `main` and genuinely read freed heap (confirmed under `MALLOC_PERTURB_`). The
   struct-field spelling is pinned as the representative row.

   **The independent adversarial review found no soundness defect and no over-rejection** — it
   independently reproduced the mutation matrix, the 213-file `emit-mir` byte-identity (200 of the
   213 actually `check` ok, so it is real coverage), and the suite/clippy state. Four hardening items
   came out of it and are in the PR: ① the ~130 "type never borrows" arms are justified by a *type*
   fact that, unlike the variant list, is **not** compiler-forced — so they now end in a
   `debug_assert!(!ty_may_borrow(…))`, converting "this result became borrow-capable" from silence
   into a test failure. Verified free: replacing that arm's body with `assert!(false)` leaves the
   whole workspace suite green (2660) and all 213 corpus files panic-free, i.e. those arms are
   reached **zero** times today. ② `MoveCheck::arena_depth` counts `arena` **only**, deliberately
   unlike the two identically-named region counters in the same file, which also count `task_group`
   — MIR keeps task groups on a stack separate from `Builder::arenas`, so a template inside one
   still dies on the enclosing loop's edge. A future "harmonization" would silently re-open the UAF
   with only the region rule left to catch it, so the field documents the difference and a
   `task_group` row pins the borrow half (mutation-checked: harmonizing the counters fails exactly
   that row). ③ `borrow_sources` recurses through an `Arena` node without entering `arena_depth` —
   the one place the sema/MIR mirror is not lexical; it errs strict and the escape check covers
   those programs, and the arm now says so, so the next reader does not "fix" it in the unsound
   direction. ④ the blast-radius row above.

   **The diagnostic wording changed, once, for everyone.** The reused temporary message said only
   "bind the owned value to a local declared outside the loop" — advice a template user cannot
   follow, since its owner is hidden and there is no owned value to bind. It now names all three
   generic escapes (bind outside the loop / `.clone()` into a local / allocate in an enclosing
   `arena`); still one message per fact, still not template-specific.

   **The reusable lesson**, and it is the same one twice: a `_` arm in a *classification* is a
   soundness hole even when every arm above it is right, because the rules that consume it can only
   act on what it reports. #620 removed one from MIR's display classification; this removes the one
   in sema's borrow classification — and removing it is what surfaced `EnumValue` and `RandSample`,
   neither of which anyone had reported. The review's item ① is the same lesson a level up: an
   exhaustive *variant* list still leaves the *justification* fail-open unless something asserts it.

   0b. **DONE (2026-07-23) — the asymmetric codegen panic hardening, finished; and the sweep found
   the class was LIVE.** #620 hardened the template display pieces only. Three named gaps plus a
   sweep of `align_codegen_llvm` for the same class:

   ① **`gen_print` shared nothing with the template path.** It dispatched on `ty` with a **catch-all
   integer tail**, so `print(array)` / `print(struct)` / `print(u())` fell through to
   `self.operand(arg).into_int_value()` and aborted the compiler with inkwell's `Found ArrayValue {
   … } but expected the IntValue variant` if the sema gate were ever bypassed. It now matches on
   `align_sema::print_kind(ty)` — the SAME classification sema's `print`/hole check and MIR's
   `TemplatePiece` selection use — with `None` a `CodegenError`, and reads the operand through the
   same `display_int` / `display_float` / `display_view` accessors a hole does. **One mechanism for
   both display sites**, and because the `match kind` is exhaustive, adding a `PrintKind` variant now
   breaks codegen too, not just sema and MIR.

   ② **`Operand::Value(id) => self.values[id]` was a direct map index.** A `()`-returning call
   defines a `ValueId` whose LLVM call yields **void**, so nothing is recorded for it, and reading it
   aborted — *before* reaching any `display_*` accessor, which is why `template "{u()}"` escaped
   #620's hardening even on the template path. `operand` is now fallible; its sibling
   `Operand::Arg(i)`'s `expect("param index in range")` went with it. That made **299 call sites**
   take `?`; three closures were rewritten to loops / `collect::<Result<_,_>>()`. No behavior change
   (see the IR evidence below).

   ③ **`json.encode`'s unrenderable field domain had no sema gate at all — and it was reachable.**
   The struct-field declaration gate only rejects an **owned** (Move) `Option` payload / `array`
   element, so every non-Move-but-unrenderable shape walked straight into codegen. Measured on
   `main`, and the three outcomes differ — worth stating precisely: `S { a: Option<C> }` with
   `C { R, G, B }` produced a bare `CodegenError` with nothing behind it; **`S { xs: array<char> }`
   aborted the compiler** (`json_payload_tag_sub`'s `unreachable!`); `S { xs: array<C> }` aborted
   only for a **payload-less** enum (`emit_json_union`'s `expect("union variant carries exactly one
   payload")`) and otherwise **printed `{"xs":[null,null]}` — silently wrong output**, because
   `ScalarArrayField` discards the `sub` pointer `json_payload_tag_sub` returns and hands the runtime
   kind 6 with a null sub. None had ever been reported. Sema now names the encodable domain itself:
   one predicate `json_encodable_scalar` (int / float / bool / `str`) serves **both** an `Option<T>`
   payload and an `array<T>` element, and an `Option<Struct>` / `array<Struct>` element is validated
   by the shared descriptor walk **in the encode direction** (below). The two new diagnostics name
   what an encodable field may be. Deliberately an allow-list, not an exhaustive match: this
   classification fails **closed** (an unlisted scalar is a diagnostic, never a wrong rendering), the
   opposite of the `_ => IntHole` tail that caused #620 — the doc comment says so. `Option<char>` is
   rejected too, matching the required-`char` field rule; it used to encode as a **number**.

   ④ **The gate must match its own encoder — so the shared decode/encode walk is direction-aware.**
   `emit_desc_table` is used by BOTH directions, so the first cut validated `array<Struct>` /
   `Option<struct>` encode fields with `decode_struct_fields_ok`. That is **strictly narrower than
   the encoder it guards**: `json_payload_tag_sub` renders an `Option<enum>` field (kind 6 with a
   real `JsonUnion` sub, written by the runtime's union encoder), while the decoder has no rule for
   an optional union. The walk now takes a `JsonDir` (`Decode`/`Encode`) — one walk, two domains,
   with the single asymmetry commented at the arm that carries it. Two things fall out: an
   encode-only program no longer reports `'json.decode' field '…'`, and the pre-existing
   decode-shaped gate on `Option<struct>` encode fields is fixed too, so
   `S { a: Option<N> }` + `N { c: Option<C> }` + `json.encode(s)` — **rejected on `main`** — now
   compiles and prints `{"a":{"c":7}}`.

   **The sweep, and what was deliberately left.** The three descriptor emitters behind the new gate
   (`json_payload_tag_sub`, `emit_json_subtable`, `emit_desc_table`, `emit_json_union`,
   `decode_field_table`) now return `CodegenError` — defense in depth on the two sites that were
   aborting minutes earlier. Left as compiler-internal single-pass invariants, and they are: the
   ~300 remaining `into_*_value()` reads, whose LLVM shape is fixed by the MIR opcode being lowered
   (`gen_float_bin`'s operands are floats because the `Rvalue` is a float binop), not by a
   type-classification that a sema gate could get wrong; the `get_nth_param(k).unwrap()`s, which read
   a thunk/trampoline codegen itself just created with a known signature; `self.funcs[…]` /
   `self.slots[…]` / `self.struct_types[…]`, keyed by names and ids the declare pass populated from
   the same MIR. The distinguishing test used throughout: **does the shape come from a
   classification, or from the opcode?** Only the former can be wrong on user input.

   **Evidence.** Regression rows in the two existing homes: `align_codegen_llvm`'s `mod tests` drives
   hand-built MIR past the sema gate (the technique the allocation-shape probes already use) —
   `print_of_an_unprintable_value_is_an_error_not_a_panic` (array / struct / slice),
   `a_unit_valued_operand_is_an_error_not_a_panic`,
   `an_out_of_range_argument_operand_is_an_error_not_a_panic`,
   `an_unrenderable_json_descriptor_payload_is_an_error_not_a_panic` (`array<char>` and a
   payload-less union); `template_ownership.rs` pins the sema half —
   `an_unencodable_json_field_is_a_diagnostic_not_a_codegen_panic` (6 rows, incl. the payload-carrying
   `array<enum>` that printed nulls and the `array<Struct>` element) plus the over-rejection control
   `every_encodable_json_field_shape_still_encodes`, which now runs THREE programs — the leaf shapes,
   an `array<Struct>` decode→encode round trip, and the `Option<enum>` widening — and asserts the
   real runtime JSON for each, plus a check-only acceptance row for an `Option<enum>` inside an
   `array<Struct>` element (no runnable form exists). **Every row mutation-checked** (reverted alone,
   rebuilt, run): restoring the `print` catch-all + raw `into_int_value` reproduces the original
   inkwell panic verbatim; restoring `self.values[id]` / the `Arg` `expect` / the `unreachable!` /
   the union `expect` each fails exactly its row; deleting either leaf gate fails exactly its rows;
   deleting the `array<Struct>` gate fails the element row; flipping EITHER descriptor call site to
   `JsonDir::Decode` fails the control; deleting the encode-only `Option<enum>` arm fails the
   control; and **tightening** the leaf gate (dropping `Scalar::Str`) fails the control. Workspace
   2666 passed / 0 failed, clippy clean. Under `ALIGNC_CACHE=off`, `alignc check` over all 116 repo
   `.align` files is verdict-identical to `main`, and `emit-llvm` over the 108 that check ok is
   byte-identical for 103 — the 5 that differ are **identical after normalizing SSA names**
   (`%sptr`/`%slen` → `%p`/`%l` at the `print(str)` site, from the now-shared `display_view`),
   verified by hashing the normalized text.

   0c. **NOT this PR's crate, but found by its sweep: `align_mir` panics on `structArray.to_array()`.**
   `E { c: i64 }` then `[E { c: 1 }].to_array()` → `to_array/scan needs a scalar element`
   (`crates/align_mir/src/lib.rs`, the `cur.expect(...)` at the append point). Sema accepts it —
   `check_array_to_array` explicitly supports a Copy-struct element (`Ty::DynStructArray`) — so this
   is an unimplemented lowering, not a rejected program: the identity collect is the ONLY broken
   shape, because `map` / `where` load a whole struct element by index and this path never does.
   `[E{..}].map(f).to_array()` works. The fix is to load the element the same way at the append
   point (the `lower_struct_elem` call the two stages already make), which is an *implementation*,
   not hardening — left out of a Gate-3 PR on purpose.

   0d. **A `()`-valued binding is a MIR LOWERING hole, not an intended rejection — and the Gate-3
   shape survives there.** These all pass `alignc check`, **abort** the compiler on `main`
   (`self.values[id]`), and on this branch surface as `value %0 has no LLVM value` with **no source
   span**: `x := u()`, `fn v() { return u() }`, `g(u())` with `g(a: ())`, `x := { u() }`,
   `x := arena { u() }`. The same programs written with `if` / `match` / `loop { break u() }` compile
   fine, which is what makes it a lowering gap rather than a language rule — MIR records no LLVM
   value for a void call, so any *direct* use of that `ValueId` has nothing to read. Only
   `print(u())` and `template "{u()}"` are genuinely sema-rejected. So the PR's hardening turned an
   abort into an error, but the error is user-reachable and span-less: the real fix is for MIR to
   lower a `()`-valued use to `Const::Unit` (or for sema to reject binding `()`), and that is a
   design call, not hardening. Recorded, not fixed.

   0e. **The codegen cache can mask a cold-build failure.** Build a `Bag`-shaped unit with an
   `array<i64>` element, then an identical unit whose element struct has a `char` field: the second
   reports `cache: main hit` and "built executable", while the same build **cold** panics (on `main`)
   in `json_payload_tag_sub`. The unit hash is taken after the unreachable function is pruned, but
   the cold path still IR-gens it. Not a miscompile — the objects agree after DCE — but build
   success depends on cache state, which silently masked three rows in an independent review's first
   pass. **Any `main`-vs-branch comparison must run with `ALIGNC_CACHE=off`.** Also latent, and in
   the same area: `ScalarArrayField` discards the `sub` pointer `json_payload_tag_sub` returns, so an
   `array<enum>` cannot be rendered at all — encoding one properly (pass `sub`, teach
   `json_encode_scalar_array`) is a feature, not hardening.

1. **`pkg.web` root + `serve()` — DONE, then rebuilt on the settled ownership model.** `apps/web/pkg/web.align` is real: per-method constructors
   (`get`/`post`/`put`/`delete`/`patch`/`head`/`options`/`any` over a shared `route()`), the
   responders `status` / `text` / `status_text` / `json` / `status_json` (each CONSUMES `Ctx`), the
   `param` passthrough, and `serve(host, port, routes)` owning the accept loop with automatic 404 and
   405 + `Allow` (new `router.allow_methods`, over a factored-out `router.best_path_route` that
   `dispatch_routes` / `method_not_allowed` / `allow_methods` now share). Pinned by
   `crates/align_driver/tests/apps_web_root.rs` (2 tests, real socket, server stays up across
   requests). The ownership question this raised is SETTLED — see below; the surface described here has since
   been rebuilt on it (`Ctx` Copy, handlers return a built response).
2. **Wire the radix tree over a route table — DONE (2026-07-21, #591).** `best_path_route` is now
   the SoA radix build reading `routes[r].pattern` directly (edge labels = zero-copy
   `(route, start, end)` triples) + the recursive **backtracking** `tree_walk` (matchit semantics),
   so `dispatch_routes` / `method_not_allowed` / `allow_methods` all go through the tree; the old
   linear scan survives as `best_path_route_linear`, the differential oracle. **The walk backtracks
   by design decision (settled by the #591 adversarial review):** the earlier no-backtracking walk
   silently 404'd `{/a/featured, /a/:id/versions}` on `/a/featured/versions` — a set the linear
   scan (production before the tree) matched — and the once-planned build-time ambiguity abort
   would have rejected that realistic table outright. **A second review finding then fixed the
   ORACLE:** the original `match_score` fold compared MAGNITUDES (a wildcard match stayed
   un-shifted), so a deep param chain outranked a static-prefix wildcard — `/:cat/:slug` beat
   `/assets/*file` on `/assets/logo`, against the httprouter/matchit/Fiber reference AND the
   fold's own documented left-to-right intent. `match_score` is now **fixed-width base-3,
   left-aligned to the path's segment count** (wildcard = 0 at its position, absorbed positions
   zero-filled → the folded prefix shifts by `3^(D-k)`), making it genuinely lexicographic;
   first-success == max `match_score` now holds for EVERY table (the re-reviewer's 154-case fuzz
   found the mismatch class; its fixtures are pinned), so no ambiguity abort is needed
   (duplicate-route / param-name-conflict aborts remain future W4 work). The W1 `slice<str>`
   `tree_dispatch` was REMOVED outright (one walker, one semantics). Tests:
   `best_path_route_tree_agrees_with_the_linear_oracle` (backtracking paths + wildcard-vs-chain
   rows + same-pattern GET/POST rows + the empty table),
   `best_path_route_backtracks_from_a_static_dead_end`, and
   `static_prefix_wildcard_outranks_a_param_chain` (absolute indices both sides). The build is
   still per call; hoisting the columns into `serve`'s scope (build once, match per request over
   borrowed slices) is the remaining recorded follow-up.
3. **`param(c, name)` sugar — DONE; W3 body accessors — DONE (2026-07-21).** Settled by the
   Copy-`Ctx` redesign below; `web.param(c, "id")`, `web.query(c, name)` and `has_query` are shipped
   in the designed spelling. `web.body(c)` / `web.body_str(c)` are shipped too: `Ctx` gained a
   `body: slice<u8>` view field (filled once by `serve` from `ctx.body()`), `body_str` is
   `.as_str()` — pinned E2E in `apps_web_root.rs::body_and_body_str_read_the_request_body` (echo /
   empty-400 / invalid-UTF-8-400). **`web.header(c, name)` — DONE 2026-07-21**, on the std.http
   enabler `ctx.headers()` (`std-design/http.md` item 10): an arbitrary-name lookup cannot ride a
   single stored view, so `Ctx` carries the detached header-table view and `web.header` forwards to
   `c.headers.get(name)`. pkg.web duplicates no lookup.
4. Backlog (`docs/impl/15-pkg-web-plan.md` §2b is the committed list): multipart/form-data is DONE
   (#619, `pkg.web.multipart`); OAuth — client flows are buildable now, validating a PUBLIC
   provider's token is blocked on RS256 (std.crypto RSA verify over the already-linked libssl);
   JWT HS384/512.
5. W3-W7 per the design doc: accessor surface, hardening matrix, the bench gate (`bench/web_router`,
   `bench/web_e2e`), middleware-lite + SSE, the Fiber comparison.

## SETTLED (owner, 2026-07-20): the FRAMEWORK owns the request handle

The three open decisions raised by building the root were all one question, and it is answered.
`Ctx` is now a **Copy struct of views** (`method`, `path`, `query`, `pattern`) owning nothing; the
request handle stays in `serve`; a handler is `fn(Ctx) -> Result<response_builder, Error>` that
BUILDS a response and hands it back. All three dead ends dissolve:

- `web.param(c, "id")` / `web.query(c, name)` / `has_query` are the designed spelling now — a Copy
  `Ctx` is not consumed by an accessor, and the whole W3 accessor surface is unblocked.
- A handler reads a param and then responds, which the old shape rejected as a live borrow across
  a move.
- **`handler Err -> 500` works**, pinned over a real socket. `serve` still holds the connection when
  the handler declines.

Responders no longer take `c` and no longer respond — they build (`web.text(s)`, `web.json(body)`,
`web.status(code)`, `web.status_text`, `web.status_json`).

Compiler enabler shipped with it: **`response_builder` is now a nameable type and a legal
`Option`/`Result` payload** (it was neither — unspellable, and refused by `scalar_arg` outright on
the reasoning that no API would wrap one). Admitted on the same terms as `http_request_ctx`: a
payload, never an array/slice/box element. `Scalar::is_move` had to learn it too — omitting it there
classified the `Result` as Copy and leaked the payload silently (measured 17.5 MB at 200k dropped
values vs 162 MB at 2M). `crates/align_driver/tests/response_builder_payload.rs`.

Still open from that cluster, both unchanged by this decision:

- **`web.json(c, x)` cannot encode `x`** — no user-written generics, so the signature is
  `json(body: str)` and the handler writes `web.json(json.encode(m))`. Arguably better (the
  encode's allocation stays visible), and the design doc now says so.
- **Streaming/SSE — DESIGNED 2026-07-21 (owner-directed); COMPILER ENABLERS 1–3 DONE 2026-07-21.**
  The full design is `docs/impl/pkg-design/web.md` → "Streaming" (+ ja mirror): stream routes in the
  SAME table via `Handler { Respond(fn…), Stream(fn(Ctx, http_stream)…) }`, `web.sse`/`web.stream`
  constructors, framework-built lazy head, `s.reject(rb)` as the pre-stream 4xx window, and the
  ownership rule extended (framework owns ctx for the whole request; the pump owns only the stream).
  std.http side: `docs/impl/std-design/http.md` item 8 — `respond_stream` becomes a non-consuming
  receiver with a lazy head + `reject`. **Implementation order (enablers, all probed):**
  1. `http_stream` nameable (`resolve_type`, the exact #583 pattern) — **DONE** (it was already a full
     `Scalar`/`Ty`; only the source spelling was missing). `http_stream_nameable.rs`.
  2. fn value as enum variant payload — **DONE.** New `Scalar::Fn(u32)` (there was none); fn value is
     Copy `{fn_ptr,env_ptr}` (16 B), so a fn-only enum is non-Move / non-drop. Swept the #583
     checklist (`scalar_to_ty`, `sort_key_order` fail-closed, `scalar_bytes` unreachable, and the
     codegen `scalar_type` fn arm that reserves 16 B instead of the catch-all's silent `i32`).
     Construction compares fn payloads BY SIGNATURE, not `fn_types` id. `ty_to_scalar(Ty::Fn)` stays
     `None` (variant payload only). `fn_variant_payload.rs`. Generic fn-payload sum types stay
     rejected at the template resolver — deferred, no consumer.
  3. the indirect-call Move-handle-param verification — **DONE (via a Move-value proxy):** an owned
     `array<i64>` (http_stream stand-in) passed by value through a match-extracted fn payload nulls in
     the caller frame (#573), no double-free over a 200k loop; move-after-use rejected. Real
     `http_stream` receiver awaits enabler 4.
  4. the std.http `respond_stream` rework — **DONE (2026-07-21):** `respond_stream` consumes `rb`
     ONLY; the ctx stays with the caller SPENT (fd lifted; later `respond`/`respond_stream` → `Err`;
     a validation `Err` leaves it UNSPENT so the caller can still respond an error; views stay valid
     mid-pump — pinned by a `ctx.path()` send inside the stream). The head is LAZY
     (`HttpStream.pending_head`; the first `send`/`finish` write attempt takes it — head+chunk /
     head+terminator go out in one write). New `s.reject(rb)` (consumes both; pre-first-send only)
     answers with a complete normal response — the pre-stream 4xx window. New
     `ExprKind`/`Rvalue::HttpStreamReject` wired through every pass; `m12_http_stream.rs` replaced
     outright (13 tests incl. spent-ctx-Err, reject-400, late-reject-Err E2E) + runtime unit tests.
  5. pkg.web wiring — **DONE (2026-07-21):** `Handler { Respond(fn(Ctx) -> Result<response_builder,
     Error>), Stream(fn(Ctx, http_stream) -> Result<(), Error>) }` + `Route.stream_type` in
     `types.align` (Route stays Copy — a fn-only enum is Copy); `web.stream(method, pattern,
     content_type, pump)` + `web.sse(pattern, pump)` (GET, text/event-stream); serve's stream arm
     (framework builds the lazy head: Content-Type from the route + `Cache-Control: no-cache`,
     then `Ok(s) => pump(c, s) else {}`). **`send_event` shipped as the std.http METHOD
     `s.send_event(data)`** — the sketched `web.send_event(s, data)` free fn is unimplementable
     (a pkg free fn takes a Move handle by value; only std bound receivers borrow — the io.copy
     restriction class) and SSE framing was already a committed std.http floor item; WHATWG
     `data: {data}\n\n` as ONE write sharing the lazy-head buffer, `send_event("")` = a legal
     empty event (`HttpStreamSend` gained `event: bool` — same variant, no new-variant sweep;
     runtime `align_rt_http_stream_send_event` over the shared `http_stream_send_parts`).
     **MoveCheck fixes en route (two, paired):** ① a match-arm binding consumed inside a `loop`
     body was a false "use of moved value" on the back-edge fixpoint (arm bindings never
     `clear_moved` on (re)initialization, unlike `Let`) — exactly serve's `Ok(s) => pump(c, s)`;
     ② the #593 adversarial review then found ① had unshielded a PRE-EXISTING hole's loop form:
     extracting a MOVE payload never marked the Local scrutinee moved, so re-matching a consumed
     scrutinee (loop or plain double-match) was accepted and silently read the MIR-nulled payload
     (wrong result, no crash). Now the scrutinee is whole-moved in any arm that extracts a Move
     payload (Copy/tag-only arms leave it live; may-join across arms). Both directions
     regression-pinned in sema. Tests: `apps_web_stream.rs` (3 E2E: SSE frames + mid-pump
     `param`/`has_query`/`body`, reject 4xx window + loop survival, one-table 404/405-Allow with
     stream rows), `m12_http_stream.rs` (+1, now 14), runtime framing unit.
  **Hard ordering note: production streaming needs concurrent serve first** (an open stream starves
  the sequential v1 loop); the design is independent of it, the shipping is not. The middleware
  section was also rewritten for the settled ownership model (`Option<response_builder>` verdict —
  legal since #583).

### Bug found and fixed while doing this

**A query string 404'd every route.** `ctx.path()` returns the raw request-TARGET, query string
included, and the first `serve` matched routes against it directly — so `GET /v1/models?limit=5` did
not match the pattern `/v1/models`. Confirmed against the shipped #578 binary (200 without a query,
404 with one). `pkg.web.internal.query.target_path` / `target_query` now split the target once per
request; `apps_web_root.rs` pins it, including that a `:param` capture reads the path half only.

### Recorded gaps from the PR-578 review — W4 slice 1 SHIPPED 2026-07-21 closes most

- **Method-string validation — DONE.** `router.validate` (pure diagnosis) + `serve` startup abort
  (stderr + `process.abort()` before binding): known uppercase method or "" only; plus pattern
  well-formedness (leading `/`, named `:`/`*` segments, `*` tail-only, no param name twice) and
  the duplicate-claim aborts the design called for — same method twice, or any row after an
  any-method route on the same PATH CLAIM (param names don't affect a claim: `/a/:x` ≡ `/a/:y`).
  A Stream row must carry a non-empty `stream_type` (else a blank `Content-Type:` — and "" is the
  invariant the HEAD fallback reads as "a Respond row"). Specific-then-`any` stays legal.
  `apps_web_validate.rs` (9 aborts + the legal-shadow serve).
- **HEAD is RFC-correct — DONE.** std.http `respond` suppresses the body for a HEAD request at the
  protocol boundary (Content-Length kept, RFC 9110 §9.3.2), and `serve` routes HEAD-with-no-row to
  the path's GET handler (Respond rows only; stream-only GET keeps HEAD at 405).
- **`allow_methods` duplicates — DONE** by construction: the duplicate-route abort makes a
  same-method pair on one claim unrepresentable.
- **404/405 empty bodies — DONE.** The automatic 404/405/500 now carry the design's fixed minimal
  JSON bodies (`{"error":"not found"}` / `"method not allowed"` / `"internal error"`).
- **A handler's `Err` logging — DONE in the W4-closing PR.** Unary handlers and stream pumps log
  method/path/full builtin `Error` to stderr without changing their response or loop semantics.

Also shipped-with-a-caveat: `serve` returns an `accept` error rather than retrying — but that now
means only a genuine listener-level fault, since #597 classifies the transient errnos inside
`accept` itself (no errno needed to reach Align's `Error` after all). And `web.group(prefix, routes)`
is NOT shipped — it needs an `array_builder` over struct elements.

**Scope rule (owner, 2026-07-20 — also in Claude's memory):** `pkg.web` is a GENERAL REST/web
framework. The LLM gateway is only its first consumer and is NEVER a reason to omit or defer a
feature. Judge by "does a REST server commonly need this?", build it in the right layer.

## Compiler work this session (all merged, all with tests)

F1 field-eligibility: fn-value fields (#554), `slice<T>` view fields (#555), Move-handle fields
(#556). F0 pkg-foundation import rules (#558). Then, each surfaced by writing the framework's real
shapes:
- **#562** moving a named owned local into a struct-literal field never nulled the source ->
  double-free (inline temporaries hid it).
- **#571** http-ctx receiver may be a field place; partial move of a Move-handle field; and
  `NullStructField` zeroed every non-enum field as a 16-byte `{ptr,len}` — an 8-byte handle field
  would have CLOBBERED the next field.
- **#573** an owned argument passed through an INDIRECT call was never nulled -> double-free.
- **#575** a fn VALUE may now return `Result<T, E>` (`FnTy.ret` Scalar -> Ty), widened to exactly
  "scalars + Result"; also fixed `map_err` silently defaulting a non-scalar mapper return.
- **#576** `store_array_elems` stored NOTHING for a non-struct-literal element — `[web.get(..),
  web.post(..)]` left elements uninitialised (zeroed scalars, garbage `str`/fn pointers); and a fixed
  struct array now coerces to `slice<Struct>`.

std/web utilities: percent (#567), x-www-form-urlencoded + query lookup (#568), HTML escaping +
cookies (#569), CORS (#570), JWT (#566).

## Build / test notes

Per-machine env (also in Claude's memory `macos-build-env`): `LLVM_SYS_221_PREFIX=/opt/homebrew/opt/llvm`,
`LLVM_CONFIG=/opt/homebrew/opt/llvm/bin/llvm-config`,
`LIBRARY_PATH=/opt/homebrew/lib:/opt/homebrew/opt/openssl@3/lib` on every cargo/alignc run. After
changing `align_runtime`, run a plain `cargo build` before `alignc run` — user programs link the
runtime staticlib. `align_runtime`'s ~9 failures here are the documented sandbox-only ones
(`https_*` / `http_get_many` / `tcp_*` / `fs_read_dir` need real DNS/sockets/fds). Suites verified
green this session: sema 151, m4 26, m5 172, struct_index 28, array_materialize 6, owned_structs 20,
owned_structs_arrays 10, nested_structs 6, fn_values 21, par_map 14, analysis_coverage 23,
m9_fs 26, m9_io 22, m10_cli 17, m10_encoding 16, m11_http 27, m11_http_server 8, m11_process 35,
modules 36, cross_module_types 5, plus every `apps_*` suite above.

Previous: **pkg.web W2 IN PROGRESS — the request pipeline runs end-to-end over a real
socket.** `apps_web_serve.rs` drives: radix-router dispatch -> `Ctx` (a struct that OWNS the
`http_request_ctx`) -> `param_value` zero-copy `:id` capture -> the handler invoked **through the
`Route.handler` fn-value field** -> a responder that CONSUMES the handle out of that field. A clean
server exit is the assertion (a double-freed request handle aborts). Enablers landed: #571 (http-ctx
receiver may be a field PLACE; MoveCheck allows a partial move of a Move-HANDLE field; MIR/codegen null
it — and codegen's `NullStructField` zeroed every non-enum field as a 16-byte `{ptr,len}`, which would
have CLOBBERED the next field for an 8-byte handle: now keyed on `handle_free_fn`) and #573 (an owned
argument moved through an INDIRECT call was never nulled -> the caller and callee both freed it; same
omission class as #562, other move site). **ONE gap left for the designed handler SIGNATURE:** a fn
VALUE cannot carry a `Result` return (`FnTy.ret` is a `Scalar`), so handlers are `fn(Ctx) -> i64`
(0 = ok) instead of `fn(Ctx) -> Result<(), Error>`. Widening `FnTy.ret` Scalar -> Ty (plus the
indirect-call ABI for an aggregate return) is the next compiler step. **Web utilities shipped this
session** (owner directive: pkg.web is a GENERAL REST framework, never scoped to the LLM gateway —
`docs/impl/15-pkg-web-plan.md` §2b is the committed backlog): JWT HS256 alg-pinned + CT-compared
(#566, byte-identical to the canonical jwt.io vector), RFC 3986 percent (#567), x-www-form-urlencoded
+ zero-alloc query lookup with escape-aware key matching (#568), HTML escaping + RFC 6265 cookies with
header-injection rejection (#569), CORS with exact-origin matching and the forbidden
wildcard+credentials pair rejected (#570). Backlog remainder: multipart/form-data; OAuth client flows
are buildable now while validating a PUBLIC provider's token is blocked on RS256 (std.crypto RSA verify
over the already-linked libssl). Previous: **pkg.web F0 + F1 COMPLETE — the compiler + package foundation for the framework is done.**
**F0 (pkg-foundation, #558):** the two import-edge path rules — D7 `internal` (a `pkg.web.internal.*` module
is importable only from within `pkg.web`) + D8 layering (a `pkg/` module imports only `core`/`std`/`pkg`) —
enforced in `align_driver::load_units` (`check_pkg_import_edge`); spec text landed (draft §17 "Packages" +
§18.3, language-spec digest, design-notes "package philosophy"); open-questions PROPOSAL → SETTLED; tests
`pkg_foundation.rs` (6). **Owner surface decision (2026-07-20):** call sites are **fully qualified** —
`pkg.web.get(...)` / `pkg.web.param(c,"id")` / `c: pkg.web.Ctx` — per D3 (no import aliases); the `web.`
shorthand in `pkg-design/web.md` examples is spelled out when the public surface lands (W2). Noted in web.md.
**W1 feasibility (probed):** the radix tree must be built as **SoA scalar columns** — `array_builder<i64>`
per Node/Leaf field + `array_builder<string>` for edge labels — NOT `array<Node>`: Align cannot build a
runtime-sized `array<struct>` (no `array_builder<struct>`, no `range`/`[v;n]`/`array_fill`; only fixed
literals / `json.decode` / transforming an existing array yield `array<struct>`). This is MORE Align-idiomatic
(the doc's own "soa/tape/offset-table" move). Mutable struct-array element-field writes (`xs[i].f = v`) DO work
(node-pool fill lever). String primitives confirmed present for pattern matching: `seg.starts_with(":")`/
`"*"`, `seg[1..seg.len()]` (str slice), `str == str`. Dotted user modules work (`pkg.web.get`, full path).
**W1 ROUTER CORE DONE (#560 slice 1 + #563 slice 2).** The pattern matcher + linear-scan oracle
(#560) AND the flat **SoA radix tree** (#563) are both merged and **differential-tested** (`tree_dispatch`
== the `dispatch` oracle over 8 routes × 14 paths: nested params, tail wildcard, static-beats-param,
trailing slash, interior misses). The tree is all-i64 SoA columns — node table (`n_param`/`n_wild`/
`n_leaf`) + edge table (`e_par`/`e_child` + a **zero-copy** label `(e_pat,e_start,e_end)` = a str view,
never stored/escaped, only compared) — built + matched in one function so the owned columns never cross
a call boundary (serve will build once in its own scope + match in the request loop). Walk = static >
`:param` > `*wildcard`, no backtracking. Backtracking-requiring route sets are out of v1 (build-abort
later). All in `apps/web/pkg/web/internal/router.align` (module `pkg.web.internal.router`); driver test
`apps_web_router.rs`. **En route, fixed a real double-free (#562):** moving a NAMED owned local (string/
array/handle) into a struct-literal field was not nulling the source (`store_value_at` skipped
`null_moved_source`) → the source local + the struct both freed the buffer; now nulled (no-op for Copy/
borrow field values). **W1 remainders (fold into W2):** capturing the `:param` VALUES during the walk (for
`pkg.web.param(c,"id")`), method-aware leaves (per-path method set → the 405 Allow set), and build-time
conflict/duplicate/ambiguity abort. **Web-utility status (owner asked 2026-07-20):** implemented — base64/
base64url/hex/utf8 (`std.encoding`), sha256/512 + hmac_sha256 + argon2id + AEAD (`std.crypto`), full JSON,
http+SSE. GAPS: **URL/percent encode-decode** (the next `std.http` floor item — `pkg.web.query` needs it,
lands W3), HTML-escape (only a deferred `template html` context), JWT (a future `pkg.jwt` on the existing
hmac+base64url — NOT core/std). markdown is opaque JSON text for the LLM gateway — no codec needed.
**NEXT: W2 (Ctx/serve/dispatch — the public `pkg.web.*` surface, fully qualified) starting with param-value
capture; and the W3 std.http URL-percent floor.** Previous: 2026-07-20, **W1 slice 1 DONE (#560):** `apps/web/pkg/web/internal/router.align` (module `pkg.web.internal.router`) —
the pattern matcher + linear-scan dispatch **oracle**: `seg_end` (segment boundary), `match_score(pattern,
path)` (specificity score `score*3 + w`, static w=2 / `:param` w=1 / `*wildcard` ends the walk → below
static/param; static>param>wildcard left-to-right, no backtracking), `dispatch(patterns, path)` (index of
the most specific match, -1 none; distinct matches → distinct scores → no ties). Pure, zero-alloc, str
views. Test `apps_web_router.rs` compiles the real file via `include_str!` under a minimal `pkg.web` root +
`main` (also exercising F0 D7/D8 end-to-end). match_score's `*3` fold assumes bounded path depth (~<39
segs; the tree carries no such bound). **NEXT: W1 slice 2 — the flat SoA radix tree** (build + match),
differential-tested against slice 1's `dispatch` oracle. Build approach: SoA i64 columns (node
`first_edge`/`n_edges`/`param_child`/`wild_leaf`/`leaf`, edge `node`, leaf `route_index`) + edge labels as
`array<string>` (owned, cloned at build — startup only; a zero-copy `(pattern_idx,start,end)` triple is the
later optimization). Since `array_builder` is grow-only with no mid-build random access, allocate each
column by pushing N defaults → `build()` → a `mut array<i64>`, then construct the tree via random-access
element writes (`col[i] = v`, which work). Conflict/duplicate/ambiguous routes abort at build. Then W2
(Ctx/serve/dispatch — the public `pkg.web.*` surface, fully qualified per the owner decision). Design +
router internals spec: `pkg-design/web.md`.
Previous milestone: 2026-07-20, **pkg.web F1 — FIELD-ELIGIBILITY WIDENING COMPLETE (all three slices merged).**
The hard compiler gate for `pkg.web` (`docs/impl/15-pkg-web-plan.md`) is done — a struct field may now be
(①) a **fn value** (`Route { handler: fn(Ctx) -> Result<(), Error> }`, #554), (③) a **`slice<T>` view**
(`Ctx { params: slice<str> }`, region-tied so an arena/local-array view can't escape via a struct, #555),
and (②) a **Move handle** (`Ctx { req: http_request_ctx }` — the struct becomes Move, drop closes the
handle exactly once via the shared `handle_free_fn`; `http_request_ctx` is now a nameable surface type,
#556). All four kinds coexist + drop cleanly (handle+string freed once, slice/fn/scalar skipped);
move-once + partial-move-out + arena-escape + `Option<handle>` all rejected cleanly. New tests:
`fn_values.rs` (+5), `struct_slice_fields.rs` (+5), `struct_handle_fields.rs` (+8); `handle_free_fn` is the
one source of truth shared by `Stmt::Drop` and `drop_struct_fields`. Each slice went PR → self-review →
`/code-review` → merge. **NEXT: F0 (pkg-foundation rules — `internal` path + pkg-layering import checks +
spec text; parallelizable) then F3/W1 (the radix-tree router core at `apps/web/pkg/web/`, per the design
doc `pkg-design/web.md`). W1 needs only F1① (done), so it is unblocked.** Previous update: 2026-07-19,
**JSON COMPLETENESS J5 `json.scan` — COMPLETE (streaming typed rows; merged
as #546 slice 1 + #547 slice 2).** `json.scan(view)` + the new `json.scanner<Row>` type (a **Copy**
`{ptr,len}` input view, region-tracked — it borrows the input, never materializes an `array<Row>`; row
type from the binding annotation `rows: json.scanner<Row> := json.scan(view)`, exactly like `decode`, no
turbofish; no `arena {}` needed — each row decodes into a per-step stack slot and its `str` fields borrow
the input). It is a **pipeline source only**: the full streaming reducer family — `.sum()` / `.count()` /
`.reduce(init,f)` / `.any(p)` / `.all(p)` / `.min()` / `.max()` — each yields **`Result<T, Error>`** (a
malformed row surfaces once as `Err`; unwrap with `?`), with the full stage set (`.field` / `.where(.field)`
/ `.where(pred)` / `.map(f)`) driven per row by a dedicated `lower_json_scan_reduce` (a streaming header:
`JsonScanNext` decodes the next object at a loop-local cursor, branching row / done / malformed — NOT
`lower_array_reduce`'s counted loop). ONE scanner handles both a top-level JSON array and NDJSON (runtime
`align_rt_json_scan_next` treats a leading `[`, inter-value `,`, whitespace/newlines as separators and
`]`/EOF as terminators; reuses the struct decode descriptor per row; a malformed row → `Error.Code(1)`,
byte-identical to `json.decode` of the same input — pitfall P2). Materializing terminals (`to_array`/`sort`/
`group_by`/…) over a stream are rejected in sema (a clean diagnostic, not mis-lowered). `Ty::JsonScanner(u32)`
+ HIR `JsonScan` threaded through every exhaustive pass (region_of = the input view / tracks_region /
ty_may_borrow / mentions_slice / MoveCheck / walk / visit / finalize / borrow_sources / slice_is_local); MIR
`JsonScanNew` + `JsonScanNext` (the latter bakes the row schema-sig for cache invalidation, P5); codegen FFI
+ descriptor reuse. **draft.md §18.1 + language-spec.md already described this exactly (design ran ahead;
the implementation matched it), so NO spec-sync (J6) change was needed — the spec is already consistent.**
Reviewed each slice adversarially before merge: fixed the malformed-error variant (→ `Error.Code(1)`, P2),
the `scan_terminal` flag leaking into stage-lambda checking (memory-unsafe if a nested scanner's
materializing terminal inherited it), a whole-struct `any`/`reduce` `cur.expect` panic (now loads the row
struct), and a non-scalar `reduce` accumulator panic (now a clean diagnostic). Tests: m5 `json_scan_*` (15),
runtime `json_scan_next_streams_array_and_ndjson`. Suites green: m5 172, sema 151, mir 7, runtime json 24;
clippy clean. **With `json.scan` the entire JSON-completeness arc (J1–J5) is COMPLETE — core.json is exactly
`decode`/`encode`/`doc`/`scan`.** **NEXT (owner-directed 2026-07-20): execute
`docs/impl/15-pkg-web-plan.md` — build `pkg.web`, the ZERO-COPY, blazing-fast REST framework.**
The owner's restored brief (a lost conversation, now pinned in the design doc AND in Claude's
per-machine memory `pkg-web-fiber-reference.md`): speed is the headline; primary reference **Go's
Fiber** (fasthttp zero-allocation philosophy), router reference the **httprouter radix-tree
lineage**; the gateway/LLM apps are merely the framework's first consumers, explicitly LATER.
**The design is DONE (v2):** `docs/impl/pkg-design/web.md` (+ja) — performance contract (zero
request-byte copies / zero per-request heap / O(segments) radix dispatch / zero-copy output /
startup-total validation, bench-pinned W5 + Fiber-comparison W7), Fiber-style surface (per-method
constructors, `:name`/`*name` patterns with static>param>wildcard priority, one-ctx handlers
`fn(c: web.Ctx) -> Result<(), Error>`, `web.param(c, "id")`), middleware-lite design
(non-capturing, Move-threaded), NO open forks. Execution order: **F1** field-eligibility widening
(the hard compiler gate — struct fields for ① fn values ② the Move `http_request_ctx` handle ③
`slice<str>`; probed: all three rejected today by `is_field_ok`'s whitelist) ∥ **F0**
pkg-foundation rules (`internal` + pkg-layering import checks + spec text; proposal in
open-questions moves → Settled on landing) → **F3** W1–W7 framework slices at `apps/web/pkg/web/`
(workspace `apps/web/`) → F4 the LLM gateway app LATER, when the owner calls it. Monorepo;
first-party; shipped with the system as a vendorable subtree. Previous
update: 2026-07-19, **JSON COMPLETENESS J4 `json.doc` — SLICE 3 SHIPPED → J4 COMPLETE (`elems()`;
branch `json-j4-doc-elems`).** `d.elems() -> slice<json.doc>` materializes one document level (each
Array element, or each Object member VALUE — keys via `key(i)`) as an arena-backed `slice<json.doc>`
ONCE (O(n) build, O(1) index — vs `at(i)`'s O(i) re-walk per call). **The key realization: no new array
type was needed** — `Ty::Slice(Scalar)` already takes a full `Scalar` and `Scalar::JsonDoc` already
exists, so `slice<json.doc>` = `Ty::Slice(Scalar::JsonDoc)` REUSES the existing slice machinery:
`.len()` and `xs[i] -> json.doc` work out of the box (`check_index`'s Move-element rejection doesn't
fire — a `json.doc` is a Copy 16-byte handle, no double-free; `region_of(Index)` binds the element to
the slice), and `slice<json.doc>` is nameable as a param type so `fn f(xs: slice<json.doc>)` walks a
level by recursion. Runtime `align_rt_json_doc_elems` bump-allocates a `DocHandle` buffer in the arena
(`checked_mul`, null-safe) + writes the `{ptr,len}` header; region = `arena.shorter(region_of(doc))`
(like `to_array`). New HIR/MIR `JsonDocElems` threaded through every exhaustive pass. Tests: `m5.rs`
`json_doc_elems_materialize_and_iterate` (materialize → index → recursive sum = 67) + the
`json-doc-elems-escape` negative (returning the slice out of the arena is an arena-region error, #297),
runtime `json_doc_elems_materializes_a_level`. Suites green: m5 157, sema 151, mir 7, runtime json_doc 8;
clippy clean. **`/align-self-review` + review pending before merge. FOLLOW-UP (not required for J4):**
full `.map`/`.where` **pipeline fusion** over a `slice<json.doc>` (closures taking `json.doc`) — index +
len + recursion already cover level iteration. **With `elems()` the entire J4 `json.doc` surface (parse,
kind, get/at, all leaf accessors, len, key, elems, nameable types) is COMPLETE.** Then **J5** `json.scan`
→ **J6** spec sync. Previous update: 2026-07-19, **JSON COMPLETENESS J4 `json.doc` — SLICE 2 SHIPPED (`len` + `key` + nameable
types; branch `json-j4-doc-slice2`).** On top of slice 1 (merged as #543): `d.len()` (member/element
count, 0 on a non-container / Missing) and `d.key(i) -> Option<str>` (the i-th object member key in
document order — objects-as-ordered-data, a `str` view region-bound to the doc). With `at(i)` these
iterate a doc array by **recursion** (no `loop` needed — `loop` is deferred). The builtin type names
**`json.doc` / `json.kind` are now nameable** in annotations: `resolve_type` special-cases the qualified
`json.doc` → `Ty::JsonDoc` / `json.kind` → the builtin enum BEFORE the import/`pub` check (which would
else reject `json` as an un-imported module), so a `fn f(d: json.doc)` helper and a `k: json.kind`
binding compile — this is what makes recursive doc iteration expressible. New HIR `JsonDocLen`/
`JsonDocKey` + MIR `Rvalue` siblings threaded through every exhaustive pass (len = `Static` i64; key =
receiver-region `str` view, like `as_str`); runtime `align_rt_json_doc_len` (i64) / `align_rt_json_doc_key`
(i32 present flag + `str` view, sharing a new `doc_write_str` helper with `as_str`). Tests: `m5.rs`
`json_doc_len_and_key_iterate_via_recursion` (recursive sum over a doc array + key-order + out-of-range),
runtime `json_doc_len_and_key`. Suites green: m5 156, sema 151, mir 7, runtime json_doc 7; clippy clean.
**`/align-self-review` + review pending before merge.** **Deferred to J4 slice 3:** `d.elems() ->
array<json.doc>` (materialize a level as a pipeline source — needs `json.doc` as an owned/slice element
+ pipeline-over-doc machinery; `at`/`len`+recursion already cover level iteration). Then **J5** `json.scan`
→ **J6** spec sync. Previous update: 2026-07-19, **JSON COMPLETENESS J4 `json.doc` — SLICE 1 SHIPPED (the schema-unknown lazy
document view MVP; branch `json-j4-doc`).** New `Ty::JsonDoc` / `Scalar::JsonDoc` — a **Copy**
`{tape,node}` handle (laid out `{ptr,i64}` like a slice), region-tied to min(input, arena). `json.doc(s)?`
inside an `arena {}` parses ONCE into an arena-backed `simdjson`-style flat node tape (per-node
sibling-skip offsets; a recursive validate-and-emit build, full escape/UTF-8 validation → malformed =
`Err`), yielding `Result<json.doc, Error>`. Navigation is **total + Missing-propagating**: `d.get(k)` /
`d.at(i)` always return a `json.doc` (`Missing` = `node < 0`, propagates). `d.kind()` → the new **builtin
`json.kind`** tag-only sum type (`Object/Array/Str/Number/Bool/Null/Missing`, registered like `Error` /
`argon2_params`; matched by bare variant name). Leaf accessors `as_str`/`as_i64`/`as_f64`/`as_bool` →
`Option` (`as_str` = zero-copy input view, escaped strings unescape into the arena — the one allocating
accessor). Runtime `align_rt_json_doc_{parse,kind,get,at,as_str,as_i64,as_f64,as_bool}`. Schema-UNKNOWN
→ NO `json_schema_sig` cache key (the tape is generic). Threaded through every exhaustive HIR/MIR pass
(new HIR `JsonDoc`/`JsonDocKind`/`JsonDocGet`/`JsonDocAt`/`JsonDocAsStr`/`JsonDocAsScalar`; MIR `Rvalue`
siblings; region_of = min(input,arena) for the doc, receiver-region for `get`/`at`/`as_str`, `Static`
for `kind`/`as_scalar`; `tracks_region` + `ty_may_borrow` + `borrow_sources` = the input roots — the
#297 escape is enforced). Codegen adds `Ty::JsonDoc` to the slice-struct groups + 8 FFI decls + the
tag-only-enum wrap for `kind()`. Method dispatch intercepts `kind`/`get`/`at`/`as_str`/`as_i64/f64/bool`
on a `json.doc` receiver BEFORE the shared-name (`as_str` on bytes / `get` on a box) handlers; the
receiver may be a temporary (Copy, never dropped → chaining `d.get("a").at(0).as_i64()` is fine). Tests:
`m5.rs` (navigate+kind-match+leaf accessors round-trip, malformed→Err, arena gate, view-escape
rejection = arena-region error), runtime `json_doc_*` (tape nav / Missing propagation / escapes+surrogate
pairs / malformed corpus). Suites green: m5 155, sema 151, mir 7, runtime json_doc 4, cache_codegen 20
(only the known macOS gate7 `LC_UUID` flake fails); clippy clean. **`/align-self-review` + `/code-review`
pending before merge.** **NEXT: J4 slice 2** — `d.len()`, `d.elems() -> array<json.doc>` (materialize a
level for pipelines), `d.key(i) -> Option<str>` (objects-as-ordered-data) → **J5** `json.scan` → **J6**
spec sync. Previous update: 2026-07-19, **JSON COMPLETENESS T1b COMPLETE — `array<Option<T>>` DEFERRED (docs-only
closure, branch `json-t1b-array-option`).** The three JSON-specific T1b matrix-fill items all shipped
(`array<scalar>` fields #538, bare scalar decode #539, `Option<struct>` encode #540). The remaining
sketch item `array<Option<T>>` is **DEFERRED as a language-type gap, not a JSON gap:** an owned array's
element is a `PrimScalar` (the deliberately non-recursive, `Copy` subset), and `Option<T>` is a
composite — so `array<Option<T>>` is un-representable in the type system today (rejected everywhere, not
just in JSON: `[Some(1), None]` fails to type). It needs a dedicated composite-element owned-array type
threaded through the whole pipeline — a real language-surface addition, and low value (a `[1,null,3]`
sparse numeric array is rare in the gateway/align-LLM shapes `array<scalar>` already covers). Per
CLAUDE.md "ideal form or defer", it waits for that language feature. Recorded in open-questions "T1b" +
json.md (+ja). **So JSON completeness advances to J4.** **NEXT: J4 `json.doc`** — the schema-unknown
lazy document view (SETTLED design in open-questions "T2": `d := json.doc(s)?` in an `arena {}` → an
arena-backed tape over the existing SIMD structural index; total Missing-propagating navigation
`d.get("k")`/`d.at(i)` → `json.doc`, `d.kind()` → `json.kind{Object,Array,Str,Number,Bool,Null,Missing}`,
leaf `as_str/as_i64/as_f64/as_bool` → `Option`, `d.key(i)`/`d.len()`/`d.elems()` → `array<json.doc>`;
a Copy view handle region-tied to min(input, arena); NOT a serde-style value tree). A substantial
multi-slice feature — start with the type + `json.doc(s)?` parse + `kind()` + leaf accessors MVP. Then
J5 `json.scan` → J6 spec sync. Previous update: 2026-07-19, **JSON COMPLETENESS T1b (part 3) SHIPPED
(MERGED as #540) — `Option<struct>` ENCODE (the Slice-B follow-up).** Decode already supported an
`Option<struct>` field; encode now renders it too — **`Some` → the nested object** (via the runtime
descriptor-driven encoder), **`None` → the field omitted** (the trailing-comma + `PopComma` scheme). A
new `OptionStructField` template piece (HIR/MIR + printer, swept through every template pass) + a new FFI
`align_rt_json_encode_object` (a single struct by its descriptor table, over the existing
`json_encode_object`). Codegen tag-branches: on `Some`, writes `"name":`, stores the payload struct to an
entry alloca, calls the object encoder with the payload's `emit_desc_table`, then a trailing comma. The
payload struct is validated encodable (`decode_struct_fields_ok`); composes recursively (a payload with a
nested plain struct + a nested `Option<str>` omits its own `None`s). **v1 boundary (unchanged):** an
`Option<Move-struct>` payload (a struct owning an `array`/`string`) stays rejected at declaration (Slice-B
— an owned Option-payload drop-as-a-field has no consumer). **Also fixed a pre-existing #514/#517
stale-cache bug the new gate caught:** `json_schema_sig` folded an `Option<struct>` field to a bare
`"Option"` (via `ty_name`), so a payload field RENAME (or an `Option<i64>`→`Option<f64>` change) at the
same slot did NOT invalidate the **decode** cache — a warm cache served a stale-key object (reproduced
decode-only end-to-end). Now recurses into the `Option<struct>` payload and renders the `Option<scalar>`
width. The `OptionStructField` piece also bakes the payload schema (belt-and-suspenders for a
pure-encode program). `/align-self-review` (Gate 1 template-pass sweep + the schema-sig gap) + `/code-review`
high run. Tests: `m5.rs` T1b (Some/None round-trip, Option-last-field comma, nested compose,
`Option<Move-struct>` rejected), `cache_codegen` gate2e (decode-only payload-rename invalidates). Suites
green: m5 151, sema 151, mir 7, cache_codegen 20 (only the known macOS gate7 flake fails); clippy clean.
**NEXT: T1b remainder** — `array<Option<T>>` compositions (the last matrix-fill item) → **J4** `json.doc`
→ **J5** `json.scan` → **J6** spec sync. Previous update: 2026-07-19, **JSON COMPLETENESS T1b (part 2)
SHIPPED (MERGED as #539) — top-level (bare) scalar decode targets (`x: i64 := json.decode("42")?`).** `json.decode`
into a bare int / float / bool target parses the WHOLE input as one JSON number / bool. The value is
`Copy` (copied out, not a view into the input), so the result is **`Static` / returnable** (returns freely
out of the `arena {}` that backed the input, unlike a str-bearing struct). A new HIR/MIR
`JsonDecodeScalar { scalar, input }` → `Rvalue::JsonDecodeScalar` → runtime `align_rt_json_decode_scalar`
(parses via the shared per-scalar `write_value`, so the same range / sign / float-width checks a scalar
*field* gets apply; leading/trailing whitespace allowed, any trailing non-whitespace → `Err`). Swept
through every exhaustive HIR/MIR pass (region_of → `Static`, effects/escape/movecheck/borrow-sources) +
the MIR printer (`scalar` in the digest). **Deferred:** a bare `str` target (an input-borrowing view —
region-tracking follow-up) and `char` (no JSON form) stay on the annotate/unsupported path.
`/align-self-review` (Gate 1 new-variant sweep — every analysis pass verified) + `/code-review` high run.
Tests: `m5.rs` T1b (int/float/bool + whitespace, returnable-from-arena, error paths = trailing-garbage /
type-mismatch / out-of-range, bare-`str` deferred). Suites green: m5 148, sema 151, mir 7, codegen 26,
cache_codegen 19 (only the known macOS gate7 flake fails); clippy clean. **NOTE:** the top-level
`array<scalar>` target (`xs: array<i64> := json.decode("[1,2,3]")?`) already existed (MMv2 slice 8c) — this
adds the BARE scalar. **NEXT: the rest of T1b** — `Option<struct>` ENCODE (the Slice-B follow-up),
`array<Option<T>>` compositions → J4 `json.doc` → J5 `json.scan` → J6 spec sync. Previous update:
2026-07-19, **JSON COMPLETENESS T1b (part 1) SHIPPED (MERGED as #538) — `array<scalar>` struct fields (`array<i64>` / `array<f64>` / `array<bool>`).** A struct
field may now be an owned scalar array — the align-LLM data shapes (embeddings `array<f64>`, token ids
`array<i64>`), decoded/encoded byte-identically and composing with J3b (a scalar-array field inside an
`array<Move-struct>` element). **New JSON descriptor kind 7:** the field's own `{ptr,len}` slot is width
16 (low byte); the ELEMENT scalar is packed into the tag's upper bits (elem-signed bit 16, elem-kind
0=int/1=bool/2=float bits 20-23, elem-width bits 24-27), so one tag carries both. **Runtime:**
`decode_scalar_array_value` parses a JSON scalar array into an owned buffer via the shared per-scalar
`write_value` (so the same range / sign / float-width checks a scalar *field* gets apply per element);
`write_value`/`json_encode_value` grew kind-7 arms; `json_encode_scalar_array` (+ FFI
`align_rt_json_encode_scalar_array`) emits `[e0,e1,…]`; `field_width` kind 7 = 16 (the else branch);
`drop_decoded_owned` grew a kind-7 flat-free arm AND `sub_owns_buffers` a kind-7 `true` (the Gate-1
decode-error-path leak — a scalar-array field / a scalar-array field inside an `array<Move-struct>`
element). **Sema:** `decode_struct_fields_ok` admits `Ty::DynArray(int/float/bool)`; `json_object_parts`
emits a new `ScalarArrayField` template piece. `array<str>` (borrowed element) / `array<char>` deferred.
**Codegen:** `json_payload_tag_sub` `Ty::DynArray(scalar)` → kind-7 tag; the `ScalarArrayField` piece →
runtime encoder call. **MIR:** `TemplatePart`/`TemplatePiece::ScalarArrayField` threaded through every
exhaustive template pass + the printer. **Also fixed a pre-existing #514/#517 stale-cache bug the new
gate caught:** MIR `ty_name(Ty::DynArray)` rendered a bare `"array"` (dropping the element), so the baked
`json_schema_sig` couldn't distinguish `array<i64>` from `array<f64>` at the same slot → a warm cache
would serve a stale wrong-element-width decode object; now renders `array<i64>`. `/align-self-review`
(Gate 1 kind-dispatch + template-pass sweep — found the drop-error-path + stale-cache holes; Gate 3
no-panic) + `/code-review` high run. Tests: `m5.rs` T1b (int/float/bool decode/encode+round-trip,
widths+empty+u64-max, composes-with-`array<Move-struct>`, type-mismatch Err, `array<str>` rejected),
`cache_codegen` gate2d (element-type-change invalidates), runtime alloc-count (scalar-array error-path
free). Suites green: m5 144, sema 151, cache_codegen 19 (+gate2d; only the known macOS gate7 flake
fails), runtime alloc-count 252 passed; clippy clean. **v1 limits (unchanged):** `.sum()`/pipelines over
an owned scalar-array field and `json.encode` of a bare `array<scalar>` stay restricted (decode + `.len()`
+ encode-as-field work). **NEXT: the rest of T1b** — top-level scalar/bool decode targets
(`x: i64 := json.decode(s)?`), `Option<struct>` ENCODE (the Slice-B follow-up), `array<Option<T>>`
compositions → J4 `json.doc` → J5 `json.scan` → J6 spec sync. Previous update: 2026-07-19,
**JSON COMPLETENESS J3b SHIPPED (MERGED as #537) — `array<Move-struct>` struct fields, closing the full OpenAI chat gateway shape
`Chat { messages: array<Message> }`.** An `array<Struct>` field (and a standalone `array<Struct>` local)
whose element is **Move** — a `string`/owned-array/Move-enum field, transitively — is now deep-freed
instead of one flat free that leaked each element's owned buffer. **Codegen:** a shared
`deep_free_struct_array(slice_ptr, eid)` helper emits a runtime loop over `len` that recursively
`drop_struct_fields` each element, then frees the AoS buffer (`get_insert_block` captures the loop
back-edge because a Move-enum element's `drop_enum` appends blocks); called from BOTH
`drop_struct_fields`'s array arm (struct field) AND `Stmt::Drop`'s new `DynStructArray`-Move arm
(standalone local — the Gate-1 sibling hole: `ms: array<Message> := json.decode(...)`). **Runtime:**
`drop_decoded_owned`'s kind-5 arm deep-frees each element (gated by a new `sub_owns_buffers` walk) before
freeing the AoS on the decode error path, and `decode_struct_array_value`'s `cleanup_partial` deep-frees
the elements already materialized in `buf[0..count]` on a mid-array parse failure (the current failing
element is cleaned by `parse_object`). **Sema:** the J3a pass-0c-3 `array<Move-struct>` rejection is
lifted (every Move element is deep-freeable); `array<string>` (a bare-`string`-element array field) stays
rejected at 0b-2. **v1 limits:** `json.encode` of a bare `array<Move-struct>` and pipelines over a
Move-struct-array field are still restricted (element access is Slice-C-limited; the decode→encode
passthrough of a `Chat` record works). `/align-self-review` (Gate 1 sibling-drop sweep — found + fixed
the standalone-local leak; Gate 2/3 FFI/panic) + `/code-review` high run. Tests: `m5.rs`
(`json_chat_array_of_move_message_roundtrip` — the full gateway shape, `json_standalone_array_of_move_struct_local_drops_clean`,
`array<string>`-element rejection), runtime alloc-count (`json_array_of_move_struct_sibling_failure_deep_frees_every_element`
exact 3==3, `..._mid_array_failure_frees_prior_elements` balance) + a shared `ALLOC_COUNT_LOCK` mutex
serializing all 5 count-asserting alloc-count tests (global-counter pollution fix). Suites green: m5 139,
runtime alloc-count full-suite 251 passed (only the known network/fd sandbox tests fail), clippy clean.
**With J3b the OpenAI chat request/response gateway CLOSES end-to-end** (`Chat { messages: array<Message> }`
with the multimodal `content` union round-trips byte-identically). **NEXT: J3 T1b matrix fill** —
top-level scalar/bool decode targets (`x: i64 := json.decode(s)?`), `array<scalar>` struct fields,
`Option<struct>` ENCODE (the Slice-B follow-up), supported-constructor compositions → **J4** `json.doc`
→ **J5** `json.scan` → **J6** spec sync. Previous update: 2026-07-19, **JSON COMPLETENESS J3a SHIPPED
(branch `json-j3a-move-enum-struct-field`) — the multimodal union as a Move-enum struct field (`Message { content: Content }`)**.
A **Move** enum (an owned `array<Struct>` payload variant) is now a legal struct field, which makes the
enclosing struct **Move** — so the full multimodal `content: str | array<Part>` union composes into a
record and decodes/encodes both shapes, round-tripping byte-identically. **Classifier:**
`struct_is_move`/`struct_is_move_rec`/`ty_owns_buffer_rec` grew an `enums` param + a `Ty::Enum` arm
(`enum_is_move`), threaded through EVERY caller (mir `Builder`, codegen, sema's `ty_is_move` /
`is_owned_droppable` / `ty_capture_is_move` / `reject_move_struct_payload` / `enum_payload_ok`) so every
Move-ness question — MoveCheck, drop-flag, escape, Result/Option-payload rejection — sees a
struct-with-Move-enum-field as Move in lockstep. The pass-0c-2 rejection was lifted. **Drop:**
`drop_struct_fields`'s new `Ty::Enum` arm frees the live variant via the tag-switched `drop_enum`;
`DropFlagInit` zeroes the aggregate → null-safe on a moved-out / unconstructed path. **Match-move:**
`match m.content { Parts(ps) => … }` moves the owned payload out of the field — `null_moved_source`'s
depth-1 `Field` arm gained a Move-enum case (mirroring the `string`-field one), and `NullStructField`
codegen became type-aware (zero the whole `{tag,payloads}` enum aggregate, not just a 16-byte slice), so
the struct's exit `Drop` frees null there — single-free (same use-consumes-the-enum semantics a
bare-local scrutinee already has; use-after-partial-move reads the nulled value, a pre-existing
sema-completeness gap shared with bare locals, not new). **JSON:** decode/encode were already kind-6-aware
(J1b-2b); only the runtime `drop_decoded_owned` grew a **kind-6** arm (`→ drop_decoded_union`) to free the
union's owned payload on the trailing-garbage error path. **`array<Move-struct>` rejection relocated to a
new post-0c pass (0c-3):** an element struct can be Move only through a not-yet-resolved enum field, so
the 0b-2 check (enums empty) let `Chat { messages: array<Message> }` slip through with a leaking flat
free — moved after 0c where `struct_is_move` is enum-accurate; `array<Message>` is now cleanly rejected
(its owned-element deep free is J3b). **v1 confinements (unchanged, now Move-enum accurate):** a Move
struct is rejected as a `Result`/`Option` Ok/Err payload across a function boundary (Slice-C constraint,
so a decode target uses `?`); reassigning a Move-enum field leaks the old buffer (only a direct `string`
leaf is drop-of-old'd — the SAME pre-existing gap `array<T>` fields have, not new). `/align-self-review`
(Gate 1 Move-reason sweep — every Move-ness pass verified enum-aware in lockstep; Gate 3 no-panic) +
`/code-review` high both run. Tests: `enum_match.rs` (`move_enum_struct_field_is_move_and_drops_clean` —
was the J2a negative test, flipped to acceptance), `m5.rs` J3 (both-shape decode/encode+round-trip,
match-move no double-free, trailing-garbage no-leak, `array<Message>` rejected). Suites green: m5 137/137,
sema 151, enum_match 58, layout_parity, cache_codegen (only the known macOS gate7 LC_UUID flake fails,
as on `main`), runtime alloc-count (only the known network/fd sandbox tests fail); clippy clean. Docs:
open-questions runway, json.md (+ja), HANDOFF. **NEXT: J3b — `array<Move-struct>` struct fields (the
owned-element deep free) to close `Chat { messages: array<Message> }`, then the T1b matrix fill
(top-level scalars, `array<scalar>` fields, `Option<struct>` encode, compositions) → J4 `json.doc` →
J5 `json.scan` → J6 spec sync.** Previous update: 2026-07-18, **JSON COMPLETENESS J2b SHIPPED — union Array shape-class arm (the owned
`array<Struct>` union variant), MERGED as #536 (`e16a132`)**. A JSON `[` now dispatches to a
shape-directed union's owned `array<Struct>` variant (shape class Array=4, O(1) first-byte), so the
full multimodal **`Content { Text(str), Parts(array<Part>) }`** union decodes both shapes
(`"hi"`→Text, `[{…}]`→Parts) and encodes the live payload **bare** — `decode(encode(x))` round-trips
byte-identically. The whole decode/encode/drop-cleanup pipeline was ALREADY kind-5-aware from Slice C
(`json_payload_tag_sub`→`emit_json_union` arm; runtime `json_shape_class('[')`=4,
`decode_union_value`/`write_value`/`field_width`, `encode_union_at`/`json_encode_value`), so the change
was minimal: sema `union_shape_class` `Scalar::DynStructArray => Some(4)`; `check_union_decodable`'s
class table `[_;4]`→`[_; JSON_SHAPE_CLASSES]` (an Array arm would else index OOB and panic) + recurse
into the array ELEMENT struct; MIR `json_union_schema_sig_into` expands the element struct's schema
(`[]{…}`, cache invalidation — `cache_codegen` gate 2c); runtime `drop_decoded_union` frees the
materialized AoS on the trailing-garbage error path (the ONE new leak surface — `alloc-count`
new==free gate). `/code-review` high run (1 minor duplication finding, rejected — bodies differ,
mirrors `encode_union_at`). Tests: `m5.rs` J2b (decode-by-shape, encode-bare+round-trip,
trailing-garbage no-leak, two-Array clash, Move-element rejected). Docs: open-questions runway,
json.md (+ja); draft §14 / language-spec already carried the design. CI green all 3 platforms.
**v1 boundary: `array<scalar>` union payloads stay rejected (no descriptor arm) → J3.** **The union
itself closes here; the multimodal `Content` union decodes/encodes standalone. NEXT: J3 — matrix fill
(T1b), which INCLUDES the two remaining gateway compositions the union can't yet compose into:
`Message { content: Content }` (a **Move-enum struct field** — J2a deferred it, needs threading
`struct_is_move`/`ty_owns_buffer_rec` + `drop_struct_fields`'s kind-6-owned arm + `drop_decoded_owned`
kind-6) and `Chat { messages: array<Message> }` (an **`array<Move-struct>`** — owned array element,
deferred by Slice C). Plus the T1b scalar targets, `array<scalar>` fields, `Option<struct>` encode,
and supported-constructor compositions.** Then J4 `json.doc` / J5 `json.scan` / J6 spec sync.
Previous update: 2026-07-18, **JSON COMPLETENESS J2a SHIPPED — enum owned `array<T>` payloads +
tag-switched drop (the multimodal-union language prerequisite), MERGED as #535 (`de8a405`)**. A sum type may now carry an
owned `array<T>` payload (`Content { Text(str), Parts(array<Part>) }`), which makes the **enum a Move
type**: its `Drop` switches on the tag and frees the live variant's owned buffer. **Classifier:**
`enum_is_move(id, enums)` = any variant payload `is_move()` — the SAME predicate codegen `drop_enum`
uses to pick fields, so classifier + drop stay in lockstep. Threaded into `is_owned_droppable` /
`needs_drop_flag` / `ty_is_move` / `ty_capture_is_move` (the enum arm) — deliberately NOT into
`struct_is_move` (22 sites); instead a **Move enum struct field is rejected** (a non-Move enum field
stays allowed, J1b). **Drop:** codegen `drop_enum` loads the i32 tag, switches, frees each owned
variant's `{ptr,len}` field-0 ptr (one flat free — element non-owned, pass 0c; verified in IR).
`DropFlagInit` zeroes the aggregate + MIR `null_moved_source` (enum arm + `EnumValue`-wrapper case)
nulls a moved-away enum → null-safe, single-free every path. **Pass 0c** admits
`array<scalar>`/`array<str>`/`array<plain-struct>`; `array<string>` / `array<Move-struct>` deferred; a
non-representable element (`array<array<T>>`, `soa`) is a **clean diagnostic, never a panic** (Gate 3 —
`/align-self-review` caught the original `ty_to_scalar().expect()` panic). `enum_payload_ok` (generic
enums) grew the same arm. MoveCheck consumes an owned payload at construction (`EnumValue` → `(true,
true)`, like a struct literal). **Region/escape** arms are J1a's, unchanged (`region_of` folds an
owned-array payload to `Static` → freely returnable; `str`-payload enums stay arena-bound). **v1
confinement (fail-closed):** a Move enum is a bare local / param / return only — rejected as a struct
field, `Option`/`Result` payload (the `Some`/`Ok`/`Err` wrap site, `reject_move_enum_payload`),
fixed-`array` element, and lambda capture (`ty_capture_is_move`), each with a "later slice" diagnostic
(tuple/box/task were already rejected). **Match-binding an owned payload works** — `lower_match_enum`
already nulls the scrutinee on a bound arm (`null_moved_source`), so the binding moves the buffer out
(the same path `Result`/`Option` use for Move payloads); an over-broad sema rejection was removed after
CI caught it breaking `match c.parse() { Ok(p) => … }` (`p: cli parsed`, a Move scalar). `/code-review`
high run: 2 self-findings (enum_is_move↔drop_enum divergence via a dead defensive struct arm; Gate-5
resolve-once) applied. Tests: `enum_match.rs` J2 section (13 —
construct/move/return/if-join drop-clean, scalar-vs-str precision, 9 deferral rejections incl. the
nested-array non-panic). Workspace green (only the known macOS `gate7` LC_UUID cache flake fails, as on
`main`); clippy clean. **NEXT: J2b — the union Array shape-class arm** (`union_shape_class`
`Scalar::DynStructArray => Some(4)`; `check_union_decodable`'s `class_owner: [_;4]` → `[_;5]`; the
runtime `json_shape_class` already reserves Array=4 and `decode_union_value` reads `class_to_arm[4]`
generically; `emit_json_union` fills the arm; encode writes the live `array<Struct>` payload bare) →
decode/encode the `array<Part>` variant → the **full multimodal `Content` union closes the REST
gateway** → then J3 matrix fill / J4 `json.doc` / J5 `json.scan` / J6 spec sync. **The owned
`array<Struct>` enum payload is only constructible via `json.decode` today (`.to_array()` over structs
is deferred), so J2a is tested standalone with `array<i64>` payloads; the `array<Struct>` case lands
end-to-end in J2b.** Previous update: 2026-07-18, **JSON COMPLETENESS J1b-2b SHIPPED — shape-directed union as a struct
field, MERGED as #534**. A struct field may now be a union (`Message { content: Content }`); with the
earlier slices, the full **`Chat { messages: array<Message> }`** OpenAI chat request round-trips
byte-identically (union field composes with nested / `Option` / `array<Struct>` fields, incl. an
object-payload union value inside an array element via the Mison speculative path). A JSON descriptor
**kind 6** whose `sub` is the `JsonUnion` (reused decode+encode): `field_width`/`write_value` (all
decode paths) + `json_encode_value` grew a kind-6 arm; `encode_union_at` factored out of the FFI
entry; `drop_decoded_owned` skips it (non-owned in v1); all `d.sub` casts null-guarded. Codegen
`json_payload_tag_sub` grew a `Ty::Enum` arm → `(6<<8, emit_json_union)`. Sema
`decode_struct_fields_ok`/`json_object_parts` grew enum-field arms — BOTH call `check_union_decodable`
so `emit_json_union` never sees a bad enum (clean sema error on decode AND encode). MIR
`json_schema_sig_into` expands a union field via cycle-safe `json_union_schema_sig_into` (the union
schema now appears inside the struct sig, invalidating the cache on a variant change). `/align-self-review`
(Gate 1 kind-6 sweep across every descriptor-kind dispatch; Gate 6 cycle-safe schema) + `/code-review`
high (0 findings) both run. Tests: `m5.rs` J1b-2b (field decode/encode, object-payload + Option
coexist, `array<Message>` with object content, non-union-enum-field rejected on both sides). CI green
all 3 platforms. **With J1b (1 + 2a + 2b) complete, the only shape the gateway still can't express is
the multimodal `content: str | array<Part>` union — its `array<Part>` variant needs an enum OWNED
payload, which is J2.** **NEXT: J2 — enum owned `array<Struct>` payloads + tag-switched drop**
(enums become Move; `MoveCheck`/`null_moved_source`/`drop_struct_fields` grow enum arms; the union
descriptor gains an array-shape arm) → the full `Content` union closes the REST gateway → then J3
matrix fill / J4 `json.doc` / J5 `json.scan` / J6 spec sync. Previous update: 2026-07-18,
**JSON COMPLETENESS J1b-2a SHIPPED — top-level shape-directed union
decode/encode, MERGED as #533**. A JSON `oneOf` maps to a sum type discriminated by the value's
**shape class** — Str (`"`) / Number (digit/`-`) / Bool (`t`/`f`) / Object (`{`), an O(1) first-byte
dispatch; encode writes the live variant's payload **bare**, so `decode(encode(x))` round-trips.
Compile-checked: a union-decodable enum has every variant carry one payload, each mapping to one
shape class, all pairwise distinct (`check_union_decodable` + `union_shape_class`). Runtime `JsonUnion`
descriptor (per-variant `JsonField` payload arm + shape-class→arm + arm→enum-tag tables);
`decode_union_value`/`align_rt_json_decode_union` dispatch on the first byte and write via the shared
`write_value`; encode reuses `json_encode_value` (factored out of `json_encode_object`) via
`align_rt_json_encode_union`. New `Rvalue::JsonDecodeUnion` + `TemplatePiece::UnionValue` swept
through EVERY exhaustive HIR/MIR analysis pass (region_of: input-region for a str-bearing union,
Static for scalar-only); `json_union_schema_sig` baked into the MIR for cache invalidation. v1
boundary: str/number/bool/object payloads (owned `array<Struct>` payload = J2); `json.encode` needs a
local binding. Tests: `m5.rs` J1b section (decode-by-shape, encode-bare+round-trip, runtime mismatch
→ Err, escape rejection, 4 compile-time clash/tag-only/no-shape rejections). `/align-self-review` +
`/code-review` high (1 finding — mid-block alloca hoisted to entry) both run. **Also fixed 3
pre-existing CI failures** (main had been red since #530): two stale negative tests whose rejection
earlier slices had already lifted (`generic_enum_invalid_payload_rejected` from J1a #531,
`struct_with_owned_field`'s `array<i64>`-field assertion from Slice C #529 — both flipped to
acceptance), and an x86_64-only stack overflow in `within_limit_chain_compiles_and_runs` (the
in-process test harness compiled a 40-deep expr on the 2 MB test-thread stack — `build_and_run` now
compiles on a 32 MB worker thread). CI green on all three platforms. **NEXT: J1b-2b — union as a
struct field** (`Message { content: Content }`): a new descriptor kind 6 wired into `write_value` /
`json_object_parts`, reusing `emit_json_union`; then **J2** (enum owned `array<Struct>` payloads +
tag-switched drop → the full multimodal `Content` union → the gateway closes). Previous update:
2026-07-18, **JSON COMPLETENESS J1b-1 SHIPPED — enum as a struct field, PR #532**
(the language prerequisite for shape-directed unions; the split first half of J1b). A struct field
may now be a **sum type** — the `Message { content: Content }` shape union decode/encode needs. An
enum is never Move today (`enum_payload_ok`: scalar / `str` / non-Move struct payloads), so an enum
field needs no `Drop`; a `str`-bearing enum field region-ties the enclosing struct (soundness), a
scalar-only enum field stays freely returnable (precision). **Sema:** `is_field_ok` admits
`Ty::Enum`; `struct_has_str_rec` gains an enum arm (so `region_of`/escape catch a struct holding a
`str`-bearing enum field leaving its arena); `ty_size_align`/`struct_size_align`/`struct_abi_layout`
thread the `enums` table + `enum_size_align` (the enum lowers to `{ i32 tag, payloads-in-variant-
order }`, the exact dual of codegen's `enum_types`, pinned by `layout_parity`); `struct_acyclic`
becomes enum-aware with a new **post-0c pass** for cycles through an enum field (`Node{c:E}`,
`E{V(Node)}`). **Codegen:** `enum_types` are built BEFORE the struct bodies (a struct field may be
an enum) and threaded into `set_struct_body`; `field_abi_align`/`logical_to_physical` grow a
`Ty::Enum` arm (align = `max(4, payload aligns)`) so sema and codegen agree on field reordering;
`layout_parity` grows `SEnum*` cases. JSON decode/encode still cleanly REJECT an enum field in sema
(the `json_payload_tag_sub` `unreachable!` stays unreachable) — that is J1b-2. `/align-self-review`
+ `/code-review` high (0 correctness findings; 2 minor comment/redundancy nits fixed) both run.
Tests: `enum_match.rs` J1b section (46 green — construct+match, all payload shapes in-scope,
scalar-only field freely returnable, str-bearing field cannot escape, cycle-through-enum rejected;
Move-struct-owns-string + enum field drops clean); sema (151) + layout_parity green; clippy clean.
**NEXT: J1b-2 — the shape-directed union decode/encode itself** (the descriptor's shape-class table
+ first-byte dispatch over str/number/bool/object payloads; enum as a decode target / union struct
field; encode writes the live payload bare; array payloads = J2). Previous update: 2026-07-18,
**JSON COMPLETENESS J1a SHIPPED — enum `str` payloads with region
tracking, MERGED as #531** (`ee5a1e4`). The design's "region tracking pending" prerequisite for
shape-directed unions. A sum type may now carry a `str`-view payload (and a `str`-bearing
plain-data / non-Move struct payload); the enum is region-tracked iff any variant payload is.
`enum_payload_ok` + pass 0c admit `Scalar::Str` and a non-Move struct; `tracks_region` (method)
gets a precise `Ty::Enum` arm, `region_of(EnumValue)` folds the shortest payload region (moved out
of the `Static` group — the bug fixed), and `ty_may_borrow` (free fn) gets a `Ty::Enum` arm + now
takes the `enums` table (threaded into `EscapeCheck`/`MoveCheck`/MIR `Builder`, all 6 call sites
updated). Verified sound (str escapes caught through direct AND struct payloads, at construction /
match-result / match-binding) AND precise (plain-struct + scalar-only enums freely returnable —
`region_of` gives the Static answer even though `tracks_region(Struct)` is conservatively true;
builtin `Error` + structured-error enums unaffected). Never Move (a `str` borrows) — owned payloads
(array<Struct>, tag-switched drop) are J2. Codegen unchanged (a str payload already lowers as a
flattened `{ptr,len}`). `/align-self-review` (Gate-1 sibling sweep) + `/code-review` high (0
findings; 2 precision/soundness tests added) both run. Tests: `enum_match.rs` J1 section (41
green); fuzz_differential (enum+match, 10) + sema (151) green; clippy clean. **NEXT: J1b — the
shape-directed union decode/encode itself** (a union-decodable enum = pairwise-distinct shape
classes, compile-checked; a new descriptor kind + first-byte dispatch; enum as a struct field /
decode target; encode writes the live payload bare) over str/number/bool/object payloads (array =
J2). Previous update: 2026-07-18, **JSON-COMPLETENESS DESIGN SETTLED — MERGED as #530** (`50b3865`;
owner-directed "design before implementation"). Three forks settled with the owner: (1) **unions =
shape-directed sum-type mapping** — a JSON `oneOf` maps to a sum type discriminated by the value's
shape class (Str/Number/Bool/Object/Array; pairwise-distinct compile-checked; O(1) first-byte
dispatch; `null` is not a class — absence stays `Option`'s; encode writes the live payload bare);
language prerequisite = enum `str` payloads (region tracking) + owned payloads (`array<Struct>`,
tag-switched drop) — enums are today neither region-tracked nor dropped (`enum_payload_ok`). (2)
**dynamic JSON = `json.doc` lazy view; serde-style value tree REJECTED** (per-node alloc +
pointer-chasing vs Nothing hidden/data-oriented; would drag recursive enums + a map type in);
simdjson on-demand over the existing SIMD index, arena tape, zero-copy views; **navigation is total
and Missing-propagating** (design-review catch: `?` is Result-only, so Option-returning `get/at`
would be unusable — absence surfaces once as `None` from a leaf `as_*`); keys-as-data via ordered
`key(i)`+`at(i)` — NO map type enters the language. (3) **catalog trimmed**: `validate<T>` /
`token` / `field_table<T>` DELETED (not left dangling); `core.json` = `decode`/`encode`/`doc`/
`scan` exactly; the no-turbofish schema-selector residual is CLOSED. Recorded across
open-questions ("JSON completeness — DESIGN SETTLED", the implementation source of truth), draft
§14 (Union Mapping / Document View / Streaming sections) + §18.1, design-notes (three rejections'
rationale), history (rejected alternatives), language-spec digest, core-design/json.md (+ja).
**Implementation slices: J1** enum `str` payloads + shape-directed unions (str/number/bool/object)
→ **J2** enum owned payloads → the gateway's `content` union closes → **J3** matrix fill
(`Option<struct>` encode, `array<scalar>` fields, top-level scalars, compositions) → **J4**
`json.doc` → **J5** `json.scan` → **J6** spec sync. **NEXT: J1.** Previous update: 2026-07-18,
**REST-GATEWAY RUNWAY COMPLETE — SLICE C (`array<Struct>` struct fields
+ array decode/encode) SHIPPED, MERGED as #529** (`9a40d60`). A struct field may now be an owned
`array<Struct>` — the `messages: array<Message>` / `choices: array<Choice>` shape; **the full OpenAI
chat-completions request/response now round-trips byte-identically through `core.json`** (Slices
A+B+C composed, `json_full_openai_response_shape_roundtrip`). Language: `is_field_ok` admits
`array<T>`; the field owns ONE heap AoS buffer freed by the struct's `Drop` (`drop_struct_fields`
array arm; `ty_size_align` {ptr,len} 16/8, layout_parity pinned). `struct_acyclic` does NOT recurse
through `array<Struct>` (heap indirection → `Node { children: array<Node> }` trees legal). v1 element
restriction (non-owned, like B): scalar / `str`-view / plain-data struct — `array<string>` /
`array<Move-struct>` rejected at declaration. Decode: descriptor kind 5 (sub = element schema); the
runtime `decode_struct_array_value` parses the JSON sub-array into an owned AoS via `parse_object`
per element (nested/Option element fields recurse), writing {ptr,len}. Encode: a dynamic length
can't unroll, so a `StructArrayField` template piece calls the runtime **descriptor-driven encoder**
`json_encode_struct_array`/`json_encode_object` — reusing the DECODE descriptors (symmetric, handles
nested/Option/str/scalar), swept through every HIR-template pass. **Memory-safety:** the decode
`Err` path frees AoS buffers already written into the partial struct (`drop_decoded_owned`, runtime
dual of `drop_struct_fields`); `/code-review` caught + fixed a nested-Move DOUBLE-FREE (null the
`{ptr,len}` slot after freeing → cleanup idempotent; pinned by
`json_nested_move_struct_array_failure_no_double_free`, alloc==free). **Known constraint:** a Move
struct (owns an array) can't be a `Result`/`Option` Ok payload across a function boundary
(pre-existing) — decode + use in-scope. Deferred: `array<scalar>` field decode, owned-element
arrays. `/align-self-review` + `/code-review` high (1 critical double-free found+fixed) both run;
clippy clean; m5 (119) + runtime (incl. alloc-count leak/double-free) + layout_parity +
fuzz_differential (10, on a `cargo build --workspace`-refreshed archive) green. **⚠️ Two process
lessons this session: (1) NEVER run `cargo fmt` on this repo — it is NOT rustfmt-clean, so `cargo
fmt -p <crate>` reformats thousands of unrelated (committed) lines; if it happens, `git show
HEAD:<file> > <file>` per file reverts, then re-apply functional edits by hand. (2) `cargo test
--features alloc-count` leaves a different-feature runtime archive under `target/debug/deps` — a
later default-feature link (e.g. fuzz_differential) then FAILS at link; `cargo build --workspace`
refreshes it (the known stale-runtime-archive papercut).** **NEXT: the owner-directed
JSON-completeness push** (open-questions "JSON completeness"): enum/union payloads (the OpenAI
multimodal `content` union — finishes the gateway), then settle `JsonValue`/map + streaming/validate
— so no "this JSON shape works, that one doesn't" gap persists. Previous update: 2026-07-18,
**REST-GATEWAY RUNWAY SLICE B SHIPPED — `Option<T>` struct fields +
optional decode/encode, MERGED as #528** (`a45ea5d`; owner chose "implement encode now"). A struct
field may now be `Option<T>` (payload scalar / `str` / nested struct), decoded + encoded with the
settled **null policy**: missing key → None, JSON `null` → None, type mismatch → Err, required field
still Errs when missing; **encode omits a None field entirely** (never `"k":null`), so
`decode(encode(x))` round-trips. Language: `is_field_ok` admits `Option<T>`; sema `ty_size_align` ↔
codegen `option_struct_type` agree on `{i8 tag, payload}` (layout_parity extended, every payload kind
+ reorder + layout(C)); `struct_acyclic`/`struct_has_str`/`tracks_region`/`ty_may_borrow` recurse
through Options. **v1 boundary: Option payload must be NON-OWNED** (`Option<string>`/`Option<Move-struct>`
rejected at declaration — owned-Option-drop-as-a-field has no consumer, and table-free
`Scalar::Struct.is_move()` would else mis-classify+leak; zero owned-drop surface, covers the whole
json consumer). Decode: `JsonField` gains `opt_tag` (-1=required, else Option tag byte offset), optional
fields exempt from `all_required_seen`, single-sourced `write_value` shared by slow+Mison paths. Encode:
an Option-bearing object uses a trailing-comma layout + one `align_rt_builder_pop_comma` before `}`
(`{"a":1}`/`{}`), via new `TemplatePart::OptionField`/`PopComma` swept through every HIR-template
analysis pass; pure-required objects keep the static layout (zero regression). **One recorded follow-up:
`Option<struct>` ENCODE** (decode supports it). `/align-self-review` + `/code-review` high (0
correctness findings; 1 test-coverage gap — nested×Option compose — pinned) both run. Tests: m5 (null
policy, Option<Struct> array via Mison, omit-None encode, round-trip, nested×Option compose),
layout_parity Option cases. Docs: open-questions runway (Slice B shipped) **+ the owner-directed
JSON-completeness plan** (after Slice C: close enum/union payloads = finishes the gateway, then settle
JsonValue/map + streaming/validate — so no "this JSON shape works, that one doesn't" gap persists),
json.md (+ja), draft §14, language-spec. clippy clean; fuzz_differential (10) + encode/json suites
green (macOS-sandbox network/fd + gate7 UUID flake fail on pristine main too). **Next: REST-gateway
runway Slice C (`array<T>` struct fields — the `messages: array<Message>`/`choices: array<Choice>`
shape), then the JSON-completeness push (enum/union payloads → JsonValue/map → streaming/validate).**
`LLVM_CONFIG=/opt/homebrew/opt/llvm/bin/llvm-config` is also needed for codegen build.rs on this Mac
(added to the macos-build-env memory). Previous update: 2026-07-18, **REST-GATEWAY RUNWAY SLICE A SHIPPED — nested-struct JSON
decode/encode, MERGED as #527** (`993faac`; owner-directed "次の予定" = the filed runway). A
`json.decode`/`json.encode` struct field may now itself be a `Struct`: decode recurses (runtime
kind-4 `JsonSubTable` pointer; `parse_object` slow path AND `write_field_indexed` Mison speculative
path both recurse via `decode_nested` — a nested field is one record-level colon whose value the
record-splitter leaves at a deeper bracket depth, so the flat colon-ordinal speculation is
undisturbed, P1/P2 honored), encode recurses through `json_object_parts` (extended field `path`),
and a nested record round-trips in declaration order (the OpenAI chat-completions `usage: Usage`
shape). `IndexField` generalized single-`field`→`Vec<u32>` path (reusing `elem_field_ptr`/
`phys_field_indices`) so a fixed struct-array element with a nested field encodes uniformly — no
partial support. `struct_has_str` recurses so a nested-`str` struct is region-tied to the input.
Codegen `emit_desc_table` recurses to emit one `JsonSubTable` global per nested struct type
(acyclic → terminates); descriptor LLVM struct gains a `sub` ptr matching the runtime `#[repr(C)]`.
**Also fixed a pre-existing stale-cache miscompile this slice would have extended (#514/#517
class):** a decode target struct's field name/type feeds only the codegen descriptor, not the
surrounding MIR, so a field RENAME at the same slot (or a nested struct's field change) left
`impl_hash` unchanged and the warm object cache served a stale object decoding the OLD key
(reproduced end-to-end). The `JsonDecode*` MIR rvalues now bake a recursive `json_schema_sig`
(names + types via sign/width-faithful `ty_name` + `layout(C)`/`align`, nested expanded) printed
into the MIR; pinned by `cache_codegen.rs` gate 2b (flat + nested). `/align-self-review` (added the
`field_width` negative-size guard + `json_object_parts` cycle guard) + `/code-review` high (0
correctness findings; 1 test-coverage gap — fixed struct-array nested encode — pinned) both run and
reflected. Tests: m5 (`json_decode_encode_nested_struct_roundtrip`, `..._array_mison`,
`json_encode_fixed_struct_array_with_nested`), runtime descriptor-level slow + Mison recursion,
cache gate 2b, example `examples/json_nested.align`. Docs: json.md (+ja mirror), draft.md §14,
language-spec, open-questions runway (Slice A marked shipped). Workspace clippy clean; json/cache/
mir/sema suites green (the 10 `align_runtime` network/fd tests + the `cache_codegen` gate7 Mach-O
`LC_UUID` exe-byte flake fail on pristine `main` too — macOS-sandbox environmental, cataloged).
**Next: REST-gateway runway Slice B (`Option<T>` struct fields + optional-field decode) then Slice C
(`array<T>` struct fields — the `choices: array<Choice>` request shape).** Slice B settles the null
policy: missing key → `None`; JSON `null` → `None`; type mismatch → `Err`; `encode` omits a `None`
field entirely. Both slices need the language-side `is_field_ok` extension (today rejects
`Option<T>`/`array<T>` fields) + layout/MoveCheck/escape field tracking — the sibling-type-class
sweep (`/align-self-review` Gate 1). Previous update: 2026-07-18, **v0.1.0 RELEASED + brew/apt DISTRIBUTION LIVE**
(owner-directed release). Tagged `v0.1.0` (annotated) + GitHub release with curated notes and the
CI-built artifacts (3 tarballs, amd64/arm64 `.deb`, `align.rb`, checksums). Fixed the release
workflow's `cp LICENSE` bug (repo is dual-licensed `LICENSE-APACHE`+`LICENSE-MIT`, no bare `LICENSE`
— PR #523) and re-ran it via `workflow_dispatch --ref main -f tag=v0.1.0` (fixed logic against the
tag tree, no tag move). **Homebrew**: created the `sanohiro/homebrew-align` tap, seeded
`Formula/align.rb` manually, cleaned the formula + `.github/align.rb.template` (dual license, dep
order — PR #524); `brew tap sanohiro/align && brew install align` verified end-to-end on this Mac
(compiles+runs). **apt**: generated an align-specific GPG signing key (`align apt signing key`,
`rsa4096/6981E7F9B2BCCBE7`, in local keyring + the `APT_GPG_PRIVATE_KEY` repo secret), enabled
GitHub Pages on gh-pages; the signed repo serves at https://sanohiro.github.io/align/ (InRelease
verifies, amd64+arm64 debs + `install.sh` live). READMEs now document brew/apt install (EN+JA).
**ONE remaining optional step** (needs the owner's credential I can't set): add the
`HOMEBREW_TAP_TOKEN` repo secret (the same PAT used for `ze`/`bcon`) so future releases auto-update
the tap — until then the tap formula is seeded manually per release (apt auto-updates already, its
secret is set). Previous update: 2026-07-18, **NEON JSON STRING-ESCAPE CLASSIFIER ACTIVATED ON aarch64 — PR #520**
(owner-directed after the #517 stop: "the ARM speed investigation, x64 already done"). #504 shipped
the AVX2/SSE2 escape classifier for `align_rt_builder_write_json_str` on x86-64; its NEON kernel
(`json_escape_map_neon` / `write_json_str_neon`, shrn-by-4 lane map) was written + differentially
tested but left `#[cfg(all(aarch64, test))]` — production aarch64 stayed scalar pending a native-ARM
no-regression measurement. This session ran that measurement on **this Apple Silicon Mac** (the
native hardware the deferral waited for). Forced NEON-vs-scalar sweep (median-of-9) placed the
crossover at the first full 16-byte block (8 B 0.95x / 12 B 0.74x → 16 B 1.70x / 32 B 3.03x / 1 KiB
9.77x); activated `JSON_ESCAPE_SIMD_MIN=16` (aarch64) + a NEON dispatch arm + promoted the two
kernels & `write_json_str_tail` to production cfg. End-to-end adoption gate through the real builder:
mostly-clean **6.04x** (WIN), escape-dense 2.10x, short 1.11x — all pass doc-13 §6.6; the existing
byte-for-byte differential oracle (already covering the NEON path) passes on this hardware, so
production NEON output is identical to scalar. Clippy clean, non-test build clean. Refreshed the
stale open-questions note (Base64 @48 B / hex @16 B were already M1-measured & active; this lands the
escape classifier). CI has no ARM runner, so the NEON path is hardware-validated here, matching the
existing json_decode_index/utf8/base64/hex NEON precedent. Remaining aarch64 SIMD item = the separate
UTF-8 portability run. Previous update: 2026-07-18, **LOCAL CONSTANT-ARRAY POOLING IS SHIPPED — MERGED as #517**
(`dc34e19`) — **and with it the owner-directed work program of 2026-07-16/17 is COMPLETE.** A
qualifying local `xs := [const…]` (non-mut, non-align(N), fixed scalar array, all-constant
elements, **length ≥ 32** — the empirical cutoff) lowers to one `llvm.memcpy` from the #514
rodata global; MemCpyOpt then folds the read-only slot into the constant (runtime-indexed reads
become `gep @const_arr`). Type-preserving by design (NOT the slice rewrite — correct for every
downstream use). Measured: 1.40x at N=32 rising to **288x at N=4096**; below the cutoff both
toggle states emit identical code. Negative controls IR-pinned; #506 donation structurally cannot
fire; `ALIGN_CONST_POOL=off` toggle + cache guard. The /code-review pass caught a CRITICAL
stale-cache miscompile before merge (reproduced end-to-end): the MIR printer omitted const-array
ELEMENT VALUES while `impl_hash` hashes that text, so a value-only table edit (999→111) served the
stale cached object under the warm default-on cache — fixed by value-exact element rendering in
BOTH print arms (also covering #514's private aggregate constants), pinned by a subprocess
warm-cache repro gate and a printed-MIR inequality gate. Suite hygiene: the pre-existing
`gate_sv4` PGO wall-time payoff assert flaked twice under parallel suite load (passes in
isolation) — converted to an `#[ignore]` manual probe per the perf-probe discipline, roadmap claim
corrected. Workspace fully green (WORKSPACE_EXIT=0 checked), clippy clean. **Session totals
(2026-07-16 → 07-18): 12 merged PRs** — adaptive stable-sort #494; the ThinLTO arc #495–#498
(CLOSED); the instrument-PGO arc #499–#502 (CLOSED, settled policy amended on review evidence);
doc-13 P3 item 3's four gates #503/#504/#506/#510 (all dispositioned); the aggregate-constants
language surface #514 (+ the rodata-write soundness hole fixed pre-merge); this pooling probe
#517. The owner's release-prep agent landed #507–#509 + LICENSE in parallel; v0.1.0 tagging is
the owner's step. **Remaining queue: EMPTY by owner direction — the loop STOPS here.** The next
fork is the owner's: the align-LLM engine (unblocks doc-14 and the consumer-gated deferrals) or
further language surface. Deferred items live in open-questions (mmap-view write provenance
follow-up, struct constants S1.5, PGO×ThinLTO, sample-PGO/BOLT — blocked on perf-less WSL2,
aarch64 SIMD activations pending native hardware). Previous update: 2026-07-17 (eleventh update
this day), **TOP-LEVEL AGGREGATE CONSTANTS ARE SHIPPED —
THE LANGUAGE SURFACE, MERGED as #514** (`11e0f26`; stray scratch files removed in `960a753`). A
top-level constant initializer may now be an array literal: `PRIMES := [2,3,5,7,11]` /
`SCALE: slice<f64> := [...]` / `DAYS := ["Mon","Tue","Wed"]`. The type is **`slice<T>` with
`Region::Static`, never `array<T>`** (ownership is a property of the type; the
`GREETING := "hello"` → `str` analogy; an `array<T>` annotation is rejected with guidance), backed
by per-unit `[N x T] private unnamed_addr constant` rodata (str elements = {ptr,len} pairs into
the string pool); a constant index folds to the element (no load); pipelines/indexing flow through
the existing borrowed-view paths allocation-free. Initializers v1 fail-closed: elements fold via
the existing ConstEval (literals/operators/scalar-const refs); Int/Float/Bool/Char/str; struct
constants deferred to S1.5 (recorded). Cross-unit invalidation is free via IConst.value_src →
interface hash, now pinned by a real CacheOutcome gate. The /code-review pass caught a REAL
soundness hole before merge (both reviewers independently proved it with running programs): writes
through a constant's slice view compiled into rodata stores (dev SIGSEGV / O2 silently deleted
store) via `mut` rebind AND `out slice<T>` args — fixed at the right altitude with a readonly-view
provenance set gating EVERY slice-write entry point (element assign, .store, .map_into, .shuffle,
out-args) through one shared rejection; static `.bytes()` closed by the same rule; the
arena/mmap-view analogue recorded as a pre-existing open follow-up. Also: the producer-side
pub-const initializer check (a pub const may reference only pub consts — closes the
whole-vs-per-unit D1 divergence, applied outright to the pre-existing scalar shape), per-unit
same-verdict coverage, diagnostic-voice fixes, the tutorial's aggregate-constant section (EN +
natural-JA mirror), and pipeline-guard regression pins (a local named like an imported module).
Spec sync per CLAUDE.md: draft.md §Constants + §12 + the relaxed pub rule, language-spec mirror,
design-notes rationale, open-questions §27 Settled + the deferred-clause removal. Workspace green
(**2332 tests**, exit-checked), clippy clean all feature states. **Next (the final work item
before the owner-directed STOP): S3 — the doc-13 §8.4 local all-constant array literal pooling
probe** reusing the S1 rodata mechanism (immutable non-escaping local `[...]` literals redirect to
the pooled constant instead of alloca + O(N) stores; gates: ≥15% on the large positive case, no
O(N) store sequence in IR, ≤3% below the empirical cutoff), then the FINAL SUMMARY and stop.
Previous update: 2026-07-17 (tenth update this day), **THE ADAPTIVE SHORT-N STRING GROUP STRATEGY
IS
SHIPPED — MERGED as #510** (`afbd1e6`) — **and with it doc-13 §11 P3 item 3 is FULLY DISPOSITIONED
(all four measure-first gates: repeated-needle #503, JSON escape SIMD #504, unique-buffer donation
#506, short-N group #510).** The str-key single-aggregate group core is adaptive: distinct-group
count ≤ `GROUP_LINEAR_MAX = 4` aggregates by linear scan over the caller's already-written
representatives (no map allocation, no per-row wyhash; byte compare + length fast-reject),
promoting to the identical seeded HashMap at the 5th distinct key with ids and first-occurrence
order continuing unbroken and the triggering row processed exactly once. Measured (AB/BA,
median-of-9, real core): cardinality 1–4 wins 1.07–2.31x in both mixed- and same-length keys, ≥5
neutral at scale; the record's hypothesized 8/16 crossover was empirically corrected to 4
(threshold 8 regressed same-length cardinality 6–8 to 0.68x). dict_encode/group_multi stay on
their own path (scoped honestly). /code-review: functional audit clean on all six axes; two
comment-accuracy fixes applied pre-merge. Differential gates pin bit-identical results incl.
first-occurrence order across cardinalities/orders/widths and cap-overflow in both phases.
Workspace green (exit-checked), clippy clean incl. --all-features. **In parallel, the owner's
release-prep agent landed #507 (build/release automation), #508 (README/toolchain docs), #509
(spec/guide updates) + LICENSE on main — v0.1.0 tagging is the owner's post-merge step.** **Next
(the final arc before the owner-directed STOP): top-level aggregate constants** — the language
surface (two-lens design settlement: static-region slice-view ownership per the string-literal
precedent; v1 initializers = literals + simple folding, fail-closed), implementation (~2-3 PRs),
then the doc-13 §8.4 local-constant pooling probe (≥15% large positive, ≤3% below cutoff), with
draft.md/language-spec/design-notes/open-questions updated per CLAUDE.md. Previous update:
2026-07-17 (ninth update this day), **UNIQUE-BUFFER DONATION IS SHIPPED — MERGED as
#506** (`2aa3357`; doc-13 P3 item 3, gate 3 of 4; doc-10 §8.1 is the owner record). A materializing
pipeline whose source is a fresh unbound owned array (`temp_free`) now donates the source buffer
as the result instead of allocate+copy+free — exactly the record's mechanically safe subset (heap
only, identical scalar layouts, map/where/scan; no arena/views/zip/struct-view/Move payloads),
with NO new IR variant (output ptr = `SlicePtr(source)`, the exit `DropValue` skipped — ownership
transfers). Behind `ALIGN_BUFFER_DONATE=off` (toggle precedent incl. the cache force-off guard,
gate13c). Measured (bench/buffer_donate, AB/BA min-of-7): 1.04–1.07x in L1/L2, **2.03–2.25x from
512 KiB through 64 MiB**; alloc==free balance pinned both ways. /code-review found the mechanism
sound on every axis (single-free invariant, fail-closed predicate, in-place ordering) with one
finding applied: the doc-claimed donation-on/off EXECUTION differential now actually runs
(subprocess `alignc run` pair over bound + escaping + scan donated results, byte-identical stdout
+ hand-computed references). Suite 10/10; workspace green with properly checked exit codes.
**Also diagnosed this session: the recurring `per_unit_surface` byte-identity failure was the
known stale-runtime-archive papercut** (target/debug vs deps divergence after feature-flag builds
— alloc-count etc. leave a different-feature archive under deps) — environmental, fixed by
`cargo build --workspace`, unrelated to any merged PR; AND the verification lesson that piped
`cargo test | grep | awk` masks exit codes — full-suite checks must assert the exit status
separately. **Next: doc-13 P3 gate 4 of 4 — the short-N group strategy gate** (measure-first,
independent), then the owner-approved top-level aggregate constants arc (language surface +
doc-13 §8.4 pooling), then STOP per the owner's instruction. Previous update: 2026-07-17 (eighth
update this day), **THE SIMD JSON ESCAPE CLASSIFIER IS SHIPPED —
MERGED as #504** (`63d64bf`; doc-13 P3 item 3, gate 2 of 4). `align_rt_builder_write_json_str`'s
scalar per-byte escape scan is replaced above the empirically confirmed 32-byte crossover by a
dispatched block classifier (AVX2 32-byte / SSE2 16-byte, one movemask per block, branchless
`subs_epu8(c,0x1F)` — DEL 0x7F untouched, 0x80–0xFF clean) with bulk copies across clean spans;
full-vector loads only while `i+BLOCK<=len`, scalar tail, no over-read. End-to-end through the
real builder (median-of-9, AB/BA): mostly-clean **5.80x/5.69x** (baseline/v3), escape-dense
**1.51x faster**, short 1.10x neutral. The differential oracle pins every path (incl. forced SSE2
on an AVX2 host) byte-for-byte across every C0 class, quote/backslash, straddling multibyte,
lengths through 4096. The aarch64 NEON candidate (shrn-by-4) compiles test-only; production
aarch64 stays scalar recorded-pending native hardware (the Base64 #487 precedent). /code-review:
zero correctness findings; one quality fix applied (an overstated `unsafe` marker dropped).
Workspace green, clippy clean all feature states; doc-13 §6.6/§11/§12 updated. **Next: doc-13 P3
gate 3 of 4 — unique-buffer donation** (doc-10 §8.1 ownership proof; measure-first), then the
short-N group strategy gate, then the top-level aggregate constants arc, then STOP per the
owner's instruction. Previous update: 2026-07-17 (seventh update this day), **REPEATED-NEEDLE
PLAN HOISTING IS SHIPPED —
MERGED as #503** (`b59d65e`; doc-13 P3 item 3, gate 1 of 4). `xs.where(fn s {
s.contains(NEEDLE) })` with an invariant needle now builds ONE memchr plan in the loop preheader
(`align_rt_str_finder_new/find/free`; the plan owns its needle copy) and reuses it per element —
LLVM structurally cannot do this (search entries withhold `memory(...)` for the ifunc dispatch
cache; the Finder build lives inside the FFI call). Recognition (sema, WhereField precedent →
`StageKind::WhereStrContains`) is restricted to a **bare free-variable path or string literal
only** — the /code-review pass PROVED the original parameter-free-expression walker changed
observable semantics (an impure needle call ran once instead of per-element; a trapping index
aborted before an empty loop; a `?`-needle flipped program legality) — non-atom needles keep the
per-call path, pinned by gates. The plan is a synthetic-owned Move resource (`Ty::StrFinder`,
`Rvalue::StrFinderNew/Find`) wired exhaustively through every analysis pass; freed exactly once on
every exit path; a real feature-gated leak gate (finder new==free counters) lives in
bench/needle_hoist. `finder_find` keeps the `feature_detect_reader` contract — the argmem-read
upgrade was falsified for 1-byte needles against the memchr 2.8.2 source (one-byte searcher hits
the AtomicPtr dispatch cache at find time). The honest identical-pipeline measurement (the
`ALIGN_NEEDLE_HOIST=off` MIR toggle, ALIGN_SORT_ADAPTIVE precedent incl. the cache force-off
guard) holds the adoption gate: **≤128 B geomean 1.95x** (1.42–3.37x), 1 KiB 1.09–1.19x, 16 KiB
1.00–1.01x neutral. Deferrals recorded in §6.6: map(find/rfind), explicit-loop shapes,
replace/split, owned-`string` needle borrow-once, field-of-path atoms; no public Pattern type
(settled). Gates: differential vs the one-shot oracle (fuzz edges + seeded 3000), IR-shape
preheader/body mutation-checked, negative controls (element-derived/impure/indexed/`?` needles),
drop/double-free, run-parity, the toggle pin. Workspace green; clippy clean all feature states
(the one full-suite flake was the pre-existing gate_sv4 PGO timing gate — passes in isolation,
re-verified). **Next: doc-13 P3 gate 2 of 4 — the JSON escape SIMD classifier** (P1-promoted,
scalar <32 crossover; adoption gate = end-to-end builder benchmark on x86 baseline/v3 +
differential tails + every control-byte class; the arm64 half stays recorded-pending native
hardware), then unique-buffer donation, then the short-N group strategy gate, then the
owner-approved top-level aggregate constants arc (language surface + doc-13 §8.4 pooling), then
STOP per the owner's instruction. Previous update: 2026-07-17 (sixth update this day),
**INSTRUMENT-PGO SV IS SHIPPED AND THE PGO ARC
IS CLOSED — MERGED as #502** (`0355782`; the full arc: S0 #499 spike → S1 #500 serial → S2 #501
cache → SV #502 verification + payoff). The SV bundle (`crates/align_driver/tests/pgo_sv.rs`, 7
gates): build-twice byte-identity for instrument AND use modes (separate cold roots, coldness via
`--cache-stats`), wrong-program / stale-source / version-corrupt profile mutation gates, the
interleaved compile-time bound (< 3.0x both modes), and the **measured PAYOFF GATE: ≈1.16x at
N=40M** (1.11–1.13x at larger N, ±0.005 stable) from profile-driven hot-loop block layout —
`step` inlined in BOTH builds (objdump-confirmed), so the win is layout, not inlining; floor
asserted at 1.03x. Two dispatch kernels (8-/16-way skewed) measured ~0.98x/0.88x — hardware
already predicts skewed dispatch — recorded as the durable negative so the sample-PGO/BOLT slice
doesn't re-measure them. **The settled 0%-match hard-error policy was AMENDED on review
evidence** (bracketed correction in the roadmap): no reliable match signal exists — the
post-pipeline entry-count tally undercounts inlined+DCE'd matches (false hard error on a
same-program stale profile), per-unit cache HITs structurally bypass any build-level gate, and
--rt-lto baked primitives match every rt-lto profile — so 0%/partial match ships as ONE prominent
aggregated warning with exit 0 (clang parity; profile mismatch is performance-only), while hard
errors stay at the reliable layer (missing/unreadable/empty/bad-magic profdata + Error-severity
diagnostics incl. version skew). The match-tally plumbing (shim out-params →
`UnitCodegen.pgo_matched/pgo_total`) feeds the warning; the `!any_hit` guard and rejected-build
deferred publish were deleted and immediate in-worker publish restored (objects are valid for
their keys regardless of ratio — the key carries the profile-content digest; doc-10 records the
semantics). `make_profdata` generalized in tests/common. Workspace green (**2272 total = 2254
passed + 18 ignored**), clippy clean default + --all-features. Deferred (arc closed): sample
PGO / BOLT evaluation (the M14 wave tail), CSPGO, PGO × --thin-lto composition. **Next: pick the
next work item from the recorded priorities** — candidates: the M14 wave tail (sample-PGO/BOLT
evaluation, "driver-managed external pipeline, evaluate later"), doc-13 P3 item 3's four
independent measure-first gates (unique-buffer donation / repeated-needle plan-hoisting / JSON
escape SIMD classifier / short-N group strategy), or the deferred ThinLTO follow-ups; the
orchestrator selects per the owner rubric. Previous update: 2026-07-17 (fifth update this day),
**INSTRUMENT-PGO S2 IS SHIPPED — CACHE
COMPOSITION, MERGED as #501** (`0066b42`). PGO builds now flow through the normal cached +
parallel per-unit path (the S1 bypass is deleted outright): `PgoKey { Off | Instrument |
Use(Hash128 of profdata bytes) }` is CodegenKey component #12 (the rt_lto_digest precedent; named
`PgoKey` to disambiguate from the path-carrying CLI `PgoMode` — doc-10 §6.2 and the settled
record carry the correction), `FirstDiff::PgoProfile` (exhaustive matches),
`CACHE_KEY_FORMAT_VERSION` and `MANIFEST_FORMAT_VERSION` both 2→3 (fail-closed decode everywhere
manifests are read), `slot_digest` deliberately excludes the mode so switches diff to
`PgoProfile`. Use-phase warnings are mutex-collected, DAG-ordered per unit; an all-HIT use build
re-reports nothing (staleness was reported at first build — documented); the instrumented link is
keyed off the CLI mode, not off whether codegen ran, so all-HIT instrument builds still link the
profile runtime. The /code-review fallback caught a REAL cache-poisoning bug the implementation
had misclassified as benign TOCTOU: the digest came from one read while libLLVM re-read the LIVE
user path per unit miss, so a mid-build profdata rewrite published B2-built objects under the B1
key (a later genuine-B1 build would HIT and silently link them). Fixed structurally: the digested
bytes are snapshotted once to a private per-invocation RAII `StagedProfdata` whose path is handed
to every emit — hit ⟹ byte-identical is restored, pinned by a plumbing gate. Gates: new
`pgo_cache.rs` (9: mode isolation incl. instrumented-never-serves-ordinary, cold→re-hit per mode,
profdata edit → `PgoProfile` miss, revert → old blob re-hit, path-independence, cold-vs-hit
byte-identity both modes, rt-lto × pgo-use disjointness) + the snapshot plumbing gate; all prior
suites green (pgo 7 / cache_codegen 15 / cache_parallel 9 / thin_lto_cache 8 / thin_lto_sv 11);
full workspace green, clippy clean all feature states. Test fixtures (BRANCHY/hh/
profile_rt_available/make_profdata) hoisted to tests/common. **Next: PGO SV — the verification
bundle closing the PGO arc**: build-twice determinism both modes, stale/wrong-profile mutation
gates, the explicit compile-time bound, and the measured PAYOFF GATE on a branch-heavy kernel
(bench-style, interleaved) — then the M14 wave tail (sample-PGO/BOLT evaluation) or the next
recorded priority. Previous update: 2026-07-17 (fourth update this day), **INSTRUMENT-PGO S1 IS
SHIPPED — SERIAL
CORRECTNESS BEHIND `--pgo-instrument` / `--pgo-use`, MERGED as #500** (`913724b`). The shim PGO
entry is a production component (the `pgo-spike` feature and `pgo_spike.rs` were deleted outright;
the 5 spike tests retargeted onto production `src/pgo.rs`); the driver wires both flags (mutually
exclusive, release/fast only, build/run/size, loud rejection elsewhere and with `--thin-lto`;
`--rt-lto` composes); N>1 wires the same per-module pipeline swap per unit, serially, verified
end-to-end on a 2-module program; the object cache is bypassed under either flag (S2 integrates
the `PgoMode` key component) with the explicit `--cache-stats` bypass line. The S0 ELF friction is
applied (instrumented links add `-Wl,--undefined=__llvm_profile_runtime` + the clang_rt.profile
archive, now probing BOTH the flat and per-target runtime layouts) and the fail-loud policy lives
in the driver: profdata pre-validation (exists/readable/non-empty/valid magic both byte orders,
hard errors naming the path) + a context diagnostic handler where Error severity hard-fails and
EVERY Warning-severity diagnostic is reported verbatim (the keyword filter was removed as
fail-open during review). Gates (`crates/align_driver/tests/pgo.rs`, 7 + extended rejection
matrix; 5 codegen `pgo::` unit tests): gen writes a non-empty profraw + prints the destination to
stderr; the full gen→run→llvm-profdata merge→use round trip; `!prof`/`__profc_` IR mutation
checks; run-parity off/instrument/use; fail-loud profdata errors; flag-off byte-identity (also
pinning the review's build_program_module prologue dedup). The /code-review fallback confirmed 8
findings, all applied pre-merge — notably the parse_pgo flag-swallowing bug (root cause: flag
strippers ran before parse_pgo, exposing the verb; fixed by ordering + a likely-flag guard).
Workspace green (**2255 total = 2237 passed + 18 ignored**), clippy clean in default +
thinlto-spike states. **Next: PGO S2 — cache composition** per the settled record: the
`PgoMode { Off | Instrument | Use(Hash128 of profdata bytes) }` component on `CodegenKey` (the
rt_lto_digest precedent), `FirstDiff::PgoProfile`, `CACHE_KEY_FORMAT_VERSION` bump, re-route PGO
builds through the normal cached/parallel path, with instrumented-vs-ordinary isolation, digest,
revert, and cold-vs-hit gates; then SV (determinism both modes, stale/wrong-profile mutations,
compile-time bound, and the measured payoff gate on a branch-heavy kernel). Previous update:
2026-07-17 (third update this day), **INSTRUMENT-PGO IS DESIGN-SETTLED AND ITS S0
SPIKE IS GO — MERGED as #499** (`f45f627`). The M14 wave remainder head after the closed ThinLTO
arc. Design settled by a two-lens review + orchestrator decisions, recorded as the roadmap's
"Instrument-PGO design SETTLED" paragraph (the S1/S2/SV source of truth): mechanism = ONE new shim
entry `align_pgo_run_pipeline` (PassBuilder + the 10-arg LLVM-22 PGOOptions +
buildPerModuleDefaultPipeline, IRInstr and IRUse) — llvm-sys 221 has NO PGO surface (verified
setter list; the earlier "likely via raw llvm-sys" guess is CORRECTED) and textual pipelines
cannot express instr-use; flags = opt-in `--pgo-instrument` / `--pgo-use <file.profdata>`
(mutually exclusive, release/fast only, build/run/size); PGO × `--thin-lto` rejected loudly in v1;
`--rt-lto` × `--pgo-use` composes; no merge wrapper (llvm-profdata-22 direct, documented
prerequisite); profraw = LLVM default + LLVM_PROFILE_FILE with the destination printed; stale
policy = missing/version-skew/0%-match hard error, partial-stale one aggregated report then
proceed; cache identity = a `PgoMode { Off | Instrument | Use(Hash128) }` key component (rt_lto
precedent). The S0 spike (feature-gated `pgo-spike`, 5 ignored tests, default builds
byte-identical) proved the round trip on a REAL Align binary: gen → 288-byte .profraw at normal
exit despite M13 internalization + gc-sections (llvm.used/SHF_GNU_RETAIN pin the counters) →
llvm-profdata-22 merge → IRUse → `!prof branch_weights` with a live diagnostic handler proving no
hash mismatch. Two load-bearing frictions recorded: ELF omits the `__llvm_profile_runtime`
reference, so the S1 driver MUST add `-Wl,--undefined=__llvm_profile_runtime` + the
clang_rt.profile archive to instrumented links; and the shim's USE rc CANNOT signal a
missing/corrupt profdata (libLLVM diagnoses on the context / exits without a handler), so the
DRIVER pre-validates the profdata and installs a diagnostic handler — the fail-loud policy lives
in the driver. Reviewed via /code-review (three findings, all applied pre-merge). Workspace green,
clippy clean in default/pgo-spike/thinlto-spike states. **Next: PGO S1 — serial whole-program
correctness behind the two flags, cache bypassed** (gates: gen links/runs/writes profraw; use
shows !prof mutation-checked; run-parity off/instrument/use; the full rejection matrix; fail-loud
profdata errors incl. pre-validation; flag-off byte-identity), then S2 (cache), then SV (incl. the
measured payoff gate on a branch-heavy kernel). Previous update: 2026-07-17 (second update this
day), **THINLTO SV IS SHIPPED AND THE THINLTO ARC IS
CLOSED — MERGED as #498** (`e804223`; the full arc: S0 #495 spike → S1 #496 serial → S2 #497
cache/parallel → SV #498 verification). The SV bundle (`crates/align_driver/tests/thin_lto_sv.rs`,
11 gates after review fixes): build-twice determinism across separate cold cache roots and across
`-j1/-j2/-j4` (one 3-build matrix), an end-to-end subprocess determinism gate hardened against
vacuous passes (exe deleted between builds, cold independence asserted via `--cache-stats`), a
different-`j` hot-serve byte-identity gate closing the settled plan's cold-vs-hit item beyond S2's
same-`j` pin, summary-level stale-mutation gates proving rejection is BY CONTENT DIGEST (swapping
structurally valid bitcode — another unit's, and a different-body same-unit race shape — hits
`CorruptEntry` → evict+rebuild; the audit found `materialize_blob` already digest-verifies every
CAS blob, so NO product hole existed), an interleaved-AB compile-time regression gate
(`--thin-lto`/off ratio < 3.0, measured ~1.0-1.1x), the key-component invalidation matrix
(llvm-version, compiler-build-id, rt-lto on/off never mix), and an end-to-end pin that the rt-lto
merge changes prelink bitcode (grounding the backend key's transitive rt-lto capture; the
merge-precedes-prelink invariant is documented at `build_thin_lto` and in the roadmap). Because
gemini-code-assist is sunset, this PR was reviewed via the `/code-review` fallback (8 finder
angles): 10 findings confirmed and ALL applied pre-merge — including hoisting the triplicated
ThinLTO fixture cluster into `tests/common/mod.rs`, exposing `cache::cas_blob_path` and
`build_thin_lto`'s written prelink paths (`ThinLtoBuild`) so tests ask the product instead of
re-deriving private conventions, moving the separate-compilation record Open→Settled in
`open-questions.md` with the opt-in-only scope qualifier restored (cross-module inlining returns
ONLY under `--thin-lto` on release/fast; default/debug multi-file builds keep zero cross-unit opt
by design), and the doc-10 §7 ThinLTO stage status. Deferred (recorded, not blockers): cross-unit
pub internalization, digest-precision evolution, ThinLTO-aware explain-opt/emit-llvm. The complete
workspace is green (**2243 total = 2230 passed + 13 ignored**) and clippy passes with warnings
denied in both feature states. **Next: instrument PGO — the M14 remainder head** (recorded shape:
InstrProfiling pass + profile runtime hook + `llvm-profdata` merge + `PGOOptions`, likely via raw
llvm-sys since inkwell does not expose it; sequenced after LTO in the wave order; start with the
two-lens design review + a feasibility spike, the ThinLTO pattern). Previous update: 2026-07-17,
**THINLTO S2 IS SHIPPED — CACHE COMPOSITION + PARALLELISM, MERGED as
#497** (`c5a0244`). `--thin-lto` builds are now incremental and parallel: PHASE 1 prelink
(parallel, cacheable per unit; artifact = summary-bearing `.bc` in a new CAS `prelink` namespace)
→ PHASE 2 thin-link (serial, uncached, fresh import/export lists every build) → PHASE 3 backend
(parallel, cacheable; key = own prelink digest ⊕ inbound import list ⊕ import-source prelink
digests ⊕ outbound export/promotion set ⊕ backend/target bits; `thinbackend` namespace). Both
phases run in the existing `thread::scope` atomic-claim loop in DAG index order; `FirstDiff` names
the missing phase (`PrelinkInput`/`CrossUnitImports`); explain-opt stays serial/uncached/
non-ThinLTO. Headline win pinned: editing a dep's private body misses only that unit's prelink and
exactly the importing units' backends — a unit importing nothing from it hits both phases. Key
hygiene: `CACHE_KEY_FORMAT_VERSION` and `MANIFEST_FORMAT_VERSION` both 1→2 (fail-closed
`UnknownVersion` decode), and the reserved `CodegenKey::cross_unit_opt_digest` was REMOVED
outright — the separate phase keys/namespaces supersede the empty/non-empty-digest scheme with
structurally stronger toggle isolation; doc-10 §6.2, the roadmap settled paragraph, and
open-questions carry appended supersession corrections. Two recorded soundness deviations: the
outbound export set is keyed (entry 3 promotes own locals per export flags) and `target_triple`
stays in the prelink key (datalayout). Gates: the 8-gate `thin_lto_cache.rs` suite (incremental
win, pub-signature transitive miss, import-sensitive precision, toggle isolation, cold-vs-hit
byte-identity through both phases, parallel == `-j 1`, cross-process all-hit, corruption
evict+rebuild) plus all prior suites. gemini reviewed with zero findings (still active on its
sunset day); validation comment posted before squash-merge. The complete workspace is green
(**2219 total = 2206 passed + 13 ignored**) and clippy passes with warnings denied in both
feature states. **Next: ThinLTO SV — the verification bundle** per the settled slice plan:
build-twice determinism as a pinned gate, cold-vs-hit byte-identity re-verified at SV scope, a
summary-index-level stale-summary fail-closed mutation (S2's gate covers CAS-blob corruption; SV
deepens it), and an explicit compile-time regression bound (the rt-lto bound discipline). After
SV closes the ThinLTO arc: next M14 remainder = instrument PGO (sequenced after LTO in the
recorded wave order). From now on gemini-code-assist is sunset — use `/code-review` on each
branch before merging. Previous update: 2026-07-16 (twenty-fourth update this day), **THINLTO S1
IS SHIPPED — SERIAL
CROSS-UNIT OPTIMIZATION BEHIND `--thin-lto`, MERGED as #496** (`a3ae142`). The 3-entry shim is now
a production component of every `alignc` (libLTO stays spike-only); the driver runs
prelink → thin-link → backend serially over private staging; `--thin-lto` is legal only on
`release`/`fast` + `build`/`run`/`size` (loud rejection elsewhere); N=1 skips all three phases
(byte-identical, gated); any shim failure aborts loudly naming phase+unit (no silent fallback);
and the object cache is BYPASSED under the flag until S2 integrates the precise digest. One
correctness-forced deviation is recorded in the roadmap S1 paragraph: entry 2 reports imports AND
exports, and entry 3 threads both through `thinLTOInternalizeAndPromoteInIndex` + an explicit
`renameModuleForThinLTO` (importing a fn that references its unit's private local requires
promotion on both sides; a leaf unit still promotes its own exports; the `undefined
str.llvm.<hash>` link failure and the reparse-identifier SIGSEGV are pinned by diagnosis — bitcode
reverts a module's identifier to the source filename, so the loader restamps the stable unit id).
Gates green (`crates/align_driver/tests/thin_lto.rs`, 9): cross-unit inline mutation-checked both
directions, the M13 wide-tuple sret positive, N=1 byte-identity, run-parity corpus,
`--export`/pub preserve survival, profile/verb rejection, flag-off byte-determinism, and a
build-twice determinism pin (de-risks SV). Both gemini findings were verified and applied (FFI
null guards in `buildIndex` rc 19; `llvm::StringMap` allocation-free lookups in the hot
`isExported` callback and module loader) with the validation comment posted before squash-merge.
The complete workspace is green (**2219 total = 2206 passed + 13 ignored**) and clippy passes with
warnings denied in both feature states. **Next: ThinLTO S2 — cache composition + parallelism**
per the settled record (populate the precise `cross_unit_opt_digest`, the CAS prelink-bitcode
part-kind, `CACHE_KEY_FORMAT_VERSION` bump, `FirstDiff` phase split, prelink/backend phases in the
`thread::scope` claim loop around the serial thin-link; gates: private-body edit → only affected
backends miss, pub-signature change → transitive miss, cross-process all-hit, parallel == `-j 1`
byte-identity), then SV. gemini-code-assist ceases review 2026-07-17 — use the `/code-review`
fallback before merging from S2 onward. Previous update: 2026-07-16 (twenty-third update this
day), **THINLTO IS DESIGN-SETTLED AND ITS S0
FEASIBILITY SPIKE IS GO — MERGED as #495** (`32ce6e6`). The M14 post-upgrade wave head (real
cross-unit optimization, un-mooted by M15) was settled by a two-lens review (soundness/cache-key +
mechanics/driver) plus orchestrator decisions, recorded as the roadmap's new **"ThinLTO design
SETTLED" paragraph — the S1/S2/SV implementation source of truth**. Mechanism = a 3-entry C++ shim
(prelink summary-bearing bitcode / thin-link per-unit import lists / per-unit backend), because
llvm-sys 221 structurally cannot emit module summaries nor drive `FunctionImporter`, and the legacy
`ThinLTOCodeGenerator` C API it exposes SIGSEGVs on summary-less bitcode (fork-probe evidence),
hides import lists, and runs its own pool+cache — incompatible with cache-first identity. Backend
cache key = the PRECISE digest (own prelink digest ⊕ import list ⊕ import-source prelink digests);
flag = opt-in `--thin-lto` on release/fast only; N=1 skips all phases (byte-identity preserved);
preserve set fail-closed = {main} ∪ --export ∪ pub; rt-lto keeps its pre-opt merge placement with
attr-xor; CAS prelink part-kind + CACHE_KEY_FORMAT_VERSION bump at S2; non-goals: no
full-LTO-over-N, no linker-plugin, explain-opt/emit-llvm stay per-unit, no profile-guided
thresholds. The S0 spike (feature-gated `thinlto-spike`, 6 ignored tests, zero default-build
impact) proved the full round-trip in-process: the cc-built shim links against prefer-dynamic
libLLVM-22.so beside llvm-sys, inkwell `LLVMModuleRef` crosses the FFI, and
`FunctionImporter` + `buildThinLTODefaultPipeline` inline a cross-module callee (relocation
disappears); import decisions deterministic after canonical edge sort; thin-link ≈70 µs at spike
scale. LLVM-22 frictions recorded in the roadmap paragraph (getGUIDAssumingExternalLinkage,
libLTO.so, MemoryBuffer-identifier keying, explicit datalayout). All four gemini findings were
verified and applied (platform-conditional C++ stdlib link, `OS.has_error()` after write, null-ctx
guard, `#[cfg(unix)]` fork probe) with the validation comment posted before squash-merge. Default
workspace unchanged and green (**2210 total = 2197 passed + 13 ignored**; the 6 spike tests exist
only under the feature); clippy clean in both feature states. **Next: implement ThinLTO S1 —
serial correctness behind `--thin-lto`** (gates: cross-unit `pub` call inlined, IR-shape
mutation-checked both directions; the M13 Slice-5 wide-tuple `sret` positive; N=1 byte-identity;
multi-file run-parity corpus; `--export`/preserve survival), then S2 (cache composition +
parallelism), then SV. Note: gemini-code-assist ceases review 2026-07-17 — from S1 onward use the
`/code-review` fallback before merging. Previous update: 2026-07-16 (twenty-second update this
day), **THE ADAPTIVE TOTAL-ORDER STABLE-SORT
PATH IS SHIPPED AS THE `w64` SHAPE.** Doc-12 §4.1's measured P1 landed as squash-merged PR **#494**
(`9be7a1b`): `lower_array_sort` now carries (1) a whole-input ordered early exit after exactly-once
key decoration, (2) an ordered run-boundary straight-copy applied only from pass 2 (`width >= 64`,
`Lt`+negate, no new primitive), and (3) merge-only ping scratch delayed behind a `len > 32` guard
(a `len <= 32` sort allocates only its materialize buffer(s): plain 2→1, keyed 4→2). Key-order
classification is fail-closed with no wildcard: `Int/Char/Str/Bool` are total-order, `Float` and
every other explicitly listed `Scalar` variant keep today's merge path, so NaN keys are structurally
unable to take any new block. Corrected before/after on this Ryzen 9 5950X (drift-immune
adjacent-ratio + identical-code-control method, the shipped compiler's own `ALIGN_SORT_ADAPTIVE=off`
baseline): `sort_u64` already-sorted 3.6x, tail-swap/1%-swaps 1.14-1.17x, random 1.00x, reverse
0.98-0.99x, 16-value cardinality 0.99-1.00x at 100k/1M; `sort_by_key` already-sorted 4.6-15.6x;
`sort_str` already-sorted 10.9x. The first cut's ≈7% random/reverse regression was root-caused to
the pass-1 boundary check by a variant-isolation sweep; a keyed width sweep ({32..512}) proved `w64`
the peak for both key modes, and the one keyed low-cardinality 100k cell (≈3.5%, comparable to the
measurement floor) is ACCEPTED as a bounded measured single-cell exception with its full trade-off
table in doc-12 §4.1. The `ALIGN_SORT_ADAPTIVE` toggle is read at MIR-lowering time so `impl_hash`
already keys the two shapes apart; defense-in-depth force-disables the object cache when the toggle
is set, pinned by cache gate13b (three subprocess builds, byte-identity across an intervening
baseline build). New `sort_adaptive.rs` (9 gates: differential/stability oracle across six input
states, structural size matrix 0..129/1024/20000, str keys, Impure-key exactly-N evaluation pin,
float/NaN old-path MIR gate, guarded-ping-alloc MIR gate, leak/double-free on early-exit and small-N
paths), the `bench/adaptive_sort/` manual probe, and an off-by-default `alloc-count` runtime
feature. Gemini reviewed with zero findings; the merge absorbed the ARM PRs #491-#493 with one
doc-13 table conflict resolved by keeping both shipped rows. The complete workspace is green
(**2210 total = 2197 passed + 13 ignored manual probes**) and workspace clippy passes with warnings
denied. Doc-12's priority list is now fully dispositioned (items 1/2/4/5/6 shipped; item 3
compaction rejected+closed by #490) and doc-13 C0/P1/P2 plus P3 item 1 are done. **Next recommended:
begin the M14 post-upgrade remainder with ThinLTO / real cross-unit optimization** — its recorded
precondition (multi-module separate compilation) is satisfied by M15, the v1 artifact reserves the
bitcode/thin-summary envelope and the empty-in-v1 cross-unit-opt digest precisely to receive it
(roadmap M15 §5 + M14 wave order), and instrument PGO then sample-PGO/BOLT stay sequenced behind
it. Previous update: 2026-07-16 (twenty-first update this day), **NATIVE APPLE-SILICON HEX DISPATCHES TO
NEON AT ITS INDEPENDENTLY MEASURED 16-BYTE CROSSOVER.** The checked-in balanced median-of-nine probe
ran repeatedly on a native Apple M1 (`uname -m = arm64`, Rosetta translation flag 0). The first
complete 16-byte NEON block improved core/allocation-inclusive time by 2.78-2.82x/1.39x, and the
production-path 1..=64 allocation-inclusive geometric mean improved 1.30x. Larger controls measured:

| Size | core / allocation-inclusive | candidate input throughput |
|---:|---:|---:|
| 1 KiB | 11.34-11.41x / 8.02-8.06x | 23.6-23.7 GB/s |
| 1 MiB | 7.99-9.58x / 9.84-10.24x | 16.8-20.1 GB/s |
| 64 MiB | 9.58-9.62x / 9.67-9.69x | 20.0-20.1 GB/s |

The worst short dispatcher observations were 0.92x core at 4 bytes and 0.96x
allocation-inclusive at 15 bytes. Inputs below 16 bytes retain the scalar oracle; direct
scalar/NEON differential tests cover every length through 4096, all alignments, and a page boundary.
Production activation is therefore enabled. This completes the requested native ARM UTF-8,
Base64/Base64url, and hex measurement set without adopting any unrelated future arm64 proposal. This
work is on branch **`agent/arm64-hex-crossover`**, based on squash-merged Base64 PR **#492**
(`514bf5d`). **Next:** final workspace validation, review, and merge. Previous update: 2026-07-16
(twentieth update this day), **NATIVE APPLE-SILICON BASE64 AND BASE64URL
DISPATCH TO NEON AT THEIR INDEPENDENTLY MEASURED 48-BYTE CROSSOVER.** The checked-in balanced
median-of-nine probe ran repeatedly on a native Apple M1 (`uname -m = arm64`, Rosetta translation
flag 0) for both the standard/padded and URL-safe/unpadded encoders. The first complete 48-byte NEON
block improved allocation-inclusive time by 1.63-1.66x, while the production-path 1..=64
selected-point geometric mean improved 1.05-1.14x. The measured core/allocation-inclusive gains were:

| Size | Base64 | Base64url |
|---:|---:|---:|
| 1 KiB | 5.79-5.81x / 4.63-4.69x | 5.81-5.83x / 4.67-4.69x |
| 1 MiB | 6.70-6.78x / 6.71-6.73x | 6.78-6.83x / 6.70-6.73x |
| 64 MiB | 6.67-6.75x / 6.62-6.70x | 6.63-6.78x / 6.61-6.68x |

Candidate input throughput was 15.9-16.2 GB/s at 1 KiB, 18.8-19.2 GB/s at 1 MiB, and 18.6-18.8
GB/s at 64 MiB.
Inputs below 48 bytes retain the scalar oracle; direct scalar/NEON differential tests cover every
length through 4096, both alphabets, all alignments, and a page boundary. Production activation is
therefore enabled for both encoders. The worst one-byte dispatcher observations were 0.81x core and
0.92x allocation-inclusive (the latter repeated at 0.96x), while the required allocation-inclusive
short-suite geometric mean remained positive. **Next:** measure and decide hex independently; do not
infer its threshold from Base64, UTF-8, or x86. Previous update: 2026-07-16 (nineteenth update this day),
**NATIVE APPLE-SILICON UTF-8 KEEPS THE
SCALAR PRODUCTION PATH.** The checked-in probe ran repeatedly on a native Apple M1
(`uname -m = arm64`, Rosetta translation flag 0) and found no length-only NEON crossover that
passes every named control. The 1..=64 selected-point candidate speedup versus scalar geometric mean
was 1.26-1.27x overall and 1.67-1.68x excluding early-invalid input, and 1 MiB multibyte input improved
6.40x. However,
1 MiB ASCII and late-invalid inputs both regressed to 0.83x, while an early-invalid byte retained
the scalar oracle's immediate return. The realistic 64 MiB mixed-text candidate still reached
15.1 GB/s versus 3.1 GB/s scalar, but the named regressions fail the no-regression gate. Production
aarch64 dispatch therefore uses the scalar oracle; the baseline-NEON implementation, tail handling,
and byte-for-byte differential tests remain as a measured candidate for a materially different
content-adaptive design. The native ARM UTF-8 deferral is closed as a negative result. This work was
squash-merged as PR **#491** (`510922e`). **Next:** measure and decide the independent
Base64/Base64url NEON crossover; do not infer its threshold from UTF-8 or x86.
Native aarch64 hex activation remains deferred. Previous update: 2026-07-16 (eighteenth update this
day), **THE FIRST COMPLETE STABLE-COMPACTION
CONSUMER IS REJECTED AND CLOSED.** The checked-in ignored probe combines an inlined total `i64 > 0`
predicate, AVX2 four-lane mask formation, and ordered direct materialization, with the current scalar
predicate/branch/append shape as oracle. Full-prefix/count differential checks precede balanced timing
over predictable/random 0/1/10/50/90/99/100% distributions at 1 KiB, 1 MiB, and 256 MiB. The best
candidate averaged 1.93x core and 1.44x allocation-inclusive, with a 2.94x/2.81x named positive, but
the 1 MiB all-survivor case reproducibly fell to 0.63x/0.49x. Always applying the identity permutation
did not fix it (0.62x worst) and reduced the matrix core mean, so that variant was discarded. This
fails the no-regression gate: retain the scalar fused loop, do not add primitive-predicate
specialization or a predicate-repeating selectivity prepass, and reconsider only for a consumer that
already owns a mask or a materially different ISA compress primitive. This work is on branch
**`agent/stable-compaction-consumer`**, based on squash-merged PR #489 (`edcf360`). The complete
workspace is green (**2200 total = 2187 passed + thirteen ignored manual probes**) and workspace
clippy passes with warnings denied. **Next recommended after this PR:** implement document 12's
already-measured P1 total-order adaptive stable-sort path (ordered-input early exit, ordered run-boundary
copy, and delayed merge-only scratch), while leaving float/NaN keys on the existing path. Native
aarch64 Base64/hex activation and UTF-8 portability measurement remain deferred. Previous update:
2026-07-16 (seventeenth update this day), **THE STABLE-COMPACTION STRUCTURAL AVX2
KERNEL PASSES ITS FULL MATRIX; PRODUCTION CONSUMER INTEGRATION IS STILL GATED.** The checked-in
ignored probe compares scalar and SIMD against the same precomputed byte mask across predictable and
random 0/1/10/50/90/99/100% selectivity, 1/4/8/16-byte elements, and 1 KiB/1 MiB/256 MiB inputs. It
verifies the complete survivor prefix and count before timing. Empty/full/one-survivor block handling
removed the structural low-selectivity regression; on this Ryzen 9 5950X the 168-case geometric mean
is 3.08x core and 2.62x allocation-inclusive, with a 9.61x/8.86x named positive at 256 MiB,
one-byte, random 50%. The reusable-output core worst case is approximately 0.99x. No production
dispatch changed: the probe deliberately starts from a precomputed mask, so the next gate is one real
total/vectorizable primitive `where(...).to_array()` consumer including predicate-to-mask formation,
inactive-lane trap suppression, exact predicate/drop order, and scalar crossover. This work is on
branch **`agent/stable-compaction-probe`**, based on squash-merged PR #488 (`3de1fd3`). The complete
workspace is green (**2199 total = 2187 passed + twelve ignored manual probes**) and workspace clippy
passes with warnings denied. **Next recommended after this PR:** build and measure that complete
predicate-to-mask materialization consumer; do not promote the isolated kernel by itself. Native
aarch64 Base64/hex activation, aarch64 UTF-8 portability measurement, and any production compaction
backend remain deferred. Previous update: 2026-07-16 (sixteenth update this day), **X86-64 HEX
ENCODE DISPATCHES TO AVX2 AT ITS
INDEPENDENTLY MEASURED 32-BYTE CROSSOVER.** The nibble-lookup/unpack backend writes the existing exact
final destination; shorter/non-AVX2 inputs retain the scalar oracle. Differential coverage compares
every length through 4096, every alignment modulo 32, and a page-aligned input. On this Ryzen 9 5950X
the allocation-inclusive 1..=64 selected-point geometric mean improved 1.21x; 32 B improved 1.98x,
1 KiB 10.84x, 1 MiB 12.04x, and 64 MiB 1.36x, with 27.58 GB/s core input throughput at 1 MiB. A
baseline-NEON candidate is implemented and cross-compiles, but production aarch64 dispatch remains
scalar pending the native probe. This work is on branch **`agent/hex-simd-probe`**, based on
squash-merged PR #487 (`2843e95`). The complete workspace is green (**2198 total = 2187 passed +
eleven ignored manual probes**) and workspace clippy passes with warnings denied. **Next recommended
after this PR:** run document 12's scalar-vs-SIMD stable-compaction experiment across selectivity and
element widths; adopt only if the materialization consumer wins. The aarch64 Base64/hex activation
gates and older aarch64 UTF-8 portability measurement remain deferred. Previous update: 2026-07-16
(fifteenth update this day), **X86-64 BASE64 ENCODE DISPATCHES TO AVX2 AT THE MEASURED 32-BYTE
CROSSOVER.** The two-lane byte-shuffle backend writes the existing exact final
destination and leaves shorter/non-AVX2 inputs on the scalar oracle. Direct differential coverage
compares every length through 4096, both alphabets/padding forms, every alignment modulo 32, and a
page-aligned input. On this Ryzen 9 5950X the allocation-inclusive 1..=64 selected-point geometric
mean improved 1.05x for both Base64 and Base64url; 32 B improved 1.22x/1.21x, 1 KiB 4.12x/4.09x,
1 MiB 5.22x/5.27x, and 64 MiB 1.52x/1.55x. A baseline-NEON backend is implemented and cross-compiles
for `aarch64-unknown-linux-gnu`, but production aarch64 dispatch remains scalar until the checked-in
probe runs on native hardware; do not guess its threshold. This work was squash-merged as PR **#487**
(`2843e95`), based on squash-merged PR #486 (`3270e2b`). The complete workspace is
green (**2196 total = 2186 passed + ten ignored manual probes**) and workspace clippy passes with
warnings denied. **Next recommended after this PR:** run the separate measure-first hex SIMD probe;
do not fold it into Base64 merely because the destination machinery is shared. Both the aarch64
Base64 activation gate and the older aarch64 UTF-8 portability measurement remain deferred. Previous
update: 2026-07-16 (fourteenth update this day), **`http.get_many` BUILDS EACH IMMUTABLE REQUEST ONCE.**
The batch now copies each caller URL directly into its final `HttpRequest`, and the uniquely
claiming worker borrows `requests[i]`; the intermediate URL vector and worker-local String clone are
gone. Bounded claiming, input-order response slots, shared keepalive reuse, lowest-index all-or-Err,
and run-to-completion failure cleanup are unchanged. The balanced construction probe removed exactly
one allocation/copy per URL and improved short 1/8-entry batches by 1.25x/1.18x; isolated 64/1K
construction was 0.94x/0.92x because the larger request headers are prebuilt, but end-to-end
zero-latency HTTP stayed flat through 64 and remained within the 3% gate at 1K (9.5->9.7 ms). The 10
ms-latency 1/8/64/1K controls were all within 1%. This work was squash-merged as PR **#486**
(`3270e2b`), based on squash-merged PR #485 (`b802545`). The complete
workspace is green (**2194 total = 2185 passed + nine ignored manual probes**) and workspace clippy
passes with warnings denied. **Next recommended after this PR:** implement the already-planned
runtime-dispatched Base64 SIMD backend against the shipped exact destination, with scalar differential
and per-target crossover gates; keep hex SIMD as a separate measure-first probe. The aarch64 UTF-8
portability measurement remains deferred. Previous update: 2026-07-16 (thirteenth update this day),
**UDP RECEIVE AND `pread` SHARE THE RAW READ WINDOW.** `Buffer::prepare_uninit_window` now centralizes
len-clear, release-build capacity repair, and spare-capacity access for reader, UDP, and positional
file reads. `recvfrom` publishes only the returned prefix, including the documented leading-prefix
truncation for oversized datagrams. `file.pread` calls POSIX `pread(2)` directly instead of forming a
fully initialized Rust slice, then publishes only the actual short-read prefix or an empty Vec at
EOF; fd offset and EINTR semantics are unchanged. This work was squash-merged as PR **#485**
(`b802545`), based on squash-merged PR #484 (`d11aaea`). The complete workspace remains green (**2192
total = 2184 passed + eight ignored manual probes**) and workspace clippy passes with warnings denied.
**Next recommended after this PR:** remove the measured redundant URL/request copy in `http.get_many`,
using prebuilt immutable requests and preserving bounded claiming, input order, pool reuse, and all
failure/no-leak gates. The aarch64 UTF-8 portability measurement remains deferred. Previous update:
2026-07-16 (twelfth update this day), **READER AND `io.copy` NO LONGER ZERO UNWRITTEN
TAILS.** Buffered-reader refill and direct `reader.read` now reserve raw Vec capacity, pass only its
`MaybeUninit` spare-capacity pointer to `read(2)`, and publish exactly the successful byte count.
Short reads expose only their initialized prefix, EOF exposes zero bytes, and EINTR retries before
publication. Lookahead is still drained before fd-fresh data, so the portable `io.copy` oracle keeps
the buffered-reader correctness fix while inheriting the zero-fill removal. The allocation-inclusive
fresh 64 KiB-window probe improved 0/1/4 KiB/full-prefix cases by 20.92x/20.83x/11.49x/1.98x. This
work is green across the complete workspace (**2192 total = 2184 passed + eight ignored manual
probes**) and workspace clippy passes with warnings denied. It was squash-merged as PR **#484**
(`d11aaea`), based on squash-merged PR #483 (`51fbb4b`). **Next
recommended after this PR:** extend the same initialized-prefix discipline to the separately audited
UDP receive and positional `pread` paths, preserving datagram truncation, short-read, offset, EINTR,
and EOF gates. The aarch64 UTF-8 portability measurement remains deferred. Previous update:
2026-07-16 (eleventh update this day), **ARENA INITIALIZATION CLASSES PRESERVE BOTH
RAW SPEED AND LAZY ZEROING.** Arena chunks now carry either raw `MaybeUninit<u8>` backing or
conservative `Vec<u8>` backing; no initialized Rust byte slice covers raw capacity. Fresh raw chunks
skip the old 64 KiB blanket zero, fresh conservative chunks retain platform lazy/calloc zero pages,
and a conservative allocation inside raw backing zeroes only its requested range. The public/generated
ABI and task records stay conservative. Only the proved full-overwrite file-view fallback, arena
builder finish, and strict SoA decode use raw storage. The balanced probe improved overwrite paths
13.42-13.52x through 48 B, 10.78x at the 2.5 KiB gateway shape, 1.91x at 64 KiB, and 1.99x at 1 MiB;
64 MiB stayed at parity. Conservative medians remained within 1% and the robust task-shaped p99
panel stayed within 5%. An intermediate exact-memset design was rejected because it destroyed lazy
zeroing on fresh 64 MiB conservative chunks. The complete workspace is green (**2190 total = 2183
passed + seven ignored manual probes**) and workspace clippy passes with warnings denied. This work
was squash-merged as PR **#483** (`51fbb4b`), based on squash-merged PR #482 (`2402149`). **Next
recommended after this PR:** apply the same initialized-write discipline to the roadmap's
reader/`io.copy` zero-fill removal, with short-read,
EINTR, EOF, and lookahead correctness gates first. The aarch64 UTF-8 portability measurement remains
deferred. Previous update: 2026-07-16 (tenth update this day), **BASE64/BASE64URL/HEX FILL ONE EXACT FINAL
PAYLOAD.** All three scalar encoders now compute checked group/tail output lengths, allocate only the
final Align-owned string, and initialize it through `MaybeUninit<u8>`; the staging Vec, second
allocation, and full-output copy are gone. Empty output remains `{null,0}`, positive OOM remains the
runtime's fail-fast contract, and differential gates cover every length through 65 bytes plus
256/4096 and every byte value. The allocation-inclusive balanced probe improved every short case;
at 64 MiB Base64/Base64url/hex improved 1.71x/1.70x/1.86x after the review-requested chunked hot
loop removed per-byte bounds checks. The roadmap JSON-copy probe was also
executed end to end for `array<i64>` with the required lexical count pass: it won only at 1-8
elements, reversed by 64, and reached 0.71-0.73x at 1K-1M, so the existing one-pass staged decoder is
retained and the negative result is recorded. The complete workspace is green (**2188 total = 2182
passed + six ignored manual probes**) and workspace clippy passes with warnings denied. This work was
squash-merged as PR **#482** (`2402149`), based on squash-merged PR #481 (`be74904`). **Next recommended
after this PR:** take document 12's highest remaining measured item, the
initialized-before-read/uninitialized arena split, beginning with its safety classification and
gateway adoption probe. The aarch64 UTF-8 portability measurement remains deferred. Previous
update: 2026-07-16 (ninth update this day), **`read_dir` AND DNS CREATE FINAL STRING PAYLOADS
ONCE.** Both runtimes now allocate each UTF-8 filename or numeric-IP payload while enumerating and
stage only `AlignStr` headers in a shared RAII owner. Any later iteration or checked-size error drops
that owner and frees every payload accumulated so far while the ABI output stays `{null,0}`. Success
publishes one final header buffer, clears the temporary owner, and transfers each payload exactly
once to the existing generic `array<string>` deep-drop representation. UTF-8 filename exclusion,
DNS first-occurrence order/deduplication, and `freeaddrinfo` coverage are unchanged. This removes one
staging allocation and full payload copy per returned entry without introducing a slab or a second
ownership representation. The complete workspace remains green (**2185 total = 2181 passed + four
ignored manual probes**) and workspace clippy passes with warnings denied. This work was
squash-merged as PR **#481** (`be74904`), based on squash-merged PR #480 (`bdca925`). **Next
recommended P2:** execute document 12's
codec exact-destination slice and the roadmap JSON-copy probe. The aarch64 UTF-8 portability
measurement remains deferred. Previous update: 2026-07-16 (eighth update this day), **`path.normalize` FILLS ONE FINAL BUFFER.** The
runtime allocates the owned result once at the proven `max(input_len, 1)` bound and uses its
initialized prefix as the component stack; ordinary components append directly and `..` rewinds to
the preceding separator. The component Vec, output Vec, and final full-output copy are gone while
relative leading `..`, absolute root clamping, repeated separators, UTF-8 bytes, and the owned-string
ABI are unchanged. A staged-oracle differential gate covers 1,000 generated paths plus 1,024-level
normal/`..` stress. The checked-in allocation-inclusive median-of-nine probe measured 2.30x on an
already-normal 16-byte path, 1.86x on a mixed 10-byte path, and 1.43x on 256 normal components. The
explicit negative shape (256 appends then 192 pops) is recorded at 0.77x because direct fill writes
components that a staging stack later discards; the required already-normal short gate still clears
decisively. Gemini's `rposition` cleanup suggestion was measured and rejected: it regressed all four
probe shapes by roughly 5-7% versus the explicit initialized-prefix loop. The complete workspace is green (**2185 total = 2181 passed + four ignored manual
probes**) and workspace clippy passes with warnings denied. This work was squash-merged as PR **#480** (`bdca925`), based on
squash-merged PR #479 (`b8b9173`). **Next recommended P2:** remove per-entry payload staging from
`fs.read_dir` and DNS results with explicit unwind cleanup, while retaining generic deep-drop
ownership. The aarch64 UTF-8 portability measurement remains deferred. Previous update: 2026-07-16
(seventh update this day), **SINGLE STR-KEY GROUPS AND DICTIONARY ENCODING
WRITE THEIR FINAL OUTPUTS DIRECTLY.** Vacant string groups now seed the caller's existing
`out_keys[id]` / `out_vals[id]` slots and occupied entries update `out_vals[id]` in place, removing
the representative and accumulator Vecs plus both final copies from the shared AoS/SoA core.
`dict_encode` likewise writes representatives directly into its caller-owned dictionary, removing
one Vec and copy. First-occurrence dense ids, aggregate wrap/min/max/count behavior, result ownership,
and multi-aggregate row-major update locality are unchanged. Every group core now validates signed
capacity through `safe_len` before pointer arithmetic, and the direct paths state the generated
input/output non-overlap contract; a sentinel gate pins that neither output crosses capacity. The
Gemini review's one valid cleanup was applied: occupied, already-initialized i64 slots now update
through a direct dereference, while vacant uninitialized output still uses `ptr.write`. The repaired
checked-in benchmark (its exported `Row` now follows current interface visibility) stayed
within 3% at 100/10K groups; at 632,390 distinct keys, four single groups improved 690.0→630.4 ms
(1.09x) and dictionary reuse 200.9→194.7 ms (1.03x), recorded as directional consecutive-run
evidence rather than balanced AB/BA. The complete workspace is green (**2183 total = 2180 passed +
three ignored manual probes**) and workspace clippy passes with warnings denied. This work was
squash-merged as PR **#479** (`b8b9173`), based on squash-merged PR #478 (`e03666e`). **Next recommended P2:** direct-fill staged
`path.normalize`, `read_dir`, and DNS result payloads, beginning with the smallest independently
measurable final-copy removal. The aarch64 UTF-8 portability measurement remains deferred. Previous
update: 2026-07-16 (sixth update this day), **DIRECT `chunks` LENGTH AND INDEX CONSUMERS ARE
VIRTUAL.** Immediate `.chunks(n).len()` now computes a guarded ceiling count in MIR, and immediate
`.chunks(n)[i]` constructs exactly one bounds-checked source sub-view. Neither path allocates or
fills the owned `{ptr,len}` header array. Dynamic, zero, and negative widths preserve the runtime's
canonical empty result without dividing by zero; a zero-width direct index takes the normal bounds
failure path. Fresh owned sources retain their synthetic owner through a returned chunk view, while
the scalar length path drops that owner immediately after consumption. Stored/escaping values and
pipeline/`par_map` consumers deliberately retain `align_rt_chunks` and its existing owned
representation. Gemini caught that the initial direct-index path evaluated its index before the
chunk source and width; the fix restored receiver-first order and closed the same pre-existing gap
for ordinary array/slice indexing. An Impure runtime gate pins source → width → index and ordinary
source → index order. Raw-IR and runtime gates also pin the allocation-free direct shapes, the
materialized fallback, exact/partial/empty results, bounds failure, and owned-temporary lifetime.
The complete workspace is green (**2182 total = 2179 passed + three ignored manual probes**) and workspace clippy
passes with warnings denied. This work was squash-merged as PR **#478** (`e03666e`), based on squash-merged PR #477
(`c2f9d95`). **Next recommended P2:** write single str-key group and dictionary outputs directly
into their already-allocated result buffers. The aarch64 UTF-8 portability measurement remains
deferred. Previous update: 2026-07-16 (fifth update this day), **OWNED BUILDER FREEZE IS ALLOCATOR-COMPATIBLE
AND ZERO-COPY.** `BuilderBuf` now grows with the same C `malloc/realloc/free` family as Align owned
strings, so `to_string()` transfers its pointer instead of allocating and copying a second payload;
this does not assume Rust's global allocator matches libc on non-glibc targets. Checked geometric
growth, best-effort capacity hints, unfinished Drop, direct file/writer borrows, and canonical empty
null/0 results are preserved. Gemini's claimed current allocator mismatch was not present (all
three exported allocation calls already used the same C family), and using fail-fast
`align_rt_alloc` for a best-effort hint would change behavior; the valid future-drift concern was
addressed by routing builder, public ABI, and array-builder storage through one internal allocator
family. The same review pass closed positive oversized `i64` truncation on 32-bit alloc/realloc
calls with `safe_len`. Arena finish remains intentionally distinct and copies into
arena-owned storage. Pointer-identity gates cover boxed/stack headers, exact capacity, and geometric
growth. A checked-in allocation-inclusive release probe (balanced median of nine) measured
1.59x-2.81x over the removed copy freeze from 64 B through 1 MiB, with both exact and capacity-zero
growth. The complete workspace is green (**2178 total = 2175 passed + three ignored manual probes**)
and workspace clippy passes with warnings denied. This work was squash-merged as PR **#477**
(`c2f9d95`), based on
squash-merged PR #476 (`5b96d5c`). **Next recommended P2:** virtualize direct-consumer
`chunks`, starting with `.len()` and direct index while stored/escaping results keep materializing.
The aarch64 UTF-8 portability measurement remains deferred. Previous update: 2026-07-16 (fourth
update this day), **PROVEN NONESCAPING BUILDER HEADERS NOW USE
ENTRY STACK STORAGE.** A checked-in ignored release probe isolated header placement while retaining
the exact payload representation and write/push calls: at 0/1/4 elements, `builder` improved by
2.69x/1.42x/1.31x and `array_builder` by 3.37x/1.59x/1.33x; 1K/1M controls stayed within 0.3%.
Codegen now applies a conservative whole-MIR proof only to directly-bound locals; aliases, user
calls, returns, captures, and unknown uses retain the boxed ABI. Dynamic template/json.encode
builders also use entry storage because their internal header is never exposed. One 64-byte,
16-aligned buffer is reused per eligible local/expression, guarded by runtime layout assertions;
payload growth, pointer stability, direct I/O, and zero-copy array freeze are unchanged. Runtime and
raw-IR gates cover consuming finishes/builds, safe reassignment, scalar and deep-string unfinished
Drop, and boxed call/return escape controls. Gemini's three findings were applied: unaudited MIR
rvalues now reject every stack-header candidate fail-closed, a future aggregate-wrapper MIR gate
pins that rule, and both stack initializers debug-check the concrete header alignment. After
refreshing the ordinary CLI/staticlib pair, the complete workspace is green (**2176 total = 2174
passed + two ignored manual probes**) and workspace clippy passes with warnings denied. This work was
squash-merged as PR **#476**, based on merged PR #475. **Next recommended
P2:** make owned builder freeze allocator-compatible and zero-copy, measured independently from the
now-shipped header placement. Portability follow-up remains: run the UTF-8 probe on native aarch64;
do not infer its threshold from x86. Previous update: 2026-07-16 (third update this day), **THE X86-64 UTF-8 CROSSOVER IS MEASURED AND
SHIPPED.** The existing ignored runtime probe now reports balanced median-of-seven scalar, direct
SIMD, and shipped-dispatch latency for ASCII, valid multibyte, early-invalid, and late-invalid
inputs from 0 through 4096 bytes, plus its 64-MiB throughput control. On this Ryzen 9 5950X / Zen 3
host AVX2 lost by 1.2–3.7x through 16 bytes; at 24/31 only multibyte favored AVX2, while at exactly
32 bytes AVX2 reached parity or won on every shape except the failure-only early-invalid case. The
x86-64 dispatcher now returns empty immediately, uses `std::str::from_utf8` below 32 bytes, keeps
AVX2 from 32 onward, and gives the awkward 33–63-byte all-ASCII tail a narrow complete ASCII proof.
AVX2/NEON padded tails also share the existing full-block ASCII fast path. In the before/after probe,
the shipped 1-byte path fell from about 12.8 ns to 4.4 ns and 4 bytes from 11.1 ns to 5.4 ns; the
64-MiB mixed-text control remains memcpy-class (**18.6 GB/s SIMD vs 4.0 scalar and 18.4 memcpy**).
The aarch64 threshold remains deliberately unchanged until this same checked-in probe runs on native
NEON hardware. Differential SIMD/scalar fuzz, the complete workspace (**2165 total = 2164 passed +
one ignored manual probe**), and workspace clippy with warnings denied are green. **Next recommended
local P1:** prototype nonescaping builder/array-builder headers separately from payload changes;
portability follow-up: run the UTF-8 probe on native aarch64 when available. Previous update:
2026-07-16 (second update this day), **`ARRAY_BUILDER_NEW` NOW CARRIES THE AUDITED
ALLOCATOR CONTRACT.** The LLVM declaration for `align_rt_array_builder_new` now uses the same
`mark_alloc_like` path as `align_rt_builder_new`: fresh returns are `noalias`, and the function is
`nofree` + `nounwind` without the unsound `willreturn` claim because OOM aborts. The allocator IR
gate now pins all four parts of that contract. The audit priority list was also corrected to record
that M14's equality/order/hash ceiling probe and guarded runtime-bitcode slice already shipped on
2026-07-14. The complete workspace remains green (**2165 total = 2164 passed + one ignored manual
probe**) and workspace clippy passes with warnings denied. **Next recommended P1 measurement:**
establish the UTF-8 scalar/SIMD crossover per target before changing `utf8_valid` dispatch.
Previous update: 2026-07-16, **ABI STRINGS NO LONGER STAGE THROUGH OWNED `String`.** Filesystem, path,
reader/writer/file, and mmap consumers now borrow the caller's validated UTF-8 bytes directly;
`getaddrinfo` and `execvp` consumers construct their required `CString` directly from that view.
The public C ABI remains defensive: non-UTF-8 and interior-NUL inputs are rejected, while wildcard
empty hosts retain their allocation-free null-node path. A helper-level 0/1/8/32/256-byte gate pins
pointer identity for borrowed views, direct C-string contents, and invalid-input behavior; the full
filesystem/network/process runtime and driver controls remain green. The complete workspace is
green (**2165 total = 2164 passed + one ignored manual probe**) and workspace clippy passes with
warnings denied. The first full-suite attempt exposed two different runtime archives under
`target/debug` and `target/debug/deps`; `cargo build --workspace` refreshed the ordinary CLI
staticlib, their content hashes then matched, and the byte-identity gate plus the complete workspace
passed. Previous update: 2026-07-15 (twenty-second update
this day), **ARENA-FREE DYNAMIC TEMPLATES HAVE SCOPED
OWNERS.** The surface result remains `str`: an explicit arena keeps the existing arena-backed path,
while an arena-free `template` or `json.encode` now finishes into a hidden synthetic `string` owner.
Borrow-owner propagation retains that allocation across local/view consumers and loop/function
cleanup drops it; scalar consumers can release it immediately. Escape analysis marks the dynamic
view `Frame`, so local use (including lifted pipeline lambdas) is accepted but returning it beyond
the hidden owner is rejected. Static-only templates fold in HIR to pooled literals with no builder
or allocation. Six dedicated tests cover the fold, 20,000 loop iterations, JSON encode, lambda
consumption, early `?`, and rejected escape. The complete workspace is green (**2164 total = 2163 passed + one
ignored manual probe**). Ready PR: **#474**. PR #473 was squash-merged as `9242c60` after Gemini feedback was applied,
answered, resolved, and summarized. **Next recommended allocation-audit item:** borrow filesystem/
path ABI strings and construct C strings directly. Previous update: 2026-07-15 (twenty-first update
this day), **DEFINITE-NULL DESTRUCTOR CALLS ARE
ELIMINATED IN MIR.** A forward drop-flag pass propagates constant ownership bits through CFG joins,
folds only conditional-drop branches whose flag has one value on every reachable incoming path,
and removes the newly unreachable destructor blocks. Moved returned strings/arrays and consumed
builders therefore no longer pay `free(null)` or handle-specific `*_free(null)` overhead, while
live and path-dependent allocations retain their conditional exactly-once destruction. MIR unit
gates pin zero drops after a definite move and one drop for an unmoved local; optimized-IR/runtime
coverage includes a returned string, frozen `array_builder`, conditional move, early return, `?`,
and real live frees. The complete workspace is green (**2158 total = 2157 passed + one ignored
manual probe**) and workspace clippy passes with warnings denied. Squash-merged as PR **#473**
(`9242c60`). Previous update: 2026-07-15 (twentieth update this
day), **LAZY MULTI-SOURCE `zip` IS SHIPPED.**
`zip(a, b, ...)` is a pipeline-only head for two or more equal-length Copy primitive-scalar
arrays/slices. Checked HIR carries the sources and an interned per-index tuple; MIR checks every
runtime length before the terminal loop, loads all sources at one increasing index, and constructs
only an SSA tuple — no tuple array or allocation. Existing `map`/`where`/reducers retain their
effect and trap ordering, including the guarded callable suffix after `where`. `map_into` proves its
destination disjoint from every source, while all runtime source loads share one input-vs-output
scope and never claim source-source disjointness. Five dedicated regressions pin the three-source
result, one counted loop, no allocation, optimized LLVM vectorization, guarded division, runtime and
static length mismatch, repeated-source aliasing, destination alias rejection, and invalid surfaces.
The complete workspace is green (**2155 total = 2154 passed + one ignored manual probe**) and
workspace clippy passes with warnings denied. The preceding synthetic-owner work was squash-merged
as PR #471 (`a2db332`) after all three Gemini findings were applied, answered in English, resolved,
and summarized in the required top-level PR comment. **Next recommended allocation-audit item:**
remove definite-null destructor calls now that ownership dataflow is path-aware. Previous update:
2026-07-15 (nineteenth update this day), **UNBOUND MOVE TEMPORARIES HAVE
VIEW-AWARE OWNERSHIP.** Fresh owned expressions used through scalar consumers now move into
path-local synthetic MIR owners and drop immediately after the scalar is produced; borrowed views
propagate those owners through string/slice/path/chunk/index/call operations until function or loop
cleanup. A separate runtime temporary bit distinguishes a fresh `if`/`match`/`else`/`?` arm from a
borrowed bound local, preventing both leaks and double frees. Escape analysis caps views of anonymous
owned storage at `Frame`, so returning a dangling `str`, subslice, or chunk element is rejected.
Synthetic owners participate in per-iteration, `break`, return, and early-error cleanup, and entry
initialization makes sibling paths safe. The original five optimized-IR leak shapes now contain
exactly five frees; eight dedicated regressions cover producers, scalar/index/view/call consumers,
mixed control flow, `?`, rejected escapes, and 20,000 loop iterations. Self-review caught and fixed
a compiler stack regression by boxing the new bookkeeping in `BuilderCtx` and keeping call lowering
out of the giant recursive frame; all five expression-depth gates pass. The complete workspace is
green (**2150 total = 2149 passed + one ignored manual probe**) and workspace clippy passes with
warnings denied. **Next recommended pipeline item:** consumer-gated lazy multi-source `zip`, starting
with 2+ equal-length Copy-scalar arrays/slices and allocation-free fused terminals. Previous update:
2026-07-15 (eighteenth update this day), **THE ZERO-COPY STRING BYTE VIEW IS
SHIPPED.** `str.bytes()` and owned `string.bytes()` now produce the specified `slice<u8>` view by
retyping the existing `{ptr,len}` descriptor: the HIR retains borrow provenance, while MIR emits no
rvalue and LLVM emits no call. Owned receivers auto-borrow rather than move. Region and borrow
liveness tests pin caller/static returns, reject owned/arena escapes, and reject use after backing
owner replacement; the runtime test reads a UTF-8 continuation byte and keeps the owned string
usable. The complete workspace remains green (**2142 total = 2141 passed + one ignored manual
probe**) and workspace clippy passes with warnings denied. Audit 13 §3.1, the source-fix ledger,
settlement record, and core string design in both languages now mark the view built. **Next
recommended C0 item:** add synthetic owners for unbound Move temporaries with view-aware liveness.
Previous update: 2026-07-15 (seventeenth update this day), **THE SETTLED STRING-CONCATENATION
CONTRACT IS ENFORCED.** `str + str` and owned `string + string` now fail in sema with the canonical
`builder` / `.write()` / `.to_string()` guidance. The obsolete MIR path that silently converted
string addition into a two-piece allocating template is removed. Stale tests and the template
example are migrated deliberately: rejection tests pin the uniform rule, while region, escape, and
Drop tests use explicit `template` values so they still exercise their original ownership
invariants rather than passing because of an unrelated diagnostic. The complete workspace remains
green (**2139 total = 2138 passed + one ignored manual probe**). Audit 13 §3.2, the source-fix
ledger, roadmap history, core string design in both languages, and the guides now agree with the
implementation. **Next recommended C0 item:** ship the already-specified zero-cost `str.bytes() ->
slice<u8>` view. Previous update: 2026-07-15 (sixteenth update this day), **DEEP PIPELINE SCALING IS MEASURED AND
REGRESSION-PINNED.** The shared 4-family × 6-depth fixture now runs through check, MIR, optimized
LLVM, and release object emission on an explicit 2 MiB stack. It asserts one fused MIR loop, no
intermediate/closure allocation, no residual simple-stage calls, and legal vector reductions
through depth 32. The same-target O2 throughput harness compares runtime input with equal-LLVM
clang-22 controls. On Ryzen 9 5950X / LLVM 22.1.8, all 24 native and x86-64-v2 points stayed within
7.1% of control; depth-32 ratios were 0.981-1.011 native and 1.000-1.005 baseline. There is no
Align-specific depth cliff. Native named/capture stage cost stayed nearly flat; v2's increase for
long dependency chains matched C and is real useful-work/code-shape cost, not abstraction overhead.
Cache-off compile time + sampled peak RSS are recorded in `bench/deep_pipeline/README.md`; the full
gate and interpretation live in audit §4.5 and `docs/open-questions.md`. Previous update: 2026-07-15
(fifteenth update this day), **DEEP PIPELINE SCALING IS NOW A RECORDED
PERFORMANCE-CONTRACT GATE.** The pipeline remains structurally strong — one fused loop, no
intermediate collections, allocation-free non-escaping captures — but the current equal-LLVM C
evidence covers shallow kernels only. `docs/impl/12-pipeline-closure-memory-io-simd-audit.md` §4.5
now requires a 1/2/4/8/16/32-stage sweep over arithmetic maps, branchless reducing `where`, scalar
captures, and the correctness-required branchy post-`where` callable case. The gate compares
runtime-provided workloads with manually fused Align and equal-LLVM C controls, inspects optimized
IR/assembly for one loop / no intermediates / no residual simple-stage calls / legal SIMD parity,
and reports throughput per performed operation so useful added work is not mistaken for abstraction
overhead. Compile time, peak memory, and small-stack compiler survival are measured separately; the
known accepted-expression-depth versus 2 MiB-stack gap remains a compiler robustness issue, not a
generated-runtime result. `docs/open-questions.md` carries the tracking summary. **Next recommended
performance evidence item:** implement this depth sweep before widening pipeline optimization claims
or changing lowering/inlining policy. Previous update: 2026-07-15 (fourteenth update this day),
**VALUE-CARRYING CONTROL FLOW PRESERVES
CLEANUP PROVENANCE — MERGED as #466** (workspace **2137 green** = 2136 passed + one ignored manual
probe; clippy `-D warnings` clean). The exhaustive 5×2 spec/test matrix now covers region composition
and owned move/drop behavior for block / `if` / `match` / `else`-unwrap / `?`. All five region cells
and block cleanup were already sound; the audit found four real heap-leak gaps where `if`, `match`,
`else`-unwrap, and `?` selected an owned value but lost its runtime individual-vs-arena bit at the
result join. Checked HIR now records static allocation provenance per expression, while MIR stores a
parallel ownership bit beside each owned control result and forwards it to the consumer; direct
local moves still transfer their path-local flag, and moved sources are cleared on the selected
edge. The mandatory self-review also added missing terminated-block guards after a diverging `if`
condition, `else` operand, or `?` operand. Ten dedicated regression cells, the complete workspace,
and clippy are green. Gemini's one high finding covered the same `lower_if` guard on both arms. The
claimed terminator overwrite was already prevented by `Builder::terminate`, but the structural
hardening was valid and applied so a diverged arm performs neither a result store nor a join
termination attempt. Both inline threads were answered and resolved, and an English review-response
and validation comment was posted before squash merge. No mandatory implementation slice remains
queued; fully-escaping function values stay deferred pending a consumer and settled heap-owned
environment/drop semantics. Previous
update: 2026-07-15 (thirteenth update this day), **FUNCTION-VALUE PURITY IS TYPE-BORNE — MERGED as
#465** (workspace **2127 green** = 2126 passed + one ignored manual probe; clippy
`-D warnings` clean). Every concrete function value and fn-typed local now has an internal
`Pure` / `Impure` / `Unknown` effect in `FnTy`; source syntax remains `fn(T) -> R`, and signature
equality deliberately excludes the inferred mutable bit. A least-fixpoint pass refines named
functions, lifted closures, imported interface summaries, and FFI pointers, while mutable locals
join all assigned targets and unresolved higher-order parameters remain fail-closed. Actual
function-value consumers (`CallFnValue` and `map_err`) read the type bit; merely taking an Impure
function's address is no longer falsely treated as performing its effect. Regression gates cover
known-Pure indirect calls and recursive cycles, Impure joins, closures, FFI, cross-unit summaries,
unknown HOFs, unused Impure values, and stable equality of independent effect cells. Gemini's one
high finding caught derived equality comparing mutable `Cell` contents; manual signature-only
equality fixed it, the inline thread was answered and resolved, and the English validation comment
was posted before squash merge. **Next recommended structural item:** write the 1:1
value-carrying-control-flow matrix (block / `if` / `match` / `else`-unwrap / `?`) for region
composition and owned move/drop behavior, and pin every matrix cell with a regression test.
Fully-escaping function values remain deliberately deferred pending a consumer and settled
heap-owned environment/drop semantics. Previous update: 2026-07-15 (twelfth update this day),
**ESCAPE FLOW USES A COMPACT CHECKED-HIR CFG — MERGED as #464** (workspace **2118 green** =
2117 passed + one ignored manual probe; clippy
`-D warnings` clean). `EscapeCheck` now has a construction phase and a transfer phase: the
exhaustive HIR expression match emits compact blocks, operations, and explicit edges for `if`,
`match`, `else`-unwrap, loop backedges, and `break`; one finite may-state worklist is the sole owner
of branch joins and loop fixpoints. A diagnostic probe reaches the fixed input of every block, then
operations replay exactly once in original syntax order, so loop convergence cannot duplicate or
reorder errors. Cleanup provenance from diverging paths remains retained, and syntax after a
terminating `break` is still checked without mutating that break edge. Two regression gates pin the
exact break snapshot and the join across multiple reachable loop exits. Gemini reported no
actionable findings; thread-aware inspection found zero review threads, and the English validation
comment was posted before squash merge. Previous update:
2026-07-15 (eleventh update this day), **OWNED CLEANUP IS PATH-LOCAL — MERGED as #463** (workspace
**2116 green** = 2115 passed + one ignored manual probe; clippy `-D warnings`
clean). Every resource-owning local now has an internal MIR boolean drop flag, while checked HIR
separately classifies each produced value as individually owned or arena-managed. Initialization,
reassignment, direct moves, tuple destructuring, `match` payload binding, loop backedges, `break`,
return, and early cleanup transfer or clear the flag explicitly; each cleanup edge branches around
`Drop` unless that path actually contains an individually owned resource. This safely removes the
fail-closed region-changing owned-reassignment error when the lifetime target check passes, covers
arena→heap, heap→arena, joined-region moves, and the pre-existing owned self-assignment corner, and
keeps Copy-view region joins conservative. Gemini's one medium finding caught two slot-growth paths
that did not grow the parallel flag metadata; the fix was applied, the inline thread was answered
and resolved, and an English review-response/validation summary was posted before squash merge.
Escape diagnostics and provenance stay at the checked-HIR boundary. Previous update:
2026-07-15 (tenth update this day), **ESCAPE PROVENANCE IS FLOW-SENSITIVE — MERGED as #462**
(workspace **2109 green** = 2108 passed + one ignored manual probe; clippy `-D warnings` clean).
`EscapeCheck` now carries one finite `EscapeState` for region and local-backed-slice provenance.
`if`, `match`, and `else`-unwrap analyze mutually exclusive paths from the same input, join only
continuing paths, and exclude branches that always return or break; straight-line assignments are
precise strong updates. Loop heads iterate that state to a fixpoint, and post-loop state is joined
from the reachable `break` exits, closing the two-iteration Frame/local-slice propagation hole.
Function-wide owned cleanup classification remains separate, so an arena-owned local on an
early-return branch is still bulk-freed rather than individually freed even though that branch does
not reach the continuation. Seven regression gates cover the fail-open, false-positive,
strong-update, sibling-control-flow, and double-free directions. Gemini had no findings;
thread-aware inspection found zero review threads, the English validation comment is on the PR,
and the PR was squash-merged. Previous update: 2026-07-15 (ninth
update this day), **ESCAPE PROVENANCE CLASSIFICATION IS
FAIL-CLOSED — MERGED as #461** (workspace **2102 green** = 2101 passed + one ignored manual
probe; clippy `-D warnings` clean). `EscapeCheck::region_of`, local-slice provenance,
region-bearing type classification, and recursive slice-type classification now use exhaustive
`ExprKind` / `Ty` matches with no permissive wildcard: a new node or type is a compile error until
its escape semantics are explicit. The dependency audit corrected the older literal-MIR placement:
`align_mir` consumes safety-verified HIR and depends on `align_sema`, while escape regions derive
`drop_locals`, so the ideal follow-up is a sema-owned region CFG/dataflow pass at the checked-HIR
boundary. Gemini's one medium documentation-style finding was applied, replied to, and resolved;
thread-aware inspection found zero unresolved threads, and the English validation comment is on the
PR. **Next recommended soundness structural item:** build that flow-sensitive region/drop-state
pass, including per-path owned-region/drop flags. Fully-escaping function values remain deliberately
deferred pending a consumer and settled heap-owned environment/drop semantics. Previous update:
2026-07-15 (eighth update this day), **INTRA-FRAME BORROW LIVENESS IS ENFORCED —
MERGED as #460** (workspace **2102 green** = 2101 passed + one ignored manual probe; clippy
`-D warnings` clean). `MoveCheck` now carries shared borrow provenance/invalidation flow state next
to its move state: source moves/replacements and reallocating buffer operations invalidate
dependent views, borrower reassignment establishes a fresh generation, fallthrough branches join,
and loop heads reach a finite may-state fixpoint. The common producer classification covers string,
slice, buffer/CLI/TCP/HTTP views, response arrays, aggregate fields, direct/indirect calls,
value-carrying control flow, tasks, and pipeline captures. Owner-slot roots remain distinct from
copied view-element provenance, so response-array bodies are sound without falsely invalidating a
materialized primitive SoA. Twenty-four adversarial/safe tests pin diagnostics, re-borrows,
diverging paths, chained/captured views, and safe materialization. Gemini had no findings,
thread-aware inspection found no review threads, and the English validation comment is on the PR.
**Next recommended soundness structural item:** move the broader escape/region analysis onto MIR
dataflow so future HIR variants cannot fall through a permissive wildcard; no receiver-specific
borrow patch remains queued. Fully-escaping function values remain deliberately deferred pending a
consumer and settled heap-owned environment/drop semantics. Previous update: 2026-07-15 (seventh
update this day), **wrapper-hidden local-slice returns are REJECTED — MERGED as #459** (workspace
**2078 green** = 2077 passed + one ignored manual probe; clippy `-D warnings` clean). Return/break
escape checks now see a frame-local slice through `Option`/`Result`, tuple, struct, calls, and
value-carrying control flow. Wrapper locals, reassignment, tuple destructuring, and `match` payload
bindings preserve that provenance, while a caller-provided slice remains returnable through the
same wrappers. Gemini had no findings, thread-aware inspection found no review threads, and the
English validation comment is on the PR. Previous update: 2026-07-15 (sixth update this day),
**qualified cross-module function values are
SHIPPED — MERGED as #458** (workspace **2074 green** = 2073 passed + one ignored manual probe;
clippy `-D warnings` clean). A shared named-function reference preserves bare or dotted module
prefixes and resolves through the same import / `pub` classifier as direct calls; all named
callable consumers and ordinary bound values support qualified functions. Gemini had no findings
and the English validation comment is on the PR.
Previous update: 2026-07-15 (fifth update this day), **the HTTP-server fd-leak timing flake is
HARDENED — MERGED as #457** (workspace **2068 green** = 2067 passed + one ignored manual probe;
clippy `-D warnings` clean). `http_server_no_fd_leak_across_cycles` now participates in the
existing fd-sensitive network-test lock and preserves the unchanged persistent-leak threshold;
Gemini's finding was applied and the review thread resolved before merge. Previous update:
2026-07-15 (fourth update this day), **M15
SV SHIPPED — MERGED as #456; the
verification bundle is green and M15 is COMPLETE** (workspace **2068 green** = 2067 passed + one
ignored manual probe; clippy `-D warnings` clean; full record in the roadmap M15 SV paragraph).
The doc-10 §7 matrix is now automated at the settled v1 object-cache boundary: existing gates cover
N=1 and cold-vs-hit byte identity, private/public/transitive invalidation, exact revert, corruption,
runtime freshness, and cross-process `impl_hash`; SV adds unimported-file stability, resolved CPU +
the full key-component namespace matrix, killed-producer orphan staging, identical and different
same-basename cross-process races, and a confirmed fail-closed hardening — interface deserialization
now recomputes the public-surface hash, so a stale/tampered Impure→Pure effect bit is rejected before
sema consumes it (the absent/Unknown/Impure effect gates remain green). Frontend and link still
re-run by design; the record does not claim caches that v1 does not have. Gemini's one high performance finding
was valid and applied before merge: hash the already-present canonical input surface directly
instead of re-encoding the decoded summary; the response and validation record are on the PR.
Previous update: 2026-07-15 (third
update this day), **M15 S3b SHIPPED — parallel unit codegen +
cache CLI + default-ON flip, MERGED as #455. S3 (incremental cache + parallel compilation +
hit/miss observability) is COMPLETE** (workspace **2061 green** + clippy clean; full record in
the roadmap M15 S3b paragraph). The incremental codegen cache is now **ON by default**
(`ALIGNC_CACHE=off` disables, path relocates; flip gated on the cold-vs-hit byte-identity
gate). Parallel codegen claims cache misses via `std::thread::scope` + an atomic claim index
(fresh LLVM `Context` per unit; target-init once pre-scope; DAG-index-ordered results/union/
link, never completion order; `-j`/`ALIGNC_JOBS`; explain-opt stays serial + uncached; gemini's
fail-fast finding applied — claim-loop `AtomicBool`, in-progress emits never interrupted).
`--cache-stats` (build/run/size) + `alignc cache clear` (symlink-safe, cache-owned subtrees
only). Runtime-archive freshness is now a **content digest** baked by `build.rs` (in-tree
`align_hash` as a build-dep, no external dep) — the post-merge `cargo test` false-stale
papercut is GONE (a genuinely stale archive still fails loud). Non-cache alignc-spawning test
files are pinned `ALIGNC_CACHE=off`; a default-ON smoke gate (temp XDG root) proves
second-build all-hit + byte-identity. **Next M15 step = SV (verification bundle): the doc-10
§7 invalidation matrix per-unit, N=1 byte-identity vs today, cold-vs-hit byte-identity,
fail-closed effect-bit gates incl. a stale/absent-interface mutation, + the S3-review
additions (cross-process impl_hash stability pin, transitive A→B→C invalidation).** Still
queued behind the M15 slices: the `http_server_no_fd_leak_across_cycles` flake-hardening slice
(recurred again during S3b full-suite runs, same signature) and the qualified cross-module
fn-value remainder (`map(util.dbl)`, open-questions). Previous update: 2026-07-15 (second
update this day), **M15 S3a SHIPPED — the incremental
codegen-stage cache (opt-in), MERGED as #454** (workspace **2051 green** + clippy clean; the S3
design was SETTLED the same day by a two-lens review — soundness/key-correctness +
driver/layout/scheduling/observability — recorded as the roadmap M15 "S3 design SETTLED"
paragraph = the S3 implementation source of truth; that review also CORRECTED settlement Q6's
frontend-key wording "direct-dep" → **TRANSITIVE** dep interface hashes — a stale-check hole as
written, since interface hashes do NOT chain (foreign type refs are by-name); the shipped
`PerUnitCheck` was already transitive). v1 caches per-unit OBJECT bytes only (frontend always
runs — it produces the keys; link always re-runs), keyed by the full doc-10 §6.2 codegen key:
compiler build id = memoized runtime hash of the `alignc` binary, exact LLVM version via
`LLVMGetVersion`, RESOLVED cpu/features shared with `create_target_machine`, `impl_hash` one
component never the whole key. CAS + versioned fail-closed manifests under
`${XDG_CACHE_HOME:-~/.cache}/alignc/<schema-ver>/` (`ALIGNC_CACHE=on|<path>|off`, DISABLED by
default until S3b's gated flip; a disabled build does zero cache-key work). Headline win pinned:
a dep private-body edit → that unit misses, every dependent HITS, exe correct.
`CacheOutcome`/`FirstDiff` observability model from slice 1 (tests assert the enum, never
elapsed time; full-key actions + a stable-slot index — the recorded deviation that makes
first-diff reasons computable). 10 integration gates + 5 unit tests incl. transitive A→B→C
invalidation, edit-revert hit, corruption evict+rebuild, cold-vs-hit byte-identity, N=1 gates
untouched. gemini reflected pre-merge: staging-leak fix APPLIED; Windows cache root REJECTED
(documented — fail-closed unsupported target); its build-id suggestion surfaced a REAL defect
(unconditional key construction with cache off — fixed via `is_enabled()` gate) while the
compile-time-constant swap was rejected (dev-rebuild false-hits). Hash decision owner-visible in
the roadmap record: keys/CAS stay in-tree 128-bit wyhash; FIRM trigger — any shared/cross-host
cache must FIRST swap to a cryptographic 256-bit digest. **Next M15 step = S3b: parallel unit
codegen over cache misses (`std::thread::scope`; LLVM target-init once pre-scope; explain-opt
stays serial), `--cache-stats`/`-j`/`ALIGNC_JOBS`/`cache clear`, runtime-archive mtime →
content digest, then the default-ON flip gated on the cold-vs-hit byte-identity gate; then SV.**
Previous update: 2026-07-15, **M15 S2b SHIPPED — the default build is per-unit, MERGED as #453**
(workspace **2035 green** (2022 + 13) + clippy clean; gemini review ZERO findings; design settled
by a two-lens review — driver/UX + soundness/byte-identity — integrated before implementation;
full record in the roadmap M15 S2b paragraph). Hard cutover: `build`/`run`/`emit-obj`/`size` all
route through the per-unit pipeline; the `build-per-unit` verb, `build_to`, and `front_to_mir`
are DELETED (no-backward-compat). **N=1 stays byte-identical to the whole-program object/exe by
construction** (entry-unit fns internal), now pinned by a sweep across profiles + `--rt-lto`.
`emit-llvm`/`emit-mir` emit each unit bottom-up with a unit banner ONLY when N>1 (N=1
byte-identical); `--stage optimized` optimizes each unit in isolation — the truth under zero
cross-unit opt (a cross-unit `pub` call visibly stays un-inlined while intra-unit calls inline).
`explain-opt` captures remarks SERIALLY per unit (process-global cl::opts) with per-unit sections
when N>1, via new `lower_program_per_unit_located` (carries BOTH exportable bits and
`stmt_lines`; factored through `lower_program_impl`, no lowering-body duplication). `emit-obj`
N>1 writes one `<module>.o` per unit; a single `[out.o]` positional with N>1 is a hard error
with guidance. **`--export` is ENTRY-UNIT-ONLY, fail-closed**: a name defined in a non-entry
unit errors naming the defining unit + "mark it `pub`" (non-entry `pub` fns are already
external — the one way to export them); never a silent no-op. `check` STAYS whole-program sema
(better "private" diagnostics; verdicts identical per S1b — note the per-unit "unknown" wording
is now the default multi-file BUILD-time experience, recorded as accepted). 13 new gates in
`crates/align_driver/tests/per_unit_surface.rs` (N=1 identity sweep, CLI-level equivalence,
export routing/fail-closed errors, emit-llvm banners + opaque boundary, explain-opt N=1
identity + sections, size multi-file, ≥3-unit run-to-run determinism, capability union
libz-only, rt-lto multi-file smoke). **Next M15 step = S3: incremental cache per the doc-10
contract + parallel unit compilation + hit/miss observability**; then SV (verification bundle).
Still queued behind the M15 slices: the `http_server_no_fd_leak_across_cycles` flake-hardening
slice and the qualified cross-module fn-value remainder (`map(util.dbl)`, open-questions).
Previous update: 2026-07-14 (seventh update this day), **M15 S2 first stage SHIPPED — per-unit
codegen + N-object link, MERGED as #449** (workspace **2019 green** + clippy clean;
adversarial gate SHIP — it built a 15-type-class cross-unit ABI parity matrix, all IDENTICAL
whole-program vs per-unit, incl. big-struct by-value, Result/Option aggregates, Move
string/array, `out` slice, Unit return; gemini 4/4 mediums applied pre-merge). New
`alignc build-per-unit` + `build_per_unit()`: `walk_per_unit` → per-unit sema (S1b) → new
`lower_program_per_unit` (own fns + consumer monomorphs; `mir.imported_fns` carries imported
`pub` signatures) → per-unit `emit_object` (new `declare_imported_fn` shares ONE `abi_map_ty`
with `declare_fn` — declare/definition signatures structurally cannot diverge, extracted per
the gate) → deterministic capability union → existing `link_objects`. **N=1 byte-identity is
EXACT** (fs::read equality; key refinement: the ENTRY unit's pub fns stay internal — importing
the entry would be a cycle, rejected by S0, and this is what makes N=1 exactly today's
whole-program object). `--rt-lto` merges per unit; `impl_hash` upgraded to stable-printed
location-free per-unit MIR (clone-free via new `align_mir::print::function_to_string`;
process-deterministic, declaration-reorder-robust; closes the S1a TODO). Default paths
untouched. Remainders → S2b: default flip + per-unit `size`/`explain-opt`/`emit-llvm`/
`emit-obj --export`; S3: incremental cache + parallel compile. **NEW pre-existing bug found by
the gate (open-questions):** Unit-returning `fn main()` returns a NONDETERMINISTIC exit code
(return register left undefined by the C-entry wrapper; same binary gave 88/216/168/120/104)
— **FIXED the same day, MERGED as #450** (workspace **2022 green**; gemini zero findings):
the existing Result-main wrapper mechanism generalized — a Unit `main` is renamed
`align_main` (internal) and the C-entry wrapper emits `call void @align_main()` +
unconditional `ret i32 0`; `-> i32` main untouched; pre-fix nondeterminism reproduced via
stash then pinned by run-same-binary-5× determinism tests on BOTH paths + a `link_hygiene`
IR-shape pin. Previous update: 2026-07-14 (sixth update), **M15 S1b
(consumer) SHIPPED — MERGED as
#447, workspace 2006 green (1969 + 37) + clippy clean** — per-unit sema consuming imported
interface summaries; full record in the roadmap M15 S1b paragraph. Adversarial gate verdict
SHIP-with-fixes: the render-to-source seam proven FAITHFUL (`layout(C)`/`align(N)`/field
order/`out[i]`/Move classification/const escaping all round-trip; const "injection" content
stays a string value; no external summary-loading surface this slice) and the whole-program
path proven unaffected (every callee is in `program.fns` → effect-seeding loop is a no-op).
Its one confirmed defect — a generic `pub` template body referencing a private helper:
whole-program accepted, per-unit rejected with a leaked `<interface:…>` location — was fixed
as a **new language rule extending #446: a generic `pub` fn's body may reference only `pub`
items** (sema pass 0e: exhaustive expr/stmt/type/pattern walker + lexical scope stack so
locals shadow item names; runs inside `check_program_with_effects`, so both checkers agree by
construction; diagnostic at the real template span; match-pattern variant names deliberately
unchecked with recorded reasoning; `draft.md` §17 + language-spec carry the rule; NO existing
fixture used the hole). gemini applied pre-merge: parsed interface ASTs cached — each dep
rendered with its OWN transitive closure (importer-independent → soundly cacheable), parsed
once per walk (O(N²) → O(N)). **Pre-existing bug surfaced by the gate and recorded in
`open-questions.md` Open:** by-name fn-value references (`map(dbl)`) fail in NON-ENTRY modules
on the untouched whole-program path too (direct calls / `map_into` fine — likely a mangled-name
gap in fn-value resolution). **The earlier 238/1/1 flake is now IDENTIFIED:**
`align_runtime::tests::http_server_no_fd_leak_across_cycles`, a timing flake under
full-suite parallel load (passes in isolation and on rerun) — it recurred THREE times on
2026-07-14 (post-#446, during the #447 review-fix verification, post-#448 merge), always the
same 238/1/1 signature. Noisy enough to deserve a small hardening slice (serialize the test
or make the fd-count check retry-bounded WITHOUT weakening its leak-detection teeth) — queue
it behind the M15 slices. **The non-entry fn-value bug is FIXED, MERGED as #448** (2013
green): the six pipeline/reducer callable-resolution sites now route through the same
`resolve_local_fn` as direct calls; the open-questions entry is narrowed to the qualified
cross-module callable remainder (`map(util.dbl)` — `pipeline_fn_name` single-segment
limitation, same class as the recorded `par_map` one). **Next M15 step = S2 (per-unit codegen
+ N-object link).** Implementation shape of the merged slice: seam =
**summary→source→re-parse**: an imported unit's public surface is rendered back
to Align source (`align_interface::summary_to_source`) and re-parsed by the EXISTING parser into an
interface-only `Module` (`Module::interface_only`), so ALL sema table-building + resolution passes
are reused unchanged — ONE resolution path (generic templates + const values must be re-parsed
anyway, so render-to-source unifies it; NO second resolver). Cross-unit effect bits seed
`compute_effect_sets`/`fn_effects`/`check_parallelism` via new `check_program_with_effects`;
a callee absent from the seed map is **fail-closed** to impure + unknown-indirect. Driver
`check_per_unit` walks the DAG bottom-up, reconstructs each transitive dep, checks each unit,
re-derives its summary, and records the **transitive (unit, interface_hash) set** per unit —
the S3 cache-key input (`PerUnitCheck`; dev verb `alignc check-per-unit`). 25 new tests (21
per_unit differential/blindness/transitive + 4 effect fail-closed); workspace green; clippy
`-D warnings` clean. Honest remainder: per-unit summary production runs per-unit MIR lowering for
capabilities only (outside the interface hash); private-cross-unit-access diagnostic differs by
design ("unknown" per-unit vs "private" whole-program — verdict identical); S2 (per-unit codegen)
and S3 (incremental cache) remain. Previous update: 2026-07-14 (fifth update), **M15 S1b entry
gate SHIPPED — a `pub`
interface may name only `pub` types, MERGED as #446** (workspace **1969 green** + clippy
clean; gemini zero findings). Flake note for the record: ONE post-merge full-suite run showed
a single failure in the `align_runtime` test binary (238/1/1 at 0.50 s; the specific test name
was not captured); the full suite re-ran clean (1969/0/1) and the binary passed 5/5 targeted
re-runs — treated as a one-off timing flake under transient load, not a regression; if it
recurs, capture the test name. Sema pass 0a-2: `check_type_exposure` recurses exhaustively over
all `ast::Type` constructors (Named/Tuple/Fn, no wildcard — a new constructor is a compile
error there) for every `pub` fn signature (type params exempt), `pub` struct field, and `pub`
sum payload, incl. generic args of qualified types (`mod.Wrapper<Secret>`); cross-module
private access was ALREADY rejected, so the check is same-module-only; `pub const`
(scalar/`str`-only) and `extern` fns (FFI scalars) are structurally exempt — documented +
tested. Diagnostic names item/type/position. **Signal: 8 existing SoA fixtures used exactly
this hole** (private struct exposed via `pub fn` over `soa<T>`/`array<T>`) and were corrected.
This closes S1a-recorded entry gate (1); **gate (2) — transitive imported-unit interface-hash
keying — still governs S1b proper** (per-unit sema consuming summaries), which is the next
M15 step. Previous update: 2026-07-14 (fourth update), **M15 S1a SHIPPED — the unit interface
summary (producer side), MERGED as #445** (workspace **1957 green** + clippy clean;
adversarial gate SHIP — fuzzed reader panic-free across 2900+ probes; gemini's one medium —
allocation-free unit-prefix match — applied pre-merge). New crate `align_interface` (ast/sema/
mir/hash deps, NO codegen): per-unit `InterfaceSummary` = `pub` signatures incl. `out[i]` +
full exported type defs + consts + **a 3-valued effect bit (Pure/Impure/Unknown) taken from
the SAME `compute_effect_sets` the parallel-safety gates use** (refactored out of
`check_parallelism` — single source of truth) + generic template bodies (source text) +
capabilities (as data, deliberately OUTSIDE the interface hash — link-summary concern).
Canonical codec: versioned, u32-LE length prefixes (loud guard), name-sorted exports,
fail-closed reader. `interface_hash` INCLUDES effect bits + template bodies (the settlement's
two easy-to-miss cases, both pinned by hash-split tests); `impl_hash` = source bytes,
`TODO(m15-s2)` → per-unit MIR. Driver: `load_units` + `build_interface_summaries` +
`emit-interface` verb; pipeline byte-identical to pre-change main (gate-proven). **S1b (the
consumer side) has TWO recorded entry gates: (1) MANDATORY first — sema must reject a `pub`
signature exposing a non-`pub` type** (today accepted, empirically confirmed; private types
are summarized by NAME only → a layout change would not flip the interface hash → stale-object
miscompile once summaries are consumed); **(2) consumers key on the TRANSITIVE imported-unit
interface-hash set** (foreign type refs are by-name). Both are in the roadmap S1 record.
Previous update: 2026-07-14 (third update), **M15 S0 SHIPPED — cyclic imports are now a
hard error, MERGED as #444** (workspace **1945 green** + clippy clean; gemini's one medium —
`&'a str`-borrowing DFS maps, zero per-node allocations — applied pre-merge). `check()` records
every import edge independently of the `seen` dedup, then a white/grey/black DFS from the entry
module reports the first cycle (direct/transitive/self-import) with the full path at the
closing edge's span; **diamond reconvergence stays legal** (Black ≠ Grey — pinned by a
build-and-run positive control); sema still runs after the diagnostic (accumulate contract).
`draft.md` §17 has the DAG paragraph (settled wording), `docs/language-spec.md` mirrors it in
one sentence. **Next M15 step = S1 (interface summary: canonical serialization + interface/impl
hashes + per-unit sema against imported summaries — effect bits, type defs, template ASTs; per
the roadmap slice plan).** Previous update: 2026-07-14 (earlier), **M15 separate-compilation
design SETTLED by the owner-mandated two-lens review** (language/soundness + driver/artifacts/cache lenses run in
parallel, integrated; full settlement + S0–SV slice plan recorded in the roadmap M15 section =
the implementation source of truth; open-questions item updated in place). Headline soundness
result: **the unit interface is COMPLETE** — escape/region is already body-blind (no hidden
escape channel), Move/Copy is type-derived, MoveCheck is caller-derivable, and the one
genuinely whole-program analysis (purity/effect) reduces to a 3-valued per-`pub`-fn effect bit computed
bottom-up over the unit DAG, **fail-CLOSED** (missing/Unknown ⇒ Impure+unknown-indirect ⇒
rejected at parallel boundaries — the #433 pattern; the fail-open-wildcard class is designed
out). Unit = one module/file; driver-discovered DAG; **new rule: cyclic imports = hard error**
(no other language-surface change); generics = instantiate-in-consumer (serialized template
ASTs; duplicate `internal` monomorphs accepted in v1, `linkonce_odr` dedup deferred to
ThinLTO); visibility = `{main} ∪ --export ∪ pub` external; **v1 = ZERO cross-unit
optimization** — artifact = keyed envelope of CAS parts (interface/impl/link hashes; consumers
depend on interface hashes ONLY — the hash INCLUDES effect bits + generic template bodies, the
two easy-to-miss interface changes), reserved bitcode parts + an empty-in-v1 cross-unit-opt
codegen-key digest make ThinLTO a later addition, not a format break; incremental cache per
the doc-10 contract (never mtime), parallel unit compile verified UNBLOCKED (fresh LLVM
`Context` per codegen entry + shipped C0); hard cutover — **N=1 IS whole-program,
byte-identical** (protects the reproducibility + rt-lto gates). Recorded honest trade:
multi-file programs lose cross-module inlining until ThinLTO. `--rt-lto` merges per-unit in
v1. `extern "C"` export-of-body stays out (noalias trust chain). Next M15 step = **S0
(cyclic-import hard error + draft.md §17)** then S1 (interface summary). Earlier same day:
**M14 Slice 2 (runtime-bitcode LTO) SHIPPED — MERGED as #443**
(design settled the same day by a two-lens review — soundness + build-integration — recorded
as "M14 Slice 2 design SETTLED" + "M14 Slice 2 SHIPPED" in the roadmap M14 section; adversarial
gate verdict SHIP with zero confirmed defects; gemini 3/3 findings applied pre-merge; workspace
**1940 green** + clippy clean). Mechanism: the four memcmp-class primitives
(`str_eq`/`starts_with`/`ends_with`/`eq_ignore_case`) + their sole callee `safe_slice` extracted
verbatim to `align_runtime/src/str_prims.rs` (ONE source, compiled twice), compiled standalone to
a minimal `.bc` by the new `align_driver/build.rs` (`rustc --emit=llvm-bc -O -Ccodegen-units=1
-Cpanic=abort --target $TARGET`) and **baked into `alignc` via `include_bytes!`** — the staleness
question dissolved (same `cargo build` regenerates it, same rustc = no LLVM-major skew). Opt-in
**`--rt-lto`** (valid on build/run/emit-obj/size/emit-llvm; diagnostic-rejected on
dev/small/tiny + non-build verbs, both negative-tested) parses it from memory, links into the
RAW module, sheds exactly the guarded `rt_contract` attrs (attr-xor pinned:
`(has body) != (carries curated attrs)`), `mark_internal`s the merged bodies directly (export
roots untouched by construction), then runs the ONE existing opt pipeline — never a second
(the probe's double-opt is what regressed `str_cmp`). **`str_cmp` exclusion is STRUCTURAL** —
its body never exists in the artifact (defined symbols in the `.bc` == the guarded four,
undefined == {bcmp}, pinned). Numbers through the REAL driver: **`eq_count` 2.95× under
`--rt-lto`** (identical hit counts), numeric control 1.01×, **compile-time +2 ms**, flag-off
objects **byte-identical** to pre-change main; `bench/rt_lto/` records them. Mutation teeth
verified both directions TWICE (implementer + independent gate). Fail-loud paths pinned:
unparseable-bitcode fallback re-annotates the guarded declares (regression test through the
public `Option<&[u8]>` seam), and datalayout mismatch is a loud fallback, never a
force-overwrite. Three recorded deviations, all source-verified: inkwell 0.9's
`MemoryBuffer::create_from_memory_range[_copy]` asserts a trailing nul and passes `len-1` (a
raw `include_bytes!` slice would silently lose the last bitcode byte → nul-append shim);
`-Cpanic=abort` drops `rust_eh_personality`; `emit-llvm --stage raw --rt-lto` also links (the
pre-opt attr-xor lens). Parked on this slice with records: the per-target-cpu runtime variant
+ cache key (the `hash64` native-tuning lever) and `utf8_valid` behind its own ≥ 1.15× bench.
Previous update: 2026-07-13, **M14 Slice 1 (LTO ceiling probe) DONE — ABOVE GATE, proceed to
Slice 2** (full record + tables in the roadmap M14 section, commit `52ecfb1`) **and the
`bench/binary_size` portability port MERGED as #442** — Codex-audit item 2 is now closed in
full. Probe verdict (median of 7, 1M short strings, znver3): **`str_eq` 2.12× native / 2.35×
generic = a genuine LTO win** (the runtime `str_eq` inlines and a constant target length folds
into an inline length-check fast path — zero call/`bcmp` for non-matching lengths);
**`str_cmp` REGRESSES to ~0.72×** under a blanket post-link `default<O2>` — hard evidence that
Slice 2's per-symbol guard is mandatory; `hash64` 1.63× native but 1.02× generic = a
native-tuning effect, better captured by the deferred per-target-cpu runtime variant, not
bitcode; numeric control 1.00×. Link+reopt+llc over the full runtime `.bc` ≈ 0.25 s. **Slice 2
re-scoped accordingly:** ship LTO visibility only for the inlinable fast-path string primitives
(`str_eq` and kin), per-symbol guarded, excluding `str_cmp`. Also confirmed: the #440
`captures(none)` fix makes `emit-llvm --stage optimized | llvm-as-22` round-trip cleanly — the
2026-07-12 `ptr none` sed hint is obsolete, the probe ran the straightforward pipeline. #442:
`bench/binary_size/{run,profiles}.sh` GNU/ELF assumptions (`stat -c`, bash-4 `mapfile`,
`readelf -d`, `.symtab` checks) replaced by a shared `lib.sh` (portable `filesize`, `llvm_tool`
discovery mirroring the driver, format-general `llvm-readobj --needed-libs`/`llvm-nm` helpers
with tool-failure degrading to `?` — gemini's one valid finding, fixed); Linux before/after
byte-identical; Mach-O branch honestly marked unexercised on real hardware. Previous update:
2026-07-13, **Codex-audit wave-2 (quick wins) COMPLETE — #439 + #440 + #441
MERGED** (three parallel worktree slices, gemini reflected before each merge; workspace
**1931 green** + clippy clean on merged main). **#441 sort:** `lower_array_sort` rewritten as a
stable bottom-up merge sort with an insertion base case (threshold 32), deliberately kept as MIR
expansion — the comparison is already polymorphic over every scalar and `str` key via `BinOp` →
`align_rt_str_cmp`, so no runtime type matrix; `sort_by_key` is true decorate-sort-undecorate
(each key computed exactly once); measured **~220× on a 100k-element sort** (~1075 ms → ~4.9 ms
sort-only); 11 new tests incl. stability and base-case⇄merge boundaries. **#440 attr hardening:**
shared `enum_kind_id` fail-loud gate (panic — every attr name is a compiler-internal literal) +
first-class `captures(none)` emission (raw CaptureInfo value 0, pinned to the LLVM 22.1.8
`ModRef.h` encoding); **root cause of the recorded `emit-llvm | llvm-as-22` round-trip failure
found and FIXED** — the removed `nocapture` name resolves to kind id 0 on LLVM 22 and prints as
the un-reparseable `ptr none`; emitting `captures` directly restores the textual round-trip,
pinned by a tool-gated `llvm-as` gate, so the roadmap M14 follow-up is resolved early. **#439
runtime quick wins:** tiny `par_map` (≤ PAR_MIN_CHUNK) now runs on the caller before `par_pool()`
ever spins up (pool non-initialization pinned via a test-only introspection hook), the identical
shape fixed in `task_group` n=1, and zero-size arena allocs return an aligned dangling pointer
with no chunk fetch (allocation-counter pinned). Operational note: after a merge that touches
`align_runtime` sources, `cargo test` alone trips the deliberate `libalign_runtime.a` staleness
guard (the `.a` refreshes only on `cargo build`) — run `cargo build --workspace` first; the 4
apparent size/staging failures right after the wave-2 merges were exactly this, not regressions.
Codex wave-2 is closed; remaining Codex items = wave-3 measure-first (JSON decode double-alloc,
I/O buffer zero-fill) + the `bench/binary_size` port. Previous update: 2026-07-13,
**Codex-audit wave-1 COMPLETE — the two remaining CONFIRMED bugs
fixed, #437 + #438 MERGED** (both two-lens designed; gemini reviews reflected before merge;
workspace **1914 green** + clippy clean on merged main, verified on the Linux/WSL2 host).
**#437 profiles-reach-the-backend:** `Profile::codegen_opt_level()` (dev=None / release=Default /
fast=Aggressive / small,tiny=Default — size comes from attrs, clang-style) threaded into
`create_target_machine`; `optsize` (+`minsize` on tiny) swept over module **definitions only**
(disjoint from the #423 rt_contract declare table by the `count_basic_blocks` split); the
diagnostic lenses (`emit-llvm`/`explain-opt`) stay pinned at Default/Release so every IR-shape
suite is byte-identical; the release (no-flag) object proven **sha256-identical** to pre-change
main. The per-profile **runtime variant + cache key is DEFERRED with a recorded key spec**
(profile / panic-strategy / LLVM-major / rustc / runtime-source-hash / target) to the M14
runtime-bitcode work + the doc-10 §2 cache layer — no cache substrate exists to key today.
**#438 `--export` roots:** repeatable `--export <name>` on `emit-obj`/`emit-llvm` only
(diagnosed, not ignored, on other verbs), fail-closed against MIR fn names, and `declare_fn`'s
internalization guard keyed on the **symbol** — gemini's one high finding was REAL
(`--export main` left `align_main` external; reproduced, fixed, regression-pinned as a harmless
no-op); export = linkage + DCE root from the same set; the bench export contract is restored
(`bench/run.sh baseline/native` + all six kernel-linking sub-benches run end-to-end again;
`bench/README.md` rewritten — `--export` is the object's C-ABI surface, independent of `pub`).
Codex wave-1 is now fully closed (item 2 shipped earlier as #426); remaining Codex work =
wave-2 quick wins, wave-3 measure-first, and the `bench/binary_size` script port. Previous
update: 2026-07-13, **artifact collision C0 FIXED**: production codegen objects live in
per-invocation private temp directories, linked executables are staged beside their final path and
atomically renamed only after success, and `run`/`size` retain private directories through use.
Twelve concurrent same-basename run/build pairs plus eight size pairs pass; reverting either object
or run staging to a shared name independently executes the wrong program. Deterministic diagnostics,
reproducibility, and whole-program CAS remain open in document 10. Also this date,
**dynamic allocation-size C0 HARDENED**: `ArenaAlloc`, `HeapAllocBuf`,
and `SoaAlloc` now reject negative counts, checked multiply/add overflow, and byte results above the
signed `i64` allocator ABI before allocator work. SoA uses an allocation-only checked offset walk,
leaving ordinary column access unchanged. Raw/O2 gates pin negative/zero/largest-fitting/one-over,
widening, and padding cases; removing the signed-result or alignment-add guard fails independently.
Also this date, **nested parallel scheduler P0 FIXED**: `par_map` now uses a shared
atomic range cursor, caller/helper drain loop, and total-range completion barrier. Saturated
`task_group -> par_map` therefore makes progress with zero idle pool workers; a forced-two-worker,
`workers + 1` task child-process gate is watchdog-bounded, and stopping each runner after one range
reproduces the old timeout. Also this date, **parallel lifted/higher-order effect P0 FIXED**:
capturing closures first contributed a conservative lifted call edge and unknown function-value
calls were rejected at Pure/`par_map` boundaries. #465 later replaced the name-edge workaround with
inferred `FnTy` effects while preserving fail-closed unknown HOF parameters and legal sequential
calls. Named/capturing/higher-order gates live in `analysis_coverage.rs`. Also this date,
**post-`where` callable P0 FIXED + sequential effects SETTLED**:
reducing MIR guards every general callable suffix/reducer on rejected elements, while safe field +
builtin `sum`/`count`/`min`/`max` suffixes retain the vectorized mask/select path. Sequential
`map`/`where`/`reduce`/`scan`/`partition`/`any`/`all` may be Impure with exact input/stage order;
`par_map` remains Pure-required and `sort_by_key` key evaluation stays separately Open. Trap,
ordering, callable-reducer, and vector-shape gates are pinned. Also this date, **spawn-capture
lifetime P0 FIXED**: `EscapeCheck` now tracks active
task-group regions and rejects direct or wrapped captures that can be freed before the group's
`wait`; frame/static/outer-arena captures remain valid, with the full matrix pinned in
`task_group.rs`. The focused source-correctness wave is recorded in
`docs/impl/source-correctness-fixes-2026-07-13.md`: UTF-8-safe `str` range boundaries,
spawn-capture and closure-result environment regions, Unit function-value ABI parity,
buffered-reader-correct `io.copy`, borrowed pipeline-source ownership, self-aliasing
`buffer.append`, line-head `!=`, duplicate struct fields, and checked dynamic allocation sizes are
regression-pinned.
Previous update: 2026-07-13, **string/array allocation-copy and short-input audit RECORDED** in
`docs/impl/13-string-array-allocation-short-input-audit.md` (UTF-8 range boundaries now fixed; other
items not started). It adds
confirmed correctness/resource prerequisites for UTF-8 slice boundaries, settled `str + str`
enforcement, arena-free template lifetime, unbound owned-temporary drops, and known-null destructor
calls. New mechanical allocation/copy work covers borrowed path ABI views, compatible zero-copy
builder freeze, direct `read_dir`/DNS/path/group outputs, tiny builder headers, and virtual direct-
consumer chunks. UTF-8 short crossover, repeated-needle plans, JSON escape SIMD, and large constant
arrays stay measure-first with explicit `0..64` gates. Language-surface ideas are questions for
Claude Code only, not decisions. Previous update: 2026-07-13,
**pipeline/closure/memory/I/O/SIMD audit RECORDED** in
`docs/impl/12-pipeline-closure-memory-io-simd-audit.md` (spawn-capture and closure-result regions,
Unit indirect-call ABI, and buffered `io.copy` fixed; other items not started). The normal
fused sequential loop, `map_into` alias metadata, capture inlining, JSON/UTF-8/string SIMD, direct
file read/mmap, and buffered small/direct large writer paths are strong. New correctness-first work:
post-`where` callable speculation and the ordinary sequential effect-contract conflict are now
fixed/settled as described above; the
`spawn` capture region gap and the related indirect closure-result gap are now fixed; Unit indirect
calls used an incompatible LLVM return ABI, and
`io.copy` skipped buffered-reader lookahead (both fixed in the
source-correctness wave). Dynamic allocation size arithmetic is now hardened as described above.
Highest-value new measured/gated work is an
initialized-before-read arena split, exact-final-allocation Base64/hex fill paired with the existing
Base64 SIMD backlog plus a new hex probe, macOS copy-path validation, HTTP batch request-copy removal,
and sequential SIMD compaction only if its selectivity matrix wins. No new syntax. Previous update:
2026-07-12,
**cache-first compilation/output-code audit RECORDED** in
`docs/impl/10-cache-first-optimization.md` (artifact collision fixed; determinism/CAS not started).
At the audit baseline, basename-only temporary object plus `run`/`size` executable names corrupted
concurrent different `main.align` builds in 40/40 paired probes; private staging now closes that defect. The record makes
private staging + content-key identity + atomic publication the pre-M15 correctness slice, defines
the staged CAS/interface-vs-implementation/runtime-link key contract and validation matrix, records
diagnostic nondeterminism (`HashMap::keys()` const evaluation: 17 stderr orders across 24 fresh
processes), and keeps four CPU-cache ideas explicitly MEASURE-FIRST (owned-temp buffer donation,
wide AoS→SoA blocked construction, `task_group` batching, stack lifetime markers after MIR
liveness). Existing valid objects/executables were byte-identical across repeated/relocated builds,
so the CAS substrate is promising but not yet regression-pinned. **Parallel execution/generated-IR
companion audit RECORDED** in `docs/impl/11-parallel-execution-optimization.md` (both correctness
P0s fixed; performance work not started): the lifted/higher-order effect holes and saturated nested
scheduler deadlock are now closed. `par_map` callers and helpers drain shared coarse ranges, so a
structured caller never depends on an idle pool worker. The already-planned per-element-thunk removal remains the main IR lever; new gated
candidates are read-only parallel capture context, wrapping-integer `par_map(...).sum()` fusion to
eliminate the full intermediate write/read, length-preserving staged kernels, low-lock task
claim/completion + queue batching, packed task records, body/byte-aware grain, and only later a
generic application of the already-recorded CPU-vs-blocking pool direction. No new source syntax
is recommended. Earlier this date: fourteenth wave
(**Codex-audit item 2 — the macOS link/size
portability slice COMPLETE, MERGED as #426**; independent adversarial gate: SHIP, zero
branch-caused confirmed defects, all three mutations caught. `ObjectFormat` +
`target_object_format()` in codegen (apple/darwin → Mach-O, windows fail-closed, else ELF; the
one triple-classification site, M15 cross-compile seam noted); `link_objects` selects its flag
dialect per format via `hygiene_flags`/`support_libs` data tables — ELF unchanged, Mach-O
`-dead_strip`/`-dead_strip_dylibs`, no support libs (libSystem re-exports), post-link external
`strip` (re-signs ad hoc; `Profile::strip` stays the sole strip decision) — plus a
`LIBRARY_PATH` hint on gated-library link failures; `llvm_tool()` discovery (build prefix →
`<name>-22` → bare name); `alignc size` fully migrated to version-matched
`llvm-readobj`/`llvm-nm` for BOTH formats (readelf/GNU-nm removed outright; Mach-O symbol sizes
derived from address deltas capped at section ends, dyld chained-fixups note,
`LC_LOAD_DYLIB` listing). macOS arm64 verification: `align_attr` (its 3 link failures resolved),
`capability_linking` (now `--needed-libs` + basename matching + a `can_link` probe),
`build_profiles` (format-branched expectations), and the new `macho_link.rs` net (hello =
libSystem-only + runs; tiny strips every symbol and stays runnable) all green — previously every
macOS link failed on ld64. Remaining macOS failures are cataloged out-of-scope in the adoption
record (ffi_byval = SysV-Linux-only by design, std.net/TLS/cloexec runtime items, APFS non-UTF-8
setup, expr_depth test-thread stack); rode along: the m11_process `/bin/true` → `coreutil()`
existence-probe portability fix and two macOS-only compile errors in the `align_runtime` test
target (clashing test `fcntl` declaration, aarch64 `needless_return`). Deferred sub-items
recorded at adoption-record item 2 — `bench/binary_size` port is the next small PR.) Earlier
this date: thirteenth wave (**LLVM/inkwell upgrade checkpoint COMPLETE — LLVM
19 → 22 MERGED as #425**, the checkpoint sequenced after M13; the M13 shape/size/bench net served
as the gate and caught exactly three behavior shifts, all re-pinned only after IR/objdump
verification). Toolchain: inkwell `llvm19-1` → `llvm22-1` (0.9.0), llvm-sys 191 → 221 (221.0.1),
`.cargo/config.toml` → `LLVM_SYS_221_PREFER_DYNAMIC=1`; the shared-only stance was re-verified and
its rationale UPDATED — apt.llvm.org llvm-22 ships static archives and Polly is no longer a
`--libs` component, so static would now work; dynamic kept as smaller + rustc-matching. clang-IR
harness → clang-22 (same-LLVM requirement); LLVM 19 stays installed (rollback/shape-diff triage).
The three shifts: (a) LLVM 22 SCEV constant-folds constant-array reductions to closed form (zero
vector-reduce emitted) → kernels re-seeded opaque from `args.len()`; under opaque data 22
vectorizes to the SAME widths/intrinsics as 19 (`reduce.add.v4i64` at v3 / `.v2i64` at v2; objdump
re-verified v2 `paddq`-no-`ymm`, v3/skylake `vpaddq`+`ymm`); clang-compare k4/k5 negative controls
deliberately stay constant-form (opaque seeding would false-positive the harness vec-detector on
SSA init stores). (b) the `vector.body` block name is unreliable for inlined reduction loops on 22
→ reduction detection re-keyed on the mangled reduce intrinsic (`llvm.vector.reduce.<op>.v<N>i64`,
which also pins width) + `vector.ph`; mutation teeth re-verified BOTH directions (emptying
`rt_contract` collapses the A8 kernel and fails its test). (c) `nocapture` auto-upgrades to
`captures(none)`, which inkwell 0.9 prints as `ptr none` → rt_contract textual pin updated;
`memory(argmem: read)` bitmask unchanged; the A8 gate (loop-invariant `hash64` hoists AND
vectorizes) alive on 22. Verification: `cargo test --workspace` **1878 green** (byte-for-byte the
M13-close baseline), clippy clean; `bench/binary_size` **byte-identical** to the 19 baseline (the
rustc-built runtime staticlib dominates; section rounding absorbs the codegen delta);
`bench/clang_ir_compare` all 5 kernels **MATCH** on 22/clang-22. Independent adversarial gate:
SHIP, all 8 implementation claims re-verified against real LLVM 22.1.8 incl. kill-mutations;
gemini zero findings. NEW recorded follow-up (roadmap): inkwell 0.9's `ptr none` shorthand is not
re-parseable by `llvm-as-22` — the textual `emit-llvm | llvm-as` dev path is broken (object
codegen and grep-based consumers unaffected); resolve at the M14 bitcode/ThinLTO boundary (emit
`captures(none)` or a `.bc` path). **M14 was RE-SCOPED the same day by a two-lens design review**
(roadmap "Post-upgrade wave": ThinLTO-across-Align-modules is moot today — one `Program` → one
module; the rustc-vs-alignc LLVM version wall DISSOLVED with the 22 jump — rustc 1.96 emits LLVM
22.1.2 bitcode, verified to merge+LTO cleanly, std does not leak into IR; the measured win surface
is per-element string/hash wrappers only, the numeric core is already saturated). **M14 Slice 1 =
the LTO ceiling probe** (wall-clock ≥ 1.15× on a str_eq/str_cmp/hash64 kernel or record-and-close
items 1+2, the #416 precedent); PGO keeps its place with written reasons. **Probe status: launched
2026-07-12 then STOPPED mid-run on session token budget — rerun next session** (procedure fully
specified in the roadmap M14 section; one practical hint from the aborted run: the probe pipeline
must NOT round-trip alignc IR through standalone `opt-22`/`llvm-as-22` — the known `ptr none`
textual-print issue rejects it; either `sed 's/ptr none/ptr captures(none)/'` the emitted text or
keep the Align side in-memory). **M15 separate
compilation (multi-module compilation units) is OWNER-MANDATED (2026-07-12)** — the owner ruled
whole-program-only must not remain; roadmap M15 section holds the design-question list
(unit boundary/artifact, cross-unit inference summaries, generics, M13 internalization +
capability-linking interactions, ThinLTO un-mooting, incremental driver); two-lens design review
FIRST, sequenced after the M14 probe verdict; the deferred ABI-flattening/fn-arg-attr/nsw items
still wait for the M14+ boundaries. **Also recorded 2026-07-12: the owner's Codex
binary-optimization audit** (out-of-repo, pre-#425 HEAD) triaged into `open-questions.md` →
"External binary-optimization audit (Codex, 2026-07-12) — adoption record" + a roadmap work-queue
note: wave 1 = 3 CONFIRMED bugs (bench export roots broken by the #418 internalization; ELF-only
linker/size tooling breaks macOS builds; profiles never reach the TargetMachine/`optsize`/runtime
variant), wave 2 = quick wins (O(n²) sort — 547× at 100k, tiny-`par_map` cold start, zero-size
arena, attr-kind fail-loud + `captures(none)` emission which likely fixes the llvm-as round-trip
follow-up), wave 3 = measure-first (JSON double-alloc, I/O zero-fill); rejected claims recorded
with reasons (the "nocapture dangerous" one is disproven by the A8 gate). Earlier: twelfth wave (**M13 COMPLETE — Slice V MERGED as #424**, the last
slice; the milestone is formally closed in the roadmap). **Upgrade-target decision (owner,
2026-07-11, end of session): LLVM 22 from apt.llvm.org** (`llvm-toolchain-trixie-22`; the owner
installs `llvm-22 llvm-22-dev clang-22` themselves) — NOT Debian backports' 21: inkwell 0.9
supports `llvm22-1(-prefer-dynamic)`, llvm-sys 221 exists, and landing on the newest before the
post-upgrade bitcode work (ThinLTO/runtime-bitcode) avoids a second version jump; Homebrew's
default `llvm` formula is already 22.1.8 so a Mac build needs nothing special (keg-only
`LLVM_SYS_221_PREFIX` only). **LLVM 19 stays installed** (rollback + shape-diff triage).
Upgrade to-do next session: `Cargo.toml` `llvm19-1` → `llvm22-1`, `.cargo/config.toml`
`LLVM_SYS_191_PREFER_DYNAMIC` → `LLVM_SYS_221_PREFER_DYNAMIC` (re-verify the shared-only
stance), point the clang-IR harness + `clang_ir_compare.rs` at clang-22 (same-LLVM requirement),
then run the whole M13 net (1878 tests + vectorize_shapes/target_cpu_isa/link_hygiene/
capability_linking + bench/binary_size + bench/clang_ir_compare) and re-measure per the roadmap
checkpoint. Slice V outcomes: (a) `--target-cpu`
empty-feature-string VERIFIED CORRECT (LLVM derives the ISA set from the CPU name; objdump: v2 →
SSE `paddq` zero `ymm`, v3/skylake → AVX2 `vpaddq`/`ymm`) and pinned by `target_cpu_isa.rs`
(3 tool-gated tests, gate-mutation-verified BOTH directions); (b) cold-edge `!prof` closed
**MOOT** — bounds/div/abort branches end in `unreachable` + the Slice-5 `noreturn` attrs already
make BranchProbabilityInfo treat them as cold (objdump: fail calls sunk past the hot `ret`); a
real `!prof` prototype produced byte-identical machine code and was reverted; correction
recorded: `?`-edges are NOT cold (an `Err` is an ordinary value); (c) the Clang-IR comparison
harness `bench/clang_ir_compare/` — **all 5 kernel pairs MATCH idiomatic C** through the same
LLVM 19 at both v2 and v3 (semantic signals: reduce intrinsics + `<N x>` widths, not block-name
strings which don't transfer cross-toolchain); divergences recorded as future leads (clang
interleaves ~11 vs ~4 vector ops/body; Align's `out dst` = no-memcheck for free where C needs
`restrict`). Gate SHIP; gemini's one high qualified-then-hardened (the shipped invocation shape
survives — errexit suppressed in the `$(facts ...)` substitution, full run exits 0 — but all
four greps now carry `|| true`). `cargo test --workspace` **1878 green** (1874 + 4), clippy
clean. **M13 total: 7 PRs #418–#424, tests 1813 → 1878, the shape/size/bench regression net is
built and green — it IS the LLVM-upgrade gate. Next: the LLVM/inkwell upgrade checkpoint**
(roadmap: inkwell `llvm19-1` → newest; re-verify the shared-only Debian linkage stance
(`LLVM_SYS_*_PREFER_DYNAMIC`); rerun the full net; re-measure Slice-V shapes; the deferred
follow-ups — ThinLTO → runtime bitcode → PGO → BOLT — go AFTER the upgrade, and the Slice-5
deferrals (ABI flattening, fn-arg attrs, nsw) wait for post-M14 ThinLTO boundaries). Earlier:
eleventh wave (**M13 Slice 5 MERGED as #423** — runtime-declare
contract attributes + regression net, after the slice was **RE-SCOPED by a two-lens design
review** the same day: the original flatten-ABI + per-arg-attribute plan was contradicted by
the whole-program reality — LLVM FunctionAttrs already infers program-fn attrs for the
non-inlined survivors (IR-verified on fib/parthunk), SROA already scalarizes the by-value
aggregates (hand-flattening measured no-op), so flattening / type-derived fn-arg attrs /
`AddProvenNoOverflow` are all DEFERRED to post-M14-ThinLTO with written reasons in the roadmap.
What shipped: **5A** — the curated `rt_contract` table on the opaque `align_rt_*` declares (the
one non-inline-redundant lever): `memory(argmem: read)` + `willreturn nofree nosync` +
`nocapture readonly` on `hash64/128` + the str-compare family; memory WITHHELD on
`utf8_valid`/`str_contains/find/rfind` (SIMD feature-detect cache = non-argument memory;
`nosync` kept — memchr 2.8.2 is Relaxed-only, gate-verified); `noreturn` on the 6 abort decls;
fail-safe default = nothing. **The A8 gate came in ABOVE-gate**: a loop-invariant `hash64` call
was per-iteration and blocked vectorization; attributed, it LICM-hoists to the pre-header AND
the loop vectorizes to `<4 x i64>` — pinned by an IR-shape test that fails if the table is
emptied (mutation-proven). **5B** — alloca-in-entry audit, bool/tag storage pin (SSA+stack `i1`;
Result/Option tag `i8`, user sum tag `i32`), ~3 canonical-loop-skeleton assertions folded into
`vectorize_shapes`. nsw scratch probe recorded below-gate (zero shape change — kernels already
vectorize). Gate SHIP (attribute soundness re-verified entry-by-entry: `safe_slice` never aborts
→ `willreturn` sound); gemini 2 mediums applied (no-alloc name check; a REAL `fn_body`
test-helper bug — first-occurrence match could extract the wrong function). `cargo test
--workspace` **1874 green** (1868 + 6), clippy clean. **Next: M13 Slice V** (verification
bundle: (a) `BuildTarget::Cpu(name)` empty-feature-string objdump check, (b) cold-edge `!prof`
weights — MEASURE first, A8 gate, (c) the Clang-IR comparison harness for 3–5 kernels) — the
LAST M13 slice; then M13 closes and the LLVM/inkwell upgrade checkpoint follows. Earlier: tenth
wave (**M13 Slice 4 MERGED as #422** — build profiles +
`alignc size`). `--profile dev|release|fast|small|tiny` → STOCK `default<O0|O2|O3|Os|Oz>`
(one `Profile` enum owns `pipeline()` + `strip()`; default = `release` = today's O2, the no-flag
path proven bit-for-bit unchanged by the gate); gc-sections/as-needed all profiles; `--strip-all`
only small/tiny (speed profiles keep symbols); `explain-opt`/`emit-llvm --stage optimized` stay
PINNED at `default<O2>` (diagnostic lenses, not builds — keeps the 3a gate path exact). New
`alignc size <file.align> [--profile p]`: builds then reports total/per-section/top-10-symbols/
relocs/DT_NEEDED via binutils (`LC_ALL=C`-pinned, failure-degrades-to-note, transient exe removed).
Numbers: hello 4.27 MB → **324 KB** small/tiny (strip dominates; O-level negligible on
runtime-dominated programs — the unstripped bulk is the runtime staticlib's symbol/debug info,
ties into the recorded runtime-split follow-up). Gate SHIP (default-path invariance bit-for-bit;
strip keeps DT_NEEDED — a stripped crypto program runs; size.rs probed under ja_JP locale/missing
binutils/compile failure; all 4 mutations caught). gemini: 2 applied (transient-exe cleanup, BSD
stat fallback), 1 REJECTED with written reason (`is_multiple_of` MSRV — toolchain pinned 1.96 and
the suggested modulo form would trip clippy's `manual_is_multiple_of` at `-D warnings`).
`cargo test --workspace` **1868 green** (1858 + 10), clippy clean. **Next: M13 Slice 5**
(internal ABI + argument attributes — the big one, may split; NOTE the 3a k7 finding re-scoped
its `noalias` motivation to cross-function/opaque-provenance cases) **then Slice V**
(verification bundle: Cpu-feature objdump check, measured cold `!prof` weights, Clang-IR
comparison harness) — those two close M13, and the LLVM/inkwell upgrade checkpoint follows.
Earlier: ninth wave (**M13 Slice 3b MERGED as #421 — Slice 3 COMPLETE**:
`alignc explain-opt` ships). Per-block `stmt_lines` MIR plumbing (populated only by
`lower_program_located`; single push site keeps the parallel invariant by construction) + opt-in
inkwell DI emission (`"Debug Info Version"` module flag stamped MANUALLY — the design doc's
inkwell claim was wrong, recorded) + Mechanism-A remarks capture (`-pass-remarks*` cl::opts
behind `Once` strictly inside explain-opt; `LLVMContextSetDiagnosticHandler`; RAII detach guard
so unwind can't leave a dangling handler) + the new verb `explain-opt <file> [--verbose]`
(missed/actionable one-liners in the diagnostic voice + one-line success summary + bucket count;
exit 0 regardless of miss count; `Vec<OptRecord>` built first, rendered second — json/score/CI
gates stay pure extensions). Translation keyed on REAL captured LLVM-19 strings: loop-vectorize
passed+missed (reason codes incl. the new honest `FpReorder`), slp/inline passed → summary;
inline misses → bucket (no lambda-inline-miss string exists — every pipeline lambda inlines;
actionable-inline deferred). Honesty rule enforced; `<unknown>`/line-0 never renders as a user
diagnostic; `align_rt_*` remarks → library/runtime bucket. Adversarial gate SHIP zero confirmed
defects (byte-identity vs main proven across 10 programs raw/optimized/obj — normal builds carry
zero debug metadata; handler lifetime traced incl. probed double-invocation; anchoring honesty
probed live); its RAII-guard note applied pre-merge. gemini zero findings. `cargo test
--workspace` **1858 green** (1844 + 14), clippy clean. **Next: M13 Slice 4** (build profiles —
`--profile dev/release/fast/small/tiny` → stock `default<O*>` + per-profile linker flags +
`alignc size`), then Slice 5 (internal ABI + argument attributes — NOTE its `noalias` motivation
was re-scoped by the 3a k7 finding) and Slice V. Earlier: eighth wave (**M13 Slice 3 design
SETTLED + Slice 3a MERGED as #420**).
The Slice 3 design was settled by a two-lens review and recorded in the new
**`docs/impl/09-explain-opt.md`** (the implementation source of truth for 3a/3b): split into
3a (optimized-IR emission + the vectorization IR-shape suite = the LLVM-upgrade gate) and 3b
(debug-loc anchoring + remarks capture + `alignc explain-opt`). Key settled facts: codegen emits
ZERO DILocations and MIR is span-free (remarks anchor `<unknown>:0:0` today → 3b needs per-block
`stmt_lines` plumbing + opt-in DI emission); the C API has NO structured-remark path (diagnostic
handler = flat `file:line:col: message` strings; C++ shim deferred with record) → v1 translation
keys on LLVM-19 message patterns; explain-opt = a new verb (report, not build; exit 0 regardless
of miss count), honesty rule (never upgrade a cost-model decline into a cause), internal
locations suppressed never fabricated, build `Vec<OptRecord>` first render second (JSON/score/CI
gates stay pure extensions); One-way boundary vs the M8 frequency lints (knowable from Align IR →
lint; needs the LLVM verdict → explain-opt). **Slice 3a then shipped (#420):** shared
`run_opt_pipeline`, `emit-llvm --stage raw|optimized` (default raw, byte-identical), and
`vectorize_shapes.rs` (12 tests, 8 kernels pinned at `x86-64-v3`, presence+absence asserts,
2 mutation tests) — map+sum/where+sum/where+min/reduce-mul/`map_into`/`.to_array()` vectorize;
`scan` + ordered-FP-sum are negative controls. **k7 empirical surprise recorded:** `map_into`
already vectorizes with zero `vector.memcheck` → Slice 5's fn-level `noalias` motivation
re-scopes to cross-function/opaque-provenance cases (mechanism vs inlined-alloca provenance not
isolated; honestly documented). Gate SHIP zero blocking defects (raw path proven byte-identical,
suite teeth mutation-verified); its 2 notes applied (per-test temp filenames — a real
remove-before-read race; honest k7 wording). gemini's 2 mediums verified and applied (one
`TargetMachine` per compile — also removed the pre-existing double-creation on the object path;
RAII test temp files). `cargo test --workspace` **1844 green** (1829 + 15), clippy clean.
**Next: M13 Slice 3b** (debug-loc + remarks + explain-opt, per `09-explain-opt.md`) or Slice 4
(build profiles) — 3b is the natural continuation; Slices 4/5/V remain. Earlier: seventh wave
(**M13 Slice 2 MERGED as #419** — capability-based linking
+ link hygiene). The unconditional `-lz -lzstd -lcrypto -lssl` link is GONE: MIR now collects an
`align_mir::Capability` (Zlib/Zstd/Crypto/Tls) from the builtin `Rvalue`s a program uses
(`rvalue_capability`, collection point = MIR, appended to `Program.link_libs` in `lower_program`),
so the driver links only what's used; it also passes `-Wl,--gc-sections -Wl,--as-needed`
unconditionally and keeps `-lpthread -ldl -lm` always (Rust-std support). Runtime NOT split (verified
one crate → one archive member; `--gc-sections` over Rust's per-function sections is what isolates a
feature, not member granularity). Capability collection + gc-sections are COUPLED (a GNU-ld quirk
retains the member's other C-lib refs once one lib resolves → `Capability::link_libs` is a monotonic
superset; always correct, `--as-needed` drops truly-unused libs). Both completion conditions met:
`capability_linking.rs` (10 tests, `readelf`-checked — the gate review added a binary-level
crypto-superset pin + a gzip libzstd-absence assert) proves `fn main()->i32=0` AND `hello` link none
of z/zstd/crypto/ssl while `gzip` keeps only `libz`; `bench/binary_size/` records before/after
(`hello` −22.6 %: 5.52 MB/4 gated deps → 4.27 MB/0; fail-loud on build errors). Adversarial gate
SHIP zero defects (fail-closed proven by mutation — unmapping Compress fails `m11_compress` at
link; http server/net/SSE proven to reach no SSL/EVP symbol — the TLS server code is cfg(test)).
gemini's link-order "high" empirically disproven (shared libs resolve their own deps; a
crypto-before-http program links fine) but its canonical dependent-first order (ssl → crypto →
zstd → z) applied for determinism + static-archive robustness. `cargo test --workspace` **1829
green** (1819 + 10), clippy clean. Deferred: fine-grained crypto/tls isolation → needs a runtime
feature-split (`open-questions.md` Open → "Runtime staticlib feature-split"). **Next: M13 Slice 3**
(optimized-IR emission + LLVM-remarks→Align translation, the vectorization IR-shape suite —
per the roadmap M13 section; then Slices 4/5/V). Earlier: sixth wave
(**M13 Slice 1 MERGED as #418**, symbol
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
MERGED (the temporary gate fixed a `mut` binding's region at initialization — region-changing
owned reassignment was a sema error and view regions intersected; #463 later replaced the owned
restriction with path-local drop flags; #404's intersect-everything fix traded the UB for an
unbounded leak and was rejected). `cargo test --workspace` **1687 green**, clippy clean. New Open
items recorded at that point: wrapper-hidden local-slice escape through a fn return (`return
Ok(xs[..])`, later fixed by #459), and the if-expression mixed-region leak (later fixed by #463).
Root stray files
triaged + deleted. Earlier same day: **#402** the `loop` expression MERGED (full slice, lexer→MIR);
**#401** runway A2 binary decode/encode; **#400** lexer escape set (other session); **#399** runway
A1 `fs.read_bytes_view`; **#398** std.http Slice 3 keepalive pool + R6 bench; **#396**/**#397**
owed-delta wave; staleness sweep #395)._

## ▶ NEXT SESSION — start here

**Repo baseline for the audits (2026-07-13, macOS arm64 machine):** **#430** plus the spawn-capture
lifetime fix described in the _Last updated_ paragraph above. The cache-first and parallel records
are respectively unimplemented and correctness-complete/performance-open; the
pipeline/closure/memory/I/O/SIMD record is partially implemented.
**This machine needs two env vars for every
`cargo build`/`cargo test`/driver-link run** — this was the undocumented blocker at session
start: `LLVM_SYS_221_PREFIX=/opt/homebrew/opt/llvm` (Homebrew LLVM 22.1.8 is keg-only) and
`LIBRARY_PATH=/opt/homebrew/lib:/opt/homebrew/opt/openssl@3/lib` (zstd + keg-only openssl@3 sit
outside cc's default search path; the same applies to running Align programs that link gated
libs — the driver now prints this hint on such link failures). On this Mac the driver suites are
green (align_attr 8/8, capability_linking 10/10 executed-not-skipped, build_profiles 9/9,
macho_link 2/2, m11_process 35/35) and `alignc build`/`size` work natively on Mach-O; the
remaining macOS full-suite failures are cataloged out-of-scope at adoption-record item 2
(ffi_byval SysV-Linux-by-design, std.net/TLS/cloexec runtime items, APFS non-UTF-8 setup,
expr_depth test-thread stack — all reproduce identically on pre-#426 main). The Linux flag path
is pinned unchanged by construction; the owed Linux full-suite re-verification is **DONE
2026-07-13 (Linux/WSL2, apt.llvm.org LLVM 22.1.8 dynamic, rustc 1.96, post-#436 main):
`cargo test --workspace` = 1907 passed / 0 failed / 1 ignored** (the by-design `#[ignore]`
manual probe `utf8_validate_throughput`), **`cargo clippy --workspace --all-targets --
-D warnings` clean** — the #427–#436 audit+fix wave (authored outside Claude Code; those PRs
carry no Claude-Code marker) added ~22 tests over the #426 baseline and is fully green on
Linux. **Next, pick one** (Codex waves 1+2, the `bench/binary_size` port, the M14 LTO probe, M14
Slice 2 `--rt-lto` #443, AND the M15 design review are all DONE — see the _Last updated_
paragraph): (a) **M15 implementation, S2b next** (S0 #444, S1a #445, #446, S1b #447, fn-value
#448, S2-first-stage #449 all SHIPPED; S2b = flip the default build to per-unit + migrate
`size`/`explain-opt`/`emit-llvm`/`emit-obj --export` + remove the whole-program-only path per
the hard-cutover settlement — slice plan + records in the roadmap M15 section; small pre-step
queued: the Unit-main nondeterministic-exit-code fix in `open-questions.md`); (b) **per-target-cpu runtime variant + cache key** — the `hash64` native-tuning
lever parked on M14 Slice 2 (roadmap M14 section + doc-10 §2 key spec); (c) cache-first C0
continuation — stabilize independent
constant diagnostics and add byte-reproducibility gates before whole-program CAS (source of truth
`docs/impl/10-cache-first-optimization.md`; MUST precede caching failed results); (d)
parallel P1 — replace the per-element runtime thunk with the recorded whole-range kernel, then add
the read-only capture context (source of truth `docs/impl/11-parallel-execution-optimization.md`);
(e) Codex wave-3 measure-first (JSON decode
double-allocation, I/O buffer zero-fill — correctness tests first, throughput-gated). Reminder:
gemini-code-assist reviews cease **2026-07-17**
— after that, `/code-review` on the branch replaces the reflect-before-merge step.

**Earlier repo state (re-verified 2026-07-10):** `main` clean, no open PRs. Newest commit is the
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
`print` (deliberate), the then-proposed escape dataflow refactor (later corrected to checked HIR and
completed through #461–#464) + purity-as-effect-bit (completed as #465), relative pointers
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
test` ≈ 990+ green. The audit's escape-dataflow structural refactor later shipped at the corrected
checked-HIR boundary through #461–#464 and purity-as-effect-bit shipped as #465; the remaining
structural record is the value-carrying-control-flow matrix in `docs/open-questions.md`. See memory
`fuzzing-infrastructure.md`, `audit-2026-07-02-fixes.md`, `m8-unsafe-raw-started.md`.

## Setup on the new machine

```bash
git clone https://github.com/sanohiro/align            # ideally into /home/<user>/prj/align
cd align
# Toolchain: Rust 1.96 + LLVM 22 (inkwell llvm22-1). apt.llvm.org: apt install llvm-22 llvm-22-dev clang-22
# .cargo/config.toml already sets LLVM_SYS_221_PREFER_DYNAMIC=1 (kept dynamic; static would now work too).
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
