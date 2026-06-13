//! Command-line structure (clap) and target parsing.

use clap::{Args, Parser, Subcommand};

/// The Tork ORM command-line tool.
#[derive(Parser)]
#[command(name = "tork-orm", about = "Tork ORM command-line tool", version)]
pub struct Cli {
    /// The top-level command.
    #[command(subcommand)]
    pub command: TopCommand,

    /// Options shared by every command.
    #[command(flatten)]
    pub global: GlobalArgs,
}

/// Top-level command groups.
#[derive(Subcommand)]
pub enum TopCommand {
    /// Database migrations.
    #[command(subcommand)]
    Migrate(MigrateCommand),
}

/// Migration subcommands.
#[derive(Subcommand)]
pub enum MigrateCommand {
    /// Apply pending migrations (optionally up to a revision; `head` = all).
    Up {
        /// A revision (or unique prefix), or `head` / nothing for all.
        target: Option<String>,
    },
    /// Revert migrations (`<n>` steps, `base` = all, or a `<revision>`).
    Down {
        /// A step count, `base`, or a revision (or unique prefix).
        target: Option<String>,
    },
    /// Show the applied state of each migration.
    Status,
    /// Revert the most recent migration and re-apply pending.
    Redo,
    /// Scaffold a new migration file.
    Create {
        /// A short name for the migration (e.g. `add_orders`).
        name: String,
    },
    /// Create the migrations directory.
    Init,
}

/// Options available on every command.
#[derive(Args, Clone)]
pub struct GlobalArgs {
    /// The database URL (also read from `DATABASE_URL`, then `DB_URL`).
    #[arg(long, short = 'd', global = true, env = "DATABASE_URL")]
    pub database_url: Option<String>,

    /// The migrations directory (default `migrations`).
    #[arg(long, global = true)]
    pub dir: Option<String>,

    /// The bookkeeping table name (default `_tork_migrations`).
    #[arg(long, global = true)]
    pub table: Option<String>,

    /// Skip confirmation prompts.
    #[arg(long, short = 'y', global = true)]
    pub yes: bool,

    /// Proceed despite a checksum mismatch on an already-applied migration
    /// (default: abort). For development only; never use in production.
    #[arg(long, global = true)]
    pub allow_checksum_mismatch: bool,

    /// Allow destructive statements (`DROP TABLE`, `DROP COLUMN`) in
    /// migrations (default: abort). Required when applying a migration
    /// that destroys data; production deploys should never set this.
    #[arg(long, global = true)]
    pub allow_destructive: bool,

    /// Disable colored output.
    #[arg(long, global = true)]
    pub no_color: bool,
}

/// Where `up` should stop.
#[derive(Debug, PartialEq, Eq)]
pub enum UpTarget {
    /// Apply all pending migrations.
    Head,
    /// Apply through the given revision (or prefix).
    To(String),
}

/// What `down` should revert.
#[derive(Debug, PartialEq, Eq)]
pub enum DownTarget {
    /// Revert the given number of migrations.
    Steps(usize),
    /// Revert every applied migration.
    Base,
    /// Revert everything after the given revision (or prefix).
    To(String),
}

/// Parses an `up` target argument.
pub fn parse_up_target(arg: Option<&str>) -> UpTarget {
    match arg {
        None | Some("head") => UpTarget::Head,
        Some(revision) => UpTarget::To(revision.to_string()),
    }
}

/// Parses a `down` target argument.
pub fn parse_down_target(arg: Option<&str>) -> DownTarget {
    match arg {
        None => DownTarget::Steps(1),
        Some("base") => DownTarget::Base,
        Some(other) => match other.parse::<usize>() {
            Ok(steps) => DownTarget::Steps(steps),
            Err(_) => DownTarget::To(other.to_string()),
        },
    }
}
