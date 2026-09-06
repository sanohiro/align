# Align v0.7.5 Release Notes

Align v0.7.5 adds the private temporary-directory lifecycle required by the
align-llm resident GPU bundle publication gate.

## Private temporary-directory lifecycle

`std.fs` adds:

```text
fs.create_private_temp_dir(prefix: str) -> Result<string, Error>
fs.remove_empty_dir(path: str) -> Result<(), Error>
```

The constructor obtains the platform-owned temporary root, canonicalizes and
walks it through retained no-follow directory descriptors, allocates the owned
absolute result before filesystem mutation, draws 128 bits from the OS CSPRNG,
and claims one new mode-0700 directory with `mkdirat`. Existing files,
directories, and symlinks are never followed, reused, or modified.

Removal accepts one strict absolute path, retains and revalidates its parent and
final directory descriptors, and performs one empty-directory-only removal. It
does not follow symlinks, remove files, or recurse. Missing, denied, occupied,
wrong-type, and replacement-race paths retain the existing `std.fs` error and
cleanup rules.

The new operations compose with `fs.create_exclusive_beneath` and
`fs.open_beneath_single_link`, so a client can stage already-verified artifacts
under one fresh private root before native path-based loading. Whole-program,
per-unit, generic replay, checked-HIR, MIR/LLVM, runtime ABI, ownership,
failure-injection, Linux, and macOS owners cover the lifecycle.
