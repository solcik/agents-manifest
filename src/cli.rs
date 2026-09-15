use crate::{
    error::{Error, IoContext, Result},
    manifest::Manifest,
    plan::{Planner, Report},
    source::{GitMode, GitOptions, GitSource, default_cache},
    transaction::{ProjectLock, ProjectReadLock, Transaction},
};
use clap::{CommandFactory, Parser, Subcommand};
use std::{
    fs,
    io::{self, Write},
    path::PathBuf,
    time::Duration,
};

#[derive(Debug, Parser)]
#[command(
    name = "agent-skills",
    version,
    about = "Synchronise immutable project skills safely",
    arg_required_else_help = true
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
    /// Select the project directory.
    #[arg(long, global = true, default_value = ".")]
    pub project: PathBuf,
    /// Select the manifest relative to the project.
    #[arg(long, global = true, default_value = ".agents/skills.yaml")]
    pub manifest: PathBuf,
    /// Select the immutable source cache.
    #[arg(long, global = true, env = "AGENTS_MANIFEST_CACHE_DIR")]
    pub cache_dir: Option<PathBuf>,
    /// Require cached sources without network access.
    #[arg(long, global = true)]
    pub offline: bool,
    /// Limit parallel source workers.
    #[arg(short = 'j', long, global = true, default_value_t = 4, value_parser = clap::value_parser!(u16).range(1..=64))]
    pub jobs: u16,
    /// Limit each Git operation in seconds.
    #[arg(long, global = true, default_value_t = 30, value_parser = clap::value_parser!(u64).range(1..=3600))]
    pub timeout: u64,
    /// Select Git authentication. Auto detects the scoped host wrapper.
    #[arg(long, global = true, value_enum, default_value_t = GitMode::Auto)]
    pub git_mode: GitMode,
    /// Emit versioned JSON output.
    #[arg(long, global = true, conflicts_with = "quiet")]
    pub json: bool,
    /// Suppress successful output.
    #[arg(short, long, global = true)]
    pub quiet: bool,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Validate manifest policy without network access.
    Validate {
        /// Override the manifest path.
        manifest_path: Option<PathBuf>,
    },
    /// Resolve sources and report changes without project writes.
    Plan,
    /// Publish a validated plan with recoverable backups.
    Sync,
    /// Detect generated content or ownership drift.
    Check,
    /// Generate shell completions.
    Completions { shell: clap_complete::Shell },
}

impl Cli {
    pub fn run(&self) -> Result<()> {
        if let Command::Completions { shell } = self.command {
            let mut bytes = Vec::new();
            clap_complete::generate(shell, &mut Self::command(), "agent-skills", &mut bytes);
            return io::stdout()
                .lock()
                .write_all(&bytes)
                .context("write shell completions");
        }
        if !cfg!(unix) {
            return Err(Error::Invalid(
                "platform: version one supports Linux and macOS".into(),
            ));
        }
        let root = fs::canonicalize(&self.project).context("resolve project directory")?;
        if !root.is_dir() {
            return Err(Error::Invalid("project: require a directory".into()));
        }
        let path = match &self.command {
            Command::Validate {
                manifest_path: Some(path),
            } => root.join(path),
            _ => root.join(&self.manifest),
        };
        let input = Manifest::load(&path)?;
        if matches!(self.command, Command::Validate { .. }) {
            return self.write_validation();
        }
        let sync = matches!(self.command, Command::Sync);
        let lock = if sync {
            Some(ProjectLock::acquire(&root)?)
        } else {
            None
        };
        let _read_lock = if sync {
            None
        } else {
            ProjectReadLock::acquire(&root)?
        };
        let transaction = lock.as_ref().map(Transaction::new);
        if let Some(transaction) = &transaction {
            transaction.recover()?;
        }
        let source = GitSource::new(GitOptions {
            cache: self
                .cache_dir
                .clone()
                .map(Ok)
                .unwrap_or_else(default_cache)?,
            offline: self.offline,
            mode: self.git_mode,
            timeout: Duration::from_secs(self.timeout),
        });
        let check = matches!(self.command, Command::Check);
        let plan = Planner::new(&root, &input, &source)
            .jobs(usize::from(self.jobs))
            .check(check)
            .build()?;
        if let Some(transaction) = &transaction {
            transaction.apply(&plan)?;
        }
        self.write_report(&plan.report)?;
        if check && !plan.operations.is_empty() {
            return Err(Error::Drift(format!(
                "check: {} generated paths differ",
                plan.operations.len()
            )));
        }
        Ok(())
    }

    fn write_validation(&self) -> Result<()> {
        if self.quiet {
            return Ok(());
        }
        let mut stdout = io::stdout().lock();
        if self.json {
            writeln!(stdout, "{{\"version\":1,\"valid\":true}}")
        } else {
            writeln!(stdout, "Manifest validation passed.")
        }
        .context("write validation result")
    }

    fn write_report(&self, report: &Report) -> Result<()> {
        if self.quiet {
            return Ok(());
        }
        let mut stdout = io::BufWriter::new(io::stdout().lock());
        if self.json {
            let bytes = serde_json::to_vec(report)
                .map_err(|_| Error::Internal("cannot encode JSON report".into()))?;
            stdout.write_all(&bytes).context("write JSON report")?;
            writeln!(stdout).context("write JSON report separator")?;
        } else if report.changes.is_empty() {
            writeln!(stdout, "Project skills are up to date.").context("write plan summary")?;
        } else {
            for change in &report.changes {
                let action = match change.action {
                    crate::plan::Action::Create => "create",
                    crate::plan::Action::Update => "update",
                    crate::plan::Action::Remove => "remove",
                };
                writeln!(stdout, "{action} {}", change.path).context("write plan change")?;
            }
        }
        stdout.flush().context("flush plan report")
    }

    pub fn write_error(&self, error: &Error) -> Result<()> {
        let mut stderr = io::stderr().lock();
        if self.json {
            serde_json::to_writer(
                &mut stderr,
                &serde_json::json!({
                    "version": 1, "error": {"code": error.code(), "message": error.to_string()}
                }),
            )
            .map_err(|_| Error::Internal("cannot encode JSON error".into()))?;
            writeln!(stderr).context("write JSON error separator")
        } else {
            writeln!(stderr, "{error}").context("write CLI error")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_definition_is_valid() {
        Cli::command().debug_assert();
    }

    #[test]
    fn rejects_zero_workers_and_timeout() {
        assert!(Cli::try_parse_from(["agent-skills", "sync", "--jobs", "0"]).is_err());
        assert!(Cli::try_parse_from(["agent-skills", "sync", "--timeout", "0"]).is_err());
    }
}
