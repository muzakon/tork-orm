-- revision: a1b2c3d4e5f6
-- down_revision:
-- migrate:up
CREATE TABLE "users" (
    "id" INTEGER PRIMARY KEY AUTOINCREMENT,
    "username" VARCHAR(50) NOT NULL,
    "email" VARCHAR(255) NOT NULL,
    "is_active" BOOLEAN NOT NULL DEFAULT 1
);

-- migrate:down
DROP TABLE "users";
