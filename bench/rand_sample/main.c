#define _POSIX_C_SOURCE 200809L
#include <inttypes.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

typedef struct { const uint8_t *ptr; int64_t len; } Array;
extern void align_rt_rng_seed_with(uint64_t *, int64_t);
extern Array align_rt_rng_sample(uint64_t *, const uint8_t *, int64_t, int64_t, int64_t);
extern void align_rt_free(void *);
static void require(int ok) { if (!ok) { fputs("sample benchmark invariant failed\n", stderr); exit(1); } }
static uint64_t rotate(uint64_t x, unsigned n) { return (x << n) | (x >> (64 - n)); }
static uint64_t next(uint64_t s[4]) {
    uint64_t result = rotate(s[0] + s[3], 23) + s[0], t = s[1] << 17;
    s[2] ^= s[0]; s[3] ^= s[1]; s[1] ^= s[2]; s[0] ^= s[3]; s[2] ^= t; s[3] = rotate(s[3], 45);
    return result;
}
static uint64_t bounded(uint64_t s[4], uint64_t range) {
    __uint128_t product = (__uint128_t)next(s) * range;
    if ((uint64_t)product < range) {
        uint64_t threshold = -range % range;
        while ((uint64_t)product < threshold) { product = (__uint128_t)next(s) * range; }
    }
    return (uint64_t)(product >> 64);
}
static uint64_t nanos(void) {
    struct timespec t;
    require(clock_gettime(CLOCK_MONOTONIC, &t) == 0);
    return (uint64_t)t.tv_sec * UINT64_C(1000000000) + (uint64_t)t.tv_nsec;
}
static void oracle(const uint64_t *source, size_t n, size_t k, uint64_t state[4], uint64_t *out) {
    size_t *permutation = malloc((n ? n : 1) * sizeof(*permutation));
    require(permutation != NULL);
    for (size_t i = 0; i < n; ++i) { permutation[i] = i; }
    for (size_t i = 0; i < k; ++i) {
        size_t j = i + (size_t)bounded(state, n - i), tmp = permutation[i];
        permutation[i] = permutation[j]; permutation[j] = tmp;
        out[i] = source[permutation[i]];
    }
    free(permutation);
}
static void run(size_t n, size_t k) {
    uint64_t *source = malloc((n ? n : 1) * sizeof(*source));
    uint64_t *expected = malloc((k ? k : 1) * 8 * sizeof(*expected));
    require(source != NULL && expected != NULL);
    for (size_t i = 0; i < n; ++i) { source[i] = ((uint64_t)i * 17) ^ UINT64_C(0x123456789abcdef); }
    uint64_t initial[8][4], final[8][4];
    for (size_t variant = 0; variant < 8; ++variant) {
        align_rt_rng_seed_with(initial[variant], (int64_t)variant * 29 + 7);
        memcpy(final[variant], initial[variant], sizeof(final[variant]));
        oracle(source, n, k, final[variant], expected + variant * k);
        uint64_t state[4]; memcpy(state, initial[variant], sizeof(state));
        Array out = align_rt_rng_sample(state, (const uint8_t *)source, (int64_t)n, (int64_t)k, 8);
        require(out.len == (int64_t)k && (k ? out.ptr != NULL : out.ptr == NULL));
        require(k == 0 || memcmp(out.ptr, expected + variant * k, k * 8) == 0);
        require(memcmp(state, final[variant], sizeof(state)) == 0);
        align_rt_free((void *)out.ptr);
    }
    size_t calls = n ? 8388608 / n : 50000;
    if (calls < 32) { calls = 32; }
    if (k <= 8 && calls < 4096) { calls = 4096; }
    if (k <= 128 && calls < 256) { calls = 256; }
    if (calls > 50000) { calls = 50000; }
    for (size_t trial = 0; trial < 7; ++trial) {
        uint64_t observed_calls = 0, observed_bytes = 0, start = nanos();
        for (size_t call = 0; call < calls; ++call) {
            size_t variant = call % 8;
            uint64_t state[4]; memcpy(state, initial[variant], sizeof(state));
            Array out = align_rt_rng_sample(state, (const uint8_t *)source, (int64_t)n, (int64_t)k, 8);
            require(out.len == (int64_t)k && (k ? out.ptr != NULL : out.ptr == NULL));
            require(memcmp(state, final[variant], sizeof(state)) == 0);
            if (k) {
                const uint64_t *values = (const uint64_t *)out.ptr;
                require(values[0] == expected[variant * k] && values[k - 1] == expected[(variant + 1) * k - 1]);
            }
            observed_calls++; observed_bytes += (uint64_t)out.len * 8;
            align_rt_free((void *)out.ptr);
        }
        uint64_t elapsed = nanos() - start;
        require(observed_calls == calls && observed_bytes == calls * k * 8 && elapsed > 0);
        printf("%zu,%zu,%zu,%" PRIu64 ",%" PRIu64 ",%.2f\n", n, k, trial, observed_calls, observed_bytes, (double)elapsed / (double)calls);
    }
    free(expected); free(source);
}
int main(void) {
    static const size_t cases[][2] = {
        {0,0},{1,0},{1,1},{64,1},{64,8},{64,64},
        {511,1},{512,1},{513,1},{1024,1},{1024,2},{1024,3},{1024,128},{1024,1024},
        {65536,1},{65536,8},{65536,127},{65536,128},{65536,129},{65536,4096},{65536,65536},
        {1048576,1},{1048576,8},{1048576,128},{1048576,2048},{1048576,2049}
    };
    puts("n,k,trial,calls,bytes,ns_per_call");
    for (size_t i = 0; i < sizeof(cases) / sizeof(cases[0]); ++i) { run(cases[i][0], cases[i][1]); }
    return 0;
}
