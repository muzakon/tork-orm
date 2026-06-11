-- revision: c3d4e5f6a7b8
-- down_revision: b2c3d4e5f6a7
-- migrate:up
-- These indexes mirror the metadata declared on the User and Post models.
CREATE UNIQUE INDEX "users_username_key" ON "users" ("username");
CREATE UNIQUE INDEX "users_email_key" ON "users" ("email");

-- The compound index supersedes the single-column foreign-key index, matching the
-- model's #[table(indexes = [index(fields = [user_id, view_count(desc)])])].
DROP INDEX "idx_posts_user_id";
CREATE INDEX "posts_user_id_view_count_idx" ON "posts" ("user_id", "view_count" DESC);

-- migrate:down
DROP INDEX "posts_user_id_view_count_idx";
CREATE INDEX "idx_posts_user_id" ON "posts" ("user_id");
DROP INDEX "users_email_key";
DROP INDEX "users_username_key";
