このディレクトリには、ロードマップの本文ではカバーしきれない `std` モジュールについて、Opus がそのまま実装に着手できる粒度の設計仕様を収めている。執筆はメインループ（Fable）が担当しており、各モジュールの実装においてこれが信頼できる情報源（source of truth）となる。

# std.crypto — implementation design (M11)

> 🌐 [English](../crypto.md) · **日本語**

> **ステータス:** M11 の symmetric/hash/KDF surface は完了済みです。post-pkg.db の asymmetric
> signature suite は以下で設計済み、実装は pending です。文書に記載した BLAKE3 の例外も
> 引き続き保留です。

## Overview

`crypto.random`、`sha256` / `sha512`、`blake3`、`hmac`、`hkdf`、`argon2id`、`aes_gcm`、`chacha20_poly1305`、`constant_time_equal`（draft §18.2）。**譲れない要件: admission 済み secret material を扱う cryptographic primitive path は、secret content に関してすべて constant-time でなければならない**（open-questions std.crypto — wrapper に secret-dependent branch/memory index を置かず、CMOV/bitwise のみ）。public length、algorithm identifier、format、allocation outcome、setup-validation result はこの保証外である。特に key import/parser/provider-validation は trusted setup operation であり、attacker-queryable timing oracle ではない。この領域は、Align の分岐なし機構がパフォーマンスのためだけでなく、**正しさの要件** そのものになる唯一の領域である。

**戦略**: **検証済みの計算エンジンを外部から借りる**。AEAD（aes_gcm、chacha20_poly1305）、ハッシュ（sha256/512）、KDF（hkdf、argon2id）、hmac は、constant-time 性が監査済みの C ライブラリを FFI でラップする — 暗号処理を自前で実装して constant-time を証明し直すよりも、実績あるライブラリの constant-time 保証をそのまま継承するほうがはるかに安全だからである。自前実装するのは `constant_time_equal` ただ一つである（分岐なしのバイト単位の差分 OR 縮約であり、Align の `where` / mask 機構に素直に乗るうえ、容易に監査できるほど単純であるため）。`crypto.random` は OS の CSPRNG（getrandom / getentropy — `rand.seed` のソースと同じものだが、ここでは鍵材料向けの crypto グレードとして公開する）。

**エンジン: OpenSSL libcrypto(EVP)、2026-07-07 決定**（`open-questions.md` Settled に記録。本ドキュメント当初の「libsodium 推奨」を置き換える）。独立したセキュリティレビューと依存関係レビューが収束した根拠は次のとおり。libcrypto は必要なプリミティブを *すべて* ネイティブにカバーする — HKDF と Argon2id も `EVP_KDF` 経由で提供される — 単一の信頼境界（trust surface）に収まり、エンジンの混在も、自前 HKDF の統合による隙間（継ぎ目）も生じない。これらの機能の大部分は **OpenSSL ≥ 3.0** で動作する。Argon2id には **OpenSSL ≥ 3.2** で追加された `ARGON2ID` provider が必要であり、存在しない場合は `Error.Code` を返す。ドライバは使用した Crypto / TLS capability が必要とする場合に限り `-lcrypto` をリンクとして追加する。このモジュールでは `crypto.random` と `constant_time_equal` はリンクを要求しない。さらに AES-GCM はサポート対象環境において constant-time である（AES-NI / PCLMULQDQ のハードウェア経路、constant-time な vpaes フォールバック — x86-64 / aarch64 で T-table AES を使うことは決してない）。そのうえ、libsodium の `crypto_aead_aes256gcm_*` と異なり、ハードウェア要件によって API が制限（ゲート）されることがない。libsodium は抽象的には依然として優れたエンジンだが、システム全体の統合面（継ぎ目）で劣る（1.0.18 クラスのリリースには HKDF が無く、AES-GCM がハードウェアゲートされる）。**blake3 は決定記録付きで見送る**: これを提供するシステムの基本エンジンが無く（Debian に `libblake3-dev` は無い。OpenSSL にも BLAKE3 は無い）、自前実装は原則 P5 に反する。また、BLAKE2b を `blake3` という別名として扱うことは禁止されている（誤解を招く API となるため）。システムライブラリとして普及すれば再び候補になるか、あるいは `pkg` レイヤーでの提供対象となる。

## Signatures

```text
crypto.random(out: mut buffer)                                  // fill with CSPRNG bytes
crypto.sha256(data: bytes) -> array<u8>    // 32-byte digest (fixed-size)
crypto.sha512(data: bytes) -> array<u8>
crypto.blake3(data: bytes) -> array<u8>    // DEFERRED v1 (no system engine provides BLAKE3 — see Overview)
crypto.hmac_sha256(key: bytes, data: bytes) -> array<u8>
crypto.hkdf_sha256(salt: bytes, ikm: bytes, info: bytes, len: i64) -> Result<buffer, Error>
crypto.argon2id(password: bytes, salt: bytes, params: argon2_params) -> Result<buffer, Error>
crypto.aes_gcm_seal(key: bytes, nonce: bytes, plaintext: bytes, aad: bytes) -> Result<buffer, Error>
crypto.aes_gcm_open(key: bytes, nonce: bytes, ciphertext: bytes, aad: bytes) -> Result<buffer, Error>
crypto.chacha20_poly1305_seal(...) / _open(...)    // same shape as aes_gcm
crypto.constant_time_equal(a: bytes, b: bytes) -> bool          // CT — self-hosted
```

**実装された表層の詳細（実装記録、2026-07-07、PR #384–#388）:**
`argon2_params { m_cost: i64, t_cost: i64, parallelism: i64, len: i64 }` は **組み込み（builtin）構造体** である。non-entry module が local `argon2_params` を宣言しない限り bare alias を利用できる。collision がある module で builtin を明示する正確な spelling は `crypto.argon2_params` であり、`import std.crypto` が必要で、その import の use として数える。通常の構造体リテラルによる構築と型検査を使う。m_cost は KiB、t_cost は反復回数、parallelism はレーン数、len は出力バイト数。エンジンに渡す前に検証する（`parallelism 1..=2^24-1`、`t_cost 1..=u32max`、`m_cost 8*parallelism..=4 GiB-in-KiB`(= 4,194,304 KiB)、`len 4..=1 GiB` → 違反時は `Error.Invalid`。エンジンの `threads` は 1 に固定し、`OSSL_set_max_threads` の利用は見送り）。
AEAD: どちらの暗号も 32 バイトの鍵と 12 バイトの nonce を取る（公開パラメータとして検証し、違反時は `Error.Invalid`）。seal の出力は **結合された** `ciphertext || 16 バイトの tag` を単一のバッファに収めたものである。open は `len >= 16` を要求する。hkdf の `len` は `1..=8160` に制限される（RFC 5869 の L ≤ 255·HashLen）。ダイジェスト / タグの戻り値は、アルゴリズムで固定された長さを持つ動的な `array<u8>` である（固定長の `array<u8; N>` は現状のランタイム戻り値 ABI では表現できないため）。FFI 演算はすべて impure。
`constant_time_equal` は pure であり、その分岐なしの性質は **コンパイル済みの機械語に照らして検証済み** である（release + debug の逆アセンブル結果において、内容に依存する分岐も、memcmp のイディオムも存在しない）。

## Type & ownership classification

byte → byte、または byte →（所有権を持つ buffer か固定長 `array<u8>`）。新しい Move 型は必要ない（buffer / array を再利用する）。固定長のダイジェストは `array<u8>`（32 または 64 バイト）で表す。

## Effect classification

FFI でラップした演算は impure（extern 呼び出し）となる。`constant_time_equal` は pure だが、分岐なしの性質を保ち続けなければならない。

## Error policy

AEAD の open における認証失敗は `Error.Invalid`（タグの不一致か長さの不一致かを **決して** 漏らさない — 単一の不透明な失敗として扱う）。KDF / argon のパラメータエラーは `Error.Invalid`。エンジンのエラーは `Error.Code`。**重要**: `aes_gcm_open` は認証失敗時に部分的な平文を返しては **ならない** — 全か無か（all-or-nothing）の、不透明なエラーとする。

## New machinery required

**OpenSSL libcrypto** への capability-gated FFI リンク（EVP ベースの操作を使う場合だけ `-lcrypto` をリンクする。一般には OpenSSL ≥ 3.0、Argon2id には ≥ 3.2 — Overview 参照）。EVP の上に構築するおよそ 6 個のランタイムラッパー: 共有の一括ダイジェスト（`EVP_Q_digest`、`EVP_sha256/512` でパラメータを差し替える）、HMAC（`EVP_MAC` の "HMAC"）、HKDF（`EVP_KDF_fetch("HKDF")` + `OSSL_PARAM` の salt/key/info）、Argon2id（`EVP_KDF_fetch("ARGON2ID")` + `OSSL_KDF_PARAM_ARGON2_*`）、そして P2 に従う全か無かの共有 AEAD seal/open ペア（`EVP_CIPHER`、AES-256-GCM / ChaCha20-Poly1305 でパラメータを差し替える）。加えて `constant_time_equal` の自前実装（分岐なし、早期 return なし）、OS の CSPRNG の上に構築する `crypto.random`。そして Argon2 のパラメータ構造体。

## Slice breakdown

1. `constant_time_equal`（自前実装、分岐なし）+ `crypto.random`（OS の CSPRNG） — 外部依存なしで、CT の規律を検証する。
2. ハッシュ（sha256/512）を EVP 経由で。blake3 は見送り（Overview 参照）。
3. hmac + hkdf。
4. AEAD（aes_gcm、chacha20_poly1305） — 全か無かの認証。
5. argon2id（KDF、設計上コストが高い）。

## Pitfalls

- **P1 (constant-time is CORRECTNESS)**: `constant_time_equal` には早期 return も、秘密情報に依存する分岐もあってはならない — 全長にわたってバイト差分を OR 縮約し、最後に一度だけ 0 チェックする。早期 break のある `for` ループは、処理タイミングを通じて長さや内容を漏洩させる。self-review では、秘密情報に依存する制御フローが無いことを検証しなければならない。これはこのモジュールを定義づける根本の制約である。
  - **長さの扱い**: 入力の *長さ* は、意図された用途（MAC タグやダイジェストの比較 — 両側とも固定で、公開済みの長さ）においては秘密ではなく **公開情報** として扱う。したがって長さが違えば即座に `false` を返してよい。constant-time の保証は、**同じ長さ**の入力の *内容* に対して成り立つ。これは libsodium の `sodium_memcmp` の契約（長さが等しいことが事実上の前提条件）と一致する。呼び出し側が、長さ自体を隠蔽することを期待して秘密の長さの入力を渡すことのないよう、この点は明示的に文書化すること。
- **P2 (AEAD all-or-nothing)**: 認証失敗時の `open()` は `Error.Invalid` を返し、平文のバイトは **ゼロ（空）** でなければならない — 部分的な平文も、区別のつくエラーも決して返さない。未検証の平文を外に出してしまうのは、AEAD の典型的な誤用である。**EVP 固有の必須の対応**: `EVP_DecryptUpdate` はタグが `EVP_DecryptFinal_ex` で検査される **前** に平文を放出するため、ラッパーは暗号文全体を内部の所有バッファへ復号し（決して外部へストリームしない）、`EVP_CIPHER_CTX_ctrl(EVP_CTRL_AEAD_SET_TAG)` で期待するタグを設定し、`EVP_DecryptFinal_ex` を呼び、`Final == 1` のときに **限って** バッファを呼び出し側へ渡さなければならない。失敗時はバッファを `OPENSSL_cleanse` してから解放し、単一の不透明な `Error.Invalid` を返す（タグ不一致か、長さ/パラメータのエラーかは区別がつかないようにする）。nonce / タグの長さは *公開* 値として事前に検証する（P1）。タグは固定で 16 バイトである。
- **P3 (nonce reuse)**: 同じ鍵で nonce を再利用するのは（特に aes_gcm では）致命的であることを文書化する。v1 は nonce を自動生成しない（呼び出し側が渡す）が、文書には強い警告を書くこと。nonce を生成する補助関数は将来の候補として記録しておく。nonce の長さ検証を必須にすることも検討する。
- **P4 (FFI memory safety, Gate 2)**: compress と同様に、FFI を越えるすべてのバッファに対して try_from / checked_mul / null ガードを適用する。
- **P5 (don't self-host the primitives)**: SHA / AES / argon を Align 内で再実装したくなる誘惑を抑えること — constant-time と正しさを証明し直すのは、監査すべき面積が膨大になる。エンジンを借りる。自前実装するのは（自明な）`constant_time_equal` だけである。
- **P6 (key material zeroization)**: 鍵を保持する buffer は Drop 時にゼロクリアされるのが望ましい（zeroize-on-drop の buffer 亜種を用意するか、呼び出し側の責務として文書化する）。v1 の検討事項として記録する — buffer の Drop は現状ただ解放するだけだが、暗号の鍵に関しては理想としては先にゼロクリアすべきである。

## Test checklist

- sha256/512 を既知のテストベクタ（NIST/RFC）に照らして検証する。blake3 は見送り。
- hmac を RFC 4231 のベクタに照らして検証する。
- hkdf を RFC 5869 に照らして検証する。
- aes_gcm / chacha20 をそれぞれのテストベクタに照らして検証する。
- タグを 1 ビット反転させた `aes_gcm_open` → `Error.Invalid` + 空の平文（P2）。
- `constant_time_equal` の true/false、および（検査/監査により）早期 return が無いこと（P1）。
- `crypto.random` が毎回異なるバイト列で埋めること。
- argon2id の既知解答との照合。
- import が必須であること。
- capability-linking テストで、EVP ベースの暗号を使う場合は libcrypto がリンクされ、Crypto / TLS capability のないプログラムにはリンクされないことを確認する。

## Asymmetric signature suite（post-pkg.db）

この節は RS256、ES256、Ed25519 の sign/verify に関する authoritative public-contract ledger
である。shipped M11 engine を拡張し、第二の crypto provider は追加しない。function と key
type の両方を algorithm-specific にするため、string/enum algorithm selector や generic key
handle によって RSA key が EC/Ed25519 operation に到達することはない。

normative algorithm と wire signature を次で固定する。

| Name | Exact operation | Public signature bytes |
|---|---|---|
| RS256 | SHA-256 を使う RSASSA-PKCS1-v1_5。RSA-PSS ではない | admitted 2048..=8192-bit key の modulus width と exact に等しい 256..=1024 bytes |
| ES256 | named curve P-256 上の ECDSA + SHA-256 | exact 64 bytes。leading zero を含む 32-byte big-endian `r` の直後に 32-byte big-endian `s`。DER は internal のみ |
| Ed25519 | caller digest、context、prehash のない complete message 上の pure Ed25519 | exact 64 bytes |

RS256/ES256 は RFC 7518 sections 3.3/3.4、Ed25519 は RFC 8032、PKCS#8/SPKI identifier は
RFC 8410 に従う。実装は exact digest/padding/group parameter を OpenSSL EVP へ渡し、provider
default に選択させない。

### Public surface ledger

6つの compiler-provided nominal type は互いに異なる **Move** owner である。

```text
rs256_private_key       rs256_public_key
es256_private_key       es256_public_key
ed25519_private_key     ed25519_public_key
```

各 bare spelling は、同一 module の declaration が bare lookup で勝つ場合を除き、import 不要の
builtin fallback である。explicit builtin spelling は `crypto.rs256_private_key` のように
`crypto.` を前置し、この qualified type spelling だけが `import std.crypto` を require し use
として count する。下記 value operation も同じ import を要求する。entry module は衝突する bare
name を宣言できない。public interface producer/import validator は bare/qualified の両方を認識し、
qualified spelling に限って reconstructed source に `std.crypto` import を追加する。

以下が exact constructor/operation であり、default/optional argument はない。`bytes` は既存の
borrowed byte-view meaning を持つ。明示した `borrow` mode は stable bound key place を要求し、
key を consume しない。

```text
crypto.rs256_private_key_from_pem(pem: str) -> Result<rs256_private_key, Error>
crypto.es256_private_key_from_pem(pem: str) -> Result<es256_private_key, Error>
crypto.ed25519_private_key_from_pem(pem: str) -> Result<ed25519_private_key, Error>

crypto.rs256_public_key_from_pem(pem: str) -> Result<rs256_public_key, Error>
crypto.es256_public_key_from_pem(pem: str) -> Result<es256_public_key, Error>
crypto.ed25519_public_key_from_pem(pem: str) -> Result<ed25519_public_key, Error>

crypto.rs256_public_key_from_jwk(n: bytes, e: bytes) -> Result<rs256_public_key, Error>
crypto.es256_public_key_from_jwk(x: bytes, y: bytes) -> Result<es256_public_key, Error>
crypto.ed25519_public_key_from_jwk(x: bytes) -> Result<ed25519_public_key, Error>

crypto.rs256_sign(borrow key: rs256_private_key, message: bytes) -> Result<buffer, Error>
crypto.es256_sign(borrow key: es256_private_key, message: bytes) -> Result<buffer, Error>
crypto.ed25519_sign(borrow key: ed25519_private_key, message: bytes) -> Result<buffer, Error>

crypto.rs256_verify(borrow key: rs256_public_key, message: bytes, signature: bytes) -> Result<bool, Error>
crypto.es256_verify(borrow key: es256_public_key, message: bytes, signature: bytes) -> Result<bool, Error>
crypto.ed25519_verify(borrow key: ed25519_public_key, message: bytes, signature: bytes) -> Result<bool, Error>
```

`_from_jwk` は JSON/encoded text ではなく、base64url decode 済み JWK field を受け取る。caller
は construction 前に唯一の既存 path `encoding.base64url_decode` を使う。private JWK、key
generation/export、certificate parser、encrypted PEM、password callback、RSA-PSS、
Ed25519ctx/ph、generic `sign`/`verify`、implicit algorithm selection は追加しない。

### Key input and validation contract

PEM constructor は bounded な RFC 7468 textual block を1つ受け取る。input は `str` により
UTF-8 であり、各 constructor は最初に `1..=65,536` bytes を要求し embedded NUL を reject
する。block は byte zero から exact `-----BEGIN PRIVATE KEY-----` または
`-----BEGIN PUBLIC KEY-----` line で始まる。base64 body は standard alphabet、canonical final
padding、LF/CRLF line ending を使う。non-final body line は exact 64 base64 characters、final
body line は 4..=64 characters かつ4の倍数である。対応する exact END line が直後に続く。
final LF/CRLF は高々1つ。leading text、horizontal whitespace、second block、その他の trailing
byte は reject する。decoded payload は trailing octet のない complete canonical DER object
1つでなければならない。`PRIVATE KEY` は version INTEGER が exact zero の unencrypted PKCS#8
v1 `PrivateKeyInfo`、`PUBLIC KEY` は `SubjectPublicKeyInfo` である。`PrivateKeyInfo` の standard
optional `[0]` attributes set は accept するが、algorithm/provider を select できず imported key
にも影響しない。optional public-key field を含む RFC 5958 `OneAsymmetricKey` version one は
reject する。OpenSSL 3.0 は admitted path でその form を decode できない。したがって
`ENCRYPTED PRIVATE KEY`、traditional `RSA PRIVATE KEY` / `EC PRIVATE KEY`（その DER payload
を `PRIVATE KEY` と relabel したものを含む）、certificate、OpenSSH key は
password/terminal/file/network/environment lookup より前に reject する。

private DER の decoder path は1つだけである。bounded non-allocating DER envelope walk がまず
top-level SEQUENCE の first child に canonical な3 octets `INTEGER, length 1, value 0` を要求し、
`AlgorithmIdentifier` に exact constructor algorithm、すなわち NULL parameter の
`rsaEncryption`、named-curve `prime256v1` の `id-ecPublicKey`、または absent parameter の
id-Ed25519 を要求する。その後 `d2i_PKCS8_PRIV_KEY_INFO`、`PKCS8_pkey_get0` agreement、
complete-cursor check、
`i2d_PKCS8_PRIV_KEY_INFO` canonical re-encoding との byte-for-byte comparison、owned library
context と `provider=default` を使う `EVP_PKCS82PKEY_ex` の順に進む。
`d2i_AutoPrivateKey_ex` は呼ばないため、PKCS#1 RSA/SEC1 EC object は auto-detected legacy
format として入れない。同じ non-allocating envelope check が OpenSSL より前に public SPKI
algorithm identifier も固定する。public DER はその後同 context の `d2i_PUBKEY_ex` を使い、complete cursor
consumption と `i2d_PUBKEY` canonical re-encoding との byte-for-byte comparison を要求する。
private re-encoding scratch はすべて secret storage で、decoded private DER と同じ
cleanse-before-free rule に従う。

decode 後、constructor は advertised key class/algorithm との exact 一致を要求する。RSA key
は 2048..=8192-bit の odd modulus と、unsigned 64-bit に収まる odd public exponent `>= 3`
を持つ。private key は provider の complete private/pairwise check、public key は public check
を通る。ES256 は exact P-256 named group と full EC private/public check を要求する。Ed25519
は parameter absent の id-Ed25519 を要求する。異なる class、curve、size、algorithm は
`Error.Invalid` であり、変換しない。

Ed25519 admission は `EVP_PKEY_public_check` を point validation として扱わない。wrapper helper
1つが、全 SPKI/JWK の exact 32-byte compressed public value と全 PKCS#8 seed-derived key から
`EVP_PKEY_get_raw_public_key` で得た value を検証する。low 255 bits を little-endian `y`、high bit
を `x` sign と解釈し、`y < p = 2^255 - 19` を要求して、fixed
`d = -121665 / 121666 mod p` により RFC 8032 section 5.1.3 recovery を実行する。
`q = (y^2 - 1) / (d*y^2 + 1) mod p` を計算し、zero denominator を reject し、fixed
`(p + 3) / 8` exponent と RFC `sqrt(-1)` correction で square root を得る。`BN_mod_sqrt` は
呼ばない。non-square result と forbidden `x = 0` + sign bit one は reject する。recovered point は
twisted-Edwards equation を満たし、input と byte-for-byte に serialize されなければならない。
complete extended-coordinate Edwards doubling を3回行った結果は projective identity
`X = 0, Y = Z` であってはならない。これにより exceptional affine inversion なしで small-order
subgroup 全体を reject する。それより強い prime-subgroup-membership promise は持たない。この
public-data variable-time check は fallible な `BIGNUM`/`BN_CTX` temporary を使い、handle publish
より前に行う。invalid encoding/point は `Error.Invalid`、BN API allocation/arithmetic failure は
`Error.Code(0)`。provider private/public check は追加の algorithm check であり、これら point
invariant の owner ではない。

JWK component は borrowed binary integer/point である。

| Constructor | Exact decoded fields and validation |
|---|---|
| `rs256_public_key_from_jwk` | `n`/`e` は leading zero のない minimal unsigned big-endian。`n` は odd かつ actual bit width 2048..=8192。`e` は 1..=8 bytes、`u64` に収まり、odd、`>= 3`。constructed RSA key は provider public check を通る。 |
| `es256_public_key_from_jwk` | `x`/`y` は各 exact 32-byte big-endian P-256 coordinate。uncompressed point `0x04 || x || y` は canonical、on-curve、non-infinite で provider public check を通る。 |
| `ed25519_public_key_from_jwk` | `x` は exact 32-byte RFC 8037 Ed25519 public value で、上記 wrapper-owned canonical/on-curve/non-small-order validation と provider algorithm check を通る。 |

cheap structural check はすべて provider key construction より前に行う。decode、algorithm
match、bounds、complete key validation が成功するまで handle を publish しない。

### Ownership, allocation, and effects

各 successful constructor は runtime shell 1つを指す fresh one-word opaque owner を返す。shell は
repeated key kind、private `OSSL_LIB_CTX` 1つ、explicit load した built-in OpenSSL `default`
`OSSL_PROVIDER`、provider-managed `EVP_PKEY` 1つを所有する。Move transfer は shell pointer を
copy して complete source を null にする。replacement
と active aggregate Drop は null-safe な `align_rt_crypto_key_free` を exact once 呼ぶ。type は
ordinary independent builtin-Move carrier rule を使う。local、by-value/shared-borrow parameter、
return、recursively admitted struct/sum/Option/Result field は valid。key を含む Move struct の
fixed/dynamic AoS array はその struct の recursive Drop plan により引き続き valid であり、whole
element read は既存 Move-element restriction に従う。direct key または tagged/sum-key を
fixed/dynamic scalar array、slice、vector/mask、array builder、pipeline の element にする形は reject
する。tuple/box placement、closure または task/parallel capture、`out`/`borrow mut` parameter、
global/constant storage、user-native/`layout(C)` exposure、print/equality/order/hash も reject する。
closed positive/negative inventory は structural carrier classifier 1つが所有し、future carrier は
fail closed する。key は input borrow lifetime や process-global registration を持たない。borrowed
sign/verify 後も owner は利用でき、message/signature view を retain しない。

private scalar material は Align `buffer` に入らない。caller-owned PEM `str` は caller storage
のままで zeroize しない。PEM scan は高々65,536-byte input を borrow する。base64 decode は
checked arithmetic で exact output length を計算し、fallible `OPENSSL_malloc` を1回呼び、
result を `SensitiveDer { ptr: NonNull<u8>, len: usize }` として保持する。fixed non-growing
allocation へ intermediate native copy なしで直接 decode する。その Drop は success と全
post-allocation base64/DER/import/validation/later-allocation failure で
`OPENSSL_clear_free(ptr, len)` を呼ぶ。provider import 後すぐに buffer を drop する。
private canonical-re-encoding scratch はすべて `OPENSSL_clear_free` を使う。
`PKCS8_PRIV_KEY_INFO` と imported `EVP_PKEY` は OpenSSL-owned のままで、それぞれ
`PKCS8_PRIV_KEY_INFO_free` と `EVP_PKEY_free` で release する。acceptance は OpenSSL 3.0 の
cleanup dependency、すなわち PKCS#8 ASN.1 free callback が private octet を cleanse し、
`EVP_PKCS82PKEY_ex` が internal encoded import buffer を clear-free することを pin する。
wrapper pointer はいずれの object からも owning call を越えて生存しない。

JWK construction は bounded component view と 65-byte stack EC point だけを使う。Ed25519
validation は construction 中だけ fixed-count の public-data BN temporary を allocate する。
provider/library-context/operation-context/key/BN/PKCS#8/re-encoding/shell allocation は fallible
で、failure path ごとに release する。

sign/verify は complete message を Align-side copy なしで borrow する。RS256/ES256 は
incremental EVP digest-sign/verify、Ed25519 は null digest name の required one-shot pure-EdDSA
call を使う。successful sign は fresh `buffer` 1つだけを allocate/publish する。RS256 は exact
modulus width、ES256/Ed25519 は64 bytes。ES256 の EVP DER signature は raw result publish 前に
bounded internal `r`/`s` storage へ decode する。failure は buffer を publish しない。verify は
public result allocation をせず、ES256 は fixed raw input を bounded internal DER へ変換する。
exact-length だが mathematical invalid/malformed な signature は `Ok(false)`。RS256 の
wrong-modulus-width、ES256/Ed25519 の non-64-byte signature も provider verify 前に
`Ok(false)`。`Ok(true)` は exact named algorithm だけでの verification を意味する。

constructor/sign/verify はすべて EVP を越えるため **Impure**。shared key cache や mutable
wrapper-global state はなく、independent key の call は overlap できる。各 constructor は ordinary
`OSSL_LIB_CTX` を作り、`default` という built-in provider を explicit load し、import、`_ex`
decode、`fromdata`、signature/digest fetch に exact property query `provider=default` を使う。
admitted construction/operation family は `d2i_PKCS8_PRIV_KEY_INFO` に続く
`EVP_PKCS82PKEY_ex`、`d2i_PUBKEY_ex`、`EVP_PKEY_CTX_new_from_name`/`EVP_PKEY_fromdata`、
`EVP_DigestSignInit_ex`/
`EVP_DigestVerifyInit_ex` である。null/global library context を使わず、OpenSSL configuration を
load せず、provider search path/default property
を変更せず、別 provider を load しない。result の `EVP_PKEY_get0_provider` pointer は publish 前に
shell provider pointer と一致し、各 sign/verify context の `EVP_PKEY_CTX_get0_provider` pointer は
engine action 前に一致しなければならない。mismatch/fetch failure は opaque `Error.Code(0)`。
したがって process-global provider/default property、`OPENSSL_CONF`、`OPENSSL_MODULES` は
implementation を substitute できない。ambient platform dependency は linked libcrypto 内蔵の
built-in default provider だけである。wrapper は exact algorithm parameter を渡し、
configuration/path/environment/terminal/network/clock を読まない。ES256 sign は provider
randomness を消費し得る。RS256/Ed25519 も標準構成が deterministic な場合を含め observable
determinism contract を約束しない。

key の task/parallel capture を forbidden にしているため shell/context は owning runtime thread に
留まる。operation は return 前に digest/PKEY context を free する。final Drop は `EVP_PKEY` を
free し、同 thread で private context の `OPENSSL_thread_stop_ex` を呼び、owned provider を unload、
library context を free、最後に shell を free する。partial construction は同じ acquired-prefix order
で unwind する。provider/context は shell より長く生存せず、shell は global OpenSSL state を
mutate しない。cleanup return status は operation の winning result を置換せず、provider unload が
failure を報告しても library-context free が final release になる。

constant-time scope は exact である。algorithm、key class、全 length、format、allocation outcome、
success/error class は public。constructor は secret private-key bytes を扱うが、PEM/DER parsing と
provider key validation は timing を約束せず trusted setup に限定する。caller は construction を
remote/repeated timing oracle として公開してはならない。successful construction 後の sign は、
fixed public length における private-key/message content に関して constant-time である。wrapper
code は private component を extract せず、それに branch/index せず、named high-level EVP
signature operation だけを使い、RSA blinding を enabled のまま保つ。pinned built-in default
provider の constant-time primitive implementation は明示した dependency であり、provider
provenance は ambient selection の assumption でなく key/operation construction 時に check する。
verification と wrapper-owned Ed25519 public-point check は public material を扱い constant-time
promise を持たない。evidence は wrapper source/LLVM、exact `_ex` context/property argument、
provider-pointer check、linked EVP API/parameter を audit する。functional vector や noisy
wall-clock statistic は constant-time evidence ではない。

### Errors and deterministic precedence

constructor format/key rejection と malformed internal ABI tag/view は `Error.Invalid`。provider
fetch/context/allocation または non-verification engine failure は既存の opaque
`Error.Code(0)`。OpenSSL error-stack number/text は公開しない。key と byte view が valid になった
後の signature length/encoding/mathematical mismatch はすべて error でなく data (`Ok(false)`)。
allocation/parse/sign failure は partial key/signature を publish しない。

OpenSSL の thread-local error queue は ambient input でなく operation-local wrapper state である。
fallible OpenSSL call の直前に wrapper は `ERR_clear_error` を呼び、その call の直後に queue
全体を classify/drain して、return または次の call より前に再び clear する。successful call
も incidental entry を clear する。call は synchronous で Align への callback を行わないため、
clear と drain の間に same-thread operation は interleave できない。independent runtime thread
は independent queue を持つ。entry 時の stale caller/provider error は意図的に discard し、
result を変えられない。

failure classification は closed かつ ordered である。

| Failing operation | `Error.Invalid` evidence | `Error.Code(0)` evidence |
|---|---|---|
| wrapper PEM/base64/DER version/canonical/complete-cursor または JWK check | direct checked rejection。result のため OpenSSL を consult しない | checked allocation/length arithmetic failure |
| `d2i_PKCS8_PRIV_KEY_INFO` または `d2i_PUBKEY_ex` | common wrapper `ERR_R_NESTED_ASN1_ERROR`/`ERR_R_MISSING_ASN1_EOS` を含む checked-in ASN.1 input-decode reason set だけを持つ non-empty drained queue | empty queue、`ERR_SYSTEM_ERROR`、fatal/malloc/internal/fetch/unsupported reason、その他の common/non-ASN.1 entry、closed set 外の entry のいずれか |
| `i2d_PKCS8_PRIV_KEY_INFO` または `i2d_PUBKEY` | なし。successful re-encoding 後の byte mismatch は wrapper canonical-DER rejection | 全 call/allocation failure |
| `EVP_PKCS82PKEY_ex` または JWK `fromdata` | 全 entry が checked-in ASN.1/RSA/EC input-rejection set、または `EVP_R_DECODE_ERROR`、`EVP_R_PRIVATE_KEY_DECODE_ERROR`、`EVP_R_INVALID_KEY`、`EVP_R_INVALID_KEY_LENGTH`、`EVP_R_INVALID_SEED_LENGTH`、`PROV_R_BAD_ENCODING`、`PROV_R_BAD_LENGTH`、`PROV_R_INVALID_DATA`、`PROV_R_INVALID_KEY`、`PROV_R_INVALID_KEY_LENGTH`、`PROV_R_INVALID_SEED_LENGTH` のいずれか | empty queue、`ERR_SYSTEM_ERROR`、fatal/malloc/internal/fetch/unsupported reason、closed input set 外の entry のいずれか |
| provider private/public/pairwise check | resource/internal entry のない documented zero invalid result | negative/unsupported result、empty failure queue、resource/internal entry のいずれか |
| provider/context/fetch/pointer-provenance/sign/verify setup または engine call | なし。verify の documented mismatch は別に `Ok(false)` | 全 failure |

mixed queue では `Error.Code(0)` が優先する。implementation は `ERR_SYSTEM_ERROR`、
`ERR_GET_LIB`、`ERR_GET_REASON`、`ERR_GET_RFLAGS` と symbolic library/reason constant で classify
し、localized text や unstable numeric literal を使わない。complete set は classifier の隣に置き、
この table の named family と equality-test する。OpenSSL version change で reason を追加するには
reviewed owner vector を要求する。empty/future unknown failure は fail closed して
`Error.Code(0)` になる。

multi-invalid runtime call の validation order は exact に次のとおり。

1. non-null かつ naturally aligned な ABI output slot を要求して zero し、closed algorithm tag を
   validate する。invalid output slot は write せず `AL_INVALID`。
2. key-taking operation は byte view を見る前に non-null/naturally aligned shell を要求し、そこに
   repeat された key kind/class を validate する。
3. slice を形成せず全 `(ptr, i64)` input pair を left-to-right に validate する。negative または
   `usize` に収まらない length と positive-length/null pointer を reject。zero length は null を許し、
   input pointer を dereference せず internal non-null empty sentinel を使う。順序は `pem`、`n` then
   `e`、`x` then `y`、`message`、または `message` then `signature`。Ed25519 の synthetic absent
   second JWK pair は exact null/zero。
4. public structural length を validate。PEM は `1..=65,536`、JWK は上記 exact component bound、
   empty message は valid。verify view 2つが valid になった後の wrong signature length は message
   content を読む前に `false` を publish する。
5. PEM envelope/base64 を validate し、exact decoded buffer を allocate して `SensitiveDer` へ
   decode する。または cheap JWK numeric/component encoding を validate。
6. private library context と explicit built-in default provider を作る。private input は上記
   PKCS#8-specific decode/version/canonical/import sequence だけを使い、public input は complete
   canonical SPKI 1つを decode する。各 error queue を scope/classify してから次へ進む。exact key
   algorithm/class/size/group、applicable provider key check、該当する independent Ed25519
   public-point check、key provider pointer と owned provider の一致を要求する。shell publish
   より前に private decode/re-encoding storage をすべて cleanse/release する。
7. private context 内で `provider=default` を使って operation context を作り、その provider pointer
   と owned provider の一致を要求し、exact digest/padding/group parameter を設定して engine を実行。
8. produced length/ES256 conversion を validate し、その後だけ owner/result を publish。

verify の signature-length failure は typed key handle check 後、message 処理前に
`Ok(false)`。malformed HIR でも invalid key を invalid signature で mask できない。cleanup は
winning result を返す前に実行し、置き換えない。

### Runtime ABI and compiler identity

internal `SignatureAlgorithm` byte は closed `0=RS256`, `1=ES256`, `2=Ed25519`。
`SignatureKeyKind` byte は closed `0=RS256-private`, `1=RS256-public`, `2=ES256-private`,
`3=ES256-public`, `4=Ed25519-private`, `5=Ed25519-public`。他の値は EVP call 前に reject。
runtime shell は kind を再保持し、各 operation が check するため malformed MIR でも static
type confusion は unsafe provider call にならない。残る private field は owned library-context、
provider、PKEY pointer であり、ABI には公開しない。

実装は次の exact internal declaration を追加する（C ABI の `algorithm` は `i32`。1-byte
range を validate してから narrow する）。

```text
i32 @align_rt_crypto_private_key_from_pem(i32, ptr, i64, ptr)
i32 @align_rt_crypto_public_key_from_pem(i32, ptr, i64, ptr)
i32 @align_rt_crypto_public_key_from_jwk(i32, ptr, i64, ptr, i64, ptr)
i32 @align_rt_crypto_sign(i32, ptr, ptr, i64, ptr)
i32 @align_rt_crypto_verify(i32, ptr, ptr, i64, ptr, i64, ptr)
void @align_rt_crypto_key_free(ptr)
```

JWK ABI は Ed25519 の absent second component を null/zero で渡す。constructor/sign result
slot は null initialize する pointer-sized handle slot、verify final slot は zero initialize する
`i32`。status `0` は success、`AL_INVALID` は `Error.Invalid`、`AL_CODE` は
`Error.Code(0)` に map する。
ordered validation contract の pointer/length/output-slot rule は5つの fallible operation row に
適用し、`key_free` は別に null-safe とする。Rust slice、shell dereference、BIO、EVP call は
validation 後だけ実行する。non-null input storage validity は checked-HIR validation 後の
compiler-internal ABI invariant である。

checked HIR/MIR は6つの unrelated enum arm でなく payloaded key type 1つを使う。canonical
type record version 3 は `Scalar::SignatureKey(kind)` に leaf tag 39 + exact one-byte kind、
`Ty::SignatureKey(kind)` に leaf tag 63 +同 byte を割り当てる。kind は payload なので next
tag 40/64 は unknown のまま。interface format 8 は不変で、nominal path は既存の
length-prefixed UTF-8 type record を使う。producer/importer は12個の bare/qualified path を認識し、
6個の qualified path に限って reconstructed source へ `std.crypto` を追加し、両方で
Move/return-cleanup identity を再構築する。各 key fingerprint は `EVP_PKEY` layout/structural definition graph
ではなく exact closed kind という nominal identity である。runtime inspection field、descriptor
thunk、source/artifact read はない。operation/helper discriminant は compiler build fingerprint、
in-process memo、frontend/object cache key、
whole/per-unit parity に一度だけ入り、exact source edit/revert は prior key を復元する。

OpenSSL libcrypto はこれら operation のいずれかが reachable なときだけ capability-link する。
new artifact/file format/CLI flag/environment variable/provider selector/package dependency はない。

### Implementation closure matrix

実装前の author-side matrix であり、reviewer の後続 discovery だけで row を close してはならない。

| Axis | Required closure | Owner evidence |
|---|---|---|
| Type formation and interface | no-import bare fallback 6 + import-required qualified name 6、local-shadow/entry-collision/import-use、Copy reject、Move/return-cleanup reconstruction、canonical kind/tag round-trip と exact next-unknown reject | `align_interface::summary` builtin/source-import sweep、`align_mir::canonical_graph` exact golden、`crypto_asymmetric::type_identity_matrix` whole/per-unit |
| Carrier closure | local、by-value/return/shared-borrow、struct/sum/Option/Result、recursive Drop される fixed/dynamic AoS Move-struct array を admit。direct または tagged/sum key の fixed/dynamic scalar array、slice、vector/mask、builder、pipeline element、tuple/box、closure/task/parallel capture、`out`/`borrow mut`、global/constant、user-native/`layout(C)`、print/equality/order/hash を reject。future carrier は fail closed | parameterized sema/checked-HIR `signature_key_carrier_matrix`、recursive DropPlan/codegen owner、malformed future-kind negative |
| Construction | private PEM 3、public PEM 3、decoded-JWK 3 constructor。success は complete shell owner 1つを initialize、failure は null。private input は dedicated decoder/import path を通る exact canonical PKCS#8 v1 `PrivateKeyInfo` version zero。wrong label、relabeled PKCS#1/SEC1 DER、`OneAsymmetricKey`、trailing/noncanonical DER、algorithm/class/curve/size/component と exact 65,536/65,537 PEM boundary を reject | runtime RFC/PEM/JWK vector + relabeled legacy/version-one negative を含む `crypto_asymmetric::constructor_matrix` |
| Ed25519 point admission | 全 SPKI/JWK public value と全 PKCS#8-derived public value が provider `public_check` と独立した wrapper-owned RFC 8032 compressed recovery、canonical `y`、sign-bit、curve-equation、re-encoding、`[8]A != identity` check を通る。BN failure と invalid point は異なる error | direct positive RFC 8032 vector、PEM/JWK を通す `y >= p`、nonsquare、`x=0/sign=1`、re-encoding、identity + 他7つの small-order negative、injected BN/raw-public-extraction failure、private-constructor helper-call assertion、provider-check success が wrapper rejection を override できない case |
| Move-in/out and cleanup | local bind、by-value parameter/return、shared borrow、struct/sum/Option/Result construction、`?`、`else`、`match`、`map_err`、branch/loop join、replacement、early return、ordinary/malformed Drop が kind 1つと exactly-one free を保持。source nulling は later Drop より先 | parameterized `crypto_asymmetric::ownership_matrix`、runtime free counter/failpoint、checked-HIR one-field negative |
| Sign/verify semantics | empty/binary/large message、RS256 padding+digest/modulus-width result、leading zero/invalid r/s を含む ES256 DER/raw、Ed25519 one-shot no-digest、valid/wrong-message/wrong-key/wrong-length signature、key reusable | runtime と `crypto_asymmetric` の RFC 7515/7518、RFC 8032/8410、OpenSSL cross-check vector |
| Private decoder and secret cleanup | base64 は non-growing `SensitiveDer` が所有する exact-length `OPENSSL_malloc` allocation 1つへ直接 decode。success と全 failure で `OPENSSL_clear_free`。wrapper version/algorithm check → `d2i_PKCS8_PRIV_KEY_INFO` → canonical/full-consumption check → `EVP_PKCS82PKEY_ex` だけを admit。private re-encoding scratch は clear-free し、PKCS#8 object/import-copy native cleanse dependency を pin | decode/DER/import/validation/success の cleanse spy と allocation failpoint、optimized source/LLVM/API で `OPENSSL_clear_free` が live なことを audit、relabeled PKCS#1/SEC1、version-one、attributes、noncanonical、trailing、malformed-inner owner vector |
| OpenSSL error classification | 各 fallible call は thread-local queue を clear、直後に drain/classify、再clear。direct invalid input と closed input-reason set は Invalid、empty/unknown/system/fatal/resource/internal/fetch/unsupported は Code、mixed stack は Code 優先。stale entry と independent-thread queue は call に影響しない | 全 algorithm の malformed ASN.1/inner-key vector、decoder/import allocation injection、stale/empty/unknown/mixed-invalid-plus-allocation/queue-empty-on-exit/parallel independent-queue owner |
| FFI/allocation/cleanup | 全 output slot を validate/alignment check 後 zero。全 input pair で negative/non-`usize`、null/zero、non-null/zero、null/positive、positive valid storage を slice/shell/EVP 前に cover。Ed25519 absent JWK は exact null/zero。injected failure ごとの libctx/provider/ctx/key/PKCS#8/DER/BIGNUM/signature/shell storage free、final free order は PKEY、thread-local context cleanup、provider、libctx、shell。partial publish なし、runtime kind recheck、reachable 時だけ libcrypto retain | runtime ABI view/slot、cleanse/error-queue/failpoint sweep、ABI declaration golden、capability-linking twin |
| Provider provenance | 各 shell は private ordinary libctx と explicit load した built-in default provider を所有。全 decode/import/signature/digest fetch は exact `provider=default`。key/operation provider pointer は owned pointer と一致。global ctx/config/search path/default property/provider を一切 consume しない | hostile `OPENSSL_CONF`/`OPENSSL_MODULES`、global null provider、incompatible global default property を持つ child-process owner、exact pointer assertion、independent-key overlap/teardown stress |
| Constant-time boundary | public BN validation を含む constructor parse/check は timing promise のない trusted setup。admitted private key と fixed public length では sign wrapper は secret key/message content を extract/branch/index せず exact high-level EVP operation を使い RSA blinding を enabled に保つ。pointer-verified built-in default provider primitive が named dependency。verification は public-data で promise 外 | wrapper source/LLVM secret-flow audit、forbidden low-level/private-component API guard、exact `_ex` libctx/property/provider-pointer と EVP algorithm/parameter/blinding inspection。timing benchmark は correctness evidence にしない |
| Compilation paths | direct/imported call、public key-bearing signature、function value、concrete key 周辺 generic monomorphization、whole/per-unit、object/frontend cache edit/revert、optimized/unoptimized LLVM、malformed HIR で identical algorithm/kind/effect/cleanup fact | `crypto_asymmetric` driver owner、interface/cache owner、checked-HIR validator matrix |
| Resource claim | PEM exact limit、RSA size bound、live key ごとの private libctx/provider/PKEY shell 1つ、private construction 中だけの exact wrapper `SensitiveDer` 1つ + bounded OpenSSL-owned PKCS#8/import storage、fixed-count Ed public BN temporary、fixed ES/Ed operation temporary、1-byte/8-MiB message で Align-side message copy なし。benchmark は local evidence で correctness gate ではない | `bench/crypto_asymmetric` live-key/peak-wrapper-allocation + private-construction peak/cleanse record + deterministic limit test |

実装は hand-written changed lines が roughly 1,000 を超える見込みでも capability PR 1つとする。
6 static type、constructor、runtime kind check、最初の sign/verify consumer は1つの proof boundary
であり、分割すると dormant key producer を land するか type/Drop/interface/ABI proof を algorithm
ごとに重複させる。RS256/ES256/Ed25519 は同一 boundary の parameterized cell であり、3つの
independent ownership mechanism ではない。

review 前に author は applicable matrix cell を implementation/regression に map し、またはここで
明示的に defer する。P1 または strategy-changing finding は implementation 続行前に matrix を reopen
する。

### Acceptance and synchronized sources

acceptance は complete matrix、`scripts/cargo.sh test -p align_runtime`、focused
`crypto_asymmetric` driver owner、interface/canonical/ABI golden、capability-linking twin、
private-decoder/cleanse/error-queue と constant-time boundary audit、bounded PR gate、Clippy を
要求する。local benchmark は ledger が
explicit message-copy/resource claim を持つためだけに実行し、correctness/constant-time gate でも
latency target でもない。

### Design-review finding-to-fix ledger

| Finding | Closure |
|---|---|
| P1 forbidden key carrier に negative ownership owner がなかった | closed positive/negative carrier inventory、fail-closed structural classifier、recursive AoS positive、parameterized sema/checked-HIR/codegen owner を追加。 |
| P1 byte-view null/length behavior が未指定だった | 全 input/output pointer、signed length、zero length、alignment、slice formation、multi-invalid rule と ABI sweep を固定。 |
| P1 private-key constant-time boundary が閉じていなかった | trusted constructor setup と fixed-public-length sign promise を分け、provider assumption と wrapper/API audit evidence を明記。 |
| P2 malformed signature と format rejection が矛盾した | `Error.Invalid` を constructor/internal-ABI rejection に限定し、post-view signature mismatch をすべて `Ok(false)` に固定。 |
| P2 bare key alias の import rule が nominal model と矛盾した | no-import bare fallback を復元し、qualified type spelling と value operation だけが `std.crypto` を要求。 |
| P2 HTTP streaming implementation status が drift した | roadmap、Settled record、draft、language digest を 2026-08-30 implemented に同期。 |
| P1 provider provenance が ambient assumption だけだった | provider axis を reopen。全 key が isolated libctx/built-in default provider を所有し、全 fetch は `provider=default`、key/operation pointer を check、teardown order を固定し、hostile-global child test が substitution を own する。 |
| P1 Ed25519 provider check が encoded point を validate しなかった | point admission を reopen。wrapper が PEM/JWK/private-derived public value に canonical RFC 8032 recovery、curve/re-encoding check、complete small-order rejection を独立実行する。 |
| P2 HTTP tail sentence 2つが implementation pending と記述していた | 残った `draft.md` と condensed spec の status sentence を implemented state に修正。 |
| P1 decoded private DER に cleanse owner がなかった | private-decoder cleanup を reopen。exact-length non-growing `SensitiveDer` と全 private re-encoding scratch を success/全 failure で clear-free し、native PKCS#8/import cleanup dependency と optimized-artifact/failpoint owner を追加。 |
| P2 auto private decoder が relabeled legacy DER を admit した | `d2i_AutoPrivateKey_ex` を除去。PKCS#8-specific decoder/import path の canonical version-zero `PrivateKeyInfo` だけを admit し、relabeled PKCS#1/SEC1 と `OneAsymmetricKey` negative を追加。 |
| P2 decoder null が invalid input と allocation/engine failure を混同した | per-call error-queue isolation、closed input-reason set、Code-dominant mixed-stack precedence、malformed/allocation/stale/empty/unknown/parallel owner を追加。 |

この節が source of truth である。public type/signature と algorithm/error/ownership contract は
`draft.md` §18.2、`docs/language-spec.md`、`docs/open-questions.md`、
`docs/impl/07-roadmap.md`、`docs/impl/19-hir-validation-ledger.md`、
`docs/impl/20-runtime-abi-ledger.md`、English original と一致させる。implementation status は
capability boundary だけで変更し、この contract を書き換えない。
