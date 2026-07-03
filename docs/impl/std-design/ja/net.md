このディレクトリには、ロードマップの本文を超えた std モジュールの Opus 実装可能な設計仕様が置かれている。
メインループ (Fable) が執筆したもので、各モジュールを実装する際の source of truth である。

# std.net — implementation design (M11)

> 🌐 [English](../net.md) · **日本語**

## Overview

低レベルソケット: tcp、udp、dns、socket。syscall に裏打ちされている。要となる再利用ポイント: 接続済み
ソケットの fd は、**既存の M9 reader/writer** にそのまま差し込める — 多態性は construction 側(fd を
所有するハンドルを返す net 側のコンストラクタ)にあり、read/write/Drop で fd を close する仕組みは
まったく同一である(draft §18.2 の io の原則。reader/writer が fd に対して汎用的であることによって実現
されている)。したがって net が追加するのはソケットのライフサイクルと DNS であり、新しい I/O パスでは
ない。

## Signatures

v1 案 — draft §18.2 はメンバー名のみ列挙している。以下は Fable が確定させた形である:

```text
// TCP client
tcp.connect(host: str, port: i64) -> Result<tcp_conn, Error>   // DNS + connect; keepalive ON by default
c.reader() -> reader          // borrow an M9 reader over the socket fd
c.writer() -> writer          // borrow an M9 writer over the socket fd
// TCP server
tcp.listen(host: str, port: i64) -> Result<tcp_listener, Error> // bind+listen; SO_REUSEADDR
l.accept() -> Result<tcp_conn, Error>
// UDP
udp.bind(host: str, port: i64) -> Result<udp_socket, Error>
u.send_to(data: bytes, host: str, port: i64) -> Result<i64, Error>
u.recv_from(buf: mut buffer) -> Result<datagram, Error>   // fills caller buffer, returns {n, peer}
// DNS
dns.resolve(host: str) -> Result<array<string>, Error>    // owned IP strings
```

## Type & ownership classification

- `tcp_conn`、`tcp_listener`、`udp_socket` は **Move 型** である(新設の `Ty::TcpConn`/`Ty::TcpListener`/
  `Ty::UdpSocket`)。それぞれ 1 つの fd を所有し、Drop = close(fd) であり、まさに reader/writer/buffer の
  Move の前例通りである。array/slice/vec/box の要素として、また Option/Result のペイロードとして
  `scalar_arg` のチョークポイントで拒否される。ただし各自のコンストラクタが返す Result の Ok ペイロード
  位置は例外である(connect/listen/accept/bind は `Result<T, Error>` を返す) — reader/writer が許可され
  たのと同様にこれらの Ok 位置は許可する(`Scalar::Buffer` #346 のテンプレート)。
- `c.reader()`/`c.writer()` は、conn の fd 上に構築された M9 の reader/writer を **借用** として返す
  (`owns_fd: false` — fd の所有・close は引き続き conn 側が行う)。したがって reader/writer のリージョ
  ンは conn `c` に束縛され、`c` の Drop を越えて使用することは拒否される
  (`region_of(TcpReader) = region_of(c)`)。これは #297 のトラップを意識した分岐である。
- `dns.resolve` は所有権付きの `array<string>` を返す(`read_dir` #339 と同様の deep-drop)。
  `datagram`/`response` は小さな構造体(Copy)であり、カウントと、必要に応じて所有権付きの
  peer/body を運ぶ。

## Effect classification

net の演算はすべて **impure**(syscall)である — 決して `par_map` のクロージャの中には入らない。

## Error policy

syscall の失敗は **共有の errno→Error テーブル**(M9)を経由する: ECONNREFUSED/ETIMEDOUT/
EHOSTUNREACH → `Error.Code(errno)`(v1 では専用のバリアントを設けない — 分岐が必要になる消費者が現れた
場合にのみテーブルを拡張する)、ENOENT 系の DNS 失敗 → resolve 専用の `Error.Invalid` または
`Error.Code`。部分的な read/write は(すでに正しく実装済みの)再利用された reader/writer が処理する。
ストリーム途中でのコネクションリセットは read/write の Error として現れる。

## Concurrency model

記録済みのレール(open-questions「Network std rails」): デフォルトでコネクション再利用(keepalive ON)。
net はバウンド付き並行処理バッチングのための **基盤** を提供する — `task_group` + `par_map` のブロッキ
ングプール(新しい async ランタイムではない。`io_uring` は後日の Linux バックエンドであって、意味論上の
モデルではない)。具体的なバッチ API(`get_many`、パイプライン化された write-then-read)は **1 層上の
`std.http`**(`cl.get_many`)に存在する — それは HTTP リクエスト/レスポンス型を操作するものであり、これ
は `std.http` が持つべきものなので、`std.net` には**置いてはならない**(net→http への依存はレイヤリング
違反/循環依存になる。http.md を参照)。net はバイトストリームに対して汎用であり続ける。1 つの静的な
ホストに対して接続ごとにループする実装は lint の対象である(post-v1 の lint、このモジュールでは記録の
みで実装はしない)。HTTP/3、TLS、ソケットのチューニング(TFO/REUSEPORT/thread-per-core)は pkg であり
std ではない。

## New machinery required

新設の Move `Ty` 3 種(TcpConn/TcpListener/UdpSocket)+ ランタイム構造体 + Drop(close);ソケットのライ
フサイクル用ランタイム関数(socket/connect/bind/listen/accept、`dns.resolve` 用の getaddrinfo、
sendto/recvfrom);バイトパスには M9 の reader/writer をそのまま再利用する(これが利点);借用された
reader/writer をその conn に束縛する `region_of` の分岐;`std.http` の `get_many` が土台とする
`task_group` + ブロッキングプールの基盤(バッチング自体は net ではなく http のもの)。新しい effect、
新しい I/O パス、async ランタイムは不要。

## Slice breakdown

1. `dns.resolve` 単体(getaddrinfo → 所有権付き `array<string>`) — 最小、Move 型なし、errno パスと
   deep-drop を検証する。
2. `tcp_conn` の Move 型 + `connect` + `reader()`/`writer()` 借用(reader/writer の再利用 — 中核となる
   証明) + Drop で fd を close + 全パスの Gate-1 スイープ。
3. `tcp_listener` + `listen` + `accept`(サーバ側)。
4. `udp_socket` + `bind` + `send_to` + `recv_from`。

(バッチ処理された `get_many` のレールは `std.http` で実装するのであってここではない — HTTP 型が必要
だからである。net が供給するのは `task_group` + ブロッキングプールの基盤のみで、これはすでに利用可能
である。)

## Pitfalls (implement carefully)

- **P1 (Move sweep ×3)**: 新設の 3 つの Move Ty は reader/writer と同様にすべてのパスを漏れなく通過し
  なければならない(`ty_is_move`/`tracks_region`/`null_moved_source`/drop/`MoveCheck`/`EscapeCheck`/
  `region_of`/finalize/MIR/codegen/print)。最もリスクが高い。見落とすと fd の二重 close またはリークに
  つながる。
- **P2 (borrowed reader/writer region, #297)**: `c.reader()`/`writer()` は conn の fd を借用する
  (`owns_fd:false`)。そのリージョンは Static ではなく必ず `region_of(c)` でなければならない — さもない
  と reader が conn の `close(fd)` より長生きし、use-after-close になる。明示的な `region_of` の分岐 +
  escape テストが必要。これは微妙な点である: reader 自体は Move 型だが、ここでは非所有の借用として振る
  舞うため、自身の Drop で fd を close してはならない(`owns_fd:false` はランタイム側ですでに処理済みだ
  が、リージョンの束縛は新規である)。
- **P3 (fd double-close)**: conn が fd を所有する。`reader()`/`writer()` の借用は `owns_fd:false` を
  設定し、conn の Drop のみが close するようにする。二重 close する経路がないことを検証する。
- **P4 (batching lives in http, not net)**: バッチ処理された `get_many` は HTTP のリクエスト/レスポン
  ス型を扱うため `std.http`(`cl.get_many`)に属し、`std.net` には**属さない** — ここに置くと net が
  http に依存することになる(レイヤリング違反/循環依存)。net が公開するのは基盤(task_group +
  `par_map` ブロッキングプール)のみである。http がこれを実装する際は: このプールを再利用する(リクエ
  ストごとにスレッドを起こさない)、`max_concurrency` で上限を設ける、1 件のリクエスト失敗はそのスロッ
  トが Err になるのであってバッチ全体の中断にはしない、ネストした `task_group` によるデッドロックを避
  ける(#301 の work-claiming の教訓)。
- **P5 (DNS owned strings deep-drop)**: `resolve` から得られる `array<string>` は各 IP 文字列を
  deep-free しなければならない(`read_dir` #339 のテンプレート)。
- **P6 (bound-receiver, #337/#338)**: conn/listener/socket は所有権付きの Move である — v1 では束縛さ
  れていない一時値をレシーバにできない(先に束縛が必要)。`tcp.connect(...).reader()` は
  Move-temp の drop 対応が実装されるまで拒否される。

## Test checklist

- `dns.resolve` の localhost が 127.0.0.1 を含む
- ローカルの listener へ connect し、reader/writer を通じてバイトを往復させる
- conn の Drop 後に reader を使う → コンパイルエラー(P2)
- accept ループが N 個のクライアントを処理する
- udp の `send_to`/`recv_from` の往復
- fd が二重に close されない(RSS/fd カウントのテストパターン)
- conn/listener を array の要素にする → 拒否される
- 束縛されていない一時値をレシーバにする → 拒否される
- import が必須であること
- (統合テストには、プロセス内で動作するループバック listener が必要 — m9 の io テストハーネスのパターン。)

**Note**: v1 はブロッキングプール上のブロッキングソケットである。Non-blocking/epoll/io_uring は同一
シグネチャの背後にある後日の Linux バックエンドであり、意味論上の変更ではない。
