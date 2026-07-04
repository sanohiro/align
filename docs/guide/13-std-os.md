# std: files, I/O, and the OS boundary

> 🌐 **English** · [Japanese](./ja/13-std-os.md)

Everything OS-shaped lives in `std`, behind explicit imports: `std.io`, `std.fs`, `std.path`, `std.env`, `std.time` (this chapter), plus `std.encoding` and `std.rand` (next chapter). The imports are capability headers — a file whose header has no `std` imports provably touches no OS. Three rules hold across all of `std`:

- Anything that can fail returns `Result<T, Error>`, mapped from errno by **one fixed table**: `ENOENT` → `NotFound`, `EACCES`/`EPERM` → `Denied`, `EINVAL` → `Invalid`, anything else → `Code(errno)`.
- Resource handles are **Move types** that close themselves on drop (chapter [05](05-memory.md)) — no `close()` to forget, no leak on the error path.
- Nothing is hidden: no global open-file table, no cwd-relative magic, no locale surprises.

## Files in one call: `std.fs`

```align
import std.fs

pub fn main(args: array<str>) -> Result<(), Error> {
    fs.write_file(args[1], "hello, disk\n")?
    if fs.exists(args[1]) { print("written") }
    data := fs.read_file(args[1])?      // whole file → owned string
    print(data.len())                   // 12
    fs.remove(args[1])?
    return Ok(())
}
```

`read_file` / `write_file` / `exists` / `remove` / `read_dir` are the whole-file tier — one call, no handle to manage. `write_file` accepts a `str`, a `builder`, or a `buffer`'s bytes. `read_dir` returns `array<string>` of names. Text reads validate UTF-8 (`Error.Invalid` on binary garbage); binary data goes through the streaming tier below.

## Zero-copy reads: `read_file_view`

```align
import std.fs
import std.io

pub fn main(args: array<str>) -> Result<(), Error> {
    arena {
        v := fs.read_file_view(args[1])?    // mmap — no read loop, no copy
        print(v.len())
        io.stdout.write(v)?
    }
    return Ok(())
}
```

`read_file_view` maps the file and hands you a `str` view of it. It **requires an enclosing `arena`** — the mapping's lifetime is the arena, the unmap is the arena's cleanup, and the view can't escape (`.clone()` if a piece must survive). The memory model from chapter [05](05-memory.md) didn't grow a special case for mmap; mmap fit the model.

## Streams: `reader`, `writer`, `buffer`

The streaming tier, for data bigger than memory:

```align
import std.fs

fn pump(r: reader, w: writer, buf: buffer) -> Result<(), Error> {
    n := r.read(buf)?               // fill buf to capacity; 0 = EOF
    if n == 0 { return Ok(()) }
    w.write(buf.bytes())?
    return pump(r, w, buf)          // tail call — the loop
}

pub fn main(args: array<str>) -> Result<(), Error> {
    r := fs.open(args[1])?          // reader — owns the fd, closes on drop
    w := fs.create(args[2])?        // writer
    buf := buffer(4096)             // reused across the whole loop
    pump(r, w, buf)?
    return Ok(())
}
```

And the shorthand for exactly that shape — `io.copy` (constant memory, whatever the file size):

```align
import std.io

pub fn main() -> Result<(), Error> {
    n := io.copy(io.stdin, io.stdout)?      // the whole of `cat`
    return Ok(())
}
```

`io.stdin` / `io.stdout` / `io.stderr` are the borrowed standard streams. For chatty output, wrap: `w := io.stdout.buffered()` … `w.flush()?`.

**The v1 rule you will trip on once:** an *owned* handle must be **bound to a local before use**. `fs.create(p)?.write(d)?` is rejected — the unnamed temporary would drop without running cleanup (today), so the compiler makes you name it. The borrowed std streams are exempt (`io.stdout.write("ok\n")?` is fine); a `.buffered()` writer is not (its final flush happens on drop — it must be named). This restriction lifts when Move temporaries get their drop (implementation in progress).

## `std.path`, `std.env`, `std.time`

```align
import std.path
import std.env
import std.time

pub fn main() -> Result<(), Error> {
    j := path.join("logs/app", "run.tar.gz")    // owned string
    print(path.dir(j))                          // logs/app     — zero-copy view
    print(path.base(j))                         // run.tar.gz   — view
    print(path.ext(j))                          // .gz          — view
    print(path.normalize("a/./b/../c"))         // a/c — lexical only, no filesystem touch

    env.set("ALIGN_GUIDE", "yes")?
    match env.get("ALIGN_GUIDE") {              // Option<string> — absence isn't an error
        Some(v) => print(v),
        None    => print("unset"),
    }

    t0 := time.instant()                        // monotonic ns — for measuring
    time.sleep(1000000)                         // 1 ms; the argument is ns, exactly i64
    t1 := time.instant()
    if t1 > t0 { print("time moved") }
    // time.now() — wall-clock UNIX ns — for timestamps
    return Ok(())
}
```

Notes that carry design weight:

- `path.base`/`dir`/`ext` return **views into their input** — no allocation, and the region rules apply (a view of an arena-mapped path can't outlive the arena).
- `env.get` returns `Option`, not `Result`: an unset variable is a normal answer, not a failure. The types tell you which kind of "no" you're getting.
- Durations are plain `i64` nanoseconds — no `Duration` type, no unit enum, no conversion API. `instant()` for intervals, `now()` for timestamps, and passing an `i32` is a type error (no implicit widening, per chapter [02](02-language-basics.md)).
- Program arguments are `main(args: array<str>)` — there is no `env.args`; argv flows through one visible door.
