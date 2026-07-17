# std services: network、HTTP、process、圧縮、暗号

> 🌐 [English](../18-std-services.md) · **日本語**

`std` の第 3 wave はファイルの外にある service まで届きます。境界の規則は同じです。import は capability を明示し、OS や engine の失敗は `Result` を返し、socket、child、client、response、stream のうち resource を所有するものはすべて Move 値です。

## `std.net`

`std.net` は byte stream の層です。DNS、TCP client/server、UDP を提供します。TCP connection は file descriptor を所有し、`reader()` と `writer()` はその connection を借用して第 [13](13-std-os.md) 章の I/O 語彙をそのまま再利用します。

```align
import std.net

pub fn main() -> Result<(), Error> {
    ips := dns.resolve("example.com")?
    print(ips.len())
    return Ok(())
}
```

主な surface は `tcp.connect`、`tcp.listen` / `accept`、`udp.bind` / `send_to` / `recv_from`、`dns.resolve` です。network 操作は impure なので `par_map` には置けません。reader、writer、その他の method view を取る前に、所有する handle を束縛してください。借用した stream が connection より長生きすることはコンパイラが防ぎます。

## `std.http`

構造のない byte stream ではなく HTTP を扱うなら `std.http` を使います。client は keepalive pool を所有し、system trust store で検証する `https://` を扱えます。返される Move response の header と body は zero-copy view です。

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

HTTP status は data です。404 は正常に受信した HTTP response であって `Err` ではありません。transport、TLS、不正な message が error です。`cl.get_many(urls, degree)` は入力順を保ちながら、上限付きで blocking I/O を重ねます。server primitive は意図的に framework より低い層です。`http.serve`、`accept`、request view、`http.response`、`respond` を提供します。SSE など body を stream する場合、`respond_stream` が `http_stream` を返します。各 chunk を `send` し、唯一の正常終端である `finish` を呼びます。

## `std.process`

```align
import std.process

pub fn main(args: array<str>) -> Result<(), Error> {
    ch := process.spawn(args[1], args[1..])?
    print(ch.wait()?)
    return Ok(())
}
```

argv slice は `argv[0]` を含みます。`child` は Move handle で、wait されない child も Drop が reap するため、黙って zombie にはなりません。`process.exec` は process image を置き換え、成功時には cleanup を実行しません。`process.exit` は先に現在の cleanup path を実行します。`process.abort` は cleanup を飛ばす明示的な即時 `_exit` path です。

## `std.compress` と `std.crypto`

圧縮は出力 buffer を所有し、調整済みの system engine を借ります。

```align
import std.compress

pub fn main() -> Result<(), Error> {
    zipped := compress.gzip_compress("align", 6)?
    plain := compress.gzip_decompress(zipped.bytes())?
    print(plain.len())
    return Ok(())
}
```

`gzip_*` と `zstd_*` は同じ byte-to-owned-buffer の形です。不正または大きすぎる圧縮入力は、上限なしの確保ではなく error になります。

`std.crypto` は OS random byte、SHA-256/512、HMAC-SHA256、HKDF-SHA256、Argon2id、AES-256-GCM、ChaCha20-Poly1305、constant-time equality を提供します。独自暗号を発明せず OpenSSL を wrap します。AEAD open は all-or-nothing で、認証失敗時に plaintext を一切渡しません。`constant_time_equal` は同じ長さの内容に対して constant-time です。入力長は公開情報として扱います。BLAKE3 は適切な監査済み system engine が得られるまで公開しません。

## high-throughput の building block

同じ std wave で、大きな program に有用な 3 つの形も加わりました。

- offset 指定ファイル用の `fs.create_rw` / `fs.open_rw` と `pread`、`pwrite`、`len`。
- 読みながら最終長が決まる結果用の、`push`、`append`、消費する `build()` を持つ `array_builder<T>`。
- streaming workload 用の buffered `read_line` と arena checkpoint/reset、および前述した HTTP response streaming。

処理を名付けられる最も狭い層を選びます。byte なら `reader` / `writer`、socket なら `std.net`、HTTP なら `std.http`、routing、middleware、protocol、framework は `pkg` です。
