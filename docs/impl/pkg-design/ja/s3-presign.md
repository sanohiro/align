# pkg.s3 — 明示的な有効期間を持つ署名付きURL

> 英語版 `../s3-presign.md` が正本。
> 状態: 設計承認済み、2026-09-09。実装待ち。

## 能力境界と公開契約台帳

実装済みの[S3パッケージ](s3.md)にquery認証を追加する。配布可能なURL、
method、送信に必要な利用者headerを返し、HTTP操作は行わない。認証情報、
endpoint、時刻、有効期間は明示する。既存のheader認証 `request` の契約は
変わらない。同じvendoring対象の `pkg.s3` が両操作を所有し、非公開の
canonicalizationとHMAC処理を共有する。

以下の宣言とP1–P8が正確な公開契約を所有する。既定値、overload、暗黙の
引数はない。引数はソース順に一度評価し、その後で検証する。

| 正確な宣言 | 入出力の意味 | 所有権・割当・effect | 識別・前提・受入 |
|---|---|---|---|
| `pub SignedHeader { name: string, value: string }` | この順のfield。小文字のheader名と正規化した値、P4。受信者が送るheaderであり、transport所有のHostは含まない。 | 2本のstringを所有する通常のMove record。構築、field move、置換、再帰Dropは既存規則。入力の借用を残さない。 | nominalなpackage record。順序付きの全field定義が通常interfaceに入る。P-G、P-O。 |
| `pub PresignedRequest { method: string, url: string, headers: array<SignedHeader> }` | この順のfield。正確なmethod、完全なASCII URL、名前昇順の利用者header、P4–P7。入力headerが空なら配列も空。body、絶対失効時刻、認証情報owner、隠れたrequest handleは持たない。 | 2本のstringと全配列要素を所有する通常のMove record。全入力rootより長生きし、何度でも借用できる。独自Drop/clone規則はない。 | 通常interface/implementation hash。新cache schema/native ABIなし。P-O、P-I。 |
| `pub fn presign(credentials: Credentials, endpoint: Endpoint, method: str, path: str, query: slice<Field>, headers: slice<Field>, now_ns: i64, expires_seconds: i64) -> Result<PresignedRequest, Error>` | 既存の借用入力record 3種は不変。P7の完全な結果、またはP1順序の `Error.Invalid`。methodを署名し、bodyは引数にしない。時刻/期間P2、query P3、header P4、署名P5–P7。 | 入力は呼出中だけ借用。既存cryptoによりImpure。全検証はpackage割当/cryptoより前。成功時は出力string/配列と一時builder/符号化string/digestを割り当て、通常終了で一時値を破棄する。既存の割当/crypto hard failureは維持する。 | 実装済みHTTP owner、record配列、時刻整形、percent encoding、SHA-256、binary-key HMACで足りる。通常ソースの1能力でありcompiler/runtime前提追加なし。P-G、P-V、P-O、P-I。 |

永続/交換binary record形式、reflection、runtime table、process-global変更、
認証情報cache、key cacheはない。wire artifactは以下で定めるURL文字列。
module-wide import capabilityは既存のHTTP/time/encoding/crypto依存を維持する。
秘密の一時値は通常Dropでありzeroizationを保証しない。出力には意図的に
access identifier、署名、任意session tokenを含む。secret keyは含めない。
これらを含むdiagnostic/logは出さない。URLと必要headerを持つ相手はserverの
認可範囲で署名済み操作を実行できる。一回限りのtokenではない。

## 番号付き規則

**P1 — 決定的検証。** 失敗はすべて `Error.Invalid`。package割当、時刻整形、
encoding、hashより前に、(1) query件数128以下、次にheader件数120以下、
(2) S3 V2のcredentials、(3) V3のendpoint、(4) V4のmethodとpath、
(5) V5のquery要素、(6) V6のheader要素、(7) `now_ns >= 0`、
(8) `expires_seconds` が1–604800両端含む、の順で検証する。
V2–V6の正確なbyte上限、UTF-8/ASCII/NUL規則、ソース順、重複/予約名拒否は
維持する。body引数がないためbody長検証もない。件数上限は利用者要素だけを
数え、生成認証queryを128件に含めない。入力拒否でHTTPを呼ばず、header abortに
到達しない。

**P2 — 明示的時刻。** 既存 `time.basic_iso` で `now_ns` を整形し、正の
秒未満部分を捨てる。最大値を含む非負i64時刻をすべて受け入れる。timestampの
先頭8 ASCII byteをdateにする。`expires_seconds` はその整数秒時刻からの期間で、
符号/先頭ゼロなしの10進数にする。i64 nanosecond期限への加算/乗算はせず、
表現可能な絶対期限は約束しない。現在時刻/認証情報の期限をローカル検査しない。
認証情報の失効/取消やpolicy拒否でserverが先に拒否する場合がある。
指定期間は推測した絶対時刻までアクセスできる保証ではない。

**P3 — query。** 許可した利用者pairは空値と重複もすべて維持する。
encoding/sortより前に次の生成pairを追加する。

| 名前 | 未符号化の値 | 存在条件 |
|---|---|---|
| `X-Amz-Algorithm` | `AWS4-HMAC-SHA256` | 常に |
| `X-Amz-Credential` | access key + `/` + date + `/` + region + `/s3/aws4_request` | 常に |
| `X-Amz-Date` | P2 timestamp | 常に |
| `X-Amz-Expires` | P2期間 | 常に |
| `X-Amz-SignedHeaders` | P4名を `;` で連結 | 常に |
| `X-Amz-Security-Token` | 正確なcredential token | `Some` の場合だけ。`None` ならpairを省略 |

全名前/値をまとめてS3 W1の符号化後の名前、次に値の独立した順序でsortする。
既存のASCII大小無視 `x-amz-` query拒否が利用者衝突を防ぐ。
`X-Amz-Signature` はcanonical queryに含めずP7でだけ追加する。
利用者入力が空でも必須5pairが存在する。token中の `+`、`/`、`=` もdataとして
percent encodingする。

**P4 — header。** 全利用者headerをS3 W3の小文字/空白正規化に通し、
正確なorigin authorityを唯一の生成 `host` rowとして加える。このHostと利用者rowの
全体を小文字名でsortし、canonical headersとSignedHeadersの両方に使う。
返却header配列はそのsort済み集合からHostだけを除く。利用者名が `x-test` と
`content-type` ならSignedHeadersは正確に `content-type;host;x-test`、
返却rowは `content-type`、次に `x-test`。最終行も含め各行末はLF、名前の
連結は `;`。date/payload hash/token/Authorization headerは生成しない。
P1はこれら認証名4種も含むV6予約名すべてを維持する。返すheader配列は同じ順の
正規化済み利用者rowだけでHostを除き、全rowが署名対象。入力headerが空のとき
だけ配列が空になり、この場合SignedHeadersは `host`。利用者の元の綴り、空白、
順序、格納領域を保持しない。V6で許可した他の `x-amz-*` headerも署名する。

**P5 — canonical bytes。** URIはslash/dot segmentを維持する既存S3 W1の
path encoding。S3 W4の6field canonical recordと同じ形で、queryをP3、headerと
名前をP4、payload fieldをliteral `UNSIGNED-PAYLOAD` にする。LFと最終LFなしの
規則は同じ。この操作はbodyをhash/束縛しない。対応checksum headerを利用者が
指定して別途checksumを束縛できるが、その解釈はserverが所有する。

**P6 — 署名。** P5をhashし、既存S3 W4のStringToSign、scope、raw-key HMAC
導出をそのまま適用する。結果は正確に64文字の小文字hex。header認証 `request`
は引き続き入力bodyをhashし、既存Authorization形式を出力する。

**P7 — 出力。** URLは正確なorigin + encoded URI + `?` + P3 canonical query
+ `&X-Amz-Signature=` + P6署名。署名は署名済みsort順へ挿入せず、常に最後のpair。
fragment、余分なseparator、改行はない。正確な入力methodをcopyし、P4利用者rowを
すべて所有する。URLはNULを含まないASCII。path/queryのUTF-8/NULはpercent escapeに
なる。request構築、header setter、body copyは行わない。出力fieldは常に全て存在し、
ordinal/unavailable値/任意出力fieldはない。P1後に別の出力サイズpolicyを加えない。
capacity計算は許可済みの有界長を使い、wrapしてはならない。

**P8 — 利用。** 受信者は選んだHTTP実装で結果のmethod/URL/全headerを送る。
Hostは明示portを含めURL authorityを正確に維持し、path/queryを正規化/再符号化しない。
browser型のURL単体利用は必要利用者headerがなく、署名したmethodを送る場合に適する。
PUT利用者はbodyとHTTP framingを用意し、空bodyも既存の明示body存在規則に従う。
認証情報を消費せず同じ結果を複数requestに再利用できる。署名対象method/target/headerの
変更は認証を無効にし得る。transport、status、response所有権、streaming、timeout、
errorは選んだHTTP APIに従う。

## 利用例と実装closure matrix

宣言と通常の位置引数呼出を分ける。このhelperは再利用可能な署名結果を借用し、
bodyは既存HTTP copyだけを通す。

```align
import pkg.s3
import std.http

fn upload(borrow client: http_client, borrow signed: pkg.s3.PresignedRequest,
  body: slice<u8>) -> Result<http_response, Error> {
  outgoing := http.request(signed.method, signed.url)
  mut i := 0
  loop {
    if i >= signed.headers.len() { break }
    outgoing.header(signed.headers[i].name, signed.headers[i].value)
    i = i + 1
  }
  outgoing.body(body)
  return client.request(outgoing)
}
```

既存 `crates/align_driver/tests/pkg_s3.rs` が新caseを所有し、隔離project、child期限、
local peerを再利用する。1つのpackage実装でcanonicalization/署名helperを共有し、
URLからwireまでの完全な境界を閉じる。helperだけの独立producer能力は不要。
性能/正確な割当回数は約束せず、benchmarkも要求しない。

| ID / 正確なowner | 実装と回帰によるclosure |
|---|---|
| P-G `presign_vectors` | AWS公開2013-05-24 GET例: canonical hash `3bfa292879f6447bbcda7001decf97f4a54dc650c8942174ae0a9121cf58ad04`、署名 `aeeed9bbccd4d02ee5c0109b86d86835f995330da4c265957d157751f604d404`。canonical bytes/完全URLを独立構成。正確なURL/method/header rowでtoken None/Some、headerゼロ/複数、encoded prefix sort、重複/空pair、binary secret、空白、authority/port、UTF-8/NUL/slash/dot pathを網羅。Hostのbyte順前後に利用者headerを置き、`content-type;host;x-test` と返却row `content-type`、`x-test` を検証する。captureしたbyteからcanonical形へ逆構成し、最後の署名pairだけを除いて検証する。 |
| P-V `presign_validation` | 本番validatorを共有するV2–V6 parameterized ownerを再利用し、presignの件数/検証入口を区別する。件数0/上限/次、期間-1/0/1/604800/604801/i64両端、時刻-1/0/小数/max、予約query/header全分類、token None/Some/空、複合不正入力。巨大サイズは非公開predicate probeでも閉じられるが公開呼出で入口を確認する。拒否入力はHTTPも送信も行わない。 |
| P-O `presign_round_trip` | 入力arena、一時credentials/field bufferを抜けて結果を返し、元入力を変更/破棄する。helper/Result/Optionでmove、置換、return、分解、反復借用、nested header Drop。if/match/else/?/loop join、early exit、move-out source nulling、cleanupは既存record/array ownerを再利用し、このaggregateを区別する置換/再利用caseを加える。local peerでGET/PUT(binary/明示空body)、正規化した必要header、token両状態、client再利用、raw拒否responseを検証。bodyを変えてもquery認証は変わらない。 |
| P-I `presign_imports_effects_cache` | whole-program/per-unit × Dev/Releaseで同じURL/wire oracle。package越しnominal record/array interfaceとborrowed projectionをcompile。正しいprimitiveを返す並列closureでImpure呼出を拒否。cold/warm、非公開署名helper変更/復元でartifactが正しく失効し、既存request vectorは不変。単一moduleの既存package inventoryで新callableを含み、native symbol/source unitは増えない。 |

malformed checked IR、generic specialization、record/array割当provenance、再帰Drop、
source nulling、interface serializationは既存compiler ownerを維持する。
所有権戦略、IR variant、FFI、runtime body framing、callback、process-global状態は変えない。
probeで欠けた不変条件が見つかった場合はscope拡張前にmatrixを開き直す。
credential refresh、presigned POST policy、stream/chunk署名、multipart、型付きoperation
schema、SigV4a、provider固有認証は対象外。

## 出典と同期

[S3 query authentication](https://docs.aws.amazon.com/AmazonS3/latest/developerguide/sigv4-query-string-auth.html)
がprotocolと公開例を所有し、
[S3 presigned URL lifetimes](https://docs.aws.amazon.com/AmazonS3/latest/userguide/using-presigned-url.html)
が指定期間とcredential/policy有効性を区別する。Alignの型、入力上限、検証順序、
所有権、出力rowはpackage設計判断。共有V2–V6とW1/W3/W4は `s3.md` が正本。

`s3.md`、両日本語mirror、`draft.md`、`docs/language-spec.md`、`docs/design-notes.md`、
`docs/open-questions.md`、`docs/impl/07-roadmap.md` を同期する。`HANDOFF.md` は能力境界を
一度記録する。独立レビュー前に公開型/例probeをcompileし、台帳とproseの状態組合せを
照合し、独立hash/HMAC実装で公開vectorを再現する。型probeは実装/署名の証拠ではない。

著者closure: 公開型/例とexportしたborrowed consumerはper-unit checkとLLVM emissionを
通過した。独立Python oracleで公開vectorを双方向に再現した。全差分の独立設計レビューは
Host順序の曖昧さをP2として1件指摘し、P4/P-GでHostと利用者row全体のsortと、
返却rowからだけHostを除く規則を明記した。著者は全生成/利用者順序規則を監査しmirrorを同期した。
