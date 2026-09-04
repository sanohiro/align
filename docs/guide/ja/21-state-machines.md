# 21 — 状態機械

> 🌐 [English](../21-state-machines.md) · **日本語**

第19章ではデータと処理を分けました。TCP 接続、ゲームのターン、UI コンポーネントを表すには、さらに、現在の状態と、それを変えるイベントを定義する必要があります。

Align では、**直和型（Sum Type）** と **ステートマシン（状態遷移機械）** を使って状態をモデル化します。

## 複数のフラグで状態を表すときの問題

オブジェクト指向のコードでは、状態を複数のフィールドの組み合わせで表現することがよくあります。

```typescript
// アンチパターン
class Connection {
    isConnected: boolean;
    isAuthenticating: boolean;
    socketId: number | null;
    errorMessage: string | null;
}
```

この構造体では、矛盾した組み合わせも表せてしまいます。`isConnected` が true なのに `socketId` が null の場合や、`isAuthenticating` と `errorMessage` の両方が設定されている場合を、どう扱えばよいでしょうか。フィールドを更新するコードで整合性を保ち、その組み合わせをテストする必要があります。

## 不正な組み合わせを型で防ぐ

Align では直和型を使って、「どの状態が存在しうるか」を正確に定義し、その特定の状態に関連するペイロード（データ）だけをそこに持たせます。

```align
ConnectionState {
    Disconnected,
    Connecting,
    Authenticating(i64),   // socket
    Connected(i64, i64),   // socket, user_id
    Failed(i64),           // error code
}
```

この定義では、`Disconnected` に `user_id` フィールドはなく、`Authenticating` を作るにはソケットが必要です。先ほどの不整合を型で防げます。ペイロードは位置で指定します。複数の値に1つの名前を付けたい場合は、`Connected(Session)` のように構造体へまとめます。借用したテキストや対応済みの所有配列もペイロードにでき、第 [05](05-memory.md) 章のリージョンと Move の規則が適用されます。

ここでソケットを表す `i64` は、モデル上の識別子です。OS のソケットを所有するハンドルではありません。すべてのペイロードが Copy なので、この例では状態をコピーできます。実際のリソースを所有する状態では、遷移時に所有権を移すか借りるかを決め、解放も扱う必要があります。識別子をコピーしても、リソース自体が複製されるわけではありません。

## 純粋関数としての「遷移」

OOP では、状態の遷移は内部のフィールドを書き換えるメソッド（`conn.connect()` など）を呼び出すことで起こります。Align では、遷移とは「現在の状態」と「イベント」を受け取り、「次の状態」を返す**純粋関数**です。

まず、起こりうるイベントを定義します。

```align
Event {
    Start,
    SocketOpened(i64),   // socket
    AuthSuccess(i64),    // user_id
    Failure(i64),        // error code
}
```

次に、遷移関数を書きます。`match` は一度に1つの値を検査するので、この関数は「表」として読めます。外側の `match` が行（状態）を選び、内側の `match` が列（イベント）を選ぶのです。

```align
fn next(state: ConnectionState, event: Event) -> ConnectionState {
    return match state {
        Disconnected => match event {
            Start => ConnectionState.Connecting,
            Failure(code) => ConnectionState.Failed(code),
            _ => state,
        },
        Connecting => match event {
            SocketOpened(s) => ConnectionState.Authenticating(s),
            Failure(code) => ConnectionState.Failed(code),
            _ => state,
        },
        Authenticating(s) => match event {
            AuthSuccess(user_id) => ConnectionState.Connected(s, user_id),
            Failure(code) => ConnectionState.Failed(code),
            _ => state,
        },
        // Connected と Failed は以降のイベントを無視する（エラーを返す設計も可）
        _ => state,
    }
}
```

## 遷移の検査とテスト

これが**有限状態機械**です。データと遷移関数に分けると、次のことができます。

1. **ケースの網羅:** `match` はすべてのバリアントを扱う必要があり、抜けがあればコンパイラが拒否します。ワイルドカードは、残りのケースをまとめて扱う指定です。
2. **テストが容易:** このロジックをテストするのに、実際のソケットを立ち上げたりモックオブジェクトを作ったりする必要はありません。ただ `next(state, event)` を呼び出し、結果をアサートするだけです。
3. **データ指向との親和性:** 何千もの `ConnectionState` の配列をメモリ上に並べ、パイプラインで一括更新できます: `states.map(fn s { next(s, ev) }).to_array()`。

遷移のロジックだけを独立してテストでき、同じパイプラインで多くの状態を処理できます。
