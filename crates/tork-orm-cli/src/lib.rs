//! The Tork ORM command-line runner.
//!
//! A small library behind the `tork-orm` binary. It parses arguments, connects to
//! the database, drives the SQL-file [`FileMigrator`](tork_orm::migration::FileMigrator),
//! and renders concise, colored output. The same `run` entry point can be reused by
//! the framework's CLI to provide `tork migrate ...`.
//!
//! Because migrations are plain `.sql` files, the binary needs no project source and
//! no compilation — just the binary, a `migrations/` directory, and a database URL:
//!
//! ```text
//! export DATABASE_URL=sqlite://app.db
//! tork-orm migrate init                 # create the migrations directory
//! tork-orm migrate create add_users     # scaffold a new migration
//! tork-orm migrate up                   # apply all pending (also: up <revision>)
//! tork-orm migrate status               # show applied / pending
//! tork-orm migrate down                 # revert one (also: down <n> | base | <revision>)
//! ```
//!
//! Each migration `.sql` carries its identity and order in headers, so files can be
//! renamed freely:
//!
//! ```sql
//! -- revision: 1975ea83b712
//! -- down_revision: a3f9c1d4e8b2
//! -- migrate:up
//! CREATE TABLE "users" ("id" INTEGER PRIMARY KEY AUTOINCREMENT);
//! -- migrate:down
//! DROP TABLE "users";
//! ```
#![forbid(unsafe_code)]

pub mod cli;
mod commands;
mod config;
mod output;
pub mod scaffold;
pub mod style;

use std::ffi::OsString;
use std::process::ExitCode;

use clap::Parser;
use tork_orm::migration::{FileMigrator, OnMismatch};
use tork_orm::{Database, OrmError};

use cli::{parse_down_target, parse_up_target, Cli, MigrateCommand, TopCommand};
use config::Config;

pub use style::{sym, Style};

/// Parses `args`, runs the requested command, and returns a process exit code.
///
/// Intended to be called from a binary's `main`, e.g.
/// `std::process::exit(tork_orm_cli::run(std::env::args_os()) as i32)` — or simply
/// returned from a `fn main() -> ExitCode`.
pub fn run<I, T>(args: I) -> ExitCode
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    let cli = Cli::parse_from(args);
    let style = Style::detect(cli.global.no_color);

    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("failed to start the async runtime: {error}");
            return ExitCode::FAILURE;
        }
    };

    match runtime.block_on(dispatch(&cli, &style)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            output::error(&style, &error);
            ExitCode::FAILURE
        }
    }
}

/// Resolves configuration, opens the database, and runs the chosen command.
async fn dispatch(cli: &Cli, style: &Style) -> Result<(), OrmError> {
    let TopCommand::Migrate(command) = &cli.command;
    run_migrate(command, &cli.global, style).await
}

/// Runs a single `migrate` subcommand against the resolved project configuration.
///
/// This is the reusable entry point the unified `tork` CLI calls to provide
/// `tork migrate ...` with the exact same behavior and colored output as the
/// standalone `tork-orm` binary.
pub async fn run_migrate(
    command: &MigrateCommand,
    global: &cli::GlobalArgs,
    style: &Style,
) -> Result<(), OrmError> {
    let config = Config::resolve(global);

    // Scaffolding commands touch only the filesystem — no database needed.
    match command {
        MigrateCommand::Create { name } => {
            return commands::create::run(style, &config.dir, &config.migrations, name)
        }
        MigrateCommand::Init => return commands::init::run(style, &config.dir, global.yes),
        _ => {}
    }

    let database = Database::connect(config.require_database_url()?, 1).await?;
    let mut migrator = FileMigrator::new(database, &config.dir).table(&config.table);
    if global.allow_checksum_mismatch {
        migrator = migrator.on_checksum_mismatch(OnMismatch::Warn);
    }
    match command {
        MigrateCommand::Status => commands::status::run(&migrator, style, &config.dir).await,
        MigrateCommand::Up { target } => {
            commands::up::run(&migrator, style, parse_up_target(target.as_deref())).await
        }
        MigrateCommand::Down { target } => {
            commands::down::run(
                &migrator,
                style,
                parse_down_target(target.as_deref()),
                global.yes,
            )
            .await
        }
        MigrateCommand::Redo => commands::redo::run(&migrator, style).await,
        MigrateCommand::Create { .. } | MigrateCommand::Init => unreachable!(),
    }
}
