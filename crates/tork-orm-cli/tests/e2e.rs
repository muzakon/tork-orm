//! End-to-end tests that run the built `tork-orm` binary against a temporary
//! SQLite file, asserting exit codes and the plain (color-off) summaries.

use std::path::Path;
use std::process::Command;

use tempfile::tempdir;

/// A `tork-orm` command with color disabled and the ambient database env cleared.
fn tork_orm() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_tork-orm"));
    command
        .env("NO_COLOR", "1")
        .env_remove("DATABASE_URL")
        .env_remove("DB_URL");
    command
}

fn write_migration(dir: &Path, revision: &str, down: &str, name: &str, up: &str, down_sql: &str) {
    let content = format!(
        "-- revision: {revision}\n-- down_revision: {down}\n-- migrate:up\n{up}\n-- migrate:down\n{down_sql}\n"
    );
    std::fs::write(dir.join(format!("{revision}_{name}.sql")), content).unwrap();
}

#[test]
fn full_lifecycle_through_the_binary() {
    let temp = tempdir().unwrap();
    let migrations = temp.path().join("migrations");
    std::fs::create_dir_all(&migrations).unwrap();
    let dir = migrations.to_str().unwrap();
    let db_url = format!("sqlite://{}/app.db", temp.path().display());

    write_migration(
        &migrations,
        "aaaa11112222",
        "",
        "create_users",
        "CREATE TABLE users (id INTEGER PRIMARY KEY);",
        "DROP TABLE users;",
    );
    write_migration(
        &migrations,
        "bbbb33334444",
        "aaaa11112222",
        "create_posts",
        "CREATE TABLE posts (id INTEGER PRIMARY KEY);",
        "DROP TABLE posts;",
    );

    // up applies both.
    let up = tork_orm()
        .args(["migrate", "-d", &db_url, "--dir", dir, "up"])
        .output()
        .unwrap();
    assert!(up.status.success());
    assert!(String::from_utf8_lossy(&up.stdout).contains("Applied 2 migrations"));

    // status reports both applied.
    let status = tork_orm()
        .args(["migrate", "-d", &db_url, "--dir", dir, "status"])
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&status.stdout).contains("2 applied"));

    // down 1 reverts the head.
    let down = tork_orm()
        .args(["migrate", "-d", &db_url, "--dir", dir, "-y", "down", "1"])
        .output()
        .unwrap();
    assert!(down.status.success());
    assert!(String::from_utf8_lossy(&down.stdout).contains("Reverted 1 migration"));

    // status now shows one pending.
    let status = tork_orm()
        .args(["migrate", "-d", &db_url, "--dir", dir, "status"])
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&status.stdout).contains("1 applied · 1 pending"));
}

#[test]
fn missing_database_url_fails_cleanly() {
    let temp = tempdir().unwrap();
    let dir = temp.path().to_str().unwrap();
    let output = tork_orm()
        .args(["migrate", "--dir", dir, "up"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("database URL"));
}

#[test]
fn create_scaffolds_a_chained_migration() {
    let temp = tempdir().unwrap();
    let dir = temp.path().join("migrations");
    let dir_str = dir.to_str().unwrap();

    assert!(tork_orm()
        .args(["migrate", "--dir", dir_str, "create", "create_users"])
        .status()
        .unwrap()
        .success());
    assert!(tork_orm()
        .args(["migrate", "--dir", dir_str, "create", "add_posts"])
        .status()
        .unwrap()
        .success());

    let files: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    assert_eq!(files.len(), 2);

    // The second migration's down_revision points at the first's revision.
    let first = files.iter().find(|f| f.contains("create_users")).unwrap();
    let first_revision = first.split('_').next().unwrap();
    let second = files.iter().find(|f| f.contains("add_posts")).unwrap();
    let second_content = std::fs::read_to_string(dir.join(second)).unwrap();
    assert!(second_content.contains(&format!("-- down_revision: {first_revision}")));
}
