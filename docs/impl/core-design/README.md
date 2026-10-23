# core — library design docs

> 🌐 **English** · [Japanese](./ja/README.md)

## Why this directory exists

These files collect the current `core` library signatures, ownership and effect
rules, errors, and test coverage by area. Each file distinguishes implemented
operations from specified operations that have not shipped, such as string
`split`.

`draft.md` remains authoritative for language semantics and design decisions.
These references give the detailed library contracts and implementation limits.
Update the relevant reference when its public contract changes; an internal
implementation change does not require restating that contract.

## The files

- [option-result.md](option-result.md) — `Option<T>` / `Result<T, E>` / builtin `Error`: constructors, `?`, `else`, `map_err`, `main` exit mapping
- [array-slice-pipeline.md](array-slice-pipeline.md) — `array<T>` / `slice<T>` / ranges / `out`, and the whole pipeline vocabulary + termination and fusion rules
- [string.md](string.md) — `str` / `string` / `bytes` / `buffer` / `builder` / `template`: methods, concat policy, UTF-8 stance
- [json.md](json.md) — `json.encode` / `json.encode_bounded` / `json.decode` (struct / array / soa targets), `json.doc`, streaming `json.scan`, error policy, zero-copy views
- [soa-groupby.md](soa-groupby.md) — `soa<T>`, column ops, `group_by` aggregates, `.agg(...)`, `dict_encode`
- [vec-mask.md](vec-mask.md) — `vecN<T>` / `maskN<T>`, lane ops, `load`/`store`, `select`/`dot`/`fma`/`sum_where`, `align(N)`
- [arena-heap.md](arena-heap.md) — `arena {}` and `heap.new` / `box`: regions, escape, drop
- [hash.md](hash.md) — `core.hash` (`hash64`/`hash128`): status and design
- [test.md](test.md) — implemented `core.test` declarations, assertions, isolated runner protocol, and terse-output policy
- [codec.md](codec.md) — implemented `core.codec` canonical columnar batch envelope, zero-copy views, and encoder
- [json-escape-benchmark-evidence.md](json-escape-benchmark-evidence.md) — trusted native-host
  controller, isolation, report schema, and acceptance boundary for the JSON escape benchmark

Template per file: **Overview → Signatures (verified unless explicitly pending) → Type & ownership → Effects → Errors &
aborts → Regions → Spec'd but not implemented → Pitfalls → Test anchors.**
