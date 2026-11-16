//! Serde helpers for the generated types.

use serde::{Deserialize, Deserializer};

/// Keeps `null` apart from absent: with `#[serde(default)]` an absent field stays
/// `None`, while a present one — `null` included — becomes `Some(_)`.
pub(crate) fn double_option<'de, T, D>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    T: Deserialize<'de>,
    D: Deserializer<'de>,
{
    Option::<T>::deserialize(deserializer).map(Some)
}
