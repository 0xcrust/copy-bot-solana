use serde::de::{Deserializer, Visitor};
use solana_sdk::pubkey::Pubkey;
use std::str::FromStr;

struct PubkeyVisitor;
impl<'de> Visitor<'de> for PubkeyVisitor {
    type Value = Pubkey;
    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str(r#"a pubkey string"#)
    }
    fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        Pubkey::from_str(v).map_err(|_| E::custom("failed parsing pubkey from str"))
    }
}

pub fn serialize<S>(key: &Pubkey, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    serializer.serialize_str(&key.to_string())
}

pub fn deserialize<'de, D>(deserializer: D) -> Result<Pubkey, D::Error>
where
    D: Deserializer<'de>,
{
    deserializer.deserialize_str(PubkeyVisitor)
}
