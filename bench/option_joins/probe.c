#include <inttypes.h>
#include <stdint.h>
#include <stdio.h>
#include <time.h>

extern int64_t checked_sum(int64_t, int64_t);
extern int64_t checked_product(int64_t, int64_t);
extern int64_t wrapping_sum(int64_t, int64_t);

static const int64_t values[] = {0, 1, -1, 2, -2, 127, 65536, INT64_MAX, INT64_MIN};

static uint64_t reference(int kind, int64_t a, int64_t b) {
    int64_t result;
    if (kind == 0) return __builtin_add_overflow(a, b, &result) ? 0 : (uint64_t)result;
    if (kind == 1) return __builtin_mul_overflow(a, b, &result) ? 0 : (uint64_t)result;
    return (uint64_t)a + (uint64_t)b;
}

int main(int argc, char **argv) {
    if (argc != 2 || argv[1][0] < '0' || argv[1][0] > '2' || argv[1][1]) return 2;
    int kind = argv[1][0] - '0';
    int64_t (*run)(int64_t, int64_t) = kind == 0 ? checked_sum : kind == 1 ? checked_product : wrapping_sum;
    uint64_t expected = 0;
    for (int a = 0; a < 9; a++) for (int b = 0; b < 9; b++) {
        uint64_t want = reference(kind, values[a], values[b]);
        uint64_t got = (uint64_t)run(values[a], values[b]);
        if (got != want) {
            fprintf(stderr, "case %d/%d: got %" PRIu64 " want %" PRIu64 "\n", a, b, got, want);
            return 1;
        }
        expected += want;
    }
    const int rounds = 100000;
    uint64_t sum = 0;
    struct timespec start, end;
    if (clock_gettime(CLOCK_MONOTONIC, &start)) return 1;
    for (int r = 0; r < rounds; r++) for (int a = 0; a < 9; a++) for (int b = 0; b < 9; b++)
        sum += (uint64_t)run(values[a], values[b]);
    if (clock_gettime(CLOCK_MONOTONIC, &end)) return 1;
    if (sum != expected * (uint64_t)rounds) return 1;
    int64_t ns = (end.tv_sec - start.tv_sec) * INT64_C(1000000000) + end.tv_nsec - start.tv_nsec;
    printf("%d,%" PRId64 ",%" PRIu64 "\n", kind, ns, sum);
    return 0;
}
