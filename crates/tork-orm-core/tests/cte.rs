use tork_orm_core::dialect::{render_select, SqliteDialect};
use tork_orm_core::query::ast::{CteQuery, SelectItem, SelectStatement};
use tork_orm_core::query::expr::{BinaryOp, Expr};
use tork_orm_core::query::func::row_number;
use tork_orm_core::Value;

// ── Basic WITH (non-recursive) ───────────────────────────────────────────────

#[test]
fn with_single_cte() {
    let inner = SelectStatement::new(
        "users",
        vec![SelectItem::Column { table: "users", column: "id" }],
    );
    let outer = SelectStatement {
        with: Some(tork_orm_core::WithClause {
            recursive: false,
            ctes: vec![tork_orm_core::Cte {
                name: "active_users",
                columns: None,
                query: CteQuery::Select(inner),
            }],
        }),
        table: "active_users",
        projection: vec![SelectItem::Column { table: "active_users", column: "id" }],
        ..SelectStatement::new("active_users", vec![SelectItem::Column { table: "active_users", column: "id" }])
    };
    let (sql, _) = render_select(&SqliteDialect::new(), &outer);
    assert_eq!(
        sql,
        "WITH \"active_users\" AS (SELECT \"users\".\"id\" FROM \"users\") SELECT \"active_users\".\"id\" FROM \"active_users\""
    );
}

#[test]
fn with_multiple_ctes() {
    let eu = SelectStatement::new(
        "users",
        vec![SelectItem::Column { table: "users", column: "id" }],
    );
    let admins = SelectStatement::new(
        "users",
        vec![SelectItem::Column { table: "users", column: "id" }],
    );
    let outer: SelectStatement = SelectStatement {
        with: Some(tork_orm_core::WithClause {
            recursive: false,
            ctes: vec![
                tork_orm_core::Cte {
                    name: "eu_users",
                    columns: None,
                    query: CteQuery::Select(eu),
                },
                tork_orm_core::Cte {
                    name: "admin_users",
                    columns: None,
                    query: CteQuery::Select(admins),
                },
            ],
        }),
        table: "eu_users",
        projection: vec![SelectItem::Column { table: "eu_users", column: "id" }],
        ..SelectStatement::new("eu_users", vec![SelectItem::Column { table: "eu_users", column: "id" }])
    };
    let (sql, _) = render_select(&SqliteDialect::new(), &outer);
    assert!(
        sql.starts_with("WITH \"eu_users\" AS (SELECT \"users\".\"id\" FROM \"users\"), \"admin_users\" AS (SELECT \"users\".\"id\" FROM \"users\") SELECT \"eu_users\".\"id\" FROM \"eu_users\"")
    );
}

#[test]
fn with_cte_with_column_names() {
    let inner = SelectStatement::new(
        "users",
        vec![SelectItem::Expression(
            Expr::binary(
                Expr::column("users", "id"),
                BinaryOp::Add,
                Expr::value(Value::Int(1)),
            )
            .as_("uid"),
        )],
    );
    let outer = SelectStatement {
        with: Some(tork_orm_core::WithClause {
            recursive: false,
            ctes: vec![tork_orm_core::Cte {
                name: "user_ids",
                columns: Some(vec!["user_id"]),
                query: CteQuery::Select(inner),
            }],
        }),
        table: "user_ids",
        projection: vec![SelectItem::Column { table: "user_ids", column: "user_id" }],
        ..SelectStatement::new("user_ids", vec![SelectItem::Column { table: "user_ids", column: "user_id" }])
    };
    let (sql, _) = render_select(&SqliteDialect::new(), &outer);
    assert_eq!(
        sql,
        "WITH \"user_ids\"(\"user_id\") AS (SELECT \"users\".\"id\" + ? AS \"uid\" FROM \"users\") SELECT \"user_ids\".\"user_id\" FROM \"user_ids\""
    );
}

#[test]
fn with_cte_into_union() {
    let inner1 = SelectStatement::new(
        "users",
        vec![SelectItem::Column { table: "users", column: "id" }],
    );
    let inner2 = SelectStatement::new(
        "users",
        vec![SelectItem::Column { table: "users", column: "id" }],
    );
    let union_stmt = tork_orm_core::UnionStatement {
        first: inner1,
        rest: vec![(true, inner2)],
        order_by: vec![],
        limit: None,
        offset: None,
        lock: None,
    };
    let outer = SelectStatement {
        with: Some(tork_orm_core::WithClause {
            recursive: false,
            ctes: vec![tork_orm_core::Cte {
                name: "combined",
                columns: None,
                query: CteQuery::Union(Box::new(union_stmt)),
            }],
        }),
        table: "combined",
        projection: vec![SelectItem::Column { table: "combined", column: "id" }],
        ..SelectStatement::new("combined", vec![SelectItem::Column { table: "combined", column: "id" }])
    };
    let (sql, _) = render_select(&SqliteDialect::new(), &outer);
    assert_eq!(
        sql,
        "WITH \"combined\" AS (SELECT \"users\".\"id\" FROM \"users\" UNION ALL SELECT \"users\".\"id\" FROM \"users\") SELECT \"combined\".\"id\" FROM \"combined\""
    );
}

// ── WITH RECURSIVE ───────────────────────────────────────────────────────────

#[test]
fn with_recursive_cte() {
    let inner1 = SelectStatement::new(
        "employees",
        vec![
            SelectItem::Column { table: "employees", column: "id" },
            SelectItem::Column { table: "employees", column: "name" },
        ],
    );
    let inner2 = SelectStatement::new(
        "employees",
        vec![
            SelectItem::Column { table: "employees", column: "id" },
            SelectItem::Column { table: "employees", column: "name" },
        ],
    );
    let union_stmt = tork_orm_core::UnionStatement {
        first: inner1,
        rest: vec![(true, inner2)],
        order_by: vec![],
        limit: None,
        offset: None,
        lock: None,
    };
    let outer = SelectStatement {
        with: Some(tork_orm_core::WithClause {
            recursive: true,
            ctes: vec![tork_orm_core::Cte {
                name: "org_tree",
                columns: Some(vec!["id", "name"]),
                query: CteQuery::Union(Box::new(union_stmt)),
            }],
        }),
        table: "org_tree",
        projection: vec![
            SelectItem::Column { table: "org_tree", column: "id" },
            SelectItem::Column { table: "org_tree", column: "name" },
        ],
        ..SelectStatement::new("org_tree", vec![SelectItem::Column { table: "org_tree", column: "id" }])
    };
    let (sql, _) = render_select(&SqliteDialect::new(), &outer);
    assert!(
        sql.starts_with("WITH RECURSIVE \"org_tree\"(\"id\", \"name\") AS (SELECT \"employees\".\"id\", \"employees\".\"name\" FROM \"employees\" UNION ALL SELECT \"employees\".\"id\", \"employees\".\"name\" FROM \"employees\") SELECT \"org_tree\".\"id\", \"org_tree\".\"name\" FROM \"org_tree\"")
    );
}

// ── Window function inside CTE ────────────────────────────────────────────────

#[test]
fn window_in_cte() {
    let inner_stmt = SelectStatement {
        projection: vec![
            SelectItem::Column { table: "employees", column: "name" },
            SelectItem::Column { table: "employees", column: "salary" },
            SelectItem::Expression(
                row_number()
                    .over()
                    .order_by([Expr::column("employees", "salary").desc()])
                    .end()
                    .as_("rn"),
            ),
        ],
        ..SelectStatement::new("employees", vec![SelectItem::Column { table: "employees", column: "name" }])
    };

    let outer = SelectStatement {
        with: Some(tork_orm_core::WithClause {
            recursive: false,
            ctes: vec![tork_orm_core::Cte {
                name: "ranked",
                columns: None,
                query: CteQuery::Select(inner_stmt),
            }],
        }),
        table: "ranked",
        projection: vec![
            SelectItem::Column { table: "ranked", column: "name" },
            SelectItem::Column { table: "ranked", column: "salary" },
        ],
        ..SelectStatement::new("ranked", vec![SelectItem::Column { table: "ranked", column: "name" }])
    };
    let (sql, _) = render_select(&SqliteDialect::new(), &outer);
    assert!(
        sql.contains("WITH \"ranked\" AS (SELECT \"employees\".\"name\", \"employees\".\"salary\", ROW_NUMBER() OVER (ORDER BY \"employees\".\"salary\" DESC) AS \"rn\" FROM \"employees\")"),
        "unexpected SQL: {sql}"
    );
    assert!(sql.contains("SELECT \"ranked\".\"name\", \"ranked\".\"salary\" FROM \"ranked\""));
}
