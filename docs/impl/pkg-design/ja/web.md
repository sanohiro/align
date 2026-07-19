このディレクトリは **first-party `pkg` ライブラリ**の設計仕様（`../../std-design/` と同じ深さ:
シグネチャ、Move/effect分類、エラーポリシー、スライス計画、落とし穴、テストアンカー）を置く場所。
first-party パッケージは本リポジトリで開発し、**システムと一緒に配布**する（pkg-foundation モデル:
利用者は `pkg/<name>/` をプロジェクトへコピー（vendoring）して取り込む。将来の fetch ツールはその
コピーを自動化するだけ）。あくまで通常の pkg 層パッケージ — 明示的に vendor され、暗黙解決は
されない。

# pkg — web

> 🌐 [English](../web.md) · **日本語**

**注意: 英語版 (`../web.md`) が正本。本書は参照用ミラーであり、乖離した場合は英語版に従う。**

## ステータス

**PROPOSAL（2026-07-20）** — 表面設計は内部整合済みだが、3つのフォーク（⚖）が owner 決裁待ち。
実行計画: `../../15-gateway-workspace-plan.md`（本書はその F2。**フレームワークが成果物** —
gateway アプリは明示的に後回し、owner 2026-07-20）。ハード前提: **F1**（非キャプチャ fn 値の
struct フィールド / 配列要素化）。F0（pkg-foundation 規則）は `internal` モジュールと階層強制を
有効にするが、パッケージ本体のビルドは妨げない。

## 概要

`pkg.web` は REST-API サーバフレームワーク: `std.http` のプロトコル床
（`serve`/`accept`/`ctx.*`/`respond`/`respond_stream`）の上の**ポリシー層**。層の規則
（2026-07-18 記録）: プロトコル（仕様で一意に決まるもの）は `std.http`、規約/ポリシー
（ルートパターン、ハンドラ形、レスポンス糖衣）は本パッケージ。プロトコルの再実装はしない。

参照: **Go 1.22 `net/http` ServeMux** が主参照 — メソッド対応パターンルーティング
（`{param}`）、自動 404/405、標準ライブラリ級の最小表面。chi/httprouter は性能参照
（radix tree — 延期。v1 は線形走査、落とし穴 P2）。middleware チェーン型（chi/gin/echo 流）は
v1 のモデルに**しない**: キャプチャする escaping クロージャ（延期中の言語機能）が必要で、かつ
Align には one-way な代替（ラッパ関数が次の関数を直接呼ぶ）がある。

ルートテーブルは**データ** — 可視な Copy struct 配列。登録の副作用なし、グローバルなし、
リフレクションなし（"nothing hidden"。コンパイラが全ルートを見る）:

```align
import pkg.web

fn list_models(ctx: http_request_ctx, params: slice<str>) -> Result<(), Error> { ... }
fn get_model(ctx: http_request_ctx, params: slice<str>) -> Result<(), Error> {
  id := params[0]                              // {id} キャプチャ（パターン順）
  ...
}

fn main() -> Result<(), Error> {
  routes := [
    web.get("/v1/models", list_models),
    web.get("/v1/models/{id}", get_model),
    web.post("/v1/chat/completions", chat),
  ]
  web.serve("127.0.0.1", 8080, routes)
}
```

## シグネチャ（提案）

```text
// ルート構築 — ⚖ フォーク A（推奨形を掲載。Forks 参照）
web.get(pattern, handler)    -> route
web.post(pattern, handler)   -> route
web.put(pattern, handler)    -> route
web.delete(pattern, handler) -> route

// 唯一のハンドラシグネチャ（"one way"）:
//   fn(ctx: http_request_ctx, params: slice<str>) -> Result<(), Error>
// `params` = {name} キャプチャの str view（パターン順・ctx のリクエストバッファに region 束縛）。
// ハンドラは ctx 経由で応答（std.http: ctx.respond / ctx.respond_stream — いずれも ctx を消費）。
// params の使用は消費より厳密に前（borrow チェックされる）。

// サービング
web.serve(host, port, routes) -> Result<(), Error>
//   Impure。逐次 accept ループ（v1 の記録済み方針）。リクエストごと: method+path をテーブルと
//   照合しディスパッチ。path 不一致 → 404、path 一致 method 不一致 → 405、ハンドラ Err → 500
//   （ベストエフォート応答後）。3 つの自動応答は固定の最小 JSON ボディ。

// リクエスト糖衣
web.body_str(ctx) -> Result<str, Error>      // ctx.body() を UTF-8 検証済み str view で
//   JSON 入力: req: ChatReq := json.decode(web.body_str(ctx)?)?   （core.json 直用。ラッパなし）

// レスポンス糖衣
web.json(ctx, x)              -> Result<(), Error>  // 200 + application/json + json.encode(x)。ctx 消費
web.status_json(ctx, code, x) -> Result<(), Error>  // ステータス明示版
web.no_content(ctx)           -> Result<(), Error>  // 204 空ボディ。ctx 消費
```

```text
// 型
route   — Copy struct { method（tag-only enum）, pattern: str, handler: fn(...) -> Result<(), Error> }
//        全フィールドが Copy（リテラルの str view・タグ・fn ポインタ）なので Copy:
//        ルートテーブルは素のデータ — リテラルで構築・保持・受け渡し可能。
```

**パターン構文（⚖ フォーク B、推奨形）:** `/` 区切りセグメント。リテラルセグメントはバイト一致、
`{name}` は非空 1 セグメントに一致しキャプチャ（出現順 = `params` の index）。Align 既存の
`template` リテラルの `{...}` ホール構文と一貫（「埋められる穴」の同じ視覚言語）で、Go 1.22
ServeMux とも一致。正規表現・省略可能セグメントなし。末尾ワイルドカード `{name...}`（Go 流）は
消費者が現れるまで延期。末尾スラッシュは厳密一致のみ（暗黙リダイレクトは隠れ挙動なので不採用）。

**マッチ規則（固定・非設定）:** 同位置ではリテラル優先（Go 規則 — `/v1/models/featured` が
`/v1/models/{id}` に勝つ）。同点になり得る 2 ルートは **serve 起動時 abort**（曖昧テーブルは
バグ。リクエスト時ではなく構築時に検出）。クエリ文字列はパターン対象外（std.http のクエリ床を
使用）。キャプチャの percent-decode は std 床に従う（std 前提参照）。

## Move/effect 分類

```text
route            Copy 値（str view + タグ + fn ポインタ）。drop されない。リテラルパターンなら
                 region = Static（計算されたパターン str は route を region 束縛 — 合法だが稀）
routes テーブル  array<route> / 固定配列 — Copy 要素の素データ
web.serve        Impure（ネットワーク）。テーブルは借用（消費しない）。Err まで走る
ハンドラ         Impure 可（本質的に I/O する）。格納 fn 値経由の呼び出し — effect ビットは
                 FnTy を通じて流れる（#465 の機構、出荷済み）
web.json ほか    Impure（ソケット書き込み）。ctx を消費（ctx.respond のミラー）
web.body_str     Pure。ctx に region 束縛された str view を返す
```

フレームワークは**純 Align** — `unsafe` なし、FFI なし、新規ランタイムシンボルなし。pkg 層が
ユーザコードにない特権を必要としないことの証明を兼ねる。

## エラーポリシー

- `web.serve` の `Err` はセットアップ失敗（bind/listen）のみ。リクエスト単位のエラーでループは
  死なない: ハンドラ `Err` → 最小 500 JSON（ベストエフォート）で継続、リクエスト行/ヘッダ不正
  （std.http パース `Err`）→ 400 で継続。
- 自動ボディは固定形の最小 JSON（`{"error":{"code":404}}` 風）。アプリのエラー形（例: OpenAI
  error object）はアプリのポリシー — `web.status_json` で構築。フレームワークは豊かなエラー語彙を
  定義しない。
- リクエストデータから到達可能な panic はゼロ: テーブル検証は起動時 abort（プログラマエラー）、
  リクエスト由来はすべて `Result`。

## std.http 前提（消費者到来につき std 側スライス。pkg.web のコードではない）

2026-07-18 に「消費者が来たら std 行き」と記録された項目 — `pkg.web` がその消費者:

1. `ctx.query(name) -> Option<str>` + percent-decode（RFC 3986 — プロトコル）。
2. SSE イベントフレーミングヘルパ（WHATWG 定義）— 最初のストリーミング消費者が必要とする時点
   （後の gateway アプリ。v1 の pkg.web 自身ではない → W4、v1 後ろへずれ得る）。
3. （W2 で確認）ディスパッチに足る path/method アクセサ — `ctx.method()` / `ctx.path()` は出荷済み。

## owner 決裁待ちフォーク（⚖）

- **A — ルート構築子:** メソッド別 `web.get/post/put/delete(pattern, handler)`（推奨: REST の
  普遍的な読み方。GET ルートの書き方が正確に 1 つ。stringly-typed なし）vs `Method` enum + 単一
  `web.route(m, pattern, handler)`。両方は持たない。
- **B — パターン構文:** `{name}`（推奨: `template` ホール + Go 1.22 と一貫）vs `:name`
  （Sinatra/Express/chi 系譜）。両方は持たない。
- **C — params の受け渡し:** パターン順の位置指定 `params: slice<str>`（推奨: ゼロアロケーション、
  map 型不要、順序はパターンに可視）vs 名前引き（`web.param(params, "id")` — 線形走査の糖衣。
  位置指定の**上に**後から非破壊で足せる）。v1 は位置指定のみで良いかを決裁。

## スライス（計画の F3。各スライス PR → review → merge）

- **W1 — 型 + マッチエンジン。** `route`/`Method`、パターン解析 + 検証、マッチャ
  （リテラル優先・曖昧 abort）を `str`/セグメント上の純関数として。ソケットなしで単体テスト
  可能。`route` struct 自体に F1（fn フィールド）が必要。
- **W2 — serve + ディスパッチ。** `std.http` serve/accept 上の accept ループ、method+path
  ディスパッチ、自動 404/405/400/500、params キャプチャ。統合テストは in-process サーバパターン
  （`crates/align_driver/tests/m11_http.rs`）。
- **W3 — リクエスト/レスポンス糖衣。** `body_str`、`json`、`status_json`、`no_content`。
- **W4 — SSE 糖衣**（std の SSE フレーミング床と共に/後に）— 最初のストリーミング消費者にゲート。
  v1 ではなく gateway アプリと同時になり得る。
- **W5 — 堅牢化 + ベンチ。** テーブル端例マトリクス（曖昧 abort、空テーブル、深いパス、長い
  セグメント）。`bench/web_router` — 手書き `match` 比のディスパッチオーバーヘッドは
  ほぼゼロであること（フレームワークの存在証明）。数値を記録。

## 落とし穴

- **P1 — ハンドラ fn 型は 1 つのまま。** `fn(http_request_ctx, slice<str>) -> Result<(), Error>`
  — アプリごとのジェネリックなハンドラ形に抗う。アプリ状態の受け渡し（DB プール等）は後の
  意図的設計（おそらく明示 state param — fn 型と route struct が変わるので、アプリごとの
  ドリフトではなく 1 回の決定にする）。
- **P2 — 線形走査は v1 として正しく、手抜きではない。** 小さい固定 REST テーブル（< ~100
  ルート）は木の構築償却より走査が速い。ベンチ（W5）が交差点の証拠を記録。radix tree は
  **計測後**の follow-up（chi/httprouter 参照）でありデフォルトではない。
- **P3 — params は ctx に region 束縛された view。** `ctx.respond` 消費後に param を保持する
  ハンドラは設計どおり borrow エラー（#460 の liveness 機構が検出）。脱出口は `.clone()` と
  文書化 — 先回りコピーで「直さない」。
- **P4 — 暗黙のレスポンス変形なし。** ヘルパは `ctx.respond` と同様に ctx を消費（Move 規律）。
  ctx にレスポンスビルダを持たせるパターンはない — ヘッダが要るハンドラは std.http の
  `response_builder` を直接使う。
- **P5 — 起動時検証は全域。** テーブル欠陥（重複、曖昧ペア、不正/空パターン）はすべて該当
  パターン名入りで `serve` 起動時 abort — リクエスト時のサプライズにしない。

## テストアンカー（計画）

`apps/gateway/pkg/web/` 併設の unit 風 example エントリ（スライスごと）、W2 のディスパッチ
マトリクス駆動統合テスト（`apps_web_*`）、`bench/web_router`（W5）。gateway アプリ（F4、後）が
全表面の検証消費者。
