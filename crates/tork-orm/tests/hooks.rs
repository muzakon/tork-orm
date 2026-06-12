//! Tests for lifecycle hooks and database-side field defaults, against in-memory SQLite.

use std::sync::atomic::{AtomicU64, Ordering};

use tork_orm::prelude::*;

/// Counts `after_create` invocations across the test models.
static AFTER_CREATE_CALLS: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Model)]
#[table(name = "widgets", hooks)]
struct Widget {
    #[field(primary_key, auto)]
    id: i64,
    name: String,
    /// Filled by `before_create` from `name`.
    slug: String,
    /// Bumped by `before_save`.
    revision: i64,
}

impl ModelHooks for Widget {
    fn before_create(&mut self) {
        self.slug = self.name.to_lowercase();
    }

    fn before_save(&mut self) {
        self.revision += 1;
    }

    async fn after_create<E: Executor + Send + Sync>(&self, _db: &E) -> Result<()> {
        AFTER_CREATE_CALLS.fetch_add(1, Ordering::SeqCst);
        // The magic name aborts the operation, to exercise rollback.
        if self.name == "boom" {
            return Err(OrmError::query("after_create rejected the row"));
        }
        Ok(())
    }
}

async fn widget_db() -> Database {
    let db = Database::connect(":memory:", 1).await.unwrap();
    db.execute(
        "CREATE TABLE widgets (id INTEGER PRIMARY KEY, name TEXT NOT NULL, slug TEXT NOT NULL, revision INTEGER NOT NULL)".into(),
        vec![],
    )
    .await
    .unwrap();
    db
}

fn new_widget(name: &str) -> Widget {
    Widget { id: 0, name: name.into(), slug: String::new(), revision: 0 }
}

#[tokio::test]
async fn before_create_mutation_is_persisted_and_after_create_runs() {
    let db = widget_db().await;
    let before = AFTER_CREATE_CALLS.load(Ordering::SeqCst);

    let stored = Widget::create(&db, &new_widget("Hello")).await.unwrap();
    // before_create lower-cased the name into `slug`, and it was persisted.
    assert_eq!(stored.slug, "hello");
    let reloaded = Widget::query().filter(Widget::id.eq(stored.id)).one(&db).await.unwrap();
    assert_eq!(reloaded.slug, "hello");
    // after_create fired (the counter is global and shared with parallel tests, so
    // assert it strictly increased rather than by an exact amount).
    assert!(AFTER_CREATE_CALLS.load(Ordering::SeqCst) > before);
}

#[tokio::test]
async fn before_save_mutates_self_and_persists() {
    let db = widget_db().await;
    let mut widget = Widget::create(&db, &new_widget("Gadget")).await.unwrap();
    assert_eq!(widget.revision, 0);

    widget.save(&db).await.unwrap();
    // The caller's instance sees the bump (save takes &mut self).
    assert_eq!(widget.revision, 1);
    // And it was written.
    let reloaded = Widget::query().filter(Widget::id.eq(widget.id)).one(&db).await.unwrap();
    assert_eq!(reloaded.revision, 1);
}

#[tokio::test]
async fn after_create_error_rolls_back_inside_a_transaction() {
    let db = widget_db().await;

    let result: tork_orm::Result<()> = db
        .transaction(|tx| {
            Box::pin(async move {
                Widget::create(tx, &new_widget("boom")).await?;
                Ok(())
            })
        })
        .await;
    assert!(result.is_err());

    // The after_create failure rolled the insert back.
    assert_eq!(Widget::query().count(&db).await.unwrap(), 0);
}

// ── DB-side defaults ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Model)]
#[table(name = "counters")]
struct Counter {
    #[field(primary_key, auto)]
    id: i64,
    label: String,
    /// A database-side default; omitted from INSERT, filled by the database.
    #[field(default = "5")]
    hits: i64,
}

#[tokio::test]
async fn raw_default_is_omitted_from_insert_and_filled_by_db() {
    let db = Database::connect(":memory:", 1).await.unwrap();
    db.execute(
        "CREATE TABLE counters (id INTEGER PRIMARY KEY, label TEXT NOT NULL, hits INTEGER NOT NULL DEFAULT 5)".into(),
        vec![],
    )
    .await
    .unwrap();

    // `hits` is constructed as 999 but omitted from the INSERT, so the DB default wins.
    let stored = Counter::create(&db, &Counter { id: 0, label: "a".into(), hits: 999 })
        .await
        .unwrap();
    assert_eq!(stored.hits, 5);
}

#[test]
fn default_metadata_is_recorded_on_columns() {
    // `default_now` records `CurrentTimestamp` on the column so `migrate generate`
    // emits `DEFAULT CURRENT_TIMESTAMP`.
    let hits = Counter::COLUMNS.iter().find(|c| c.name == "hits").unwrap();
    assert_eq!(hits.default, Some(ColumnDefault::Raw("5")));
}

#[derive(Debug, Clone, Model)]
#[table(name = "events")]
struct Event {
    #[field(primary_key, auto)]
    id: i64,
    kind: String,
    #[field(default_now)]
    created_at: time::OffsetDateTime,
}

#[test]
fn default_now_records_current_timestamp() {
    let created = Event::COLUMNS.iter().find(|c| c.name == "created_at").unwrap();
    assert_eq!(created.default, Some(ColumnDefault::CurrentTimestamp));
    // The defaulted column is excluded from the model's insert values.
    let event = Event { id: 0, kind: "click".into(), created_at: time::OffsetDateTime::UNIX_EPOCH };
    let inserted: Vec<&str> = event.insert_values().iter().map(|(name, _)| *name).collect();
    assert!(!inserted.contains(&"created_at"));
    assert!(inserted.contains(&"kind"));
}
