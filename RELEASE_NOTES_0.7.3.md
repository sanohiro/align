# Align v0.7.3 Release Notes

Align v0.7.3 fixes the MIR producer-certification regression exposed by the
align-llm runtime bundle verifier and adds the retained-root filesystem policy
needed by its resident GPU bundle publication gate.

## Compiler correction

Producer certification now preserves owned `string` provenance when a dynamic
array-of-struct field is viewed as `str`, carried through loop joins, and moved
into a `Result` record. Borrowed or otherwise uncertified string escapes remain
rejected. Whole-program and per-unit regression fixtures cover the corrected
producer graph.

## Retained-root single-link open

`std.fs` adds:

```text
fs.open_beneath_single_link(root: str, relative: str) -> Result<reader, Error>
```

The constructor preserves `fs.open_beneath` path grammar, retained-directory
traversal, no-follow checks, regular-file and descriptor-identity validation,
ownership, cleanup, and error mapping. Before publishing the existing owned
reader, it requires the already-opened descriptor's existing `fstat` record to
report exactly one hard link. Zero or multiple links return `Error.Invalid`
without publishing a reader or reading artifact bytes.

`fs.open_beneath` is unchanged for callers that intentionally permit hard
links. The new constructor exposes no descriptor or file metadata and makes no
persistent immutability claim after its point-in-time descriptor check.
