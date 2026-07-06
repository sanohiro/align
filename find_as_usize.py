import re
with open("crates/align_runtime/src/lib.rs", "r") as f:
    lines = f.readlines()
for i, line in enumerate(lines):
    if "as usize" in line and not "(d.tag & 0xff)" in line and not "as u64 - 1" in line and not "as u64).to_le_bytes" in line and not "HEX[" in line:
        # Also ignore the safe_slice function I just wrote
        if "safe_slice" in line or "from_raw_parts" in line:
            continue
        print(f"Line {i+1}: {line.strip()}")
