# Escaped-output entry measurement

Run `bash bench/encoding_writes/run.sh` on an idle macOS or Linux host. It builds
the ordinary release runtime, links the C harness against the real native
entries, prints CSV samples, and removes its temporary executable. It honors
`CARGO_TARGET_DIR` and `CC`; no optional native codec/crypto libraries are needed.

Kind 0 is percent/component, 1 percent/path, 2 HTML, and 3 form. Each has plain
and mixed UTF-8 input near 256 bytes, 4KiB and 64KiB; whole repeated seeds keep
UTF-8 valid, so the CSV records actual lengths. Every case checks exact bytes
against an independent oracle before timing. Nine samples include final output
allocation/free and account producer-returned lengths, calls and output bytes.

Compare identical corpora, compilers and flags with alternating baseline and
candidate runs after all local builds/tests finish. Preserve separate statically
linked executables when alternating revisions. Timing is a local measurement,
not a CI assertion or application-throughput guarantee. Plan 123 records the
qualified before/after measurements and the destination initialization proof.
