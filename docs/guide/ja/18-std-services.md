# std services: network、HTTP、process、圧縮、暗号

> 🌐 [English](../18-std-services.md) · **日本語**

`std` ライブラリの第3波は、ファイルシステムという枠を超えて、外部の「サービス」とのやり取りにまで及びます。言語と外部境界に関する設計ルールはこれまでと全く同じです。`import` はそのモジュールが要求するケイパビリティ（権限）を明示し、OS や実行エンジン由来の失敗はすべて `Result` として返され、ソケット、子プロセス、HTTP クライアント、レスポンス、ストリームなど、何らかのリソースを所有する型はすべて「Move 値」として扱われます。

## `std.net`

`std.net` は、生のバイトストリームを扱うための低レイヤーなネットワーク層です。DNS の名前解決、TCP のクライアントおよびサーバー、そして UDP の機能を提供します。TCP コネクションはファイルディスクリプタを所有権として持ち、そこから `reader()` と `writer()` を通じてコネクションを借用することで、[13](13-std-os.md) 章で学んだ I/O の共通語彙をそのまま再利用して読み書きを行います。

```align
import std.net

pub fn main() -> Result<(), Error> {
    ips := dns.resolve("example.com")?
    print(ips.len())
    return Ok(())
}
```

主な公開 API（サーフェス）は、`tcp.connect`、`tcp.listen` と `accept`、`udp.bind` / `send_to` / `recv_from`、そして `dns.resolve` です。ネットワーク操作は明らかに「非純粋（impure）」な副作用を伴うため、`par_map` の内部では呼び出せません。`reader` や `writer` などのメソッドによるビューを取得する前に、所有権を持つハンドルをローカル変数に束縛してください。借用されたストリームが、元のコネクションよりも長生きしてしまう（ダングリング参照になる）事態は、コンパイラによって完全に防がれます。

## `std.http`

構造を持たない生のバイトストリームではなく、HTTP プロトコルを扱う場合は `std.http` を使用します。HTTP クライアントは内部でキープアライブ（keep-alive）のコネクションプールを所有し、システムのトラストストア（証明書ストア）を用いて安全に検証された `https://` 通信を扱うことができます。リクエスト後に返されるレスポンスオブジェクトは Move 値であり、そこから得られるヘッダーやボディのデータは、メモリコピーを伴わない「ゼロコピー・ビュー」として提供されます。

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

HTTP のステータスコードは単なる「データ」です。例えば `404 Not Found` は、正常に受信できた HTTP レスポンスの1つであって、Align における `Err`（実行エラー）ではありません。`Err` となるのは、トランスポート層の切断、TLS の検証失敗、または不正な形式の HTTP メッセージを受け取った場合のみです。

`cl.get_many(urls, degree)` を使うと、入力された URL の順序を保ちながら、指定した同時実行数の上限（`degree`）の範囲内でブロッキング I/O を効率的に多重化できます。また、提供されているサーバー用のプリミティブ（基本部品）は、あえて Web フレームワークよりも低いレイヤーに留められています。`http.serve`、`accept`、リクエストビュー、`http.response` の構築、そして `respond` による返信機能のみを提供します。Server-Sent Events（SSE）のようにボディをストリーミング送信する場合は、`respond_stream` が `http_stream` を返します。そこに各チャンクを `send` で送り込み、最後に唯一の正常な終端処理である `finish` を呼び出します。

## `std.process`

```align
import std.process

pub fn main(args: array<str>) -> Result<(), Error> {
    ch := process.spawn(args[1], args[1..])?
    print(ch.wait()?)
    return Ok(())
}
```

引数のスライス `args` は、実行ファイル名である `argv[0]` を含みます。生成された子プロセス（`child`）は Move ハンドルとして扱われます。もし `wait` されずにスコープを抜けたとしても、Drop 処理が自動的にプロセスを刈り取る（reap する）ため、暗黙のうちにゾンビプロセスが残ることはありません。

`process.exec` は現在のプロセスイメージを新しいプログラムに置き換えます（成功時には現在のプロセスのクリーンアップ処理は実行されません）。`process.exit` は、プログラムを終了する前に現在のスコープにあるクリーンアップ処理（Drop など）をすべて実行します。一方 `process.abort` は、クリーンアップをすべてスキップして OS レベルで即座に終了する（`_exit` に相当する）パスです。

## `std.compress` と `std.crypto`

圧縮ライブラリは、出力先となる `buffer` を内部で所有し、システムに組み込まれた高度にチューニング済みの圧縮エンジンを借用して動作します。

```align
import std.compress

pub fn main() -> Result<(), Error> {
    zipped := compress.gzip_compress("align", 6)?
    plain := compress.gzip_decompress(zipped.bytes())?
    print(plain.len())
    return Ok(())
}
```

`gzip_*` ファミリーと `zstd_*` ファミリーの関数は、どちらも「バイト列を受け取り、所有権のあるバッファを返す」という同じインターフェース（byte-to-owned-buffer）を持っています。不正な圧縮データや、展開後のサイズが異常に大きくなるような悪意ある入力（いわゆる Zip Bomb）に対しては、無制限にメモリを確保するのではなく、安全な制限を超えた時点で `error` を返して処理を中断します。

`std.crypto` モジュールは、OS 提供の安全な乱数、SHA-256 / SHA-512、HMAC-SHA256、HKDF-SHA256、Argon2id、AES-256-GCM、ChaCha20-Poly1305、そして暗号論的に安全な定数時間での比較（constant-time equality）を提供します。Align は「独自の暗号アルゴリズムを発明しない」という原則を貫き、信頼された OpenSSL エンジンを薄くラップして提供します。

パスワードハッシュの Argon2id を利用するには、OpenSSL 3.2 で追加されたプロバイダが必要です。それより古い実行エンジン環境では、出力を一切生成せずに `Error.Code` を返します。AEAD（認証付き暗号）の復号（open）処理は「オール・オア・ナッシング（すべてかゼロか）」の原則に従っており、認証タグの検証に失敗した場合は部分的な平文であっても一切アクセスさせません。`constant_time_equal` は、入力された2つのデータが同じ長さである場合に限り、比較にかかる時間が一定（定数時間）になることを保証します（入力の「長さ」自体は隠蔽すべき秘密情報ではなく、公開情報として扱われます）。なお、高速なハッシュ関数である BLAKE3 は、監査済みの適切なシステムエンジンが安定して利用可能になるまでは標準ライブラリとして公開しません。

## high-throughput の building block

この「第3波」の標準ライブラリ群の追加と同時に、大規模なプログラムを構築する際に非常に有用となる「3つの新しい構成要素（ビルディングブロック）」も導入されました。

- **オフセット指定のファイル操作**： `fs.create_rw` や `fs.open_rw` によってランダムアクセス可能なファイルを開き、指定した位置から読み書きする `pread`、`pwrite`、およびファイルサイズを取得する `len` メソッド。
- **動的配列の構築**： データを読み込みながら最終的な要素数が決まっていくような処理のために、`push` や `append` で要素を追加し、最後に `build()` で所有権を消費して配列を完成させる `array_builder<T>`。
- **ストリーミング処理向けの最適化**： バッファリングされた `read_line`（行単位の読み込み）、アリーナのメモリを部分的に再利用するための `checkpoint` と `reset` 機構、そして前述した HTTP レスポンスのストリーミング機能。

システムを構築する際は、「実行したい処理を過不足なく表現できる、最も薄い（狭い）レイヤー」を選択してください。単なるバイト列の移動なら `reader` / `writer` を、ソケットレベルの制御が必要なら `std.net` を、HTTP のセマンティクスが必要なら `std.http` を選びます。それ以上の高度なルーティング、ミドルウェア、特定のアプリケーションプロトコル、Web フレームワークなどは、標準ライブラリ（`std`）の範疇ではなく、将来のパッケージエコシステム（`pkg`）が担うべき領域です。
