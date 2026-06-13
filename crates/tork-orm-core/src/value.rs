//! The dialect-agnostic value type that crosses the database driver boundary.
//!
//! Every literal that appears in a query is carried as a [`Value`] and bound as a
//! parameter, never interpolated into SQL text. Each driver converts between
//! [`Value`] and its own native representation, so the rest of the ORM stays free
//! of backend-specific types.

use time::OffsetDateTime;

/// A single database value, independent of any backend.
///
/// This is the common currency between the query layer and a driver: query
/// parameters are lowered into `Value`s, and result columns are read back as
/// `Value`s before being converted into Rust types via [`FromValue`].
///
/// # Examples
///
/// ```
/// use tork_orm_core::Value;
///
/// let name = Value::Text("alice".to_string());
/// let active = Value::Bool(true);
/// assert!(matches!(name, Value::Text(_)));
/// assert!(matches!(active, Value::Bool(true)));
/// ```
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    /// SQL `NULL`.
    Null,
    /// A boolean, stored as an integer `0` or `1` by backends without a native type.
    Bool(bool),
    /// A signed 64-bit integer.
    Int(i64),
    /// A 64-bit floating point number.
    Real(f64),
    /// UTF-8 text.
    Text(String),
    /// Raw bytes.
    Blob(Vec<u8>),
    /// A timestamp, rendered to and parsed from RFC 3339 text by default.
    Timestamp(OffsetDateTime),
    /// A JSON document (PostgreSQL `json`/`jsonb`).
    Json(serde_json::Value),
    /// A UUID (PostgreSQL `uuid`).
    Uuid(uuid::Uuid),
    /// An array of values (PostgreSQL `type[]`), each element a [`Value`].
    Array(Vec<Value>),
}

impl Value {
    /// Returns `true` if this value is [`Value::Null`].
    ///
    /// # Examples
    ///
    /// ```
    /// use tork_orm_core::Value;
    ///
    /// assert!(Value::Null.is_null());
    /// assert!(!Value::Int(1).is_null());
    /// ```
    pub fn is_null(&self) -> bool {
        matches!(self, Value::Null)
    }
}

/// Converts a Rust value into a bound [`Value`].
///
/// Implemented for the primitive column types the ORM understands. A blanket
/// implementation maps `Option<T>` so that `None` becomes [`Value::Null`].
pub trait BindValue {
    /// Lowers this value into a bound parameter.
    fn to_value(&self) -> Value;
}

impl BindValue for Value {
    fn to_value(&self) -> Value {
        self.clone()
    }
}

impl BindValue for bool {
    fn to_value(&self) -> Value {
        Value::Bool(*self)
    }
}

impl BindValue for i64 {
    fn to_value(&self) -> Value {
        Value::Int(*self)
    }
}

impl BindValue for i32 {
    fn to_value(&self) -> Value {
        Value::Int(i64::from(*self))
    }
}

impl BindValue for u8 {
    fn to_value(&self) -> Value {
        Value::Int(i64::from(*self))
    }
}

impl BindValue for u16 {
    fn to_value(&self) -> Value {
        Value::Int(i64::from(*self))
    }
}

impl BindValue for u32 {
    fn to_value(&self) -> Value {
        Value::Int(i64::from(*self))
    }
}

impl BindValue for u64 {
    fn to_value(&self) -> Value {
        Value::Int(*self as i64)
    }
}

impl BindValue for usize {
    fn to_value(&self) -> Value {
        Value::Int(*self as i64)
    }
}

impl BindValue for i8 {
    fn to_value(&self) -> Value {
        Value::Int(i64::from(*self))
    }
}

impl BindValue for i16 {
    fn to_value(&self) -> Value {
        Value::Int(i64::from(*self))
    }
}

impl BindValue for f32 {
    fn to_value(&self) -> Value {
        Value::Real(f64::from(*self))
    }
}

impl BindValue for f64 {
    fn to_value(&self) -> Value {
        Value::Real(*self)
    }
}

impl BindValue for String {
    fn to_value(&self) -> Value {
        Value::Text(self.clone())
    }
}

impl BindValue for &str {
    fn to_value(&self) -> Value {
        Value::Text((*self).to_string())
    }
}

impl BindValue for Vec<u8> {
    fn to_value(&self) -> Value {
        Value::Blob(self.clone())
    }
}

impl BindValue for OffsetDateTime {
    fn to_value(&self) -> Value {
        Value::Timestamp(*self)
    }
}

impl BindValue for serde_json::Value {
    fn to_value(&self) -> Value {
        Value::Json(self.clone())
    }
}

impl BindValue for uuid::Uuid {
    fn to_value(&self) -> Value {
        Value::Uuid(*self)
    }
}

/// Generates `BindValue`/`FromValue` for `Vec<T>` as a SQL array, for each listed
/// element type.
///
/// These are concrete (not a blanket `impl<T> ... for Vec<T>`) so they never overlap
/// with the `Vec<u8>` blob impl: `Vec<u8>` stays a blob, `Vec<i64>`/`Vec<String>`/…
/// become arrays.
macro_rules! impl_array_value {
    ($($element:ty),+ $(,)?) => {$(
        impl BindValue for Vec<$element> {
            fn to_value(&self) -> Value {
                Value::Array(self.iter().map(BindValue::to_value).collect())
            }
        }

        impl FromValue for Vec<$element> {
            fn from_value(value: Value) -> crate::Result<Self> {
                match value {
                    Value::Array(items) => items
                        .into_iter()
                        .map(<$element as FromValue>::from_value)
                        .collect(),
                    other => Err(mismatch(concat!("Vec<", stringify!($element), ">"), &other)),
                }
            }
        }
    )+};
}

impl_array_value!(i32, i64, f64, bool, String);

impl<T: BindValue> BindValue for Option<T> {
    fn to_value(&self) -> Value {
        match self {
            Some(inner) => inner.to_value(),
            None => Value::Null,
        }
    }
}

/// Converts a [`Value`] read from a result row into a Rust type.
///
/// Returns an [`Err`](crate::OrmError) when the stored value cannot be coerced to
/// the requested type (for example a `NULL` read into a non-optional field).
pub trait FromValue: Sized {
    /// Attempts to read this type from a database value.
    fn from_value(value: Value) -> crate::Result<Self>;
}

/// Builds a type-mismatch error for a failed [`FromValue`] conversion.
fn mismatch(expected: &str, value: &Value) -> crate::OrmError {
    crate::OrmError::conversion(format!(
        "cannot read {expected} from value `{value:?}`"
    ))
}

impl FromValue for Value {
    fn from_value(value: Value) -> crate::Result<Self> {
        Ok(value)
    }
}

impl FromValue for bool {
    fn from_value(value: Value) -> crate::Result<Self> {
        match value {
            Value::Bool(b) => Ok(b),
            Value::Int(i) => Ok(i != 0),
            other => Err(mismatch("bool", &other)),
        }
    }
}

impl FromValue for i64 {
    fn from_value(value: Value) -> crate::Result<Self> {
        match value {
            Value::Int(i) => Ok(i),
            Value::Bool(b) => Ok(i64::from(b)),
            other => Err(mismatch("i64", &other)),
        }
    }
}

impl FromValue for i32 {
    fn from_value(value: Value) -> crate::Result<Self> {
        let wide = i64::from_value(value)?;
        i32::try_from(wide).map_err(|_| crate::OrmError::conversion("integer out of range for i32"))
    }
}

impl FromValue for u8 {
    fn from_value(value: Value) -> crate::Result<Self> {
        let wide = i64::from_value(value)?;
        u8::try_from(wide).map_err(|_| crate::OrmError::conversion("integer out of range for u8"))
    }
}

impl FromValue for u16 {
    fn from_value(value: Value) -> crate::Result<Self> {
        let wide = i64::from_value(value)?;
        u16::try_from(wide).map_err(|_| crate::OrmError::conversion("integer out of range for u16"))
    }
}

impl FromValue for u32 {
    fn from_value(value: Value) -> crate::Result<Self> {
        let wide = i64::from_value(value)?;
        u32::try_from(wide).map_err(|_| crate::OrmError::conversion("integer out of range for u32"))
    }
}

impl FromValue for u64 {
    fn from_value(value: Value) -> crate::Result<Self> {
        let wide = i64::from_value(value)?;
        u64::try_from(wide).map_err(|_| crate::OrmError::conversion("integer out of range for u64"))
    }
}

impl FromValue for usize {
    fn from_value(value: Value) -> crate::Result<Self> {
        let wide = i64::from_value(value)?;
        usize::try_from(wide).map_err(|_| crate::OrmError::conversion("integer out of range for usize"))
    }
}

impl FromValue for i8 {
    fn from_value(value: Value) -> crate::Result<Self> {
        let wide = i64::from_value(value)?;
        i8::try_from(wide).map_err(|_| crate::OrmError::conversion("integer out of range for i8"))
    }
}

impl FromValue for i16 {
    fn from_value(value: Value) -> crate::Result<Self> {
        let wide = i64::from_value(value)?;
        i16::try_from(wide).map_err(|_| crate::OrmError::conversion("integer out of range for i16"))
    }
}

impl FromValue for f32 {
    fn from_value(value: Value) -> crate::Result<Self> {
        f64::from_value(value).map(|v| v as f32)
    }
}

impl FromValue for f64 {
    fn from_value(value: Value) -> crate::Result<Self> {
        match value {
            Value::Real(r) => Ok(r),
            Value::Int(i) => Ok(i as f64),
            other => Err(mismatch("f64", &other)),
        }
    }
}

impl FromValue for String {
    fn from_value(value: Value) -> crate::Result<Self> {
        match value {
            Value::Text(s) => Ok(s),
            other => Err(mismatch("String", &other)),
        }
    }
}

impl FromValue for Vec<u8> {
    fn from_value(value: Value) -> crate::Result<Self> {
        match value {
            Value::Blob(b) => Ok(b),
            Value::Text(s) => Ok(s.into_bytes()),
            other => Err(mismatch("Vec<u8>", &other)),
        }
    }
}

impl FromValue for OffsetDateTime {
    fn from_value(value: Value) -> crate::Result<Self> {
        match value {
            Value::Timestamp(ts) => Ok(ts),
            Value::Text(s) => parse_timestamp_text(&s),
            other => Err(mismatch("OffsetDateTime", &other)),
        }
    }
}

/// Parses a timestamp from text, accepting RFC 3339 first and then SQLite's
/// `CURRENT_TIMESTAMP` form (`YYYY-MM-DD HH:MM:SS`, UTC, no offset), which is what
/// a database-side default writes into a text-affinity column on SQLite.
fn parse_timestamp_text(text: &str) -> crate::Result<OffsetDateTime> {
    use time::format_description::well_known::Rfc3339;
    if let Ok(ts) = OffsetDateTime::parse(text, &Rfc3339) {
        return Ok(ts);
    }
    let sqlite_format = time::macros::format_description!(
        "[year]-[month]-[day] [hour]:[minute]:[second]"
    );
    if let Ok(naive) = time::PrimitiveDateTime::parse(text, &sqlite_format) {
        return Ok(naive.assume_utc());
    }
    Err(crate::OrmError::conversion(format!(
        "invalid timestamp text `{text}` (expected RFC 3339 or `YYYY-MM-DD HH:MM:SS`)"
    )))
}

impl FromValue for serde_json::Value {
    fn from_value(value: Value) -> crate::Result<Self> {
        match value {
            Value::Json(j) => Ok(j),
            // A backend without a native JSON type may return it as text.
            Value::Text(s) => serde_json::from_str(&s)
                .map_err(|_| crate::OrmError::conversion("invalid JSON text")),
            other => Err(mismatch("serde_json::Value", &other)),
        }
    }
}

impl FromValue for uuid::Uuid {
    fn from_value(value: Value) -> crate::Result<Self> {
        match value {
            Value::Uuid(u) => Ok(u),
            // A backend without a native UUID type may return it as text.
            Value::Text(s) => uuid::Uuid::parse_str(&s)
                .map_err(|_| crate::OrmError::conversion("invalid UUID text")),
            other => Err(mismatch("uuid::Uuid", &other)),
        }
    }
}

impl<T: FromValue> FromValue for Option<T> {
    fn from_value(value: Value) -> crate::Result<Self> {
        match value {
            Value::Null => Ok(None),
            other => T::from_value(other).map(Some),
        }
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn i64_rejects_text_value() {
        let result = i64::from_value(Value::Text("not-a-number".to_string()));
        let err = result.expect_err("expected conversion error");
        assert!(matches!(err.kind(), crate::error::ErrorKind::Conversion));
    }

    #[test]
    fn i32_rejects_out_of_range() {
        let result = i32::from_value(Value::Int(i64::MAX));
        let err = result.expect_err("expected range error");
        assert!(matches!(err.kind(), crate::error::ErrorKind::Conversion));
        assert!(err.message().contains("out of range"));
    }

    #[test]
    fn bool_rejects_text_value() {
        let result = bool::from_value(Value::Text("yes".to_string()));
        assert!(matches!(
            result.expect_err("expected conversion error").kind(),
            crate::error::ErrorKind::Conversion
        ));
    }

    #[test]
    fn f64_rejects_text_value() {
        let result = f64::from_value(Value::Text("NaN".to_string()));
        assert!(matches!(
            result.expect_err("expected conversion error").kind(),
            crate::error::ErrorKind::Conversion
        ));
    }

    #[test]
    fn string_rejects_int_value() {
        let result = String::from_value(Value::Int(42));
        assert!(matches!(
            result.expect_err("expected conversion error").kind(),
            crate::error::ErrorKind::Conversion
        ));
    }

    #[test]
    fn offset_datetime_rejects_garbage_text() {
        let result = OffsetDateTime::from_value(Value::Text("not a date".to_string()));
        let err = result.expect_err("expected conversion error");
        assert!(matches!(err.kind(), crate::error::ErrorKind::Conversion));
    }

    #[test]
    fn bind_unsigned_lowers_to_int() {
        assert!(matches!(42_u8.to_value(), Value::Int(42)));
        assert!(matches!(42_u16.to_value(), Value::Int(42)));
        assert!(matches!(42_u32.to_value(), Value::Int(42)));
        assert!(matches!(42_u64.to_value(), Value::Int(42)));
        assert!(matches!(42_usize.to_value(), Value::Int(42)));
    }

    #[test]
    fn bind_signed_lowers_to_int() {
        assert!(matches!((-1_i8).to_value(), Value::Int(-1)));
        assert!(matches!((-1_i16).to_value(), Value::Int(-1)));
    }

    #[test]
    fn bind_f32_lowers_to_real() {
        assert!(matches!(1.5_f32.to_value(), Value::Real(_)));
    }

    #[test]
    fn from_value_unsigned_round_trip() {
        let v: u32 = u32::from_value(Value::Int(42)).unwrap();
        assert_eq!(v, 42);
        let v: u8 = u8::from_value(Value::Int(255)).unwrap();
        assert_eq!(v, 255);
        let v: u64 = u64::from_value(Value::Int(i64::MAX)).unwrap();
        assert_eq!(v, i64::MAX as u64);
    }

    #[test]
    fn from_value_unsigned_rejects_out_of_range() {
        assert!(u8::from_value(Value::Int(-1)).is_err());
        assert!(u32::from_value(Value::Int(i64::MAX)).is_err());
        assert!(i16::from_value(Value::Int(i64::MAX)).is_err());
    }

    #[test]
    fn from_value_f32_round_trip() {
        let v: f32 = f32::from_value(Value::Real(1.5)).unwrap();
        assert!((v - 1.5).abs() < f32::EPSILON);
    }
}
