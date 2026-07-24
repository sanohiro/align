# Align v0.4.0 Release Notes

The headline of v0.4.0 is a **complete standard-library round of work driven by a real client** — `align-llm`, a local LLM coding system built against Align. Three filed requests ship together: capturing a child process's output, bounding network I/O with timeouts, and decoding JSON string arrays. 6 merged changes since v0.3.0.

## `std.process` — captured output + cwd / env / timeout

The old `process.spawn` inherited the parent's file descriptors, so a child's `stdout`/`stderr` went straight to the terminal. That is fixed. A new builder captures a command's output, working directory, environment, and a run deadline — the core of any build/test/lint loop.

```align
import std.process

c := process.command("go", ["go", "test", "./..."])
c.cwd(repo_dir)
c.env("NODE_ENV", "test")
c.timeout_ns(30_000_000_000)          // 30 s — the child's process group is killed on overrun
out := c.run()?
match out.code() { 0 => parse(out.stdout()), _ => report(out.stderr()) }
```

- `process.command(cmd, args)` is a Move builder; `c.cwd(dir)` / `c.env(name, value)` / `c.env_clear()` / `c.timeout_ns(ns)` are in-place setters (the `http.request` idiom — bound-local, `()`-returning).
- `c.run()` forks a child with **both** streams captured, drains both pipes concurrently to EOF (no two-pipe deadlock), reaps the child, and yields a `run_output` handle: `out.code()` is the exit code, `out.stdout()` / `out.stderr()` are zero-copy `str` views into the owned capture (region-bound to `out`; `.clone()` to persist past its scope). Captured output is UTF-8-validated (invalid → `Error.Invalid`, the `fs.read_file` rule).
- A `timeout_ns` overrun `SIGKILL`s the child's whole process group (a `sleep` under `sh -c` included), reaps it, and returns `Err(Error.Timeout)` — bounding wall-clock time, distinct from a nonzero exit or a transport error.

## `Error.Timeout` — a new builtin error category

Bounding an operation needs a way for the caller to tell "it timed out" apart from "it failed" and "it exited nonzero". The builtin `Error` sum type gains a fifth variant, inserted between `Denied` and `Code`:

```align
Error { NotFound, Invalid, Denied, Timeout, Code(i32) }
```

`Timeout` is produced **only** where a deadline is enforced explicitly (a `std.process` run's `timeout_ns`; the `std.net`/`std.http` I/O timeouts below) — an unrelated `ETIMEDOUT` errno still maps to `Error.Code`. At `main` it exits with code 4.

## `std.net` + `std.http` — I/O timeouts

A hung or black-holed peer could stall a loop forever. Network I/O is now bounded, for both plaintext and HTTPS/TLS, with the timeout-unset path byte-identical to before.

```align
cl := http.client()
cl.timeout(10_000_000_000)              // 10 s default for every request on this client
r := http.request("POST", url)
r.timeout(30_000_000_000)               // per-request override
resp := cl.request(r)?                  // Err(Error.Timeout) if connect / send / receive overruns
```

- **net rail:** `tcp.connect` grows a connect-timeout substrate (non-blocking `connect` + `poll` deadline; `timeout_ns == 0` is the unchanged blocking path), and `conn.read_timeout_ns(ns)` / `conn.write_timeout_ns(ns)` arm `SO_RCVTIMEO`/`SO_SNDTIMEO` on the socket. Expiry surfaces as `Err(Error.Timeout)`.
- **http surface:** `http.client().timeout(ns)` (a client default) and `http.request(...).timeout(ns)` (a per-request override) resolve one effective per-op deadline, threaded through connect and armed on the socket for send/receive — including under TLS, where an `SSL_read`/`SSL_write` timeout maps to `Error.Timeout`. A pooled keepalive connection re-arms per request, so a stale deadline never leaks into a later call.

## `core.json` — `array<str>` struct fields

`json.decode` now accepts a struct field of type `array<str>` — the natural shape for an argv list, a `stop` list, or a tool-name list. Scalar-array fields (`array<i64>` / `array<f64>` / `array<bool>`) and `array<str>` *encode* already worked; this closes the last gap.

```align
Spec { id: str, argv: array<str>, code: i64 }
r: Spec := json.decode(task_json)?      // "argv":["git","status","--porcelain"] → owned array<str>
r.argv[2]                               // "--porcelain"
```

A decoded `array<str>` element is a zero-copy `str` view into the input (the same rule as a top-level `str` field): the owned array spine borrows the input, so the decoded struct is input-region-bound; `.clone()` an element to keep it past the input's lifetime.

## Documentation

The library design specs tracked the code: `docs/impl/std-design/process.md`, `http.md`, `net.md`, and `docs/impl/core-design/json.md` were updated (with their Japanese mirrors), and the `Error.Timeout` variant is recorded across `draft.md`, `docs/language-spec.md`, the tutorial guide, and `docs/open-questions.md`.

## Backward Compatibility Warning

**Align makes zero backward compatibility guarantees during the 0.x series.** As we iterate towards a stable 1.0, the language syntax, standard library APIs, and ABI may break without warning or legacy fallbacks.

v0.4.0 is additive except for one deliberate break: **`Error` gains a `Timeout` variant.** A `match` on `Error` that was exhaustive now needs a `Timeout` arm (or a wildcard). This is the only source change v0.4.0 requires of an existing program.

## Known Intentional Limitations

Carried over from v0.3.0 (unchanged): `extern "C"` export-of-body; Windows (Align targets Linux x86-64/aarch64 and macOS Apple Silicon); capturing escaping closures; no database drivers; no application state in handlers; JWT is HS256 only; multipart is not wired into the core web surface by design; and the `std.regex` limits (no look-around/backreferences, no `rx"..."` literal, no implicit cache).

New in v0.4.0:

- **`std.process` has no raw-bytes capture tier yet.** `run()` returns UTF-8-validated `str` output; a `run_bytes()` tier for arbitrary binary tool output is designed and deferred until a consumer needs it.
- **A `json.decode` `array<str>` element cannot contain a JSON escape.** A zero-copy `str` view can't unescape, so a `\`-bearing element decodes to `Error.Invalid` — the same pre-existing limitation as an escaped `str` field. Argv / tag / stop lists are unescaped in practice.
- **Top-level `array<str> := json.decode` is deferred.** A struct *field* rides its enclosing struct's input-region binding; a top-level array result would have to carry that region itself (the scalar top-level array is deliberately returnable), so it is a separate slice.
