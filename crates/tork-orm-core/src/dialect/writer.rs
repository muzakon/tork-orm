//! The shared SQL writer.
//!
//! A [`QueryWriter`] accumulates SQL text and an ordered list of bound parameters,
//! deferring the backend-specific bits (identifier quoting, placeholder spelling)
//! to a [`Dialect`]. The query layer renders the AST through it, so all dialects
//! share one rendering walk and differ only in their primitives.

use crate::dialect::{Dialect, DialectKind};
use crate::query::ast::{
    CteQuery, JoinKind, LockClause, LockStrength, LockWait, SelectItem, SelectStatement,
    UnionStatement, WithClause,
};
use crate::query::expr::{Expr, WindowBound};
use crate::query::write::{DeleteStatement, InsertStatement, OnConflict, UpdateStatement};
use crate::value::Value;

/// Builds a SQL string and its bound parameters for a dialect.
pub struct QueryWriter<'a> {
    dialect: &'a dyn Dialect,
    sql: String,
    params: Vec<Value>,
    inline_values: bool,
}

impl<'a> QueryWriter<'a> {
    /// Creates a writer that renders for `dialect`.
    pub fn new(dialect: &'a dyn Dialect) -> Self {
        Self {
            dialect,
            sql: String::new(),
            params: Vec::new(),
            inline_values: false,
        }
    }

    /// Creates a writer that renders values inline as SQL literals instead of as
    /// bound placeholders.
    ///
    /// DDL cannot bind parameters, so a partial-index predicate has to embed its
    /// values directly. A writer in this mode leaves [`QueryWriter::finish`]'s
    /// parameter list empty.
    pub fn new_inline(dialect: &'a dyn Dialect) -> Self {
        Self {
            dialect,
            sql: String::new(),
            params: Vec::new(),
            inline_values: true,
        }
    }

    /// Appends raw SQL text.
    pub fn push_sql(&mut self, sql: &str) {
        self.sql.push_str(sql);
    }

    /// Appends a scalar function name, keeping only identifier-safe characters.
    ///
    /// Function names are rendered verbatim (not as bound parameters), so a name
    /// built from untrusted input could otherwise inject SQL. Dropping everything
    /// but letters, digits, `_`, and `.` leaves legitimate names (`count`,
    /// `json_extract`, `schema.fn`) unchanged while neutralizing any payload into a
    /// harmless token the database rejects as an unknown function.
    fn push_function_name(&mut self, name: &str) {
        for ch in name.chars() {
            if ch.is_ascii_alphanumeric() || ch == '_' || ch == '.' {
                self.sql.push(ch);
            }
        }
    }

    /// Appends a quoted identifier.
    pub fn push_identifier(&mut self, identifier: &str) {
        self.dialect.quote_identifier(identifier, &mut self.sql);
    }

    /// Appends a `"table"."column"` reference.
    pub fn push_qualified(&mut self, table: &str, column: &str) {
        self.push_identifier(table);
        self.sql.push('.');
        self.push_identifier(column);
    }

    /// Appends a placeholder and records its bound value.
    ///
    /// In inline mode the value is written directly as a SQL literal and the
    /// parameter list is left untouched.
    pub fn push_bind(&mut self, value: Value) {
        if self.inline_values {
            self.write_value_literal(&value);
            return;
        }
        let index = self.params.len();
        self.dialect.placeholder(index, &mut self.sql);
        self.params.push(value);
    }

    /// Writes a value as an inline SQL literal.
    fn write_value_literal(&mut self, value: &Value) {
        match value {
            Value::Null => self.sql.push_str("NULL"),
            Value::Bool(flag) => self.sql.push_str(self.dialect.bool_literal(*flag)),
            Value::Int(number) => self.sql.push_str(&number.to_string()),
            Value::Real(number) => self.sql.push_str(&number.to_string()),
            Value::Text(text) => self.dialect.escape_string_literal(text, &mut self.sql),
            Value::Timestamp(ts) => {
                let text = ts
                    .format(&time::format_description::well_known::Rfc3339)
                    .unwrap_or_default();
                self.dialect.escape_string_literal(&text, &mut self.sql);
            }
            Value::Blob(bytes) => {
                self.sql.push_str("X'");
                for byte in bytes {
                    use std::fmt::Write;
                    let _ = write!(self.sql, "{byte:02x}");
                }
                self.sql.push('\'');
            }
            // PostgreSQL-specific values do not appear in the inline-literal contexts
            // (index predicates, DDL defaults); render a defensive quoted text form.
            Value::Json(json) => self
                .dialect
                .escape_string_literal(&json.to_string(), &mut self.sql),
            Value::Uuid(uuid) => self
                .dialect
                .escape_string_literal(&uuid.to_string(), &mut self.sql),
            Value::Array(items) => self
                .dialect
                .escape_string_literal(&format!("{items:?}"), &mut self.sql),
        }
    }

    /// Renders a boolean expression.
    pub fn write_expr(&mut self, expr: &Expr) {
        use crate::query::expr::BinaryOp;
        match expr {
            Expr::Column { table, column } => self.push_qualified(table, column),
            Expr::Value(value) => self.push_bind(value.clone()),
            Expr::Binary { left, op: BinaryOp::ILike, right } => self.write_ilike(left, right),
            // JSON access/containment render differently per dialect (PostgreSQL
            // operators vs MySQL JSON-path / JSON_CONTAINS).
            Expr::Binary { left, op: BinaryOp::JsonGet, right } => {
                self.write_json_get(left, right, false)
            }
            Expr::Binary { left, op: BinaryOp::JsonGetText, right } => {
                self.write_json_get(left, right, true)
            }
            Expr::Binary { left, op: BinaryOp::Contains, right } => self.write_contains(left, right),
            Expr::Binary { left, op, right } => {
                self.write_expr(left);
                self.sql.push(' ');
                self.push_sql(op.as_sql());
                self.sql.push(' ');
                self.write_expr(right);
            }
            Expr::Between { expr, low, high } => {
                self.write_expr(expr);
                self.push_sql(" BETWEEN ");
                self.write_expr(low);
                self.push_sql(" AND ");
                self.write_expr(high);
            }
            Expr::Logical { op, items } => self.write_logical(*op, items),
            Expr::Not(inner) => {
                self.push_sql("NOT (");
                self.write_expr(inner);
                self.sql.push(')');
            }
            Expr::InList { expr, values } => self.write_in_list(expr, values),
            Expr::IsNull { expr, negated } => {
                self.write_expr(expr);
                self.push_sql(if *negated { " IS NOT NULL" } else { " IS NULL" });
            }
            Expr::Aggregate { func, args, filter } => {
                self.push_sql(func.as_sql());
                self.sql.push('(');
                match filter {
                    // MySQL has no `FILTER (WHERE ...)`; emulate as
                    // `func(CASE WHEN cond THEN arg END)`.
                    Some(cond) if !self.dialect.supports_filter_clause() => {
                        if args.is_empty() {
                            self.push_sql("CASE WHEN ");
                            self.write_expr(cond);
                            self.push_sql(" THEN 1 END");
                        } else {
                            for (i, arg) in args.iter().enumerate() {
                                if i != 0 {
                                    self.push_sql(", ");
                                }
                                self.push_sql("CASE WHEN ");
                                self.write_expr(cond);
                                self.push_sql(" THEN ");
                                self.write_expr(arg);
                                self.push_sql(" END");
                            }
                        }
                        self.sql.push(')');
                    }
                    _ => {
                        for (i, arg) in args.iter().enumerate() {
                            if i != 0 {
                                self.push_sql(", ");
                            }
                            self.write_expr(arg);
                        }
                        self.sql.push(')');
                        if let Some(filter_expr) = filter {
                            self.push_sql(" FILTER (WHERE ");
                            self.write_expr(filter_expr);
                            self.sql.push(')');
                        }
                    }
                }
            }
            Expr::Func { name, args } => {
                self.push_function_name(name);
                self.sql.push('(');
                for (index, arg) in args.iter().enumerate() {
                    if index != 0 {
                        self.push_sql(", ");
                    }
                    self.write_expr(arg);
                }
                self.sql.push(')');
            }
            Expr::CountStar => self.push_sql("COUNT(*)"),
            Expr::Alias { expr, alias } => {
                self.write_expr(expr);
                self.push_sql(" AS ");
                self.push_identifier(alias);
            }
            Expr::Case { whens, else_expr } => {
                self.push_sql("CASE");
                for (cond, result) in whens {
                    self.push_sql(" WHEN ");
                    self.write_expr(cond);
                    self.push_sql(" THEN ");
                    self.write_expr(result);
                }
                if let Some(default) = else_expr {
                    self.push_sql(" ELSE ");
                    self.write_expr(default);
                }
                self.push_sql(" END");
            }
            Expr::Subquery(stmt) => {
                self.sql.push('(');
                self.write_select(stmt);
                self.sql.push(')');
            }
            Expr::InSubquery { expr, subquery, negated } => {
                self.write_expr(expr);
                self.push_sql(if *negated { " NOT IN (" } else { " IN (" });
                self.write_select(subquery);
                self.sql.push(')');
            }
            Expr::Raw { sql, params } => {
                // The raw SQL already contains its own `?` placeholders; just
                // record the bound values without emitting additional markers.
                self.push_sql(sql);
                for p in params {
                    self.params.push(p.clone());
                }
            }
            Expr::Exists { subquery, negated } => {
                self.push_sql(if *negated { "NOT EXISTS (" } else { "EXISTS (" });
                self.write_select(subquery);
                self.sql.push(')');
            }
            Expr::Excluded(column) => {
                if self.dialect.kind() == DialectKind::Mysql {
                    // MySQL refers to the would-be-inserted row via `VALUES(col)`.
                    self.push_sql("VALUES(");
                    self.push_identifier(column);
                    self.sql.push(')');
                } else {
                    // `EXCLUDED` is a keyword pseudo-table, left unquoted; the column
                    // is quoted normally. Accepted by PostgreSQL and SQLite (≥ 3.24).
                    self.push_sql("EXCLUDED.");
                    self.push_identifier(column);
                }
            }
            Expr::Extract { field, source } => {
                self.push_sql("EXTRACT(");
                self.push_sql(field);
                self.push_sql(" FROM ");
                self.write_expr(source);
                self.sql.push(')');
            }
            Expr::Over { expr, window } => {
                self.write_expr(expr);
                self.push_sql(" OVER (");
                let mut needs_space = false;

                if !window.partition_by.is_empty() {
                    self.push_sql("PARTITION BY ");
                    for (i, col) in window.partition_by.iter().enumerate() {
                        if i != 0 {
                            self.push_sql(", ");
                        }
                        self.write_expr(col);
                    }
                    needs_space = true;
                }

                if !window.order_by.is_empty() {
                    if needs_space {
                        self.sql.push(' ');
                    }
                    self.push_sql("ORDER BY ");
                    for (i, term) in window.order_by.iter().enumerate() {
                        if i != 0 {
                            self.push_sql(", ");
                        }
                        self.write_expr(&term.expr);
                        self.push_sql(if term.descending { " DESC" } else { " ASC" });
                        if let Some(nulls_first) = term.nulls {
                            self.push_sql(if nulls_first { " NULLS FIRST" } else { " NULLS LAST" });
                        }
                    }
                    needs_space = true;
                }

                if let Some(frame) = &window.frame {
                    if needs_space {
                        self.sql.push(' ');
                    }
                    self.push_sql(&frame.unit.to_string());
                    self.push_sql(" BETWEEN ");
                    self.write_window_bound(&frame.start);
                    self.push_sql(" AND ");
                    if let Some(end) = &frame.end {
                        self.write_window_bound(end);
                    } else {
                        self.push_sql("CURRENT ROW");
                    }
                }

                self.sql.push(')');
            }
        }
    }

    /// Writes a single window frame bound (value part only — the unit keyword
    /// like `ROWS`/`RANGE` is written separately).
    fn write_window_bound(&mut self, bound: &WindowBound) {
        match bound {
            WindowBound::UnboundedPreceding => self.push_sql("UNBOUNDED PRECEDING"),
            WindowBound::Preceding(expr) => {
                self.write_expr(expr);
                self.push_sql(" PRECEDING");
            }
            WindowBound::CurrentRow => self.push_sql("CURRENT ROW"),
            WindowBound::Following(expr) => {
                self.write_expr(expr);
                self.push_sql(" FOLLOWING");
            }
            WindowBound::UnboundedFollowing => self.push_sql("UNBOUNDED FOLLOWING"),
        }
    }

    /// Renders a `WITH [RECURSIVE] cte AS (query), ...` clause.
    fn write_with_clause(&mut self, with: &WithClause) {
        self.push_sql("WITH ");
        if with.recursive {
            self.push_sql("RECURSIVE ");
        }
        for (i, cte) in with.ctes.iter().enumerate() {
            if i != 0 {
                self.push_sql(", ");
            }
            self.push_identifier(cte.name);
            if let Some(cols) = &cte.columns {
                self.sql.push('(');
                for (j, col) in cols.iter().enumerate() {
                    if j != 0 {
                        self.push_sql(", ");
                    }
                    self.push_identifier(col);
                }
                self.sql.push(')');
            }
            self.push_sql(" AS (");
            match &cte.query {
                CteQuery::Select(stmt) => self.write_select(stmt),
                CteQuery::Union(stmt) => self.write_union(stmt),
            }
            self.sql.push(')');
        }
        self.sql.push(' ');
    }

    /// Renders a case-insensitive LIKE as `lower(left) LIKE lower(right)`.
    ///
    /// SQLite has no ILIKE keyword; the `lower()` wrapping makes the comparison
    /// case-insensitive for both ASCII and Unicode.
    fn write_ilike(&mut self, left: &Expr, right: &Expr) {
        self.push_sql("lower(");
        self.write_expr(left);
        self.push_sql(") LIKE lower(");
        self.write_expr(right);
        self.sql.push(')');
    }

    /// Renders a JSON field access (`->` / `->>`).
    ///
    /// When the key is a text literal it is inlined as a dialect-appropriate path:
    /// PostgreSQL uses the bare key (`-> 'k'`), MySQL a JSON path (`-> '$.k'`).
    fn write_json_get(&mut self, left: &Expr, right: &Expr, as_text: bool) {
        self.write_expr(left);
        self.push_sql(if as_text { " ->> " } else { " -> " });
        if let Expr::Value(Value::Text(key)) = right {
            let path = if self.dialect.kind() == DialectKind::Mysql {
                format!("$.{key}")
            } else {
                key.clone()
            };
            self.dialect.escape_string_literal(&path, &mut self.sql);
        } else {
            self.write_expr(right);
        }
    }

    /// Renders JSON containment: PostgreSQL `left @> right`, MySQL
    /// `JSON_CONTAINS(left, right)`.
    fn write_contains(&mut self, left: &Expr, right: &Expr) {
        if self.dialect.kind() == DialectKind::Mysql {
            self.push_sql("JSON_CONTAINS(");
            self.write_expr(left);
            self.push_sql(", ");
            self.write_expr(right);
            self.sql.push(')');
        } else {
            self.write_expr(left);
            self.push_sql(" @> ");
            self.write_expr(right);
        }
    }

    /// Renders an `AND`/`OR` group, parenthesizing when it joins more than one item.
    fn write_logical(&mut self, op: crate::query::expr::LogicalOp, items: &[Expr]) {
        use crate::query::expr::LogicalOp;
        match items {
            // An empty group is the connective's identity: AND of nothing is true,
            // OR of nothing is false.
            [] => self.push_sql(match op {
                LogicalOp::And => "1 = 1",
                LogicalOp::Or => "0 = 1",
            }),
            [single] => self.write_expr(single),
            many => {
                self.sql.push('(');
                for (index, item) in many.iter().enumerate() {
                    if index != 0 {
                        self.sql.push(' ');
                        self.push_sql(op.as_sql());
                        self.sql.push(' ');
                    }
                    self.write_expr(item);
                }
                self.sql.push(')');
            }
        }
    }

    /// Renders a membership test, collapsing an empty list to a false constant.
    fn write_in_list(&mut self, expr: &Expr, values: &[Value]) {
        if values.is_empty() {
            self.push_sql("0 = 1");
            return;
        }
        self.write_expr(expr);
        self.push_sql(" IN (");
        for (index, value) in values.iter().enumerate() {
            if index != 0 {
                self.push_sql(", ");
            }
            self.push_bind(value.clone());
        }
        self.sql.push(')');
    }

    /// Renders the `WHERE` clause for a statement's top-level filters.
    ///
    /// The filters are joined by `AND` without an outer parenthesis; any nested
    /// group renders its own parentheses.
    fn write_where(&mut self, filters: &[Expr]) {
        if filters.is_empty() {
            return;
        }
        self.push_sql(" WHERE ");
        for (index, filter) in filters.iter().enumerate() {
            if index != 0 {
                self.push_sql(" AND ");
            }
            self.write_expr(filter);
        }
    }

    /// Renders a `SELECT` statement.
    pub fn write_select(&mut self, statement: &SelectStatement) {
        if let Some(with) = &statement.with {
            self.write_with_clause(with);
        }
        self.push_sql("SELECT ");
        if !statement.distinct_on.is_empty() {
            self.push_sql("DISTINCT ON (");
            for (index, expr) in statement.distinct_on.iter().enumerate() {
                if index != 0 {
                    self.push_sql(", ");
                }
                self.write_expr(expr);
            }
            self.push_sql(") ");
        } else if statement.distinct {
            self.push_sql("DISTINCT ");
        }
        for (index, item) in statement.projection.iter().enumerate() {
            if index != 0 {
                self.push_sql(", ");
            }
            match item {
                SelectItem::Column { table, column } => self.push_qualified(table, column),
                SelectItem::Expression(expr) => self.write_expr(expr),
            }
        }
        self.push_sql(" FROM ");
        self.push_identifier(statement.table);
        for join in &statement.joins {
            self.sql.push(' ');
            self.push_sql(join.kind.as_sql());
            self.sql.push(' ');
            self.push_identifier(join.table);
            if let Some(alias) = join.alias {
                self.push_sql(" AS ");
                self.push_identifier(alias);
            }
            if join.kind != JoinKind::Cross {
                self.push_sql(" ON ");
                self.push_qualified(join.left_table, join.left_column);
                self.push_sql(" = ");
                self.push_qualified(join.right_table, join.right_column);
            }
        }
        self.write_where(&statement.filters);

        if !statement.group_by.is_empty() {
            self.push_sql(" GROUP BY ");
            for (index, expr) in statement.group_by.iter().enumerate() {
                if index != 0 {
                    self.push_sql(", ");
                }
                self.write_expr(expr);
            }
        }

        if let Some(having) = &statement.having {
            self.push_sql(" HAVING ");
            self.write_expr(having);
        }

        if !statement.order_by.is_empty() {
            self.push_sql(" ORDER BY ");
            for (index, term) in statement.order_by.iter().enumerate() {
                if index != 0 {
                    self.push_sql(", ");
                }
                self.write_expr(&term.expr);
                self.push_sql(if term.descending { " DESC" } else { " ASC" });
                if let Some(nulls_first) = term.nulls {
                    self.push_sql(if nulls_first { " NULLS FIRST" } else { " NULLS LAST" });
                }
            }
        }

        if let Some(limit) = statement.limit {
            self.push_sql(" LIMIT ");
            self.push_sql(&limit.to_string());
        }
        if let Some(offset) = statement.offset {
            self.push_sql(" OFFSET ");
            self.push_sql(&offset.to_string());
        }
        self.write_lock(statement.lock.as_ref());
    }

    /// Renders a row-level locking clause (`FOR UPDATE`/`FOR SHARE` with an
    /// optional `OF` list and wait policy).
    fn write_lock(&mut self, lock: Option<&LockClause>) {
        let Some(lock) = lock else {
            return;
        };
        self.push_sql(match lock.strength {
            LockStrength::Update => " FOR UPDATE",
            LockStrength::Share => " FOR SHARE",
        });
        if !lock.of.is_empty() {
            self.push_sql(" OF ");
            for (index, table) in lock.of.iter().enumerate() {
                if index != 0 {
                    self.push_sql(", ");
                }
                self.push_identifier(table);
            }
        }
        match lock.wait {
            LockWait::Wait => {}
            LockWait::NoWait => self.push_sql(" NOWAIT"),
            LockWait::SkipLocked => self.push_sql(" SKIP LOCKED"),
        }
    }

    /// Renders a `UNION` / `UNION ALL` of several `SELECT` statements.
    ///
    /// `ORDER BY`, `LIMIT`, and `OFFSET` appear once at the end and apply to
    /// the whole combined result.
    pub fn write_union(&mut self, statement: &UnionStatement) {
        self.write_select(&statement.first);
        for (is_all, stmt) in &statement.rest {
            self.push_sql(if *is_all { " UNION ALL " } else { " UNION " });
            self.write_select(stmt);
        }
        if !statement.order_by.is_empty() {
            self.push_sql(" ORDER BY ");
            for (index, term) in statement.order_by.iter().enumerate() {
                if index != 0 {
                    self.push_sql(", ");
                }
                self.write_expr(&term.expr);
                self.push_sql(if term.descending { " DESC" } else { " ASC" });
                if let Some(nulls_first) = term.nulls {
                    self.push_sql(if nulls_first { " NULLS FIRST" } else { " NULLS LAST" });
                }
            }
        }
        if let Some(limit) = statement.limit {
            self.push_sql(" LIMIT ");
            self.push_sql(&limit.to_string());
        }
        if let Some(offset) = statement.offset {
            self.push_sql(" OFFSET ");
            self.push_sql(&offset.to_string());
        }
        self.write_lock(statement.lock.as_ref());
    }

    /// Renders a `SELECT COUNT(*)` over a statement's table and filters.
    pub fn write_count(&mut self, statement: &SelectStatement) {
        self.push_sql("SELECT COUNT(*) FROM ");
        self.push_identifier(statement.table);
        self.write_where(&statement.filters);
    }

    /// Renders an existence check over a statement's table and filters.
    pub fn write_exists(&mut self, statement: &SelectStatement) {
        self.push_sql("SELECT EXISTS(SELECT 1 FROM ");
        self.push_identifier(statement.table);
        self.write_where(&statement.filters);
        self.sql.push(')');
    }

    /// Renders a comma-separated list of quoted conflict-target columns.
    fn render_conflict_target(&mut self, columns: &[&'static str]) {
        for (index, column) in columns.iter().enumerate() {
            if index != 0 {
                self.push_sql(", ");
            }
            self.push_identifier(column);
        }
    }

    /// Renders an `INSERT` statement.
    pub fn write_insert(&mut self, statement: &InsertStatement) {
        let is_mysql = self.dialect.kind() == DialectKind::Mysql;
        // MySQL spells "skip on conflict" as `INSERT IGNORE`.
        if is_mysql && matches!(statement.on_conflict, OnConflict::DoNothing { .. }) {
            self.push_sql("INSERT IGNORE INTO ");
        } else {
            self.push_sql("INSERT INTO ");
        }
        self.push_identifier(statement.table);
        self.push_sql(" (");
        for (index, column) in statement.columns.iter().enumerate() {
            if index != 0 {
                self.push_sql(", ");
            }
            self.push_identifier(column);
        }
        self.push_sql(") VALUES ");
        for (row_index, row) in statement.rows.iter().enumerate() {
            if row_index != 0 {
                self.push_sql(", ");
            }
            self.sql.push('(');
            for (value_index, value) in row.iter().enumerate() {
                if value_index != 0 {
                    self.push_sql(", ");
                }
                self.push_bind(value.clone());
            }
            self.sql.push(')');
        }
        match &statement.on_conflict {
            OnConflict::None => {}
            OnConflict::Update { constraint, updates } => {
                if is_mysql {
                    // MySQL has no conflict target; `EXCLUDED.col` renders as
                    // `VALUES(col)` (handled in `write_expr`).
                    self.push_sql(" ON DUPLICATE KEY UPDATE ");
                } else {
                    self.push_sql(" ON CONFLICT (");
                    self.render_conflict_target(constraint);
                    self.push_sql(") DO UPDATE SET ");
                }
                for (index, assignment) in updates.iter().enumerate() {
                    if index != 0 {
                        self.push_sql(", ");
                    }
                    self.push_identifier(assignment.column);
                    self.push_sql(" = ");
                    self.write_expr(&assignment.value);
                }
            }
            OnConflict::DoNothing { constraint } => {
                if is_mysql {
                    // Handled by the `INSERT IGNORE` prefix; nothing to append.
                } else {
                    self.push_sql(" ON CONFLICT ");
                    if !constraint.is_empty() {
                        self.sql.push('(');
                        self.render_conflict_target(constraint);
                        self.push_sql(") ");
                    }
                    self.push_sql("DO NOTHING");
                }
            }
        }
        if !statement.returning.is_empty() {
            self.push_sql(" RETURNING ");
            for (index, column) in statement.returning.iter().enumerate() {
                if index != 0 {
                    self.push_sql(", ");
                }
                self.push_identifier(column);
            }
        }
    }

    /// Renders an `UPDATE` statement.
    pub fn write_update(&mut self, statement: &UpdateStatement) {
        self.push_sql("UPDATE ");
        self.push_identifier(statement.table);
        self.push_sql(" SET ");
        for (index, assignment) in statement.assignments.iter().enumerate() {
            if index != 0 {
                self.push_sql(", ");
            }
            self.push_identifier(assignment.column);
            self.push_sql(" = ");
            self.write_expr(&assignment.value);
        }
        self.write_where(&statement.filters);
        if !statement.returning.is_empty() {
            self.push_sql(" RETURNING ");
            for (index, column) in statement.returning.iter().enumerate() {
                if index != 0 {
                    self.push_sql(", ");
                }
                self.push_identifier(column);
            }
        }
    }

    /// Renders a `DELETE` statement.
    pub fn write_delete(&mut self, statement: &DeleteStatement) {
        self.push_sql("DELETE FROM ");
        self.push_identifier(statement.table);
        self.write_where(&statement.filters);
        if !statement.returning.is_empty() {
            self.push_sql(" RETURNING ");
            for (index, column) in statement.returning.iter().enumerate() {
                if index != 0 {
                    self.push_sql(", ");
                }
                self.push_identifier(column);
            }
        }
    }

    /// Consumes the writer, returning the SQL string and its bound parameters.
    pub fn finish(self) -> (String, Vec<Value>) {
        (self.sql, self.params)
    }
}

/// Renders a `UNION` statement to SQL and bound parameters.
pub fn render_union(dialect: &dyn Dialect, statement: &UnionStatement) -> (String, Vec<Value>) {
    let mut writer = QueryWriter::new(dialect);
    writer.write_union(statement);
    writer.finish()
}

/// Renders a `SELECT` statement to SQL and bound parameters.
pub fn render_select(dialect: &dyn Dialect, statement: &SelectStatement) -> (String, Vec<Value>) {
    let mut writer = QueryWriter::new(dialect);
    writer.write_select(statement);
    writer.finish()
}

/// Renders a count query to SQL and bound parameters.
pub fn render_count(dialect: &dyn Dialect, statement: &SelectStatement) -> (String, Vec<Value>) {
    let mut writer = QueryWriter::new(dialect);
    writer.write_count(statement);
    writer.finish()
}

/// Renders an existence query to SQL and bound parameters.
pub fn render_exists(dialect: &dyn Dialect, statement: &SelectStatement) -> (String, Vec<Value>) {
    let mut writer = QueryWriter::new(dialect);
    writer.write_exists(statement);
    writer.finish()
}

/// Renders an `INSERT` statement to SQL and bound parameters.
pub fn render_insert(dialect: &dyn Dialect, statement: &InsertStatement) -> (String, Vec<Value>) {
    let mut writer = QueryWriter::new(dialect);
    writer.write_insert(statement);
    writer.finish()
}

/// Renders an `UPDATE` statement to SQL and bound parameters.
pub fn render_update(dialect: &dyn Dialect, statement: &UpdateStatement) -> (String, Vec<Value>) {
    let mut writer = QueryWriter::new(dialect);
    writer.write_update(statement);
    writer.finish()
}

/// Renders a `DELETE` statement to SQL and bound parameters.
pub fn render_delete(dialect: &dyn Dialect, statement: &DeleteStatement) -> (String, Vec<Value>) {
    let mut writer = QueryWriter::new(dialect);
    writer.write_delete(statement);
    writer.finish()
}

/// Renders a standalone boolean expression to SQL and its bound parameters.
///
/// A convenience over building a [`QueryWriter`] directly, used to render a
/// predicate (such as a `WHERE` clause) in isolation.
pub fn render_expr(dialect: &dyn Dialect, expr: &Expr) -> (String, Vec<Value>) {
    let mut writer = QueryWriter::new(dialect);
    writer.write_expr(expr);
    writer.finish()
}

/// Renders a boolean expression with its values inlined as SQL literals.
///
/// Used where parameters cannot be bound — notably a partial index's `WHERE`
/// clause in DDL. The returned string is self-contained; there are no parameters.
pub fn predicate_sql(dialect: &dyn Dialect, expr: &Expr) -> String {
    let mut writer = QueryWriter::new_inline(dialect);
    writer.write_expr(expr);
    writer.finish().0
}

/// Writes a single-quoted SQL string literal, doubling embedded quotes.
pub fn quote_string_literal(value: &str, out: &mut String) {
    quote_string_literal_with(value, out, false);
}

/// Like [`quote_string_literal`] but also escapes backslashes, for backends where
/// a backslash is an escape character inside single-quoted strings (MySQL, and
/// PostgreSQL with the non-default `standard_conforming_strings = off`). Without
/// this, a backslash before a quote could escape the doubled quote and break out
/// of the literal.
pub fn quote_string_literal_mysql(value: &str, out: &mut String) {
    quote_string_literal_with(value, out, true);
}

/// Writes `value` as a single-quoted SQL literal, doubling embedded quotes and,
/// when `escape_backslash` is set, doubling backslashes too.
fn quote_string_literal_with(value: &str, out: &mut String, escape_backslash: bool) {
    out.push('\'');
    for ch in value.chars() {
        match ch {
            '\'' => {
                out.push('\'');
                out.push('\'');
            }
            '\\' if escape_backslash => {
                out.push('\\');
                out.push('\\');
            }
            _ => out.push(ch),
        }
    }
    out.push('\'');
}
