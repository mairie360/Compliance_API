//! `compliance-backup` (MAIR-500): seals a throwaway copy of the database before it is backed up,
//! and unseals a restored database. Run by the backup Job of Devops/Deploiment, never on the live
//! database.
//!
//!   compliance-backup seal   <database url> <inventory.yaml> <out dir>
//!   compliance-backup unseal <database url> <in dir>
//!
//! `<database url>` may be `env`: the libpq variables (`PGHOST`, `PGPORT`, `PGUSER`, `PGPASSWORD`,
//! `PGDATABASE`) are read instead, so that a password never goes through a URL or a command line.
//!
//! The key manager comes from `SCW_SECRET_KEY`, `SCW_DEFAULT_PROJECT_ID`, `SCW_REGION` (see
//! `backup::keys`). Exit 0 / 1 (failed: the backup must not be shipped) / 2 (usage).

use compliance_api::backup::keys::ScalewayKeyManager;
use compliance_api::backup::{plan, seal};
use std::path::Path;
use std::process::ExitCode;

async fn connect(url: &str) -> Result<sqlx::PgPool, String> {
    let result = if url == "env" {
        sqlx::PgPool::connect_with(sqlx::postgres::PgConnectOptions::new()).await
    } else {
        sqlx::PgPool::connect(url).await
    };
    result.map_err(|_| "cannot connect to the database".to_owned())
}

async fn run(args: &[String]) -> Result<String, String> {
    let var = |name: &str| std::env::var(name).ok().filter(|v| !v.trim().is_empty());
    let keys = ScalewayKeyManager::from_env(&var)
        .ok_or("the key manager is not configured (SCW_SECRET_KEY, SCW_DEFAULT_PROJECT_ID)")?;
    match args {
        [command, url, inventory, out] if command == "seal" => {
            let pool = sqlx::PgPool::connect(url)
                .await
                .map_err(|_| "cannot connect to the copy".to_owned())?;
            let inventory = std::fs::read_to_string(inventory)
                .map_err(|e| format!("cannot read the inventory: {e}"))?;
            let plans = plan::plan(&inventory, &seal::referenced_tables(&pool).await?)?;
            let report = seal::seal(&pool, &plans, &keys, Path::new(out)).await?;
            Ok(format!(
                "sealed {} of {} users ({} tables)",
                report.sealed_users,
                report.users,
                report.tables.len()
            ))
        }
        [command, url, input] if command == "unseal" => {
            let pool = sqlx::PgPool::connect(url)
                .await
                .map_err(|_| "cannot connect to the restored database".to_owned())?;
            let report = seal::unseal(&pool, &keys, Path::new(input)).await?;
            Ok(format!(
                "restored {} users, {} left sealed (erased)",
                report.restored, report.left_sealed
            ))
        }
        _ => Err("usage".to_owned()),
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args).await {
        Ok(summary) => {
            println!("{summary}");
            ExitCode::SUCCESS
        }
        Err(reason) if reason == "usage" => {
            eprintln!("usage: compliance-backup seal <database url> <inventory.yaml> <out dir> | unseal <database url> <in dir>");
            ExitCode::from(2)
        }
        Err(reason) => {
            eprintln!("compliance-backup: {reason}");
            ExitCode::FAILURE
        }
    }
}
