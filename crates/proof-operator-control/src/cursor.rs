use std::sync::Arc;

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::Duration;
use proof_kernel::{
    canonicalize, canonicalize_serialized, constant_time_eq_32, CapabilitySet, CursorClaims,
    CursorRoute, CursorSort, OperatorControlEnvironment, OperatorCursorCodec, OperatorCursorError,
    OperatorRandomPurpose, OperatorReadRoute, OperatorReadScope, VerifiedPageWindow,
};
use uuid::Uuid;
use zeroize::{Zeroize, Zeroizing};

const CURSOR_DOMAIN: &[u8] = b"Proof-Operator-Cursor-v1";
const MAX_CURSOR_LENGTH: usize = 1536;

/// Process-local cursor codec. Restarting the process destroys its key.
pub struct ProcessCursorCodec {
    environment: Arc<dyn OperatorControlEnvironment>,
    key: Zeroizing<[u8; 32]>,
}

impl ProcessCursorCodec {
    /// Creates a codec from fresh process entropy.
    pub fn new(
        environment: Arc<dyn OperatorControlEnvironment>,
    ) -> Result<Self, OperatorCursorError> {
        let mut key = Zeroizing::new([0_u8; 32]);
        environment
            .fill_random(OperatorRandomPurpose::CursorKey, key.as_mut())
            .map_err(|_| OperatorCursorError::Unavailable)?;
        Ok(Self { environment, key })
    }

    fn mac(&self, payload: &[u8]) -> [u8; 32] {
        let mut hasher = blake3::Hasher::new_keyed(&*self.key);
        hasher.update(CURSOR_DOMAIN);
        hasher.update(&[0]);
        hasher.update(payload);
        hasher.finalize().into()
    }
}

impl Drop for ProcessCursorCodec {
    fn drop(&mut self) {
        self.key.zeroize();
    }
}

impl OperatorCursorCodec for ProcessCursorCodec {
    fn open_page(
        &self,
        scope: OperatorReadScope,
        cursor: Option<&str>,
        page_size: u64,
    ) -> Result<VerifiedPageWindow, OperatorCursorError> {
        scope.validate().map_err(|_| OperatorCursorError::Stale)?;
        if !(1..=100).contains(&page_size) {
            return Err(OperatorCursorError::Stale);
        }
        let Some(cursor) = cursor else {
            return Ok(VerifiedPageWindow::first());
        };
        if cursor.is_empty() || cursor.len() > MAX_CURSOR_LENGTH || cursor.contains('=') {
            return Err(OperatorCursorError::Stale);
        }
        let decoded = Zeroizing::new(
            URL_SAFE_NO_PAD
                .decode(cursor.as_bytes())
                .map_err(|_| OperatorCursorError::Stale)?,
        );
        if URL_SAFE_NO_PAD.encode(decoded.as_slice()) != cursor || decoded.len() <= 32 {
            return Err(OperatorCursorError::Stale);
        }
        let (payload, supplied_mac) = decoded.split_at(decoded.len() - 32);
        let expected_mac = self.mac(payload);
        let supplied_mac: &[u8; 32] = supplied_mac
            .try_into()
            .map_err(|_| OperatorCursorError::Stale)?;
        if !constant_time_eq_32(&expected_mac, supplied_mac) {
            return Err(OperatorCursorError::Stale);
        }
        let payload = std::str::from_utf8(payload).map_err(|_| OperatorCursorError::Stale)?;
        let value: serde_json::Value =
            serde_json::from_str(payload).map_err(|_| OperatorCursorError::Stale)?;
        let canonical = canonicalize(&value).map_err(|_| OperatorCursorError::Stale)?;
        if canonical.as_str() != payload {
            return Err(OperatorCursorError::Stale);
        }
        let claims: CursorClaims =
            serde_json::from_str(payload).map_err(|_| OperatorCursorError::Stale)?;
        let now = self
            .environment
            .trusted_utc_now()
            .map_err(|_| OperatorCursorError::Unavailable)?;
        claims
            .validate_for_scope(&scope, page_size, now)
            .map_err(|_| OperatorCursorError::Stale)?;
        VerifiedPageWindow::continuation(
            claims.high_water_sequence,
            claims.last_sequence,
            claims.last_id,
        )
        .map_err(|_| OperatorCursorError::Stale)
    }

    fn seal_page(
        &self,
        scope: OperatorReadScope,
        page_size: u64,
        high_water_sequence: u64,
        last_sequence: u64,
        last_id: Uuid,
    ) -> Result<String, OperatorCursorError> {
        scope.validate().map_err(|_| OperatorCursorError::Stale)?;
        let route = match scope.route {
            OperatorReadRoute::Approvals => CursorRoute::Approvals,
            OperatorReadRoute::Attention => CursorRoute::Attention,
            OperatorReadRoute::Audit => CursorRoute::Audit,
            OperatorReadRoute::Commands => CursorRoute::Commands,
            _ => return Err(OperatorCursorError::Stale),
        };
        let filter_digest = scope.filter_digest.ok_or(OperatorCursorError::Stale)?;
        let now = self
            .environment
            .trusted_utc_now()
            .map_err(|_| OperatorCursorError::Unavailable)?;
        let expires_at = now
            .checked_add_signed(Duration::seconds(300))
            .map(|deadline| deadline.min(scope.session_absolute_expires_at))
            .filter(|deadline| *deadline > now)
            .ok_or(OperatorCursorError::Stale)?;
        let required_capabilities = CapabilitySet::new(scope.required_capabilities.clone())
            .map_err(|_| OperatorCursorError::Stale)?;
        let claims = CursorClaims {
            schema: CursorClaims::SCHEMA.to_owned(),
            route,
            workspace_id: scope.workspace_id,
            server_instance_id: scope.server_instance_id,
            session_id: scope.session_id,
            human_id: scope.human_id,
            auth_epoch: scope.auth_epoch,
            required_capabilities,
            filter_digest,
            sort: CursorSort::SequenceDescIdDesc,
            page_size,
            high_water_sequence,
            last_sequence,
            last_id,
            issued_at: now,
            expires_at,
        };
        claims
            .validate_for_scope(&scope, page_size, now)
            .map_err(|_| OperatorCursorError::Stale)?;
        let canonical =
            canonicalize_serialized(&claims).map_err(|_| OperatorCursorError::Unavailable)?;
        let mut bytes = Zeroizing::new(canonical.as_bytes().to_vec());
        let mac = self.mac(&bytes);
        bytes.extend_from_slice(&mac);
        let encoded = URL_SAFE_NO_PAD.encode(bytes.as_slice());
        if encoded.len() > MAX_CURSOR_LENGTH {
            return Err(OperatorCursorError::Unavailable);
        }
        Ok(encoded)
    }
}
