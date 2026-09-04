# pkg — ws

> 英語版 `../ws.md` が権威であり、このファイルは同期された日本語 mirror である。
>
> **状態:** IMPLEMENTED 2026-09-04。implementation capability は accepted contract を拡張せず、
> owner test と runtime row と共にこの surface を有効化した。

## 権威 public-contract ledger

V1 は既存 `pkg.web` route table に統合する RFC 6455 HTTP/1.1 server である。standalone
listener、client、raw frame API、extension framework、compression、background task は含まない。
`pkg.web` が routing/middleware/request view/accept loop/`SO_REUSEPORT` を、`std.http` が
protocol-neutral Upgrade transport を、`pkg.ws` だけが handshake、内部 SHA-1、frame grammar、
mask、message assembly、自動 control reply、close-code policy を所有する。

| Public surface | 入力・default・validation・評価 | 結果・error・順序・effect | ownership・lifetime・allocation・cleanup | owner・ABI・identity | acceptance owner |
|---|---|---|---|---|---|
| `http_headers.count(name: str) -> i64` | receiver と name を一度評価。name は nonempty ASCII token、NUL 不可。不正 name は scan 前 abort。native row は null/misaligned ctx と negative/address-space-unrepresentable length/positive-length null name を reference/slice formation 前、invalid token byte を table scan 前に hard-abort。dangling nonnull pointer は検出可能な ABI contract 外。 | Pure/zero-allocation。物理 header row 数を field name ASCII-case-insensitive で返し、comma member は数えない。結果は `0..=HTTP_MAX_HEADERS` で malformed sentinel はない。 | Copy。call 中だけ request table を borrow。 | `std.http`; `HttpHeadersCount`; 既存 A37。method/key は interface/object/link identity に入る。 | zero/one/repeated/case/invalid name、whole/per-unit、malformed HIR、native hard-abort、ABI。 |
| `http_headers.tokens_valid(name: str) -> bool` | `count` と同じ receiver/name/native hard-abort validation。該当する全物理 row を OWS 付き comma-separated RFC token list として読む。 | Pure/zero-allocation。absent は true。各 row が一つ以上の nonempty ASCII token member を持ち、empty/trailing member がなければ true。quoted string は token ではない。malformed ABI は false にしない。 | Copy、view/cursor/状態なし。 | `std.http`; `HttpHeadersTokensValid`; A20。 | repeated row/member、OWS、empty/quoted/non-ASCII/control、split、native hard-abort、RFC-token oracle。 |
| `http_headers.contains_token(name: str, token: str) -> bool` | receiver/name/token を左から一度。両文字列は nonempty ASCII token、NUL 不可。不正なら scan 前 abort。native row は ctx、complete name view/token、complete token view/token の順に検証し、null/alignment/length/range は safe view formation 前、invalid token byte は table scan 前に hard-abort。 | Pure/zero-allocation。全該当 row/member を検索し token を ASCII-case-insensitive 比較。ほかの member の妥当性は証明せず、malformed ABI は false にしない。 | call-bounded borrow のみ。 | `std.http`; `HttpHeadersContainsToken`; A120。 | repeated/case/OWS/collision/malformed-neighbor、native validation-order/hard-abort、whole/per-unit、ABI。 |
| `http_headers.contains_token_exact(name: str, token: str) -> bool` | `contains_token` と同じ評価・native validation order。 | Pure/zero-allocation。header name は case-insensitive のまま、全 repeated row の trimmed member を token と byte-exact 比較。隣接 member は証明しない。 | call-bounded borrow のみ。 | `std.http`; `HttpHeadersContainsTokenExact`; A120。 | repeated/OWS/exact/case-negative/malformed-neighbor、whole/per-unit、native hard-abort、ABI。 |
| `http_request_ctx.upgrade_ready() -> bool`; `pkg.web.types.Ctx.upgrade_ready: bool` | bound live ctx。引数/default なし。field は middleware/handler dispatch 前に一回 copy。native row は null/misaligned ctx を reference formation 前に hard-abortし、dangling nonnull pointer は検出可能な ABI contract 外。 | Pure/zero-allocation。parsed request が HTTP/1.1 かつ complete request 後の parser residual がない時だけ true。I/O/context spend なしで、malformed ABI は false にしない。 | Copy result/field。view/mutation/allocation/retained borrow なし。 | `std.http`; `HttpCtxUpgradeReady`; A03。Ctx の trailing field と共に interface/object/link identity に入る。 | HTTP/1.0/1.1 × residual empty/nonempty、middleware/prepare visibility、native hard-abort、no-I/O、whole/per-unit、malformed HIR、ABI。 |
| `http_upgrade` | `ctx.respond_upgrade` success だけが作る global `std.http` Move handle。raw handle は same-frame local または by-value/borrow/borrow-mut parameter。tagged carrier は unnested same-frame `Result<http_upgrade, E>` local 一つだけで、E の complete graph に upgrade を含めない。constructor/`map_err` で形成し `?`/`else`/`match` で consume 可。parameter/field/capture/return、Option/reversed/nested Result、aggregate/collection/box/global/out/extern/task/parallel は不可。 | live handle は upgraded byte stream 一つを所有。read/write failure は close+poison し以後 sticky Error。spent の read/write/deadline は buffer/state/clock/I/O を変えず Invalid、shutdown だけ idempotent Ok。Drop は close-only。macOS/iOS では checked `SO_NOSIGPIPE` success 後だけ accepted socket を request ctx に入れ、failure は一回 close して request read/ctx publication 前に `srv.accept` から mapped OS Error を返す。 | accepted fd と deadline state を含む小さい runtime allocation。一回 move/source null。ctx は別 owner の spent 状態で view を保持。 | canonical v3 leaf は `Ty::HttpUpgrade=71` / `Scalar::HttpUpgrade=47`。root `[3,0,0,0,0,71]`、field `[47]`。 | semantic-byte 双方向と unknown 72/48/truncated/trailing、carrier/move/drop/control、live/spent/poisoned、checked socket-option failure、whole/per-unit。 |
| `ctx.respond_upgrade(rb) -> Result<http_upgrade, Error>` | bound live ctx、rb の順。rb だけ consume。write 前の exact order は HTTP/1.1、parser residual absent、status exact 101、body absent、各 header を insertion order で nonempty ASCII token name と HTAB/SP/visible-ASCII/obs-text value として検証、Upgrade row exactly one、Upgrade member を持つ valid Connection row exactly one、CL absent、TE absent。semantic validation 後に `H = len("HTTP/1.1 101 Switching Protocols\r\n") + sum(len(name)+len(": ")+len(value)+len("\r\n")) + len("\r\n")` を checked addition し、unrepresentable total は ctx unspent の Invalid。 | Impure。first validation/size failure は ctx unspent。次に fixed status line、全 stored header byte を insertion order、final empty lineとして exact `H`-byte head に一回 allocate/fill し、handle shell も fd lift 前に allocate。ctx spent 後に complete head を書き、success のみ handle publish。write failure は fd close、ctx spent、no handle。allocation failure は fd transfer/wire byte 前の locked hard-abort OOM。 | entry 時の moved-in builder shell/header vector/header strings の producer-requested live heap を `B`、`U = size_of::<HttpUpgrade>()` とすると allocator-private metadata を除く exact operation high-water は `B + H + U`。serialization は growth/relocation/second copy なしの exact `H` bytes 一つで、exact `U`-byte handle と全 `B` bytes に共存。compile-time shell/layout assertion と allocation probe が所有。head は write attempt 後、builder は publication 前に free。success だけ fd transfer。ctx は parse buffer を pump 中保持。 | `HttpRespondUpgrade` HIR/MIR/LLVM; A24。 | 1.0/1.1 × residual、status/body/full header syntax/order/framing、checked `H`/byte-exact serialization/`B+H+U` high-water/OOM、validation-before-write、ownership/view lifetime、raw socket、malformed HIR/ABI。 |
| `u.read_exact(out: mut buffer, count: i64) -> Result<(), Error>` | handle、mutable bare-local buffer、count。`0..=capacity`; 不正値は state/I/O 前 abort。state は buffer clear 前に選び、spent は Invalid、poisoned は sticky Error を buffer mutation なしで返す。live だけ out.len=0。 | live で exact count を EINTR retry。0 は syscall なし success。途中 EOF は NotFound。failure は publish zero、close+poison。count より先を読まない。 | 両 owner を borrow mut。growth/allocation なし、partial byte 非公開。spent/poisoned は両 owner を変えない。 | `HttpUpgradeReadExact`; A20。 | invalid args × live/spent/poisoned、capacity zero/exact/next、split、EOF/timeout/EINTR/error、no-overread、generation、sticky error。 |
| `u.write(data: slice<u8>) -> Result<(), Error>` | receiver、data。positive length は valid borrowed range。argument を state より先に検証。spent は Invalid、poisoned は sticky Error、どちらも I/O なし。live empty は syscall なし。 | Impure、SIGPIPE-safe write-all。partial failure は close+poison。 | synchronous borrow、retain/copy なし。spent/poisoned は allocation/state change なし。 | `HttpUpgradeWrite`; A20。 | invalid range × live/spent/poisoned、empty/nonempty、short/EINTR/EPIPE/timeout、exact/no-copy/platform。 |
| `u.deadline(timeout_ns: i64) -> Result<(), Error>` | receiver、`1..=86400000000000`。invalid は handle state/clock/state work 前 abort。spent は Invalid、poisoned は sticky Error、clock/mutation なし。 | live で monotonic start+budget を記録し prior deadline を replace。各 syscall 前に同じ残予算を再計算し ceil。early timeout wakeup は予算があれば retry、exhaustion 後 syscall なし Timeout。call/frame/partial ごとに reset しない。 | borrow mut、fixed state、allocation なし。 | `HttpUpgradeDeadline`; A04。 | invalid bound × live/spent/poisoned、replacement/cumulative exhaustion/early wakeup/ceil/no-call/overlap。 |
| `u.shutdown() -> Result<(), Error>` | live または spent receiver。 | terminal。live は native `shutdown(SHUT_RDWR)` を一回呼び、ENOTCONN は already shutdown として扱い、cleanup close status は無視して retry せず spent。その他 shutdown failure は close 後 mapped Error。successful repeated は syscall 無し Ok。poisoned は fd already closed を保証して syscall 無し sticky error。WebSocket frame は書かない。 | handle allocation は Drop まで残る。shutdown/close は各一回。live Drop は native shutdown 無しの close-only。 | `HttpUpgradeShutdown` A03、`HttpUpgradeFree` A62。 | live/spent/poisoned/peer-closed/shutdown errno/repeat/Drop/no-frame。 |
| `pkg.web.types.UpgradeAccepted { response: response_builder, selected: string }` | field/source order exact。selected は protocol-defined opaque metadata、`""` は allocation-free absent。 | Move。`UpgradeDecision.Accept` payload だけ。 | builder と string の一 owner。selected は upgrade success 後だけ pump へ transfer、failure fallback 前に drop。 | `pkg.web.types` nominal interface graph。 | field/order/move/drop/interface。 |
| `pkg.web.types.UpgradeDecision { Accept(UpgradeAccepted), Reject(response_builder), Failed(Error) }` | tag/source order exact `0..=2`。prepare が一つ返す。 | Accept は respond_upgrade。success は pump、Err は ordinary handler method/path/error を一回 log し同じ ctx で fixed 500 を試す。validation failure で ctx unspent の時だけ write、committed-write failure 後は silently fail。Reject は ordinary ctx.respond で handler-error log なし。Failed は一回 log+fixed 500。 | active payload 一つ。全 branch/early cleanup。Accept error は fallback builder 前に selected drop。 | ordinary sum identity。 | tag/payload/control/cleanup/fallback/log/malformed HIR/interface。 |
| `pkg.web.types.UpgradeHandler { validate: fn(slice<str>) -> bool, prepare: fn(Ctx, slice<str>) -> UpgradeDecision, pump: fn(Ctx, http_upgrade, string) -> Result<(), Error> }` | field/source order exact。全 callback は noncapturing Copy、validate は Pure。 | serve の pre-bind validation が common checks 後・segment/pair checks 前に一回 validate。false は exact diagnostic。prepare は middleware 後、pump は successful 101 後だけ一回。 | 16-byte fn value 三つ、allocation/Drop なし。pump は handle を consume/Drop。 | function signature/effect と nominal graph は interface identity。 | validator true/false/effect/order/diagnostic、pump logging、whole/per-unit、capture 禁止。 |
| `pkg.web.upgrade(method, pattern, values, validate, prepare, pump) -> Route` | 左から一度、default なし。constructor 自体は protocol validation をせず、Pure validator と values を保持。 | Pure。serve の pre-bind route validation が validator false を bind/tree construction 前に abort。Handler.Upgrade、empty stream_type。既存 radix/405/group/middleware、HEAD fallback は Respond だけ。 | Copy Route が method/pattern/values/callbacks を保持。allocation なし。 | `pkg.web` + `Route` trailing `upgrade_values`。 | mixed routing/group/404/405/HEAD/middleware、validator order、zero hot-path allocation。 |
| `pkg.ws.Message { Text(string), Binary(array<u8>), Close(Close) }` | tag/source order exact `0..=2`。continuation/Ping/Pong/raw control は公開しない。 | Move。complete UTF-8 text、complete bytes、validated peer close。 | nonempty payload は ordinary owned heap storage。connection/scratch を borrow しない。 | `pkg.ws` nominal sum graph。 | variants/order/interface、empty/nonempty allocation、move/drop。 |
| `pkg.ws.Close { code: Option<i64>, reason: string }` | field order exact。None は empty payload。Some は `1000,1001,1002,1003,1007..1014`、registered `3000,3003,3008`、private `4000..4999` のみ。reason は valid UTF-8。 | Move。length 1、invalid code/UTF-8 は返さない。 | reason string のみ所有。 | nominal interface。 | empty/code/reason、全 code boundary、UTF-8、cleanup。 |
| `pkg.ws.route(pattern, protocols, pump) -> pkg.web.types.Route` | 左から一度、default なし。constructor は protocol list を inspect せず保持。package の Pure startup validator が各 protocol の nonempty RFC token と byte-exact uniqueness を要求。empty は selection なし、nonempty は client offer match 必須、server-list 最初の match。 | Pure/zero-allocation。invalid config は `pkg.web.serve` の pre-bind validation だけで abort。GET Upgrade route。middleware が handshake より先。selected は owned string、`""` は none。 | Route が views を serve 中 retain。constructor allocation なし、nonempty selection だけ clone。 | ordinary `pkg.ws` source。private SHA-1 は public interface 外。 | protocol validator/selection、startup diagnostic、mixed routing/middleware、whole/per-unit、vendoring。 |
| `pkg.ws.receive(borrow mut connection, max_message_bytes) -> Result<Message, Error>` | cap `0..=536870912`; invalid は allocation/I/O 前 abort。0 は zero-byte Text/Binary だけ。各 call は固定 1048576-byte source-work allowance も持つ。masked frame は exact 6/8/14 header bytes、control frame は payload bytes も charge。data payload は caller cap だけ。各 charged read 前に checked subtraction し、exact exhaustion は可、rejected next unit は読まない。 | Impure。complete Text/Binary または Close 一つまで読む。fragment assembly、Ping→Pong、Pong ignore。mask/RSV/opcode/length/control/continuation/message cap/source-work/UTF-8/Close 不正を exact policy で fail。protocol/text/either-limit は 1002/1007/1009 を best-effort write、shutdown、Invalid。valid peer Close は 1010 以外 byte-exact echo、client-only 1010 は empty Close で acknowledge し、original code/reason を返す。reply error が優先。abrupt EOF は NotFound。 | fixed 32 KiB buffer と empty heap-mode builder。initialized length と単一 retained capacity は最大 512 MiB。realloc は old/new payload を同時に、Text conversion は staging array と exact result string を同時に数える。64-bit target の call-attributable producer-requested live-heap ceiling は allocator metadata を除き exact `1073774720` bytes: shell budget 128 + buffer 32768 + payload 536870912 × 2。compile-time shell assertion と live-byte resource probe が所有。mask byte は一回 append。Binary は transfer、Text は one clone、Close は nonempty reason だけ allocate。failure は unpublished storage free。 | RFC state/source-work/resource probe は ordinary `pkg.ws` source、exact I/O は std。parser runtime key/sidecar/registry/HIR op なし。 | frame Cartesian product、message/source-work exact+rejected-next（zero continuation/Ping/Pong flood）、UTF-8、1010 empty acknowledgment、全 close、reply precedence、steady/realloc/Text peak、allocation/copy/cleanup、official differential。 |
| `pkg.ws.send_text(borrow mut connection, text) -> Result<(), Error>` | valid str。empty/NUL 可、length は RFC 63-bit。 | unmasked FIN Text、minimal length header、borrowed payload を送る。header failure 後 payload なし。 | fixed header、payload copy/retain なし。 | ordinary source。 | 0/125/126/65535/65536/i64、wire/no-copy/failure/interop。 |
| `pkg.ws.send_binary(borrow mut connection, data) -> Result<(), Error>` | empty/NUL/arbitrary bytes、同じ length。 | opcode 2 の同じ契約。 | payload copy なし。 | ordinary source。 | 同じ boundary/wire/error + binary identity。 |
| `pkg.ws.close(connection, code, reason, timeout_ns) -> Result<(), Error>` | exact typed signature は英語 ledger。code/reason/timeout を左から検証後 handle transfer。server-sendable code は 1010 を除く上記 assigned code、reason `0..=123`、timeout `1..=86400000000000`。 | cumulative monotonic deadline を設定し、unmasked Close、peer Close まで同じ残予算で masked frame を読む。Closing 後 data/Pong は discard、Ping は budget reset なしで Pong。valid peer Close で handshake 完了し server TCP close。timeout/transport/protocol は close+その Error。 | fixed scratch/control payload、reason no-copy。consume handle を全 path で一回 cleanup。 | ordinary source over deadline/read/write/shutdown、IANA snapshot に pin。 | code/reason/deadline boundary/exhaustion、peer control/data/error、simultaneous close、endian、source null、close once。 |

## 境界と public use

```text
pkg.web route + middleware + prefork accept
  -> protocol-neutral HTTP/1.1 Upgrade
  -> pkg.ws RFC 6455 handshake
  -> pump-local Move transport
  -> complete typed message
```

REST/SSE/WebSocket は同じ Route slice と `pkg.web.serve` を使う。第二 listener/router/pool は
ない。open connection は stream route と同様 worker 一つを占有する。application が expected
long-lived connection + HTTP traffic に合わせて workers を明示する。

protocol-neutral seam は `pkg.ws` より下に置く。`pkg.web` は `pkg.ws` を import せず、`pkg.ws`
が通常方向に `pkg.web` を import するため package cycle はない。Upgrade handle は raw fd、
address、TLS、reader/writer、HTTP parser へ変換できない。

```align
import pkg.web.types
import pkg.ws

fn chat(c: pkg.web.types.Ctx, connection: http_upgrade, protocol: string) -> Result<(), Error> {
  mut ws := connection
  loop {
    message := pkg.ws.receive(ws, 1048576)?
    match message {
      Text(text) => pkg.ws.send_text(ws, text)?
      Binary(data) => pkg.ws.send_binary(ws, data[..])?
      Close(_) => { break }
    }
  }
  return Ok(())
}
```

```align
import pkg.web
import pkg.ws

fn main() -> Result<(), Error> {
  protocols := ["chat.v1"]
  routes := [
    pkg.web.get("/health", health),
    pkg.ws.route("/chat", protocols[..], chat),
  ]
  return pkg.web.serve("127.0.0.1", 8080, routes[..], 4)
}
```

## Handshake

ordinary router は exact GET だけをこの row に admit し、他 method は既存 404/405 selection に従い
prepare を呼ばない。exact-GET route と middleware Proceed 後、次を最初の failure まで順に検証し、
その前に SHA-1/base64/write/transport publication を行わない。

1. selected method が exact `GET` であることを防御的に再確認後、`ctx.upgrade_ready`
   （HTTP/1.1 かつ parser residual なし）、その後 body empty。
2. nonempty `Host` が exactly one。
3. 全 `Upgrade` row が valid token list で `websocket` member を持つ。
4. 全 `Connection` row が valid token list で `Upgrade` member を持つ。
5. `Sec-WebSocket-Version` exactly one、value exact `13`。
6. `Sec-WebSocket-Key` exactly one、16 bytes の canonical standard base64（24 ASCII、末尾
   `==`、tail bits zero）。
7. 全 `Sec-WebSocket-Protocol` row が valid token list。server order 最初の byte-exact offer を
   選ぶ。nonempty server list に match がなければ reject。

prepare 内 failure は empty-body 400、WebSocket version failure は `Sec-WebSocket-Version: 13` も返す。
HTTP version/residual failure にはこの header を付けない。Origin は先に
middleware が判断する。extension は negotiate せず、RSV は常に不正。success は
`base64(SHA-1(key_text || GUID))` と exact four/optional headers の 101。SHA-1 はこの固定 proof
専用 private helper で public crypto へ追加しない。RFC key/accept golden は
`dGhlIHNhbXBsZSBub25jZQ==` → `s3pPLMBiTxaQ9kYGzzhZRbK+xOo=`。

## Frame/message state

- client frame は masked、server frame は unmasked、RSV zero。
- 7/16/64-bit length は minimal encoding、127 の high bit zero、i64 representable。
- opcode 1/2 が Text/Binary を開始、0 は active fragment の continuation のみ。
- control は FIN-only、最大 125、fragment 間に可。reserved opcode は 1002。
- Ping は identical Pong 後 continue。Pong は consume/ignore。partial message は receive call 内。
- Close empty は `code: None`。length 1、invalid code/reason は失敗。valid payload は client-only
  1010 以外 exact echo、1010 は empty Close で acknowledge し、original code/reason を shutdown 後に返す。
- Text UTF-8 は reassembled message 全体で検証。
- data cumulative cap exact は成功、next byte は payload を読まず 1009。別に masked header の exact
  6/8/14 bytes と control payload を固定 1048576-byte source-work allowance に charge する。exact
  exhaustion は成功、next charged unit は読まず 1009。data payload は work 外、control は data cap 外。

caller validation は transfer/I/O 前。frame syntax、length representability、message cap、payload
transport、unmask、final UTF-8 の順。malformed+oversized は 1002、valid length cap excess は 1009。
自動 reply/close/shutdown の transport failure が protocol result より優先する。server-initiated
close は一つの cumulative deadline 内で peer Close を待ち、Closing 中 Ping reply で予算を reset
せず、data を捨てる。

## Native ABI

実装は次の十一 keyed row を同時に追加し、すべて既存 shape を再利用する。

| Key | Symbol | Shape |
|---|---|---|
| `HttpRespondUpgrade` | `align_rt_http_respond_upgrade` | A24 |
| `HttpUpgradeReadExact` | `align_rt_http_upgrade_read_exact` | A20 |
| `HttpUpgradeWrite` | `align_rt_http_upgrade_write` | A20 |
| `HttpUpgradeDeadline` | `align_rt_http_upgrade_deadline` | A04 |
| `HttpUpgradeShutdown` | `align_rt_http_upgrade_shutdown` | A03 |
| `HttpUpgradeFree` | `align_rt_http_upgrade_free` | A62 |
| `HttpHeadersCount` | `align_rt_http_headers_count` | A37 |
| `HttpHeadersTokensValid` | `align_rt_http_headers_tokens_valid` | A20 |
| `HttpHeadersContainsToken` | `align_rt_http_headers_contains_token` | A120 |
| `HttpHeadersContainsTokenExact` | `align_rt_http_headers_contains_token_exact` | A120 |
| `HttpCtxUpgradeReady` | `align_rt_http_ctx_upgrade_ready` | A03 |

A124 は消費しない。implementation activation で keyed inventory は十一増えた。readiness は ctx を
borrow して何も retain せず、null/misaligned receiver は reference formation 前に hard-abort。source HIR は
その状態を作れず、malformed native state は通常の false と混同しない。exact native signatures は
英語 ledger が権威。pointer/length/null/capacity/output
を slice/deref/I/O/ownership より先に検証する。RespondUpgrade は writable aligned output を最初に
要求して zero 化し、不正 output では入力を inspect/consume しない。次に nonnull aligned builder を
take してから ctx を検証するため、以後の全 status は builder を consume する。HTTP/1.1/residual-free と
全 response-header syntax を transfer 前に検証する。read は buffer/count と state を live buffer clear 前に
検証し、Upgrade output は full head write まで null。全 transport row は caller-invalid を
live/spent/poisoned より先に選ぶ。free は null-safe、unwind なし。

## Implementation closure matrix

std transport、web Handler、ws consumer は互いに dormant な一連の capability なので一 PR とする。
1,000 hand-written lines を超え得るが、分割すると unusable public seam と ownership proof 重複を
作るため、socket oracle が一回で fd transfer を閉じる境界の方が integration risk が低い。

| Axis | Cells / evidence |
|---|---|
| type formation | exact 71/47 codec leaf、raw same-frame local/parameter、unnested same-frame Result Ok from constructor/`map_err`、全他 placement negative、semantic-byte/malformed owner、variant tripwire。 |
| ownership | rb validation、checked serialized-head size/allocation、fd move 前の handle-shell allocation、ctx spend、publication、replacement、Result/pump move、`?`/else/match/map_err/branch/loop/early exit/Drop、fd/allocation counter、exact head high-water/OOM-before-transfer failpoint。 |
| headers/readiness | repeated row/token split/source-invalid abort、native null/alignment/length/range は safe view formation 前、token byte は table scan 前に hard-abort、HTTP version × residual、lifetime/no allocation differential + direct-ABI subprocess oracle。 |
| web dispatch | Respond/Stream/Upgrade × route/method/HEAD/405/group/middleware、validator true/false/effect/order、prepare/write/pump result。 |
| handshake | seven validation phases、duplicate/case/token/key/version/protocol selection/extension/SHA-1 golden/browser interop。 |
| frames | FIN/RSV/opcode/mask/length/fragment/control/TCP split/coalescing/mask position、exact header/control work charge の oracle + mutation corpus。 |
| messages | Text/Binary UTF-8、Ping/Pong policy、1010 empty acknowledgment を含む Close、outgoing send/timed close、全 code。 |
| bounds | message cap と固定 source-work allowance の exact/rejected-next（zero continuation/Ping/Pong flood）、length multi-invalid、caller-invalid × live/spent/poisoned、no-overread、reply/write/shutdown precedence。 |
| allocation | builder storage と preallocated handle shell に共存する exact `H`-byte Upgrade response head、fixed scratch/shell budget/`array_builder` staging/Binary transfer/Text clone/no send copy/OOM/cleanup/fd close once。receive requested live heap exact 1073774720。checked-`H` high-water と receive resource probe。 |
| ABI/cache | eleven keys と reused shape の exact empty curated function-attribute set、native query hard-abort、checked `SO_NOSIGPIPE` accept failure、HIR/MIR/LLVM、direct runtime/subprocess、rt-LTO、whole/per-unit、edit/revert、vendored inventory。 |

## 整合 pass と deferred

英語 ledger に列挙した全 source of truth が一致し、public record の type/order/default/effect/
ownership/allocation/error/identity/test、handshake product に HTTP version × residual、frame Cartesian
product、message/source-work exact+rejected-next、steady/realloc/Binary/Text allocation ceiling、
multi-invalid precedence、native widths/pointers/count、caller-invalid × live/spent/poisoned、
carrier restriction、syntax-checked example を閉じてから review
する。Ping/Pong は call-local で sidecar/registry/hidden heartbeat を作らない。
Header/readiness query は detectable malformed native ctx/view shape を reference/slice formation 前、
invalid token byte を table scan 前に hard-abort して zero/false と混同しない。Upgrade response は checked exact wire length、fd transfer 前の
head/handle-shell allocation、live builder storage との overlap を所有する。macOS/iOS accept は
`SO_NOSIGPIPE` を checked install し、failure は一回 close して ctx を publish しない。

WebSocket client、HTTP/2 extended CONNECT、TLS termination、permessage-deflate、extension/raw frame、
background heartbeat/async/broadcast registry/standalone listener は deferred。

## Review finding-to-fix ledger

| Finding class | 解決と反映 |
|---|---|
| P1 canonical type identity | post-`pkg.csv` append-only leaf 71/47 と双方向 semantic-byte、unknown/truncated/trailing rejection を type/HIR/MIR/cache ledger に固定。 |
| P1 parser residual | `HttpRequestCtx` に residual fact を保持し HTTP/1.1-and-residual-free readiness を `pkg.web.Ctx` に公開。prepare は 400、transfer は fd move 前に再検証。 |
| P1 spent handle | caller-invalid を先にし、全 operation の live/spent/poisoned を固定。spent は mutation/clock/I/O なし Invalid、shutdown は idempotent。 |
| P2 HTTP version path | exact GET 直後に readiness を検査し、HTTP/1.0 は SHA-1 前に通常 400。 |
| P2 response-header syntax | 全 name/value を insertion order で RFC syntax 検証し、Upgrade-specific field/write より前に reject。 |
| P2 route purity | generic Upgrade に Pure total validator を追加し、protocol config は既存 pre-bind validation で exact diagnostic と共に reject。 |
| Author carrier audit | raw same-frame handle と unnested same-frame Result Ok の positive grammar にし、`map_err`/consuming control を含め storage/escape を禁止。 |
| P1 Copy web context | shipped Copy view record を維持し `serve` が request owner を保持。prepare と pump に同じ値を渡し、英日とも readiness bool だけ append。 |
| P2 client-only close code | received 1010 は data として返すが server response は empty Close。ほかの accepted peer Close だけ byte-exact echo。 |
| P2 frame-work bound | masked header と control payload の exact bytes を固定 per-call 1048576 allowance に charge。exact/rejected-next no-read と 1009 owner。 |
| P2 transient allocation bound | scratch、shell budget、realloc old/new、Text staging/result を exact 1073774720 requested-live-byte ceiling と resource probe で所有。 |
| P1 checked SIGPIPE suppression | best-effort `TCP_NODELAY`/`SO_KEEPALIVE` 後、macOS/iOS accepted socket の `SO_NOSIGPIPE` installation を checked prerequisite にする。failure errno を保持し、close status を無視して no-retry close 一回、request read/ctx publication/Upgrade/write 前に original mapped OS Error を `srv.accept` から返す。Linux は `MSG_NOSIGNAL`。 |
| P2 malformed query ABI | ctx/name/token 順で detectable null/alignment/length/range defect を safe view formation 前、invalid token byte を table scan 前に hard-abort し、zero/false と混同しない。各 row を subprocess owner で覆う。 |
| P2 reused ABI attributes | reused shape の existing empty curated function-attribute set を保持。Rust C export は unwind しないが、generated declaration に LLVM `nounwind` promise を追加せず shared fingerprint を変えない。 |
| P2 Upgrade head storage | checked exact wire length `H` を計算し、growth/second copy なしで exact `H`-byte head を allocate/fill、handle shell も fd transfer 前に allocate し、全 builder storage との coexist peak を instrument。 |
| P2 stale readiness ABI prose | native inventory に残った null/misaligned-to-false 文を authoritative pre-reference hard-abort rule に置換し、英日とも malformed state と通常の HTTP/1.0/residual false を混同しない。 |

## References

- [RFC 6455 — The WebSocket Protocol](https://www.rfc-editor.org/rfc/rfc6455.html)
- [RFC 9110 — HTTP Semantics](https://www.rfc-editor.org/rfc/rfc9110.html)
- [IANA WebSocket Protocol Registries](https://www.iana.org/assignments/websocket/) — 2026-09-03
  取得、registry last updated 2026-06-10。close-code row はこの snapshot に固定し、自動では拡張しない。
