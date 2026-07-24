# 14. The Big Crunch

> 🌐 [English](../14-the-big-crunch.md) · **日本語**

**Q1.** 10ギガバイトのログファイルを渡されました。目標は、各 `user_id` が何回リクエストしたかを数えること — そしてついでに、10ギガバイト全体のうち何件が `status == 200` で返ったかも数えることです。

**A1.** 読み込み、パースし、フィルタをかけ、グループ化し、カウントする必要があります。

---

**Q2.** オブジェクト指向の直感はこう囁きます。「`file.read_to_string()` を使おう」と。

**A2.** するとマシンは悲鳴を上げます。たった今、あなたはヒープに10ギガバイトのメモリを確保し、OSのバッファからプログラムのメモリへとデータを丸写し（コピー）したからです。

---

**Q3.** では、他の言語でやるように1行ずつストリーム処理しますか？ `for line in file.lines() { ... }`

**A3.** 少しマシになりました。しかし、文字列を一つずつ `LogLine` オブジェクトにパースしていくやり方では、数百万個の小さなオブジェクトがばらまかれ、キャッシュを荒らし、パイプラインの流れを寸断してしまいます。

---

**Q4.** ならば、10ギガバイトのデータをどうやって読めと？

**A4.** 
```align
arena {
    view := fs.read_file_view("access.log")?
    ...
}
```
通常の空でない file なら、これはOSレベルの memory map（`mmap`）です。最初に2つ目の user-space buffer を作らず、触れた page が OS の page cache を通って届きます。view は arena の中に住みます（8章）— 寿命を最初に宣言します。そして `?` は7章の教えです。text file を開き validate する処理は失敗しえます。

---

**Q5.** しかしそれは単なるテキストの窓です。どうやって列（カラム）を取り出すのですか？

**A5.** 同じ arena の中に、直接「横倒し（SoA）」でデコードします：
```align
    logs: soa<Log> := json.decode(view)?
```

---

**Q6.** `json.decode` は、ログ内のすべてのURLに対してヒープ上に文字列（string）をアロケーションするのですか？

**A6.** いいえ。デコードされた文字列カラムは `str` の**ビュー**のカラムです — その一つ一つが、すでに手元にある `view`（メモリマップされたファイル）の一部を指し示しているだけです。コピーはゼロです。

---

**Q7.** これで `soa<Log>` である `logs` が手に入りました。`status == 200` だったリクエストは何件ですか？

**A7.** カラムはただのスライスなので、3章で学んだことがそのまま使えます：
```align
    ok := logs.status.where(fn s { s == 200 }).count()
```

---

**Q8.** この `where` は、小さな新しい配列を作ったのですか？

**A8.** いいえ。`where` はパイプラインのステージです — `count` と融合し、カラムを1回のパスで駆け抜けます。メモリは一切移動していません。

---

**Q9.** では、`user_id` ごとのリクエスト数は？

**A9.** 10章に答えがあります：
```align
    g := logs.group_by(.user_id).count()
```
`g.0` はユニークなユーザーのカラム、`g.1` はそれぞれのカウントで、行の位置が揃っています。

---

**Q10.** 全てを一息に繋げてみてください。

**A10.**
```align
import std.fs
import core.json

Log { user_id: i64, status: i64 }

fn main() -> Result<(), Error> {
    arena {
        view := fs.read_file_view("access.log")?
        logs: soa<Log> := json.decode(view)?

        ok := logs.status.where(fn s { s == 200 }).count()
        g := logs.group_by(.user_id).count()

        print(ok)
        print(g.0.len())
    }
    return Ok(())
}
```

---

**Q11.** このプログラムは、自分ではどれだけのメモリを浪費しましたか？

**A11.** row-object design より churn はずっと少ないですが、memory はゼロではありません。file mapping は address space を使い、触れた page は resident になります。decoded column は row 数に比例し、grouping の state と結果 column は distinct user 数に比例します。消えたのは copied input buffer、何百万もの小さな row object、filter 済みの中間 array です。

---

**Q12.** `arena` ブロックが終わるとどうなりますか？

**A12.** mapped view は unmap され、batch storage はその境界で解放されます。忘れられた row を tracing GC が探し回ることはありません。lifetime は最初から指させる block でした。

---

**Q13.** 何百万もの row object も、隠れた中間 array も作らずに、10ギガバイトを処理しました。

**A13.** これが The Big Crunch です。data は平らなまま、必要な batch と grouping memory は目に見え、すべての pass に理由がありました。

---

**Q14.** では *zero-copy* は、memory も仕事もゼロという意味？

**A14.** どちらでもありません。特定の境界で byte を複製しなかったという意味です。mapping は owned input copy を避け、decoded `str` field は mapped byte を view します。parse は仕事をし、数値 column は storage を必要とし、OS は page を memory hierarchy の中で動かします。どの copy が消えたのかを名指してください。

---

**Q15.** global な `status == 200` の count だけが必要で、grouping も batch の再利用もないなら？

**A15.** mapped text から typed row を stream します。

```align
rows: json.scanner<Log> := json.scan(view)
ok := rows.status.where(fn s { s == 200 }).count()?
```

各 row は reducer へ流れながら decode され、`soa<Log>` batch は materialize されません。途中で malformed input が見つかりうるので reducer は `Result` を返します。

---

**Q16.** 元の user ごとの `group_by` にも scanner を使わないのはなぜ？

**A16.** scanner は fused された non-materializing reduction のためのものです。`group_by`、`sort`、`to_array` は scanner の終端ではありません。再利用する column や grouped materialization が必要なら、batch を作り、目に見える形で支払います。一つの bounded answer が直接流れ出るなら scan します。

---

**Q17.** input の形を選んでください。20GB の NDJSON、最後に count 一つだけ、sort も grouping も再利用もなし。

**A17.** view として map し、`json.scan` から fused reducer へ流します。要求される答えは bounded ですが、materialize した batch はそうではありません。

---

**Q18.** 同じ file を、20個の report が5つの hot field について繰り返し scan します。

**A18.** 宣言した field を arena 内の `soa<T>` へ decode し、column を再利用します。一度の batch allocation と parse が、多数の field-wise pass で返済されます。

---

**Q19.** 同じ data でも、record を一つずつ受け取り、すぐ次へ送るなら？

**A19.** scanner を選びます。SoA は繰り返す column work に勝つのであり、大きな file すべてにつける勲章ではありません。

---

**Q20.** mapped file から decode した `str` field を、arena のあとで返したい。何が足りませんか？

**A20.** ownership です。その field は mapping への view にすぎません。境界で必要な survivor だけを clone するか、arena 内で使い終えるよう caller を設計します。

---

**Q21.** scanner は900万行を正しく処理したあと malformed JSON を見つけました。partial count を何もなかったように返してよい？

**A21.** いいえ。reducer は `Result` を返し、`?` が失敗を上へ渡します。streaming が節約するのは materialization であり、correctness ではありません。partial answer が必要なら、明示的な interface と error policy が必要です。

---

> **第十四の戒律**
>
> *データをオブジェクトの形に丸めるな。データは平らに敷き詰め、大地（ディスク）から直接マッピングし、その上をパイプラインに流させよ。*
