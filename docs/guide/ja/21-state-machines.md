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

直和型で状態を列挙し、それぞれの状態が持つデータを指定します。

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

この例では、純粋な遷移関数が現在の状態とイベントを受け取り、次の状態を返します。I/O は遷移関数の外で扱います。

まず、起こりうるイベントを定義します。

```align
Event {
    Start,
    SocketOpened(i64),   // socket
    AuthSuccess(i64),    // user_id
    Failure(i64),        // error code
}
```

遷移関数では、外側の `match` で現在の状態を選び、内側の `match` でイベントを選びます。

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

この有限状態機械は、次の3つの観点で検査・テストできます。

1. **ケースの網羅:** `match` はすべてのバリアントを扱う必要があり、抜けがあればコンパイラが拒否します。ワイルドカードは、残りのケースをまとめて扱う指定です。
2. **テスト:** ソケットを開かずに `next(state, event)` を呼び、結果を検査できます。
3. **データ指向との親和性:** 何千もの `ConnectionState` の配列をメモリ上に並べ、パイプラインで一括更新できます: `states.map(fn s { next(s, ev) }).to_array()`。
