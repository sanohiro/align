# The Little Aligner

> 🌐 [English](../README.md) · **日本語**

*『The Little Schemer』(邦題『Scheme手習い』)の伝統にならって。*

これはリファレンスでも教科書でもありません — 教科書は[ガイド](../../guide/ja/README.md)のほうです。これは**ドリル本**です。小さな問いと小さな答えが延々と続く会話であり、一つひとつが前の問いから半歩だけ先へ進みます。Align のうち、ほかの言語とは似ていない部分 — pipeline、`match`、Move、arena、そしてデータを横倒しにして列にすること — を、その言い回しがひとりでに口をついて出るようになるまで、あなたの手に覚えさせます。

## 使い方

問いを読みます。**答えを読む前に、声に出して答えてください。** 当たっていたら先へ進む。外れていたら数問だけ戻る — 答えはそこで組み立てられています。プログラムが出てきたら実行してもかまいません(`alignc run`)。でもまずは自分がコンパイラになったつもりで。たいていの問いは、前のページだけを頼りに答えられます。

答えが一言のこともあります。前の問いとそっくり同じに見える問いもあります — その違いこそが学びです。そしてある規則がそれに値したとき、それは**戒律(Commandment)**として刻まれます。

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

ここにあるものはすべて、いまの `alignc` で動きます。では、召し上がれ。
