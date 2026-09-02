//! Strict v1 authentication DTOs and immutable authority configuration.

use std::fmt;

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::{DateTime, SecondsFormat, Utc};
use ed25519_dalek::VerifyingKey;
use proof_kernel::{
    control_digest_serialized, Capability, CapabilitySet, ControlDigest, HumanEnrollment,
    OperatorWorkspace, PrincipalKind,
};
use serde::{de::Error as _, Deserialize, Deserializer, Serialize, Serializer};
use uuid::Uuid;
use zeroize::{Zeroize, Zeroizing};

use crate::{authority::public_key_fingerprint, OperatorAuthError};

pub const CHALLENGE_TTL_SECONDS: u64 = 120;
pub const SESSION_ABSOLUTE_TTL_SECONDS: u64 = 900;
pub const SESSION_IDLE_TTL_SECONDS: u64 = 300;

pub const ALL_CAPABILITIES: [Capability; 6] = [
    Capability::ApprovalDecide,
    Capability::ApprovalRead,
    Capability::AuditRead,
    Capability::RunCancel,
    Capability::RunRead,
    Capability::RunResume,
];

#[derive(Debug, Clone)]
pub struct AuthPolicy {
    pub(crate) workspace_id: Uuid,
    pub(crate) workspace_fingerprint: ControlDigest,
    pub(crate) server_instance_id: Uuid,
    pub(crate) human_id: Uuid,
    pub(crate) human_public_key: VerifyingKey,
    pub(crate) human_public_key_fingerprint: ControlDigest,
    pub(crate) auth_epoch: u64,
    pub(crate) policy_revision: u64,
    pub(crate) origin: String,
    pub(crate) enrolled_capabilities: CapabilitySet,
    pub(crate) workspace_capabilities: CapabilitySet,
    pub(crate) supported_capabilities: CapabilitySet,
}

impl AuthPolicy {
    pub fn from_workspace(
        workspace: &OperatorWorkspace,
        enrollment: &HumanEnrollment,
        server_instance_id: Uuid,
        human_public_key: VerifyingKey,
        origin: String,
    ) -> Result<Self, OperatorAuthError> {
        let capability_set_digest =
            control_digest_serialized("Proof-Operator-Capability-Set-v1", &enrollment.capabilities)
                .map_err(|_| OperatorAuthError::InvalidRequest)?;
        let human_id = workspace.human.principal_id.as_uuid();
        let expected_public_key = URL_SAFE_NO_PAD.encode(human_public_key.as_bytes());
        if workspace.schema != OperatorWorkspace::SCHEMA
            || enrollment.schema != HumanEnrollment::SCHEMA
            || workspace.workspace_id != enrollment.workspace_id
            || workspace.human != enrollment.human
            || workspace.human.kind != PrincipalKind::Human
            || workspace.capabilities != enrollment.capabilities
            || enrollment.capability_set_digest != capability_set_digest
            || workspace.fingerprint_input.workspace_id != workspace.workspace_id
            || workspace.fingerprint_input.human_id != human_id
            || workspace.fingerprint_input.human_public_key != workspace.human.public_key
            || workspace.human.public_key != expected_public_key
            || workspace.human.public_key_fingerprint != public_key_fingerprint(&human_public_key)
        {
            return Err(OperatorAuthError::InvalidRequest);
        }
        let policy = Self {
            workspace_id: workspace.workspace_id,
            workspace_fingerprint: workspace.workspace_fingerprint,
            server_instance_id,
            human_id,
            human_public_key,
            human_public_key_fingerprint: workspace.human.public_key_fingerprint,
            auth_epoch: workspace.auth_epoch,
            policy_revision: workspace.policy_revision,
            origin,
            enrolled_capabilities: enrollment.capabilities.clone(),
            workspace_capabilities: workspace.capabilities.clone(),
            supported_capabilities: CapabilitySet::all(),
        };
        policy.validate()?;
        Ok(policy)
    }

    pub fn validate(&self) -> Result<(), OperatorAuthError> {
        if !is_uuid_v7(self.workspace_id)
            || !is_uuid_v7(self.server_instance_id)
            || !is_uuid_v7(self.human_id)
            || self.auth_epoch != 1
            || self.policy_revision != 1
            || self.human_public_key_fingerprint != public_key_fingerprint(&self.human_public_key)
            || !valid_origin(&self.origin)
            || !is_canonical_capability_set(self.enrolled_capabilities.as_slice())
            || !is_canonical_capability_set(self.workspace_capabilities.as_slice())
            || !is_canonical_capability_set(self.supported_capabilities.as_slice())
            || self.enrolled_capabilities != self.workspace_capabilities
        {
            return Err(OperatorAuthError::InvalidRequest);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChallengeIssueRequest {
    pub schema: String,
    pub client_nonce_digest: ControlDigest,
    pub requested_capabilities: CapabilitySet,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionChallenge {
    pub schema: String,
    #[serde(with = "strict_uuid_v7")]
    pub challenge_id: Uuid,
    #[serde(with = "strict_uuid_v7")]
    pub server_instance_id: Uuid,
    pub server_nonce: String,
    #[serde(with = "strict_uuid_v7")]
    pub workspace_id: Uuid,
    pub workspace_fingerprint: ControlDigest,
    #[serde(with = "strict_uuid_v7")]
    pub human_id: Uuid,
    pub human_public_key_fingerprint: ControlDigest,
    pub auth_epoch: u64,
    pub policy_revision: u64,
    pub origin: String,
    pub client_nonce_digest: ControlDigest,
    pub requested_capabilities: CapabilitySet,
    pub granted_capabilities: CapabilitySet,
    #[serde(with = "strict_datetime_utc")]
    pub issued_at: DateTime<Utc>,
    #[serde(with = "strict_datetime_utc")]
    pub expires_at: DateTime<Utc>,
    pub challenge_ttl_seconds: u64,
    pub session_absolute_ttl_seconds: u64,
    pub session_idle_ttl_seconds: u64,
}

impl Drop for SessionChallenge {
    fn drop(&mut self) {
        self.server_nonce.zeroize();
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChallengeIssueResponse {
    pub schema: String,
    pub challenge: SessionChallenge,
    pub challenge_code: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionAttestation {
    pub schema: String,
    pub challenge: SessionChallenge,
    pub signature_algorithm: String,
    pub signature: String,
    pub signed_bytes_digest: ControlDigest,
}

#[derive(PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionExchangeRequest {
    pub schema: String,
    #[serde(with = "strict_uuid_v7")]
    pub challenge_id: Uuid,
    pub client_nonce: String,
}

impl fmt::Debug for SessionExchangeRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SessionExchangeRequest")
            .field("schema", &self.schema)
            .field("challenge_id", &self.challenge_id)
            .field("client_nonce", &"[REDACTED]")
            .finish()
    }
}

impl Drop for SessionExchangeRequest {
    fn drop(&mut self) {
        self.client_nonce.zeroize();
    }
}

/// One response-only raw session token. It cannot be cloned or formatted.
pub struct SessionToken(Zeroizing<[u8; 32]>);

/// Borrow-only encoded session header with zeroizing, nonduplicable ownership.
pub struct SessionHeaderValue(Zeroizing<[u8; 64]>);

impl SessionHeaderValue {
    pub fn as_bytes(&self) -> &[u8] {
        self.0.as_ref()
    }
}

impl fmt::Debug for SessionHeaderValue {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SessionHeaderValue([REDACTED])")
    }
}

impl SessionToken {
    pub(crate) fn new(bytes: Zeroizing<[u8; 32]>) -> Self {
        Self(bytes)
    }

    /// Returns the canonical lowercase header value in zeroizing ownership.
    pub fn header_value(&self) -> SessionHeaderValue {
        let mut output = Zeroizing::new([0_u8; 64]);
        encode_hex_into(self.0.as_ref(), output.as_mut());
        SessionHeaderValue(output)
    }
}

impl Serialize for SessionToken {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut encoded = Zeroizing::new([0_u8; 64]);
        encode_hex_into(self.0.as_ref(), encoded.as_mut());
        let value = std::str::from_utf8(encoded.as_ref()).map_err(serde::ser::Error::custom)?;
        serializer.serialize_str(value)
    }
}

impl<'de> Deserialize<'de> for SessionToken {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let mut encoded = String::deserialize(deserializer)?;
        let result = decode_hex32(encoded.as_bytes())
            .map(SessionToken::new)
            .ok_or_else(|| {
                D::Error::custom("session token must be 64 lowercase hexadecimal characters")
            });
        encoded.zeroize();
        result
    }
}

impl fmt::Debug for SessionToken {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SessionToken([REDACTED])")
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionExchangeResponse {
    pub schema: String,
    #[serde(with = "strict_uuid_v7")]
    pub session_id: Uuid,
    pub session_token: SessionToken,
    #[serde(with = "strict_uuid_v7")]
    pub workspace_id: Uuid,
    #[serde(with = "strict_uuid_v7")]
    pub server_instance_id: Uuid,
    #[serde(with = "strict_uuid_v7")]
    pub human_id: Uuid,
    pub auth_epoch: u64,
    pub policy_revision: u64,
    pub granted_capabilities: CapabilitySet,
    #[serde(with = "strict_datetime_utc")]
    pub issued_at: DateTime<Utc>,
    #[serde(with = "strict_datetime_utc")]
    pub absolute_expires_at: DateTime<Utc>,
    #[serde(with = "strict_datetime_utc")]
    pub idle_expires_at: DateTime<Utc>,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionClaims {
    pub schema: String,
    #[serde(with = "strict_uuid_v7")]
    pub session_id: Uuid,
    pub token_digest: ControlDigest,
    #[serde(with = "strict_uuid_v7")]
    pub workspace_id: Uuid,
    #[serde(with = "strict_uuid_v7")]
    pub server_instance_id: Uuid,
    #[serde(with = "strict_uuid_v7")]
    pub human_id: Uuid,
    pub auth_epoch: u64,
    pub policy_revision: u64,
    pub origin: String,
    pub granted_capabilities: CapabilitySet,
    #[serde(with = "strict_datetime_utc")]
    pub issued_at: DateTime<Utc>,
    #[serde(with = "strict_datetime_utc")]
    pub absolute_expires_at: DateTime<Utc>,
    pub authority_digest: ControlDigest,
    #[serde(with = "strict_datetime_utc")]
    pub idle_expires_at: DateTime<Utc>,
}

impl fmt::Debug for SessionClaims {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SessionClaims")
            .field("schema", &self.schema)
            .field("session_id", &self.session_id)
            .field("token_digest", &"[REDACTED]")
            .field("workspace_id", &self.workspace_id)
            .field("server_instance_id", &self.server_instance_id)
            .field("human_id", &self.human_id)
            .field("auth_epoch", &self.auth_epoch)
            .field("policy_revision", &self.policy_revision)
            .field("origin", &self.origin)
            .field("granted_capabilities", &self.granted_capabilities)
            .field("issued_at", &self.issued_at)
            .field("absolute_expires_at", &self.absolute_expires_at)
            .field("authority_digest", &self.authority_digest)
            .field("idle_expires_at", &self.idle_expires_at)
            .finish()
    }
}

/// Non-secret immutable scope passed to a protected callback under the auth lock.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthorizedSession {
    pub session_id: Uuid,
    pub workspace_id: Uuid,
    pub server_instance_id: Uuid,
    pub human_id: Uuid,
    pub auth_epoch: u64,
    pub policy_revision: u64,
    pub origin: String,
    pub granted_capabilities: CapabilitySet,
    pub issued_at: DateTime<Utc>,
    pub absolute_expires_at: DateTime<Utc>,
    pub authority_digest: ControlDigest,
}

impl AuthorizedSession {
    pub fn authority_binding(&self) -> proof_kernel::SessionAuthorityBinding {
        proof_kernel::SessionAuthorityBinding {
            schema: proof_kernel::SessionAuthorityBinding::SCHEMA.to_owned(),
            session_id: self.session_id,
            workspace_id: self.workspace_id,
            server_instance_id: self.server_instance_id,
            human_id: self.human_id,
            auth_epoch: self.auth_epoch,
            policy_revision: self.policy_revision,
            origin: self.origin.clone(),
            granted_capabilities: self.granted_capabilities.clone(),
            issued_at: self.issued_at,
            absolute_expires_at: self.absolute_expires_at,
        }
    }
}

pub(crate) fn is_canonical_capability_set(capabilities: &[Capability]) -> bool {
    !capabilities.is_empty()
        && capabilities.len() <= ALL_CAPABILITIES.len()
        && capabilities.windows(2).all(|pair| pair[0] < pair[1])
}

pub(crate) fn is_uuid_v7(id: Uuid) -> bool {
    id.get_version_num() == 7 && id.get_variant() == uuid::Variant::RFC4122
}

pub(crate) fn valid_origin(value: &str) -> bool {
    let Some(port) = value.strip_prefix("http://127.0.0.1:") else {
        return false;
    };
    !port.is_empty()
        && !port.starts_with('0')
        && port.bytes().all(|byte| byte.is_ascii_digit())
        && port.parse::<u16>().is_ok_and(|port| port != 0)
}

pub(crate) fn encode_hex(bytes: &[u8]) -> String {
    let mut output = vec![0_u8; bytes.len() * 2];
    encode_hex_into(bytes, &mut output);
    String::from_utf8(output).expect("hex is valid UTF-8")
}

fn encode_hex_into(bytes: &[u8], output: &mut [u8]) {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    debug_assert_eq!(output.len(), bytes.len() * 2);
    for (index, byte) in bytes.iter().copied().enumerate() {
        output[index * 2] = HEX[usize::from(byte >> 4)];
        output[index * 2 + 1] = HEX[usize::from(byte & 0x0f)];
    }
}

fn decode_hex32(input: &[u8]) -> Option<Zeroizing<[u8; 32]>> {
    if input.len() != 64 {
        return None;
    }
    let mut output = Zeroizing::new([0_u8; 32]);
    for (index, pair) in input.chunks_exact(2).enumerate() {
        output[index] = (decode_hex_nibble(pair[0])? << 4) | decode_hex_nibble(pair[1])?;
    }
    Some(output)
}

fn decode_hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        _ => None,
    }
}

mod strict_uuid_v7 {
    use super::*;

    pub fn serialize<S>(value: &Uuid, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        if !is_uuid_v7(*value) {
            return Err(serde::ser::Error::custom(
                "UUID must be canonical lowercase hyphenated UUIDv7",
            ));
        }
        serializer.serialize_str(&value.hyphenated().to_string())
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Uuid, D::Error>
    where
        D: Deserializer<'de>,
    {
        let encoded = String::deserialize(deserializer)?;
        let value = Uuid::parse_str(&encoded).map_err(D::Error::custom)?;
        if !is_uuid_v7(value) || value.hyphenated().to_string() != encoded {
            return Err(D::Error::custom(
                "UUID must be canonical lowercase hyphenated UUIDv7",
            ));
        }
        Ok(value)
    }
}

mod strict_datetime_utc {
    use super::*;

    pub fn serialize<S>(value: &DateTime<Utc>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let encoded = value.to_rfc3339_opts(SecondsFormat::AutoSi, true);
        if !has_rfc3339_utc_shape(encoded.as_bytes()) {
            return Err(serde::ser::Error::custom(
                "timestamp must be canonical RFC3339 UTC with seconds from 00 through 59",
            ));
        }
        serializer.serialize_str(&encoded)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<DateTime<Utc>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let encoded = String::deserialize(deserializer)?;
        if !has_rfc3339_utc_shape(encoded.as_bytes()) {
            return Err(D::Error::custom(
                "timestamp must be canonical RFC3339 UTC with a trailing Z",
            ));
        }
        DateTime::parse_from_rfc3339(&encoded)
            .map(|value| value.with_timezone(&Utc))
            .map_err(D::Error::custom)
    }

    fn has_rfc3339_utc_shape(bytes: &[u8]) -> bool {
        let fixed_shape = bytes.len() >= 20
            && bytes[4] == b'-'
            && bytes[7] == b'-'
            && bytes[10] == b'T'
            && bytes[13] == b':'
            && bytes[16] == b':'
            && bytes[17].is_ascii_digit()
            && bytes[17] <= b'5'
            && bytes[18].is_ascii_digit()
            && [0..4, 5..7, 8..10, 11..13, 14..16, 17..19]
                .into_iter()
                .flatten()
                .all(|index| bytes[index].is_ascii_digit());
        if !fixed_shape {
            return false;
        }
        match bytes.len() {
            20 => bytes[19] == b'Z',
            22..=30 => {
                bytes[19] == b'.'
                    && bytes[bytes.len() - 1] == b'Z'
                    && bytes[20..bytes.len() - 1].iter().all(u8::is_ascii_digit)
            }
            _ => false,
        }
    }
}
