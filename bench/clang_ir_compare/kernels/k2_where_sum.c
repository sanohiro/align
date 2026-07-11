/* K2 — masked where + sum, the C equivalent of `xs.where(big).sum()`. */
#include <stddef.h>
long run(const long *xs, size_t n) {
    long acc = 0;
    for (size_t i = 0; i < n; i++) if (xs[i] > 6) acc += xs[i];
    return acc;
}
