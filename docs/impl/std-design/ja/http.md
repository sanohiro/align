このディレクトリには、ロードマップの本文だけでは足りない std モジュールについて、Opus がそのまま実装に
移せる粒度の設計仕様を収めている。執筆はメインループ (Fable) が担当しており、各モジュールを実装する際は
これが source of truth となる。

# std.http — implementation design (M11)

> 🌐 [English](../http.md) · **日本語**

## Overview

HTTP/1.1 のプリミティブであって、フレームワークではない(draft §18.2)。std.net のソケットの上に構築す
る。メンバーは request、response、header、method、status、client、server プリミティブ。コネクション再利用
は net のレールに従う。**クライアント側の HTTPS/TLS は出荷済み**(スライス 5):`https://` は
`cl.get/post/request` + `cl.get_many` を通じてそのまま動作し、OpenSSL libssl 上で(システム信頼ストアに
対する必須の検証 + ホスト名バインディングを伴って)crypto の libcrypto と並んで動的リンクされる。サーバ側
TLS はクライアント優先で先送り。HTTP/3、ルーティング、ミドルウェアは std ではなく pkg である。

**モジュール状態:COMPLETE**(スライス 1–6 出荷済み。クライアント側 TLS はスライス 5)。サーバ側 TLS、
クライアント証明書、カスタム CA、セッション再開、失効確認は記録済みの v1 後バックログ。

## Signatures

v1 案で、Fable が確定させた形:

```text
// Client
cl := http.client()                         // owns a connection pool (Move)
cl.get(url: str) -> Result<response, Error>
cl.post(url: str, body: bytes) -> Result<response, Error>
cl.request(req: request) -> Result<response, Error>
// Request/response building
r := http.request(method: str, url: str)    // builder (Move — owns header list + body buf)
r.header(name: str, value: str)
r.body(data: bytes)
resp.status() -> i64
resp.header(name: str) -> Option<str>       // view into resp
resp.body() -> bytes                         // view into resp (region-bound)
// Server primitive (not a framework) — surface settled 2026-07-10 (two-lens design review)
srv := http.serve(host: str, port: i64) -> Result<http_server, Error>
srv.accept() -> Result<http_request_ctx, Error>   // one request; caller writes the response
ctx.method() -> str                          // view into ctx (region-bound)
ctx.path() -> str                            // view into ctx (region-bound)
ctx.header(name: str) -> Option<str>         // view into ctx (region-bound)
ctx.body() -> bytes                          // view into ctx (region-bound)
rb := http.response(status: i64)             // response_builder (Move — owns header list + body buf;
                                             // the build-dual of `request`; named apart from the
                                             // parsed read-view `response`)
rb.header(name: str, value: str)             // bound receiver; CR/LF/NUL aborts (P6)
rb.body(data: bytes)                         // optional — a header-only response is legal
ctx.respond(rb) -> Result<(), Error>         // consumes BOTH ctx and rb; one-write serialize (R4);
                                             // closes the accepted fd (v1: one request per conn)
// Batched client (the rail — moved here from net; see Concurrency in net.md)
cl.get_many(urls: slice<str>, max_concurrency: i64) -> Result<array<response>, Error>
```

## Type & ownership classification

- `client`、`request`、`http_server`、`http_request_ctx`、`response_builder` は **Move 型** である
  (プールしたコネクション、ヘッダーリスト、ボディバッファ、listen 中または accept 済みのソケットを所有
  する)。根拠は reader/writer の Move の前例に加えて、これらが包む net の Move 型である。`response_builder`
  は、パース済みの読み取りビューである `response` とはあえて別の型にしている: build(ヘッダーリスト →
  シリアライズ)と parse(オフセットテーブル → ビュー)が同じ利用箇所を共有することは決してないので、
  1 つの型に多重定義するとすべてのゲッターに内部の Parsed|Built 分岐を足すだけで、収束の利得はゼロになる。
  意味のある対称性は方向によるものであり、それは保たれている: `response_builder` ≅ `request`(ビルダー)、
  `http_request_ctx` の読み取り ≅ `response` の読み取り(ビュー)。
- `ctx.method()/path()/header()/body()` は **ctx にリージョン束縛されたビュー** を返す(#297 の分岐)。
  これらは `resp.status()/header()/body()` のちょうど読み取り側の双対である。
- `response` は自身のヘッダーブロックとボディバッファを所有する(Move)。`resp.header()`/`resp.body()` は
  **resp にリージョン束縛されたビュー** を返す(#297 を意識した `region_of` 分岐 — net の借用した
  reader/writer や `json.decode` と同じ)。
- Move の拒否は `scalar_arg` のチョークポイントで行うが、自分のコンストラクタが返す Result の Ok 位置だけ
  は例外とする(net のテンプレート)。

## Effect classification

すべて impure である(net 経由のネットワーク syscall)。

## Error policy

トランスポート層のエラーは std.net から伝播する(errno→Error テーブル)。HTTP レベルのエラー(不正な
レスポンス、不正なステータス行)は `Error.Invalid` にする。4xx/5xx のステータスはエラーでは**ない** —
それはそのステータスを持つ有効なレスポンスであり、呼び出し側が `resp.status()` で分岐する。`Err` になる
のはトランスポート/パースの失敗だけである。(これは意図的な One-way の判断である: HTTP のステータスは
データであって、Result のエラーではない。)

## Performance requirements (owner directive, 2026-07-07 — requirements, not aspirations)

オーナーは std.http を **速く** したいと考えている。`open-questions.md` に記録された計測済みのレール
(外部の design-note レビュー: keepalive 1.48×、pipeline 化した write-then-read 19.1×、並行数を絞った
`get_many` は 64 リクエストで 12.8×)は、std の残りがすでに従っているゼロコピーの規律に加えて、v1 の
エンジニアリング要件である。具体的には次のとおり。

- **R1 — ゼロコピーのレスポンス**: 所有するレスポンスバッファは 1 つ。status 行 / ヘッダー / ボディは
  **オフセットテーブル + そのバッファへのビュー** としてパースする(ヘッダーごとの `string` 割り当ても、
  ボディのコピーも無い)。`resp.header()`/`resp.body()` はすでにリージョン束縛のビューを返している —
  内部表現も実際にゼロコピーでなければならない。
- **R2 — 初日から SIMD 裏打ちのスキャン**: ヘッダー/行のスキャンは、ランタイム既存の memchr レイヤー
  (#310: AVX2+NEON+scalar、`str` 検索向けにすでに出荷済み)に乗せる — CRLF / `:` は memchr で見つけ、
  1 バイトずつのスカラーループは決して使わない。simdjson 流の完全な構造的スキャン(JSON と共有する
  バイト分類器)は後日の最適化として記録にとどめる。memchr は今日ただで使える。
- **R3 — デフォルトでコネクション再利用**: プール(Slice 3)はオプションではなく要件である —
  同じ host:port への `cl.get()` は、オプトイン無しで生きているコネクションを再利用する(keepalive)。
  計測された 1.48× は下限であり、pipeline 化した 19.1× の形は `get_many` のバッチ処理がその上に築くもの
  である。
- **R4 — ホットパスの syscall 規律**: クライアントのコネクションに `TCP_NODELAY`(Nagle でリクエストの
  末尾を遅延させない)。リクエスト全体(start-line + ヘッダー + ボディ)を 1 つのバッファにシリアライズ
  し、**1 回の write** で送る(ヘッダーごとの write は無し)。ソケットの読み出しは M9 のバッファ付き
  reader を通す(行ごとの read syscall は無し)。
- **R5 — `get_many` = task_group + ParPool の claim ループ**(#301)で並行数を絞る — 計測された 12.8× の
  I/O オーバーラップの形。新しい async ランタイムでは**ない**。`io_uring` は記録済みの決定どおり、後日の
  Linux バックエンドにとどめる。
- **R6 — ベンチマークで完了をゲートする**: `bench/http_client` のハーネス(ローカルの平文サーバ。
  keepalive GET のレイテンシ/スループット + `get_many` のスケーリング)を Rust ベースラインに照らして
  計測する — このリポジトリの「主張の前に計測」ルールに従い、数値が README に載るまでモジュールは
  「速く仕上がった」とは言わない。**R6 はスライス 3 の時点で満たされた:** `bench/http_client` は出荷済みで
  (出荷したプールをその C-ABI エントリポイント経由でインプロセスの localhost サーバに対して駆動する)、
  **keepalive で 2.86× 高速化**(下限 1.48× — 達成)と、再利用パスでの **手書き Rust `std::net` と同等**を
  記録している(`bench/http_client/README.md` を参照)。**`get_many` のスケーリング部分も今や達成された
  (2026-07-10、R5 のスライス):** 12 ms のレイテンシを注入した状態で degree 16 の 64 GETs — **15.4× の
  オーバーラップ**(理想 ≈ degree)、**同 degree の Rust スレッドプールの 1.01×**(同等)。正直な報告のための
  但し書きはベンチの README にある(degree とコア数を添えて引用すること)。R6 はこれで完全に満たされた。

## New machinery required

上記の Move 型 + net のソケット上での HTTP/1.1 のパース/シリアライズ + コネクションプールの再利用。新しい
I/O パスは要らない(net の reader/writer を使う)。TLS ラッパーは先送り(HTTPS を塞ぐ)。ヘッダーのパース
は **R2** に従い memchr 裏打ちのスキャンとする(完全な構造的スキャン/バイト分類器への格上げは後日に記録)。

## Slice breakdown

1. request/response の構造体 + ヘッダーリスト + HTTP/1.1 のシリアライズ/パース(ソケットはまだ不要 —
   純粋なエンコード/デコードとして単独でテストできる)。**完了**(ブランチ `m11-http-slice1-parse`)。
   公開された表面: `http.request(method, url)`(全域 — URL の検証はここではなくシリアライズ時に行うので、
   実行時に渡された URL でビルダーが abort することはない)、`r.header(name, value)` / `r.body(data)`
   (ハンドル経由でその場で変更、レシーバは束縛済みが必要、P6 の CR/LF/NUL は abort)、
   `http.parse(bytes) -> Result<response, Error>`(response のコンストラクタ兼コーデックの基本要素 —
   スライス 2 のクライアントも同じエンジンを再利用する。使い捨てではなく恒久的な基本要素)、
   `resp.status()` / `resp.header(name)`(大文字小文字を無視する `Option<str>` のビュー)/ `resp.body()`
   (`slice<u8>` のビュー)— 2 つのゲッターはどちらも `resp` にリージョン束縛される(#297)。シリアライズは
   **ランタイム専用のコーデック**(`align_rt_http_serialize`、R4 に沿った 1 本の連続バッファ、単体テスト済み)
   のままで、スライス 2 のクライアントがそれをレンダリングして 1 回の write で送る — まだ言語のビルトインでは
   ない。スライス 1 の演算はすべて **Pure**(ソケットなし)。`Host` と `Content-Length`(ボディが非空のとき)は
   自動付与し、呼び出し側が `Host`/`Content-Length` を指定した場合は拒否する(CL 重複によるスマグリング対策)。
   `chunked` の Transfer-Encoding は `Error.Invalid`(v1 は Content-Length フレーミングのみ。R1 を守る
   デチャンクは先送り)。上限: ヘッダー 128 個以下、ボディ 1 GiB 以下。R1 ゼロコピー: response は 1 本の
   バイトバッファ + オフセット表を所有し、スキャンは `memchr` クレート(R2)に載せる。
2. client + 1 つの net の `tcp_conn` 上での get/post(平文)。**完了**(ブランチ
   `m11-http-slice2-client`)。提供する API(`import std.http` の下、すべて **非純粋** — ネットワーク):
   `http.client()`(Move の `http client` ハンドル。v1 では ZST — プール状態はまだ持たないが、FFI
   のエントリポイントはすでに `*mut HttpClient` を受け取るので、スライス 3 は同じ言語表面のままプールを
   追加できる)、`cl.get(url) -> Result<response, Error>` / `cl.post(url, body) -> Result<response,
   Error>` / `cl.request(req) -> Result<response, Error>`(バインド済みレシーバのゲート。`cl` は借用、
   `request` は Move の `req` を**消費する**)。各リクエストは 1 本の新しい net `tcp_conn` 上で実行する:
   connect(`align_rt_tcp_connect` を再利用 — DNS + connect + SO_KEEPALIVE)→ **TCP_NODELAY**(R4)→
   シリアライズ済みリクエストの **1 回の write**(R4。スライス 1 の `http_serialize_core` 経由 — Host と
   Content-Length を自動付与し、メソッド/ヘッダー/スマグリングを検証)→ レスポンスをソケットから 32 KiB
   ずつ(1 行ずつではなく — R4)Content-Length まで読み、スライス 1 の `http_parse_core`(R1 ゼロコピー)で
   パースする。4xx/5xx は `Ok(response)`(P2)。`https://` や不正な URL はリクエスト時点で `Error.Invalid`
   (P1 — 黙って平文にダウングレードしない)。フレーミングは Content-Length(または read-to-close)。
   chunked は `Error.Invalid` のまま(スライス 1 の方針)。パーサはストリーミング読み取りが「もっとバイトが
   必要」と「不正」を 1 つの共通デコーダで区別できるよう、`Incomplete`/`Invalid` の 2 分岐にリファクタした。
   プールはまだなし(各リクエストは新規接続して閉じる — keepalive の再利用はスライス 3)。`get_many` /
   server / HTTPS は残る。
3. コネクションプールの再利用(レール — keepalive、デフォルトで再利用)。**完了**(ブランチ
   `http-slice3-pool`)。`http.client()` はもう ZST ではなく、変わらない言語表面・FFI ABI の裏で
   **keepalive のコネクションプール**(`Mutex<HashMap<(host, port), Vec<IdleConn>>>`)を所有する
   (コンパイラは `HttpClient` を不透明なハンドルポインタとして扱っているため、このスライスは純粋な
   ランタイム変更 — sema/MIR/codegen の編集は無い)。同じ `(host, port)` への連続した
   `get`/`post`/`request` は、オプトイン無しで生きている idle コネクションを**再利用する**(R3)。
   `Drop`(`align_rt_http_client_free`)はプール内のすべてのコネクションを閉じる(P5)。**再利用判定
   (正しさに直結 — 汚れたコネクションを再利用すると次のレスポンスをミスフレームする):** 完了した
   コネクションは、keep-alive(HTTP/1.1 のデフォルト。`Connection: close` や非 1.1 は再利用しない —
   レスポンスヘッダから `http_head_keep_alive` が判定)**かつ** Content-Length フレーミング
   (read-to-close はコネクション終端で終わる → 再利用しない)、フレーム済みメッセージを超える余剰バイトが
   無い(余剰 ⇒ 汚れ ⇒ 破棄)、**かつ** そのレスポンスが**完全にパースできた**場合にのみプールへ戻す —
   プール判定は `http_parse_core` の**後**に走るので、ストリーミングのパスは通したが所有パースが拒否した
   (信頼できないストリームの)コネクションは閉じ、決してプールしない。**スタール再試行:** サーバが既に
   落としていた再利用 idle コネクションは、いずれのレスポンスバイトも受け取る前に失敗する。その一件だけ
   新規コネクションで一度だけ透過的に再試行する(リクエストはほぼ確実に未処理 — idle クローズの競合)。
   再試行は**プールをバイパスする**(常に新規 connect。サーバ再起動後は同一ホストに死んだコネクションが
   複数あり得るため、2 本目のプール済みコネクションは決して引かない)。新規コネクションの失敗やレスポンス
   途中の失敗はそのまま返す。**SIGPIPE:** クライアントの書き込み経路は `send(MSG_NOSIGNAL)`(Linux)/
   `SO_NOSIGPIPE`(macOS)を使い、落ちた再利用コネクションへの書き込みはプロセスを殺さず `EPIPE`
   (→ 再試行)を返す(グローバルなシグナルハンドラは入れない)。**プール上限/衛生:** ホストあたり idle は
   8 本まで。90 秒より古い idle は take **と** put の両方で回収する(新鮮なコネクションを stale のために
   捨てない。超過分は回収後にのみ閉じる)。空になったバケットのキーは map から削除する(多数のホストに
   接続しても空 `Vec` が無制限に増えない)。**R6 達成:** `bench/http_client`(下記)がプールを
   **keepalive で 2.86× 高速化**(下限 1.48×)、**手書き Rust `std::net` と同等**と記録した。テスト:
   `align_runtime` のユニット(1 コネクションで 3 gets を再利用/ `Connection: close` はプールしない/
   スタール再試行/ `http_head_keep_alive` の判定表)+ ドライバテスト(2 gets が 1 コネクションを再利用、
   サーバの accept 数で観測)。
4. server プリミティブ(serve/accept、レスポンスは呼び出し側が書く)。**完了**(ブランチ
   `http-slice4-server`)。提供する API(`import std.http` の下、server の演算は **非純粋**):
   `http.serve(host, port) -> Result<http_server, Error>`(listen 中の fd を所有する Move ハンドル —
   net の `tcp.listen` を包み、SO_REUSEADDR + backlog 128 の後に fd を取り出す)。`srv.accept() ->
   Result<http_request_ctx, Error>`(accept 済みの fd + ゼロコピーのオフセットテーブルにパースしたリクエストを
   所有する Move ハンドル。`HttpResponse` の R1 の鏡像 — head の終端まで 32 KiB ずつストリーミング read +
   Content-Length によるボディフレーミング。Incomplete/Invalid の分岐と 256 KiB-head / 128-header /
   1 GiB-body の上限を再利用する。不正なリクエストはそのコネクションを閉じて `Error.Invalid` を返し、
   リスナーは生き続ける)。`ctx.method()/path()`(`str` ビュー)、`ctx.header(name)`(大文字小文字を無視する
   `Option<str>` ビュー)、`ctx.body()`(`slice<u8>` ビュー)— すべて `ctx` にリージョン束縛される(#297)。
   `http.response(status)` -> `response_builder`(Move。パース済みの `response` とは別の Ty + 表示名)+
   `rb.header(name, value)`(バインド済みレシーバ、P6 の CR/LF/NUL は **abort**)+ `rb.body(data)`(任意)。
   `ctx.respond(rb) -> Result<(), Error>`(ctx と rb の **両方を消費する** — `cl.request(req)` と同様に
   MIR が両スロットを null にする。シリアライズ = ステータス行 + ヘッダー + ボディがセットされた場合にのみ
   自動 Content-Length。1 回の write、R4。MSG_NOSIGNAL/SO_NOSIGPIPE。fd を閉じる、v1 は 1 コネクション
   1 リクエスト)。`METHOD SP target SP HTTP/1.1` 向けの **新規** `http_parse_request_head` が、下記の
   5 つの inbound スマグリング対策をすべて実装する。**3 つの新しい Move 型**
   (`http_server`/`http_request_ctx`/`response_builder`)は Gate-1 の twin-mirror スイープ一式を通した
   (2 つの Result ペイロード向けの Ty + Scalar。`response_builder` は `http request` と同じく Ty のみ。
   respond の二重消費に対する `null_moved_source` が見落としやすい分岐だった)。テスト: `align_runtime` の
   ユニット(request-head パーサ + 5 つのガードそれぞれ + シリアライズのフレーミング + N サイクルにわたる
   fd リーク)+ ドライバの e2e(`m11_http_server.rs`: Rust クライアントで駆動する Align サーバ、**さらに
   出荷した Align の `cl.get` クライアントを Align サーバに対して回すドッグフード実行**、加えて Gate-1 の
   コンパイル拒否)。**確定した記録からの調整が 2 点、いずれもここに記録する:**(1)リクエスト行のパーサは
   `HTTP/1.0` **と** `HTTP/1.1` を受理する(v1 は常にコネクションを閉じるので 1.0 か 1.1 かの永続性は無関係。
   ガードの弱体化ではない — 5 つのガードは不変)。(2)`respond` は常に `Connection: close` を出す
   (RFC 9112 §9.6 が非永続サーバに対して **義務付ける** — 自動 Content-Length のコネクション管理側の双対で
   あり、編集的な `Date`/`Server` ヘッダーではない)。また respond 時に呼び出し側指定の `Connection` /
   `Transfer-Encoding` を、確定済みの呼び出し側 `Content-Length` 拒否と並んで拒否する。HTTPS/サーバ側
   keepalive/並行サービングは記録どおりそのまま先送りする。確定した表面(2026-07-10。2 つの独立した設計
   レビュー: 言語の純粋性のレンズ + システム進化のレンズ。どちらも批准 — 完全な表面は上の Signatures を
   参照)とその決定は次のとおり。
   - **レスポンスの構築 = `response_builder`**(`http.response(status)` + `.header` + `.body` +
     `ctx.respond(rb)`)。これはクライアントの `request` ビルダーのちょうど鏡像である — status は
     method/url と同じく構築時のフィールドである。引数形式の `respond(status, headers, body)` は
     表現できず(可変長引数も dict リテラルも無い)、ヘッダー無しの `respond(status, body)` は
     プリミティブとしては制限が強すぎる(Content-Type を付けられない)。
   - **`respond` は ctx と rb の両方を消費する**(前例: `cl.request(req)` は Move の `req` を消費する):
     二重 respond と close 後の使用を静的に禁じる。1 回の write でシリアライズする(R4)。
   - **自動ヘッダーの方針(クライアントのシリアライズの鏡像):** ボディがセットされた場合にのみ
     `Content-Length` を自動付与する。呼び出し側が指定した Content-Length は拒否する(スマグリング対策)。
     **Date/Server は自動付与しない** — 編集的なヘッダーは呼び出し側のもの(フレームワーク = pkg の領分)。
   - **v1 は accept したコネクション 1 本につき 1 リクエスト**(`respond` が fd を閉じる)。サーバ側の
     keepalive は後日、この表面の裏に見えない形で入る: `respond` の close はクライアントのスライス 3 の
     再利用判定を鏡像にした close-or-pool になり、`accept()` は生かしたコネクションから次のリクエストを
     取り出す — シグネチャの変更は無い(ZST→プールの前例)。
   - **`http_parse_request_head` は新規**(レスポンスのヘッダパーサは `HTTP/` + status を手がかりにして
     おり、`METHOD SP target SP HTTP/1.1` には再利用できない)。Incomplete/Invalid のストリーミング分岐、
     ヘッダーブロックのスキャン、上限(head 256 KiB / ヘッダー 128 個 / body 1 GiB)は再利用する。サーバの
     パース側は、クライアント寛容なレスポンスパーサに欠けている 5 つの inbound スマグリング対策を足さなければ
     ならない:(1)厳格な CRLF 行末 — 素の LF は拒否する。(2)フィールド名とコロンの間の空白を拒否する
     (RFC 9110 のサーバ MUST)。(3)Content-Length + Transfer-Encoding の同時指定を拒否する(TE 単独は
     すでに → `Error.Invalid`、CL のみのフレーミング)。(4)明示的な target 形式 — origin-form(`/path`)は
     受理し、absolute-/authority-/asterisk-form は `Error.Invalid` で拒否する(v1)。(5)シリアライズ側の
     メソッドトークン + CR/LF/NUL のガードを inbound の行にも鏡像適用する。
   - **並行性: v1 は逐次の accept→respond ループである。** `spawn` のキャプチャは今日 Copy/スカラーのみなので、
     Move の ctx はタスクへ渡せない — **Move-capture-into-spawn は並行サービングの記録済み前提条件である**
     (その消費者に紐づく。スライス 4 のブロッカーではない — A5 の単一 GPU ゲートウェイはいずれにせよ推論を
     直列化する)。
   - **SSE/ストリーミング(ランウェイ A5)は `respond` の変更ではなく兄弟の演算として入ることを確約する:**
     将来の `ctx.respond_stream(rb) -> Result<http_stream, Error>`(rb はヘッダーのみで構築)と、Move の
     `http_stream.send(chunk) -> Result<(), Error>` + Drop = 終端チャンク + close。chunked な **write**
     パス(新規、CL のみのパースとは非衝突)が必要になる。v1 の表面はすでにそれを許容している
     (`.body()` は任意)ので、何も塗り込んでいない。
   - **R 要件: R1/R2/R4 が適用され、必須である**(ゼロコピーのリクエストオフセットテーブル。memchr
     スキャン。1 回の write の respond)。v1 にサーバのベンチゲートは無い — 軽い accept→respond の往復ベンチは
     再利用パスが初めて存在する keepalive/並行性とともに入る。
5. **HTTPS/TLS(クライアント側)— 出荷済み 2026-07-10**(設計確定 + 実装済み。ブランチ
   `http-slice5-tls`)。新しいユーザー向けの表面はゼロ — `https://` が `cl.get/post/request` **と**
   `cl.get_many` を通じて動き出す(ワーカーは exchange パスを共有するので、バッチ内でも HTTPS は透過的である)。
   `http://` はバイト単位で不変。DC-1 の粗い `https://` 拒否の負債は解消された。**実装メモ(実装どおり):**
   - **Conn 抽象:** 内部の 1 つの `Conn` enum(`Plain { fd }` / `Tls { ssl, fd }`)が `write_all` /
     `read`(→ ソース非依存の `ConnRead` = `Data`/`Eof`/`Err`)/ `close` のメソッドを持つ。これにより
     ストリーミングのレスポンスループとその Incomplete/Invalid フレーミング分岐が、平文と TLS を通じて
     単一ソース化される — クライアント寛容なパースが分岐することは決してない。`http_socket_exchange` は
     `&mut Conn` を受け取る。
   - **エンジン:** OpenSSL libssl。libcrypto のラッパーを鏡像にした 1 つの `#[link(name = "ssl")]` extern
     ブロック。ドライバは `-lcrypto` と並んで `-lssl` をリンクする。プロセス全体で 1 つの `SSL_CTX` を
     `OnceLock` に置き、`SSL_CTX_set_default_verify_paths`(システムストア)+ TLS 1.2 下限で遅延構築する。
     `get_many` のワーカーが発行する並行な `SSL_new` に対してスレッドセーフである。
   - **接続ごとの検証(`http_tls_connect` 内、すべてハンドシェイクの前):** `SSL_VERIFY_PEER`。DNS の
     authority には `SSL_set1_host` + `X509_CHECK_FLAG_NO_PARTIAL_WILDCARDS` + SNI
     (`SSL_set_tlsext_host_name`)。IP リテラルの authority には `X509_VERIFY_PARAM_set1_ip_asc` を使い
     SNI は付けない(RFC 6066)。ALPN は `http/1.1` を広告する。デフォルトポートは 443(http は 80)。
   - **エラー分類:** 検証失敗(`SSL_get_verify_result != X509_V_OK`、最初に確認)→ `Error.Denied`。
     ハンドシェイク/トランスポートの syscall → errno マップした `Error.Code`。TLS アラート/プロトコル違反 →
     `Error.Invalid`。どのエラー経路でも `SSL*` **と** fd を解放する(`close_tls` = 一方向の `SSL_shutdown` +
     `SSL_free` + `close`)。`SSL_read`/`SSL_write` は `SSL_get_error` で包む(ブロッキングソケットでの
     `WANT_*` は再試行、`ZERO_RETURN` は EOF、`SYSCALL`-with-errno-0 は unclean EOF)。
   - **SIGPIPE:** HTTPS の exchange 全体(ハンドシェイク + I/O + 後始末)をスレッドごとの
     `pthread_sigmask` でブロックし、直前のマスクを復元する前に保留中の SIGPIPE をゼロタイムアウトの
     `sigtimedwait` で吸い出す(`SigpipeBlock` の RAII ガード。スキームが https のときの perform だけで
     保持する)。macOS/BSD ではこのガードは no-op の ZST — connect 時に設定するソケットごとの
     `SO_NOSIGPIPE` が SSL BIO の `write(2)` をすでにカバーしている。平文は従来どおり `MSG_NOSIGNAL` のまま。
   - **プール:** キーは `(scheme, host, port)` になった — TLS 接続が平文のバケットを満たすこと(またはその逆)
     は決してない。`IdleConn` は生きた `SSL*` を持つ(再利用 = 同じ `SSL`、再ハンドシェイクなし)。すべての
     コンストラクタ/コンシューマ(`take_idle`/`put_idle`/ クライアントの `Drop`/ stale 回収/ 超過)が
     TLS を意識している。スタール再試行のロジックはそのまま移植できる — ハンドシェイク失敗は新規パスでしか
     起きないので、誤って再試行されることはない。
   - **テスト:** `align_runtime` のユニット — 分類(自己署名 → Denied、ホスト名不一致の証明書 → Denied、
     拒否 → Code、TLS でないゴミサーバ → Invalid)、正常系の往復(IP パス + DNS/SNI パス)、TLS プールの
     再利用(1 コネクション / 2 gets)、プールのスキームキーイング、http+https 混在の `get_many`、N 回の
     TLS サイクルにわたる `/proc/self/fd` のリークなし — 埋め込みの PEM フィクスチャを持つローカルの libssl
     テストサーバに対して。正常系のパスは **テスト専用の信頼フック** を使う: テスト CA をクライアントストアに
     加える `#[cfg(test)]` の `OnceLock`(`TLS_TEST_CA_FILE`)であり、出荷するランタイムからは(実行時ガード
     ではなく構造的に)コンパイルで除外されるため、リリースビルドには信頼フックが一切なく、検証は必須のまま
     である。ドライバテストはルーティングの変更(`https://` が接続前に拒否されるのではなく接続する)を
     証明する。正常系の TLS 往復はドライバのハーネスからは駆動できない — `#[cfg(test)]` の信頼フックが
     ドライバがリンクするランタイムには存在しないためである。

   **確定した設計(批准どおり):** 新しいユーザー向けの表面はゼロ — `https://` は `cl.get/post/request` を
   通じてただ動き出す(URL のスキームが、挙動を変えるべき唯一の入力である)。
   - **エンジン = OpenSSL libssl**(crypto の確定済み libcrypto と同じパッケージ + ≥3.2 下限、`-lcrypto`
     と同様に動的リンク)。*リンク* は crypto の確定を再利用するが、**信頼判断は本当に新しいセマンティクス
     であり、独自の記録(これ)を持つ**: 証明書は **システム信頼ストア** に対して **常に検証される**
     (`SSL_CTX_set_default_verify_paths()`。ハードコードのパスは決して使わない。配備上の注記: OS の
     `ca-certificates` パッケージが無いとすべてのハンドシェイクが fail-closed になる)。v1 には
     無効化/カスタム CA/クライアント証明書/再開の表面は無い(設定面が存在しない — 凍結済みのシグネチャと
     一貫している)。常に fail closed。
   - **ホスト名バインディングは任意ではなく必須である — chain-verify のみは欠陥である。** 記録は正確な
     API を義務付ける: `SSL_set_verify(SSL_VERIFY_PEER)` + `SSL_set1_host(host)`(DNS 名。
     `SSL_set_hostflags(X509_CHECK_FLAG_NO_PARTIAL_WILDCARDS)` を伴う)または IP リテラルの authority には
     `X509_VERIFY_PARAM_set1_ip_asc(host)` を、OpenSSL がホスト名照合を検証に織り込むよう **ハンドシェイクの
     前** に設定する。`SSL_set_tlsext_host_name`(SNI)は URL のホストから。ALPN は `http/1.1` を広告。
     TLS ≥ 1.2。
   - **エラー分類:** 証明書/ホスト名/信頼の検証失敗 → **`Error.Denied`**(拒否された信頼判断 — 新しい
     variant をゼロにしたまま検証失敗を不正な URL と区別する)。ハンドシェイク/トランスポートの syscall
     失敗 → errno マップした `Error.Code`。レスポンス途中の TLS アラートやプロトコル違反 → `Error.Invalid`。
     どのエラー経路でも fd **と** `SSL*` を解放する(crypto の規律)。読み取りループは `SSL_read`/`SSL_write`
     を `SSL_get_error` で包む(`WANT_*` は再試行 / `ZERO_RETURN` は EOF / `SYSCALL` は errno / `SSL` は
     Invalid)。Incomplete/Invalid の分岐はソース非依存で、そのまま移植できる。
   - **SIGPIPE:** `MSG_NOSIGNAL` は `SSL_write` に届かず(BIO の書き込みはフラグを運ばない)、Linux には
     `SO_NOSIGPIPE` が無い。プロセス全体の `signal(SIGPIPE, SIG_IGN)` は検討したが **却下した** — 記録済みの
     no-global-handler 規律を破ってしまうためである。確定した機構: **スレッドごとの `pthread_sigmask`** —
     TLS の exchange の周りで `SIGPIPE` をブロックし(ワーカースレッドは開始時にブロックする)、復元前に
     ゼロタイムアウトの `sigtimedwait` で保留中のシグナルを吸い出す。
   - **プール:** キーは **(scheme, host, port)** になる — TLS 接続が平文のバケットを満たすこと(またはその逆)
     は決してあってはならない。再利用 = 生きた `SSL*` の再利用(再ハンドシェイクなし。セッション再開ではない)。
     スタール再試行の判定はきれいに移植できる(ハンドシェイク失敗は新規パスでしか起きないので、誤って再試行
     されない)。Drop/期限切れ: ベストエフォートの一方向 `SSL_shutdown`(ピアを待たない)、`SSL_free`、`close`
     — Content-Length フレーミングにより truncation 攻撃は無意味になる(短いボディはすでに `Error.Invalid`)。
   - **サーバ側 TLS はそのまま先送り** — 半端に出荷するのではなく一貫性を保つ: サーバプリミティブは記録済みの
     信頼済みネットワークの caveat を負う。クライアント優先は align-LLM A5 の消費者と一致する。
6. **`cl.get_many(urls, max_concurrency)`(R5)— 設計確定 + 出荷済み 2026-07-10**(同じ 2 レンズの
   レビュー。実装はブランチ `http-get-many`)。下の確定どおりそのまま出荷した — 前提となる
   `array<response>` の不透明 Move ハンドル配列の機能(ランタイム専用の構築、`rs[i]` のレシーバ位置での借用、
   要素ごとの drop)と R5 のベンチ(degree 16 で 15.4× のオーバーラップ、Rust プールと同等 — 上の R6 を参照)を
   含む。確定した記録:
   - **結果は入力順**(`urls[i]` → `results[i]`)。**all-or-Err**: トランスポート/パースの失敗はいずれも、
     **最小インデックス** のエラーでバッチ全体を失敗させる(決定的 — `tg_wait` の慣習に一致)。要素ごとの
     `array<Result<response, Error>>` は **表現できない**(`Result` は `Ty` であって `Scalar` では決してなく、
     配列の要素は `Scalar` である)— all-or-Err が唯一の正直な形であり、将来への指し示しとともに記録する
     (スロットごとのエラーは、もしあれば `Scalar::Result` クラスの機能を待つ)。4xx/5xx は `Ok` のデータの
     まま。空の `urls` → `Ok` の空配列。GET のみ(`request_many` は消費者が現れるまで先送り — R5 の本質は
     レールであって動詞の集合ではない)。`max_concurrency <= 0` は **abort**(プログラマのバグ、`rand.range`
     と同じクラス)。
   - **完走する、短絡なし:** キャンセルのプリミティブは無く、ブロッキング read は中断できないので、失敗時は
     残りのワーカーが完走してその結果は破棄され、最初(最小インデックス)のエラーが報告される。したがって
     no-timeout の制約はバッチ処理で **増幅される**(停止した 1 つのサーバがバッチ全体を握る)— 記録済み。
     修正は将来のデッドライン/構造化キャンセルのスライスに属する。
   - **機構: 専用の並行数制限つきブロッキング I/O ワーカープールであり、CPU サイズの ParPool ではない。**
     R5 の草案は「task_group + ParPool の claim ループ」と書いていたが、ParPool は
     `available_parallelism()` にサイズされ I/O オーバーラップをコア数で頭打ちにする — I/O バウンドの
     バッチ処理には合わない形である(オーバーラップはコア数 ≫ が欲しい)。確定: ランタイムは
     `min(max_concurrency, urls.len())` 個のスコープ付きブロッキングワーカーを spawn し、共有カウンタから
     URL のインデックスを claim して結果を入力順にスロットする。これはまさに確定済みの「async = task_group +
     ブロッキングワーカー」の立場である。生きた fd はワーカー数(+ 完了時にホストあたり ≤8 プール)で
     縛られる。pipeline 化した 19.1× のレールは get_many の成果物では **ない**(スライス 3 の再利用判定が
     未ドレインコネクションの再利用を禁じる)— 12.8× のマルチコネクションのオーバーラップの形がそれである。
   - **前提となる機能(コンパイラ): `array<response>` — 不透明な Move ハンドルの動的配列。** 今日 `response`
     は配列要素として拒否される(所有ハンドルの除外)ので、凍結済みの戻り型には狭い新機能が必要であり、その
     消費者である get_many と **ともに** 出荷する(#399 の `Scalar::Slice` + 消費者の前例): 構築は
     **ランタイムのみ**(ユーザー側の `[resp1, resp2]` リテラルは拒否のまま)。レシーバ位置の `rs[i]` は
     **借用** である(バインドされたメソッド呼び出し — `rs[i].status()`、`rs[i].body()` — は配列にリージョン
     束縛されたビュー。所有フィールド借用の前例)。要素を外へ move するのは v1 では拒否する。配列全体の move
     はソースを null にする。Drop = 要素ごとの `http_resp_free` ループ + ストレージ解放。新しい要素クラスには
     完全な twin-mirror スイープが必要である。
   - **ベンチ(R6 の get_many 部分を閉じる):** インプロセスの localhost サーバに対する 64 URLs に
     **リクエストごとのレイテンシを注入**(localhost の RTT ≈ 0 だとオーバーラップの利得が見えなくなる)、
     同 degree の固定スレッドプールを使う Rust ベースラインと比較する。正直な報告: 計測されたオーバーラップ
     係数 + マシンのコア数 + 同 degree での Rust との同等性 — ハードウェア非依存の 12.8× という主張ではない。

## Known v1 limitations (Slice 2/3/5)

- **HTTPS はクライアント側のみ(スライス 5)。** サーバ側 TLS は先送り — `http.serve` は平文であり、その
  記録済みの信頼済みネットワークの caveat(下記)が残る。クライアント優先は align-LLM A5 の消費者と一致する。
  サーバ TLS は半端な出荷ではなく、一貫した v1 後の作業である。
- **証明書の失効確認は無い(スライス 5)。** 検証はシステム信頼ストアに対する chain + ホスト名であり、
  CRL / OCSP / OCSP-stapling の確認は無い。失効しているが未期限切れで、なお信頼されたルートまで chain する
  証明書は受理される。失効確認は記録済みの v1 後バックログである(クライアント証明書、カスタム CA、セッション
  再開と並んで — いずれも凍結済みのシグネチャに設定面を持たない)。
- **システム信頼ストアが存在している必要がある(スライス 5 の配備上の注記)。** 信頼ルートは
  `SSL_CTX_set_default_verify_paths()`(ハードコードのパスは決して使わない)から来る。OS の `ca-certificates`
  パッケージ(または相当物)が無いとストアは空になり、**すべての** HTTPS ハンドシェイクが `Error.Denied` で
  fail CLOSED する — 正しい fail-closed の姿勢だが、述べておく価値のある配備上の前提である: HTTPS リクエストを
  行う任意のコンテナ/イメージには `ca-certificates` を同梱すること。
- **タイムアウトのギャップのサーバ側へのエスカレーション(スライス 4、セキュリティ上の注意 — 2026-07-10 に
  確定)。** クライアントでは I/O デッドラインの欠如は堅牢性のギャップだが、**サーバ**ではセキュリティ境界に
  なる: 1 つの slow-loris クライアント(接続だけして、その後停止するか上限より下でちびちび送る)が、
  唯一のブロッキング accept スレッドを永久に握る — v1 の逐次 accept ループでは、これは容易にサーバ全体の
  denial of service になる。**したがって v1 のサーバプリミティブは信頼できないネットワーク上では安全でない**。
  その記録済みの信頼前提は **localhost / 信頼済みネットワークのゲートウェイ**(align-LLM のランウェイ A5 の
  消費者)であり、そこでは slow-loris は脅威モデルの対象外である。read/accept のデッドラインは
  **v1 後のサーバ堅牢化の第一歩**であり、下のクライアント側タイムアウトの注記より優先度が高い。
- **read/connect の I/O タイムアウトが無い(G3-1, medium, 継承)— スライス 3 を越えて意図的に先送り。**
  TCP のハンドシェイクを完了させた後に停止するサーバ — 何も送らない、上限より少ないバイトをちびちび
  送る、`Content-Length` より少なく送ってソケットを握り続ける — は、呼び出しスレッドを**無期限に**
  ブロックする。バイト上限(head 256 KiB / body 1 GiB)が縛るのは*メモリ*であって*時間*ではない。これは
  net のレールが文書化している no-timeout の挙動(`align_rt_tcp_connect`)を、http クライアントが connect
  **と** read の両方で継承したものである。**スライス 3 での判断(記録のみ、未実装):** スライス 2 の注記は
  タイムアウトのフォローアップを「プールが per-conn のデッドライン管理を必要とするスライス 3 のプール作業と
  ともに」入れると書いていた。だがスライス 3 を実装してみると、その言い回しは別物を混同していた。プールの
  デッドライン管理とは **idle 期限切れ**(90 秒より古いコネクションを再利用しない)であり — これはスライス 3
  が**実装している** — connect/read の **I/O デッドライン**ではない。本物の I/O タイムアウトの追加は
  分離可能でより大きな変更であり、http ローカルな理想形を持たない:(1)**connect** タイムアウトの理想的な
  置き場は net レール(non-blocking `connect` + `poll` の基盤 — net.md が後日のバックエンドとして既に挙げて
  いる)であり、http に半分だけ入れれば二つ目の部分的な機構になる。(2)**read** タイムアウトは数行
  (`SO_RCVTIMEO`)だが、*固定値*は正当な低速/大容量転送を黙って壊し、v1 には凍結済みの
  `get`/`post`/`request` シグネチャを広げずにリクエスト単位で設定する**設定面が無い**(別の設計判断)。
  「理想形か、さもなくば先送り」に従い、スライス 3 はプールの idle 期限切れと SIGPIPE 安全/スタール再試行の
  堅牢性を出荷し、**I/O タイムアウトは net レールの non-blocking/deadline 基盤へ先送り**する
  (セマンティクス上は不変)。半端な実装を入れる代わりに、v1 の既知の制約としてここに記録する。
  - **サブケース — HEAD / 304 のフレーミング(スライス 1/2 から継承)。** `HEAD` レスポンスや
    `304 Not Modified` は、正当に `Content-Length` ヘッダを持つが**ボディを持たない**。v1 の読み取り
    ループは純粋に `Content-Length` でフレーミングする(リクエストメソッドやステータスで特別扱いしない)
    ため、決して来ないボディバイトを待ち続ける → 上と同じ無期限ブロックになる。v1 の表面は `HEAD` を
    手軽には出していない(`get`/`post`/`request` のみ)が、メソッド `HEAD` で組んだ `request` はこれに
    当たる。メソッド/ステータスを見たフレーミング(HEAD/1xx/204/304 はボディ無し)は、de-chunking を足す
    のと同じスライスで入れる。スライス 3 では修正せず、ここに記録する。
- **~~`https://` の拒否が粗い(DC-1, low)。~~ スライス 5 で解消。** `https://` はもはや `Error.Invalid`
  に写らず、検証済み TLS 経路にルーティングされる。検証失敗は明確な `Error.Denied`、TLS トランスポート不良は
  `Error.Code`、プロトコル違反は `Error.Invalid`。(メッセージレスな `Error` enum はより広い別課題として残る
  が、DC-1 の「HTTPS 未対応」負債は解消 — HTTPS は *対応済み*。)

## Pitfalls

- **P1 (黙ってダウングレードしない — 実 TLS で)**: `https://` は決して平文で送ってはならない。スライス 5
  はこれを、スキームを拒否するのではなく検証済み TLS で接続すること(必須の証明書 + ホスト名検証、
  fail-closed → `Error.Denied`)で満たす。黙ってダウングレードするのはセキュリティ上の落とし穴のままである
  (Nothing-hidden 違反)。保証は「https は TLS を意味する」であり、エンジンが強制する。
- **P2 (ステータスはデータ)**: 4xx/5xx を `Err` に写してはならない — `Err` はトランスポート/パースの失敗
  だけである。`get()` が 404 を返すなら、それは `Ok(status 404 のレスポンス)` である。ここを取り違えると、
  呼び出し側が二重のエラー処理を強いられる厄介な設計になる。
- **P3 (レスポンスのビューのリージョン, #297)**: `resp.header()`/`body()` は resp を指すビューである。
  `region_of` は Static ではなく `region_of(resp)` でなければならない。resp の Drop を越えた escape は拒否
  する。
- **P4 (Move sweep + bound-receiver)**: client/request/server/ctx は Move である — 全パスの Gate-1 スイープ
  と bound-receiver のゲート(#337/#338)が要る。v1 では束縛していない一時値をレシーバにできない。
- **P5 (コネクションプールの Drop)**: client はプールしたコネクションを所有する。Drop はそのすべてを close
  する。プールの入れ替わりを通じて fd がリークしないこと。
- **P6 (request smuggling / header injection)**: ヘッダー名・値の中の CR/LF は拒否する(header injection
  → request smuggling)。検証は `r.header()` の時点で行う。

## Test checklist

- リクエストをシリアライズする → 正確なバイト列になる
- 既知のレスポンスをパースする → status/headers/body が取れる
- ローカルの平文サーバに対する `get()` → 200 の往復
- 404 → `Err` ではなく `Ok(status 404)`(P2)
- `https://` → 検証済み TLS の往復(スライス 5)。信頼できない/ホスト名不一致の証明書 → `Error.Denied`
- ヘッダー中の CRLF → 拒否(P6)
- resp を越えて escape するレスポンスボディのビュー → コンパイルエラー(P3)
- プールが 2 回の get にまたがって conn を再利用する
- Move の拒否 + 束縛していないレシーバの拒否
- import が必須であること
- `bench/http_client` の数値を Rust ベースラインに照らして記録する(R6 — 完了はベンチマークでゲートする)
