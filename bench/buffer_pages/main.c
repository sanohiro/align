#define _GNU_SOURCE
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <sys/mman.h>
#include <sys/resource.h>
#include <sys/wait.h>
#include <unistd.h>
#include <errno.h>

#ifdef __linux__
typedef struct { unsigned char *ptr; int64_t len; } View;
typedef struct { struct timespec wall; struct rusage usage; } Stamp;
extern void *align_rt_buffer_filled(int64_t, unsigned char, int64_t, int32_t);
extern void align_rt_buffer_bytes(void *, View *);
extern void align_rt_buffer_free(void *);
static void require(int ok) { if (!ok) { perror("buffer page probe"); exit(2); } }
static Stamp stamp(void) {
    Stamp s;
    require(clock_gettime(CLOCK_MONOTONIC, &s.wall) == 0);
    require(getrusage(RUSAGE_SELF, &s.usage) == 0);
    return s;
}
static int64_t cpu(struct timeval t) { return (int64_t)t.tv_sec * 1000000000 + (int64_t)t.tv_usec * 1000; }
/* smaps values describe intersecting VMAs, not exclusive ownership of every byte in them. */
static long huge_kib(const unsigned char *p, size_t n) {
    FILE *f = fopen("/proc/self/smaps", "r"); require(f != NULL);
    char line[512]; unsigned long begin, end; long value, total = 0; int selected = 0;
    while (fgets(line, sizeof(line), f)) {
        if (sscanf(line, "%lx-%lx", &begin, &end) == 2)
            selected = n && end > (uintptr_t)p && begin < (uintptr_t)p + n;
        else if (selected && sscanf(line, "AnonHugePages: %ld kB", &value) == 1) total += value;
    }
    require(!ferror(f)); require(fclose(f) == 0); return total;
}
static void report(int mode, int read_first, int trial, const char *phase, size_t n,
                   const unsigned char *p, int advice, Stamp before) {
    Stamp after = stamp();
    int64_t wall = (int64_t)(after.wall.tv_sec - before.wall.tv_sec) * 1000000000
                 + after.wall.tv_nsec - before.wall.tv_nsec;
    printf("%d,%d,%d,%s,%zu,%zu,%d,%lld,%lld,%lld,%ld,%ld,%ld\n", mode, read_first, trial,
           phase, n, p ? (size_t)((uintptr_t)p % 4096) : 0, advice, (long long)wall,
           (long long)(cpu(after.usage.ru_utime) - cpu(before.usage.ru_utime)),
           (long long)(cpu(after.usage.ru_stime) - cpu(before.usage.ru_stime)),
           after.usage.ru_minflt - before.usage.ru_minflt, after.usage.ru_maxrss,
           p ? huge_kib(p, n) : 0);
}
static void check_bytes(const unsigned char *p, size_t n, unsigned char value) {
    uint64_t sum = 0;
    for (size_t i = 0; i < n; ++i) sum += p[i];
    require(sum == (uint64_t)n * value);
}
static void row(int mode, int read_first, int trial) {
    const size_t n = 551157760; /* One actual client cache slot, not a synthetic power of two. */
    const size_t page = (size_t)sysconf(_SC_PAGESIZE);
    require(page > 0 && (page & (page - 1)) == 0);
    const size_t span = (n + page - 1) & ~(page - 1);
    View view = {0}; void *owner = NULL; int advice = -1;
    Stamp start = stamp();
    if (mode < 2) {
        owner = align_rt_buffer_filled((int64_t)n, 0, 64, mode);
        require(owner != NULL);
        align_rt_buffer_bytes(owner, &view);
        require(view.len == (int64_t)n);
    } else {
        view.ptr = mmap(NULL, span, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
        require(view.ptr != MAP_FAILED);
        if (mode == 3) advice = madvise(view.ptr, n & ~(page - 1), MADV_HUGEPAGE);
    }
    require((uintptr_t)view.ptr % 64 == 0);
    report(mode, read_first, trial, "acquire", n, view.ptr, advice, start);
    if (read_first) {
        start = stamp(); check_bytes(view.ptr, n, 0);
        report(mode, read_first, trial, "first_read", n, view.ptr, advice, start);
    } else {
        /* Verify lazy initialized bytes without faulting the entire write-first working set. */
        require(view.ptr[0] == 0 && view.ptr[n - 1] == 0);
    }
    start = stamp(); memset(view.ptr, 0x5a, n);
    report(mode, read_first, trial, "first_write", n, view.ptr, advice, start);
    check_bytes(view.ptr, n, 0x5a);
    start = stamp(); memset(view.ptr, 0xa5, n);
    report(mode, read_first, trial, "rewrite", n, view.ptr, advice, start);
    check_bytes(view.ptr, n, 0xa5);
    start = stamp();
    if (owner) align_rt_buffer_free(owner); else require(munmap(view.ptr, span) == 0);
    report(mode, read_first, trial, "drop", n, NULL, advice, start);
}
#endif
int main(void) {
#ifndef __linux__
    fputs("This page-policy measurement requires Linux.\n", stderr);
    return 2;
#else
    puts("mode,read_first,trial,phase,bytes,offset_mod_4096,advice_result,wall_ns,user_ns,sys_ns,minor_faults,peak_rss_kib,intersecting_anon_huge_kib");
    fflush(stdout);
    for (int trial = 0; trial < 5; ++trial) {
        for (int read_first = 0; read_first <= 1; ++read_first) {
            for (int step = 0; step < 4; ++step) {
                /* Alternate paired order to reduce monotonically warming-host bias. */
                const int mode = trial % 2 ? 3 - step : step;
                pid_t pid = fork(); require(pid >= 0);
                if (pid == 0) { row(mode, read_first, trial); exit(0); }
                int status = 0; pid_t waited;
                do { waited = waitpid(pid, &status, 0); } while (waited < 0 && errno == EINTR);
                require(waited == pid && WIFEXITED(status) && WEXITSTATUS(status) == 0);
            }
        }
    }
    return 0;
#endif
}
