#include <inttypes.h>
#include <stdint.h>
#include <stdio.h>
#include <time.h>

extern int64_t build_checked(int64_t, int64_t, int64_t);

static int64_t reference(int64_t n, int64_t seed) {
    return 17 * n * (n - 1) / 2 + n * seed;
}

int main(int argc, char **argv) {
    if (argc != 2 || (argv[1][0] != '0' && argv[1][0] != '1') || argv[1][1]) return 2;
    const int kind = argv[1][0] - '0';
    const int64_t lengths[] = {0, 1, 7, 63, 64, 65, 4096, 100000};
    for (unsigned i = 0; i < sizeof lengths / sizeof *lengths; i++) {
        for (int64_t seed = 3; seed <= 5; seed++) {
            int64_t n = lengths[i];
            if (build_checked(n, kind ? n : 0, seed) != reference(n, seed)) {
                fprintf(stderr, "array mismatch at length %" PRId64 ", seed %" PRId64 "\n", n, seed);
                return 1;
            }
        }
    }
    const int64_t n = 100000;
    const int rounds = 300;
    int64_t expected = 0, sum = 0;
    for (int r = 0; r < rounds; r++) expected += reference(n, 3 + r % 3);
    struct timespec start, end;
    if (clock_gettime(CLOCK_MONOTONIC, &start)) return 1;
    for (int r = 0; r < rounds; r++) sum += build_checked(n, kind ? n : 0, 3 + r % 3);
    if (clock_gettime(CLOCK_MONOTONIC, &end)) return 1;
    if (sum != expected) {
        fprintf(stderr, "timed array checksum mismatch\n");
        return 1;
    }
    int64_t ns = (end.tv_sec - start.tv_sec) * INT64_C(1000000000) + end.tv_nsec - start.tv_nsec;
    printf("%d,%" PRId64 ",%" PRId64 "\n", kind, ns, sum);
    return 0;
}
