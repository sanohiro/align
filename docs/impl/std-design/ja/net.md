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
仕組みは同一である(draft §18.2 の io 原則。reader/writer が fd に対して汎用であることで実現している)。
つまり net が足すのはソケットのライフサイクルと DNS だけで、新しい I/O パスは足さない。下記の
`pkg.kv` prerequisite は writer ABI を 1 個のまま保ち、socket write が SIGPIPE を安全に抑止できる
private sink provenance だけを加える。

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
テーブルを拡張する)。resolver failure は errno でなく EAI value であり、
`EAI_NONAME`/`EAI_NODATA` は `Error.Invalid`、他の nonzero EAI value は
`encoded := AL_CODE.saturating_add(eai.saturating_abs())`、次に
`Error.Code(encoded - AL_CODE)` にする。
部分的な read/write は再利用する reader/writer が処理する。ストリーム途中の connection reset は read/write
Error として表面化する。下記の `pkg.kv` prerequisite は現在の Linux SIGPIPE hole を閉じ、closed peer への
write が Error を返す前に process を terminate しないようにする。

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
用の getaddrinfo、sendto/recvfrom)。バイトパスは M9 の reader/writer ABI を再利用し(ここが最大の利点)、
下記の private socket-sink hardening を加える。借用した reader/writer をその conn に束縛する `region_of`
分岐。そして `std.http` の `get_many` が
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
- connection-derived writer で closed peer へ write すると、SIGPIPE で process termination せず Error を返す
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
> `align_rt_tcp_connect` パラメータを通して有効タイムアウトを渡す。下記の design-candidate
> prerequisite はこの public surface や ABI identity を変えず、positive-timeout の mode transition と
> quantization を厳密化する。

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

**connect** デッドラインは `align_rt_tcp_connect` に宿る: positive `timeout_ns` は
non-blocking mode を使う。immediate zero は成功、`EINPROGRESS`/`EAGAIN`/`EWOULDBLOCK` は
`poll(POLLOUT)` へ入り、その他の immediate errno は map する。readiness は `SO_ERROR` で解決し、poll
timeout は `AL_TIMEOUT` を返す。下記 inactive prerequisite が mode installation/restoration を checked にする。
`timeout_ns == 0` は現在の
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

ブラックホール化されたアドレスへ logical wait deadline 付きで connect → early expiry せず
`Err(Timeout)`。相手が accept した後に決して送らない conn で `read_timeout_ns` を設定 → read が
configured blocking wait 後に `Err(Timeout)` を返す。
`ns == 0` はブロッキング挙動を保つ。

---

## Checked shared timeout substrate (`pkg.kv` prerequisite 1 — DESIGN CANDIDATE 2026-09-02)

> **ステータス:** 最初の independently useful prerequisite。`pkg.kv` design が accept されるまで未実装。
> public signature、compiler operation、runtime symbol、ABI shape、registry key、row count は変更しない。

各 usable resolver address と positive `timeout_ns` に対し、`align_rt_tcp_connect` は最初の `F_GETFL` 直前に
monotonic start と positive `Duration` budget を記録し、次に `F_GETFL` と
`F_SETFL(flags | O_NONBLOCK)` を検査する。いずれかの failure で fixed errno-mapped status を記録し、
candidate を close し、`connect` を呼ばず次の address へ進む。checked installation 後に immediate
`connect` を正確に 1 回呼ぶ。zero は成功、`EINPROGRESS`/`EAGAIN`/`EWOULDBLOCK` は wait へ入り、その他の
errno は直ちに map する。いずれの immediate terminal result も同時に budget が exhaust していても優先する。
in-progress path は同じ start/budget pair を継続する。absolute `start + budget` は作らないので、
`Instant::checked_add` overflow が huge positive timeout を unbounded wait に変えることはない。

- 各 iteration で budget から `start.elapsed()` を引く。positive remainder は次の millisecond へ切り上げ、
  1 回の `poll` 用に `i32::MAX` で saturate するため、positive i64 全域は repeated chunk で bounded のまま。
  exhausted remainder は次の poll を行わず `AL_TIMEOUT` を返し、最後の zero-timeout `poll` call も行わない。
- EINTR は remainder を再計算し、その他の poll error は直ちに map する。`poll` の zero は再度
  monotonic recheck を行い、まだ時間が残る場合だけ再度 poll、budget を使い切っていれば追加の poll 無しで
  `AL_TIMEOUT` とする。
- positive readiness/error event は同時に使い切った budget より優先し、`SO_ERROR` で解決する。

各 immediate/polled success 後に `F_GETFL` と `F_SETFL(flags & !O_NONBLOCK)` を検査する。
restoration failure はその status を記録し、candidate を close して次へ進む。blocking mode の checked
restoration が済むまで connection は publish しない。nonpositive raw-ABI blocking path は不変で、public HTTP
caller はこの ABI 前に negative value を拒否し、raw `tcp.connect` は zero を渡す。DNS と複数 address の合計に end-to-end
deadline は無く、scheduler/kernel delay で address の logical deadline 後に return することはある。

nonzero `getaddrinfo` result は address iteration 前に返る。`EAI_NONAME`/`EAI_NODATA` は `AL_INVALID`、
他の symbolic EAI value は `AL_CODE.saturating_add(eai.saturating_abs())` へ map。
connection output は null のまま、transient host/service storage は drop、address-list owner は escape せず、
socket を試さない。symbolic EAI owner が両 category、null output、cleanup、zero socket call を pin。

resolution 成功後の resolver order は observable。unsupported family、null address、zero address length は
last failure を変えずに skip する。最初に成功した usable address が勝つ。usable address が無ければ
`AL_INVALID` を返し、attempt した candidate がすべて失敗すれば socket creation、nonblocking `F_GETFL`、
nonblocking `F_SETFL`、immediate connect errno、poll error/timeout、`getsockopt(SO_ERROR)` failure、
nonzero `SO_ERROR`、blocking-restore `F_GETFL`、blocking-restore `F_SETFL` のうち最後の status を返す。
mixed-address owner は各 failure class の後に skipped entry と later success を置き、all-failure variant は
last attempted status と close count を固定する。

同じ prerequisite が public `read_timeout_ns`/`write_timeout_ns` と planned checked package row で共有する
socket-timeout conversion を修正する。全 positive nanosecond 値を `ceil(ns / 1000)` microseconds とし、それを
normalized `timeval { tv_sec, tv_usec: 0..999999 }` に分割する。exact microsecond は exact のまま、zero は既存の
clear/no-timeout 意味を保つ。option は 1 回の blocking progress wait を bound し、multi-read/multi-write operation 全体を
bound しない。

planned source-reachable `TcpConnSetIoTimeout` consumer は全 non-null compatible caller に、call 全体で
exclusive な 1 個の live/unfreed connection を要求する。entry 時にその connection 由来の live
reader/writer shell またはその shell を retain する value はなく、read/write/configuration/
reader-or-writer construction/free/Drop の overlap も禁止。exact normalized `timeval` を使い receive を send より先に install。entry option state を
`{R0,S0}`、requested state を `T` とすると、receive failure は `setsockopt` を exact 1 回呼び mapped status を
返して send call なし、state は `{R0,S0}`。send failure は exact 2 回、send mapped status、state は `{T,S0}`。
success は exact 2 回、zero、state は `{T,T}`。どちらかの option failure は compatible caller に retirement、
後続 read/write/configuration/reader-or-writer construction/retry 禁止、exact 1 回の free/Drop を要求する。
zero-derived-shell entry state により、その close と順序付ける shell cleanup はない。success は usability を
保ち、その後 shell を構築できるが、後の overwrite 前に全 derived shell/retaining value を Drop する。
package は fresh unpublished clear/clear connection だけで shell construction 前に call し、どちらの failure も
resolution 再開/別 address 試行なしで close。owner は live/exclusive/zero-derived-shell entry precondition、
pre-armed state、option order/call count/returned status、retry prohibition、overlap 中/failure 後の
constructor call zero、retirement、close/Drop を pin。structural owner 1 個が complete active recursive
Drop graph を通じた target-connection provenance で retainer を分類する。local/call、struct field、nested
`Option`/`Result`、admitted user-sum path にある direct/buffered reader、writer、logger-owned writer leaf を
walk し、retaining struct の source-constructed fixed array element も含む。この last path は既存 struct-field と
fixed Move-struct-array rule を compose し、direct handle array element を admit しない。canonical formation/Drop graph から
derive し new-edge tripwire を持ち、inactive/moved-out state、other/mixed-connection shell、zero/one/multiple
target leaf を cross。zero だけ compatible。各 positive carrier class は configure-construct-move-into-
move-out-where-supported-or-recursive-Drop-reconfigure を完了。direct handle collection/box/tuple と direct
reader/writer user-sum payload は formation negative。nameable dynamic-array/slice retaining-struct/sum shape、
admitted non-tuple shape の user-struct-field closure、direct `DynStructArray` に許容される dynamic-array/
slice element、tuple、builtin `Option`/`Result` edge は explicit no-live-producer owner を維持。

ceil-to-microsecond conversion は出荷済み `std.http` の plain/TLS/pool rearm にも届く。
poll-millisecond helper は `process.command` にも届き、その consumer は従来の post-syscall
timeout-wins precedence を保ったまま、positive-i64 全域に同じ monotonic start-plus-budget arithmetic と
ceil conversion を使う。これらは package-local conversion の分岐ではなく、1 個の shared prerequisite である。

acceptance owner は exact/next/maximum-positive ns、us、ms、chunk、deadline boundary、immediate/polled
success での `F_GETFL`/`F_SETFL` installation/restoration failure、early zero-result recheck と
exhausted/no-call poll、`EINPROGRESS`/`EAGAIN`/`EWOULDBLOCK` とその他 immediate errno、EINTR remainder
recomputation、deadline 時の readiness、全 resolver skip/last-status failure class、symbolic EAI branch、
mixed-address close/continuation、no early expiry、publish する全 connection の blocking-mode probe、
exact-timeval と pre-armed receive/send state および caller-retirement product、HTTP plain/TLS/pool rearm、
command pipe-drain/post-EOF reap を含む。

---

## SIGPIPE-safe connection-derived writer (`pkg.kv` prerequisite 2 — DESIGN CANDIDATE 2026-09-02)

> **ステータス:** independently useful な safety prerequisite。`pkg.kv` design が accept されるまで未実装。
> public signature、compiler operation、runtime symbol、ABI shape、registry key、row count は変更しない。

既存の `c.writer() -> writer` が唯一の TCP byte-write surface のまま。private runtime `Writer` state に
sink kind と macOS/BSD readiness bit を加え、`align_rt_tcp_conn_writer` だけが socket/not-ready に設定。
standard-stream/file constructor は generic-fd kind を設定。socket-kind の nonempty `w.write(...)` は
1 本の complete-write loop と exact existing Result taxonomy を維持し、platform rule は次のとおり。

- Linux は全 attempt で `send(MSG_NOSIGNAL)` を呼ぶ。
- macOS/BSD はその writer shell の最初の send 前に `SO_NOSIGPIPE` を lazy install し、成功だけを cache。
  option installation failure はその call が byte を送る前に fixed errno-mapped `Error` を返し、shell は
  not-ready のまま後続 call で retry する。成功後は今回以降の write で `send` を使う。
- partial send は残り view を進め、EINTR は retry。armed blocking-socket の
  `EAGAIN`/`EWOULDBLOCK` は `Error.Timeout` のまま。positive-length zero progress は spin/stale errno
  でなく deterministic `Error.Code(0)`。

process-global signal handler/mask はない。同じ call の earlier attempt が byte を送った後に failure が
起こり得るため、caller が error を受け replay policy を所有する。file/standard-stream writer は既存の
`write(2)` path を byte-for-byte で維持。connection-derived writer は unbuffered かつ
`owns_fd: false` のままで、`flush`/Drop に pending write はなく socket を close するのは `tcp_conn` だけ。
`SO_NOSIGPIPE` は monotone/idempotent な per-socket setting。overlap する shell は各々設定を試せるが、
各 shell は自身の成功後だけ send し、失敗した shell は retryable。shell Drop は clear せず、connection
close が破棄する。同じ connection-derived writer を使う logger/`io.copy` は第二 path を開かず socket
sink kind を継承する。

既存の keyed `IoWriterWriteBuilder` identity、A19
`i32 @align_rt_io_writer_write_builder(ptr, ptr)` declaration、
`unsafe extern "C" fn(*mut Writer, *mut Builder) -> i32` Rust ABI、attribute、shipped 330/347/355
keyed/base/maximum count への inclusion は変わらない。source-visible builder overload は builder byte を
borrow して hardened `IoWriterWrite` row に delegate するので、socket sink policy を迂回できない。

acceptance owner は macOS/BSD での failed install/no send 後の retry、overlap する shell の両方の
success/failure order、option clear 無しの shell Drop、setting を破棄する connection close を含む。
Linux/macOS の subprocess closed-peer test は direct slice/builder overload、logger、`io.copy` route を覆い、SIGPIPE で終了せず
必ず `Error` を返す。direct partial/EINTR/timeout/zero-progress test と file/std writer parity は別個の owner のまま。

checked timeout substrate を最初、writer hardening を 2 番目に出荷する。その後、planned
`TcpConnSetIoTimeout` row とその `pkg.kv` package consumer を同時に activate する。exact package consumption と
implementation boundary は `../pkg-design/kv.md`、one-row reservation と prerequisite の不変 ABI identity は
`../20-runtime-abi-ledger.md` に記録する。
