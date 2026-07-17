# 22. Building a system: ECS

> 🌐 [English](../22-building-a-system.md) · **日本語**

あなたはオブジェクト指向を忘れました（19章）。プールを使って長生きするメモリを管理する方法を知りました（20章）。enum を使って状態をモデル化する方法も知りました（21章）。

では、これらをすべて組み合わせて、どうやってアプリケーション全体を構築するのでしょうか？ データ指向設計の代表的なパターンである、ミニチュア版の **Entity-Component-System (ECS)** アーキテクチャを作ってみましょう。

## アーキテクチャ

OOP では、ゲームのエンティティはフィールドとメソッドを持つ「クラス」です。ECS では以下のようになります。
- **Entity（エンティティ）** は単なる ID（例えば `i64`）です。データは一切持ちません。
- **Component（コンポーネント）** は純粋なデータです。SoA（Struct of Arrays）の列（カラム）として保存されます。
- **System（システム）** は、コンポーネントの列をパイプラインで反復処理する関数です。

位置（Position）、速度（Velocity）、描画用のスプライト（Renderable）を持つ 2D の世界をモデル化してみましょう。

## コンポーネント（The Components）

`GameObject` というクラスを作る代わりに、コンポーネントの平坦な配列を定義します。

```align
Position { x: f32, y: f32 }
Velocity { dx: f32, dy: f32 }

World {
    // エンティティは暗黙的です。これらの配列のインデックスが Entity ID になります。
    // Option を使うことで、疎なコンポーネント（すべてのエンティティがすべてのコンポーネントを持つわけではない状態）を表現できます。
    positions: array<Option<Position>>,
    velocities: array<Option<Velocity>>,
    sprites: array<Option<string>>,
}
```

## システム（The System）

System はコンポーネントに対して操作を行う関数です。どのクラスにも属していません。速度（Velocity）に基づいて位置（Position）を更新する Physics System（物理システム）を書いてみましょう。

Align では、これをコンポーネント配列に対するパイプラインとして記述します。

```align
fn physics_system(world: mut World, dt: f32) {
    // Position と Velocity の「両方」を持つエンティティだけを処理したい。
    // `zip` は2つの配列を束ねます。
    world.positions.zip(world.velocities).map_in_place(
        fn (opt_pos, opt_vel) {
            match (opt_pos, opt_vel) {
                (Some(mut p), Some(v)) => {
                    p.x = p.x + v.dx * dt
                    p.y = p.y + v.dy * dt
                    Some(p)
                },
                _ => opt_pos // どちらか一方が欠けていれば何もしない
            }
        }
    )
}
```

## ゲームループ（The Game Loop）

これらすべてを `loop`（11章）で包み込みます。

```align
fn main() -> i32 {
    mut world := spawn_initial_world()
    
    loop {
        dt := time.delta()
        
        // 1. 入力の処理 (System)
        input_system(world)
        
        // 2. 物理演算の更新 (System)
        physics_system(world, dt)
        
        // 3. 描画 (System)
        render_system(world)
        
        if window.should_close() { break 0 }
    }
}
```

## なぜこの構造がスケールするのか

1. **疎結合:** `physics_system` はスプライトを気にしません。`render_system` は速度を気にしません。明日「体力（Health）」コンポーネントを追加しても、物理演算のコードには一切触れなくて済みます。
2. **予測可能性:** すべてが上から下へと流れます。他のメソッドを暗黙のうちに呼び出すような、隠された `Update()` メソッドは存在しません。
3. **パフォーマンス:** コンポーネントは連続した配列であるため、CPU のプリフェッチャが完璧にストリーミングしてくれます。`alignc emit-llvm` を実行すれば、`physics_system` が隙間なく詰め込まれた SIMD のベクターループにコンパイルされていることがわかるはずです。

データが入り、データが変換され、データが出ていく。それが Align です。
