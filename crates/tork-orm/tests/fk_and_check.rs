//! Tests for model-declared foreign-key actions (`on_delete`/`on_update`) and
//! table-level CHECK constraints, which flow into `Model` metadata and the
//! generated migration DDL.

use tork_orm::prelude::*;
use tork_orm::ForeignKeyAction;

#[derive(Debug, Clone, Model)]
#[table(name = "parents")]
struct Parent {
    #[field(primary_key, auto)]
    id: i64,
}

#[derive(Debug, Clone, Model)]
#[table(name = "children", check = "score >= 0", check = "score <= 100")]
struct Child {
    #[field(primary_key, auto)]
    id: i64,
    #[field(foreign_key = Parent::id, on_delete = "cascade", on_update = "restrict")]
    parent_id: i64,
    #[field(foreign_key = Parent::id, on_delete = "set_null")]
    sponsor_id: Option<i64>,
    score: i64,
}

fn column(name: &str) -> &'static tork_orm::ColumnDef {
    <Child as Model>::COLUMNS.iter().find(|c| c.name == name).unwrap()
}

#[test]
fn foreign_key_actions_are_recorded() {
    let fk = column("parent_id").foreign_key.unwrap();
    assert_eq!(fk.table, "parents");
    assert_eq!(fk.column, "id");
    assert_eq!(fk.on_delete, ForeignKeyAction::Cascade);
    assert_eq!(fk.on_update, ForeignKeyAction::Restrict);
}

#[test]
fn omitted_actions_default_to_no_action() {
    let fk = column("sponsor_id").foreign_key.unwrap();
    assert_eq!(fk.on_delete, ForeignKeyAction::SetNull);
    assert_eq!(fk.on_update, ForeignKeyAction::NoAction);
}

#[test]
fn table_checks_are_recorded() {
    assert_eq!(<Child as Model>::CHECKS.to_vec(), vec!["score >= 0", "score <= 100"]);
    assert!(<Parent as Model>::CHECKS.is_empty());
}
