# ツールチェーン: alignc、フォーマッタ、リント

> 🌐 [English](../16-toolchain.md) · **日本語**

1 つのバイナリ `alignc` が、ツールチェーン全体を担います。コンパイラ、ランナー、フォーマッタ、そして自分が得ている機械語コードを監査させてくれる IR ダンプです。まだ学ぶべきビルドファイルの方言はありません。ビルドの単位はファイルとそのインポートです。

## 実際に使うコマンド

```text
alignc check file.align         # fast: parse + typecheck + lints, no codegen
alignc run   file.align [args…] # build + execute; trailing args → main(args)
alignc build file.align         # emit a native executable next to you
alignc fmt   file.align --write # normalize formatting in place
```

編集ループは `check`(サブ秒、すべての診断を表示)と `run` です。`build` はデプロイ可能な成果物、つまりただのネイティブ実行ファイルを渡します。同梱すべきランタイムはありません。複数ファイルのプログラムはエントリファイルからビルドされ、インポートはそこからの相対で見つかります(第 [09](09-generics-and-modules.md) 章)。

## コンパイラが見たものを見る

```text
alignc emit-mir  file.align     # the mid-level IR: what your code means
alignc emit-llvm file.align     # LLVM IR: what your code became
alignc emit-obj  file.align     # object file only (link it yourself)
```

`emit-llvm` は習慣にする価値があります。本書はパイプラインが融合しベクトル化されると繰り返し主張してきました。それを鵜呑みにしないでください。パイプラインの IR をダンプし、ループが 1 つであることと `<4 x i64>` のようなベクトル型を探してください。パフォーマンスの疑問が生じたら、答えはコマンド 1 つの先にあります。これが「四者同時最適化」の実践的な意味です。あなたのプログラムに対するハードウェアの視点は、伝聞ではなく検査可能なのです。

## フォーマッタ

`alignc fmt` は正規化された形を表示し、`--write` はファイルを書き換えます。その哲学はほとんどのフォーマッタより意図的に狭いものです。正規化するのは**無意味なばらつきだけ**、すなわちスペーシング、`;` の配置、末尾のカンマ、アラインメントです。改行を折り直したり、1 行対複数行を強制したりは**しません**。パイプラインが 1 行として読みやすいか 5 行として読みやすいかは、*あなたが*選んだ情報であり、フォーマッタはそれを保ちます。(パースできないファイルのフォーマットも拒否します。理解できないコードを「修正」することは決してありません。)常に走らせてください。差分は意味的なものだけになります。

## リント

`check`(そしてすべてのビルド)はリントスイートを走らせます。設定も `#[allow]` もありません。スイートは小さく意図的で、深刻度で分かれ方が普通ではありません。

**ハードエラー** — リントの服を着た正しさのルールです。

```text
unhandled Result        a discarded Result<_, _> — handle it with ? / match / a binding
```

**警告** — パフォーマンスの正直さです。ビルドをブロックすることは決してありません。

```text
lossy conversion        an `as` that truncates (defined behavior, but flagged)
huge struct copy        by-value copy past ~2 cache lines — take a view or restructure
unnecessary heap        a box that never escapes — use a plain value
wasteful default        a large literal array defaulting to a wider element than it needs
unused import           an import no code in the file uses
```

これらの多くは前の章で出会っています。実際の初心者コードで発火するからです。第 [05](05-memory.md) 章でヒープが不要だったボックス、第 [02](02-language-basics.md) 章の `i64 as i8` です。それが意図された体験です。リントは、あなたが書き終えたまさにその行で、言語のパフォーマンスモデルがあなたに語りかけるものであって、スタイルの取り締まりではありません。警告が発火したら、直し方はほぼ常に本書が教えるイディオムです。警告に同意できないときは、そのまま出荷できます(警告はビルドを失敗させません)。ただし先に測定してください。これらはそれぞれ、言語が他の手段では可視化できないコストを指しているからです。

## 意図的に欠けているもの(今のところ)

パッケージマネージャも、ビルドシステムも、テストランナーも、デバッガ統合もありません。プレリリースの段階では、単一バイナリのツールチェーンこそが要点です。上記のすべては今日動作し、腐敗しうる設定を持ちません。`pkg` レイヤ(フレームワーク、エコシステム)は core と std の*外*に住むよう設計されているので、言語が必須のビルド儀式を育てることは決してありません。ツールの表面は広がると見込んでください。そして哲学 — 1 つのバイナリ、ゼロ設定、要求に応じた IR — は変わらないと見込んでください。
