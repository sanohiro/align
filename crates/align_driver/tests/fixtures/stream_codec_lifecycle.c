#include <fcntl.h>
#include <stdint.h>
#include <unistd.h>
#include <zlib.h>
#include <zstd.h>
#include <zstd_errors.h>

/* GNU ld wraps the actual runtime/provider calls in this executable. */
static int made_r, dropped_r, made_w, dropped_w;
extern void *__real_align_rt_io_reader_stdin(void);
extern void *__real_align_rt_io_writer_std(int32_t, int32_t);
extern void __real_align_rt_io_reader_free(void *);
extern void __real_align_rt_io_writer_free(void *);
void *__wrap_align_rt_io_reader_stdin(void) {
    made_r++;
    return __real_align_rt_io_reader_stdin();
}
void *__wrap_align_rt_io_writer_std(int32_t fd, int32_t buffered) {
    made_w++;
    return __real_align_rt_io_writer_std(fd, buffered);
}
void __wrap_align_rt_io_reader_free(void *p) {
    if (p) dropped_r++;
    __real_align_rt_io_reader_free(p);
}
void __wrap_align_rt_io_writer_free(void *p) {
    if (p) dropped_w++;
    __real_align_rt_io_writer_free(p);
}
int64_t observed(void) {
    return made_r == made_w && made_r == dropped_r && made_w == dropped_w ? made_r : -1;
}

int32_t fail_descriptor(int32_t fd) {
    int replacement = open(fd == STDOUT_FILENO ? "/dev/full" : "/dev/null", O_WRONLY);
    if (replacement < 0) return -1;
    int result = dup2(replacement, fd);
    close(replacement);
    return result;
}

enum { Z_ENCODE, Z_DECODE, ZSTD_ENCODE, ZSTD_DECODE, KIND_COUNT };
static int acquired[KIND_COUNT], released[KIND_COUNT];
static int setup_fault, injected;

void fail_next_setup(void) { setup_fault = 1; }
int64_t balanced(int32_t kind) {
    for (int i = 0; i < KIND_COUNT; i++)
        if (acquired[i] != released[i]) return -1;
    return kind >= 0 && kind < KIND_COUNT ? acquired[kind] : -1;
}
int32_t observed_injections(void) { return injected; }

extern int __real_deflateInit2_(z_streamp, int, int, int, int, int, const char *, int);
int __wrap_deflateInit2_(z_streamp s, int level, int method, int bits,
        int mem, int strategy, const char *version, int size) {
    int rc = __real_deflateInit2_(s, level, method, bits, mem, strategy, version, size);
    if (rc == Z_OK) acquired[Z_ENCODE]++;
    return rc;
}
extern int __real_inflateInit2_(z_streamp, int, const char *, int);
int __wrap_inflateInit2_(z_streamp s, int bits, const char *version, int size) {
    int rc = __real_inflateInit2_(s, bits, version, size);
    if (rc == Z_OK) acquired[Z_DECODE]++;
    return rc;
}
extern int __real_deflateEnd(z_streamp);
int __wrap_deflateEnd(z_streamp s) { released[Z_ENCODE]++; return __real_deflateEnd(s); }
extern int __real_inflateEnd(z_streamp);
int __wrap_inflateEnd(z_streamp s) { released[Z_DECODE]++; return __real_inflateEnd(s); }
extern int __real_deflateSetDictionary(z_streamp, const Bytef *, uInt);
int __wrap_deflateSetDictionary(z_streamp s, const Bytef *bytes, uInt size) {
    if (setup_fault) { setup_fault = 0; injected++; return Z_STREAM_ERROR; }
    return __real_deflateSetDictionary(s, bytes, size);
}

extern ZSTD_CCtx *__real_ZSTD_createCCtx(void);
ZSTD_CCtx *__wrap_ZSTD_createCCtx(void) {
    ZSTD_CCtx *ctx = __real_ZSTD_createCCtx();
    if (ctx) acquired[ZSTD_ENCODE]++;
    return ctx;
}
extern size_t __real_ZSTD_freeCCtx(ZSTD_CCtx *);
size_t __wrap_ZSTD_freeCCtx(ZSTD_CCtx *ctx) { if (ctx) released[ZSTD_ENCODE]++; return __real_ZSTD_freeCCtx(ctx); }
extern ZSTD_DStream *__real_ZSTD_createDStream(void);
ZSTD_DStream *__wrap_ZSTD_createDStream(void) {
    ZSTD_DStream *ctx = __real_ZSTD_createDStream();
    if (ctx) acquired[ZSTD_DECODE]++;
    return ctx;
}
extern size_t __real_ZSTD_freeDStream(ZSTD_DStream *);
size_t __wrap_ZSTD_freeDStream(ZSTD_DStream *ctx) { if (ctx) released[ZSTD_DECODE]++; return __real_ZSTD_freeDStream(ctx); }
extern size_t __real_ZSTD_CCtx_loadDictionary(ZSTD_CCtx *, const void *, size_t);
size_t __wrap_ZSTD_CCtx_loadDictionary(ZSTD_CCtx *ctx, const void *bytes, size_t size) {
    if (setup_fault) { setup_fault = 0; injected++; return (size_t)0 - ZSTD_error_dictionary_corrupted; }
    return __real_ZSTD_CCtx_loadDictionary(ctx, bytes, size);
}
