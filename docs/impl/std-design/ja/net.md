このディレクトリには、ロードマップの本文ではカバーしきれない `std` モジュールについて、Opus がそのまま実装に
着手できる粒度の設計仕様を収めている。執筆はメインループ（Fable）が担当しており、各モジュールの実装において
これが信頼できる情報源（source of truth）となる。

# std.net — implementation design (M11)

> 🌐 [English](../net.md) · **日本語**

> **ステータス:** M11 で完了済みです。DNS、TCP client/server、UDP は実装済みです。

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
u.recv_from(buf: mut buffer) -> Result<i64, Error>        // fills caller buffer, returns byte count (v1)
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

**`l.accept()` だけは例外であり、それは意図的である: inbound コネクション 1 本の失敗は、listener の失敗
ではない。** `accept(2)` は両者を同じ errno で報告するので、コネクションを記述しているほうは返さずに内部で
リトライする — `EINTR`、`ECONNABORTED`（client が SYN と accept の間で諦めた）、そして Linux では
accept(2) が名指ししている、接続にすでに保留されているネットワークエラー（`ENETDOWN`、`EPROTO`、
`ENOPROTOOPT`、`EHOSTDOWN`、`ENONET`、`EHOSTUNREACH`、`EOPNOTSUPP`、`ENETUNREACH`）— man page が
「EAGAIN と同様にリトライして扱え」と述べているものである。したがって `loop { c := l.accept()? … }` と
書かれた accept ループは、client 1 本の行儀の悪さで終わらされることはない — 上記の errno が `connect` では
`Error.Code` に届くのに、ここでは届かない理由はまさにこれである。それ以外はすべて、fd の枯渇
（`EMFILE`/`ENFILE`）も含めて**返す**: 素の listener は回収できる idle コネクションを持たないので、その判断は
呼び出し側のものであり続ける。（std.http のサーバ側レールはこのノイズ規則を共有し、さらに枯渇からも回復
する — http.md item 9 — あちらは parked コネクションの集合を実際に持っているからである。）

## Concurrency model

記録済みの基盤（レール）(open-questions「Network std rails」)は、デフォルトでコネクションを再利用する(keepalive
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

(バッチ化した `get_many` の基盤（レール）は、ここではなく `std.http` で実装する — HTTP 型が要るからである。net が
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

## I/O timeouts (align-llm Request 2 — 完了: net レール #633 + http サーフェス #634、2026-07-24)

> **ステータス:** 以下の net レールは実装済み — `align_rt_tcp_connect` に `timeout_ns` パラメータが加わり
> (ノンブロッキング connect + `poll(POLLOUT)` デッドライン;`timeout_ns == 0` は従来どおりのブロッキング
> connect)、`c.read_timeout_ns(ns)` / `c.write_timeout_ns(ns)`(`setsockopt(SO_RCVTIMEO/SO_SNDTIMEO)`)は
> その場でデッドラインを設定し、その満了を reader/writer のバイト経路が `Err(Error.Timeout)` として表面化する。
> raw な `tcp.connect(host, port)` サーフェスはタイムアウト無しのままでリテラル `0` を渡す。`std.http` の
> `cl.timeout(ns)` / `r.timeout(ns)` サーフェス(http.md「I/O timeouts」)は #634 で出荷済みで、同じ
> `align_rt_tcp_connect` パラメータを通して有効タイムアウトを渡す。

`std.http` のリクエスト単位タイムアウト(http.md「I/O timeouts」)は net 基盤（レール）に載るので、基盤はここで
設計する;net は raw-socket 呼び出し側向けにこれを直接も公開する。動機は `align-llm` の LLM API 呼び出し
(ブラックホール化されたコネクションがループを停滞させてはならない)である。出典:
`../align-llm/docs/align-requests.md` の Request 2。

### Surface

read/write デッドラインは束縛ローカルの conn への in-place セッタである(同じ Move ビルダのイディオム):

```text
c := tcp.connect(host, port)?
c.read_timeout_ns(ns: i64)      // SO_RCVTIMEO; 0 = block forever (default)   -> ()
c.write_timeout_ns(ns: i64)     // SO_SNDTIMEO                                 -> ()
```

負の `ns` は構築時に拒否する(abort)。デッドラインを超える read/write は `Err(Timeout)` を返す — 共有の
`Error.Timeout` バリアント(正準定義は `process.md`;`AL_TIMEOUT = 4`)。デッドライン期限切れ
(`SO_RCVTIMEO` からの `EAGAIN`/`EWOULDBLOCK`)は syscall 境界でのみ spurious wakeup と区別不能なので、
read/write 箇所はデッドライン武装済みの `EAGAIN` を明示的に `AL_TIMEOUT` に変換する(タイムアウト非武装の fd では
汎用 errno 経路は不変)。

### Connect timeout — the shared substrate

**connect** デッドラインは `align_rt_tcp_connect`(ランタイム `:679`;現在は「no connect timeout」、`:621`)に
宿る:これが `timeout_ns` パラメータを得る — non-blocking `connect` → `EINPROGRESS` → ns デッドラインで
`poll(POLLOUT)` → `SO_ERROR` を確認;poll タイムアウトは `AL_TIMEOUT` を返す。`timeout_ns == 0` は現在の
ブロッキング connect を正確に保つ。`std.http` はこの同じパラメータを通して有効リクエストタイムアウトを渡す。
raw-`net` の `tcp.connect(host, port)` シグネチャは v1 ではタイムアウト無しのまま(デッドラインを設定する
connect 前のハンドルが無く、Align には任意引数が無い);raw-socket の消費者が有界 connect を必要とするなら、
`tcp.connect_timeout(host, port, ns)` という兄弟が記録済みのフォローアップである。ここで行う要点は、基盤が一度
存在し `std.http` がそれを再利用することであって、二つ目の http ローカルな機構ではない。

### New machinery

`Error.Timeout` + `AL_TIMEOUT`(共有、process.md 参照)。`align_rt_tcp_connect` が `timeout_ns` を得る。
`align_rt_tcp_read_timeout` / `align_rt_tcp_write_timeout`(`setsockopt(SO_RCVTIMEO/SO_SNDTIMEO)`)+
`read_timeout_ns` / `write_timeout_ns` に対する sema の `TcpConn` メソッドディスパッチ。新 Ty も新 I/O 経路も
無い — 既存のブロッキングレール上のソケットオプションである。

### Test / gate

ブラックホール化された(決して accept しない)アドレスへ上限付きで connect → 上限内で `Err(Timeout)`。相手が
accept した後に決して送らない conn で `read_timeout_ns` を設定 → read が上限内で `Err(Timeout)` を返す。
`ns == 0` はブロッキング挙動を保つ。
