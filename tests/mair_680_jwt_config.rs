//! MAIR-680: the API starts with Core's public key alone, the configuration check that
//! `AppState::new` runs at startup (`enforce_jwt_config`) passes without `JWT_SECRET`, and since
//! `mairie360_api_lib` 6.0.0 the old shared secret no longer replaces the public key.
//!
//! Own test binary: it changes the process-wide JWT variables.

use mairie360_api_lib::jwt_manager::{validate_jwt_config, JwtConfigError};

/// A production-like public key (not the lib's committed test key, which is refused outside test
/// stacks).
const DEPLOYED_PUBLIC_KEY: &str = "-----BEGIN PUBLIC KEY-----
MCowBQYDK2VwAyEAtZHz1BNkijjkLJlnK9l7vikDl0aivrmbwllpT68Lnss=
-----END PUBLIC KEY-----
";

#[test]
fn mair_680_public_key_alone_is_a_valid_configuration() {
    std::env::remove_var("JWT_SECRET");
    std::env::remove_var("JWT_PRIVATE_KEY");
    std::env::set_var("JWT_TIMEOUT", "3600");

    // Deployments may pass the PEM with escaped line breaks.
    for public_key in [
        DEPLOYED_PUBLIC_KEY.to_string(),
        DEPLOYED_PUBLIC_KEY.replace('\n', "\\n"),
    ] {
        std::env::set_var("JWT_PUBLIC_KEY", &public_key);
        assert_eq!(validate_jwt_config(), Ok(()), "{public_key}");
    }

    // No public key: the API must not start, even with the old shared secret.
    std::env::remove_var("JWT_PUBLIC_KEY");
    assert_eq!(validate_jwt_config(), Err(JwtConfigError::MissingPublicKey));
    // Built at run time: a literal secret in the repo would trip Gitleaks.
    std::env::set_var("JWT_SECRET", "old-shared-secret-".repeat(2));
    assert_eq!(validate_jwt_config(), Err(JwtConfigError::MissingPublicKey));
    std::env::remove_var("JWT_SECRET");
}
