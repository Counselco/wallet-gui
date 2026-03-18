/*
 * Deterministic Dilithium2 keypair generation from a 32-byte seed.
 *
 * Identical to PQClean's PQCLEAN_DILITHIUM2_CLEAN_crypto_sign_keypair()
 * but uses a caller-provided seed instead of randombytes().
 *
 * All PQClean internal functions are resolved at link time from
 * the pqcrypto-dilithium static library already in the build.
 */

#include <string.h>
#include <stdint.h>
#include "params.h"
#include "polyvec.h"
#include "packing.h"
#include "fips202.h"

/* Extern declarations — resolved from pqcrypto-dilithium at link time */
extern void PQCLEAN_DILITHIUM2_CLEAN_polyvec_matrix_expand(
    polyvecl mat[K], const uint8_t rho[SEEDBYTES]);
extern void PQCLEAN_DILITHIUM2_CLEAN_polyvecl_uniform_eta(
    polyvecl *v, const uint8_t seed[CRHBYTES], uint16_t nonce);
extern void PQCLEAN_DILITHIUM2_CLEAN_polyveck_uniform_eta(
    polyveck *v, const uint8_t seed[CRHBYTES], uint16_t nonce);
extern void PQCLEAN_DILITHIUM2_CLEAN_polyvecl_ntt(polyvecl *v);
extern void PQCLEAN_DILITHIUM2_CLEAN_polyvec_matrix_pointwise_montgomery(
    polyveck *t, const polyvecl mat[K], const polyvecl *v);
extern void PQCLEAN_DILITHIUM2_CLEAN_polyveck_reduce(polyveck *v);
extern void PQCLEAN_DILITHIUM2_CLEAN_polyveck_invntt_tomont(polyveck *v);
extern void PQCLEAN_DILITHIUM2_CLEAN_polyveck_add(
    polyveck *w, const polyveck *u, const polyveck *v);
extern void PQCLEAN_DILITHIUM2_CLEAN_polyveck_caddq(polyveck *v);
extern void PQCLEAN_DILITHIUM2_CLEAN_polyveck_power2round(
    polyveck *v1, polyveck *v0, const polyveck *v);

/*
 * Generate a Dilithium2 keypair deterministically from a 32-byte seed.
 *
 * The seed replaces the randombytes() call in the standard keygen.
 * Everything after that point is identical to the PQClean reference.
 *
 * Returns 0 on success.
 */
int chronx_dilithium2_seed_keypair(
    uint8_t pk[1312],   /* PQCLEAN_DILITHIUM2_CLEAN_CRYPTO_PUBLICKEYBYTES */
    uint8_t sk[2560],   /* PQCLEAN_DILITHIUM2_CLEAN_CRYPTO_SECRETKEYBYTES */
    const uint8_t seed[32])
{
    uint8_t seedbuf[2 * SEEDBYTES + CRHBYTES];
    uint8_t tr[TRBYTES];
    const uint8_t *rho, *rhoprime, *key;
    polyvecl mat[K];
    polyvecl s1, s1hat;
    polyveck s2, t1, t0;

    /* Use provided seed instead of randombytes(seedbuf, SEEDBYTES) */
    memcpy(seedbuf, seed, SEEDBYTES);
    shake256(seedbuf, 2 * SEEDBYTES + CRHBYTES, seedbuf, SEEDBYTES);
    rho = seedbuf;
    rhoprime = rho + SEEDBYTES;
    key = rhoprime + CRHBYTES;

    /* Expand matrix */
    PQCLEAN_DILITHIUM2_CLEAN_polyvec_matrix_expand(mat, rho);

    /* Sample short vectors s1 and s2 */
    PQCLEAN_DILITHIUM2_CLEAN_polyvecl_uniform_eta(&s1, rhoprime, 0);
    PQCLEAN_DILITHIUM2_CLEAN_polyveck_uniform_eta(&s2, rhoprime, L);

    /* Matrix-vector multiplication */
    s1hat = s1;
    PQCLEAN_DILITHIUM2_CLEAN_polyvecl_ntt(&s1hat);
    PQCLEAN_DILITHIUM2_CLEAN_polyvec_matrix_pointwise_montgomery(&t1, mat, &s1hat);
    PQCLEAN_DILITHIUM2_CLEAN_polyveck_reduce(&t1);
    PQCLEAN_DILITHIUM2_CLEAN_polyveck_invntt_tomont(&t1);

    /* Add error vector s2 */
    PQCLEAN_DILITHIUM2_CLEAN_polyveck_add(&t1, &t1, &s2);

    /* Extract t1 and write public key */
    PQCLEAN_DILITHIUM2_CLEAN_polyveck_caddq(&t1);
    PQCLEAN_DILITHIUM2_CLEAN_polyveck_power2round(&t1, &t0, &t1);
    PQCLEAN_DILITHIUM2_CLEAN_pack_pk(pk, rho, &t1);

    /* Compute H(rho, t1) and write secret key */
    shake256(tr, TRBYTES, pk, PQCLEAN_DILITHIUM2_CLEAN_CRYPTO_PUBLICKEYBYTES);
    PQCLEAN_DILITHIUM2_CLEAN_pack_sk(sk, rho, tr, key, &t0, &s1, &s2);

    return 0;
}
