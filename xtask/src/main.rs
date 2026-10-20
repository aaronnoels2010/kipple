// SPDX-License-Identifier: GPL-3.0-or-later
//! Kipple repository tooling: `cargo xtask <command>`.
//!
//!   cargo xtask layering               crates only depend inward (ARCHITECTURE §2)
//!   cargo xtask clippy-config          the generated clippy.toml files are up to date
//!   cargo xtask clippy-config --write  regenerate them from xtask/src/clippy_config.rs

mod clippy_config;
mod layering;

use std::path::Path;
use std::process::{Command, ExitCode};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("layering") => layering_cmd(),
        Some("clippy-config") => clippy_config_cmd(args.get(1).is_some_and(|a| a == "--write")),
        _ => {
            eprintln!("usage: cargo xtask <layering | clippy-config [--write]>");
            ExitCode::from(2)
        }
    }
}

fn report(what: &str, problems: &[String]) -> ExitCode {
    if problems.is_empty() {
        println!("{what}: OK");
        return ExitCode::SUCCESS;
    }
    for p in problems {
        eprintln!("{what}: {p}");
    }
    ExitCode::FAILURE
}

fn layering_cmd() -> ExitCode {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let out = match Command::new(cargo)
        .args(["metadata", "--format-version", "1", "--no-deps"])
        .output()
    {
        Ok(o) if o.status.success() => o,
        Ok(o) => {
            return report(
                "layering",
                &[String::from_utf8_lossy(&o.stderr).into_owned()],
            );
        }
        Err(e) => return report("layering", &[format!("could not run cargo metadata: {e}")]),
    };
    let graph = match layering::from_metadata(&String::from_utf8_lossy(&out.stdout)) {
        Ok(g) => g,
        Err(e) => return report("layering", &[e]),
    };
    let problems: Vec<String> = layering::check(&graph, &layering::kipple_rules())
        .iter()
        .map(ToString::to_string)
        .collect();
    report("layering", &problems)
}

fn clippy_config_cmd(write: bool) -> ExitCode {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut problems = Vec::new();
    for (rel, expected) in clippy_config::files() {
        let path = repo.join(rel);
        if write {
            if let Err(e) = write_file(&path, &expected) {
                problems.push(format!("could not write {rel}: {e}"));
            }
            continue;
        }
        match std::fs::read_to_string(&path) {
            Ok(actual) if actual.replace("\r\n", "\n") == expected => {}
            Ok(_) => problems.push(format!(
                "{rel} is out of date; run `cargo xtask clippy-config --write`"
            )),
            Err(e) => problems.push(format!("could not read {rel}: {e}")),
        }
    }
    report("clippy-config", &problems)
}

/// Repo tooling, not app code: ADR-0002's ban on file mutation doesn't apply here.
#[allow(clippy::disallowed_methods)]
fn write_file(path: &Path, content: &str) -> std::io::Result<()> {
    std::fs::write(path, content)
}
