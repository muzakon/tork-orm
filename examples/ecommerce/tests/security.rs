//! Security: every value crosses the driver boundary as a bound parameter, so a
//! SQL-injection payload is stored and compared as a literal, never executed.

use ecommerce::models::*;
use ecommerce::testkit::*;
use tork_orm::prelude::*;

async fn db() -> Database {
    migrated(":memory:", 1).await.unwrap()
}

#[tokio::test]
async fn injection_payload_is_stored_as_a_literal() {
    let db = db().await;
    let payload = "Robert'); DROP TABLE users;-- @evil.com";

    let created = User::create(&db, &user(payload)).await.unwrap();

    // The `users` table still exists and the payload round-tripped verbatim.
    let found = User::find(&db, created.id).await.unwrap();
    assert_eq!(found.email, payload);
    assert_eq!(User::query().count(&db).await.unwrap(), 1);
}

#[tokio::test]
async fn tautology_in_a_filter_value_matches_nothing() {
    let db = db().await;
    User::create(&db, &user("real@x.com")).await.unwrap();

    // A classic `' OR '1'='1` payload as a bound value cannot widen the match.
    let rows = User::query().filter(User::email.eq("' OR '1'='1")).all(&db).await.unwrap();
    assert!(rows.is_empty(), "a tautology payload must not bypass the filter");

    // The table and its single real row are untouched.
    assert_eq!(User::query().count(&db).await.unwrap(), 1);
}

#[tokio::test]
async fn filter_raw_still_binds_its_parameters() {
    let db = db().await;
    User::create(&db, &user("alice@x.com")).await.unwrap();

    // filter_raw uses `?` placeholders; the value is bound, not interpolated.
    let rows = User::query()
        .filter_raw("email = ?", ["x'; DROP TABLE users;--"])
        .all(&db)
        .await
        .unwrap();
    assert!(rows.is_empty());
    assert_eq!(User::query().count(&db).await.unwrap(), 1);
}
