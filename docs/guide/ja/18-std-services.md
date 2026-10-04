# std: ネットワーク、HTTP、プロセス、圧縮、暗号

> 🌐 [English](../18-std-services.md) · **日本語**

本章ではネットワーク、HTTP、プロセス、圧縮、暗号を扱います。これまでと同じく、使う機能をインポートで明示し、OS やライブラリの失敗は `Result` で返します。リソースを所有するソケット、子プロセス、クライアント、レスポンス、ストリームは Move 値です。

## `std.net`

`std.net` は DNS、TCP クライアントとサーバー、UDP を提供します。TCP 接続はファイルディスクリプタを所有します。`reader()` と `writer()` はその接続を借用し、第 [13](13-std-os.md) 章と同じ I/O 操作で読み書きします。

```align
import std.net

pub fn main() -> Result<(), Error> {
    ips := dns.resolve("example.com")?
    print(ips.len())
    return Ok(())
}
```

主な API は `tcp.connect`、`tcp.listen` / `accept`、`udp.bind` / `send_to` / `recv_from`、`dns.resolve` です。ネットワーク操作は非純粋なので `par_map` 内では呼べません。reader や writer などのビューを得る前に、所有ハンドルを変数へ束縛してください。借用したストリームを接続より長く保持することは、コンパイラが禁止します。

## `std.http`

HTTP には `std.http` を使います。クライアントはキープアライブ用の接続プールを持ち、システムの証明書ストアで検証する `https://` に対応します。レスポンスは Move 値で、ヘッダーとボディはゼロコピーのビューとして参照できます。

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

`404 Not Found` も、正常に受信した HTTP レスポンスとして扱います。HTTP ステータスだけでは `Err` になりません。通信、TLS、不正なメッセージの受信に失敗した場合はエラーになります。

`cl.get_many(urls, degree)` は、入力順を保ちつつ、指定した上限内で複数のブロッキング I/O を並行して進めます。サーバー側には `http.serve`、`accept`、リクエストビュー、`http.response`、`respond` などの基本操作があります。SSE などでボディをストリーミングする場合は `respond_stream` が `http_stream` を返します。リクエストコンテキストは借用された spent 状態になり、内容のビューは引き続き参照できます。チャンクを `send` で送り、正常に完了するには `finish` を呼びます。最初の `send` より前なら、`reject(rb)` で通常のエラーレスポンスに切り替えることもできます。

### 本文全体を保持せずバイナリを受信する

実行できる [http_fetch の例](../../../examples/http_fetch.align) は、
`std.cli`、`std.http`、`std.io` を組み合わせています。
リポジトリのルートでビルドします。

```bash
./target/debug/alignc build examples/http_fetch.align
./http_fetch --url http://127.0.0.1:8082/v1/jobs/EPOCH/ID/artifacts/mux > scene.mp4
```

URL は利用するエンドポイントに置き換えてください。
[multimodal の参照例](27-multimodal-reference.md) では、ジョブ完了後にこの成果物
ルートを使えます。終了コード 0 は、本文の受信が完了し、標準出力への書き込みも
すべて成功したことを意味します。途中で失敗すると、不完全なファイルが残る場合が
あります。標準出力のリダイレクトは、ファイルを原子的に公開する処理ではありません。

この例は最終ステータス 200 のみを受け入れ、本文のバイトをそのまま書き出します。
64 KiB のバッファを 1 個使い、`stream.read(out)` で上書きします。
`io.stdout.write(out.bytes())` は、次の読み取りの前に借用したバイトビューを
書き出します。読み取り数 0 が本文の完了を表します。チャンクのフレーミングは
ストリームが取り除き、アプリケーションは UTF-8 変換も本文全体の蓄積もしません。

`--max-body-bytes` の既定値は 64 MiB で、復号済み本文の 1..1073741824 バイトを
指定できます。`--timeout-ns` の既定値は 30 秒で、正の値が必要です。
両方の制限を通信前に検証します。HTTP のタイムアウトは接続、送信、
各トランスポート受信に個別に適用され、DNS、標準出力への書き込み、
ダウンロード全体の所要時間は制限しません。進行中の通信は、1 回分の
タイムアウトより長くかかる場合があります。`--help` は別途実行してください。
空の URL、不正な制限値、200 以外のステータスは `Error.Invalid`、
本文サイズ上限の超過は `Error.Code(-1)` になります。
通信、フレーミング、タイムアウト、出力のエラーは `?` で伝播します。

## `std.process`

```align
import std.process

pub fn main(args: array<str>) -> Result<(), Error> {
    ch := process.spawn(args[1], args[1..])?
    print(ch.wait()?)
    return Ok(())
}
```

引数のスライスには `argv[0]` も含めます。`child` は Move ハンドルです。`wait` せずに drop しても、後始末で子プロセスを回収するため、ゾンビプロセスが残りません。

`process.exec` は現在のプロセスイメージを置き換え、成功時には後始末を行いません。`process.exit` は現在のクリーンアップ処理を実行してから終了します。`process.abort` は `_exit` に相当し、後始末をせず即座に終了します。

## `std.compress` と `std.crypto`

圧縮関数はシステムの圧縮エンジンを使い、所有権のある出力バッファを返します。

```align
import std.compress

pub fn main() -> Result<(), Error> {
    zipped := compress.gzip_compress("align", 6)?
    plain := compress.gzip_decompress(zipped.bytes())?
    print(plain.len())
    return Ok(())
}
```

`gzip_*` と `zstd_*` はどちらも、バイト列を受け取り、所有権のあるバッファを返します。不正な圧縮データやサイズ上限を超える入力はエラーになり、無制限にメモリを確保することはありません。

`std.crypto` は OS の乱数、SHA-256/512、HMAC-SHA256、HKDF-SHA256、Argon2id、AES-256-GCM、ChaCha20-Poly1305、定数時間の比較を提供します。暗号処理には OpenSSL を使います。

Argon2id には OpenSSL 3.2 で追加されたプロバイダが必要で、古い環境では出力を生成せず `Error.Code` を返します。AEAD の復号では、認証に失敗すると平文を一切返しません。`constant_time_equal` は同じ長さの入力に対し、内容によらず一定時間で比較します。長さ自体は公開情報として扱います。BLAKE3 は、適切な監査済みのシステムエンジンが利用できるまで提供しません。

### ファイルを少しずつハッシュする

大きな成果物は、ファイル全体を保持せず、同じバイトバッファを再利用して
`crypto.sha256_stream()` に渡せます。次のプログラムは `artifact.bin` をハッシュします。

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

読み取りのたびに、バッファの初期化済みバイト列を置き換えます。Update がその
バイト列を借用するのは呼び出し中だけなので、次の読み取りで同じ領域を使えます。
入力はバイナリで、NUL や UTF-8 でないバイトを変換する必要はありません。
`finish()` はダイジェストを消費し、所有権のある32バイトの結果を返します。
16進エンコードも所有権のある文字列を作ります。`?` はファイルを開く際や
読み取る際のエラーを伝播し、通常の Drop が reader と未完了のダイジェストを
解放します。ハッシュの対象は実際に読み取ったバイト列であり、ファイルシステムの
安定したスナップショットを保証しません。所有権とプロバイダ失敗時の規則は
[増分 SHA-256 の契約](../../impl/std-design/ja/crypto.md#増分-sha-256)を参照してください。

## `std.log`

ロガーには、出力先の writer と出力する最低レベルを渡します。次のプログラムは標準エラーに `[INFO] ready` を書き、Debug の行は出力しません。

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

`log.new` は writer を消費するので、以後はロガーを使います。レベルは `Debug`、`Info`、`Warn`、`Error`、`Off` です。`line` はテキストまたは builder を受け取り、Unit を返します。出力が失敗すると最初のエラーを内部に記録して、以後の書き込みを止めます。`flush()` はそのエラーを `Result` で返します。ログの欠落をプログラムの結果に反映したい場合は、明示的に呼んでください。

無効なレベルでも引数は評価されます。重いメッセージの組み立てを省くには、`if logger.enabled(log.level.Debug) { ... }` で囲みます。メッセージ内の改行はエスケープされ、1レコードが1行になります。タイムスタンプ、構造化フィールド、グローバルなロガーは追加しません。本文の組み立てには通常のテンプレートや builder を使います。詳しい出力形式と所有権は[ログ機能の設計](../../impl/std-design/ja/log.md)にあります。

## 大量のデータを処理するための基本機能

ストリーミングや大きなプログラムには、次の機能も使えます。

- **オフセット指定のファイル操作**： `fs.create_rw` や `fs.open_rw` によってランダムアクセス可能なファイルを開き、指定した位置から読み書きする `pread`、`pwrite`、およびファイルサイズを取得する `len` メソッド。
- **動的配列の構築**： データを読み込みながら最終的な要素数が決まっていくような処理のために、`push` や `append` で要素を追加し、最後に `build()` で所有権を消費して配列を完成させる `array_builder<T>`。
- **ストリーミング処理向けの最適化**： バッファリングされた `read_line`（行単位の読み込み）、アリーナのメモリを部分的に再利用するための `checkpoint` と `reset` 機構、そして前述した HTTP レスポンスのストリーミング機能。

処理に必要な層を選んでください。バイト列の読み書きは `reader` / `writer`、ソケットは `std.net`、HTTP は `std.http` を使います。ルーティング、ミドルウェア、アプリケーションプロトコル、フレームワークは `pkg` が担当します。Align が提供する `pkg.web`、`pkg.frame`、`pkg.auth` は、第 [23](23-packages.md) 章で紹介します。
