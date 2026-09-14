// Linux allocation diagnostics and portable timing use different binaries.
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <inttypes.h>
extern uint32_t bits_roundtrip(float);
extern uint32_t call_roundtrip(float);
extern uint32_t heap_control(float);
extern uint32_t byte_words_sum(const uint8_t *, int64_t);
extern uint32_t typed_words_sum(const uint32_t *, int64_t);
extern int64_t fresh_sum(void);
extern float byte_loop(const uint8_t *, int64_t);
extern float byte_max(const uint8_t *, int64_t);
extern float typed_max(const float *, int64_t);
static uint64_t allocs, frees, bytes;
#ifdef COUNT_ALLOC
static int counting;
void *__real_malloc(size_t);
void *__real_calloc(size_t, size_t);
void *__real_realloc(void *, size_t);
void __real_free(void *);
void *__wrap_malloc(size_t n) { if(counting) { ++allocs; bytes += n; } return __real_malloc(n); }
void *__wrap_calloc(size_t n, size_t m) { if(counting) { ++allocs; bytes += n*m; } return __real_calloc(n,m); }
void *__wrap_realloc(void *p, size_t n) { if(counting) { ++allocs; bytes += n; } return __real_realloc(p,n); }
void __wrap_free(void *p) { if(counting && p) ++frees; __real_free(p); }
#endif
static uint64_t now(void) { struct timespec t; if(clock_gettime(CLOCK_MONOTONIC,&t)) abort(); return (uint64_t)t.tv_sec*1000000000u + (uint64_t)t.tv_nsec; }
int main(int argc, char **argv) {
    if(argc != 4) return 2;
    int mode=atoi(argv[1]), n=atoi(argv[2]), reps=atoi(argv[3]);
    if(mode<0 || mode>8 || n<0 || n>50000 || reps<=0) return 2;
    float *input=malloc(((size_t)n+1)*sizeof(float));
    if(!input) return 3;
    for(int i=0;i<n;++i) input[i]=(float)(i%97)-48.0f;
    uint64_t checksum=0;
#ifdef COUNT_ALLOC
    counting=1;
#endif
    uint64_t start=now();
    for(int i=0;i<reps;++i) {
        uint32_t raw;
        if(mode==0) raw=bits_roundtrip((float)(i%97));
        else if(mode==3) raw=heap_control((float)(i%97));
        else if(mode==4) raw=(uint32_t)fresh_sum();
        else if(mode==5) raw=call_roundtrip((float)(i%97));
        else if(mode==7) raw=byte_words_sum((const uint8_t *)input,(int64_t)n*4);
        else if(mode==8) raw=typed_words_sum((const uint32_t *)input,n);
        else { float result=mode==6 ? byte_loop((const uint8_t *)input,(int64_t)n*4) : mode==1 ? byte_max((const uint8_t *)input,(int64_t)n*4) : typed_max(input,n); memcpy(&raw,&result,4); }
        checksum+=raw;
    }
    uint64_t elapsed=now()-start;
#ifdef COUNT_ALLOC
    counting=0;
#endif
    printf("mode=%d n=%d reps=%d ns_per_call=%.3f allocs=%"PRIu64" frees=%"PRIu64" bytes=%"PRIu64" checksum=%"PRIu64"\n", mode,n,reps,(double)elapsed/reps,allocs,frees,bytes,checksum);
    free(input);
    return 0;
}
