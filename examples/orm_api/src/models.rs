//! Database models, relations, and the API DTOs they map to.

use tork::api_model;
use tork_orm::prelude::*;

/// A user row.
#[derive(Debug, Clone, Model)]
#[table(name = "users")]
pub struct User {
    #[field(primary_key, auto)]
    pub id: i64,
    #[field(varchar(length = 50))]
    pub username: String,
    #[field(varchar(length = 255))]
    pub email: String,
    pub is_active: bool,
}

/// A post row, belonging to a user.
#[derive(Debug, Clone, Model)]
#[table(name = "posts")]
pub struct Post {
    #[field(primary_key, auto)]
    pub id: i64,
    #[field(foreign_key = User::id)]
    pub user_id: i64,
    pub title: String,
    pub view_count: i64,
}

#[relations]
impl User {
    #[has_many(Post, foreign_key = Post::user_id)]
    pub fn posts() {}
}

#[relations]
impl Post {
    #[belongs_to(User, foreign_key = Post::user_id)]
    pub fn author() {}
}

/// The public representation of a user.
#[api_model]
pub struct UserOut {
    pub id: i64,
    pub username: String,
    pub email: String,
    pub is_active: bool,
}

impl From<User> for UserOut {
    fn from(user: User) -> Self {
        Self {
            id: user.id,
            username: user.username,
            email: user.email,
            is_active: user.is_active,
        }
    }
}

/// The public representation of a post.
#[api_model]
pub struct PostOut {
    pub id: i64,
    pub title: String,
    pub view_count: i64,
}

impl From<&Post> for PostOut {
    fn from(post: &Post) -> Self {
        Self {
            id: post.id,
            title: post.title.clone(),
            view_count: post.view_count,
        }
    }
}

/// A user together with their posts, built from a preloaded query.
#[api_model]
pub struct UserWithPostsOut {
    pub id: i64,
    pub username: String,
    pub posts: Vec<PostOut>,
}

impl From<Preloaded<User>> for UserWithPostsOut {
    fn from(user: Preloaded<User>) -> Self {
        let posts = user.get::<Post>().iter().map(PostOut::from).collect();
        Self {
            id: user.id,
            username: user.username.clone(),
            posts,
        }
    }
}

/// The body accepted when creating a user.
#[api_model]
pub struct CreateUserInput {
    #[field(min_length = 1, max_length = 50)]
    pub username: String,
    #[field(min_length = 3, max_length = 255)]
    pub email: String,
}

/// Aggregated per-user statistics, mapped directly from a projection query and
/// serialized as the response.
#[api_model]
#[derive(QueryResult)]
pub struct UserStatsOut {
    pub user_id: i64,
    pub username: String,
    pub post_count: i64,
    pub total_views: i64,
}
