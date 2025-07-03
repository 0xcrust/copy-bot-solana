use serde::Deserialize;
use std::iter::FromIterator;

pub fn serialize<'a, S: serde::Serializer, T: IntoIterator<Item = &'a V>, V: ToString + 'a>(
    target: T,
    s: S,
) -> Result<S::Ok, S::Error> {
    let container: Vec<_> = target.into_iter().map(|k| k.to_string()).collect();
    serde::Serialize::serialize(&container, s)
}

pub fn deserialize<'de, D: serde::Deserializer<'de>, T: FromIterator<V>, V: std::str::FromStr>(
    d: D,
) -> Result<T, D::Error> {
    let hashmap_as_vec: Vec<String> = Deserialize::deserialize(d)?;
    let mut res = vec![];
    for item in hashmap_as_vec {
        res.push(V::from_str(&item).map_err(|e| {
            serde::de::Error::custom(format!(
                "string conversion error in collection deserialization"
            ))
        })?);
    }
    Ok(T::from_iter(res))
}
