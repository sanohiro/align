# Build a CSV-to-JSON command-line app

> 🌐 **English** · [Japanese](./ja/practical-cli.md)

Build a small program that reads scores from a CSV file, counts active rows,
and writes a JSON summary. You will create a project outside the compiler
repository, vendor one package, separate computation from I/O, and run tests
before building an executable.

This uses the current repository's compiler and `pkg.csv`. Follow
[getting started](01-getting-started.md) to install or build `alignc`; keep the
compiler and copied package sources from a compatible revision. A packaged
release can lag the repository. The [API task index](api-by-task.md) helps find
the operations used here.

## 1. Create the project

In a shell, set `ALIGN_SOURCE` to your Align checkout and choose a new directory
for the app. `alignc` must be on `PATH`; for a source build, add that checkout's
`target/debug` or `target/release` directory.

```bash
ALIGN_SOURCE=/path/to/align
mkdir score-report
cd score-report
cp -R "$ALIGN_SOURCE/apps/csv/pkg" ./pkg
```

Copy the complete `pkg` subtree, including `csv/internal/descriptor.align`.
`import pkg.csv` resolves to these local sources. There is no package manifest
or download command; see [packages](23-packages.md).

Create the three files below. The finished source tree is:

```text
score-report/
  main.align
  report.align
  scores.csv
  pkg/
    csv.align
    csv/
      internal/
        descriptor.align
```

Save this input as **`scores.csv`**, using LF line endings:

```csv
active,score
true,10
false,999
true,32
```

## 2. Put the computation and its tests in a module

Save the complete block as **`report.align`**:

```align
module report

import core.test
import pkg.csv

Score { active: bool, score: i32 }

pub Summary { rows: i64, active_rows: i64, score_sum: i64 }

fn csv_error(error: pkg.csv.Error) -> Error = match error {
    Invalid => Error.Invalid,
    LimitExceeded => Error.Code(-1),
}

pub fn summarize(input: str) -> Result<Summary, Error> {
    options := pkg.csv.DecodeOptions {
        header: pkg.csv.Header.Present,
        line_ending: pkg.csv.LineEnding.Lf,
        max_rows: 100000,
    }
    arena out {
        decoded: Result<soa<Score>, pkg.csv.Error> := pkg.csv.decode(input, out, options)
        rows := decoded.map_err(csv_error)?
        return Ok(Summary {
            rows: rows.len(),
            active_rows: rows.where(.active).count(),
            score_sum: rows.where(.active).score.map(fn score { score as i64 }).sum(),
        })
    }
}

test "only active scores contribute" {
    result := summarize("active,score\ntrue,10\nfalse,999\ntrue,32\n")?
    test.expect_eq(result.rows, 3)
    test.expect_eq(result.active_rows, 2)
    test.expect_eq(result.score_sum, 42)
}

test "a header without data has a zero summary" {
    result := summarize("active,score\n")?
    test.expect_eq(result.rows, 0)
    test.expect_eq(result.active_rows, 0)
    test.expect_eq(result.score_sum, 0)
}

test "invalid numeric input is an error" {
    rejected := match summarize("active,score\ntrue,oops\n") {
        Ok(_) => false,
        Err(error) => match error {
            Invalid => true,
            _ => false,
        },
    }
    test.expect(rejected)
}
```

The expected `soa<Score>` type supplies the CSV schema. Header names select the
columns; the explicit options require LF separators and at most 100,000 data
rows. Invalid syntax or cells become `Error.Invalid`; this app maps the
package's `LimitExceeded` to `Error.Code(-1)`. The mapping belongs to the app,
not to `pkg.csv`.

The columns live in `out`. `Summary` contains only Copy integers, so returning
it keeps no reference to the input or arena. Each score is parsed as `i32` and
widened to `i64` before summing; even 100,000 extreme `i32` scores fit in that
sum. The two filtered reductions compute a count and a total separately.

Run the tests now:

```bash
alignc test report.align
```

The final result is `test result: ok. 3 passed; 0 failed`. A test declaration
belongs at module scope. `?` fails the test on an error, and assertions compare
the individual fields because records do not support `==`.

## 3. Add the command-line boundary

Save this complete block as **`main.align`**:

```align
module main

import core.json
import std.cli
import std.fs
import std.io
import report

pub fn main(args: array<str>) -> Result<(), Error> {
    command := cli.command("score-report")
    command.flag_str("input", "scores.csv")
    command.flag_bool("help")
    parsed := command.parse(args)?
    output := io.stdout

    if parsed.get_bool("help") {
        usage := command.usage()
        output.write(usage)?
        output.write("\n")?
        return Ok(())
    }

    path := parsed.get_str("input")
    input := fs.read_file(path)?
    summary := report.summarize(input)?
    encoded := json.encode_bounded(summary, 256)?
    output.write(encoded)?
    output.write("\n")?
    return Ok(())
}
```

`main` owns the file read, argument parsing, and output. The parsed flag's path
view stays within `parsed`'s lifetime; `read_file` returns an owned UTF-8
`string`, which the `str` parameter of `summarize` borrows. JSON encoding returns
another owned string and rejects an output above the explicit 256-byte cap.
`?` propagates failures, including failed stdout writes.

This app reads the whole file before decoding. The row cap is not an input-byte
or memory cap. Use it for small local files; the existing
[CSV app](../../apps/csv/main.align) demonstrates a bounded buffer reader when
you need an explicit input-byte limit.

## 4. Check, run, and build

Run these commands from `score-report/`:

```bash
alignc check main.align
alignc check-per-unit main.align
alignc test main.align
alignc run main.align -- --input scores.csv
```

`test main.align` reaches the same three tests through `import report` and does
not call `main`. Tests are discovered through imports, not by scanning a
`tests/` directory. The two check commands cover different frontend paths;
only a build or run also checks native compilation and linking. See
[the toolchain](16-toolchain.md).

The run prints one JSON line:

```json
{"rows":3,"active_rows":2,"score_sum":42}
```

Build an executable and run it directly:

```bash
alignc build main.align --profile release
./main --input scores.csv
./main --help
```

The build writes `main` in the current directory. Test bodies are omitted from
the production executable. `--` separates program arguments from `alignc run`
options; the executable itself does not need that separator.

## 5. Try the failure path

Save **`bad.csv`**:

```csv
active,score
true,oops
```

Then run:

```bash
./main --input bad.csv
./main --input missing.csv
```

The first command fails numeric conversion; the second fails if `missing.csv`
does not exist. Both exit nonzero and produce no JSON summary. Input validation
and JSON encoding finish before the first summary write. An output error can
still leave a prefix; redirecting `./main --input scores.csv > summary.json`
does not make publication atomic, and the shell may already have created or
truncated the output file before the app starts.

When editing, use `alignc fmt report.align` and `alignc fmt main.align`, then
rerun `alignc test main.align` and the sample command. Keep pure data processing
in `report`; put new filesystem or CLI operations in `main`. For related
operations, use the [API task index](api-by-task.md),
[module guide](09-generics-and-modules.md), and
[CSV contract](../impl/pkg-design/csv.md).
