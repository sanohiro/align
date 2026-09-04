# ツールチェーン: alignc、align-repl、フォーマッタ、lint

> 🌐 [English](../16-toolchain.md) · **日本語**

`alignc` はコンパイラ、実行、整形、キャッシュ管理、検査の各機能を持ちます。`align-repl` は同じコンパイラを対話的に使うための別のバイナリです。複数ファイルのプログラムも1つのエントリファイルから始まり、インポートから依存グラフが決まるので、別のビルド定義ファイルは不要です。

## 実際に使うコマンド

```text
alignc check file.align         # whole-program の parse + typecheck + lint
alignc run   file.align [args…] # build + execute。後続引数は main(args) へ
alignc test  file.align         # 指定ファイルとインポート先のテストを実行
alignc build file.align         # current directory に <stem> という executable
alignc fmt   file.align --write # formatting をその場で正規化
```

編集中は `check` と `run` を繰り返し使います。複数ファイルのビルドでは、`.align` ファイルごとにモジュールをコンパイルし、明示的なインターフェースでインポートを検査して、到達可能な依存グラフをリンクします。`check-per-unit` はこのインターフェース単位の検査を行い、`emit-interface` は各単位の公開 API と、インターフェースおよび実装のハッシュを表示します。

ビルドには内容のハッシュで識別するキャッシュが2層あり、どちらも既定で有効です。統計を要求しなければ、キャッシュの動作は表示されません。コード生成は並列ワーカーでも実行されます。

```text
alignc build app.align --cache-stats -j 4
alignc cache clear
```

`--cache-stats` は処理順に、フロントエンド、コード生成の統計を報告します。

```text
alignc: cache: main frontend hit
alignc: cache: 1 frontend: 1 hit, 0 miss
alignc: cache: main hit
alignc: cache: 1 unit(s): 1 hit, 0 miss
```

**フロントエンドキャッシュ**は、各コンパイル単位の検査済みインターフェース、診断、リンクライブラリを保存し、別プロセスでの再ビルドでも検査を省けるようにします。キャッシュの識別には、その単位のソースのバイト列、検査時に参照した直接・間接の依存インターフェース一式、コンパイラとインターフェース形式の識別子、ターゲットトリプルを使います。プロファイル、`--target-cpu`、ランタイム LTO、PGO モードはフロントエンドの出力に影響しないため含めず、同じエントリを異なるビルド設定で使えます。

**コード生成キャッシュ**はオブジェクトのバイト列を保存します。識別にはプロファイル、ターゲット CPU、エクスポート指定、ランタイムのビットコード、LLVM の識別子、PGO モードも含めます。そのため `--profile` や `--target-cpu` を変えて同じソースをビルドすると、フロントエンドはヒットしてもコード生成はミスします。`--cache-stats` でその理由を確認できます。

```text
alignc: cache: main frontend hit
alignc: cache: main miss (profile)
```

キャッシュヒットは、タイムスタンプが新しいという意味ではなく、保存済みのバイト列を再利用できるという意味です。`-j` は `ALIGNC_JOBS` より優先します。`ALIGNC_CACHE=off` は両方のキャッシュを無効にし、`ALIGNC_CACHE=<path>` は保存先を変更します。2層は同じルートの別々のサブディレクトリに保存され、`alignc cache clear` はそのルートを空にします。

`pkg.db` パッケージ（第 [23](23-packages.md) 章）を使うプロジェクトでは、さらに 5 つのサブコマンドが使えます。それ以外のプロジェクトには関係しません。

```text
alignc db prepare file.align            検査済み SQLite/PostgreSQL メタデータを再生成
alignc db migrate --entry file.align    明示的なマイグレーションカタログを適用
alignc db status  --entry file.align    マイグレーションの状態を報告
alignc db check   --entry file.align    期待どおりの状態であることを要求
alignc db repair  --entry file.align    dirty な 1 件をチェックサム束縛で修復
```

## コンパイラが見たものを見る

```text
alignc emit-mir  file.align
alignc emit-llvm file.align --stage raw
alignc emit-llvm file.align --stage optimized
alignc emit-obj  file.align
alignc explain-opt file.align --verbose
alignc size file.align --profile tiny
```

`emit-mir` はプログラムの意味を表す中間表現を表示します。最適化前の LLVM IR では低レベルな表現への変換結果を、最適化後の IR では LLVM が最適化した結果を確認できます。`explain-opt` はベクトル化などの最適化に関する情報をソース行に対応づけます。`size` は選択したプロファイルで `build` と同じ成果物を作り、サイズの内訳を報告します。単独のオブジェクトや IR を出力するときは、`--export name` を繰り返し指定すると、エントリ単位の特定の関数を外部に公開できます。

## プロファイル、ターゲット、プログラム全体の最適化

```text
--profile dev|release|fast|small|tiny   # O0, O2, O3, Os, Oz
--target-cpu baseline|native|<LLVM CPU>
--rt-lto / --no-rt-lto                 # runtime bitcode LTO の強制 on/off（既定: release/fast で on）
--thin-lto                             # cross-unit ThinLTO
```

既定は移植性のある `baseline` ターゲットと `release` プロファイルです。`native` は現在のマシン向け、`x86-64-v3` などの LLVM CPU 名は配布先のハードウェアが決まっている場合に使えます。
ランタイム LTO は `release` / `fast` で**既定で有効**です。文字列述語のパイプラインで実測2〜3倍の速度改善があり、その他の測定では性能低下はなく、コンパイル時間の増加は1〜2msでした。`dev` / `small` / `tiny` では無効で、`--no-rt-lto` / `--rt-lto` で切り替えられます。`--thin-lto` はコンパイル時間と最適化の範囲を変えるため、明示的に指定します。`release` / `fast` のリンクを伴う `build` / `run` / `size` に適用され、並列化とキャッシュに対応し、ランタイム LTO と組み合わせられます。

本番を代表する処理を使って、実行時の計測結果に基づく PGO も利用できます。

```text
alignc build app.align --profile fast --pgo-instrument
./app                                      # 表示された .profraw file を書く
llvm-profdata-22 merge default.profraw -o app.profdata
alignc build app.align --profile fast --pgo-use app.profdata
```

コンパイラは生のプロファイルデータの出力先を表示します。計測モードと利用モードは同時に指定できず、キャッシュも別々です。現在は `--thin-lto` と組み合わせられませんが、`--rt-lto` とは併用できます。

プロファイルが存在しない、読めない、壊れている、バージョンが合わない場合はビルドエラーになります。読めても古いプロファイルや別のプログラムのプロファイルであれば、警告を出してビルドを続けます。この不一致は性能に影響しますが、プログラムの意味は変えません。

## リンカ

`alignc` はリンクをシステムの C ドライバ経由で行います。ELF ターゲットではさらに、LLVM の `ld.lld` を使うようそのドライバへ指示します。`ld.lld` は `alignc` が元から必要とする LLVM ツールチェーンに同梱されているため、新たにインストールするものはありません。環境変数 `ALIGNC_LINKER` で選択を固定できます。

```text
ALIGNC_LINKER=lld       ELF: ld.lld を使う。見つからなければ明示的に失敗
                        Mach-O: 黙って無視せず、ハードエラー
ALIGNC_LINKER=system    常にシステムリンカを使う
未設定（既定）          ELF: ツールチェーンに ld.lld があればそれ、なければシステムリンカ
                        Mach-O: 常にシステムリンカ
```

これ以外の値はエラーです。変わるのはリンク速度だけで、オブジェクト、保護用のリンクフラグ、プロファイルごとのシンボル削除、最適化は同じです。macOS では Apple のリンカを使い、この設定の影響は受けません。リンクに失敗すると、使ったリンカ名が表示されます。lld で失敗した場合には、システムのリンカへ切り替える `ALIGNC_LINKER=system` も案内されます。

## フォーマッタ

`alignc fmt` は整形したソースを出力し、`--write` はファイルを書き換えます。空白、`;` の配置、末尾のカンマ、位置揃えなど、意味に影響しない違いをそろえ、改行位置は保ちます。構文解析できないファイルは整形しません。日常的に使うと、差分で意味のある変更を読み取りやすくなります。

## lint

`check` と `build` は毎回 lint を実行します。ファイル単位の抑制機能はありません。

**ハードエラー**は正しいプログラムを書くための検査です。
- `unhandled Result`：返された `Result` を処理していない場合。

**警告**はビルドを止めずに、変換やメモリ使用上の問題を知らせます。
- `lossy conversion`：値の情報を失う可能性がある変換。
- `huge struct copy`：およそ2キャッシュライン（128バイト）を超える構造体のコピー。
- `unnecessary heap`：ヒープに確保した直後に値を取り出して捨てる処理。
- `wasteful default`：大きなリテラル配列に、必要以上に広い既定の要素型を使うこと。
- `unused import`：使われていないインポート。

警告の位置を確認し、データの配置や操作を変えれば改善できるかを検討してください。性能に関する警告を残す場合は、`explain-opt`、`size`、代表的なベンチマークで影響を測定します。

## `core.test` でテストを書く

モジュールのトップレベルに、名前を付けた `test` 宣言を書きます。アサーションを使うには `core.test` をインポートします。次を `arithmetic.align` として保存してください。`main` は不要です。

```align
module arithmetic

import core.test

fn twice(value: i64) -> i64 = value * 2

test "twice a positive number" {
    test.expect_eq(twice(21), 42)
}

test "zero stays zero" {
    test.expect(twice(0) == 0)
}
```

```text
alignc test arithmetic.align
```

指定したファイルと、そこから直接・間接にインポートするモジュールのテストを実行します。`tests/` ディレクトリやファイル名の末尾を使った自動探索は行いません。依存先のモジュールから順に、各モジュールでは宣言順に実行します。この例が成功すると、最後に `test result: ok. 2 passed; 0 failed` と表示されます。

`test.expect` は `bool` を検査し、`test.expect_eq` は通常のスカラーまたは文字列の等価比較を使います。アサーションはテストブロック内の文として書き、補助関数やクロージャの中には置けません。補助関数から `Result` を返し、テスト側で `?` を使うことはできます。ブロックの終わりまで進めば成功です。最初のアサーション失敗や返された `Err` でそのテストは失敗し、通常の後始末を行います。

ランナーは一度ビルドしてから、テストを別々のプロセスで1つずつ実行します。既定の上限は、1テスト60秒、標準出力と標準エラーがそれぞれ1 MiBです。`--timeout-ns` と `--max-output-bytes` で変更できます。`-j` が変えるのはコンパイルの並列度で、テストの同時実行数ではありません。失敗したテストがある場合や、テストが見つからない場合は、非ゼロの終了コードを返します。現在、テストからは補助関数経由でも `process.command` を呼べません。外部プロセスと組み合わせる検証には、別のテストスクリプトを使います。

`check` や `build` もテスト宣言を型検査しますが、通常の成果物にテストコードは含めません。`alignc test` が自動的に `main` を呼ぶことはなく、`align-repl` にテスト宣言を入力することもできません。その他のオプションと出力規則は[テスト機能の設計](../../impl/core-design/ja/test.md)を参照してください。

## align-repl

もう 1 つのバイナリ `align-repl` は AOT REPL です。`alignc` と同じリリースアーカイブ、`.deb`、Homebrew formula に同梱されているので、パッケージ版をインストールしていればすでに入っています。引数は取りません。

```text
$ align-repl
align> 1 + 2
3
```

セッションは**少しずつ書き足していく1つの Align プログラム**です。入力のたびに `alignc build` と同じドライバで全体を再コンパイルし、生成されたネイティブバイナリを実行します。インタプリタや JIT は使いません。プロファイル、ランタイム LTO の既定値、生成するオブジェクトは通常のコンパイルと同じです。

入力のたびにプログラム全体を実行し直します。すでに表示した出力は省き、新しい行だけを表示します。

```text
align> x := 5
align> print(x * 2)
10
```

同じ名前にもう一度束縛すると、**先に書いた行をその場で置き換え**、後続の行を新しい値で実行し直します。これにより、シャドーイングを禁止する規則を保ちます。以前の出力も変わる場合があるため、REPL は見出しを付けて実行結果全体を表示します。

```text
align> x := 21
align-repl: re-execution differs from the previous run (a replaced binding, nondeterminism, or an external side effect) — full output follows
42
align-repl: replaced entry 1
```

`:list` は実際にコンパイルされているプログラムを表示します。左がソース行番号、その隣がエントリの序数です。序数は再利用されないので、削除したエントリは欠番として残ります。

```text
align> :list
   1             | // generated by align-repl; every line below is real Align
   2             | // `main` is fixed at `-> Result<(), Error>` so `?` works in every entry
   3             | // every statement re-runs on each entry; external side effects are repeated
   4             | fn main() -> Result<(), Error> {
   5    1   main |   x := 5
   6    2   main |   print(x * 2)
   7             |   return Ok(())
   8             | }
```

値を print できないエントリは、代わりに型を表示します。このとき何も束縛されず、値も消費されません。

```text
align> xs := [1, 2, 3]
align> xs
<array<i64>[3]>
```

### コマンド

```text
:help              this text                :undo            remove the last entry
:quit              exit (also Ctrl-D)       :drop N          remove entry N
:list              show the program         :clear           drop every entry
:type EXPR         the type of EXPR         :out             reprint the last output
:const NAME := E   a top-level constant     :time [N]        time the built binary
:save PATH         write a .align file      :save! PATH      … overwriting
```

`:save` は実際にコンパイルしたプログラムを保存します。そのファイルを `alignc` でコンパイルすると、バイト単位で同じオブジェクトを生成します。

`:const` が必要なのは、自分で定義した `fn` から `main` の束縛が見えないからです。セッションの `x := …` は `main` の中のローカルです。関数から参照したい値はトップレベル定数にします。

```text
align> :const WIDTH := 6
align> fn area(h: i64) -> i64 = WIDTH * h
align> print(area(7))
42
```

`:time [N]` はビルド済みバイナリを N 回実行し、min/median/max を報告します。測っているのは**あなたのプログラム**であってコンパイラではありません。コンパイル時間は含まれず、各サンプルにはプロセス起動が含まれます。報告される起動時間の下限は、そこから差し引くためのものです。

```text
align> :time 3
3 runs: min 1.6 ms, median 1.8 ms, max 3.7 ms
```

Ctrl-D と `:quit` はどちらも終了します。Ctrl-C はセッションを終了させます。プログラムの実行中であれば、そのプログラムも一緒に終了します。REPL はシグナルハンドラを一切設置しないためです。

行編集は組み込まれていないため、矢印キーによるヒストリは使えません。必要であれば `rlwrap align-repl` として外側から付けられます。

先に知っておく価値のある制限が 2 つあります。1 つは、リージョンに属する値がエントリをまたげないことです。`arena` と `heap.new` は言語仕様上ブロックスコープであり、各エントリは 1 つの文なので、あるエントリで確保した box は次のエントリの時点ですでに drop されています。ブロックごと 1 つのエントリとして入力してください。括弧が開いている間、プロンプトは入力を読み続けます。

```text
align> arena {
...     ys := [1, 2, 3, 4, 5].map(fn v: i64 { v * 2 }).where(fn v: i64 { v > 4 }).to_array()
...     print(ys.sum())
...   }
24
```

もう 1 つは、行をまたぐメソッドチェーンが括弧の内側でしか継続しないことです。トップレベルでは 1 行に書くか、括弧で囲んでください。

`ALIGNC_CACHE`、`ALIGNC_JOBS`、`ALIGNC_LINKER` は `alignc` と同じドライバのコードが読み取り、REPL が独自に解釈し直すことはありません。したがってキャッシュを無効にしてもセッションはそのまま動きます。

```text
$ ALIGNC_CACHE=off align-repl
align> 6 * 7
42
```

## 意図的に欠けているもの

Align には、パッケージレジストリやリゾルバ、取得コマンド、プロジェクトのマニフェスト、デバッガ統合はまだありません。言語内のテストには、上で紹介した `alignc test` を使えます。ソースパッケージはプロジェクトの `pkg/` 以下に置き、依存グラフは `import` とファイルシステムで決まります。マニフェストや lockfile はありません。Homebrew や apt が配布するのはコンパイラとランタイムで、ソースパッケージは含みません。パッケージの使い方は第 [23](23-packages.md) 章で説明します。

ツールチェーンの中心は `alignc` です。インポートからビルド対象を決め、内容のハッシュで成果物を識別し、最適化の結果を検査できます。
