//! The Renovate auto-approval is the reusable workflow of mairie360/CICD, which approves a Renovate
//! pull request only while Renovate alone wrote and pushed its branch: this repository only calls
//! it, with the same release on both pins and no permission beyond `pull-requests: write`.

use std::fs;
use std::path::PathBuf;

fn workflow() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".github/workflows/auto-approve.yml");
    fs::read_to_string(path).expect("read .github/workflows/auto-approve.yml")
}

fn live(line: &str) -> &str {
    line.split(" #").next().unwrap_or(line).trim()
}

#[test]
fn mair_685_auto_approve_calls_the_reusable_workflow_of_cicd() {
    let text = workflow();
    let uses: Vec<&str> = text.lines().filter_map(|line| live(line).strip_prefix("uses: ")).collect();
    assert_eq!(uses.len(), 1, "one call and no local step: {uses:?}");
    let tag = uses[0]
        .strip_prefix("mairie360/CICD/.github/workflows/auto-approve.yml@")
        .expect("calls the auto-approval of mairie360/CICD");
    let parts: Vec<&str> = tag.strip_prefix('v').unwrap_or_default().split('.').collect();
    assert!(
        parts.len() == 3 && parts.iter().all(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_digit())),
        "pinned to a release tag: {tag}"
    );
    let input = format!("cicd_version: {tag}");
    assert!(text.lines().any(|line| live(line) == input), "cicd_version is {tag} too");
    assert!(!text.lines().any(|line| live(line) == "steps:"), "no local step");
}

#[test]
fn mair_685_auto_approve_only_grants_pull_requests_write() {
    let text = workflow();
    assert!(text.lines().any(|line| line == "permissions: {}"), "no default token scope");
    let scopes: Vec<&str> =
        text.lines().map(live).filter(|line| line.ends_with(": write") || line.ends_with(": read")).collect();
    assert_eq!(scopes, vec!["pull-requests: write"]);
}

#[test]
fn mair_685_auto_approve_runs_on_every_push_to_the_branch() {
    assert!(workflow().lines().any(|line| live(line) == "types: [opened, reopened, synchronize]"));
}
