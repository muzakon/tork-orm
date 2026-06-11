//! End-to-end tests for the example app: boot it (running the Db lifespan that
//! migrates and seeds an in-memory database) and drive the endpoints in-process.

use orm_api::db::Db;
use orm_api::routers;
use serde_json::json;
use tork::testing::TestClient;
use tork::App;

async fn client() -> TestClient {
    let app = App::new()
        .lifespan::<Db>()
        .include_router(routers::router())
        .build_test()
        .await
        .unwrap();
    TestClient::new(app).await.unwrap()
}

#[tokio::test]
async fn lists_seeded_users() {
    let client = client().await;
    let response = client.get("/users").send().await.unwrap();
    assert_eq!(response.status(), 200);
    let body = response.json::<serde_json::Value>().await.unwrap();
    assert_eq!(body.as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn user_with_posts_is_preloaded() {
    let client = client().await;
    let response = client.get("/users/1/posts").send().await.unwrap();
    assert_eq!(response.status(), 200);
    let body = response.json::<serde_json::Value>().await.unwrap();
    assert_eq!(body["username"], "alice");
    // alice was seeded with two posts.
    assert_eq!(body["posts"].as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn stats_aggregate_orders_by_views() {
    let client = client().await;
    let response = client.get("/users/stats").send().await.unwrap();
    assert_eq!(response.status(), 200);
    let body = response.json::<serde_json::Value>().await.unwrap();
    let rows = body.as_array().unwrap();
    // alice (2 posts, 200 views) sorts before bob (1 post, 30 views).
    assert_eq!(rows[0]["username"], "alice");
    assert_eq!(rows[0]["post_count"], 2);
    assert_eq!(rows[0]["total_views"], 200);
}

#[tokio::test]
async fn creates_a_user() {
    let client = client().await;
    let response = client
        .post("/users")
        .json(&json!({ "username": "carol", "email": "carol@example.com" }))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 201);
    let body = response.json::<serde_json::Value>().await.unwrap();
    assert_eq!(body["username"], "carol");
    assert!(body["id"].as_i64().unwrap() > 0);
}

#[tokio::test]
async fn missing_user_is_404() {
    let client = client().await;
    let response = client.get("/users/999").send().await.unwrap();
    assert_eq!(response.status(), 404);
}
