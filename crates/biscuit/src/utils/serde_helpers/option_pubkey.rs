use super::pubkey;

use serde::de::{Deserialize, Deserializer};
use serde::{Serialize, Serializer};
use solana_sdk::pubkey::Pubkey;

pub fn serialize<S>(key: &Option<Pubkey>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    #[derive(Serialize)]
    struct Helper<'a>(#[serde(serialize_with = "pubkey::serialize")] &'a Pubkey);
    key.as_ref().map(Helper).serialize(serializer)
}

pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<Pubkey>, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(serde::Deserialize)]
    struct Helper(#[serde(deserialize_with = "pubkey::deserialize")] Pubkey);
    let helper = Option::deserialize(deserializer)?;
    Ok(helper.map(|Helper(external)| external))
}
