//! Tests for functional index entries plus per-column nulls/collate modifiers
//! declared through `#[table(indexes = [...])]`.

use tork_orm::dialect::SqliteDialect;
use tork_orm::prelude::*;

#[derive(Debug, Clone, Model)]
#[table(name = "users", indexes = [
    // Functional unique index, method form.
    unique(fields = [ expr(email.lower()) ]),
    // Functional index, free-function form.
    index(name = "idx_users_upper_username", fields = [ expr(upper(username)) ]),
    // Mixed plain column + ordering + null placement + collation.
    index(fields = [ tenant_id, score(desc, nulls_last), display_name(collate = "NOCASE") ]),
    // Functional partial predicate.
    index(name = "idx_admin", fields = [ id ], where = email.lower().eq("admin@x.com")),
])]
struct User {
    #[field(primary_key, auto)]
    id: i64,
    tenant_id: i64,
    username: String,
    email: String,
    display_name: String,
    score: i64,
}

#[test]
fn functional_unique_index_metadata_and_sql() {
    let dialect = SqliteDialect::new();
    let statements = User::index_statements(&dialect).unwrap();

    assert!(
        statements.iter().any(|s| s
            == "CREATE UNIQUE INDEX \"users_expr_key\" ON \"users\" ((lower(\"users\".\"email\")))"),
        "{statements:?}"
    );
    assert!(statements.iter().any(|s| s
        == "CREATE INDEX \"idx_users_upper_username\" ON \"users\" ((upper(\"users\".\"username\")))"));
}

#[test]
fn nulls_and_collate_modifiers_render() {
    let dialect = SqliteDialect::new();
    let statements = User::index_statements(&dialect).unwrap();
    assert!(
        statements.iter().any(|s| s
            == "CREATE INDEX \"users_tenant_id_score_display_name_idx\" ON \"users\" \
                (\"tenant_id\", \"score\" DESC NULLS LAST, \"display_name\" COLLATE NOCASE)"),
        "{statements:?}"
    );
}

#[test]
fn functional_partial_predicate_renders() {
    let dialect = SqliteDialect::new();
    let statements = User::index_statements(&dialect).unwrap();
    assert!(
        statements.iter().any(|s| s
            == "CREATE INDEX \"idx_admin\" ON \"users\" (\"id\") \
                WHERE lower(\"users\".\"email\") = 'admin@x.com'"),
        "{statements:?}"
    );
}

#[test]
fn expression_index_is_in_metadata() {
    let indexes = User::indexes();
    let functional = indexes.iter().find(|i| i.name == "users_expr_key").unwrap();
    assert_eq!(functional.columns.len(), 1);
    assert!(functional.columns[0].expression.is_some());
    assert!(functional.unique);
}
