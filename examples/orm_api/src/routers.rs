//! HTTP routes that query the database through an injected `Arc<Database>`.

use std::sync::Arc;

use tork::{api_router, get, post, Valid};
use tork_orm::prelude::*;

use crate::models::{
    CreateUserInput, Post, User, UserOut, UserStatsOut, UserWithPostsOut,
};

#[api_router(prefix = "/users", tags = ["users"])]
pub mod users_router {
    use super::*;

    /// Lists every user.
    #[get("", response_model = Vec<UserOut>, summary = "List users")]
    pub async fn list_users(db: Arc<Database>) -> tork::Result<Vec<UserOut>> {
        let users = User::query().order_by(User::id.asc()).all(&db).await?;
        Ok(users.into_iter().map(UserOut::from).collect())
    }

    /// Aggregated per-user post statistics.
    #[get("/stats", response_model = Vec<UserStatsOut>, summary = "Per-user post stats")]
    pub async fn stats(db: Arc<Database>) -> tork::Result<Vec<UserStatsOut>> {
        let stats = User::query()
            .select((
                User::id.as_("user_id"),
                User::username,
                Post::id.count().as_("post_count"),
                Post::view_count.sum().as_("total_views"),
            ))
            .join(User::posts())
            .group_by((User::id, User::username))
            .order_by(Post::view_count.sum().desc())
            .all_as::<UserStatsOut>(&db)
            .await?;
        Ok(stats)
    }

    /// Returns a single user by id.
    #[get("/{id}", response_model = UserOut, summary = "Get user by id")]
    pub async fn get_user(id: i64, db: Arc<Database>) -> tork::Result<UserOut> {
        let user = User::query().filter(User::id.eq(id)).one(&db).await?;
        Ok(UserOut::from(user))
    }

    /// Returns a user together with their posts, loaded in one extra query.
    #[get("/{id}/posts", response_model = UserWithPostsOut, summary = "Get user with posts")]
    pub async fn user_posts(id: i64, db: Arc<Database>) -> tork::Result<UserWithPostsOut> {
        let loaded = User::query()
            .filter(User::id.eq(id))
            .preload(User::posts().order_by(Post::view_count.desc()))
            .all(&db)
            .await?;
        let user = loaded
            .into_iter()
            .next()
            .ok_or_else(|| tork::Error::not_found("user not found"))?;
        Ok(UserWithPostsOut::from(user))
    }

    /// Creates a user.
    #[post("", response_model = UserOut, summary = "Create user", status_code = 201)]
    pub async fn create_user(
        db: Arc<Database>,
        body: Valid<CreateUserInput>,
    ) -> tork::Result<UserOut> {
        let input = body.into_inner();
        let user = User::create(
            &db,
            &User {
                id: 0,
                username: input.username,
                email: input.email,
                is_active: true,
            },
        )
        .await?;
        Ok(UserOut::from(user))
    }
}

/// Builds the users router.
pub fn router() -> tork::Router {
    users_router::router()
}
