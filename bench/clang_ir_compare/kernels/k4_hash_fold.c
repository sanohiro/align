/* K4 — hash-style fold, the C equivalent of `xs.reduce(0, mix)` with `mix(h,x) = h*31 + x`. */
#include <stddef.h>
long run(const long *xs, size_t n) {
    long h = 0;
    for (size_t i = 0; i < n; i++) h = h * 31 + xs[i];
    return h;
}
