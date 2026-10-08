# Admit fixed read windows in streaming examples

Status: the original best-effort-window safeguard below is superseded by
[plan148](148-fallible-example-read-windows.md). The examples now use plan135's
fallible constructor and propagate its original error before reading; their
source fixtures inject a typed constructor Err instead of a degraded success.
The original defect and qualification record remain historical evidence.

The existing best-effort buffer constructor can publish capacity zero when its
payload reservation fails. A zero-capacity reader read returns zero without
I/O; HTTP reads instead abort before the stream ABI. The file-copy and SHA-256
examples therefore mistake an unavailable window for EOF, while HTTP fetch
cannot report an ordinary error. SSE watch already checks its requested window.

Use the existing capacity query immediately after each fixed 65536-byte buffer
constructor in file_copy, file_sha256 and http_fetch. A different capacity is
Error.Invalid before the first read, write or digest update. Keep existing
argument, file-open, destination-create and HTTP-status admission order. Copy's
fresh destination can already exist and remains empty on this error; hash's
reader/digest and fetch's response may already exist and drop normally. No
digest or successful copy result is published. Empty inputs also require the
window. This does not make every allocation fallible or change any library API.

| Closure axis | Implementation and owner |
| --- | --- |
| Unavailable window | Existing file_copy_examples, m11_crypto_stream and http_read_stream fixtures compile each actual example with its sole fixed buffer constructor replaced by buffer(0), independently of host memory pressure. Empty and nonempty inputs require normal Invalid with exact stderr and no successful output. The source substitution is asserted unique. |
| Input/output integrity | Copy retains input and leaves only its already-created empty destination. Hash covers file/stdin without a digest. Fetch covers successful final heads without binary output. Existing normal binary, multi-window, limit, protocol and output-failure owners rerun unchanged. |
| Admission and lifetime | Add one scalar query/check after construction, before the loop. CLI/help, source/destination admission and HTTP request/status behavior remain. No views exist at the check; existing Move/Drop and Result propagation own all exits. No runtime, FFI, IR, interface, cache or compiler strategy changes. |
| Discrimination | Preserve baseline failures before adding source checks. The same degraded-window owners must pass afterward; restoring missing checks must reproduce false success or abort. Existing successful whole/per-unit and native-source owners retain their scope. |
| Source agreement | This ledger, plans97/100/110 and the English/Japanese chapter02/13/18 read-loop examples describe capacity refusal. Copy's already-created empty destination is retained. Syntax-check the edited guide snippets. No language or library specification changes. |

One capability closes the same constructor-admission defect across all fixed
read-window examples. Reuse existing guarded fixtures; no new test binary,
shared harness, allocation hook or memory-exhaustion test is needed. Run the
three owners on Linux/macOS, then one fresh full-diff inspection and final-SHA
preflight. No new performance/resource improvement is claimed.

The baseline fails all three degraded-window owners: copy/hash report success
for unread nonempty input, and HTTP exits without the normal Invalid report.
With the checks, all 24 tests in the three owner targets pass on Linux ARM64
and macOS ARM64. All six edited guide snippets syntax-check.
