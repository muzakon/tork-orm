//! End-to-end tests for `migrate generate`: diffing models against a live DB and
//! writing the reconciling migration.

use tork_orm::migration::generate::{generate, write_migration, SchemaChange};
use tork_orm::migration::introspect::existing_indexes;
use tork_orm::prelude::*;

#[derive(Debug, Clone, Model)]
#[table(name = "widgets")]
struct Widget {
    #[field(primary_key, auto)]
    id: i64,
    #[field(index)]
    name: String,
}

#[derive(Debug, Clone, Model)]
#[table(name = "gadgets")]
struct Gadget {
    #[field(primary_key, auto)]
    id: i64,
    #[field(unique)]
    code: String,
}

async fn seed_widgets(db: &Database) {
    db.execute(
        "CREATE TABLE widgets (id INTEGER PRIMARY KEY, name TEXT NOT NULL)".into(),
        vec![],
    )
    .await
    .unwrap();
    // An index the model no longer declares.
    db.execute(
        "CREATE INDEX \"widgets_old_idx\" ON \"widgets\" (\"name\")".into(),
        vec![],
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn reconciles_indexes_on_existing_table() {
    let db = Database::connect(":memory:", 1).await.unwrap();
    seed_widgets(&db).await;

    let change = generate(&db, &[Widget::table_schema()]).await.unwrap();
    assert!(change
        .up
        .iter()
        .any(|s| s == "CREATE INDEX \"widgets_name_idx\" ON \"widgets\" (\"name\")"));
    assert!(change
        .up
        .iter()
        .any(|s| s == "DROP INDEX IF EXISTS \"widgets_old_idx\""));
    assert!(change.down.iter().any(|s| s.contains("widgets_old_idx")));

    // Applying the up statements converges the DB to the model's indexes.
    for statement in &change.up {
        db.execute(statement.clone(), vec![]).await.unwrap();
    }
    let names: Vec<String> = existing_indexes(&db, "widgets")
        .await
        .unwrap()
        .into_iter()
        .map(|index| index.name)
        .collect();
    assert_eq!(names, vec!["widgets_name_idx".to_string()]);

    // A second generate is a no-op now that they match.
    let again = generate(&db, &[Widget::table_schema()]).await.unwrap();
    assert!(again.is_empty());
}

#[tokio::test]
async fn creates_a_missing_table_with_indexes() {
    let db = Database::connect(":memory:", 1).await.unwrap();
    let change = generate(&db, &[Gadget::table_schema()]).await.unwrap();

    assert!(change.up.iter().any(|s| s.starts_with("CREATE TABLE \"gadgets\"")));
    assert!(change
        .up
        .iter()
        .any(|s| s == "CREATE UNIQUE INDEX \"gadgets_code_key\" ON \"gadgets\" (\"code\")"));
    assert!(change.down.iter().any(|s| s.contains("DROP TABLE")));

    for statement in &change.up {
        db.execute(statement.clone(), vec![]).await.unwrap();
    }
    // The table now exists and a re-run is a no-op.
    assert!(generate(&db, &[Gadget::table_schema()]).await.unwrap().is_empty());
}

#[test]
fn write_migration_emits_a_valid_revision_file() {
    let dir = tempfile::tempdir().unwrap();
    let change = SchemaChange {
        up: vec!["CREATE INDEX \"a_idx\" ON \"a\" (\"x\")".to_string()],
        down: vec!["DROP INDEX IF EXISTS \"a_idx\"".to_string()],
    };
    let path = write_migration(dir.path(), "add indexes", &change)
        .unwrap()
        .expect("a file should be written");
    let body = std::fs::read_to_string(&path).unwrap();
    assert!(body.contains("-- revision: "));
    assert!(body.contains("-- migrate:up"));
    assert!(body.contains("CREATE INDEX \"a_idx\" ON \"a\" (\"x\");"));
    assert!(body.contains("-- migrate:down"));
    assert!(path.file_name().unwrap().to_string_lossy().ends_with("_add_indexes.sql"));

    // An empty change writes nothing.
    let empty = write_migration(dir.path(), "noop", &SchemaChange::default()).unwrap();
    assert!(empty.is_none());
}
