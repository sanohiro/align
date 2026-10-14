# Unicode 16.0.0 inputs

Official inputs from https://www.unicode.org/Public/16.0.0/ucd/:

- `UnicodeData.txt`: canonical decomposition and combining classes.
- `DerivedNormalizationProps.txt`: full composition exclusions.
- `NormalizationTest.txt`: independent NFC conformance expectations, including
  the Part1 complement rule in its header.

`LICENSE.txt` is the Unicode License V3 from https://www.unicode.org/license.txt.
The original files retain their notices. `scripts/generate-unicode.py` pins each
SHA256, derives the runtime tables without an ambient Unicode database and
supports `--check` for exact offline reproduction. Generated Rust table bytes
participate in the existing runtime source/artifact identity. Updating Unicode
requires an explicit source, version, conformance and public-contract change.

The runtime follows Unicode 16 UAX15 revision 56:
https://www.unicode.org/reports/tr15/tr15-56.html.
