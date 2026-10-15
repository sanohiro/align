# Runnable examples

Run these commands from the repository root after installing the prerequisites in
the [project README](../README.md):

```text
scripts/cargo.sh build --workspace
./target/debug/alignc check examples/borrowed_array.align
./target/debug/alignc run examples/borrowed_array.align --profile dev
```

Start with these small examples of the current language:

| Example | What it demonstrates | Output |
| --- | --- | --- |
| [borrowed_array.align](borrowed_array.align) | Read and update one owned array through `borrow` and `borrow mut` | `6`, then `12` |
| [zip_map_into.align](zip_map_into.align) | Combine two inputs directly into a distinct caller-owned output buffer | `12`, `24`, `36` |
| [json_owned.align](json_owned.align) | Return decoded owners after the input expires, then encode with an explicit byte cap | Name, score sum, JSON, and the still-usable name |
| [loop.align](loop.align) | Return an `Option` from a value-carrying `break` | `13`, then `-1` |

For other language features, see [pipeline.align](pipeline.align),
[result.align](result.align), [tuple_field_move.align](tuple_field_move.align),
[task_group.align](task_group.align), and [modules/main.align](modules/main.align).

Many numeric examples return the computed value from `main() -> i32`, so a nonzero
process exit status can be deliberate. For example, `arraysum.align` exits with
42, and `result.align` demonstrates an error propagated to exit code 7. The four
examples in the table print their results and exit with 0 on success.

Filesystem, HTTP, process and multimodal examples need their documented inputs
or services. Read each entry point before running it; support modules are not
standalone programs. `alignc check` validates source without running the program.
