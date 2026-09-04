# パッケージ: 用途に合わせて選び、ソースを取り込む

> 🌐 [English](../23-packages.md) · **日本語**

`core` は言語の基本操作、`std` は OS・サービスの API、`pkg` はアプリケーション向けのソースライブラリを提供します。以下のパッケージは実装済みです。用途に合わせて選び、ソースをプロジェクトへコピーして使います。レジストリや取得ツールはまだありません。

## パッケージはソースツリー

パッケージのルートは `pkg/<name>.align` で、必要に応じて `pkg/<name>/` 以下にサブモジュールを置きます。特別な解決方式はなく、通常のモジュール規則がそのまま働きます。

```text
main.align
pkg/
  db.align
  auth.align
  frame.align
  kv.align
  web.align
  db/
    sqlite.align
    postgres.align
    pool.align
  kv/
    internal/
      resource.align
  web/
    types.align
    cookie.align
    internal/
      router.align
```

`import pkg.web` は `pkg/web.align`、`import pkg.web.cookie` は `pkg/web/cookie.align` に解決されます。呼び出しや型名は `pkg.web.get(...)`、`pkg.web.types.Ctx` のように常に完全修飾します。

Vendoring は、利用側のプロジェクトにパッケージのソースを取り込むことです。次のワークスペースの `pkg/` ディレクトリを、アプリケーションのルートへコピーまたはマージします。

| 用途 | パッケージ | ソースの場所 |
|---|---|---|
| HTTP ルート、ミドルウェア、SSE | `pkg.web` | [apps/web/pkg](../../../apps/web/pkg) |
| WebSocket サーバーのルート | `pkg.ws` と依存先の `pkg.web` | [apps/ws/pkg](../../../apps/ws/pkg) と [apps/web/pkg](../../../apps/web/pkg) |
| エスケープ付き HTML の生成 | `pkg.template` | [apps/template/pkg](../../../apps/template/pkg) |
| CSV から型付きの列へ変換 | `pkg.csv` | [apps/csv/pkg](../../../apps/csv/pkg) |
| 型付きの列の結合 | `pkg.frame` | [apps/frame/pkg](../../../apps/frame/pkg) |
| トークン、パスワードハッシュ、セッショントークン | `pkg.auth` | [apps/auth/pkg](../../../apps/auth/pkg) |
| RESP2 によるキー・値の操作 | `pkg.kv` | [apps/kv/pkg](../../../apps/kv/pkg) |
| SQLite と PostgreSQL | `pkg.db` | [apps/db/pkg](../../../apps/db/pkg) |

これらのソースは、`alignc` のアーカイブ、Debian パッケージ、Homebrew formula には含まれません。新しく追加されたパッケージを試すときは、それに対応したコンパイラを使ってください。配布済みリリースがリポジトリの最新状態に追い付いていない場合があります。ルートファイルだけでなく、内部モジュールも一緒にコピーします。

パッケージ用のマニフェスト、lockfile、レジストリ、バージョンソルバ、ダウンロードコマンドはありません。依存グラフは `import` とファイルシステムから決まり、1つのソースツリーに存在できる `pkg/<name>` は1つです。依存関係の更新や監査は、vendoring したソース自体の更新や監査として行います。

## コンパイラが強制する2つの境界

コンパイラは各 import に対して、次の2つのパス規則を検査します。

- `internal` モジュールを import できるのは、その親をルートとするサブツリー内だけです。`pkg.web` は `pkg.web.internal.router` を import できますが、`main` や `pkg.auth` からはできません。
- `pkg/` 以下のモジュールが import できるのは `core`、`std`、または別の `pkg` モジュールだけです。利用側プロジェクトのモジュールへ逆向きに依存することはできません。

新しい可視性構文やビルド言語を追加せず、これらの規則だけでパッケージ内部を隠し、依存方向を一方向に保ちます。

## `pkg.web`

`pkg.web` は `std.http` を使うゼロコピーの REST フレームワークです。通常のハンドラは、リクエストへのビューからなる Copy 型のコンテキストを受け取り、レスポンスを作って返します。リクエストハンドルはフレームワークが保持し、一致するパスがなければ404、メソッドが合わなければ405、ハンドラが失敗すれば500を返します。

```align
import pkg.web
import pkg.web.types

fn hello(c: pkg.web.types.Ctx) -> Result<response_builder, Error> {
    return pkg.web.text(pkg.web.param(c, "name"))
}

pub fn main() -> Result<(), Error> {
    routes := [
        pkg.web.get("/hello/:name", hello),
    ]
    return pkg.web.serve("127.0.0.1", 8080, routes, 1)
}
```

ルートコンストラクタは一般的な HTTP メソッドと `any` を提供し、パターンは静的セグメント、`:param`、末尾の `*wildcard` を扱います。`group` と `group_with` はプレフィックスと宣言順のミドルウェアを追加します。リクエストアクセサには `param`、`query`、`has_query`、`header`、`body`、`body_str` があり、レスポンダには `text`、`json`、`status`、`status_text`、`status_json` があります。

`serve(host, port, routes, workers)` でワーカー数を明示します。1つなら呼び出し元スレッドで実行し、複数ならそれぞれの `SO_REUSEPORT` リスナーを使います。ストリーミングには `stream`、SSE にはその専用形である `sse` を使います。不正なルートテーブルや実行できないワーカー数は、プログラミングエラーとして起動時に異常終了します。

関連する公開モジュールには、次の機能があります。

- `pkg.web.cookie` はリクエスト Cookie を読み、ヘッダーインジェクションを検査した `Set-Cookie` 値を構築します。
- `pkg.web.cors` は CORS ポリシーを判定し、認証情報を許可する設定とワイルドカードの不正な組み合わせを拒否します。
- `pkg.web.multipart` は `multipart/form-data` のボディを、ゼロコピーの `Part` ビューとして走査します。アプリケーションが `pkg.web.body(c)` を渡し、走査位置を管理します。

ハンドラへアプリケーション状態を渡す引数は、まだありません。データベースアクセスには、後述する `pkg.db` を使います。

## `pkg.ws`

`pkg.ws` は `pkg.web` に WebSocket サーバーのルートを追加します。上の表にある両方のソースが必要です。REST や SSE と同じルート配列に `pkg.ws.route(pattern, protocols, pump)` を置きます。pump 関数は `http_upgrade` 接続の所有権と、選ばれたプロトコルの文字列を受け取ります。`pkg.ws.receive` にメッセージのサイズ上限を渡すと、完成した `Text`、`Binary`、`Close` のいずれかが返ります。返信には `send_text`、`send_binary`、`close` を使います。

接続中の WebSocket は、サーバーのワーカーを1つ使い続けます。通常の HTTP リクエストに加えて、同時に維持する接続数も考えてワーカー数を決めてください。接続ごとにスレッドが自動生成されることはありません。これはサーバー用で、WebSocket クライアントではありません。pump の例とハンドシェイクの条件は[WebSocket の設計](../../impl/pkg-design/ja/ws.md)を参照してください。

## `pkg.template`

HTML を生成するときは、外部のテキストを言語の `template` に直接埋め込まず、`pkg.template` でエスケープします。パッケージを取り込んだ後は、次のプログラムをそのまま実行できます。

```align
import pkg.template

fn greeting(name: str) -> string {
    mut output := pkg.template.html()
    pkg.template.raw(output, "<p>")
    pkg.template.write(output, name)
    pkg.template.raw(output, "</p>")
    return pkg.template.to_string(output)
}

fn main() -> i32 {
    print(greeting("<Align>"))
    return 0
}
```

出力は `<p>&lt;Align&gt;</p>` です。`write` はテキストをエスケープし、`raw` は信頼できるマークアップをそのまま追加します。`to_string` は Move 型の builder を消費し、所有権のある文字列を返します。エスケープは要素の本文や、すでに引用符で囲まれた属性値に使えますが、URL、JavaScript、CSS、属性名の検証はしません。言語の `template` はテキストを整形する機能で、HTML エスケープは行いません。詳細は[HTML builder の設計](../../impl/pkg-design/ja/template.md)にあります。

## `pkg.csv`

`pkg.csv.decode` は、メモリ上の UTF-8 の CSV 文書を `soa<R>` に読み込みます。書き込み先の名前付きアリーナと、`header`、`line_ending`、`max_rows` を持つ `pkg.csv.DecodeOptions` を渡します。既定値の推論はありません。ヘッダーには `Header.Present` / `Header.Absent`、改行には `LineEnding.Lf` / `LineEnding.CrLf` のどちらかを明示します。行の型は `rows: soa<Trade>` のような型注釈で指定します。JSON のデコードと同じ考え方です。

ヘッダーがあれば名前で列を選び、なければ構造体のフィールドの宣言順と列数に一致する必要があります。結果は書き込み先のアリーナに属します。ファイルを読む場合は、別途 `std.fs` を使います。エラーは `pkg.csv.Error` なので、組み込みの `Error` を返す `main` では処理するか変換してください。この API は文書全体のデコード用で、ストリーミングや CSV の生成は行いません。詳細と使用例は[CSV の設計](../../impl/pkg-design/ja/csv.md)にあります。

## `pkg.frame`

`pkg.frame` は検証済みの `core.codec` の i64 列または文字列列に対して、結果数に上限のある内部結合を行います。結果の `RowPair { left, right }` は入力の行番号の組で、結果自体がその配列を所有します。左の行番号順、同じ左行の中では右の行番号順に並びます。入力バッチを実体化したり保持し続けたりはしません。必須引数 `max_pairs` で、重複キーによる結果数の増加とメモリ確保量に上限を設けます。null を含むキー、複合キー、外部結合、ハッシュ表を作る側の自動選択、並列化、ディスクへの退避には対応していません。詳細は [`pkg.frame` の設計](../../impl/pkg-design/ja/frame.md) を参照してください。

## `pkg.auth`

`pkg.auth` は `std.crypto` の監査済み機能を使い、HS256 トークン、規定の形式の Argon2id パスワードレコード、不透明な256ビットのセッショントークンを扱います。それぞれに処理量や入力の上限があります。クレームは JSON テキストで扱い、認証の方針は呼び出し側で明示します。

```align
import pkg.auth
import std.encoding

fn main() -> Result<(), Error> {
    key := encoding.hex_decode("000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f")?
    claims := "{\"sub\":\"42\",\"exp\":2000}"
    token := pkg.auth.encode_hs256(claims, key.bytes())?
    verified := pkg.auth.verify_hs256(token, key.bytes(), 1000000000000)?
    print(verified)

    policy := pkg.auth.Argon2Policy{m_cost: 65536, t_cost: 3, parallelism: 1}
    phc := pkg.auth.password_hash("correct horse".bytes(), policy)?
    print(pkg.auth.password_verify("correct horse".bytes(), phc, policy)?)
    print(pkg.auth.session_token())
    return Ok(())
}
```

この例のパスワード処理には、Argon2id を使える OpenSSL 3.2 以降が必要です。第 [01](01-getting-started.md) 章を参照してください。

JSON の解析より前にトークンのバイト列を認証し、アルゴリズムを HS256 に固定して署名を定数時間で比較します。整数形式の `exp` / `nbf` クレームがあれば、必須引数 `now_ns` を使って検査します。内部で時計は読みません。パスワード検証では Argon2 の実行前に、呼び出し側が指定したメモリ量、反復回数、並列度の3つの上限を検査します。既定のパスワード方針、鍵の検索、発行者や対象者の検査方針、Cookie、セッション保存機能は含みません。上限とエラーの詳細は [`pkg.auth` の設計](../../impl/pkg-design/ja/auth.md) にあります。

## `pkg.kv`

`pkg.kv` は GET、SET、単一キーの DEL を扱う、型の定まった同期 RESP2 クライアントです。通信は平文です。接続と I/O のタイムアウト、レスポンスの最大サイズを明示します。Move 型の `client` は `borrow mut` で借用して一度に1つの操作を行い、drop 時に接続を閉じます。

```align
import pkg.kv

fn open_store() -> Result<pkg.kv.client, pkg.kv.Error> {
    options := pkg.kv.ClientOptions {
        connect_timeout_ns: 1000000000,
        io_timeout_ns: 1000000000,
        max_response_bytes: 1048576,
    }
    return pkg.kv.connect("127.0.0.1", 6379, options)
}

fn create_session(
    borrow mut store: pkg.kv.client,
    key: str,
    payload: str,
) -> Result<bool, pkg.kv.Error> {
    options := pkg.kv.SetOptions {
        condition: pkg.kv.SetCondition.IfAbsent,
        expires_in_ns: Some(900000000000),
    }
    return pkg.kv.set(store, key, payload, options)
}
```

`get` は所有権のある文字列を `Option` で返します。`set` は `Always` / `IfAbsent` / `IfPresent` の条件に従って設定したか、`delete` はキーが存在したかを返します。通信の失敗、サイズ超過、不正なレスポンスの後はクライアントを使えなくなり、以降の操作は `Closed` を返します。既定の接続先、認証、TLS、RESP3 の交渉、任意のコマンドを送る API、パイプライン、再試行、リダイレクト、プール、トランザクション、スクリプト、pub/sub は提供しません。公開の `pkg.kv` と非公開の実装 `pkg.kv.internal.resource` は一緒に配布します。上限とエラーの詳細は [`pkg.kv` の設計](../../impl/pkg-design/ja/kv.md) にあります。

## `pkg.db`

`pkg.db` は Align が提供するデータベースパッケージで、他と同じようにソースをコピーして使います。[apps/db/pkg](../../../apps/db/pkg) に `pkg/db.align` と、`pkg.db.sqlite`、`pkg.db.postgres`、`pkg.db.pool` の各モジュールがあります。

`pkg.db` は SQLite と PostgreSQL に対応する、型付きの静的クエリとコマンドを提供します。`alignc db prepare` で作ったスキーマメタデータを使い、コンパイル時にクエリを検査できます。プリペアドステートメント、トランザクション、型付きの行ストリーム、一対多・多対一の結果、マイグレーション、`EXPLAIN` を含む読み取り専用のカタログ検査にも対応します。

大量の結果には、上限付きのバッチと SoA ビュー、PostgreSQL の1行単位・バッチ単位の取得、固定容量で待機しないプール、明示的な動的 SQL を使えます。PostgreSQL の実行はデッドラインとキャンセルに対応し、SQLite のスカラー関数にはコンパイラが検査したコールバックを使います。より多くの論理型、PostgreSQL COPY とコールバック、SQLite の照合順序は、具体的な利用者の要件に基づいて設計する今後の機能です。

コンパイラ側には `alignc db prepare`、`db migrate`、`db status`、`db check`、`db repair` があります（第 [16](16-toolchain.md) 章）。これらでスキーマメタデータとマイグレーションを管理します。契約の正本は `docs/impl/pkg-design/db.md` です。

具体的な使い方は、第 [24](24-database.md) 章で作業ごとに説明します。
