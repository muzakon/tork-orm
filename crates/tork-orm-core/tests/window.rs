use tork_orm_core::dialect::{render_expr, render_select, SqliteDialect};
use tork_orm_core::query::expr::{AggFunc, Expr, WindowBound};
use tork_orm_core::query::func::*;
use tork_orm_core::Value;

fn render(expr: &Expr) -> (String, Vec<Value>) {
    render_expr(&SqliteDialect::new(), expr)
}

// ── OVER with aggregates ─────────────────────────────────────────────────────

#[test]
fn aggregate_over_empty_window() {
    let (sql, _) = render(&Expr::aggregate(AggFunc::Count, [Expr::column("t", "id")])
        .over().end());
    assert_eq!(sql, "COUNT(\"t\".\"id\") OVER ()");
}

#[test]
fn aggregate_over_with_partition_by() {
    let (sql, _) = render(
        &Expr::aggregate(AggFunc::Sum, [Expr::column("t", "amount")])
            .over()
            .partition_by([Expr::column("t", "dept")])
            .end(),
    );
    assert_eq!(sql, "SUM(\"t\".\"amount\") OVER (PARTITION BY \"t\".\"dept\")");
}

#[test]
fn aggregate_over_with_order_by() {
    let (sql, _) = render(
        &Expr::aggregate(AggFunc::Sum, [Expr::column("t", "amount")])
            .over()
            .order_by([Expr::column("t", "date").asc()])
            .end(),
    );
    assert_eq!(
        sql,
        "SUM(\"t\".\"amount\") OVER (ORDER BY \"t\".\"date\" ASC)"
    );
}

#[test]
fn aggregate_over_with_partition_and_order() {
    let (sql, _) = render(
        &Expr::aggregate(AggFunc::Count, [Expr::column("t", "id")])
            .over()
            .partition_by([Expr::column("t", "dept")])
            .order_by([Expr::column("t", "salary").desc()])
            .end(),
    );
    assert_eq!(
        sql,
        "COUNT(\"t\".\"id\") OVER (PARTITION BY \"t\".\"dept\" ORDER BY \"t\".\"salary\" DESC)"
    );
}

// ── Window frames ────────────────────────────────────────────────────────────

#[test]
fn rows_between_unbounded_preceding_and_current_row() {
    let (sql, _) = render(
        &Expr::aggregate(AggFunc::Sum, [Expr::column("t", "amount")])
            .over()
            .order_by([Expr::column("t", "date").asc()])
            .rows_between(WindowBound::UnboundedPreceding, WindowBound::CurrentRow)
            .end(),
    );
    assert_eq!(
        sql,
        "SUM(\"t\".\"amount\") OVER (ORDER BY \"t\".\"date\" ASC ROWS BETWEEN UNBOUNDED PRECEDING AND CURRENT ROW)"
    );
}

#[test]
fn range_between_preceding_and_following() {
    let (sql, _) = render(
        &Expr::aggregate(AggFunc::Avg, [Expr::column("t", "price")])
            .over()
            .order_by([Expr::column("t", "date").asc()])
            .range_between(
                WindowBound::Preceding(Box::new(Expr::value(Value::Int(3)))),
                WindowBound::Following(Box::new(Expr::value(Value::Int(3)))),
            )
            .end(),
    );
    assert_eq!(
        sql,
        "AVG(\"t\".\"price\") OVER (ORDER BY \"t\".\"date\" ASC RANGE BETWEEN ? PRECEDING AND ? FOLLOWING)"
    );
}

// ── Pure window functions ────────────────────────────────────────────────────

#[test]
fn row_number_over_order_by() {
    let (sql, _) = render(&row_number().over().order_by([Expr::column("t", "id").asc()]).end());
    assert_eq!(
        sql,
        "ROW_NUMBER() OVER (ORDER BY \"t\".\"id\" ASC)"
    );
}

#[test]
fn rank_over_partition_by_order_by() {
    let (sql, _) = render(
        &rank()
            .over()
            .partition_by([Expr::column("t", "dept")])
            .order_by([Expr::column("t", "salary").desc()])
            .end(),
    );
    assert_eq!(
        sql,
        "RANK() OVER (PARTITION BY \"t\".\"dept\" ORDER BY \"t\".\"salary\" DESC)"
    );
}

#[test]
fn dense_rank_renders() {
    let (sql, _) = render(&dense_rank().over().order_by([Expr::column("t", "score").desc()]).end());
    assert_eq!(sql, "DENSE_RANK() OVER (ORDER BY \"t\".\"score\" DESC)");
}

#[test]
fn ntile_renders() {
    let (sql, _) = render(&ntile(4).over().order_by([Expr::column("t", "id").asc()]).end());
    assert_eq!(sql, "NTILE(?) OVER (ORDER BY \"t\".\"id\" ASC)");
}

#[test]
fn lag_renders() {
    let (sql, _) = render(&lag(Expr::column("t", "salary")).over().order_by([Expr::column("t", "id").asc()]).end());
    assert_eq!(
        sql,
        "LAG(\"t\".\"salary\") OVER (ORDER BY \"t\".\"id\" ASC)"
    );
}

#[test]
fn lag_with_offset_renders() {
    let (sql, _) = render(
        &lag_offset(Expr::column("t", "salary"), 2)
            .over()
            .order_by([Expr::column("t", "id").asc()])
            .end(),
    );
    assert_eq!(
        sql,
        "LAG(\"t\".\"salary\", ?) OVER (ORDER BY \"t\".\"id\" ASC)"
    );
}

#[test]
fn lead_renders() {
    let (sql, _) = render(&lead(Expr::column("t", "salary")).over().order_by([Expr::column("t", "id").asc()]).end());
    assert_eq!(
        sql,
        "LEAD(\"t\".\"salary\") OVER (ORDER BY \"t\".\"id\" ASC)"
    );
}

#[test]
fn first_value_renders() {
    let (sql, _) = render(
        &first_value(Expr::column("t", "amount"))
            .over()
            .order_by([Expr::column("t", "date").asc()])
            .rows_between(WindowBound::UnboundedPreceding, WindowBound::CurrentRow)
            .end(),
    );
    assert_eq!(
        sql,
        "FIRST_VALUE(\"t\".\"amount\") OVER (ORDER BY \"t\".\"date\" ASC ROWS BETWEEN UNBOUNDED PRECEDING AND CURRENT ROW)"
    );
}

#[test]
fn last_value_renders() {
    let (sql, _) = render(
        &last_value(Expr::column("t", "amount"))
            .over()
            .order_by([Expr::column("t", "date").asc()])
            .rows_between(WindowBound::UnboundedPreceding, WindowBound::UnboundedFollowing)
            .end(),
    );
    assert_eq!(
        sql,
        "LAST_VALUE(\"t\".\"amount\") OVER (ORDER BY \"t\".\"date\" ASC ROWS BETWEEN UNBOUNDED PRECEDING AND UNBOUNDED FOLLOWING)"
    );
}

#[test]
fn nth_value_renders() {
    let (sql, _) = render(
        &nth_value(Expr::column("t", "amount"), 3)
            .over()
            .order_by([Expr::column("t", "date").asc()])
            .end(),
    );
    assert_eq!(
        sql,
        "NTH_VALUE(\"t\".\"amount\", ?) OVER (ORDER BY \"t\".\"date\" ASC)"
    );
}

#[test]
fn percent_rank_renders() {
    let (sql, _) = render(&percent_rank().over().order_by([Expr::column("t", "score").desc()]).end());
    assert_eq!(sql, "PERCENT_RANK() OVER (ORDER BY \"t\".\"score\" DESC)");
}

#[test]
fn cume_dist_renders() {
    let (sql, _) = render(&cume_dist().over().order_by([Expr::column("t", "score").desc()]).end());
    assert_eq!(sql, "CUME_DIST() OVER (ORDER BY \"t\".\"score\" DESC)");
}

// ── Aggregate FILTER before OVER ──────────────────────────────────────────────

#[test]
fn aggregate_filter_then_over() {
    let expr = Expr::aggregate(AggFunc::Count, [Expr::column("t", "id")])
        .filter(Expr::binary(
            Expr::column("t", "active"),
            tork_orm_core::query::expr::BinaryOp::Eq,
            Expr::value(Value::Bool(true)),
        ))
        .over()
        .partition_by([Expr::column("t", "dept")])
        .end();
    let (sql, _) = render(&expr);
    assert_eq!(
        sql,
        "COUNT(\"t\".\"id\") FILTER (WHERE \"t\".\"active\" = ?) OVER (PARTITION BY \"t\".\"dept\")"
    );
}

// ── Window function in SELECT projection ──────────────────────────────────────

#[test]
fn window_function_in_select() {
    use tork_orm_core::query::ast::{SelectItem, SelectStatement};
    let stmt = SelectStatement::new(
        "employees",
        vec![
            SelectItem::Column { table: "employees", column: "name" },
            SelectItem::Expression(
                row_number()
                    .over()
                    .order_by([Expr::column("employees", "salary").desc()])
                    .end()
                    .as_("rank"),
            ),
        ],
    );
    let (sql, _) = render_select(&SqliteDialect::new(), &stmt);
    assert!(sql.contains("ROW_NUMBER() OVER (ORDER BY \"employees\".\"salary\" DESC) AS \"rank\""));
}

// ── Complex: DENSE_RANK with PARTITION BY + ORDER BY ─────────────────────────

#[test]
fn dense_rank_with_filter_in_window() {
    // Simulate grouping: first FILTER (WHERE ...) then OVER
    let expr = Expr::aggregate(AggFunc::Count, [Expr::column("t", "id")])
        .filter(Expr::binary(
            Expr::column("t", "status"),
            tork_orm_core::query::expr::BinaryOp::Eq,
            Expr::value(Value::Text("active".into())),
        ))
        .over()
        .partition_by([Expr::column("t", "dept")])
        .order_by([Expr::column("t", "date").asc()])
        .rows_between(WindowBound::UnboundedPreceding, WindowBound::CurrentRow)
        .end();
    let (sql, _) = render(&expr);
    assert_eq!(
        sql,
        "COUNT(\"t\".\"id\") FILTER (WHERE \"t\".\"status\" = ?) OVER (PARTITION BY \"t\".\"dept\" ORDER BY \"t\".\"date\" ASC ROWS BETWEEN UNBOUNDED PRECEDING AND CURRENT ROW)"
    );
}
