//! Tests that `#[derive(Model)]` registers a model for `migrate generate`.

use tork_orm::prelude::*;

#[derive(Debug, Clone, Model)]
#[table(name = "reg_users", indexes = [
    unique(fields = [ email ]),
])]
struct RegUser {
    #[field(primary_key, auto)]
    id: i64,
    email: String,
}

#[test]
fn model_appears_in_registry_with_schema() {
    let models = tork_orm::registered_models();
    let user = models
        .iter()
        .find(|schema| schema.table == "reg_users")
        .expect("the model should be registered");

    assert_eq!(user.columns.len(), 2);
    assert!(user
        .indexes
        .iter()
        .any(|index| index.name == "reg_users_email_key" && index.unique));
}
