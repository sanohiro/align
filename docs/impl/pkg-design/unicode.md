# pkg.unicode — explicit Unicode normalization

The vendorable subtree is `apps/unicode/pkg`. Import `pkg.unicode` after copying
that subtree into the application's package search root. The package owns
Unicode 16.0.0 NFC; it does not change byte-based string equality or ordering.

## Surface

```align
pub fn version() -> str
pub fn nfc(input: str) -> Result<string, Error>
```

`version()` returns static `"16.0.0"` without allocation or errors. `nfc` borrows
UTF-8 input and returns an independent owned UTF-8 string. The result never
retains input or an input arena, even when input is already NFC. Ordinary Move,
Result and string Drop rules apply. Empty input returns an empty owned string
without allocation.

NFC applies canonical decomposition, stable canonical combining-class ordering
and canonical composition, including Hangul and full composition exclusions.
Embedded NUL, noncharacters and unassigned Unicode scalars are accepted. There
is no locale dependency, implicit normalization, replacement decoding, arbitrary
input-size cap or stream-safe insertion. Other normalization forms, grapheme
segmentation and case/locale operations are not part of this package.

Temporary storage and final output allocation are fallible. Unrepresentable
input, scratch or output extents return `Error.Invalid`; allocation refusal
returns `Error.Code(ENOMEM)` using the target's native ENOMEM value. Valid source
`str` already guarantees UTF-8; malformed native metadata/text is also Invalid.
No hidden final clone or hard-OOM allocation occurs in the normalization path.
The transform is Pure; ordinary heap allocation in Pure closures remains allowed.

```align
import pkg.unicode

fn normalized() -> Result<string, Error> {
  // The input is e followed by a combining acute accent.
  return pkg.unicode.nfc("é")
}
```

Unicode data is compiled into the runtime, independent of OS and locale. An
upgrade is an explicit package/compiler source change that updates `version()`
and conformance together. Official data, source hashes and Unicode License V3
are retained under `crates/align_runtime/data/unicode-16.0.0`. The offline
`scripts/generate-unicode.py --check` reproduces tables; native owners apply all
five official NFC columns and the Part1 complement identity rule.

The empty `pkg.unicode.internal.descriptor` module is compiler-private. The
canonical wrapper is the only source entry to its operation; application code
cannot call the bridge or declare the runtime symbol. [Plan191](../191-unicode-nfc.md)
owns the exact native ABI, validation order, allocation and compiler closure.
Voice V004's tokenizer adoption and parity check remain consumer-owned.
