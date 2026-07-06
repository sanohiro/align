このディレクトリには、ロードマップの本文だけでは足りない std モジュールについて、Opus がそのまま実装に
移せる粒度の設計仕様を収めている。執筆はメインループ (Fable) が担当しており、各モジュールを実装する際は
これが source of truth となる。

# std.net — implementation design (M11)

> 🌐 [English](../net.md) · **日本語**

## Overview

低レベルのソケット群である: tcp、udp、dns、socket。いずれも syscall に裏打ちされる。設計の要は再利用に
ある。接続済みソケットの fd は、**既存の M9 reader/writer** にそのまま差し込める。多態性は construction
の側 — fd を所有するハンドルを返す net 側のコンストラクタ — にあり、read/write と Drop での fd close の
仕組みは全く同一である(draft §18.2 の io 原則。reader/writer が fd に対して汎用であることで実現してい
る)。つまり net が足すのはソケットのライフサイクルと DNS だけで、新しい I/O パスは足さない。

## Signatures

v1 案である。draft §18.2 はメンバー名を列挙するだけなので、以下は Fable が確定させた形を示す:

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
  `Ty::UdpSocket`)。いずれも fd を 1 つ所有し、Drop = close(fd) となる — reader/writer/buffer の Move の
  前例そのままである。array/slice/vec/box の要素や Option/Result のペイロードとしては `scalar_arg` の
  チョークポイントで拒否する。ただし自分のコンストラクタが返す Result の Ok ペイロード位置だけは例外で
  ある(connect/listen/accept/bind はいずれも `Result<T, Error>` を返す)。reader/writer が許可されたのと
  同じく、これらの Ok 位置は許可する(`Scalar::Buffer` #346 のテンプレート)。
- `c.reader()`/`c.writer()` は、conn の fd 上に構築した M9 の reader/writer を **借用** として返す
  (`owns_fd: false` — fd の所有と close は引き続き conn 側が担う)。したがって reader/writer のリージョン
  は conn `c` に束縛され、`c` の Drop を越えて使うことは拒否される(`region_of(TcpReader) = region_of(c)`)。
  これは #297 のトラップを意識した分岐である。
- `dns.resolve` は所有権付きの `array<string>` を返す(`read_dir` #339 と同じ deep-drop)。
  `datagram`/`response` は小さな構造体(Copy)で、カウントと、必要に応じて所有権付きの peer/body を運ぶ。
  - **Slice 4 v1 の形(実装済み):** `recv_from` は受信**バイト数**のみを返す(`Result<i64, Error>`)。
    `reader.read` とまったく同じ形(呼び出し側のバッファを埋め、バイト数を返す)。理想形である
    `datagram {n, peer}` は**先送り**する:`Result` の `Ok` ペイロードは単一の `Scalar` であり
    (`Scalar::Tuple` は存在しない)、peer アドレスは所有権付きの `string` なので、`{n, peer}` は
    所有フィールドを持つ組み込み Move 構造体という集約型を新たに合成する必要がある — これは
    「あるべき姿、さもなくば先送り」が禁じる特殊ケースの魔法である。第一級の組み込み構造体戻り値が
    入るまで待つ。ソケット自体は syscall(`recvfrom`)で peer を受け取っているが、v1 では単に破棄する
    (`src_addr` は null)。

## Effect classification

net の操作はすべて **impure**(syscall)である — `par_map` のクロージャには決して入らない。

## Error policy

syscall の失敗は **共有の errno→Error テーブル**(M9)を通す。ECONNREFUSED/ETIMEDOUT/EHOSTUNREACH は
`Error.Code(errno)` になる(v1 では専用バリアントを設けず、これらで分岐したい消費者が現れたときに初めて
テーブルを拡張する)。ENOENT 系の DNS 失敗は、resolve 専用の `Error.Invalid` か `Error.Code` にする。
部分的な read/write は、再利用する reader/writer 側がすでに正しく処理している。ストリーム途中のコネクション
リセットは read/write の Error として表面化する。

## Concurrency model

記録済みのレール(open-questions「Network std rails」)は、デフォルトでコネクションを再利用する(keepalive
ON)ことである。net は、上限付き並行バッチングのための **基盤** を提供する — `task_group` と `par_map` の
ブロッキングプールである(新しい async ランタイムではない。`io_uring` はあくまで後日の Linux バックエンド
であって、意味論上のモデルではない)。具体的なバッチ API(`get_many`、パイプライン化した write-then-read)
は **1 層上の `std.http`**(`cl.get_many`)に置く。これらは HTTP のリクエスト/レスポンス型を扱うもので、
それらは `std.http` の型だから、`std.net` に**置いてはならない**(net→http の依存はレイヤリング違反、
すなわち循環依存になる。http.md 参照)。net はバイトストリームに対して汎用のままにしておく。単一の静的
ホストに対して接続ごとにループを回す実装は lint の対象だが、これは post-v1 の lint として記録するだけで、
このモジュールでは実装しない。HTTP/3、TLS、ソケットのチューニング(TFO/REUSEPORT/thread-per-core)は
std ではなく pkg の領分である。

## New machinery required

必要になるものは次のとおり。Move 型の `Ty` 3 種(TcpConn/TcpListener/UdpSocket)+ ランタイム構造体 +
Drop(close)。ソケットのライフサイクル用ランタイム関数(socket/connect/bind/listen/accept、`dns.resolve`
用の getaddrinfo、sendto/recvfrom)。バイトパスは M9 の reader/writer をそのまま再利用する(ここが最大の
利点)。借用した reader/writer をその conn に束縛する `region_of` 分岐。そして `std.http` の `get_many` が
土台にする `task_group` + ブロッキングプールの基盤(バッチング自体は net ではなく http の担当)。新しい
effect も、新しい I/O パスも、async ランタイムも要らない。

## Slice breakdown

1. `dns.resolve` 単体(getaddrinfo → 所有権付き `array<string>`) — 最小で、Move 型を伴わず、errno パスと
   deep-drop を検証できる。
2. `tcp_conn` の Move 型 + `connect` + `reader()`/`writer()` の借用(reader/writer 再利用の核心的な証明)
   + Drop での fd close + 全パスの Gate-1 スイープ。
3. `tcp_listener` + `listen` + `accept`(サーバ側)。
4. `udp_socket` + `bind` + `send_to` + `recv_from`。

(バッチ化した `get_many` のレールは、ここではなく `std.http` で実装する — HTTP 型が要るからである。net が
供給するのは `task_group` + ブロッキングプールの基盤だけで、これはすでに利用できる。)

## Pitfalls (implement carefully)

- **P1 (Move sweep ×3)**: 新設の 3 つの Move Ty は、reader/writer と同じく全パスを漏れなく通す必要がある
  (`ty_is_move`/`tracks_region`/`null_moved_source`/drop/`MoveCheck`/`EscapeCheck`/`region_of`/finalize/
  MIR/codegen/print)。最もリスクが高い。漏らせば fd の二重 close かリークになる。
- **P2 (借用した reader/writer のリージョン, #297)**: `c.reader()`/`writer()` は conn の fd を借用する
  (`owns_fd:false`)。そのリージョンは Static ではなく必ず `region_of(c)` でなければならない。さもないと
  reader が conn の `close(fd)` より長生きし、use-after-close になる。`region_of` の分岐を明示的に加え、
  escape テストを用意する。ここは微妙な点である: reader 自体は Move 型だが、ここでは非所有の借用として
  振る舞うため、自身の Drop で fd を close してはならない(`owns_fd:false` により close 抑止はランタイム側
  ですでに処理済みだが、リージョン束縛の方は新規である)。
- **P3 (fd の二重 close)**: fd を所有するのは conn である。`reader()`/`writer()` の借用は `owns_fd:false`
  を立て、close するのは conn の Drop だけになるようにする。二重に close する経路がないことを検証する。
- **P4 (バッチングは net ではなく http にある)**: バッチ化した `get_many` は HTTP のリクエスト/レスポンス
  型を扱うので、`std.net` ではなく `std.http`(`cl.get_many`)に属する — ここに置くと net が http に依存
  してしまう(レイヤリング違反、すなわち循環依存)。net が公開するのは基盤(task_group + `par_map` の
  ブロッキングプール)だけである。http 側で実装するときは、このプールを再利用し(リクエストごとにスレッド
  を起こさない)、`max_concurrency` で上限を設け、1 件のリクエスト失敗はそのスロットを Err にするだけで
  バッチ全体を中断させず、ネストした `task_group` によるデッドロックを避ける(#301 の work-claiming の
  教訓)。
- **P5 (DNS の所有権付き文字列の deep-drop)**: `resolve` が返す `array<string>` は、各 IP 文字列を
  deep-free しなければならない(`read_dir` #339 のテンプレート)。
- **P6 (bound-receiver, #337/#338)**: conn/listener/socket は所有権付きの Move なので、v1 では束縛して
  いない一時値をレシーバにできない(先に束縛する)。`tcp.connect(...).reader()` は Move 一時値の drop
  対応が入るまで拒否する。

## Test checklist

- `dns.resolve` の localhost に 127.0.0.1 が含まれる
- ローカルの listener へ connect し、reader/writer 経由でバイトを往復させる
- conn の Drop 後に reader を使う → コンパイルエラー(P2)
- accept ループが N 個のクライアントをさばく
- udp の `send_to`/`recv_from` の往復
- fd が二重に close されない(RSS/fd カウントのテストパターン)
- conn/listener を array の要素にする → 拒否
- 束縛していない一時値をレシーバにする → 拒否
- import が必須であること
- (統合テストにはプロセス内で動くループバック listener が要る — m9 の io テストハーネスのパターン。)

**Note**: v1 はブロッキングプール上のブロッキングソケットである。Non-blocking/epoll/io_uring は、同じ
シグネチャの背後に置く後日の Linux バックエンドであって、意味論上の変更ではない。
