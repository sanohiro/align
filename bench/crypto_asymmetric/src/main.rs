//! Local resource evidence for the post-pkg.db asymmetric signature boundary.

use align_runtime::{
    AlignStr, Buffer, CryptoKey, align_rt_buffer_bytes, align_rt_buffer_free, align_rt_buffer_len,
    align_rt_crypto_key_free, align_rt_crypto_private_key_from_pem,
    align_rt_crypto_probe_live_keys, align_rt_crypto_probe_live_sensitive,
    align_rt_crypto_probe_peak_keys, align_rt_crypto_probe_peak_sensitive,
    align_rt_crypto_probe_reset, align_rt_crypto_probe_sensitive_cleanses,
    align_rt_crypto_public_key_from_pem, align_rt_crypto_sign, align_rt_crypto_verify,
};
use std::hint::black_box;
use std::ptr;
use std::time::{Duration, Instant};

const ED25519_PRIVATE_PEM: &[u8] = b"-----BEGIN PRIVATE KEY-----\n\
MC4CAQAwBQYDK2VwBCIEIJ1hsZ3v/VpguoRK9JLsLMREScVpezJpGXA7rAMcrn9g\n\
-----END PRIVATE KEY-----\n";
const ED25519_PUBLIC_PEM: &[u8] = b"-----BEGIN PUBLIC KEY-----\n\
MCowBQYDK2VwAyEA11qYAYKxCrfVS/7TyWQHOg7hcvPapiMlrwIaaPcHURo=\n\
-----END PUBLIC KEY-----\n";

unsafe fn private_key() -> *mut CryptoKey {
    let mut key = ptr::null_mut();
    assert_eq!(
        unsafe {
            align_rt_crypto_private_key_from_pem(
                2,
                ED25519_PRIVATE_PEM.as_ptr(),
                ED25519_PRIVATE_PEM.len() as i64,
                &mut key,
            )
        },
        0,
    );
    key
}

unsafe fn public_key() -> *mut CryptoKey {
    let mut key = ptr::null_mut();
    assert_eq!(
        unsafe {
            align_rt_crypto_public_key_from_pem(
                2,
                ED25519_PUBLIC_PEM.as_ptr(),
                ED25519_PUBLIC_PEM.len() as i64,
                &mut key,
            )
        },
        0,
    );
    key
}

unsafe fn sign(key: *mut CryptoKey, message: &[u8]) -> *mut Buffer {
    let mut signature = ptr::null_mut();
    assert_eq!(
        unsafe {
            align_rt_crypto_sign(
                2,
                key,
                message.as_ptr(),
                message.len() as i64,
                &mut signature,
            )
        },
        0,
    );
    assert_eq!(unsafe { align_rt_buffer_len(signature) }, 64);
    signature
}

unsafe fn measure_sign(key: *mut CryptoKey, message: &[u8], iterations: usize) -> Duration {
    let start = Instant::now();
    for _ in 0..iterations {
        let signature = unsafe { sign(key, black_box(message)) };
        black_box(unsafe { align_rt_buffer_len(signature) });
        unsafe { align_rt_buffer_free(signature) };
    }
    start.elapsed()
}

unsafe fn measure_verify(
    key: *mut CryptoKey,
    message: &[u8],
    signature: *mut Buffer,
    iterations: usize,
) -> Duration {
    let mut view = AlignStr {
        ptr: ptr::null(),
        len: 0,
    };
    unsafe { align_rt_buffer_bytes(signature, &mut view) };
    assert_eq!(view.len, 64);
    let start = Instant::now();
    for _ in 0..iterations {
        let mut verified = 0;
        assert_eq!(
            unsafe {
                align_rt_crypto_verify(
                    2,
                    key,
                    black_box(message).as_ptr(),
                    message.len() as i64,
                    view.ptr,
                    view.len,
                    &mut verified,
                )
            },
            0,
        );
        assert_eq!(verified, 1);
    }
    start.elapsed()
}

fn main() {
    unsafe {
        assert_eq!(align_rt_crypto_probe_reset(), 0);
        let mut keys = Vec::with_capacity(64);
        for _ in 0..64 {
            keys.push(public_key());
        }
        assert_eq!(align_rt_crypto_probe_live_keys(), 64);
        assert_eq!(align_rt_crypto_probe_peak_keys(), 64);
        for key in keys {
            align_rt_crypto_key_free(key);
        }
        assert_eq!(align_rt_crypto_probe_live_keys(), 0);
        assert_eq!(align_rt_crypto_probe_live_sensitive(), 0);
        println!("live-key matrix: peak=64 final=0");

        assert_eq!(align_rt_crypto_probe_reset(), 0);
        let private = private_key();
        assert_eq!(align_rt_crypto_probe_live_sensitive(), 0);
        assert_eq!(align_rt_crypto_probe_peak_sensitive(), 2);
        assert_eq!(align_rt_crypto_probe_sensitive_cleanses(), 2);
        println!("private construction: sensitive peak=2 cleanses=2 final=0");

        let public = public_key();
        let one = [0x5a];
        let large = vec![0xa5; 8 * 1024 * 1024];
        let one_elapsed = measure_sign(private, &one, 100);
        let large_elapsed = measure_sign(private, &large, 5);
        let one_signature = sign(private, &one);
        let large_signature = sign(private, &large);
        let one_verify_elapsed = measure_verify(public, &one, one_signature, 100);
        let large_verify_elapsed = measure_verify(public, &large, large_signature, 5);
        align_rt_buffer_free(one_signature);
        align_rt_buffer_free(large_signature);
        align_rt_crypto_key_free(private);
        align_rt_crypto_key_free(public);
        assert_eq!(align_rt_crypto_probe_live_keys(), 0);
        println!(
            "borrowed-message sign/verify: 1 byte / 100 = {:?} / {:?}; 8 MiB / 5 = {:?} / {:?}",
            one_elapsed, one_verify_elapsed, large_elapsed, large_verify_elapsed,
        );
    }
}
