このディレクトリには、ロードマップの本文ではカバーしきれない `std` モジュールについて、Opus がそのまま実装に
着手できる粒度の設計仕様を収めている。執筆はメインループ (Fable) が担当しており、各モジュールを実装する際は
これが信頼できる情報源（source of truth）となる。

# std.http — implementation design (M11)

> 🌐 [English](../http.md) · **日本語**

## Overview

HTTP/1.1 のプリミティブであり、フレームワークではない(draft §18.2)。std.net のソケットの上に構築す
る。メンバーは request、response、header、method、status、client、server プリミティブ。コネクション再利用
は net の基盤（レール）に従う。**クライアント側の HTTPS/TLS は出荷済み**(スライス 5):`https://` は
`cl.get/post/request` + `cl.get_many` を通じてそのまま動作し、OpenSSL libssl 上で(システム信頼ストアに
対する必須の検証 + ホスト名バインディングを伴って)crypto の libcrypto と並んで動的リンクされる。サーバ側
TLS はクライアント優先で先送り。HTTP/3、ルーティング、ミドルウェアは std ではなく pkg である。

**モジュール状態: COMPLETE**(スライス 1–6 出荷済み。クライアント側 TLS はスライス 5)。サーバ側 TLS、
クライアント証明書、カスタム CA、セッション再開、失効確認は記録済みの v1 後バックログ。

## Signatures

v1 案として、Fable が確定させた形式:

```text
// Client
cl := http.client()                         // コネクションプールを所有する (Move)
cl.max_response_body_bytes(limit: i64)      // 0 は固定既定値へ戻し、正値は受信 allocation を制限する
cl.get(url: str) -> Result<response, Error>
cl.post(url: str, body: bytes) -> Result<response, Error>
cl.request(req: request) -> Result<response, Error>
// Request/response building
r := http.request(method: str, url: str)    // builder (Move — header list と body buf を所有する)
r.max_response_body_bytes(limit: i64)       // 0 は client を継承し、正値は上限を狭めるだけ
r.header(name: str, value: str)
r.body(data: bytes)
resp.status() -> i64
resp.header(name: str) -> Option<str>       // view into resp
resp.body() -> bytes                         // view into resp (region-bound)
// Server primitive (not a framework) — surface settled 2026-07-10 (two-lens design review)
srv := http.serve(host: str, port: i64) -> Result<http_server, Error>
srv := http.serve_shared(host: str, port: i64) -> Result<http_server, Error>
                                             // the prefork sibling: same bind + SO_REUSEPORT, so N
                                             // workers each own a listener on ONE port (item 9 ①)
srv.accept() -> Result<http_request_ctx, Error>   // one request; caller writes the response.
                                             // Yields the next request off a KEPT-ALIVE connection
                                             // before accepting a new one (item 9 ②) — same surface
ctx.method() -> str                          // view into ctx (region-bound)
ctx.path() -> str                            // view into ctx (region-bound)
ctx.headers() -> http_headers                // the parsed header table as a Copy, non-owning VIEW
                                             // (region-bound to ctx, like ctx.body(); item 10).
                                             // A struct field type, so a Copy per-request context
                                             // can carry it — `http_headers` is a GLOBAL type name
hs.get(name: str) -> Option<str>             // RFC 9110 §5.1 case-insensitive lookup; the returned
                                             // str views the request buffer and INHERITS `hs`'s
                                             // region (so a wrapper taking the view through a
                                             // parameter compiles — item 10 ④)
ctx.body() -> bytes                          // view into ctx (region-bound)
rb := http.response(status: i64)             // response_builder (Move — owns header list + body buf;
                                             // the build-dual of `request`; named apart from the
                                             // parsed read-view `response`)
rb.header(name: str, value: str)             // bound receiver; CR/LF/NUL aborts (P6)
rb.body(data: bytes)                         // optional — a bodiless response is legal and frames
                                             // as Content-Length: 0 (except 1xx/204/304)
ctx.respond(rb) -> Result<(), Error>         // consumes BOTH ctx and rb; one-write serialize (R4);
                                             // PARKS an eligible 1.1 connection for keep-alive (no
                                             // Connection header), else closes the accepted fd;
                                             // a HEAD request gets the body SUPPRESSED, its
                                             // Content-Length kept (RFC 9110 §9.3.2; W4)
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
  設計上の対称性はデータの向きに基づいており、それは以下のように保たれている: `response_builder` ≅ `request`(ビルダー)、
  `http_request_ctx` の読み取り ≅ `response` の読み取り(ビュー)。
- **`response_builder` は型名として書ける型であり、`Option`/`Result` のペイロードとしても有効である**
  (2026-07-20)。当初はそのどちらでもなかった — ソース上で型名として書けず、`scalar_arg` は無条件に拒否して
  いた。理由は「`http.response` が直接返すのだから、これを包む API は存在しないだろう」というものだった。
  pkg.web の所有権の決定がまさにそれを必要とする — ハンドラがレスポンスを**組み立てて返す**
  (`fn(Ctx) -> Result<response_builder, Error>`)ことで、フレームワークがリクエストハンドルを保持し続け、
  ハンドラが失敗しても応答できるようになる。現在は `http_request_ctx` と同じ条件で許可されている:
  ペイロードとしては合法、配列/スライス/box の**要素**としては依然として拒否 — 要素の読み出しはハンドルを
  コピーするので、両方のコピーが解放してしまうためである。
  
  これが健全なのは、ビルダーが**保持するバイト列をすべて所有し、何も借用しない**からである —
  `rb.header(name, value)` は `String::from_utf8_lossy(..).into_owned()` を、`rb.body(data)` は
  `data.to_vec()` を格納する。これによりビルダーは、そのヘッダー/ボディの生成元となったローカルより長生き
  でき、リージョン追跡の対象にならない。**したがってゼロコピーな `rb.body` は最適化ではなく破壊的変更で
  ある**。`response_builder_payload.rs` がこのコピー意味論を両側から固定している(プログラムが生存すること、
  および死んだローカルからボディを作ったハンドラの応答がワイヤ上でバイト単位に一致すること)。
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

オーナーは std.http を **高速** にしたいと考えている。`open-questions.md` に記録された計測済みの基盤（レール）
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
  バイト分類器)は後日の最適化として記録にとどめる。memchr は現時点で追加コストなしに利用可能である。
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
  「高速に仕上がった」とは見なさない。**R6 はスライス 3 の時点で満たされた:** `bench/http_client` は出荷済みで
  (出荷したプールをその C-ABI エントリポイント経由でインプロセスの localhost サーバに対して駆動する)、
  **keepalive で 2.86× 高速化**(下限 1.48× — 達成)と、再利用パスでの **手書き Rust `std::net` と同等**を
  記録している(`bench/http_client/README.md` を参照)。**`get_many` のスケーリング部分も今や達成された
  (2026-07-10、R5 のスライス):** 12 ms のレイテンシを注入した状態で degree 16 の 64 GETs — **15.4× の
  オーバーラップ**(理想 ≈ degree)、**同 degree の Rust スレッドプールの 1.01×**(同等)。正確な報告のための
  但し書き（caveat）はベンチマークの README に記載されている(degree とコア数を添えて引用すること)。R6 はこれで完全に満たされた。

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
   元の slice は `chunked` を拒否していたが、Request 4 により、同じ R1 準拠 decoder で supported
   HTTP/1.1 response を de-chunk する。上限: ヘッダー 128 個以下、ボディ 1 GiB 以下。R1 ゼロコピー:
   response は 1 本のバイトバッファ + オフセット表を所有し、スキャンは `memchr` クレート(R2)に載せる。
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
   (P1 — 黙って平文にダウングレードしない)。フレーミングは Content-Length、HTTP/1.1 chunked、
   read-to-close をサポートし、Request 4 が元の CL-only path を置き換えた。パーサはストリーミング読み取りが
   「もっとバイトが必要」と「不正」を 1 つの共通デコーダで区別できるよう、`Incomplete`/`Invalid` の 2 分岐にリファクタした。
   プールはまだなし(各リクエストは新規接続して閉じる — keepalive の再利用はスライス 3)。`get_many` /
   server / HTTPS は残る。
3. コネクションプールの再利用(基盤（レール） — keepalive、デフォルトで再利用)。**完了**(ブランチ
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
   リスナーは生き続ける)。`ctx.method()/path()`(`str` ビュー)、`ctx.headers()`(パース済みヘッダー
   テーブルの Copy な `http_headers` ビュー。大文字小文字を無視する `Option<str>` の lookup は
   `hs.get(name)` — item 10 であり、`ctx.header(name)` を**置換した**)、`ctx.body()`(`slice<u8>`
   ビュー)— すべて `ctx` にリージョン束縛される(#297)。
   `http.response(status)` -> `response_builder`(Move。パース済みの `response` とは別の Ty + 表示名)+
   `rb.header(name, value)`(バインド済みレシーバ、P6 の CR/LF/NUL は **abort**)+ `rb.body(data)`(任意)。
   `ctx.respond(rb) -> Result<(), Error>`(ctx と rb の **両方を消費する** — `cl.request(req)` と同様に
   MIR が両スロットを null にする。シリアライズ = ステータス行 + ヘッダー + 自動 Content-Length
   (ボディを持ちうるステータスならボディ未設定でも `0`)。1 回の write、R4。MSG_NOSIGNAL/SO_NOSIGPIPE。
   fd を閉じる、v1 は 1 コネクション 1 リクエスト)。**W4 (2026-07-21): HEAD リクエストに対する `respond` はボディバイトを抑制し、その
   `Content-Length` は保持する(RFC 9110 §9.3.2)** — プロトコル境界で強制されるため、bodied ビルダーで
   HEAD に応答する呼び出し側(pkg.web の HEAD→GET ルーティングを含む)はすべて構築上 RFC 準拠になる。
   `respond_stream` / `reject` は不変(stream に HEAD 形は無い)。`METHOD SP target SP HTTP/1.1`
   向けの **新規** `http_parse_request_head` が、下記の
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
   - **自動ヘッダーの方針(クライアントのシリアライズの鏡像):** ボディを持ちうるステータスのレスポンス
     には常に `Content-Length` を自動付与する — セットされた長さ、ボディ未設定なら `0`(2026-07-21 に
     keep-alive と同時に修正: フレーミングヘッダの無いレスポンスは「close まで読む」を意味し、persistent
     なコネクションを禁じてしまう。そしてその用途に正当性はない — close 区切りのフレーミングは
     `respond_stream` の 1.0 モードの仕事である)。`1xx`/`204`/`304` はボディを持たないので、フレーミング
     ヘッダは付けない。呼び出し側が指定した Content-Length は拒否する(スマグリング対策)。
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
   - **並行性: v1 は逐次の accept→respond ループである。** `spawn` のキャプチャは現時点では Copy / スカラーのみに制限されているため、
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
   - **エンジン = OpenSSL libssl**(libcrypto と同じパッケージ。TLS には OpenSSL ≥3.0)。HTTPS を使う場合に
     `-lcrypto` とともに capability-link する。*リンク* は crypto の確定を再利用するが、**信頼判断は本当に
     新しいセマンティクスであり、独自の記録(これ)を持つ**: 証明書は **システム信頼ストア** に対して **常に検証される**
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
   - **サーバ側 TLS はそのまま先送り** — 不完全な状態で出荷するのではなく、一貫性を保つ: サーバプリミティブは記録済みの
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
     基盤（レール）であって動詞の集合ではない)。`max_concurrency <= 0` は **abort**(プログラマのバグ、`rand.range`
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
     縛られる。pipeline 化した 19.1× の基盤（レール）は get_many の成果物では **ない**(スライス 3 の再利用判定が
     未ドレインコネクションの再利用を禁じる)— 12.8× のマルチコネクションのオーバーラップの形がそれである。
   - **前提となる機能(コンパイラ): `array<response>` — 不透明な Move ハンドルの動的配列。** 現時点では `response`
     は配列要素として拒否される(所有ハンドルの除外)ので、凍結済みの戻り型には狭い新機能が必要であり、その
     消費者である get_many と **ともに** 出荷する(#399 の `Scalar::Slice` + 消費者の前例): 構築は
     **ランタイムのみ**(ユーザー側の `[resp1, resp2]` リテラルは拒否のまま)。レシーバ位置の `rs[i]` は
     **借用** である(バインドされたメソッド呼び出し — `rs[i].status()`、`rs[i].body()` — は配列にリージョン
     束縛されたビュー。所有フィールド借用の前例)。要素を外へ move するのは v1 では拒否する。配列全体の move
     はソースを null にする。Drop = 要素ごとの `http_resp_free` ループ + ストレージ解放。新しい要素クラスには
     完全な twin-mirror スイープが必要である。
   - **ベンチ(R6 の get_many 部分を閉じる):** インプロセスの localhost サーバに対する 64 URLs に
     **リクエストごとのレイテンシを注入**(localhost の RTT ≈ 0 だとオーバーラップの利得が見えなくなる)、
     同 degree の固定スレッドプールを使う Rust ベースラインと比較する。正確な報告: 計測されたオーバーラップ
     係数 + マシンのコア数 + 同 degree での Rust との同等性 — ハードウェア非依存の 12.8× という主張ではない。
7. **SSE/chunked ストリーミングレスポンス（`respond_stream`、runway A5 の残り）— 2026-07-11 設計確定、
   出荷済み。** ランタイム: `HttpStream { fd, framed, poisoned }` + `align_rt_http_respond_stream` /
   `_stream_send` / `_stream_finish` / `_stream_free`; head のシリアライザは `http_serialize_head` に
   単一化されている（respond は CL+body を、respond_stream は TE を追加する）; リクエストの HTTP
   バージョンはパース → `HttpRequestHead.http11` → `HttpRequestCtx.http11` → stream の `framed` と
   貫かれる。コンパイラ: `Ty::HttpStream`/`Scalar::HttpStream`（`Result` の Ok ペイロードに乗る Move
   ハンドル。accept の前例）、HIR の `HttpRespondStream`/`HttpStreamSend`/`HttpStreamFinish`、いずれも
   `lower_http` を通す。テストはランタイムの unit（フレームエンコーダ、バージョン、共有 head の一致、
   poison、空 send の no-op）+ `crates/align_driver/tests/m12_http_stream.rs`（1.1 chunked / 1.0 raw /
   切断 / poison / Align client の chunked dogfood / 二重消費 + bodied abort の
   ゲート）。（2 レンズのレビュー、Fable が統合。）gateway のトークンストリーミング層は: 呼び出し側が
   SSE の `data: …\n\n` 行をボディ内容として書き、std.http は**転送フレーミングのみ**を提供する
   （フレームワーク境界を保つ）。
   - `ctx.respond_stream(rb) -> Result<http_stream, Error>` — ctx と rb の**両方**を消費する
     （`respond` の前例）。rb は **header-only** でなければならない: 既にボディが設定されていれば
     プログラマの契約バグ → **abort**（bodied なら `respond` の経路である; `rand.range` と同じ abort
     クラス — client のデータではなくコード構造に起因する）。head のシリアライズ = ステータス +
     ヘッダ + 自動の `Transfer-Encoding: chunked` + 自動の `Connection: close`（自動 CL の鏡）;
     **head のシリアライザは `respond` と単一化されている**（呼び出し側の CL/TE/Connection 拒否ループと
     P6 ガードを含む共有 head 関数 1 つ。respond は CL+body を、respond_stream は TE を追加する）。
   - **HTTP/1.0 の client（必須。レビューで発見 — バージョンは当時パースした後に破棄されていた）:**
     リクエストの HTTP バージョンを parse→head→ctx→stream と貫く。1.0 リクエストに対して chunked は
     不正なので、stream は **close 区切りの raw モード**で構築する（stream 上の `framed: bool`）:
     TE ヘッダなし、`send` はペイロードバイトを非フレームで書き、`finish`/Drop は close するだけ
     （read-to-close は 1.0 の正当なフレーミングである）。
   - **`http_stream`**（Move。ctx から持ち上げた fd を所有する。free-standing — ctx から何も借用せず
     region 束縛もない。Move ハンドルの標準的な除外規則に従う）。`s.send(chunk: bytes) ->
     Result<(), Error>` — 1 つのチャンクフレーム（小文字 hex の長さ、`0x` なし、CRLF ペイロード CRLF）を
     1 つのバッファで組み立て、`http_send_all` で **1 回の write**（MSG_NOSIGNAL/EINTR/部分書き込みの
     規律。EPIPE → Error）。**`send("")` は Ok を返す no-op である** — 空チャンクはプロトコルの
     **終端子**であり、かつ空の出力ステップは予見できる gateway のデータ（トークンをまたいで分割された
     マルチバイト UTF-8 コードポイントは 0 バイトにデトークナイズされる）であってプログラマのバグでは
     ない。何も書かないのが正直な意味論である。TCP_NODELAY は accept 時点で設定済み — 1 回の send =
     即座に見える 1 イベント（トークンストリーミングのレイテンシ要件）。
   - **`s.finish() -> Result<(), Error>` が唯一のクリーンな終端子である** — stream を消費し
     （`null_moved_source` の新しい腕。見落としやすい方）、`0\r\n\r\n` を書き（framed モード。
     トレーラは省略 — RFC 9112 §7.1 に適合）、close し、エラーを表に出す。**Drop は close のみで、
     終端の write を行わない** — これは先にコミットした項目をあえて**修正する**: v1 には write の
     デッドラインがないので、停止した peer への Drop 時の終端 write は単一の accept ループを無限に
     ブロックし得る。加えて、終端チャンクの欠落こそ chunked の送信側が切断を通知する方法そのもので
     あり、唐突な close の方が安全であり切断に対して正直でもある（明示的な操作はエラーを表に出し、
     Drop は黙る、という file/conn の前例の分担）。失敗した `send` が立てる **`poisoned` フラグ**に
     より、`finish` は終端の write をスキップして close し、Err を返す（stream はクリーンに終端
     しなかった）。
   - ストリーミングは slow-loris の caveat を再確認させる: stream は設計上、生成の全期間にわたって
     単一のブロッキング accept スレッドを保持する — 信頼済みネットワークの前提は攻撃時の caveat と
     いうだけでなく、設計上の荷重を負っている。
   - Request 4 が後に元の client 非対称性を除去した。Align 自身の whole-body client は streaming
     server response を de-chunk し、decoded payload を公開する。
8. **`respond_stream` の作り直し（pkg.web stream ルート向け）— 2026-07-21 設計、同日出荷。**
   pkg.web のストリーミング設計（`docs/impl/pkg-design/web.md` → 「ストリーミング」）が消費者である。
   stream ハンドラの実行中も framework がリクエストコンテキストを所有し続けること、および head 確定前の
   4xx 窓を必要とする。変更は 3 点、いずれも pre-release の完全置換（M12 テストを完全更新、compat パス無し）:
   - **① 非消費レシーバ。** `ctx.respond_stream(rb) -> Result<http_stream, Error>` は `rb` **のみ**を
     消費する。fd は従来どおり stream に持ち上げる; `ctx` は呼び出し側に残り **spent** となる: 以後の
     `respond`/`respond_stream` は `Err`（abort ではない — bodied-rb の契約バグと違い、通常の制御フローで
     到達し得る）; その Drop はパースバッファのみ解放し fd close はスキップ（持ち上げ済み）。これが
     `Ctx` の view（path/query/**body** — LLM の pump はストリーミング中にプロンプトを読む）を pump
     呼び出しの間ずっと有効に保つ。前例: `rb.header` は既に変異する非消費 bound receiver である。
   - **② 遅延 head。** `respond_stream` は rb を即時に検証する（header-only 契約、P6 ガード、
     TE/Connection ポリシー — 不変、bodied rb は依然 abort）が、head の書き込みは行わず stream ハンドルに
     直列化して保存し、最初の `send`（または `finish`）が書く。観測可能な変更: client は最初のイベントまで
     何も見ない — fn doc に明記、③の対価である。
   - **③ `s.reject(rb) -> Result<(), Error>`。** 最初の send より前でのみ合法（以後は `Err`、poison には
     触れない）: 保存済み head を破棄し、`rb` を完結した**通常**レスポンスとして書き（respond の
     serializer、CL+body）、close する。stream を消費する。これが stream ルート唯一の stream 前
     4xx/5xx 経路である — 検証は pump 内で行い、`reject` がそれに応える。
   - `send`/`finish`/Drop/poison の意味論は他は不変; `framed`（1.0/1.1）は従来どおり `respond_stream`
     時点で選ばれ、保存 head に焼き込まれる。
   - **出荷記録。** ランタイム: `HttpStream.pending_head`（最初の `send`/`finish` の書き込み試行が取得 —
     その書き込みが失敗しても確定扱い; head+初回チャンク / head+終端は 1 回の write で出る）、
     `align_rt_http_stream_reject`、および `respond`/`respond_stream` 双方の spent-fd（`fd < 0`）`Err`
     チェック; `respond_stream` の検証 `Err` は ctx を**未 spent** のまま残す（呼び出し側はまだ通常の
     `respond` でエラー応答できる）。言語側: `s.reject(rb)` を
     `ExprKind::HttpStreamReject`/`Rvalue::HttpStreamReject` で（両方消費、MIR が両スロットを null）;
     `HttpRespondStream` は `rb` のみ null。テスト: `align_runtime` unit（lazy-head/reject/spent-ctx
     契約）+ `m12_http_stream.rs`（13 本: pump 中の `ctx.path()` 借用ストリーム、spent-ctx `respond` →
     `Err` E2E、reject → 通常 400 E2E、遅延 reject → `Err` + 切断、reject の move ゲート）。
   - **④ `s.send_event(data) -> Result<(), Error>` — 2026-07-21 出荷**（pkg.web ストリーミングの
     enabler 5、その最初の消費者と共に — 確約済みの「最初のストリーミング消費者が着地したときの SSE
     イベントフレーミング（WHATWG）」床項目）。`data` を 1 つのイベントフレーム `data: {data}\n\n` として
     包み、チャンクフレーミングと（まだ保留かもしれない）遅延 head と**同じバッファ内**で組み立てる —
     head + チャンクフレーミング + イベントを 1 回の `http_send_all` write で; raw（1.0）モードはイベン
     トバイトを非フレームで書く。**`send_event("")` は合法な空イベント**（`data: \n\n`、8 ペイロード
     バイト — チャンク終端とは決して被らない）なので、`send("")` と違い実際の write であり head を確定
     する。複数行 `data` は v1 では caller の責務（裸の `\n` はイベントのフィールド構造を変える — 分割は
     記録済みの pkg.web バックログ）。`s` の借用は `send` と全く同じ（poison ラッチ共有）。これは
     **メソッド**であって `pkg.web` 自由関数ではない。なぜなら pkg レベルの自由関数は Move ハンドルを値
     渡しで取り（ユーザ関数に借用パラメータは無い — `io.copy` の bound-receiver 制限クラス）、pump が
     これから finish すべき stream を消費してしまうからである。ランタイム: 共有 `http_stream_send_parts`
     ヘルパの上の `align_rt_http_stream_send_event`。言語側: `HttpStreamSend`/`Rvalue::HttpStreamSend`
     が `event: bool` を得た（同一 variant なので全解析パスが `send` として扱う — 新 variant のソウンド
     ネス掃きは不要）。テスト: ランタイムのフレーミング unit（フレーム付き 空/非空 + raw）、
     `m12_http_stream.rs` の `send_event` E2E、および pkg.web の `apps_web_stream.rs` スイート。
9. **prefork リスナー + サーバ側コネクション keep-alive — 2026-07-21 設計 + 出荷**
   （消費者 = pkg.web の並行 serve、`pkg-design/web.md` → 「並行 serve」）。std の変更は 2 点;
   keep-alive が先に着地する（v1 の逐次ループ上で独立にテスト可能）。
   - **① `http.serve_shared(host, port) -> Result<http server, Error>`** — リスナー上の
     `SO_REUSEPORT` を除いて `http.serve` と同一であり、N ワーカーが 1 つのポート上でそれぞれ**自分
     専用の**リスナーを bind し、カーネルがコネクションを振り分ける。フラグではなく**兄弟**演算:
     `http.serve` は strict-bind セマンティクスを保つ（誤った 2 つ目のサーバは依然として大声で失敗
     すべき; ポート共有は明示的な選択 — `respond`/`respond_stream` の兄弟の前例、bool トラップなし）。
     ポータビリティ: Linux は適切に振り分ける; macOS は TCP に対しこのオプションを受け付けるが分配品質
     は未規定 — 記録するだけでゲートしない（ベンチ機は Linux）。
   - **② コネクション keep-alive は完全に `accept`/`respond` の内側で — ループ形状はどの呼び出し側に
     とっても不変。** サーバハンドル毎に **上限つきの parked 集合**（256 コネクション。満杯時は最も
     長く使われていないものを close して空きを作る）。設計当初は単一スロット — 「v1 の一度に 1
     コネクションの姿勢を明示化したもの」— だったが、これは**実装中に是正した**: 1 スロットでは新規
     コネクションが毎回それまでのものを evict するので、「このコネクションは持続する」と告げられた
     ばかりの client が次のリクエストを失う（POST では client 側で安全に再試行できない失敗である）。
     処理は依然として厳密に一度 1 リクエストずつであり、park されたコネクションは idle であって
     in-flight ではない:
     - **適格性**（パース時に計算され ctx に載る）: リクエストが HTTP/1.1 であり、`Connection: close`
       ヘッダを持たず、パースバッファ内に自分のボディを越える**残余バイトを残さない**こと（パイプ
       ライン化する client には応答してから close する — 残余の持ち越しは意図的に**作らない**; 本物の
       keep-alive client はレスポンスを待つので、残余 ≈ 皆無）。1.0 の keep-alive（レガシーの
       `Connection: keep-alive`）は非対応 — 従来どおり close。
     - **`ctx.respond`**: 適格 + 書き込み成功 → fd は close せずサーバのスロットへ **PARK** され、自動の
       `Connection: close` ヘッダは**省略される**（不在 = 持続が 1.1 のデフォルト; fasthttp も同様に
       する — ベンチ経路で最も軽いバイト数）。不適格 → `Connection: close` + close、従来どおり。
     - **レスポンスは常に自己完結的にフレーミングされるようになった**（実装中に確定; RFC 9112 §6.3）。
       `respond` は以前、ボディが SET されたときにのみ `Content-Length` を出していたので、ボディなしの
       `200` は「コネクションが閉じるまで」という枠付けになっていた — keep-alive できず、しかもワイヤ上で
       切り詰められた stream と区別がつかない。**完全に改めた:** ボディを持ちうるステータスのレスポンスは
       どちらの場合もフレーミングされる（セットされた長さ、またはボディ未設定なら `0`）; `1xx`/`204`/`304`
       はボディを持たないのでフレーミングヘッダを付けない; そこにボディが SET されていた場合は黙って
       落とさず**拒否する**（`Err`。同じ関数で呼び出し側指定の `Content-Length` が受けるのと同じ扱い）—
       この種のレスポンスはフィールドが何を言っていようと最初の空行で終端するので、そのバイトは
       keep-alive されたコネクション上で**次のレスポンスの先頭**として読まれてしまうし、呼び出し側の
       データを黙って捨てることはこの境界が絶対にやってはならないことだからである。`respond_stream` も
       同じステータスを拒否する（既に終わったレスポンスの後に stream を続けることはできない）。HEAD は
       従来どおり黙って抑制する — こちらはビルダからは見えない**リクエスト**が理由だからである。
       したがって keep-alive の可否は**リクエストだけ**で決まり、ボディなしのレスポンス
       （`web.status(201)`）もコネクションに留まる。
       `respond_stream`、`reject`、および全エラーパスは
       従来の常時 close セマンティクスを保つ（stream の終端子はその close であり、reject 窓はエラー
       パス — 2 つ目のフレーミングモードに値しない、記録のみ）。
     - **`srv.accept()`**: park が空 → 従来どおり素の `accept(2)`。そうでなければ
       `poll({…parked, listener}, infinite)`; park されたコネクションが readable → それを集合から
       **取り出し**、そこから**次の**リクエストをパースする（新しいパースバッファ — zero-copy view は
       リクエスト毎に保たれる）; listener が readable → 新規コネクションを取り、**park 集合には一切
       手を触れない** — 集合が有界なのは増える側（`respond` が満杯の集合へ park するとき最も冷たい
       1 本を evict する）であって、accept 側に弁は要らない。むしろそこに弁を置くと、集合に決して
       加わらないコネクションのためにも発火してしまう。本物の fd の逼迫には `NoFds` 経路（後述）と
       いう別の答えがあり、そちらは `accept` が実際に fd を使い果たしたときにだけ 1 本を費やす。
       （accept 時の弁は #595 で一度 ship され、まさにこの理由で #597 で削除された: 満杯のときに
       届いた `Connection: close` や不正なリクエストのたびに暖まったコネクションを 1 本殺し、暖まった
       集合が恒久的に目減りしていた。しかもどのテストもそれを観測していなかった。あの弁が唯一果たして
       いた実務 — FIN を返さずに消えた peer の parked コネクションの回収、そういうコネクションは沈黙して
       いるので `poll` は永久に報告しない — は、いまや accept したすべてのコネクションに設定する
       **`SO_KEEPALIVE`** が担う: カーネルの probe がやがてそれを hangup に変え、このループが閉じる。
       ミリ秒ではなく数時間だが有界であり、本物の fd 逼迫のもとでは `NoFds` が即座に回収する。削除の
       代償はワーカーあたりの最悪 fd 数で、parked MAX **+ 1**（in-flight の 1 本）になる。）準備完了の
       走査は常に parked を優先するのではなく、**ローテートする開始位置**から始める:
       「parked 優先」固定の走査では、忙しい keep-alive client が listener を完全に飢餓させてしまう —
       `SO_REUSEPORT` では自分専用のキューを持つので兄弟ワーカーが肩代わりして drain することはできず、
       新規コネクションは backlog が SYN を落とすまで滞留する。parked の EOF / パースエラー → その 1 本を
       close して見直す。idle タイムアウトは無い: idle な parked fd は `poll` で待つだけであり、それが
       accept の通常の idle 状態そのものである。`POLLNVAL` は `POLLHUP`/`POLLERR` と並べて監視する —
       これが無いと無効な fd がどの分岐にも当たらない revent を返し、待機ループが spin してしまう。
       `accept` の表面と `Result` は不変。
     - **不正なリクエストはもはや `accept` から一切表に出ない**（ここで是正した — keep-alive 以前から
       の挙動でもあり、prefork がそれを致命的にした）。smuggling / bare-LF / 平文ポートへの TLS と
       いったリクエストは、listener が全く健全なままの**リクエスト単位の障害**なので、`accept` はその
       コネクションを close して待機を続ける — parked 経路が既にやっていたのと全く同じである。これを
       返していたために、スキャナが 1 本繋いだだけで呼び出し側の accept ループ（`srv.accept()?`）が
       死んでいた; prefork では worker ごとに 1 本ずつでサーバ全体が落ちた。エラーを返すのは実際の
       `accept(2)` の失敗だけであり、それこそが serve ループにおける `srv.accept()?` を正しくする。
     - **一時的な `accept(2)` の errno も表に出ない** — 同じ論法をシステムコール自体に適用したもので
       ある。分類ひとつ（`classify_accept_error`）が、3 つのケースで全てを決める:
       - **ノイズ → `Again`。** `EINTR`; **`ECONNABORTED`**（client が SYN と `accept` の間に消えて
         おり、返るはずだったコネクションはもう存在しない — さもなければ client は繋いで即座に reset
         するだけで worker を殺せてしまう）; そして Linux では、accept(2) が明示的に「EAGAIN と同様に
         リトライして扱え」と述べている**接続にすでに保留されているネットワークエラー**（`ENETDOWN`,
         `EPROTO`, `ENOPROTOOPT`, `EHOSTDOWN`, `ENONET`, `EHOSTUNREACH`, `EOPNOTSUPP`,
         `ENETUNREACH`）— いずれも listener ではなく**そのコネクション**を記述している。Linux で実際に
         飛ぶのはこの最後の一群のほうである（Linux は通常ハンドシェイクを完了させ、reset は後から
         報告する）; `accept` からの `ECONNABORTED` はおおむね BSD の事象である。**`Again` は待機へ
         戻る。その場で `accept` をやり直すことは決してしない** — listener は blocking なので、その場で
         リトライするとスレッドがそこに張り付き、（同じ `poll` を共有する）parked keep-alive
         コネクションは無関係な新規コネクションが来るまで捌かれなくなる。ゆえに `http_accept_conn` は
         `accept` をちょうど 1 回だけ行い、リトライは呼び出し側のループが持つ。
       - **`EMFILE`/`ENFILE` → `NoFds`**、回復可能な fd の枯渇: `accept` は fd を 1 つ返してやり、
         リトライする。**どの 1 本を手放すかは、この待機の `revents` から選ぶ**: **リクエストが読めて
         いない**最も冷たい parked コネクション — 次のリクエストが既に届いているものを閉じれば、client が
         答えを受け取れないリクエストを落とすことになる — であり、全てが readable なら最も冷たいものを
         そのまま手放す。枯渇したテーブルでも前進はしなければならないからである。さらにこれは
         **待機 10 ms あたり 1 本にペーシングされる**（`http_yield_for_fds`）: prefork のワーカーは
         プロセスの fd テーブルを共有する一方、parked 集合は**別々に**持つので、足りない fd はたいてい
         兄弟のものであり（`ENFILE` に至ってはシステム全体で、自分の分を返しても全く効かないことが
         ある）、ペーシングが無ければ 1 ワーカーは自分が招いたのではない逼迫のもとで、暖まった集合を
         タイトループで一気に焼き尽くす。ペーシングの状態は `accept` の呼び出しごとに持つ — 焼き尽くしが
         起こりうるのはまさにそこだからである: accept に失敗し続ける呼び出しは、まず待つことなしに
         2 本目を手放すことはできない。（呼び出しが返ればリセットされる — だが呼び出しが返るのは
         リクエストを返すか `Fatal` で失敗するときだけであり、どちらもループではない。）手放せるものが
         尽きたら、ただバックオフする。
       - **それ以外 → `Fatal`**、そのまま返す: listener 水準の真の障害こそ、serve ループが目にすべき
         唯一の `accept` の失敗である。

       結果として、従来はサーバが死んでいたところが劣化（待っているリクエストを捌くために、暖まった
       コネクションを、ペーシングされた形で 1 本費やす）で済む。**規則のノイズ側の半分は std.net の
       `tcp_accept` と共有する述語 1 つ**である — あちらにも全く同じ穴があったし、accept ループは
       accept ループである。枯渇側は共有し**ない**: 素の listener は返してやれる parked 集合を持たない
       ので、その判断は `net` の呼び出し側が持ち続ける。
     - **interim（`1xx`）レスポンスは決して park しない** — 完全なレスポンスではないので、それを受け
       取った client は最終レスポンスを待つ。コネクションは keep-alive 以前と同様に close する。
     - **drop 順序の安全性（唯一の鋭い縁）:** ctx は解放済みのサーバへ park してはならない。集合は
       ランタイム内部の refcount セル（`Arc<Mutex<ParkSlot>>`）であり、サーバハンドルが保持し**かつ**
       accept 時に各 ctx へ clone される — リクエスト毎に refcount を 1 つ bump するだけで、ユーザに
       見える割り当てはなく、構築上 uncontended（prefork が全ワーカーに自分のサーバハンドルを与える
       ので、mutex はスレッドを決して跨がない）。サーバが先に drop → セルは dead としてマークされ
       `respond` は単に close する; ctx が先に drop → refcount が解放される。ランタイムの寿命の細部の
       ために sema/region の表面は追加しない（却下: ctx の region を srv に縛る — より重く、item 4 で
       出荷した free-standing ハンドルモデルには誤り）。
   - **テストマトリクス（仕様）:** 1 コネクション上で 2 リクエスト E2E（同一ソケット、両方 200、
     リクエスト毎に view が正しい）; `Connection: close` リクエストの尊重; 1.0 は close; パイプライン化
     （残余）リクエストは応答してから close; 満杯の集合へ新規コネクションが **park された**ときに最も
     冷たい parked fd が evict される（届いただけでは足りない — 上の弁の注記を見よ）; parked EOF の
     回復; stream/reject のコネクションは常に close; HEAD（ボディ抑制）+ keep-alive の合成;
     keepalive × pkg.web serve E2E（ループ不変）; `serve_shared` の二重 bind は成功する一方で素の
     `serve` の二重 bind は依然として失敗する; prefork E2E — W ワーカー、同時 client、他が応答する間に
     1 つの held-open stream。
   - **出荷記録。** ランタイム: 共有 `tcp_listen_impl(…, reuseport)` の背後に置いた `SO_REUSEPORT` +
     `align_rt_http_serve_shared`; parked 集合は `Arc<Mutex<ParkSlot>>`（`Live(Vec<fd>)`/`Dead`。
     後者は `HttpServer::drop` が設定し、park 中の fd を**すべて**閉じる）; 適格性は
     `http_read_request` で計算し（`http_request_wants_close` + 残余チェック）ctx に載せる;
     `align_rt_http_accept` は `http_wait_parked_or_listener`（新しい `poll(2)` extern）+
     `http_accept_conn`（1 回の呼び出しにつき `accept` はちょうど 1 回）を中心に再構成。その失敗は
     `classify_accept_error`（`Again`/`NoFds`/`Fatal`。ノイズ側の半分は net 側と共有する述語
     `accept_errno_is_noise`）を通し、枯渇時は `http_yield_for_fds` → `http_relieve_fd_pressure` へ
     回す; `http_serialize_head(rb, persistent, extra)` は keep-alive 経路で
     `Connection` 行を省く。言語側: `ExprKind::HttpServe`/`Rvalue::HttpServe` が `shared: bool` を得た
     （variant ではなく**フィールド** — 全解析パスは引き続き `http.serve` として扱う）、
     `http.serve_shared` は同じ `check_http_serve` を通って dispatch する。テスト: ランタイムの
     keep-alive unit 11 本（1 コネクション 2 リクエスト、3 つの不適格規則、HEAD との合成、新規の
     トラフィックが parked コネクションを evict **しない**こと、park 時の容量の弁が最も冷たい 1 本を
     evict すること — および満杯のときの one-shot リクエストは**誰の**コネクションも奪わないこと
     （#597、削除された accept 時の弁）、parked EOF の回復、fd 衛生、ボディなしレスポンスの
     フレーミング、ボディを持てないステータスがセット済みボディを抑制すること、interim レスポンスが
     park しないこと、4 クライアント同時 park）
     + `serve_shared` の二重 bind unit + `accept` の errno unit 3 本（保留ネットワークエラー族を含む
     分類テーブル; リクエストが待っているものより idle なコネクションを選ぶ回収と、バックオフ 1 回
     につき 1 本のペーシング; および `RLIMIT_NOFILE` を下げた別プロセスでの fd 枯渇 E2E — fd テーブル
     が満杯の状態でも parked コネクションが回収され、待っているリクエストは変わらず応答される。
     libtest の「フィルタ不一致でも exit 0」に対しては、子プロセス自身の "1 passed" サマリを
     assert して fail-open を塞いでいる）;
     driver の `m11_http_server.rs`（`serve_shared` E2E + ゲート）、
     `apps_web_root.rs`（keep-alive × pkg.web のループ。2 番目の client が 1 番目からコネクションを
     奪わないことを含む）、`apps_web_prefork.rs`（同時 client、`/proc/net/tcp` から読み出した
     ワーカー毎に 1 つのリスナー、他が応答する間に 1 ワーカーを占有する held-open stream、範囲外の
     `workers` の abort）。
   - **呼び出し側/テスト向けの挙動注記:** 適格な HTTP/1.1 リクエストはコネクションを**開いたまま**に
     するので、EOF まで読む client はサーバが終了するか容量の逼迫が parked コネクションを evict
     するまでブロックする。コネクション毎 1 リクエストの client は `Connection: close` を送る（driver
     テスト共有の `one_shot` ヘルパ）か、`Content-Length` で読みをフレーム化しなければならない。
10. **`ctx.headers()` — 切り離したヘッダーテーブルのビュー — 2026-07-21 出荷**（ブランチ
    `http-headers-view`。消費者 = pkg.web の `web.header(c, name)`、`pkg-design/web.md` →
    「ctx アクセサ」）。以下の設計はその記録である。①–⑨ は書かれたとおりに出荷され、末尾の
    **What actually shipped** が、実装が設計の書いていないことを教えた 4 箇所を記録している。

    **問題を正確に。** framework のリクエスト毎コンテキストは、**何も所有しないビューだけの Copy
    struct** である — pkg.web の `Ctx` がその形をしているのには荷重のかかった理由がある（所有する
    `Ctx` は自分自身のアクセサに消費されてしまい、失敗したハンドラは framework が 500 を返すために
    なお必要とするハンドルを消費済みにしてしまう）。他のアクセサはどれも struct が運べるビューに乗る:
    `method`/`path`/`query` は `str`、`body` は `slice<u8>` である。**ヘッダー lookup はそれができない。
    名前はハンドラが問い合わせるまで分からないからである** — 借用される値は 1 つのスパンではなく、
    パース済みテーブル全体である。したがって、framework が raw head のビューの上に RFC 9110 lookup を
    再実装する（std.http が既に持つものの 2 つ目の実装 — One way に反する）か、std.http がテーブル
    **そのもの**である値を借用として手渡すか、どちらかになる。本項はその値である。

    - **① 表面。** `ctx.header(name)` は補完ではなく**置換**である — lookup の綴りを 1 つに保つため:
      ```text
      ctx.headers() -> http_headers        // a Copy, non-owning VIEW of the parsed header table;
                                           // region-bound to ctx exactly like ctx.body()
      hs.get(name: str) -> Option<str>     // RFC 9110 §5.1 case-insensitive lookup; the returned
                                           // str views the request buffer, region-bound to `hs`
      ```
      `ctx.header(name)` は全呼び出し箇所で `ctx.headers().get(name)` になる（ドキュメント以外に Align の
      呼び出し箇所は無い — 今日のコストはゼロで、機構が 1 つになる利益は永続する）。パース済みの
      **レスポンス**は `resp.header(name)` のままである: 呼び出し側が既に所有している値からわざわざ
      切り離す理由が無いし、ビュー型を共有するなら無関係な 2 つのランタイム struct を区別する判別子を
      持たせる羽目になる。この非対称性は意図的なものであり、取り繕わずここに記録しておく。
    - **② 表現 = ctx ポインタそのもの。** `align_rt_http_ctx_header` は既に `*const HttpRequestCtx` +
      名前を取って `AlignStr` を書き出す — つまりビューは*同じポインタ*であり、`hs.get(name)` は
      **既存の呼び出し**へ lower する。**ランタイムのコードは一切増えない**; `ctx.headers()` は ctx
      オペランドの `Rvalue::Use` へ lower する。この enabler は丸ごと型システムの変更である。
    - **③ 型 = `Ty::HttpHeaders` — Copy、非所有、region 追跡あり。** 倣うべき前例は Move ハンドルでは
      なく `Ty::JsonDoc`/`Ty::JsonScanner`（Copy + `tracks_region` + `ty_may_borrow`）である。これは裸の
      8 バイトポインタであり、今日の Copy 型にそういうものは 1 つも無いので、`ty_size_align` には
      `(16, 8)` の catch-all ではなく専用の `(8, 8)` の腕が要る。
    - **④ region の意味論 — 設計全体が懸かっている 1 行。** `region_of` の
      `HttpCtxMethod | HttpCtxPath | HttpCtxHeader | HttpCtxBody` の腕は、結果を
      `Frame.shorter(region_of(ctx))` で頭打ちにする。lookup がこれを継承すると、その頭打ちは
      `fn header(c: Ctx, name: str) -> Option<str> = c.headers.get(name)` — pkg.web のラッパーであり、
      そもそもの狙いそのもの — を**コンパイル時に拒否**させてしまう（「ローカルストレージを借用する
      ビューは返せない」）。等価な `ctx.header` ラッパーを使い、今日のコンパイラ上で確認済み。したがって
      2 つの操作は分離しなければならない:
      - `ctx.headers()`（**新規**の `ExprKind::HttpCtxHeaders`）は頭打ちを保つ:
        `Frame.shorter(region_of(ctx))`。ローカルのハンドルから作られたビューは、そのハンドルを所有する
        フレームの外へは出られない。
      - `hs.get(name)`（**既存**の `ExprKind::HttpCtxHeader`。そのオペランドをハンドルからビューへ
        差し替える）は**継承**する: `region_of(hs)`。パラメータ経由 — 呼び出し側が呼び出しより長生き
        すると証明できる場所 — ではこれが `Static` になり、ラッパーがコンパイルできる。これは
        `str`/`slice` のビューがパラメータ経由で既に従っているのと全く同じ規則であり、新しい例外では
        ない。
    - **⑤ 新しい `Ty` がタダでは手に入らないソウンドネスのチェックリスト。** `Ty` の variant 追加が
      コンパイラに強制されるのは**4 つ**のパスである（`ty_mentions_slice`、`tracks_region`、そして 2 つの
      `ty_name`）。それ以外はすべて `matches!` のリストか、fail **open** する `_ =>` の腕である。
      見落とすと致命的なものが 3 つあり、うち最初の 2 つは**ペア**である — どちらか一方だけでも同じ静かな
      use-after-free を生む（`hs := ctx.headers()` … `ctx.respond(rb)?` … `hs.get("host")` が解放済み
      バッファを読む）:
      - **`ty_may_borrow`** — これが無いと `Let` はそのビューの借用 provenance を一切記録しない。
      - **`borrow_sources_inner`** — その末尾は `_ => BorrowRoots::new()` なので、他の 8 つのパスが
        網羅的であるにもかかわらず、ここでは新しい `ExprKind` が強制**されない**。`HttpCtxHeaders` は
        ctx のストレージ root へ写像しなければならない。（既存の `HttpCtxHeader` の腕は既に
        `storage_roots(operand)` を読んでおり変更不要 — 上の 2 つの追加があれば、`storage_roots` の
        `_ => borrow_sources(e)` の落ち込みが一時ビューを正しく連鎖させる。）
      - **`scalar_type` のポインタの腕** — 見落とすと `_ =>` が `int_type` の `_ => i32` へ落ち、
        ポインタを黙って切り詰める。この全く同じバグは `Ty::Fn` で一度実際に起きている。
      続いて: `is_field_ok`（さもないと `Ctx` がこれを運べない）、`resolve_type` —
      `http_request_ctx`/`response_builder`/`http_stream` と同じく import 不要の**グローバル表面名**
      として。`pkg.web.types` が設計どおり依存の無い葉のままでいられるようにするためである — そして
      `ty_size_align`。**`ty_size_align` は安全性ではなく lint の精度である:** 唯一の消費者は
      huge-struct-copy lint であり、実レイアウトは `scalar_type` + `field_abi_align`（`_ => 8`、既に
      正しい）が決める。`Ty::HttpRequestCtx` — 出荷済みの struct フィールド型 — も今日まったく同じ
      16 対 8 の過大報告をしていることに注意。両方を直し、`sema_and_codegen_struct_layout_agree` に行を
      足す。あれは手書きのテーブルで、Move ハンドルにも `Ty::Fn` にも `Ty::Slice` フィールドにも行が無い。
      あえて追加**しない**もの: `ty_is_move`、`is_owned_droppable`、`handle_free_fn`、
      `null_moved_source`、`DropPlan` の所有 leaf 集合（囲む struct を Move にしてはならない）、そして — 重要な
      ことに — **`Scalar` variant は作らない**。これにより、fail-closed のデフォルトのままビューは
      `Option`/`Result` のペイロードと配列要素から締め出される（専用の診断を用意する価値はある: 今日は
      「must be a scalar (composite payloads are not supported yet)」と報告する。答えは正しいが筋書きが
      違う）。
    - **⑥ dispatch と effect、どちらも間違えやすい。** `"get"` は既に catch-all の腕
      （`"get" if recv_ty != Ty::HttpClient => check_box_get`）が押さえており、`hs.get(name)` を
      *"'get' takes no arguments"* の診断へ飲み込んでしまう — ビルド失敗ではなく単に悪いメッセージなので、
      何も捕まえてくれない。新しい腕はその**上**、同じ理由で `json.doc` の腕が置かれているのとちょうど
      同じ位置に置く。`check_http_ctx_method` が適用するレシーバの **place ゲート**（`Local | Field`）は
      `ctx.headers()` には保ち（これは引き続き `srv.accept()?.headers()` を拒否する）、`hs.get(name)` には
      **継承させない** — 規定の綴り `ctx.headers().get(name)` のレシーバは place ではなく、ビューは drop
      すべきものを何も所有しないからである。MIR は `lower_expr` に新しいインラインの腕を足すのではなく
      `lower_http` の dispatch リストを通す（`expr_depth` の余裕に関する注記、#296）。Effect は **Pure** —
      ポインタのコピーであり、lookup は不変バッファの読み取り専用スキャンである。ヘッダーを読むハンドラが
      `par_map`/`task_group` の下で合法であり続けられるのは、これによる。
    - **⑦ v1 に入れないもの: 反復。** `hs.count()`/`hs.name(i)`/`hs.value(i)` は全ヘッダーを転送する
      プロキシに役立つはずであり、ランタイムは既にスパンを持っている。却下ではなく先送りである: REST が
      必要とするのは lookup であり、アクセサが 1 つ増えるたびに、fail open する `Ty` の掃きに晒される
      ノードが 1 つ増える。列挙を必要とする消費者が現れたら、同じビュー上の兄弟ノード 3 つで済む —
      新しい型は要らない。
    - **⑧ 却下した代替案と、その理由。** *ヘッダーを `slice<str>` フィールドへ先に取り出す*（新しい
      `Ty` が一切要らない）が最有力であり、2 つの点で負ける: ランタイムが持つのは `AlignStr` ではなく
      オフセットのスパンなので、slice の実体化は**リクエストごとの割り当て**になる — しかも現在の性能
      目標である 4.1 µs の予算の上で — そして lookup は pkg.web 側に住むことになり、2 つ目の RFC 9110
      実装になる。*`Ctx` に `http_request_ctx` を借用させる*は最初から論外である: ハンドルは Move なので
      `Ctx` が Move になり、`types.align` に記録された理由がすべて再来する。*切り離したリクエストビュー
      全体へ一般化する*（1 つの値の上の `.method()/.path()/.body()`）は「これは一般化すべきでは?」という
      自然な問いだが、今日は負ける: その 3 つは既にハンドル上で動くし、pkg.web の `path`/`query`/`pattern`
      は素通しではなく**導出**されるので、どのみち `Ctx` は畳めない。そしてアクセサが 1 つ増えるたびに、
      fail open する掃きを通るノードが 1 つ増える。名前がヘッダーの形のままなのは、それがこの型の用途
      だからである。
    - **テストマトリクス（仕様）:** パラメータ経由のラッパーがコンパイルでき、ビューを返せること
      （分離を動機づけている性質そのもの）; **ローカル**のハンドルから作ったビューは、返すことも、
      ループから `break` して出すことも、`serve` の反復をまたいで自分の ctx より長生きすることもできない
      こと; `ctx.respond(rb)` の後の `hs.get()` はコンパイルエラーであること — ただし**裸のローカル**で。
      囲む struct に `str` フィールドが 1 つでもあると、それが借用 root を供給して穴を覆い隠してしまう
      からである; `ctx.respond_stream(rb)` の**後**に **stream の pump の中**で行う `hs.get()` は
      **コンパイルでき、かつ動く**こと（この経路は ctx を借用し、決して解放しない）; 大文字小文字を無視
      したヒット + ミス（`Option`）の pkg.web 経由 E2E; 配列要素 / `Option` ペイロードとしてのビューは
      拒否されること; これを運ぶ `Ctx` が Copy のままであること（drop は出ず、二重 free も無い）、および
      sema と codegen が一致することを assert する struct レイアウトの行。
    - **⑨ `ctx.header(name)` の削除掃き。** Align の呼び出し箇所はどこにも無い（`.align` ファイルにも、
      Rust に埋め込まれたテストプログラムにも）— 確認済み。変わるのは: 本ファイル（§ Signatures +
      Slice breakdown）、その ja ミラー、`docs/open-questions.md`、そしてコンパイラ側 — メソッド dispatch
      の腕、`check_http_ctx_method` の `"header"` ケース、そして "try method / path / header / …" の
      サジェスト文字列。`rb.header` / `r.header` / `resp.header` はレシーバが違うので残る。
    - **実際に出荷されたもの（2026-07-21）— 設計が不完全だった箇所も含めて。** ①–⑧ は設計どおり:
      `Ty::HttpHeaders`（Copy、`tracks_region`、`ty_may_borrow`、`Scalar` 無し、専用の `(8, 8)` の腕）、
      `Rvalue::Use` へ lower する `ExprKind::HttpCtxHeaders`、既存の `HttpCtxHeader` ノードのビューへの
      差し替え、region の分離（`headers()` は `Frame` で頭打ち、`get` は継承）、`check_box_get` の上に
      置いた `hs.get` の dispatch の腕、Pure、そして新しいランタイムコードは **ゼロ**。
      `Ty::HttpRequestCtx` の 16 対 8 の過大報告も一緒に直り、`sema_and_codegen_struct_layout_agree` には
      Move ハンドルのフィールド、`http_headers` のフィールド、`Ty::Fn` のフィールド、`slice<T>` の
      フィールド、そして pkg.web の `Ctx` そのものの形の行が加わった。設計自身の記述に対する訂正が 4 つ:
      - **⑤ は新しい `ExprKind` に対してコンパイラが強制するパスを数え落としている。** `region_of` と
        `slice_is_local` はどちらも `ExprKind` に対して網羅的であり、したがって設計全体が懸かっている
        当の *region* の規則は fail open ではなく **fail-closed** である。fail open な表面は、⑤ が `Ty`
        レベルで挙げているもの（`ty_may_borrow`、`scalar_type`）と、⑤ が正しく特定している唯一の
        `ExprKind` の末尾（`borrow_sources_inner`）だけである。それぞれを mutation-check した:
        `ty_may_borrow` か `borrow_sources_inner` の腕を落とすと、裸のローカルでの `respond` 後 use の
        テスト **と** 反復をまたぐテストの両方が捕まえる。`scalar_type` の腕を落とすと pkg.web の E2E が
        全滅する。`tracks_region`・`region_of`・`slice_is_local` を落とすとビルドが通らない。
      - **⑤ の「専用の診断」は 1 箇所ではなく 2 箇所必要だった。** `payload_scalar` は 2 つある —
        `what` ラベルを取る自由関数版と、`Checker` のメソッド版 — で、メソッド版は全呼び出し元に対して
        `"Option payload"` をハードコードしていた。そのため*配列要素*としての拒否が自分を Option の
        ペイロードだと名乗っていた。メソッドは今やチェック中の位置を引数に取り、この型に限らず
        すべての型についてこの既存の誤ラベルを直している。
      - **テストマトリクスの「`serve` の反復をまたいで自分の ctx より長生きすることはできない」は
        半分しか正しくなく、残り半分は既存の穴だった — 2026-07-22 に修正済み（このスライスではない）。**
        `MoveCheck` が借用の世代を終わらせるのは所有者が**ムーブまたは再代入**されたときだけで、
        ループの反復の終端で **drop** されたときではなかった。そして `Region::Frame` は「このフレーム」と
        「この反復」を区別できない。pkg.web の形が安全だったのは `ctx.respond(rb)` が毎回ハンドルを
        **ムーブする**からであり、そのケースは元から拒否されていた。ctx をただ drop させるだけの
        ループ本体は次の反復へビューを漏らしていた。これは Move ハンドル上のあらゆるビューに共通で
        （`ctx.path()` の素の `str` でも、std のハンドルが 1 つも出てこない素の `string` でも同一に再現する）。
        `loop_moves` が今やループの反復ごとの drop 集合を back-edge と全ての `break` で終わらせる。
        詳細は `docs/open-questions.md` の #460 の隣。**ここでの主張のうち 2 つは誤りだった**:
        `arena {}` の変種はこのバグの実例ではまったくなく（`arena {}` 内のヒープ所有ローカルは*関数*の
        終端で drop される — `emit-mir` は `arena_end` の後に drop を出す — 本当にアリーナ上にある
        ストレージは region 規則が既に捕まえる）、この型が `str` より確実に派手に失敗するわけでもない:
        どちらも形状とアロケータに依存する UB である。
      - **⑨ のサジェスト文字列は削除した名前には届かない — だから削除した名前に専用の腕を与えた。**
        ctx メソッドの dispatch の腕は名前でガードされている（`"method" | "path" | "headers" | …`）
        ので、`ctx.header(x)` は `check_http_ctx_method` に到達せず、本来なら着地するはずだった
        "try …" のリストは古い綴りの呼び出し側がたどり着く場所ではない。たどり着く先は汎用の
        *"unknown method"* だった。レビュー 1 巡目がヒントを本物にすることを求めたので、
        `"header" if recv_ty == Ty::HttpRequestCtx` の腕が置換後の綴りを明示して**エラーにする** —
        何も resolve しないので、compat path ではなく診断である。（サジェスト文字列自体も更新した。
        ガードのリストに**入っている**未知のメソッドには引き続き役立つ。）

## Known v1 limitations (Slice 2/3/5)

- **HTTPS はクライアント側のみ(スライス 5)。** サーバ側 TLS は先送り — `http.serve` は平文であり、その
  記録済みの信頼済みネットワークの caveat(下記)が残る。クライアント優先は align-LLM A5 の消費者と一致する。
  サーバ TLS は不完全な出荷ではなく、一貫した v1 後の作業である。
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
  net の基盤（レール）が文書化している no-timeout の挙動(`align_rt_tcp_connect`)を、http クライアントが connect
  **と** read の両方で継承したものである。**スライス 3 での判断(記録のみ、未実装):** スライス 2 の注記は
  タイムアウトのフォローアップを「プールが per-conn のデッドライン管理を必要とするスライス 3 のプール作業と
  ともに」入れると書いていた。だがスライス 3 を実装してみると、その言い回しは別物を混同していた。プールの
  デッドライン管理とは **idle 期限切れ**(90 秒より古いコネクションを再利用しない)であり — これはスライス 3
  が**実装している** — connect/read の **I/O デッドライン**ではない。本物の I/O タイムアウトの追加は
  分離可能でより大きな変更であり、http ローカルな理想形を持たない:(1)**connect** タイムアウトの理想的な
  置き場は net 基盤（レール）(non-blocking `connect` + `poll` の基盤 — net.md が後日のバックエンドとして既に挙げて
  いる)であり、http に半分だけ入れれば二つ目の部分的な機構になる。(2)**read** タイムアウトは数行
  (`SO_RCVTIMEO`)だが、*固定値*は正当な低速/大容量転送を黙って壊し、v1 には凍結済みの
  `get`/`post`/`request` シグネチャを広げずにリクエスト単位で設定する**設定面が無い**(別の設計判断)。
  「理想形か、さもなくば先送り」に従い、スライス 3 はプールの idle 期限切れと SIGPIPE 安全/スタール再試行の
  堅牢性を出荷し、**I/O タイムアウトは net 基盤（レール）の non-blocking/deadline 基盤へ先送り**する
  (セマンティクス上は不変)。半端な実装を入れる代わりに、v1 の既知の制約としてここに記録する。
  - **→ 解決(align-llm Request 2)。下記「I/O timeouts」を参照。** align-llm(コネクションを
    ブラックホール化しうるエンドポイントへの LLM API 呼び出し)が、具体的なクライアント需要と欠けていた
    設定面を提供した。net 基盤（レール）の non-blocking-`connect` 基盤 + `SO_RCVTIMEO`/`SO_SNDTIMEO` と、
    クライアント既定 / リクエスト単位上書きの `timeout(ns)` — 本注記が欠けていると言っていた 2 つ — は
    **実装済み**(G3-1 の先送りはクライアント側で完全に解決)。上記のサーバ側エスカレーションは引き続き
    独自の v1 後の堅牢化項目として残る。
  - **サブケース — HEAD / 304 のフレーミング(スライス 1/2 から継承)。** `HEAD` レスポンスや
    `304 Not Modified` は、正当に `Content-Length` ヘッダを持つが**ボディを持たない**。v1 の読み取り
    ループは純粋に `Content-Length` でフレーミングする(リクエストメソッドやステータスで特別扱いしない)
    ため、決して来ないボディバイトを待ち続ける → 上と同じ無期限ブロックになる。v1 の表面は `HEAD` を
    手軽には出していない(`get`/`post`/`request` のみ)が、メソッド `HEAD` で組んだ `request` はこれに
    当たる。**下記 align-llm Request 4 で実装済み。** 同じ capability が method/status-aware
    framing、informational response の advancement、client-side chunk de-framing を、二つ目の transport
    surface を増やさずに追加する。
- **~~`https://` の拒否が粗い(DC-1, low)。~~ スライス 5 で解消。** `https://` はもはや `Error.Invalid`
  に写らず、検証済み TLS 経路にルーティングされる。検証失敗は明確な `Error.Denied`、TLS トランスポート不良は
  `Error.Code`、プロトコル違反は `Error.Invalid`。(メッセージレスな `Error` enum はより広い別課題として残る
  が、DC-1 の「HTTPS 未対応」負債は解消 — HTTPS は *対応済み*。)

## I/O timeouts (align-llm Request 2 — DESIGNED 2026-07-24, IMPLEMENTED 2026-07-24)

上記 G3-1 の先送りを解決する。動機は `align-llm` の LLM API 呼び出し(`POST /v1/chat/completions`)であり、
コネクションをハングまたはブラックホール化しうるエンドポイントが相手である。タイムアウトが無いと、1 リクエストが
verify/repair ループ全体を無期限に停滞させる。出典:`../align-llm/docs/align-requests.md` の Request 2
(優先度: high)。

### Surface — one `timeout(ns)`, client default + per-request override

どちらも既存の束縛ローカル Move ビルダへの in-place セッタである(`.header()`/`.body()` のイディオム — `()` を
返し、凍結済みの `get`/`post`/`request` シグネチャに新引数を足さない):

```text
cl := http.client()
cl.timeout(ns: i64)              // default timeout for every request on this client   -> ()
r := http.request("POST", url)
r.timeout(ns: i64)               // per-request override (0 = "use the client default") -> ()
```

クライアントの `ns == 0` は「タイムアウト無し」を意味する(現在のブロッキング挙動 — 後方安全な既定)。リクエスト
自身の `timeout` はクライアントのものを上書きし、未設定のリクエストタイムアウトはクライアントのものを継承する。
負の `ns` は構築時に拒否する(CR/LF に対する `r.header()` と同じく abort)。

**connect/read/write を別々にではなく 1 つのノブ**(owner criteria: 一つのやり方、人間が理解できる)。単一の `ns` を
**各**ブロッキング操作 — connect、send、receive — のデッドラインとして適用する。よって accept しない相手は connect
の上限に当たり、accept した後に応答しない相手は receive の上限に当たる。これは操作ごとのデッドラインであり、
リクエスト全体にわたる単一の実時間デッドラインでは**ない**(後者は connect+send+recv を貫くデッドライン算術を要し、
このワークロードには利益が少ない)。操作ごとと文書化する;これが最も単純な表面でゲート(ハングしたリクエストが
上限内に返る)を満たす。

### A timeout is `Error.Timeout`

「トランスポート/TLS/不正メッセージの失敗はエラー;HTTP ステータスはデータ」と一貫して、タイムアウトは
**`Error.Timeout`** バリアントとして表面化する(共有の新バリアント — 正準定義は `process.md` の
「Shared prerequisite: the `Error.Timeout` variant」;`AL_TIMEOUT = 4`)。これは `Error.Code`(トランスポート
errno)、`Error.Denied`(TLS 検証失敗)、4xx/5xx ステータスを運ぶ通常の `Ok(response)` とは別である。

### Runtime design (raw-libc sockets; all hook sites already located)

- **connect タイムアウト** — net 基盤（レール）(net.md)。`align_rt_tcp_connect`(現在はランタイム `:679` の
  ブロッキング `connect`、`:621` の「no connect timeout」注記)が `timeout_ns` パラメータを得る:fd を
  non-blocking にし、`connect`(`EINPROGRESS` を期待)、ns デッドラインで `poll(POLLOUT)`、`SO_ERROR` を確認;
  poll タイムアウト時に **`AL_TIMEOUT`** を返す。`timeout_ns == 0` は今日のブロッキング経路を不変に保つ。
  `http_connect_fd`(`:14748`)が有効リクエストタイムアウトを下へ渡す。
- **read タイムアウト** — conn fd への `SO_RCVTIMEO`(`plain_read`/`read`、`:14001`;TLS 経路の `SSL_read` も
  同じ fd を使うので、ソケットオプションがそれも縛る)。期限切れは `EAGAIN`/`EWOULDBLOCK` を生む;read 箇所で
  それを **`AL_TIMEOUT`** に変換する(汎用 errno 経路ではない — 自分のデッドラインだと分かっている)。
- **write タイムアウト** — fd への `SO_SNDTIMEO`(`http_send_all`/`send`、`:14511`)。対称。
- 有効タイムアウトは connect 直後(plain と TLS の両方)、最初の I/O の前に fd へ設定する。新リクエストで再利用
  されるプール済み keepalive conn は、新リクエストの有効タイムアウトを再適用する(プールは conn を格納し、
  デッドラインは格納しない)。

### New machinery

`Error.Timeout` + `AL_TIMEOUT`(共有、process.md 参照)。`align_rt_tcp_connect` が `timeout_ns` 引数を得る
(net.md のスライス、`std.net` と共有)。クライアント + リクエストハンドル(`HttpClient` / `HttpRequest` ランタイム
構造体)への `timeout_ns` フィールドと、新セッタ `align_rt_http_client_timeout` / `align_rt_http_timeout`、
および `timeout` に対する sema の `HttpClient`/`HttpRequest` メソッドディスパッチ。有効タイムアウトの解決
(リクエスト上書き、無ければクライアント既定)は `http_client_perform` で行う。

### 実装済みの姿(2026-07-24)

- **言語。** 新しい HIR/MIR/codegen バリアント `HttpRequestTimeout { req, ns }` /
  `HttpClientTimeout { client, ns }`(`Ty::Unit`、i64 引数、束縛ローカルレシーバ —
  `r.body`/`c.timeout_ns` と同じ形)を、あらゆる sema パス(EffectScan — **Pure**、フィールド格納;
  `region_of`→Static;`slice_is_local`;EscapeCheck walk/visit;MoveCheck — 借用のみ、何も消費しない;
  `borrow_sources`;finalize)へ通し、`lower_http` 経由で `align_rt_http_timeout` /
  `align_rt_http_client_timeout` へ下ろす。負の `ns` はセッタで abort、非 i64 引数は型エラー。
- **有効タイムアウト。** `http_client_perform` が `req.timeout_ns > 0 ? req : client` を解決
  (クライアント既定は `get_many` の共有ワーカが安全に読めるよう `AtomicI64`)し、1 つの `ns` を
  `http_connect_fd`/`http_tls_connect`(→ `align_rt_tcp_connect(…, ns, …)`、connect + ハンドシェイク)と
  `http_arm_conn_timeout(fd, ns)`(`SO_RCVTIMEO` + `SO_SNDTIMEO` 両方)へ通す。後者は **毎リクエスト** —
  新規 AND 再利用プール conn — で交換前に arm する。**再 arm/クリア:** `SO_*TIMEO` はプール済み fd に
  残るため、再利用 conn は常に再 arm し、`ns == 0` はゼロ `timeval` =「永久ブロック」へ**クリア**する
  (タイムアウト付きリクエストがプールした conn が、後続の無タイムアウトリクエストへ古いデッドラインを
  持ち込まない)。新規 conn で `ns == 0` の場合は arm 自体をスキップ(`setsockopt` 無し — タイムアウト前と
  バイト一致)。
- **失効 → `AL_TIMEOUT`。** 平文 `plain_read`/`http_send_all` はブロッキング fd の
  `EAGAIN`/`EWOULDBLOCK` を `io_read_write_status` で写す(ブロッキングソケットはデッドラインが arm された
  ときのみ `EAGAIN` を返すので、`ns == 0` では写像はバイト一致)。**TLS** が微妙な部分:`SO_*TIMEO` の失効は
  下層の `recv`/`send` を `EAGAIN` にし、OpenSSL はそれを `SSL_ERROR_SYSCALL` **または**
  `SSL_ERROR_WANT_READ`/`WANT_WRITE`(バージョン依存)として表面化する。`tls_read`/`tls_write_all` は
  `SSL_get_error` の**前に** `errno` を捕捉し、デッドラインが arm されているとき(`has_deadline`)に
  `err ∈ {SYSCALL, WANT_READ, WANT_WRITE}` **かつ** `errno ∈ {EAGAIN, EWOULDBLOCK}` を `AL_TIMEOUT` に
  写す(それ以外は `WANT_*` はリトライ、`SYSCALL` は errno を写す — 従来通りで、`ns == 0` はバイト一致・
  スピンなし)。ハンドシェイク(`SSL_connect`)も同じ arm された fd 上で走り、同一条件を `AL_TIMEOUT` に写す。

### Test / gate

**コネクションを accept するが決して応答しない**エンドポイントへのリクエストが、無期限にブロックする代わりに設定
上限内で `Err(Timeout)` を返す(Request-2 の受け入れゲート)。ブラックホール化された(決して accept しない)アドレスへの
connect が上限内で `Err(Timeout)` を返す。`ns == 0` は現在のブロッキング挙動を保つ。通常の高速リクエストは影響
を受けない。**出荷したテスト:** `align_runtime` ユニット — セッタ格納 + リクエストがクライアントを上書き、
accept 後無応答 → `AL_TIMEOUT`(read 経路、`SO_RCVTIMEO` + 平文写像のエンドツーエンド)、フルバックログの
ループバックブラックホール → `AL_TIMEOUT`(connect 経路、Linux)、高速応答は無影響 — さらに
`crates/align_driver/tests/http_timeout.rs` の Align 表面経由 E2E(`cl.timeout` / `r.timeout` →
サイレントサーバで `Err(Error.Timeout)`、リクエスト単位上書き、高速時は無効、および unbound レシーバ / 非 i64
のコンパイルゲート)。TLS のタイムアウト経路は共有 `SO_*TIMEO` 機構と平文 E2E でカバーされる(正の TLS
ラウンドトリップはドライバハーネスから駆動できない — `#[cfg(test)]` の信頼フックがそこには無い、スライス 5 の
記録どおり);TLS トランスポートが唯一異なる `tls_read`/`tls_write_all` の `has_deadline` 写像は上に記述した。
既存の http/TLS/pool/get_many/stream テストはすべて不変で通る(有効 0 のバイト一致不変条件)。

## Client response framing (align-llm Request 4 — DESIGNED + IMPLEMENTED 2026-08-14)

`align-llm` の C1 provider adapter はすでに SSE event を parse するが、provider response は一般に
HTTP/1.1 `Transfer-Encoding: chunked` を使う。Request 4 より前の whole-body client は provider が body を
見る前にこの framing を拒否していた。Request 4 は既存の `cl.request` boundary を完成させる。chunked response
を de-frame し、同じ owned `response` と zero-copy `resp.body()` view を返す。public type、method、
option、ABI entry point、streaming-input handle、provider 固有 abstraction は追加しない。

これは framing capability であり、設定可能な receive-bound capability ではない。未設定の
exchange では既存の固定 `HTTP_MAX_BODY` ceiling が decoded-body ceiling のままで、超過は
`Error.Invalid` のままである。Request 5 は下記の explicit receive-bound behavior を追加する。
align-llm Request 5 が public client/request cap と limit-specific error を別途所有する。両者を
組み合わせた cap/framing adoption gate は Request 5 が所有する。

### Public-contract ledger

| Surface / state | Exact contract |
|---|---|
| Existing calls | `cl.get(url)`、`cl.post(url, body)`、`cl.request(req)`、すべての `cl.get_many` worker がこの framing engine を使う。`http.parse(bytes)` は request method を持たないため ordinary method semantics で同じ complete-response decoder を使う。signature は変えない。 |
| Ownership and views | success は既存の Move `response` を返す。exact final response head と、それに続く contiguous decoded body を含む一つの retained byte buffer を所有する。header span は exact final-head byte を引き続き index し、`status()`、`header()`、`body()` の lifetime/allocation rule は不変。interim head、chunk line、delimiter、trailer は保持しない。 |
| Request-method selection | exact uppercase `HEAD` だけが HEAD response semantics を選ぶ。exact uppercase `CONNECT` は、この whole-body API が tunnel を返せないため、DNS、connect、write、pool access、その他の network side effect より前に `Error.Invalid` で拒否する。lowercase/mixed-case の `head` と `connect` は ordinary extension method。`http.parse(bytes)` は ordinary non-HEAD semantics を使う。 |
| Response-head wire syntax | すべての interim/final head は exact CRLF を使う。status line は exactly `HTTP/1.0 SP 3DIGIT SP [reason-phrase] CRLF` または `HTTP/1.1 SP 3DIGIT SP [reason-phrase] CRLF` で、code は `100..=999`、reason phrase は HTAB、SP、visible ASCII、obs-text だけを含む。各 field name は nonempty RFC `token` の直後に `:` を持ち、field value は HTAB、SP、visible ASCII、obs-text だけを含む。bare LF、unknown version、余分/不足した status separator、obs-fold、`:` 前の whitespace、その他 control byte は `Error.Invalid`。supported chunked transfer coding はさらに HTTP/1.1 を要求する。head 全体を framing/body suppression 選択より前に validate する。 |
| Informational responses | `101` 以外のすべての `100..=199` response は interim head。validation し、`Content-Length` と `Transfer-Encoding` をどちらも持ってはならず、payload を消費せず、discard して次の co-read byte から parse を続ける。各 terminating empty line を含む interim/final head wire span 全体で一つの cumulative `HTTP_MAX_HEADER_BLOCK = 262,144` byte allowance を共有する。各 head は独立に `HTTP_MAX_HEADERS = 128` を守る。`101` は `Error.Invalid` で、response/upgraded handle を公開しない。 |
| Bodyless final responses | exact `HEAD` への final response と final `204`/`304` は zero-length body を作り、payload、chunk terminator、trailer を読まない。status `204` 以外の exact `HEAD` とすべての `304` は、任意 decimal magnitude の syntactically valid `Content-Length` または一つの supported `Transfer-Encoding: chunked` value を metadata として持てる。exact final header は discoverable のまま。`204` は method にかかわらず両 field を許さない。malformed/conflicting length、unsupported transfer coding、`Content-Length` と `Transfer-Encoding` の同時存在は suppression より前に拒否する。 |
| Transfer-Encoding | すべての field value を wire order で結合し、optional SP/HTAB 付き comma で分割する。supported sequence は、parameter を持たない case-insensitive `chunked` coding が exactly one の場合だけ。empty element、repeated `chunked`、別 coding、malformed token syntax は `Error.Invalid`。supported transfer coding と任意の `Content-Length` の同時存在も `Error.Invalid`。 |
| Content-Length / close-delimited | 既存の decimal、equal-duplicate、fixed-cap、truncation、close-delimited behavior を保つ。payload-bearing response では `Content-Length` を close-delimited より先に選び、supported chunked field をその両方より先に選ぶ。read-to-close response は引き続き reuse 不可。 |
| Chunk-size line | `HTTP_MAX_CHUNK_LINE = 8,192` wire bytes は terminating CRLF を含む。line は一つ以上の ASCII hexadecimal digit、zero or more RFC 9112 chunk extension、exact CRLF。extension name は RFC `token`、optional value は `token` または valid quoted-pair escaping を持つ quoted-string。bare LF、invalid grammar、guard 内での CRLF 欠落、guard を越える byte は `Error.Invalid`。excess byte に依存する syntax/magnitude state より guard excess が優先し、syntactically valid magnitude が固定 decoded-body ceiling を越えれば target conversion なしで `Error.Invalid`。 |
| Cumulative chunk framing | `HTTP_MAX_CHUNK_FRAMING = 262,144` wire bytes は、すべての chunk-size line とその CRLF、各 nonzero payload 後の CRLF を、zero-chunk line を含めて数える。payload と trailer はそれぞれ別の limit を持つため除外する。co-read 済み byte は次の read より前に charge する。allowance exactly で終わる terminal zero line は受理し、allowance 越しの byte を必要とすれば over-guard probe 無しで `Error.Invalid`。decoded length と独立した bound なので many-tiny-chunk/extension work を制限する。 |
| Chunk payload | nonzero size は target conversion、reserve、次の payload read より前に cumulative decoded length と `HTTP_MAX_BODY` に照らして検査する。exactly size bytes の payload と exact CRLF が必要。zero は trailer を選ぶ。size line、payload、payload CRLF、zero chunk、trailer の truncation は `Error.Invalid` で、partial response は成功しない。zero は terminal なので empty data chunk は存在しない。 |
| Trailers | `HTTP_MAX_TRAILER_BLOCK = HTTP_MAX_HEADER_BLOCK = 262,144` wire bytes は zero-chunk line 後の最初の byte から terminating empty CRLF までを数える。trailer line は exact CRLF、直後に `:` を持つ RFC-token field name、HTAB/SP/visible ASCII/obs-text だけの value を要求する。`Content-Length` と `Transfer-Encoding` trailer field は invalid。trailer count は final head の `HTTP_MAX_HEADERS` 未使用分を消費する。trailer は incrementally validate するが retain、merge、expose しない。guard exactly で終わる valid terminator は受理し、guard 越しに認識されるものは over-guard probe 無しで invalid。 |
| Errors and precedence | request validation が network work より先。各 available head では exact wire syntax、`Content-Length` normalization/conflict、transfer-coding syntax/conflict が method/status body selection より先。per-line/cumulative framing/trailer guard は excess byte を必要とする grammar state より先、complete in-guard grammar は decoded-size check より先、decoded-size excess は payload/trailer read より先。malformed/truncated framing は `Error.Invalid`、armed I/O deadline は `Error.Timeout`、他の transport/TLS failure は既存 taxonomy のまま。全 failure で output slot は null のまま。 |
| Connection fate | bodyless、Content-Length、terminal-chunk-plus-valid-trailers response は self-delimited。final head が keep-alive eligible、Align scratch に residual byte が無い、TLS connection が pending application byte (`SSL_pending`) も buffered record/plaintext work (`SSL_has_pending`) も報告しない、のすべてを満たす場合だけ pool する。chunk metadata/trailer を完全に消費してから pool する。read-to-close、`Connection: close`、`101`、malformed/truncated framing、fixed-cap excess、任意 scratch residual、いずれかの TLS-pending condition は pool せず close する。reused idle connection の zero-response-byte failure は既存の一度の fresh retry を保ち、response byte が一つでもあれば retry しない。 |
| Plaintext / TLS | 一つの parser/state machine が既存 `Conn` abstraction を両 transport で消費する。同じ read limit、error、cleanup、pooling rule、timeout behavior を適用する。TLS は plaintext-only staging や別 framing path を増やさない。 |
| Allocation / performance | header/chunk discovery は一つの `HTTP_CLIENT_READ_CHUNK = 32,768`-byte reusable scratch allocation を使う。`HTTP_MAX_RESPONSE_LOGICAL = HTTP_MAX_HEADER_BLOCK + HTTP_MAX_BODY = 1,074,003,968` とする。response accumulator は `Vec` の allocator-dependent reserve heuristic ではなく exact-capacity boxed byte allocation を使う。checked geometric target はそれぞれ `HTTP_MAX_RESPONSE_LOGICAL` 以下で、final allocation は allocation-preserving に response `Vec` へ変換する。stored Rust capacity + scratch の steady ceiling は `HTTP_MAX_RESPONSE_LOGICAL + scratch = 1,074,036,736` bytes。growth 中は old capacity が new capacity より strictly small なので、old + new + scratch は `2 * HTTP_MAX_RESPONSE_LOGICAL + scratch = 2,148,040,704` bytes 未満。この transient ceiling は simultaneously live な両 Rust allocation layout を数えるが、layout 外の allocator bookkeeping/allocator-internal transition space は除外する。accumulator は validated final-head/decoded-payload progress からだけ grow し、peer-declared size だけでは grow しない。parser state は constant-size で、chunk table、trailer table、separately accumulated second body、per-chunk allocation を禁止する。count 済み old/new pair はこの single accumulator を geometric relocation する間だけ存在する。allocation failure は `Error.Invalid` ではなく既存の fatal OOM behavior。 |
| Consumer acceptance | provider fixture が二つの SSE chunk と terminal zero chunk を返し、OpenAI-compatible/llama.cpp adapter は concatenated payload だけを受け取る。direct fixture が status/header/body preservation、全 malformed/truncated case、bodyless/informational framing、plaintext/TLS parity、sequential reuse、close-delimited non-reuse を証明する。 |

上記 `Content-Length` の arbitrary-magnitude allowance は `HEAD` と `304` の metadata 専用である。
leading zero を除いて digit sequence を比較し decimal magnitude を normalize する。`usize` へ変換せず、
`HTTP_MAX_BODY` と比較せず、それから reserve せず、body を読まない。duplicate value は normalized
magnitude で等しい(`003` は `3` と等しい)。payload-bearing length は explicit Request 5 bound
が無ければ既存の target-representable/global-cap requirement を保ち、explicit bound では
Request 5 の wider arbitrary-magnitude/limit-specific comparison を使う。

### Streaming decoder and retained layout

parser は discovery 中の byte と返す handle が所有する byte を分離する。

```text
fixed read scratch -> interim head (validate, discard)
                   -> final head (copy exact bytes once, keep header spans)
                   -> chunk line / CRLF / trailers (validate, discard)
                   -> decoded chunk payload (append or read directly into response buffer)

response.buf = exact final head || decoded payload
response.body_start = final head length
response.body_len = cumulative decoded payload length
```

`HttpHead` は header scan で chunked を拒否する代わりに explicit framing classification を持つ。
transport loop が request-method/status selection、interim advancement、byte guard、reuse を所有する。
complete-buffer `http.parse` path は ordinary method semantics で同じ state machine を呼ぶ。従来通り、
最初の complete self-delimited response 後の byte は body に加わらない。新しい retained layout は
unreachable tail capacity として持たず discard する。final-head byte を rewrite せず、trailer byte を
`HttpHeaderSpans` に入れないため、既存 header lookup は byte-exact のまま。

固定 scratch read は head、size line、trailer line を完成させる boundary より後の byte を co-read してよい。
その byte は一つの 32 KiB scratch allowance 内に留まり次 state へ渡る。failure を認識した後は retained
body storage へ copy せず、後続 transport read を行わない。zero-chunk 後の全 read は残り trailer
allowance に clamp する。chunk-framing/post-zero-chunk の co-read 済み byte はすべて次の read より前に
cumulative allowance へ charge する。read は current line、cumulative framing、trailer の残量のうち
最小値へ clamp する。chunk line、cumulative chunk framing、trailer block の one-byte over-guard probe は無い。

### Framing and reuse matrix

| Method / final status | Valid metadata | Selected body | Reusable after exact completion |
|---|---|---|---|
| exact `HEAD`、`204` 以外の final status | framing field 無し、任意 valid CL、supported HTTP/1.1 chunked TE | zero bytes、body framing を消費しない | final head が keep-alive で scratch/TLS に residual/pending byte が無いとき yes |
| any method、`204` | CL/TE とも無し | zero bytes | 同上 |
| ordinary method、`304` | framing field 無し、任意 valid CL、supported HTTP/1.1 chunked TE | zero bytes、body framing を消費しない | 同上 |
| ordinary method、other final status + supported HTTP/1.1 chunked TE | TE only | valid trailer まで decoded chunk | terminal chunk/trailer 完了かつ scratch/TLS に residual/pending byte 無しで yes |
| ordinary method、other final status + CL | CL only | exactly CL bytes | 既存 rule |
| ordinary method、other final status + neither | none | EOF まで read | never |
| any、interim non-`101` | CL/TE とも無し | zero、次 head へ advance | completion ではない |
| any、`101` または invalid/conflicting framing | any | none、fail | never |

### Implementation closure matrix

この matrix が implementation PR の authoritative record である。implementation はこの reviewed strategy
を reopen せずに従う。public row、allocation strategy、error/reuse precedence を変えるなら matrix を先に
reopen し、fresh design review を要求する。

| Closure axis | Required implementation evidence | Owner |
|---|---|---|
| Header formation and validation | `HttpHead` が exact version/status/header span と normalized CL/TE fact を記録する。parameterized table が exact CRLF、admitted version 両方、malformed/unknown version、three-digit status grammar/range、reason byte、immediate colon を持つ token name、value/obs-fold、CL duplicate/leading zero、TE token list、HTTP/1.0 chunked rejection、CL+TE、per-head header count、bodyless forbidden/permitted metadata を覆う。 | `align_runtime` HTTP parser unit |
| Request validation | exact `CONNECT` が pool lookup/connect/write より先に fail し、case variant は fixture に到達する。exact `HEAD` だけが suppression を選ぶ。 | runtime side-effect counter fixture + driver E2E |
| Interim construction/discard | same-read/split-read の `100`、`102`、`103`、`199` が co-read final byte を保つ。framing field、`101`、cumulative head byte excess、malformed/truncated interim head は final handle 無しで fail する。同時に live な header-span table は最大一つ。 | runtime parameterized state-machine owner |
| Chunk move-in / compaction | same-read、every-boundary split、many-tiny-chunk、extension、uppercase/lowercase hex、embedded NUL payload、exact decoded cap、exact cumulative-framing cap case が一つの retained final head + byte-exact decoded body を作る。 | runtime decoder matrix + allocation instrumentation |
| Chunk malformed / early exits | empty/invalid/overlong size line、cumulative framing cap + 1、over-global magnitude、truncated payload、missing payload CRLF、missing zero chunk、全 state の EOF/error/timeout は response と later read 無し。 | runtime fault-injection matrix |
| Trailer validation / discard | empty trailer、exact guard、guard+1、continuous unterminated input、invalid name/value、forbidden framing field、shared header-count boundary、final header と同名、every-boundary split が trailer byte/offset を保持せず header lookup を変えない。 | runtime trailer matrix + structural instrumentation |
| Bodyless / final selection | `HEAD`、`204`、`304` が absent/CL/chunked metadata の Cartesian row(global cap 超の値を含む)を覆い、terminator/trailer read を行わない。lowercase method は payload-bearing control。 | runtime framing matrix + plaintext driver fixture |
| Response move-out / Drop | success は exactly one response を box し、全 failure は null のまま scratch/accumulator を free する。`get_many` が chunked/ordinary response を混在させても既存 response-array success/error Drop path が exact。 | live-response/allocation counter + `get_many` owner |
| Pool and replacement | chunked/bodyless/CL の first response は complete self-delimitation 後だけ一つの plaintext/TLS connection を second response に reuse する。close、scratch residual、`SSL_pending`、`SSL_has_pending`、malformed、cap、`101`、read-to-close は close。stale zero-byte retry は一度の fresh attempt のまま。TLS co-read fixture は next-response byte を Align scratch と OpenSSL pending storage に別々に置く。 | runtime pool matrix、plaintext + verified-TLS |
| Whole / per-call consumers | `get`、`post`、`request`、`get_many`、`http.parse` が method knowledge に応じた framing behavior を共有する。package/compiler/ABI surface は変えない。 | runtime unit + `crates/align_driver/tests/http_chunked_client.rs` |
| Plaintext / TLS parity | identical byte partition/failure が `Conn::Plain`/`Conn::Tls` を通る。timeout mapping/teardown だけが `Conn` より下で transport-specific。 | plaintext vector と対にした verified-TLS runtime fixture |
| Resource and allocation parity | instrumented accumulator allocation-layout capacity は ledger の exact steady/transient numeric ceiling（growth 中の old allocation を含む）と one 32 KiB scratch を守り、allocation-preserving conversion 後の final response capacity も同じ bound 内。exact cumulative-framing cap と cap + 1 が bounded metadata work を証明し、chunk/trailer structural state は constant-size。unvalidated CL/chunk magnitude を growth target に使わない。 | runtime capacity/high-water instrumentation |

benchmark は correctness gate ではない。この capability は新しい throughput claim をせず accepted framing を
変える。既存 R1/R4 requirement は保つため、owner は allocation/copy count を記録し、per-chunk allocation
または second body buffer を加える regression を拒否する。

### Delivery and adoption

implementation は一つの runtime/owner-test capability PR。compiler/package/ABI layer は変更しない。
hand-written diff が約 1,000 行を超えるのは、strict head parser、incremental chunk/trailer decoder、
exact-capacity accumulator、plaintext/TLS reuse verdict と単一 owner matrix が一つの producer-to-consumer
failure domain をなすためである。分割すれば dormant parser を残すか、framing/cleanup/allocation proof を
一時的 boundary 間で重複させる。実装は historical item-7 client-asymmetry test を outright update する。
merge 後、align-llm は次の許可された consumer-prerequisite wave で Align を rebuild/pin し、provider
stream fixture を Content-Length から chunked へ切り替え、valid SSE、malformed/truncated rejection、final
status/header/body preservation を証明する。sibling request register が adoption evidence の lifecycle owner
であり続ける。

## Client response body の上限制御 (align-llm Request 5 — SHIPPED 2026-08-15)

provider call には whole-body client が allocation した後の length check ではなく、受信中に効く
operation-sized limit が必要である。Request 5 は Request 4 framing engine に client default と request
override を一つずつ加える。streaming-input handle、第二 decoder、provider policy、ambient config、package
API は加えない。

### 公開契約台帳

| Surface / state | 厳密な契約 |
|---|---|
| Public methods | `cl.max_response_body_bytes(limit: i64) -> ()` は bound `http.client`、`r.max_response_body_bytes(limit: i64) -> ()` は bound `http.request` を mutate する。既存の timeout/header/body 設定メソッドと同じく、どちらも I/O を行わない Pure setter で、Move receiver を consume せず borrow する。request を consume するのは `cl.request(r)` だけ。 |
| Input and defaults | `limit == 0` は stored value を clear する。clear client は fixed `HTTP_MAX_BODY = 1,073,741,824`、clear request は client を inherit する。positive は `1..=HTTP_MAX_BODY` かつ target-`usize` representable。negative/larger/unrepresentable/null handle は previous value と network work より前に abort。ambient input は無い。 |
| Selection | `get`/`post` は client value、`request` は `min(positive client or HTTP_MAX_BODY, positive request or HTTP_MAX_BODY)`。どちらかが positive なら positive `HTTP_MAX_BODY` 自体も *explicit*、zero/unset は non-explicit。`get_many` は worker start 前に client value を一度 snapshot。source borrow が concurrent setter を除外し、native atomic storage が race-free snapshot を保つ。 |
| Success and ownership | exact-limit payload は既存 Move `response` を返し、`status()`/`header()`/`body()` は region-bound zero-copy view のまま。explicitly bounded response は fixed header/body allocation を別々に Drop まで所有するが opaque handle ABI/source ownership は不変。setter は input view を保持しない。 |
| Limit result | explicit bound を超える payload を最初に認識した時点で stable `Error.Code(-1)` を返す。explicit bound が無い場合の target/global excess は従来どおり `Error.Invalid` である。`-1` は `std.http` receive-body limit 専用で、HTTP status `100..=599` と common errno mapping が公開する non-negative raw OS code の外にある。`Error.Timeout`、transport `Error.Code(errno)`、successful HTTP 413 とも異なり、partial response/body は返さない。 |
| Native status mapping | runtime は HTTP-private `AL_HTTP_BODY_LIMIT = -1` を使う。client-response Result lowerer だけが positive な common category/errno mapping 前にこれを `Error.Code(-1)` へ写す。この sentinel は saturating `AL_CODE + errno` と衝突せず、generic errno decoder には入らず、`Error` variant/layout を増やさない。 |
| Content-Length | 各 field を arbitrary-precision normalized decimal magnitude として parse。syntax、equal duplicate normalization、conflicting duplicate、CL/TE conflict が cap より先。payload-bearing final response の valid magnitude が explicit cap 超なら `usize`/`HTTP_MAX_BODY` 超でも `Error.Code(-1)`。explicit でなければ target/global excess は `Error.Invalid`。peer value から reserve せず、excess 後に payload read しない。 |
| Method/status composition | Request 4 の exact-uppercase `HEAD`/`CONNECT`、interim、`204`、`304` rule を維持。bodyless final は metadata を validate するが cap compare、body allocate、payload/chunk/trailer read をしない。cap は returned final payload だけ。 |
| Chunked payload | line/framing/trailer guard と grammar は不変。guard と complete-line syntax が cap より先。valid chunk magnitude で cumulative decoded bytes が explicit cap を超えるなら target conversion/payload allocation/next read 前に `Error.Code(-1)`。non-explicit global excess は `Error.Invalid`。terminal chunk 前の limit 後は trailer を読まない。 |
| Close-delimited payload | co-read payload を先に consume。以後は remaining selected bytes + one scratch-only probe byte 以下だけ read。最初の excess byte は `Error.Code(-1)` で retained payload に入らず later read も無い。success でも non-reusable。 |
| Read and error precedence | setter/request validation、complete head/framing syntax/conflict、body selection、fixed guard、complete in-guard grammar、cap comparison、payload allocation/read の順。recognizable excess がまだ無い read の timeout/transport error は既存 outcome。truncation/malformed framing は `Error.Invalid`。 |
| Co-read exception | one fixed `HTTP_CLIENT_READ_CHUNK = 32,768` scratch read が head/chunk-line boundary 後の byte を含み得る。excess 判定後は scratch-only で retained storage/later read に入らない。他の probe/staging allowance は無い。 |
| Allocation | explicit exchange は fixed `HTTP_MAX_HEADER_BLOCK` final-head allocation と fixed scratch を使い、payload-bearing final head を選んだ後だけ exact `selected cap` body allocation を加える。どちらも grow/compact せず peer CL/chunk magnitude から size を決めず、success へ allocation-preserving move。peak Align-owned response bytes は `selected cap + HTTP_MAX_HEADER_BLOCK + HTTP_CLIENT_READ_CHUNK` 以下。262,144 なら exact 557,056 bytes。bodyless は body region を allocate しない。unconfigured は Request 4 one-buffer geometric accumulator/ceiling を維持。allocation failure は fatal no-unwind OOM。configured bound に限り R1 の internal one-buffer rule を二領域へ amend するが public zero-copy requirement は維持。 |
| Structural metadata | live な interim/final header table は最大一つ、survive する offset は final header だけ。trailer は bytes/offset 無しで final header-count remainder を消費。decoder state は constant-size。body length、peer magnitude、chunk count、trailer volume 由来の capacity は無い。 |
| Cleanup and reuse | `Error.Code(-1)` では output null、accumulator/scratch を free、partial plaintext/TLS connection を close して pool/retry しない。client は fresh connection で再利用可能。exact bounded CL/chunked/bodyless success は Request 4 reuse verdict、close-delimited success は non-reusable。 |
| Batch | `get_many` は全 worker を完了し input ordinal で store、completion order と無関係に lowest-index error を返す。error 時 response array は無く successful sibling を free。limit も malformed/timeout/transport と同じ ordinal rule。各 worker は独立 allowance を所有。 |
| Plaintext / TLS | 一つの selection/decoder state が `Conn::Plain`/`Conn::Tls` を扱う。Align-owned TLS application scratch は ceiling 内。kernel/libssl opaque buffer は除外するが peer length/chunk magnitude/selected cap/body から capacity を導出しない。 |
| Compiler/runtime owner | Sema は receiver/method/arity/type/bound-local、checked HIR は receiver/`i64` envelope、MIR は setter rvalue と HTTP-private limit mapping、LLVM は setter declaration/call と HTTP result mapping、runtime は storage/snapshot/selection/decoder/allocation/cleanup/batch order を所有。package owner は無い。 |
| ABI and identity | A66 に `void @align_rt_http_max_response_body_bytes(ptr, i64)` と `void @align_rt_http_client_max_response_body_bytes(ptr, i64)` を追加。他の signature/type tag/interface record/handle ABI は不変。method spelling/MIR discriminant は interface-format-6/cache identity に入り、exact edit/revert で元 hash に戻る。 |
| Prerequisite and adoption | Request 4 `f04672bce6f8689c9b219d0a20e770571e2d638b` が framing を供給。Align implementation 後、sibling が merge を pin、`provider_http` に 262,144 を設定し combined framing/cap/cleanup matrix と HTTP status 区別を証明する。 |

### Framing と limit のマトリクス

| 最終 body 選択 | Explicit bound | 超過結果 / connection |
|---|---:|---|
| exact `HEAD`、`204`、`304` bodyless | either | compare 無し。empty body + normal bodyless reuse verdict |
| Content-Length / chunked / close-delimited payload | no | fixed global excess は `Error.Invalid`; close |
| Content-Length payload | yes | normalized magnitude above cap は reserve/read 前に `Error.Code(-1)`; close |
| chunked payload | yes | valid guarded size line 後の cumulative decoded excess は `Error.Code(-1)`; response/trailer read 無し; close |
| close-delimited payload | yes | co-read 後 remaining-plus-one scratch probe; excess は `Error.Code(-1)`; close |
| malformed/conflicting/truncated framing | either | `Error.Invalid`; close |
| recognizable excess 前の deadline/transport failure | either | existing `Error.Timeout` / `Error.Code(errno)`; close |

### 実装 closure matrix

public setter だけでは dormant、decoder-only cap は unnameable なので一つの cross-layer capability とする。
Request 4 state machine/owner を再利用し expected hand-written change は概ね 1,000 lines 未満。

| Closure axis | 必須 evidence | Owner |
|---|---|---|
| Formation and setters | receiver 両方、exact `i64`、arity、bound-local、zero clear/inherit、positive endpoint、invalid abort-before-store、whole/per-unit spelling、interface/cache edit-revert。 | Sema/checked-HIR/MIR owner + abort fixture |
| Selection and concurrency | get/post inheritance、request min-selection、explicit `HTTP_MAX_BODY` vs zero、one `get_many` snapshot。 | runtime table + driver dispatch/batch |
| Fixed/chunked/close receive | CL normalization/precedence、exact/cap+1、oversized valid magnitude、guard/malformed/truncated、tiny chunks、co-read/probe、no post-limit read/trailer。 | runtime parser/decoder/transport matrix |
| Bodyless/interim | HEAD/204/304/interim は valid metadata を cap compare/body allocate せず、malformed/101/case-variant control を維持。 | method/status cap twins |
| Move-out and cleanup | success は header/body response allocation transfer、failure は null + 各 live allocation/scratch の exact free、later client use clean、ordinary `?`/`else`/`match`/`map_err` ownership。 | allocation/response/fd counter + driver consumer |
| Batch precedence | lowest-index malformed-vs-limit completion twins、exact success、failure no array、sibling Drop、failed connection teardown。 | `get_many` owner |
| ABI / lowering | two A66 setters、whole/per-unit、HTTP sentinel → `Error.Code(-1)`、他 status 不変、malformed checked HIR は LLVM 前に reject。 | MIR/LLVM/ABI/checked-HIR owner |
| Resource ceiling | 262,144 で CL/close/tiny-chunk/trailer/plaintext/TLS の 557,056 maximum、peer-derived capacity 無し、fixed structural state。 | runtime high-water/failpoint instrumentation |
| Consumer discriminants | limit `Code(-1)`、HTTP 413 は status data、malformed Invalid、deadline Timeout、OS fault non-negative `Code(errno)`。 | driver match + align-llm adoption |

benchmark は不要。この contract は throughput ではなく resource ceiling を claim し、runtime instrumentation
が直接測る。code/allocation/public method/framing precedence/layer ownership の変更は implementation 前に ledger
を reopen する。

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
- item 10 — `ctx.headers()`: パラメータ経由のラッパーがコンパイルでき、かつ E2E でテーブルを読める。
  **ローカル**のハンドルから作ったビューは、return / `break` / struct への包み込み / ctx を**消費する**
  serve の反復をまたぐ保持、のいずれでも拒否される(pkg.web の形)。ctx をただ **drop** するだけの反復を
  またぐ保持も同様に拒否される(`a_view_of_a_handle_dropped_at_the_end_of_an_iteration_is_rejected` —
  2026-07-22 に修正して反転した既知の穴)。`ctx.respond(rb)` の後の
  `hs.get()` は**裸のローカル**で拒否される(囲む struct の `str` フィールドが穴を覆い隠す)。
  `ctx.respond_stream(rb)` の後の `hs.get()` はコンパイルでき、かつ動く。大文字小文字を無視したヒット +
  ミスの `pkg.web` 経由 E2E。`Option`/`Result` のペイロードとして、また配列要素としてのビューは拒否される。
  これを運ぶ struct は Copy のまま(drop は出ない)で、`sema_and_codegen_struct_layout_agree` に行がある。
  (`crates/align_driver/tests/http_headers_view.rs` +
  `apps_web_root.rs::web_header_reads_the_request_header_table`)
