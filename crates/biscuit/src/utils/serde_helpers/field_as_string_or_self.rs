use serde::{de, Deserialize, Deserializer};
use std::str::FromStr;

pub fn deserialize<'de, T, D>(deserializer: D) -> Result<T, D::Error>
where
    T: FromStr + Deserialize<'de>,
    D: Deserializer<'de>,
    <T as FromStr>::Err: std::fmt::Debug,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    pub enum Helper<S> {
        Str(String),
        Original(S),
    }

    let deserialized: Helper<T> = Helper::deserialize(deserializer)?;
    match deserialized {
        Helper::Str(s) => s
            .parse()
            .map_err(|e| de::Error::custom(format!("Parse error: {:?}", e))),
        Helper::Original(original) => Ok(original),
    }
}
