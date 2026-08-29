use serde::Serialize;
use sha2::{Digest, Sha256};

pub fn canonical_digest<T: Serialize>(value: &T) -> String {
    let encoded = canonical_json(value).expect("canonical JSON serialization cannot fail");
    let hash = Sha256::digest(encoded.as_bytes());
    format!("sha256:{}", hex::encode(hash))
}

fn canonical_json<T: Serialize>(value: &T) -> Result<String, serde_json::Error> {
    let json = serde_json::to_value(value)?;
    let mut canonical_value = json;
    canonicalize(&mut canonical_value)?;
    serde_json::to_string(&canonical_value)
}

fn canonicalize(value: &mut serde_json::Value) -> Result<(), serde_json::Error> {
    match value {
        serde_json::Value::Array(items) => {
            for item in items {
                canonicalize(item)?;
            }
        }
        serde_json::Value::Object(map) => {
            let mut canonical = serde_json::Map::new();
            for (key, mut item) in std::mem::take(map) {
                canonicalize(&mut item)?;
                canonical.insert(key, item);
            }
            *map = canonical;
        }
        _ => {}
    }
    Ok(())
}
