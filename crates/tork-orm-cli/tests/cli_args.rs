//! Tests for argument parsing and target resolution (pure, no database).

use clap::Parser;
use tork_orm_cli::cli::{
    parse_down_target, parse_up_target, Cli, DownTarget, MigrateCommand, TopCommand, UpTarget,
};

#[test]
fn up_targets_parse() {
    assert_eq!(parse_up_target(None), UpTarget::Head);
    assert_eq!(parse_up_target(Some("head")), UpTarget::Head);
    assert_eq!(parse_up_target(Some("abc123")), UpTarget::To("abc123".into()));
}

#[test]
fn down_targets_parse() {
    assert_eq!(parse_down_target(None), DownTarget::Steps(1));
    assert_eq!(parse_down_target(Some("3")), DownTarget::Steps(3));
    assert_eq!(parse_down_target(Some("base")), DownTarget::Base);
    assert_eq!(
        parse_down_target(Some("abc123")),
        DownTarget::To("abc123".into())
    );
}

#[test]
fn parses_migrate_up_with_global_flags() {
    let cli = Cli::try_parse_from([
        "tork-orm",
        "migrate",
        "--dir",
        "db/migrations",
        "--table",
        "_schema",
        "up",
        "head",
    ])
    .unwrap();

    assert_eq!(cli.global.dir.as_deref(), Some("db/migrations"));
    assert_eq!(cli.global.table.as_deref(), Some("_schema"));
    match cli.command {
        TopCommand::Migrate(MigrateCommand::Up { target }) => {
            assert_eq!(target.as_deref(), Some("head"));
        }
        _ => panic!("expected migrate up"),
    }
}

#[test]
fn parses_migrate_down_with_steps() {
    let cli = Cli::try_parse_from(["tork-orm", "migrate", "down", "2"]).unwrap();
    match cli.command {
        TopCommand::Migrate(MigrateCommand::Down { target }) => {
            assert_eq!(parse_down_target(target.as_deref()), DownTarget::Steps(2));
        }
        _ => panic!("expected migrate down"),
    }
}

#[test]
fn status_needs_no_target() {
    let cli = Cli::try_parse_from(["tork-orm", "migrate", "status"]).unwrap();
    assert!(matches!(
        cli.command,
        TopCommand::Migrate(MigrateCommand::Status)
    ));
}

#[test]
fn an_unknown_subcommand_is_an_error() {
    assert!(Cli::try_parse_from(["tork-orm", "migrate", "frobnicate"]).is_err());
}
