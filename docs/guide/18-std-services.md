# std services: network, HTTP, processes, compression, crypto

> 🌐 **English** · [Japanese](./ja/18-std-services.md)

This chapter covers networking, HTTP, processes, compression, and cryptography. The same boundary rules apply: imports name capabilities, operating-system and engine failures return `Result`, and sockets, children, clients, responses, and streams that own resources are Move values.

## `std.net`

`std.net` is the byte-stream layer: DNS, TCP client/server, and UDP. A TCP connection owns its file descriptor; its `reader()` and `writer()` borrow that connection and reuse the I/O vocabulary from chapter [13](13-std-os.md).

```align
import std.net

pub fn main() -> Result<(), Error> {
    ips := dns.resolve("example.com")?
    print(ips.len())
    return Ok(())
}
```

The main surface is `tcp.connect`, `tcp.listen`/`accept`, `udp.bind`/`send_to`/`recv_from`, and `dns.resolve`. Network operations are impure and therefore cannot appear in `par_map`. Bind owning handles before taking a reader, writer, or another method view; the compiler prevents a borrowed stream from outliving its connection.

## `std.http`

Use `std.http` when the data is HTTP rather than an unstructured byte stream. The client owns a keepalive pool, supports verified `https://` through the system trust store, and returns a Move response whose headers and body are zero-copy views.

```align
import std.cli
import std.http

pub fn main(args: array<str>) -> Result<(), Error> {
    c := cli.command("get")
    c.flag_str("url", "https://example.com/")
    p := c.parse(args)?

    cl := http.client()
    resp := cl.get(p.get_str("url"))?
    print(resp.status())
    print(resp.body().len())
    return Ok(())
}
```

HTTP status is data: a 404 is a successful HTTP response, not an `Err`. Transport, TLS, and malformed-message failures are errors. `cl.get_many(urls, degree)` performs bounded blocking-I/O overlap while preserving input order. Server primitives are deliberately below framework level: `http.serve`, `accept`, request views, `http.response`, and `respond`. For SSE or another streaming body, `respond_stream` yields an `http_stream` while the request context stays readable (borrowed, spent); call `send` for each chunk and `finish` for the sole clean terminator — or, before the first `send`, `reject(rb)` to answer with a normal error response instead.

### Receive binary bodies without retaining the whole response

The runnable [http_fetch example](../../examples/http_fetch.align) composes
`std.cli`, `std.http` and `std.io`. Build it from the repository root:

```bash
./target/debug/alignc build examples/http_fetch.align
./http_fetch --url http://127.0.0.1:8082/v1/jobs/EPOCH/ID/artifacts/mux > scene.mp4
```

Replace the URL with your endpoint. The [multimodal reference](27-multimodal-reference.md)
supplies that artifact route after a job completes. Only a zero exit status means
the body was complete and every stdout write succeeded; an error may leave a
partial file. Redirecting stdout does not publish a file atomically.

The example accepts only final status 200 and preserves all body bytes. It uses
one 64 KiB buffer: `stream.read(out)` overwrites that buffer, and
`io.stdout.write(out.bytes())` consumes its borrowed byte view before the next
read. A zero count means completion. Chunk framing is removed by the stream;
the application performs no UTF-8 conversion or whole-body accumulation.

`--max-body-bytes` defaults to 64 MiB and accepts 1..1073741824 decoded bytes.
`--timeout-ns` defaults to 30 seconds and must be positive. Both limits are
validated before network I/O. The HTTP timeout applies independently to each
connect, send and transport receive; it does not bound DNS, stdout writes or
the total download. A peer making progress can take longer than one timeout.
Use `--help` separately to print usage. Empty URL, invalid limits and non-200
status return `Error.Invalid`; exceeding the body cap is `Error.Code(-1)`. Transport,
framing, timeout and output errors propagate through `?`.

## `std.process`

```align
import std.process

pub fn main(args: array<str>) -> Result<(), Error> {
    ch := process.spawn(args[1], args[1..])?
    print(ch.wait()?)
    return Ok(())
}
```

The argv slice includes `argv[0]`. A `child` is a Move handle and Drop reaps an unwaited child, so it cannot silently become a zombie. `process.exec` replaces the image and runs no cleanup on success. `process.exit` performs the current cleanup path first; `process.abort` is the explicit immediate `_exit` path and skips cleanup.

## `std.compress` and `std.crypto`

Compression owns its output buffer and borrows the tuned system engines:

```align
import std.compress

pub fn main() -> Result<(), Error> {
    zipped := compress.gzip_compress("align", 6)?
    plain := compress.gzip_decompress(zipped.bytes())?
    print(plain.len())
    return Ok(())
}
```

`gzip_*` and `zstd_*` share that byte-to-owned-buffer shape. Invalid or oversized compressed input is an error rather than an unbounded allocation.

`std.crypto` provides OS random bytes, SHA-256/512, HMAC-SHA256, HKDF-SHA256, Argon2id, AES-256-GCM, ChaCha20-Poly1305, and constant-time equality. It wraps OpenSSL instead of inventing cryptography. Argon2id requires the provider added in OpenSSL 3.2; on an older engine that operation returns `Error.Code` without producing output. AEAD open is all-or-nothing: authentication failure releases no plaintext. `constant_time_equal` is constant-time over equal-length contents; input length is public. BLAKE3 is not exposed until a suitable audited system engine exists.

### Hash a file incrementally

For a large artifact, feed a reused byte buffer into `crypto.sha256_stream()`
instead of retaining the whole file. This program hashes `artifact.bin`:

```align
import std.crypto
import std.encoding
import std.fs

fn sha256_reader(input: reader) -> Result<string, Error> {
    digest := crypto.sha256_stream()
    mut chunk := buffer(65536)
    loop {
        n := input.read(chunk)?
        if n == 0 { break }
        digest.update(chunk.bytes())
    }
    result := digest.finish()
    return Ok(encoding.hex_encode(result[..]))
}

pub fn main() -> Result<(), Error> {
    input := fs.open("artifact.bin")?
    print(sha256_reader(input)?)
    return Ok(())
}
```

Each read replaces the buffer's initialized bytes. Update borrows those bytes
only for the call; the next read can reuse the same storage. Input is binary,
so NUL and non-UTF-8 bytes need no conversion. `finish()` consumes the digest
and produces an owned 32-byte result; hex encoding produces an owned string.
`?` propagates file/read errors and ordinary Drop releases the reader and any
unfinished digest. A digest describes the bytes actually read, rather than
certifying a stable filesystem snapshot. See the
[incremental SHA-256 contract](../impl/std-design/crypto.md#incremental-sha-256)
for the ownership and provider-failure rules.

The runnable [file_sha256 example](../../examples/file_sha256.align) adds CLI
selection, an input byte cap and checked stdout writes:

```bash
./target/debug/alignc build examples/file_sha256.align
./file_sha256 --file scene.mp4 --max-input-bytes 4294967296
./file_sha256 < scene.mp4
```

An omitted or empty `--file` reads stdin. `--max-input-bytes` defaults to 1 GiB
and accepts 0..2305843009213693951; zero admits only empty input. Parsing and cap
validation precede opening a file or reading stdin. Use `--help` for usage.
The example reuses one 64 KiB window and emits one lowercase 64-character digest
plus newline only after EOF. A cap violation returns `Error.Code(-1)` before
updating the digest, but the read may already have consumed one window past the
cap. Input errors emit no digest. A stdout error propagates and may leave a
digest prefix; the byte cap does not bound blocking input time. Existing crypto
provider/allocation failures remain hard errors as specified above.

## `std.log`

A logger owns the writer you give it and uses an explicit minimum level. This program writes `[INFO] ready` to stderr and suppresses the debug record:

```align
import std.io
import std.log

fn main() -> Result<(), Error> {
    logger := log.new(io.stderr.buffered(), log.level.Info)
    logger.line(log.level.Info, "ready")
    logger.line(log.level.Debug, "details")
    return logger.flush()
}
```

`log.new` consumes the writer; use the logger from then on. The levels are `Debug`, `Info`, `Warn`, `Error`, and `Off`. `line` accepts text or a builder and returns Unit. It records the first output failure internally and suppresses later writes; `flush()` reports that failure through `Result`. Call it explicitly when losing log output should affect your program's result.

Arguments are evaluated even for a disabled level. Use `if logger.enabled(log.level.Debug) { ... }` around expensive message construction. The logger escapes line breaks so each record occupies one line. It does not add timestamps, structured fields, or a global logger; use ordinary templates or builders for message text. See the [logging design](../impl/std-design/log.md) for the exact format and ownership rules.

## High-throughput building blocks

Three other tools support streaming and larger programs:

- `fs.create_rw` / `fs.open_rw` with `pread`, `pwrite`, and `len` for offset-addressed files.
- `array_builder<T>` with `push`, `append`, and consuming `build()` for a result whose final length is discovered while reading.
- buffered `read_line` and arena checkpoint/reset for streaming workloads, plus HTTP response streaming described above.

Choose the narrowest layer that names the work: `reader`/`writer` for bytes, `std.net` for sockets, `std.http` for HTTP, and `pkg` for routing, middleware, protocols, and frameworks. The first-party `pkg.web`, `pkg.frame`, and `pkg.auth` packages now provide concrete examples; chapter [23](23-packages.md) introduces them.
