# std services: network, HTTP, processes, compression, crypto

> 🌐 **English** · [Japanese](./ja/18-std-services.md)

The third wave of `std` reaches beyond files to services. The same boundary rules still hold: imports name capabilities, operating-system and engine failures return `Result`, and every socket, child, client, response, and stream that owns a resource is a Move value.

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

HTTP status is data: a 404 is a successful HTTP response, not an `Err`. Transport, TLS, and malformed-message failures are errors. `cl.get_many(urls, degree)` performs bounded blocking-I/O overlap while preserving input order. Server primitives are deliberately below framework level: `http.serve`, `accept`, request views, `http.response`, and `respond`. For SSE or another streaming body, `respond_stream` yields an `http_stream`; call `send` for each chunk and `finish` for the sole clean terminator.

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

## High-throughput building blocks

The same std wave added three useful shapes for larger programs:

- `fs.create_rw` / `fs.open_rw` with `pread`, `pwrite`, and `len` for offset-addressed files.
- `array_builder<T>` with `push`, `append`, and consuming `build()` for a result whose final length is discovered while reading.
- buffered `read_line` and arena checkpoint/reset for streaming workloads, plus HTTP response streaming described above.

Choose the narrowest layer that names the work: `reader`/`writer` for bytes, `std.net` for sockets, `std.http` for HTTP, and `pkg` for routing, middleware, protocols, and frameworks.
