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
  web.serve("127.0.0.1", 8080, routes, 4)
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

// サービング — Impure。`workers` 本のリクエストループ（prefork、SO_REUSEPORT — 「並行 serve」を参照）
web.serve(host, port, routes, workers) -> Result<(), Error>
//   起動時: テーブル検証（不正 → パターン名入り abort）。`workers < 1` も abort。リクエスト毎:
//   パース（std.http、zero-copy）→ request-target を path と query に分割 → radix ディスパッチ
//   → ハンドラ → 返されたものを**書き込む**。自動応答: path 不一致 → 404、path 一致 method
//   不一致 → 405（Allow 付き）、ハンドラ Err → 500。ループはリクエスト単位で死なない。
//   `workers == 1` はそのループを呼び出しスレッド上でインライン実行する（スレッドを一切作らない）;
//   `>= 2` はその数だけワーカーを spawn し、各々が**自分専用の**リスナーを持つ。コネクションは
//   std.http の内側で keep-alive されるので、ループの形状はどちらでも同一である。

// ctx アクセサ（全て Pure。全て c に region 束縛された view を返す）
web.param(c, name)   -> str              // :param キャプチャ（固定スロット配列。total —
                                         //   パターンにない名前は起動時検出可能なバグ）
web.query(c, name)   -> Option<str>      // std.http クエリ床（RFC 3986 percent-decode 済み）
web.header(c, name)  -> Option<str>      // 2026-07-21 出荷: RFC 9110 §5.1 大文字小文字を無視
web.body(c)          -> slice<u8>        // 2026-07-21 出荷: `Ctx.body` が zero-copy view を運ぶ
web.body_str(c)      -> Result<str, Error>    //   body_str = `.as_str()`(検証済み view)
//   JSON 入力: req: ChatReq := json.decode(web.body_str(c)?)?   — core.json、view デコード
//   `web.header` は std.http の enabler(`std-design/http.md` の item 10)に乗り、それと一緒に
//   出荷された。解いた問題: Copy の `Ctx` は何も所有せず、任意名のヘッダー lookup は `body` のような
//   単一の保存 view には乗らない — ヘッダー**名**はハンドラが問い合わせるまで分からないので、借用
//   される値はパース済みテーブル全体だからである。候補は 2 つあった: raw head の `str`/`slice<u8>`
//   view フィールド + pkg.web 側の RFC 9110 lookup(std.http の lookup の複製で One way に反し、
//   テーブルの実体化にリクエスト毎の割り当てが要る)か、パース済みテーブルを切り離した view として
//   公開する std.http enabler か。切り離した view が勝った:
//   `ctx.headers() -> http_headers`(Copy で region 束縛の非所有 view。その表現は ctx ポインタ
//   **そのもの**なので、`hs.get(name)` は既存のランタイム lookup を再利用し、ランタイムのコードは
//   一切増えない)、`Ctx` はこれをもう 1 つのフィールドとして運び、
//   `web.header(c, name) = c.headers.get(name)` が転送する。pkg.web は自前の lookup を**一切**出荷
//   しない。lookup の綴りを 1 つに保つため、`ctx.header(name)` は std.http 側で
//   `ctx.headers().get(name)` に**置換された**。

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
             マッチしたパターン、body の view、および切り離したヘッダーテーブルの view）。何も所有
             しない — リクエストハンドルは `serve` に留まり、view はハンドラ呼び出しの間有効である。
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
ポリシー。unary handler と stream pump の全 `Err` は request method/path と組み込みエラー値全体
（`NotFound`、`Invalid`、`Denied`、`Code(n)`）を best-effort の stderr 1 行に残し、ログ失敗で
ループを殺さない。リクエスト由来の panic はゼロ。

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
  s.send_event("tick")?
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
s.send_event(data)       -> Result<(), Error>           // `data: {data}\n\n` 1 フレームを 1 send で;
                                                        //   単一行 data（複数行は caller 責務）;
                                                        //   send_event("") は合法な空イベント
```

**`send_event` は `http_stream` の**メソッド**であり、`web.*` 自由関数ではない**（enabler 5 の出荷
中に改めた — 表面は当初 `web.send_event(s, data)` として素描されていた）。理由は 2 つ、うち 1 つが
決定的である: pkg レベルの自由関数は Move ハンドルを**値渡し**で取る — Align にユーザ関数の借用パラ
メータは無い（借用は std の bound-receiver 機構であり、記録済みの `io.copy` 制限）— ので
`web.send_event(s, …)?; s.finish()` はコンパイルできない: ラッパーが、pump がこれから finish すべき
まさにその stream を消費してしまう。加えて SSE イベントフレーミングは既に std.http の床項目として
確約済みで（「最初のストリーミング消費者が着地したときの SSE イベントフレーミング（WHATWG）」、上の
前提条件）、フレーミングは他の stream 書き込みと同居する — `send` / `send_event` / `finish` /
`reject`、1 つのハンドル上の 1 つのメソッド族。pkg.web はラッパーを**一切出荷しない**（`web.header`
の非重複ルール）。

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
    match ctx.respond_stream(rb) {
      Ok(s) => pump(c, s) else {}                     // head 以後の Err: 返せるものが無い
      Err(e) => {}                                    // client 消滅 -> 次のリクエストへ
    }
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
  stream を終える（drop が fd を閉じ、client は切断を見る）。unary handler `Err` と同じ
  method/path/error の stderr 診断を残す。
- ループはリクエスト単位で死なない。不変。

### 順序制約（ハード）— 2026-07-21 解除

v1 の `serve` は逐次であり — **開いた stream は他の全 client を飢えさせた** — そのためストリーミングは
テスト専用で出荷し、並行 serve フォローアップにゲートされていた。そのフォローアップは今や**出荷済み**
である（下の「並行 serve（prefork）」）: stream はちょうど `W` ワーカーの 1 つを占有し、残る `W - 1` は
serve を続けるので、ゲートは可視のサイジング判断（`workers >= 想定同時ストリーム数 + 1`）になる。
`crates/align_driver/tests/apps_web_prefork.rs`
（`a_held_open_stream_occupies_one_worker_while_the_others_serve`）で固定。ストリーミング設計は
並行化で何一つ形を変えなかった — まさにこの注記が予告したとおりである。

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
5. **pkg.web の配線 — 完了（2026-07-21）。** `types.align` の `Handler`（`Respond`/`Stream`）+
   `Route.stream_type`; `web.stream` / `web.sse` コンストラクタ; 上の擬似コードそのままの `serve` の
   stream 腕; そして std.http メソッドとしての `s.send_event(data)`（WHATWG `data: {data}\n\n`、遅延
   head のバッファを共有する 1 回の write — head + フレーミング + イベントを単一の `send` で; 空イベ
   ントは `send("")` の no-op と違い実フレーム; 共有 `http_stream_send_parts` の上のランタイム
   `align_rt_http_stream_send_event`）。途上で **MoveCheck の偽陽性**を修正: `loop` 本体内で消費され
   る match 腕バインディングが back-edge の不動点を汚染していた（腕バインディングは `Let` と違い
   （再）初期化時に moved ビットをクリアしていなかった — まさに serve の `Ok(s) => pump(c, s)`）。
   E2E: `crates/align_driver/tests/apps_web_stream.rs`（3 本 — SSE フレーム + pump 中の
   `param`/`has_query`/`body` 読み、ループが生き残る reject 4xx 窓、単一テーブル共存: stream ルート
   405 `Allow` + 404）、`m12_http_stream.rs`（+1 `send_event`）、ランタイムの unit フレーミングテスト、
   および MoveCheck 修正の sema 回帰固定。**もはやテスト専用ではない**: 並行 serve が同日に出荷され
   （下の prefork セクション）、ハード順序注記は解除され、stream のコストはサーバ全体ではなく 1 ワーカー
   で済む。

### バックログ（記録のみ、v1 外）

heartbeat/keep-alive コメント、`event:`/`id:` フィールド + `Last-Event-ID` 再開、複数行
`send_event` data の分割、stream ルートのリクエスト毎 head カスタマイズ、stream の
タイムアウト/バックプレッシャ — いずれも消費者を待つ。

## 並行 serve（prefork）+ コネクション keep-alive — 2026-07-21 設計 + 出荷

**問題は二重にある。** v1 の `serve` は 1 本のブロッキングループである: 開いた SSE/chat stream は
他の全 client を飢えさせ（上のハード順序注記 — 本番ストリーミングはこれにゲートされている）、
コネクション毎 1 リクエストは W5/W7 ベンチを無意味にする（keep-alive された Fiber はリクエストを
計測するが、リクエスト毎に close する Align は TCP ハンドシェイクを計測してしまう）。1 つの設計が
両方を覆う。

### 設計: 共有状態ではなく prefork

`serve` は可視のワーカー数を得る。`W` 個のワーカーがそれぞれ同一ポート上で**自分専用の**リスナーを
所有し（`SO_REUSEPORT` — fasthttp/nginx の prefork 系譜）、各々が既存の逐次リクエストループを
そのまま走らせる。カーネルが到来コネクションを複数のリスナーに振り分ける。

```text
pkg.web.serve(host, port, routes, workers) -> Result<(), Error>   // シグネチャの完全変更
//   workers == 1  -> 今日のループそのまま、呼び出しスレッド上（task_group なし、スレッド 0）
//   workers >= 2  -> task_group { spawn W workers }; 各ワーカー: 自身の http.serve_shared
//                    リスナー + 不変の accept/dispatch/respond ループ
//   workers <  1  -> 起動時 abort（validate クラス: プログラマの設定エラー）
//   workers >  process.cpu_count() + 1 -> 同じ abort（実装中に発見）: worker は決して return せず、
//                    task_group は利用可能並列度でサイズされたプール + 呼び出しスレッド上でタスクを
//                    走らせるので、その数を超えたタスクはそもそも起動しない。黙って少ないループで
//                    serve する代わりに abort し、このパラメータが与える約束を守る。注意: これにより
//                    上限はマシン依存になる — 固定の数値を書いたソース行は、より小さいマシンや cgroup
//                    の CPU クォータ下（`cpu_count()` はこれを尊重する）では abort する。
//                    `workers = process.cpu_count()` と書くこと。
```

- **パラメータによる Nothing hidden:** スレッド生成はどの呼び出し箇所でも可視である — `serve(...,
  4)` はソース上で 4 スレッドと言っている。`serve` 内の `spawn` は普通の pkg レベル Align
  （`task_group { spawn(fn { worker(...) }) }`）であって、ランタイムの魔法ではない。
- **構築上、共有可変状態なし。** 代替案 — 1 つのリスナーハンドルを N ワーカーで共有する — は言語
  自身に拒否された: Move の `http server` ハンドルは N 個のクロージャに値でキャプチャできず、借用は
  `spawn` を越えない。`SO_REUSEPORT` はその共有を解消する: 各ワーカーは自分の Move リスナー、自分の
  parked keep-alive スロット、自分のリクエストループを所有する — ロックゼロ、競合ゼロ、`spawn` の値
  キャプチャが既に強制している「共有可変状態なし」規則そのものである（draft §Task Group）。
- **唯一の共有入力は route テーブル** — Copy 行の `slice<Route>` で、各ワーカーへ値で（16 バイトの
  view ディスクリプタとして）キャプチャされる; 背後の配列は region により構造化 `task_group` より
  長生きする。**2026-07-21 探査済み: この形状全体は今日そのままコンパイルされ正しく動く** —
  Impure な spawn 本体、slice view + 自分のワーカー index をそれぞれキャプチャするループ spawn された
  ワーカー、実スレッド上での fn 値 `Route.handler` を通した間接呼び出し、`wait()?`。**コンパイラ
  enabler は存在しない**; このアークは std.http 作業のみである。
- **エラーセマンティクス（部分的な degradation）。** リスナーレベルの障害に当たったワーカーは自分の
  `Err` を返して死ぬ; 他のワーカーは serve を続ける。`wait()?` は全タスクを join するので、`serve` が
  返る — 最初のエラーとともに — のは**全**ワーカーが死んだときだけである。リクエスト毎の障害は
  ワーカーを決して殺さない（不変）。**過渡的な `accept(2)` の errno も同様に表に出ない**（std.http で
  出荷済み、http.md item 9）: `EINTR`/`ECONNABORTED`/保留ネットワークエラー族は待機へ戻り、
  `EMFILE`/`ENFILE` は idle な parked keep-alive コネクションを 1 本費やして — ワーカーは fd テーブルを
  共有する一方で parked 集合は共有しないので、待機 10 ms あたり 1 本にペーシングされる — リトライする。
  したがって「リスナーレベルの障害」は文字どおりそれだけを意味するようになり、client が accept される
  前にコネクションを落とした、あるいは fd テーブルが埋まった、というだけでワーカーが死ぬことはもう無い。
  そのために Align の `Error` へ errno が届くことはなく、分類は完全に `accept` の内側に収まっている。
- **ストリーミング解禁。** 開いた stream はちょうど自分のワーカーを占有する; `W - 1` は serve を
  続ける。本番ストリーミングのゲートは「十分なワーカー数で走らせる」— アプリのソース上で可視の容量
  判断 — になる。目安のサイジング規則として `workers >= 想定同時ストリーム数 + 1` を fn doc に
  記録する。
- **サイジング:** ベンチゲートは `workers = process.cpu_count()` で走らせる; fn doc はこれをデフォルト
  として推奨し、これは同時に上限でもある（上記）。この accessor は本アークで必要になった std の追加で
  ある — 推奨サイジングはこれまで Align では書けなかった（`std-design/process.md`）。

### keep-alive は完全に std.http に乗る

リクエストループは keep-alive のために変わらない — `srv.accept()` が新規コネクションを accept する
前に、kept-alive コネクションから次のリクエストを yield することを覚えるだけであり、`ctx.respond`
が適格なコネクションを close する代わりに返すことを覚えるだけである。プロトコル設計の全容
（適格性、単一の parked スロット、poll 優先、no-pipelining 規則、`Connection` ヘッダ変更、drop 順序
安全性）は std.http item 9 — `docs/impl/std-design/http.md`。pkg.web の serve ループは前後でバイト
単位で同一であり、上の prefork ラッパーだけが pkg 側の作業である。

### スライス（実装順）

1. **std.http `http.serve_shared(host, port)`** — 完了。`SO_REUSEPORT` リスナーを兄弟演算として
   （`http.serve` は strict-bind セマンティクスを保つ: 誤った二重サーバは依然として大声で失敗
   すべき; reuse は明示的な選択であり、`respond`/`respond_stream` の兄弟の前例）。
2. **std.http keep-alive** — 完了（item 9 ②: parked スロット + poll + 適格性。prefork が存在する前に
   逐次 serve に対してテスト済み）。**すべての呼び出し側に影響する挙動上の帰結が 1 つある:** 適格な
   1.1 リクエストはコネクションを開いたまま残すので、EOF まで読む client は `Connection: close` を
   送る（driver テスト共有の `one_shot` ヘルパ）か、`Content-Length` で読みをフレーム化しなければ
   ならなくなった。
3. **pkg.web prefork** — 完了。`serve(host, port, routes, workers)`、`workers == 1` のインライン
   パス（strict bind、スレッド 0）、`task_group` ラッパー、そしてすべての呼び出し箇所の完全更新。
   ワーカー本体は切り出した `worker(host, port, routes, shared)` であり、その bind 行は
   `srv := if shared { http.serve_shared(…)? } else { http.serve(…)? }` — ごく普通の値を返す `if`
   なので、新たに `http_server` を型名として書けるようにする必要はなかった。探査どおりコンパイラ
   enabler は**不要**だった — ただし std の追加 `process.cpu_count()` は**必要**だった。`task_group`
   は利用可能並列度でサイズされたプールへディスパッチするので、それより多い「決して戻らない」ワーカーは
   起動されないままになる。よって `serve` は上限を超えると abort し、推奨サイジング
   `workers = cores` がこれでようやく書けるようになった。`apps_web_prefork.rs` は実際に bind された
   リスナー数を数える（`/proc/net/tcp`）ので、「W タスクを spawn した」を「W 本のループが動いている」と
   取り違えることは二度と起きない。
4. **W5 ベンチゲート** — 完了。`bench/web_e2e` は keep-alive + prefork の外部比較を固定し、
   `bench/web_router` は同一パス・同一深さを 6/128 ルート表で比較する。static/param を sibling
   chain の先頭・末尾で分け、隣接 AB/BA ペア比の中央値を使う。Linux x86_64 baseline CI は
   chain 先頭 <= 1.35×、全 shape <= 2.75× を固定する。理想の 1.00× は未達で、ローカル計測の
   末尾 1.87–2.11× は per-node sibling scan を正直に示す。

## スライス（計画の F3）

- **W1 — router コア。** パターン解析 + 検証。**radix tree**（static/param/wildcard ノード、
  優先順位、衝突検出）+ マッチャをセグメント上の純関数として。param スロットキャプチャ。線形
  走査オラクルとの差分テスト。F1① が必要。
- **W2 — Ctx + serve + ディスパッチ。** `web.Ctx`（F1②③ が必要）。std.http 上の accept ループ。
  自動 404/405/400/500。`group`。統合テストは in-process サーバパターン
  （`crates/align_driver/tests/m11_http.rs`）。
- **W3 — アクセサ + レスポンダ。** param/query/header/body/body_str、json/status_json/text/status。
- **W4 — 堅牢化。** 第 1 スライス 2026-07-21 出荷: **起動時テーブル検証**
  （`router.validate`、純粋な診断。`serve` は bind 前に stderr へ出力 + `process.abort()`
  — エラーポリシーの「起動時 abort、Err にしない」）: 既知の大文字メソッドまたは `""`、先頭 `/`
  のパターン、名前付き `:`/`*` セグメント、`*` は tail のみ、1 パターン内で同じパラメータ名を
  二度使わない、すべての Stream 行に空でない `stream_type`（空だと空欄の `Content-Type:` を
  送出してしまう上、`stream_type == ""` は HEAD フォールバックが「Respond 行」と読む不変条件）、
  そして後続の行が決して勝てない PATH CLAIM の重複なし — 同一メソッド二度
  （405 `Allow` join を重複させていたのもこれ）や、その claim に対する any-method ルートより後の
  任意の行。パラメータ名は claim に影響しない（`/a/:x` ≡ `/a/:y`）。1 パターン上の
  specific-then-`any` は合法のまま（フォールバック方向）。**HEAD は RFC 準拠**（9110 §9.3.2）:
  std.http の `respond` は HEAD リクエストに対しプロトコル境界でボディを抑制する
  （`Content-Length` は送出したまま）。`serve` は明示行のない HEAD をそのパスの GET ハンドラへ
  ルーティングする（Respond 行のみ — stream head に HEAD 形はないので、stream 専用 GET は
  HEAD を 405 のままにする）。**自動 404/405/500 は固定の最小 JSON ボディを持つ**
  （`{"error":"not found"}` / `"method not allowed"` / `"internal error"`、
  `Content-Type: application/json`）。テスト: `apps_web_validate.rs`（9 つの abort +
  合法シャドウの serve）、`apps_web_root.rs` の HEAD/body マトリクス、runtime シリアライザ
  ユニット。keep-alive 再利用は**出荷済み**（std.http item 9 ②。`apps_web_root.rs` の keep-alive
  E2E）。**route-tree 端例マトリクスも出荷済み:** 線形 oracle の安全な base-3 score 幅を越える
  64 セグメント、byte-exact な 4 KiB static hit/miss、4 KiB の zero-copy param capture、同一 claim
  の GET/POST + 405 `Allow`、空テーブルの全 query helper を絶対期待値で固定する。**不正リクエスト
  マトリクスも出荷済み:** 実ソケットで request-line、target form、header syntax、
  Transfer-Encoding、矛盾する Content-Length を通し、各ケース後の正常ルーティングで不正接続だけが
  閉じることを固定する。**ハンドラ `Err` ロギングも出荷済み:** unary handler と stream pump は
  実 method/path と組み込み `Error` 全体を stderr 1 行に残し、500/stream close と serve-loop 生存を
  E2E で固定する。W4 は完了。
- **W5 — router/e2e ベンチゲート。完了。** `bench/web_router` の同一パス scaling ceiling と
  `bench/web_e2e` の std.http/Fiber 比較で、パフォーマンス契約の現状を回帰固定する。
- **W6 — middleware-lite + ストリーミング** — 両方 **設計済み**（上のセクション、2026-07-21）。
  ストリーミングは **配線済み・E2E 固定済みで、本番ゲートも解除された**: 並行 serve が 2026-07-21 に
  出荷され（上の prefork セクション）、stream のコストはサーバ全体ではなく 1 ワーカーで済む。
  middleware-lite は設計のみのまま。
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
