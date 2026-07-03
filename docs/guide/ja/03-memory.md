# メモリ: value, arena, heap

> 🌐 [English](../03-memory.md) · **日本語**

Align にはガベージコレクタも、手動の `free` もありません。その代わり、データがどこに生きるかはあなたが選ぶプロパティであり、コンパイラがその後始末(クリーンアップ)を挿入します。生きる場所は3つあります。

## Value(デフォルト)

ほとんどのデータはただの value(値) — 数値、小さな構造体、固定長配列です。value はスタック上に生き、小さければコピーされ、何も考える必要はありません。プリミティブと小さな構造体は **Copy**(コピー)です — 受け渡しすればそのたびに複製されます。value を明示的に free することはありません。スコープが終わればそれで消えます。

```align
Point { x: f64, y: f64 }

fn main() -> i32 {
    p := Point { x: 1.0, y: 2.0 }   // a value, lives here, no cleanup needed
    return 0
}
```

## Arena(まとめて解放する、スコープ付きの allocation)

同じライフタイムを持つ多くのものを allocate する必要があるとき — ドキュメントをパースする、グラフを構築する、リクエストを処理する、といった場面では `arena` を使ってください。arena の内側で allocate されたものはすべて、arena ブロックが終わるときに、ひとつの安価な操作でまとめて解放されます。オブジェクトごとの管理は不要です。

```align
arena {
    // allocations in here draw from the arena
    b := heap.new(42)      // a box, allocated in this arena
    x := b.get()           // read the value back
    // ... build more ...
}   // the whole arena is freed here, all at once
```

arena は「たくさん allocate してから、それを丸ごと捨てる作業フェーズがある」という状況への答えです。速く(ポインタを進めるだけの bump pointer 方式)、断片化(フラグメンテーション)がなく、free は allocate した量に関係なく O(1) です。これはメモリをライフタイムでバッチ化するという、データ指向的なメモリ管理のやり方です。

## Heap box(ヒープボックス)

`box<T>` は、`heap.new(x)` で作られる明示的な単一の heap allocation です。現在の設計では、box は取り囲む arena の中に生き(つまりそのライフタイムは arena のものになり)ます。`.get()` は値をコピーして取り出し、`.clone()` は box を深くコピー(deep-copy)します。

```align
arena {
    b := heap.new(100)
    v := b.get()           // v == 100
}
```

## Move 型と escape(脱出)

`string`、`array`、`buffer`、`box` のような、heap のリソースを所有する型は Copy ではなく **Move**(ムーブ)です。ある変数に代入すると所有権が移動し、元の束縛は二度と使えなくなります。これが、ガベージコレクタも可視のライフタイムもなしに、二重解放(double-free)や解放後使用(use-after-free)がないことを Align が保証する方法です。

```align
a := some_string()
b := a          // ownership moves to b
// using `a` here is a compile error — it was moved
```

そして、arena の中で allocate された値は、その arena を **escape(脱出)** することができません — それを return すること、あるいはもっと長生きする場所に格納することは、コンパイルエラーになります。コンパイラは各値が属する region(領域)を追跡します。あなたがライフタイム注釈を書くことは決してなく、コンパイラがそれを推論し、ダングリング(不正な参照)になるプログラムを単に拒否するだけです。

## 身につけるべき習慣

「このデータのライフタイムは何か」と自問してください。ローカルなライフタイムを持つ単一の値 → ただの value でよい。共有された、スコープ付きのライフタイムを持つひとかたまりのデータ → arena。この判断を、作業の各フェーズごとに一度だけ下すこと、それが Align におけるメモリ管理のすべてです。
