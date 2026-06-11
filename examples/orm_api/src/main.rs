//! Binary entrypoint for the ORM example application.

use tork::{App, OpenApi};

use orm_api::db::Db;
use orm_api::routers;

#[tork::main]
async fn main() -> tork::Result<()> {
    App::new()
        // Builds the database (connect, migrate, seed) and registers it as a
        // resource injected into handlers as `Arc<Database>`.
        .lifespan::<Db>()
        .include_router(routers::router())
        .openapi(
            OpenApi::new()
                .title("ORM API")
                .version("1.0.0")
                .json("/openapi.json")
                .docs("/docs"),
        )
        .serve("0.0.0.0:8000")
        .await
}
