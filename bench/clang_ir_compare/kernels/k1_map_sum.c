/* K1 — map + sum reduction, the C equivalent of `xs.map(dbl).sum()`. */
#include <stddef.h>
long run(const long *xs, size_t n) {
    long acc = 0;
    for (size_t i = 0; i < n; i++) acc += xs[i] * 2;
    return acc;
}
