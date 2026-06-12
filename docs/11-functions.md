# 11. Scalar Functions

Scalar functions transform a single value per row. Tork ORM provides a built-in set that covers the most common SQL operations. Each function is available both as a free function (call it with any expression) and, for the relevant column types, as a method on the column handle.

All names are imported by `use tork_orm::prelude::*`.

---

## Numeric functions

These methods are available on any `Column<M, T>` where `T` is numeric (`i64`, `i32`, or `f64`).

| Function | SQL | Description |
|---|---|---|
| `round(expr)` / `col.round()` | `round(expr)` | Nearest integer |
| `ceil(expr)` / `col.ceil()` | `ceil(expr)` | Smallest integer >= value |
| `floor(expr)` / `col.floor()` | `floor(expr)` | Largest integer <= value |
| `abs(expr)` / `col.abs()` | `abs(expr)` | Absolute value |

```rust
use tork_orm::prelude::*;

#[derive(Debug, Clone, Model)]
#[table(name = "products")]
struct Product {
    #[field(primary_key, auto)]
    id: i64,
    price: f64,
}

#[derive(Debug, QueryResult)]
struct PriceRow {
    rounded: f64,
    floored: f64,
}

// Column method form
let rows = Product::query()
    .select((
        Product::price.round().as_("rounded"),
        Product::price.floor().as_("floored"),
    ))
    .all_as::<PriceRow>(&db)
    .await?;

// Free function form — identical SQL output
let expr = round(Product::price);
let expr = ceil(Product::price);
let expr = floor(Product::price);
```

---

## String functions

These methods are available on `Column<M, String>` (or `Column<M, Option<String>>`).

| Function | SQL | Description |
|---|---|---|
| `lower(expr)` / `col.lower()` | `lower(expr)` | Lowercase |
| `upper(expr)` / `col.upper()` | `upper(expr)` | Uppercase |
| `trim(expr)` / `col.trim()` | `trim(expr)` | Strip leading/trailing whitespace |
| `length(expr)` / `col.length()` | `length(expr)` | Character count |
| `substr(expr, start)` / `col.substr(start)` | `substr(expr, start)` | Substring from position `start` (1-based, to end of string) |
| `substr_len(expr, start, len)` / `col.substr_len(start, len)` | `substr(expr, start, len)` | Substring of exactly `len` characters starting at `start` |
| `concat(args)` | `concat(args...)` | Concatenate two or more expressions |

```rust
#[derive(Debug, Clone, Model)]
#[table(name = "users")]
struct User {
    #[field(primary_key, auto)]
    id: i64,
    username: String,
    email: String,
}

// Normalize before comparison
User::query()
    .filter(User::email.lower().eq("admin@example.com"))
    .one(&db)
    .await?;

// Extract domain (everything after '@')
// SQLite instr() returns position of first match
let at_pos = func("instr", [Expr::from(User::email), Expr::value(Value::Text("@".into()))]);
let domain = substr(User::email, at_pos.add(Expr::value(Value::Int(1))));

// First three characters of username
User::query()
    .select((User::username.substr_len(1, 3).as_("prefix"),))
    .all_as::<PrefixRow>(&db)
    .await?;

// Concatenate first_name and last_name
let full_name = concat([Expr::from(User::first_name), Expr::from(User::last_name)]);
```

---

## Null-handling

| Function | SQL | Description |
|---|---|---|
| `coalesce(a, b)` | `coalesce(a, b)` | Returns `a` if not NULL, otherwise `b` |

```rust
// Fall back to "anonymous" when the display_name column is NULL.
let name_expr = coalesce(User::display_name, Expr::value(Value::Text("anonymous".into())));
```

---

## Escape hatch

`func("name", args)` emits any SQL function not covered by the built-ins. The name is written verbatim.

```rust
// SQLite date() scalar
let date_expr = func("date", [Expr::column("events", "created_at")]);

// Three-argument replace()
let replaced = func("replace", [
    Expr::from(User::email),
    Expr::value(Value::Text("@old.com".into())),
    Expr::value(Value::Text("@new.com".into())),
]);
```

---

## Composing functions

Function results are `Expr` values, so they can be nested, compared, and used anywhere an expression is accepted — including `filter`, `select`, `order_by`, `group_by`, and `having`.

```rust
// Order by the length of the username.
User::query()
    .order_by(User::username.length().desc())
    .all(&db)
    .await?;

// Filter rows where the trimmed username matches.
User::query()
    .filter(User::username.trim().eq("alice"))
    .all(&db)
    .await?;
```
