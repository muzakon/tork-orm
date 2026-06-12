#![cfg(feature = "postgres")]

use tork_orm_core::dialect::{render_expr, PostgresDialect};
use tork_orm_core::query::expr::Expr;
use tork_orm_core::query::func::{self};
use tork_orm_core::Value;

fn render(expr: &Expr) -> (String, Vec<Value>) {
    render_expr(&PostgresDialect::new(), expr)
}

#[test]
fn to_tsvector_sql() {
    let expr = func::to_tsvector("english", Expr::column("articles", "body"));
    let (sql, _) = render(&expr);
    assert_eq!(
        sql,
        r#"to_tsvector($1, "articles"."body")"#,
        "to_tsvector with config and column"
    );
}

#[test]
fn to_tsquery_sql() {
    let expr = func::to_tsquery("english", Expr::value(Value::Text("search & terms".into())));
    let (sql, _) = render(&expr);
    assert_eq!(
        sql,
        "to_tsquery($1, $2)",
        "to_tsquery with config and text"
    );
}

#[test]
fn plainto_tsquery_sql() {
    let expr = func::plainto_tsquery("english", Expr::value(Value::Text("search terms".into())));
    let (sql, _) = render(&expr);
    assert_eq!(
        sql,
        "plainto_tsquery($1, $2)",
        "plainto_tsquery with plain text"
    );
}

#[test]
fn phraseto_tsquery_sql() {
    let expr = func::phraseto_tsquery("english", Expr::value(Value::Text("exact phrase".into())));
    let (sql, _) = render(&expr);
    assert_eq!(
        sql,
        "phraseto_tsquery($1, $2)",
        "phraseto_tsquery with phrase"
    );
}

#[test]
fn ts_rank_sql() {
    let vector = func::to_tsvector("english", Expr::column("articles", "body"));
    let query = func::to_tsquery("english", Expr::value(Value::Text("search & terms".into())));
    let expr = func::ts_rank(vector, query);
    let (sql, _) = render(&expr);
    assert_eq!(
        sql,
        r#"ts_rank(to_tsvector($1, "articles"."body"), to_tsquery($2, $3))"#,
        "ts_rank over vector and query"
    );
}

#[test]
fn ts_rank_cd_sql() {
    let vector = func::to_tsvector("english", Expr::column("articles", "body"));
    let query = func::to_tsquery("english", Expr::value(Value::Text("search & terms".into())));
    let expr = func::ts_rank_cd(vector, query);
    let (sql, _) = render(&expr);
    assert_eq!(
        sql,
        r#"ts_rank_cd(to_tsvector($1, "articles"."body"), to_tsquery($2, $3))"#,
        "ts_rank_cd over vector and query"
    );
}

#[test]
fn ts_headline_sql() {
    let query = func::to_tsquery("english", Expr::value(Value::Text("search".into())));
    let expr = func::ts_headline("english", Expr::column("articles", "body"), query);
    let (sql, _) = render(&expr);
    assert_eq!(
        sql,
        r#"ts_headline($1, "articles"."body", to_tsquery($2, $3))"#,
        "ts_headline with config, text, query"
    );
}

#[test]
fn tsquery_cast_sql() {
    let expr = func::tsquery("search & terms");
    let (sql, _) = render(&expr);
    assert_eq!(
        sql,
        "tsquery($1)",
        "tsquery cast function call"
    );
}

#[test]
fn ts_match_operator_sql() {
    let vector = func::to_tsvector("english", Expr::column("articles", "body"));
    let query = func::to_tsquery("english", Expr::value(Value::Text("search & terms".into())));
    let expr = vector.matches(query);
    let (sql, _) = render(&expr);
    assert_eq!(
        sql,
        r#"to_tsvector($1, "articles"."body") @@ to_tsquery($2, $3)"#,
        "tsvector @@ tsquery match"
    );
}

#[test]
fn ts_match_convenience_sql() {
    let expr = Expr::column("articles", "body").ts_match("english", "search terms");
    let (sql, _) = render(&expr);
    assert_eq!(
        sql,
        r#"to_tsvector($1, "articles"."body") @@ to_tsquery($2, $3)"#,
        "expression.ts_match() convenience method"
    );
}
