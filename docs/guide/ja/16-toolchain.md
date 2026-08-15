# ツールチェーン: alignc、align-repl、フォーマッタ、lint

> 🌐 [English](../16-toolchain.md) · **日本語**

単一の `alignc` バイナリの中に、コンパイラ、ランナー、フォーマッタ、キャッシュ制御、そしてコード検査ツールがすべて統合されています。もう 1 つの `align-repl` は、同じコンパイラを対話セッションとして動かします。複数ファイルで構成されるプログラム（マルチファイル・プログラム）は常に1つのエントリファイルから始まり、ファイル内の `import` 宣言が自動的にビルドグラフを構築するため、Makefile のような独自のビルドスクリプト言語（dialect）は必要ありません。

## 実際に使うコマンド

```text
alignc check file.align         # whole-program の parse + typecheck + lint
alignc run   file.align [args…] # build + execute。後続引数は main(args) へ
alignc build file.align         # current directory に <stem> という executable
alignc fmt   file.align --write # formatting をその場で正規化
```

日常的なコーディングにおける編集ループは `check` と `run` の繰り返しになります。マルチファイル・ビルドでは、`.align` ファイルごとに1つのモジュールとしてコンパイルが行われます。コンパイラは明示的なインターフェースに基づいて `import` の整合性を検査し、到達可能なモジュールの依存関係（DAG）をリンクします。`check-per-unit` コマンドを使用するとインターフェースベースのチェッカーを利用でき、`emit-interface` コマンドを使用すると各コンパイル単位の公開サーフェス（API）と、インターフェースおよび実装のハッシュ値を確認できます。

ビルドの背後では、コンテンツアドレス方式のキャッシュが 2 層動いています。どちらもデフォルトで有効で、明示的に要求しない限り動作の様子が表示されることはありません。コード生成（codegen）フェーズはこれに加えて並列ワーカーを使います。

```text
alignc build app.align --cache-stats -j 4
alignc cache clear
```

`--cache-stats` は、この 2 層をパイプライン順（フロントエンド、続いて codegen）で報告します。

```text
alignc: cache: main frontend hit
alignc: cache: 1 frontend: 1 hit, 0 miss
alignc: cache: main hit
alignc: cache: 1 unit(s): 1 hit, 0 miss
```

**フロントエンドキャッシュ**は、各コンパイル単位の検査済みインターフェースサマリ、診断メッセージ、リンクライブラリを保存します。これにより、別プロセスで再ビルドしてもその単位を検査し直す必要がなくなります。同一性判定に含まれるのはフロントエンドの入力だけです。すなわち、その単位のソースバイト列、検査に使った推移的なインターフェースクロージャ、コンパイラとインターフェース形式の識別子、そしてターゲットトリプルです。プロファイル、`--target-cpu`、ランタイム LTO、PGO モードは意図的に含まれていません。これらはフロントエンドの出力を変えないため、1 つのエントリがあらゆるビルド構成に効きます。

**codegen キャッシュ**はオブジェクトのバイト列を保存するため、同一性判定にはこれらバックエンド側のつまみ、つまりプロファイル、ターゲット CPU、エクスポート指定、ランタイムのビットコード、LLVM の識別子、PGO モードが含まれます。したがって、同じソースを別の `--profile` や `--target-cpu` でビルドし直すと、フロントエンドはヒットし codegen だけがミスします。`--cache-stats` はその理由も表示します。

```text
alignc: cache: main frontend hit
alignc: cache: main miss (profile)
```

どちらの層であれ「ヒット」は単にファイルタイムスタンプが新しいという意味ではなく、過去に生成したバイト列を安全に「再利用できる」という厳密な意味を持ちます。コマンドラインの `-j` オプションは環境変数 `ALIGNC_JOBS` よりも優先されます。`ALIGNC_CACHE=off` は 2 層とも無効化し、`ALIGNC_CACHE=<path>` は保存先を変更します。2 層は 1 つのキャッシュルート内の互いに素なサブツリーに置かれ、`alignc cache clear` はそのルートを空にします。

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

`emit-mir` コマンドは、プログラムの意味論（セマンティクス）を確認するためのレンズの役割を果たします。`raw` ステージの LLVM IR は最適化前のローリング（低レベル化）結果を示し、`optimized` ステージの IR は LLVM が実際に生成した最終的な形を示します。`explain-opt` コマンドは、自動ベクトル化などの「最適化に関する備考（optimization remark）」を元のソースコードの行に対応づけて説明します。`size` コマンドは、選択したプロファイルで `build` と全く同じ実行ファイル（アーティファクト）を作成し、そのファイルサイズ（バイト数）の内訳を報告します。スタンドアロンのオブジェクトファイルや IR を出力する際は、`--export name` オプションを複数回指定することで、エントリ単位の特定の関数を選択して外部へ公開（エクスポート）することができます。

## profile、target、whole-program optimization

```text
--profile dev|release|fast|small|tiny   # O0, O2, O3, Os, Oz
--target-cpu baseline|native|<LLVM CPU>
--rt-lto / --no-rt-lto                 # runtime bitcode LTO の強制 on/off（既定: release/fast で on）
--thin-lto                             # cross-unit ThinLTO
```

デフォルトのターゲットとプロファイルは、ポータブルな `baseline` および `release` です。`native` はコンパイルを実行している現在のマシンに最適化されたターゲットであり、`x86-64-v3` のような名前付きの LLVM CPU アーキテクチャ指定は、デプロイ先のハードウェア環境（フリート）が既知の場合に推奨されます。
ランタイム LTO は、最適化プロファイル（`release`/`fast`）では**デフォルトで有効**です（文字列述語パイプラインで実測 2〜3 倍、他は非退行、コンパイル時間 +1〜2ms）。`dev`/`small`/`tiny` では無効で、`--no-rt-lto` / `--rt-lto` でどちらの方向にも強制できます。`--thin-lto`（クロスモジュールの ThinLTO）は、コンパイル時間（コスト）と最適化の適用範囲を大きく変えるため、引き続き明示的な指定が必要です。`release` または `fast` プロファイルでのみ機能し、リンク処理を伴う `build`、`run`、`size` コマンドに適用され、並列処理やキャッシュ機構の恩恵を受けながら、ランタイム LTO と組み合わせることが可能です。

代表的な production workload には instrumented PGO が使えます。

```text
alignc build app.align --profile fast --pgo-instrument
./app                                      # 表示された .profraw file を書く
llvm-profdata-22 merge default.profraw -o app.profdata
alignc build app.align --profile fast --pgo-use app.profdata
```

コンパイラは、実際の生のプロファイルデータ（`.profraw`）の出力先パスを表示します。プロファイリング用の計測（instrument）モードと、そのデータの利用（use）モードは排他的であり、キャッシュもそれぞれ独立して管理されます。現時点では `--thin-lto` と組み合わせることはできませんが、`--rt-lto` との組み合わせは可能です。

指定されたプロファイルファイルが存在しない、読み取れない、破損している、あるいはバージョンが不整合である場合は、「ハードエラー（コンパイル失敗）」になります。一方で、ファイル自体は正常に読み取れるが、ソースコードの変更によって内容が古くなっていたり、あるいは全く別のプログラムのプロファイルデータを渡したりした場合は、目立つ警告（warning）を出した上でコンパイルを続行します。なぜなら、プロファイルデータの不一致が影響を与えるのはプログラムの「パフォーマンス」のみであり、プログラムの「意味論（セマンティクスや正しい動作）」を壊すことはないからです。

## リンカ

`alignc` はリンクをシステムの C ドライバ経由で行います。ELF ターゲットではさらに、LLVM の `ld.lld` を使うようそのドライバへ指示します。`ld.lld` は `alignc` が元から必要とする LLVM ツールチェーンに同梱されているため、新たにインストールするものはありません。環境変数 `ALIGNC_LINKER` で選択を固定できます。

```text
ALIGNC_LINKER=lld       ELF: ld.lld を使う。見つからなければ明示的に失敗
                        Mach-O: 黙って無視せず、ハードエラー
ALIGNC_LINKER=system    常にシステムリンカを使う
未設定（既定）          ELF: ツールチェーンに ld.lld があればそれ、なければシステムリンカ
                        Mach-O: 常にシステムリンカ
```

これ以外の値はハードエラーです。変わるのはリンク速度だけで、オブジェクト、衛生フラグ、プロファイルごとの strip、適用される最適化はどちらでも同一です。つまりこれは `--profile` の代わりではありません。macOS は影響を受けません。Apple のリンカはすでに十分高速で、Mach-O が lld を選ぶことはないからです。リンクに失敗した場合は実際に走ったリンカ名が示され、lld でのリンク失敗ではさらに `ALIGNC_LINKER=system` という逃げ道も示されます。

## フォーマッタ

`alignc fmt` はソースコードを標準的なフォーマット（正規形）に整形して出力し、`--write` オプションを付けるとファイル自体を書き換えます。このフォーマッタは、スペースの数、セミコロン `;` の配置、末尾のカンマ、インデントの揃え方といった「意味を持たない構文の差」だけを正規化し、プログラマが意図した改行位置はそのまま保持します。文法エラーがありパースできないファイルはフォーマットされません。Git などのバージョン管理システム上の diff（差分）を「プログラムの意味上の変更」だけにするために、日常的なコーディングの習慣として実行してください。

## lint

すべての `check` コマンドおよび `build` コマンドの実行時に、組み込みの Lint スイートが自動的に走ります。特定のファイルや行単位で Lint 警告を抑制（suppress）する機能はありません。

**ハードエラー（コンパイル失敗）** はプログラムの正当性（correctness）を守るためのものです。
- `unhandled Result`： 返された `Result` 型を `?` 演算子、`match`、`else` ブロック、または変数への束縛のいずれかで適切に処理していない場合に発生します。

**警告（warning）** はビルド自体を止めませんが、パフォーマンス上の「決定的なコスト」を可視化するためのものです。
- `lossy conversion`： `as` キャストによってデータが失われる（切り捨てられる）可能性がある変換。
- `huge struct copy`： およそ 2 キャッシュライン（128 バイト）を超えるような巨大な構造体の値渡し（コピー）。
- `unnecessary heap`： ヒープ領域にアロケートした直後に、すぐに値を読み取って捨てるような非効率な処理パターン。
- `wasteful default`： 巨大なリテラル配列において、コンパイラが推論した要素の型が必要以上に広い（メモリの無駄遣いになっている）状態。
- `unused import`： そのファイル内で一度も使用されていないインポート（無駄なケイパビリティの要求）。

これらの警告は単なる「コーディングスタイルのルール」ではなく、ソースコードの行単位で語りかけてくる「パフォーマンスモデルからのフィードバック」です。警告が出た場合は、まずデータ構造（データシェイプ）を修正することを検討してください。もし正当な理由があって意図的に警告を残すのであれば、`explain-opt` や `size` コマンド、あるいは代表的なベンチマークテストを用いて、最終的なアーティファクトの性能を必ず計測してください。

## align-repl

もう 1 つのバイナリ `align-repl` は AOT REPL です。`alignc` と同じリリースアーカイブ、`.deb`、Homebrew formula に同梱されているので、パッケージ版をインストールしていればすでに入っています。引数は取りません。

```text
$ align-repl
align> 1 + 2
3
```

インタプリタも JIT もありません。セッションは**1 つの育っていく Align プログラム**です。各エントリがそこに差し込まれ、プログラム全体が `alignc build` と同じドライバ呼び出しで再コンパイルされ、生成されたネイティブバイナリが実行されます。したがって挙動は本番のコンパイルと同一です。profile も `rt-lto` の既定も、生成されるオブジェクトも同じものです。

再実行はこのモデルの本質であり、実装の都合ではありません。エントリごとにプログラム全体が再び走るため、すでに見た出力は省略され、新しい行だけが表示されます。

```text
align> x := 5
align> print(x * 2)
10
```

同じ名前への再束縛は、**先の行をその場で書き換えます**。Align はシャドーイングを禁止しているので、他の意味になりようがないからです。そして後続の行が新しい値で再実行されます。これは以前の出力を変えるので、REPL は差分を隠さず、バナーを添えて実行結果の全体を表示します。

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

`:save` が出口です。コンパイルされたプログラムそのものを書き出すので、そのファイルに対する `alignc build` は同じオブジェクトを生成します。

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

現時点の Align には、専用のパッケージレジストリやリゾルバ、取得コマンド、プロジェクトのマニフェストファイル（`Cargo.toml` や `package.json` のようなもの）、汎用的なテストランナー、高度なデバッガ統合などはまだ用意されていません。ただし `pkg` レイヤー自体はすでに利用でき、依存ソースをプロジェクトの `pkg/` 以下へ vendoring します。依存グラフは `import` とファイルシステムから決まり、マニフェストや lockfile はありません。Homebrew や apt が配布するのはコンパイラとランタイムであり、これらのソースパッケージは含みません。現在のパッケージモデルは第 [23](23-packages.md) 章で解説します。

現在のツールチェーンのスコープ（契約）は意図的に小さく保たれており、「単一のバイナリ」、「`import` 宣言から自動発見されるビルド」、「コンテンツベースのハッシュで識別されるアーティファクト」、そして「内部が検査可能な最適化プロセス」の4点を中核としています。
