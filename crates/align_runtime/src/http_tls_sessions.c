/* One single-use issuance per private HTTPS client. The SSL_CTX owns its cache, each SSL
 * owns its uncommitted callbacks and authentication interval. No callback accesses a Rust client. */
#define _POSIX_C_SOURCE 200809L
#include <openssl/ssl.h>
#include <openssl/err.h>
#include <stdint.h>
#include <limits.h>
#include <time.h>

enum { denied = 3, engine_error = 5 };
#define MAX_AGE_NS INT64_C(90000000000)

struct clock_pair { int64_t wall, monotonic; };
struct interval { int64_t from, until, verified_at; };
struct issuance { SSL_SESSION *session; struct interval auth; };
struct session_cache { CRYPTO_RWLOCK *lock; struct issuance slot; };
struct exchange { SSL_SESSION *pending; struct interval auth; int authenticated; };

static CRYPTO_ONCE indices_once = CRYPTO_ONCE_STATIC_INIT;
static int cache_index = -1, exchange_index = -1;

static void free_cache(void *parent, void *pointer, CRYPTO_EX_DATA *data, int idx, long arg, void *extra) {
    (void)parent; (void)data; (void)idx; (void)arg; (void)extra;
    struct session_cache *cache = pointer;
    if (cache) {
        SSL_SESSION_free(cache->slot.session);
        CRYPTO_THREAD_lock_free(cache->lock);
        OPENSSL_free(cache);
    }
}
static void free_exchange(void *parent, void *pointer, CRYPTO_EX_DATA *data, int idx, long arg, void *extra) {
    (void)parent; (void)data; (void)idx; (void)arg; (void)extra;
    struct exchange *ex = pointer;
    if (ex) { SSL_SESSION_free(ex->pending); OPENSSL_free(ex); }
}
static void allocate_indices(void) {
    cache_index = SSL_CTX_get_ex_new_index(0, NULL, NULL, NULL, free_cache);
    exchange_index = SSL_get_ex_new_index(0, NULL, NULL, NULL, free_exchange);
}
static int read_clock(const struct clock_pair *injected, struct clock_pair *now) {
    if (injected) { *now = *injected; return now->monotonic >= 0 && now->wall >= 0; }
    struct timespec mono;
    time_t wall = time(NULL);
    if (wall < 0 || clock_gettime(CLOCK_MONOTONIC, &mono) != 0 || mono.tv_sec < 0 ||
        mono.tv_sec > (INT64_MAX - mono.tv_nsec) / INT64_C(1000000000)) return 0;
    now->wall = wall;
    now->monotonic = mono.tv_sec * INT64_C(1000000000) + mono.tv_nsec;
    return 1;
}
static int current(struct interval auth, struct clock_pair now) {
    return now.wall >= auth.from && now.wall < auth.until && now.monotonic >= auth.verified_at
        && now.monotonic - auth.verified_at < MAX_AGE_NS;
}
static int capture(SSL *ssl, SSL_SESSION *session) {
    struct exchange *ex = SSL_get_ex_data(ssl, exchange_index);
    if (!ex) return 0;
    /* A capture is optional. Preserve errors already owned by the enclosing SSL operation. */
    int marked = ERR_set_mark();
    SSL_SESSION *copy = SSL_SESSION_is_resumable(session) ? SSL_SESSION_dup(session) : NULL;
    SSL_SESSION_free(ex->pending);
    ex->pending = copy;
    if (marked) ERR_pop_to_mark(); else ERR_clear_error();
    return 0;
}

int align_tls_sessions_enable(SSL_CTX *ctx) {
    if (!CRYPTO_THREAD_run_once(&indices_once, allocate_indices) || cache_index < 0 || exchange_index < 0)
        return engine_error;
    struct session_cache *cache = OPENSSL_zalloc(sizeof(*cache));
    if (!cache) return engine_error;
    cache->lock = CRYPTO_THREAD_lock_new();
    if (!cache->lock || !SSL_CTX_set_ex_data(ctx, cache_index, cache)) {
        free_cache(NULL, cache, NULL, 0, 0, NULL);
        return engine_error;
    }
    if (SSL_CTX_clear_options(ctx, SSL_OP_NO_TICKET) & SSL_OP_NO_TICKET) return engine_error;
    if (!(SSL_CTX_set_options(ctx, SSL_OP_NO_RENEGOTIATION) & SSL_OP_NO_RENEGOTIATION)) return engine_error;
    SSL_CTX_set_post_handshake_auth(ctx, 0);
    SSL_CTX_set_session_cache_mode(ctx, SSL_SESS_CACHE_CLIENT | SSL_SESS_CACHE_NO_INTERNAL);
    SSL_CTX_sess_set_new_cb(ctx, capture);
    return 0;
}

int align_tls_sessions_prepare(SSL *ssl, const struct clock_pair *clock) {
    struct session_cache *cache = SSL_CTX_get_ex_data(SSL_get_SSL_CTX(ssl), cache_index);
    struct exchange *ex = OPENSSL_zalloc(sizeof(*ex));
    if (!ex || !SSL_set_ex_data(ssl, exchange_index, ex)) { OPENSSL_free(ex); return engine_error; }
    if (!CRYPTO_THREAD_write_lock(cache->lock)) return engine_error;
    struct issuance candidate = cache->slot;
    cache->slot = (struct issuance){0};
    CRYPTO_THREAD_unlock(cache->lock);
    int status = 0;
    struct clock_pair now;
    if (candidate.session && read_clock(clock, &now) && current(candidate.auth, now)
        && SSL_SESSION_is_resumable(candidate.session)) {
        if (SSL_set_session(ssl, candidate.session) != 1) status = engine_error;
        else { ex->auth = candidate.auth; ex->authenticated = 1; }
    }
    SSL_SESSION_free(candidate.session);
    return status;
}

static int epoch_seconds(const ASN1_TIME *epoch, const ASN1_TIME *value, int64_t *out) {
    int days, seconds;
    if (!value || !ASN1_TIME_diff(&days, &seconds, epoch, value)) return 0;
    *out = (int64_t)days * 86400 + seconds;
    return 1;
}
static int intersect_certificate(struct interval *auth, const ASN1_TIME *epoch, X509 *cert) {
    int64_t from, until;
    if (!cert || !epoch_seconds(epoch, X509_get0_notBefore(cert), &from)
        || !epoch_seconds(epoch, X509_get0_notAfter(cert), &until) || from >= until) return 0;
    if (auth->from < from) auth->from = from;
    if (auth->until > until) auth->until = until;
    return auth->from < auth->until;
}
static int certificate_interval(SSL *ssl, struct interval *auth, struct clock_pair now) {
    const STACK_OF(X509) *peer = SSL_get0_verified_chain(ssl);
    if (sk_X509_num(peer) <= 0) return 0;
    ASN1_TIME *epoch = ASN1_TIME_set(NULL, 0);
    if (!epoch) return 0;
    *auth = (struct interval){ .from = now.wall, .until = INT64_MAX, .verified_at = now.monotonic };
    int valid = 1;
    for (int i = 0; valid && i < sk_X509_num(peer); i++)
        valid = intersect_certificate(auth, epoch, sk_X509_value(peer, i));
    X509 *client = SSL_get_certificate(ssl);
    if (valid && client) {
        STACK_OF(X509) *chain = NULL;
        valid = intersect_certificate(auth, epoch, client) && SSL_get0_chain_certs(ssl, &chain);
        for (int i = 0; valid && i < sk_X509_num(chain); i++)
            valid = intersect_certificate(auth, epoch, sk_X509_value(chain, i));
    }
    ASN1_TIME_free(epoch);
    return valid && current(*auth, now);
}

int align_tls_sessions_verified(SSL *ssl, const struct clock_pair *clock) {
    struct exchange *ex = SSL_get_ex_data(ssl, exchange_index);
    struct clock_pair now;
    int timed = read_clock(clock, &now);
    if (SSL_get_verify_result(ssl) != X509_V_OK) return denied;
    if (SSL_session_reused(ssl)) return timed && ex->authenticated && current(ex->auth, now) ? 0 : denied;
    ex->authenticated = timed && certificate_interval(ssl, &ex->auth, now);
    ERR_clear_error();
    return 0;
}

static int intersect_session(struct interval *auth, SSL_SESSION *session) {
#if OPENSSL_VERSION_NUMBER >= 0x30400000L
    int64_t issued = SSL_SESSION_get_time_ex(session);
#else
    int64_t issued = SSL_SESSION_get_time(session);
#endif
    long lifetime = SSL_SESSION_get_timeout(session);
    unsigned long hint = SSL_SESSION_get_ticket_lifetime_hint(session);
    if (issued < 0 || lifetime <= 0 || issued > INT64_MAX - lifetime) return 0;
    int64_t end = (int64_t)issued + lifetime;
    if (hint && hint < (unsigned long)lifetime) end = (int64_t)issued + (int64_t)hint;
    if (auth->from < issued) auth->from = issued;
    if (auth->until > end) auth->until = end;
    return auth->from < auth->until;
}

void align_tls_sessions_complete(SSL *ssl, const struct clock_pair *clock) {
    struct exchange *ex = SSL_get_ex_data(ssl, exchange_index);
    if (!ex || !ex->pending) return;
    struct issuance issue = { ex->pending, ex->auth };
    ex->pending = NULL;
    struct clock_pair now;
    if (ex->authenticated && SSL_SESSION_is_resumable(issue.session) && intersect_session(&issue.auth, issue.session)
        && read_clock(clock, &now) && current(issue.auth, now)) {
        struct session_cache *cache = SSL_CTX_get_ex_data(SSL_get_SSL_CTX(ssl), cache_index);
        if (CRYPTO_THREAD_write_lock(cache->lock)) {
            struct issuance previous = cache->slot;
            cache->slot = issue;
            CRYPTO_THREAD_unlock(cache->lock);
            SSL_SESSION_free(previous.session);
            return;
        }
    }
    SSL_SESSION_free(issue.session);
}
