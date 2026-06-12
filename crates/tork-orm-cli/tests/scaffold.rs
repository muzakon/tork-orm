//! Tests for the migration scaffolding helpers (pure, no filesystem).

use tork_orm_cli::scaffold::{
    new_revision, render_file_name, snake_case, template, DateParts,
};

#[test]
fn snake_case_normalizes_names() {
    assert_eq!(snake_case("Add Orders!"), "add_orders");
    assert_eq!(snake_case("create-users"), "create_users");
    assert_eq!(snake_case("  spaced  out  "), "spaced_out");
}

#[test]
fn revisions_are_unique_12_char_hex() {
    let revision = new_revision();
    assert_eq!(revision.len(), 12);
    assert!(revision.chars().all(|c| c.is_ascii_hexdigit()));
    assert_ne!(new_revision(), new_revision());
}

#[test]
fn default_template_combines_revision_and_name() {
    let date = DateParts {
        year: "2026".into(),
        month: "06".into(),
        day: "12".into(),
        hour: "00".into(),
        minute: "00".into(),
        second: "00".into(),
    };
    assert_eq!(
        render_file_name("{rev}_{slug}", "abc123def456", "add_orders", &date),
        "abc123def456_add_orders.sql"
    );
}

#[test]
fn template_has_headers_and_markers() {
    let content = template("abc123", Some("parent99"), "create_users");
    assert!(content.contains("-- revision: abc123"));
    assert!(content
        .lines()
        .any(|line| line.trim() == "-- down_revision: parent99"));
    assert!(content.contains("-- migrate:up"));
    assert!(content.contains("-- migrate:down"));
}

#[test]
fn base_migration_has_an_empty_down_revision() {
    let content = template("abc123", None, "create_users");
    assert!(content
        .lines()
        .any(|line| line.trim() == "-- down_revision:"));
}
