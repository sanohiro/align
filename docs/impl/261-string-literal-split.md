# Literal text splitting

The media retry-seed parser and LLM validation readers walk a string, find a
literal comma and slice each field by hand. A literal split can return those
fields directly. It needs no regex compilation. The existing regex operation
returns independent numeric match spans; literal text fields are borrowed views.

## Contract and closure

| Surface | Contract | Implementation and acceptance |
|---|---|---|
| Formation and order | `text.split(separator: str) -> array<str>`. No import or arena required. Exactly one positional argument, no default. Evaluate receiver then separator once; both accept the same str/string coercion as find. Validate arity before checking argument type. Pure. | Source driver in whole and per-unit compilation, including imported generic, owned/borrowed operands, invalid arity/type, evaluation order, and early exit while evaluating separator. |
| Field boundaries | Match literal UTF-8 non-overlapping substrings from left to right. Return the text before, between and after all matches, preserving empty fields. No match gives one field containing all the input. Empty separator matches every Unicode scalar boundary, including both ends. Splitting empty text on the empty separator gives two empty fields. Embedded NUL is ordinary data; no normalization, locale or case folding. | Compare diverse UTF-8, overlapping/repeated separators, empty input, scalar and supplementary encodings and leading/trailing/consecutive delimiters to an independent field oracle. |
| Storage | The Move array owns its independently allocated contiguous str headers; each header views exact original bytes. No per-field allocation or input copy. Ordinary terminal allocation-size/OOM policy for checked representability or allocator exhaustion. No source consumed. The separator is live only through the call; derived views depend on the receiver's backing and UTF-8 validation generation. Replacing the array releases only the headers. | Observe pointer offsets and balanced allocations/free for bind, replace, discard, transfer and early exit. Reject element escape from local owned text, expired arena, mutation through byte alias or expired UTF-8 validation. Accept valid later mutation of an independent delimiter owner, dead-result and static-input cases. |
| Compiler | New HIR/MIR operation carries source and separator, reuses the two-input evaluation and access check and the existing array/view layouts. Explicit individually allocated header provenance (even inside an arena); fresh header contents derive from source only. A temporary owned receiver keeps its drop-flagged backing through the enclosing scope or loop iteration, as for a retained call borrow. Do not attach that backing to the newly allocated spine as a disposable or donatable pipeline temporary. Header donation, release and input lifetime are independent. Replay, generic instantiation, reference-return summaries, task capture and drops use their ordinary owners. | Allocated/freed header and text counts for collection, filtered collection, scalar terminals and loop/early exits with fresh and bound receivers, inside and outside arenas. Conditional and loop joins, selected `match` / `if` / `else`, return, `?` and `map_err` carriers, replacement, whole/per-unit and cache hit; malformed HIR results/operands and malformed MIR operand/result types reject before LLVM. |
| Runtime | `align_rt_str_split(ptr, i64, ptr, i64) -> {ptr, i64}`. A returned header array is owned by Align and released by align_rt_free; the two inputs may overlap. Nonempty views must be readable throughout. Null+zero denotes empty. Publish safe zero-length field views without arithmetic on null. Negative or unrepresentable length, null+positive length and invalid UTF-8 at this unsafe entry return the canonical empty array. Validate before allocating. No I/O, per-process configuration, hidden heap copies, retained separator or extra native handle. | A typed registry row and declaration golden, native range/alias and UTF-8 negatives, pointer/length fields, exactly-once release. Shape A86. Native IndirectStorage, unstated aggregate memory, input ordinal 0 escaping through the fields, separator ordinal 2 noncapturing, aggregate returns_fresh false. Validate malformed lengths before calling helpers which normalize invalid views to empty. No timing/ISA promise. |

The public sources are draft §12 and §18.1, the language digest and English and
Japanese core string docs. The checked-HIR and runtime ABI ledgers own the
operation and native export respectively. The independently useful capability
runs from construction to consumption and cleanup in one PR.

`str_split` drives whole-program and per-unit compilation, input validation,
branch and loop joins, temporary cleanup and allocation/free parity. Native
`str_split_headers_alias_the_input_and_reject_invalid_views` checks exact field
addresses and malformed inputs. The `str_split` HIR and MIR mutation owners
check both arguments and the declared result through their normal validators.
