//! Rendering tests for `CREATE INDEX`, covering ordering, partial predicates, and
//! the validation of features SQLite does not support.

#![cfg(feature = "migrations")]

use tork_orm_core::dialect::SqliteDialect;
use tork_orm_core::migration::render::create_index;
use tork_orm_core::query::expr::{BinaryOp, Expr};
use tork_orm_core::{IndexColumn, IndexDef, Value};

fn index(name: &str, columns: Vec<IndexColumn>) -> IndexDef {
    let mut def = IndexDef::new(name);
    def.columns = columns;
    def
}

#[test]
fn single_column_index() {
    let dialect = SqliteDialect::new();
    let def = index("idx_posts_user_id", vec![IndexColumn::new("user_id")]);
    let sql = create_index(&dialect, "posts", &def, false).unwrap();
    assert_eq!(
        sql,
        "CREATE INDEX \"idx_posts_user_id\" ON \"posts\" (\"user_id\")"
    );
}

#[test]
fn compound_unique_index_with_descending_column() {
    let dialect = SqliteDialect::new();
    let mut def = index(
        "uq_posts_user_created",
        vec![
            IndexColumn::new("user_id"),
            IndexColumn::new("created_at").desc(),
        ],
    );
    def.unique = true;
    let sql = create_index(&dialect, "posts", &def, true).unwrap();
    assert_eq!(
        sql,
        "CREATE UNIQUE INDEX IF NOT EXISTS \"uq_posts_user_created\" ON \"posts\" \
         (\"user_id\", \"created_at\" DESC)"
    );
}

#[test]
fn partial_index_renders_inline_literal() {
    let dialect = SqliteDialect::new();
    let mut def = index("idx_posts_published", vec![IndexColumn::new("created_at")]);
    def.predicate = Some(Expr::binary(
        Expr::column("posts", "status"),
        BinaryOp::Eq,
        Expr::value(Value::Text("published".to_string())),
    ));
    let sql = create_index(&dialect, "posts", &def, false).unwrap();
    assert_eq!(
        sql,
        "CREATE INDEX \"idx_posts_published\" ON \"posts\" (\"created_at\") \
         WHERE \"posts\".\"status\" = 'published'"
    );
}

#[test]
fn boolean_predicate_uses_dialect_literal() {
    let dialect = SqliteDialect::new();
    let mut def = index("idx_users_active", vec![IndexColumn::new("email")]);
    def.predicate = Some(Expr::binary(
        Expr::column("users", "is_active"),
        BinaryOp::Eq,
        Expr::value(Value::Bool(true)),
    ));
    let sql = create_index(&dialect, "users", &def, false).unwrap();
    assert!(sql.ends_with("WHERE \"users\".\"is_active\" = 1"), "{sql}");
}

#[test]
fn functional_index_wraps_expression() {
    let dialect = SqliteDialect::new();
    let mut def = index("idx_users_lower_email", Vec::new());
    def.columns = vec![IndexColumn::expression(Expr::func(
        "lower",
        [Expr::column("users", "email")],
    ))];
    def.unique = true;
    let sql = create_index(&dialect, "users", &def, false).unwrap();
    assert_eq!(
        sql,
        "CREATE UNIQUE INDEX \"idx_users_lower_email\" ON \"users\" \
         ((lower(\"users\".\"email\")))"
    );
}

#[test]
fn nulls_and_collation_render() {
    let dialect = SqliteDialect::new();
    let mut def = index(
        "idx_users_name",
        vec![
            IndexColumn::new("score").desc().nulls_last(),
            IndexColumn::new("name").collate("NOCASE"),
        ],
    );
    def.unique = false;
    let sql = create_index(&dialect, "users", &def, false).unwrap();
    assert_eq!(
        sql,
        "CREATE INDEX \"idx_users_name\" ON \"users\" \
         (\"score\" DESC NULLS LAST, \"name\" COLLATE NOCASE)"
    );
}

#[test]
fn index_method_errors_on_sqlite() {
    let dialect = SqliteDialect::new();
    let mut def = index("idx_posts_meta", vec![IndexColumn::new("metadata")]);
    def.method = Some("gin".to_string());
    let error = create_index(&dialect, "posts", &def, false).unwrap_err();
    assert!(
        error.to_string().contains("does not support index method `gin`"),
        "{error}"
    );
}

#[test]
fn covering_columns_error_on_sqlite() {
    let dialect = SqliteDialect::new();
    let mut def = index("idx_posts_user", vec![IndexColumn::new("user_id")]);
    def.include = vec!["title".to_string(), "status".to_string()];
    let error = create_index(&dialect, "posts", &def, false).unwrap_err();
    assert!(error.to_string().contains("INCLUDE"), "{error}");
}

#[test]
fn operator_class_errors_on_sqlite() {
    let dialect = SqliteDialect::new();
    let def = index(
        "idx_posts_meta",
        vec![IndexColumn::new("metadata").opclass("gin_trgm_ops")],
    );
    let error = create_index(&dialect, "posts", &def, false).unwrap_err();
    assert!(
        error.to_string().contains("operator class"),
        "{error}"
    );
}
