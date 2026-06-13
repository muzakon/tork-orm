//! Tests for migration checksums and status: an already-applied migration whose
//! definition changed is detected, and status reports per-migration state.

use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;
use tork_orm_core::migration::*;
use tork_orm_core::{Database, Result};

/// A `users` table whose `username` length is parameterized, so two versions with
/// the same revision render different DDL (and thus different checksums).
struct CreateUsers {
    username_length: u32,
}

impl MigrationTrait for CreateUsers {
    fn revision(&self) -> &'static str {
        "20260611_000001"
    }

    fn name(&self) -> &'static str {
        "create_users"
    }

    fn up<'a>(&'a self, schema: &'a mut SchemaManager<'_>) -> BoxFuture<'a, Result<()>> {
        let length = self.username_length;
        Box::pin(async move {
            schema
                .create_table("users")
                .column(Column::new("id").bigint().primary_key().auto_increment())
                .column(Column::new("username").varchar(length).not_null())
                .execute()
                .await?;
            Ok(())
        })
    }

    fn down<'a>(&'a self, schema: &'a mut SchemaManager<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            schema.drop_table("users").execute().await?;
            Ok(())
        })
    }
}

fn set(username_length: u32) -> MigrationSet {
    MigrationSet::new(vec![boxed(CreateUsers { username_length })])
}

#[tokio::test]
async fn rerun_with_same_definition_is_a_noop() {
    let db = Database::connect(":memory:", 1).await.unwrap();
    assert_eq!(Migrator::new(&db, set(50)).up().await.unwrap(), 1);
    // Same definition → same checksum → nothing to do, no warning path taken.
    assert_eq!(Migrator::new(&db, set(50)).up().await.unwrap(), 0);
}

#[tokio::test]
async fn status_reports_applied_and_checksum_match() {
    let db = Database::connect(":memory:", 1).await.unwrap();

    // Before applying: pending, no checksum comparison.
    let before = Migrator::new(&db, set(50)).status().await.unwrap();
    assert_eq!(before.len(), 1);
    assert!(!before[0].applied);
    assert_eq!(before[0].checksum_matches, None);
    assert_eq!(before[0].name, "create_users");

    Migrator::new(&db, set(50)).up().await.unwrap();

    // After applying with the same definition: applied and matching.
    let matching = Migrator::new(&db, set(50)).status().await.unwrap();
    assert!(matching[0].applied);
    assert_eq!(matching[0].checksum_matches, Some(true));

    // A changed definition (different length) is applied but no longer matches.
    let changed = Migrator::new(&db, set(100)).status().await.unwrap();
    assert!(changed[0].applied);
    assert_eq!(changed[0].checksum_matches, Some(false));
}

#[tokio::test]
async fn changed_checksum_errors_by_default_and_warn_overrides() {
    let db = Database::connect(":memory:", 1).await.unwrap();
    Migrator::new(&db, set(50)).up().await.unwrap();

    // The default policy aborts on a changed already-applied migration, so a
    // silently edited applied migration cannot drift the schema.
    let error = Migrator::new(&db, set(100)).up().await.unwrap_err();
    assert_eq!(error.kind(), tork_orm_core::ErrorKind::Configuration);

    // The explicit Warn policy continues (warning to stderr) and applies nothing.
    let applied = Migrator::new(&db, set(100))
        .on_checksum_mismatch(OnMismatch::Warn)
        .up()
        .await
        .unwrap();
    assert_eq!(applied, 0);
}

#[tokio::test]
async fn applied_at_is_rfc3339() {
    let db = Database::connect(":memory:", 1).await.unwrap();
    Migrator::new(&db, set(50)).up().await.unwrap();

    let rows = db
        .fetch_all(
            "SELECT applied_at, checksum FROM _tork_migrations".into(),
            vec![],
        )
        .await
        .unwrap();
    let applied_at = rows[0].get::<String>("applied_at").unwrap();
    assert!(OffsetDateTime::parse(&applied_at, &Rfc3339).is_ok());

    // The checksum was recorded (a 16-hex FNV-1a digest, not a placeholder).
    let checksum = rows[0].get::<String>("checksum").unwrap();
    assert_eq!(checksum.len(), 16);
}
