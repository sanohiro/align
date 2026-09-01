# pkg — auth

> [English](../auth.md) · **日本語**
>
> **注意:** 英語版 (`../auth.md`) が正本。本書は同期ミラーである。
>
> **ステータス:** 実装済み（2026-09-01）。

## 公開契約台帳

V1 は出荷済み JSON、encoding、crypto、time 表現を通常の package source で合成する。新しい暗号
primitive、native ABI、key owner、clock read、ambient auth state は加えない。

| 公開表面 | 入力・検証・結果 | ownership・effect・owner |
|---|---|---|
| `pub Argon2Policy { m_cost: i64, t_cost: i64, parallelism: i64 }` | `crypto.argon2_params` と同じ KiB、iteration、lane。既定値なし。hash では exact 値、verify では保存 PHC に許す独立 inclusive maximum。hash は `p=1..=16777215`、`t=1..=4294967295`、`m=8*p..=4194304`。verify maximum は各 engine ceiling 内の正値で、保存 tuple は hash 関係と全 maximum を満たす。 | 3 i64 の Copy/Pure record。借用・割当・Drop・secret なし。`pkg.auth` nominal 定義と通常 interface/cache owner が所有。 |
| `pkg.auth.encode_hs256(claims_json: str, key: slice<u8>) -> Result<string, Error>` | 左から 1 回評価。key は 32 byte 以上。claims は 8192 byte 以下の strict RFC 8259 JSON object、semantic duplicate key なし。allocation-free precheck が既知の parser leniency である string 内 raw C0 と leading-zero integer を `json.doc` 前に拒否。present `exp`/`nbf` は integer-form i64 NumericDate 秒。header は exact `{"alg":"HS256","typ":"JWT"}`。成功は最大 11004 byte の unpadded base64url compact JWS。無効入力・length arithmetic は `Invalid`。 | 入力は呼出中だけ借用。成功は owned string 1 個、全 temporary は return 前に Drop。HMAC FFI のため Impure。新 checked op/ABI はない。capability は module-wide で、session-only consumer も JSON/base64/HMAC/Argon2/random と libcrypto を保持する。 |
| `pkg.auth.verify_hs256(token: str, key: slice<u8>, now_ns: i64) -> Result<string, Error>` | key は同じ。`now_ns` は必須の非負 Unix wall-clock ns。token は 1..=16384 byte、exact 3 個の非空 canonical unpadded base64url segment、signature は 32 byte。元の `header.payload` を先に HMAC/CT 比較し、不一致は JSON parse 前に `Denied`。認証後 header の non-strict/malformed/non-object/duplicate は `Invalid`。valid unique object の `alg!=HS256`、present `typ` が string `JWT` 以外、`crit` present は `Denied`。payload は 8192 byte 以下の strict unique object。present `exp`/`nbf` は i64 秒。`now_s=now_ns/1e9` に対し `now_s < exp`、`now_s >= nbf`。 | 成功は payload JSON の byte-exact owned clone。malformed/base64/bound/key/now/認証済み JSON・claim 型は `Invalid`、MAC/header policy/time failure は `Denied`。HMAC FFI のため Impure。未認証 JSON を parse/return/log/保持しない。 |
| `pkg.auth.password_hash(password: slice<u8>, policy: Argon2Policy) -> Result<string, Error>` | 空・NUL を含む任意 byte password を許す。policy 検証後、CSPRNG salt 16 byte、Argon2id v19 tag 32 byteを生成。成功は exact `$argon2id$v=19$m=<m>,t=<t>,p=<p>$<salt>$<tag>`。decimal は正、符号・leading zero なし。salt/tag は standard unpadded base64 の 22/43 文字。policy は `Invalid`。shipped Argon2 status split をそのまま伝播し、provider/context/output-reserve failure は exact `Code(0)`、`EVP_KDF_derive` rejection は `Invalid`。package builder/string allocation と random failure は abort。 | password は借用・非保持。owned PHC string 1 個だけ公開し、salt/tag/encoding/builder は Drop。random/Argon2 FFI のため Impure。既存 Drop は zeroize を約束せず、V1 は第二の secret owner を加えない。 |
| `pkg.auth.password_verify(password: slice<u8>, phc: str, maximum: Argon2Policy) -> Result<bool, Error>` | maximum を PHC 読取前に検証。parser は上記 exact canonical grammar、5 個の `$`、exact identifier/version/order、canonical decimal、16/32 decoded byte だけを受理。保存 policy は engine relation と caller の 3 maximum 以下。全検証後に Argon2 を 1 回実行。tag は 32 byte 全体を CT 比較。 | match は `Ok(true)`、wrong password は `Ok(false)`。maximum、malformed/noncanonical/unsupported/over-limit は KDF 前に `Invalid`。shipped Argon2 status split をそのまま伝播し、provider/context/output-reserve failure は exact `Code(0)`、`EVP_KDF_derive` rejection は `Invalid`。入力非保持、全 temporary Drop。Argon2 FFI のため Impure。 |
| `pkg.auth.session_token() -> string` | 引数・default・clock・seed・prefix・store なし。CSPRNG 32 byte を exact 43 文字 `[A-Za-z0-9_-]` の unpadded base64url にする。random/allocation failure は abort。uniqueness guarantee はなく、衝突確率は OS CSPRNG に従う。 | ordinary owned string。temporary buffer は Drop。返す bearer secret も zeroize しない通常 string。Impure。registry/cookie/expiry/storage/revocation owner なし。 |

## 決定と範囲

```text
出荷済み JSON + base64url + HMAC + explicit now_ns  -> 認証済み HS256 claims JSON
出荷済み CSPRNG + Argon2id + standard base64       -> canonical bounded PHC
出荷済み CSPRNG + base64url                         -> opaque 256-bit session token
```

claim は JSON text のままで、schema、issuer、audience、subject、role、cookie、session store、revocation
は application owner に残す。password policy と verify が許す native work は caller value。clock も
caller value で、`verify_hs256` は `time.now()` を呼ばない。

JWT は HS256 だけ。出荷済み asymmetric primitive は将来の issuer/JWKS 能力の前提であって、algorithm
selector、network fetch、key cache、provider policy を追加する理由ではない。`pkg.auth` は旧
`pkg.jwt` prototype を outright replacement し、alias や old/new parallel path は残さない。

## 公開利用

```align
import pkg.auth

fn issue(claims: str, key: slice<u8>) -> Result<string, Error> =
  pkg.auth.encode_hs256(claims, key)

fn accept(token: str, key: slice<u8>, now_ns: i64) -> Result<string, Error> =
  pkg.auth.verify_hs256(token, key, now_ns)
```

```align
import pkg.auth

fn store(password: slice<u8>) -> Result<string, Error> {
  policy := pkg.auth.Argon2Policy{m_cost: 65536, t_cost: 3, parallelism: 1}
  return pkg.auth.password_hash(password, policy)
}

fn check(password: slice<u8>, stored: str) -> Result<bool, Error> {
  maximum := pkg.auth.Argon2Policy{m_cost: 262144, t_cost: 6, parallelism: 4}
  return pkg.auth.password_verify(password, stored, maximum)
}
```

```align
import pkg.auth

fn new_session() -> string = pkg.auth.session_token()
```

宣言と positional call を分離している。string の key/password は `.bytes()` で binary boundary を明示する。

## JWT grammar・検証順序

encode は key、claims bound、strict lexical precheck、JSON object、semantic duplicate、`exp`、`nbf`、
output arithmetic の順に検証し、その後だけ HMAC と allocation を行う。exact limit は成功し、error
は partial string を公開しない。

allocation-free precheck は第二の JSON parser ではない。`outside`、`string`、`escaped-byte` state で
UTF-8 byte を走査し、string 内の unescaped `0x00..=0x1f` を拒否する。backslash はこの scan で次の
1 byte だけを保護し、実 escape/Unicode grammar は `json.doc` が検証する。string 外では input start
または JSON whitespace、`[`、`{`、`,`、`:` の後だけを value boundary とし、`0` の直後の decimal
digit と `-0` の直後の decimal digit を拒否する。他の token/nesting/UTF-8/number/trailing rule は
`json.doc` が所有する。これにより shared parser を変更せず、文書化済みの 2 leniency だけを閉じる。

verify の固定順序は次である。

1. short key、negative `now_ns`、empty/oversized token、length arithmetic を拒否。
2. exact 2 dot、3 nonempty segment、unpadded URL alphabet/canonical trailing bits、32-byte signature。
3. original `header.payload` の HMAC を全 32 byte 比較。不一致は JSON work 前に `Denied`。
4. authenticated header に strict precheck を適用し object/unique として parse。lexical/parse/
   non-object/duplicate は `Invalid`。その後の `alg=HS256`、optional `typ=JWT`、`crit` absent policy
   failure は `Denied`。
5. authenticated payload に strict precheck を適用し 8192 byte 以下の object/unique として parse。
   lexical/parse/duplicate または present non-i64 `exp`/`nbf` は `Invalid`。
6. `now_s=now_ns/1000000000` で exp、nbf の順に検査。missing は constraint なし。
7. exact payload byte を clone して唯一の結果として公開し、temporary を全 Drop。

NumericDate は integer-form JSON number だけ。fraction/exponent は整数値でも拒否する。`iat`、`iss`、
`aud`、`sub`、`jti` と private claim は byte-exact に保持し、implicit policy を持たない。

## PHC grammar・work bound・順序

```text
$argon2id$v=19$m=<positive decimal>,t=<positive decimal>,p=<positive decimal>$<22 base64 chars>$<43 base64 chars>
```

standard unpadded base64 であって base64url ではない。padding、whitespace、別 order/name、missing/
extra param、別 version/algorithm/separator、leading `+`/zero、非 canonical trailing bits を拒否する。

hash は random 前に exact policy を検証。verify は 3 maximum、grammar、decimal overflow、engine bound
と `m>=8*p`、caller maximum、salt、tag の順で検証し、その後だけ Argon2 を 1 回呼ぶ。untrusted PHC
は call-site ceiling を超える work を要求できない。public invalidity は `Invalid`、provider のみ `Code`、
tag mismatch は `Ok(false)`。

salt は常に fresh 16 byte、tag は 32 byte。default work factor、pepper、automatic upgrade、password
normalization、UTF-8 requirement、prehash はない。Argon2 provider/context/output-reserve failure
は exact `Error.Code(0)`、`EVP_KDF_derive` rejection は `Error.Invalid`、後段の package
builder/string allocation は hard OOM。

## ownership、allocation、effect、secret

全入力は呼出中だけの borrow。結果に view はない。成功は password verify の Copy bool 以外 ordinary
owned string 1 個。recoverable error は owned result を公開せず、通常 Result cleanup が全 control path
を覆う。全 5 関数は Impure。4 関数は HMAC/Argon2 FFI、session は OS CSPRNG を呼ぶ。clock、global
mutable state、filesystem、network、environment、artifact/source I/O、provider selector はない。

V1 は通常 buffer/array/string Drop を継承し zeroization を約束しない。key/password は package code が
copy しない。zeroizing secret owner は別の core/std 契約であり、ここで第二の ownership model を作らない。

## package・runtime・cache 境界

vendor 可能な `pkg.auth` 1 module が public Copy record 1 個と public function 5 個を所有し、
`core.json`、`std.crypto`、`std.encoding` を import する。internal module、native declaration、compiler
recognition、checked-HIR discriminator、runtime ABI row、reflection/static artifact/ambient option はない。

direct/import call は通常 Align function semantics。現行 function-value subset は parameter/result が
scalar の signature だけを許すため、slice parameter と owned `Result` を持つ本 surface は local、
function-field、control-joined function value にならず、package は例外を加えない。whole-program は body を
直接読み、per-unit interface は signature と `Argon2Policy` を serialize する。通常の source/interface/dependency hash が cache と既存
runtime capability retention を所有する。collection は call reachability でなく module-wide なので、
どの `pkg.auth` 関数を import しても JSON/base64/HMAC/Argon2/random と libcrypto を保持する。
session-only consumer も同じである。同名の別 module 関数に特別な意味はない。`pkg.auth` がない
project は auth code を保持せず、import は通常 unresolved diagnostic。

compiler/runtime の新 persisted format はない。JWT/PHC は package output として独立 vector で固定する。
新境界がないため HIR/runtime ledger は変更しない。

## 計算量・性能境界

JWT は bounded byte 数に線形だが、semantic duplicate 検査だけ object member 数に bounded quadratic。
password work は明示 policy/maximum が支配する。session は 32 input/43 output byte 固定。throughput、
latency、allocation count、zeroization cost、memory ratio の promise はなく benchmark gate もない。

## V1 非目標

HS384/512、asymmetric JWT、algorithm selector、JWK/JWKS/OIDC/OAuth、issuer/audience/scope/role、key
fetch/cache/rotation、refresh token、cookie/CSRF、session store/revocation/expiry、password reset、MFA/
WebAuthn/TOTP、pepper store、password rule/normalization、auto upgrade/rehash advice、PHC agility、他 KDF、
zeroizing string、user DB、HTTP middleware、clock read は含まない。

## 実装 closure matrix

実装は約 1,000 hand-written 行未満で、通常 package source と owner tests の 1 capability で閉じる。

| 軸 | 必須 closure / owner 証拠 |
|---|---|
| public formation/identity | exact module/record/5 signature/core Error/import/type、ordinary direct/import call、現行 scalar-only function-value rejection、whole/per-unit。package source/public surface extraction、既存 rejection owner。 |
| JWT encode | fixed header、canonical segment/signing/tag、claims/token bounds、partial result なし。独立 RFC vector、segment decoder、empty/exact/next claims bound、exact result length。 |
| JWT verify | key/time/shape/auth/JSON/claim 順序、MAC before JSON、C0/leading-zero precheck、unique keys、malformed header `Invalid` と alg/typ/crit `Denied`、exp/nbf edge。raw C0/leading-zero encode/authenticated header/payload、mutation、escaped duplicate、unauthenticated invalid-payload sentinel。 |
| PHC | exact grammar/version/order/decimal/base64/salt/tag/policy。独立 vector、one-byte mutation、source-fixed 16-byte random buffer、real-random shape/distinct sample。 |
| password resource | KDF 前の maximum、engine relation、3 inclusive ceiling、NUL/empty、32-byte CT true/false、provider/context/output-reserve `Code(0)` と derive rejection `Invalid` の exact split。maximum→PHC→KDF source-order owner、policy product、既存 primitive failure owner。 |
| ownership/effect | 全 result path の Drop/非保持、全関数 Impure、package の secret-dependent compare なし。allocation parity、MIR/control owner。 |
| capability/cache | 既存 ABI/semantics 不変、import 時は complete module capability/libcrypto retain、session-only positive、package absence negative、edit/revert cache、optimized/unoptimized、whole/per-unit。 |
| session | 32 random byte、43 canonical character、clock/prefix/store なし、ordinary Drop。decode oracle、alphabet/length、multi-sample sanity。 |

## 実装 closure evidence

author-side matrix-to-diff pass は compiler/runtime 変更なしで次の owner に閉じる。

| matrix row | implementation / regression owner |
|---|---|
| public/identity/whole-per-unit/capability | `apps/auth/pkg/auth.align`、`apps_auth::jwt_vector_and_whole_per_unit_execution_are_exact`、`apps_auth::session_only_use_keeps_module_crypto_and_public_surface_is_closed`。exact public source、現行 function-value rejection、session-only libcrypto retain を含む。既存 package-foundation/unit-cache owner が absence と通常 invalidation を保持。 |
| JWT bytes/auth/policy/bounds/precedence | `apps_auth::jwt_vector_and_whole_per_unit_execution_are_exact`、`apps_auth::jwt_validation_order_and_error_classes_cover_strict_authenticated_products`、`apps_auth::jwt_claim_and_result_bounds_are_exact`。3 segment の alphabet/unpadded length/trailing-bit canonicality と MAC-before-JSON precedence、OpenSSL HMAC-SHA256 独立 vector、既存 JSON/encoding owner を使用。 |
| PHC/resource/comparison | `apps_auth::password_phc_vector_policy_and_canonical_mutation_matrix_are_exact`。OpenSSL 3.5 Argon2id 独立 vector、canonical mutation、policy、NUL/empty、true/false を閉じる。既存 `m11_crypto` owner が native status split、bounds、random、CT、cleanup を保持。 |
| ownership/effect | `apps_auth::owned_results_escape_inputs_and_auth_operations_remain_impure` と whole/per-unit JWT owner。既存 JSON/encoding/crypto control/Drop owner を再利用。 |
| session | `apps_auth::session_only_use_keeps_module_crypto_and_public_surface_is_closed` が 32-byte decode、43-character output、2 sample inequality、Impure/module-wide behavior を閉じる。 |

## 実装レビュー finding-to-fix 台帳

fresh full-diff implementation review は P1 を 1 件返した。fix は segment class 全体を閉じる。

| finding | class-wide resolution |
|---|---|
| P1: optional padding と malformed header/payload segment text が authentication へ到達可能 | HMAC 前に 3 個すべての nonempty segment を検査する allocation-free canonical base64url validator を追加。`=`、non-URL byte、length mod 4 == 1、nonzero unused trailing bit を拒否する。owner は bad tag と malformed header/payload/signature、canonical bytes だが unauthenticated invalid JSON の `Denied`、otherwise-valid signature の padded alias `Invalid` を分離。 |

## 正典と author consistency pass

英語台帳、本書、`draft.md`、`docs/language-spec.md`、`docs/design-notes.md`、`docs/history.md`、
`docs/open-questions.md`、`docs/impl/07-roadmap.md`、`HANDOFF.md` を一致させる。実装が本物の新 compiler/
native boundary を発見しない限り HIR/runtime ledger は変更せず、発見時は設計を reopen する。

設計は 2026-09-01 の独立レビュー findings を解消して受理済み。author pass は全型・順序・default・ownership・allocation・error・effect、JWT 全 state、PHC 全 product、
UTF-8/NUL/native input、multi-invalid precedence、非 ambient 性、wire grammar/vector、producer-owned
inspection、syntax-checked example、全 ledger invariant の acceptance owner を照合する。promise のない
benchmark は gate にしない。

## 設計レビュー finding-to-fix 台帳

| finding | 解消 |
|---|---|
| P1: `json.doc` の raw C0 / leading-zero leniency | allocation-free lexical precheck、exact state/boundary、`Invalid` precedence、encode/authenticated header/payload owner を追加。shared JSON は不変。 |
| P2: used-body capability promise | 実際の module-wide whole/per-unit collection に修正。session-only でも complete capability/libcrypto を保持。 |
| P2: malformed authenticated header error | lexical/parse/non-object/duplicate は `Invalid`、valid document の alg/typ/crit policy は `Denied` に統一。 |
| P2: Argon2 output allocation | native provider/context/output-reserve `Code(0)`、derive-rejection `Invalid`、package-owned 後段 OOM を区別。 |
| P2: candidate/Settled state | 英語・日本語 ledger、roadmap、handoff、Settled、history を受理済みに統一。 |

上記修正の changed-slice review は追加 P2 を 1 件返した。

| finding | 解消 |
|---|---|
| P2: 最初の Argon2 修正が derive rejection も `Code(0)` に分類 | shipped primitive の complete split、すなわち provider/context/output-reserve は `Code(0)`、derive rejection は `Invalid`、後段 package-owned OOM は abort に復元し、public row、precedence、closure owner、digest、roadmap、handoff を同期。 |
