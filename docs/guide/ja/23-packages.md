# パッケージ: vendoring、pkg.web、pkg.jwt

> 🌐 [English](../23-packages.md) · **日本語**

`core` は言語のデータレイヤー、`std` は OS 境界、`pkg` はフレームワークやドメインライブラリを置くソースパッケージのレイヤーです。パッケージ基盤と first-party の `pkg.web`、`pkg.jwt` は現在すでに利用できます。意図的にまだ存在しないのは、レジストリや取得ツールです。

## パッケージはソースツリー

パッケージのルートは `pkg/<name>.align` で、必要に応じて `pkg/<name>/` 以下にサブモジュールを置きます。特別な解決方式はなく、通常のモジュール規則がそのまま働きます。

```text
main.align
pkg/
  jwt.align
  web.align
  web/
    types.align
    cookie.align
    internal/
      router.align
```

`import pkg.web` は `pkg/web.align`、`import pkg.web.cookie` は `pkg/web/cookie.align` に解決されます。呼び出しや型名は `pkg.web.get(...)`、`pkg.web.types.Ctx` のように常に完全修飾します。

Vendoring とは、このソースサブツリーを利用側プロジェクトへコピーすることです。このリポジトリの [apps/web/pkg](../../../apps/web/pkg) と [apps/jwt/pkg](../../../apps/jwt/pkg) はパッケージ作者用ワークスペースなので、その `pkg/` ディレクトリをアプリケーションのルートへコピーまたはマージします。これらは `alignc` のアーカイブ、Debian パッケージ、Homebrew formula には埋め込まれていません。

パッケージ用のマニフェスト、lockfile、レジストリ、バージョンソルバ、ダウンロードコマンドはありません。依存グラフは `import` とファイルシステムから決まり、1つのソースツリーに存在できる `pkg/<name>` は1つです。依存関係の更新や監査は、vendoring したソース自体の更新や監査として行います。

## コンパイラが強制する2つの境界

コンパイラは各 import に対して、次の2つのパス規則を検査します。

- `internal` モジュールを import できるのは、その親をルートとするサブツリー内だけです。`pkg.web` は `pkg.web.internal.router` を import できますが、`main` や `pkg.jwt` からはできません。
- `pkg/` 以下のモジュールが import できるのは `core`、`std`、または別の `pkg` モジュールだけです。利用側プロジェクトのモジュールへ逆向きに依存することはできません。

新しい可視性構文やビルド言語を追加せず、これらの規則だけでパッケージ内部を隠し、依存方向を一方向に保ちます。

## `pkg.web`

`pkg.web` は `std.http` 上に構築された zero-copy REST フレームワークです。通常のハンドラは、リクエストへのビューだけを持つ Copy なコンテキストを受け取り、レスポンスを構築して返します。リクエストハンドル自体はフレームワークが保持するため、未一致のパスを 404、メソッド不一致を 405、ハンドラの失敗を 500 に変換できます。

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

`serve(host, port, routes, workers)` により、並行度は呼び出し箇所で明示されます。worker が1つなら呼び出し元スレッドで実行し、複数なら別々の `SO_REUSEPORT` listener を使用します。ストリーミングルートには `stream`、Server-Sent Events 用の特殊形には `sse` を使います。不正なルートテーブルや実行不可能な worker 数は、プログラミングエラーとして起動時に abort します。

公開 companion module は、目的を絞った組み合わせ可能な機能を提供します。

- `pkg.web.cookie` はリクエスト Cookie を読み、ヘッダーインジェクションを検査した `Set-Cookie` 値を構築します。
- `pkg.web.cors` は CORS ポリシーを判定し、不正な wildcard と credentials の組み合わせを暗黙に許可しません。
- `pkg.web.multipart` は `multipart/form-data` の body を zero-copy な `Part` ビューとして走査します。アプリケーションが `pkg.web.body(c)` と反復オフセットを渡します。

ハンドラへアプリケーション状態を渡す引数やデータベースパッケージはまだありません。これらは現在の制限であり、フレームワークが裏側に隠している機能ではありません。

## `pkg.jwt`

`pkg.jwt` は HS256 による compact JSON Web Token を実装します。claim は JSON テキストのまま扱い、そのスキーマは `core.json`、署名付き envelope はこのパッケージが担当します。

```align
import pkg.jwt

pub fn main() -> Result<(), Error> {
    claims := "{\"sub\":\"42\",\"exp\":2000}"
    token := pkg.jwt.encode_hs256(claims, "secret")
    decoded := pkg.jwt.decode_hs256(token, "secret")?
    print(pkg.jwt.time_claims_valid(decoded, 1000))
    return Ok(())
}
```

検証時はトークン自身の `alg` フィールドを信頼せず、アルゴリズムを HS256 に固定し、署名を定数時間で比較します。`time_claims_valid` は、署名検証とは分離して、任意の `exp` と `nbf` NumericDate claim を検査します。HS384/512、RSA、ECDSA、公開プロバイダの OIDC 検証は、それぞれに必要な監査済み暗号プリミティブが揃うまで公開しません。
