//! `alignc size <file.align> [--profile p]` — the size report (M13 Slice 4).
//!
//! Input decision (documented at the source of truth `docs/impl/07-roadmap.md` M13 Slice 4): the
//! argument is an **Align source file**, not a pre-built binary. `size` builds it with the requested
//! `--profile` (so `alignc size app.align --profile tiny` measures exactly what `alignc build
//! --profile tiny` would produce — the report tracks the profile that made it) and reports on that
//! artifact. This keeps `size` a first-class build-adjacent verb rather than a generic ELF inspector.
//!
//! Implementation decision: we shell out to the system **binutils** (`readelf` / `nm`), already
//! implicit toolchain dependencies of every `alignc build` (they ship with the `cc`/`ld` used to
//! link). No new crate dependency is taken (the workspace stays lean — `align_codegen_llvm` is the
//! only heavy dep). Every tool invocation is failure-tolerant: a missing/erroring tool degrades that
//! one section of the report to a note, never a panic.

use std::path::Path;
use std::process::{Command, ExitCode};

use align_driver::{BuildTarget, Profile};

/// How many largest symbols to list.
const TOP_SYMBOLS: usize = 10;

/// `alignc size <file>`: build with `profile`, then print the size breakdown of the executable.
pub fn run_size(path: &str, target: BuildTarget, profile: Profile) -> ExitCode {
    let Some(mir) = crate::front_to_mir(path) else {
        return ExitCode::FAILURE;
    };
    // Build into a private temp path (like `run`), never the cwd — `size` is a report, not `build`.
    let exe = std::env::temp_dir().join(format!("align-size-{}", crate::stem(path)));
    if let Err(code) = crate::build_to(path, &mir, &exe, target, profile) {
        return code;
    }
    match report(&exe, profile) {
        Ok(text) => {
            print!("{text}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("alignc: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Build the human-readable size report for `exe`. Only a failure to `stat` the file itself is fatal
/// (the build must have produced it); each binutils-derived section is best-effort.
fn report(exe: &Path, profile: Profile) -> Result<String, String> {
    use std::fmt::Write;
    let total = std::fs::metadata(exe)
        .map(|m| m.len())
        .map_err(|e| format!("cannot stat built executable {}: {e}", exe.display()))?;

    let mut out = String::new();
    let _ = writeln!(out, "size report: {}", exe.display());
    let _ = writeln!(out, "  profile:    {} ({})", profile.name(), profile.pipeline());
    let _ = writeln!(out, "  total size: {} bytes", group_digits(total));

    // Per-section sizes (readelf -SW), largest first.
    let _ = writeln!(out, "\nsections (largest first):");
    match sections(exe) {
        Some(mut secs) if !secs.is_empty() => {
            secs.sort_by_key(|s| std::cmp::Reverse(s.size));
            for s in &secs {
                let _ = writeln!(out, "  {:<20} {:>12}  {}", s.name, group_digits(s.size), s.kind);
            }
        }
        _ => {
            let _ = writeln!(out, "  (unavailable: `readelf -S` produced no parseable sections)");
        }
    }

    // Largest symbols (nm --size-sort). Stripped binaries have no .symtab → a note, not an error.
    let _ = writeln!(out, "\nlargest symbols (top {TOP_SYMBOLS}):");
    match symbols(exe) {
        Some(syms) if !syms.is_empty() => {
            for s in syms.iter().take(TOP_SYMBOLS) {
                let _ = writeln!(out, "  {:>12}  {} {}", group_digits(s.size), s.kind, s.name);
            }
        }
        _ => {
            let _ = writeln!(out, "  (none: the symbol table is absent — a stripped binary, or `nm` found no symbols)");
        }
    }

    // Relocations + dynamic dependencies (readelf -r / -d).
    let _ = writeln!(out, "\nrelocations: {}", reloc_count(exe).map_or_else(|| "(unavailable)".to_string(), group_digits));

    let _ = writeln!(out, "\ndynamic dependencies (DT_NEEDED):");
    match dynamic_deps(exe) {
        Some(deps) if !deps.is_empty() => {
            for d in &deps {
                let _ = writeln!(out, "  {d}");
            }
        }
        Some(_) => {
            let _ = writeln!(out, "  (none — statically resolved)");
        }
        None => {
            let _ = writeln!(out, "  (unavailable: `readelf -d` did not run)");
        }
    }

    Ok(out)
}

/// One ELF section's name / type / byte size.
struct Section {
    name: String,
    kind: String,
    size: u64,
}

/// Parse `readelf -SW <exe>` into sections. `None` if `readelf` cannot run at all. Robust to the
/// header/`[Nr]`/NULL lines: each real section line has, after the `]` closing the section number,
/// the fields `Name Type Address Off Size …` — we read the 1st (name), 2nd (type) and 5th (size,
/// hex). Non-conforming lines (headers, the size-0 NULL section) fail the hex parse and are skipped.
fn sections(exe: &Path) -> Option<Vec<Section>> {
    let text = run_tool("readelf", &["-SW", &exe.to_string_lossy()])?;
    let mut secs = Vec::new();
    for line in text.lines() {
        // The section number is bracketed (`[ 1]`); the section body follows the first `]`.
        let Some((_, body)) = line.split_once(']') else { continue };
        let toks: Vec<&str> = body.split_whitespace().collect();
        if toks.len() < 5 {
            continue;
        }
        // toks: [Name, Type, Address, Off, Size, ...]
        let Ok(size) = u64::from_str_radix(toks[4], 16) else { continue };
        if size == 0 {
            continue; // NULL / empty sections carry no size signal
        }
        secs.push(Section { name: toks[0].to_string(), kind: toks[1].to_string(), size });
    }
    Some(secs)
}

/// One symbol's decimal byte size / type letter / name.
struct Symbol {
    size: u64,
    kind: String,
    name: String,
}

/// Parse `nm --print-size --size-sort --radix=d <exe>` into symbols, **largest first**. `nm` sorts
/// ascending by size, so we reverse. `None`/empty when the binary is stripped (no `.symtab`) — `nm`
/// then exits nonzero with "no symbols", which we treat as "no symbol data", not an error.
fn symbols(exe: &Path) -> Option<Vec<Symbol>> {
    let text = run_tool("nm", &["--print-size", "--size-sort", "--radix=d", &exe.to_string_lossy()])?;
    let mut syms = Vec::new();
    for line in text.lines() {
        // Format: "<addr> <size> <type> <name>". Symbols without a size are omitted by --print-size.
        let toks: Vec<&str> = line.split_whitespace().collect();
        if toks.len() < 4 {
            continue;
        }
        let Ok(size) = toks[1].parse::<u64>() else { continue };
        // The name may itself contain spaces only in exotic C++ manglings; join the tail to be safe.
        let name = toks[3..].join(" ");
        syms.push(Symbol { size, kind: toks[2].to_string(), name });
    }
    syms.reverse();
    Some(syms)
}

/// Total relocation entries across all relocation sections (`readelf -rW`). Sums the "contains N
/// entries" counts readelf prints per section. `None` if `readelf` cannot run; `Some(0)` for a file
/// with no relocations.
fn reloc_count(exe: &Path) -> Option<u64> {
    let text = run_tool("readelf", &["-rW", &exe.to_string_lossy()])?;
    let mut total: u64 = 0;
    for line in text.lines() {
        // "Relocation section '.rela.dyn' at offset 0x… contains 12 entries:"
        if let Some(rest) = line.split("contains").nth(1)
            && let Some(n) = rest.split_whitespace().next().and_then(|t| t.parse::<u64>().ok())
        {
            total += n;
        }
    }
    Some(total)
}

/// The `DT_NEEDED` shared-library names (`readelf -dW`). `None` if `readelf` cannot run; `Some(vec![])`
/// for a binary with no dynamic dependencies.
fn dynamic_deps(exe: &Path) -> Option<Vec<String>> {
    let text = run_tool("readelf", &["-dW", &exe.to_string_lossy()])?;
    let mut deps = Vec::new();
    for line in text.lines() {
        if line.contains("(NEEDED)") {
            // "… Shared library: [libc.so.6]"
            if let Some(name) = line.split_once('[').and_then(|(_, r)| r.split_once(']')).map(|(n, _)| n) {
                deps.push(name.to_string());
            }
        }
    }
    Some(deps)
}

/// Run a binutils tool and return its stdout as a string. `None` when the tool cannot be launched
/// (not installed) — the caller degrades that section to a note. A nonzero exit that still produced
/// stdout (e.g. `nm` on a stripped file) is returned as-is; a nonzero exit with no stdout is `None`.
fn run_tool(tool: &str, args: &[&str]) -> Option<String> {
    let output = Command::new(tool).args(args).output().ok()?;
    let text = String::from_utf8_lossy(&output.stdout).into_owned();
    if text.trim().is_empty() && !output.status.success() {
        return None;
    }
    Some(text)
}

/// Group a byte count with thousands separators for readability (`5519840` → `5,519,840`). No `std`
/// locale dependency — a plain ASCII grouping.
fn group_digits(n: u64) -> String {
    let s = n.to_string();
    let bytes = s.as_bytes();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, &b) in bytes.iter().enumerate() {
        if i > 0 && (bytes.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(b as char);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::group_digits;

    #[test]
    fn group_digits_inserts_thousands_separators() {
        assert_eq!(group_digits(0), "0");
        assert_eq!(group_digits(42), "42");
        assert_eq!(group_digits(999), "999");
        assert_eq!(group_digits(1000), "1,000");
        assert_eq!(group_digits(5_519_840), "5,519,840");
        assert_eq!(group_digits(1_000_000), "1,000,000");
    }
}
