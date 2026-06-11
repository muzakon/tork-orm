# 7. Aggregates, Grouping, and Projections

While standard queries map database rows directly to model structs, Tork ORM also allows selecting custom projections, calculating aggregates, grouping results, and mapping them to lightweight Data Transfer Objects (DTOs).

---

## 1. Custom Projections (`select`)

By default, a `QuerySet` selects all columns defined on the model. Use the `select` method to restrict columns or compute expressions.

The `select` method accepts a tuple of column or expression handles. You can alias any expression using `.as_("alias_name")`:

```rust
// Select only ids and usernames
let query = User::query()
    .select((User::id, User::username));
```

---

## 2. Aggregate Functions

You can invoke SQL aggregate functions directly on column handles:

| Function | SQL Translation | Example |
|---|---|---|
| `.count()` | `COUNT(column)` | `Post::id.count().as_("total")` |
| `.sum()` | `SUM(column)` | `Post::view_count.sum().as_("total_views")` |
| `.avg()` | `AVG(column)` | `Post::view_count.avg().as_("average_views")` |
| `.min()` | `MIN(column)` | `Post::view_count.min().as_("lowest_views")` |
| `.max()` | `MAX(column)` | `Post::view_count.max().as_("highest_views")` |

---

## 3. Mapping to DTOs (`#[derive(QueryResult)]` & `all_as`)

When you project custom fields or aggregates, the result set no longer matches the fields of the original model. In Tork ORM, you handle this by defining a target struct annotated with `#[derive(QueryResult)]` and calling `all_as::<Target>()`.

### Detailed Example: User Post Stats
Suppose you want to query all active users, join their posts, group by user, and calculate the count of posts and the sum of view counts.

First, define your DTO struct:
```rust
#[derive(Debug, QueryResult)]
pub struct UserPostStats {
    pub user_id: i64,
    pub username: String,
    pub post_count: i64,
    pub total_views: i64,
}
```

Next, build and run the query:
```rust
let stats = User::query()
    .select((
        User::id.as_("user_id"),
        User::username,
        Post::id.count().as_("post_count"),
        Post::view_count.sum().as_("total_views"),
    ))
    .join(User::posts())
    .filter(User::is_active.eq(true))
    .group_by((User::id, User::username))
    .having(Post::id.count().gt(2)) // Only users with more than 2 posts
    .order_by(Post::view_count.sum().desc()) // Order by views descending
    .all_as::<UserPostStats>(&db) // Map to DTO
    .await?;

for user_stat in stats {
    println!(
        "User {} (ID: {}) has {} posts with a total of {} views.",
        user_stat.username, user_stat.user_id, user_stat.post_count, user_stat.total_views
    );
}
```

---

## 4. Group By and Having

As shown in the example above:

- `.group_by(tuple)`: Groups rows by one or more columns. It takes a tuple of column handles, e.g., `group_by((User::id, User::username))`.
- `.having(expression)`: Filters groups based on an aggregate condition. It takes a query expression, e.g., `having(Post::id.count().gt(2))`.

---

## 5. Simple Aggregate Example

If you only want a single aggregate result, you can map it to a simple one-field DTO:

```rust
#[derive(QueryResult)]
struct TotalCount {
    total: i64,
}

let result = Post::query()
    .select((Post::id.count().as_("total"),))
    .all_as::<TotalCount>(&db)
    .await?;

println!("Total posts: {}", result[0].total);
```
