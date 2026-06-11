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
