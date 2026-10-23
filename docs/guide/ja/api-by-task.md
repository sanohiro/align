# 用途から探すAPI索引

> 🌐 [English](../api-by-task.md) · **日本語**

やりたいことから、既存のAPI、使用例や解説、型・所有権・エラー・制限の詳しい契約を
探すための索引です。API欄は検索用の名前であり、そのまま実行できる呼び出し式では
ありません。実装済みの機能から選んで掲載しています。別のAPI仕様や、計画中の機能を
すべて並べた一覧ではありません。

- [配列やレコードを処理する](#配列やレコードを処理する)
- [文字列やデータ形式を扱う](#文字列やデータ形式を扱う)
- [ファイルを読み入力を受け取る](#ファイルを読み入力を受け取る)
- [サービスに接続し外部プログラムを動かす](#サービスに接続し外部プログラムを動かす)
- [パッケージでアプリを組み立てる](#パッケージでアプリを組み立てる)
- [プログラムを検査してテストする](#プログラムを検査してテストする)

`core` は言語の操作、`std` はOSやサービスとの境界を提供し、`pkg` はソースを
プロジェクトへコピーして使うライブラリです。`fs.read_file` などの標準ライブラリの
呼び出しには `import std.fs` が必要です。パッケージでは `import pkg.csv` の後も
`pkg.csv.decode` のように修飾した名前を使います。複数の操作を組み合わせるには、
[CSVからJSONを出すCLIの実践ガイド](practical-cli.md)を試してください。

## 配列やレコードを処理する

| やりたいこと | API・言語機能 | 使用例・解説 | 詳しい契約 |
|---|---|---|---|
| 値を絞り込み、変換し、集計する | `.where`, `.map`, `.sum`, `.reduce` | [パイプライン](06-pipelines.md) | [配列・スライス・パイプライン](../../impl/core-design/ja/array-slice-pipeline.md) |
| 2つの入力から、呼び出し側が持つ出力領域へ書く | `zip`, `.map_into` | [完全な例](../../../examples/zip_map_into.align) | [配列・スライス・パイプライン](../../impl/core-design/ja/array-slice-pipeline.md) |
| 所有配列をヘルパー関数で読み取る・更新する | `borrow`, `borrow mut`, `slice<T>` | [完全な例](../../../examples/borrowed_array.align) | [メモリと借用](05-memory.md) |
| 列ごとに処理する・キー別に集計する | `soa<T>`, `.group_by` | [データ指向プログラミング](11-data-oriented.md) | [SoAとグループ集計](../../impl/core-design/ja/soa-groupby.md) |
| 同じ Pure な関数を多数の要素へ並列に適用する | `.par_map` | [データ並列](10-closures-and-parallelism.md) | [言語仕様（英語）](../../../draft.md) |
| I/O を含む独立した処理を並行して実行し、結果を集める | `task_group`, `spawn`, `wait`, `.get` | [タスク並列](10-closures-and-parallelism.md) | [言語仕様（英語）](../../../draft.md) |
| 値がない場合や失敗を扱う | `Option`, `Result`, `else`, `?`, `match`, `.map_err` | [エラー](04-errors.md) | [OptionとResult](../../impl/core-design/ja/option-result.md) |

結果を保存したい場合は、明示的に実体化する終端操作を選びます。借用ビューは所有者の
寿命の範囲内で使います。`par_map` に渡す関数には Pure が必要です。
spawn したタスクは I/O を実行できますが、共有キャプチャはグループの join まで
読み取り専用です。

## 文字列やデータ形式を扱う

| やりたいこと | API・言語機能 | 使用例・解説 | 詳しい契約 |
|---|---|---|---|
| 元のデータがなくなった後も文字列を保持する | `str.clone()` → 所有文字列 `string` | [文字列とテキスト](07-strings-and-text.md) | [文字列とbuilder](../../impl/core-design/ja/string.md) |
| 連結を繰り返さずに文字列を組み立てる | `builder`, `.write`, `.write_int`, `.to_string` | [builderの例](07-strings-and-text.md) | [文字列とbuilder](../../impl/core-design/ja/string.md) |
| JSONから独立した所有フィールドを読み出す | `json.decode`, `string` / `array<T>` のレコードフィールド | [所有JSONの完全な例](../../../examples/json_owned.align) | [JSONの型と所有権](../../impl/core-design/ja/json.md) |
| JSONを借用文字列や列として読む | `json.decode`, `str`, `soa<T>`, `arena` | [JSON](08-json.md) | [JSONの型と所有権](../../impl/core-design/ja/json.md) |
| スキーマが未知の JSON を調べる・転送する | `json.doc`, `.get`, `.at`, `json.encode_options` | [ドキュメントビュー](08-json.md#スキーマが未知の場合) | [JSONの型と所有権](../../impl/core-design/ja/json.md) |
| JSONを出力する・出力バイト数に上限を設ける | `json.encode`, `json.encode_bounded` | [エンコード](08-json.md) | [JSON](../../impl/core-design/ja/json.md) |
| CSVを型付きの列へデコードする | `pkg.csv.decode`, `DecodeOptions` | [アプリ全体の例](practical-cli.md) | [CSV](../../impl/pkg-design/ja/csv.md) |
| 型付きの列をバイナリ形式で受け渡す | `codec.encoder`, `codec.open` | [使用例](../../impl/core-design/ja/codec.md) | [列指向codec](../../impl/core-design/ja/codec.md) |
| base64・hexを扱う、UTF-8を検証する | `encoding.base64_encode`, `encoding.hex_decode`, `.as_str` | [エンコーディング](14-std-encoding-rand-cli.md) | [標準ライブラリ仕様（英語）](../../../draft.md) |
| パターンで文字列を検索する | `regex.compile`, `.find`, `.is_match` | [正規表現](14-std-encoding-rand-cli.md) | [正規表現](../../impl/std-design/ja/regex.md) |
| UnicodeをNFCに正規化する | `pkg.unicode.nfc` | [パッケージのソース](../../../apps/unicode/pkg/unicode.align) | [Unicode正規化](../../impl/pkg-design/ja/unicode.md) |

JSONデコードの対象型は、呼び出しに型引数を書くのではなく、束縛の型注釈で指定します。
`str` のビューと所有フィールド `string` では寿命が異なります。扱える型の形は
JSONの契約を確認してください。任意のバイト列を文字列として読む操作は失敗し得ます。
UTF-8が必要な操作に渡すまでは、バイナリデータをバイト列のまま扱います。

## ファイルを読み入力を受け取る

| やりたいこと | API・言語機能 | 使用例・解説 | 詳しい契約 |
|---|---|---|---|
| 小さなUTF-8ファイルを読む・書く | `fs.read_file`, `fs.write_file` | [ファイル操作](13-std-os.md) | [ファイルシステム](../../impl/std-design/ja/fs.md)、[標準ライブラリ仕様（英語）](../../../draft.md) |
| 作業領域を制限してバイナリをコピーする | `reader.read`, `buffer.try_new`, `writer.write` | [ストリームのループ](13-std-os.md) | [I/O仕様（英語）](../../../draft.md) |
| コマンドラインのフラグを解析する | `cli.command`, `.flag_str`, `.flag_bool`, `.parse` | [アプリの入出力](practical-cli.md#3-コマンドラインの入出力を加える) | [CLI](../../impl/std-design/ja/cli.md) |
| 標準出力・標準エラーへ書き、失敗を検出する | `io.stdout`, `io.stderr`, `.write` | [アプリ全体の例](practical-cli.md) | [I/O仕様（英語）](../../../draft.md) |
| 上限を明示してディレクトリを走査する | `fs.open_directory`, `.cursor`, `.next` | [ツリー集計の例](../../../examples/tree_summary.align) | [ファイルシステム](../../impl/std-design/ja/fs.md) |
| 既存のエントリーを置き換えずにファイルを作る | `fs.create_exclusive`, `fs.rename_no_replace` | [ファイルの公開](13-std-os.md) | [ファイルシステム](../../impl/std-design/ja/fs.md) |

ファイル全体の読み取りでは、入力全体を保持する領域を確保します。`reader`、`writer`、
ディレクトリのハンドルがリソースを所有するか借用するかは、生成方法で決まります。
派生したビューを使う間は所有者を生かしておきます。ファイルや標準出力への書き込みは、
途中まで書いた後に失敗する場合があります。

## サービスに接続し外部プログラムを動かす

| やりたいこと | API・言語機能 | 使用例・解説 | 詳しい契約 |
|---|---|---|---|
| HTTPリクエストを送る | `http.client`, `.get`, `.request` | [HTTP](18-std-services.md) | [HTTP](../../impl/std-design/ja/http.md) |
| 上限を指定してHTTPのバイナリをダウンロードする | `.request_stream`, ストリームの `.read` | [取得処理の完全な例](../../../examples/http_fetch.align) | [HTTPストリーム受信](../../impl/std-design/ja/http.md) |
| TCP接続やホスト名の解決を行う | `import std.net` の `tcp.connect`, `dns.resolve` | [ネットワーク](18-std-services.md) | [ネットワーク](../../impl/std-design/ja/net.md) |
| 子プログラムを動かし、その出力を調べる | `process.command`, `.run`, `.code`, `.stdout` | [出力キャプチャの例](../../impl/std-design/ja/process.md) | [プロセス](../../impl/std-design/ja/process.md) |
| バイト列を圧縮する・ファイルを逐次ハッシュする | `compress.gzip_compress`, `crypto.sha256_stream` | [サービスとハッシュの例](18-std-services.md) | [圧縮](../../impl/std-design/ja/compress.md)、[暗号](../../impl/std-design/ja/crypto.md) |
| 明示したwriterへログを書く | `log.new`, `.line`, `.flush` | [ログ](18-std-services.md) | [ログ](../../impl/std-design/ja/log.md) |

HTTPのエラーステータスと子プロセスの非ゼロ終了コードは、結果のデータです。
通信や起動のエラーとは別に確認します。APIが提供する上限やタイムアウトを設定し、
その適用範囲も確認してください。タイムアウトが処理全体の期限になるとは限りません。
キャプチャした出力やHTTP本文のビューは、それを所有する結果ハンドルに依存します。

## パッケージでアプリを組み立てる

各パッケージは、内部モジュールと明記された依存関係を含めて、ソースのサブツリー全体を
コピーします。配置は[パッケージの説明](23-packages.md)を参照してください。
パッケージのソースは、コンパイラのバイナリには含まれていません。
リンク先の設計資料には実装済みの操作だけでなく、制限や将来に延期した機能もあります。
選ぶ操作のステータスと範囲を確認してください。

| やりたいこと | パッケージ | 使用例・解説 | 詳しい契約 |
|---|---|---|---|
| HTTPルート・ミドルウェア・SSEを提供する | `pkg.web` | [Webパッケージ](23-packages.md#pkgweb) | [Web](../../impl/pkg-design/ja/web.md) |
| WebSocketのサーバールートを追加する | `pkg.ws` と `pkg.web` | [WebSocket](23-packages.md#pkgws) | [WebSocket](../../impl/pkg-design/ja/ws.md) |
| 値をエスケープしてHTMLを組み立てる | `pkg.template` | [テンプレート](23-packages.md#pkgtemplate) | [テンプレート](../../impl/pkg-design/ja/template.md) |
| 型付きの列を結合する | `pkg.frame` | [結合のシグネチャと使用例](../../impl/pkg-design/ja/frame.md) | [Frame](../../impl/pkg-design/ja/frame.md) |
| SQLiteやPostgreSQLへクエリーする | `pkg.db` | [データベース入門](24-database.md) | [データベース](../../impl/pkg-design/ja/db.md) |
| トークンを検証する・パスワードをハッシュする | `pkg.auth` | [認証](23-packages.md#pkgauth) | [認証](../../impl/pkg-design/ja/auth.md) |
| RESP2のキーバリューサービスへ接続する | `pkg.kv` | [キーバリューの例](23-packages.md#pkgkv) | [キーバリュークライアント](../../impl/pkg-design/ja/kv.md) |

CSVとUnicodeのパッケージは[データ形式の表](#文字列やデータ形式を扱う)にあります。

## プログラムを検査してテストする

| やりたいこと | ツール・機能 | 使用例・解説 | 詳しい契約 |
|---|---|---|---|
| コードを複数のファイルに分ける | `module`, `import`, `pub` | [モジュール](09-generics-and-modules.md) | [言語仕様（英語）](../../../draft.md) |
| 実行前にプログラムを検査する | `alignc check`, `alignc check-per-unit`, `alignc build` | [アプリの作業手順](practical-cli.md#4-検査して実行ファイルを作る) | [ツールチェーン](16-toolchain.md) |
| 単体テストを書いて実行する | `test`, `test.expect`, `test.expect_eq`, `alignc test` | [モジュール内のテスト](practical-cli.md#2-計算とテストをモジュールに置く) | [テスト](../../impl/core-design/ja/test.md) |
| 性能を計測して生成コードを調べる | `alignc emit-llvm`, `alignc explain-opt`, `alignc size` | [性能ガイド](26-performance.md) | [最適化の説明（英語）](../../impl/09-explain-opt.md) |

短い独立したプログラムは[実行可能なexamples](../../../examples/README.md)から探せます。
順に読みたい場合は[ガイドの目次](README.md)へ戻ってください。
