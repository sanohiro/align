import re

def main():
    path = "crates/align_runtime/src/lib.rs"
    with open(path, "r") as f:
        content = f.read()

    # The previous script replaced:
    # usize::try_from(var) -> isize::try_from(var).map(|n| n as usize)
    # let's replace back `isize::try_from(X).map(|Y| Y as usize)` with `usize::try_from(X).filter(|&x| x <= isize::MAX as usize)`
    
    # We have multiple variants.
    # 1. `isize::try_from(X).map(|Y| Y as usize)`
    content = re.sub(
        r'isize::try_from\(([^)]+)\)\.map\(\|[a-zA-Z0-9_]+\|\s*[a-zA-Z0-9_]+\s+as\s+usize\)',
        r'usize::try_from(\1).filter(|&x| x <= isize::MAX as usize)',
        content
    )
    
    # 2. `isize::try_from(X).ok().map(|Y| Y as usize)`
    content = re.sub(
        r'isize::try_from\(([^)]+)\)\.ok\(\)\.map\(\|[a-zA-Z0-9_]+\|\s*[a-zA-Z0-9_]+\s+as\s+usize\)',
        r'usize::try_from(\1).ok().filter(|&x| x <= isize::MAX as usize)',
        content
    )

    with open(path, "w") as f:
        f.write(content)

if __name__ == "__main__":
    main()
