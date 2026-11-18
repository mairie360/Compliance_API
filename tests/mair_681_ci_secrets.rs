//! The release secrets live in the Prod environment (main only) and reach the prod promotion
//! alone; pull request runs only get the low-quota key of the AI reviews. The workflows are read
//! job by job, so a quoted `inherit`, a key mapped from another secret or a release secret read
//! by another job of the same file are caught too.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

const RELEASE_JOBS: &[&str] = &["promote-prod.yml:promote"];
const RELEASE_SECRETS: &[&str] = &["RELEASE_APP_ID", "RELEASE_APP_PK", "ANTHROPIC_API_KEY"];
const REVIEW_CALLERS: &[&str] = &[
    "APIs_cicd",
    "BFFs-cicd",
    "front-libs-cicd",
    "gdpr-pr-cicd",
    "promote-prod",
];

enum Secrets {
    None,
    Scalar(String),
    Map(Vec<(String, String)>),
}

struct Job {
    place: String,
    uses: String,
    secrets: Secrets,
    lines: Vec<String>,
}

struct Workflow {
    file: String,
    live: String,
    jobs: Vec<Job>,
}

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn strip_comment(line: &str) -> String {
    let bytes = line.as_bytes();
    let cut = (0..bytes.len())
        .find(|&i| bytes[i] == b'#' && (i == 0 || bytes[i - 1].is_ascii_whitespace()));
    line[..cut.unwrap_or(line.len())].trim_end().to_string()
}

fn unquote(value: &str) -> String {
    value
        .trim()
        .trim_matches(|c| c == '\'' || c == '"')
        .to_string()
}

fn is_name(text: &str) -> bool {
    !text.is_empty()
        && text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// Names read through `secrets.<NAME>` in a piece of workflow.
fn secret_refs(text: &str) -> Vec<String> {
    text.match_indices("secrets.")
        .map(|(start, found)| {
            text[start + found.len()..]
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                .collect::<String>()
        })
        .filter(|name| !name.is_empty())
        .collect()
}

fn release_reads(text: &str) -> usize {
    secret_refs(text)
        .iter()
        .filter(|name| RELEASE_SECRETS.contains(&name.as_str()))
        .count()
}

fn parse_jobs(file: &str, text: &str) -> Vec<Job> {
    let mut jobs: Vec<Job> = Vec::new();
    let (mut in_jobs, mut in_secrets) = (false, false);
    for raw in text.lines() {
        let line = strip_comment(raw);
        let trimmed = line.trim_start();
        let indent = line.len() - trimmed.len();
        if line == "jobs:" {
            in_jobs = true;
            continue;
        }
        if !in_jobs || trimmed.is_empty() {
            continue;
        }
        if indent == 0 {
            in_jobs = false;
            continue;
        }
        if indent == 2 && trimmed.ends_with(':') && is_name(&trimmed[..trimmed.len() - 1]) {
            jobs.push(Job {
                place: format!("{file}:{}", &trimmed[..trimmed.len() - 1]),
                uses: String::new(),
                secrets: Secrets::None,
                lines: Vec::new(),
            });
            in_secrets = false;
            continue;
        }
        let Some(job) = jobs.last_mut() else { continue };
        job.lines.push(line.clone());
        if in_secrets && indent <= 4 {
            in_secrets = false;
        }
        if in_secrets {
            if let (Some((key, value)), Secrets::Map(map)) =
                (trimmed.split_once(':'), &mut job.secrets)
            {
                map.push((key.trim().to_string(), value.trim().to_string()));
            }
            continue;
        }
        if indent == 4 {
            if let Some(value) = trimmed.strip_prefix("uses:") {
                job.uses = unquote(value);
            } else if let Some(value) = trimmed.strip_prefix("secrets:") {
                let value = unquote(value);
                in_secrets = value.is_empty();
                job.secrets = if in_secrets {
                    Secrets::Map(Vec::new())
                } else {
                    Secrets::Scalar(value)
                };
            }
        }
    }
    jobs
}

fn workflows() -> Vec<Workflow> {
    let dir = root().join(".github/workflows");
    let mut files: Vec<PathBuf> = fs::read_dir(&dir)
        .expect("read .github/workflows")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|ext| ext == "yml" || ext == "yaml")
        })
        .collect();
    files.sort();
    files
        .into_iter()
        .map(|path| {
            let file = path
                .file_name()
                .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
            let text = fs::read_to_string(&path).expect("read workflow");
            let live = text
                .lines()
                .map(strip_comment)
                .collect::<Vec<_>>()
                .join("\n");
            let jobs = parse_jobs(&file, &text);
            Workflow { file, live, jobs }
        })
        .collect()
}

fn jobs() -> Vec<Job> {
    workflows()
        .into_iter()
        .flat_map(|workflow| workflow.jobs)
        .collect()
}

fn mapping(job: &Job) -> &[(String, String)] {
    match &job.secrets {
        Secrets::Map(map) => map,
        _ => &[],
    }
}

#[test]
fn mair_681_no_reusable_workflow_call_inherits_every_secret() {
    for job in jobs() {
        if let Secrets::Scalar(value) = &job.secrets {
            panic!("{}: secrets: {value}", job.place);
        }
    }
}

#[test]
fn mair_681_each_secret_is_read_from_the_secret_of_the_same_name() {
    for job in jobs() {
        for (key, expression) in mapping(&job) {
            assert_eq!(
                secret_refs(expression),
                vec![key.clone()],
                "{}: {key} reads {expression}",
                job.place
            );
        }
    }
}

#[test]
fn mair_681_release_secrets_only_read_by_the_prod_promotion_job() {
    for workflow in workflows() {
        let in_release_jobs: usize = workflow
            .jobs
            .iter()
            .filter(|job| RELEASE_JOBS.contains(&job.place.as_str()))
            .map(|job| release_reads(&job.lines.join("\n")))
            .sum();
        assert_eq!(
            release_reads(&workflow.live),
            in_release_jobs,
            "{}",
            workflow.file
        );
    }
}

#[test]
fn mair_681_every_ai_review_gets_its_dedicated_key() {
    let jobs = jobs();
    for job in &jobs {
        let reviews = REVIEW_CALLERS.iter().any(|name| {
            job.uses
                .contains(&format!("mairie360/CICD/.github/workflows/{name}.yml@"))
        });
        if reviews {
            assert!(
                mapping(job)
                    .iter()
                    .any(|(key, _)| key == "ANTHROPIC_REVIEW_API_KEY"),
                "{} does not pass ANTHROPIC_REVIEW_API_KEY",
                job.place
            );
        }
    }
}

#[test]
fn mair_681_release_journal_is_no_longer_tracked() {
    let output = Command::new("git")
        .arg("-C")
        .arg(root())
        .args(["ls-files", "--", "RELEASE_JOURNAL.md"])
        .output()
        .expect("run git ls-files");
    assert!(output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stdout).trim().is_empty(),
        "RELEASE_JOURNAL.md is tracked"
    );
}
