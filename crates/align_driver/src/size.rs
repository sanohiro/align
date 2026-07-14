//! `alignc size <file.align> [--profile p]` — the size report (M13 Slice 4).
//!
//! Input decision (documented at the source of truth `docs/impl/07-roadmap.md` M13 Slice 4): the
//! argument is an **Align source file**, not a pre-built binary. `size` builds it with the requested
//! `--profile` (so `alignc size app.align --profile tiny` measures exactly what `alignc build
//! --profile tiny` would produce — the report tracks the profile that made it) and reports on that
//! artifact. This keeps `size` a first-class build-adjacent verb rather than a generic object
//! inspector.
//!
//! Implementation decision: we shell out to **`llvm-readobj` / `llvm-nm`** — the tools shipped with
//! the very LLVM `alignc` requires to exist at all, i.e. a *stronger* implicit dependency than the
//! platform binutils (GNU `readelf`/`nm` do not exist on macOS, and Apple's `nm` speaks a different
//! dialect). They read both object formats the driver links (ELF and Mach-O), and
//! [`align_driver::llvm_tool`] finds the version matching the linked LLVM. No new crate dependency
//! is taken (the workspace stays lean — `align_codegen_llvm` is the only heavy dep). Every tool
//! invocation is failure-tolerant: a missing/erroring tool degrades that one section of the report
//! to a note, never a panic.

use std::path::Path;
use std::process::{Command, ExitCode};

use align_driver::{llvm_tool, target_object_format, BuildTarget, ObjectFormat, Profile};

/// How many largest symbols to list.
const TOP_SYMBOLS: usize = 10;

/// `alignc size <file>`: build with `profile`, then print the size breakdown of the executable.
pub fn run_size(path: &str, target: BuildTarget, profile: Profile, rt_lto: bool) -> ExitCode {
    // Keep the complete executable private through the report. Another `size` process cannot
    // replace or remove it while llvm-readobj/llvm-nm are inspecting it.
    let stage = match crate::ArtifactStage::temp("align-size") {
        Ok(stage) => stage,
        Err(e) => {
            eprintln!("alignc: cannot create size staging directory: {e}");
            return ExitCode::FAILURE;
        }
    };
    let exe = stage.path().join("program");
    // Build via the per-unit path (M15 S2b), then report on the single final executable.
    if let Err(code) = crate::build_per_unit_to(path, &exe, target, profile, rt_lto) {
        return code;
    }
    let res = report(&exe, profile);
    match res {
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

/// Build the human-readable size report for `exe`. Only a failure to `stat` the file itself (the
/// build must have produced it) or an unclassifiable target is fatal; each tool-derived section is
/// best-effort. The object format comes from [`target_object_format`], not from sniffing the file —
/// `size` measures the executable it just built for the build target.
fn report(exe: &Path, profile: Profile) -> Result<String, String> {
    use std::fmt::Write;
    let format = target_object_format()?;
    let total = std::fs::metadata(exe)
        .map(|m| m.len())
        .map_err(|e| format!("cannot stat built executable {}: {e}", exe.display()))?;
    let readobj = llvm_tool("llvm-readobj");
    let nm = llvm_tool("llvm-nm");

    let mut out = String::new();
    let _ = writeln!(out, "size report: {}", exe.display());
    let _ = writeln!(out, "  profile:    {} ({})", profile.name(), profile.pipeline());
    let _ = writeln!(out, "  total size: {} bytes", group_digits(total));

    // Per-section sizes (llvm-readobj --sections), largest first. The section table is also the
    // Mach-O symbol-size cap below, so compute it once.
    let secs = readobj.as_ref().and_then(|t| sections(t, exe));
    let _ = writeln!(out, "\nsections (largest first):");
    match &secs {
        Some(secs) if !secs.is_empty() => {
            let mut sorted: Vec<&Section> = secs.iter().collect();
            sorted.sort_by_key(|s| std::cmp::Reverse(s.size));
            for s in sorted {
                let _ = writeln!(out, "  {:<20} {:>12}  {}", s.display_name(), group_digits(s.size), s.kind);
            }
        }
        _ => {
            let _ = writeln!(out, "  (unavailable: `llvm-readobj --sections` produced no parseable sections)");
        }
    }

    // Largest symbols. ELF symbol tables carry sizes (`llvm-nm --print-size`); Mach-O ones do not,
    // so sizes are derived from consecutive symbol addresses, capped at the owning section's end.
    // Stripped binaries have no symbol table → a note, not an error.
    match format {
        ObjectFormat::Elf => {
            let _ = writeln!(out, "\nlargest symbols (top {TOP_SYMBOLS}):");
        }
        ObjectFormat::MachO => {
            let _ = writeln!(
                out,
                "\nlargest symbols (top {TOP_SYMBOLS}, sizes derived from symbol address deltas — \
                 Mach-O carries no symbol sizes):"
            );
        }
    }
    let syms = nm.as_ref().and_then(|t| match format {
        ObjectFormat::Elf => elf_symbols(t, exe),
        ObjectFormat::MachO => macho_symbols(t, exe, secs.as_deref().unwrap_or(&[])),
    });
    match syms {
        Some(syms) if !syms.is_empty() => {
            for s in syms.iter().take(TOP_SYMBOLS) {
                let _ = writeln!(out, "  {:>12}  {} {}", group_digits(s.size), s.kind, s.name);
            }
        }
        _ => {
            let _ = writeln!(out, "  (none: the symbol table is absent — a stripped binary, or `llvm-nm` found no symbols)");
        }
    }

    // Relocation entries (llvm-readobj --relocations). A Mach-O executable legitimately has zero:
    // its fixups live in the dyld chained-fixups load command, which classic relocation counting
    // does not see — say so rather than print a bare misleading 0.
    match readobj.as_ref().and_then(|t| reloc_count(t, exe)) {
        Some(0) if format == ObjectFormat::MachO => {
            let _ = writeln!(out, "\nrelocations: 0 (Mach-O executables use dyld chained fixups; classic relocations are 0)");
        }
        Some(n) => {
            let _ = writeln!(out, "\nrelocations: {}", group_digits(n));
        }
        None => {
            let _ = writeln!(out, "\nrelocations: (unavailable)");
        }
    }

    // Dynamic dependencies (llvm-readobj --needed-libs): DT_NEEDED entries on ELF, LC_LOAD_DYLIB
    // install names (full paths — shown as-is) on Mach-O.
    let deps_label = match format {
        ObjectFormat::Elf => "DT_NEEDED",
        ObjectFormat::MachO => "LC_LOAD_DYLIB",
    };
    let _ = writeln!(out, "\ndynamic dependencies ({deps_label}):");
    match readobj.as_ref().and_then(|t| needed_libs(t, exe)) {
        Some(deps) if !deps.is_empty() => {
            for d in &deps {
                let _ = writeln!(out, "  {d}");
            }
        }
        Some(_) => {
            let _ = writeln!(out, "  (none — statically resolved)");
        }
        None => {
            let _ = writeln!(out, "  (unavailable: `llvm-readobj --needed-libs` did not run)");
        }
    }

    Ok(out)
}

/// One section's name / (Mach-O) segment / type / byte size / load address.
struct Section {
    name: String,
    /// Mach-O only: the segment the section lives in. Displayed as `__TEXT,__text` — the canonical
    /// Mach-O spelling, which also disambiguates same-named sections across segments (`__const`
    /// exists in both `__TEXT` and `__DATA_CONST`).
    segment: Option<String>,
    kind: String,
    size: u64,
    /// Load address — used to cap Mach-O derived symbol sizes at the owning section's end.
    addr: u64,
}

impl Section {
    fn display_name(&self) -> String {
        match &self.segment {
            Some(seg) => format!("{seg},{}", self.name),
            None => self.name.clone(),
        }
    }
}

/// Parse `llvm-readobj --sections <exe>` (both the ELF and Mach-O dialects) into sections. `None`
/// if the tool cannot run at all. The output is a block per section; within a block each field is
/// one `Key: Value` line, so the parser is a small accumulator keyed on trimmed line prefixes —
/// a new `Name:` starts the next section. Prefix matching is exact (`strip_prefix`), so sibling
/// keys like `EntrySize:` or `RelocationOffset:` never contaminate `Size:`/`Offset:`. Numbers are
/// hex with an `0x` prefix (Mach-O `Size`, all `Address`es) or decimal without (ELF `Size`).
/// Size-0 sections (the ELF NULL section, empty sections) carry no size signal and are skipped.
fn sections(tool: &Path, exe: &Path) -> Option<Vec<Section>> {
    /// The in-progress section block (all fields optional until seen).
    #[derive(Default)]
    struct Draft {
        name: String,
        segment: Option<String>,
        kind: String,
        size: Option<u64>,
        addr: u64,
    }
    /// A completed block becomes a [`Section`] iff it carried a nonzero `Size:` (the ELF NULL /
    /// empty sections have no size signal).
    fn flush(cur: &mut Option<Draft>, secs: &mut Vec<Section>) {
        if let Some(d) = cur.take()
            && let Some(size) = d.size
            && size > 0
        {
            secs.push(Section { name: d.name, segment: d.segment, kind: d.kind, size, addr: d.addr });
        }
    }
    /// The first whitespace-separated token of a field value — drops llvm-readobj's parenthesized
    /// raw/annotation forms (`__text (5F 5F …)`, `SHT_PROGBITS (0x1)`).
    fn first_token(v: &str) -> Option<&str> {
        v.split_whitespace().next()
    }

    let text = run_tool(tool, &["--sections", &exe.to_string_lossy()])?;
    let mut secs: Vec<Section> = Vec::new();
    let mut cur: Option<Draft> = None;
    for line in text.lines() {
        let l = line.trim();
        if let Some(v) = l.strip_prefix("Name:") {
            flush(&mut cur, &mut secs);
            cur = Some(Draft { name: first_token(v).unwrap_or("").to_string(), ..Draft::default() });
        } else if let Some(v) = l.strip_prefix("Segment:") {
            // Mach-O only.
            if let Some(c) = cur.as_mut() {
                c.segment = first_token(v).map(|s| s.to_string());
            }
        } else if let Some(v) = l.strip_prefix("Type:") {
            if let Some(c) = cur.as_mut() {
                c.kind = first_token(v).unwrap_or("").to_string();
            }
        } else if let Some(v) = l.strip_prefix("Size:") {
            if let Some(c) = cur.as_mut() {
                c.size = first_token(v).and_then(parse_u64);
            }
        } else if let Some(v) = l.strip_prefix("Address:")
            && let Some(c) = cur.as_mut()
        {
            c.addr = first_token(v).and_then(parse_u64).unwrap_or(0);
        }
    }
    flush(&mut cur, &mut secs);
    Some(secs)
}

/// Parse one llvm-readobj number: hex when `0x`-prefixed, decimal otherwise.
fn parse_u64(tok: &str) -> Option<u64> {
    match tok.strip_prefix("0x") {
        Some(hex) => u64::from_str_radix(hex, 16).ok(),
        None => tok.parse().ok(),
    }
}

/// One symbol's byte size / type letter / name.
struct Symbol {
    size: u64,
    kind: String,
    name: String,
}

/// ELF: parse `llvm-nm --print-size --size-sort --radix=d <exe>` into symbols, **largest first**.
/// `nm` sorts ascending by size, so we reverse. `None`/empty when the binary is stripped (no
/// `.symtab`) — `nm` then reports "no symbols", which is "no symbol data", not an error.
fn elf_symbols(tool: &Path, exe: &Path) -> Option<Vec<Symbol>> {
    let text = run_tool(tool, &["--print-size", "--size-sort", "--radix=d", &exe.to_string_lossy()])?;
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

/// Mach-O: the symbol table stores no sizes, so derive them from `llvm-nm -n --defined-only`
/// (address-sorted `ADDR TYPE NAME` lines, hex addresses): a symbol extends to the next symbol's
/// address, capped at its owning section's end (`secs` — so the last symbol in a section is not
/// credited with the following section's bytes). The Mach-O header symbols (`__mh_execute_header`
/// etc.) span whole segments, not code — excluded. Stripped binaries yield nothing after the
/// filter → the caller's absent-symbol-table note applies unchanged.
fn macho_symbols(tool: &Path, exe: &Path, secs: &[Section]) -> Option<Vec<Symbol>> {
    let text = run_tool(tool, &["-n", "--defined-only", &exe.to_string_lossy()])?;
    let mut entries: Vec<(u64, String, String)> = Vec::new();
    for line in text.lines() {
        let toks: Vec<&str> = line.split_whitespace().collect();
        if toks.len() < 3 {
            continue;
        }
        let Ok(addr) = u64::from_str_radix(toks[0], 16) else { continue };
        let name = toks[2..].join(" ");
        if name.starts_with("__mh_") {
            continue;
        }
        entries.push((addr, toks[1].to_string(), name));
    }
    let mut syms: Vec<Symbol> = Vec::with_capacity(entries.len());
    for (i, (addr, kind, name)) in entries.iter().enumerate() {
        let section_end = secs
            .iter()
            .find(|s| s.addr <= *addr && *addr < s.addr.saturating_add(s.size))
            .map(|s| s.addr.saturating_add(s.size));
        // Strictly greater: aliases share an address, and the first of a same-address run
        // must be credited with the run's real extent, not a zero delta (ELF's st_size path
        // gives every alias its full size — keep the formats consistent).
        let next_addr = entries[i + 1..].iter().find(|e| e.0 > *addr).map(|e| e.0);
        let end = match (next_addr, section_end) {
            (Some(n), Some(se)) => n.min(se),
            (Some(n), None) => n,
            (None, Some(se)) => se,
            (None, None) => *addr, // no bound at all: report 0 rather than guess
        };
        syms.push(Symbol { size: end.saturating_sub(*addr), kind: kind.clone(), name: name.clone() });
    }
    syms.sort_by_key(|s| std::cmp::Reverse(s.size));
    Some(syms)
}

/// Total relocation entries (`llvm-readobj --relocations`): the entry lines are the ones starting
/// with a hex offset (`0x…`) after trimming, in both the ELF and Mach-O dialects. `None` if the
/// tool cannot run; `Some(0)` for a file with no relocations.
fn reloc_count(tool: &Path, exe: &Path) -> Option<u64> {
    let text = run_tool(tool, &["--relocations", &exe.to_string_lossy()])?;
    Some(text.lines().filter(|l| l.trim().starts_with("0x")).count() as u64)
}

/// The dynamic-dependency names (`llvm-readobj --needed-libs`): the lines inside the
/// `NeededLibraries [ … ]` block — `DT_NEEDED` sonames on ELF, `LC_LOAD_DYLIB` install names
/// (full paths) on Mach-O. `None` if the tool cannot run; `Some(vec![])` for a binary with no
/// dynamic dependencies.
fn needed_libs(tool: &Path, exe: &Path) -> Option<Vec<String>> {
    let text = run_tool(tool, &["--needed-libs", &exe.to_string_lossy()])?;
    let mut deps = Vec::new();
    let mut inside = false;
    for line in text.lines() {
        let l = line.trim();
        if l == "NeededLibraries [" {
            inside = true;
        } else if inside && l == "]" {
            break;
        } else if inside && !l.is_empty() {
            deps.push(l.to_string());
        }
    }
    Some(deps)
}

/// Run an LLVM tool and return its stdout as a string. `None` when the tool cannot be launched —
/// the caller degrades that section of the report to a note. A nonzero exit that still produced
/// stdout (e.g. `llvm-nm` on a stripped file) is returned as-is; a nonzero exit with no stdout is
/// `None`.
fn run_tool(tool: &Path, args: &[&str]) -> Option<String> {
    // LC_ALL=C pins the output to the untranslated form — the parsers key on English field names.
    let output = Command::new(tool)
        .args(args)
        .env("LC_ALL", "C")
        .output()
        .ok()?;
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
    use super::{group_digits, parse_u64};

    #[test]
    fn group_digits_inserts_thousands_separators() {
        assert_eq!(group_digits(0), "0");
        assert_eq!(group_digits(42), "42");
        assert_eq!(group_digits(999), "999");
        assert_eq!(group_digits(1000), "1,000");
        assert_eq!(group_digits(5_519_840), "5,519,840");
        assert_eq!(group_digits(1_000_000), "1,000,000");
    }

    #[test]
    fn parse_u64_reads_hex_and_decimal() {
        // Mach-O sizes/addresses are 0x-hex; ELF sizes are plain decimal.
        assert_eq!(parse_u64("0x14"), Some(0x14));
        assert_eq!(parse_u64("0x100000328"), Some(0x1_0000_0328));
        assert_eq!(parse_u64("28"), Some(28));
        assert_eq!(parse_u64("0"), Some(0));
        assert_eq!(parse_u64("xyz"), None);
        assert_eq!(parse_u64("0xzz"), None);
    }
}
