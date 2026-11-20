//! Serialization helpers shared by transport and stored document types.

/// For optional JSON fields where `null` means something else than "not sent":
/// absent → `None`, `null` → `Some(None)`, a value → `Some(Some(value))`. Use with
/// `#[serde(default, deserialize_with = "…")]`.
pub fn double_option<'de, T, D>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    T: serde::Deserialize<'de>,
    D: serde::Deserializer<'de>,
{
    Ok(Some(<Option<T> as serde::Deserialize>::deserialize(
        deserializer,
    )?))
}
