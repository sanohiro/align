# pkg — ws

> 英語版 `../ws.md` が権威であり、このファイルは同期された日本語 mirror である。
>
> **状態:** designed、implementation pending。この文書の surface は、implementation capability が owner test
> と runtime row を一緒に有効化するまで未実装である。

## 権威 public-contract ledger

V1 は既存 `pkg.web` route table に統合する RFC 6455 HTTP/1.1 server である。standalone
listener、client、raw frame API、extension framework、compression、background task は含まない。
`pkg.web` が routing/middleware/request view/accept loop/`SO_REUSEPORT` を、`std.http` が
protocol-neutral Upgrade transport を、`pkg.ws` だけが handshake、内部 SHA-1、frame grammar、
mask、message assembly、自動 control reply、close-code policy を所有する。

| Public surface | 入力・default・validation・評価 | 結果・error・順序・effect | ownership・lifetime・allocation・cleanup | owner・ABI・identity | acceptance owner |
|---|---|---|---|---|---|
| `http_headers.count(name: str) -> i64` | receiver と name を一度評価。name は nonempty ASCII token、NUL 不可。不正 name は scan 前 abort。 | Pure/zero-allocation。物理 header row 数を field name ASCII-case-insensitive で返す。comma member は数えない。 | Copy。call 中だけ request table を borrow。 | `std.http`; `HttpHeadersCount`; 既存 A37。method/key は interface/object/link identity に入る。 | zero/one/repeated/case/invalid name、whole/per-unit、malformed HIR、ABI。 |
| `http_headers.tokens_valid(name: str) -> bool` | 同じ name 規則。該当する全物理 row を OWS 付き comma-separated RFC token list として読む。 | Pure/zero-allocation。absent は true。各 row が一つ以上の nonempty ASCII token member を持ち、empty/trailing member がなければ true。quoted string は token ではない。 | Copy、view/cursor/状態なし。 | `std.http`; `HttpHeadersTokensValid`; A20。 | repeated row/member、OWS、empty/quoted/non-ASCII/control、split、RFC-token oracle。 |
| `http_headers.contains_token(name: str, token: str) -> bool` | receiver/name/token を左から一度。両文字列は nonempty ASCII token、NUL 不可。不正なら scan 前 abort。 | Pure/zero-allocation。全該当 row/member を検索し token を ASCII-case-insensitive 比較。ほかの member の妥当性は証明しない。 | call-bounded borrow のみ。 | `std.http`; `HttpHeadersContainsToken`; A120。 | repeated/case/OWS/collision/malformed-neighbor、whole/per-unit、ABI。 |
| `http_upgrade` | `ctx.respond_upgrade` success だけが作る global `std.http` Move handle。non-Copy/non-comparable/non-printable。local と by-value/borrow/borrow-mut parameter は可。constructor の direct Result Ok 以外の tag、user aggregate/collection/box/global/out/extern/capture/task/parallel/return は不可。 | live handle は upgraded byte stream 一つを所有。read/write/deadline/shutdown は Impure、`borrow mut` で直列化。read/write failure は close+poison し、以後同じ builtin Error を I/O なしで返す。Drop は close-only。 | accepted fd と monotonic start-plus-budget を含む小さい runtime state allocation。一回 move、source null。ctx は別 owner の spent 状態で pump 中 view を保持。 | `Ty/Scalar::HttpUpgrade`; compiler が carrier/effect/Drop、runtime が fd/deadline/sticky error。 | formation、constructor carrier、move/drop/replacement/control、禁止 placement/capture/return、deadline/sticky error、whole/per-unit。 |
| `ctx.respond_upgrade(rb) -> Result<http_upgrade, Error>` | bound live ctx、rb の順。rb だけ consume。write 前の exact order は HTTP/1.1、status exact 101、body absent、全 stored header の既存 insertion-order name/value guard、one-or-more token を持つ Upgrade row exactly one、Upgrade member を持つ valid Connection token-list row exactly one、CL absent、TE absent。protocol token 自体は解釈しない。 | Impure。first validation failure は ctx unspent + Invalid。validation/serialization 後に fd を lift、ctx spent、101 head を全書込。success のみ handle publish。write failure は fd close、ctx spent、no handle。park しない。 | source-valid call は rb を全結果で free。success だけ fd transfer。ctx は parse buffer を pump 中保持。 | `HttpRespondUpgrade` HIR/MIR/LLVM; runtime key A24。新 ABI shape なし、A124 は次の unused。 | 1.0/1.1、status/body/header-order/framing、validation-before-write、ownership/spent/view lifetime、raw socket、malformed HIR/ABI。 |
| `u.read_exact(out: mut buffer, count: i64) -> Result<(), Error>` | live handle、mutable bare-local buffer、count。`0..=capacity`; 不正値または positive count + zero capacity は状態/I/O 前 abort。最初に out.len=0。 | exact count を EINTR retry で読む。0 は syscall なし success。途中 EOF は NotFound。failure は publish zero、close+poison。count より先を読まない。 | 両 owner を call 中 borrow mut。growth/allocation なし、partial byte 非公開。 | `HttpUpgradeReadExact`; A20。 | capacity zero/exact/next、全 split、EOF/timeout/EINTR/error、coalesced no-overread、buffer generation、sticky error。 |
| `u.write(data: slice<u8>) -> Result<(), Error>` | live receiver、data。empty は syscall なし。NUL/arbitrary bytes は data。 | Impure、SIGPIPE-safe write-all。partial failure は close+poison。 | synchronous borrow、retain/copy/allocation なし。 | `HttpUpgradeWrite`; A20。 | empty/nonempty、short/EINTR/EPIPE/timeout、exact bytes、no-copy、platform。 |
| `u.deadline(timeout_ns: i64) -> Result<(), Error>` | live receiver、`1..=86400000000000`。invalid は clock/state work 前 abort。 | monotonic start+budget を一つ記録し prior deadline を replace。以後の read/write は各 syscall 前に同じ残予算を再計算し、早く切れない native unit へ ceil。native timeout wakeup は clock を再確認し予算が残れば retry。exhaustion 後は syscall なしで Timeout。call/frame/partial transfer ごとに reset しない。fresh live では失敗せず、poisoned は sticky error replay。 | borrow mut、fixed clock/budget state のみ、allocation なし。 | `HttpUpgradeDeadline`; A04。 | bounds/replacement、partial/multi-frame cumulative exhaustion、early wakeup、ceil/recheck、poison replay、expiry 後 no-call、overlap。 |
| `u.shutdown() -> Result<(), Error>` | live または spent receiver。 | terminal。live は native `shutdown(SHUT_RDWR)` を一回呼び、ENOTCONN は already shutdown として扱い、cleanup close status は無視して retry せず spent。その他 shutdown failure は close 後 mapped Error。successful repeated は syscall 無し Ok。poisoned は fd already closed を保証して syscall 無し sticky error。WebSocket frame は書かない。 | handle allocation は Drop まで残る。shutdown/close は各一回。live Drop は native shutdown 無しの close-only。 | `HttpUpgradeShutdown` A03、`HttpUpgradeFree` A62。 | live/spent/poisoned/peer-closed/shutdown errno/repeat/Drop/no-frame。 |
| `pkg.web.types.UpgradeAccepted { response: response_builder, selected: string }` | field/source order exact。selected は protocol-defined opaque metadata、`""` は allocation-free absent。 | Move。`UpgradeDecision.Accept` payload だけ。 | builder と string の一 owner。selected は upgrade success 後だけ pump へ transfer、failure fallback 前に drop。 | `pkg.web.types` nominal interface graph。 | field/order/move/drop/interface。 |
| `pkg.web.types.UpgradeDecision { Accept(UpgradeAccepted), Reject(response_builder), Failed(Error) }` | tag/source order exact `0..=2`。prepare が一つ返す。 | Accept は respond_upgrade。success は pump、Err は ordinary handler method/path/error を一回 log し同じ ctx で fixed 500 を試す。validation failure で ctx unspent の時だけ write、committed-write failure 後は silently fail。Reject は ordinary ctx.respond で handler-error log なし。Failed は一回 log+fixed 500。 | active payload 一つ。全 branch/early cleanup。Accept error は fallback builder 前に selected drop。 | ordinary sum identity。 | tag/payload/control/cleanup/fallback/log/malformed HIR/interface。 |
| `pkg.web.types.UpgradeHandler { prepare: fn(Ctx, slice<str>) -> UpgradeDecision, pump: fn(Ctx, http_upgrade, string) -> Result<(), Error> }` | field/source order exact。両方 noncapturing Copy function value。 | Copy。prepare は middleware 後・write 前に一回。pump は successful 101 後だけ一回、transport と selected string を所有。pump Ok は log なし、Err は Stream と同じ exact method/path/error diagnostic を出し HTTP response は追加しない。 | 16-byte fn value 二つ、allocation/Drop なし。 | function signature/effect と nominal graph は interface identity。 | direct/imported fn、signature/effect mismatch、pump Ok/Err logging、whole/per-unit dispatch、capture 禁止。 |
| `pkg.web.upgrade(method, pattern, values, prepare, pump) -> Route` | exact typed signature は英語 ledger のとおり。左から一度、default なし。protocol validation はせず values を opaque config として保持。 | Pure。Handler.Upgrade、empty stream_type。既存 radix/405/group/middleware。HEAD fallback は Respond だけ。 | Copy Route が method/pattern/values を serve lifetime borrow。allocation なし。 | `pkg.web` + `Route` trailing `upgrade_values: slice<str>`。 | mixed handler routing、group、404/405/HEAD/middleware、zero hot-path allocation。 |
| `pkg.ws.Message { Text(string), Binary(array<u8>), Close(Close) }` | tag/source order exact `0..=2`。continuation/Ping/Pong/raw control は公開しない。 | Move。complete UTF-8 text、complete bytes、validated peer close。 | nonempty payload は ordinary owned heap storage。connection/scratch を borrow しない。 | `pkg.ws` nominal sum graph。 | variants/order/interface、empty/nonempty allocation、move/drop。 |
| `pkg.ws.Close { code: Option<i64>, reason: string }` | field order exact。None は empty payload。Some は `1000,1001,1002,1003,1007..1014`、registered `3000,3003,3008`、private `4000..4999` のみ。reason は valid UTF-8。 | Move。length 1、invalid code/UTF-8 は返さない。 | reason string のみ所有。 | nominal interface。 | empty/code/reason、全 code boundary、UTF-8、cleanup。 |
| `pkg.ws.route(pattern, protocols, pump) -> pkg.web.types.Route` | exact typed signature は英語 ledger。protocol は nonempty RFC token、byte-exact duplicate は abort。empty list は selection なし。nonempty list は offer match 必須、server-list 最初の match。 | invalid static config 以外 Pure。GET Upgrade route。middleware が handshake より先。selected は owned string、`""` は none。 | Route が views を serve 中 retain。constructor allocation なし、nonempty selection だけ clone。 | ordinary `pkg.ws` source。private SHA-1 は public interface 外。 | protocol validation/selection、mixed routing/middleware、whole/per-unit、vendoring。 |
| `pkg.ws.receive(borrow mut connection, max_message_bytes) -> Result<Message, Error>` | cap `0..=536870912`; invalid は allocation/I/O 前 abort。0 は zero-byte Text/Binary だけ。 | Impure。complete Text/Binary または Close 一つまで読む。fragment を assembly、interleaved Ping は identical Pong、Pong は consume/ignore。client mask 必須。RSV/reserved opcode/nonminimal length/control/continuation/cap/UTF-8/Close 不正を exact policy で fail。protocol/text/limit は 1002/1007/1009 を best-effort write、shutdown、Invalid。reply write error が優先。valid Close は byte-exact echo+shutdown 後 publish。abrupt EOF は NotFound。 | valid bound 後、fixed-capacity 32 KiB `buffer` 一 allocation と empty heap-mode `array_builder<u8>` accumulator を call 中所有。initialized length は cap 以下、nonempty growth は shipped `max(4, needed).next_power_of_two()` rule なので payload capacity は global 512 MiB 以下。mask removal は各 decoded byte を一回 append。control-only/zero payload は accumulator allocation なし。Binary は allocation transfer、Text は built byte array の complete view を validate して一回 clone 後 staging free、Close は nonempty reason だけ allocate。failure は unpublished storage free。 | RFC state は ordinary `pkg.ws` source、exact I/O は std。parser runtime key/sidecar/registry/HIR op なし。 | complete frame Cartesian product、cap、UTF-8 split、close、auto reply precedence、allocation/growth/copy/cleanup、official differential。 |
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

1. selected method が exact `GET` であることを防御的に再確認後、body empty。
2. nonempty `Host` が exactly one。
3. 全 `Upgrade` row が valid token list で `websocket` member を持つ。
4. 全 `Connection` row が valid token list で `Upgrade` member を持つ。
5. `Sec-WebSocket-Version` exactly one、value exact `13`。
6. `Sec-WebSocket-Key` exactly one、16 bytes の canonical standard base64（24 ASCII、末尾
   `==`、tail bits zero）。
7. 全 `Sec-WebSocket-Protocol` row が valid token list。server order 最初の byte-exact offer を
   選ぶ。nonempty server list に match がなければ reject。

prepare 内 failure は empty-body 400、version failure は `Sec-WebSocket-Version: 13` も返す。Origin は先に
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
- Close empty は `code: None`。length 1、invalid code/reason は失敗。valid payload を exact echo
  して shutdown 後に返す。
- Text UTF-8 は reassembled message 全体で検証。
- data cumulative cap exact は成功、next byte は payload を読まず 1009。control は cap 外。

caller validation は transfer/I/O 前。frame syntax、length representability、message cap、payload
transport、unmask、final UTF-8 の順。malformed+oversized は 1002、valid length cap excess は 1009。
自動 reply/close/shutdown の transport failure が protocol result より優先する。server-initiated
close は一つの cumulative deadline 内で peer Close を待ち、Closing 中 Ping reply で予算を reset
せず、data を捨てる。

## Native ABI

実装時だけ次の九 keyed row を同時に追加し、すべて既存 shape を再利用する。

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

A124 は消費しない。exact native signatures は英語 ledger が権威。pointer/length/null/capacity/output
を slice/deref/I/O/ownership より先に検証する。RespondUpgrade は writable aligned output を最初に
要求して zero 化し、不正 output では入力を inspect/consume しない。次に nonnull aligned builder を
take してから ctx を検証するため、以後の全 status は builder を consume する。read output は complete
success まで length zero、Upgrade output は full head write まで null。free は null-safe、unwind なし。

## Implementation closure matrix

std transport、web Handler、ws consumer は互いに dormant な一連の capability なので一 PR とする。
1,000 hand-written lines を超え得るが、分割すると unusable public seam と ownership proof 重複を
作るため、socket oracle が一回で fd transfer を閉じる境界の方が integration risk が低い。

| Axis | Cells / evidence |
|---|---|
| type formation | constructor direct Ok、local/by-value/borrow/borrow-mut/pump、全 aggregate/tag/collection/capture/task/parallel/out/extern/global/return negative、variant tripwire。 |
| ownership | rb/fd/ctx move、publication、replacement、pump、`?`/else/match/map_err/branch/loop/early exit/Drop、fd/allocation counter。 |
| headers | repeated row/token split/invalid args/lifetime/no allocation differential oracle。 |
| web dispatch | handler kind × route kind × method/HEAD/405 × group/middleware × prepare decision/write/pump result。 |
| handshake | seven validation phases、duplicate/case/token/key/version/protocol selection/extension/SHA-1 golden/browser interop。 |
| frames | FIN/RSV/opcode/mask/length/fragment/control/TCP split/coalescing/mask position の oracle + mutation corpus。 |
| messages | Text/Binary UTF-8、Ping/Pong policy、Close/outgoing send/timed close、全 code。 |
| bounds | cap/length multi-invalid、no-overread、reply/write/shutdown precedence。 |
| allocation | fixed scratch/`array_builder<u8>` staging/Binary transfer/Text one-clone/no send copy/OOM/unpublished cleanup/fd close once。 |
| ABI/cache | nine keys、HIR/MIR/LLVM、runtime input matrix、rt-LTO、whole/per-unit、edit/revert、vendored inventory。 |

## 整合 pass と deferred

英語 ledger に列挙した全 source of truth が一致し、public record の type/order/default/effect/
ownership/allocation/error/identity/test、handshake product、frame Cartesian product、multi-invalid
precedence、native widths/pointers/count、carrier restriction、syntax-checked example を閉じてから review
する。Ping/Pong は call-local で sidecar/registry/hidden heartbeat を作らない。

WebSocket client、HTTP/2 extended CONNECT、TLS termination、permessage-deflate、extension/raw frame、
background heartbeat/async/broadcast registry/standalone listener は deferred。

## References

- [RFC 6455 — The WebSocket Protocol](https://www.rfc-editor.org/rfc/rfc6455.html)
- [RFC 9110 — HTTP Semantics](https://www.rfc-editor.org/rfc/rfc9110.html)
- [IANA WebSocket Protocol Registries](https://www.iana.org/assignments/websocket/) — 2026-09-03
  取得、registry last updated 2026-06-10。close-code row はこの snapshot に固定し、自動では拡張しない。
