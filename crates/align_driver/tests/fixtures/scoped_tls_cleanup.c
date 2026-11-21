/* ELF link observer for compiler temporary ownership and native TLS setup cleanup. */
#include <dirent.h>
#include <stdint.h>
#include <stdlib.h>

struct text { void *data; int64_t len; };
extern struct text __real_align_rt_str_clone(const unsigned char *, int64_t);
extern void __real_align_rt_free(void *);
extern void *__real_SSL_new(void *);
extern void __real_SSL_free(void *);
extern int __real_SSL_set_fd(void *, int);
extern int __real_SSL_set1_host(void *, const char *);
extern int __real_X509_VERIFY_PARAM_set1_ip_asc(void *, const char *);

static void *clones[5];
static int tracked, released, stage, injected, created, freed, baseline;
static int fd_count(void) {
    DIR *dir = opendir("/proc/self/fd");
    if (!dir) abort();
    int n = 0;
    while (readdir(dir)) n++;
    closedir(dir);
    return n;
}
void tls_stage(int next) {
    stage = next;
    baseline = fd_count();
    injected = created = freed = 0;
}
struct text __wrap_align_rt_str_clone(const unsigned char *data, int64_t n) {
    struct text result = __real_align_rt_str_clone(data, n);
    if (stage == 0 && result.data && n > 0) {
        if (tracked >= 5) abort();
        clones[tracked++] = result.data;
    }
    return result;
}
void __wrap_align_rt_free(void *p) {
    if (p) for (int i = 0; i < tracked; i++) {
        if (clones[i] == p) { clones[i] = NULL; released++; break; }
    }
    __real_align_rt_free(p);
}
void *__wrap_SSL_new(void *ctx) {
    if (stage == 1) { injected++; return NULL; }
    void *ssl = __real_SSL_new(ctx);
    if (ssl) created++;
    return ssl;
}
void __wrap_SSL_free(void *ssl) {
    if (ssl) freed++;
    __real_SSL_free(ssl);
}
int __wrap_SSL_set_fd(void *ssl, int fd) {
    if (stage == 2) { injected++; return 0; }
    return __real_SSL_set_fd(ssl, fd);
}
int __wrap_X509_VERIFY_PARAM_set1_ip_asc(void *p, const char *ip) {
    if (stage == 3) { injected++; return 0; }
    return __real_X509_VERIFY_PARAM_set1_ip_asc(p, ip);
}
int __wrap_SSL_set1_host(void *ssl, const char *host) {
    if (stage == 4) { injected++; return 0; }
    return __real_SSL_set1_host(ssl, host);
}
int tls_balance(void) {
    if (stage == 0) return tracked == 5 && released == tracked;
    return injected == 1 && created == (stage != 1) && freed == created && fd_count() == baseline;
}
