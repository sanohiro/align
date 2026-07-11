/* K3 — two-slice materialize, the C equivalent of `src.map(dbl).map_into(dst)`.
 * `restrict` mirrors Align's `out dst` no-alias guarantee (the semantically-equal C): the type
 * system proves src/dst distinct, so the fair C says so too. Drop `restrict` to observe clang
 * insert a `vector.memcheck` runtime overlap guard — a divergence recorded in the README. */
#include <stddef.h>
void scale(const long *restrict src, long *restrict dst, size_t n) {
    for (size_t i = 0; i < n; i++) dst[i] = src[i] * 2;
}
