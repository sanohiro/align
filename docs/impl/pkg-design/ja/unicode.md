# pkg.unicode — 明示的な Unicode 正規化

コピー可能なパッケージは `apps/unicode/pkg` です。このサブツリーをアプリケーションの
パッケージ検索ルートへ配置して `pkg.unicode` を import します。Unicode 16.0.0 の NFC
を提供します。文字列のバイト単位の等価性や順序は変更しません。

## 公開 API

```align
pub fn version() -> str
pub fn nfc(input: str) -> Result<string, Error>
```

`version()` は確保やエラーなしで静的な `"16.0.0"` を返します。`nfc` は UTF-8 入力を
借用し、独立して所有する UTF-8 文字列を返します。既に NFC の入力でも、結果は入力や
入力の arena を保持しません。通常の Move・Result・string Drop の規則に従います。
空入力は確保なしで空の所有文字列を返します。

NFC は正準分解、同一結合クラスの順序を保つ正準順序化、正準合成を行います。
Hangul と完全な合成除外規則も対象です。埋め込み NUL、非文字、未割り当ての Unicode
スカラー値を受け入れます。ロケール依存、暗黙の正規化、置換デコード、任意の入力長上限、
stream-safe 文字の挿入はありません。他の正規化形式、書記素分割、大小文字・ロケール操作
はこのパッケージの対象外です。

一時領域と最終出力の確保は失敗を返せます。入力・一時領域・出力の大きさをターゲット上で
表現できなければ `Error.Invalid`、確保を拒否されたらターゲットの native ENOMEM 値を使う
`Error.Code(ENOMEM)` を返します。ソースの有効な `str` は既に UTF-8 を保証しますが、
native 境界の不正なメタデータや文字列も Invalid になります。正規化経路に隠れた最終 clone
や、メモリ不足で中断する確保はありません。変換は Pure であり、Pure closure 内の通常の
ヒープ確保は引き続き許可されます。

```align
import pkg.unicode

fn normalized() -> Result<string, Error> {
  // 入力は e と結合アキュートアクセントです。
  return pkg.unicode.nfc("é")
}
```

Unicode データは runtime に組み込まれ、OS やロケールに依存しません。更新時は
パッケージ・コンパイラーのソース変更として `version()` と適合性検証を同時に更新します。
公式データ、ソースのハッシュ、Unicode License V3 は
`crates/align_runtime/data/unicode-16.0.0` に保持します。オフラインの
`scripts/generate-unicode.py --check` がテーブルを再現します。native テストは公式 NFC
の全 5 列と、Part1 にない単一文字の恒等性規則を検証します。

空の `pkg.unicode.internal.descriptor` はコンパイラー内部用です。正規の wrapper だけが
内部操作を呼べます。アプリケーションから内部 bridge や runtime シンボルを直接利用
することはできません。[Plan191](../../191-unicode-nfc.md) が native ABI、検証順序、確保、
コンパイラーの検証範囲を定義します。voice V004 の採用と tokenizer parity 検証は
consumer 側が担当します。
