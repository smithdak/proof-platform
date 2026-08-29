use proof_kernel::canonicalize;
use serde::Serialize;
use sha2::{Digest, Sha256};

pub fn canonical_digest<T: Serialize>(value: &T) -> String {
    let encoded = canonical_json(value).expect("canonical JSON serialization cannot fail");
    let hash = Sha256::digest(encoded.as_bytes());
    format!("sha256:{}", hex::encode(hash))
}

fn canonical_json<T: Serialize>(value: &T) -> Result<String, serde_json::Error> {
    let json = serde_json::to_value(value)?;
    canonicalize(&json)
        .map(|encoded| encoded.as_str().to_owned())
        .map_err(|error| serde_json::Error::io(std::io::Error::other(error.to_string())))
}

#[cfg(test)]
mod tests {
    use super::canonical_digest;
    use serde_json::json;

    #[test]
    fn canonical_digest_is_stable_across_key_order() {
        let first = json!({"workflow_id": "1", "name": "Release"});
        let second = json!({"name": "Release", "workflow_id": "1"});
        assert_eq!(canonical_digest(&first), canonical_digest(&second));
    }
}
