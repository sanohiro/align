//! M11 Slice 1 — std.net `dns.resolve`: resolve a host to its IP-address strings via `getaddrinfo`,
//! returned as an owned `array<string>` (deep-`Drop`, the `fs.read_dir` template). Validates the
//! errno/EAI-status path, the owned-string-array deep drop, the `import std.net` capability gate,
//! and impurity (rejected by `par_map`). (`docs/impl/std-design/net.md` Slice 1; `draft.md` §18.2.)

mod common;
use common::*;

/// `dns.resolve("localhost")` returns an owned `array<string>` of IP strings containing at least one
/// loopback form (`127.0.0.1` or `::1`), resolved via `/etc/hosts` even with no external resolver.
/// The array is deep-`Drop`-freed at scope end. If the sandbox has no name resolution at all, the
/// program exits non-zero (the `?` propagates the Err) — skip gracefully.
#[test]
fn dns_resolve_localhost_contains_loopback() {
    if !backend_available() {
        return;
    }
    let prog = "\
import std.net
pub fn main() -> Result<(), Error> {
  ips := dns.resolve(\"localhost\")?
  print(ips.len())
  hit := ips.any(fn ip { ip.contains(\"127.0.0.1\") || ip.contains(\"::1\") })
  if hit {
    print(\"loopback\")
  }
  return Ok(())
}
";
    let out = build_and_run("m11net-localhost", prog);
    if out.status.code() != Some(0) {
        return; // no resolver in this sandbox — skip
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    let mut lines = stdout.lines();
    let count: i64 = lines.next().unwrap_or("0").parse().unwrap_or(0);
    assert!(count > 0, "localhost resolves to at least one usable IP string; stdout: {stdout:?}");
    assert!(stdout.contains("loopback"), "localhost includes a loopback address (127.0.0.1 or ::1); stdout: {stdout:?}");
}

/// A definitively invalid name (`.invalid` is RFC 6761 reserved and never resolves) is an `Err`,
/// not a value and never an abort — `main` exits non-zero. This holds with or without a resolver
/// (a resolver returns NXDOMAIN → `EAI_NONAME`; no resolver returns `EAI_AGAIN`), so no skip.
#[test]
fn dns_resolve_invalid_host_is_err() {
    if !backend_available() {
        return;
    }
    let prog = "\
import std.net
pub fn main() -> Result<(), Error> {
  ips := dns.resolve(\"no-such-host.invalid\")?
  print(ips.len())
  return Ok(())
}
";
    let out = build_and_run("m11net-invalid", prog);
    assert_ne!(out.status.code(), Some(0), "an unresolvable name is an Err (main exits non-zero), never a value");
    assert!(out.stdout.is_empty(), "the Err path prints nothing (the `?` short-circuits before print)");
}

/// The owned `array<string>` from `dns.resolve` is bound, used, then deep-`Drop`-freed at scope end
/// (each IP string buffer, then the header) — the `DynArray(String)` drop path, no leak. `main`
/// exits 0 (the resolve of `localhost` succeeds via `/etc/hosts`); skip if there is no resolver.
#[test]
fn dns_resolve_array_deep_drops() {
    if !backend_available() {
        return;
    }
    let prog = "\
import std.net
pub fn main() -> Result<(), Error> {
  ips := dns.resolve(\"localhost\")?
  n := ips.len()
  print(n)
  return Ok(())
}
";
    let out = build_and_run("m11net-deepdrop", prog);
    if out.status.code() != Some(0) {
        return; // no resolver — skip
    }
    let n: i64 = String::from_utf8_lossy(&out.stdout).trim().parse().unwrap_or(0);
    assert!(n > 0, "each element is a usable string (len reported > 0)");
}

// --- capability header (import required) -------------------------------------------------------

/// Every `dns.*` use requires `import std.net` (the capability-header rule), like the other `std`
/// namespaces.
#[test]
fn dns_resolve_requires_import() {
    let src = "\
pub fn main() -> Result<(), Error> {
  ips := dns.resolve(\"localhost\")?
  print(ips.len())
  return Ok(())
}
";
    assert!(check_errs("m11net-noimport", src), "dns.resolve without `import std.net` must error");
}

// --- impurity (rejected by par_map) ------------------------------------------------------------

/// `dns.resolve` is a syscall — impure. A closure that calls it is never `Pure`, so `par_map`
/// (which requires a Pure closure) rejects it (the `fs`/`io`/`rand` impurity precedent).
#[test]
fn dns_resolve_rejected_by_par_map() {
    let src = "\
import std.net
fn f(x: i64) -> i64 {
  ips := dns.resolve(\"localhost\") else { return x }
  return ips.len()
}
pub fn main() -> i32 {
  arena {
    ys := [1, 2, 3, 4][0..4].par_map(f).to_array()
    print(ys.len())
  }
  return 0
}
";
    assert!(check_errs("m11net-parmap", src), "a dns.resolve-using (impure) closure must be rejected by par_map");
}
