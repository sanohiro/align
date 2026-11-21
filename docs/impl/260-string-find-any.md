# Literal delimiter search

The media retry parser and LLM text parsers already locate delimiters and
slice at byte offsets. Section 18.1 lists two different capabilities.
Finding the first of several scalar delimiters returns the UTF-8 byte offset of the first matching scalar.
Splitting on a literal separator materializes multiple results and needs
its own allocation and lifetime contract. Each is useful independently.

## Public contract and implementation closure

| Boundary | Contract | Acceptance |
|---|---|---|
| Source | `text.find_any(chars: str) -> Option<i64>`. One required positional argument. Evaluate receiver then argument, once each. Both admit borrowed text and the existing auto-borrow of owned strings. Normal method arity precedes argument-type checking. Pure. | Imported helper in both compilation modes, owned arguments remain usable, wrong arity/type. |
| Match | Return the first UTF-8 byte offset in the receiver whose complete Unicode scalar occurs in the argument. Empty receiver or set and absence give None. Set order, repetitions and embedded NUL carry their literal semantics. No normalization, case folding or locale. | Empty/missing, reordered and repeated delimiters, equal leading/continuation bytes in different scalars, multibyte offsets, supplementary-plane characters and NUL. |
| Storage and control | Read both views through the call. Allocate no heap storage, retain no input and return the ordinary Copy Option. Existing argument invalidation checks, branches, loops, propagation, reassignment, return and Drop follow the two-input string predicate and scalar Option machinery. | Mid-argument invalidation rejection and post-result reuse, a diverging argument with an owned receiver, result control flow and the runtime allocation owner. |
| Compiler and cache | Append one discriminator to the existing string-predicate family. Checked HIR admits only the exact text operands and Option result. MIR preserves the discriminator in printing and canonical identity. Reuse ordinary generic specialization, per-unit interface signatures and compiler-build cache identity. Substring-only finder hoisting must never select this predicate. | Whole/per-unit imported generic, exact MIR name and a loop whose scalar-set answer differs from substring search. |
| Native | `align_rt_str_find_any(ptr, i64, ptr, i64) -> i64`, A27. Return the byte offset or -1. Each positive admitted range must be live readable UTF-8; overlapping inputs are valid. A null, nonpositive or unrepresentable range counts as empty. No output storage, input writes or pointer capture. Short ASCII sets can use memchr dispatch; the ABI records DispatchCache. No physical ISA or throughput promise. | Registry, declaration golden and export parity, zero allocation and direct native boundary cases. |

The authoritative source spelling is shared by draft §12/§18.1, the language
digest and the English/Japanese core string references. The ABI ledger owns the
symbol inventory. This capability leaves all public type layouts intact.

The driver owner is `str_find_any`. Runtime owners are
`str_find_any_matches_scalar_oracle_and_native_boundary` and the existing
`runtime_effect_memory_claims` tests. The checked-HIR and native ABI ledgers,
the declaration golden and `test-runtime-abi-exports.sh` cover the typed
discriminator, native call shape and exported definition.
