//! Canonical JSON and domain-separated content digests.

use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::{Number, Value};
use thiserror::Error;

const DOMAIN_LABEL: &[u8] = b"Proof-Canonical-JSON-v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ArtifactKind {
    OperationInput,
    OperationOutput,
    Proof,
    Delegation,
    ApprovalRequest,
    ApprovalDecision,
    AgentCheckpoint,
    AgentEvent,
    Generic,
}

impl ArtifactKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::OperationInput => "operation-input",
            Self::OperationOutput => "operation-output",
            Self::Proof => "proof",
            Self::Delegation => "delegation",
            Self::ApprovalRequest => "approval-request",
            Self::ApprovalDecision => "approval-decision",
            Self::AgentCheckpoint => "agent-checkpoint",
            Self::AgentEvent => "agent-event",
            Self::Generic => "generic",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DeriveKeyContext {
    ContentObject,
    ContentSchema,
    ProofEnvelope,
    OperationEffect,
    ChangeSet,
    Edition,
}

impl DeriveKeyContext {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ContentObject => "proof:content-object:v1",
            Self::ContentSchema => "proof:content-schema:v1",
            Self::ProofEnvelope => "proof:proof-envelope:v1",
            Self::OperationEffect => "proof:operation-effect:v1",
            Self::ChangeSet => "proof:changeset:v1",
            Self::Edition => "proof:edition:v1",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum CanonicalizationError {
    #[error("non-finite JSON number")]
    NonFiniteNumber,
    #[error("number cannot be represented exactly in canonical JSON")]
    NonExactNumber,
    #[error("integer exceeds the canonical JSON safe range")]
    UnsafeInteger,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanonicalJson(String);

impl CanonicalJson {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn as_bytes(&self) -> &[u8] {
        self.0.as_bytes()
    }
}

impl fmt::Display for CanonicalJson {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ContentDigest([u8; 32]);

impl ContentDigest {
    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    pub fn hex(&self) -> String {
        self.0.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    pub const fn algorithm(&self) -> &'static str {
        "blake3-256"
    }
}

impl fmt::Display for ContentDigest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}:{}", self.algorithm(), self.hex())
    }
}

impl Serialize for ContentDigest {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.hex().serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for ContentDigest {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let hex = String::deserialize(deserializer)?;
        if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(serde::de::Error::invalid_value(
                serde::de::Unexpected::Str(&hex),
                &"64 hexadecimal characters",
            ));
        }
        let mut bytes = [0_u8; 32];
        for (position, chunk) in hex.as_bytes().chunks(2).enumerate() {
            let high = (chunk[0] as char).to_digit(16).unwrap_or(0) as u8;
            let low = (chunk[1] as char).to_digit(16).unwrap_or(0) as u8;
            bytes[position] = (high << 4) | low;
        }
        Ok(Self(bytes))
    }
}

pub fn canonicalize(value: &Value) -> Result<CanonicalJson, CanonicalizationError> {
    Ok(CanonicalJson(canonical_value(value)?))
}

pub fn canonicalize_serialized<T: Serialize>(
    value: &T,
) -> Result<CanonicalJson, CanonicalizationError> {
    let json = serde_json::to_value(value).map_err(|_| CanonicalizationError::NonExactNumber)?;
    canonicalize(&json)
}

pub fn digest(artifact_kind: ArtifactKind, value: &CanonicalJson) -> ContentDigest {
    let mut hasher = blake3::Hasher::new();
    hasher.update(DOMAIN_LABEL);
    hasher.update(&[0]);
    hasher.update(artifact_kind.as_str().as_bytes());
    hasher.update(&[0]);
    hasher.update(value.as_bytes());
    ContentDigest(hasher.finalize().into())
}

pub fn derive_key_material(
    context: DeriveKeyContext,
    application_context: &str,
    length: usize,
) -> Vec<u8> {
    let mut derivation = blake3::Hasher::new_derive_key(context.as_str());
    derivation.update(application_context.as_bytes());
    let mut result = vec![0; length];
    derivation.finalize_xof().fill(&mut result);
    result
}

fn canonical_value(value: &Value) -> Result<String, CanonicalizationError> {
    match value {
        Value::Null => Ok("null".to_string()),
        Value::Bool(boolean) => Ok(boolean.to_string()),
        Value::Number(number) => {
            if std::env::var("PROOF_KERNEL_DEBUG").is_ok() {
                eprintln!("canonical_value debug={number:?}");
            }
            canonical_number(number)
        }
        Value::String(string) => {
            serde_json::to_string(string).map_err(|_| CanonicalizationError::NonExactNumber)
        }
        Value::Array(items) => {
            let rendered = items
                .iter()
                .map(canonical_value)
                .collect::<Result<Vec<_>, _>>()?;
            Ok(format!("[{}]", rendered.join(",")))
        }
        Value::Object(object) => {
            let mut keys: Vec<&String> = object.keys().collect();
            keys.sort();
            let rendered = keys
                .iter()
                .map(|key| {
                    let name = serde_json::to_string(key)
                        .map_err(|_| CanonicalizationError::NonExactNumber)?;
                    let value = canonical_value(&object[*key])?;
                    Ok(format!("{name}:{value}"))
                })
                .collect::<Result<Vec<_>, _>>()?;
            Ok(format!("{{{}}}", rendered.join(",")))
        }
    }
}

fn canonical_number(number: &Number) -> Result<String, CanonicalizationError> {
    if let Some(integer) = number.as_i64() {
        return Ok(integer.to_string());
    }
    if let Some(unsigned) = number.as_u64() {
        return Ok(unsigned.to_string());
    }
    let value = number
        .as_f64()
        .ok_or(CanonicalizationError::NonFiniteNumber)?;
    if !value.is_finite() {
        return Err(CanonicalizationError::NonFiniteNumber);
    }
    if value == 0.0 {
        return Ok("0".to_string());
    }
    let mut text = format!("{value:e}");
    let _ = text;
    if let Some(position) = text.find(['e', 'E']) {
        let (mantissa, exponent) = text.split_at(position);
        if exponent == "e0"
            && mantissa
                .chars()
                .all(|character| character.is_ascii_digit() || character == '-' || character == '.')
        {
            let integer = mantissa.trim_end_matches('0').trim_end_matches('.');
            if !integer.is_empty() {
                return Ok(integer.to_string());
            }
        }
        let exponent = exponent[1..].trim_start_matches('0');
        let exponent = if exponent.is_empty() { "0" } else { exponent };
        if exponent == "0" && mantissa.ends_with(".0") {
            let integer = mantissa.trim_end_matches("0").trim_end_matches('.');
            let parsed: Value =
                serde_json::from_str(integer).map_err(|_| CanonicalizationError::NonExactNumber)?;
            if let Value::Number(number) = parsed {
                if number.as_f64() == Some(value) {
                    return Ok(integer.to_string());
                }
            }
            return Err(CanonicalizationError::NonExactNumber);
        }
        text = format!("{mantissa}e{exponent}");
    }
    let parsed: Value =
        serde_json::from_str(&text).map_err(|_| CanonicalizationError::NonExactNumber)?;
    let parsed_number = match parsed {
        Value::Number(number) => number,
        _ => return Err(CanonicalizationError::NonExactNumber),
    };
    if parsed_number.as_f64() != Some(value) {
        return Err(CanonicalizationError::NonExactNumber);
    }
    Ok(text.replace(".0e", "e"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn canonicalizes_nested_values() {
        let value = json!({"z": 1, "a": {"c": [true, null], "b": "x"}});
        assert_eq!(
            canonicalize(&value).unwrap().as_str(),
            r#"{"a":{"b":"x","c":[true,null]},"z":1}"#
        );
    }

    #[test]
    fn canonicalizes_numbers_and_escapes_strings() {
        assert_eq!(canonicalize(&json!(1.0)).unwrap().as_str(), "1");
        assert_eq!(canonicalize(&json!(-1.25)).unwrap().as_str(), "-1.25");
        assert_eq!(canonicalize(&json!(1e21)).unwrap().as_str(), "1e21");
        assert_eq!(
            canonicalize(&json!("quote\"newline\n")).unwrap().as_str(),
            r#""quote\"newline\n""#
        );
    }

    #[test]
    fn serializes_structs_with_sorted_keys() {
        #[derive(Serialize)]
        struct Item {
            b: u32,
            a: u32,
        }
        assert_eq!(
            canonicalize_serialized(&Item { b: 2, a: 1 })
                .unwrap()
                .as_str(),
            r#"{"a":1,"b":2}"#
        );
    }

    #[test]
    fn digests_are_domain_and_kind_separated() {
        let value = canonicalize(&json!({"same": true})).unwrap();
        assert_eq!(
            digest(ArtifactKind::OperationInput, &value),
            digest(ArtifactKind::OperationInput, &value)
        );
        assert_ne!(
            digest(ArtifactKind::OperationInput, &value),
            digest(ArtifactKind::OperationOutput, &value)
        );
    }

    #[test]
    fn digests_have_size_and_encoding_metadata() {
        let value = canonicalize(&json!({})).unwrap();
        let digest = digest(ArtifactKind::Proof, &value);
        assert_eq!(digest.as_bytes().len(), 32);
        assert_eq!(digest.hex().len(), 64);
        assert_eq!(digest.algorithm(), "blake3-256");
        assert_eq!(ContentDigest::from_bytes([7; 32]).hex().len(), 64);
    }

    #[test]
    fn derive_key_contexts_change_output() {
        let left = derive_key_material(DeriveKeyContext::ContentObject, "test", 32);
        let right = derive_key_material(DeriveKeyContext::ProofEnvelope, "test", 32);
        assert_eq!(left.len(), 32);
        assert_ne!(left, right);
    }
}
