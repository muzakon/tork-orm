# 4. Querying and QuerySets

Tork ORM uses a chainable query builder called `QuerySet`. Queries are built using the auto-generated column handles on your model struct. Since these handles are typed, database column comparisons are validated at compile time.

---

## 1. Initializing a Query

To start building a query, call the `query()` method on your model:

```rust
let query = User::query();
```

All queries are executed against a database connection handle (e.g. `&db` where `db: Database`).

---

## 2. Filtering Results

Filters restrict the rows returned by the query. You can combine multiple conditions using AND, OR, and NOT connectives.

### A. The `filter` Method (Implicit AND)
Calling `filter` applies an `AND` constraint. You can chain multiple `filter` calls:

```rust
// WHERE is_active = 1 AND username = 'alice'
let users = User::query()
    .filter(User::is_active.eq(true))
    .filter(User::username.eq("alice"))
    .all(&db)
    .await?;
```

Supported column comparison methods include:
- `.eq(val)`: Column equals value (`=`).
- `.ne(val)`: Column does not equal value (`<>`).
- `.gt(val)`: Column is greater than value (`>`).
- `.ge(val)`: Column is greater than or equal to value (`>=`).
- `.lt(val)`: Column is less than value (`<`).
- `.le(val)`: Column is less than or equal to value (`<=`).

### B. The `filter_any` Method (OR Group)
Pass an array or iterator of conditions to `filter_any` to wrap them in an `OR` group.

```rust
// WHERE is_active = 1 AND (username = 'alice' OR username = 'bob')
let users = User::query()
    .filter(User::is_active.eq(true))
    .filter_any([
        User::username.eq("alice"),
        User::username.eq("bob"),
    ])
    .all(&db)
    .await?;
```

### C. The `filter_not` Method (NOT Negation)
Negates the passed constraint.

```rust
// WHERE NOT (is_active = 1)
let users = User::query()
    .filter_not(User::is_active.eq(true))
    .all(&db)
    .await?;
```

---

## 3. Sorting (Ordering)

Use the `order_by` method to sort results. Pass a column handle with `.asc()` or `.desc()` modifiers.

```rust
// ORDER BY id DESC
let users = User::query()
    .order_by(User::id.desc())
    .all(&db)
    .await?;

// ORDER BY username ASC
let users = User::query()
    .order_by(User::username.asc())
    .all(&db)
    .await?;
```

---

## 4. Pagination (Limit and Offset)

Restrict the number of rows fetched, or skip a certain number of rows for pagination:

- `limit(n)`: Limits the query to at most `n` rows.
- `offset(n)`: Skips the first `n` rows of the result set.

```rust
// Fetch users 11 to 30 (page 2 of 20 items per page)
let users = User::query()
    .order_by(User::id.asc())
    .limit(20)
    .offset(10)
    .all(&db)
    .await?;
```

---

## 5. Execution Terminals

A `QuerySet` does not run any SQL until you call one of its "terminal" methods. Each terminal is asynchronous and takes a reference to the `Database` handle.

### `.all(&db)`
Executes the query and returns all matching rows as a list of models:
```rust
let users: Vec<User> = User::query().all(&db).await?;
```

### `.first(&db)`
Fetches the first row matching the query, returning `None` if no rows match:
```rust
let user: Option<User> = User::query()
    .filter(User::username.eq("alice"))
    .first(&db)
    .await?;
```

### `.one(&db)`
Fetches exactly one row matching the query. If no row matches, or if multiple rows match, it returns an error:
```rust
let user: Result<User> = User::query()
    .filter(User::username.eq("alice"))
    .one(&db)
    .await?;

match user {
    Ok(u) => println!("Found: {}", u.username),
    Err(e) if e.kind() == ErrorKind::NotFound => println!("No such user"),
    Err(e) if e.kind() == ErrorKind::MultipleFound => println!("Ambiguous search"),
    Err(e) => println!("Database error: {}", e),
}
```

### `.count(&db)`
Executes a `COUNT(*)` query matching the filter conditions and returns the total as an `i64`:
```rust
let active_count: i64 = User::query()
    .filter(User::is_active.eq(true))
    .count(&db)
    .await?;
```

### `.exists(&db)`
Checks if any rows match the query. It returns a boolean and is optimized to stop scanning immediately upon finding a match:
```rust
let exists: bool = User::query()
    .filter(User::username.eq("alice"))
    .exists(&db)
    .await?;
```

---

## 6. Pattern Matching

String columns expose `.like()` for pattern matching and `.ilike()` for case-insensitive matching. Both methods are only available on `String` (and `Option<String>`) columns — using them on a numeric column is a compile error.

**Wildcards:** `%` matches any sequence of characters; `_` matches exactly one character. Wildcards are not escaped automatically.

```rust
// Prefix match: finds "alice" but not "bob"
let users = User::query()
    .filter(User::username.like("ali%"))
    .all(&db)
    .await?;

// Case-insensitive: finds "alice" even when searching for "ALICE"
let users = User::query()
    .filter(User::username.ilike("ALICE"))
    .all(&db)
    .await?;

// Substring, case-insensitive
let users = User::query()
    .filter(User::email.ilike("%@example.com"))
    .all(&db)
    .await?;
```

On SQLite, `ilike` is rendered as `lower(column) LIKE lower(pattern)` because SQLite has no native `ILIKE` keyword. This also makes the comparison case-insensitive for non-ASCII Unicode characters.

---

## 7. Range Filters

`.between(low, high)` emits a `BETWEEN` predicate. Both bounds are **inclusive**. The method is available on any column type that can be compared, including integers, floats, and strings (lexicographic order).

```rust
// Integer range (inclusive)
let users = User::query()
    .filter(User::id.between(10_i64, 20_i64))
    .all(&db)
    .await?;
// SQL: WHERE "users"."id" BETWEEN ? AND ?  params: [10, 20]

// String range (lexicographic)
let users = User::query()
    .filter(User::username.between("a", "m"))
    .all(&db)
    .await?;
```

When `low > high`, no rows match (SQL `BETWEEN` is always false for an inverted range).

---

## 8. NOT IN

`.not_in(values)` excludes rows whose column value appears in the provided list.

```rust
// Exclude specific usernames
let users = User::query()
    .filter(User::username.not_in(["alice", "bob"]))
    .all(&db)
    .await?;
// SQL: WHERE NOT ("users"."username" IN (?, ?))

// Exclude a dynamic list
let banned_ids: Vec<i64> = vec![3, 7, 12];
let users = User::query()
    .filter(User::id.not_in(banned_ids))
    .all(&db)
    .await?;
```

An empty list matches **all rows** (`NOT (0 = 1)` is always true). This mirrors the behaviour of `.in_list([])`, which matches no rows.

---

## 9. Grouping and Aggregates

Use `.group_by()` to collapse rows into groups and `.having()` to filter those groups. Combined with `.select()` and `.all_as::<T>()`, this covers the full SQL aggregate pattern.

### group_by

`.group_by()` accepts a single expression, a tuple of expressions, or any iterator of expressions.

```rust
// Count posts per user
#[derive(QueryResult)]
struct PostCount {
    user_id: i64,
    post_count: i64,
}

let stats = User::query()
    .join(User::posts())
    .select((
        User::id.as_("user_id"),
        Post::id.count().as_("post_count"),
    ))
    .group_by(User::id)
    .order_by(Post::id.count().desc())
    .all_as::<PostCount>(&db)
    .await?;
```

### having

`.having()` filters groups after aggregation. It takes any `Expr`, typically an aggregate expression with a comparison:

```rust
// Only users with more than two posts
let stats = User::query()
    .join(User::posts())
    .select((
        User::id.as_("user_id"),
        User::username,
        Post::id.count().as_("post_count"),
    ))
    .group_by((User::id, User::username))
    .having(Post::id.count().gt(2_i64))
    .order_by(Post::id.count().desc())
    .all_as::<PostCount>(&db)
    .await?;
// SQL: ...GROUP BY "users"."id", "users"."username"
//         HAVING COUNT("posts"."id") > ?
```

`.having()` without a prior `.group_by()` is valid SQL and filters the implicit single group (equivalent to a `WHERE` on an aggregate).

---

## 10. Outer Joins

`.join()` performs an `INNER JOIN` and drops any left-side rows that have no matching right-side row. `.left_join()` performs a `LEFT JOIN` and keeps every left-side row, filling the right-side columns with `NULL` when no match exists.

```rust
#[derive(QueryResult)]
struct UserWithCount {
    id: i64,
    username: String,
    post_count: i64,
}

// All users, including those with zero posts.
let rows = User::query()
    .left_join(User::posts())
    .select((
        User::id,
        User::username,
        Post::id.count().as_("post_count"),
    ))
    .group_by((User::id, User::username))
    .all_as::<UserWithCount>(&db)
    .await?;
// SQL: SELECT ... FROM "users" LEFT JOIN "posts" ON "users"."id" = "posts"."user_id"
//      GROUP BY "users"."id", "users"."username"
// A user with no posts appears with post_count = 0 (COUNT returns 0 for NULLs).
```

Use `.join()` when you only want rows that have at least one related record. Use `.left_join()` when you want all rows regardless of whether a related record exists.

---

## 11. Arithmetic

Numeric columns (`i64`, `i32`, `f64`) expose arithmetic methods that produce an `Expr`. The result can be used in `.select()`, `.filter()`, or `.having()`.

```rust
// Scale a column in a projection
#[derive(QueryResult)]
struct PostViews {
    id: i64,
    doubled_views: i64,
}

let rows = Post::query()
    .select((
        Post::id,
        Post::view_count.mul(2_i64).as_("doubled_views"),
    ))
    .all_as::<PostViews>(&db)
    .await?;
// SQL: SELECT "posts"."id", "posts"."view_count" * ? AS "doubled_views" FROM "posts"

// All five operators
let a = Post::view_count.add(10_i64);  // view_count + 10
let b = Post::view_count.sub(1_i64);   // view_count - 1
let c = Post::view_count.mul(2_i64);   // view_count * 2
let d = Post::view_count.div(4_i64);   // view_count / 4
let e = Post::view_count.rem(7_i64);   // view_count % 7
```

To combine two columns arithmetically, call `.expr()` to get an `Expr` first and then use the `Expr` arithmetic methods:

```rust
// Add two columns together (no bound parameter)
let total = Post::view_count.expr().add(Post::id.expr());
// SQL fragment: "posts"."view_count" + "posts"."id"
```

Arithmetic expressions can be chained: `.mul(2).add(Expr::value(Value::Int(1)))` renders `col * ? + ?`.

---

## 12. CASE / WHEN

`Expr::case()` starts a builder that produces a `CASE WHEN ... END` expression. Call `.when(condition, result)` for each branch, optionally `.else_(default)`, and finalize with `.end()`.

```rust
// Map is_active to a display string
let status_label = Expr::case()
    .when(
        User::is_active.eq(true),
        Expr::value(Value::Text("active".into())),
    )
    .when(
        User::is_active.eq(false),
        Expr::value(Value::Text("inactive".into())),
    )
    .else_(Expr::value(Value::Text("unknown".into())))
    .end()
    .as_("status");

#[derive(QueryResult)]
struct UserStatus {
    id: i64,
    username: String,
    status: String,
}

let rows = User::query()
    .select((User::id, User::username, status_label))
    .all_as::<UserStatus>(&db)
    .await?;
// SQL: SELECT ..., CASE WHEN ... THEN ? WHEN ... THEN ? ELSE ? END AS "status"
```

When `.else_()` is omitted, `NULL` is returned for rows that match no branch (standard SQL behavior). Branches are evaluated top to bottom and the first match wins.

---

## 13. Ordering Extras

### NULLS FIRST / NULLS LAST

By default, `NULL` placement in `ORDER BY` is database-defined (SQLite puts `NULL` before non-null values when sorting ascending). To make the placement explicit, chain `.nulls_first()` or `.nulls_last()` onto any `OrderItem`:

```rust
// NULLs sorted last in an ascending column (common production default)
let users = User::query()
    .order_by(User::nickname.asc().nulls_last())
    .all(&db)
    .await?;
// SQL: ORDER BY "users"."nickname" ASC NULLS LAST

// NULLs sorted first in a descending column
let posts = Post::query()
    .order_by(Post::view_count.desc().nulls_first())
    .all(&db)
    .await?;
// SQL: ORDER BY "posts"."view_count" DESC NULLS FIRST
```

Without `.nulls_first()` or `.nulls_last()`, no `NULLS` clause is emitted and the database uses its own default.

---

## 14. String Helpers

String columns expose convenience methods that build `LIKE` and `ILIKE` patterns for the most common text searches. They save you from writing the `%` wildcards by hand and are only available on `String` (and `Option<String>`) columns.

| Method | Pattern | Example |
|---|---|---|
| `starts_with(s)` | `s%` | username starts with "ali" |
| `ends_with(s)` | `%s` | email ends with "@example.com" |
| `contains(s)` | `%s%` | bio contains "rust" |
| `istarts_with(s)` | case-insensitive `s%` | username starts with "ALI" (any case) |
| `iends_with(s)` | case-insensitive `%s` | email ends with "@EXAMPLE.COM" |
| `icontains(s)` | case-insensitive `%s%` | bio contains "Rust" (any case) |

```rust
// Find users whose username starts with "ali"
User::query()
    .filter(User::username.starts_with("ali"))
    .all(&db)
    .await?;

// Find users whose email ends with a given domain
User::query()
    .filter(User::email.ends_with("@example.com"))
    .all(&db)
    .await?;

// Substring search
User::query()
    .filter(User::username.contains("ali"))
    .all(&db)
    .await?;

// Case-insensitive variants (SQLite: lower(col) LIKE lower(?))
User::query()
    .filter(User::username.icontains("ALICE"))
    .all(&db)
    .await?;
```

---

## 15. Subqueries

Subqueries let you use the result of one query inside another. There are two forms: a membership test (`IN (SELECT ...)`) and a scalar subquery used as an expression value.

### A. IN Subquery

`Column::in_subquery` builds a `WHERE col IN (SELECT ...)` predicate. The inner query can be filtered and projected like any other `QuerySet`.

```rust
// Fetch posts whose author is an active user.
let posts = Post::query()
    .filter(Post::user_id.in_subquery(
        User::query()
            .filter(User::is_active.eq(true))
            .select((User::id,)),
    ))
    .all(&db)
    .await?;
```

Use `not_in_subquery` for the inverse:

```rust
// Fetch posts whose author has been deactivated.
let orphaned = Post::query()
    .filter(Post::user_id.not_in_subquery(
        User::query()
            .filter(User::is_active.eq(true))
            .select((User::id,)),
    ))
    .all(&db)
    .await?;
```

### B. Scalar Subquery

`QuerySet::to_subquery` converts a query into a `(SELECT ...)` expression that can be used anywhere an `Expr` is accepted — a filter, a projection, or a `HAVING` clause.

```rust
// Fetch posts with above-average view count.
let avg_views = Post::query()
    .select((Post::view_count.avg().as_("avg"),))
    .to_subquery();

let popular = Post::query()
    .filter(Expr::binary(
        Post::view_count.expr(),
        BinaryOp::Gt,
        avg_views,
    ))
    .all(&db)
    .await?;
```

---

## 16. Scalar Functions

Tork ORM ships a set of common scalar SQL functions. Each is available as a free function (imported via `use tork_orm::prelude::*`) and, for the most common column types, as a method on the column handle.

### Numeric functions

| Free function | Column method | SQL | Description |
|---|---|---|---|
| `round(expr)` | `col.round()` | `round(expr)` | Round to nearest integer |
| `ceil(expr)` | `col.ceil()` | `ceil(expr)` | Ceiling (smallest integer >= value) |
| `floor(expr)` | `col.floor()` | `floor(expr)` | Floor (largest integer <= value) |
| `abs(expr)` | `col.abs()` | `abs(expr)` | Absolute value |

```rust
Post::query()
    .select((Post::price.round().as_("rounded"),))
    .all_as::<PriceRow>(&db)
    .await?;
```

### String functions

| Free function | Column method | SQL | Description |
|---|---|---|---|
| `lower(expr)` | `col.lower()` | `lower(expr)` | Lowercase |
| `upper(expr)` | `col.upper()` | `upper(expr)` | Uppercase |
| `trim(expr)` | `col.trim()` | `trim(expr)` | Strip leading/trailing whitespace |
| `length(expr)` | `col.length()` | `length(expr)` | Character count |
| `substr(expr, start)` | `col.substr(start)` | `substr(expr, start)` | Substring from `start` (1-based) |
| `substr_len(expr, start, len)` | `col.substr_len(start, len)` | `substr(expr, start, len)` | Substring of length `len` |
| `concat(args)` | — | `concat(args...)` | Concatenate two or more strings |

```rust
// First three characters of each username
User::query()
    .select((User::username.substr_len(1, 3).as_("prefix"),))
    .all_as::<PrefixRow>(&db)
    .await?;

// Combine first and last name
Post::query()
    .select((concat([Expr::from(User::first_name), Expr::from(User::last_name)]).as_("full_name"),))
    .all_as::<NameRow>(&db)
    .await?;
```

### Null-handling

| Free function | SQL | Description |
|---|---|---|
| `coalesce(a, b)` | `coalesce(a, b)` | First non-NULL value |

### Escape hatch

`func("name", [args...])` calls any SQL function not covered by the built-ins:

```rust
let expr = func("date", [Expr::column("events", "created_at")]);
```

The `i`-prefixed variants delegate to `.ilike()`, which on SQLite renders as `lower(column) LIKE lower(pattern)`. Wildcards inside the search string (`%`, `_`) are **not** escaped — use `.like()` directly if you need to pass a pattern with explicit wildcards.
