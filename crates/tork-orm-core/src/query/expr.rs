//! The boolean expression AST used in `WHERE` and `HAVING` clauses.
//!
//! Expressions are built by the typed [`Column`](crate::Column) handles and the
//! query builder's filter combinators, then rendered to SQL plus an ordered list
//! of bound parameters by a [`Dialect`](crate::dialect::Dialect). Keeping the AST
//! backend-neutral is what lets one set of queries target any dialect.

use crate::query::ast::{OrderItem, SelectStatement};
use crate::value::{BindValue, Value};

/// An aggregate function applied to an expression.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AggFunc {
    /// `COUNT`
    Count,
    /// `SUM`
    Sum,
    /// `AVG`
    Avg,
    /// `MIN`
    Min,
    /// `MAX`
    Max,
}

impl AggFunc {
    /// Returns the SQL name of this function.
    pub fn as_sql(self) -> &'static str {
        match self {
            AggFunc::Count => "COUNT",
            AggFunc::Sum => "SUM",
            AggFunc::Avg => "AVG",
            AggFunc::Min => "MIN",
            AggFunc::Max => "MAX",
        }
    }
}

/// A binary comparison operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOp {
    /// `=`
    Eq,
    /// `<>`
    Ne,
    /// `>`
    Gt,
    /// `>=`
    Ge,
    /// `<`
    Lt,
    /// `<=`
    Le,
    /// `+` — addition.
    Add,
    /// `-` — subtraction.
    Sub,
    /// `*` — multiplication.
    Mul,
    /// `/` — division.
    Div,
    /// `%` — remainder (modulo).
    Mod,
    /// `LIKE` — pattern match (case-sensitive on most backends).
    Like,
    /// `ILIKE` — case-insensitive pattern match.
    ///
    /// On SQLite, rendered as `lower(col) LIKE lower(pattern)` since SQLite has
    /// no native ILIKE keyword. This covers non-ASCII Unicode correctly.
    ILike,
}

impl BinaryOp {
    /// Returns the SQL spelling of this operator.
    pub fn as_sql(self) -> &'static str {
        match self {
            BinaryOp::Eq => "=",
            BinaryOp::Ne => "<>",
            BinaryOp::Gt => ">",
            BinaryOp::Ge => ">=",
            BinaryOp::Lt => "<",
            BinaryOp::Le => "<=",
            BinaryOp::Add => "+",
            BinaryOp::Sub => "-",
            BinaryOp::Mul => "*",
            BinaryOp::Div => "/",
            BinaryOp::Mod => "%",
            BinaryOp::Like => "LIKE",
            BinaryOp::ILike => "ILIKE",
        }
    }
}

/// A logical connective joining several expressions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogicalOp {
    /// `AND`
    And,
    /// `OR`
    Or,
}

impl LogicalOp {
    /// Returns the SQL spelling of this connective.
    pub fn as_sql(self) -> &'static str {
        match self {
            LogicalOp::And => "AND",
            LogicalOp::Or => "OR",
        }
    }
}

/// A boolean expression over columns and bound values.
#[derive(Debug, Clone)]
pub enum Expr {
    /// A qualified column reference, `"table"."column"`.
    Column {
        /// The owning table.
        table: &'static str,
        /// The column name.
        column: &'static str,
    },
    /// A bound literal value.
    Value(Value),
    /// A binary comparison between two expressions.
    Binary {
        /// The left operand.
        left: Box<Expr>,
        /// The operator.
        op: BinaryOp,
        /// The right operand.
        right: Box<Expr>,
    },
    /// Several expressions joined by `AND` or `OR`.
    Logical {
        /// The connective.
        op: LogicalOp,
        /// The joined expressions.
        items: Vec<Expr>,
    },
    /// The negation of an expression.
    Not(Box<Expr>),
    /// A membership test, `expr IN (values...)`.
    InList {
        /// The tested expression.
        expr: Box<Expr>,
        /// The candidate values.
        values: Vec<Value>,
    },
    /// A null test, `expr IS [NOT] NULL`.
    IsNull {
        /// The tested expression.
        expr: Box<Expr>,
        /// Whether the test is negated (`IS NOT NULL`).
        negated: bool,
    },
    /// An aggregate over an expression, `FUNC(expr)`.
    Aggregate {
        /// The aggregate function.
        func: AggFunc,
        /// The aggregated expression.
        arg: Box<Expr>,
    },
    /// A scalar function call, `name(arg, ...)`, such as `lower(email)`.
    Func {
        /// The function name, emitted verbatim.
        name: String,
        /// The call arguments, in order.
        args: Vec<Expr>,
    },
    /// `COUNT(*)`.
    CountStar,
    /// An aliased expression, `expr AS "alias"`.
    Alias {
        /// The aliased expression.
        expr: Box<Expr>,
        /// The output name.
        alias: &'static str,
    },
    /// A range check, `expr BETWEEN low AND high` (inclusive on both ends).
    Between {
        /// The tested expression.
        expr: Box<Expr>,
        /// The lower bound.
        low: Box<Expr>,
        /// The upper bound.
        high: Box<Expr>,
    },
    /// `CASE WHEN cond THEN result ... [ELSE default] END`.
    Case {
        /// The condition-result pairs, evaluated in order.
        whens: Vec<(Expr, Expr)>,
        /// The fallback expression when no condition matches.
        else_expr: Option<Box<Expr>>,
    },
    /// A scalar subquery — `(SELECT ...)`.
    ///
    /// Can appear anywhere an expression is expected: comparisons, projections,
    /// `HAVING` clauses. The caller is responsible for ensuring the subquery
    /// returns at most one row and one column.
    Subquery(Box<SelectStatement>),
    /// A membership test against a subquery — `col IN (SELECT ...)` or
    /// `col NOT IN (SELECT ...)`.
    InSubquery {
        /// The expression to test.
        expr: Box<Expr>,
        /// The subquery that produces candidate values.
        subquery: Box<SelectStatement>,
        /// Whether the test is negated (`NOT IN`).
        negated: bool,
    },
    /// A verbatim SQL fragment with pre-bound parameters.
    ///
    /// Use [`QuerySet::filter_raw`](crate::QuerySet) for WHERE predicates that
    /// the builder cannot express. Use [`Expr::raw`] for column-free constant
    /// fragments (`CURRENT_TIMESTAMP`, `RANDOM()`, etc.) that take no parameters.
    Raw {
        /// The raw SQL text, emitted verbatim. Write `?` for each bound value.
        sql: String,
        /// Bound parameters, matched positionally to each `?` placeholder.
        params: Vec<Value>,
    },
    /// An existence test — `EXISTS (SELECT ...)` or `NOT EXISTS (SELECT ...)`.
    ///
    /// Typically used with a correlated subquery that references a column from
    /// the outer query via [`Column::expr`](crate::Column::expr).
    Exists {
        /// The subquery to test.
        subquery: Box<SelectStatement>,
        /// `true` → `NOT EXISTS`.
        negated: bool,
    },
}

/// Builder for a `CASE WHEN` expression.
///
/// Constructed via [`Expr::case()`]; finalized by calling [`end`](Self::end).
///
/// # Examples
///
/// ```
/// use tork_orm_core::query::expr::Expr;
/// use tork_orm_core::Value;
///
/// let expr = Expr::case()
///     .when(Expr::value(Value::Bool(true)), Expr::value(Value::Int(1)))
///     .else_(Expr::value(Value::Int(0)))
///     .end();
/// # let _ = expr;
/// ```
pub struct CaseWhen {
    whens: Vec<(Expr, Expr)>,
    else_expr: Option<Box<Expr>>,
}

impl CaseWhen {
    /// Adds a `WHEN cond THEN result` branch.
    pub fn when(mut self, cond: Expr, result: Expr) -> Self {
        self.whens.push((cond, result));
        self
    }

    /// Sets the `ELSE default` fallback.
    pub fn else_(mut self, default: Expr) -> Self {
        self.else_expr = Some(Box::new(default));
        self
    }

    /// Finalizes the expression into an [`Expr::Case`] node.
    pub fn end(self) -> Expr {
        Expr::Case {
            whens: self.whens,
            else_expr: self.else_expr,
        }
    }
}

impl Expr {
    /// Builds a column reference.
    pub fn column(table: &'static str, column: &'static str) -> Self {
        Expr::Column { table, column }
    }

    /// Builds a bound value.
    pub fn value(value: Value) -> Self {
        Expr::Value(value)
    }

    /// Builds a binary comparison.
    pub fn binary(left: Expr, op: BinaryOp, right: Expr) -> Self {
        Expr::Binary {
            left: Box::new(left),
            op,
            right: Box::new(right),
        }
    }

    /// Joins expressions with `AND`.
    ///
    /// An empty input is the always-true expression.
    pub fn all(items: impl IntoIterator<Item = Expr>) -> Self {
        Expr::Logical {
            op: LogicalOp::And,
            items: items.into_iter().collect(),
        }
    }

    /// Joins expressions with `OR`.
    ///
    /// An empty input is the always-false expression.
    pub fn any(items: impl IntoIterator<Item = Expr>) -> Self {
        Expr::Logical {
            op: LogicalOp::Or,
            items: items.into_iter().collect(),
        }
    }

    /// Negates an expression.
    // This is a constructor taking an expression, not a `self` method, so it does
    // not conflict with `std::ops::Not` in practice; the name mirrors `all`/`any`.
    #[allow(clippy::should_implement_trait)]
    pub fn not(expr: Expr) -> Self {
        Expr::Not(Box::new(expr))
    }

    /// Builds a membership test.
    pub fn in_list(expr: Expr, values: Vec<Value>) -> Self {
        Expr::InList {
            expr: Box::new(expr),
            values,
        }
    }

    /// Builds a null test (`negated` selects `IS NOT NULL`).
    pub fn is_null(expr: Expr, negated: bool) -> Self {
        Expr::IsNull {
            expr: Box::new(expr),
            negated,
        }
    }

    /// Builds an aggregate over an expression.
    pub fn aggregate(func: AggFunc, arg: Expr) -> Self {
        Expr::Aggregate {
            func,
            arg: Box::new(arg),
        }
    }

    /// Builds a scalar function call, `name(args...)`.
    ///
    /// The name is emitted verbatim, so it must be a valid SQL function for the
    /// target backend. Used for functional indexes and function predicates, for
    /// example `Expr::func("lower", [Expr::column("users", "email")])`.
    pub fn func(name: impl Into<String>, args: impl IntoIterator<Item = Expr>) -> Self {
        Expr::Func {
            name: name.into(),
            args: args.into_iter().collect(),
        }
    }

    /// Builds a range check (`BETWEEN low AND high`, inclusive on both ends).
    pub fn between(expr: Expr, low: Expr, high: Expr) -> Self {
        Expr::Between {
            expr: Box::new(expr),
            low: Box::new(low),
            high: Box::new(high),
        }
    }

    /// Starts a `CASE WHEN` chain.
    ///
    /// Add branches with [`CaseWhen::when`] and finalize with [`CaseWhen::end`].
    pub fn case() -> CaseWhen {
        CaseWhen {
            whens: Vec::new(),
            else_expr: None,
        }
    }

    /// Wraps a `SelectStatement` into a scalar subquery expression `(SELECT ...)`.
    ///
    /// The returned expression can appear anywhere an expression is valid. The
    /// most common way to build the statement is via [`QuerySet::to_subquery`].
    pub fn subquery(stmt: SelectStatement) -> Self {
        Expr::Subquery(Box::new(stmt))
    }

    /// Builds a `col IN (SELECT ...)` or `col NOT IN (SELECT ...)` test.
    ///
    /// Prefer the typed [`Column::in_subquery`] and [`Column::not_in_subquery`]
    /// helpers over this constructor when a typed column is available.
    pub fn in_subquery(expr: Expr, stmt: SelectStatement, negated: bool) -> Self {
        Expr::InSubquery {
            expr: Box::new(expr),
            subquery: Box::new(stmt),
            negated,
        }
    }

    /// Embeds a verbatim SQL fragment with no bound parameters.
    ///
    /// Reserved for column-free constants and database built-ins that have no
    /// typed builder equivalent (`CURRENT_TIMESTAMP`, `RANDOM()`, etc.). The
    /// string is emitted exactly as written — no quoting, no parameter binding.
    ///
    /// For parameterized raw WHERE predicates use
    /// [`QuerySet::filter_raw`](crate::QuerySet) instead.
    pub fn raw(sql: impl Into<String>) -> Self {
        Expr::Raw { sql: sql.into(), params: Vec::new() }
    }

    /// Builds `EXISTS (SELECT ...)` — true when the subquery returns any row.
    ///
    /// Use a correlated subquery to test a relationship from the outer row:
    /// ```ignore
    /// User::query()
    ///     .filter(Expr::exists(
    ///         Post::query().filter(Post::user_id.eq(User::id.expr())),
    ///     ))
    /// ```
    pub fn exists<X: crate::model::Model>(qs: crate::query::queryset::QuerySet<X>) -> Self {
        Expr::Exists { subquery: Box::new(qs.into_statement()), negated: false }
    }

    /// Builds `NOT EXISTS (SELECT ...)` — true when the subquery returns no rows.
    pub fn not_exists<X: crate::model::Model>(qs: crate::query::queryset::QuerySet<X>) -> Self {
        Expr::Exists { subquery: Box::new(qs.into_statement()), negated: true }
    }

    /// Aliases this expression, `expr AS "alias"`.
    ///
    /// Used in a projection so the output column has a stable name to map onto a
    /// [`QueryResult`](crate::FromRow) field.
    pub fn as_(self, alias: &'static str) -> Self {
        Expr::Alias {
            expr: Box::new(self),
            alias,
        }
    }

    /// Builds a comparison of this expression against a bound value.
    ///
    /// Handy for `HAVING` over an aggregate, for example
    /// `Post::id.count().gt(3)`.
    fn compare(self, op: BinaryOp, value: impl BindValue) -> Self {
        Expr::binary(self, op, Expr::Value(value.to_value()))
    }

    /// `expr = value`
    pub fn eq(self, value: impl BindValue) -> Self {
        self.compare(BinaryOp::Eq, value)
    }

    /// `expr <> value`
    pub fn ne(self, value: impl BindValue) -> Self {
        self.compare(BinaryOp::Ne, value)
    }

    /// `expr > value`
    pub fn gt(self, value: impl BindValue) -> Self {
        self.compare(BinaryOp::Gt, value)
    }

    /// `expr >= value`
    pub fn ge(self, value: impl BindValue) -> Self {
        self.compare(BinaryOp::Ge, value)
    }

    /// `expr < value`
    pub fn lt(self, value: impl BindValue) -> Self {
        self.compare(BinaryOp::Lt, value)
    }

    /// `expr <= value`
    pub fn le(self, value: impl BindValue) -> Self {
        self.compare(BinaryOp::Le, value)
    }

    /// `expr + rhs`
    pub fn add(self, rhs: Expr) -> Self {
        Expr::binary(self, BinaryOp::Add, rhs)
    }

    /// `expr - rhs`
    pub fn sub(self, rhs: Expr) -> Self {
        Expr::binary(self, BinaryOp::Sub, rhs)
    }

    /// `expr * rhs`
    pub fn mul(self, rhs: Expr) -> Self {
        Expr::binary(self, BinaryOp::Mul, rhs)
    }

    /// `expr / rhs`
    pub fn div(self, rhs: Expr) -> Self {
        Expr::binary(self, BinaryOp::Div, rhs)
    }

    /// `expr % rhs`
    pub fn rem(self, rhs: Expr) -> Self {
        Expr::binary(self, BinaryOp::Mod, rhs)
    }

    /// Orders by this expression ascending.
    pub fn asc(self) -> OrderItem {
        OrderItem::new(self, false)
    }

    /// Orders by this expression descending.
    pub fn desc(self) -> OrderItem {
        OrderItem::new(self, true)
    }
}
