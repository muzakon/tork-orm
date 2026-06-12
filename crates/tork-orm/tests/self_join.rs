//! Tests for self-joins: joining a table to itself under an alias with
//! `self_join` / `self_left_join`. SQL is asserted by rendering; behaviour runs
//! against in-memory SQLite.

use tork_orm::dialect::{render_select, SqliteDialect};
use tork_orm::prelude::*;

#[derive(Debug, Clone, Model, PartialEq)]
#[table(name = "employees")]
struct Employee {
    #[field(primary_key, auto)]
    id: i64,
    name: String,
    manager_id: Option<i64>,
}

/// A projection pairing an employee with their manager's name.
#[derive(Debug, QueryResult, PartialEq)]
struct WithManager {
    name: String,
    manager_name: String,
}

#[test]
fn self_join_renders_alias_and_on() {
    let (sql, _) = render_select(
        &SqliteDialect::new(),
        &Employee::query().self_join("mgr", "manager_id", "id").into_statement(),
    );
    assert!(
        sql.contains(
            "INNER JOIN \"employees\" AS \"mgr\" ON \"employees\".\"manager_id\" = \"mgr\".\"id\""
        ),
        "unexpected self-join SQL: {sql}"
    );

    let (left, _) = render_select(
        &SqliteDialect::new(),
        &Employee::query().self_left_join("mgr", "manager_id", "id").into_statement(),
    );
    assert!(left.contains("LEFT JOIN \"employees\" AS \"mgr\" ON "), "unexpected SQL: {left}");
}

async fn employee_db() -> Database {
    let db = Database::connect(":memory:", 1).await.unwrap();
    db.execute(
        "CREATE TABLE employees (id INTEGER PRIMARY KEY, name TEXT NOT NULL, manager_id INTEGER)"
            .into(),
        vec![],
    )
    .await
    .unwrap();
    let alice = Employee::create(
        &db,
        &Employee { id: 0, name: "Alice".into(), manager_id: None },
    )
    .await
    .unwrap();
    for name in ["Bob", "Carol"] {
        Employee::create(
            &db,
            &Employee { id: 0, name: name.into(), manager_id: Some(alice.id) },
        )
        .await
        .unwrap();
    }
    db
}

#[tokio::test]
async fn self_join_filters_by_the_aliased_table() {
    let db = employee_db().await;
    // Employees whose manager is "Alice".
    let reports = Employee::query()
        .self_join("mgr", "manager_id", "id")
        .filter(Expr::column("mgr", "name").eq("Alice"))
        .order_by(Employee::id.asc())
        .all(&db)
        .await
        .unwrap();
    assert_eq!(reports.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(), vec!["Bob", "Carol"]);
}

#[tokio::test]
async fn self_join_projects_the_aliased_columns() {
    let db = employee_db().await;
    let rows = Employee::query()
        .self_join("mgr", "manager_id", "id")
        .select((Employee::name, Expr::column("mgr", "name").as_("manager_name")))
        .order_by(Employee::id.asc())
        .all_as::<WithManager>(&db)
        .await
        .unwrap();
    assert_eq!(
        rows,
        vec![
            WithManager { name: "Bob".into(), manager_name: "Alice".into() },
            WithManager { name: "Carol".into(), manager_name: "Alice".into() },
        ]
    );
}

#[tokio::test]
async fn self_left_join_keeps_unmatched_base_rows() {
    let db = employee_db().await;
    // With a LEFT join, Alice (no manager) is kept.
    let all = Employee::query()
        .self_left_join("mgr", "manager_id", "id")
        .order_by(Employee::id.asc())
        .all(&db)
        .await
        .unwrap();
    assert_eq!(all.len(), 3);
}
