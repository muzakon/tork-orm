-- revision: b2c3d4e5f6a7
-- down_revision: a1b2c3d4e5f6
-- migrate:up
CREATE TABLE "posts" (
    "id" INTEGER PRIMARY KEY AUTOINCREMENT,
    "user_id" INTEGER NOT NULL REFERENCES "users"("id"),
    "title" TEXT NOT NULL,
    "view_count" INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX "idx_posts_user_id" ON "posts" ("user_id");

-- migrate:down
DROP TABLE "posts";
