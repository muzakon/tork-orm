use tork_orm_core::dialect::{render_expr, SqliteDialect};
use tork_orm_core::query::expr::Expr;
use tork_orm_core::query::func::*;
use tork_orm_core::Value;

fn render(expr: &Expr) -> (String, Vec<Value>) {
    render_expr(&SqliteDialect::new(), expr)
}

// ── Current date/time ────────────────────────────────────────────────────────

#[test]
fn current_timestamp_raw() {
    let (sql, _) = render(&current_timestamp());
    assert_eq!(sql, "CURRENT_TIMESTAMP");
}

#[test]
fn current_date_raw() {
    let (sql, _) = render(&current_date());
    assert_eq!(sql, "CURRENT_DATE");
}

#[test]
fn current_time_raw() {
    let (sql, _) = render(&current_time());
    assert_eq!(sql, "CURRENT_TIME");
}

#[test]
fn now_function() {
    let (sql, _) = render(&now());
    assert_eq!(sql, "NOW()");
}

// ── EXTRACT ──────────────────────────────────────────────────────────────────

#[test]
fn extract_year_from_column() {
    let (sql, params) = render(&extract("YEAR", Expr::column("t", "created_at")));
    assert_eq!(sql, "EXTRACT(YEAR FROM \"t\".\"created_at\")");
    assert!(params.is_empty());
}

#[test]
fn extract_month_from_column() {
    let (sql, _) = render(&extract("MONTH", Expr::column("t", "created_at")));
    assert_eq!(sql, "EXTRACT(MONTH FROM \"t\".\"created_at\")");
}

#[test]
fn extract_in_predicate() {
    let expr = extract("YEAR", Expr::column("t", "created_at")).eq(Value::Int(2024));
    let (sql, params) = render(&expr);
    assert_eq!(
        sql,
        "EXTRACT(YEAR FROM \"t\".\"created_at\") = ?"
    );
    assert_eq!(params, vec![Value::Int(2024)]);
}

// ── PostgreSQL-specific (test renders, gated at build time) ──────────────────

#[cfg(feature = "postgres")]
#[test]
fn date_trunc_renders() {
    let (sql, params) = render(&date_trunc("month", Expr::column("t", "created_at")));
    assert_eq!(
        sql,
        "date_trunc(?, \"t\".\"created_at\")"
    );
    assert_eq!(params, vec![Value::Text("month".into())]);
}

#[cfg(feature = "postgres")]
#[test]
fn age_renders() {
    let (sql, _) = render(&age(Expr::column("t", "created_at"), now()));
    assert_eq!(sql, "AGE(\"t\".\"created_at\", NOW())");
}

#[cfg(feature = "postgres")]
#[test]
fn to_char_renders() {
    let (sql, params) = render(&to_char(Expr::column("t", "created_at"), "YYYY-MM-DD"));
    assert_eq!(
        sql,
        "TO_CHAR(\"t\".\"created_at\", ?)"
    );
    assert_eq!(params, vec![Value::Text("YYYY-MM-DD".into())]);
}

#[cfg(feature = "postgres")]
#[test]
fn at_time_zone_renders() {
    let (sql, params) = render(&at_time_zone("UTC", Expr::column("t", "created_at")));
    assert_eq!(sql, "timezone(?, \"t\".\"created_at\")");
    assert_eq!(params, vec![Value::Text("UTC".into())]);
}
