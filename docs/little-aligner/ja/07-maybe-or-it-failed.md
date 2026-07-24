# 7. Maybe, or It Failed

> 🌐 [English](../07-maybe-or-it-failed.md) · **日本語**

**Q1.** `[1, 3, 5]` の中の偶数のリスト — その最初の要素は何ですか？

**A1.** ありません。そして「ない」ということにも型が要ります: `Option<i64>` — `Some(n)` か `None` のどちらか。

---

**Q2.** `None` は `null` と同じですか？

**A2.** いいえ。そしてその違いこそが重要です。`null` はどんな参照の中にも潜んで待ち構えています。`None` は `Option` の中にだけ存在し、型システムは、それがないときに何が起きるかをあなたが指定するまで、`i64` を渡してくれません。

---

**Q3.** では `Some(5)` から数を取り出すにはどうしますか？

**A3.** 方法は2つあります。手軽な方法: `x := maybe else 0` — ペイロード、さもなくばあなたが指定した代替値。丁寧な方法: `match` で `Some(n) =>` と `None =>` の両方を網羅的に処理します。

---

**Q4.** `safe_head` が `Option<i64>` を返すとして、`safe_head([1, 2, 3]) else -1` は何ですか？

**A4.** `1`。そして空の slice に対しては `-1`。「ない」場合の扱いは呼び出し側が決めました — 関数側で勝手に決める必要はなかったのです。

---

**Q5.** さて失敗です。ファイルを読もうとする関数は、どんな型を返しますか？

**A5.** `Result<string, Error>` — `Ok(contents)` か `Err(why)` のどちらか。失敗もまた「値」であり、その中に理由が入っています。

---

**Q6.** `Option` と `Result` の違いは何ですか？

**A6.** `None` はよくある一般的な回答です（「最初の偶数はない」 — 問題ありません）。`Err` は事情のある失敗です(`NotFound`、`Denied`…)。「ない」ことがありふれているなら `Option`。誰かが *なぜ* を知る必要があるなら `Result` です。

---

**Q7.** ここに、失敗しうる別の関数を呼ぶ、失敗しうる関数があります:

```align
fn load(path: str) -> Result<i64, Error> {
    data := fs.read_file(path)?
    return Ok(data.len())
}
```

`?` は何をしていますか?

**A7.** たった1文字で、エラー処理のすべてをこなします。`read_file` が `Ok(s)` で返ってきたら `data` は `s` になり、続行します。`Err(e)` なら、いますぐ `Err(e)` を呼び手へ **return** します。値を取り出すか、エラーで抜けるかのどちらかです。

---

**Q8.** では、最終的に誰がエラーを *処理* するのですか？

**A8.** 処理できる者がします。各層は、それを上位へ渡す（`?`）か、`match` で適切に処理するかのどちらかを選択します。いちばん上の `main() -> Result<(), Error>` で、最後まで到達した `Err` が非ゼロの終了コードに変わります。エラーは常にただの「値」として伝播します — 隠れたスタックの巻き戻し（unwinding）や、遠く離れた場所での例外キャッチもありません。

---

**Q9.** どうでもいい `Result` は、ただ無視してもいいですか？

```align
fs.write_file("log.txt", "hi")
```

**A9.** コンパイラはノーと言います — *未処理の Result*、これはハードエラーです。ちゃんと処理してください(`?`、`match`、`else`、あるいは変数に束縛して後で決める)。Align では失敗してもよいのですが、失敗を *黙って握りつぶす* ことは許されません。

---

**Q10.** 組み込みの `Error` には何が入っていますか？

**A10.** OSが返すエラーのカテゴリ — `NotFound`、`Invalid`、`Denied` — と、残りのための `Code(n)` です。ほかの sum type と同じように `match` します(6章で学んだとおり。`Error` はタグの付いたただの sum type です)。

---

**Q11.** では、自分のエラー型 — そして継ぎ目をまたぐ `？` は?

**A11.** 型を宣言し（`ParseErr { Empty, BadChar }`）、境界で **明示的に** 変換します:

```align
v := inner(n).map_err(to_error)?
```

`?` は暗黙のうちに型を変換することはありません。`map_err` を使うことで、`ParseErr` が `Error` に変換された場所が読み手にも明確に伝わります。

---

**Q12.** `Result` に `else` を — 使えますか？

**A12.** はい。`value := result else fallback` と書けます。これは `Err` のペイロードを捨て、代替値を使うことを明示します。理由が本当に不要な場合だけ使い、理由を確認するなら `match`、失敗を先へ渡すなら `?` を使ってください。

---

**Q13.** `safe_head` を3回呼びます。

```align
a := safe_head([7, 8]) else 0
b := safe_head([]) else 0
c := safe_head([]) else -1
```

それぞれは？

**A13.** `a` は `7`、`b` は `0`、`c` は `-1`。producer は absence だけを報告し、caller がそれぞれの意味を与えました。

---

**Q14.** 任意の nickname がないことと、必須の入力 file がないこと。同じ型？

**A14.** いいえ。nickname は `Option<str>`。なくても普通です。必須 file は `Result<string, Error>`。理由が必要になりうる失敗です。

---

**Q15.** happy path を追ってください。

```align
fn load_score(path: str) -> Result<i64, Error> {
    text := fs.read_file(path)?
    score := parse_score(text).map_err(to_error)?
    return Ok(score)
}
```

**A15.** `read_file` が `Ok(text)`、`parse_score` が `Ok(score)` を返し、関数がその score を `Ok` に入れて caller へ返します。

---

**Q16.** file がありません。`parse_score` は動きますか？

**A16.** いいえ。最初の `?` が file の `Err` をただちに `load_score` から返します。後ろの仕事は途中まで行われるのではなく、始まりません。

---

**Q17.** file はあるが text が不正です。どの error が外へ出ますか？

**A17.** `parse_score` の error が `map_err(to_error)` で明示的に `Error` へ変換され、2つ目の `?` が上へ渡します。

---

**Q18.** 不正な score は0にするが、file がないことは失敗のままにしたい。`else 0` はどこ？

**A18.**

```align
text := fs.read_file(path)?
score := parse_score(text) else 0
return Ok(score)
```

policy は、それが変える境界に置きます。file error は物語を保ち、parse error だけを意図的に捨てます。

---

**Q19.** `fs.read_file(path) else ""` にもしないのはなぜ？

**A19.** 「読めない file は空の入力」というのが本当に application の policy なら、そうしてもかまいません。`else` は短い `?` ではなく、別の問いへの答えです。文字数ではなく意味で選びます。

---

> **第七の戒律**
>
> *「ない」は `Option`、失敗は `Result`。失敗は `?` で上へ渡し、`Result` を一つも無視して放置するな。*
