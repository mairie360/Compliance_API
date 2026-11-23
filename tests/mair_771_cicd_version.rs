//! MAIR-771: every workflow of this repo calls the same mairie360/CICD release, v4.12.0 or later
//! (the first one with the GDPR controls). This guard fails when a workflow falls behind that
//! release, or when the workflows disagree on the version they call (the `uses:` ref and
//! `cicd_version` must stay equal, since the actions are checked out at `cicd_version`).

use std::fs;
use std::path::Path;

const MIN_CICD: (u64, u64, u64) = (4, 12, 0);
fn workflows() -> Vec<(String, String)> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join(".github/workflows");
    let mut found: Vec<(String, String)> = fs::read_dir(&dir)
        .unwrap_or_else(|err| panic!("{}: {err}", dir.display()))
        .map(|entry| entry.expect("workflow entry").path())
        .filter(|path| {
            path.extension()
                .is_some_and(|ext| ext == "yml" || ext == "yaml")
        })
        .map(|path| {
            let name = path
                .file_name()
                .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
            let text = fs::read_to_string(&path).unwrap_or_else(|err| panic!("{name}: {err}"));
            (name, text)
        })
        .collect();
    found.sort();
    found
}

fn parse_version(raw: &str) -> Option<(u64, u64, u64)> {
    let mut parts = raw.strip_prefix('v')?.split('.').map(str::parse::<u64>);
    let version = (
        parts.next()?.ok()?,
        parts.next()?.ok()?,
        parts.next()?.ok()?,
    );
    parts.next().is_none().then_some(version)
}

/// Every CICD version a workflow names: the `uses:` refs (the `# vX.Y.Z` comment of a SHA pin)
/// and the `cicd_version:` inputs.
fn cicd_versions(workflow: &str) -> Vec<String> {
    let mut versions = Vec::new();
    for line in workflow.lines().map(str::trim) {
        if let Some(rest) = line.strip_prefix("uses: mairie360/CICD/") {
            let reference = rest.split_once('@').map_or("", |(_, r)| r);
            let (pin, comment) = reference.split_once('#').unwrap_or((reference, ""));
            let comment = comment.trim();
            versions.push(
                if comment.is_empty() {
                    pin.trim()
                } else {
                    comment
                }
                .to_owned(),
            );
        } else if let Some(rest) = line.strip_prefix("cicd_version:") {
            versions.push(rest.split('#').next().unwrap_or("").trim().to_owned());
        }
    }
    versions
}

#[test]
fn mair_771_every_workflow_calls_the_same_cicd_release() {
    let mut seen: Vec<(String, String)> = Vec::new();
    for (name, text) in workflows() {
        seen.extend(cicd_versions(&text).into_iter().map(|v| (name.clone(), v)));
    }
    assert!(!seen.is_empty(), "no workflow calls mairie360/CICD");
    let first = seen[0].1.clone();
    for (name, version) in &seen {
        assert_eq!(
            version, &first,
            "{name} pins CICD {version}, expected {first} everywhere"
        );
    }
    let parsed = parse_version(&first).unwrap_or_else(|| panic!("CICD pin {first} is not vX.Y.Z"));
    assert!(
        parsed >= MIN_CICD,
        "CICD {first} predates v4.12.0: no gdpr_contract, gdpr_marker nor gdpr_simulation"
    );
}

#[test]
fn mair_771_version_parser_rejects_what_is_not_a_release() {
    assert_eq!(parse_version("v5.2.2"), Some((5, 2, 2)));
    assert!(parse_version("v4.4.3").is_some_and(|v| v < MIN_CICD));
    assert_eq!(parse_version("main"), None);
    assert_eq!(parse_version("v5.2"), None);
    assert_eq!(
        cicd_versions("    uses: mairie360/CICD/.github/workflows/x.yml@0123abc # v5.2.2\n      cicd_version: v5.2.2  # note"),
        vec!["v5.2.2".to_owned(), "v5.2.2".to_owned()]
    );
}
