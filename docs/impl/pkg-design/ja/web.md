このディレクトリは **first-party `pkg` ライブラリ**の設計仕様（`../../std-design/` と同じ深さ）を
置く場所。first-party パッケージは本リポジトリで開発し、**システムと一緒に配布**する
（pkg-foundation モデル: 利用者は `pkg/<name>/` をコピー（vendoring）して取り込む。将来の fetch
ツールはそのコピーを自動化するだけ）。あくまで通常の pkg 層パッケージ — 明示的に vendor され、
暗黙解決はされない。

# pkg — web

> 🌐 [English](../web.md) · **日本語**

**注意: 英語版 (`../web.md`) が正本。乖離した場合は英語版に従う。**

## ステータス

**DESIGN v2（2026-07-20、owner 指示）。** 失われた会話記録から復元し、二度と失わないようここに
固定する owner の要求: **成果物は「zero-copy のくっそ高速な REST フレームワーク」— 速度が主題で
あり副産物ではない。** 主参照は **Go の Fiber**（基盤 fasthttp の zero-allocation 哲学 + Express
系 API）。router は **httprouter/fasthttp 系 radix tree** を参照（意図的に別参照 — フレームワーク
モデルは Fiber、ディスパッチは radix router）。gateway / LLM アプリは単なる最初の消費者
（「それで作るものが LLM 系というだけ」）— 本設計を規定しない。実行計画: `../../15-pkg-web-plan.md`。
ハードなコンパイラ前提: **F1 フィールド許可拡張**（前提の節参照）。

## なぜ Align が勝てるか

Fiber が速いのは fasthttp がリクエスト毎アロケーションを拒否しバッファを再利用するから — だが Go は
GC・interface boxing・`string([]byte)` 境界のコピーを払い続ける。Align では Fiber の規律が
**デフォルト**になる: `std.http` は既にリクエストを 1 バッファ + オフセット表にパースし（R1）、
全アクセサは `str`/`slice<u8>` **view**（構造的に zero-copy。region により view の漏出はコンパイル
エラー）、SIMD JSON は view を持つ struct へ直接デコード、リクエスト毎 `arena {}` は O(1) リセット、
GC は存在しない。フレームワークの仕事は**その連鎖を壊さずに routing + エルゴノミクスを足す**こと:
ホットパスでリクエストバイトのコピーもヒープ接触も禁止。これが存在証明であり、W5/W7 が計測・
回帰固定された数値にする。

## パフォーマンス契約（設計不変条件、ベンチで固定）

```text
1. リクエストバイトのコピーゼロ   — path・params・query・headers・body すべてリクエスト
                                    バッファへの view。フレームワークはリクエストデータから
                                    文字列を実体化しない（.clone() はアプリの明示的脱出口）
2. リクエスト毎ヒープ割当ゼロ     — ホットパスはヒープ割当なし。リクエスト毎スクラッチは
                                    リクエスト arena（O(1) 一括リセット）
3. O(セグメント数) ディスパッチ   — 起動時構築の radix tree（static > param > wildcard 優先、
                                    httprouter 規則）。リクエスト時のパターン解析なし・regex
                                    なし・map なし。param 値は固定スロット配列へ
4. zero-copy 出力                 — レスポンスボディはレスポンスライタへ直接エンコード
                                    （library-foundations の zero-allocation output パターン）
5. 起動時全域検証                 — route tree は serve() で一度だけ構築・検証（衝突/曖昧は
                                    abort）。リクエストパスは検証作業をしない
```

ベンチアンカー（W5/W7）: `bench/web_router`（手書き `match` 比 — 誤差内必須）、`bench/web_e2e`
（素の `std.http` ループ比 req/s — フレームワークのオーバーヘッド ≈ ゼロ必須。加えて同一マシンの
Go Fiber 比較 — plaintext + JSON echo で **Fiber と同等以上**が目標）。

## 表面（Fiber 参考、Align 流儀）

```align
import pkg.web

// ハンドラ: 唯一のシグネチャ — fn(c: web.Ctx) -> Result<(), Error>
fn get_model(c: web.Ctx) -> Result<(), Error> {
  id := web.param(c, "id")               // リクエスト path への str view
  m := lookup(id)
  web.json(c, m)                          // エンコード → レスポンスライタ。c を消費
}

fn main() -> Result<(), Error> {
  routes := [
    web.get("/v1/models", list_models),
    web.get("/v1/models/:id", get_model),
    web.post("/v1/chat/completions", chat),
  ]
  web.serve("127.0.0.1", 8080, routes)
}
```

app オブジェクトなし・登録副作用なし・グローバルなし・リフレクションなし: ルートテーブルは可視の
Copy struct 配列**値**で、`serve` が起動時に radix tree へコンパイルする。（Align にユーザ定義
メソッドはないので Fiber の `c.Params("id")` は修飾呼び出し `web.param(c, "id")` になる — 同じ
エルゴノミクス、1 つの呼び出し規約。）

## シグネチャ

```text
// ルート（メソッド別構築子 — Fiber/Express の読み方。GET ルートの書き方は 1 つ）
web.get(pattern, handler)     -> route
web.post(pattern, handler)    -> route
web.put(pattern, handler)     -> route
web.delete(pattern, handler)  -> route
web.patch(pattern, handler)   -> route

// グループ（純データ: prefix + routes → prefix 付き routes。クロージャ不要）
web.group(prefix, routes)     -> array<route>

// サービング — Impure。v1 は逐次 accept（並行化は記録済みの計測付き follow-up）
web.serve(host, port, routes) -> Result<(), Error>
//   起動時: radix tree 構築 + 検証（重複/曖昧 → パターン名入り abort）。リクエスト毎:
//   パース（std.http、zero-copy）→ radix ディスパッチ → ハンドラ。自動応答: path 不一致 → 404、
//   path 一致 method 不一致 → 405（Allow 付き）、パース失敗 → 400、ハンドラ Err → 500。
//   固定の最小 JSON ボディ。ループはリクエスト単位で死なない。

// ctx アクセサ（全て Pure。全て c に region 束縛された view を返す）
web.param(c, name)   -> str              // :param キャプチャ（固定スロット配列。total —
                                         //   パターンにない名前は起動時検出可能なバグ）
web.query(c, name)   -> Option<str>      // std.http クエリ床（RFC 3986 percent-decode 済み）
web.header(c, name)  -> Option<str>
web.body(c)          -> slice<u8>
web.body_str(c)      -> Result<str, Error>    // UTF-8 検証済み view
//   JSON 入力: req: ChatReq := json.decode(web.body_str(c)?)?   — core.json、view デコード

// レスポンダ（Impure。c を消費 — Move 規律、ctx.respond のミラー）
web.json(c, x)               -> Result<(), Error>   // 200 + application/json + json.encode(x)
web.status_json(c, code, x)  -> Result<(), Error>
web.text(c, s)               -> Result<(), Error>   // 200 + text/plain
web.status(c, code)          -> Result<(), Error>   // ステータス + 空ボディ
```

```text
// 型
web.Ctx    — リクエスト毎コンテキスト struct: std.http リクエストハンドル + param スロット配列
             （名前はマッチしたルート由来 — Static。値は path への view）。Move struct
             （リクエストハンドルを所有）。レスポンダがちょうど 1 回消費。
Route      — Copy struct { method（tag-only enum）, pattern: str, handler: fn(Ctx) -> Result<(), Error> }
```

**パターン構文（Fiber/httprouter 系譜 — 復元された参照により決定）:** `/` 区切り。リテラルは
バイト一致。`:name` は非空 1 セグメントに一致しキャプチャ。末尾 `*name` は残り全部をキャプチャ
（tail wildcard）。各ノードの優先順位: **static > `:param` > `*wildcard`**（httprouter 規則 —
`/v1/models/featured` が `/v1/models/:id` に勝つ）。同点になり得る 2 ルート → 起動時 abort。
regex なし・省略可能セグメントなし・末尾スラッシュ厳密一致（隠れリダイレクトなし）。クエリ文字列は
パターン対象外。

## 前提（コンパイラ / std — 土台）

- **F1 — フィールド許可拡張（唯一のハードな言語スライス）。** `web.Ctx` と `Route` は現行
  ホワイトリスト外の struct フィールドを要する（2026-07-20 実測: fn フィールドは
  "struct fields must be a primitive scalar, str, or a plain struct" エラー）: ① **fn 値**
  フィールド（Copy ポインタ — `Route.handler`。effect ビットは FnTy 経由、#465）、② **Move
  ハンドル**フィールド（`Ctx` 内の `http_request_ctx` — `Ctx` は Move struct になる。Move
  フィールドの drop/move 機構は J3a の Move-enum フィールドで実証済み）、③ **`slice<str>`**
  フィールド（param スロット — view slice、`str` フィールド同様に region 追跡）。いずれも既存
  分類機構の再利用で、スライスは `is_field_ok` + layout/drop/region の掃引を広げる。キャプチャ
  する escaping クロージャは対象外のまま（延期継続）。
- **F0 — pkg-foundation 規則**（`internal` + 階層 import チェック + 仕様文書）: `pkg.web.internal.*`
  モジュール（radix tree の置き場所）を可能にする — F1 と並行可。
- **std.http 床（消費者到来）:** `ctx.query` + percent-decode（プロトコル → std）。SSE イベント
  フレーミング（WHATWG）は最初のストリーミング消費者と共に（LLM アプリ — W6+）。

## Move/effect 分類

```text
Route / テーブル   Copy データ（リテラル str view + タグ + fn ポインタ）。Static。drop なし
radix tree         serve 内で一度構築（arena か起動時ヒープ所有。serve 終了時解放）
web.Ctx            Move struct（リクエストハンドル所有）。serve がリクエスト毎に生成、
                   レスポンダがちょうど 1 回消費。params は view（drop なし）
web.serve          Impure。テーブル借用。セットアップ Err まで走る
アクセサ           Pure。c に region 束縛された view（レスポンダ消費後の脱出 = コンパイルエラー）
レスポンダ         Impure。c を消費
ハンドラ           Impure 可。Route.handler 経由呼び出し（FnTy effect ビット、fail-closed）
```

フレームワークは**純 Align** — `unsafe` なし、FFI なし、新規ランタイムシンボルなし。

## エラーポリシー

`serve` の `Err` はセットアップのみ（tree 構築失敗は起動時 abort — プログラマエラー）。
リクエスト毎: 不正リクエスト → 400、不一致 → 404/405、ハンドラ `Err` → 500 — 固定最小 JSON、
ループ継続。アプリのエラー語彙（OpenAI error object 等）は `web.status_json` で構築するアプリ
ポリシー。リクエスト由来の panic はゼロ。

## Middleware（設計は今、着地は後 — W6）

Fiber の `c.Next()` チェーンはキャプチャクロージャ（延期）を要する。v1 互換モデルは
**非キャプチャ pre-handler リスト**を Move で連鎖: `fn(c: Ctx) -> Result<Option<Ctx>, Error>` —
続行なら `Some(c)`（ctx を返却）、応答済み停止なら `None`、`Err` は 500。グループが保持:
`web.group_with(prefix, [auth, log], routes)`。auth/logging/CORS ヘッダをクロージャなしで賄う。
状態付き middleware はキャプチャクロージャ機能と実消費者を待つ。F1 検証時に `Option<Ctx>`
（Move struct の Option）を確認 — ギャップがあれば代替形は 2 variant enum
`Verdict { Proceed(Ctx), Done }`（Move-enum payload は J2 で出荷済み）。

## スライス（計画の F3）

- **W1 — router コア。** パターン解析 + 検証。**radix tree**（static/param/wildcard ノード、
  優先順位、衝突検出）+ マッチャをセグメント上の純関数として。param スロットキャプチャ。線形
  走査オラクルとの差分テスト。F1① が必要。
- **W2 — Ctx + serve + ディスパッチ。** `web.Ctx`（F1②③ が必要）。std.http 上の accept ループ。
  自動 404/405/400/500。`group`。統合テストは in-process サーバパターン
  （`crates/align_driver/tests/m11_http.rs`）。
- **W3 — アクセサ + レスポンダ。** param/query/header/body/body_str、json/status_json/text/status。
- **W4 — 堅牢化。** route-tree 端例マトリクス（衝突、深いパス、長セグメント、空テーブル、`*`
  tail、メソッド集合）。不正リクエストマトリクス。keepalive 再利用。
- **W5 — router/e2e ベンチゲート。** `bench/web_router` + `bench/web_e2e`（素の std.http 比 ≈
  ゼロオーバーヘッド必須）— パフォーマンス契約を回帰固定。
- **W6 — middleware-lite + SSE 糖衣**（std SSE 床と共に）— 最初の消費者にゲート。
- **W7 — 外部比較。** 同一マシン Fiber（Go）の plaintext + JSON echo ベンチ。数値とギャップ分析を
  本書に記録。

## 落とし穴

- **P1 — ハンドラシグネチャは永遠に 1 つ。** `fn(Ctx) -> Result<(), Error>`。アプリ状態
  （DB プール等）は将来の 1 回の意図的決定（おそらく明示 state param で fn 型を一度だけ変える）。
- **P2 — radix tree は設計であり最適化ではない。** 線形走査は W1 の差分テストオラクルとして
  のみ存在。（Fiber/httprouter はまさにディスパッチの参照。）
- **P3 — params/view の脱出規律。** レスポンダ消費後まで view を保持するのは設計どおりコンパイル
  エラー（#460 liveness）。脱出口は `.clone()` と文書化 — 「安全のため」の先回りコピーは不変条件 1
  を壊すので絶対にしない。
- **P4 — 隠れたレスポンス状態なし。** レスポンダは `Ctx` を消費（Move）。ctx 内ビルダ変異
  パターンなし。糖衣を超えるヘッダは std.http `response_builder` を直接。
- **P5 — ホットパスは何も割り当てない。** 各 W スライス PR は自分のバイトの居場所（view / arena /
  起動時）を明記。W5 ベンチが強制するが、レビューが先に確認する。
- **P6 — 405 は tree のパス毎メソッド集合が必要**（Allow ヘッダ）— W1 のノードレイアウトに設計
  時点で入れる。W4 で後付けしない。

## テストアンカー（計画）

ワークスペース `apps/web/`（フレームワーク作者ワークスペース: `pkg/web/` + 併設 example/test
エントリ）。driver 統合テスト `apps_web_*`（W2/W4 マトリクス）。`bench/web_router` /
`bench/web_e2e`（W5）/ Fiber 比較（W7）。LLM gateway アプリ（後日・別）は最初の全表面消費者 —
本パッケージの受け入れ条件ではない。
