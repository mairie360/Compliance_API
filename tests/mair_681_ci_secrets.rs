//! The release secrets live in the Prod environment (main only) and reach the prod promotion
//! alone; pull request runs only get the low-quota key of the AI reviews. The workflows are read
//! job by job (quoted keys included): every call of a mairie360/CICD workflow passes exactly the
//! secrets that workflow reads, each as a direct `${{ secrets.<same name> }}`, and no other job
//! reads a release secret, whatever the notation.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

const RELEASE_JOBS: &[&str] = &["promote-prod.yml:promote"];
const RELEASE_SECRETS: &[&str] = &["RELEASE_APP_ID", "RELEASE_APP_PK", "ANTHROPIC_API_KEY"];
const REVIEW: &[&str] = &["ANTHROPIC_REVIEW_API_KEY", "ANTHROPIC_WORKSPACE_ID"];
const BUILD: &[&str] = &["CODECOV_TOKEN", "N8N_WEBHOOK_SECRET"];
const RELEASE_APP: &[&str] = &["RELEASE_APP_ID", "RELEASE_APP_PK"];
/// The nightly prod dry run only hands the review key over when it asks for the AI review.
const DRY_RUN_REVIEW: &str =
    "${{ needs.check.outputs.ai == 'true' && secrets.ANTHROPIC_REVIEW_API_KEY || '' }}";
const CICD_WORKFLOWS: &str = "mairie360/CICD/.github/workflows/";

/// Secrets each mairie360/CICD reusable workflow reads, by called workflow (`[dry-run]`: nightly gates).
fn expected(callee: &str) -> Option<Vec<&'static str>> {
    let set: Vec<&[&'static str]> = match callee {
        "APIs_cicd.yml" | "BFFs-cicd.yml" | "front-libs-cicd.yml" => vec![REVIEW, BUILD],
        "back-lib-cicd.yml" => vec![&["CARGO_REGISTRY_TOKEN"], BUILD, RELEASE_APP],
        "bffs-lib-cicd.yml" | "database_cicd.yml" | "frontend-cicd.yml" => vec![BUILD],
        "frontend-dev-exception.yml" => vec![],
        "gdpr-pr-cicd.yml" => vec![REVIEW],
        "promote-prod.yml" => vec![
            &["ANTHROPIC_API_KEY", "N8N_WEBHOOK_SECRET"],
            REVIEW,
            RELEASE_APP,
        ],
        "promote-prod.yml [dry-run]" => vec![&["N8N_WEBHOOK_SECRET"], REVIEW],
        "promote-staging.yml" | "promote-staging.yml [dry-run]" | "terraform_cicd.yml" => {
            vec![&["N8N_WEBHOOK_SECRET"]]
        }
        _ => return None,
    };
    let mut keys: Vec<&'static str> = set.concat();
    keys.sort_unstable();
    Some(keys)
}

enum Secrets {
    Absent,
    Scalar(String),
    Map(Vec<(String, String)>),
}

struct Job {
    place: String,
    file: String,
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

fn unquote(text: &str) -> String {
    let text = text.trim();
    let quoted = text.len() >= 2
        && (text.starts_with('"') && text.ends_with('"')
            || text.starts_with('\'') && text.ends_with('\''));
    if quoted {
        text[1..text.len() - 1].to_string()
    } else {
        text.to_string()
    }
}

/// `key: value` of a block mapping line, the key unquoted; `None` for another line.
fn key_value(line: &str) -> Option<(String, String)> {
    let text = line.trim_start();
    let quote = text.chars().next().filter(|c| *c == '"' || *c == '\'');
    let colon = match quote {
        Some(q) => text[1..]
            .find(q)
            .map(|end| end + 2)
            .filter(|&i| text[i..].trim_start().starts_with(':'))?,
        None => text.find(':')?,
    };
    let key = unquote(text[..colon].trim());
    let rest = text[colon..].trim_start().strip_prefix(':')?;
    if key.is_empty() || !(rest.is_empty() || rest.starts_with(' ')) {
        return None;
    }
    Some((key, rest.trim().to_string()))
}

fn parse_jobs(file: &str, text: &str) -> Vec<Job> {
    let mut jobs: Vec<Job> = Vec::new();
    let (mut in_jobs, mut in_secrets) = (false, false);
    for raw in text.lines() {
        let line = strip_comment(raw);
        let trimmed = line.trim_start();
        if trimmed.is_empty() {
            continue;
        }
        let indent = line.len() - trimmed.len();
        let entry = key_value(&line);
        if indent == 0 {
            in_jobs = matches!(&entry, Some((key, value)) if key == "jobs" && value.is_empty());
            continue;
        }
        if !in_jobs {
            continue;
        }
        if let (2, Some((key, value))) = (indent, &entry) {
            if value.is_empty() {
                jobs.push(Job {
                    place: format!("{file}:{key}"),
                    file: file.to_string(),
                    uses: String::new(),
                    secrets: Secrets::Absent,
                    lines: Vec::new(),
                });
                in_secrets = false;
                continue;
            }
        }
        let Some(job) = jobs.last_mut() else { continue };
        job.lines.push(line.clone());
        if in_secrets && indent <= 4 {
            in_secrets = false;
        }
        if in_secrets {
            if let (Some((key, value)), Secrets::Map(map)) = (&entry, &mut job.secrets) {
                map.push((key.clone(), unquote(value)));
            }
            continue;
        }
        match (indent, entry) {
            (4, Some((key, value))) if key == "uses" => job.uses = unquote(&value),
            (4, Some((key, value))) if key == "secrets" => {
                let value = unquote(&value);
                in_secrets = value.is_empty();
                job.secrets = if in_secrets {
                    Secrets::Map(Vec::new())
                } else {
                    Secrets::Scalar(value)
                };
            }
            _ => {}
        }
    }
    jobs
}

/// Release secrets read in a piece of workflow, in dot or index notation, any case.
fn release_reads(text: &str) -> usize {
    let lower = text.to_ascii_lowercase();
    let is_name = |c: char| c.is_ascii_alphanumeric() || c == '_';
    lower
        .match_indices("secrets")
        .filter_map(|(start, word)| {
            let rest = lower[start + word.len()..].trim_start();
            let name: String = if let Some(after) = rest.strip_prefix('.') {
                after
                    .trim_start()
                    .chars()
                    .take_while(|c| is_name(*c))
                    .collect()
            } else {
                let after = rest.strip_prefix('[')?.trim_start();
                let quote = after.chars().next().filter(|c| *c == '\'' || *c == '"')?;
                after[1..].chars().take_while(|c| *c != quote).collect()
            };
            Some(name.to_ascii_uppercase())
        })
        .filter(|name| RELEASE_SECRETS.contains(&name.as_str()))
        .count()
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

fn callee(job: &Job) -> Option<String> {
    let called = job.uses.strip_prefix(CICD_WORKFLOWS)?.split('@').next()?;
    Some(if job.file == "nightly-gates.yml" {
        format!("{called} [dry-run]")
    } else {
        called.to_string()
    })
}

#[test]
fn mair_681_every_workflow_with_jobs_is_read() {
    for workflow in workflows() {
        let declares_jobs = workflow.live.lines().any(|line| {
            key_value(line).is_some_and(|(key, _)| key == "jobs") && !line.starts_with(' ')
        });
        assert!(
            !declares_jobs || !workflow.jobs.is_empty(),
            "{}: no job could be read",
            workflow.file
        );
    }
}

#[test]
fn mair_681_no_job_inherits_every_secret_or_lists_them_inline() {
    for job in jobs() {
        if let Secrets::Scalar(value) = &job.secrets {
            panic!(
                "{}: secrets: {value} (list them as a block mapping)",
                job.place
            );
        }
    }
}

#[test]
fn mair_681_each_cicd_call_passes_exactly_the_secrets_the_workflow_reads() {
    for job in jobs() {
        let Some(callee) = callee(&job) else { continue };
        let wanted = expected(&callee)
            .unwrap_or_else(|| panic!("{}: no expected secret list for {callee}", job.place));
        let mut keys: Vec<&str> = mapping(&job).iter().map(|(key, _)| key.as_str()).collect();
        keys.sort_unstable();
        assert_eq!(keys, wanted, "{}", job.place);
    }
}

#[test]
fn mair_681_each_secret_handed_over_is_a_direct_read_of_the_same_name() {
    for job in jobs() {
        let Some(callee) = callee(&job) else { continue };
        for (key, value) in mapping(&job) {
            let wanted =
                if callee == "promote-prod.yml [dry-run]" && key == "ANTHROPIC_REVIEW_API_KEY" {
                    DRY_RUN_REVIEW.to_string()
                } else {
                    format!("${{{{ secrets.{key} }}}}")
                };
            assert_eq!(value, &wanted, "{}: {key}", job.place);
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
