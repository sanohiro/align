このディレクトリには、ロードマップの本文を超えた std モジュールの Opus 実装可能な設計仕様が置かれている。
メインループ (Fable) が執筆したもので、各モジュールを実装する際の source of truth である。

# std.http — implementation design (M11)

> 🌐 [English](../http.md) · **日本語**

## Overview

HTTP/1.1 のプリミティブであり、フレームワークではない(draft §18.2)。std.net のソケットの上に構築され
る。メンバー: request、response、header、method、status、client、server プリミティブ。コネクション再
利用は net のレールに従う。**TLS は隠れた依存関係である**: HTTPS には FFI 経由の TLS エンジン
(BoringSSL/rustls-ffi クラス。compress/crypto の「エンジンを借用する」方式と同様)が必要である。v1 は
**平文の HTTP/1.1 のみ**とし、HTTPS は TLS の FFI ラッパーが実装されるまで M11 内で先送りする(中途半端
な形で出荷せず、記録するにとどめる)。HTTP/3、ルーティング、ミドルウェアは std ではなく pkg である。

## Signatures

v1 案。Fable が確定させた形:

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

- `client`、`request`、`http_server`、`http_request_ctx` は **Move 型** である(プールされたコネクショ
  ン、ヘッダーリスト、ボディバッファ、accept 済みのソケットを所有する)。reader/writer の Move の前例に
  加え、これらが包む net の Move 型が根拠である。
- `response` は自身のヘッダーブロックとボディバッファを所有する(Move)。`resp.header()`/`resp.body()`
  は **resp にリージョン束縛されたビュー**を返す(#297 を意識した `region_of` の分岐 — net の借用された
  reader/writer や `json.decode` と同様)。
- 各自のコンストラクタが返す Result の Ok 位置を除き、`scalar_arg` のチョークポイントで Move が拒否さ
  れる(net のテンプレート通り)。

## Effect classification

すべて impure である(net 経由のネットワーク syscall)。

## Error policy

トランスポート層のエラーは std.net から伝播する(errno→Error テーブル)。HTTP レベルのエラー(不正な
レスポンス、不正なステータス行)→ `Error.Invalid`。4xx/5xx のステータスはエラーでは**ない** — それはそ
のステータスを持つ有効なレスポンスである(呼び出し側が `resp.status()` で分岐する)。`Err` になるのはトラ
ンスポート/パースの失敗のみである。(これは意図的な One-way の判断である: HTTP のステータスはデータで
あり、Result のエラーではない。)

## New machinery required

上記の Move 型 + net のソケット上での HTTP/1.1 のパース/シリアライズ + コネクションプールの再利用。
新しい I/O パスは不要(net の reader/writer)。TLS ラッパーは先送り(HTTPS をブロックする)。ヘッダーの
パースは v1 ではスカラー処理である(simdjson 流の構造的スキャンは後日の最適化であり、記録のみ)。

## Slice breakdown

1. request/response の構造体 + ヘッダーリスト + HTTP/1.1 のシリアライズ/パース(ソケットはまだ不要 —
   純粋なエンコード/デコードとして単独でテスト可能)。
2. client + 1 つの net の `tcp_conn` 上での get/post(平文)。
3. コネクションプールの再利用(レール — keepalive、デフォルトで再利用)。
4. server プリミティブ(serve/accept、呼び出し側がレスポンスを書く)。
5. [TLS 実装後に先送り] FFI の TLS ラッパー経由の HTTPS。

## Pitfalls

- **P1 (TLS defer honesty)**: v1 は平文のみである。`https://` の URL を黙って受理して平文で送信しては
  **ならない** — TLS のスライスが実装されるまでは、`https://` を明確な「HTTPS not supported in v1
  (TLS wrapper pending)」という `Error.Invalid` で拒否すること。黙ってダウングレードするのはセキュリ
  ティ上の落とし穴である(Nothing-hidden 違反)。
- **P2 (status-is-data)**: 4xx/5xx を `Err` に写像しては**ならない** — トランスポート/パースの失敗のみ
  が `Err` になる。`get()` が 404 を返す場合は `Ok(status 404 のレスポンス)` である。ここを誤ると呼び出
  し側が二重のエラーハンドリングを強いられる厄介な設計になる。
- **P3 (response view region, #297)**: `resp.header()`/`body()` は resp へのビューであり、`region_of`
  は Static ではなく `region_of(resp)` でなければならない。resp の Drop を越えた escape は拒否される。
- **P4 (Move sweep + bound-receiver)**: client/request/server/ctx は Move である — 全パスの Gate-1
  スイープ + bound-receiver のゲート(#337/#338)が必要。v1 では束縛されていない一時値をレシーバには
  できない。
- **P5 (connection pool Drop)**: client はプールされた conn を所有する。Drop はすべてを close する。
  プールの入れ替わりを通じて fd がリークしないこと。
- **P6 (request smuggling / header injection)**: ヘッダー名・値中の CR/LF は拒否する(header injection
  → request smuggling)。`r.header()` の呼び出し時に検証すること。

## Test checklist

- リクエストをシリアライズする → 正確なバイト列になる
- 既知のレスポンスをパースする → status/headers/body が正しい
- ローカルの平文サーバに対する `get()` → 200 の往復
- 404 → `Err` ではなく `Ok(status 404)`(P2)
- `https://` → `Error.Invalid`(P1)
- ヘッダー中の CRLF → 拒否される(P6)
- resp を越えて escape するレスポンスボディのビュー → コンパイルエラー(P3)
- プールが 2 回の get にまたがって conn を再利用する
- Move の拒否 + 束縛されていないレシーバの拒否
- import が必須であること
