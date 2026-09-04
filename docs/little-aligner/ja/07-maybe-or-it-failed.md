# 7. Maybe, or It Failed

> 🌐 [English](../07-maybe-or-it-failed.md) · **日本語**

**Q1.** `[1, 3, 5]` から最初の偶数を探すと、何が見つかりますか？

**A1.** 見つかりません。値がない場合も表せる型が必要です。`Option<i64>` なら、見つかった値を `Some(n)`、見つからなかったことを `None` で表せます。

---

**Q2.** `None` は `null` と同じですか？

**A2.** `None` は `Option` のバリアントです。型を見れば、値がない場合もあるとわかります。中の `i64` を使うには、`Some` と `None` の両方を扱う必要があります。

---

**Q3.** では `Some(5)` から数を取り出すにはどうしますか？

**A3.** 代替値を使うなら `x := maybe else 0` と書きます。`Some` なら中の値、`None` なら `0` が `x` になります。それぞれの場合に別の処理をしたいなら、`match` で `Some(n)` と `None` を扱います。

---

**Q4.** `safe_head` は先頭の要素を `Some` で返し、空のスライスなら `None` を返す関数とします。`safe_head([1, 2, 3]) else -1` は何ですか？

**A4.** `1`。空のスライスなら `-1` です。値がない場合の扱いを、呼び出し側で選べます。

`safe_head` 自体は、Q20 で書いてみます。

---

**Q5.** さて失敗です。ファイルを読もうとする関数は、どんな型を返しますか？

**A5.** `Result<string, Error>` — `Ok(contents)` か `Err(why)` のどちらか。失敗もまた「値」であり、その中に理由が入っています。

---

**Q6.** `Option` と `Result` の違いは何ですか？

**A6.** `None` は「値がない」を表します。「最初の偶数がない」など、通常起こりうる結果です。`Err` は失敗とその理由を表します。値の有無だけでよければ `Option`、失敗の理由も伝えたいなら `Result` を使います。

---

**Q7.** ここに、失敗しうる別の関数を呼ぶ、失敗しうる関数があります:

```align
fn load(path: str) -> Result<i64, Error> {
    data := fs.read_file(path)?
    return Ok(data.len())
}
```

`?` は何をしていますか?

**A7.** `read_file` が `Ok(s)` を返したら、`s` を `data` に束縛して続行します。`Err(e)` なら、その場で `Err(e)` を呼び出し元へ返します。

---

**Q8.** では、最終的に誰がエラーを *処理* するのですか？

**A8.** その場で対処できる関数が処理します。呼び出し元へ渡すなら `?`、理由を調べて対処するなら `match` です。`main() -> Result<(), Error>` まで届いた `Err` は、非ゼロの終了コードになります。例外によるスタックの巻き戻しはありません。

---

**Q9.** どうでもいい `Result` は、ただ無視してもいいですか？

```align
fs.write_file("log.txt", "hi")
```

**A9.** コンパイルエラーです。`?`、`match`、`else` のいずれかで扱うか、変数に束縛してあとで処理します。Q12 で見るように、`else` で理由を捨てることもできますが、その選択はコードに書きます。

---

**Q10.** 組み込みの `Error` には何が入っていますか？

**A10.** `NotFound`、`Invalid`、`Denied`、`Timeout`、`Code(i32)` の5つのバリアントです。`match` で、それぞれの場合を扱えます。`main` から返すと、最初の4つは順に終了コード `1`、`2`、`3`、`4` になり、`Code(c)` は `c` になります。

---

**Q11.** 自分でエラー型を作り、それを返す関数に `?` を使うには？

**A11.** `ParseErr { Empty, BadChar }` のように宣言します。`inner` が `Result<i64, ParseErr>` を返し、呼び出し元のエラー型も `ParseErr` なら、`inner(n)?` と書けます。組み込みの `Error` など、別のエラー型に合わせるときだけ変換が必要です。

```align
v := inner(n).map_err(to_error)?
```

`?` は暗黙のうちに型を変換することはありません。`map_err` を使うことで、`ParseErr` が `Error` に変換された場所が読み手にも明確に伝わります。

---

**Q12.** `Result` にも `else` を使えますか？

**A12.** はい。`value := result else fallback` と書けます。これは `Err` のペイロードを捨て、代替値を使うことを明示します。理由が本当に不要な場合だけ使い、理由を確認するなら `match`、失敗を先へ渡すなら `?` を使ってください。

---

**Q13.** `safe_head` を3回呼びます。

```align
a := safe_head([7, 8]) else 0
b := safe_head([]) else 0
c := safe_head([]) else -1
```

それぞれは？

**A13.** `a` は `7`、`b` は `0`、`c` は `-1`。`safe_head` は値の有無を返し、呼び出し側が必要に応じて代替値を選びます。

---

**Q14.** ニックネームが未登録なことと、必須の入力ファイルがないこと。同じ型で表しますか？

**A14.** ニックネームには `Option<str>` が適しています。未登録でも通常の状態だからです。必須ファイルの読み込みには `Result<string, Error>` を使い、失敗の理由を伝えます。

---

**Q15.** 両方の処理が成功する場合を追ってください。

```align
fn load_score(path: str) -> Result<i64, Error> {
    text := fs.read_file(path)?
    score := parse_score(text).map_err(to_error)?
    return Ok(score)
}
```

**A15.** `read_file` が `Ok(text)`、`parse_score` が `Ok(score)` を返し、関数がその点数を `Ok` に入れて返します。

---

**Q16.** ファイルがありません。`parse_score` は動きますか？

**A16.** 動きません。最初の `?` が `read_file` の `Err` を `load_score` から返すので、`parse_score` には進みません。

---

**Q17.** ファイルはあるがテキストが不正です。どのエラーが外へ出ますか？

**A17.** `parse_score` のエラーです。`map_err(to_error)` で `Error` に変換され、2つ目の `?` が呼び出し元へ返します。

---

**Q18.** 不正な点数は0にするが、ファイルがないことは失敗のままにしたい。`else 0` はどこ？

**A18.**

```align
text := fs.read_file(path)?
score := parse_score(text) else 0
return Ok(score)
```

`else 0` を使うのは解析結果だけです。ファイルを読めなかった場合のエラーは、そのまま呼び出し元へ返します。

---

**Q19.** `fs.read_file(path) else ""` にもしないのはなぜ？

**A19.** 読めなかったファイルを空の入力として扱いたいなら、そう書けます。ただし、読み込みの失敗と空ファイルを区別できなくなります。求める動作に合うか確認してください。

---

**Q20.** では、`safe_head(xs: slice<i64>) -> Option<i64>` を書いてください。`xs[0]` を読む前に、何を確かめますか？

**A20.** 要素があるかどうかです。

```align
fn safe_head(xs: slice<i64>) -> Option<i64> {
    if xs.len() == 0 { return None }
    return Some(xs[0])
}
```

空なら、配列にアクセスする前に戻ります。要素があれば、その値が0でも負の数でも `Some` を返します。

---

**Q21.** どちらも `-1` と表示されます。同じ意味でしょうか？

```align
found := safe_head([-1, 7])
missing := safe_head([])
print(found else -1)
print(missing else -1)
```

**A21.** 違います。`found` は `Some(-1)`、`missing` は `None` です。代替値を使うと、この違いが見えなくなります。区別したいなら、バリアントを調べます。

```align
print(match found { Some(_) => true, None => false })     // true
print(match missing { Some(_) => true, None => false })   // false
```

呼び出し側にとって、この区別が不要なときに代替値を使います。

---

**Q22.** 先頭の要素に1を足し、空なら `None` を返したいとします。`safe_head(xs)` の値を `?` で取り出せますか？

**A22.** できません。`?` は `Result` 専用です。`Option` では、`else` 側で関数から戻れます。

```align
fn head_plus_one(xs: slice<i64>) -> Option<i64> {
    head := safe_head(xs) else { return None }
    return Some(head + 1)
}
```

`head_plus_one([4, 9])` は `Some(5)` を返します。空なら、足し算をする前に `None` を返します。

---

> **第七の戒律**
>
> *「ない」は `Option`、失敗は `Result`。失敗は `?` で上へ渡し、`Result` を一つも無視して放置するな。*
