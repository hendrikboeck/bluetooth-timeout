// -- std imports
use std::time::Duration;

// -- crate imports
use serde::{Deserialize, Deserializer};

pub mod humantime_serde_duration {
    use super::*;

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Duration, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        humantime::parse_duration(&s).map_err(serde::de::Error::custom)
    }

    pub fn deserialize_vec<'de, D>(deserializer: D) -> Result<Vec<Duration>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let v = Vec::<String>::deserialize(deserializer)?;
        v.into_iter()
            .map(|s| humantime::parse_duration(&s).map_err(serde::de::Error::custom))
            .collect()
    }
}
