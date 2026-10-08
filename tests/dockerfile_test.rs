//! Regression guard for MAIR-229: the production image must not run as root.
//!
//! Reads the production `Dockerfile` and checks its last (runtime) stage: it must be built
//! from the distroless `nonroot` variant and declare a numeric, non-zero `USER` so that
//! Kubernetes can enforce `runAsNonRoot: true`.

use std::path::Path;

/// Returns the instructions of the last build stage (from its `FROM` line onwards).
fn runtime_stage(dockerfile: &str) -> Vec<String> {
    let lines: Vec<String> = dockerfile
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(str::to_owned)
        .collect();
    let start = lines
        .iter()
        .rposition(|line| line.to_ascii_uppercase().starts_with("FROM "))
        .expect("Dockerfile has no FROM instruction");
    lines[start..].to_vec()
}

fn production_runtime_stage() -> Vec<String> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("Dockerfile");
    let content = std::fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("cannot read {}: {err}", path.display()));
    runtime_stage(&content)
}

#[test]
fn runtime_stage_uses_distroless_nonroot_image() {
    let stage = production_runtime_stage();
    let from = &stage[0];
    assert!(
        from.contains("gcr.io/distroless/") && from.contains(":nonroot"),
        "runtime stage must use a distroless `:nonroot` image, got `{from}`"
    );
}

#[test]
fn runtime_stage_declares_numeric_non_root_user() {
    let stage = production_runtime_stage();
    let user = stage
        .iter()
        .rev()
        .find(|line| line.to_ascii_uppercase().starts_with("USER "))
        .expect("runtime stage must declare a USER instruction");
    let uid = user[5..].trim().split(':').next().unwrap_or_default();
    let uid: u32 = uid
        .parse()
        .unwrap_or_else(|_| panic!("USER must be numeric for runAsNonRoot, got `{user}`"));
    assert_ne!(uid, 0, "runtime stage must not run as root");
}

/// `FROM` lines of a Dockerfile (comments and blank lines skipped).
fn from_lines(file: &str) -> Vec<String> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(file);
    let content = std::fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("cannot read {}: {err}", path.display()));
    content
        .lines()
        .map(str::trim)
        .filter(|line| line.to_ascii_uppercase().starts_with("FROM "))
        .map(str::to_owned)
        .collect()
}

/// MAIR-396: a re-pushed tag must not change the image, so every base image carries a digest, and
/// the development image never follows `latest`.
#[test]
fn every_base_image_is_pinned_by_digest() {
    for file in ["Dockerfile", "development.Dockerfile"] {
        for from in from_lines(file) {
            assert!(
                from.contains("@sha256:"),
                "{file}: `{from}` must pin its image by digest"
            );
            assert!(
                !from.contains(":latest"),
                "{file}: `{from}` must use a versioned tag"
            );
        }
    }
}

/// MAIR-396: the release binary is built from the committed `Cargo.lock`, never re-resolved.
#[test]
fn release_build_is_locked() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("Dockerfile");
    let content = std::fs::read_to_string(&path).expect("cannot read Dockerfile");
    let builds: Vec<&str> = content
        .lines()
        .map(str::trim)
        .filter(|line| line.contains("cargo build"))
        .collect();
    assert!(!builds.is_empty(), "the Dockerfile must build the API");
    for build in builds {
        assert!(build.contains("--locked"), "`{build}` must pass `--locked`");
    }
}
