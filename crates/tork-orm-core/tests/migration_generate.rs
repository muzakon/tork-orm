//! Tests for column-level schema diffing in `migrate generate`.

#![cfg(feature = "migrations")]

use tork_orm_core::migration::generate::{generate, SchemaChange};
use tork_orm_core::migration::introspect::existing_columns;
use tork_orm_core::{ColumnDef, Database, SqlType};
use tork_orm_core::registry::TableSchema;

fn schema(table: &'static str, columns: Vec<ColumnDef>) -> TableSchema {
    TableSchema { table, columns, indexes: vec![], checks: vec![] }
}

fn col(name: &'static str, sql_type: SqlType, nullable: bool) -> ColumnDef {
    ColumnDef {
        name,
        sql_type,
        primary_key: false,
        auto: false,
        nullable,
        foreign_key: None,
        default: None,
    }
}

fn pk_col(name: &'static str) -> ColumnDef {
    ColumnDef {
        name,
        sql_type: SqlType::Integer,
        primary_key: true,
        auto: true,
        nullable: false,
        foreign_key: None,
        default: None,
    }
}

async fn setup(sql: &str) -> Database {
    let db = Database::connect(":memory:", 1).await.unwrap();
    db.execute(sql.to_string(), vec![]).await.unwrap();
    db
}

fn has_stmt(change: &SchemaChange, needle: &str) -> bool {
    change.up.iter().any(|s| s.contains(needle))
}

fn has_down(change: &SchemaChange, needle: &str) -> bool {
    change.down.iter().any(|s| s.contains(needle))
}

// ── ADD COLUMN ──────────────────────────────────────────────────────────────

#[tokio::test]
async fn adds_column_missing_from_db() {
    let db = setup("CREATE TABLE t (id INTEGER PRIMARY KEY, name TEXT NOT NULL)").await;

    let models = schema(
        "t",
        vec![
            pk_col("id"),
            col("name", SqlType::Text, false),
            col("score", SqlType::Real, true),
        ],
    );
    let change = generate(&db, &[models]).await.unwrap();

    assert!(!change.is_empty());
    assert!(has_stmt(&change, "ADD COLUMN"), "expected ADD COLUMN in up: {:?}", change.up);
    assert!(has_stmt(&change, "\"score\""), "expected score column: {:?}", change.up);
}

#[tokio::test]
async fn add_column_is_idempotent() {
    // After applying the up migration the diff should be empty.
    let db = setup("CREATE TABLE t (id INTEGER PRIMARY KEY, name TEXT NOT NULL)").await;
    let models = schema(
        "t",
        vec![
            pk_col("id"),
            col("name", SqlType::Text, false),
            col("score", SqlType::Real, true),
        ],
    );
    let change = generate(&db, &[models.clone()]).await.unwrap();
    for stmt in &change.up {
        if !stmt.trim_start().starts_with("--") {
            db.execute(stmt.clone(), vec![]).await.unwrap();
        }
    }
    let again = generate(&db, &[models]).await.unwrap();
    assert!(again.is_empty(), "expected no-op after applying: {:?}", again.up);
}

#[tokio::test]
async fn add_column_down_drops_the_column() {
    let db = setup("CREATE TABLE t (id INTEGER PRIMARY KEY, name TEXT NOT NULL)").await;
    let models = schema(
        "t",
        vec![pk_col("id"), col("name", SqlType::Text, false), col("score", SqlType::Real, true)],
    );
    let change = generate(&db, &[models]).await.unwrap();
    assert!(has_down(&change, "DROP COLUMN"), "expected DROP COLUMN in down: {:?}", change.down);
    assert!(has_down(&change, "\"score\""), "expected score in down: {:?}", change.down);
}

#[tokio::test]
async fn not_null_column_added_as_nullable_with_note() {
    let db = setup("CREATE TABLE t (id INTEGER PRIMARY KEY)").await;
    let models = schema(
        "t",
        vec![pk_col("id"), col("required", SqlType::Text, false)],
    );
    let change = generate(&db, &[models]).await.unwrap();

    // The ADD COLUMN must be present.
    assert!(has_stmt(&change, "ADD COLUMN"));
    // A NOTE comment must explain the nullable-downgrade.
    assert!(
        change.up.iter().any(|s| s.starts_with("-- NOTE:") && s.contains("nullable")),
        "expected nullable NOTE: {:?}",
        change.up
    );
    // Executing the migration should not error even on a non-empty table.
    db.execute(
        "INSERT INTO t (id) VALUES (1)".to_string(),
        vec![],
    )
    .await
    .unwrap();
    for stmt in &change.up {
        if !stmt.trim_start().starts_with("--") {
            db.execute(stmt.clone(), vec![]).await.unwrap();
        }
    }
}

// ── DROP COLUMN ─────────────────────────────────────────────────────────────

#[tokio::test]
async fn drops_column_removed_from_model() {
    let db = setup(
        "CREATE TABLE t (id INTEGER PRIMARY KEY, name TEXT NOT NULL, obsolete TEXT)",
    )
    .await;
    let models = schema(
        "t",
        vec![pk_col("id"), col("name", SqlType::Text, false)],
    );
    let change = generate(&db, &[models]).await.unwrap();

    assert!(!change.is_empty());
    assert!(has_stmt(&change, "DROP COLUMN"), "expected DROP COLUMN: {:?}", change.up);
    assert!(has_stmt(&change, "\"obsolete\""), "expected obsolete col: {:?}", change.up);
}

#[tokio::test]
async fn drop_column_down_restores_the_column() {
    let db = setup(
        "CREATE TABLE t (id INTEGER PRIMARY KEY, name TEXT NOT NULL, obsolete TEXT)",
    )
    .await;
    let models = schema("t", vec![pk_col("id"), col("name", SqlType::Text, false)]);
    let change = generate(&db, &[models]).await.unwrap();

    assert!(has_down(&change, "ADD COLUMN"), "expected ADD COLUMN in down: {:?}", change.down);
    assert!(has_down(&change, "\"obsolete\""));
}

#[tokio::test]
async fn drop_column_is_idempotent() {
    let db = setup(
        "CREATE TABLE t (id INTEGER PRIMARY KEY, name TEXT NOT NULL, obsolete TEXT)",
    )
    .await;
    let models = schema("t", vec![pk_col("id"), col("name", SqlType::Text, false)]);
    let change = generate(&db, &[models.clone()]).await.unwrap();

    for stmt in &change.up {
        if !stmt.trim_start().starts_with("--") {
            db.execute(stmt.clone(), vec![]).await.unwrap();
        }
    }
    let cols = existing_columns(&db, "t").await.unwrap();
    assert!(cols.iter().all(|c| c.name != "obsolete"), "obsolete should be gone");

    let again = generate(&db, &[models]).await.unwrap();
    assert!(again.is_empty(), "expected no-op: {:?}", again.up);
}

#[tokio::test]
async fn primary_key_removal_emits_note_not_drop() {
    // PK columns cannot be dropped automatically; the diff should emit a NOTE.
    let db = setup("CREATE TABLE t (id INTEGER PRIMARY KEY, name TEXT)").await;
    // Model that removed the PK column (unlikely in practice but must not panic).
    let models = schema("t", vec![col("name", SqlType::Text, true)]);
    let change = generate(&db, &[models]).await.unwrap();

    // The diff is empty (only comments) because there is nothing executable to do.
    assert!(
        change.is_empty(),
        "pk removal should produce no executable statements: {:?}",
        change.up
    );
    assert!(
        change.up.iter().any(|s| s.starts_with("-- NOTE:")),
        "expected a NOTE for pk column: {:?}",
        change.up
    );
}

// ── TYPE MISMATCH NOTE ───────────────────────────────────────────────────────

#[tokio::test]
async fn type_mismatch_emits_note_but_no_executable_statement() {
    // The model says BIGINT but the DB has INTEGER.
    let db = setup("CREATE TABLE t (id INTEGER PRIMARY KEY, count INTEGER NOT NULL)").await;
    let models = schema(
        "t",
        vec![pk_col("id"), col("count", SqlType::BigInt, false)],
    );
    let change = generate(&db, &[models]).await.unwrap();

    // Only a NOTE comment; no executable ALTER TABLE.
    assert!(
        change.is_empty(),
        "type mismatch should have no executable statements: {:?}",
        change.up
    );
    assert!(
        change.up.iter().any(|s| s.starts_with("-- NOTE:")),
        "expected a NOTE for type mismatch: {:?}",
        change.up
    );
}

// ── MATCHING SCHEMA ──────────────────────────────────────────────────────────

#[tokio::test]
async fn no_diff_when_schema_matches() {
    let db = setup(
        "CREATE TABLE t (id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL, bio TEXT)",
    )
    .await;
    let models = schema(
        "t",
        vec![
            pk_col("id"),
            col("name", SqlType::Text, false),
            col("bio", SqlType::Text, true),
        ],
    );
    let change = generate(&db, &[models]).await.unwrap();
    assert!(change.is_empty(), "expected no diff: {:?}", change.up);
}
