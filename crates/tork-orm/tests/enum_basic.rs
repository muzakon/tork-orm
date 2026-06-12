//! Tests for `#[derive(DbEnum)]`: stored values, casing conventions, per-variant
//! overrides, and the generated `BindValue`/`FromValue` conversions. No database
//! is involved here.

use tork_orm::prelude::*;
use tork_orm::{SqlType, Value};

#[derive(Debug, Clone, Copy, PartialEq, DbEnum)]
enum Status {
    Active,
    Inactive,
    #[db_enum(rename = "on_hold")]
    OnHold,
}

#[derive(Debug, Clone, Copy, PartialEq, DbEnum)]
#[db_enum(name = "user_role", rename_all = "SCREAMING_SNAKE_CASE")]
enum Role {
    Admin,
    StaffMember,
}

#[derive(Debug, Clone, Copy, PartialEq, DbEnum)]
#[db_enum(rename_all = "kebab-case")]
enum Shipping {
    NextDay,
    InStorePickup,
}

#[test]
fn default_casing_is_snake_case_with_per_variant_override() {
    assert_eq!(Status::Active.as_db_str(), "active");
    assert_eq!(Status::Inactive.as_db_str(), "inactive");
    // The explicit rename wins over the snake_case default.
    assert_eq!(Status::OnHold.as_db_str(), "on_hold");
    assert_eq!(Status::ENUM_NAME, "status");
    assert_eq!(Status::VARIANTS, &["active", "inactive", "on_hold"]);
}

#[test]
fn name_and_rename_all_overrides_apply() {
    assert_eq!(Role::ENUM_NAME, "user_role");
    assert_eq!(Role::Admin.as_db_str(), "ADMIN");
    assert_eq!(Role::StaffMember.as_db_str(), "STAFF_MEMBER");

    assert_eq!(Shipping::NextDay.as_db_str(), "next-day");
    assert_eq!(Shipping::InStorePickup.as_db_str(), "in-store-pickup");
}

#[test]
fn sql_type_carries_name_and_variants() {
    assert_eq!(
        <Status as DbEnum>::SQL_TYPE,
        SqlType::Enum {
            name: "status",
            variants: &["active", "inactive", "on_hold"],
        }
    );
}

#[test]
fn from_db_str_round_trips_and_rejects_unknown() {
    assert_eq!(Status::from_db_str("active").unwrap(), Status::Active);
    assert_eq!(Status::from_db_str("on_hold").unwrap(), Status::OnHold);
    assert!(Status::from_db_str("ON_HOLD").is_err());
    assert!(Status::from_db_str("bogus").is_err());
}

#[test]
fn bind_value_lowers_to_text() {
    assert_eq!(Status::Active.to_value(), Value::Text("active".into()));
    assert_eq!(Status::OnHold.to_value(), Value::Text("on_hold".into()));
}

#[test]
fn from_value_reads_text_and_rejects_other_shapes() {
    assert_eq!(
        Status::from_value(Value::Text("inactive".into())).unwrap(),
        Status::Inactive
    );
    // A non-text value cannot be an enum.
    assert!(Status::from_value(Value::Int(1)).is_err());
    // Text that is not a known variant is rejected.
    assert!(Status::from_value(Value::Text("nope".into())).is_err());
}
