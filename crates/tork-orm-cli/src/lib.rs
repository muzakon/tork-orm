//! The Tork ORM command-line runner.
//!
//! A small library behind the `tork-orm` binary. It parses arguments, connects to
//! the database, drives the SQL-file [`FileMigrator`](tork_orm::migration::FileMigrator),
//! and renders concise, colored output. The same `run` entry point can be reused by
//! the framework's CLI to provide `tork migrate ...`.
#![forbid(unsafe_code)]

pub mod cli;
mod commands;
mod config;
mod output;
mod style;

use std::ffi::OsString;
use std::process::ExitCode;

use clap::Parser;
use tork_orm::migration::FileMigrator;
use tork_orm::{Database, OrmError};

use cli::{parse_down_target, parse_up_target, Cli, MigrateCommand, TopCommand};
use config::Config;
use style::Style;

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
    let config = Config::resolve(&cli.global);

    match &cli.command {
        TopCommand::Migrate(command) => {
            let database = Database::connect(config.require_database_url()?, 1).await?;
            let migrator = FileMigrator::new(database, &config.dir).table(&config.table);

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
                        cli.global.yes,
                    )
                    .await
                }
                MigrateCommand::Redo => commands::redo::run(&migrator, style).await,
            }
        }
    }
}
