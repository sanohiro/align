# The Little Aligner

> 🌐 [English](../README.md) · **日本語**

*『The Little Schemer』（邦題『Scheme手習い』）の伝統にならって。*

これはリファレンスでも教科書でもありません — 教科書は[ガイド](../../guide/ja/README.md)のほうです。これは**ドリル**です。小さな問いと答えが続く対話であり、一歩ずつ着実に前へ進みます。Align のうち、ほかの言語とは似ていない部分 — pipeline、`match`、Move、arena、そしてデータを列指向（カラムナー）に配置すること — を、自然に書けるようになるまで手に覚えさせます。

すべての syntax や library module を案内する本ではありません。見たことのない問題を前にして、自分から Align の問いを立てられるようになったとき、この本は成功しています。data はどんな形か。これは flow か、choice か、control の円か。誰が所有し、どれだけ生き、machine はどの column に触れ、cost はどこに書かれているか。目標は Align と顔見知りになることではありません。あなたを **aligner** にすることです。

## 使い方

問いを読みます。**答えを読む前に、声に出して答えてください。** 当たっていたら先へ進む。外れていたら数問だけ戻る — 答えはそこで組み立てられています。プログラムが出てきたら実行してもかまいません(`alignc run`)。でもまずは自分がコンパイラになったつもりで。大半の問いは、前のページまでの知識だけで答えられます。

答えが一言のこともあります。前の問いとそっくり同じに見える問いもあります — その違いこそが学びです。そしてある規則が十分に重要であるとき、それは**戒律（Commandment）**として刻まれます。

この本を一度読み終えたら、気に入ったプログラムを一つ選び、実行せずにもう一度読んでください。答えを予想する。データを追う。すべての値がどこで死ぬかを指さす。パス、アロケーション、コピーを数える。[最後の章](15-read-it-four-ways.md)では、この二度目の読み方を練習します。見覚えがあるだけなら、そのページは親しいだけです。思い出せるようになって、はじめてその言語は自分のものになります。

## 各章

1. [Toys](01-toys.md) — 値、束縛、そして関数
2. [Do It Again](02-do-it-again.md) — `map`
3. [Keep Some](03-keep-some.md) — `where` とフィールド射影
4. [Collapse It](04-collapse-it.md) — 畳み込み: `sum`、`count`、`reduce`、その仲間たち
5. [Chains](05-chains.md) — pipeline 全体、そしてなぜそれがループ1回で済むのか
6. [One of Many](06-one-of-many.md) — sum type と `match`
7. [Maybe, or It Failed](07-maybe-or-it-failed.md) — `Option`、`Result`、`?`
8. [Whose Is It?](08-whose-is-it.md) — Copy、Move、arena、そして `.clone()`
9. [Turn It Sideways](09-turn-it-sideways.md) — `soa`: 列としてのデータ
10. [Count Me by Name](10-count-me-by-name.md) — `group_by`、`agg`、`dict_encode`
11. [Do It Until](11-do-it-until.md) — `loop` 式、pipeline で言い表せないとき
12. [Do It Apart](12-do-it-apart.md) — Pure な仕事、`par_map`、structured task
13. [Four at a Time](13-four-at-a-time.md) — 明示的 SIMD、ベクトル、マスク
14. [The Big Crunch](14-the-big-crunch.md) — mmap、ゼロコピーのパイプライン、そしてすべてを統合する
15. [Read It Four Ways](15-read-it-four-ways.md) — 答え、流れ、寿命、仕事量

ここにあるものはすべて、いまの `alignc` で動きます。それでは、始めましょう。
