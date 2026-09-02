use std::sync::Mutex;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OperatorRandomPurpose {
    ChallengeNonce,
    SessionToken,
    CursorKey,
    LeaseToken,
    DispatchToken,
    UuidEntropy,
}

#[derive(Debug, Clone, Copy, Error, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperatorEnvironmentError {
    #[error("clock unavailable")]
    ClockUnavailable,
    #[error("entropy unavailable")]
    EntropyUnavailable,
}

pub trait OperatorControlEnvironment: Send + Sync {
    fn trusted_utc_now(&self) -> Result<DateTime<Utc>, OperatorEnvironmentError>;
    fn monotonic_millis(&self) -> Result<u64, OperatorEnvironmentError>;
    fn fill_random(
        &self,
        purpose: OperatorRandomPurpose,
        output: &mut [u8],
    ) -> Result<(), OperatorEnvironmentError>;
    fn new_uuid_v7(&self) -> Result<Uuid, OperatorEnvironmentError>;
}

#[derive(Debug)]
struct RecordingState {
    utc: DateTime<Utc>,
    monotonic: u64,
    stream: u64,
    calls: Vec<OperatorRandomPurpose>,
}

/// Deterministic environment for kernel, storage, auth, and runtime tests.
#[derive(Debug)]
pub struct RecordingOperatorControlEnvironment {
    state: Mutex<RecordingState>,
    seed: [u8; 32],
}

impl RecordingOperatorControlEnvironment {
    pub fn new(utc: DateTime<Utc>, seed: [u8; 32]) -> Self {
        Self {
            state: Mutex::new(RecordingState {
                utc,
                monotonic: 0,
                stream: 0,
                calls: Vec::new(),
            }),
            seed,
        }
    }
    pub fn set_utc(&self, utc: DateTime<Utc>) {
        self.state.lock().expect("environment lock poisoned").utc = utc;
    }
    pub fn advance_monotonic(&self, millis: u64) -> Result<(), OperatorEnvironmentError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| OperatorEnvironmentError::ClockUnavailable)?;
        state.monotonic = state
            .monotonic
            .checked_add(millis)
            .ok_or(OperatorEnvironmentError::ClockUnavailable)?;
        Ok(())
    }
    pub fn random_calls(&self) -> Vec<OperatorRandomPurpose> {
        self.state
            .lock()
            .expect("environment lock poisoned")
            .calls
            .clone()
    }
}

impl OperatorControlEnvironment for RecordingOperatorControlEnvironment {
    fn trusted_utc_now(&self) -> Result<DateTime<Utc>, OperatorEnvironmentError> {
        Ok(self
            .state
            .lock()
            .map_err(|_| OperatorEnvironmentError::ClockUnavailable)?
            .utc)
    }
    fn monotonic_millis(&self) -> Result<u64, OperatorEnvironmentError> {
        Ok(self
            .state
            .lock()
            .map_err(|_| OperatorEnvironmentError::ClockUnavailable)?
            .monotonic)
    }
    fn fill_random(
        &self,
        purpose: OperatorRandomPurpose,
        output: &mut [u8],
    ) -> Result<(), OperatorEnvironmentError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| OperatorEnvironmentError::EntropyUnavailable)?;
        state.calls.push(purpose);
        let mut offset = 0;
        while offset < output.len() {
            let mut hasher = sha2::Sha256::new();
            use sha2::Digest as _;
            hasher.update(b"Proof-Operator-Recording-Entropy-v1");
            hasher.update([0]);
            hasher.update(self.seed);
            hasher.update([purpose as u8]);
            hasher.update(state.stream.to_be_bytes());
            state.stream = state
                .stream
                .checked_add(1)
                .ok_or(OperatorEnvironmentError::EntropyUnavailable)?;
            let block = hasher.finalize();
            let count = (output.len() - offset).min(block.len());
            output[offset..offset + count].copy_from_slice(&block[..count]);
            offset += count;
        }
        Ok(())
    }
    fn new_uuid_v7(&self) -> Result<Uuid, OperatorEnvironmentError> {
        let now = self.trusted_utc_now()?;
        let millis = now.timestamp_millis();
        if millis < 0 {
            return Err(OperatorEnvironmentError::ClockUnavailable);
        }
        let mut bytes = [0_u8; 16];
        bytes[..6].copy_from_slice(&(millis as u64).to_be_bytes()[2..]);
        self.fill_random(OperatorRandomPurpose::UuidEntropy, &mut bytes[6..])?;
        bytes[6] = (bytes[6] & 0x0f) | 0x70;
        bytes[8] = (bytes[8] & 0x3f) | 0x80;
        Ok(Uuid::from_bytes(bytes))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fake_is_deterministic_and_purpose_tagged() {
        let now = "2032-01-01T00:00:00Z".parse().unwrap();
        let left = RecordingOperatorControlEnvironment::new(now, [7; 32]);
        let right = RecordingOperatorControlEnvironment::new(now, [7; 32]);
        assert_eq!(left.new_uuid_v7().unwrap(), right.new_uuid_v7().unwrap());
        assert_eq!(
            left.random_calls(),
            vec![OperatorRandomPurpose::UuidEntropy]
        );
        left.advance_monotonic(10).unwrap();
        assert_eq!(left.monotonic_millis(), Ok(10));
    }
}
