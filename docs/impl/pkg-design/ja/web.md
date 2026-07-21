このディレクトリは **first-party `pkg` ライブラリ**の設計仕様（`../../std-design/` と同じ深さ）を
置く場所。first-party パッケージは本リポジトリで開発し、**システムと一緒に配布**する
（pkg-foundation モデル: 利用者は `pkg/<name>/` をコピー（vendoring）して取り込む。将来の fetch
ツールはそのコピーを自動化するだけ）。あくまで通常の pkg 層パッケージ — 明示的に vendor され、
暗黙解決はされない。

# pkg — web

> 🌐 [English](../web.md) · **日本語**

**注意: 英語版 (`../web.md`) が正本。乖離した場合は英語版に従う。**

## ステータス

**DESIGN v2（2026-07-20、owner 指示。同日、帰属を訂正）。** 失われた会話記録から復元し、二度と
失わないようここに固定する owner の要求: **成果物は「Align らしくデータ志向な、zero-copy の
くっそ高速 REST フレームワーク」— 速度が主題で、無駄（bloat）は拒否**（最小表面。投機的機能なし）。
**参照は器具であって命令ではない:** owner が「既存でこれに当たるのは何か」と聞いた時に *Claude* が
**Go の Fiber**（fasthttp の zero-allocation 哲学）を最近似として挙げた — owner の選択ではなく、
より良い既存参照があれば随時差し替えてよい。router の参照も同様に器具的: **httprouter/fasthttp**
（radix 系譜）と Rust の **matchit**（最小・最速級の radix マッチャ）。全決定の判定基準は
Align らしさ（データ志向 / nothing hidden / one way / 最小）であり、「フレームワーク X が
そうだから」は理由にならない。**router は第一級の要件**: 最初の消費者アプリ（OpenAI 互換・固定
パス）には不要でも、REST フレームワークには必須 — だから後付けではなく Align らしい設計
（下記）を与える。gateway / LLM アプリは単なる最初の消費者（「それで作るものが LLM 系という
だけ」）— 本設計を規定しない。実行計画: `../../15-pkg-web-plan.md`。ハードなコンパイラ前提:
**F1 フィールド許可拡張**（前提の節参照）。

## 最小主義（owner 制約）

表面は正確に: routing、ctx アクセサ、レスポンダ、middleware-lite、SSE 糖衣 — それだけ。
テンプレートエンジン・静的ファイル・セッション・websocket・ORM フック・設定システム・ライフ
サイクルコールバックは**なし**: 消費者が要求したら別パッケージ。全追加は消費者名を挙げること。
「フレームワークには普通ある」は理由にならない（"one way" + no-bloat 要求）。

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
3. O(セグメント数) ディスパッチ   — 起動時構築の radix 構造（static > param > wildcard 優先、
                                    httprouter/matchit 規則）を **Align 流**に格納: フラットな
                                    連続配列（node 表 + offset 参照の edge 表）でポインタ
                                    追跡なし — router 自体がデータ志向（cache-line に優しい
                                    walk。soa/tape/offset-table と同じ設計手筋）。リクエスト時の
                                    パターン解析なし・regex なし・map なし。param 値は固定
                                    スロット配列へ
4. zero-copy 出力                 — レスポンスボディはレスポンスライタへ直接エンコード
                                    （library-foundations の zero-allocation output パターン）
5. 起動時全域検証                 — route tree は serve() で一度だけ構築・検証（衝突/曖昧は
                                    abort）。リクエストパスは検証作業をしない
```

ベンチアンカー（W5/W7）: `bench/web_router`（手書き `match` 比 — 誤差内必須）、`bench/web_e2e`
（素の `std.http` ループ比 req/s — フレームワークのオーバーヘッド ≈ ゼロ必須。加えて同一マシンの
Go Fiber 比較 — plaintext + JSON echo で **Fiber と同等以上**が目標）。

**リクエストハンドルを誰が所有するか（オーナー決定、2026-07-20）: `serve` である。** 最初の実装は
ハンドラに所有させていた — `Ctx` がハンドルを所有する Move struct で、レスポンダがそれを消費した。
その形でフレームワークを作ると、根本原因が同一の行き止まりが 3 つ生じた: アクセサはすべてコンテキ
ストから借用するので `web.param(c, name)` はハンドラがまだ応答に使うコンテキストを消費してしまう。
param を読んでから応答することは端的に拒否される（`c` を move する時点で借用が生きている）。そして
「ハンドラ Err → 500」が実装できない — 失敗した時点でハンドラは既にハンドルを消費しており、応答
する手段が残っていないからである。ハンドルを `serve` に移すと 3 つとも解消する: ハンドラはリクエ
ストの関数となってレスポンスを組み立て、接続を保持し続けているフレームワークがそれを書き込むか、
500 を返す。これに必要だったコンパイラ側の enabler が、`response_builder` を型名として書けるように
し `Result` のペイロードとして許可することだった（`docs/impl/std-design/http.md`）。

## 表面（Fiber 参考、Align 流儀）

```align
import pkg.web

// ハンドラ: 唯一のシグネチャ — fn(c: web.Ctx) -> Result<response_builder, Error>
// ハンドラはレスポンスを**組み立てて返す**。書き込むのはフレームワーク。
fn get_model(c: web.Ctx) -> Result<response_builder, Error> {
  id := web.param(c, "id")               // リクエスト path への str view
  m := lookup(id)?                        // `?` が使える: 失敗は 500 になる
  web.json(json.encode(m))
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
//   パース（std.http、zero-copy）→ request-target を path と query に分割 → radix ディスパッチ
//   → ハンドラ → 返されたものを**書き込む**。自動応答: path 不一致 → 404、path 一致 method
//   不一致 → 405（Allow 付き）、ハンドラ Err → 500。ループはリクエスト単位で死なない。

// ctx アクセサ（全て Pure。全て c に region 束縛された view を返す）
web.param(c, name)   -> str              // :param キャプチャ（固定スロット配列。total —
                                         //   パターンにない名前は起動時検出可能なバグ）
web.query(c, name)   -> Option<str>      // std.http クエリ床（RFC 3986 percent-decode 済み）
web.header(c, name)  -> Option<str>      // 未出荷 — 下の注記を参照
web.body(c)          -> slice<u8>        // 2026-07-21 出荷: `Ctx.body` が zero-copy view を運ぶ
web.body_str(c)      -> Result<str, Error>    //   body_str = `.as_str()`(検証済み view)
//   JSON 入力: req: ChatReq := json.decode(web.body_str(c)?)?   — core.json、view デコード
//   `web.header` のブロッカー(2026-07-21 記録): Copy の `Ctx` は何も所有せず、任意名の
//   ヘッダー lookup は `body` のような単一の保存 view には乗らない。raw head の view フィールド +
//   pkg.web 側の RFC 9110 lookup(std.http の lookup の複製 — One way に反する)か、パース済み
//   ヘッダーテーブルを切り離した view として公開する std.http enabler(理想形 — 例:Ctx が運べる
//   view 値としての `ctx.headers()`)が要る。enabler を先に設計すること。第二の lookup は出荷しない。

// レスポンダ（Pure。レスポンスを**組み立てる**だけでリクエストハンドルに触れないので、ハンドラは
// アクセサとレスポンダを任意の順序で何度でも呼べる）
web.json(body)               -> Result<response_builder, Error>  // 200 + application/json
web.status_json(code, body)  -> Result<response_builder, Error>
web.text(s)                  -> Result<response_builder, Error>  // 200 + text/plain
web.status_text(code, s)     -> Result<response_builder, Error>
web.status(code)             -> Result<response_builder, Error>  // ステータス + 空ボディ
//   `body` は値ではなく**エンコード済み**の文書である: Align にユーザ定義ジェネリクスは無いので
//   `x` 自身をエンコードする `json(x)` は表現できない。また `web.json(json.encode(m))` の方が読み
//   としても良い — エンコードの確保がハンドラ内で可視のままになる（Nothing hidden）。
```

```text
// 型
web.Ctx    — リクエスト毎コンテキスト: view だけを持つ **Copy** struct（method, path, query,
             およびマッチしたパターン）。何も所有しない — リクエストハンドルは `serve` に留まり、
             view はハンドラ呼び出しの間有効である。
Route      — Copy struct { method: str, pattern: str,
                           handler: fn(Ctx) -> Result<response_builder, Error> }
```

**パターン構文（Fiber/httprouter 系譜 — 復元された参照により決定）:** `/` 区切り。リテラルは
バイト一致。`:name` は非空 1 セグメントに一致しキャプチャ。末尾 `*name` は残り全部をキャプチャ
（tail wildcard）。各ノードの優先順位: **static > `:param` > `*wildcard`**（httprouter 規則 —
`/v1/models/featured` が `/v1/models/:id` に勝つ）。同点になり得る 2 ルート → 起動時 abort。
regex なし・省略可能セグメントなし・末尾スラッシュ厳密一致（隠れリダイレクトなし）。クエリ文字列は
パターン対象外。

## Router 内部（W1 の実装可能仕様）

route table（可視データ）は `serve()` 起動時に**フラット radix 構造**へコンパイル — 連続配列・
offset 索引・ポインタゼロ（Align の設計手筋: soa/tape/offset-table）:

```text
Node  { first_edge: i64, n_edges: i64,     // static 子。label ソート済み（二分探索）
        param_child: i64,                  // -1 か node index（唯一の :param 子）
        wild_leaf: i64,                    // -1 か leaf index（唯一の末尾 *name）
        leaf: i64 }                        // -1 か leaf index（この node で終わるルート）
Edge  { label: str, node: i64 }            // label = リテラル 1 セグメント（バイト比較）
Leaf  { method_handlers: Method 毎の配列   // ハンドラ fn or 欠席 → この行がパスのメソッド
                                           //   集合そのもの（405 の Allow が只で出る）
        param_names: slice<str>, n_params: i64 }   // web.param 用・パターン順の名前
```

構築（起動時、素のヒープ — serve 終了時解放）: 各ルートをセグメント毎に挿入。リテラルは static
edge を追加/検索。`:name` は node 唯一の param 子を主張（同位置に別名 `:a`/`:b` = 衝突 → 両
パターン名入り **abort**）。`*name` は唯一の wildcard leaf を主張（末尾のみ。衝突 abort）。
(method, path) 重複 leaf → abort。各 node の edge をソート。leaf 毎に param 名を格納。

マッチ（リクエスト毎、割当ゼロ）: path を `/` で分割（in place — offset のみ、コピーなし）。
root から walk。各 node で static edge を**先に**（セグメントで二分探索）、なければ param 子
（セグメント view を固定スロット配列 `params[i]` へキャプチャ）、なければ wildcard leaf
（`/` 込みの残り全部をキャプチャ）。終端 leaf のメソッド行がハンドラを与える（在 → ディスパッチ。
欠だが行が非空 → 405 + 行から Allow。leaf なし → 404）。static > param > wildcard は**全 node**
で成立し、**バックトラックあり**（matchit 意味論 — 2026-07-21、#591 レビューで確定）: 優先枝が
path の深部で行き止まったら巻き戻して次の代替枝を試すので、`{/a/featured, /a/:id/versions}` は
`/a/featured/versions` を `:id` 行にルーティングする。オラクルの `match_score` は **path の
セグメント数に左詰めした固定幅 base-3**（static 2 / param 1 / wildcard 0、wildcard が吸収する
位置は 0 埋め）— つまり真に左→右の辞書式比較であり、walk の最初の成功 = オラクルの最大値が
**全テーブル**で成立するので、構築時の曖昧性 abort を要するルート集合の形は存在しない。
（2件のレビュー発見でここに確定した: 旧稿は「バックトラックなし + abort」だったが、その abort
はまさに現実的なテーブルを拒否してしまい、tree 化以前の本番ディスパッチ = 線形スキャンはそれらを
正しくマッチしていた。また、オラクルの旧 fold はシフトなしの**大小比較**で、`/assets/logo` を
httprouter/matchit/Fiber のリファレンスに反して `/assets/*file` ではなく `/:cat/:slug` に
ランク付けしていた — 左詰めで、文書化されていた左→右の意図にオラクル側を修正した。）
(method, path) 重複行と param 名衝突は引き続き構築時 abort。`web.param(c, "name")` = 高々 n_params 個の名前 view の線形走査
（n は極小。map なし）。

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

## Middleware（確定した所有権モデルに合わせ 2026-07-21 再設計 — 着地は W6）

Fiber の `c.Next()` チェーンはキャプチャクロージャ（延期）を要する。framework がハンドルを所有する
モデルでは、v1 の形は元の Move 連鎖設計より単純になる: `Ctx` は Copy なので、pre-handler は
それを消費も返却もしない —

```text
fn(c: Ctx) -> Result<Option<response_builder>, Error>
//   None      -> 続行（次の pre-handler / ハンドラへ）
//   Some(rb)  -> 短絡: serve が rb を書き、ハンドラは走らない（auth 拒否、リダイレクト）
//   Err       -> 500（ハンドラの Err と同じ）
```

`Option<response_builder>` は #583 以降合法なペイロードである。グループがリストを保持:
`web.group_with(prefix, [auth, log], routes)`。auth/logging/CORS ヘッダをクロージャなしで賄う。
状態付き middleware はキャプチャクロージャ機能と実消費者を待つ。


## ストリーミング（SSE + 汎用）— 2026-07-21 設計、着地は W6

**問題。** ハンドラは `fn(Ctx) -> Result<response_builder, Error>` — 完結した 1 レスポンスを組み立てる。
SSE や LLM トークンストリームは代わりに接続を保持して逐次書き込むので、ストリーミングには第二の
相互作用モデルが要る。確定済みの所有権規則は壊れず拡張される: **framework はリクエスト全体を通じて
リクエストハンドルを所有し、ストリームハンドラは加えてレスポンス STREAM を所有する** — stream は
レスポンス head が確定した瞬間に初めて存在し、それは framework が別の応答（404/405/500）を返せる
余地がちょうど尽きる瞬間でもある。何も手放していない。

### 表面

```align
// 第二（かつ最後）のハンドラシグネチャ。stream ルートに限定。`c` 経由でリクエストを借用し
// （serve がまだハンドルを保持しているので pump 呼び出しの間ずっと有効）、レスポンス stream を所有する。
fn events(c: pkg.web.types.Ctx, s: http_stream) -> Result<(), Error> {
  web.send_event(s, "tick")?
  s.finish()
}

routes := [
  web.get("/v1/models", list_models),
  web.sse("/v1/events", events),                                   // GET; text/event-stream
  web.stream("POST", "/v1/chat/completions", "application/x-ndjson", chat),
]
```

```text
web.sse(pattern, pump)                       -> route   // method は GET（EventSource は常に GET）、
                                                        //   Content-Type は text/event-stream
web.stream(method, pattern, content_type, pump) -> route // 一般形
web.send_event(s, data)  -> Result<(), Error>           // `data: {data}\n\n` 1 フレームを 1 send で;
                                                        //   単一行 data（複数行は caller 責務）
```

### 型

```text
Handler {
  Respond(fn(Ctx) -> Result<response_builder, Error>),
  Stream(fn(Ctx, http_stream) -> Result<(), Error>),
}
Route { method: str, pattern: str, stream_type: str, handler: Handler }
//   stream_type: stream head の Content-Type; Respond ルートでは ""（読まれない）。
```

テーブルは 1 つ、dispatch も 1 つ: stream ルートは同じ radix tree・同じ method 解決を通り、他の行と
同様 405 の `Allow` にも寄与する。`Handler` は Align 流儀の or-kind — sum 型であり、filler fn を
持つ 2 fn フィールドではなく（却下: filler は magic sentinel）、第二のルートテーブルでもない
（却下: 優先順位/405 がテーブル間に割れる）。

### serve の意味論

```text
match r.handler {
  Respond(h) => rb := answer(h, c); ctx.respond(rb) else {}          // 不変
  Stream(pump) => {
    rb := http.response(200)
    rb.header("Content-Type", r.stream_type)
    rb.header("Cache-Control", "no-cache")            // キャッシュされる stream は無意味; 常に付与
    s := ctx.respond_stream(rb) else { <pump をスキップ> }  // client 消滅 -> 次のリクエストへ
    pump(c, s) else {}                                // head 以後の Err: 返せるものが無い
  }
}
```

- `respond_stream` は**非消費**の bound receiver（下記 std.http 変更①）: `ctx` は serve のフレームに
  残るので、`c` の view は pump 呼び出しの間ずっと有効 — これが借用規則の下で
  `fn(Ctx, http_stream)` を well-formed にする。fd は stream に持ち上げられ、`ctx` は spent
  （二度目の respond は `Err`）; その drop はパースバッファのみ解放する。
- **head は遅延**（std.http 変更②）: `respond_stream` は head を保存し、最初の `send`（または
  `finish`）が書く。それ以前は `s.reject(rb) -> Result<(), Error>`（std.http 変更③）が保存済み
  head を破棄し、代わりに `rb` を完結した通常レスポンスとして書く — send 後は `Err`。これが
  stream ルートの 4xx 窓を与える: pump 内でリクエストを parse/検証し、不正入力には
  `return s.reject(...)`、正常入力には stream する — fn は 1 つ、別個の validate フェーズは無い
  （却下: ルート毎の validate fn は parse を二重化し Route を肥大させる）。
- 最初の send 以後、エラー窓は HTTP 自身の規則により存在しない: stream 途中の pump `Err` は単に
  stream を終える（drop が fd を閉じ、client は切断を見る）。ハンドラ Err と同じ silent-`Err`
  姿勢 — W4 のロギングが両方を覆う。
- ループはリクエスト単位で死なない。不変。

### 順序制約（ハード）

v1 の `serve` は逐次 — **開いた stream は他の全 client を飢えさせる**。ストリーミングは記録済みの
並行 serve フォローアップと同時（または後）に着地しなければならない; 逐次ループの上で出荷するのは
テスト専用である。これは順序の注記であって設計依存ではない: 上記のどれも並行化で形を変えない。

### Enabler（2026-07-21 探査済み; 実装順）

1. **`http_stream` をソースで型名に — 完了。** `resolve_type` エントリ、#583 の `response_builder`
   と全く同じパターン; `http_stream` は既に完全な `Scalar`/`Ty`（`respond_stream` の `Ok` payload、
   `.send`/`.finish` を持つ）だったので、欠けていたのはソース表記だけ。`crates/align_driver/tests/
   http_stream_nameable.rs` で固定（param/return 表記、型引数なし、配列要素は依然不可）。
2. **fn 値を enum variant payload に — 完了。** 新 `Scalar::Fn(u32)` variant（元々なかった — fn 値は
   `Ty::Fn` でスカラー形がなく、variant payload では表現できなかった）。fn 値は Copy `{fn_ptr,
   env_ptr}`（16 バイト、8-align）なので fn のみの enum は非 Move で drop されず、混在 enum の
   tag 分岐 drop は fn スロットをスキップする。#583 のチェックリストを掃いた — `scalar_to_ty`、MIR
   `sort_key_order`（fail-closed 腕）、codegen `scalar_bytes`（unreachable）、そして catch-all の
   暗黙 `i32` の代わりに 16 バイトスロットを確保する codegen `scalar_type` の fn 腕。構築時、fn
   payload は **`fn_types` id ではなくシグネチャで比較**（各 `fn` 式は新しい `FnTy` を intern する）。
   `ty_to_scalar(Ty::Fn)` は `None` のまま（fn は variant payload 専用で、`Option`/`Result`/`box`
   payload ではない）。`crates/align_driver/tests/fn_variant_payload.rs` で固定（dispatch、実際の
   `Handler` シグネチャ、Copy/非 drop、`align_interface` 経由のクロスモジュール往復、
   `Route { handler: Handler }` 配列形状、fn+Move 配列混在 drop、誤シグネチャの拒否）。
   **defer（fail-closed、消費者なし）:** fn payload を持つ *generic* sum 型は template payload
   resolver で拒否 — 中途半端には出荷しない。
3. **Move ハンドルをパラメータに持つ fn 値シグネチャ — 完了（Move 値 proxy で検証）。** #573 が
   間接呼び出し後に caller フレームで owned-arg を null 化する; 200k ループのテストが owned
   `array<i64>`（`http_stream` の代役）を値渡しで match 抽出した fn payload に通し、二重 free が
   ないこと（signal 終了ではなく完走）と move-after-use 拒否を検証。実 `http_stream` receiver は
   enabler 4 待ち。
4. **std.http `respond_stream` の作り直し — 完了**（変更①–③、2026-07-21 出荷）。ctx は借用され
   成功時に SPENT で残る（以後の `respond`/`respond_stream` は `Err`; 検証 `Err` は未 spent のまま）、
   head は遅延（stream に保存し最初の `send`/`finish` が書く）、`s.reject(rb)` が stream 前に完結した
   通常レスポンスで応える。出荷記録の全文は `docs/impl/std-design/http.md` item 8; M12 テストは完全
   置換（`m12_http_stream.rs`、13 本 — pump 中の `ctx.path()` 読みを含む。これは enabler 5 が必要と
   する stream ハンドラの借用形そのものである）。

### バックログ（記録のみ、v1 外）

heartbeat/keep-alive コメント、`event:`/`id:` フィールド + `Last-Event-ID` 再開、複数行
`send_event` data の分割、stream ルートのリクエスト毎 head カスタマイズ、stream の
タイムアウト/バックプレッシャ — いずれも消費者を待つ。

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
- **W6 — middleware-lite + ストリーミング** — 両方 **設計済み**（上のセクション、2026-07-21）;
  実装はストリーミング enabler と（本番 stream には）並行 serve にゲート。
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
