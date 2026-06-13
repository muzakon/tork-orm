# 6. Relations and Preloading

Tork ORM offers declarative relationship definition and an eager preloading engine designed to solve the common "N+1 queries" problem.

---

## 1. Defining Relationships

Relationships are declared inside an `impl` block of a model struct, annotated with the `#[relations]` attribute. You define relationship accessor methods carrying either `#[has_many]` or `#[belongs_to]` attributes.

### A. One-to-Many (`#[has_many]`)
A parent model owns zero or more child models. Use `#[has_many(TargetModel, foreign_key = TargetModel::fk)]`.

### B. Many-to-One (`#[belongs_to]`)
A child model references a parent model. Use `#[belongs_to(ParentModel, foreign_key = Self::fk)]`.

```rust
use tork_orm::prelude::*;

#[derive(Debug, Clone, Model)]
#[table(name = "users")]
pub struct User {
    #[field(primary_key, auto)]
    pub id: i64,
    pub username: String,
}

#[derive(Debug, Clone, Model)]
#[table(name = "posts")]
pub struct Post {
    #[field(primary_key, auto)]
    pub id: i64,
    #[field(foreign_key = User::id)]
    pub user_id: i64,
    pub title: String,
    pub published: bool,
}

// Declare relations on User
#[relations]
impl User {
    #[has_many(Post, foreign_key = Post::user_id)]
    pub fn posts() {}
}

// Declare relations on Post
#[relations]
impl Post {
    #[belongs_to(User, foreign_key = Post::user_id)]
    pub fn author() {}
}
```

Calling these methods (e.g. `User::posts()`) returns a `Relation` descriptor which is used in joins and preloading. You can inspect relation metadata programmatically:

```rust
let rel = User::posts();
assert_eq!(rel.kind(), RelationKind::HasMany);
assert_eq!(rel.target_table(), "posts");
```

---

## 2. Filtering with Joins

You can filter parent models based on criteria from related child models by calling `.join()` on a `QuerySet`.

### Inner Join Filtering
```rust
// Find active users who have at least one published post
let users = User::query()
    .join(User::posts())
    .filter(Post::published.eq(true))
    .distinct() // Deduplicate parent rows
    .all(&db)
    .await?;
```

### The Importance of `.distinct()`
Because `join()` performs a standard SQL `INNER JOIN`, a parent row will be duplicated in the database result set for every matching child row (e.g. if Alice has 2 published posts, joining without `distinct()` yields her user row twice). 

Always call `.distinct()` on the query if you only want unique parent instances.

---

## 3. Preloading (Solving the N+1 Problem)

If you load a list of users and then query the database for each user's posts individually, you execute $N + 1$ queries. Tork ORM solves this by batch-fetching related records.

### How to Preload
Call `.preload()` on the `QuerySet` and pass the relation descriptor. The query returns a list of parents wrapped in `Preloaded<M>`:

```rust
// Fetches all users and preloads their posts
let users: Vec<Preloaded<User>> = User::query()
    .order_by(User::id.asc())
    .preload(User::posts())
    .all(&db)
    .await?;

for user in users {
    // 1. Deref directly accesses the parent model's fields
    println!("User: {}", user.username);

    // 2. Fetch the preloaded posts using get::<Target>()
    let user_posts: &[Post] = user.get::<Post>();
    for post in user_posts {
        println!("  Post: {}", post.title);
    }
}
```

### Preloading Customization (Filters & Order)
You can filter or sort the preloaded relation using query chaining on the relation descriptor itself:

```rust
let users = User::query()
    .preload(
        User::posts()
            .filter(Post::published.eq(true)) // Only load published posts
            .order_by(Post::id.desc())        // Newest first
    )
    .all(&db)
    .await?;
```

### How Preloading Optimizes Queries
When preloading, Tork ORM aggregates all parent primary keys and runs exactly **one additional batch query** per preloaded relationship (using a SQL `IN` query), rather than executing a query for each parent.

```rust
let before = db.statement_count();

let users = User::query()
    .preload(User::posts())
    .all(&db)
    .await?;

let queries_run = db.statement_count() - before;
// Exactly 2 queries were executed:
// Query 1: SELECT * FROM users
// Query 2: SELECT * FROM posts WHERE user_id IN (?, ?, ...)
assert_eq!(queries_run, 2);
```

The `IN (...)` list binds one parameter per distinct parent key, and databases cap how many parameters one statement may carry (SQLite 999, PostgreSQL and MySQL 65535). When you preload more parents than that ceiling, Tork ORM transparently splits the keys into chunks and runs one batch query per chunk, then stitches every child back onto its parent. Preloading 10,000 parents simply runs a few batch queries instead of failing with `too many SQL variables`.

### Many-to-One Preloading
Preloading works identically in reverse:

```rust
let posts = Post::query()
    .preload(Post::author()) // Preload the author User for each post
    .all(&db)
    .await?;

for post in posts {
    let authors: &[User] = post.get::<User>();
    if let Some(author) = authors.first() {
        println!("Post '{}' was written by {}", post.title, author.username);
    }
}
```

### Two Relations to the Same Type (`get_via`)
`get::<C>()` looks rows up by their model type, which is ambiguous when a parent has **two relations to the same child type** (for example an `Article` with both an `author` and an `editor`, each a `User`). Preload both, then read each one with `get_via`, passing the specific relation:

```rust
let articles = Article::query()
    .preload(Article::author())
    .preload(Article::editor())
    .all(&db)
    .await?;

for article in &articles {
    let author = article.get_via(&Article::author());
    let editor = article.get_via(&Article::editor());
    // Each relation keeps its own rows; neither overwrites the other.
}
```

`get_via` distinguishes the relations by their join columns. `get::<C>()` still works when there is only one relation to a type; reach for `get_via` only when a parent preloads more than one relation to the same type.
