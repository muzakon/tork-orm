//! Tests preloading two relations that target the *same* child model type.
//!
//! An article has both an `author` and an `editor`, each a `User`. Preloading
//! both must keep separate slots: `get_via` returns each relation's own rows,
//! instead of the second preload silently overwriting the first.

use tork_orm::prelude::*;

#[derive(Debug, Clone, Model, PartialEq)]
#[table(name = "users")]
struct User {
    #[field(primary_key, auto)]
    id: i64,
    username: String,
}

#[derive(Debug, Clone, Model)]
#[table(name = "articles")]
struct Article {
    #[field(primary_key, auto)]
    id: i64,
    #[field(foreign_key = User::id)]
    author_id: i64,
    #[field(foreign_key = User::id)]
    editor_id: i64,
    title: String,
}

#[relations]
impl Article {
    #[belongs_to(User, foreign_key = Article::author_id)]
    pub fn author() {}

    #[belongs_to(User, foreign_key = Article::editor_id)]
    pub fn editor() {}
}

async fn seed() -> Database {
    let db = Database::connect(":memory:", 1).await.unwrap();
    db.execute(
        "CREATE TABLE users (id INTEGER PRIMARY KEY, username TEXT NOT NULL)".into(),
        vec![],
    )
    .await
    .unwrap();
    db.execute(
        "CREATE TABLE articles (id INTEGER PRIMARY KEY, author_id INTEGER NOT NULL, editor_id INTEGER NOT NULL, title TEXT NOT NULL)".into(),
        vec![],
    )
    .await
    .unwrap();

    for username in ["alice", "bob"] {
        db.execute(
            "INSERT INTO users (username) VALUES (?)".into(),
            vec![Value::Text(username.into())],
        )
        .await
        .unwrap();
    }
    // alice (id 1) authored it; bob (id 2) edited it.
    db.execute(
        "INSERT INTO articles (author_id, editor_id, title) VALUES (1, 2, 'launch')".into(),
        vec![],
    )
    .await
    .unwrap();
    db
}

#[tokio::test]
async fn two_relations_to_the_same_type_keep_separate_slots() {
    let db = seed().await;

    let articles = Article::query()
        .preload(Article::author())
        .preload(Article::editor())
        .all(&db)
        .await
        .unwrap();

    assert_eq!(articles.len(), 1);
    let article = &articles[0];

    // get_via selects exactly which relation's rows to read.
    let author = article.get_via(&Article::author());
    let editor = article.get_via(&Article::editor());

    assert_eq!(author.len(), 1);
    assert_eq!(editor.len(), 1);
    assert_eq!(author[0].username, "alice");
    assert_eq!(editor[0].username, "bob");
    // The second preload did not overwrite the first.
    assert_ne!(author[0].username, editor[0].username);
}
