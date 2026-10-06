# Keep whole-file fallbacks on the opened descriptor

A single ordinary FIFO writer reproduces a hang in all three whole-file readers:
`fs.read_file`, `fs.read_file_view`, and `fs.read_bytes_view`. On macOS the writer
also receives EPIPE. Linux and macOS probes time out after the writer closes.
Each entry opens the FIFO for metadata, closes that reader, then opens the path
again for fallback. The second open no longer has a writer. This violates the
existing documented special-file fallback behavior; no new filesystem API is
needed.

Keep one successfully acquired `std::fs::File` owner through the entire call.
Arena fallbacks read from that file instead of its pathname. Owned-text reads
keep their direct-allocation regular-file path; when its metadata snapshot no
longer matches, rewind the same regular file before the existing read-to-Vec
fallback. Special/zero-size files have not been consumed and go directly to that
fallback. Missing paths, UTF-8 admission, error status conventions, heap/arena
ownership, mmap admission and the known truncation/SIGBUS limitation stay intact.
No atomic filesystem snapshot or deadline is introduced.

The owned fast path currently passes uninitialized malloc bytes as a Rust Read
buffer. The [Read safety contract](https://doc.rust-lang.org/std/io/trait.Read.html#method.read)
requires initialized input. While restructuring that exact fallback boundary,
fill the direct allocation through the existing native read ABI using raw
pointers, retry EINTR, check each returned count, and form a byte slice only
after every requested byte is initialized. The EOF probe uses initialized local
storage. Retain the owned allocation in an immediately armed cleanup guard until
publication, including short/read-error/UTF-8/fallback exits. Do not add a zero
fill or a second complete payload copy to the ordinary regular-file path.

| Closure axis | Implementation and owner |
| --- | --- |
| Entry admission and error identity | Keep owned-text null/path rejection and existing numeric error convention; view entries still clear output before arena/path admission and retain errno mapping. `entry_admission_preserves_status_and_empty_output` and existing missing/invalid/binary owners cover the entry boundary. |
| One file owner and fallback identity | Open once, retain File through metadata, direct read/mmap, rewind where needed and copy fallback. A native FIFO owner covers all three entries with exactly one writer, empty/text/binary input, EOF and deadline-bound cleanup. An opened-file helper owner unlinks/replaces the original path before fallback and must read the retained file, never a replacement path. |
| Raw initialization and regular-file snapshot changes | A parameterized raw-read owner covers short chunks, EINTR, EOF, failure and impossible returned counts; guard bytes and initialized-prefix tracking bound every write. Ordinary regular-file UTF-8, invalid UTF-8, empty and large input reuse native owners. Explicit expected-size controls exercise shrink/growth and same-file rewind fallback deterministically. |
| Heap and arena publication/cleanup | The owned payload guard frees every abandoned direct buffer and releases only complete validated output. Arena copying happens after complete read and text validation; bytes accepts arbitrary input. Successful mmap registration/invalid-text unmap and arena cleanup remain unchanged. Existing mmap/proc/UTF-8 owners plus native handle cleanup cover both paths. |
| Language consumers | One compiled FIFO owner covers owned text, arena text and arena bytes through existing signatures in whole/per-unit compilation; returned owned text remains independently usable and arena views remain scoped. Existing M9 UTF-8 and arena owners retain control flow/Drop contracts. No IR/type/generic/interface/cache changes. |
| Fixture lifecycle | Exclusively acquired private directory holds every FIFO/source/artifact. Native file/arena/result/process owners are armed before later fallible work. One writer uses bounded nonblocking acquisition/write; reader execution runs in an exact-filter child with an immediate kill/reap guard and one deadline, never a detached blocked thread. |
| Resource scope | Preserve the regular-file direct payload allocation and mmap paths. The isolated `direct_payload_allocation_and_abandoned_buffers_balance` owner counts one final runtime allocation and no Rust temporary allocation for a regular file, and balances every abandoned/final payload across snapshot and invalid-content cases. This is a correctness repair, not a new throughput/RSS claim or a new special-file timeout. |

This is one runtime file-read capability because all three fallbacks share the
same reopened-path defect. The raw initialization repair belongs to the owned
fast path that chooses that fallback. The author matrix pass and a fresh
independent strategy review precede implementation; one independent complete
implementation review and normal owner/gate/Clippy checks precede publication.
The public API and existing language/library mirrors need no normative changes.

Acceptance ownership:

- `crates/align_runtime/src/fs_read_tests.rs`: `raw_fill_admits_only_complete_initialized_prefixes`,
  `owned_snapshot_fallback_rewinds_the_original_file`,
  `arena_fallback_uses_original_descriptor_and_validates_before_allocation`,
  `fifo_readers_complete_after_one_writer_closes`, and the allocation owner above.
- Existing runtime `fs_read_file_fast_path_and_fallbacks`, `fs_read_file_view_*`
  and invalid-UTF-8 owners retain regular-file, empty, missing, mmap and Linux
  `/proc` coverage.
- `crates/align_driver/tests/fs_whole_file.rs` runs the imported consumer in
  whole-program and per-unit modes. `m9_fs` and `m9_path_env_time` own existing
  binary/text byte parity, errors, arena escape rejection and owned return behavior.
