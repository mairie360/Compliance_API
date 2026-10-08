//! Masking patterns of the logs (MAIR-498): `masking-patterns.yaml`, compiled in, served to the
//! OpenTelemetry Collector of the instance (`GET /api/v1/masking-patterns`) which replaces the
//! matches before storage. `mask` applies the same patterns here (journal excerpts, tests).

use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;
use utoipa::ToSchema;

const SOURCE: &str = include_str!("../masking-patterns.yaml");

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct MaskingPattern {
    pub name: String,
    /// RE2 syntax (the collector's).
    pub regex: String,
    pub replacement: String,
}

#[derive(Deserialize)]
struct Patterns {
    version: u32,
    patterns: Vec<MaskingPattern>,
}

/// The patterns of `masking-patterns.yaml`.
pub static PATTERNS: LazyLock<Vec<MaskingPattern>> = LazyLock::new(|| {
    let file: Patterns = yaml_serde::from_str(SOURCE).expect("masking-patterns.yaml is valid");
    assert_eq!(file.version, 1, "masking-patterns.yaml version");
    file.patterns
});

static COMPILED: LazyLock<Vec<(Regex, String)>> = LazyLock::new(|| {
    PATTERNS
        .iter()
        .map(|p| {
            (
                Regex::new(&p.regex).expect("valid masking pattern"),
                p.replacement.clone(),
            )
        })
        .collect()
});

/// `text` with every match of the patterns replaced.
#[must_use]
pub fn mask(text: &str) -> String {
    COMPILED
        .iter()
        .fold(text.to_owned(), |acc, (regex, replacement)| {
            regex.replace_all(&acc, replacement.as_str()).into_owned()
        })
}

#[cfg(test)]
mod tests {
    use super::{mask, PATTERNS};

    #[test]
    fn every_pattern_compiles_and_masks_its_kind() {
        assert!(PATTERNS.len() >= 5);
        let line = "login failed for Jane.Doe@mairie.fr from 10.0.0.4 phone 06 12 34 56 78 \
                    token Bearer abcdefghijklmnopqrstuv jwt eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxIn0.c2ln \
                    hash $argon2id$v=19$m=19456,t=2,p=1$c2FsdA$aGFzaA";
        let masked = mask(line);
        assert_eq!(
            masked,
            "login failed for <email> from 10.0.0.4 phone <phone> token Bearer <token> jwt <jwt> hash <hash>",
            "the IP address is kept for security by decision"
        );
        assert_eq!(mask("+33 6 12 34 56 78"), "<phone>");
    }
}
