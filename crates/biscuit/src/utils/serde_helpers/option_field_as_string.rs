use serde::de::{Deserialize, Deserializer, Error};
use serde::{Serialize, Serializer};
use std::str::FromStr;

pub fn serialize<T, S>(t: &Option<T>, serializer: S) -> Result<S::Ok, S::Error>
where
    T: ToString,
    S: Serializer,
{
    if let Some(t) = t {
        t.to_string().serialize(serializer)
    } else {
        serializer.serialize_none()
    }
}

pub fn deserialize<'de, T, D>(deserializer: D) -> Result<Option<T>, D::Error>
where
    T: FromStr,
    D: Deserializer<'de>,
    <T as FromStr>::Err: std::fmt::Debug,
{
    let t: Option<String> = Option::deserialize(deserializer)?;

    match t {
        Some(s) => T::from_str(&s)
            .map(Some)
            .map_err(|_| Error::custom(format!("Parse error for {}", s))),
        None => Ok(None),
    }
}
