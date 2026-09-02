use std::{fmt, str::FromStr};

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde::{de::Error as _, Deserialize, Deserializer, Serialize, Serializer};
use sha2::{Digest as _, Sha256};
use subtle::ConstantTimeEq;
use thiserror::Error;

use crate::{canonicalize_serialized, CanonicalizationError};

pub(crate) mod strict_uuid_v7 {
    use serde::{Deserialize, Deserializer, Serializer};
    use uuid::Uuid;

    pub fn serialize<S: Serializer>(value: &Uuid, serializer: S) -> Result<S::Ok, S::Error> {
        if !super::uuid_is_v7(*value) {
            return Err(serde::ser::Error::custom(
                "UUID must be canonical RFC4122 UUIDv7",
            ));
        }
        serializer.serialize_str(&value.hyphenated().to_string())
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Uuid, D::Error> {
        let wire = String::deserialize(deserializer)?;
        let value = Uuid::parse_str(&wire).map_err(serde::de::Error::custom)?;
        if wire.len() != 36
            || wire.bytes().any(|byte| byte.is_ascii_uppercase())
            || value.hyphenated().to_string() != wire
            || !super::uuid_is_v7(value)
        {
            return Err(serde::de::Error::custom(
                "UUID must be canonical lowercase hyphenated RFC4122 UUIDv7",
            ));
        }
        Ok(value)
    }
}

pub(crate) mod strict_optional_uuid_v7 {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use uuid::Uuid;

    struct Strict<'a>(&'a Uuid);
    impl Serialize for Strict<'_> {
        fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
            super::strict_uuid_v7::serialize(self.0, serializer)
        }
    }

    pub fn serialize<S: Serializer>(
        value: &Option<Uuid>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        match value {
            Some(value) => serializer.serialize_some(&Strict(value)),
            None => serializer.serialize_none(),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<Uuid>, D::Error> {
        let wire = Option::<String>::deserialize(deserializer)?;
        wire.map(|wire| {
            let value = Uuid::parse_str(&wire).map_err(serde::de::Error::custom)?;
            if wire.len() != 36
                || wire.bytes().any(|byte| byte.is_ascii_uppercase())
                || value.hyphenated().to_string() != wire
                || !super::uuid_is_v7(value)
            {
                return Err(serde::de::Error::custom(
                    "UUID must be canonical lowercase hyphenated RFC4122 UUIDv7",
                ));
            }
            Ok(value)
        })
        .transpose()
    }
}

pub(crate) mod strict_principal_id {
    use serde::{Deserializer, Serializer};

    use crate::PrincipalId;

    pub fn serialize<S: Serializer>(value: &PrincipalId, serializer: S) -> Result<S::Ok, S::Error> {
        super::strict_uuid_v7::serialize(&value.as_uuid(), serializer)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<PrincipalId, D::Error> {
        super::strict_uuid_v7::deserialize(deserializer).map(PrincipalId::new)
    }
}

pub(crate) mod strict_utc {
    use chrono::{DateTime, SecondsFormat, Timelike, Utc};
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(
        value: &DateTime<Utc>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        if value.nanosecond() >= 1_000_000_000 {
            return Err(serde::ser::Error::custom(
                "UTC leap seconds are unsupported",
            ));
        }
        let wire = value.to_rfc3339_opts(SecondsFormat::AutoSi, true);
        if !valid_wire(&wire) {
            return Err(serde::ser::Error::custom(
                "time is outside canonical RFC3339 UTC",
            ));
        }
        serializer.serialize_str(&wire)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<DateTime<Utc>, D::Error> {
        let wire = String::deserialize(deserializer)?;
        if !valid_wire(&wire) {
            return Err(serde::de::Error::custom(
                "time must be canonical RFC3339 UTC with Z suffix",
            ));
        }
        let value = DateTime::parse_from_rfc3339(&wire)
            .map_err(serde::de::Error::custom)?
            .with_timezone(&Utc);
        if value.nanosecond() >= 1_000_000_000 {
            return Err(serde::de::Error::custom("UTC leap seconds are unsupported"));
        }
        Ok(value)
    }

    fn valid_wire(value: &str) -> bool {
        let bytes = value.as_bytes();
        if bytes.len() < 20
            || bytes[4] != b'-'
            || bytes[7] != b'-'
            || bytes[10] != b'T'
            || bytes[13] != b':'
            || bytes[16] != b':'
            || bytes.last() != Some(&b'Z')
            || !bytes[..19]
                .iter()
                .enumerate()
                .filter(|(index, _)| !matches!(index, 4 | 7 | 10 | 13 | 16))
                .all(|(_, byte)| byte.is_ascii_digit())
            || &value[17..19] > "59"
        {
            return false;
        }
        match bytes.len() {
            20 => true,
            22..=30 => {
                bytes[19] == b'.' && bytes[20..bytes.len() - 1].iter().all(u8::is_ascii_digit)
            }
            _ => false,
        }
    }
}

pub(crate) mod strict_optional_utc {
    use chrono::{DateTime, Utc};
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    struct Strict<'a>(&'a DateTime<Utc>);
    impl Serialize for Strict<'_> {
        fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
            super::strict_utc::serialize(self.0, serializer)
        }
    }

    pub fn serialize<S: Serializer>(
        value: &Option<DateTime<Utc>>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        match value {
            Some(value) => serializer.serialize_some(&Strict(value)),
            None => serializer.serialize_none(),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<DateTime<Utc>>, D::Error> {
        let wire = Option::<String>::deserialize(deserializer)?;
        wire.map(|wire| {
            let deserializer = serde::de::value::StringDeserializer::<D::Error>::new(wire);
            super::strict_utc::deserialize(deserializer)
        })
        .transpose()
    }
}

pub(crate) mod strict_safe_integer {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(value: &u64, serializer: S) -> Result<S::Ok, S::Error> {
        if *value > super::MAX_SAFE_INTEGER {
            return Err(serde::ser::Error::custom(
                "integer exceeds the JSON safe-integer maximum",
            ));
        }
        serializer.serialize_u64(*value)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u64, D::Error> {
        let value = u64::deserialize(deserializer)?;
        if value > super::MAX_SAFE_INTEGER {
            return Err(serde::de::Error::custom(
                "integer exceeds the JSON safe-integer maximum",
            ));
        }
        Ok(value)
    }
}

pub(crate) mod strict_optional_safe_integer {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    struct Strict(u64);
    impl Serialize for Strict {
        fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
            super::strict_safe_integer::serialize(&self.0, serializer)
        }
    }

    pub fn serialize<S: Serializer>(value: &Option<u64>, serializer: S) -> Result<S::Ok, S::Error> {
        match value {
            Some(value) => serializer.serialize_some(&Strict(*value)),
            None => serializer.serialize_none(),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<u64>, D::Error> {
        Option::<u64>::deserialize(deserializer)?
            .map(|value| {
                if value > super::MAX_SAFE_INTEGER {
                    Err(serde::de::Error::custom(
                        "integer exceeds the JSON safe-integer maximum",
                    ))
                } else {
                    Ok(value)
                }
            })
            .transpose()
    }
}

pub const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;
pub fn uuid_is_v7(value: uuid::Uuid) -> bool {
    value.get_version_num() == 7 && value.get_variant() == uuid::Variant::RFC4122
}

pub(crate) fn valid_operation_name(value: &str) -> bool {
    value.len() <= 128
        && value.split_once('.').is_some_and(|(domain, action)| {
            !domain.is_empty()
                && !action.is_empty()
                && !action.contains('.')
                && valid_lower_identifier(domain)
                && valid_lower_identifier(action)
        })
}

pub(crate) fn valid_operation_version(value: &str) -> bool {
    let Some(digits) = value.strip_prefix('v') else {
        return false;
    };
    value.len() <= 16
        && !digits.is_empty()
        && !digits.starts_with('0')
        && digits.bytes().all(|byte| byte.is_ascii_digit())
}

pub(crate) fn valid_adapter_name(value: &str) -> bool {
    (2..=64).contains(&value.len())
        && value.as_bytes().first().is_some_and(u8::is_ascii_lowercase)
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'_' | b'-')
        })
}

pub(crate) fn valid_model_name(value: &str) -> bool {
    (1..=128).contains(&value.len())
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b':' | b'/' | b'-')
        })
}

pub(crate) fn valid_fixed_base64url(value: &str, decoded_len: usize) -> bool {
    URL_SAFE_NO_PAD
        .decode(value)
        .is_ok_and(|bytes| bytes.len() == decoded_len && URL_SAFE_NO_PAD.encode(bytes) == value)
}

fn valid_lower_identifier(value: &str) -> bool {
    value.as_bytes().first().is_some_and(u8::is_ascii_lowercase)
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ControlDigest([u8; 32]);

impl ControlDigest {
    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
    pub fn encoded(&self) -> String {
        format!("blake3-256:{}", hex(&self.0))
    }
}

impl fmt::Display for ControlDigest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.encoded())
    }
}

impl FromStr for ControlDigest {
    type Err = DigestParseError;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let hex = value.strip_prefix("blake3-256:").ok_or(DigestParseError)?;
        parse_hex32(hex).map(Self)
    }
}

impl Serialize for ControlDigest {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.encoded())
    }
}

impl<'de> Deserialize<'de> for ControlDigest {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer)?
            .parse()
            .map_err(D::Error::custom)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ArtifactDigest([u8; 32]);

impl ArtifactDigest {
    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
    pub fn encoded(&self) -> String {
        format!("sha256:{}", hex(&self.0))
    }
}

impl fmt::Display for ArtifactDigest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.encoded())
    }
}

impl FromStr for ArtifactDigest {
    type Err = DigestParseError;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let hex = value.strip_prefix("sha256:").ok_or(DigestParseError)?;
        parse_hex32(hex).map(Self)
    }
}

impl Serialize for ArtifactDigest {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.encoded())
    }
}

impl<'de> Deserialize<'de> for ArtifactDigest {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer)?
            .parse()
            .map_err(D::Error::custom)
    }
}

#[derive(Debug, Clone, Copy, Error, PartialEq, Eq)]
#[error("digest must use its exact algorithm prefix and 64 lowercase hexadecimal characters")]
pub struct DigestParseError;

pub fn control_digest(domain_label: &str, payload: &[u8]) -> ControlDigest {
    let mut hasher = blake3::Hasher::new();
    hasher.update(domain_label.as_bytes());
    hasher.update(&[0]);
    hasher.update(payload);
    ControlDigest::from_bytes(hasher.finalize().into())
}

pub fn control_digest_serialized<T: Serialize>(
    domain_label: &str,
    value: &T,
) -> Result<ControlDigest, CanonicalizationError> {
    let canonical = canonicalize_serialized(value)?;
    Ok(control_digest(domain_label, canonical.as_bytes()))
}

/// SHA-256 of exact raw artifact bytes. This helper never canonicalizes input.
pub fn raw_artifact_sha256(bytes: &[u8]) -> ArtifactDigest {
    ArtifactDigest::from_bytes(Sha256::digest(bytes).into())
}

/// Compares all 32 bytes without data-dependent early return.
pub fn constant_time_eq_32(left: &[u8; 32], right: &[u8; 32]) -> bool {
    bool::from(left.ct_eq(right))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Capability {
    #[serde(rename = "approval.decide")]
    ApprovalDecide,
    #[serde(rename = "approval.read")]
    ApprovalRead,
    #[serde(rename = "audit.read")]
    AuditRead,
    #[serde(rename = "run.cancel")]
    RunCancel,
    #[serde(rename = "run.read")]
    RunRead,
    #[serde(rename = "run.resume")]
    RunResume,
}

impl Capability {
    pub const ALL: [Self; 6] = [
        Self::ApprovalDecide,
        Self::ApprovalRead,
        Self::AuditRead,
        Self::RunCancel,
        Self::RunRead,
        Self::RunResume,
    ];
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum CapabilitySetError {
    #[error("capability set must contain between one and six entries")]
    InvalidLength,
    #[error("capability set must be unique and in canonical order")]
    NonCanonical,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CapabilitySet(Vec<Capability>);

impl CapabilitySet {
    pub fn new(values: Vec<Capability>) -> Result<Self, CapabilitySetError> {
        if values.is_empty() || values.len() > Capability::ALL.len() {
            return Err(CapabilitySetError::InvalidLength);
        }
        if values.windows(2).any(|pair| pair[0] >= pair[1]) {
            return Err(CapabilitySetError::NonCanonical);
        }
        Ok(Self(values))
    }
    pub fn all() -> Self {
        Self(Capability::ALL.to_vec())
    }
    pub fn as_slice(&self) -> &[Capability] {
        &self.0
    }
    pub fn contains(&self, capability: Capability) -> bool {
        self.0.binary_search(&capability).is_ok()
    }
    pub fn iter(&self) -> impl Iterator<Item = Capability> + '_ {
        self.0.iter().copied()
    }
}

impl Serialize for CapabilitySet {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for CapabilitySet {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::new(Vec::<Capability>::deserialize(deserializer)?).map_err(D::Error::custom)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BudgetAmounts {
    #[serde(with = "strict_safe_integer")]
    pub steps: u64,
    #[serde(with = "strict_safe_integer")]
    pub tokens: u64,
    #[serde(with = "strict_safe_integer")]
    pub duration_ms: u64,
    #[serde(with = "strict_safe_integer")]
    pub cost_microusd: u64,
    #[serde(with = "strict_safe_integer")]
    pub tool_dispatches: u64,
}

impl BudgetAmounts {
    pub fn is_safe(&self) -> bool {
        [
            self.steps,
            self.tokens,
            self.duration_ms,
            self.cost_microusd,
            self.tool_dispatches,
        ]
        .into_iter()
        .all(|value| value <= MAX_SAFE_INTEGER)
    }
    pub fn fits_within(&self, ceiling: &Self) -> bool {
        self.steps <= ceiling.steps
            && self.tokens <= ceiling.tokens
            && self.duration_ms <= ceiling.duration_ms
            && self.cost_microusd <= ceiling.cost_microusd
            && self.tool_dispatches <= ceiling.tool_dispatches
    }
}

fn parse_hex32(value: &str) -> Result<[u8; 32], DigestParseError> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(DigestParseError);
    }
    let mut result = [0_u8; 32];
    for (index, pair) in value.as_bytes().chunks_exact(2).enumerate() {
        result[index] = (nibble(pair[0])? << 4) | nibble(pair[1])?;
    }
    Ok(result)
}

fn nibble(value: u8) -> Result<u8, DigestParseError> {
    match value {
        b'0'..=b'9' => Ok(value - b'0'),
        b'a'..=b'f' => Ok(value - b'a' + 10),
        _ => Err(DigestParseError),
    }
}

fn hex(bytes: &[u8; 32]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut result = String::with_capacity(64);
    for byte in bytes {
        result.push(DIGITS[(byte >> 4) as usize] as char);
        result.push(DIGITS[(byte & 0xf) as usize] as char);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};
    use serde::{Deserialize, Serialize};

    #[derive(Serialize, Deserialize)]
    struct StrictWire {
        #[serde(with = "super::strict_uuid_v7")]
        id: uuid::Uuid,
        #[serde(with = "super::strict_utc")]
        at: chrono::DateTime<Utc>,
    }
    #[test]
    fn digest_wires_are_strict_and_helpers_are_stable() {
        let artifact = raw_artifact_sha256(b"abc");
        assert_eq!(
            artifact.encoded(),
            "sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(artifact.encoded().parse::<ArtifactDigest>(), Ok(artifact));
        assert!("sha256:ABC".parse::<ArtifactDigest>().is_err());
        assert!(constant_time_eq_32(
            artifact.as_bytes(),
            artifact.as_bytes()
        ));
    }
    #[test]
    fn capability_sets_require_canonical_order() {
        assert_eq!(CapabilitySet::all().as_slice(), &Capability::ALL);
        assert!(CapabilitySet::new(vec![Capability::RunRead, Capability::ApprovalRead]).is_err());
    }

    #[test]
    fn uuidv7_requires_the_rfc4122_variant() {
        fn uuid_with_variant(variant: u8) -> uuid::Uuid {
            let mut bytes = [0_u8; 16];
            bytes[6] = 0x70;
            bytes[8] = variant;
            uuid::Uuid::from_bytes(bytes)
        }
        assert!(uuid_is_v7(uuid_with_variant(0x80)));
        assert!(!uuid_is_v7(uuid_with_variant(0x00)));
        assert!(!uuid_is_v7(uuid_with_variant(0xc0)));
        assert!(!uuid_is_v7(uuid_with_variant(0xe0)));
    }

    #[test]
    fn strict_wire_rejects_uuid_normalization_offsets_and_leap_seconds() {
        let canonical =
            r#"{"id":"01890f47-9bcd-7def-8123-456789ab0001","at":"2030-01-01T00:00:00Z"}"#;
        assert!(serde_json::from_str::<StrictWire>(canonical).is_ok());
        for invalid in [
            r#"{"id":"01890F47-9BCD-7DEF-8123-456789AB0001","at":"2030-01-01T00:00:00Z"}"#,
            r#"{"id":"01890f479bcd7def8123456789ab0001","at":"2030-01-01T00:00:00Z"}"#,
            r#"{"id":"01890f47-9bcd-7def-0123-456789ab0001","at":"2030-01-01T00:00:00Z"}"#,
            r#"{"id":"01890f47-9bcd-7def-c123-456789ab0001","at":"2030-01-01T00:00:00Z"}"#,
            r#"{"id":"01890f47-9bcd-7def-e123-456789ab0001","at":"2030-01-01T00:00:00Z"}"#,
            r#"{"id":"01890f47-9bcd-7def-8123-456789ab0001","at":"2030-01-01T01:00:00+01:00"}"#,
            r#"{"id":"01890f47-9bcd-7def-8123-456789ab0001","at":"2030-01-01T00:00:60Z"}"#,
        ] {
            assert!(serde_json::from_str::<StrictWire>(invalid).is_err());
        }
        let invalid_uuid = StrictWire {
            id: uuid::Uuid::nil(),
            at: Utc.with_ymd_and_hms(2030, 1, 1, 0, 0, 0).unwrap(),
        };
        assert!(serde_json::to_string(&invalid_uuid).is_err());
        let expanded_year = StrictWire {
            id: uuid::Uuid::parse_str("01890f47-9bcd-7def-8123-456789ab0001").unwrap(),
            at: Utc.with_ymd_and_hms(10_000, 1, 1, 0, 0, 0).unwrap(),
        };
        assert!(serde_json::to_string(&expanded_year).is_err());
    }

    #[test]
    fn budget_amounts_reject_unsafe_json_integers_in_both_directions() {
        let invalid = format!(
            r#"{{"steps":{},"tokens":0,"duration_ms":0,"cost_microusd":0,"tool_dispatches":0}}"#,
            MAX_SAFE_INTEGER + 1
        );
        assert!(serde_json::from_str::<BudgetAmounts>(&invalid).is_err());

        let amounts = BudgetAmounts {
            steps: MAX_SAFE_INTEGER + 1,
            ..BudgetAmounts::default()
        };
        assert!(serde_json::to_string(&amounts).is_err());
    }
}
