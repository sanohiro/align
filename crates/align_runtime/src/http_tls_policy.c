/* Native objects for one immutable, verified HTTPS client. Rust owns the SSL_CTX on success. */
#include <openssl/err.h>
#include <openssl/ssl.h>
#include <openssl/x509v3.h>
#include <stddef.h>
#include <stdint.h>
#include <limits.h>

struct der_view { const unsigned char *data; size_t len; };

enum { tls_invalid = 2, tls_internal = 5, max_list = 64 };

static int error_status(int fallback) {
    unsigned long reason;
    while ((reason = ERR_get_error()) != 0) {
        if (ERR_GET_REASON(reason) == ERR_R_MALLOC_FAILURE) fallback = tls_internal;
    }
    return fallback;
}

static int ca_certificate(X509 *cert) {
    const uint32_t flags = X509_get_extension_flags(cert);
    return !(flags & EXFLAG_INVALID) && (flags & EXFLAG_BCONS) && (flags & EXFLAG_CA);
}

static int self_signed(X509 *cert) {
    return X509_NAME_cmp(X509_get_subject_name(cert), X509_get_issuer_name(cert)) == 0
        && X509_self_signed(cert, 1) == 1;
}

static X509 *read_certificate(struct der_view der) {
    const unsigned char *p = der.data;
    X509 *cert = d2i_X509(NULL, &p, (long)der.len);
    if (p != der.data + der.len) { X509_free(cert); return NULL; }
    return cert;
}

static int valid_direct_crl(X509_CRL *crl) {
    const ASN1_TIME *start = X509_CRL_get0_lastUpdate(crl);
    const ASN1_TIME *end = X509_CRL_get0_nextUpdate(crl);
    if (!start || !end || ASN1_TIME_compare(start, end) != -1 ||
        X509_CRL_get_ext_by_NID(crl, NID_delta_crl, -1) >= 0 ||
        X509_CRL_get_ext_by_NID(crl, NID_issuing_distribution_point, -1) >= 0) return 0;
    const STACK_OF(X509_REVOKED) *revoked = X509_CRL_get_REVOKED(crl);
    for (int i = 0; i < sk_X509_REVOKED_num(revoked); i++) {
        if (X509_REVOKED_get_ext_by_NID(sk_X509_REVOKED_value(revoked, i),
                                        NID_certificate_issuer, -1) >= 0) return 0;
    }
    return 1;
}

/* Every context is private until the Rust owner publishes the completed client. */
int align_tls_policy_init(int system_trust, SSL_CTX **out) {
    int status = tls_internal;
    *out = NULL;
    ERR_clear_error();
    SSL_CTX *ctx = SSL_CTX_new(TLS_client_method());
    if (!ctx || !SSL_CTX_set_min_proto_version(ctx, TLS1_2_VERSION)) goto finish;
    SSL_CTX_set_verify(ctx, SSL_VERIFY_PEER, NULL);
    if (!(SSL_CTX_set_mode(ctx, SSL_MODE_NO_AUTO_CHAIN) & SSL_MODE_NO_AUTO_CHAIN)) goto finish;
    SSL_CTX_set_session_cache_mode(ctx, SSL_SESS_CACHE_OFF);
    if (!(SSL_CTX_set_options(ctx, SSL_OP_NO_TICKET) & SSL_OP_NO_TICKET)) goto finish;
    /* Bind all effective verification paths to this client's store. The context owns
     * the first reference and both per-certificate store slots acquire one. */
    X509_STORE *store = X509_STORE_new();
    if (!store) goto finish;
    SSL_CTX_set_cert_store(ctx, store);
    if (!SSL_CTX_set1_verify_cert_store(ctx, store) || !SSL_CTX_set1_chain_cert_store(ctx, store)) goto finish;
    if (system_trust && !SSL_CTX_set_default_verify_paths(ctx)) goto finish;
    *out = ctx;
    return 0;
finish:
    status = error_status(status);
    SSL_CTX_free(ctx);
    ERR_clear_error();
    return status;
}

/* Process one complete field before Rust decodes the next field. Native objects retained
 * by SSL_CTX or its store hold their own references; all temporary references are local. */
int align_tls_policy_field(SSL_CTX *ctx, size_t field, const struct der_view *items, size_t count) {
    X509 *certs[max_list] = {0};
    X509_CRL *crls[max_list] = {0};
    PKCS8_PRIV_KEY_INFO *pkcs8 = NULL;
    EVP_PKEY *key = NULL;
    int status = tls_invalid;
    ERR_clear_error();
    if (field >= 4 || count > (field == 2 ? 1u : max_list)) goto finish;
    X509_STORE *store = SSL_CTX_get_cert_store(ctx);
    for (size_t i = 0; i < count; i++) {
        struct der_view der = items[i];
        status = tls_invalid;
        ERR_clear_error();
        if (!der.data || !der.len || der.len > LONG_MAX) goto finish;
        const unsigned char *p = der.data;
        if (field < 2) {
            X509 *cert = read_certificate(der);
            if (!cert) goto finish;
            certs[i] = cert;
            for (size_t j = 0; j < i; j++) if (X509_cmp(cert, certs[j]) == 0) goto finish;
            if (X509_get_extension_flags(cert) & EXFLAG_INVALID) goto finish;
            if (field == 0) {
                if (!ca_certificate(cert) || !self_signed(cert)) goto finish;
                status = tls_internal;
                if (!X509_STORE_add_cert(store, cert)) goto finish;
            } else if (!i) {
                if (X509_check_ca(cert) != 0 || SSL_CTX_use_certificate(ctx, cert) != 1) goto finish;
            } else {
                X509 *previous = certs[i - 1];
                if (!ca_certificate(cert) || self_signed(cert) ||
                    X509_check_issued(cert, previous) != X509_V_OK ||
                    X509_verify(previous, X509_get0_pubkey(cert)) != 1) goto finish;
                status = tls_internal;
                if (!SSL_CTX_add1_chain_cert(ctx, cert)) goto finish;
            }
        } else if (field == 2) {
            pkcs8 = d2i_PKCS8_PRIV_KEY_INFO(NULL, &p, (long)der.len);
            if (!pkcs8 || p != der.data + der.len) goto finish;
            key = EVP_PKCS82PKEY(pkcs8);
            if (!key || SSL_CTX_use_PrivateKey(ctx, key) != 1 || SSL_CTX_check_private_key(ctx) != 1) goto finish;
        } else {
            X509_CRL *crl = d2i_X509_CRL(NULL, &p, (long)der.len);
            crls[i] = crl;
            if (!crl || p != der.data + der.len || !valid_direct_crl(crl)) goto finish;
            for (size_t j = 0; j < i; j++) {
                if (!X509_NAME_cmp(X509_CRL_get_issuer(crl), X509_CRL_get_issuer(crls[j]))) goto finish;
            }
            status = tls_internal;
            if (!X509_STORE_add_crl(store, crl)) goto finish;
        }
    }
    if (field == 3 && count && !X509_STORE_set_flags(store, X509_V_FLAG_CRL_CHECK | X509_V_FLAG_CRL_CHECK_ALL)) {
        status = tls_internal;
        goto finish;
    }
    status = 0;
finish:
    if (status) status = error_status(status);
    for (size_t i = 0; i < count && i < max_list; i++) {
        X509_free(certs[i]);
        X509_CRL_free(crls[i]);
    }
    EVP_PKEY_free(key);
    PKCS8_PRIV_KEY_INFO_free(pkcs8);
    ERR_clear_error();
    return status;
}

/* A CRL-enabled context has only explicit roots and CRLs. Its store never performs lookups. */
int align_tls_crls_current(SSL_CTX *ctx, const time_t *at) {
    X509_STORE *store = SSL_CTX_get_cert_store(ctx);
    if (!X509_STORE_lock(store)) return 0;
    const STACK_OF(X509_OBJECT) *objects = X509_STORE_get0_objects(store);
    int valid = 1;
    time_t now = at ? *at : time(NULL);
    for (int i = 0; i < sk_X509_OBJECT_num(objects); i++) {
        X509_CRL *crl = X509_OBJECT_get0_X509_CRL(sk_X509_OBJECT_value(objects, i));
        if (crl && (X509_cmp_time(X509_CRL_get0_lastUpdate(crl), &now) >= 0 ||
                    X509_cmp_time(X509_CRL_get0_nextUpdate(crl), &now) <= 0)) { valid = 0; break; }
    }
    X509_STORE_unlock(store);
    return valid;
}
