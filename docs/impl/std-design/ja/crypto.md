このディレクトリには、ロードマップの本文を超えた std モジュールの Opus 実装可能な設計仕様が置かれている。
メインループ (Fable) が執筆したもので、各モジュールを実装する際の source of truth である。

# std.crypto — implementation design (M11)

> 🌐 [English](../crypto.md) · **日本語**

## Overview

crypto.random、sha256/sha512、blake3、hmac、hkdf、argon2id、aes_gcm、chacha20_poly1305、
constant_time_equal(draft §18.2)。**譲れない要件: 秘密情報に依存するすべての経路は constant-time で
なければならない**(open-questions std.crypto — 秘密情報に依存する分岐やメモリインデックスは禁止。
CMOV/ビット演算のみ)。ここは、Align の分岐なし機構がパフォーマンス上の選択ではなく **正しさの要件** に
なる唯一の領域である。

**戦略**: **実績あるエンジンを借用する**。AEAD(aes_gcm、chacha20_poly1305)、ハッシュ(sha256/512、
blake3)、KDF(hkdf、argon2id)、hmac は、constant-time であることが監査済みの C ライブラリ
(libsodium/BoringSSL クラス)を FFI でラップする — 自前で暗号を実装し constant-time を再証明するより
も、そのライブラリの constant-time 保証を継承するほうがはるかに安全である。`constant_time_equal` だけ
は唯一の自前実装である(分岐なしのバイト差分 OR 縮約であり、Align の `where`/mask の機構にとって自然
であり、監査するのに十分なほど単純である)。`crypto.random` → OS の CSPRNG(getrandom/getentropy —
rand.seed のソースと同じものだが、ここでは鍵材料用に crypto グレードとして公開される)。

## Signatures

```text
crypto.random(out: mut buffer)                                  // fill with CSPRNG bytes
crypto.sha256(data: bytes) -> array<u8>    // 32-byte digest (fixed-size)
crypto.sha512(data: bytes) -> array<u8>
crypto.blake3(data: bytes) -> array<u8>
crypto.hmac_sha256(key: bytes, data: bytes) -> array<u8>
crypto.hkdf_sha256(salt: bytes, ikm: bytes, info: bytes, len: i64) -> Result<buffer, Error>
crypto.argon2id(password: bytes, salt: bytes, params: argon2_params) -> Result<buffer, Error>
crypto.aes_gcm_seal(key: bytes, nonce: bytes, plaintext: bytes, aad: bytes) -> Result<buffer, Error>
crypto.aes_gcm_open(key: bytes, nonce: bytes, ciphertext: bytes, aad: bytes) -> Result<buffer, Error>
crypto.chacha20_poly1305_seal(...) / _open(...)    // same shape as aes_gcm
crypto.constant_time_equal(a: bytes, b: bytes) -> bool          // CT — self-hosted
```

## Type & ownership classification

byte→byte、あるいは byte→所有権付き buffer か固定長 `array<u8>`。新しい Move 型は不要(buffer/array
を再利用する)。固定長のダイジェストは `array<u8>`(32/64)として表す。

## Effect classification

FFI でラップされた演算は impure(extern 呼び出し)である。`constant_time_equal` は pure だが分岐なしで
あり続けなければならない。

## Error policy

AEAD の open における認証失敗 → `Error.Invalid`(タグの不一致なのか長さの不一致なのかを**決して**漏
らさない — 単一の不透明な失敗にする)。KDF/argon のパラメータエラー → `Error.Invalid`。エンジンのエラー
→ `Error.Code`。**重要**: `aes_gcm_open` は認証失敗時に部分的な平文を返しては**ならない** — 全か無か
であり、不透明なエラーとする。

## New machinery required

暗号ライブラリへの FFI リンク(libsodium を推奨する — 単一の依存で、constant-time が監査済みであり、
すべてのプリミティブをカバーする);`constant_time_equal` の自前実装(分岐なし、早期 return なし);
OS の CSPRNG 上に構築する `crypto.random`。Argon2 のパラメータ構造体。

## Slice breakdown

1. `constant_time_equal`(自前実装、分岐なし)+ `crypto.random`(OS の CSPRNG) — 外部依存なし、CT の
   規律を検証する。
2. ハッシュ(sha256/512、blake3)を FFI 経由で。
3. hmac + hkdf。
4. AEAD(aes_gcm、chacha20_poly1305) — 全か無かの認証。
5. argon2id(KDF、意図的にコストの高い処理)。

## Pitfalls

- **P1 (constant-time is CORRECTNESS)**: `constant_time_equal` には早期 return があってはならず、秘密
  情報に依存する分岐があってもならない — 全長にわたるバイト差分の OR 縮約の後、単一の 0 チェックを行う
  こと。早期 break のある `for` は、タイミングを通じて長さ/内容を漏らす。self-review では秘密情報に依
  存する制御フローがないことを検証しなければならない。これはこのモジュールを定義づける制約である。
  - **長さの扱い**: 入力の *長さ* は(意図された用途 — MAC タグやダイジェストの比較 — において)秘密で
    はなく **公開情報** として扱う(両側とも固定の、公開された長さである)。したがって長さが異なる場合
    は直ちに `false` を返してよい。constant-time の保証は **同じ長さ** の入力の *内容* に対するもので
    ある。これは libsodium の `sodium_memcmp` の契約(等しい長さであることが事実上の前提条件である)と
    一致する。呼び出し側が秘密の長さを持つ入力を渡して長さ自体が隠されることを期待しないよう、これを
    明示的に文書化すること。
- **P2 (AEAD all-or-nothing)**: 認証失敗時の `open()` は `Error.Invalid` を返し、平文のバイトは
  **ゼロ**でなければならない — 部分的な平文も、識別可能なエラーも決して返さない。未検証の平文を解放し
  てしまうのは、AEAD における典型的な誤用である。
- **P3 (nonce reuse)**: 同一の鍵で nonce を再利用することは(特に aes_gcm では)致命的であることを文書
  化する。v1 は nonce を自動生成しない(呼び出し側が供給する)が、文書には警告を書くこと。nonce 生成の
  補助関数は候補として記録する。nonce の長さ検証を必須にすることも検討する。
- **P4 (FFI memory safety, Gate 2)**: compress と同様に、FFI を越えるすべてのバッファに対して
  try_from/checked_mul/null ガードを適用する。
- **P5 (don't self-host the primitives)**: SHA/AES/argon を Align 内で再実装することには抵抗するこ
  と — constant-time と正しさを再証明するのは、監査すべき範囲が膨大になる。エンジンを借用すること。
  自前実装するのは(自明な)`constant_time_equal` のみである。
- **P6 (key material zeroization)**: 鍵を保持する buffer は Drop 時にゼロクリアされるべきである
  (zeroize-on-drop の buffer 亜種を用意するか、呼び出し側にその責務があることを文書化する)。v1 での
  検討事項として記録する — buffer の Drop は現状単に解放するだけであり、暗号の鍵に関しては理想として
  は先にゼロクリアすべきである。

## Test checklist

- sha256/512/blake3 を既知のテストベクタ(NIST/RFC)に照らして検証する
- hmac を RFC 4231 のベクタに照らして検証する
- hkdf を RFC 5869 に照らして検証する
- aes_gcm/chacha20 をそれぞれのテストベクタに照らして検証する
- タグを 1 ビット反転させた `aes_gcm_open` → `Error.Invalid` + ゼロの平文(P2)
- `constant_time_equal` の true/false + (検査/監査により)早期 return がないこと(P1)
- `crypto.random` が毎回異なるバイト列を生成する
- argon2id の既知の解答との照合
- import が必須であること
- (FFI のテストは libsodium の存在有無でゲートすること。)
