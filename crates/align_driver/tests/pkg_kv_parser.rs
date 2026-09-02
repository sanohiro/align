//! `pkg.kv` RESP grammar, synchronization, and error-precedence owner.
//!
//! The production package source is rewritten only in this test fixture so a uniquely named C
//! reader returns an exact byte vector on each native read. One per-unit executable is built, then
//! every table row runs as its own deadline-bounded child. This makes coalescing, fragmentation,
//! EOF, post-retirement I/O, and parser-work counts deterministic without a real socket or port.

mod common;
use common::*;

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, TryLockError};
use std::time::{Duration, Instant};

const PROCESS_TIMEOUT: Duration = Duration::from_secs(30);
const CASE_TIMEOUT: Duration = Duration::from_secs(2);
const CC_PROBE_TIMEOUT: Duration = Duration::from_secs(5);
const PIPE_SETUP_TIMEOUT: Duration = Duration::from_secs(1);
const PROCESS_CLEANUP_TIMEOUT: Duration = Duration::from_secs(3);
const DRAIN_CANCEL_RESERVE: Duration = Duration::from_millis(100);
const CHILD_POLL_INTERVAL: Duration = Duration::from_millis(5);
const MAX_CAPTURE_BYTES: usize = 64 * 1024;
const LINK_CHILD_ENV: &str = "ALIGN_PKG_KV_PARSER_LINK_CHILD";
const LINK_EXE_ENV: &str = "ALIGN_PKG_KV_PARSER_LINK_EXE";
const LINK_OBJECT_COUNT_ENV: &str = "ALIGN_PKG_KV_PARSER_LINK_OBJECT_COUNT";
const LINK_LIBRARY_COUNT_ENV: &str = "ALIGN_PKG_KV_PARSER_LINK_LIBRARY_COUNT";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ParserWorkInventory {
    functions: usize,
    loops: usize,
    index_sites: usize,
    comparison_operators: usize,
    comparison_statements: usize,
    match_sites: usize,
    runtime_hooks: usize,
}

fn normalized_source_fingerprint(source: &str) -> u64 {
    // FNV-1a is deliberately implemented here rather than delegated to a tool. The fingerprint is
    // a source-shape tripwire, not a security boundary, and is stable on every host.
    source
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with("//"))
        .flat_map(|line| line.bytes().chain(std::iter::once(b'\n')))
        .fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
        })
}

fn exact_source_fingerprint(source: &str) -> u64 {
    source
        .bytes()
        .fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
        })
}

fn comparison_operator_count(line: &str) -> usize {
    [" == ", " != ", " <= ", " >= ", " < ", " > "]
        .iter()
        .map(|operator| line.matches(operator).count())
        .sum()
}

fn continuation_statement_start(lines: &[&str], mut index: usize) -> usize {
    while index > 0 {
        let trimmed = lines[index].trim_start();
        if !trimmed.starts_with("||") && !trimmed.starts_with("&&") {
            break;
        }
        index -= 1;
    }
    index
}

fn instrument_parser_work(source: String) -> (String, ParserWorkInventory) {
    const REGION_START: &str = "fn new_read_input(";
    const EXPECTED_FUNCTIONS: [&str; 22] = [
        "new_read_input",
        "free_read_input",
        "refill",
        "next_byte",
        "required_byte",
        "trailing_in_current_read",
        "bulk_length",
        "parse_error_reply",
        "parse_get",
        "parse_set",
        "parse_delete_integer",
        "parse_delete",
        "read_get_response",
        "read_set_response",
        "read_delete_response",
        "expiry_milliseconds",
        "finish_get",
        "finish_bool",
        "connect",
        "get",
        "set",
        "delete",
    ];

    assert_eq!(source.matches(REGION_START).count(), 1);
    let start = source
        .find(REGION_START)
        .expect("pkg.kv parser region start");
    // The region deliberately extends through EOF. It includes refill/input ownership, every parser
    // helper, every read/finish wrapper, and the public get/set/delete callers. The exact whole-file
    // fingerprint below also trips if a newly reachable helper is inserted before this boundary.
    let region = &source[start..];
    let fingerprint = normalized_source_fingerprint(region);
    assert_eq!(
        fingerprint, 17_493_975_231_586_487_682,
        "pkg.kv parser source shape changed; audit every reachable scan and refresh the inventory",
    );

    let lines = region.lines().collect::<Vec<_>>();
    let functions = lines
        .iter()
        .filter_map(|line| {
            line.trim()
                .strip_prefix("fn ")
                .or_else(|| line.trim().strip_prefix("pub fn "))
        })
        .filter_map(|tail| tail.split_once('(').map(|(name, _)| name))
        .collect::<Vec<_>>();
    assert_eq!(functions, EXPECTED_FUNCTIONS);

    let mut comparison_starts = BTreeSet::new();
    let mut loops = BTreeSet::new();
    let mut index_sites = 0;
    let mut comparison_operators = 0;
    let mut match_sites = 0;
    for (index, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with("//") {
            continue;
        }
        if trimmed == "loop {" {
            loops.insert(index);
        }
        if trimmed.contains('[') && trimmed.contains(']') {
            index_sites += 1;
        }
        let operators = comparison_operator_count(trimmed);
        if operators != 0 {
            comparison_operators += operators;
            comparison_starts.insert(continuation_statement_start(&lines, index));
        }
        if trimmed.contains("match ") {
            match_sites += 1;
        }
    }
    let inventory = ParserWorkInventory {
        functions: functions.len(),
        loops: loops.len(),
        index_sites,
        comparison_operators,
        comparison_statements: comparison_starts.len(),
        match_sites,
        runtime_hooks: loops.len(),
    };
    assert_eq!(
        inventory,
        ParserWorkInventory {
            functions: 22,
            loops: 4,
            index_sites: 3,
            comparison_operators: 79,
            comparison_statements: 66,
            match_sites: 15,
            runtime_hooks: 4,
        },
        "pkg.kv parser work inventory changed; audit and instrument every new work site",
    );

    let mut instrumented = String::with_capacity(region.len() + inventory.runtime_hooks * 72);
    let mut next_site = 0;
    for (index, line) in lines.iter().enumerate() {
        let indentation = &line[..line.len() - line.trim_start().len()];
        writeln!(instrumented, "{line}").unwrap();
        if loops.contains(&index) {
            writeln!(
                instrumented,
                "{indentation}  unsafe {{ align_kv_parser_stub_work_increment({next_site}) }}"
            )
            .unwrap();
            next_site += 1;
        }
    }
    assert_eq!(next_site, inventory.runtime_hooks);
    instrumented.pop();
    assert_eq!(
        instrumented
            .matches("unsafe { align_kv_parser_stub_work_increment(")
            .count(),
        inventory.runtime_hooks,
        "every loop iteration must receive its unique runtime counter",
    );

    let mut output = String::with_capacity(source.len() + instrumented.len() - region.len());
    output.push_str(&source[..start]);
    output.push_str(&instrumented);
    (output, inventory)
}

#[derive(Clone, Copy)]
struct RewriteSpec {
    original: &'static str,
    replacement: &'static str,
    declarations: usize,
    calls: usize,
}

fn identifier_start(byte: u8) -> bool {
    byte == b'_' || byte.is_ascii_alphabetic()
}

fn identifier_continue(byte: u8) -> bool {
    identifier_start(byte) || byte.is_ascii_digit()
}

fn replace_required(source: &str, label: &str, specs: &[RewriteSpec]) -> String {
    #[derive(Clone, Copy)]
    enum LexState {
        Code,
        Comment,
        String { escaped: bool },
    }

    let bytes = source.as_bytes();
    let mut output = String::with_capacity(source.len());
    let mut declarations = vec![0; specs.len()];
    let mut calls = vec![0; specs.len()];
    let mut index = 0;
    let mut previous_was_fn = false;
    let mut state = LexState::Code;
    while index < bytes.len() {
        match state {
            LexState::Comment => {
                let byte = bytes[index];
                if byte.is_ascii() {
                    output.push(char::from(byte));
                    index += 1;
                } else {
                    let character = source[index..].chars().next().expect("valid UTF-8 source");
                    output.push(character);
                    index += character.len_utf8();
                }
                if byte == b'\n' {
                    state = LexState::Code;
                }
            }
            LexState::String { escaped } => {
                let byte = bytes[index];
                if byte.is_ascii() {
                    output.push(char::from(byte));
                    index += 1;
                } else {
                    let character = source[index..].chars().next().expect("valid UTF-8 source");
                    output.push(character);
                    index += character.len_utf8();
                }
                state = if escaped {
                    LexState::String { escaped: false }
                } else if byte == b'\\' {
                    LexState::String { escaped: true }
                } else if byte == b'"' {
                    LexState::Code
                } else {
                    LexState::String { escaped: false }
                };
            }
            LexState::Code => {
                if bytes[index] == b'/' && bytes.get(index + 1) == Some(&b'/') {
                    output.push_str("//");
                    index += 2;
                    state = LexState::Comment;
                    continue;
                }
                if bytes[index] == b'"' {
                    output.push('"');
                    index += 1;
                    state = LexState::String { escaped: false };
                    previous_was_fn = false;
                    continue;
                }
                if identifier_start(bytes[index]) {
                    let start = index;
                    index += 1;
                    while index < bytes.len() && identifier_continue(bytes[index]) {
                        index += 1;
                    }
                    let identifier = &source[start..index];
                    if let Some((spec_index, spec)) = specs
                        .iter()
                        .enumerate()
                        .find(|(_, spec)| spec.original == identifier)
                    {
                        let next = bytes[index..]
                            .iter()
                            .position(|byte| !byte.is_ascii_whitespace())
                            .map(|offset| bytes[index + offset]);
                        assert_eq!(
                            next,
                            Some(b'('),
                            "rewritten `{label}` symbol `{identifier}` is neither a declaration nor a live call",
                        );
                        if previous_was_fn {
                            declarations[spec_index] += 1;
                        } else {
                            calls[spec_index] += 1;
                        }
                        output.push_str(spec.replacement);
                    } else {
                        output.push_str(identifier);
                    }
                    previous_was_fn = identifier == "fn";
                    continue;
                }

                let byte = bytes[index];
                if byte.is_ascii() {
                    output.push(char::from(byte));
                    index += 1;
                } else {
                    let character = source[index..].chars().next().expect("valid UTF-8 source");
                    output.push(character);
                    index += character.len_utf8();
                }
                if !byte.is_ascii_whitespace() {
                    previous_was_fn = false;
                }
            }
        }
    }

    assert!(matches!(state, LexState::Code | LexState::Comment));
    for (spec_index, spec) in specs.iter().enumerate() {
        assert_eq!(
            (declarations[spec_index], calls[spec_index]),
            (spec.declarations, spec.calls),
            "rewritten `{label}` symbol `{}` declaration/live-call inventory changed",
            spec.original,
        );
    }
    output
}

fn assert_lexical_rewriter() {
    const SPEC: RewriteSpec = RewriteSpec {
        original: "align_rt_probe",
        replacement: "align_stub_probe",
        declarations: 1,
        calls: 1,
    };
    let source = concat!(
        "extern \"C\" { fn align_rt_probe() }\n",
        "fn use_probe() { align_rt_probe() }\n",
        "// align_rt_probe()\n",
        "text := \"align_rt_probe()\"\n",
        "align_rt_probe_suffix := 1\n",
    );
    let expected = source
        .replacen("align_rt_probe", "align_stub_probe", 1)
        .replacen("align_rt_probe", "align_stub_probe", 1);
    assert_eq!(replace_required(source, "lexical probe", &[SPEC]), expected);
}

fn assert_source_fingerprint(label: &str, source: &str, expected: u64) {
    assert_eq!(
        exact_source_fingerprint(source),
        expected,
        "`{label}` source fingerprint changed; refresh the exact lexical rewrite inventory",
    );
}

fn parser_sources() -> (String, String, ParserWorkInventory) {
    assert_lexical_rewriter();
    let native = [
        RewriteSpec {
            original: "align_rt_tcp_connect",
            replacement: "align_kv_parser_stub_tcp_connect",
            declarations: 1,
            calls: 1,
        },
        RewriteSpec {
            original: "align_rt_tcp_conn_set_io_timeout",
            replacement: "align_kv_parser_stub_set_io_timeout",
            declarations: 1,
            calls: 1,
        },
        RewriteSpec {
            original: "align_rt_tcp_conn_free",
            replacement: "align_kv_parser_stub_conn_free",
            declarations: 1,
            calls: 1,
        },
        RewriteSpec {
            original: "align_rt_tcp_conn_reader",
            replacement: "align_kv_parser_stub_conn_reader",
            declarations: 1,
            calls: 1,
        },
        RewriteSpec {
            original: "align_rt_tcp_conn_writer",
            replacement: "align_kv_parser_stub_conn_writer",
            declarations: 1,
            calls: 1,
        },
        RewriteSpec {
            original: "align_rt_io_reader_read",
            replacement: "align_kv_parser_stub_reader_read",
            declarations: 1,
            calls: 1,
        },
        RewriteSpec {
            original: "align_rt_io_reader_free",
            replacement: "align_kv_parser_stub_reader_free",
            declarations: 1,
            calls: 0,
        },
        RewriteSpec {
            original: "align_rt_io_writer_write",
            replacement: "align_kv_parser_stub_writer_write",
            declarations: 1,
            calls: 1,
        },
        RewriteSpec {
            original: "align_rt_io_writer_free",
            replacement: "align_kv_parser_stub_writer_free",
            declarations: 1,
            calls: 0,
        },
        RewriteSpec {
            original: "align_rt_buffer_new",
            replacement: "align_kv_parser_stub_buffer_new",
            declarations: 1,
            calls: 1,
        },
        RewriteSpec {
            original: "align_rt_buffer_bytes",
            replacement: "align_kv_parser_stub_buffer_bytes",
            declarations: 1,
            calls: 1,
        },
        RewriteSpec {
            original: "align_rt_buffer_capacity",
            replacement: "align_kv_parser_stub_buffer_capacity",
            declarations: 1,
            calls: 1,
        },
        RewriteSpec {
            original: "align_rt_buffer_free",
            replacement: "align_kv_parser_stub_buffer_free",
            declarations: 1,
            calls: 1,
        },
    ];
    let root_source = fixture("apps/kv/pkg/kv.align");
    assert_source_fingerprint("pkg.kv", root_source, 9_990_699_685_482_827_385);
    let root = replace_required(root_source, "pkg.kv", &native);
    assert_source_fingerprint("rewritten pkg.kv", &root, 14_607_054_099_248_186_247);

    // Keep byte advancement exact and independent of the exhaustive parser-work hooks. The source
    // inventory below makes a new loop, index, comparison, match, helper, or call-site change an
    // explicit audit event and then instruments every admitted work site mechanically.
    let (mut root, inventory) = instrument_parser_work(root);
    const ADVANCE: &str = "    raw.store(input, 32, input_index + 1)";
    assert_eq!(root.lines().filter(|line| *line == ADVANCE).count(), 1);
    root = root.replacen(
        ADVANCE,
        "    align_kv_parser_stub_byte_advance()\n    raw.store(input, 32, input_index + 1)",
        1,
    );
    root.push_str(
        r#"

extern "C" {
  fn align_kv_parser_stub_byte_advance()
  fn align_kv_parser_stub_work_increment(site: i64)
}
"#,
    );

    let internal_native = [
        RewriteSpec {
            original: "align_rt_tcp_conn_free",
            replacement: "align_kv_parser_stub_conn_free",
            declarations: 1,
            calls: 1,
        },
        RewriteSpec {
            original: "align_rt_io_reader_free",
            replacement: "align_kv_parser_stub_reader_free",
            declarations: 1,
            calls: 1,
        },
        RewriteSpec {
            original: "align_rt_io_writer_free",
            replacement: "align_kv_parser_stub_writer_free",
            declarations: 1,
            calls: 1,
        },
    ];
    let internal_source = fixture("apps/kv/pkg/kv/internal/resource.align");
    assert_source_fingerprint(
        "pkg.kv internal resource",
        internal_source,
        4_024_889_105_027_112_977,
    );
    let internal = replace_required(
        internal_source,
        "pkg.kv internal resource",
        &internal_native,
    );
    assert_source_fingerprint(
        "rewritten pkg.kv internal resource",
        &internal,
        4_919_108_930_481_249_269,
    );
    (root, internal, inventory)
}

#[derive(Clone, Copy, Debug)]
enum Operation {
    Get,
    SetAlways,
    SetIfAbsent,
    Delete,
}

impl Operation {
    fn request(self, key: &str) -> Vec<u8> {
        match self {
            Self::Get => resp_request(&[b"GET", key.as_bytes()]),
            Self::SetAlways => resp_request(&[b"SET", key.as_bytes(), b"v"]),
            Self::SetIfAbsent => resp_request(&[b"SET", key.as_bytes(), b"v", b"NX"]),
            Self::Delete => resp_request(&[b"DEL", key.as_bytes()]),
        }
    }

    fn code(self) -> i32 {
        match self {
            Self::Get => 0,
            Self::SetAlways => 1,
            Self::SetIfAbsent => 2,
            Self::Delete => 3,
        }
    }
}

#[derive(Clone, Debug)]
enum Expected {
    Server(Vec<u8>),
    Decode,
    ResponseTooLarge,
    Protocol,
    Closed,
    Missing,
    Text(Vec<u8>),
    TextLength(i64),
    Bool(bool),
}

impl Expected {
    fn code(&self) -> i32 {
        match self {
            Self::Missing => 1,
            Self::Text(_) | Self::TextLength(_) => 2,
            Self::Bool(false) => 3,
            Self::Bool(true) => 4,
            Self::Server(_) => 5,
            Self::Decode => 6,
            Self::ResponseTooLarge => 7,
            Self::Protocol => 8,
            Self::Closed => 9,
        }
    }

    fn expected_text(&self) -> Vec<u8> {
        match self {
            Self::Server(text) | Self::Text(text) => text.clone(),
            Self::TextLength(length) => vec![b'a'; *length as usize],
            _ => Vec::new(),
        }
    }

    fn reusable_error(&self) -> bool {
        matches!(self, Self::Server(_) | Self::Decode)
    }

    fn terminal_error(&self) -> bool {
        matches!(self, Self::ResponseTooLarge | Self::Protocol | Self::Closed)
    }
}

#[derive(Clone, Debug)]
struct Case {
    label: String,
    cap: i64,
    operation: Operation,
    chunks: Vec<Vec<u8>>,
    eof_after_chunks: bool,
    expected: Expected,
    expected_first_steps: usize,
}

impl Case {
    fn coalesced(
        label: impl Into<String>,
        cap: i64,
        operation: Operation,
        reply: Vec<u8>,
        expected: Expected,
    ) -> Self {
        let expected_first_steps = reply.len();
        Self {
            label: label.into(),
            cap,
            operation,
            chunks: vec![reply],
            eof_after_chunks: false,
            expected,
            expected_first_steps,
        }
    }

    fn fragmented(
        label: impl Into<String>,
        cap: i64,
        operation: Operation,
        chunks: Vec<Vec<u8>>,
        expected: Expected,
    ) -> Self {
        let expected_first_steps = chunks.iter().map(Vec::len).sum();
        Self {
            label: label.into(),
            cap,
            operation,
            chunks,
            eof_after_chunks: false,
            expected,
            expected_first_steps,
        }
    }

    fn eof(
        label: impl Into<String>,
        cap: i64,
        operation: Operation,
        prefix: Vec<u8>,
        expected: Expected,
    ) -> Self {
        let expected_first_steps = prefix.len();
        let chunks = if prefix.is_empty() {
            Vec::new()
        } else {
            vec![prefix]
        };
        Self {
            label: label.into(),
            cap,
            operation,
            chunks,
            eof_after_chunks: true,
            expected,
            expected_first_steps,
        }
    }

    fn consuming(mut self, steps: usize) -> Self {
        assert!(steps <= self.chunks.iter().map(Vec::len).sum());
        self.expected_first_steps = steps;
        self
    }

    fn key(&self, index: usize) -> String {
        format!("case-{index}")
    }

    fn scripted_chunks(&self) -> Vec<Vec<u8>> {
        let mut chunks = self.chunks.clone();
        if self.expected.reusable_error() {
            chunks.push(b"$-1\r\n".to_vec());
        }
        chunks
    }

    fn expected_steps(&self) -> usize {
        self.expected_first_steps + usize::from(self.expected.reusable_error()) * 5
    }

    fn expected_reads(&self) -> usize {
        self.chunks.len()
            + usize::from(self.eof_after_chunks)
            + usize::from(self.expected.reusable_error())
    }

    fn expected_wire_operations(&self) -> usize {
        1 + usize::from(self.expected.reusable_error())
    }

    fn parser_site_visit_bound(&self) -> usize {
        // The fingerprinted response call graph has no recursion or opaque scanning primitive. For
        // one static loop site, each visit either consumes the next byte, performs one
        // command-level transition, or observes the final decision/EOF edge. The bound is therefore
        // per site, not multiplied by the source inventory: a nested rescan makes its loop site
        // exceed this linear allowance at either the 256- or 8192-byte witness.
        self.expected_steps() + self.expected_wire_operations() + 1
    }

    fn expected_writes(&self, index: usize) -> Vec<u8> {
        let mut writes = self.operation.request(&self.key(index));
        if self.expected.reusable_error() {
            writes.extend_from_slice(&Operation::Get.request("reuse"));
        }
        writes
    }
}

fn resp_request(arguments: &[&[u8]]) -> Vec<u8> {
    let mut request = format!("*{}\r\n", arguments.len()).into_bytes();
    for argument in arguments {
        request.extend_from_slice(format!("${}\r\n", argument.len()).as_bytes());
        request.extend_from_slice(argument);
        request.extend_from_slice(b"\r\n");
    }
    request
}

fn one_byte_chunks(bytes: &[u8]) -> Vec<Vec<u8>> {
    bytes.iter().map(|byte| vec![*byte]).collect()
}

fn multipart_chunks(bytes: &[u8]) -> Vec<Vec<u8>> {
    assert!(bytes.len() >= 4);
    let first = 1;
    let second = bytes.len() / 2;
    let third = bytes.len() - 1;
    vec![
        bytes[..first].to_vec(),
        bytes[first..second].to_vec(),
        bytes[second..third].to_vec(),
        bytes[third..].to_vec(),
    ]
}

fn add_fragmentation_product(
    cases: &mut Vec<Case>,
    stem: &str,
    cap: i64,
    operation: Operation,
    frame: &[u8],
    expected: Expected,
) {
    cases.push(Case::coalesced(
        format!("{stem}-coalesced"),
        cap,
        operation,
        frame.to_vec(),
        expected.clone(),
    ));
    cases.push(Case::fragmented(
        format!("{stem}-one-byte"),
        cap,
        operation,
        one_byte_chunks(frame),
        expected.clone(),
    ));
    for split in 1..frame.len() {
        cases.push(Case::fragmented(
            format!("{stem}-split-{split}"),
            cap,
            operation,
            vec![frame[..split].to_vec(), frame[split..].to_vec()],
            expected.clone(),
        ));
    }
    cases.push(Case::fragmented(
        format!("{stem}-multipart"),
        cap,
        operation,
        multipart_chunks(frame),
        expected,
    ));
}

fn add_eof_ordinals(
    cases: &mut Vec<Case>,
    stem: &str,
    cap: i64,
    operation: Operation,
    complete_frame: &[u8],
) {
    for cut in 0..complete_frame.len() {
        cases.push(Case::eof(
            format!("{stem}-eof-{cut}"),
            cap,
            operation,
            complete_frame[..cut].to_vec(),
            if cut == 0 {
                Expected::Closed
            } else {
                Expected::Protocol
            },
        ));
    }
}

#[derive(Clone, Copy)]
enum ErrorEnding {
    Crlf,
    CrNonLf,
    LoneLf,
}

impl ErrorEnding {
    fn name(self) -> &'static str {
        match self {
            Self::Crlf => "crlf",
            Self::CrNonLf => "cr-non-lf",
            Self::LoneLf => "lone-lf",
        }
    }

    fn bytes(self) -> &'static [u8] {
        match self {
            Self::Crlf => b"\r\n",
            Self::CrNonLf => b"\rX",
            Self::LoneLf => b"\n",
        }
    }
}

fn error_decision_ordinal(payload_len: usize, ending: ErrorEnding, over_cap: bool) -> usize {
    if over_cap {
        // Marker plus the fifth payload byte for cap four.
        return 1 + 5;
    }
    1 + payload_len
        + match ending {
            ErrorEnding::Crlf | ErrorEnding::CrNonLf => 2,
            ErrorEnding::LoneLf => 1,
        }
}

fn fragmented_through_decision(frame: &[u8], decision_ordinal: usize) -> Vec<Vec<u8>> {
    assert!(decision_ordinal > 0 && decision_ordinal <= frame.len());
    let decision_index = decision_ordinal - 1;
    let mut chunks = one_byte_chunks(&frame[..decision_index]);
    chunks.push(frame[decision_index..].to_vec());
    chunks
}

fn add_error_precedence_product(cases: &mut Vec<Case>) {
    for (length_name, payload) in [
        ("below", b"ABC".as_slice()),
        ("exact", b"ABCD".as_slice()),
        ("next", b"ABCDE".as_slice()),
    ] {
        for ending in [ErrorEnding::Crlf, ErrorEnding::CrNonLf, ErrorEnding::LoneLf] {
            for trailing in [false, true] {
                let mut frame = vec![b'-'];
                frame.extend_from_slice(payload);
                frame.extend_from_slice(ending.bytes());
                if trailing {
                    frame.push(b'Z');
                }
                let over_cap = payload.len() > 4;
                let decision = error_decision_ordinal(payload.len(), ending, over_cap);
                let expected = if over_cap {
                    Expected::ResponseTooLarge
                } else if matches!(ending, ErrorEnding::Crlf) && !trailing {
                    Expected::Server(payload.to_vec())
                } else {
                    Expected::Protocol
                };
                let trailing_name = if trailing { "trailing" } else { "complete" };
                let stem = format!(
                    "error-product-{length_name}-{}-{trailing_name}",
                    ending.name()
                );
                cases.push(
                    Case::coalesced(
                        format!("{stem}-coalesced"),
                        4,
                        Operation::Get,
                        frame.clone(),
                        expected.clone(),
                    )
                    .consuming(decision),
                );
                cases.push(
                    Case::fragmented(
                        format!("{stem}-fragmented"),
                        4,
                        Operation::Get,
                        fragmented_through_decision(&frame, decision),
                        expected,
                    )
                    .consuming(decision),
                );
            }
        }
    }
}

fn add_marker_sweeps(cases: &mut Vec<Case>) {
    // RESP2 plus every RESP3 type/chunk/stream marker. `?` and NUL pin unknown-byte rejection.
    let markers = [
        ("simple-string", b'+'),
        ("simple-error", b'-'),
        ("integer", b':'),
        ("bulk-string", b'$'),
        ("array", b'*'),
        ("null", b'_'),
        ("boolean", b'#'),
        ("double", b','),
        ("big-number", b'('),
        ("bulk-error", b'!'),
        ("verbatim-string", b'='),
        ("map", b'%'),
        ("set", b'~'),
        ("push", b'>'),
        ("attribute", b'|'),
        ("stream-end", b'.'),
        ("chunk", b';'),
        ("unknown", b'?'),
        ("nul", 0),
    ];
    for (operation_name, operation, accepted) in [
        ("get", Operation::Get, b"$-".as_slice()),
        ("set", Operation::SetAlways, b"+$-".as_slice()),
        ("conditional-set", Operation::SetIfAbsent, b"+$-".as_slice()),
        ("delete", Operation::Delete, b":-".as_slice()),
    ] {
        for (marker_name, marker) in markers {
            if accepted.contains(&marker) {
                continue;
            }
            cases.push(Case::coalesced(
                format!("marker-{operation_name}-{marker_name}"),
                8,
                operation,
                vec![marker],
                Expected::Protocol,
            ));
        }
    }
}

fn add_null_mutations(cases: &mut Vec<Case>) {
    let mutations = [
        ("zero", b"$-0\r\n".as_slice(), 3),
        ("two", b"$-2\r\n".as_slice(), 3),
        ("leading-zero", b"$-01\r\n".as_slice(), 3),
        ("plus-one", b"$+1\r\n".as_slice(), 2),
        ("missing-cr", b"$-1X".as_slice(), 4),
        ("lone-lf", b"$-1\n".as_slice(), 4),
        ("cr-non-lf", b"$-1\rX".as_slice(), 5),
        ("trailing", b"$-1\r\nX".as_slice(), 5),
    ];
    for (operation_name, operation) in [
        ("get", Operation::Get),
        ("conditional-set", Operation::SetIfAbsent),
    ] {
        for (mutation_name, reply, consumed) in mutations {
            cases.push(
                Case::coalesced(
                    format!("null-{operation_name}-{mutation_name}"),
                    8,
                    operation,
                    reply.to_vec(),
                    Expected::Protocol,
                )
                .consuming(consumed),
            );
        }
    }
    cases.push(Case::coalesced(
        "null-unconditional-set",
        8,
        Operation::SetAlways,
        b"$-1\r\n".to_vec(),
        Expected::Protocol,
    ));
}

fn parser_cases() -> Vec<Case> {
    let mut cases = Vec::new();

    // Complete accepted branches own coalescing, every possible two-part split, one-byte reads,
    // and an uneven multipart shape. Error branches exercise the shared parser from each command.
    add_fragmentation_product(
        &mut cases,
        "get-bulk",
        3,
        Operation::Get,
        b"$3\r\nabc\r\n",
        Expected::Text(b"abc".to_vec()),
    );
    add_fragmentation_product(
        &mut cases,
        "get-error",
        8,
        Operation::Get,
        b"-ERR\r\n",
        Expected::Server(b"ERR".to_vec()),
    );
    add_fragmentation_product(
        &mut cases,
        "get-null",
        8,
        Operation::Get,
        b"$-1\r\n",
        Expected::Missing,
    );
    add_fragmentation_product(
        &mut cases,
        "set-ok",
        8,
        Operation::SetAlways,
        b"+OK\r\n",
        Expected::Bool(true),
    );
    add_fragmentation_product(
        &mut cases,
        "set-null",
        8,
        Operation::SetIfAbsent,
        b"$-1\r\n",
        Expected::Bool(false),
    );
    add_fragmentation_product(
        &mut cases,
        "set-error",
        8,
        Operation::SetAlways,
        b"-ERR\r\n",
        Expected::Server(b"ERR".to_vec()),
    );
    add_fragmentation_product(
        &mut cases,
        "delete-integer",
        8,
        Operation::Delete,
        b":+01\r\n",
        Expected::Bool(true),
    );
    add_fragmentation_product(
        &mut cases,
        "delete-error",
        8,
        Operation::Delete,
        b"-ERR\r\n",
        Expected::Server(b"ERR".to_vec()),
    );

    // Every truncation prefix is owned for every distinct parser branch. Cut zero is the only
    // Closed producer; all later EOF ordinals have consumed a prefix and are Protocol.
    for (stem, cap, operation, frame) in [
        ("get-bulk", 3, Operation::Get, b"$3\r\nabc\r\n".as_slice()),
        ("get-error", 8, Operation::Get, b"-ERR\r\n".as_slice()),
        ("get-null", 8, Operation::Get, b"$-1\r\n".as_slice()),
        ("set-ok", 8, Operation::SetAlways, b"+OK\r\n".as_slice()),
        ("set-null", 8, Operation::SetIfAbsent, b"$-1\r\n".as_slice()),
        ("set-error", 8, Operation::SetAlways, b"-ERR\r\n".as_slice()),
        (
            "delete-integer",
            8,
            Operation::Delete,
            b":+01\r\n".as_slice(),
        ),
        ("delete-error", 8, Operation::Delete, b"-ERR\r\n".as_slice()),
    ] {
        add_eof_ordinals(&mut cases, stem, cap, operation, frame);
    }

    // Full error payload-boundary x line-ending x same-read-trailing x fragmentation product.
    add_error_precedence_product(&mut cases);
    let invalid_error = vec![b'-', b'A', 0xff, b'B', b'C', b'\r', b'\n'];
    for (operation_name, operation, chunks) in [
        ("get", Operation::Get, one_byte_chunks(&invalid_error)),
        ("set", Operation::SetAlways, vec![invalid_error.clone()]),
        (
            "conditional-set",
            Operation::SetIfAbsent,
            multipart_chunks(&invalid_error),
        ),
        (
            "delete",
            Operation::Delete,
            vec![invalid_error[..3].to_vec(), invalid_error[3..].to_vec()],
        ),
    ] {
        cases.push(Case::fragmented(
            format!("error-invalid-utf8-{operation_name}"),
            4,
            operation,
            chunks,
            Expected::Decode,
        ));
    }
    let mut invalid_error_trailing = invalid_error.clone();
    invalid_error_trailing.push(b'X');
    cases.push(
        Case::coalesced(
            "error-invalid-utf8-trailing",
            4,
            Operation::Get,
            invalid_error_trailing,
            Expected::Protocol,
        )
        .consuming(invalid_error.len()),
    );
    cases.push(Case::coalesced(
        "error-invalid-utf8-next-payload",
        4,
        Operation::Get,
        vec![b'-', b'A', 0xff, b'B', b'C', b'X'],
        Expected::ResponseTooLarge,
    ));
    cases.push(Case::coalesced(
        "error-invalid-utf8-cr-non-lf",
        4,
        Operation::Get,
        vec![b'-', b'A', 0xff, b'B', b'C', b'\r', b'X'],
        Expected::Protocol,
    ));
    cases.push(
        Case::coalesced(
            "error-invalid-utf8-next-cap",
            4,
            Operation::Get,
            vec![b'-', b'A', 0xff, b'B', b'C', b'D', b'\r', b'\n'],
            Expected::ResponseTooLarge,
        )
        .consuming(6),
    );
    cases.push(Case::fragmented(
        "error-exact-nul-server",
        4,
        Operation::Delete,
        vec![b"-A\0".to_vec(), b"BC\r\n".to_vec()],
        Expected::Server(b"A\0BC".to_vec()),
    ));
    cases.push(Case::fragmented(
        "error-empty-one-byte",
        0,
        Operation::Get,
        one_byte_chunks(b"-\r\n"),
        Expected::Server(Vec::new()),
    ));
    cases.push(
        Case::coalesced(
            "error-empty-trailing",
            0,
            Operation::Get,
            b"-\r\nX".to_vec(),
            Expected::Protocol,
        )
        .consuming(3),
    );
    cases.push(
        Case::coalesced(
            "error-empty-next-payload",
            0,
            Operation::Get,
            b"-A\r\n".to_vec(),
            Expected::ResponseTooLarge,
        )
        .consuming(2),
    );

    // GET bulk payload framing, including framing-before-UTF-8 classification.
    let invalid_bulk = vec![b'$', b'2', b'\r', b'\n', 0xff, 0xfe, b'\r', b'\n'];
    cases.push(Case::coalesced(
        "get-invalid-utf8",
        2,
        Operation::Get,
        invalid_bulk.clone(),
        Expected::Decode,
    ));
    cases.push(Case::fragmented(
        "get-invalid-utf8-one-byte",
        2,
        Operation::Get,
        one_byte_chunks(&invalid_bulk),
        Expected::Decode,
    ));
    let mut invalid_trailing = invalid_bulk.clone();
    invalid_trailing.push(b'X');
    cases.push(
        Case::coalesced(
            "get-invalid-utf8-trailing",
            2,
            Operation::Get,
            invalid_trailing,
            Expected::Protocol,
        )
        .consuming(invalid_bulk.len()),
    );
    cases.push(Case::coalesced(
        "get-invalid-utf8-missing-cr",
        2,
        Operation::Get,
        vec![b'$', b'2', b'\r', b'\n', 0xff, 0xfe, b'X'],
        Expected::Protocol,
    ));
    cases.push(Case::coalesced(
        "get-invalid-utf8-cr-non-lf",
        2,
        Operation::Get,
        vec![b'$', b'2', b'\r', b'\n', 0xff, 0xfe, b'\r', b'X'],
        Expected::Protocol,
    ));
    cases.push(
        Case::coalesced(
            "get-valid-bulk-trailing",
            3,
            Operation::Get,
            b"$3\r\nabc\r\nX".to_vec(),
            Expected::Protocol,
        )
        .consuming(9),
    );
    for (label, reply) in [
        ("get-bulk-missing-payload-cr", b"$3\r\nabcX".as_slice()),
        ("get-bulk-payload-cr-non-lf", b"$3\r\nabc\rX".as_slice()),
        ("get-bulk-header-missing-cr", b"$3X".as_slice()),
        ("get-bulk-header-cr-non-lf", b"$3\rX".as_slice()),
    ] {
        cases.push(Case::coalesced(
            label,
            3,
            Operation::Get,
            reply.to_vec(),
            Expected::Protocol,
        ));
    }
    cases.push(Case::coalesced(
        "get-empty-bulk",
        0,
        Operation::Get,
        b"$0\r\n\r\n".to_vec(),
        Expected::Text(Vec::new()),
    ));

    // The bulk control grammar decides malformed text before declared magnitude, but a plausible
    // 65th control digit wins before any later malformed byte. The huge line is syntactically valid
    // and exceeds both i64 and the configured cap without overflowing parser arithmetic.
    cases.push(Case::coalesced(
        "bulk-control-exact-64-nonzero",
        1,
        Operation::Get,
        [b"$".as_slice(), "0".repeat(63).as_bytes(), b"1\r\nx\r\n"].concat(),
        Expected::Text(b"x".to_vec()),
    ));
    cases.push(
        Case::coalesced(
            "bulk-control-requested-65",
            1,
            Operation::Get,
            [b"$".as_slice(), "0".repeat(65).as_bytes(), b"X-after-size"].concat(),
            Expected::ResponseTooLarge,
        )
        .consuming(66),
    );
    cases.push(Case::coalesced(
        "bulk-control-invalid-at-65",
        1,
        Operation::Get,
        [b"$".as_slice(), "0".repeat(64).as_bytes(), b"X"].concat(),
        Expected::Protocol,
    ));
    cases.push(Case::coalesced(
        "bulk-over-cap-valid",
        8,
        Operation::Get,
        b"$9\r\n".to_vec(),
        Expected::ResponseTooLarge,
    ));
    cases.push(Case::coalesced(
        "bulk-over-cap-malformed-byte",
        8,
        Operation::Get,
        b"$9X".to_vec(),
        Expected::Protocol,
    ));
    cases.push(Case::coalesced(
        "bulk-over-cap-malformed-lf",
        8,
        Operation::Get,
        b"$9\rX".to_vec(),
        Expected::Protocol,
    ));
    cases.push(Case::coalesced(
        "bulk-over-i64-syntactically-valid",
        8,
        Operation::Get,
        b"$9223372036854775808\r\n".to_vec(),
        Expected::ResponseTooLarge,
    ));

    // SET and DEL branch-local completed-frame mutations and same-read trailing bytes.
    cases.push(
        Case::coalesced(
            "set-ok-trailing",
            8,
            Operation::SetAlways,
            b"+OK\r\nX".to_vec(),
            Expected::Protocol,
        )
        .consuming(5),
    );
    for (label, reply) in [
        ("set-ok-wrong-k", b"+OX".as_slice()),
        ("set-ok-missing-cr", b"+OKX".as_slice()),
        ("set-ok-cr-non-lf", b"+OK\rX".as_slice()),
        ("set-alternate-simple", b"+NO\r\n".as_slice()),
    ] {
        let consumed = if label == "set-alternate-simple" {
            2
        } else {
            reply.len()
        };
        cases.push(
            Case::coalesced(
                label,
                8,
                Operation::SetAlways,
                reply.to_vec(),
                Expected::Protocol,
            )
            .consuming(consumed),
        );
    }
    cases.push(
        Case::coalesced(
            "delete-integer-trailing",
            8,
            Operation::Delete,
            b":1\r\nX".to_vec(),
            Expected::Protocol,
        )
        .consuming(4),
    );
    for (label, reply) in [
        ("delete-integer-missing-cr", b":1X".as_slice()),
        ("delete-integer-cr-non-lf", b":1\rX".as_slice()),
    ] {
        cases.push(Case::coalesced(
            label,
            8,
            Operation::Delete,
            reply.to_vec(),
            Expected::Protocol,
        ));
    }

    add_null_mutations(&mut cases);

    // Official DEL spellings and all value/control boundaries.
    for (label, reply, value) in [
        ("delete-zero", b":0\r\n".as_slice(), false),
        ("delete-plus-zero", b":+0\r\n".as_slice(), false),
        ("delete-minus-zero", b":-0\r\n".as_slice(), false),
        ("delete-leading-zero", b":0000\r\n".as_slice(), false),
        ("delete-one", b":1\r\n".as_slice(), true),
        ("delete-leading-one", b":0001\r\n".as_slice(), true),
    ] {
        cases.push(Case::coalesced(
            label,
            8,
            Operation::Delete,
            reply.to_vec(),
            Expected::Bool(value),
        ));
    }
    cases.push(Case::coalesced(
        "delete-control-exact-64",
        8,
        Operation::Delete,
        [b":".as_slice(), "0".repeat(63).as_bytes(), b"1\r\n"].concat(),
        Expected::Bool(true),
    ));
    cases.push(Case::fragmented(
        "delete-signed-control-exact-64",
        8,
        Operation::Delete,
        vec![
            [b":+".as_slice(), "0".repeat(62).as_bytes()].concat(),
            b"1\r".to_vec(),
            b"\n".to_vec(),
        ],
        Expected::Bool(true),
    ));
    cases.push(
        Case::coalesced(
            "delete-control-requested-65",
            8,
            Operation::Delete,
            [b":".as_slice(), "0".repeat(65).as_bytes(), b"X-after-size"].concat(),
            Expected::ResponseTooLarge,
        )
        .consuming(66),
    );
    cases.push(Case::coalesced(
        "delete-control-invalid-at-65",
        8,
        Operation::Delete,
        [b":".as_slice(), "0".repeat(64).as_bytes(), b"X"].concat(),
        Expected::Protocol,
    ));
    for (label, reply, consumed) in [
        ("delete-negative", b":-1\r\n".as_slice(), 5),
        ("delete-two", b":2\r\n".as_slice(), 4),
        ("delete-plus-leading-two", b":+0002\r\n".as_slice(), 8),
        (
            "delete-positive-overflow",
            b":9223372036854775808\r\n".as_slice(),
            20,
        ),
        (
            "delete-negative-overflow",
            b":-9223372036854775809\r\n".as_slice(),
            21,
        ),
        (
            "delete-negative-minimum",
            b":-9223372036854775808\r\n".as_slice(),
            23,
        ),
        (
            "delete-positive-maximum",
            b":9223372036854775807\r\n".as_slice(),
            22,
        ),
    ] {
        cases.push(
            Case::coalesced(
                label,
                8,
                Operation::Delete,
                reply.to_vec(),
                Expected::Protocol,
            )
            .consuming(consumed),
        );
    }

    add_marker_sweeps(&mut cases);

    // Exact byte-advance counts on two widely separated payload sizes make the linear-work owner
    // mutation-discriminating rather than a small-input smoke test.
    for length in [256_usize, 8192] {
        let mut frame = format!("${length}\r\n").into_bytes();
        frame.extend(std::iter::repeat_n(b'a', length));
        frame.extend_from_slice(b"\r\n");
        let chunks = if length == 256 {
            vec![frame]
        } else {
            frame.chunks(1024).map(<[u8]>::to_vec).collect()
        };
        cases.push(Case::fragmented(
            format!("linear-bulk-{length}"),
            length as i64,
            Operation::Get,
            chunks,
            Expected::TextLength(length as i64),
        ));
    }

    let mut labels = cases
        .iter()
        .map(|case| case.label.as_str())
        .collect::<Vec<_>>();
    labels.sort_unstable();
    assert!(
        labels.windows(2).all(|pair| pair[0] != pair[1]),
        "parser case labels must be unique",
    );
    cases
}

const ALIGN_PRELUDE: &str = r#"module main
import pkg.kv
import std.process

extern "C" {
  fn align_kv_parser_stub_select_case(text: str, length: i64) -> i64
  fn align_kv_parser_stub_configure(case_index: i64)
  fn align_kv_parser_stub_operation() -> i32
  fn align_kv_parser_stub_cap() -> i64
  fn align_kv_parser_stub_expected_code() -> i32
  fn align_kv_parser_stub_check_text(text: str, length: i64) -> i32
  fn align_kv_parser_stub_reusable() -> i32
  fn align_kv_parser_stub_terminal() -> i32
  fn align_kv_parser_stub_checkpoint_io()
  fn align_kv_parser_stub_io_unchanged() -> i32
  fn align_kv_parser_stub_case_valid() -> i32
}

fn options(cap: i64) -> pkg.kv.ClientOptions = pkg.kv.ClientOptions {
  connect_timeout_ns: 1,
  io_timeout_ns: 1,
  max_response_bytes: cap,
}

fn error_code(error: pkg.kv.Error) -> i32 = match error {
  Server(message) => unsafe {
    if align_kv_parser_stub_check_text(message, message.len()) == 1 { 5 } else { 99 }
  }
  Decode => 6
  ResponseTooLarge => 7
  Protocol => 8
  Closed => 9
  Invalid => 99
  Io(_) => 99
}

fn get_code(result: Result<Option<string>, pkg.kv.Error>) -> i32 = match result {
  Ok(value) => match value {
    None => 1
    Some(text) => unsafe {
      if align_kv_parser_stub_check_text(text, text.len()) == 1 { 2 } else { 99 }
    }
  }
  Err(error) => error_code(error)
}

fn bool_code(result: Result<bool, pkg.kv.Error>) -> i32 = match result {
  Ok(value) => if value { 4 } else { 3 }
  Err(error) => error_code(error)
}

fn run_case(case_index: i64) -> i32 {
  unsafe { align_kv_parser_stub_configure(case_index) }
  operation := unsafe { align_kv_parser_stub_operation() }
  expected := unsafe { align_kv_parser_stub_expected_code() }
  cap := unsafe { align_kv_parser_stub_cap() }
  key_builder := builder()
  key_builder.write("case-")
  key_builder.write_int(case_index)
  key := key_builder.to_string()
  mut owner := pkg.kv.connect("stub", case_index + 1, options(cap)) else { return 1 }
  actual := if operation == 0 {
    get_code(pkg.kv.get(owner, key))
  } else if operation == 1 {
    bool_code(pkg.kv.set(owner, key, "v", pkg.kv.SetOptions {
      condition: pkg.kv.SetCondition.Always,
      expires_in_ns: None,
    }))
  } else if operation == 2 {
    bool_code(pkg.kv.set(owner, key, "v", pkg.kv.SetOptions {
      condition: pkg.kv.SetCondition.IfAbsent,
      expires_in_ns: None,
    }))
  } else if operation == 3 {
    bool_code(pkg.kv.delete(owner, key))
  } else {
    return 1
  }
  if actual != expected { return 2 }

  reusable := unsafe { align_kv_parser_stub_reusable() }
  terminal := unsafe { align_kv_parser_stub_terminal() }
  if reusable == 1 {
    if get_code(pkg.kv.get(owner, "reuse")) != 1 { return 3 }
  } else if terminal == 1 {
    unsafe { align_kv_parser_stub_checkpoint_io() }
    if get_code(pkg.kv.get(owner, "closed-must-not-write")) != 9 { return 3 }
    unsafe { if align_kv_parser_stub_io_unchanged() != 1 { return 4 } }
  }
  unsafe { if align_kv_parser_stub_case_valid() != 1 { return 5 } }
  return 0
}
"#;

fn build_align_main() -> String {
    let mut source = ALIGN_PRELUDE.to_owned();
    source.push_str("\npub fn main(args: array<str>) -> Result<(), Error> {\n");
    source.push_str("  if args.len() != 2 { process.abort() }\n");
    source.push_str(
        "  case_index := unsafe { align_kv_parser_stub_select_case(args[1], args[1].len()) }\n",
    );
    source.push_str("  if case_index < 0 || run_case(case_index) != 0 { process.abort() }\n");
    source.push_str("  return Ok(())\n}\n");
    source
}

fn write_c_bytes(source: &mut String, name: &str, value: &[u8]) {
    assert!(!value.is_empty(), "C byte arrays must be nonempty");
    writeln!(source, "static const uint8_t {name}[] = {{").unwrap();
    for row in value.chunks(16) {
        source.push_str("    ");
        for byte in row {
            write!(source, "0x{byte:02x}, ").unwrap();
        }
        source.push('\n');
    }
    source.push_str("};\n");
}

fn build_c_fixture(cases: &[Case], inventory: ParserWorkInventory) -> String {
    let max_write_bytes = cases
        .iter()
        .enumerate()
        .map(|(index, case)| case.expected_writes(index).len())
        .max()
        .expect("nonempty parser table");
    let mut source = format!(
        r#"#include <stdint.h>
#include <stddef.h>
#include <string.h>

#define READ_CAPACITY 32768
#define MAX_WRITE_BYTES {}
#define WORK_SITE_COUNT {}

typedef struct {{
    const uint8_t *data;
    int64_t length;
}} Chunk;

typedef struct {{
    const uint8_t *expected_write;
    int64_t expected_write_length;
    const uint8_t *expected_text;
    int64_t expected_text_length;
    const Chunk *chunks;
    int64_t chunk_count;
    int64_t expected_reads;
    int64_t expected_steps;
    int64_t max_work_site_calls;
    int64_t expected_wire_operations;
    int64_t cap;
    int32_t operation;
    int32_t expected_code;
    int32_t reusable;
    int32_t terminal;
}} Script;

"#,
        max_write_bytes + 1,
        inventory.runtime_hooks,
    );

    for (case_index, case) in cases.iter().enumerate() {
        let expected_write = case.expected_writes(case_index);
        write_c_bytes(
            &mut source,
            &format!("case_{case_index}_write"),
            &expected_write,
        );
        let expected_text = case.expected.expected_text();
        if !expected_text.is_empty() {
            write_c_bytes(
                &mut source,
                &format!("case_{case_index}_text"),
                &expected_text,
            );
        }
        let chunks = case.scripted_chunks();
        for (chunk_index, chunk) in chunks.iter().enumerate() {
            write_c_bytes(
                &mut source,
                &format!("case_{case_index}_chunk_{chunk_index}"),
                chunk,
            );
        }
        if !chunks.is_empty() {
            writeln!(source, "static const Chunk case_{case_index}_chunks[] = {{").unwrap();
            for (chunk_index, chunk) in chunks.iter().enumerate() {
                writeln!(
                    source,
                    "    {{ case_{case_index}_chunk_{chunk_index}, {} }},",
                    chunk.len(),
                )
                .unwrap();
            }
            source.push_str("};\n");
        }
    }

    source.push_str("\nstatic const Script scripts[] = {\n");
    for (case_index, case) in cases.iter().enumerate() {
        let chunks = case.scripted_chunks();
        let chunk_pointer = if chunks.is_empty() {
            "NULL".to_owned()
        } else {
            format!("case_{case_index}_chunks")
        };
        let expected_text = case.expected.expected_text();
        let text_pointer = if expected_text.is_empty() {
            "NULL".to_owned()
        } else {
            format!("case_{case_index}_text")
        };
        writeln!(
            source,
            "    {{ case_{case_index}_write, {}, {text_pointer}, {}, {chunk_pointer}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {} }},",
            case.expected_writes(case_index).len(),
            expected_text.len(),
            chunks.len(),
            case.expected_reads(),
            case.expected_steps(),
            case.parser_site_visit_bound(),
            case.expected_wire_operations(),
            case.cap,
            case.operation.code(),
            case.expected.code(),
            i32::from(case.expected.reusable_error()),
            i32::from(case.expected.terminal_error()),
        )
        .unwrap();
    }
    source.push_str("};\n");

    source.push_str(
        r#"
typedef struct {
    int64_t length;
    uint8_t bytes[READ_CAPACITY];
} Buffer;

static uint8_t connection_storage;
static uint8_t reader_storage;
static uint8_t writer_storage;
static void *const connection_token = &connection_storage;
static void *const reader_token = &reader_storage;
static void *const writer_token = &writer_storage;
static Buffer buffer;
static int32_t buffer_in_use;

static const Script *current;
static int64_t next_chunk;
static int64_t written_length;
static uint8_t written[MAX_WRITE_BYTES];
static int64_t connect_calls;
static int64_t timeout_calls;
static int64_t conn_reader_calls;
static int64_t conn_writer_calls;
static int64_t conn_free_calls;
static int64_t reader_free_calls;
static int64_t writer_free_calls;
static int64_t buffer_new_calls;
static int64_t buffer_free_calls;
static int64_t reader_read_calls;
static int64_t writer_write_calls;
static int64_t byte_advance_calls;
static int64_t work_calls;
static int64_t work_site_calls[WORK_SITE_COUNT];
static int64_t checkpoint_reads;
static int64_t checkpoint_writes;
static int64_t checkpoint_byte_advances;
static int64_t checkpoint_work;
static int32_t checkpoint_set;
static int32_t protocol_errors;

int64_t align_kv_parser_stub_select_case(const uint8_t *text, int64_t length) {
    if (text == NULL || length <= 0 || length > 19) {
        return -1;
    }
    int64_t value = 0;
    for (int64_t index = 0; index < length; index += 1) {
        if (text[index] < '0' || text[index] > '9') {
            return -1;
        }
        int64_t digit = (int64_t)(text[index] - '0');
        if (value > (INT64_MAX - digit) / 10) {
            return -1;
        }
        value = value * 10 + digit;
    }
    if (value < 0 || value >= (int64_t)(sizeof(scripts) / sizeof(scripts[0]))) {
        return -1;
    }
    return value;
}

void align_kv_parser_stub_configure(int64_t case_index) {
    current = NULL;
    next_chunk = 0;
    written_length = 0;
    connect_calls = 0;
    timeout_calls = 0;
    conn_reader_calls = 0;
    conn_writer_calls = 0;
    conn_free_calls = 0;
    reader_free_calls = 0;
    writer_free_calls = 0;
    buffer_new_calls = 0;
    buffer_free_calls = 0;
    reader_read_calls = 0;
    writer_write_calls = 0;
    byte_advance_calls = 0;
    work_calls = 0;
    memset(work_site_calls, 0, sizeof(work_site_calls));
    checkpoint_reads = 0;
    checkpoint_writes = 0;
    checkpoint_byte_advances = 0;
    checkpoint_work = 0;
    checkpoint_set = 0;
    protocol_errors = 0;
    buffer_in_use = 0;
    buffer.length = 0;
    if (case_index < 0
        || case_index >= (int64_t)(sizeof(scripts) / sizeof(scripts[0]))) {
        protocol_errors += 1;
        return;
    }
    current = &scripts[case_index];
}

int32_t align_kv_parser_stub_operation(void) {
    return current == NULL ? -1 : current->operation;
}

int64_t align_kv_parser_stub_cap(void) {
    return current == NULL ? -1 : current->cap;
}

int32_t align_kv_parser_stub_expected_code(void) {
    return current == NULL ? -1 : current->expected_code;
}

int32_t align_kv_parser_stub_reusable(void) {
    return current == NULL ? 0 : current->reusable;
}

int32_t align_kv_parser_stub_terminal(void) {
    return current == NULL ? 0 : current->terminal;
}

int32_t align_kv_parser_stub_check_text(const uint8_t *text, int64_t length) {
    if (current == NULL || length != current->expected_text_length) {
        return 0;
    }
    if (length == 0) {
        return 1;
    }
    return text != NULL
        && current->expected_text != NULL
        && memcmp(text, current->expected_text, (size_t)length) == 0;
}

int32_t align_kv_parser_stub_tcp_connect(
    const uint8_t *host,
    int64_t host_length,
    int64_t port,
    int64_t timeout_ns,
    void **output
) {
    connect_calls += 1;
    if (current == NULL || host == NULL || host_length != 4
        || memcmp(host, "stub", 4) != 0 || port < 1 || port > 65535
        || timeout_ns != 1 || output == NULL) {
        protocol_errors += 1;
    }
    if (output != NULL) {
        *output = connection_token;
    }
    return 0;
}

int32_t align_kv_parser_stub_set_io_timeout(void *connection, int64_t timeout_ns) {
    timeout_calls += 1;
    if (connection != connection_token || timeout_ns != 1) {
        protocol_errors += 1;
    }
    return 0;
}

void *align_kv_parser_stub_conn_reader(void *connection) {
    conn_reader_calls += 1;
    if (connection != connection_token) {
        protocol_errors += 1;
    }
    return reader_token;
}

void *align_kv_parser_stub_conn_writer(void *connection) {
    conn_writer_calls += 1;
    if (connection != connection_token) {
        protocol_errors += 1;
    }
    return writer_token;
}

void align_kv_parser_stub_conn_free(void *connection) {
    conn_free_calls += 1;
    if (connection != connection_token) {
        protocol_errors += 1;
    }
}

void align_kv_parser_stub_reader_free(void *reader) {
    reader_free_calls += 1;
    if (reader != reader_token) {
        protocol_errors += 1;
    }
}

void align_kv_parser_stub_writer_free(void *writer) {
    writer_free_calls += 1;
    if (writer != writer_token) {
        protocol_errors += 1;
    }
}

void *align_kv_parser_stub_buffer_new(int64_t capacity) {
    buffer_new_calls += 1;
    if (capacity != READ_CAPACITY || buffer_in_use) {
        protocol_errors += 1;
    }
    buffer_in_use = 1;
    buffer.length = 0;
    return &buffer;
}

void align_kv_parser_stub_buffer_bytes(void *raw_buffer, void *output) {
    if (raw_buffer != &buffer || output == NULL || !buffer_in_use) {
        protocol_errors += 1;
        return;
    }
    void *pointer = buffer.bytes;
    memcpy(output, &pointer, sizeof(pointer));
    memcpy((uint8_t *)output + 8, &buffer.length, sizeof(buffer.length));
}

int64_t align_kv_parser_stub_buffer_capacity(void *raw_buffer) {
    if (raw_buffer != &buffer || !buffer_in_use) {
        protocol_errors += 1;
    }
    return READ_CAPACITY;
}

void align_kv_parser_stub_buffer_free(void *raw_buffer) {
    buffer_free_calls += 1;
    if (raw_buffer != &buffer || !buffer_in_use) {
        protocol_errors += 1;
    }
    buffer_in_use = 0;
    buffer.length = 0;
}

int64_t align_kv_parser_stub_reader_read(void *reader, void *raw_buffer) {
    reader_read_calls += 1;
    if (reader != reader_token || raw_buffer != &buffer || !buffer_in_use
        || current == NULL) {
        protocol_errors += 1;
        return -2;
    }
    if (next_chunk >= current->chunk_count) {
        buffer.length = 0;
        return 0;
    }
    Chunk chunk = current->chunks[next_chunk];
    next_chunk += 1;
    if (chunk.data == NULL || chunk.length <= 0 || chunk.length > READ_CAPACITY) {
        protocol_errors += 1;
        return -2;
    }
    memcpy(buffer.bytes, chunk.data, (size_t)chunk.length);
    buffer.length = chunk.length;
    return chunk.length;
}

int32_t align_kv_parser_stub_writer_write(
    void *writer,
    const uint8_t *bytes,
    int64_t length
) {
    writer_write_calls += 1;
    if (writer != writer_token || length < 0
        || (length > 0 && bytes == NULL)
        || written_length > MAX_WRITE_BYTES - length) {
        protocol_errors += 1;
        return 2;
    }
    if (length > 0) {
        memcpy(written + written_length, bytes, (size_t)length);
    }
    written_length += length;
    return 0;
}

void align_kv_parser_stub_byte_advance(void) {
    byte_advance_calls += 1;
}

void align_kv_parser_stub_work_increment(int64_t site) {
    if (site < 0 || site >= WORK_SITE_COUNT) {
        protocol_errors += 1;
        return;
    }
    work_calls += 1;
    work_site_calls[site] += 1;
}

void align_kv_parser_stub_checkpoint_io(void) {
    checkpoint_reads = reader_read_calls;
    checkpoint_writes = writer_write_calls;
    checkpoint_byte_advances = byte_advance_calls;
    checkpoint_work = work_calls;
    checkpoint_set = 1;
}

int32_t align_kv_parser_stub_io_unchanged(void) {
    return checkpoint_set
        && checkpoint_reads == reader_read_calls
        && checkpoint_writes == writer_write_calls
        && checkpoint_byte_advances == byte_advance_calls
        && checkpoint_work == work_calls;
}

int32_t align_kv_parser_stub_case_valid(void) {
    if (current == NULL) {
        return 0;
    }
    for (int64_t site = 0; site < WORK_SITE_COUNT; site += 1) {
        if (work_site_calls[site] > current->max_work_site_calls) {
            return 0;
        }
    }
    if (protocol_errors != 0 || buffer_in_use
        || connect_calls != 1 || timeout_calls != 1
        || conn_reader_calls != 1 || conn_writer_calls != 1
        || reader_read_calls != current->expected_reads
        || byte_advance_calls != current->expected_steps
        || buffer_new_calls != current->expected_wire_operations
        || buffer_free_calls != current->expected_wire_operations
        || next_chunk != current->chunk_count
        || writer_write_calls <= 0
        || written_length != current->expected_write_length
        || memcmp(written, current->expected_write, (size_t)written_length) != 0) {
        return 0;
    }
    if (current->terminal) {
        return checkpoint_set
            && conn_free_calls == 1
            && reader_free_calls == 1
            && writer_free_calls == 1;
    }
    return !checkpoint_set
        && conn_free_calls == 0
        && reader_free_calls == 0
        && writer_free_calls == 0;
}
"#,
    );
    source
}

struct BuiltParserExe {
    exe: PathBuf,
    dir: PathBuf,
}

impl Drop for BuiltParserExe {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[derive(Debug)]
struct ProcessOutput {
    status: ExitStatus,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

struct ChildGuard {
    child: Option<std::process::Child>,
    cleanup_deadline: Option<Instant>,
}

impl ChildGuard {
    fn new(child: std::process::Child) -> Self {
        Self {
            child: Some(child),
            cleanup_deadline: None,
        }
    }

    fn child_mut(&mut self) -> &mut std::process::Child {
        self.child.as_mut().expect("armed child guard")
    }

    fn begin_cleanup(&mut self, deadline: Instant) {
        self.cleanup_deadline = Some(deadline);
    }

    fn disarm_reaped(&mut self, _status: ExitStatus) {
        self.child.take();
    }
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        let Some(mut child) = self.child.take() else {
            return;
        };
        // This is the unwind backstop for every operation after spawn. Explicit cleanup installs
        // its absolute deadline here before its first attempt, so unwinding cannot silently extend
        // that bound. A panic before explicit cleanup receives one fresh bounded cleanup window.
        let deadline = self
            .cleanup_deadline
            .unwrap_or_else(|| Instant::now() + PROCESS_CLEANUP_TIMEOUT);
        let _ = kill_process_group_until(&child, deadline);
        let _ = kill_direct_until(&mut child, deadline);
        let _ = reap_child_until(&mut child, deadline);
    }
}

#[derive(Clone, Debug, Default)]
struct DrainedPipe {
    bytes: Vec<u8>,
    error: Option<String>,
    overflow: bool,
    eof: bool,
}

struct PipeDrainState {
    capture: Mutex<DrainedPipe>,
}

struct PipeDrain {
    label: &'static str,
    state: Arc<PipeDrainState>,
    handle: Option<std::thread::JoinHandle<()>>,
    capture: DrainedPipe,
}

#[cfg(unix)]
trait CapturedPipe: Read + Send + std::os::fd::AsRawFd {}

#[cfg(unix)]
impl<T: Read + Send + std::os::fd::AsRawFd> CapturedPipe for T {}

#[cfg(not(unix))]
trait CapturedPipe: Read + Send {}

#[cfg(not(unix))]
impl<T: Read + Send> CapturedPipe for T {}

fn sleep_until(deadline: Instant, interval: Duration) {
    let remaining = deadline.saturating_duration_since(Instant::now());
    if !remaining.is_zero() {
        std::thread::sleep(remaining.min(interval));
    }
}

#[cfg(unix)]
fn set_pipe_nonblocking(
    pipe: &impl CapturedPipe,
    deadline: Instant,
    pipe_label: &str,
) -> Result<(), String> {
    let fd = pipe.as_raw_fd();
    let flags = loop {
        // SAFETY: `fd` belongs to the live child pipe, and F_GETFL does not use a third argument.
        let result = unsafe { libc::fcntl(fd, libc::F_GETFL) };
        if result >= 0 {
            break result;
        }
        let error = std::io::Error::last_os_error();
        if error.kind() == std::io::ErrorKind::Interrupted && Instant::now() < deadline {
            continue;
        }
        return Err(format!(
            "make {pipe_label} nonblocking (get flags): {error}"
        ));
    };
    loop {
        // SAFETY: `fd` belongs to the live child pipe, and the existing flags remain valid with
        // O_NONBLOCK added.
        let result = unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) };
        if result >= 0 {
            return Ok(());
        }
        let error = std::io::Error::last_os_error();
        if error.kind() == std::io::ErrorKind::Interrupted && Instant::now() < deadline {
            continue;
        }
        return Err(format!(
            "make {pipe_label} nonblocking (set flags): {error}"
        ));
    }
}

#[cfg(not(unix))]
fn set_pipe_nonblocking(
    _pipe: &impl CapturedPipe,
    _deadline: Instant,
    _pipe_label: &str,
) -> Result<(), String> {
    Ok(())
}

fn update_pipe_capture(state: &PipeDrainState, update: impl FnOnce(&mut DrainedPipe)) {
    let mut capture = state
        .capture
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    update(&mut capture);
}

fn start_pipe_drain(
    mut pipe: impl CapturedPipe + 'static,
    cancel: Arc<AtomicBool>,
    setup_deadline: Instant,
    pipe_label: &'static str,
) -> Result<PipeDrain, String> {
    set_pipe_nonblocking(&pipe, setup_deadline, pipe_label)?;
    let state = Arc::new(PipeDrainState {
        capture: Mutex::new(DrainedPipe::default()),
    });
    let worker_state = Arc::clone(&state);
    let handle = std::thread::Builder::new()
        .name(format!("align-{pipe_label}-drain"))
        .spawn(move || {
            let mut chunk = [0_u8; 4096];
            loop {
                if cancel.load(Ordering::Acquire) {
                    break;
                }
                match pipe.read(&mut chunk) {
                    Ok(0) => {
                        update_pipe_capture(&worker_state, |capture| capture.eof = true);
                        break;
                    }
                    Ok(count) => update_pipe_capture(&worker_state, |capture| {
                        let retained =
                            count.min(MAX_CAPTURE_BYTES.saturating_sub(capture.bytes.len()));
                        capture.bytes.extend_from_slice(&chunk[..retained]);
                        capture.overflow |= retained != count;
                    }),
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(CHILD_POLL_INTERVAL);
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                    Err(error) => {
                        update_pipe_capture(&worker_state, |capture| {
                            capture.error = Some(error.to_string());
                        });
                        break;
                    }
                }
            }
        })
        .map_err(|error| format!("spawn {pipe_label} drain: {error}"))?;
    Ok(PipeDrain {
        label: pipe_label,
        state,
        handle: Some(handle),
        capture: DrainedPipe::default(),
    })
}

fn join_finished_pipe_drains(drains: &mut [PipeDrain], issues: &mut Vec<String>) {
    for drain in drains {
        let finished = drain
            .handle
            .as_ref()
            .is_some_and(std::thread::JoinHandle::is_finished);
        if !finished {
            continue;
        }
        let Some(handle) = drain.handle.take() else {
            continue;
        };
        if handle.join().is_err() {
            issues.push(format!("{} drain thread panicked", drain.label));
        }
    }
}

fn wait_for_pipe_drains_until(
    drains: &mut [PipeDrain],
    deadline: Instant,
    issues: &mut Vec<String>,
) {
    loop {
        join_finished_pipe_drains(drains, issues);
        if drains.iter().all(|drain| drain.handle.is_none()) || Instant::now() >= deadline {
            return;
        }
        sleep_until(deadline, CHILD_POLL_INTERVAL);
    }
}

fn finish_pipe_drains(
    drains: &mut [PipeDrain],
    cancel: &AtomicBool,
    cleanup_deadline: Instant,
) -> Vec<String> {
    let mut issues = Vec::new();
    let now = Instant::now();
    let eof_deadline = cleanup_deadline
        .checked_sub(DRAIN_CANCEL_RESERVE)
        .unwrap_or(now)
        .max(now);
    wait_for_pipe_drains_until(drains, eof_deadline, &mut issues);
    cancel.store(true, Ordering::Release);
    wait_for_pipe_drains_until(drains, cleanup_deadline, &mut issues);
    join_finished_pipe_drains(drains, &mut issues);

    for drain in drains {
        if drain.handle.is_some() {
            issues.push(format!(
                "{} drain thread exceeded the cleanup deadline",
                drain.label
            ));
        }
        drain.capture = match drain.state.capture.try_lock() {
            Ok(capture) => capture.clone(),
            Err(TryLockError::Poisoned(poisoned)) => {
                issues.push(format!("{} capture mutex was poisoned", drain.label));
                poisoned.into_inner().clone()
            }
            Err(TryLockError::WouldBlock) => {
                issues.push(format!(
                    "{} capture unavailable without blocking at cleanup deadline",
                    drain.label
                ));
                DrainedPipe::default()
            }
        };
        if let Some(error) = &drain.capture.error {
            issues.push(format!("{} capture failed: {error}", drain.label));
        }
        if drain.capture.overflow {
            issues.push(format!(
                "{} capture exceeded its {MAX_CAPTURE_BYTES}-byte cap",
                drain.label
            ));
        }
        if !drain.capture.eof {
            issues.push(format!("{} drain did not reach EOF", drain.label));
        }
    }
    issues
}

fn isolate_process_group(command: &mut Command) {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
}

fn kill_process_group_until(child: &std::process::Child, deadline: Instant) -> Option<String> {
    #[cfg(unix)]
    let result = match i32::try_from(child.id()) {
        Ok(pid) => loop {
            // SAFETY: the child was placed in a fresh process group whose id is its positive pid.
            let result = unsafe { libc::kill(-pid, libc::SIGKILL) };
            if result == 0 {
                break None;
            }
            let error = std::io::Error::last_os_error();
            if error.raw_os_error() == Some(libc::ESRCH) {
                break None;
            }
            if error.kind() == std::io::ErrorKind::Interrupted && Instant::now() < deadline {
                continue;
            }
            break Some(error.to_string());
        },
        Err(error) => Some(format!("child pid is not representable as i32: {error}")),
    };
    #[cfg(not(unix))]
    let result = {
        let _ = (child, deadline);
        None
    };

    result
}

struct DirectKillFailure {
    message: String,
    exited_race: bool,
}

fn kill_direct_until(
    child: &mut std::process::Child,
    deadline: Instant,
) -> Option<DirectKillFailure> {
    loop {
        match child.kill() {
            Ok(()) => return None,
            Err(error)
                if error.kind() == std::io::ErrorKind::Interrupted && Instant::now() < deadline => {
            }
            Err(error) => {
                let exited_race = matches!(
                    error.kind(),
                    std::io::ErrorKind::InvalidInput | std::io::ErrorKind::NotFound
                ) || {
                    #[cfg(unix)]
                    {
                        error.raw_os_error() == Some(libc::ESRCH)
                    }
                    #[cfg(not(unix))]
                    {
                        false
                    }
                };
                return Some(DirectKillFailure {
                    message: error.to_string(),
                    exited_race,
                });
            }
        }
    }
}

fn reap_child_until(
    child: &mut std::process::Child,
    deadline: Instant,
) -> Result<ExitStatus, String> {
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status),
            Ok(None) if Instant::now() >= deadline => {
                return Err("reap exceeded the cleanup deadline".to_owned());
            }
            Ok(None) => sleep_until(deadline, CHILD_POLL_INTERVAL),
            Err(error)
                if error.kind() == std::io::ErrorKind::Interrupted && Instant::now() < deadline => {
            }
            Err(error) => return Err(format!("reap poll failed: {error}")),
        }
    }
}

struct ChildCleanup {
    status: Option<ExitStatus>,
    issues: Vec<String>,
}

fn cleanup_child_until(
    child: &mut std::process::Child,
    initial_status: Option<ExitStatus>,
    deadline: Instant,
) -> ChildCleanup {
    let group_error = kill_process_group_until(child, deadline);
    let direct_error = kill_direct_until(child, deadline);
    let mut issues = Vec::new();
    let status = match initial_status {
        Some(status) => Some(status),
        None => match reap_child_until(child, deadline) {
            Ok(status) => Some(status),
            Err(error) => {
                issues.push(error);
                None
            }
        },
    };
    if let Some(error) = group_error {
        issues.push(format!("process-group kill failed: {error}"));
    }
    if let Some(error) = direct_error
        && !(error.exited_race && status.is_some())
    {
        issues.push(format!("direct kill failed: {}", error.message));
    }
    ChildCleanup { status, issues }
}

enum ChildPrimary {
    Exited(ExitStatus),
    TimedOut,
    PollFailed(String),
}

fn poll_child(child: &mut std::process::Child, timeout: Duration) -> ChildPrimary {
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return ChildPrimary::Exited(status),
            Ok(None) if Instant::now() >= deadline => return ChildPrimary::TimedOut,
            Ok(None) => sleep_until(deadline, CHILD_POLL_INTERVAL),
            Err(error)
                if error.kind() == std::io::ErrorKind::Interrupted && Instant::now() < deadline => {
            }
            Err(error) => return ChildPrimary::PollFailed(error.to_string()),
        }
    }
}

fn drain_text(drains: &[PipeDrain], label: &str) -> String {
    drains
        .iter()
        .find(|drain| drain.label == label)
        .map(|drain| String::from_utf8_lossy(&drain.capture.bytes).into_owned())
        .unwrap_or_default()
}

fn cleanup_setup_failure(
    child: &mut ChildGuard,
    drains: &mut [PipeDrain],
    cancel: &AtomicBool,
    command_label: &str,
    setup_error: &str,
) -> ! {
    let cleanup_deadline = Instant::now() + PROCESS_CLEANUP_TIMEOUT;
    child.begin_cleanup(cleanup_deadline);
    let mut cleanup = cleanup_child_until(child.child_mut(), None, cleanup_deadline);
    cleanup
        .issues
        .extend(finish_pipe_drains(drains, cancel, cleanup_deadline));
    if let Some(status) = cleanup.status {
        child.disarm_reaped(status);
    }
    panic!(
        "{command_label}: {setup_error}; cleanup: {}; stdout: {}; stderr: {}",
        cleanup.issues.join("; "),
        drain_text(drains, "stdout"),
        drain_text(drains, "stderr"),
    );
}

fn run_command_bounded(
    command: &mut Command,
    timeout: Duration,
    command_label: &str,
) -> ProcessOutput {
    isolate_process_group(command);
    let cancel = Arc::new(AtomicBool::new(false));
    let mut drains = Vec::with_capacity(2);
    let mut child = ChildGuard::new(
        command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap_or_else(|error| panic!("cannot spawn {command_label}: {error}")),
    );
    let (stdout, stderr) = match (
        child.child_mut().stdout.take(),
        child.child_mut().stderr.take(),
    ) {
        (Some(stdout), Some(stderr)) => (stdout, stderr),
        (stdout, stderr) => {
            drop(stdout);
            drop(stderr);
            cleanup_setup_failure(
                &mut child,
                &mut drains,
                cancel.as_ref(),
                command_label,
                "configured pipe missing",
            );
        }
    };
    let setup_deadline = Instant::now() + PIPE_SETUP_TIMEOUT;
    match start_pipe_drain(stdout, Arc::clone(&cancel), setup_deadline, "stdout") {
        Ok(drain) => drains.push(drain),
        Err(error) => {
            drop(stderr);
            cleanup_setup_failure(
                &mut child,
                &mut drains,
                cancel.as_ref(),
                command_label,
                &error,
            );
        }
    }
    match start_pipe_drain(stderr, Arc::clone(&cancel), setup_deadline, "stderr") {
        Ok(drain) => drains.push(drain),
        Err(error) => cleanup_setup_failure(
            &mut child,
            &mut drains,
            cancel.as_ref(),
            command_label,
            &error,
        ),
    }

    let primary = poll_child(child.child_mut(), timeout);
    let (initial_status, failure) = match primary {
        ChildPrimary::Exited(status) => (Some(status), None),
        ChildPrimary::TimedOut => (None, Some(format!("exceeded its {timeout:?} deadline"))),
        ChildPrimary::PollFailed(error) => (None, Some(format!("poll failed: {error}"))),
    };
    let cleanup_deadline = Instant::now() + PROCESS_CLEANUP_TIMEOUT;
    child.begin_cleanup(cleanup_deadline);
    let mut cleanup = cleanup_child_until(child.child_mut(), initial_status, cleanup_deadline);
    cleanup.issues.extend(finish_pipe_drains(
        &mut drains,
        cancel.as_ref(),
        cleanup_deadline,
    ));
    if let Some(status) = cleanup.status {
        child.disarm_reaped(status);
    }
    if cleanup.status.is_none() {
        cleanup
            .issues
            .push("child has no exit status after cleanup".to_owned());
    }
    let stdout = drain_text(&drains, "stdout");
    let stderr = drain_text(&drains, "stderr");
    if let Some(failure) = failure {
        panic!(
            "{command_label} {failure}; cleanup: {}; stdout: {stdout}; stderr: {stderr}",
            cleanup.issues.join("; "),
        );
    }
    assert!(
        cleanup.issues.is_empty(),
        "{command_label}: cleanup failed: {}; stdout: {stdout}; stderr: {stderr}",
        cleanup.issues.join("; "),
    );
    ProcessOutput {
        status: cleanup
            .status
            .expect("successful bounded cleanup has an exit status"),
        stdout: drains[0].capture.bytes.clone(),
        stderr: drains[1].capture.bytes.clone(),
    }
}

fn cc_available_bounded() -> bool {
    let mut command = Command::new("cc");
    command
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    isolate_process_group(&mut command);
    let mut child = ChildGuard::new(match command.spawn() {
        Ok(child) => child,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return false,
        Err(error) => panic!("cannot spawn parser C compiler probe: {error}"),
    });
    let primary = poll_child(child.child_mut(), CC_PROBE_TIMEOUT);
    let (initial_status, failure) = match primary {
        ChildPrimary::Exited(status) => (Some(status), None),
        ChildPrimary::TimedOut => (
            None,
            Some(format!("exceeded its {CC_PROBE_TIMEOUT:?} deadline")),
        ),
        ChildPrimary::PollFailed(error) => (None, Some(format!("poll failed: {error}"))),
    };
    let cleanup_deadline = Instant::now() + PROCESS_CLEANUP_TIMEOUT;
    child.begin_cleanup(cleanup_deadline);
    let cleanup = cleanup_child_until(child.child_mut(), initial_status, cleanup_deadline);
    if let Some(status) = cleanup.status {
        child.disarm_reaped(status);
    }
    if failure.is_some() || !cleanup.issues.is_empty() || cleanup.status.is_none() {
        panic!(
            "parser C compiler probe {}; cleanup: {}",
            failure.unwrap_or_else(|| "cleanup failed".to_owned()),
            cleanup.issues.join("; "),
        );
    }
    let status = cleanup
        .status
        .expect("successful bounded compiler probe cleanup has an exit status");
    assert!(
        status.success(),
        "parser C compiler probe failed as {status}"
    );
    true
}

fn link_env_count(name: &str) -> usize {
    std::env::var(name)
        .unwrap_or_else(|error| panic!("read parser link-child `{name}`: {error}"))
        .parse()
        .unwrap_or_else(|error| panic!("parse parser link-child `{name}`: {error}"))
}

#[test]
fn pkg_kv_parser_link_child() {
    if std::env::var_os(LINK_CHILD_ENV).as_deref() != Some(std::ffi::OsStr::new("1")) {
        return;
    }
    let object_count = link_env_count(LINK_OBJECT_COUNT_ENV);
    let objects = (0..object_count)
        .map(|index| {
            PathBuf::from(
                std::env::var_os(format!("{LINK_CHILD_ENV}_OBJECT_{index}"))
                    .unwrap_or_else(|| panic!("missing parser link-child object {index}")),
            )
        })
        .collect::<Vec<_>>();
    let object_refs = objects.iter().map(PathBuf::as_path).collect::<Vec<_>>();
    let library_count = link_env_count(LINK_LIBRARY_COUNT_ENV);
    let libraries = (0..library_count)
        .map(|index| {
            std::env::var(format!("{LINK_CHILD_ENV}_LIBRARY_{index}"))
                .unwrap_or_else(|error| panic!("read parser link-child library {index}: {error}"))
        })
        .collect::<Vec<_>>();
    let executable = PathBuf::from(
        std::env::var_os(LINK_EXE_ENV).expect("missing parser link-child executable"),
    );
    link_objects(&object_refs, &executable, &libraries, Profile::Release)
        .expect("link pkg.kv parser executable");
}

fn link_objects_bounded(objects: &[PathBuf], exe: &Path, libraries: &[String]) {
    let mut command =
        Command::new(std::env::current_exe().expect("resolve pkg.kv parser test executable"));
    command
        .args(["--exact", "pkg_kv_parser_link_child", "--nocapture"])
        .env(LINK_CHILD_ENV, "1")
        .env(LINK_EXE_ENV, exe)
        .env(LINK_OBJECT_COUNT_ENV, objects.len().to_string())
        .env(LINK_LIBRARY_COUNT_ENV, libraries.len().to_string());
    for (index, object) in objects.iter().enumerate() {
        command.env(format!("{LINK_CHILD_ENV}_OBJECT_{index}"), object);
    }
    for (index, library) in libraries.iter().enumerate() {
        command.env(format!("{LINK_CHILD_ENV}_LIBRARY_{index}"), library);
    }
    let output = run_command_bounded(
        &mut command,
        PROCESS_TIMEOUT,
        "link pkg.kv parser executable",
    );
    assert!(
        output.status.success(),
        "pkg.kv parser executable link failed as {}; stdout `{}`; stderr `{}`",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

fn build_parser_exe(files: &[(&str, &str)], c_source: &str) -> BuiltParserExe {
    static NONCE: AtomicU64 = AtomicU64::new(0);
    let nonce = NONCE.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "align-pkg-kv-parser-{}-{nonce}",
        std::process::id()
    ));
    std::fs::create_dir(&dir).expect("create unique parser-project directory");
    // Arm cleanup before the first later fallible operation. `create_dir` rejects a stale path, so
    // PID reuse cannot silently compile against remnants from a prior failed process.
    let built = BuiltParserExe {
        exe: dir.join(format!("pkg-kv-parser{}", std::env::consts::EXE_SUFFIX)),
        dir,
    };
    for &(name, source) in files {
        let path = built.dir.join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create parser-project module directory");
        }
        std::fs::write(path, source).expect("write parser-project source");
    }

    let entry = built.dir.join("main.align");
    let entry_source = std::fs::read_to_string(&entry).expect("read parser-project entry");
    let mut source_map = SourceMap::new();
    let walk = build_per_unit(&mut source_map, &entry.display().to_string(), &entry_source);
    assert!(
        !walk.diags.has_errors(),
        "unexpected per-unit errors:\n{}",
        align_driver::format_diagnostics(&source_map, &walk.diags),
    );

    let mut objects = Vec::with_capacity(walk.units.len() + 1);
    let mut link_libraries = Vec::new();
    for (index, unit) in walk.units.iter().enumerate() {
        let object = built.dir.join(format!("unit-{index}.o"));
        emit_object_file(
            &unit.mir,
            &object,
            BuildTarget::Baseline,
            Profile::Release,
            &[],
            false,
        )
        .unwrap_or_else(|error| panic!("codegen for unit `{}`: {error}", unit.unit));
        for library in &unit.mir.link_libs {
            if !link_libraries.contains(library) {
                link_libraries.push(library.clone());
            }
        }
        objects.push(object);
    }

    let c_path = built.dir.join("parser.c");
    let c_object = built.dir.join("parser.o");
    std::fs::write(&c_path, c_source).expect("write C parser fixture");
    let compiled = run_command_bounded(
        Command::new("cc")
            .args(["-std=c11", "-c", "-O0"])
            .arg(&c_path)
            .arg("-o")
            .arg(&c_object),
        PROCESS_TIMEOUT,
        "compile pkg.kv parser C fixture",
    );
    assert!(
        compiled.status.success(),
        "C parser fixture failed as {}; stdout `{}`; stderr `{}`",
        compiled.status,
        String::from_utf8_lossy(&compiled.stdout),
        String::from_utf8_lossy(&compiled.stderr),
    );
    objects.push(c_object);

    let ordered_libraries = order_link_libs(&link_libraries);
    link_objects_bounded(&objects, &built.exe, &ordered_libraries);
    built
}

fn run_parser_case(executable: &Path, argument: &str) -> ProcessOutput {
    run_command_bounded(
        Command::new(executable).arg(argument),
        CASE_TIMEOUT,
        argument,
    )
}

#[test]
fn resp_grammar_precedence_fragmentation_eof_and_linear_work_matrix() {
    // Source-shape and work-site tripwires are compiler-independent and must not disappear merely
    // because this host lacks a backend or C toolchain.
    let (root, internal, inventory) = parser_sources();
    let cases = parser_cases();
    assert_eq!(cases.len(), 288, "the parser closure table changed");
    for length in [256_usize, 8192] {
        let case = cases
            .iter()
            .find(|case| case.label == format!("linear-bulk-{length}"))
            .unwrap_or_else(|| panic!("missing exact linear-work witness {length}"));
        let expected_steps = format!("${length}\r\n").len() + length + 2;
        assert_eq!(case.cap, length as i64);
        assert_eq!(case.expected_first_steps, expected_steps);
        assert_eq!(case.expected_steps(), expected_steps);
        assert_eq!(
            case.parser_site_visit_bound(),
            expected_steps + case.expected_wire_operations() + 1,
        );
        assert!(matches!(case.expected, Expected::TextLength(value) if value == length as i64));
    }
    assert_eq!(
        cases
            .iter()
            .filter(|case| case.label.contains("-eof-"))
            .count(),
        48,
        "every byte ordinal of all eight parser branches",
    );
    assert_eq!(
        cases
            .iter()
            .filter(|case| case.label.starts_with("error-product-"))
            .count(),
        36,
        "3 payload bounds x 3 endings x 2 trailing states x 2 read shapes",
    );
    assert_eq!(
        cases
            .iter()
            .filter(|case| case.label.starts_with("marker-"))
            .count(),
        66,
        "RESP2/RESP3/unknown marker sweep for all commands",
    );
    let complete_branch_product = [
        "get-bulk-",
        "get-error-",
        "get-null-",
        "set-ok-",
        "set-null-",
        "set-error-",
        "delete-integer-",
        "delete-error-",
    ];
    assert_eq!(
        cases
            .iter()
            .filter(|case| {
                complete_branch_product
                    .iter()
                    .any(|prefix| case.label.starts_with(prefix))
                    && !case.label.contains("-eof-")
                    && (case.label.ends_with("-coalesced")
                        || case.label.ends_with("-one-byte")
                        || case.label.contains("-split-")
                        || case.label.ends_with("-multipart"))
            })
            .count(),
        64,
        "coalesced + one-byte + every split + multipart accepted-branch product",
    );
    assert_eq!(
        cases
            .iter()
            .filter(|case| case.expected.reusable_error())
            .count(),
        36,
        "every Server/Decode row has one subsequent scripted command",
    );
    assert_eq!(
        cases
            .iter()
            .filter(|case| case.expected.terminal_error())
            .count(),
        200,
        "every terminal row checkpoints zero-I/O later Closed",
    );

    let main = build_align_main();
    let c_source = build_c_fixture(&cases, inventory);
    // Keep every source fingerprint, case cardinality, exact 256/8192 witness, and generated table
    // check above the documented toolchain skip. Only compilation/link/execution needs the backend
    // and C compiler.
    if !backend_available() || !cc_available_bounded() {
        return;
    }
    let files = [
        ("pkg/kv/internal/resource.align", internal.as_str()),
        ("pkg/kv.align", root.as_str()),
        ("main.align", main.as_str()),
    ];
    let built = build_parser_exe(&files, &c_source);
    for (index, case) in cases.iter().enumerate() {
        let argument = index.to_string();
        let output = run_parser_case(&built.exe, &argument);
        assert_eq!(
            output.status.code(),
            Some(0),
            "parser closure case `{}` failed with {}; stdout `{}`; stderr `{}`",
            case.label,
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
    }
    let overflow_selector = run_parser_case(&built.exe, "9999999999999999999");
    assert!(
        !overflow_selector.status.success(),
        "overflowing C case selector must be rejected deterministically; stdout `{}`; stderr `{}`",
        String::from_utf8_lossy(&overflow_selector.stdout),
        String::from_utf8_lossy(&overflow_selector.stderr),
    );
}
