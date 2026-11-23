//! MAIR-680: the API starts with Core's public key alone. Without `JWT_SECRET` the configuration
//! check that `AppState::new` runs at startup (`enforce_jwt_config`) must pass: the shared secret is
//! no longer needed, so it can be removed from every service but Core.
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

    // Neither the key nor the secret: the API must not start.
    std::env::remove_var("JWT_PUBLIC_KEY");
    assert_eq!(validate_jwt_config(), Err(JwtConfigError::MissingSecret));
}
