# pkg — kv

> [English](../kv.md) · **日本語**
>
> **注意:** 英語版 (`../kv.md`) が正本。本書は同期ミラーである。
>
> **ステータス:** design candidate。独立レビューが閉じるまで公開契約は未承認。

## 公開契約台帳

この表が最初の `pkg.kv` capability の正本である。後続の本文と実装は field を明確化できるが、
拡張してはならない。V1 は plaintext TCP 上の同期 RESP2 text-value client 1 個である。generic
Redis command surface、protocol negotiation、compiler operation、ambient endpoint、hidden retry は
加えない。package-internal かつ source-reachable な runtime 1 行で checked timeout installation を閉じる。
既存 TCP-derived writer path は in-place に強化し、この package だけでなく全 `std.net` consumer が
SIGPIPE-safe write を得る。

| 公開表面 | exact input・default・検証・評価 | exact result・error・順序・effect | ownership・lifetime・allocation・cleanup | compiler/runtime/package owner・artifact・cache identity | prerequisite・acceptance owner |
|---|---|---|---|---|---|
| `pub resource client = pkg.kv.internal.resource.drop_client` | `connect` 成功時だけ構築される opaque non-null resource 1 個。nominal、Move、non-Copy、比較不能、print 不能で、public raw conversion/constructor はない。 | live value は同期 mutable operation を 1 個だけ許す。transport failure、oversized response、malformed/unexpected/truncated RESP reply は最初の error を返す前に close し、以後は I/O なしで常に `Error.Closed`。完全に framed された non-UTF-8 GET/error payload は例外として reusable `Decode`。public close operation は `Drop` だけ。 | resource は package state allocation 1 個と、live 中は runtime TCP connection、non-owning reader shell、non-owning unbuffered writer shell を各 1 個所有。live value の Move は 4 allocations 全部を移し、`borrow mut` は call-bounded で request overlap を排除。Drop は writer、reader の順に free し、socket を高々 1 回 close、state を厳密に 1 回 free。 | `pkg.kv` が nominal identity と synthesized Drop thunk、`pkg.kv.internal.resource` が private state と hook を所有。既存 resource interface identity は nominal name、representation version、Drop-thunk fingerprint を含む。 | 出荷済み opaque resource/TCP。formation/visibility、Move/Drop、control flow、malformed state、close-once、later-Closed、whole/per-unit、interface identity owner。 |
| `pub ClientOptions { connect_timeout_ns: i64, io_timeout_ns: i64, max_response_bytes: i64 }` | field/source order は表示どおり exact。default なし。両 timeout は `1..=86400000000000` ns。`max_response_bytes` は `0..=536870912` で、GET bulk payload または owned RESP error payload の inclusive cap。 | Copy/Pure。不正 field は `connect` 中に field order で、DNS/allocation/socket work より前に `Error.Invalid`。正の 1 microsecond 未満の I/O timeout は出荷済み TCP の 1 microsecond clamp を継承。 | i64 field 3 個。borrow/allocation/Drop/retained ambient state なし。成功時は socket が設定済み I/O timeout を保持し、package state が response cap を保持。connect timeout は構築で消費。 | nominal 定義は `pkg.kv` owner。whole-program/per-unit interface は name と ordered fields を serialize し、完全な定義が interface/dependency/cache identity に入る。 | 出荷済み i64-ns duration/TCP timeout machinery。field/order/exact/next bound、no-default、sub-microsecond、whole/per-unit、cache owner。 |
| `pub SetCondition { Always, IfAbsent, IfPresent }` | closed source/discriminator order は exact に `Always = 0`, `IfAbsent = 1`, `IfPresent = 2`。 | Copy/Pure。順に condition token なし、`NX`、`XX` に対応。integer/string selector や unknown fallback はない。 | Copy tag 1 個。borrow/allocation/Drop/retained state なし。 | nominal sum と interface discriminator order は `pkg.kv` owner。 | exact tag/order、construction/match、interface、malformed checked-HIR owner。 |
| `pub SetOptions { condition: SetCondition, expires_in_ns: Option<i64> }` | field/source order は表示どおり exact。default なし。`None` は persistent value。`Some(ns)` は `1..=i64::MAX` で、checked `ceil(ns / 1000000)` により Redis `PX` millisecond へ変換。 | Copy/Pure。不正 expiry は request construction/I/O より前に `Error.Invalid`。`None` は意図的に plain `SET` を使い、Redis SET semantics に従って既存 key TTL を削除。 | Copy tag と i64 が 1 個ずつ、Copy condition 1 個。borrow/allocation/Drop/clock read/retained state なし。 | nominal 定義は `pkg.kv` owner。完全な reachable definition graph が通常の interface/dependency identity に入る。 | exact condition/expiry product、ns-to-ms boundary/overflow、persistence/TTL interop、interface/cache owner。 |
| `pub Error { Invalid, Io(core.Error), Server(string), Decode, ResponseTooLarge, Protocol, Closed }` | closed source/discriminator order は表示どおり exact に `0..=6`。`Invalid` は caller input/options。`Io` は builtin transport category/code を変更せず保持。`Server` は完全な UTF-8 RESP error payload 1 個。`Decode` は完全に消費した non-UTF-8 bulk/error string。`ResponseTooLarge` は caller cap 超過の GET/error payload、またはそれ以外の admitted control line の 64 byte 超過。`Protocol` は malformed、unexpected、partial truncation、trailing framing/control data。`Closed` は reply byte より前の EOF、または retired client の後続利用。 | `Server` が string を所有するため Move。message synthesis/logging/retry/reconnect/redirect handling/second cleanup error はない。operation の最初の error が勝つ。完全で bounded な `Server` response と完全な `Decode` では synchronized client を再利用可能。`Invalid` は I/O 前。`Io`、`ResponseTooLarge`、`Protocol`、first-observation `Closed` は retire。 | allocation を所有するのは `Server` だけ。error の Move で移し、Drop が通常どおり free。reply view/scratch buffer は escape しない。他 variant は allocation なし。 | 通常 package sum identity。`Io` は tag を変えず always-available な `core.Error` identity を再利用。 | variant/payload/order/interface owner、全 producer x reuse/close state、owned-error escape/Drop、whole/per-unit、malformed-HIR owner。 |
| `pkg.kv.connect(host: str, port: i64, options: ClientOptions) -> Result<client, Error>` | 引数は左から右に 1 回評価。nonempty かつ U+0000 なしの host、`1..=65535` の `port`、source order の option fields の順に、全 side effect より前に検証。host はそれ以外 byte-exact UTF-8 のまま system resolver へ渡す。URL/default host/port/database number/credential/environment/config file はない。 | 成功は TCP connection 1 個を確立し、保持する socket I/O timeout を両方 strict に設定し、reader、writer shell の順に構築して live client を返す。PING/AUTH/SELECT/HELLO その他 Redis byte は送信しない。native connect/configuration failure は `Error.Io(core.Error)`。`connect_timeout_ns` は resolved address ごとの socket-connect attempt を制限し、DNS と address list 全体の end-to-end deadline ではない。Impure。 | host は resolution 中だけ借用し、runtime の NUL-terminated resolver input へ一時的に 1 回 copy。成功は connection、reader shell、writer shell、package state の exact 4 allocations を保持。失敗した全 candidate socket と resolver allocation は runtime が cleanup。timeout configuration failure は wrapper/state 構築前に新 socket を close し、client を公開しない。wrapper/state OOM は hard-abort policy。 | 通常 package source は出荷済み `align_rt_tcp_connect`/free と planned unkeyed `align_rt_tcp_conn_set_io_timeout` の exact compatible extern を使う。新 ABI shape/checked-HIR operation/compiler recognition はない。 | 出荷済み TCP/resource と planned checked-timeout row。validation/no-side-effect、resolver/address ordering、per-attempt timeout、IPv4/IPv6 loopback、native status、strict timeout installation、construction order、cleanup、effect、whole/per-unit owner。 |
| `pkg.kv.get(borrow mut owner: client, key: str) -> Result<Option<string>, Error>` | receiver、key の順に 1 回評価。live state、key length `0..=536870912`、checked canonical RESP request length の順に allocation/I/O より前に検証。request は bulk string のため empty UTF-8 key と embedded NUL/CR/LF を許す。 | exact uppercase 2-element RESP2 `GET` を送信。bulk reply は owned `Some(string)` 1 個、null bulk `$-1` は `None`、zero-length bulk は `Some("")`。完全な non-UTF-8 bulk は消費後 `Decode` を返し client を live のまま保つ。valid bounded `-` reply は `Server`、完全な non-UTF-8 error も reusable `Decode`。他 type/length/framing、partial EOF、current read 内の completed reply 後の byte は `Protocol`。1 byte 前の EOF は `Closed`。cap 超過を宣言した bulk は drain せず `ResponseTooLarge`。Impure。 | key は同期 write 中だけ借用し非保持。成功は ordinary owned result string 1 個を公開し、`None` は result allocation なし。retained reader/writer shell は synchronized success 後も存続し、receive chunk/line state/conversion storage/unpublished output は operation owner で全 exit 時に Drop。返す value は client/key/scratch を借用しない。 | package source が既存 TCP-derived writer、reader、buffer、UTF-8 row と exact compatible extern 上の RESP assembly/parser state を所有。generic writer prerequisite が全 connection-derived writer の SIGPIPE を抑止し、package-specific write row/runtime parser はない。 | official RESP2/GET semantics。independent wire vector、fragmentation/coalescing、null/empty/exact/next bound、UTF-8/NUL/CRLF、ownership/Drop、safe-write、error/reuse、loopback owner。 |
| `pkg.kv.set(borrow mut owner: client, key: str, value: str, options: SetOptions) -> Result<bool, Error>` | receiver、key、value、options を左から右に 1 回評価。live state、key/value length を各 `0..=536870912`、condition、expiry、全 request-length/decimal calculation の順に allocation/I/O より前に検証。empty と embedded NUL/CR/LF の key/value byte は有効。 | canonical RESP2 `SET` 1 個を `SET key value`、optional `NX`/`XX`、optional `PX <ceil-ms>` の順で送信。exact `+OK` は `true`。null bulk `$-1` は `IfAbsent`/`IfPresent` だけ `false`、`Always` では `Protocol`。valid bounded UTF-8 `-` reply は `Server`、完全な non-UTF-8 error は reusable `Decode`。他 success spelling/type/integer/bulk/framing と current-read trailing byte はすべて `Protocol`。Impure。 | input は call 中だけ借用。request framing は bounded operation-owned decimal/header storage を使い、key/value は保持・clone せず直接 write。bool result は allocation なし。retained writer/reader shell は synchronized success 後も存続し、全 operation scratch は return 前に Drop。 | 通常 package source は hardened existing connection-derived writer と既存 read/buffer row を利用。atomic SET condition/expiry behavior と server clock は Redis owner。package は clock を読まない。 | official SET semantics。3 conditions x 2 expiry states、exact ns/ms edge、persistence/expiry behavior、collision/non-resurrection use、byte golden、partial-write/response failure、ownership/effect owner。 |
| `pkg.kv.delete(borrow mut owner: client, key: str) -> Result<bool, Error>` | receiver、key の順に 1 回評価。live state、key length `0..=536870912`、canonical request arithmetic の順に I/O 前に検証。key byte admission は `get` と同じ。 | exact uppercase one-key RESP2 `DEL` を送信。値が 0 の valid RESP signed-i64 integer spelling（`0`、optional sign、leading zeros）は `false`、値 1 は `true`。他 value/overflow/reply type は `Protocol`。valid bounded UTF-8 `-` reply は `Server`、完全な non-UTF-8 error は reusable `Decode`。Impure。 | key は call-bounded で非保持。bool result と通常 request framing は value-sized allocation 不要。retained writer/reader shell は synchronized success 後も存続し、全 operation scratch は return 前に Drop。 | 同じ hardened existing writer/read boundary。package-specific write row/multi-key overload はない。 | official DEL semantics。0/1 の optional-sign/leading-zero spelling、negative/two/overflow/type mutation、error、fragmentation、ownership、effect、reuse owner。 |

## 決定と範囲

最初の capability は意図的に opaque mutable owner 1 個上の 1 request/1 reply である。

```text
system resolver + plaintext TCP + RESP2  ->  pkg.kv.client
GET                                       ->  Option<owned string>
SET + explicit condition + explicit TTL   ->  bool applied
single-key DEL                            ->  bool removed
```

第二の Redis protocol API にならず、最初の実証 consumer を満たす範囲である。
`pkg.auth.session_token()` は high-entropy key を供給するが uniqueness を約束しないため、
`IfAbsent` が caller に atomic collision check を与える。`IfPresent` は expired/revoked session を
resurrect せず既存 session を refresh/replace する。optional duration が explicit server-side expiry、
one-key DEL が logout/revocation を担う。

GET/SET/DEL は generic command function に渡す string ではなく typed operation。その closed reply
shape により reuse 前の synchronization を証明できる。public RESP value sum も、typed state machine
に未読 nested data を残し得る escape hatch もない。

## 公開利用

Align の call は positional なので、declaration と call を分離して示す。

```align
import pkg.kv

fn open() -> Result<pkg.kv.client, pkg.kv.Error> {
  options := pkg.kv.ClientOptions {
    connect_timeout_ns: 1000000000,
    io_timeout_ns: 1000000000,
    max_response_bytes: 1048576,
  }
  return pkg.kv.connect("127.0.0.1", 6379, options)
}
```

```align
import pkg.kv

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

```align
import pkg.kv

fn revoke(
  borrow mut store: pkg.kv.client,
  key: str,
) -> Result<bool, pkg.kv.Error> = pkg.kv.delete(store, key)
```

どの例も named argument、optional parameter、implicit endpoint/configuration、method dispatch、
client clock、未実装 syntax に依存しない。

## 入力上限・option・検証優先順位

V1 は request key/value ごと、および設定する response cap の inclusive maximum を `536870912`
byte（512 MiB）に固定する。server 側設定が異なっても client contract であり、通常の RESP
bulk-string ceiling と一致する。key/value input は既に caller-owned で、value-sized request へ copy
せず length-delimited byte として write。`max_response_bytes` が唯一の value-sized receive allocation
を明示する。exact limit は成功し、next byte は失敗。

全 public string は型により valid UTF-8。RESP bulk framing は empty text と embedded U+0000/CR/LF を
escaping なしで許す。`connect` だけは、出荷済み system resolver が一時 C string を要するため U+0000
を拒否する。normalization/prefix/namespace/hash tag/case fold/Unicode reinterpretation はない。

`connect` は host、port、`connect_timeout_ns`、`io_timeout_ns`、`max_response_bytes` の順に検証。
command はまず完全な resource state を検証するため、不正 key/options product より `Closed` が勝ち
I/O はしない。その後 key、存在する場合 value、condition、expiry、checked wire arithmetic の順。
`set` は加算 overflow なしに正の ns を
`ns / 1000000 + (if ns % 1000000 == 0 { 0 } else { 1 })` で変換し、結果は常に正の i64 ms。
完全な public validation pass より前に builder/native call/socket write はない。

2 timeout field は hidden wall-clock promise ではなく、次の exact substrate semantics を持つ。

- `connect_timeout_ns` は synchronous DNS resolution 後の socket connect attempt ごとを制限する。
  DNS と複数 resolved address の合計は制限しない。
- `io_timeout_ns` は checked package-internal TCP row により blocking socket の receive/send timeout
  の両方へ設定。multi-read command 全体でなく、progress を待つ 1 blocking read/write を制限する。
  timeout は `Error.Io(core.Error.Timeout)` を返し、request の partial send または response の
  partial consume があり得るため client を close。

## canonical RESP2 byte

command は bulk string の array で、uppercase ASCII command name、canonical unsigned decimal length、
exact CRLF を使う。package が emit する shape は次の 4 種だけ。

```text
GET(k)                  = *2\r\n$3\r\nGET\r\n$K\r\n<k>\r\n
SET(k,v,Always,None)    = *3\r\n$3\r\nSET\r\n$K\r\n<k>\r\n$V\r\n<v>\r\n
SET(k,v,c,Some(ns))     = *5/*6, SET k v [NX|XX] PX ceil-ms
SET(k,v,c,None)         = *3/*4, SET k v [NX|XX]
DEL(k)                  = *2\r\n$3\r\nDEL\r\n$K\r\n<k>\r\n
```

`K`/`V` は UTF-8 byte length。SET の 2 行は condition token の有無に応じ array length 5/6 と 3/4
を選ぶ。exact semantic-to-wire golden は次を含む。

```text
GET "k"                                  *2\r\n$3\r\nGET\r\n$1\r\nk\r\n
SET "k" "v" Always persistent           *3\r\n$3\r\nSET\r\n$1\r\nk\r\n$1\r\nv\r\n
SET "k" "v" IfAbsent 1500001 ns         *6\r\n$3\r\nSET\r\n$1\r\nk\r\n$1\r\nv\r\n$2\r\nNX\r\n$2\r\nPX\r\n$1\r\n2\r\n
DEL "k"                                  *2\r\n$3\r\nDEL\r\n$1\r\nk\r\n
```

独立 byte-to-semantic response golden は `$3\r\none\r\n` → `Some("one")`、`$-1\r\n` → `None`、
`$0\r\n\r\n` → `Some("")`、`+OK\r\n` → SET `true`、conditional `$-1\r\n` → SET `false`、
`:0\r\n` / `:1\r\n` → DEL false/true、`-ERR denied\r\n` → `Server("ERR denied")`。
test/implementation code は package encoder/parser から両側を導出せず、この byte を独立に再現する。

## reply grammar・同期・error 優先順位

parser は期待 command shape から開始するが、全 operation で RESP error を最初に認識する。V1 が受理
する server form は closed surface に必要な次だけ。

- GET: `$-1\r\n`、または `$<one-or-more-digit nonnegative decimal>\r\n<payload>\r\n`。
- SET: exact `+OK\r\n`、および conditional SET だけ `$-1\r\n`。
- DEL: parse value が exact に 0 または 1 の signed-i64 integer frame。
- 全 command: UTF-8 payload の `-<0..=max_response_bytes payload bytes>\r\n`。

request length/count text は canonical unsigned ASCII。response bulk length は leading zero を含む
1 digit 以上の unsigned decimal と、null 用 exact `-1` を受理。magnitude より先に digit grammar を
検証し、valid magnitude が configured cap を超えれば i64 に収まらなくても `ResponseTooLarge`。
admitted magnitude は必ず収まる。RESP integer text は optional `+`/`-`、leading zero を含む 1 digit
以上を受理し、signed i64 に収まる必要がある。それ以外の admitted non-error control line は marker
と CRLF を除き 64 byte cap、exact cap は成功。識別可能な invalid byte または i64 overflow は直ちに
`Protocol`、未確定 control byte の 65 byte 目を要求した時点は `ResponseTooLarge`。CRLF は exact。
array、RESP3 type、nested reply、null array、alternate simple string、semantically wrong integer、
lone CR/LF、current native read 内の completed reply 後の byte は protocol failure。input は 1 byte
ずつでも、1 read 内に複数 response part が来てもよく、framing は TCP chunk boundary と独立。

response decision order は固定。

1. negative native read status は `Io(core.Error)`。response byte 前の EOF は `Closed`、prefix 後は
   `Protocol`。いずれも client を retire。
2. 認識した `-` frame は読みながら bound。inclusive cap を越えれば `ResponseTooLarge`、drain せず
   close。完全な non-UTF-8 error payload は `Decode`、それ以外は clone して `Server`。両方とも
   synchronization 完了結果なので connection は live。
3. current command が許さない reply marker を拒否し、canonical line grammar と semantic value を
   検証。失敗はすべて `Protocol` で close。
4. GET は観測した全 length byte を検証後、valid digit magnitude と比較。cap 超過は
   `ResponseTooLarge`、drain せず close。payload と終端 CRLF を exact に読む。完全な non-UTF-8
   payload は client を live のまま `Decode`、それ以外は owned clone を公開。
5. success/`Server`/`Decode` 公開前に、同じ native read 内で complete frame 後の trailing byte を
   `Protocol` として拒否し close。後から来る unsolicited byte は future server reply と区別不能。
   V1 は Redis の one-reply-per-command contract に依存し、pipeline はしない。

全 fragment は、package-only row で bypass せず、全 `std.net` consumer 向けに harden した既存の
connection-derived `writer` を使う。private sink kind により Linux `send(MSG_NOSIGNAL)` または checked
macOS/BSD `SO_NOSIGPIPE` + `send` を選び、file/standard-stream writer は既存 generic fd path を維持。
retained macOS/BSD writer shell は option installation の成功だけを cache し、最初の失敗は byte を
送らず後続 call で retry。socket option は monotone/idempotent で、別 shell が overlap しても各 shell
は自身の installation 成功後だけ send する。一方の失敗は他方が成功していても byte を送らず、shell
は option を clear せず、owner connection の close が破棄する。
partial write と EINTR は共有 writer loop、socket timeout は共有 read/write status mapping を維持し、
positive-length zero progress は deterministic `core.Error.Code(0)`、option-install failure は byte を
送らない。writer error は 0 byte 以上の request byte が peer に届いた後かもしれないため
`Io(core.Error)` として close。GET/DEL でも automatic replay しない。cleanup は先の error を置換せず、
Drop は後の close failure を報告できない。

## ownership・allocation・state・cleanup

public resource は package-owned 40-byte、8-aligned v1 state を指す native word 1 個。

```text
offset 0   u32 version = 1
offset 4   u8  state: 0 live, 1 closed
offset 5   u8  zero
offset 6   u16 zero
offset 8   raw runtime TCP connection: non-null iff live
offset 16  raw non-owning runtime reader shell: non-null iff live
offset 24  raw non-owning unbuffered runtime writer shell: non-null iff live
offset 32  i64 max_response_bytes: 0..=536870912
```

全 operation は retained pointer を dereference または call する前に record 全体を検証。close は
3 pointer を local へ copy し、
state 1 と offset 8/16/24 の null を store 後、writer、reader、TCP owner の順に free。unbuffered
non-owning writer は pending flush を持たず、どちらの shell も fd を close しない。Drop は必要なら同じ
live-to-closed transition を反復し、その後 package state を free。既に closed なら state だけ free。
他 version/tag/reserved byte/pointer-state product/retained bound は internal malformed-state failure。
public operation は native I/O なしで `Closed`、Drop は untrusted pointer を call せず hard-abort。
safe consumer code はこの record を構築・変更できない。

command は resource を mutable borrow するため、第二 operation/task capture/replacement/move/Drop は
current request/reply と overlap 不能。network effect により全 operation は Impure で、resource rule と
独立に parallel closure 不適格。lock/shared client/global registry/background reader/callback/reversible な
post-publication connection-global mode transition はない。macOS/BSD では retained writer の monotone
SIGPIPE-ready transition が上記 failure/retry/close rule に従い、別 package operation と overlap しない。

reader/writer shell は `connect` が 1 回構築し、per-command shell allocation なしで再利用。request header
と decimal text は bounded operation storage。key/value byte は call-bounded `str` view
から write し非保持。receive chunk と framing state は response cap + fixed protocol overhead で bounded。
GET 成功または Server error は complete frame synchronization 後に ordinary owned string result 1 個を
allocate。V1 は consuming buffer-to-string freeze を加えないため、peak storage に N-byte receive buffer
と N-byte final owned copy の両方を含み得る。全 native receive buffer は first read 前に実際の
`buffer.capacity()` と requested positive capacity を比較し、不一致は EOF に見せず OOM policy で
hard-abort。intermediate raw/source buffer は unpublished で、全 error 時に最初に Drop。OOM は言語の
既存 hard-abort contract。live client が保持する exact 4 allocations を除き、per-command scratch/result
allocation-count、zero-copy receive、zeroization、throughput、latency は約束しない。

## package・runtime・artifact・cache boundary

vendorable subtree は root `pkg.kv` と `pkg.kv.internal.resource` を所有する。internal source は既に
keyed な TCP connect/free/reader/writer、I/O read/write/free、buffer new/bytes/capacity/free row と、
planned で source-reachable な unkeyed row 1 個について exact type-compatible extern declaration を使う。

```align
extern "C" {
  fn align_rt_tcp_connect(host: str, host_len: i64, port: i64, timeout_ns: i64, out: raw) -> i32
  fn align_rt_tcp_conn_free(connection: raw)
  fn align_rt_tcp_conn_reader(connection: raw) -> raw
  fn align_rt_tcp_conn_writer(connection: raw) -> raw
  fn align_rt_tcp_conn_set_io_timeout(connection: raw, timeout_ns: i64) -> i32
  fn align_rt_io_reader_read(reader: raw, output: raw) -> i64
  fn align_rt_io_reader_free(reader: raw)
  fn align_rt_io_writer_write(writer: raw, bytes: slice<u8>, length: i64) -> i32
  fn align_rt_io_writer_free(writer: raw)
  fn align_rt_buffer_new(capacity: i64) -> raw
  fn align_rt_buffer_bytes(buffer: raw, out: raw)
  fn align_rt_buffer_capacity(buffer: raw) -> i64
  fn align_rt_buffer_free(buffer: raw)
}
```

FFI `str`/`slice<u8>` は data pointer だけを供給するため、隣接する explicit length が exact compatibility
に必須で、常に同じ source view の `.len()`。新しい 1 行は次。

```text
TcpConnSetIoTimeout  align_rt_tcp_conn_set_io_timeout  i32(ptr, i64)  // ABI A04
```

`TcpConnSetIoTimeout` は non-null live connection と `1..=86400000000000` の timeout を要求する。
最初に `SO_RCVTIMEO` を設定し、失敗すれば `SO_SNDTIMEO` を試さず fixed errno-mapped status を返す。
成功時だけ `SO_SNDTIMEO` を設定してその status を返し、両方成功した場合だけ zero。第二 option
失敗時は第一が設定済みだが、package は unpublished connection を直ちに close するため、他 operation
との overlap も rollback もない。この row は
allocation/retain/close をしない。mandatory base export、source-reachable compatible extern、
collision-reserved unkeyed identity である。既存 ABI shape を再利用するため、activation は exact
base/maximum count を 347/355 から 348/356 に変え、keyed count は 330 のまま、A123 は次の
unreserved shape のまま。

既存 `TcpConnWriter`/`IoWriterWrite`/`IoWriterFree` の identity、declaration、attribute、count は不変。
private runtime `Writer` に socket sink kind を加え、`align_rt_tcp_conn_writer` だけが設定する。この kind
からの nonempty write は上記 SIGPIPE-safe send policy を使い、成功した `SO_NOSIGPIPE` だけを cache。
他 constructor は byte-identical な fd path を維持。option は monotone な per-socket setting で、overlap
する shell は各々試行でき、各 shell は自身の成功後だけ送信し、失敗した shell は retryable。shell Drop
は restore/clear しない。connection-derived writer は unbuffered/non-owning のままなので、free path は
hidden write を行わず socket を close せず、connection close が fd と option を破棄する。

source extern compatibility は各 registry row の exact LLVM type/attribute/symbol/runtime definition を
再利用し、第二 physical symbol を宣言せず collision check を bypass しない。package は HIR/MIR variant、
compiler-recognized function spelling、reflection table、static artifact、schema input、environment option
を加えない。`docs/impl/19-hir-validation-ledger.md` は不変。`docs/impl/20-runtime-abi-ledger.md` は exact
1-row inactive delta を reserve し、ABI を変えない既存 writer hardening を pin する。

whole-program compilation は通常 package body を見る。per-unit compilation は resource、
`ClientOptions`、`SetCondition`、`SetOptions`、`Error`、public signature 4 個を serialize し、producer
object は resource Drop thunk と既存 native dependency を保持。現 capability collection は module-wide
なので、どの operation を使っても root/internal TCP/I/O/buffer set 全体を保持し、call-spelling selector
で変わらない。package source/interface と dependency implementation hash が通常 object/link/cache
identity を決める。endpoint/resolver result/response/clock/source file/runtime inspection は artifact
identity に入らない。

現 function-value subset は scalar-only。この resource/reference/string/owned-Result signature は local、
aggregate field、control-joined function value を形成できず、V1 は例外を加えない。`pkg.kv` のない
project は package-specific source/native reachability を保持しない。

## complexity・performance boundary

encoding/parsing は visible key/value/reply byte に対して linear。response byte を constant 回より多く
parse せず、quadratic delimiter scan は不可。SET condition/TTL work は decimal formatting 以外 constant。
実装は bounded chunked socket read/write を使えるが、V1 は syscall count/chunk size/allocation count/
latency/throughput/server-version performance/memory-ratio target を約束しない。benchmark は acceptance
gate ではない。

## V1 non-goal と将来 boundary

generic command/reply API、binary key/value sibling、PING、EXISTS、MGET/MSET、INCR/rate-limit primitive、
TTL query/touch-only operation、compare-and-swap、transaction、Lua/script、pipeline、batch、pub/sub、
stream、list/set/hash/sorted-set operation、RESP3/HELLO、client tracking、cluster/Sentinel discovery/
redirection、replica/read preference、pool、shared/thread-safe client、reconnect/retry/backoff、AUTH/ACL
credential、SELECT/database number、URL parser、configuration file/environment、TLS/rediss、Unix socket、
proxy、metrics、tracing、framework session abstraction は含まない。TLS と credential は一緒に設計し、
V1 が plaintext secret downgrade を促さないようにする。将来の typed command/transport policy は各々
consumer と exact ledger を要し、ここで string/option tag の背後に reserve しない。

## 実装 closure matrix

generic TCP-writer hardening は、既に出荷済みの `std.net` consumer と閉じた signal-safety failure
domain を持つ distinct prerequisite capability なので、package より先に independently useful な PR
として land する。public signature/ABI identity は変更しない。残る client/resource/parser/3 command
は 1 本の strict producer-to-consumer chain。parser-only または
connection-only PR は stable public consumer を残さず、command 分割は同じ synchronization/poisoning/
fake-server/capability/Drop proof を重複させる。adversarial owner matrix を含め capability はおよそ
1,000 changed hand-written lines を超え得るが、全 reply kind が publication 前に同じ state machine で
閉じるため、1 boundary の方が integration risk が低い。

| axis | 必須 closure | owner evidence |
|---|---|---|
| public formation・identity | exact module/resource/record/sum definition、field/discriminator order、4 signature、qualification、visibility、direct/imported call、generic consumer-wrapper monomorphization、現 function-value rejection、whole/per-unit interface parity。 | public-source extraction、positive consumer compile/run、near-spelling/type/arity negative、monomorphic/generic-wrapper parity、interface round-trip、generic alias control。 |
| connect・option admission | host/U+0000、port、ordered option bound 3 個、exact/next boundary、complete validation 前 side effect なし、DNS + per-address timeout semantics、timeout application、全 native status、reader-then-writer-then-state construction、IPv4/IPv6、Redis byte なし。 | instrumented connect/runtime counter と loopback listener、resolver/refused/timeout/malformed/retained-allocation/allocation-cleanup failpoint。 |
| request byte | exact GET/SET-product/DEL array、uppercase token、canonical decimal、value-sized request copy なしの 512-MiB admission、embedded NUL/CR/LF、first write 前の全 arithmetic。 | independent semantic-to-byte golden、boundary/mutation table、fragmented writer、partial-write owner。 |
| reply framing | 全 marker、leading zero/integer sign を含む official bulk/integer grammar、64-byte control cap、CRLF、fragmented/coalesced read、null/empty/exact/next cap、error cap、trailing byte、全 ordinal の EOF、quadratic scan なし。 | deterministic scripted TCP peer による one-byte/all-split/multi-part product、independent byte-to-semantic golden、comparison counter。 |
| GET semantics | bulk/null/empty ownership、exact byte、full consumption 後 UTF-8 decode、Decode reuse、wrong kind/oversized declaration close。 | official vector + valid/invalid UTF-8、lifetime escape、subsequent command、allocation/Drop、no-drain probe。 |
| SET semantics | Always/NX/XX x None/Some、condition-before-PX wire order、ceil-ns conversion、persistent TTL removal、+OK/null result matrix、server error、unexpected status。 | scripted byte + collision/expiry/refresh-without-resurrection/exact-next duration/wrong-reply product の Redis-compatible state model。 |
| DEL semantics | one-key request、0/1 の全 official signed/leading-zero spelling、server error、他の全 value/type。 | false/true と sign/leading-zero/negative/two/overflow/type mutation matrix、reuse/close check。 |
| error・poison state | I/O 前 Invalid、bounded UTF-8 Server/full-bulk Decode は reusable、Io/too-large/protocol/truncation/partial-write は close、first error 保持、以後の全 call は zero I/O の Closed。 | error-producer x command x before/during/after-frame x reuse table、native call counter、first-error/cleanup-failure probe。 |
| ownership・cleanup | resource formation、move-in/out/return/replacement、if/match/else/?/map_err/branch/loop/early return、source nulling、state/socket/wrapper/scratch/result の Drop once、malformed state は untrusted call なし。 | resource/drop counter、allocation parity、parameterized control-flow owner、state semantic-to-byte/byte-to-semantic golden、malformed field product。 |
| ABI・effect・capability・cache | 既存 row の exact compatible reuse と `TcpConnSetIoTimeout` の atomic activation、strict timeout status、既存 connection-writer の sink provenance と partial/EINTR/zero/EPIPE/timeout mapping、writer ABI/count を変えない Linux/macOS SIGPIPE safety、新 ABI shape/HIR row なし、Impure operation、module-wide native retention、package absence、source/interface/dependency edit invalidation。 | exact registry/golden/base-export/type/collision/source-reuse owner、file/std/socket writer-kind parity、Linux/macOS subprocess closed-peer signal owner、package whole/per-unit IR/link run、effect check、no-package negative、add/remove/edit/revert cache twin。 |

## source of truth と author consistency pass

`../kv.md` の英語 ledger、`docs/impl/pkg-design/ja/kv.md`、`draft.md`、`docs/language-spec.md`、
`docs/design-notes.md`、`docs/history.md`、`docs/open-questions.md`、`docs/impl/07-roadmap.md`、
`docs/impl/std-design/net.md` とその日本語ミラー、`docs/impl/20-runtime-abi-ledger.md`、`HANDOFF.md` は
一致しなければならない。HIR ledger は不変。
exact 1-row reservation を越える ABI change、または public writer-surface change はこの design を reopen する。

candidate review 中は `docs/open-questions.md` がこの項目を Open に置き、`docs/history.md` に Settled
entry はない。acceptance 時は exact reviewed contract を Settled へ移し、history record を追加し、
各 candidate status を accepted/inactive の該当状態へ変更してから implementation を許可する。

独立 review 前の author-side pass は 2026-09-02 に完了:

- 全 public argument/result に exact type、evaluation order、default、ownership、lifetime、allocation、
  cleanup、error、effect rule が 1 個ずつある。
- command、condition、expiry、response marker、verification state、option state、field presence、row order、
  discriminator、unavailable-result product が exhaustive。
- host/key/value/error text の UTF-8、embedded-NUL、CR/LF、boundary validation、pre-side-effect semantics が固定。
- multi-invalid call の state/host/port/option/key/value/condition/expiry/wire と native/reply/error precedence が deterministic。
- endpoint/credential/database/retry/clock/resolver result/configuration/artifact/source input は ambient でない。
- canonical RESP scalar/tag/sequence order/malformed rejection、independent semantic-to-byte と byte-to-semantic golden が固定。
- resource record/RESP state machine の全 state/tag/reserved/pointer/length product、overlap exclusion、
  failed-second-operation behavior、error preservation、Drop order が固定。
- exact existing producer-owned runtime row が reflection/artifact I/O なしで native state を供給。
- example は accepted syntax を使い declaration と positional call を分離。
- acceptance owner が全 ledger invariant を覆い、約束していない benchmark を gate にしない。

official protocol/command reference: Redis
[RESP](https://redis.io/docs/latest/develop/reference/protocol-spec/)、
[GET](https://redis.io/docs/latest/commands/get/)、
[SET](https://redis.io/docs/latest/commands/set/)、
[DEL](https://redis.io/docs/latest/commands/del/)。

## design-review finding-to-fix ledger

独立 review は未実施。finding はまずこの ledger を変更し、その後全 source of truth へ 1 pass で伝播する。
