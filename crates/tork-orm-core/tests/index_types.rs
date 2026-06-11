//! Unit tests for the index metadata types.

use tork_orm_core::{IndexColumn, IndexDef};

#[test]
fn index_column_builder_sets_ordering_and_opclass() {
    let ascending = IndexColumn::new("created_at");
    assert_eq!(ascending.name, "created_at");
    assert!(!ascending.descending);
    assert!(ascending.opclass.is_none());

    let descending = IndexColumn::new("created_at").desc();
    assert!(descending.descending);

    let reset = IndexColumn::new("created_at").desc().asc();
    assert!(!reset.descending);

    let with_opclass = IndexColumn::new("metadata").opclass("gin_trgm_ops");
    assert_eq!(with_opclass.opclass.as_deref(), Some("gin_trgm_ops"));
}

#[test]
fn index_def_starts_empty() {
    let index = IndexDef::new("idx_posts_user_id");
    assert_eq!(index.name, "idx_posts_user_id");
    assert!(index.columns.is_empty());
    assert!(!index.unique);
    assert!(index.predicate.is_none());
    assert!(index.method.is_none());
    assert!(index.include.is_empty());
}
