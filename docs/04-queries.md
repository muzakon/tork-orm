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
