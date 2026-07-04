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

## New machinery required

上記の Move 型 + net のソケット上での HTTP/1.1 のパース/シリアライズ + コネクションプールの再利用。新しい
I/O パスは要らない(net の reader/writer を使う)。TLS ラッパーは先送り(HTTPS を塞ぐ)。ヘッダーのパース
は v1 ではスカラー処理とする(simdjson 流の構造的スキャンは後日の最適化であり、記録のみ)。

## Slice breakdown

1. request/response の構造体 + ヘッダーリスト + HTTP/1.1 のシリアライズ/パース(ソケットはまだ不要 —
   純粋なエンコード/デコードとして単独でテストできる)。
2. client + 1 つの net の `tcp_conn` 上での get/post(平文)。
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
