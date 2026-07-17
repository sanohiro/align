# ツールチェーン: alignc、フォーマッタ、lint

> 🌐 [English](../16-toolchain.md) · **日本語**

1 つの `alignc` binary に compiler、runner、formatter、cache control、inspection tool がまとまっています。multi-file program は 1 つの entry file から始まり、import が build graph を作るため、別の build-file dialect はありません。

## 実際に使うコマンド

```text
alignc check file.align         # whole-program の parse + typecheck + lint
alignc run   file.align [args…] # build + execute。後続引数は main(args) へ
alignc build file.align         # current directory に <stem> という executable
alignc fmt   file.align --write # formatting をその場で正規化
```

編集ループは `check` と `run` です。multi-file build は `.align` file ごとに 1 module を compile し、明示的な interface に対して import を検査し、到達可能な DAG を link します。`check-per-unit` は interface-based checker を公開し、`emit-interface` は各 unit の public surface と interface/implementation hash を表示します。

codegen は既定で有効な content-addressed object cache と parallel worker を使います。要求しない限り表示はしません。

```text
alignc build app.align --cache-stats -j 4
alignc cache clear
```

`-j` は `ALIGNC_JOBS` より優先されます。`ALIGNC_CACHE=off` で cache を無効化し、`ALIGNC_CACHE=<path>` で移動できます。cache identity には source/interface content、compiler と LLVM identity、target、profile、export、runtime bitcode、PGO mode が含まれます。したがって hit は単に timestamp が新しいという意味ではなく、byte を再利用できるという意味です。

## コンパイラが見たものを見る

```text
alignc emit-mir  file.align
alignc emit-llvm file.align --stage raw
alignc emit-llvm file.align --stage optimized
alignc emit-obj  file.align
alignc explain-opt file.align --verbose
alignc size file.align --profile tiny
```

`emit-mir` は意味の lens です。raw LLVM IR は最適化前の lowering を、optimized IR は LLVM が実際に作った形を示します。`explain-opt` は vectorization などの optimization remark を source line へ戻して説明します。`size` は選択 profile で `build` と同じ artifact を作り、byte の内訳を報告します。standalone object/IR では `--export name` を繰り返し、entry unit の選択した関数を外部公開できます。

## profile、target、whole-program optimization

```text
--profile dev|release|fast|small|tiny   # O0, O2, O3, Os, Oz
--target-cpu baseline|native|<LLVM CPU>
--rt-lto                               # 選択した runtime bitcode を inline
--thin-lto                             # cross-unit ThinLTO
```

既定は portable な `baseline` と `release` です。`native` は現在の machine 用、`x86-64-v3` のような名前付き LLVM CPU は既知の deployment fleet に向きます。`--rt-lto` と `--thin-lto` は compile cost と optimization scope を変えるため明示的です。どちらも `release` または `fast` が必要です。ThinLTO は link する `build` / `run` / `size` に適用され、parallel かつ cached で、runtime LTO と組み合わせられます。

代表的な production workload には instrumented PGO が使えます。

```text
alignc build app.align --profile fast --pgo-instrument
./app                                      # 表示された .profraw file を書く
llvm-profdata-22 merge default.profraw -o app.profdata
alignc build app.align --profile fast --pgo-use app.profdata
```

compiler は実際の raw profile 出力先を表示します。instrument と use mode は排他的で別々に cache され、現在 `--thin-lto` とは組み合わせられません。`--rt-lto` とは組み合わせられます。存在しない、読めない、壊れた、version 不整合の profile は hard error です。古い、または別 program の readable な profile は目立つ warning を出して build を続けます。profile mismatch が変えるのは performance であり program semantics ではないからです。

## フォーマッタ

`alignc fmt` は正規形を出力し、`--write` は file を書き換えます。spacing、`;` の配置、末尾 comma、alignment という意味のない差だけを正規化し、改行は保持します。parse できない file は format しません。diff を意味上の変更だけにするため、日常的に実行してください。

## lint

すべての check と build が lint suite を実行します。file ごとの suppression surface はありません。

**hard error** は correctness を守ります。

```text
unhandled Result        ?、match、else、binding のいずれかで処理する
```

**warning** は build を止めず、決定的な cost を見えるようにします。

```text
lossy conversion        情報を捨てうる `as`
huge struct copy        およそ 2 cache line を超える by-value copy
unnecessary heap        allocate して直ちに読む狭い形
wasteful default        大きな literal array が必要以上に広い推論 element を使う
unused import           その file で使われない imported capability
```

これらは style rule ではなく、source line で話す performance model です。まず data shape を直します。意図的に warning を残すなら `explain-opt`、`size`、代表的 benchmark で artifact を測ってください。

## 意図的に欠けているもの

Align 言語用 package registry/resolver、project manifest、general test runner、debugger integration はまだありません。Homebrew と apt が配布するのはコンパイラ本体であり、source dependency は将来の `pkg` layer として core と std の外に残します。現在の contract は意図的に小さく、1 binary、import-discovered build、content-identified artifact、inspectable optimization です。
