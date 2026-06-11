//! Tests for `#[table(indexes = [...])]`: compound and unique indexes, per-column
//! ordering, partial `where` predicates, and the Postgres-only metadata that
//! renders to an error on SQLite.

use tork_orm::dialect::SqliteDialect;
use tork_orm::prelude::*;

#[derive(Debug, Clone, PartialEq)]
enum PostStatus {
    Draft,
    Published,
}

impl BindValue for PostStatus {
    fn to_value(&self) -> Value {
        match self {
            PostStatus::Draft => Value::Text("draft".into()),
            PostStatus::Published => Value::Text("published".into()),
        }
    }
}

impl FromValue for PostStatus {
    fn from_value(value: Value) -> tork_orm::Result<Self> {
        match value {
            Value::Text(text) if text == "published" => Ok(PostStatus::Published),
            _ => Ok(PostStatus::Draft),
        }
    }
}

#[derive(Debug, Clone, Model)]
#[table(name = "posts", indexes = [
    index(fields = [user_id, status, created_at(desc)]),
    unique(name = "uq_posts_user_slug", fields = [user_id, slug]),
    index(fields = [created_at(desc)], where = status.eq(PostStatus::Published)),
])]
struct Post {
    #[field(primary_key, auto)]
    id: i64,
    #[field(foreign_key = User::id)]
    user_id: i64,
    #[field(varchar(length = 50))]
    slug: String,
    status: PostStatus,
    title: String,
    created_at: i64,
}

#[derive(Debug, Clone, Model)]
#[table(name = "users")]
struct User {
    #[field(primary_key, auto)]
    id: i64,
}

#[derive(Debug, Clone, Model)]
#[table(name = "documents", indexes = [
    index(fields = [body], using = "gin"),
    index(fields = [owner_id], include = [title]),
])]
struct Document {
    #[field(primary_key, auto)]
    id: i64,
    owner_id: i64,
    title: String,
    body: String,
}

#[test]
fn compound_unique_and_partial_indexes_are_collected() {
    let indexes = Post::indexes();
    // Three table-level indexes; the foreign-key auto-index on user_id is
    // suppressed because a table index already leads with it.
    assert_eq!(indexes.len(), 3);

    let compound = indexes.iter().find(|i| i.name == "posts_user_id_status_created_at_idx").unwrap();
    assert!(!compound.unique);
    assert_eq!(compound.columns.len(), 3);
    assert_eq!(compound.columns[2].name, "created_at");
    assert!(compound.columns[2].descending);

    let unique = indexes.iter().find(|i| i.name == "uq_posts_user_slug").unwrap();
    assert!(unique.unique);
    assert_eq!(unique.columns.len(), 2);

    let partial = indexes
        .iter()
        .find(|i| i.name == "posts_created_at_idx")
        .unwrap();
    assert!(partial.predicate.is_some());
}

#[test]
fn foreign_key_auto_index_suppressed_by_leading_table_index() {
    // No standalone "posts_user_id_idx"; the compound index covers it.
    assert!(Post::indexes()
        .iter()
        .all(|i| i.name != "posts_user_id_idx"));
}

#[test]
fn partial_predicate_renders_inline_literal() {
    let dialect = SqliteDialect::new();
    let statements = Post::index_statements(&dialect).unwrap();
    assert!(
        statements.iter().any(|s| s
            == "CREATE INDEX \"posts_created_at_idx\" ON \"posts\" (\"created_at\" DESC) \
                WHERE \"posts\".\"status\" = 'published'"),
        "{statements:?}"
    );
}

#[test]
fn compound_index_renders_with_descending() {
    let dialect = SqliteDialect::new();
    let statements = Post::index_statements(&dialect).unwrap();
    assert!(statements.iter().any(|s| s
        == "CREATE INDEX \"posts_user_id_status_created_at_idx\" ON \"posts\" \
            (\"user_id\", \"status\", \"created_at\" DESC)"));
}

#[test]
fn postgres_only_features_are_stored_but_error_on_sqlite() {
    let documents = Document::indexes();
    let gin = documents.iter().find(|i| i.method.is_some()).unwrap();
    assert_eq!(gin.method.as_deref(), Some("gin"));

    let covering = documents.iter().find(|i| !i.include.is_empty()).unwrap();
    assert_eq!(covering.include, vec!["title".to_string()]);

    // Rendering them for SQLite reports the unsupported feature.
    let dialect = SqliteDialect::new();
    assert!(Document::index_statements(&dialect).is_err());
}
