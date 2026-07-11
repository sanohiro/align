/* K5 — prefix scan, the C equivalent of `xs.scan(0, add)` then reading the last element. */
#include <stddef.h>
void scan(const long *xs, long *out, size_t n) {
    long a = 0;
    for (size_t i = 0; i < n; i++) { a += xs[i]; out[i] = a; }
}
