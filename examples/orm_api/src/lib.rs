//! Example application showing the Tork ORM used natively with the Tork web
//! framework: models and relations, a database resource built in a lifespan, and
//! handlers that query through an injected `Arc<Database>`.

pub mod db;
pub mod models;
pub mod routers;
