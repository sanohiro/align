"""Recheck fixed sample goldens without calling the native math under test.

Run explicitly with Python 3; the Rust/native owners consume the checked-in bits.
Decimal exp/ln at two precisions plus integer nearest-even IEEE rounding avoids
host libm and a double-rounding f64-to-f32 conversion. This qualifies samples,
not an all-input accuracy bound.
"""
from decimal import Decimal, localcontext
from pathlib import Path
import math
import re
import struct


def rounded_ieee(value, width):
    sign = int(value.is_signed()) << (width - 1)
    numerator, denominator = abs(value).as_integer_ratio()
    if numerator == 0:
        return sign
    precision, bias = (24, 127) if width == 32 else (53, 1023)
    exponent = numerator.bit_length() - denominator.bit_length()
    if (numerator < denominator << exponent if exponent >= 0
            else numerator << -exponent < denominator):
        exponent -= 1
    shift = max(exponent, 1 - bias) - precision + 1
    if shift < 0:
        numerator <<= -shift
    else:
        denominator <<= shift
    quotient, remainder = divmod(numerator, denominator)
    quotient += (remainder * 2 > denominator or
                 (remainder * 2 == denominator and quotient % 2 != 0))
    if exponent < 1 - bias:
        return sign | quotient
    if quotient == 1 << precision:
        quotient >>= 1
        exponent += 1
    if exponent > bias:
        return sign | ((2 * bias + 1) << (precision - 1))
    return sign | ((exponent + bias) << (precision - 1)) | (quotient - (1 << (precision - 1)))


def main():
    source = Path(__file__).with_name("math_libm_cases.rs").read_text()
    rows = re.findall(r'Sample \{ operation: "(\w+)", bits: (\d+), input: (0x[0-9a-f]+), expected: (None|Some\(0x[0-9a-f]+\)) \}', source)
    assert len(rows) == 106
    for operation, width, raw, expected in rows:
        width, raw = int(width), int(raw, 16)
        x = struct.unpack(">f" if width == 32 else ">d", raw.to_bytes(width // 8, "big"))[0]
        expected = None if expected == "None" else int(expected[5:-1], 16)
        results = []
        for precision in (600, 900):
            with localcontext() as context:
                context.prec = precision
                if math.isnan(x) or (operation == "log1p" and x < -1):
                    result = None
                elif x == 0:
                    result = raw
                elif operation == "log1p" and x == -1:
                    result = 0xff800000 if width == 32 else 0xfff0000000000000
                elif math.isinf(x):
                    result = raw if x > 0 else rounded_ieee(Decimal(-1), width)
                else:
                    exact_input = Decimal.from_float(x)
                    value = exact_input.exp() - 1 if operation == "expm1" else (exact_input + 1).ln()
                    result = rounded_ieee(value, width)
                results.append(result)
        assert results == [expected, expected], (operation, width, hex(raw), results, expected)
    print(f"{len(rows)} independent IEEE sample goldens verified at 600 and 900 digits")


if __name__ == "__main__":
    main()
