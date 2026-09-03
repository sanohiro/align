# pkg — kv

> [English](../kv.md) · **日本語**
>
> **注意:** 英語版 (`../kv.md`) が正本。本書は同期ミラーである。
>
> **ステータス:** 2026-09-02 に implemented。2 つの shared prerequisite、package source、
> checked runtime row は記録済みの joint capability boundary で active。

## 公開契約台帳

この表が最初の `pkg.kv` capability の正本である。後続の本文と実装は field を明確化できるが、
拡張してはならない。V1 は plaintext TCP 上の同期 RESP2 text-value client 1 個である。generic
Redis command surface、protocol negotiation、compiler operation、ambient endpoint、hidden retry は
加えない。package-internal かつ source-reachable な runtime 1 行で checked timeout installation を閉じる。
この row が activate する前に shared connect/timeout substrate の checked fd-mode transition、
start-plus-budget deadline arithmetic、`std.net`/`std.http`/`process.command` consumer の正の timeout の
exact quantization を閉じた。
既存 TCP-derived writer path は in-place に強化し、この package だけでなく全 `std.net` consumer が
SIGPIPE-safe write を得る。

| 公開表面 | exact input・default・検証・評価 | exact result・error・順序・effect | ownership・lifetime・allocation・cleanup | compiler/runtime/package owner・artifact・cache identity | prerequisite・acceptance owner |
|---|---|---|---|---|---|
| `pub resource client = pkg.kv.internal.resource.drop_client` | `connect` 成功時だけ構築される opaque non-null resource 1 個。nominal、Move、non-Copy、比較不能、print 不能で、public raw conversion/constructor はない。 | live value は同期 mutable operation を 1 個だけ許す。transport failure、oversized response、malformed/unexpected/truncated RESP reply は選択した terminal error を返す前に close し、以後は I/O なしで常に `Error.Closed`。完全に framed された non-UTF-8 GET payload、または CR/LF を除く non-UTF-8 grammar-valid Simple Error payload は例外として reusable `Decode`。public close operation は `Drop` だけ。 | resource は package state allocation 1 個と、live 中は runtime TCP connection、non-owning reader shell、non-owning unbuffered writer shell を各 1 個所有。live value の Move は 4 allocations 全部を移し、`borrow mut` は call-bounded で request overlap を排除。Drop は writer、reader の順に free し、socket を高々 1 回 close、state を厳密に 1 回 free。 | unit `pkg.kv` が interface resource record `{ name: "client", type_params: [], generic_arity: 0, representation_version: 1, drop_thunk: "__align_resource_drop$pkg.kv$client", drop_abi_fingerprint: b"align-res-drop-1" }` を所有し、`pkg.kv.internal.resource` が private hook/state を所有。全 field を serialize して interface identity に含める。 | 出荷済み opaque resource/TCP。formation/visibility、Move/Drop、control flow、malformed state、close-once、later-Closed、whole/per-unit、独立した 6-field interface mutation owner。 |
| `pub ClientOptions { connect_timeout_ns: i64, io_timeout_ns: i64, max_response_bytes: i64 }` | field/source order は表示どおり exact。default なし。両 timeout は `1..=86400000000000` ns。`max_response_bytes` は `0..=536870912` で、GET bulk payload または owned RESP error payload の inclusive cap。 | Copy/Pure。不正 field は `connect` 中に field order で、DNS/allocation/socket work より前に `Error.Invalid`。正の connect remainder は `poll` が表現できる次の millisecond へ切り上げ、正の I/O timeout は `timeval` が表現できる次の microsecond へ切り上げる。どちらも早期 expire しない。 | i64 field 3 個。borrow/allocation/Drop/retained ambient state なし。成功時は socket が設定済み I/O timeout を保持し、package state が response cap を保持。connect timeout は構築で消費。 | nominal 定義は `pkg.kv` owner。whole-program/per-unit interface は name と ordered fields を serialize し、完全な定義が interface/dependency/cache identity に入る。 | 出荷済み i64-ns duration/TCP machinery と timeout-substrate prerequisite。field/order/exact/next bound、no-default、ns/ms/us boundary、whole/per-unit、cache owner。 |
| `pub SetCondition { Always, IfAbsent, IfPresent }` | closed source/discriminator order は exact に `Always = 0`, `IfAbsent = 1`, `IfPresent = 2`。 | Copy/Pure。順に condition token なし、`NX`、`XX` に対応。integer/string selector や unknown fallback はない。 | Copy tag 1 個。borrow/allocation/Drop/retained state なし。 | nominal sum と interface discriminator order は `pkg.kv` owner。 | exact tag/order、construction/match、interface、malformed checked-HIR owner。 |
| `pub SetOptions { condition: SetCondition, expires_in_ns: Option<i64> }` | field/source order は表示どおり exact。default なし。`None` は persistent value。`Some(ns)` は `1..=i64::MAX` で、checked `ceil(ns / 1000000)` により Redis `PX` millisecond へ変換。 | Copy/Pure。不正 expiry は request construction/I/O より前に `Error.Invalid`。`None` は意図的に plain `SET` を使い、Redis SET semantics に従って既存 key TTL を削除。 | Copy tag と i64 が 1 個ずつ、Copy condition 1 個。borrow/allocation/Drop/clock read/retained state なし。 | nominal 定義は `pkg.kv` owner。完全な reachable definition graph が通常の interface/dependency identity に入る。 | exact condition/expiry product、ns-to-ms boundary/overflow、persistence/TTL interop、interface/cache owner。 |
| `pub Error { Invalid, Io(core.Error), Server(string), Decode, ResponseTooLarge, Protocol, Closed }` | closed source/discriminator order は表示どおり exact に `0..=6`。`Invalid` は caller input/options。`Io` は builtin transport category/code を変更せず保持。`Server` は CR/LF を含まない完全な UTF-8 RESP Simple Error payload 1 個。`Decode` は完全に消費した non-UTF-8 bulk または valid に framed された error string。`ResponseTooLarge` は caller cap 超過の GET/error payload、またはそれ以外の admitted control line の 64 byte 超過。`Protocol` は malformed、unexpected、partial truncation、trailing framing/control data。`Closed` は reply byte より前の EOF、または retired client の後続利用。malformed private resource record は `Closed` producer ではなく、全 public operation と Drop が native I/O または untrusted pointer access 前に hard-abort する。 | `Server` が string を所有するため Move。message synthesis/logging/retry/reconnect/redirect handling/second cleanup error はない。package operation が terminal error を選択した後は cleanup error が置換しない。resolved-address iteration には後述の別の last-failed-candidate rule がある。完全で bounded な `Server` response と完全な `Decode` では synchronized client を再利用可能。`Invalid` は I/O 前。`Io`、`ResponseTooLarge`、`Protocol`、first-observation `Closed` は retire。 | nonempty `Server` は string allocation 1 個を所有し、empty `Server("")` は canonical `{null, 0}` owned string で result buffer allocation なし。Move は representation を移し、Drop は nonempty buffer を通常どおり free。reply view/scratch buffer は escape しない。他 variant は allocation なし。 | 通常 package sum identity。`Io` は tag を変えず always-available な `core.Error` identity を再利用。 | variant/payload/order/interface owner、全 producer x reuse/close state、empty/nonempty owned-error allocation/escape/Drop、whole/per-unit、malformed-HIR owner。 |
| `pkg.kv.connect(host: str, port: i64, options: ClientOptions) -> Result<client, Error>` | 引数は左から右に 1 回評価。nonempty かつ U+0000 なしの host、`1..=65535` の `port`、source order の option fields の順に、全 side effect より前に検証。host はそれ以外 byte-exact UTF-8 のまま system resolver へ渡す。URL/default host/port/database number/credential/environment/config file はない。 | nonzero `getaddrinfo` result は address iteration 前に終了し、`EAI_NONAME`/`EAI_NODATA` は `Io(core.Error.Invalid)`、他の EAI は `encoded := AL_CODE.saturating_add(eai.saturating_abs())` を計算して `Io(core.Error.Code(encoded - AL_CODE))` へ map。resolution 成功後は usable address を返却順に試し、unsupported family、null address、zero address length は skip、最初の成功が勝つ。その list に usable entry がなければ `Io(core.Error.Invalid)`、全 attempted entry が失敗すれば最後の socket/connect/mode-transition failure。成功は checked nonblocking connect を完了し blocking mode を checked-restored した socket だけを公開し、保持する I/O timeout を両方 strict に設定し、reader、writer shell の順に構築して Redis byte は送らない。receive/send どちらの timeout installation failure でも selected connection を retire/close し別の resolved address を試さない。send failure では close 前に receive が変わっている場合がある。DNS と aggregate address list には end-to-end deadline がない。Impure。 | host は resolution 中だけ借用し、runtime の NUL-terminated resolver input へ一時的に 1 回 copy。resolver failure は address list を保持せず connection output を null のままにし、socket を試さず transient host/service storage を drop。成功は connection、reader shell、writer shell、package state の exact 4 allocations を保持。失敗した全 candidate socket と成功した resolver list は runtime が cleanup。どちらの timeout-installation failure も wrapper/state 構築前に新 socket を close し、client を公開しない。wrapper/state OOM は hard-abort policy。 | 通常 package source は出荷済み `align_rt_tcp_connect`/free と active unkeyed `align_rt_tcp_conn_set_io_timeout` の exact compatible extern を使う。compiler registry は fixed-ABI compatibility/collision/reachability のため physical timeout symbol を認識するが、新 language builtin、checked-HIR/MIR operation、ABI shape、call-spelling selector はない。 | 出荷済み TCP/resource、timeout-substrate hardening、active checked-timeout row。validation/no-side-effect、resolver EAI mapping と ordering/skips/empty/mixed failure、checked mode transition、timeout quantization/precedence、IPv4/IPv6 loopback、native status、strict timeout installation、construction/cleanup/effect、whole/per-unit owner。 |
| `pkg.kv.get(borrow mut owner: client, key: str) -> Result<Option<string>, Error>` | receiver、key の順に 1 回評価。live state、key length `0..=536870912`、checked canonical RESP request length の順に allocation/I/O より前に検証。request は bulk string のため empty UTF-8 key と embedded NUL/CR/LF を許す。 | exact uppercase 2-element RESP2 `GET` を送信。bulk reply は owned `Some(string)` 1 個、null bulk `$-1` は `None`、zero-length bulk は `Some("")`。完全な non-UTF-8 bulk は消費後 `Decode` を返し client を live のまま保つ。payload が CR/LF を除く完全で bounded な grammar-valid Simple Error frame は framing 後、UTF-8 なら `Server`、それ以外は reusable `Decode`。他 type/length/framing、partial EOF、current read 内の completed reply 後の byte は `Protocol`。1 byte 前の EOF は `Closed`。cap 超過を宣言した bulk は drain せず `ResponseTooLarge`。Impure。 | key は同期 write 中だけ借用し非保持。nonempty GET 成功は ordinary owned string allocation 1 個を公開し、empty `Some("")` は canonical `{null, 0}`、`None` は result allocation なし。retained reader/writer shell は synchronized success 後も存続し、receive chunk/line state/conversion storage/unpublished output は operation owner で全 exit 時に Drop。返す value は client/key/scratch を借用しない。 | package source が既存 TCP-derived writer、reader、buffer、UTF-8 row と exact compatible extern 上の RESP assembly/parser state を所有。generic writer prerequisite が全 connection-derived writer の SIGPIPE を抑止し、package-specific write row/runtime parser はない。 | official RESP2/GET semantics。independent wire vector、fragmentation/coalescing、null/empty/nonempty allocation、exact/next bound、UTF-8/NUL/CRLF、ownership/Drop、safe-write、error/reuse、loopback owner。 |
| `pkg.kv.set(borrow mut owner: client, key: str, value: str, options: SetOptions) -> Result<bool, Error>` | receiver、key、value、options を左から右に 1 回評価。live state、key/value length を各 `0..=536870912`、condition、expiry、全 request-length/decimal calculation の順に allocation/I/O より前に検証。empty と embedded NUL/CR/LF の key/value byte は有効。 | canonical RESP2 `SET` 1 個を `SET key value`、optional `NX`/`XX`、optional `PX <ceil-ms>` の順で送信。exact `+OK` は `true`。null bulk `$-1` は `IfAbsent`/`IfPresent` だけ `false`、`Always` では `Protocol`。payload が CR/LF を除く完全で bounded な grammar-valid Simple Error frame は UTF-8 なら `Server`、それ以外は reusable `Decode`。他 success spelling/type/integer/bulk/framing と current-read trailing byte はすべて `Protocol`。Impure。 | input は call 中だけ借用。request framing は bounded operation-owned decimal/header storage を使い、key/value は保持・clone せず直接 write。bool result は allocation なし。retained writer/reader shell は synchronized success 後も存続し、全 operation scratch は return 前に Drop。 | 通常 package source は hardened existing connection-derived writer と既存 read/buffer row を利用。atomic SET condition/expiry behavior と server clock は Redis owner。package は clock を読まない。 | official SET semantics。3 conditions x 2 expiry states、exact ns/ms edge、persistence/expiry behavior、collision/non-resurrection use、byte golden、partial-write/response failure、ownership/effect owner。 |
| `pkg.kv.delete(borrow mut owner: client, key: str) -> Result<bool, Error>` | receiver、key の順に 1 回評価。live state、key length `0..=536870912`、canonical request arithmetic の順に I/O 前に検証。key byte admission は `get` と同じ。 | exact uppercase one-key RESP2 `DEL` を送信。値が 0 の valid RESP signed-i64 integer spelling（`0`、optional sign、leading zeros）は `false`、値 1 は `true`。他 value/overflow/reply type は `Protocol`。payload が CR/LF を除く完全で bounded な grammar-valid Simple Error frame は UTF-8 なら `Server`、それ以外は reusable `Decode`。Impure。 | key は call-bounded で非保持。bool result と通常 request framing は value-sized allocation 不要。retained writer/reader shell は synchronized success 後も存続し、全 operation scratch は return 前に Drop。 | 同じ hardened existing writer/read boundary。package-specific write row/multi-key overload はない。 | official DEL semantics。0/1 の optional-sign/leading-zero spelling、negative/two/overflow/type mutation、error、fragmentation、ownership、effect、reuse owner。 |

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
command はまず完全な resource state を検証する。malformed record は native I/O または untrusted pointer
access 前に hard-abort。canonical record では不正 key/options product より `Closed` が勝ち I/O はしない。
その後 key、存在する場合 value、condition、expiry、checked wire arithmetic の順。
`set` は加算 overflow なしに正の ns を
`ns / 1000000 + (if ns % 1000000 == 0 { 0 } else { 1 })` で変換し、結果は常に正の i64 ms。
完全な public validation pass より前に builder/native call/socket write はない。

2 timeout field は hidden wall-clock promise ではなく、次の exact substrate semantics を持つ。

- `connect_timeout_ns` は synchronous DNS resolution 後、usable socket address ごとの最初の
  `F_GETFL` 直前に fresh monotonic start と positive `Duration` budget を記録する。absolute
  `start + budget` を作らないため、shared substrate の positive-i64 全域が overflow で
  unbounded wait にならない。`connect` 前に `F_GETFL` と
  `F_SETFL(O_NONBLOCK)` の両方を checked にする。失敗時は
  mapped status を記録して candidate を close し、`connect` を呼ばず次の address へ進む。checked
  installation 後に immediate `connect` を 1 回発行する。zero は success、
  `EINPROGRESS`/`EAGAIN`/`EWOULDBLOCK` は wait に入り、他の全 errno は直ちに map する。どちらの
  immediate terminal result も budget が同時に exhaust していても勝つ。in-progress path は正の
  remaining duration を次の millisecond へ**切り上げ**、`i32::MAX` で saturate して `poll` する。
  既に deadline へ達していれば `AL_TIMEOUT`、`EINTR` は remainder を再計算し、`poll` の zero return は
  monotonic budget を再検査して時間が残れば再度 poll する。budget が exhaust した後は additional
  `poll` call の前に `AL_TIMEOUT` を返す。他の poll error は直ちに map する。実行中の poll から返った
  positive readiness/error event は budget が同時に exhaust していても勝ち、`SO_ERROR` で解決する。
  immediate/polled connect の成功後は毎回 `F_GETFL` と
  `F_SETFL(flags & !O_NONBLOCK)` の両方を checked にし、restoration failure は candidate を close して
  mapped failure を記録する。blocking mode の checked restoration 前に socket を公開しない。
  scheduler/kernel delay により requested instant より後に返り得るため、これは logical wait deadline
  であって不可能な end-to-end wall-clock guarantee ではない。DNS と複数 resolved address の合計は
  制限しない。
- `io_timeout_ns` は checked package-internal TCP row により blocking socket の receive/send timeout
  の両方へ設定。正の nanosecond 値はすべて次の microsecond へ**切り上げ**てから、normalized
  `timeval { tv_sec, tv_usec: 0..999999 }` に分割する。exact microsecond は exact のままで、`0` は
  この package の admitted range 外で既存の clear/no-timeout value のまま。kernel は option value
  より遅く return を schedule し得る。multi-read command 全体でなく、progress を待つ 1 blocking
  read/write を制限する。timeout は `Error.Io(core.Error.Timeout)` を返し、request の partial send
  または response の partial consume があり得るため client を close。construction は両 option
  installation 成功後だけ続行し、receive/send どちらの failure でも fresh unpublished connection を
  retire/close。send failure では receive option が既に変更済みでも同じ。

nonzero `getaddrinfo` result は pre-iteration の別 branch である。出荷済み runtime は
`EAI_NONAME`/`EAI_NODATA` を `AL_INVALID`、他の symbolic EAI value を
`encoded := AL_CODE.saturating_add(eai.saturating_abs())` へ map する。したがって package decode は前者を `Io(core.Error.Invalid)`、後者を
`Io(core.Error.Code(encoded - AL_CODE))` とする。connection output は null のまま、address entry/socket を
試さず、address-list owner は escape せず、transient NUL-terminated host/service storage は return 前に drop。

resolution 成功後の resolver order は observable。unsupported family、null address、zero address length は
last failure を変えず skip する。最初に成功した usable address が勝つ。usable address がなければ substrate は
`AL_INVALID` を返し、全 attempted candidate が失敗した場合は最後の socket、nonblocking-install、
connect/poll/`SO_ERROR`、または blocking-restoration status を返す。package-level I/O-timeout installation
はこの selection 後だけ行い、失敗時は selected unpublished connection を retire/close して、resolution の
再開や別 address の試行を行わず返す。後続 cleanup failure は選択済み error を置換しない。

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
- 全 command: `-<0..=max_response_bytes の CR/LF を除く payload bytes>\r\n`。

request length/count text は canonical unsigned ASCII。response bulk length は leading zero を含む
1 digit 以上の unsigned decimal と、null 用 exact `-1` を受理。magnitude より先に digit grammar を
検証し、valid magnitude が configured cap を超えれば i64 に収まらなくても `ResponseTooLarge`。
admitted magnitude は必ず収まる。RESP integer text は optional `+`/`-`、leading zero を含む 1 digit
以上を受理し、signed i64 に収まる必要がある。それ以外の admitted non-error control line は marker
と CRLF を除き 64 byte cap、exact cap は成功。識別可能な invalid byte または i64 overflow は直ちに
`Protocol`、未確定 control byte の 65 byte 目を要求した時点は `ResponseTooLarge`。CRLF は exact。
array、RESP3 type、nested reply、null array、alternate simple string、semantically wrong integer、
Simple Error payload 内の CR/LF、current native read 内の completed reply 後の byte は protocol failure。input は 1 byte
ずつでも、1 read 内に複数 response part が来てもよく、framing は TCP chunk boundary と独立。

Simple Error では CR は terminal CRLF の開始としてのみ認識し、payload にはならない。CR の次が LF
以外、または lone LF は read 間で split されても `Protocol`。payload が exact に
`max_response_bytes` に達した後、次 byte は required CR terminator を識別するためだけに inspect する。
別の non-CR/LF payload byte は `ResponseTooLarge`、LF は `Protocol`、CR は同じ exact-next-LF rule に従う。
したがって exact-cap frame は成功でき、次の admitted payload byte は `ResponseTooLarge`、malformed line
ending は UTF-8 classification 前に `Protocol` を選ぶ。

response decision order は固定。

1. negative native read status は `Io(core.Error)`。response byte 前の EOF は `Closed`、prefix 後は
   `Protocol`。いずれも client を retire。
2. 認識した `-` frame は NUL/invalid UTF-8 を許すが CR/LF を除く byte として、text classification より
   先に bound/framing する。最初の CR は terminal CRLF でなければならず、lone CR、lone LF、CR の次が
   LF 以外なら `Protocol`。terminator より前に inclusive payload cap を越えれば `ResponseTooLarge`、
   drain せず close。terminal CRLF と same-read trailing byte の検証後、完全な non-UTF-8 payload は
   `Decode`、それ以外は clone して `Server`。両方とも synchronization 完了結果なので connection は live。
3. current command が許さない reply marker を拒否し、canonical line grammar と semantic value を
   検証。失敗はすべて `Protocol` で close。
4. GET は観測した全 length byte を検証後、valid digit magnitude と比較。cap 超過は
   `ResponseTooLarge`、drain せず close。payload と終端 CRLF を exact に読む。完全な non-UTF-8
   payload は client を live のまま `Decode`、それ以外は owned clone を公開。
5. success/`Server`/`Decode` 公開前に、同じ native read 内で complete frame 後の trailing byte を
   `Protocol` として拒否し close。UTF-8 classification と final clone はこの check 後だけ行う。
   後から来る unsolicited byte は future server reply と区別不能。V1 は Redis の
   one-reply-per-command contract に依存し、pipeline はしない。

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

writer prerequisite は、macOS/BSD の failed-install/no-send から retry/success、2 個の overlapping shell
について両順序の success/failure、option clear を伴わない shell Drop、setting を破棄する connection
close を直接 own する。Linux/macOS subprocess owner は closed peer に対する direct slice/builder
writer overload、logger、`io.copy` route を覆い、signal termination でなく返却された `Error` を要求する。
file/standard-stream parity と partial/EINTR/timeout/zero-progress owner は、これら state-transition test と独立に保つ。

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
全 public operation と Drop は explicit existing `ProcessAbort` dependency を通じ、native I/O または
untrusted pointer access 前に hard-abort する。これは `Closed` producer ではない。safe consumer code は
この record を構築・変更できない。

command は resource を mutable borrow するため、第二 operation/task capture/replacement/move/Drop は
current request/reply と overlap 不能。network effect により全 operation は Impure で、resource rule と
独立に parallel closure 不適格。lock/shared client/global registry/background reader/callback/reversible な
post-publication connection-global mode transition はない。macOS/BSD では retained writer の monotone
SIGPIPE-ready transition が上記 failure/retry/close rule に従い、別 package operation と overlap しない。

reader/writer shell は `connect` が 1 回構築し、per-command shell allocation なしで再利用。request header
と decimal text は bounded operation storage。key/value byte は call-bounded `str` view
から write し非保持。receive chunk と framing state は response cap + fixed protocol overhead で bounded。
nonempty GET 成功または `Server` error は complete frame synchronization 後に ordinary owned string
result 1 個を allocate。empty `Some("")` または `Server("")` は final buffer allocation なしで canonical
`{null, 0}` owned string を公開する。V1 は consuming buffer-to-string freeze を加えないため、nonempty
result の peak storage に N-byte receive buffer と N-byte final owned copy の両方を含み得る。全 native
receive buffer は first read 前に実際の
`buffer.capacity()` と requested positive capacity を比較し、不一致は EOF に見せず OOM policy で
hard-abort。intermediate raw/source buffer は unpublished で、全 error 時に最初に Drop。OOM は言語の
既存 hard-abort contract。live client が保持する exact 4 allocations を除き、per-command scratch/result
allocation-count、zero-copy receive、zeroization、throughput、latency は約束しない。

## package・runtime・artifact・cache boundary

vendorable subtree は root `pkg.kv` と `pkg.kv.internal.resource` を所有する。internal module は
`std.process` を import し、impossible native state はすべて `process.abort()` を呼ぶ。extern の新設や
recoverable package error への変換ではなく、出荷済み keyed `ProcessAbort` row を選ぶ。source は既に
keyed な TCP connect/free/reader/writer、I/O read/write/free、buffer new/bytes/capacity/free row と、
active で source-reachable な unkeyed row 1 個について exact type-compatible extern declaration を使う。

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

`TcpConnSetIoTimeout` は最初に null connection を `AL_INVALID` で拒否し、その後
`1..=86400000000000` 外の timeout を `AL_INVALID` で拒否する。どちらも fd を読まず
`setsockopt` を呼ばず、range rejection 後もそれ以外は live な connection は usable。全 non-null call には、
pointer が 1 個の live/unfreed `TcpConn` を指し、caller が call 全体で exclusive logical access を持ち、entry 時に
その connection 由来の live reader/writer shell またはその shell を retain する別 value が 1 個もないという
unsafe precondition がある。dangling/concurrently aliased pointer または live derived shell は precondition 違反で
検出不能。read/write/別 configuration/reader-writer construction/free/Drop は call と overlap 不可。

この precondition の retainer は numeric fd equality でなく runtime provenance で分類する。target leaf は、
その `TcpConn` 由来の initialized reader（unbuffered/buffered）、derived writer、またはその writer を own する
`log.logger`。live value は active recursive Drop graph が target leaf を 1 個以上 reach する場合だけ target
retainer。現在の positive value grammar は direct leaf、acyclic user-struct field graph、任意に nest した
`Option`/`Result` の active `Some`/`Ok`/`Err` path、direct logger/retaining struct/別 sum/tagged carrier を
root とする active user-sum payload path、retaining Move struct を in-place construct した fixed struct array
の各 element を通る。その後 local/parameter/return/by-value call を move しても leaf provenance は不変。
ここで fixed-array element は retaining struct であって direct handle element ではなく、既存 struct-field と
fixed Move-struct-array rule を compose するだけで direct handle placement を widen しない。direct
reader/writer user-sum payload と direct reader/writer/logger の collection/fixed-array/tuple/box element
は formation を reject。`align(N)` のない retaining struct の `array<RetainingStruct>`、任意の retaining
struct の `slice<RetainingStruct>`、retaining sum に対応する admitted dynamic-array/slice shape は structural
type を form できる。direct `DynStructArray` はさらに dynamic-array/slice element、tuple element、または
builtin `Option`/`Result` payload にも置ける。この段落で admitted な shape は tuple wrapper を除きすべて
user-struct field に置け、その後 ordinary acyclic struct/tagged/sum carrier grammar を再帰できる。現行
`.to_array`、heap/region builder、
JSON decode、Move-element slice producer は live handle-
retaining value を作れないため producer-negative で
あり positive lifecycle case ではない。`None`、inactive `Result`/user-sum arm、moved/null leaf、別 connection
由来 shell だけの同じ carrier shape は target leaf zero。fd number 再利用は provenance を変えない。carrier
は multiple/mixed-provenance leaf を reach でき、active target count が exact zero の場合だけ compatible。

admitted input では上記 normalized positive-timeout `timeval` を構築し、最初に `SO_RCVTIMEO` を設定。
失敗すれば `SO_SNDTIMEO` を試さず fixed errno-mapped status を返す。成功時だけ `SO_SNDTIMEO` を設定して
その status を返し、両方成功した場合だけ zero。entry 時の receive/send option state を `R0`/`S0`、requested
state を `T` とすると、receive failure は `{R0,S0}`、send failure は `{T,S0}`、success は `{T,T}` を残す。
どちらかの option failure 後も connection owner は caller にあるが、compatible caller はそれを retire し、
read/write/configuration/reader-or-writer construction/retry をせず ordinary free/Drop path へ exact 1 回渡す。
zero-derived-shell entry precondition により、その close と順序付ける shell/retaining wrapper は存在しない。
success は usability を保ち、その後 reader/writer を構築できるが、後の timeout call は全 derived shell とそれを
retain する value を Drop して同じ zero-shell entry state を復元した後だけ compatible。row 自体は
allocate/retain/rollback/close/consume しない。`pkg.kv` は両 entry state が clear の fresh、exclusive、
unpublished connection だけで、どちらの shell も構築する前に call し、nonzero option result なら resolution を
再開せず別 address を試さず直ちに close。parameterized structural owner は canonical `DropPlan` から
recursive cleanup node を derive し、exhaustive `DropPlan`-node match が future cleanup-node variant の分類を
必須にする。retaining struct の fixed array は `DropPlan` node を追加せず `ty_is_move` と element struct plan を
compose するため、別の explicit composition owner を持つ。source formation と no-live-producer negative が sema の
private storage-provenance analysis を export せず admitted/excluded carrier edge を own する。owner は
direct/buffered leaf、local/moved/call-transferred placement、
active/inactive/moved-out state、target/other/mixed provenance、zero/one/multiple target leaf を cross。
nonzero target count は unsafe row を invoke せず compatible call set から exclude。各 positive carrier class
について、zero で configure、leaf を construct して carrier へ move、supported なら move-out 後 Drop、
または smallest owning carrier を recursive Drop、zero を再観測、reconfigure する。fixed-struct-array case は
recursive-Drop branch。formation/producer negative は excluded collection/box/tuple/sum-payload/materializer/
builder/decode/Move-slice edge をすべて pin。direct-runtime half は両 option state を pre-arm し、exact
`timeval`、option order/call count/returned status、`{R0,S0}`/`{T,S0}`/`{T,T}` post-state、exclusive-call
precondition、overlap 中/option failure 後の reader/writer constructor call zero、caller retirement、後続
close/Drop を固定する。
mandatory base export、
source-reachable compatible extern、collision-reserved unkeyed identity である。既存 ABI
shape を再利用するため、activation は exact base/maximum count を 347/355 から 348/356 に変え、keyed
count は 330 のまま。unkeyed count は 18、そのうち source-reachable は 13。A123 は当時次の
unreserved shape であり、後続の `pkg.csv` design が一度予約し、その実装が activation した。
LLVM/Rust definition は既存の A04/default-C-calling-convention contract を
使い、curated function/return/parameter attribute はない。

通常 package source は全 native status を明示的に decode する。常に `core.Error.Code(status)` を構築する
`error(status)` は使用できない。i32-status row では zero が success、`1`、`2`、`3`、`4` は順に
`NotFound`、`Invalid`、`Denied`、`Timeout`、`5..=i32::MAX` は `Code(status - 5)`。負の i32 status は
impossible ABI result として hard-abort。reader result は正の byte 数、zero EOF、または負で encode
された status。source は parser state を変更する前に分類する。`i64::MIN`、
`-(i32::MAX as i64)` より小さい全 value、requested buffer capacity を超える全 positive count は buffer
view を inspect する前に abort。admitted negative、zero、in-cap positive だけが、その後 typed slice を
構築せず raw `{ptr, len}` header を読む。admitted negative は `len == 0` を要求し、checked-negate して
明示的に i32 へ narrow し、同じ status table を適用。zero も `len == 0` を要求して EOF を意味する。
両 empty case は null pointer と runtime-owned non-null empty pointer の双方を許し、dereference しない。
in-cap positive count は `len == count` と non-null pointer を要求して
から typed slice construction/parsing へ進む。negative/zero/positive の length mismatch、または positive
count と null pointer は abort。runtime が nonempty view pointer provenance を
所有し、matching non-null pointer の forged value は unsafe native ABI contract の外で検出不能。
`align_rt_tcp_connect` では zero は non-null output connection を要求し、
nonzero は null を要求する。矛盾した status/pointer product は ownership change 前に hard-abort。
全 category sentinel、`Code(0)`、representative positive code、signed-width boundary、
byte-count x view-length x pointer-representation product、malformed product に独立 owner を置く。
各 hard-abort branch は explicit `std.process`
dependency を使う通常 package source であり、後続の parsing/publication/ownership change より前に exercise する。

既存 `TcpConnWriter`/`IoWriterWrite`/`IoWriterWriteBuilder`/`IoWriterFree` の identity、declaration、
attribute、count は不変。`IoWriterWriteBuilder` は引き続き `IoWriterWrite` へ delegate するため、
source-visible builder overload も同じ sink policy に到達する。private runtime `Writer` に socket sink
kind を加え、`align_rt_tcp_conn_writer` だけが設定する。この kind
からの nonempty write は上記 SIGPIPE-safe send policy を使い、成功した `SO_NOSIGPIPE` だけを cache。
他 constructor は byte-identical な fd path を維持。option は monotone な per-socket setting で、overlap
する shell は各々試行でき、各 shell は自身の成功後だけ送信し、失敗した shell は retryable。shell Drop
は restore/clear しない。connection-derived writer は unbuffered/non-owning のままなので、free path は
hidden write を行わず socket を close せず、connection close が fd と option を破棄する。

source extern compatibility は各 registry row の exact LLVM type/attribute/symbol/runtime definition を
再利用し、第二 physical symbol を宣言せず collision check を bypass しない。compiler は
`align_rt_tcp_conn_set_io_timeout` を fixed physical ABI symbol として認識する一方、package は language
builtin、HIR/MIR variant、call-spelling capability selector、reflection table、static artifact、schema input、
environment option を加えない。`docs/impl/19-hir-validation-ledger.md` は不変。
`docs/impl/20-runtime-abi-ledger.md` は active な exact 1-row delta を記録し、ABI を変えない
既存 writer hardening を pin する。

whole-program compilation は通常 package body を見る。per-unit compilation は resource、
`ClientOptions`、`SetCondition`、`SetOptions`、`Error`、public signature 4 個を serialize し、producer
object は resource Drop thunk と既存 native dependency を保持。現 capability collection は module-wide
なので、どの operation を使っても root/internal TCP/I/O/buffer と keyed `ProcessAbort` set 全体を保持し、
call-spelling selector で変わらない。通常 per-unit cache identity では、unit の source byte に対する全 edit がその unit の
frontend key を miss する。semantic private-body edit はその unit の structural object も miss し、
final link は変更済み object を消費する一方、consumer は private implementation hash でなく dependency
interface hash に依存するため unchanged frontend/object が hit する。exported-surface edit は interface
hash を変え、transitive interface set にそれを含む全 reverse dependency の frontend/object を miss
させる。span-erased semantic MIR が不変の source-only edit は structural object に再 hit できる。
exact revert は以前の key に再 hit でき、unrelated unit は一貫して hit。whole-program mode は通常の
complete-source identity を維持する。endpoint、resolver result、response、clock、ambient/runtime-inspected
source file、runtime inspection は artifact identity に入らず、vendored package source 自体は explicit
input である。

4 つの named `pkg.kv` function は whole-program/per-unit compilation の既存 function-value behavior に
従い、local、control-joined、fn-typed parameter、neutral-named struct-field value を形成できる。
indirect call は `connect` の 3 `ByValue` argument、`get`/`delete` の `BorrowMut` + `ByValue`
argument、`set` の `BorrowMut` + 3 `ByValue` argument を維持する。MIR は
`call_indirect_with_cleanup` を使い、LLVM emission は成功し、全 result は `DynamicBit` return
cleanup を使う。これは package 例外ではなく通常の language parity である。`pkg.kv` のない
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

shared timeout substrate は最初の distinct prerequisite capability として着地済み。positive connect
deadline を enforceable にし、nonblocking connection の公開を防ぎ、ABI identity を変えずに既に出荷済みの
`std.http`/`std.net` consumer の shared connect/I/O quantization を修正する。deterministic resolver、
transition、process、HTTP consumer owner を含めると、この prerequisite もおよそ 1,000 changed
hand-written lines を超え得る。shared quantization proof の重複を避け、別々に land した consumer が
互換性のない timeout rule へ drift するのを防ぐため、1 boundary とする。generic TCP-writer
hardening は、閉じた signal-safety failure domain を持つ第二の independently useful prerequisite として着地済みで、
同様に public signature/ABI identity を変更しない。raw-syscall classification、両 platform state
machine、transitive-route subprocess owner、macOS execution owner、同期 status mirror を数えると、
およそ 1,000 changed hand-written lines を超え得る。strict producer/consumer half に分けて dormant
sink policy と signal-safety proof の重複を作らないため、1 boundary とする。timeout row と
client/resource/parser/3 command は 1 本の strict producer-to-consumer chain。dormant row、parser-only、
connection-only package PR は stable public consumer を残さず、command 分割は同じ
synchronization/poisoning/fake-server/capability/Drop proof を重複させる。adversarial owner matrix を含め
package capability はおよそ 1,000 changed hand-written lines を超え得るが、全 reply kind が publication
前に同じ state machine で閉じるため、1 boundary の方が integration risk が低い。

| axis | 必須 closure | owner evidence |
|---|---|---|
| public formation・identity | exact module/resource/record/sum definition、field/discriminator order、4 signature、qualification、visibility、direct/imported call、generic consumer-wrapper monomorphization、local/control-joined/fn-typed-parameter/neutral-named-struct-field function value、exact indirect argument mode、`DynamicBit` return cleanup、whole/per-unit interface parity。 | public-source extraction、positive consumer compile/run、near-spelling/type/arity negative、monomorphic/generic-wrapper parity、interface round-trip、generic alias control、4 function 全ての `call_indirect_with_cleanup` MIR と LLVM-emission parity。 |
| shared timeout substrate | absolute deadline を作らない positive-i64 全域の monotonic start-plus-budget arithmetic。per-address ceil-ns-to-ms `poll` conversion、zero-result recheck、exhaustion 後の poll なし、immediate/readiness precedence、checked nonblocking install/blocking restore、candidate close/continuation。`process.command` は同じ start/budget と ceil conversion を使い、従来の timeout-wins checkpoint order を維持。`std.net`/`std.http` が共有する positive socket timeout は ns を normalized `timeval` microsecond へ ceil。全 zero-timeout behavior は不変。 | direct exact/next/maximum ns/us/ms/chunk/deadline owner。immediate/polled success の `F_GETFL`/`F_SETFL` install/restore failpoint、`EINPROGRESS`/`EAGAIN`/`EWOULDBLOCK` と他の immediate errno、early zero-result と exhausted/no-call poll、EINTR、readiness-at-deadline、mixed-address、no-publication/blocking-mode、command capture/reap、HTTP plain/TLS/pool-rearm probe。 |
| connect・option admission | host/U+0000、port、ordered option bound 3 個、exact/next boundary、complete validation 前 side effect なし、iteration 前の nonzero resolver failure、その後の ordered/skipped/empty/first-success/last-failure address semantics、address retry なしの timeout application、全 native status/pointer product、reader-then-writer-then-state construction、IPv4/IPv6、Redis byte なし。 | symbolic `EAI_NONAME`/`EAI_NODATA`/transient/other owner が mapped package error、null output、socket attempt なし、transient cleanup を pin。その後 instrumented connect/runtime counter と loopback listener は socket failure、nonblocking GET/SET failure、immediate errno、poll error/timeout、`getsockopt` failure/nonzero `SO_ERROR`、blocking-restore GET/SET failure、各 failure 後の skipped entry、later success を parameterize し、各 vector の last attempted status と close count を固定。malformed status/pointer、retained-allocation、allocation/cleanup failpoint が残る product を閉じる。 |
| request byte | exact GET/SET-product/DEL array、uppercase token、canonical decimal、value-sized request copy なしの 512-MiB admission、embedded NUL/CR/LF、first write 前の全 arithmetic。 | independent semantic-to-byte golden、boundary/mutation table、fragmented writer、partial-write owner。 |
| reply framing | 全 marker、leading zero/integer sign を含む official bulk/integer grammar、64-byte control cap、exact CRLF、fragmented/coalesced read、null/empty/exact/next cap、error cap、trailing byte、全 ordinal の EOF、quadratic scan なし。Simple Error payload は CR/LF を除外し、その exact-cap/next-payload/lone-or-split-CR/LF precedence を UTF-8 classification 前に固定。 | deterministic scripted TCP peer による one-byte/all-split/multi-part product、independent byte-to-semantic golden、error payload length x `{next payload, CRLF, CR-non-LF, lone LF}` x fragmentation/trailing product、comparison counter。 |
| GET・error semantics | bulk/null/empty ownership、exact byte、NUL/invalid UTF-8 を許す一方 CR/LF を除く bounded error byte、framing/trailing validation 後だけ UTF-8 classification、Decode reuse、wrong kind/oversized declaration close。 | official vector + empty/nonempty valid/invalid UTF-8、lone/split CR/LF、exact/next cap、empty `Server`/GET allocation/Drop、lifetime escape、subsequent command、no-drain probe。 |
| SET semantics | Always/NX/XX x None/Some、condition-before-PX wire order、ceil-ns conversion、persistent TTL removal、+OK/null result matrix、server error、unexpected status。 | scripted byte + collision/expiry/refresh-without-resurrection/exact-next duration/wrong-reply product の Redis-compatible state model。 |
| DEL semantics | one-key request、0/1 の全 official signed/leading-zero spelling、server error、他の全 value/type。 | false/true と sign/leading-zero/negative/two/overflow/type mutation matrix、reuse/close check。 |
| error・native status・poison state | I/O 前 Invalid。bounded UTF-8 Server と complete non-UTF-8 Decode は reusable。exact `0/1/2/3/4/>=5` status decode。reader `{invalid negative, admitted negative, zero, admitted positive, oversized positive}` x view length x `{null, non-null}` pointer representation と checked i32 narrowing、typed-slice construction 前の raw-header validation。invalid-negative/oversized-positive は header inspection 前に abort。Io/too-large/protocol/truncation/partial-write は close。selected terminal error を cleanup より保持。以後の全 call は zero I/O の Closed。malformed private resource state は Closed ではなく native I/O/untrusted access 前に `process.abort`。全 impossible native product は parser/publication/ownership change 前に `process.abort` へ到達。 | error-producer x command x before/during/after-frame x reuse table。early-abort/no-header inspection、両 empty pointer form、positive-null abort を含む全 category/representative code/width/count/length/pointer/malformed product、operation/Drop x one-field-at-a-time malformed resource state が zero native call の `ProcessAbort` を pin。native call counter、explicit `ProcessAbort` IR/capability retention、no-import negative、selected-error/cleanup-failure probe。 |
| ownership・cleanup | resource formation、move-in/out/return/replacement、if/match/else/?/map_err/branch/loop/early return、source nulling、state/socket/wrapper/scratch/result の Drop once、malformed state は untrusted access 前に abort。 | resource/drop counter、allocation parity、parameterized control-flow owner、state semantic-to-byte/byte-to-semantic golden、operation/Drop x malformed-field abort product。 |
| ABI・effect・capability・cache | fixed-symbol `TcpConnSetIoTimeout` の null-then-range/no-side-effect validation と atomic activation。全 non-null caller は live/unfreed/exclusive connection を渡し、entry 時にその connection 由来の live reader/writer shell またはそれを retain する value が zero で、read/write/configuration/reader-or-writer construction/free/Drop overlap を排除。target-connection count は active recursive Drop graph が reach する initialized reader/writer/logger-owned-writer leaf 数で、fd-number equality と独立。pre-armed receive/send entry/post-state product、option failure 後は caller retirement、後続 read/write/configuration/reader-or-writer construction/retry 禁止、1 回の free/Drop が必須。validation rejection はそれ以外 live な connection を維持し、success も usability を保持。後の timeout call 前に success 後の derived shell/retainer をすべて Drop。default-C A04/no-curated-attribute identity。既存 connection-writer の sink provenance と slice/builder overload を通る partial/EINTR/zero/EPIPE/timeout mapping。writer ABI/count を変えない Linux/macOS SIGPIPE state/transitive route。新 ABI shape/language builtin/HIR/MIR row/selector なし。Impure operation、module-wide TCP/I/O/buffer/`ProcessAbort` retention、package absence、exact own-source/public-interface/private-dependency cache outcome。 | exact registry/golden/base-export/type/attribute/collision/source-reuse、null x range、live/dangling/aliased/overlap と zero-derived-shell entry precondition、exact-timeval、pre-armed `{R0,S0}` x receive-fail/no-send、send-fail/`{T,S0}`、both-success/`{T,T}` owner。source-derived `DropPlan` structural owner 1 個と exhaustive `DropPlan`-node tripwire、fixed retaining-struct array の explicit `ty_is_move`/element-plan composition、source formation/no-live-producer owner が direct/buffered reader、direct writer、logger-owned writer、recursive struct field、nested active `Option`/`Result`、logger/struct/sum/tagged carrier を root とする active user-sum path、source-produced fixed struct-array element を walk し、local/moved/call-transferred placement、active/inactive/moved-out state、target/other/mixed provenance、zero/one/multiple target leaf を cross。exact zero だけ compatible。direct handle collection/box/tuple と direct reader/writer user-sum payload は formation negative。nameable dynamic-array/slice retaining-struct/sum shape、admitted non-tuple shape の user-struct-field closure、direct `DynStructArray` に許容される dynamic-array/slice element、tuple、builtin `Option`/`Result` edge は explicit no-live-producer owner を維持。range-rejection retry と option-failure retry 禁止、overlap 中/failure 後の constructor call zero、各 positive carrier の configure-construct-move-into-move-out-where-supported-or-recursive-Drop-reconfigure cycle、retirement、package close/no-address-retry、compatible-caller free/Drop。failed-install/retry/overlap/Drop と file/std owner。各 native subprocess owner は最初に nonempty live-socket send を完了して macOS `SO_NOSIGPIPE` を install/cache し、その後 local `SHUT_WR` に入り、direct slice/builder/logger/`io.copy` で signal termination なしの exact `EPIPE` を要求。package whole/per-unit IR/link run、effect check、exact `ProcessAbort` dependency、6-field resource mutation、no-package negative、private/public/add/remove/edit/revert cache twin。 |

post-open macOS execution により native-SIGPIPE-owner axis を reopen する。peer-close だけの AF_UNIX
send は `EINVAL` を返し得て、local `SHUT_WR` 後だけに `SO_NOSIGPIPE` を install すると send 前に
failure し得る。どちらも signal suppression を証明できない。このため各 native route owner は最初に
live-socket send 成功と shell-local readiness を確立し、その後 `SHUT_WR` に入り、次の nonempty route
write に exact `EPIPE` を要求する。public contract、runtime ABI、production state machine は変えず、
owner evidence だけを強化する。

author-side implementation consistency pass は contract-only mismatch 1 件を発見した。accepted design が
この 4 function value の rejection を誤って予測していた。修正は既存 language behavior を記録するもので、
4 つの direct signature や runtime behavior を変えない。fresh focused ledger review は final
implementation review 前に CLEAN を返した。

## source of truth と author consistency pass

`../kv.md` の英語 ledger、`docs/impl/pkg-design/ja/kv.md`、`draft.md`、`docs/language-spec.md`、
`docs/design-notes.md`、`docs/history.md`、`docs/open-questions.md`、`docs/impl/07-roadmap.md`、
`net`/`http`/`process` の英語・日本語 `std-design` 文書、`docs/impl/20-runtime-abi-ledger.md`、`HANDOFF.md` は
一致しなければならない。HIR ledger は不変。
exact active 1-row delta を越える ABI change、または public writer-surface change はこの design を reopen する。

exact range
`ad5d6969194c26b4cbd8c7521d15ed6ac05f49f7...d85efdb94cf81036e7555d4a1621c5356d602be3`
の fresh full review は P0–P3 finding なしでこの contract を accept した。`docs/open-questions.md` は
Settled として記録し、`docs/history.md` は decision を記録する。implementation は下記 prerequisite order に
従い、残る runtime row は joint package boundary で activate した。

5 回目の finding-ledger repair 後、5 回目の author-side ledger-to-prose および closure-matrix consistency
pass は、another fresh complete review 前の 2026-09-02 に完了:

- 全 public argument/result に exact type、evaluation order、default、ownership、lifetime、allocation、
  cleanup、error、effect rule が 1 個ずつある。
- command、condition、expiry、response marker、verification state、option state、field presence、row order、
  discriminator、unavailable-result product が exhaustive。
- host/key/value/error text の UTF-8、embedded-NUL、Simple Error CR/LF exclusion と exact terminator
  precedence、boundary validation、pre-side-effect semantics が固定。
- multi-invalid call の state/host/port/option/key/value/condition/expiry/wire と native/reply/error precedence が deterministic。
- shared connect、HTTP socket、command-capture timeout consumer の start/budget arithmetic、ceil conversion、
  zero-result/exhaustion behavior、それぞれの terminal-event precedence が固定。
- native status、reader count x view length x pointer representation、connect status x output、resolver EAI
  category、receive/send option-call と entry/post-state product が exhaustive。impossible native product は
  parsing/publication/後続 ownership change 前に explicit existing `std.process`/`ProcessAbort` dependency
  へ到達し、全 malformed private-resource operation/Drop は native I/O/untrusted access 前に同じ dependency
  へ到達。
- endpoint/credential/database/retry/clock/resolver result/configuration/artifact/runtime-inspected source input は
  ambient でなく、vendored package file は explicit compiler input のまま。
- canonical RESP scalar/tag/sequence order/malformed rejection、independent semantic-to-byte と byte-to-semantic golden が固定。
- resource record、RESP state machine、source-reachable timeout row の全 state/tag/reserved/pointer/length
  product、zero-derived-shell entry state、constructor call を含む live/exclusive overlap exclusion、
  fixed retaining-struct array、target/other/mixed provenance、zero/one/multiple leaf を含む complete active
  recursive reader/writer/logger carrier graph、各 construct-move-into-move-out-where-supported-or-recursive-
  Drop-reconfigure cycle、exhaustive `DropPlan`-node tripwire、explicit fixed-struct-array composition、全
  formation/no-live-producer owner、failed-second-option 後の
  no-operation/construction/retry retirement、error preservation、Drop order が固定。
- exact existing producer-owned runtime row が reflection/artifact I/O なしで native state を供給し、
  slice/builder writer overload は同じ hardened sink へ合流。
- example は accepted syntax を使い declaration と positional call を分離。
- acceptance owner が全 ledger invariant を覆い、約束していない benchmark を gate にしない。

official protocol/command reference: Redis
[RESP](https://redis.io/docs/latest/develop/reference/protocol-spec/)、
[GET](https://redis.io/docs/latest/commands/get/)、
[SET](https://redis.io/docs/latest/commands/set/)、
[DEL](https://redis.io/docs/latest/commands/del/)。

## design-review finding-to-fix ledger

`ad5d6969194c26b4cbd8c7521d15ed6ac05f49f7...45a5cea85579c2dd5170cd6e41958f114bcad3c3`
を exact base とする独立 review は P1 1 件、P2 10 件、P3 1 件を返した。この ledger が authoritative
repair を記録する。fresh complete review が design を accept する前に、全 row を日本語 mirror と
synchronized summary へ伝播しなければならない。

| finding | authoritative correction・closure owner |
|---|---|
| P1 unchecked connect fd mode | independently useful な shared-timeout prerequisite を加える。checked `F_GETFL`/`F_SETFL` install/restore、failure 時の close-and-continue、checked blocking restoration 前の publication 禁止。direct immediate/polled failpoint が全 transition を own。 |
| P2 timeout quantization/precedence | positive-i64 全域に monotonic start-plus-budget arithmetic を使う。positive connect と `process.command` wait は ns を millisecond へ ceil し early zero を recheck。connect は immediate/readiness event を優先し、command は従来の timeout-wins checkpoint を維持。positive `std.net`/`std.http` I/O option は ns を normalized microsecond へ ceil。exact/next/maximum と readiness-at-deadline owner が全 shared consumer を pin。 |
| P2 multi-address selection | nonzero resolver failure を iteration 前に map し、その後 usable entry を順に試し first success を返す。successful resolution で usable entry がなければ substrate は `AL_INVALID` を返し package source が `Io(core.Error.Invalid)` へ map、attempted failure があれば最後の socket/connect/mode failure。post-selection timeout configuration は resolution を restart しない。symbolic EAI/mixed-failure owner が distinct branch、ordering、cleanup、native/package error layer を pin。 |
| P2 native status decode | package source は fixed `0/1/2/3/4/>=5` table を実装し、checked i32 narrowing と typed-slice construction 前の raw-header validation を伴う invalid-negative/admitted-negative/zero/positive reader count x view-length x pointer-representation product を exhaust し、connect-status/output product を検査し、全 impossible ABI result に explicit `std.process`/`ProcessAbort` dependency を使う。category/code/width/product と whole/per-unit capability owner が閉じる。 |
| P2 new-row malformed input | `TcpConnSetIoTimeout` は最初に null、その後 inclusive timeout range を検証し、fd access 前に `AL_INVALID`。全 non-null caller は live derived reader/writer shell/retaining value が zero で、read/write/configuration/reader-or-writer construction/free/Drop overlap のない live/unfreed/exclusive connection を渡す。direct runtime evidence は null x range、entry-shell/overlap/provenance precondition、exact `timeval`、pre-armed `{R0,S0}` から receive-fail/no-send/`{R0,S0}`、send-fail/`{T,S0}`、both-success/`{T,T}` の transition を覆う。range rejection は live connection を維持し、admitted-input option failure は後続 read/write/configuration/reader-or-writer construction/retry 禁止、caller retirement、後続 1 回の free/Drop を必須にする。package は publication/address retry なしで close。 |
| P2 RESP error grammar | NUL/invalid UTF-8 を許す一方 CR/LF を除く bounded error payload を frame し、exact terminal CRLF と same-read trailing byte を検証してから UTF-8 `Server` または non-UTF-8 reusable `Decode` を選ぶ。empty/invalid UTF-8、exact/next-cap、lone/split CR/LF vector が distinction/precedence を own。 |
| P2 empty owned allocation | empty GET/`Server` result は final buffer なしの canonical `{null, 0}`。nonempty result だけが 1 個を所有。empty/nonempty allocation/Drop counter が rule を own。 |
| P2 SIGPIPE state evidence | failed-install/no-send→retry、overlapping-shell order、Drop/no-clear、connection-close、applicable platform 上の direct slice/builder/logger/`io.copy` closed-peer owner を加える。既存 `IoWriterWriteBuilder` identity は不変で、hardened write row へ delegate する。 |
| P2 physical-symbol recognition | exact ABI compatibility/collision/reachability のため compiler registry recognition を維持する一方、language builtin、HIR/MIR operation、ABI shape、call-spelling selector を加えない。wrong-type/collision/source-reuse owner が pin。 |
| P2 resource interface identity | non-generic `pkg.kv.client` の serialized field 6 個すべてを pin。exact generated thunk と `b"align-res-drop-1"` を含み、interface owner で各 field を独立に mutate。 |
| P2 cache identity | own-source byte edit は frontend を miss。semantic private-body edit は自身の object/final link を miss する一方 consumer frontend/object は hit。public interface edit は transitive reverse dependency を miss。source-only semantic no-op は object-hit 可。exact edit/revert cache twin が各 scope を own。 |
| P3 package inventory | source 出荷前、normative summary は implemented vendorable subtree 4 個と accepted but unimplemented design `pkg.kv` を分けて記載。 |

fresh full review
`ad5d6969194c26b4cbd8c7521d15ed6ac05f49f7...f300756f86c0f28c59556a15d4c64ff918ed590a`
は P1 1 件、P2 3 件を返した。この second repair は raw-view と source-reachable-native-boundary matrix axis を reopen:

| finding | authoritative correction・closure owner |
|---|---|
| P1 reader count/view pointer | 最初に count を classify し、invalid-negative/oversized-positive は raw-header inspection 前に abort。その後 typed-slice construction 前に raw `{ptr,len}` を inspect。admitted negative/zero は zero length を要求し null/non-null empty pointer を許して dereference しない。admitted positive は exact length/non-null pointer を要求。count x length x pointer owner は early-abort/no-header、両 empty form、positive-null abort を含む。 |
| P2 timeout compatible-caller lifecycle | 全 non-null compatible caller は live derived reader/writer shell/retaining value が zero で、read/write/configuration/reader-or-writer construction/free/Drop overlap のない live/unfreed/exclusive connection を渡す。pre-armed `{R0,S0}` は `{R0,S0}`/`{T,S0}`/`{T,T}` へ transition。どちらかの option failure は retirement、後続 read/write/configuration/reader-or-writer construction/retry 禁止、後続 1 回の free/Drop が必須で、validation rejection と success は usability を保持。direct/package owner は許可される range-rejection retry と禁止される option-failure retry を区別し、overlap 中/後続 constructor call zero、publication 禁止、close once を固定。success-construct-Drop-reconfigure owner は次の call 前に zero-shell entry state を復元。 |
| P2 resolver failure partition | nonzero `getaddrinfo` result は iteration 前に name/no-data を `Io(Invalid)`、他 symbolic EAI を `Io(Code)` へ map。output は null、transient storage は drop、socket attempt なし。symbolic EAI owner が successful empty/skipped list と区別。 |
| P2 Simple Error CR/LF | payload は CR/LF 以外の任意 byte を許し、CRLF だけが terminator。exact/next-cap x lone/split CR/LF x fragmentation/trailing owner が UTF-8 classification 前の `ResponseTooLarge`/`Protocol` を pin。 |

P1 が native validation order を変更し、P2 timeout finding が general source-reachable lifecycle を完成させたため、
complete revised diff はもう 1 回の fresh full review を必要とした。exact review
`ad5d6969194c26b4cbd8c7521d15ed6ac05f49f7...978e2d457029c1276df17b3d47f11854d5227109`
は P3 consistency finding 2 件を返した。この third repair は
post-failure-construction-exclusion と malformed-state-error-partition axis を reopen:

| finding | authoritative correction・closure owner |
|---|---|
| P3 timeout action-list synchronization | すべての source/summary に canonical lifecycle 1 個を適用する。call 中は read/write/configuration/reader-or-writer construction/free/Drop overlap を禁止。どちらかの option failure 後は read/write/configuration/reader-or-writer construction/retry を行わず、free/Drop を exact 1 回行う。compatible-caller owner は overlap attempt 中と failure 後の reader/writer constructor call zero を assert。 |
| P3 malformed-state `Closed` contradiction | exact public `Closed` producer set は変更しない。malformed private resource record は recoverable package error でなく internal invariant violation。全 operation と Drop は native I/O/untrusted pointer access 前に explicit existing `ProcessAbort` dependency へ到達。operation/Drop x one-field-at-a-time record corruption owner は abort と native call zero を assert。 |

malformed-state correction は internal safety strategy を変更し、lifecycle correction は source-reachable
dangling-shell path を閉じたため、complete repair はもう 1 回の fresh full review を受けた。exact review
`ad5d6969194c26b4cbd8c7521d15ed6ac05f49f7...7148d4414355365a6c2cbb77d169b1ac8181c5bf`
は P2 finding 1 件を返した。この fourth repair は derived-shell-entry-state matrix axis を reopen:

| finding | authoritative correction・closure owner |
|---|---|
| P2 pre-existing derived shell | non-null compatible caller は entry 時、その connection 由来の live reader/writer shell と、その shell を retain する value が zero。idle でも live direct/moved/buffered shell、logger、その他 retainer は unsafe-precondition 違反。call 中 constructor なし。success 後は shell を構築できるが、後の timeout call 前に全 shell/retainer を Drop。option failure は zero shell から始まり、construction を禁止し、shell cleanup order なしで connection を 1 回 close。entry-state owner は never-constructed zero、constructed-then-dropped zero、live direct/buffered reader、live direct/logger-retained writer、moved/call-transferred reader/writer を区別し、package sequence は timeout-before-reader-before-writer を pin。 |

この finding は、同じ source-reachable dangling-shell class の残っていた pre-entry half を閉じる。
fresh full review
`ad5d6969194c26b4cbd8c7521d15ed6ac05f49f7...70ddb527dadaf095792b4bd9fe57d764a7380329`
は P3 finding 1 件を返した。この fifth repair は recursive-derived-shell-carrier matrix axis を reopen:

| finding | authoritative correction・closure owner |
|---|---|
| P3 recursive shell-carrier owner graph | target retainer を complete active recursive Drop graph の runtime provenance で定義。direct/buffered reader、direct writer、logger-owned writer leaf は local/call、recursive struct field、nested active `Option`/`Result`、admitted user-sum path、retaining struct の source-constructible fixed array を移動できる。canonical `DropPlan` node から source-backed parameterized owner 1 個を derive し、exhaustive `DropPlan`-node tripwire を追加。fixed retaining-struct array は explicit `ty_is_move`/element-plan composition で別に own し、admitted/excluded storage edge は source formation/no-live-producer test で own する。active/inactive/moved-out state、target/other/mixed provenance、zero/one/multiple leaf を cross し、nonzero target count は unsafe row を invoke せず incompatible。各 positive class は configure → construct → carrier へ move → supported なら move-out 後 Drop または recursive Drop → zero-count reconfigure。direct handle collection/box/tuple と direct reader/writer sum payload は formation negative。nameable dynamic-array/slice retaining-struct/sum shape、admitted non-tuple shape の user-struct-field closure、direct `DynStructArray` に許容される dynamic-array/slice element、tuple、builtin `Option`/`Result` edge は explicit materializer/builder/decode/slice no-live-producer owner を維持。 |

この finding は、同じ source-reachable dangling-shell class の recursively reachable な未閉包 half を閉じる。
exact range
`ad5d6969194c26b4cbd8c7521d15ed6ac05f49f7...d85efdb94cf81036e7555d4a1621c5356d602be3`
の fresh full review は P0–P3 finding なしで CLEAN を返した。この exact contract は accepted となった。
その review checkpoint 時点では shared timeout と generic writer-hardening prerequisite が implemented、
package implementation が記録済み order のまま pending だった。その後 package source と runtime row は同時に
activate した。

その後の acceptance-status audit は summary-only conflation の P3 1 件を返した。impossible native product と
malformed private record を pre-I/O guarantee 1 個にまとめていた。修正は public-contract ledger row と safety
strategy を変更しない。impossible native product は parsing/publication/ownership change 前に abort し、
malformed private operation/Drop は native I/O/untrusted pointer access 前に abort する。focused
finding-to-fix inspection は CLEAN を返した。
