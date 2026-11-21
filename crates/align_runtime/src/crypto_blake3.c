#include <blake3.h>
#include <stdint.h>

_Static_assert(BLAKE3_OUT_LEN == 32, "Align's digest result is 32 bytes");

typedef struct {
    const uint8_t *ptr;
    int64_t len;
} AlignStr;

extern uint8_t *align_rt_alloc(int64_t size);

/* Keep the only provider references in this archive member so unrelated
   programs can link the runtime without loading libblake3. */
AlignStr align_rt_crypto_blake3(const uint8_t *input, int64_t len) {
    blake3_hasher state;
    blake3_hasher_init(&state);
    if (input != NULL && len > 0 && (uint64_t)len <= PTRDIFF_MAX) {
        blake3_hasher_update(&state, input, (size_t)len);
    }
    uint8_t *digest = align_rt_alloc(BLAKE3_OUT_LEN);
    blake3_hasher_finalize(&state, digest, BLAKE3_OUT_LEN);
    return (AlignStr){digest, BLAKE3_OUT_LEN};
}
