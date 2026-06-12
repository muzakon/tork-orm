# 5. Writes: Insert, Update, and Delete

Tork ORM supports inserting, updating, and deleting database records. These operations can be performed either on individual model instances or as bulk operations using a `QuerySet`.

---

## 1. Creating Records (Insert)

### A. Insert a Single Instance (`Model::create`)
To insert a new record, construct a model instance and call `<Model>::create`. 

- **Primary Keys:** If the model has an auto-incrementing primary key (e.g. `#[field(primary_key, auto)]`), you can pass any value (like `0`) in the input struct. The database-generated ID will be fetched (via `RETURNING`) and the returned model will contain the actual ID.

```rust
let new_user = User {
    id: 0, // Ignored because of #[field(primary_key, auto)]
    username: "alice".into(),
    email: "alice@example.com".into(),
    is_active: true,
};

// Inserts and returns the stored row containing the database-generated ID.
let stored_user = User::create(&db, &new_user).await?;
assert_eq!(stored_user.id, 1);
```

### B. Bulk Inserting Instances (`Model::bulk_create`)
To insert multiple records efficiently in a single query, use `<Model>::bulk_create`. It returns the number of inserted rows as a `usize`.

```rust
let batch = [
    User { id: 0, username: "bob".into(), email: "bob@x.com".into(), is_active: true },
    User { id: 0, username: "carol".into(), email: "carol@x.com".into(), is_active: true },
];

let inserted_count = User::bulk_create(&db, &batch).await?;
assert_eq!(inserted_count, 2);
```
*Note: If the input slice is empty, `bulk_create` returns `0` immediately without querying the database.*

---

## 2. Updating Records

### A. Updating a Single Instance (`save`)
If you have retrieved a model instance from the database and modified its fields, you can write the changes back using the `save` method. This updates all columns using the instance's primary key value in the `WHERE` clause.

It returns the number of affected rows (typically `1`).

```rust
// Fetch a user
let mut user = User::query()
    .filter(User::username.eq("alice"))
    .one(&db)
    .await?;

// Modify fields
user.email = "alice.new@example.com".into();
user.is_active = false;

// Save changes back to the database
let affected_rows = user.save(&db).await?;
assert_eq!(affected_rows, 1);
```

### B. Bulk Updates (`QuerySet::update`)
To update specific columns on multiple rows matching a query filter without fetching them first, use the `update` method on a `QuerySet`.

It takes a list of column assignments (built with `Column::set`) and returns the number of updated rows.

`Column::set` accepts either a typed literal or any `Expr`, so you can express atomic increment patterns without a read-modify-write cycle:

```rust
// Literal assignment — deactivate every user whose username is not "bob".
let deactivated_count = User::query()
    .filter(User::username.ne("bob"))
    .update(&db, [User::is_active.set(false)])
    .await?;

// Expression assignment — atomically increment a counter.
Post::query()
    .filter(Post::id.eq(post_id))
    .update(&db, [Post::view_count.set(Post::view_count.add(1_i64))])
    .await?;
```

---

## 3. Deleting Records

### A. Delete an Instance (`Model::delete`)
If you have a model instance in memory, call `delete` directly on it. The row matching the instance's primary key is removed.

```rust
let user = User::query()
    .filter(User::username.eq("bob"))
    .one(&db)
    .await?;

let affected = user.delete(&db).await?;
assert_eq!(affected, 1);
```

### B. Bulk Delete (`QuerySet::delete`)
Delete every row matching a query filter — or the whole table if no filter is applied.

```rust
// Delete a specific user by filter.
let deleted = User::query()
    .filter(User::username.eq("bob"))
    .delete(&db)
    .await?;
assert_eq!(deleted, 1);

// Delete all rows in the table.
let total_deleted = User::query().delete(&db).await?;
println!("Deleted {} remaining users", total_deleted);
```

---

## 4. Upsert (Insert or Replace)

`Model::upsert` inserts a row, replacing any existing row that conflicts on a unique key. It uses `INSERT OR REPLACE INTO` (SQLite) which deletes the conflicting row first and then inserts the new one.

It returns the stored row, including any database-assigned columns (just like `create`).

```rust
// Insert "apple" the first time.
let first = Product::upsert(&db, &Product { id: 0, name: "apple".into(), price: 1.50 }).await?;

// Upsert the same name — the conflicting row is replaced.
let updated = Product::upsert(&db, &Product { id: 0, name: "apple".into(), price: 1.99 }).await?;
assert_eq!(updated.name, "apple");
assert_eq!(updated.price, 1.99);
```

Because `INSERT OR REPLACE` deletes the conflicting row before inserting, auto-increment primary keys are re-assigned on replacement. If you need to preserve the original primary key, use `save()` after fetching the row.

---

## 5. RETURNING: Get Rows Back from Update and Delete

### A. `update_returning`

`update_returning` works like `update` but appends a `RETURNING` clause so the updated rows are returned as `Vec<M>` instead of a row count. All columns of `M` are returned.

```rust
// Deactivate users whose username is not "bob" and get them back.
let deactivated: Vec<User> = User::query()
    .filter(User::username.ne("bob"))
    .update_returning(&db, [User::is_active.set(false)])
    .await?;

for user in &deactivated {
    println!("{} was deactivated", user.username);
}
```

### B. `delete_returning`

`delete_returning` works like `delete` but returns the removed rows as `Vec<M>`. Useful for audit logging or soft-delete pipelines.

```rust
// Delete all inactive users and get their data before removal.
let removed: Vec<User> = User::query()
    .filter(User::is_active.eq(false))
    .delete_returning(&db)
    .await?;

println!("Removed {} inactive accounts", removed.len());
```

Both methods require SQLite 3.35 or later (available in all recent releases of the bundled `rusqlite`).

---

## 6. Convenience Methods: Get or Create / Update or Create

These methods combine a lookup with a write in a single call, matching the common "find-or-create" pattern.

### A. `get_or_create`

`Model::get_or_create(executor, filter, value)` tries to find a row matching `filter`. If found, it returns `(row, false)`. If not, it inserts `value` and returns `(stored_row, true)`.

```rust
// Try to find alice; create her if she does not exist.
let (user, created) = User::get_or_create(
    &db,
    |q| q.filter(User::username.eq("alice")),
    &User { id: 0, username: "alice".into(), email: "alice@x.com".into(), is_active: true },
).await?;

if created {
    println!("Created new user: {}", user.username);
} else {
    println!("Found existing user: {}", user.username);
}
```

The lookup uses `one_or_none`, so it errors with `MultipleFound` if the filter matches more than one row.

### B. `update_or_create`

`Model::update_or_create(executor, filter, value)` finds a row by `filter`. If found, the row is updated with `value`'s fields. If not, `value` is inserted. Returns `(row, false)` for an update or `(row, true)` for a create.

```rust
// Update alice's email if she exists, or create her if she does not.
let (user, created) = User::update_or_create(
    &db,
    |q| q.filter(User::username.eq("alice")),
    &User { id: 0, username: "alice".into(), email: "alice-new@x.com".into(), is_active: false },
).await?;
```

### C. `first_or_create`

`Model::first_or_create(executor, filter, value)` finds the first row by `filter`. If found, returns it. If not, inserts `value` and returns the stored row. Unlike `get_or_create`, multiple matches silently return the first one.

```rust
// Return the first active user or create a new one.
let user = User::first_or_create(
    &db,
    |q| q.filter(User::is_active.eq(true)),
    &User { id: 0, username: "default".into(), email: "default@x.com".into(), is_active: true },
).await?;
```

## 7. Automatic Timestamps

A `#[field(created_at)]` column is filled by the database default when a row is inserted, and a `#[field(updated_at)]` column is additionally refreshed to the current database time on every `save()`. (See [Defining Models](02-models.md) for the field declarations; both expect a `DEFAULT CURRENT_TIMESTAMP` in the migration.)

```rust
let mut doc = Document::find(&db, 1).await?;
doc.body = "edited".into();
doc.save(&db).await?;   // updated_at becomes the database NOW(); created_at is untouched
```

`create()` returns the stored row with both timestamps populated by the database.

## 8. Optimistic Locking

A `#[field(version)]` column guards against lost updates from concurrent writers. Each `save()` only updates the row when its version still matches the one you loaded, increments the version in the same statement, and bumps the in-memory value:

```rust
let mut a = Document::find(&db, 1).await?;   // version 4
let mut b = Document::find(&db, 1).await?;   // version 4

a.body = "from a".into();
a.save(&db).await?;                          // ok; the row and `a` are now version 5

b.body = "from b".into();
let result = b.save(&db).await;              // `b` is stale (still version 4)
assert_eq!(result.unwrap_err().kind(), ErrorKind::Conflict);
```

A conflict returns an `ErrorKind::Conflict` error (mapped to HTTP 409 when bridged into the Tork framework). Reload the row and retry.

## 9. Soft Delete

When a model has a `#[field(deleted_at)]` column, `delete()` becomes a *soft* delete: it stamps `deleted_at` with the current time instead of removing the row. Default queries then hide it (see [Querying](04-queries.md), Soft-Delete Query Scope).

```rust
let note = Note::find(&db, 1).await?;
note.delete(&db).await?;        // soft: sets deleted_at, the row stays in the table

note.restore(&db).await?;       // clears deleted_at
note.force_delete(&db).await?;  // permanent removal, bypassing soft-delete
```

The same operations exist on a `QuerySet` for bulk use:

```rust
Note::query().filter(Note::stale.eq(true)).delete(&db).await?;   // soft-delete many
Note::query().only_deleted().restore(&db).await?;               // restore all trashed
Note::query().with_deleted().hard_delete(&db).await?;           // purge everything
```
