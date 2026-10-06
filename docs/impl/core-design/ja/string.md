このディレクトリには、`core` ライブラリの各領域について、`../std-design/` と同等の粒度（シグネチャ、Move/effect の分類、エラー方針、落とし穴（Pitfalls）、テストアンカー）で記述された公式な設計ドキュメントを収めている。
執筆はメインループ（Fable）が担当している。

# core — str / string / builder / template

> 🌐 [English](../string.md) · **日本語**

## Overview

テキスト処理（draft §12–§13） — 借用ビュー型、所有権を持つバッファ型、組み立て用の builder、そして 1 つの template 形式からなる。全体を通してバイト指向の UTF-8 として扱われる。検索系メソッドは memchr 系の SIMD スキャンレイヤの上に構築される（#310）。
ここでの中心的な方針は **「すべての文字列のアロケーション（メモリ確保）先が明確であり、目に見える形になっていること」** である — arena、所有者（owner）、または builder のいずれかに属する。パイプラインのラムダ内での隠れたアロケーションはコンパイルエラーとなる。

## Signatures and settled surface

```text
"lit"                      -> str        // single-line only; \n \t \" escapes; UTF-8
'A' / 'あ'                 -> char       // one Unicode scalar
s.is_char_boundary(index: i64) -> bool  // 全域的なバイト境界判定; アロケーションなし
s.parse_i64() -> Result<i64, Error>  // ASCII 十進数の検査付き変換; アロケーションなし
s.len()                    -> i64        // BYTE length ("あ".len() == 3)
s.contains(n) / s.starts_with(n) / s.ends_with(n)      -> bool
s.eq_ignore_ascii_case(t)  -> bool       // ASCII fold only, not Unicode
s.find(n) / s.rfind(n)     -> Option<i64>   // byte index of first/last occurrence
s.trim() / s.trim_start() / s.trim_end()    -> str   // ASCII-whitespace; zero-copy sub-view
s[a..b]                    -> str        // range view; region-tied; NO s[i] byte indexing
s.bytes()                  -> slice<u8>  // zero-copy byte view; UTF-8 義務なし
s.clone()                  -> string     // deep copy; the arena-escape hatch
a + b                      -> compile error; builder is the one concatenation path

b := builder()  /  builder(cap)
b.write(s: str|string)  /  b.write_int(i: i64)
b.to_string()              -> string     // the finisher (there is no finish()/build())

fn append(borrow mut output: builder, text: str)  // 直接呼び出す helper の宣言

template "…{expr}…"        -> str        // holes: int, float, str, bool, char; full expressions
```

レシーバは自動で借用される — 上記のどのメソッドも `str` または `string` を受け取る（所有権を持つ `string` は消費されず、ビューとして扱われる）。`hash64` / `hash128` もこれらのビューを受け取る（[hash.md](hash.md) を参照）。

`builder` は型名として記述できる opaque な Move 型である。直接呼び出す関数の
値渡し引数と戻り値は所有権を移す。`borrow mut` はすべての追記メソッドと
排他的 borrow の転送を許す。共有 `borrow` はスコープ末尾や分岐を経ても読み取り専用である。
caller は mutable な binding を渡し、helper の終了後に finish できる。
借りた builder は move・返却・capture・finish できない。排他的な置換は通常の
Drop-before-store に従う。追記はバイトをコピーし、入力のビューを保持しない。
handle の引数渡し自体はアロケーションしない。aggregate への配置と関数値の生成は
対象外のままである。既存の関数型の戻り値構文では `fn() -> builder` を記述できるが、
その関数値の生成は引き続き拒否される。
[Plan 82](../../82-text-builder-parameter-plan.md) がこの境界を定め、
`text_builder_params.rs` がソース・native 実行・whole-program/per-unit の一致を検証する。

## Type & ownership classification

- `str` — Copy 可能なビュー `{ptr, len}` であり、region はそれが指し示すデータに依存する（リテラルの場合は region-0/static となる）。
- `string` — 所有権を持つ Move 型のヒープバッファ。破棄時（drop）に解放され、再代入されると古いデータは drop される。必要に応じて `str` に自動借用される。
- `builder` — 所有権を持つアキュムレータ。`to_string` の呼び出しによって終了する。隣接する `write` は MIR のピープホール（peephole）最適化によって融合（fuse）される（`fuse_builder_writes` において、`"lit" + int + "lit"` は 1 回のランタイム呼び出しに変換される） — 新しい形式の `write` を追加する場合は、このバッチ処理を迂回するのではなく拡張すること。
- `template` 式の結果は、arena 内では arena に region 付けされた `str` となる。arena 外で動的に生成された結果は、隠蔽されたスコープ付き `string` 所有者を参照する、フレーム境界（frame-bounded）ビューとなる。静的部分のみで構成されている場合は、プールされたリテラルに畳み込まれる（[audit 13](../../13-string-array-allocation-short-input-audit.md#33-fixed-2026-07-15--arena-free-template-and-jsonencode-have-scoped-owners)）。

## Effects

Pure（I/O なし）。*アロケーションの可視性* というルールはエフェクトシステムではなく構造的に強制される — `str + str` はあらゆる場所においてハードエラーとなる（決定済みの仕様）。arena 外での `template` 式の結果はパイプラインのラムダ内でローカルに消費できるが、そのフレーム境界ビューをラムダの戻り値として返すことはできない（`lambda.rs`）。チェッカーは一律の文字列連結ルールを強制しており、古い MIR の連結パスも削除された（audit 13 §3.2、2026-07-15 修正済み）。

## Errors & aborts

整数変換は `Result<i64, Error>` を返し、不正な形式や範囲外の入力は `Error.Invalid` となる。`s[a..b]` における範囲外アクセス（out of bounds）は abort を引き起こす。非 UTF-8 な *入力* のエラー処理は `std` 境界での関心事である（`fs.read_file` → `Error.Invalid`）。core の文字列操作は不変条件が満たされている前提でバイト指向を保つ。範囲の部分ビュー作成（range lowering）は、仕様どおり O(1) で両端の UTF-8 スカラー境界チェックを行い、違反時は abort する（audit 13 §3.1、2026-07-13 修正済み）。

`s.parse_i64() -> Result<i64, Error>` は入力全体を ASCII の
`[+-]?[0-9]+` として変換する。先頭の符号は一つまでで、先頭のゼロと符号付きゼロを
許容する。空白は拒否し、必要なら明示的に `.trim()` を呼ぶ。空文字、符号のみ、
非 ASCII 数字、埋め込み NUL、区切り、基数接頭辞、小数、指数、i64 の範囲外は
ラップや停止をせず `Err(Error.Invalid)` を返す。所有 `string` を含む受信側を一度だけ
評価して借用し、Copy な結果はビューを保持しない。Pure で、ヒープ確保も入力のコピーも
行わず、ロケール状態を参照しない。

`s.is_char_boundary(index: i64) -> bool` は UTF-8 のバイト境界を判定する。
負の位置とバイト長を超える位置は false、0 と末尾は true、それ以外は
`(byte & 0xc0) != 0x80` を返す。両端と範囲外ではバイトを読まず、
アロケーションも行わない。所有する `string` は通常どおり借用する。
位置引数の評価が終わるまで文字列は有効でなければならない。
結果の bool はビューを保持しない。Pure な操作であり、Result や範囲外停止は
発生しない。通常の `s[a..b]` の範囲・UTF-8 境界違反による停止は変わらない。

`starts_with` と `ends_with` は内部の文字列スライスを作らず、境界内のバイトを
比較する。空の検索文字列は一致し、元より長ければ不一致となる。
アロケーションはなく、比較するバイト数は検索文字列の長さ以下である。
特定の libc 呼び出しや SIMD による速度向上は保証しない。

## Regions

`region_of(trim*/s[a..b]/s.bytes()) = region_of(s)` — サブビューは元の region を継承する。`clone` は owned なデータを返すため region を持たない。`string` の struct フィールドの読み取りは、Frame region の `str` として借用される（所有権を持つ struct は移動する）。

## 仕様先行(未実装)

- **`split`** および **`find_any`**（§18.1 カタログ） — 現状ディスパッチ用のアームはない。特に `split` は大きな課題（大物）である — その戻り値の型（ビューの `array<str>` — つまり region 付きビュー要素を持つ Move 配列）を実現するには、Move 要素コレクションに関する対応が必要になる。owned-copies による妥協的な形態でリリースしてはならない（「理想形で出すか、さもなくば defer（延期）する」）。現状では `find` / `rfind` と `s[a..b]` を組み合わせて手動で split 処理を構築する。
- `s[i]` による直接のバイトアクセスはない — UTF-8 としての保証を外すことが呼び出し側で明確になるよう、`s.bytes()[i]` という明示的なバイトビューを使用する。
- §13 / §18.1 の language template variant（`html`、`raw`、json-template など） — 現在は
  plain な `template "…"` のみ。DESIGNED の `pkg.template` はこの syntax を変えず、default-escaped
  `write` と explicit `raw` を持つ opaque HTML builder を別に提供する。contextual language-template
  parsing は deferred のまま。

## Pitfalls

- P1 — すべての検索や比較は **バイト指向** である。ユーザー向けの説明にはそれが文字（char）なのかバイト（byte）なのかを明記すること（find が返すのは *バイト* インデックスであり、`s[a..b]` への入力として有効な値である。文字数ではない）。
- P2 — `str + str` は単にパイプラインラムダ内の規則というわけではなく、どこでもハードエラーとなる決定済みの仕様である。連結には builder を使用すること。これを lint レベルに弱めたり、古い arena 用の連結実装を復活させたりしてはならない。
- P3 — `builder.to_string` が唯一の完了処理（finisher）である。`finish()` のような別名を追加することは One-way レビューの方針に反する。
- P4 — `eq_ignore_ascii_case` はその名前が示す通り、設計上 ASCII 専用である。Unicode の大文字小文字同一視（case-fold）はロケールに依存した（汚染された）別の機能である — non-goals に従い、スコープ外として拒否すること。

## Test anchors

`m5.rs`（find / rfind のペア、trim ファミリ、ゼロコピーの bytes ビュー、fuse を含む builder、template、エスケープ、UTF-8 のバイト長、print 型の網羅性チェックを含むメソッド）。`lambda.rs:271/280/287/294`（ラムダ内でのアロケーションの拒否 + ラムダ内での arena の許可）。`hash.rs`（ビューの受け入れ）。`fuzz_fmt.rs`（文字列を多用するソースの formatter 往復テスト）。例として `strings.align`、`template.align`。文字列連結の拒否は reducer、名前付き関数、ラムダの各コンテキストで一貫してカバーされている。SIMD スキャンの固定: #310 differential oracle。

## バイト列の上書きと型付きビュー

次の宣言は既存のストレージを操作し、確保や所有者のコピーは行わない。
`S` は `u16`、`i16`、`u32`、`i32`、`u64`、`i64`、`f32`、`f64`、`E` は `le` または `be`。

```text
slice<u8>.set_u8(offset: i64, value: u8) -> ()
slice<u8>.set_i8(offset: i64, value: i8) -> ()
slice<u8>.set_S_E(offset: i64, value: S) -> ()
slice<u8>.fill(value: u8) -> ()
slice<u8>.fill_S_E(value: S) -> ()
slice<u8>.copy_from(source: slice<u8>) -> ()
slice<u8>.view_le<T>() -> Option<slice<T>>
slice<T>.as_bytes() -> slice<u8>
```

書き込みには書き込み可能な backing が必要で、長さと容量は維持する。store は値の幅全体を検査し、
型付き fill は長さが幅の倍数であること、copy は両者の長さが等しいことを要求する。
検査失敗では書き込み前に停止する。copy は元データを借用し、独立した backing の証明を必要とする。
重複または不明な backing は拒否する。空の fill/copy は有効で、`fill_u8` という別名はない。
正確なアクセス・effect・検証規則は [plan 65](../../65-open-issue-batch-plan.md) に従う。

型付きビューは Pure な descriptor 操作で、元データの寿命と読み書き権限を保持する。
`T` は `S` と同じ 8 型。呼び出しでは `.view_le()` と書き、完全な期待型から `T` を推論し、
型引数は書かない。長さやアラインメントが不正なら `view_le` は `None` を返し、little-endian
以外のターゲットはコンパイル時に拒否する。`as_bytes` はネイティブ表現を公開する。
`mut` ヘッダーだけでは共有・文字列由来の backing に書き込み権限を与えられない。
自動 SIMD 化は保証しない。正確な契約は [plan 78](../../78-checked-byte-view-plan.md)、検証対象は
`bytes_ops.rs`、`runway_a2_binary_codec.rs`、`consumer_borrow_boundaries.rs`。

## 明示的なコンストラクタ容量

`buffer.filled(length: i64, value: u8) -> buffer` は、指定した長さの初期化済み
バイト列を返す。確保容量は長さ以上。長さが正ならペイロードを一度確保し、
ゼロならペイロードは確保しない。Move ハンドル自体は確保を伴い得る。
初期化は O(length)。負数・サイズのオーバーフローは確保前に停止し、OOM も
停止する。通常の `buffer(capacity)` は従来どおり best-effort の空の読み取り
ウィンドウを作る。

両コンストラクタは末尾に省略可能な `alignment: i64` を受け取り、既定値は 1。
呼び出しは `buffer(capacity, alignment)` と
`buffer.filled(length, value, alignment)`。引数はソース順に一度ずつ評価する。
アラインメントは 1 以上 536870912 以下の 2 の冪でなければならず、それ以外は
ペイロードやハンドルの確保前に停止する。容量や長さがゼロまたは不正でも同じで、
アラインメントをサイズより先に検証する。既存の best-effort 予約と、初期化・拡張時の
終了型確保失敗ポリシーは変わらない。ペイロード先頭のアドレスは指定値の倍数となり、
空の場合も逆参照できない整列済みセンチネルを返す。拡張はアドレスを移動し得るが
アラインメントを維持し、容量内の読み込みは両方を維持する。Move と return は所有者と
保証を移し、置換は新しい所有者の保証を採用する。Drop は確保時と同じレイアウトを使う。
借用、初期化済み長さ、使用可能容量、ビュー無効化は従来どおり。内部オフセットの整列、
物理メモリ常駐、速度、LLVM のビュー読み込みの強い整列条件は保証しない。
`align(N)` は構造体と固定配列の格納領域属性のままで、バッファのペイロードは
コンストラクタ引数で直接指定する。[plan 131](../../131-aligned-buffer-payload.md) を参照。

`b.capacity() -> i64` は現在の読み取りウィンドウ容量を返す、引数なしの Pure な
非消費クエリである。初期化済みの `b.len()` とは独立し、追加確保やビューの保持は
ない。既存の安定したローカル・フィールド・借用ペイロードの受信者規則に従う。
通常の `buffer(n)` は予約成功なら `n`、不正な要求や予約失敗ならゼロを公開する。
filled の容量は長さ以上。put・append・成功した `read_line` は従来の容量と新しい
初期化済み長さ（行なら終端を除いた本体長）の大きい方を維持する。短い read、EOF、
失敗した行読み取りでも容量は縮まらない。デコードなどで返る buffer の容量は
初期化済み長さと等しい。`read_line` は元の容量を超えて成長できるが、終端を除いた
本体は 64 MiB 以下で、読み取りの分割位置によらず追加前に上限を確認する（plan 102）。
既存の大きな予約領域は維持する。容量が制限するのは容量付きの fill である。内部アロケータの余剰領域はウィンドウを広げない。
物理メモリへの常駐や将来の成長成功は保証せず、best-effort な通常の生成と
成長時・OOM 時の停止方針は変えない（[plan 87](../../87-buffer-read-capacity-plan.md)）。

`b.append_filled(length: i64, value: u8) -> ()` は同じファミリの append メンバー
である。`mut buffer` の公開ウィンドウを `value` の `length` バイト分ちょうど
伸ばす。拡張は一度だけで、拡張の連続やバイトごとの呼び出しにはならない。
長さゼロは no-op。負数やオーバーフローする長さは書き込み前に停止し、
コンストラクタと同じ終了ポリシーに従う。`buffer(capacity)` は公開せずに予約
するだけなので、繰り返しバイトでウィンドウを伸ばす手段はこれになる。
型付きの `append_filled_S_E` 形式は存在しない。`fill_S_E` は長さが既知の
公開済みウィンドウを上書きするのに対し、append 形式は独自の要素数の文法を
必要とし、それを要求する実プログラムの記録がまだないためである。

期待型で要素型を指定する `array_builder()` と `array_builder(out)` は、末尾に
省略可能な i64 容量を取る。呼び出しは `array_builder(capacity)` または
`array_builder(out, capacity)`。省略時はゼロ。初期要素数はゼロのままで、
少なくとも指定容量回の push が追加拡張なしで収まる。要素数 × stride と
対象の確保サイズの検査は確保前に行う。heap の build は領域を移譲し、region
の build は従来どおり連続領域へ実体化する。要素型・寿命・Drop・純粋性の規則は
変わらない。
