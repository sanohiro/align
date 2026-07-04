This directory holds the authoritative per-area design docs for the `core` library, at the same
depth as `../std-design/` (signatures, Move/effect classification, error policy, pitfalls, test
anchors). Authored by the main loop (Fable).

# core — library design docs

> 🌐 **English** · [Japanese](./ja/README.md)

## Why this directory exists

`std` got per-module implementable design specs (`../std-design/`) *before* implementation.
`core` grew the other way: it shipped milestone by milestone (M0–M10), its normative surface is
scattered across `draft.md` (§5 Option/Result, §7 array/slice, §8 data processing, §9 SIMD, §12
string, §13 template, §14 JSON, §18.1 catalog), and §18.1 is a thin name catalog that in places
runs **ahead** of the implementation (e.g. `split`, `json.scan`). These docs close that gap: one
file per core area, recording the **implemented, test-pinned surface** at std-design depth, plus
an explicit *spec'd-but-not-implemented* section so drift is visible instead of ambient.

Precedence: `draft.md` stays the language-level source of truth for *semantics and direction*;
these docs are the source of truth for the **current library surface** — exact signatures,
ownership/effect/region classification, abort-vs-`Result` policy, and which tests pin each
behavior. When implementing or changing a core area, update the matching file here in the same
PR (same rule as std-design).

## The files

- [option-result.md](option-result.md) — `Option<T>` / `Result<T, E>` / builtin `Error`: constructors, `?`, `else`, `map_err`, `main` exit mapping
- [array-slice-pipeline.md](array-slice-pipeline.md) — `array<T>` / `slice<T>` / ranges / `out`, and the whole pipeline vocabulary + termination and fusion rules
- [string.md](string.md) — `str` / `string` / `bytes` / `buffer` / `builder` / `template`: methods, concat policy, UTF-8 stance
- [json.md](json.md) — `json.encode` / `json.decode` (struct / array / soa targets), error policy, zero-copy views
- [soa-groupby.md](soa-groupby.md) — `soa<T>`, column ops, `group_by` aggregates, `.agg(...)`, `dict_encode`
- [vec-mask.md](vec-mask.md) — `vecN<T>` / `maskN<T>`, lane ops, `load`/`store`, `select`/`dot`/`fma`/`sum_where`, `align(N)`
- [arena-heap.md](arena-heap.md) — `arena {}` and `heap.new` / `box`: regions, escape, drop
- [hash.md](hash.md) — `core.hash` (`hash64`/`hash128`): status and design

Template per file: **Overview → Signatures (verified) → Type & ownership → Effects → Errors &
aborts → Regions → Spec'd but not implemented → Pitfalls → Test anchors.**
