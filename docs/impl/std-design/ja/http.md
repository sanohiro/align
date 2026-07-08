このディレクトリには、ロードマップの本文だけでは足りない std モジュールについて、Opus がそのまま実装に
移せる粒度の設計仕様を収めている。執筆はメインループ (Fable) が担当しており、各モジュールを実装する際は
これが source of truth となる。

# std.http — implementation design (M11)

> 🌐 [English](../http.md) · **日本語**

## Overview

HTTP/1.1 のプリミティブであって、フレームワークではない(draft §18.2)。std.net のソケットの上に構築す
る。メンバーは request、response、header、method、status、client、server プリミティブ。コネクション再利用
は net のレールに従う。**隠れた依存は TLS である**: HTTPS には FFI 経由の TLS エンジン(BoringSSL/
rustls-ffi クラス。compress/crypto の「エンジンを借用する」方式と同じ)が要る。v1 は **平文の HTTP/1.1
のみ**とし、HTTPS は TLS の FFI ラッパーが入るまで M11 内で先送りする(中途半端に出荷せず、記録にとどめ
る)。HTTP/3、ルーティング、ミドルウェアは std ではなく pkg である。

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
// Server primitive (not a framework)
srv := http.serve(host: str, port: i64) -> Result<http_server, Error>
srv.accept() -> Result<http_request_ctx, Error>   // one request; caller writes the response
// Batched client (the rail — moved here from net; see Concurrency in net.md)
cl.get_many(urls: slice<str>, max_concurrency: i64) -> Result<array<response>, Error>
```

## Type & ownership classification

- `client`、`request`、`http_server`、`http_request_ctx` は **Move 型** である(プールしたコネクション、
  ヘッダーリスト、ボディバッファ、accept 済みソケットを所有する)。根拠は reader/writer の Move の前例に
  加えて、これらが包む net の Move 型である。
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
  「速く仕上がった」とは言わない。

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
3. コネクションプールの再利用(レール — keepalive、デフォルトで再利用)。
4. server プリミティブ(serve/accept、レスポンスは呼び出し側が書く)。
5. [TLS 実装後に先送り] FFI の TLS ラッパー経由の HTTPS。

## Pitfalls

- **P1 (TLS 先送りの誠実さ)**: v1 は平文のみである。`https://` の URL を黙って受理して平文で送っては
  **ならない** — TLS のスライスが入るまでは、`https://` を「HTTPS not supported in v1 (TLS wrapper
  pending)」という明確な `Error.Invalid` で拒否する。黙ってダウングレードするのはセキュリティ上の落とし穴
  である(Nothing-hidden 違反)。
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
- `https://` → `Error.Invalid`(P1)
- ヘッダー中の CRLF → 拒否(P6)
- resp を越えて escape するレスポンスボディのビュー → コンパイルエラー(P3)
- プールが 2 回の get にまたがって conn を再利用する
- Move の拒否 + 束縛していないレシーバの拒否
- import が必須であること
- `bench/http_client` の数値を Rust ベースラインに照らして記録する(R6 — 完了はベンチマークでゲートする)
