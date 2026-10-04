# std.os

**Implemented memory observation:**
The two nullary RAM queries below follow the independently reviewed plan 101.

`os.physical_memory() -> Result<i64, Error>` and
`os.available_memory() -> Result<i64, Error>` are Impure nullary observations
requiring `import std.os`. Success reports bytes: total is positive; available
is nonnegative and bounded by the total queried in that operation. Values are
ordinary Copy results; no memory owner, borrowed view or allocation is returned.

Linux/WSL2 reads at most 8192 bytes of `/proc/meminfo`: MemTotal supplies kernel-usable RAM
and MemAvailable supplies its estimate. These are the current kernel/VM view,
without cgroup or address-space-limit adjustment. macOS uses hw.memsize for total
and the explicit `(free_count + inactive_count) * host_page_size` advisory estimate
for available bytes. Speculative pages are already included in free_count.
No fallback, subprocess, environment setting or observation cache exists.
Unsupported, malformed or unrepresentable observations return Invalid; native
POSIX errors use the existing mapping. Native scratch is fixed; successful
results allocate no Align/Rust heap storage.

These observations neither reserve memory nor guarantee a later allocation.
They do not report VRAM or per-model usage. Calls are independent and do not
certify an atomic snapshot. Applications retain allocation checks and explicit
caps; container limits can be lower than the observed total. The exact native
query/cleanup order, bounds, ABI and owners are in
[OS memory observation](../101-os-memory-observation-plan.md).

**Implemented identity observation:**
`os.identity() -> Result<os.identity_info, Error>` observes current real UID/GID,
in that order, through native getuid/getgid. It is Impure, requires std.os,
allocates nothing and changes no credentials. The qualified ordinary Copy record
has exactly `real_uid: i64`, `real_gid: i64`, both nonnegative on success.
Checked-conversion/unsupported-platform failure is Invalid; the pair is not an
atomic or authenticated identity. [Plan 54](../54-r69-r76-prerequisite-batch-plan.md)
owns its exact ABI and verification. The implemented host surface follows.

`os.host() -> Result<os.host_info, Error>` requires `import std.os` and is Impure.
The qualified-only ordinary Move record has declaration-order fields
`system: string`, `release: string`, `machine: string`, `cpu: Option<string>`,
and `logical_cpu_count: Option<i64>`. User construction, partial moves and recursive
Drop use ordinary record rules. Each successful call owns its strings independently,
including inside an arena; no observation buffer is retained.

On supported 64-bit Linux/macOS targets, system/release/machine are exact uname
sysname/release/machine strings. Validate them in that order, requiring a nonempty,
NUL-terminated native field and strict UTF-8, before querying the optional count
or allocating text. Missing terminators, empty text and invalid UTF-8 return
`Error.Invalid`; uname failures use the existing errno mapping. Native padding
past the first NUL is ignored. `cpu` is always None in this capability. A positive,
i64-representable `sysconf(_SC_NPROCESSORS_ONLN)` result becomes Some(count);
unavailable, nonpositive or unrepresentable counts become None without discarding
successful mandatory fields. Unsupported targets return `Error.Invalid`.

There are no arguments, defaults, environment reads, subprocesses, distribution-file
parsing, CPU-model inference, caches or global-state changes. OOM retains the normal
hard-error policy. The observation is not an authenticated identity or an atomic
snapshot, and its online CPU count makes no physical-machine, affinity or quota
promise. The exact contract and closure owners are in
[host observation](../42-host-observation-plan.md).
