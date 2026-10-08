# Bounded rejection of excess Base64 padding

Status: **IMPLEMENTED 2026-10-08 — native/source owners and local ARM64 measurements qualified.**

The shared standard/URL-safe decoder counts the entire trailing `=` run before
rejecting more than two pads. No valid input can have three pads, so reject at
the third observed pad. This retains plan125's grammar and ownership strategy.

| Boundary | Invariant and owner |
| --- | --- |
| Admission | Inspect at most three trailing bytes equal to `=` before rejecting excess padding. Keep first-symbol admission, optional zero/one/two padding, remainder checks, alphabet separation and canonical tail bits. Existing independent scalar oracle plus short/long padding products cover both alphabets and all valid remainder classes. |
| Publication and allocation | Excess padding returns existing Invalid with null output, before output allocation; no new error, byte view, allocation, retained input or cleanup path. Native publication and isolated allocation owners cover short/long refusal alongside valid output. |
| Source consumers | Existing m10_encoding owns both source decoders and Result/Drop behavior. No FFI, IR, interface or language contract change. |
| Measurement | Extend the existing independent-encoder probe with excess-padding cases at both long sizes, preserving normal and early/late-invalid controls. Measure identical ordinary release archives in ABBA order, verify complete successful bytes and producer status/count/length per call; retain all samples without a timing gate. |
| Probe ownership | Route the existing Base64 entry through the shared bounded runner. Preserve its source-mtime producer refresh for all shared-cache probes so a Fresh result cannot silently select another worktree's top-level archive. Source bytes stay unchanged; actual stale-archive replacement and existing per-wrapper process lifecycle owners qualify the runner. |

The author-side invariant pass and one inspection-only full-diff review cover
this internal refinement. No separate strategy or public design review is needed.

## Qualification

Seven active Base64 native owners, the separate actual allocation owner and
all 18 m10_encoding source owners pass on macOS/Linux ARM64. The padding product
matches its independent scalar oracle; native long-suffix errors publish no
buffer and allocate no payload. All five probe wrappers pass the shared phase
lifecycle product. The real Rust/C artifact owner replaces both published and
fingerprinted archives while preserving Cargo's successful fingerprint; it
passes with the refresh and fails at execution when that refresh is omitted.

The extended probe retains every final ABBA observation. Roughly 87 KiB of
excess padding changes from about 28 us to 4.8 ns per complete native call on
both measured ARM64 hosts; successful long controls remain within 1.4%.
The worst short increase is 2.17 ns for macOS empty URL-safe input. The probe
README and sample tables retain the controls and limits. No language or
normative encoding specification changes are required.
