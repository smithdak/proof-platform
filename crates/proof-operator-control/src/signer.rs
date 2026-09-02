use std::{fs::File, io::Read, os::unix::fs::MetadataExt};

use base64::{engine::general_purpose, Engine as _};
use ed25519_dalek::{Signer as _, SigningKey, Verifier as _, VerifyingKey};
use proof_kernel::{DescriptorIdentity, PrincipalBinding, PrincipalKind};
use proof_operator_auth::{
    challenge_signed_bytes_digest, challenge_signing_bytes, SessionAttestation, SessionChallenge,
};
use rustix::fs::{openat, Mode, OFlags};
use serde::Deserialize;
use zeroize::{Zeroize, Zeroizing};

use crate::{ChallengeSigner, TerminalCeremonyError};

const MAX_KEY_FILE_BYTES: u64 = 4096;

/// Descriptor-backed Human signer. It retains only the already-validated
/// approvers directory and public identity; private bytes are opened,
/// revalidated, used, and zeroized for each individual ceremony.
pub struct DescriptorHumanChallengeSigner {
    approvers_directory: File,
    expected_file: DescriptorIdentity,
    human: PrincipalBinding,
}

impl DescriptorHumanChallengeSigner {
    pub fn new(
        approvers_directory: File,
        expected_file: DescriptorIdentity,
        human: PrincipalBinding,
    ) -> Result<Self, TerminalCeremonyError> {
        validate_directory(&approvers_directory)?;
        human
            .validate()
            .map_err(|_| TerminalCeremonyError::SigningFailed)?;
        if human.kind != PrincipalKind::Human {
            return Err(TerminalCeremonyError::SigningFailed);
        }
        Ok(Self {
            approvers_directory,
            expected_file,
            human,
        })
    }

    fn load_key(&self) -> Result<SigningKey, TerminalCeremonyError> {
        validate_directory(&self.approvers_directory)?;
        let filename = format!("{}.json", self.human.principal_id.as_uuid());
        let fd = openat(
            &self.approvers_directory,
            filename,
            OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
            Mode::empty(),
        )
        .map_err(|_| TerminalCeremonyError::SigningFailed)?;
        let mut file = File::from(fd);
        validate_key_file(&file, self.expected_file)?;

        let length = file
            .metadata()
            .map_err(|_| TerminalCeremonyError::SigningFailed)?
            .len();
        let capacity = usize::try_from(length)
            .ok()
            .filter(|length| *length <= MAX_KEY_FILE_BYTES as usize)
            .ok_or(TerminalCeremonyError::SigningFailed)?;
        let mut bytes = Zeroizing::new(Vec::with_capacity(capacity));
        file.read_to_end(bytes.as_mut())
            .map_err(|_| TerminalCeremonyError::SigningFailed)?;
        if bytes.len() as u64 != length {
            return Err(TerminalCeremonyError::SigningFailed);
        }
        let stored: StoredHumanKey = serde_json::from_slice(bytes.as_slice())
            .map_err(|_| TerminalCeremonyError::SigningFailed)?;
        validate_stored_key(&stored, &self.human)
    }
}

impl ChallengeSigner for DescriptorHumanChallengeSigner {
    fn sign_challenge(
        &self,
        challenge: &SessionChallenge,
    ) -> Result<SessionAttestation, TerminalCeremonyError> {
        if challenge.human_id != self.human.principal_id.as_uuid()
            || challenge.human_public_key_fingerprint != self.human.public_key_fingerprint
        {
            return Err(TerminalCeremonyError::SigningFailed);
        }
        let signing_bytes =
            challenge_signing_bytes(challenge).map_err(|_| TerminalCeremonyError::SigningFailed)?;
        let signing_key = self.load_key()?;
        let signature = signing_key.sign(&signing_bytes);
        signing_key
            .verifying_key()
            .verify(&signing_bytes, &signature)
            .map_err(|_| TerminalCeremonyError::SigningFailed)?;
        Ok(SessionAttestation {
            schema: "proof.operator.session.attestation/v1".to_owned(),
            challenge: challenge.clone(),
            signature_algorithm: "ed25519".to_owned(),
            signature: general_purpose::URL_SAFE_NO_PAD.encode(signature.to_bytes()),
            signed_bytes_digest: challenge_signed_bytes_digest(challenge)
                .map_err(|_| TerminalCeremonyError::SigningFailed)?,
        })
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredHumanKey {
    principal_id: uuid::Uuid,
    kind: PrincipalKind,
    #[serde(rename = "created_at")]
    _created_at: chrono::DateTime<chrono::Utc>,
    public_key: [u8; 32],
    signing_key: String,
}

impl Drop for StoredHumanKey {
    fn drop(&mut self) {
        self.public_key.zeroize();
        self.signing_key.zeroize();
    }
}

fn validate_stored_key(
    stored: &StoredHumanKey,
    human: &PrincipalBinding,
) -> Result<SigningKey, TerminalCeremonyError> {
    let public_key = decode_public_key(&human.public_key)?;
    if stored.principal_id != human.principal_id.as_uuid()
        || stored.kind != PrincipalKind::Human
        || stored.public_key != public_key
    {
        return Err(TerminalCeremonyError::SigningFailed);
    }
    let decoded = Zeroizing::new(
        general_purpose::STANDARD
            .decode(stored.signing_key.as_bytes())
            .map_err(|_| TerminalCeremonyError::SigningFailed)?,
    );
    if general_purpose::STANDARD.encode(decoded.as_slice()) != stored.signing_key {
        return Err(TerminalCeremonyError::SigningFailed);
    }
    let mut secret = Zeroizing::new(
        <[u8; 32]>::try_from(decoded.as_slice())
            .map_err(|_| TerminalCeremonyError::SigningFailed)?,
    );
    let signing_key = SigningKey::from_bytes(&*secret);
    secret.zeroize();
    if signing_key.verifying_key().as_bytes() != &public_key {
        return Err(TerminalCeremonyError::SigningFailed);
    }
    Ok(signing_key)
}

fn decode_public_key(value: &str) -> Result<[u8; 32], TerminalCeremonyError> {
    let decoded = general_purpose::URL_SAFE_NO_PAD
        .decode(value.as_bytes())
        .map_err(|_| TerminalCeremonyError::SigningFailed)?;
    if general_purpose::URL_SAFE_NO_PAD.encode(&decoded) != value {
        return Err(TerminalCeremonyError::SigningFailed);
    }
    decoded
        .try_into()
        .map_err(|_| TerminalCeremonyError::SigningFailed)
}

fn validate_directory(directory: &File) -> Result<(), TerminalCeremonyError> {
    let metadata = directory
        .metadata()
        .map_err(|_| TerminalCeremonyError::SigningFailed)?;
    if !metadata.is_dir()
        || metadata.uid() != rustix::process::geteuid().as_raw()
        || metadata.mode() & 0o777 != 0o700
    {
        return Err(TerminalCeremonyError::SigningFailed);
    }
    Ok(())
}

fn validate_key_file(
    file: &File,
    expected: DescriptorIdentity,
) -> Result<(), TerminalCeremonyError> {
    let metadata = file
        .metadata()
        .map_err(|_| TerminalCeremonyError::SigningFailed)?;
    if !metadata.is_file()
        || metadata.uid() != rustix::process::geteuid().as_raw()
        || metadata.mode() & 0o777 != 0o600
        || metadata.nlink() != 1
        || metadata.len() == 0
        || metadata.len() > MAX_KEY_FILE_BYTES
        || metadata.dev() != expected.device
        || metadata.ino() != expected.inode
    {
        return Err(TerminalCeremonyError::SigningFailed);
    }
    Ok(())
}
