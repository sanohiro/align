# パッケージ: vendoring、pkg.web、pkg.frame、pkg.auth、pkg.kv、pkg.db

> 🌐 [English](../23-packages.md) · **日本語**

`core` は言語のデータレイヤー、`std` は OS 境界、`pkg` はフレームワークやドメインライブラリを置くソースパッケージのレイヤーです。パッケージ基盤と first-party の `pkg.web`、`pkg.frame`、`pkg.auth`、`pkg.kv`、`pkg.db` は現在すでに利用できます。意図的にまだ存在しないのは、レジストリや取得ツールです。

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

Vendoring とは、このソースサブツリーを利用側プロジェクトへコピーすることです。このリポジトリの [apps/web/pkg](../../../apps/web/pkg)、[apps/frame/pkg](../../../apps/frame/pkg)、[apps/auth/pkg](../../../apps/auth/pkg)、[apps/kv/pkg](../../../apps/kv/pkg)、[apps/db/pkg](../../../apps/db/pkg) はパッケージ作者用ワークスペースなので、その `pkg/` ディレクトリをアプリケーションのルートへコピーまたはマージします。これらは `alignc` のアーカイブ、Debian パッケージ、Homebrew formula には埋め込まれていません。

パッケージ用のマニフェスト、lockfile、レジストリ、バージョンソルバ、ダウンロードコマンドはありません。依存グラフは `import` とファイルシステムから決まり、1つのソースツリーに存在できる `pkg/<name>` は1つです。依存関係の更新や監査は、vendoring したソース自体の更新や監査として行います。

## コンパイラが強制する2つの境界

コンパイラは各 import に対して、次の2つのパス規則を検査します。

- `internal` モジュールを import できるのは、その親をルートとするサブツリー内だけです。`pkg.web` は `pkg.web.internal.router` を import できますが、`main` や `pkg.auth` からはできません。
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

ハンドラへアプリケーション状態を渡す引数はまだありません。これは現在の制限であり、フレームワークが裏側に隠している機能ではありません。データベースアクセスは別パッケージ `pkg.db` が担当します（後述）。

## `pkg.frame`

`pkg.frame` は検証済み `core.codec` の i64 または string column に対して、上限付きで安定した inner join を行います。入力 batch を materialize/retain せず、source ordinal の owned `RowPair { left, right }` を left-major、right-ascending 順で返します。必須の `max_pairs` により、duplicate-key fanout と output allocation が可視になります。nullable/composite key、outer join、adaptive build-side selection、parallelism、spill は意図的に含みません。exact surface は [`pkg.frame` design](../../impl/pkg-design/ja/frame.md) にあります。

## `pkg.auth`

`pkg.auth` は監査済みの `std.crypto` primitive から、上限付き HS256 token、canonical Argon2id password record、opaque 256-bit session token の 3 protocol を組み立てます。claim は JSON テキストのまま扱い、policy はすべて call site に明示します。

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

検証は JSON parse より先に compact bytes を認証し、algorithm を HS256 に固定して signature を定数時間で比較します。必須の `now_ns` は hidden clock read なしに optional integer-form `exp` / `nbf` を検査します。password verify は Argon2 実行前に caller の 3 work ceiling を強制します。default password policy、key lookup、issuer/audience policy、cookie、session store は含みません。exact bound/error は [`pkg.auth` design](../../impl/pkg-design/ja/auth.md) が正本です。

## `pkg.kv`

`pkg.kv` は typed GET、SET、single-key DEL を扱う同期 plaintext RESP2 client です。connection / I/O timeout と maximum response size は明示的です。Move な `client` は同時に1つの `borrow mut` operation から使い、Drop を public close operation とします。

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

`get` は owned optional string、`set` は `Always` / `IfAbsent` / `IfPresent` condition が適用されたか、`delete` は key が存在したかを返します。transport、oversized response、malformed response は client を retire し、その後の operation は `Closed` を返します。default endpoint、authentication、TLS、RESP3 negotiation、generic command API、pipeline、retry、redirect、pool、transaction、script、pub/sub surface はありません。root `pkg.kv` と private 実装 `pkg.kv.internal.resource` は同時に出荷されます。exact bound/error rule は [`pkg.kv` design](../../impl/pkg-design/ja/kv.md) が正本です。

## `pkg.db` ― コミット済みロードマップ完了

`pkg.db` は first-party のデータベースパッケージで、vendoring の形は他の 4 つと同じです。[apps/db/pkg](../../../apps/db/pkg) に `pkg/db.align` があり、その下に `pkg.db.sqlite`、`pkg.db.postgres`、`pkg.db.pool` の各モジュールが並びます。

完了しているのは、公開初回リリースの範囲です。型付きの静的クエリとコマンドは、実際のスキーマメタデータに対してコンパイル時に検査され、SQLite と PostgreSQL の両方で実行でき、そのメタデータをオフラインで再生成できます。プリペアドステートメント、トランザクション、デッドラインとキャンセルを備えた型付き行ストリーム、一対多／多対一の複合出力、マイグレーションのライフサイクル管理、`EXPLAIN` を含む読み取り専用のカタログ検査も、すべて入っています。

コミット済みの全ロードマップも出荷済みです。上限付きのバッチ／SoA デリバリ、PostgreSQL ネイティブの single-row / portal-batch デリバリ、明示的な固定容量・待機なしの `pkg.db.pool`、ドライバ明示の動的 SQL、証明付き SQLite scalar function が入っています。最後のレール横断監査により、全 owner suite がローカルと CI の必須ゲートで走ります。より広い論理型、PostgreSQL COPY と callback、SQLite collation は D1〜D14 の未完了ではなく、consumer が具体化してから決める将来の面です。

コンパイラ側はすでに手元のバイナリに入っています。`alignc db prepare`、`db migrate`、`db status`、`db check`、`db repair`（第 [16](16-toolchain.md) 章）が、検査済みメタデータとマイグレーションカタログを操作します。契約の正本は `docs/impl/pkg-design/db.md` です。

出荷済みの範囲を実務でどう使うかは、課題ごとに整理した第 [24](24-database.md) 章が扱います。
