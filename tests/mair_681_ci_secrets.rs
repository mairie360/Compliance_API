//! The release secrets live in the Prod environment (main only) and reach the prod promotion
//! alone; pull request runs only get the low-quota key of the AI reviews.

use std::fs;
use std::path::PathBuf;

const RELEASE_ONLY: &[&str] = &["promote-prod.yml"];

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn workflows() -> Vec<(String, String)> {
    let dir = root().join(".github/workflows");
    let mut files: Vec<(String, String)> = fs::read_dir(&dir)
        .expect("read .github/workflows")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|ext| ext == "yml" || ext == "yaml")
        })
        .map(|path| {
            let name = path
                .file_name()
                .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
            (name, fs::read_to_string(&path).expect("read workflow"))
        })
        .collect();
    files.sort();
    files
}

fn passes(text: &str, secret: &str) -> bool {
    text.lines()
        .any(|line| line.starts_with(' ') && line.trim_start().starts_with(&format!("{secret}:")))
}

#[test]
fn mair_681_no_workflow_hands_every_secret_to_a_reusable_workflow() {
    for (name, text) in workflows() {
        assert!(
            !text.contains("secrets: inherit"),
            "{name} uses secrets: inherit"
        );
    }
}

#[test]
fn mair_681_release_secrets_only_reach_the_prod_promotion() {
    for (name, text) in workflows() {
        if RELEASE_ONLY.contains(&name.as_str()) {
            continue;
        }
        for secret in ["RELEASE_APP_ID", "RELEASE_APP_PK", "ANTHROPIC_API_KEY"] {
            assert!(!passes(&text, secret), "{name} passes {secret}");
        }
    }
}

#[test]
fn mair_681_ai_review_of_pull_requests_gets_its_dedicated_key() {
    // This repository has no GDPR PR review yet: any workflow that adds one passes the review key.
    for (name, text) in workflows() {
        if text.contains("gdpr-pr-cicd.yml") {
            assert!(
                passes(&text, "ANTHROPIC_REVIEW_API_KEY"),
                "{name} does not pass ANTHROPIC_REVIEW_API_KEY"
            );
        }
    }
}

#[test]
fn mair_681_release_journal_is_no_longer_committed() {
    assert!(!root().join("RELEASE_JOURNAL.md").exists());
}
