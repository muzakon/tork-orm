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

## 10. Joins

The ORM exposes four join kinds. All except `cross_join` take a `Relation` argument so the join condition is inferred from the foreign key.

| Method | SQL | When to use |
|---|---|---|
| `.join(rel)` | `INNER JOIN` | Only rows matched on both sides |
| `.left_join(rel)` | `LEFT JOIN` | All left rows; unmatched right is `NULL` |
| `.right_join(rel)` | `RIGHT JOIN` | All right rows; unmatched left is `NULL` |
| `.full_join(rel)` | `FULL OUTER JOIN` | All rows from both sides |
| `.cross_join::<C>()` | `CROSS JOIN` | Cartesian product; no `ON` condition |

`RIGHT JOIN` and `FULL OUTER JOIN` require SQLite 3.39 or later. The AST supports them for forward compatibility.

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

// Cartesian product: every size paired with every color.
// cross_join takes a type parameter instead of a Relation.
let pairs = Size::query()
    .cross_join::<Color>()
    .all(&db)
    .await?;
// SQL: SELECT ... FROM "sizes" CROSS JOIN "colors"
```

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

---

## 17. Raw SQL

When the query builder cannot express a condition, use `filter_raw` to inject a raw SQL predicate. Params are plain Rust values — no `Value::` wrapping required. Write `?` for each placeholder.

```rust
// Filter by a database function the builder does not cover
User::query()
    .filter_raw("LENGTH(username) > ?", [5_i64])
    .all(&db)
    .await?;

// Multiple params, different types — pass Value explicitly for mixed types
User::query()
    .filter_raw("created_at > ? AND created_at < ?", [
        Value::Text("2024-01-01".into()),
        Value::Text("2025-01-01".into()),
    ])
    .all(&db)
    .await?;
```

For a SQL fragment without placeholders, use `Expr::raw` as a filter value instead:

```rust
// No-param raw expression on the RHS of a comparison
User::query()
    .filter(User::created_at.lt(Expr::raw("date('now', '-30 days')")))
    .all(&db)
    .await?;

// Constant in a projection
User::query()
    .select((Expr::raw("CURRENT_TIMESTAMP").as_("now"),))
    .all_as::<NowRow>(&db)
    .await?;
```

> `Expr::raw` is reserved for column-free, no-param constants. Use `filter_raw` whenever you need to bind values.

---

## 18. EXISTS / NOT EXISTS Subquery

`Expr::exists` and `Expr::not_exists` test whether a correlated subquery returns any rows. They are most useful when paired with a reference to the outer row's column via `.expr()`.

```rust
// Users who have written at least one post
User::query()
    .filter(Expr::exists(
        Post::query().filter(Post::user_id.eq(User::id.expr())),
    ))
    .all(&db)
    .await?;

// Users who have never written a post
User::query()
    .filter(Expr::not_exists(
        Post::query().filter(Post::user_id.eq(User::id.expr())),
    ))
    .all(&db)
    .await?;
```

The outer table's column is referenced with `.expr()` — it is emitted as a bare `"table"."column"` reference inside the subquery. There is no automatic correlation or join; the semantics are exactly what you write.

`EXISTS` can also be used with a non-correlated subquery as a guard:

```rust
// Only run the outer query if there is at least one active config row
Settings::query()
    .filter(Expr::exists(Config::query().filter(Config::active.eq(true))))
    .all(&db)
    .await?;
```

---

## 19. UNION / UNION ALL

`.union(other)` combines the rows of two queries, removing duplicates. `.union_all(other)` preserves duplicates. Both return a `UnionQuery<M>` that supports `ORDER BY`, `LIMIT`, `OFFSET`, `all`, `first`, and `count`.

```rust
// Active users UNION inactive users — deduplicates (redundant here, but shows the API).
let all_users = User::query()
    .filter(User::is_active.eq(true))
    .union(User::query().filter(User::is_active.eq(false)))
    .all(&db)
    .await?;
// SQL: SELECT ... FROM "users" WHERE ... UNION SELECT ... FROM "users" WHERE ...

// UNION ALL preserves every row, including duplicates.
let with_dupes = User::query()
    .filter(User::is_active.eq(true))
    .union_all(User::query().filter(User::is_active.eq(true)))
    .all(&db)
    .await?;

// Chain more than two branches.
let three_way = User::query()
    .filter(User::username.eq("alice"))
    .union(User::query().filter(User::username.eq("bob")))
    .union(User::query().filter(User::username.eq("carol")))
    .order_by(User::id.asc())
    .limit(2)
    .all(&db)
    .await?;
// ORDER BY and LIMIT apply to the whole combined result.

// COUNT wraps the union in a derived table.
let total = User::query()
    .filter(User::is_active.eq(true))
    .union(User::query().filter(User::is_active.eq(false)))
    .count(&db)
    .await?;
```

The ORDER BY and LIMIT / OFFSET clauses on `UnionQuery` apply to the whole result, not to any individual branch. If you need to sort or limit inside a branch, build that `QuerySet` separately first.

---

## 20. Shorthand Lookups

The ORM provides several shortcuts for the most common lookup patterns.

### A. `find` — Primary Key Lookup

`Model::find(executor, pk)` returns the row with the given primary key, or an error if no such row exists:

```rust
// Fetch user by primary key (errors with NotFound if the row is missing).
let user: User = User::find(&db, 1).await?;
println!("Found: {}", user.username);

// The PK can be any BindValue: i64, i32, String, &str, etc.
let user = User::find(&db, 42_i64).await?;
```

It is equivalent to:
```rust
User::query().filter(User::id.eq(42)).one(&db).await
```

### B. `get_or_none` — Optional Primary Key Lookup

`Model::get_or_none(executor, pk)` returns `Some(row)` when the key exists and `None` when it does not. Like `find`, it detects and errors on multiple matches (which should never happen for a primary key):

```rust
if let Some(user) = User::get_or_none(&db, 1).await? {
    println!("User exists: {}", user.username);
} else {
    println!("No user with that id");
}
```

### C. `one_or_none` — QuerySet Terminal

`QuerySet::one_or_none()` returns `Ok(None)` when no row matches, `Ok(Some(m))` when exactly one matches, and errors with `MultipleFound` when more than one matches. It differs from `first` (which silently adds `LIMIT 1`) and from `one` (which errors on both zero and multiple):

```rust
// Returns None if the username does not exist; errors if it is ambiguous.
let user: Option<User> = User::query()
    .filter(User::username.eq("alice"))
    .one_or_none(&db)
    .await?;

match user {
    Some(u) => println!("Found: {}", u.username),
    None => println!("No matching user"),
}
```

---

## 21. Pluck — Value Extraction

Extract a single column's values as a flat `Vec<T>` without defining a DTO struct. Useful for building ID lists, dropdown options, or simple lookups.

```rust
// All usernames as Vec<String>.
let names: Vec<String> = User::query()
    .order_by(User::id.asc())
    .pluck(&db, User::username)
    .await?;

// Active user IDs as Vec<i64>.
let active_ids: Vec<i64> = User::query()
    .filter(User::is_active.eq(true))
    .pluck(&db, User::id)
    .await?;

// Distinct status values.
let statuses: Vec<bool> = User::query()
    .distinct()
    .pluck(&db, User::is_active)
    .await?;

// Pluck respects filters, ordering, and pagination.
let page: Vec<String> = User::query()
    .order_by(User::id.asc())
    .limit(10)
    .offset(20)
    .pluck(&db, User::username)
    .await?;

// An empty result set returns an empty Vec.
let nobody: Vec<String> = User::query()
    .filter(User::username.eq("nobody"))
    .pluck(&db, User::username)
    .await?;
assert!(nobody.is_empty());
```

---

## 22. Pagination

Raw `limit`/`offset` works fine for simple cases, but when you need page metadata (total count, page count, etc.) the `paginate` helper runs both queries and packages everything into a [`Page`] value.

```rust
use tork_orm::Page;

// Page 2 of users, 10 per page.
let page: Page<User> = User::query()
    .order_by(User::id.asc())
    .paginate(&db, 2, 10)
    .await?;

println!("Showing {}-{} of {} (page {} of {})",
    (page.page - 1) * page.page_size + 1,
    (page.page - 1) * page.page_size + page.items.len() as u64,
    page.total,
    page.page,
    page.pages,
);

for user in &page.items {
    println!("{}", user.username);
}
```

The [`Page`] struct carries everything you need to render pagination UI:

| Field | Type | Description |
|-------|------|-------------|
| `items` | `Vec<M>` | The rows on this page |
| `total` | `i64` | Total number of matching rows |
| `page` | `u64` | Current page number (1-based) |
| `page_size` | `u64` | Items per page |
| `pages` | `u64` | Total number of pages |

The page number is clamped: page 0 behaves like page 1, and a page beyond the end returns the last page. A result set with zero rows has `page = 1`, `pages = 1`, and an empty `items`.

Combine with [`select`](#7-grouping-and-aggregates) and [`paginate_as`] to paginate projections:

```rust
#[derive(QueryResult)]
struct UserSummary {
    id: i64,
    username: String,
}

let page: Page<UserSummary> = User::query()
    .select((User::id, User::username))
    .order_by(User::id.asc())
    .paginate_as::<UserSummary, _>(&db, 1, 20)
    .await?;
```

---

## 23. `none()` — Empty Result Set

`.none()` adds a `0 = 1` filter that guarantees the query returns zero rows. Useful as a starting point that only gets populated once filters are applied, or as a placeholder branch in a union:

```rust
let empty = User::query().none().all(&db).await?;
assert!(empty.is_empty());
```

## 24. Row-Level Locking (`for_update`, `for_share`, ...)

`.for_update()` appends `FOR UPDATE` to the `SELECT`, locking matched rows against concurrent modification until the transaction commits. Must be used inside a transaction.

```rust
let user = User::query()
    .filter(User::id.eq(42))
    .for_update()
    .one(&db)
    .await?;
```

`.for_share()` takes a weaker shared lock (`FOR SHARE`) that still allows concurrent reads. Three modifiers refine the wait behavior, and each implies `FOR UPDATE` when used on its own:

- `.skip_locked()` skips rows already locked by another transaction (`SKIP LOCKED`), the classic job-queue pattern.
- `.nowait()` fails immediately instead of waiting (`NOWAIT`).
- `.lock_of(&[Table::TABLE])` restricts the lock to specific tables (`OF ...`).

```rust
// Claim the next available job without blocking on locked rows.
let jobs = Job::query()
    .filter(Job::state.eq(JobState::Pending))
    .order_by(Job::id.asc())
    .limit(1)
    .for_update()
    .skip_locked()
    .all(&db)
    .await?;
```

A bare `FOR UPDATE` runs on every backend with row locking (PostgreSQL, MySQL, SQLite 3.54+). The modifiers (`for_share`, `skip_locked`, `nowait`, `lock_of`) require PostgreSQL or MySQL; using them on SQLite returns a clear error at execution time.

## 25. `chunk(size)` — Batch Processing

`.chunk(size)` loads all matching rows in batches using offset-based pagination. Returns `Vec<Vec<M>>` — each inner `Vec` is one batch of up to `size` rows. The last batch may be smaller. Useful for memory-constrained processing of large result sets:

```rust
let batches = User::query()
    .filter(User::is_active.eq(true))
    .order_by(User::id.asc())
    .chunk(&db, 100)
    .await?;

for batch in batches {
    for user in batch {
        println!("{}", user.username);
    }
}
```

**Eager loading:** all batches are fetched before the method returns; there is no server-side cursor. Each batch runs a separate `SELECT` with `LIMIT size OFFSET n` against the same query.

## 26. `distinct_on(...)` — Distinct On (PostgreSQL)

`.distinct_on((cols...))` keeps only the first row of each group of the given expressions, ordered by the query's `ORDER BY`. It is the idiomatic way to fetch "the top row per group".

```rust
// The cheapest product in each category.
let cheapest = Product::query()
    .distinct_on((Product::category,))
    .order_by(Product::category.asc())
    .order_by(Product::price.asc())
    .all(&db)
    .await?;
```

`DISTINCT ON` is a PostgreSQL feature. On SQLite or MySQL the query is rejected with a clear error at execution time.

## 27. Keyset (Seek) Pagination

Offset pagination (Section 22) grows slower as the offset increases, because the database still scans and discards the skipped rows. Keyset pagination instead remembers the last row seen and seeks past it, staying fast at any depth.

Pass the ordering-key values of the last row of the previous page to `.keyset_after(cursor)`. The cursor holds one `Value` per `ORDER BY` term, in the same order; build them with `BindValue::to_value`.

```rust
// First page.
let page1 = Article::query()
    .order_by(Article::published_at.desc())
    .order_by(Article::id.desc())
    .limit(20)
    .all(&db)
    .await?;

// Next page: seek past the last row of page 1.
if let Some(last) = page1.last() {
    let cursor = vec![last.published_at.to_value(), last.id.to_value()];
    let page2 = Article::query()
        .order_by(Article::published_at.desc())
        .order_by(Article::id.desc())
        .keyset_after(cursor)
        .limit(20)
        .all(&db)
        .await?;
}
```

`.keyset_after` produces a lexicographic comparison such as `(published_at < ?) OR (published_at = ? AND id < ?)`, honoring each term's `ASC`/`DESC` direction. `.keyset_before(cursor)` walks backwards. Include a final unique column (such as the primary key) in the ordering so the cursor is unambiguous.

## 28. Soft-Delete Query Scope

When a model declares a `#[field(deleted_at)]` column (see [Defining Models](02-models.md)), every query is scoped to non-deleted rows by default: the ORM adds `WHERE deleted_at IS NULL` automatically, including inside subqueries and unions.

```rust
// Only live rows (default).
let live = Note::query().all(&db).await?;

// Include soft-deleted rows.
let everything = Note::query().with_deleted().all(&db).await?;

// Only the soft-deleted rows.
let trashed = Note::query().only_deleted().all(&db).await?;
```

`find` and `get_or_none` respect the default scope too, so a soft-deleted row is reported as not found unless you opt in with `with_deleted()`. The delete and restore operations are covered in [Writes](05-writes.md).
