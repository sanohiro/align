#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

typedef struct { const char *ptr; int64_t len; } Str;
extern int64_t prefix_kind(Str);
extern int64_t suffix_kind(Str);
extern int64_t dynamic_predicates(Str, Str);

static const char *words[] = {"alpha", "beta", "gamma", "delta", "epsilon", "zeta"};
static int64_t reference(Str hay, Str needle, int suffix) {
    return hay.len >= needle.len && memcmp(hay.ptr + (suffix ? hay.len - needle.len : 0), needle.ptr, (size_t)needle.len) == 0;
}

int main(int argc, char **argv) {
    if (argc != 2 || argv[1][0] < '0' || argv[1][0] > '2' || argv[1][1]) return 2;
    int kind = argv[1][0] - '0';
    char data[32][96], needles[32][24];
    Str input[32], needle[32];
    int64_t expected = 0;
    for (int i = 0; i < 32; i++) {
        const char *word = words[i % 6];
        if (kind == 0) snprintf(data[i], sizeof data[i], "%s%s/item", i % 4 ? "" : "missing-", word);
        else snprintf(data[i], sizeof data[i], "item.%s%s", word, i % 4 ? "" : "-missing");
        // The classifier's prefix fixture uses slash-delimited words.
        snprintf(needles[i], sizeof needles[i], "%s", i % 5 ? word : "");
        input[i] = (Str){data[i], (int64_t)strlen(data[i])};
        needle[i] = (Str){needles[i], (int64_t)strlen(needles[i])};
        int64_t want = 0;
        if (kind == 2) want = reference(input[i], needle[i], 0) + 2 * reference(input[i], needle[i], 1);
        else for (int j = 0; j < 6; j++) {
            char literal[24];
            snprintf(literal, sizeof literal, kind == 0 ? "%s/" : ".%s", words[j]);
            Str n = {literal, (int64_t)strlen(literal)};
            if (reference(input[i], n, kind == 1)) { want = j + 1; break; }
        }
        int64_t got = kind == 0 ? prefix_kind(input[i]) : kind == 1 ? suffix_kind(input[i]) : dynamic_predicates(input[i], needle[i]);
        if (got != want) { fprintf(stderr, "case %d: got %lld want %lld\n", i, (long long)got, (long long)want); return 1; }
        expected += want;
    }
    const int rounds = 200000;
    int64_t sum = 0;
    struct timespec start, end;
    if (clock_gettime(CLOCK_MONOTONIC, &start)) return 1;
    for (int r = 0; r < rounds; r++) for (int i = 0; i < 32; i++) {
        sum += kind == 0 ? prefix_kind(input[i]) : kind == 1 ? suffix_kind(input[i]) : dynamic_predicates(input[i], needle[i]);
    }
    if (clock_gettime(CLOCK_MONOTONIC, &end)) return 1;
    if (sum != expected * rounds) return 1;
    int64_t ns = (end.tv_sec - start.tv_sec) * INT64_C(1000000000) + end.tv_nsec - start.tv_nsec;
    printf("%d,%lld,%lld\n", kind, (long long)ns, (long long)sum);
    return 0;
}
