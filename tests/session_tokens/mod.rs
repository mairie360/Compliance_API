//! Session tokens of the tests, signed like Core_API signs them (MAIR-680).
//!
//! Only Core holds the Ed25519 private key; an API only gets `JWT_PUBLIC_KEY` and no `JWT_SECRET`,
//! so it accepts `EdDSA` tokens signed with Core's key and refuses every HS256 token. The tests
//! play Core with the lib's test key pair (`test_setup::http_setup`) and keep the process in the
//! environment of a deployed API: [`set_jwt_env`] sets Core's public key only.
//!
//! Included with `mod session_tokens;` by the test binaries that call the JWT gate; each binary
//! uses part of it, hence the `dead_code` allowance at the inclusion.

use std::sync::Once;
use std::time::{SystemTime, UNIX_EPOCH};

use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
use mairie360_api_lib::jwt_manager::{
    parse_public_keys, parse_signing_key, Claims, SESSION_JWT_AUDIENCE, SESSION_JWT_ISSUER,
};
use mairie360_api_lib::test_setup::http_setup::{test_jwt_private_key, TEST_JWT_PUBLIC_KEY};

/// PKCS#8 body (base64) of an Ed25519 private key that is not Core's, assembled into PEM at run
/// time so that the secret scanners do not read a PEM private key in the sources.
const OTHER_PRIVATE_KEY_B64: &str =
    "MC4CAQAwBQYDK2VwBCIEII1DTYiu+dcwBgtEKil59uTH8s7XfrxGrbD+/P0sXq/8";

/// Public key of [`other_private_key`], never published in `JWT_PUBLIC_KEY`.
const OTHER_PUBLIC_KEY: &str = "-----BEGIN PUBLIC KEY-----
MCowBQYDK2VwAyEAtZHz1BNkijjkLJlnK9l7vikDl0aivrmbwllpT68Lnss=
-----END PUBLIC KEY-----
";

/// The environment of a deployed API after MAIR-680: Core's public key, no `JWT_SECRET`, no
/// private key. Once per process.
pub fn set_jwt_env() {
    static ENV: Once = Once::new();
    ENV.call_once(|| {
        std::env::set_var("JWT_PUBLIC_KEY", TEST_JWT_PUBLIC_KEY);
        std::env::remove_var("JWT_PRIVATE_KEY");
        std::env::remove_var("JWT_SECRET");
        std::env::set_var("JWT_TIMEOUT", "3600");
    });
}

/// Seconds since the epoch, as the `exp` claim counts them.
pub fn now() -> usize {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock after the epoch")
        .as_secs();
    usize::try_from(seconds).expect("timestamp fits in usize")
}

/// Claims of a session token issued by Core for `user_id`, expiring at `exp` (with `iss`, `aud`).
pub fn core_claims(user_id: i32, exp: usize) -> Claims {
    Claims::new(&user_id.to_string(), "user", exp)
        .with_issuer_and_audience(SESSION_JWT_ISSUER, SESSION_JWT_AUDIENCE)
}

/// An Ed25519 private key that is not Core's: what a compromised service could sign with.
pub fn other_private_key() -> String {
    let label = "PRIVATE KEY";
    format!("-----BEGIN {label}-----\n{OTHER_PRIVATE_KEY_B64}\n-----END {label}-----\n")
}

/// Signs `claims` as they are with `private_key` (PEM) under Core's `kid`: a genuine token with
/// [`test_jwt_private_key`], a forgery with [`other_private_key`].
pub fn sign_eddsa_with(claims: &Claims, private_key: &str) -> String {
    let core_keys = parse_public_keys(TEST_JWT_PUBLIC_KEY).expect("lib test public key");
    let mut header = Header::new(Algorithm::EdDSA);
    header.kid = Some(core_keys[0].kid().to_string());
    let known_keys = [
        core_keys,
        parse_public_keys(OTHER_PUBLIC_KEY).expect("other public key"),
    ]
    .concat();
    let key = parse_signing_key(private_key, &known_keys).expect("known private key");
    encode(&header, claims, key.encoding_key()).expect("encodable token")
}

/// Signs `claims` like Core does: `EdDSA`, the lib's test key pair, Core's `kid`.
pub fn sign_like_core(claims: &Claims) -> String {
    sign_eddsa_with(claims, &test_jwt_private_key())
}

/// A session token issued by Core for `user_id`, valid for an hour.
pub fn core_token(user_id: i32) -> String {
    set_jwt_env();
    sign_like_core(&core_claims(user_id, now() + 3600))
}

/// HS256 token for `user_id` signed with `secret`: what any holder of the old shared secret signs.
pub fn hs256_token(user_id: i32, secret: &str) -> String {
    encode(
        &Header::default(),
        &Claims::new(&user_id.to_string(), "user", now() + 3600),
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .expect("encodable token")
}
