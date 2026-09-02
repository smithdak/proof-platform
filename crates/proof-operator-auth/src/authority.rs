//! Linearizable in-memory challenge and session authority.

use std::sync::{Arc, Mutex, MutexGuard};

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use blake3::Hasher;
use chrono::Duration;
use ed25519_dalek::{Signature, Verifier};
use proof_kernel::{
    canonicalize_serialized, AuditEvent, AuditEventKind, AuditOutcome, CapabilitySet,
    ControlAuditAppendRequest, ControlAuditAppendResult, ControlAuthorityEventKind, ControlDigest,
    OperatorAuthorityAuditStore, OperatorControlEnvironment, OperatorRandomPurpose,
    SessionAuthorityBinding,
};
use sha2::{Digest as _, Sha256};
use subtle::ConstantTimeEq;
use uuid::Uuid;
use zeroize::{Zeroize, Zeroizing};

use crate::error::{AuthorizedCallError, OperatorAuthError};
use crate::types::{
    encode_hex, is_canonical_capability_set, is_uuid_v7, AuthPolicy, AuthorizedSession,
    ChallengeIssueRequest, ChallengeIssueResponse, SessionAttestation, SessionChallenge,
    SessionExchangeRequest, SessionExchangeResponse, SessionToken, CHALLENGE_TTL_SECONDS,
    SESSION_ABSOLUTE_TTL_SECONDS, SESSION_IDLE_TTL_SECONDS,
};

const CLIENT_NONCE_DOMAIN: &[u8] = b"Proof-Operator-Client-Nonce-v1";
const PUBLIC_KEY_DOMAIN: &[u8] = b"Proof-Operator-Public-Key-v1";
const CHALLENGE_DOMAIN: &[u8] = b"Proof-Operator-Session-Challenge-v1";
const TOKEN_DOMAIN: &[u8] = b"Proof-Operator-Session-Token-v1";
const AUTHORITY_DOMAIN: &[u8] = b"Proof-Operator-Session-Authority-v1";

pub fn client_nonce_digest(nonce: &[u8; 32]) -> ControlDigest {
    control_digest(CLIENT_NONCE_DOMAIN, nonce)
}

pub fn public_key_fingerprint(public_key: &ed25519_dalek::VerifyingKey) -> ControlDigest {
    control_digest(PUBLIC_KEY_DOMAIN, public_key.as_bytes())
}

pub fn challenge_signing_bytes(challenge: &SessionChallenge) -> Result<Vec<u8>, OperatorAuthError> {
    let canonical =
        canonicalize_serialized(challenge).map_err(|_| OperatorAuthError::InvalidRequest)?;
    let mut bytes = Vec::with_capacity(CHALLENGE_DOMAIN.len() + 1 + canonical.as_str().len());
    bytes.extend_from_slice(CHALLENGE_DOMAIN);
    bytes.push(0);
    bytes.extend_from_slice(canonical.as_str().as_bytes());
    Ok(bytes)
}

pub fn challenge_signed_bytes_digest(
    challenge: &SessionChallenge,
) -> Result<ControlDigest, OperatorAuthError> {
    let bytes = challenge_signing_bytes(challenge)?;
    Ok(ControlDigest::from_bytes(*blake3::hash(&bytes).as_bytes()))
}

pub fn challenge_code(challenge: &SessionChallenge) -> Result<String, OperatorAuthError> {
    let canonical =
        canonicalize_serialized(challenge).map_err(|_| OperatorAuthError::InvalidRequest)?;
    let digest = Sha256::digest(canonical.as_str().as_bytes());
    Ok(encode_hex(&digest[..5]))
}

pub struct OperatorAuthAuthority {
    policy: AuthPolicy,
    environment: Arc<dyn OperatorControlEnvironment>,
    audit: Arc<dyn OperatorAuthorityAuditStore>,
    state: Mutex<AuthState>,
}

impl OperatorAuthAuthority {
    pub fn new(
        policy: AuthPolicy,
        environment: Arc<dyn OperatorControlEnvironment>,
        audit: Arc<dyn OperatorAuthorityAuditStore>,
    ) -> Result<Self, OperatorAuthError> {
        policy.validate()?;
        if public_key_fingerprint(&policy.human_public_key) != policy.human_public_key_fingerprint {
            return Err(OperatorAuthError::InvalidRequest);
        }
        Ok(Self {
            policy,
            environment,
            audit,
            state: Mutex::new(AuthState::default()),
        })
    }

    pub fn issue_challenge(
        &self,
        request: ChallengeIssueRequest,
    ) -> Result<ChallengeIssueResponse, OperatorAuthError> {
        if request.schema != "proof.operator.session.challenge-issue-request/v1"
            || !is_canonical_capability_set(request.requested_capabilities.as_slice())
        {
            return Err(OperatorAuthError::InvalidRequest);
        }

        let granted = intersect_capabilities(
            &request.requested_capabilities,
            &self.policy.enrolled_capabilities,
            &self.policy.workspace_capabilities,
            &self.policy.supported_capabilities,
        )?;
        let mut state = self.lock_state()?;
        let issued_at = match self.environment.trusted_utc_now() {
            Ok(now) => now,
            Err(_) => return Err(clear_unavailable(&mut state)),
        };
        let issued_tick = match self.environment.monotonic_millis() {
            Ok(tick) => tick,
            Err(_) => return Err(clear_unavailable(&mut state)),
        };
        if state.pending.as_ref().is_some_and(|pending| {
            issued_tick >= pending.expiry_tick || issued_at >= pending.challenge.expires_at
        }) {
            state.pending.take();
        }
        if state.pending.is_some() {
            return Err(OperatorAuthError::ChallengePending);
        }
        let challenge_id = match self.environment.new_uuid_v7() {
            Ok(id) if is_uuid_v7(id) => id,
            Ok(_) | Err(_) => return Err(clear_unavailable(&mut state)),
        };
        let mut nonce = Zeroizing::new([0_u8; 32]);
        if self
            .environment
            .fill_random(OperatorRandomPurpose::ChallengeNonce, nonce.as_mut())
            .is_err()
        {
            return Err(clear_unavailable(&mut state));
        }
        let server_nonce = encode_hex(nonce.as_ref());
        nonce.zeroize();
        let Some(expiry_tick) = issued_tick.checked_add(CHALLENGE_TTL_SECONDS * 1000) else {
            return Err(clear_unavailable(&mut state));
        };
        let Some(expires_at) =
            issued_at.checked_add_signed(Duration::seconds(CHALLENGE_TTL_SECONDS as i64))
        else {
            return Err(clear_unavailable(&mut state));
        };

        let challenge = SessionChallenge {
            schema: "proof.operator.session.challenge/v1".to_owned(),
            challenge_id,
            server_instance_id: self.policy.server_instance_id,
            server_nonce,
            workspace_id: self.policy.workspace_id,
            workspace_fingerprint: self.policy.workspace_fingerprint.clone(),
            human_id: self.policy.human_id,
            human_public_key_fingerprint: self.policy.human_public_key_fingerprint.clone(),
            auth_epoch: self.policy.auth_epoch,
            policy_revision: self.policy.policy_revision,
            origin: self.policy.origin.clone(),
            client_nonce_digest: request.client_nonce_digest,
            requested_capabilities: request.requested_capabilities,
            granted_capabilities: granted,
            issued_at,
            expires_at,
            challenge_ttl_seconds: CHALLENGE_TTL_SECONDS,
            session_absolute_ttl_seconds: SESSION_ABSOLUTE_TTL_SECONDS,
            session_idle_ttl_seconds: SESSION_IDLE_TTL_SECONDS,
        };
        let code = challenge_code(&challenge).map_err(|_| clear_unavailable(&mut state))?;
        let challenge_digest =
            challenge_signed_bytes_digest(&challenge).map_err(|_| clear_unavailable(&mut state))?;
        let intent = ControlAuditAppendRequest {
            schema: ControlAuditAppendRequest::SCHEMA.to_owned(),
            kind: ControlAuthorityEventKind::SessionChallengeIssued,
            challenge_id: Some(challenge_id),
            session_id: None,
            related_session_id: None,
            workspace_id: self.policy.workspace_id,
            server_instance_id: self.policy.server_instance_id,
            human_id: Some(self.policy.human_id),
            challenge_digest: Some(challenge_digest),
            session_authority_digest: None,
            auth_epoch: Some(self.policy.auth_epoch),
            policy_revision: Some(self.policy.policy_revision),
        };
        if self.append_audit(intent).is_err() {
            return Err(clear_unavailable(&mut state));
        }
        state.pending = Some(PendingChallenge {
            challenge: challenge.clone(),
            expiry_tick,
            attestation: None,
        });
        Ok(ChallengeIssueResponse {
            schema: "proof.operator.session.challenge-issue-response/v1".to_owned(),
            challenge,
            challenge_code: code,
        })
    }

    pub fn submit_attestation(
        &self,
        attestation: SessionAttestation,
    ) -> Result<(), OperatorAuthError> {
        let mut state = self.lock_state()?;
        let now = match self.environment.trusted_utc_now() {
            Ok(now) => now,
            Err(_) => return Err(clear_unavailable(&mut state)),
        };
        let tick = match self.environment.monotonic_millis() {
            Ok(tick) => tick,
            Err(_) => return Err(clear_unavailable(&mut state)),
        };
        let result = state
            .pending
            .as_ref()
            .ok_or(OperatorAuthError::AuthenticationRequired)
            .and_then(|pending| {
                if pending.attestation.is_some()
                    || tick >= pending.expiry_tick
                    || now >= pending.challenge.expires_at
                    || attestation.schema != "proof.operator.session.attestation/v1"
                    || attestation.signature_algorithm != "ed25519"
                    || attestation.challenge != pending.challenge
                    || !challenge_matches_policy(&attestation.challenge, &self.policy)
                {
                    return Err(OperatorAuthError::AuthenticationRequired);
                }
                let signing_bytes = challenge_signing_bytes(&attestation.challenge)
                    .map_err(|_| OperatorAuthError::AuthenticationRequired)?;
                let expected_digest =
                    ControlDigest::from_bytes(*blake3::hash(&signing_bytes).as_bytes());
                if attestation.signed_bytes_digest != expected_digest {
                    return Err(OperatorAuthError::AuthenticationRequired);
                }
                let signature_bytes = decode_signature(&attestation.signature)
                    .ok_or(OperatorAuthError::AuthenticationRequired)?;
                let signature = Signature::from_bytes(&signature_bytes);
                self.policy
                    .human_public_key
                    .verify(&signing_bytes, &signature)
                    .map_err(|_| OperatorAuthError::AuthenticationRequired)
            });

        if result.is_err() {
            state.pending.take();
            return result;
        }
        if let Some(pending) = state.pending.as_mut() {
            pending.attestation = Some(attestation);
        }
        Ok(())
    }

    pub fn consume_failed_challenge(&self, challenge_id: Uuid) -> Result<(), OperatorAuthError> {
        let mut state = self.lock_state()?;
        if !state
            .pending
            .as_ref()
            .is_some_and(|pending| pending.challenge.challenge_id == challenge_id)
        {
            return Err(OperatorAuthError::AuthenticationRequired);
        }
        state.pending.take();
        Ok(())
    }

    pub fn exchange(
        &self,
        mut request: SessionExchangeRequest,
    ) -> Result<SessionExchangeResponse, OperatorAuthError> {
        let mut state = self.lock_state()?;
        let now = match self.environment.trusted_utc_now() {
            Ok(now) => now,
            Err(_) => return Err(clear_unavailable(&mut state)),
        };
        let tick = match self.environment.monotonic_millis() {
            Ok(tick) => tick,
            Err(_) => return Err(clear_unavailable(&mut state)),
        };
        let (candidate_nonce, canonical_nonce) = decode_hex32(request.client_nonce.as_bytes());
        request.client_nonce.zeroize();
        let candidate_nonce_digest = client_nonce_digest(&candidate_nonce);
        let dummy_nonce_digest = ControlDigest::from_bytes([0_u8; 32]);
        let expected_nonce_digest = state
            .pending
            .as_ref()
            .map_or(&dummy_nonce_digest, |pending| {
                &pending.challenge.client_nonce_digest
            });
        let nonce_matches = bool::from(
            expected_nonce_digest
                .as_bytes()
                .ct_eq(candidate_nonce_digest.as_bytes()),
        );

        let valid = state.pending.as_ref().is_some_and(|pending| {
            request.schema == "proof.operator.session.exchange-request/v1"
                && request.challenge_id == pending.challenge.challenge_id
                && pending.attestation.is_some()
                && tick < pending.expiry_tick
                && now < pending.challenge.expires_at
                && challenge_matches_policy(&pending.challenge, &self.policy)
                && nonce_matches
                && canonical_nonce
        });
        let pending = state.pending.take();
        if !valid {
            return Err(OperatorAuthError::AuthenticationRequired);
        }
        let pending = pending.expect("valid exchange has a pending challenge");

        let session_id = match self.environment.new_uuid_v7() {
            Ok(id) if is_uuid_v7(id) => id,
            Ok(_) | Err(_) => return Err(clear_unavailable(&mut state)),
        };
        let mut token = Zeroizing::new([0_u8; 32]);
        if self
            .environment
            .fill_random(OperatorRandomPurpose::SessionToken, token.as_mut())
            .is_err()
        {
            return Err(clear_unavailable(&mut state));
        }
        let Some(absolute_tick) = tick.checked_add(SESSION_ABSOLUTE_TTL_SECONDS * 1000) else {
            return Err(clear_unavailable(&mut state));
        };
        let Some(idle_tick) = tick.checked_add(SESSION_IDLE_TTL_SECONDS * 1000) else {
            return Err(clear_unavailable(&mut state));
        };
        let Some(absolute_expires_at) =
            now.checked_add_signed(Duration::seconds(SESSION_ABSOLUTE_TTL_SECONDS as i64))
        else {
            return Err(clear_unavailable(&mut state));
        };
        let Some(idle_expires_at) =
            now.checked_add_signed(Duration::seconds(SESSION_IDLE_TTL_SECONDS as i64))
        else {
            return Err(clear_unavailable(&mut state));
        };
        let binding = SessionAuthorityBinding {
            schema: SessionAuthorityBinding::SCHEMA.to_owned(),
            session_id,
            workspace_id: self.policy.workspace_id,
            server_instance_id: self.policy.server_instance_id,
            human_id: self.policy.human_id,
            auth_epoch: self.policy.auth_epoch,
            policy_revision: self.policy.policy_revision,
            origin: self.policy.origin.clone(),
            granted_capabilities: pending.challenge.granted_capabilities.clone(),
            issued_at: now,
            absolute_expires_at,
        };
        let authority_digest = match canonicalize_serialized(&binding) {
            Ok(canonical) => control_digest(AUTHORITY_DOMAIN, canonical.as_str().as_bytes()),
            Err(_) => {
                return Err(clear_unavailable(&mut state));
            }
        };
        let token_digest = secret_digest(TOKEN_DOMAIN, token.as_ref());
        let authorized = AuthorizedSession {
            session_id,
            workspace_id: binding.workspace_id,
            server_instance_id: binding.server_instance_id,
            human_id: binding.human_id,
            auth_epoch: binding.auth_epoch,
            policy_revision: binding.policy_revision,
            origin: binding.origin,
            granted_capabilities: binding.granted_capabilities.clone(),
            issued_at: binding.issued_at,
            absolute_expires_at: binding.absolute_expires_at,
            authority_digest: authority_digest.clone(),
        };
        let replaced_session_id = state
            .session
            .as_ref()
            .map(|active| active.authority.session_id);
        let intent = ControlAuditAppendRequest {
            schema: ControlAuditAppendRequest::SCHEMA.to_owned(),
            kind: if replaced_session_id.is_some() {
                ControlAuthorityEventKind::SessionReplaced
            } else {
                ControlAuthorityEventKind::SessionIssued
            },
            challenge_id: Some(pending.challenge.challenge_id),
            session_id: Some(session_id),
            related_session_id: replaced_session_id,
            workspace_id: self.policy.workspace_id,
            server_instance_id: self.policy.server_instance_id,
            human_id: Some(self.policy.human_id),
            challenge_digest: Some(
                challenge_signed_bytes_digest(&pending.challenge)
                    .map_err(|_| clear_unavailable(&mut state))?,
            ),
            session_authority_digest: Some(authority_digest),
            auth_epoch: Some(self.policy.auth_epoch),
            policy_revision: Some(self.policy.policy_revision),
        };
        if self.append_audit(intent).is_err() {
            return Err(clear_unavailable(&mut state));
        }
        state.session = Some(ActiveSession {
            authority: authorized,
            token_digest,
            absolute_expiry_tick: absolute_tick,
            idle_expiry_tick: idle_tick,
        });

        Ok(SessionExchangeResponse {
            schema: "proof.operator.session.exchange-response/v1".to_owned(),
            session_id,
            session_token: SessionToken::new(token),
            workspace_id: self.policy.workspace_id,
            server_instance_id: self.policy.server_instance_id,
            human_id: self.policy.human_id,
            auth_epoch: self.policy.auth_epoch,
            policy_revision: self.policy.policy_revision,
            granted_capabilities: binding.granted_capabilities,
            issued_at: now,
            absolute_expires_at,
            idle_expires_at,
        })
    }

    pub fn authorize_with<T, E, F>(
        &self,
        header_values: &[&[u8]],
        required: &CapabilitySet,
        callback: F,
    ) -> Result<T, AuthorizedCallError<E>>
    where
        F: FnOnce(&AuthorizedSession) -> Result<T, E>,
    {
        if !is_canonical_capability_set(required.as_slice()) {
            return Err(OperatorAuthError::InvalidRequest.into());
        }
        self.authorize_inner(header_values, Some(required), callback)
    }

    pub fn authorize_any_with<T, E, F>(
        &self,
        header_values: &[&[u8]],
        callback: F,
    ) -> Result<T, AuthorizedCallError<E>>
    where
        F: FnOnce(&AuthorizedSession) -> Result<T, E>,
    {
        self.authorize_inner(header_values, None, callback)
    }

    pub fn revoke_with<T, E, F>(
        &self,
        header_values: &[&[u8]],
        durable_revoke: F,
    ) -> Result<T, AuthorizedCallError<E>>
    where
        F: FnOnce(&AuthorizedSession) -> Result<T, E>,
    {
        let mut state = self.lock_state().map_err(AuthorizedCallError::Auth)?;
        let (authority, _) = self
            .authenticate_locked(&mut state, header_values, None)
            .map_err(AuthorizedCallError::Auth)?;
        match durable_revoke(&authority) {
            Ok(result) => {
                state.session.take();
                Ok(result)
            }
            Err(error) => {
                state.clear();
                Err(AuthorizedCallError::Callback(error))
            }
        }
    }

    pub fn invalidate_for_shutdown(&self) -> Result<(), OperatorAuthError> {
        let mut state = self.lock_state()?;
        let intent = ControlAuditAppendRequest {
            schema: ControlAuditAppendRequest::SCHEMA.to_owned(),
            workspace_id: self.policy.workspace_id,
            server_instance_id: self.policy.server_instance_id,
            kind: ControlAuthorityEventKind::ControlShutdown,
            human_id: None,
            session_id: None,
            challenge_id: None,
            challenge_digest: None,
            session_authority_digest: None,
            related_session_id: None,
            auth_epoch: None,
            policy_revision: None,
        };
        if self.append_audit(intent).is_err() {
            return Err(clear_unavailable(&mut state));
        }
        state.clear();
        Ok(())
    }

    fn authorize_inner<T, E, F>(
        &self,
        header_values: &[&[u8]],
        required: Option<&CapabilitySet>,
        callback: F,
    ) -> Result<T, AuthorizedCallError<E>>
    where
        F: FnOnce(&AuthorizedSession) -> Result<T, E>,
    {
        let mut state = self.lock_state().map_err(AuthorizedCallError::Auth)?;
        let (authority, refreshed_idle_tick) = self
            .authenticate_locked(&mut state, header_values, required)
            .map_err(AuthorizedCallError::Auth)?;
        let result = callback(&authority).map_err(AuthorizedCallError::Callback)?;
        if let Some(active) = state.session.as_mut() {
            if active.authority.session_id == authority.session_id {
                active.idle_expiry_tick = refreshed_idle_tick;
            }
        }
        Ok(result)
    }

    fn authenticate_locked(
        &self,
        state: &mut AuthState,
        header_values: &[&[u8]],
        required: Option<&CapabilitySet>,
    ) -> Result<(AuthorizedSession, u64), OperatorAuthError> {
        let tick = match self.environment.monotonic_millis() {
            Ok(tick) => tick,
            Err(_) => return Err(clear_unavailable(state)),
        };
        if state.session.as_ref().is_some_and(|session| {
            tick >= session.absolute_expiry_tick || tick >= session.idle_expiry_tick
        }) {
            let expired = state.session.take().expect("expired session exists");
            let intent = ControlAuditAppendRequest {
                schema: ControlAuditAppendRequest::SCHEMA.to_owned(),
                kind: ControlAuthorityEventKind::SessionExpired,
                challenge_id: None,
                session_id: Some(expired.authority.session_id),
                related_session_id: None,
                workspace_id: self.policy.workspace_id,
                server_instance_id: self.policy.server_instance_id,
                human_id: Some(self.policy.human_id),
                challenge_digest: None,
                session_authority_digest: Some(expired.authority.authority_digest),
                auth_epoch: Some(self.policy.auth_epoch),
                policy_revision: Some(self.policy.policy_revision),
            };
            if self.append_audit(intent).is_err() {
                return Err(clear_unavailable(state));
            }
        }

        let selected = if header_values.len() == 1 {
            header_values[0]
        } else {
            &[]
        };
        let (candidate, canonical) = decode_hex32(selected);
        let candidate_digest = secret_digest(TOKEN_DOMAIN, candidate.as_ref());
        let dummy = [0_u8; 32];
        let expected = state
            .session
            .as_ref()
            .map_or(&dummy, |session| &*session.token_digest);
        let token_matches = bool::from(expected.ct_eq(&*candidate_digest));

        let Some(active) = state.session.as_ref() else {
            return Err(OperatorAuthError::AuthenticationRequired);
        };
        if !canonical
            || header_values.len() != 1
            || !token_matches
            || active.authority.workspace_id != self.policy.workspace_id
            || active.authority.server_instance_id != self.policy.server_instance_id
            || active.authority.human_id != self.policy.human_id
            || active.authority.auth_epoch != self.policy.auth_epoch
            || active.authority.policy_revision != self.policy.policy_revision
            || active.authority.origin != self.policy.origin
        {
            return Err(OperatorAuthError::AuthenticationRequired);
        }
        if let Some(required) = required {
            if !required
                .iter()
                .all(|capability| active.authority.granted_capabilities.contains(capability))
            {
                return Err(OperatorAuthError::CapabilityRequired);
            }
        }
        let authority = active.authority.clone();
        let absolute_expiry_tick = active.absolute_expiry_tick;
        let refreshed_idle_tick = tick
            .checked_add(SESSION_IDLE_TTL_SECONDS * 1000)
            .map(|deadline| deadline.min(absolute_expiry_tick))
            .ok_or_else(|| clear_unavailable(state))?;
        Ok((authority, refreshed_idle_tick))
    }

    fn lock_state(&self) -> Result<MutexGuard<'_, AuthState>, OperatorAuthError> {
        self.state
            .lock()
            .map_err(|_| OperatorAuthError::ControlUnavailable)
    }

    fn append_audit(&self, request: ControlAuditAppendRequest) -> Result<(), ()> {
        let expected = request.clone();
        let result = self.audit.append_authority_event(request).map_err(|_| ())?;
        if audit_result_matches(&expected, &result) {
            Ok(())
        } else {
            Err(())
        }
    }
}

#[derive(Default)]
struct AuthState {
    pending: Option<PendingChallenge>,
    session: Option<ActiveSession>,
}

impl AuthState {
    fn clear(&mut self) {
        self.pending.take();
        self.session.take();
    }
}

struct PendingChallenge {
    challenge: SessionChallenge,
    expiry_tick: u64,
    attestation: Option<SessionAttestation>,
}

struct ActiveSession {
    authority: AuthorizedSession,
    token_digest: Zeroizing<[u8; 32]>,
    absolute_expiry_tick: u64,
    idle_expiry_tick: u64,
}

fn intersect_capabilities(
    requested: &CapabilitySet,
    enrolled: &CapabilitySet,
    policy: &CapabilitySet,
    supported: &CapabilitySet,
) -> Result<CapabilitySet, OperatorAuthError> {
    let granted = requested
        .iter()
        .filter(|capability| {
            enrolled.contains(*capability)
                && policy.contains(*capability)
                && supported.contains(*capability)
        })
        .collect();
    CapabilitySet::new(granted).map_err(|_| OperatorAuthError::InvalidRequest)
}

fn challenge_matches_policy(challenge: &SessionChallenge, policy: &AuthPolicy) -> bool {
    challenge.schema == "proof.operator.session.challenge/v1"
        && is_uuid_v7(challenge.challenge_id)
        && challenge.server_instance_id == policy.server_instance_id
        && challenge.workspace_id == policy.workspace_id
        && challenge.workspace_fingerprint == policy.workspace_fingerprint
        && challenge.human_id == policy.human_id
        && challenge.human_public_key_fingerprint == policy.human_public_key_fingerprint
        && challenge.auth_epoch == policy.auth_epoch
        && challenge.policy_revision == policy.policy_revision
        && challenge.origin == policy.origin
        && challenge.challenge_ttl_seconds == CHALLENGE_TTL_SECONDS
        && challenge.session_absolute_ttl_seconds == SESSION_ABSOLUTE_TTL_SECONDS
        && challenge.session_idle_ttl_seconds == SESSION_IDLE_TTL_SECONDS
        && is_canonical_capability_set(challenge.requested_capabilities.as_slice())
        && is_canonical_capability_set(challenge.granted_capabilities.as_slice())
}

fn control_digest(domain: &[u8], payload: &[u8]) -> ControlDigest {
    let mut hasher = Hasher::new();
    hasher.update(domain);
    hasher.update(&[0]);
    hasher.update(payload);
    ControlDigest::from_bytes(*hasher.finalize().as_bytes())
}

fn secret_digest(domain: &[u8], payload: &[u8]) -> Zeroizing<[u8; 32]> {
    let mut hasher = Hasher::new();
    hasher.update(domain);
    hasher.update(&[0]);
    hasher.update(payload);
    let mut digest = Zeroizing::new([0_u8; 32]);
    digest.copy_from_slice(hasher.finalize().as_bytes());
    digest
}

fn decode_signature(value: &str) -> Option<[u8; 64]> {
    if value.len() != 86 || value.contains('=') {
        return None;
    }
    let decoded = URL_SAFE_NO_PAD.decode(value.as_bytes()).ok()?;
    let bytes: [u8; 64] = decoded.try_into().ok()?;
    if URL_SAFE_NO_PAD.encode(bytes) != value {
        return None;
    }
    Some(bytes)
}

fn decode_hex32(input: &[u8]) -> (Zeroizing<[u8; 32]>, bool) {
    let mut output = Zeroizing::new([0_u8; 32]);
    let mut valid = input.len() == 64;
    for index in 0..32 {
        let high = input.get(index * 2).copied().unwrap_or(0);
        let low = input.get(index * 2 + 1).copied().unwrap_or(0);
        let (high, high_valid) = decode_nibble(high);
        let (low, low_valid) = decode_nibble(low);
        output[index] = (high << 4) | low;
        valid &= high_valid & low_valid;
    }
    (output, valid)
}

fn decode_nibble(byte: u8) -> (u8, bool) {
    match byte {
        b'0'..=b'9' => (byte - b'0', true),
        b'a'..=b'f' => (byte - b'a' + 10, true),
        _ => (0, false),
    }
}

fn clear_unavailable(state: &mut AuthState) -> OperatorAuthError {
    state.clear();
    OperatorAuthError::ControlUnavailable
}

fn audit_result_matches(
    request: &ControlAuditAppendRequest,
    result: &ControlAuditAppendResult,
) -> bool {
    let event = &result.event;
    result.schema == ControlAuditAppendResult::SCHEMA
        && event.schema == AuditEvent::SCHEMA
        && event.workspace_id == request.workspace_id
        && event.kind == audit_event_kind(request.kind)
        && event.outcome
            == if request.kind == ControlAuthorityEventKind::SessionExpired {
                AuditOutcome::Expired
            } else {
                AuditOutcome::Accepted
            }
        && event.human_id == request.human_id
        && event.session_id == request.session_id
        && event.challenge_id == request.challenge_id
        && event.challenge_digest == request.challenge_digest
        && event.session_authority_digest == request.session_authority_digest
        && event.related_session_id == request.related_session_id
        && event.server_instance_id == Some(request.server_instance_id)
        && event.auth_epoch == request.auth_epoch
        && event.policy_revision == request.policy_revision
}

fn audit_event_kind(kind: ControlAuthorityEventKind) -> AuditEventKind {
    match kind {
        ControlAuthorityEventKind::ControlShutdown => AuditEventKind::ControlShutdown,
        ControlAuthorityEventKind::SessionChallengeIssued => AuditEventKind::SessionChallengeIssued,
        ControlAuthorityEventKind::SessionExpired => AuditEventKind::SessionExpired,
        ControlAuthorityEventKind::SessionIssued => AuditEventKind::SessionIssued,
        ControlAuthorityEventKind::SessionReplaced => AuditEventKind::SessionReplaced,
    }
}
