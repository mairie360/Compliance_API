//! MAIR-998: the cargo tests (testcontainers) and the docker compose stacks run the Database images
//! of the stage under test instead of a pinned release. Every `ghcr.io/mairie360/database` and
//! `liquibase-migrations` image of the compose files uses the moving tag `${UPSTREAM_TAG:-dev}`,
//! which mairie360/CICD sets to `dev` (PRs, main), `staging` (promote-staging gates) or `latest`
//! (promote-prod gates); `TEST_DB_VERSION` in `.cargo/config.toml` is `dev`, the stage of the cargo
//! tests. Nothing has to be re-pinned when the Database releases. The template image name of the
//! scaffold (`service_name`) must not come back either.
use std::fs;
use std::path::Path;

/// `${UPSTREAM_TAG:-dev}`, assembled so that no literal looks like a format string.
fn stage_tag() -> String {
    ["$", "{", "UPSTREAM_TAG:-dev", "}"].concat()
}

fn read(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(name);
    fs::read_to_string(&path).unwrap_or_else(|err| panic!("{}: {err}", path.display()))
}

fn test_db_version() -> String {
    read(".cargo/config.toml")
        .lines()
        .find_map(|line| line.trim().strip_prefix("TEST_DB_VERSION"))
        .and_then(|rest| rest.split('"').nth(1))
        .expect("TEST_DB_VERSION in .cargo/config.toml")
        .to_string()
}

fn compose_files() -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(env!("CARGO_MANIFEST_DIR"))
        .unwrap()
        .filter_map(|entry| entry.ok()?.file_name().into_string().ok())
        .filter(|name| {
            name.starts_with("docker-compose")
                && Path::new(name)
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("yml"))
        })
        .collect();
    names.sort();
    names
}

/// Tags of the Database images of one compose file, comments left out. An image reference without
/// a tag yields an empty tag, so it fails the comparison instead of being skipped. A tag that is a
/// variable keeps its `${…}` form.
fn database_tags(content: &str) -> Vec<String> {
    content
        .lines()
        .filter(|line| !line.trim_start().starts_with('#'))
        .filter_map(|line| {
            let rest = line.split("ghcr.io/mairie360/").nth(1)?;
            let name_end = rest
                .find([':', '@', '}', '"', '\'', ' '])
                .unwrap_or(rest.len());
            let (image, after) = rest.split_at(name_end);
            matches!(image, "database" | "liquibase-migrations").then(|| {
                after.strip_prefix(':').map_or_else(String::new, |tag| {
                    if tag.starts_with("${") {
                        tag.find('}').map_or(tag, |end| &tag[..=end]).to_string()
                    } else {
                        tag.trim_end_matches(['}', '"', '\'', ' ']).to_string()
                    }
                })
            })
        })
        .collect()
}

#[test]
fn mair_998_cargo_tests_run_the_dev_database() {
    assert_eq!(
        test_db_version(),
        "dev",
        "TEST_DB_VERSION follows the dev stage, like the compose stacks outside the promotions"
    );
}

#[test]
fn mair_998_compose_files_follow_the_stage_tag() {
    for name in compose_files() {
        let tags = database_tags(&read(&name));
        assert!(!tags.is_empty(), "{name} runs no Database image");
        for tag in tags {
            assert_eq!(
                tag,
                stage_tag(),
                "{name} pins a Database image instead of following the stage"
            );
        }
    }
}

#[test]
fn compose_files_no_longer_name_the_template_image() {
    for name in compose_files() {
        assert!(
            !read(&name).contains("service_name"),
            "{name} still names the template image `service_name`"
        );
    }
}

#[test]
fn mair_998_a_pinned_untagged_or_commented_database_image_is_seen_as_such() {
    let compose = "image: ghcr.io/mairie360/database:STAGE\n\
                   image: $DB_DEFAULT-ghcr.io/mairie360/liquibase-migrations:STAGE}\n\
                   image: $DB_DEFAULT-ghcr.io/mairie360/database:3.4.0}\n\
                   image: ghcr.io/mairie360/liquibase-migrations\n\
                   image: ghcr.io/mairie360/database@sha256:abc\n\
                   # image: ghcr.io/mairie360/database:1.0.0\n\
                   image: ghcr.io/mairie360/other:1.0.0"
        .replace("STAGE", &stage_tag())
        .replace("$DB_DEFAULT", &["$", "{", "DB_IMAGE:"].concat());
    let stage = stage_tag();
    assert_eq!(
        database_tags(&compose),
        vec![stage.as_str(), stage.as_str(), "3.4.0", "", ""],
        "every Database image is listed with its tag, comments left out"
    );
}
