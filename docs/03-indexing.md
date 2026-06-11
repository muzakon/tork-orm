# 3. Indexing A to Z

Tork ORM features a robust, dialect-aware indexing engine. Indexes can be declared at the field level, at the table level (for compound, functional, partial, and database-specific options), or built programmatically via the migration DDL builder.

---

## 1. Field-Level Indexes

The simplest way to index a single column is directly on the field definition inside the model struct.

### Standard Index
Annotate a field with `#[field(index)]`. This generates a standard index named `<table_name>_<column_name>_idx`.
```rust
#[derive(Model)]
#[table(name = "users")]
struct User {
    #[field(primary_key, auto)]
    id: i64,
    #[field(index)]
    email: String, // Generates "users_email_idx"
}
```

### Unique Index
Annotate a field with `#[field(unique)]`. This generates a unique constraint/index named `<table_name>_<column_name>_key`.
```rust
#[derive(Model)]
#[table(name = "users")]
struct User {
    #[field(primary_key, auto)]
    id: i64,
    #[field(varchar(length = 50), unique)]
    username: String, // Generates UNIQUE INDEX "users_username_key"
}
```

### Foreign Key Auto-Indexing
By default, any field carrying `#[field(foreign_key = ...)]` automatically gets a non-unique index. This is because relational databases require index support on foreign keys to optimize joins and check constraints.
```rust
#[derive(Model)]
#[table(name = "posts")]
struct Post {
    #[field(primary_key, auto)]
    id: i64,
    #[field(foreign_key = User::id)]
    user_id: i64, // Generates "posts_user_id_idx" automatically
}
```

#### Opting Out of Foreign Key Auto-Indexing
If you write queries that never filter by a foreign key or if write-throughput is critical, you can disable the auto-index with `index = false`:
```rust
#[derive(Model)]
#[table(name = "events")]
struct Event {
    #[field(primary_key, auto)]
    id: i64,
    #[field(foreign_key = User::id, index = false)]
    user_id: i64, // No automatic index is generated
}
```

---

## 2. Table-Level Indexes

For complex scenarios—like compound columns, specific sorting rules, partial indexing, or expression-based indexes—Tork ORM provides the `indexes` list inside the `#[table(...)]` attribute.

Table-level index declarations follow the syntax `index(...)` or `unique(...)`.

### A. Compound Indexes
A compound index indexes multiple columns in a specific order. Order matters; queries filtering on a prefix of the columns will utilize the index.
```rust
#[derive(Model)]
#[table(name = "posts", indexes = [
    index(fields = [user_id, status, created_at])
])]
struct Post {
    id: i64,
    user_id: i64,
    status: String,
    created_at: i64,
}
```

### B. Column Ordering (ASC / DESC)
You can specify the sorting order of individual columns in an index. For example, sorting timestamps in descending order helps speed up queries sorted by newest-first.
```rust
#[derive(Model)]
#[table(name = "posts", indexes = [
    index(fields = [user_id, created_at(desc)]) // user_id ASC, created_at DESC
])]
struct Post {
    id: i64,
    user_id: i64,
    created_at: i64,
}
```

### C. Nulls Placement (`nulls_first` / `nulls_last`)
Control where null values are grouped relative to non-null values. This is essential for matching specific ORDER BY queries.
```rust
#[derive(Model)]
#[table(name = "users", indexes = [
    index(fields = [score(desc, nulls_last)])
])]
struct User {
    id: i64,
    score: Option<i64>,
}
```

### D. Collation
Collation determines how text is compared. For instance, SQLite's `NOCASE` allows case-insensitive indexes.
```rust
#[derive(Model)]
#[table(name = "users", indexes = [
    index(fields = [display_name(collate = "NOCASE")])
])]
struct User {
    id: i64,
    display_name: String,
}
```

### E. Partial Indexes (`where` Predicate)
A partial index only indexes rows that satisfy a specific boolean predicate. This saves disk space and increases insert performance by excluding irrelevant rows (e.g., draft posts or inactive users).

The `where` predicate utilizes the model's column handles and standard query expressions:
```rust
#[derive(Model)]
#[table(name = "posts", indexes = [
    // Index created_at only for published posts
    index(fields = [created_at(desc)], where = status.eq("published"))
])]
struct Post {
    id: i64,
    status: String,
    created_at: i64,
}
```

### F. Functional / Expression Indexes
Instead of indexing a column value directly, you can index the result of a function or expression computed from one or more columns. Tork ORM supports both method-style and free-function style expressions.

```rust
#[derive(Model)]
#[table(name = "users", indexes = [
    // Unique index on the lowercase email (method form)
    unique(fields = [ expr(email.lower()) ]),
    
    // Index on uppercase username (free-function form)
    index(name = "idx_users_upper_username", fields = [ expr(upper(username)) ]),
    
    // Complex expression combining columns and filters
    index(name = "idx_admin", fields = [ id ], where = email.lower().eq("admin@x.com"))
])]
struct User {
    id: i64,
    username: String,
    email: String,
}
```
Under SQLite, these render using double parentheses to wrap the expression:
```sql
CREATE UNIQUE INDEX "users_expr_key" ON "users" ((lower("users"."email")));
```

### G. PostgreSQL-Only Features

Tork ORM allows declaring PostgreSQL-specific index features in model metadata. However, trying to render these for the `SqliteDialect` will return a compile or run-time validation error.

#### Index Methods (`using`)
Specify database-specific indexing methods like `gin` (Generalized Inverted Index) or `gist`.
```rust
#[derive(Model)]
#[table(name = "documents", indexes = [
    index(fields = [body], using = "gin")
])]
struct Document {
    id: i64,
    body: String,
}
```
*Note: SQLite does not support methods other than B-Tree, so this will trigger an error if applied to SQLite.*

#### Covering Indexes (`include`)
Include non-key columns directly in the leaf nodes of the index to allow "index-only scans".
```rust
#[derive(Model)]
#[table(name = "documents", indexes = [
    index(fields = [owner_id], include = [title])
])]
struct Document {
    id: i64,
    owner_id: i64,
    title: String,
}
```
*Note: SQLite does not support the `INCLUDE` clause, and will return an error at render time.*

---

## 3. Automatic Index Suppression Rules

To prevent bloated schemas and redundant indexes, Tork ORM enforces two automated suppression rules:

1. **Table-Index Prefix Suppression:** If you define a table-level index (such as a compound index) whose **first column** is a foreign key, the ORM will suppress the automatic single-column foreign key index for that field.
   ```rust
   #[derive(Model)]
   #[table(name = "posts", indexes = [
       index(fields = [user_id, status]) // Starts with user_id
   ])]
   struct Post {
       id: i64,
       #[field(foreign_key = User::id)]
       user_id: i64, // Auto index "posts_user_id_idx" is suppressed!
       status: String,
   }
   ```
2. **Explicit Attribute Suppression:** If you define a unique index directly on a foreign key field, the non-unique automatic index is suppressed:
   ```rust
   #[derive(Model)]
   #[table(name = "tags")]
   struct Tag {
       id: i64,
       #[field(foreign_key = User::id, unique)]
       owner_id: i64, // Only "tags_owner_id_key" is generated. No "tags_owner_id_idx"!
   }
   ```

---

## 4. Programmatic Index Construction

If you are building database schema migrations programmatically, you can assemble index definitions using the core builder types:

```rust
use tork_orm::migration::{IndexColumn, SchemaManager};
use tork_orm::dialect::SqliteDialect;

async fn create_custom_index(schema: &mut SchemaManager<'_>) -> Result<()> {
    schema
        .create_index("uq_posts_user_created")
        .on_table("posts")
        .unique()
        .columns([
            IndexColumn::new("user_id"),
            IndexColumn::new("created_at").desc(),
        ])
        .if_not_exists()
        .execute()
        .await?;
    Ok(())
}
```

This compiles directly into:
```sql
CREATE UNIQUE INDEX IF NOT EXISTS "uq_posts_user_created" ON "posts" ("user_id", "created_at" DESC)
```
