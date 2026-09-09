# HTTPクライアント所有値のパッケージ境界

> 英語版 `../http-client-composition.md` が正本です。
> 状態: 提案、2026-09-09。実装済みの名前付き時刻形式とパスエンコードに続く、
> S3/SigV4 の前提機能です。

## 根拠と機能境界

現在のコンパイラでは `http.client()` と `http.request(...)` を型推論される
ローカル所有値として利用できますが、ヘルパーの引数型として名前を記述できません。
リクエストを `Result` で返すことも、解析済みレスポンスの型名を記述することもできません。
`prepare() -> Result<http_request, Error>` と
`send(borrow mut client: http_client, req: http_request) -> Result<http_response, Error>`
を試すと、3種類の未知の型と不正なリクエストペイロードが診断されます。
これはライブラリの合成機能の不足であり、所有権や並列モデルの変更ではありません。

1つの実装でクライアント・リクエスト・レスポンスの関数境界を閉じます。
署名処理は失敗し得るリクエストを返し、呼び出し側は明示的なクライアントを再利用し、
解析処理は完了したレスポンスを借用できます。S3 がなくても通常の HTTP パッケージで
独立に利用できる境界です。型名、ペイロード、ネイティブ消費、インポートされたヘルパーを
分割すると、不完全または不健全な中間状態になります。手書き差分が1,000行を超えても、
3種類に共通する所有権と永続化の証明を1回で閉じる方が、ゲートの反復より統合リスクを減らします。

## 正式な公開契約台帳

別名、新コンストラクタ、暗黙のクライアント・認証情報・時計、再試行方針、HTTPプロトコル、
S3 API は追加しません。

| 公開面 | 入力・既定値・結果・エラー | 所有権・寿命・割り当て | 担当・識別・検証 |
|---|---|---|---|
| `http_client` | 既存の `http.client()` の結果を表す、型引数なしのグローバル組み込み型名。型名の記述に import は不要で、構築には既存の import が必要。不正な型引数はコンパイルエラー。 | 既存プールを所有する公称の不透明な Move 値。移動は割り当てなし。Drop は既存の解放を正確に1回実行。Copy・等値比較・表示・raw変換・暗黙の複製は不可。 | 新しい `Scalar::HttpClient` と既存の `Ty::HttpClient`。H1、O1、I1。 |
| `http_request` | 既存の `http.request(method: str, url: str)` の結果のグローバル型名。型引数なし。構築は従来どおり全域的で、検証はシリアライズ・送信時、setter の中断規則も不変。 | method、URL、ヘッダ、コピーした本文を所有。ヘルパーから返却可能。移動は割り当てなし。未完了の Drop は既存の request free を正確に1回実行。コンストラクタとsetterの明示的なコピーは従来どおりで、入力ビューは保持しない。 | 新しい `Scalar::HttpRequest` と既存の `Ty::HttpRequest`。H1、O1、W1、I1。 |
| `http_response` | 既存の `http.parse` と本文一括受信の成功値のグローバル型名。型引数なし。`http.response(status)` が構築する既存の `response_builder` とは別型。 | 既存の公称 Move レスポンス。新しい scalar 表現は不要。ヘッダ・本文ビューはその正確なストレージルートを借用。移動はコピーなし、Drop は既存の解析済みレスポンス解放。 | 既存の `Scalar::HttpResponse` と `Ty::HttpResponse`。H1、O1、V1、I1。 |
| 3型の所有キャリア | ローカル、値渡し引数・戻り値、再帰的に所有するレコード・ユーザーsum・タプル・`Option`・`Result`。ソースで構築した固定長の Move レコード配列は既存の移動パス規則を使用。ジェネリック置換後も同じ。 | 生きた各所有値は移動、移動元の無効化、Drop が正確に1回。部分移動、再初期化、置換、早期終了を含む。借用から所有権を生成しない。 | 既存機構に2つの scalar 葉を追加。直接のハンドル集合要素、所有レコードの動的集合、所有値のbox、定数・グローバル、脱出するcapture、task・並列capture、公開externは既存規則で拒否。既存のバッチ専用 `array<http_response>` は維持し、client/request配列表現は追加しない。H1、O1、M1。 |
| 借用ヘルパー引数 | 既存の `borrow`・`borrow mut` と寿命推論を使用。借用所有値を消費、所有値として返却、Drop、置換することは不可。返したレスポンスビューは入力ルートを保持。raw/SSEストリームの既存キャリア・起源規則も維持。 | 新たな割り当て・参照カウント・ネイティブシェルなし。呼び出し側が所有。可変貸出は、保持されたストリームを含む貸出寿命中、重なる共有・可変アクセスと移動・Drop・置換を排除。 | 借用・移動・region検査とインポートされた所有権要約。V1、M1、I1。 |
| clientメソッド | `get`、`post`、`request`、`request_stream`、`get_many`、`timeout`、`max_response_body_bytes` は借用引数に対して排他的な `borrow mut` が必要。共有 `borrow` では不可。所有ローカル・値渡し引数に新たな `mut` 束縛は不要。引き続き束縛済みローカルのみ。 | 完了レスポンスはclientから独立。ストリーミングレスポンスは既存のclient貸出を保持。requestを受け取る両メソッドは、通信失敗時も正確に1回消費。未完了ストリームを無効化するclientアクセスは不可。 | 既存HIR・native操作。インポート・ジェネリックにも同じ排他性とregion規則。通信はImpure、setterはPure。M1、V1、W1。 |
| requestメソッド | `header`、`body`、`timeout`、`max_response_body_bytes` は借用された束縛済みローカルに `borrow mut` を要求。`serialize() -> Result<buffer, Error>` は読み取りのみで共有借用可。requestを消費しない。 | setterは入力を呼び出し中にコピーし入力貸出を保持しない。シリアライズ結果は独立所有。共有・可変借用されたrequestを消費してはならず、native消費操作には所有する値のみを渡す。 | 既存操作・割り当て・エラー契約。M1、W1、V1。 |
| responseメソッド | `status() -> i64`、`header(name: str) -> Option<str>`、`body() -> slice<u8>` は共有借用された束縛済みローカルで利用可能。他の受信者規則は不変。検索名は保持しない。 | statusは独立Copy。ヘッダ・本文ビューは、ヘルパー戻り値、フィールド、Option、合流、importを通してresponseルートを保持。生きたビューがある間のルートの移動・置換・Dropを拒否。完了responseはclient/request/入力bufferを借用しない。 | 既存region生成と保持ルート要約。V1、I1。 |

`borrow mut` 引数を持つヘルパーへの呼び出しには、呼び出し側の可変ストレージ
(`mut client := http.client()`) が必要です。所有ローカルの既存メソッドを直接呼ぶ規則とは別です。

受信者と引数はソース順に各1回評価します。型、借用、不正HIR、所有権の違反はnative出力前の
コンパイルエラーです。実行時エラーや優先順位は追加しません。URL、ヘッダ、timeout、上限、通信、
TLS、フレーミング、割り当て失敗は既存HTTP台帳の規則に従い、OOMは中断します。

## 表現と永続化

3型は異なる公称組み込み型です。native表現が同じ1ポインタでも同一型にはなりません。
既存の構築・メソッド・消費・解放のABI、シンボル、実行時fingerprint、capabilityは変更しません。
解析済みresponseのDropは `align_rt_http_resp_free` です。
`align_rt_http_response_free` は別の `response_builder` 用です。

2つのscalar追加は、型・分類・レイアウト・所有権・native envelope・生成元検証・interfaceの
全経路を更新します。新しい式・rvalue opcodeやnativeシンボルは不要です。clientをrequestや
responseとして扱う偽造、借用の不正消費、無効なnative入出力ストレージをLLVM生成前に拒否します。
ポインタのサイズ一致だけでは意味上の型同一性を証明できません。

正規scalar codecに、ペイロードなしの1バイトタグ49 (`http_client`) と50 (`http_request`) を
追加します。responseは27のまま。51～255は未知のタグです。独立した双方向goldenは、順に
16進 `31`、`32`、`1b` を固定します。不完全な外側レコードと未知タグは拒否します。
encoding変更規則に従いinterface `FORMAT_VERSION` を10から11に進め、既存タグは振り直しません。
公称型、順序付き集約型定義、引数モード、ジェネリック本体、保持ルート要約は既存のinterface・
依存・object cache identityに入ります。cold/warm/edit/restoreと全体コンパイルは一致し、
旧interfaceはmissまたは検証失敗となり、別ハンドルへ読み替えません。実行時I/Oは増えません。

## 実装閉包マトリクスと検証項目

主担当は `align_driver --test http_client_composition`。以下は予定される正確なテスト名です。
変更による欠陥を検出できる既存ownerは再利用でき、preflight前に実装箇所と最終対応を記録します。

| ID | 検証軸 | 正確なowner |
|---|---|---|
| H1 | 3型名・arity、直接・再帰キャリア、ジェネリック、禁止集合・box・capture・parallel・extern・型混同の拒否 | `formation_and_carrier_matrix` |
| O1 | 構築、移動、移動元無効化、再帰Drop、部分移動・再初期化、置換、返却、if/match/else/?/map_err、分岐・loop合流、早期終了 | `ownership_control_flow_whole_and_unit` |
| M1 | 全client/requestメソッドの共有・可変権限、serialize/responseの共有読取、借用消費と同一ルート重複の拒否 | `borrow_authority_and_consumption_matrix` |
| V1 | responseビューの戻り寿命、独立status/serialize/完了response、helper/import/genericを通したstreamのclient貸出、無効化の拒否 | `retained_views_and_stream_origins` |
| W1 | 返したrequestのシリアライズと呼び出し側clientからの送信、literal/owned/NUL/binary、成功・失敗時の消費、プール再利用、全体/単位・最適化あり/なし | `package_request_and_pool_wire_round_trip` |
| I1 | import/genericのモード、完全公称型グラフ、codecタグと不正レコード、cache復元、借用ルートの保持 | `interfaces_and_cache_restore`; `align_mir --lib canonical_field_codec_covers_every_primitive_and_scalar_tag` と `canonical_type_codec` |
| P1 | HIR型、nativeアクセスとslot証明、型の置換、欠けた証明、借用消費偽造の拒否 | `align_mir --lib hir_body_validator_native`; `align_codegen_llvm --lib xml_mir_gate_authenticates_types_on_every_producer_edge` |
| A1 | ポインタABIと解放、移動の無割り当て、成功・失敗消費時の二重解放・リーク防止、部分移動cleanup | `ownership_control_flow_whole_and_unit` と既存HTTP lifecycle owner。構造検証でcleanupを識別できない場合は既存割り当てprobeを使用。 |

速度やピークメモリの改善は約束せず、benchmark gateは不要です。新しい実行時割り当て経路はなく、
新しいグローバルprobe ABIも不要です。所有権テストはルート喪失や二重所有の実際の欠陥を検出し、
ソースの文字列出現数だけを証拠にしてはいけません。

## 使用例と同期

次は提案APIの例であり、実装済みの主張ではありません。

```align
module main
import std.http

fn prepare(url: str) -> Result<http_request, Error> {
  req := http.request("GET", url)
  req.header("accept", "application/octet-stream")
  return Ok(req)
}

fn send(borrow mut client: http_client, req: http_request) -> Result<http_response, Error> {
  return client.request(req)
}

fn main() -> Result<(), Error> {
  mut client := http.client()
  req := prepare("https://example.com/object")?
  response := send(client, req)?
  print(response.status())
  return Ok(())
}
```

既存parserはこの構文を受け入れ、欠けている型・ペイロード機能のみを診断します。
実装ownerでは同じ構成を隔離loopbackで全体・単位コンパイルし、実行します。

同期対象は英日両台帳、`http.md` とその日本語版、`draft.md`、`docs/language-spec.md`、
`docs/design-notes.md`、`docs/open-questions.md` のSettled、`docs/impl/07-roadmap.md`。
HIR・runtime ABI台帳は規範的契約が変わる場合のみ更新します。HANDOFFは受理された機能境界で
前提作業を1回記録します。S3のendpoint・認証情報・正規リクエスト・署名・response/status・
相互運用契約は、この前提の後に別のパッケージ設計で確定します。
