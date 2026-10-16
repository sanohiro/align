#define _POSIX_C_SOURCE 200809L
#include <inttypes.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
typedef struct { const unsigned char *ptr; int64_t len; } View;
extern View align_rt_path_base(const unsigned char *, int64_t);
extern View align_rt_path_dir(const unsigned char *, int64_t);
extern View align_rt_path_ext(const unsigned char *, int64_t);
typedef struct { int group; const char *text; int64_t len; size_t offset[3]; int64_t length[3]; } Golden;
/* Explicit independent offsets/lengths. Empty views retain the original pointer. */
static const Golden goldens[] = {
    {0, "f", 1, {0, 0, 0}, {1, 0, 0}},
    {0, "f.x", 3, {0, 0, 1}, {3, 0, 2}},
    {0, "file.txt", 8, {0, 0, 4}, {8, 0, 4}},
    {0, "/usr/bin/ls", 11, {9, 0, 0}, {2, 8, 0}},
    {0, "a/b", 3, {2, 0, 0}, {1, 1, 0}},
    {0, "archive.tar.gz", 14, {0, 0, 11}, {14, 0, 3}},
    {0, ".bashrc", 7, {0, 0, 0}, {7, 0, 0}},
    {0, "dir/.hidden", 11, {4, 0, 0}, {7, 3, 0}},
    {0, "dir/name.ext/", 13, {4, 0, 8}, {8, 3, 4}},
    {0, "a.b/file", 8, {4, 0, 0}, {4, 3, 0}},
    {0, "a/b.c", 5, {2, 0, 3}, {3, 1, 2}},
    {0, "plain", 5, {0, 0, 0}, {5, 0, 0}},
    {1, "nnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnn", 32, {0, 0, 0}, {32, 0, 0}},
    {1, "/root/nnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnn", 38, {6, 0, 0}, {32, 5, 0}},
    {1, "nnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnn", 64, {0, 0, 0}, {64, 0, 0}},
    {1, "/root/nnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnn", 70, {6, 0, 0}, {64, 5, 0}},
    {1, "nnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnn", 128, {0, 0, 0}, {128, 0, 0}},
    {1, "/root/nnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnn", 134, {6, 0, 0}, {128, 5, 0}},
    {1, "nnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnn", 255, {0, 0, 0}, {255, 0, 0}},
    {1, "/root/nnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnn", 261, {6, 0, 0}, {255, 5, 0}},
    {1, "nnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnn", 1024, {0, 0, 0}, {1024, 0, 0}},
    {1, "/root/nnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnn", 1030, {6, 0, 0}, {1024, 5, 0}},
    {2, "name.xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", 37, {0, 0, 4}, {37, 0, 33}},
    {2, "/root/name.xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", 43, {6, 0, 10}, {37, 5, 33}},
    {2, "name.xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", 69, {0, 0, 4}, {69, 0, 65}},
    {2, "/root/name.xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", 75, {6, 0, 10}, {69, 5, 65}},
    {2, "name.xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", 133, {0, 0, 4}, {133, 0, 129}},
    {2, "/root/name.xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", 139, {6, 0, 10}, {133, 5, 129}},
    {2, "name.xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", 260, {0, 0, 4}, {260, 0, 256}},
    {2, "/root/name.xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", 266, {6, 0, 10}, {260, 5, 256}},
    {2, "name.xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", 1029, {0, 0, 4}, {1029, 0, 1025}},
    {2, "/root/name.xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", 1035, {6, 0, 10}, {1029, 5, 1025}},
    {3, "seg/seg/seg/seg/seg/seg/seg/seg/f.x", 35, {32, 0, 33}, {3, 31, 2}},
    {3, "seg/seg/seg/seg/seg/seg/seg/seg/f", 33, {32, 0, 0}, {1, 31, 0}},
    {3, "seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/f.x", 131, {128, 0, 129}, {3, 127, 2}},
    {3, "seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/f", 129, {128, 0, 0}, {1, 127, 0}},
    {3, "seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/f.x", 515, {512, 0, 513}, {3, 511, 2}},
    {3, "seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/f", 513, {512, 0, 0}, {1, 511, 0}},
    {3, "seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/f.x", 2051, {2048, 0, 2049}, {3, 2047, 2}},
    {3, "seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/seg/f", 2049, {2048, 0, 0}, {1, 2047, 0}},
    {4, "", 0, {0, 0, 0}, {0, 0, 0}},
    {4, "/", 1, {0, 0, 0}, {1, 1, 0}},
    {4, "////", 4, {0, 0, 0}, {1, 1, 0}},
    {4, ".", 1, {0, 0, 0}, {1, 0, 0}},
    {4, "..", 2, {0, 0, 1}, {2, 0, 1}},
    {4, "...", 3, {0, 0, 2}, {3, 0, 1}},
    {4, "a.", 2, {0, 0, 1}, {2, 0, 1}},
    {4, "a/.", 3, {2, 0, 0}, {1, 1, 0}},
    {4, ".x", 2, {0, 0, 0}, {2, 0, 0}},
    {4, "a/.x", 4, {2, 0, 0}, {2, 1, 0}},
    {4, "a/.x.y", 6, {2, 0, 4}, {4, 1, 2}},
    {4, "a//b...///", 10, {3, 0, 6}, {4, 1, 1}},
    {4, "/a/b////", 8, {3, 0, 0}, {1, 2, 0}},
    {4, "\316\261/\316\262.\347\225\214", 9, {3, 0, 5}, {6, 2, 4}},
    {4, "dir\134f.ext", 9, {0, 0, 5}, {9, 0, 4}},
    {4, "f\000.x", 4, {0, 0, 2}, {4, 0, 2}},
};
typedef View (*Operation)(const unsigned char *, int64_t);
static uint64_t now_ns(void) {
    struct timespec t;
    if (clock_gettime(CLOCK_MONOTONIC, &t) != 0) abort();
    return (uint64_t)t.tv_sec * UINT64_C(1000000000) + (uint64_t)t.tv_nsec;
}
static uint64_t run(Operation op, int kind, const Golden **selected, size_t count, size_t rounds, uint64_t *calls) {
    uint64_t checksum = 0; *calls = 0;
    for (size_t r = 0; r < rounds; ++r) {
        for (size_t i = 0; i < count; ++i) {
            const Golden *g = selected[i];
            View v = op((const unsigned char *)g->text, g->len);
            const unsigned char *expected = (const unsigned char *)g->text + g->offset[kind];
            if (v.ptr != expected || v.len != g->length[kind] || memcmp(v.ptr, expected, (size_t)v.len) != 0) {
                fprintf(stderr, "view mismatch: kind %d row %zu\n", kind, (size_t)(g - goldens));
                exit(1);
            }
            ++*calls;
            checksum += (uint64_t)v.len + (uint64_t)(v.ptr - (const unsigned char *)g->text);
        }
    }
    return checksum;
}
int main(int argc, char **argv) {
    if (argc != 3 || argv[1][0] < '0' || argv[1][0] > '2' || argv[1][1] != 0 || argv[2][0] < '0' || argv[2][0] > '4' || argv[2][1] != 0) return 2;
    int kind = argv[1][0] - '0', group = argv[2][0] - '0';
    Operation operations[] = {align_rt_path_base, align_rt_path_dir, align_rt_path_ext};
    const Golden *selected[sizeof goldens / sizeof *goldens];
    size_t count = 0; uint64_t per_sum = 0;
    for (size_t i = 0; i < sizeof goldens / sizeof *goldens; ++i) {
        const Golden *g = &goldens[i];
        if (g->group == group) { selected[count++] = g; per_sum += g->offset[kind] + (uint64_t)g->length[kind]; }
    }
    if (!count) return 3;
    uint64_t calls, start = now_ns();
    do { (void)run(operations[kind], kind, selected, count, 1000, &calls); } while (now_ns() - start < UINT64_C(50000000));
    size_t rounds = (size_t)(UINT64_C(1000000) / count);
    puts("kind,group,trial,calls,checksum,ns_per_call");
    for (int trial = 0; trial < 7; ++trial) {
        start = now_ns();
        uint64_t checksum = run(operations[kind], kind, selected, count, rounds, &calls);
        uint64_t elapsed = now_ns() - start;
        if (calls != count * rounds || checksum != per_sum * rounds) return 4;
        printf("%d,%d,%d,%" PRIu64 ",%" PRIu64 ",%.3f\n", kind, group, trial, calls, checksum, (double)elapsed / (double)calls);
    }
    return 0;
}
