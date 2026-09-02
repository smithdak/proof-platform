use std::fs;
#[cfg(unix)]
use std::os::unix::fs::MetadataExt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Barrier, Mutex};
use std::thread;

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::{DateTime, Duration, TimeZone, Timelike, Utc};
use ed25519_dalek::{Signer, SigningKey};
use proof_kernel::{
    control_digest_serialized, AuditEvent, AuditEventKind, AuditOutcome, Capability, CapabilitySet,
    ControlAuditAppendRequest, ControlAuditAppendResult, ControlAuthorityEventKind, ControlDigest,
    DescriptorIdentity, HumanEnrollment, OperatorAuthorityAuditStore, OperatorControlEnvironment,
    OperatorEnvironmentError, OperatorRandomPurpose, OperatorStoreError, OperatorWorkspace,
    PrincipalBinding, PrincipalId, PrincipalKind, WorkspaceFingerprintInput,
};
use rand::{rngs::StdRng, SeedableRng};
use uuid::Uuid;

use crate::{
    challenge_code, challenge_signed_bytes_digest, challenge_signing_bytes, client_nonce_digest,
    public_key_fingerprint, AuthPolicy, AuthorizedCallError, ChallengeIssueRequest,
    OperatorAuthAuthority, OperatorAuthError, SessionAttestation, SessionExchangeRequest,
    ALL_CAPABILITIES,
};

static WORKSPACE_SEQUENCE: AtomicU64 = AtomicU64::new(1);

struct DisposableWorkspace {
    sequence: u64,
    root: PathBuf,
    proof_directory: PathBuf,
    control_lock: PathBuf,
    agent_key_file: PathBuf,
    human_key_file: PathBuf,
}

impl DisposableWorkspace {
    fn new() -> Self {
        let sequence = WORKSPACE_SEQUENCE
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                current.checked_mul(4).and_then(|_| current.checked_add(1))
            })
            .expect("disposable workspace sequence must not overflow");
        let path = std::env::temp_dir().join(format!(
            "proof-operator-auth-test-{}-{sequence}",
            std::process::id()
        ));
        let proof_directory = path.join(".proof");
        fs::create_dir(&path).expect("create disposable test workspace");
        fs::create_dir(&proof_directory).expect("create disposable trusted .proof directory");
        let control_lock = proof_directory.join("operator.lock");
        let agent_key_file = proof_directory.join("agent.identity");
        let human_key_file = proof_directory.join("human.identity");
        for file in [&control_lock, &agent_key_file, &human_key_file] {
            fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(file)
                .expect("create descriptor-only trusted workspace fixture");
        }
        Self {
            sequence,
            root: path,
            proof_directory,
            control_lock,
            agent_key_file,
            human_key_file,
        }
    }

    fn fixture_uuid(&self, slot: u64) -> Uuid {
        assert!((1..=4).contains(&slot));
        let sequence_index = self
            .sequence
            .checked_sub(1)
            .expect("fixture sequence starts at one");
        let value = sequence_index
            .checked_mul(4)
            .and_then(|base| base.checked_add(slot))
            .expect("fixture UUID sequence must not overflow");
        uuid_v7(value)
    }

    #[cfg(unix)]
    fn descriptor(path: &std::path::Path) -> DescriptorIdentity {
        let metadata = fs::metadata(path).expect("read disposable workspace descriptor identity");
        DescriptorIdentity {
            device: metadata.dev(),
            inode: metadata.ino(),
        }
    }

    #[cfg(unix)]
    fn documents(
        &self,
        human_key: &SigningKey,
        agent_key: &SigningKey,
        capabilities: CapabilitySet,
    ) -> (OperatorWorkspace, HumanEnrollment) {
        let human_id = self.fixture_uuid(3);
        let agent_id = self.fixture_uuid(4);
        let human = PrincipalBinding {
            principal_id: PrincipalId::new(human_id),
            kind: PrincipalKind::Human,
            public_key: URL_SAFE_NO_PAD.encode(human_key.verifying_key().as_bytes()),
            public_key_fingerprint: public_key_fingerprint(&human_key.verifying_key()),
        };
        let agent = PrincipalBinding {
            principal_id: PrincipalId::new(agent_id),
            kind: PrincipalKind::Agent,
            public_key: URL_SAFE_NO_PAD.encode(agent_key.verifying_key().as_bytes()),
            public_key_fingerprint: public_key_fingerprint(&agent_key.verifying_key()),
        };
        let fingerprint_input = WorkspaceFingerprintInput {
            schema: WorkspaceFingerprintInput::SCHEMA.to_owned(),
            workspace_id: self.fixture_uuid(1),
            proof_directory: Self::descriptor(&self.proof_directory),
            control_lock: Self::descriptor(&self.control_lock),
            agent_key_file: Self::descriptor(&self.agent_key_file),
            human_key_file: Self::descriptor(&self.human_key_file),
            agent_id,
            human_id,
            agent_public_key: agent.public_key.clone(),
            human_public_key: human.public_key.clone(),
        };
        let workspace_fingerprint =
            control_digest_serialized("Proof-Operator-Workspace-v1", &fingerprint_input).unwrap();
        let now = Utc.with_ymd_and_hms(2035, 1, 2, 3, 4, 5).unwrap();
        let workspace = OperatorWorkspace {
            schema: OperatorWorkspace::SCHEMA.to_owned(),
            workspace_id: fingerprint_input.workspace_id,
            database_name: "operator.db".to_owned(),
            fingerprint_input,
            workspace_fingerprint,
            schema_catalog_digest: ControlDigest::from_bytes([0x92; 32]),
            agent,
            human: human.clone(),
            auth_epoch: 1,
            policy_revision: 1,
            capabilities: capabilities.clone(),
            created_at: now,
            updated_at: now,
            binding_digest: ControlDigest::from_bytes([0x93; 32]),
        };
        let enrollment = HumanEnrollment {
            schema: HumanEnrollment::SCHEMA.to_owned(),
            workspace_id: workspace.workspace_id,
            human,
            capabilities: capabilities.clone(),
            capability_set_digest: control_digest_serialized(
                "Proof-Operator-Capability-Set-v1",
                &capabilities,
            )
            .unwrap(),
            enrolled_at: now,
        };
        (workspace, enrollment)
    }
}

impl Drop for DisposableWorkspace {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).expect("remove disposable test workspace");
    }
}

struct FakeEnvironment {
    utc: Mutex<DateTime<Utc>>,
    monotonic: AtomicU64,
    entropy_counter: AtomicU64,
    uuid_counter: AtomicU64,
    fail_clock: AtomicBool,
    fail_entropy: AtomicBool,
    invalid_uuid_variant: AtomicBool,
}

impl FakeEnvironment {
    fn new() -> Self {
        Self {
            utc: Mutex::new(Utc.with_ymd_and_hms(2035, 1, 2, 3, 4, 5).unwrap()),
            monotonic: AtomicU64::new(10_000),
            entropy_counter: AtomicU64::new(1),
            uuid_counter: AtomicU64::new(10),
            fail_clock: AtomicBool::new(false),
            fail_entropy: AtomicBool::new(false),
            invalid_uuid_variant: AtomicBool::new(false),
        }
    }

    fn advance(&self, seconds: u64) {
        self.monotonic.fetch_add(seconds * 1000, Ordering::SeqCst);
        let mut utc = self.utc.lock().unwrap();
        *utc = utc
            .checked_add_signed(Duration::seconds(seconds as i64))
            .unwrap();
    }
}

impl OperatorControlEnvironment for FakeEnvironment {
    fn trusted_utc_now(&self) -> Result<DateTime<Utc>, OperatorEnvironmentError> {
        if self.fail_clock.load(Ordering::SeqCst) {
            return Err(OperatorEnvironmentError::ClockUnavailable);
        }
        Ok(*self.utc.lock().unwrap())
    }

    fn monotonic_millis(&self) -> Result<u64, OperatorEnvironmentError> {
        if self.fail_clock.load(Ordering::SeqCst) {
            return Err(OperatorEnvironmentError::ClockUnavailable);
        }
        Ok(self.monotonic.load(Ordering::SeqCst))
    }

    fn fill_random(
        &self,
        purpose: OperatorRandomPurpose,
        output: &mut [u8],
    ) -> Result<(), OperatorEnvironmentError> {
        if self.fail_entropy.load(Ordering::SeqCst) {
            return Err(OperatorEnvironmentError::EntropyUnavailable);
        }
        let purpose_byte = match purpose {
            OperatorRandomPurpose::ChallengeNonce => 0x11,
            OperatorRandomPurpose::SessionToken => 0x22,
            OperatorRandomPurpose::CursorKey => 0x33,
            OperatorRandomPurpose::LeaseToken => 0x44,
            OperatorRandomPurpose::DispatchToken => 0x55,
            OperatorRandomPurpose::UuidEntropy => 0x66,
        };
        let sequence = self.entropy_counter.fetch_add(1, Ordering::SeqCst);
        for (index, byte) in output.iter_mut().enumerate() {
            *byte = purpose_byte ^ (sequence as u8) ^ (index as u8);
        }
        Ok(())
    }

    fn new_uuid_v7(&self) -> Result<Uuid, OperatorEnvironmentError> {
        if self.fail_entropy.load(Ordering::SeqCst) {
            return Err(OperatorEnvironmentError::EntropyUnavailable);
        }
        let id = uuid_v7(self.uuid_counter.fetch_add(1, Ordering::SeqCst));
        if self.invalid_uuid_variant.load(Ordering::SeqCst) {
            let mut bytes = *id.as_bytes();
            bytes[8] &= 0x3f;
            return Ok(Uuid::from_bytes(bytes));
        }
        Ok(id)
    }
}

#[derive(Default)]
struct RecordingAudit {
    intents: Mutex<Vec<ControlAuditAppendRequest>>,
    fail: AtomicBool,
    corrupt: AtomicBool,
}

impl OperatorAuthorityAuditStore for RecordingAudit {
    fn append_authority_event(
        &self,
        intent: ControlAuditAppendRequest,
    ) -> Result<ControlAuditAppendResult, OperatorStoreError> {
        if self.fail.load(Ordering::SeqCst) {
            return Err(OperatorStoreError::Unavailable);
        }
        let sequence = self.intents.lock().unwrap().len() as u64 + 1;
        let mut event = AuditEvent {
            schema: AuditEvent::SCHEMA.to_owned(),
            workspace_id: intent.workspace_id,
            event_id: uuid_v7(10_000 + sequence),
            sequence,
            kind: match intent.kind {
                ControlAuthorityEventKind::ControlShutdown => AuditEventKind::ControlShutdown,
                ControlAuthorityEventKind::SessionChallengeIssued => {
                    AuditEventKind::SessionChallengeIssued
                }
                ControlAuthorityEventKind::SessionExpired => AuditEventKind::SessionExpired,
                ControlAuthorityEventKind::SessionIssued => AuditEventKind::SessionIssued,
                ControlAuthorityEventKind::SessionReplaced => AuditEventKind::SessionReplaced,
            },
            outcome: if intent.kind == ControlAuthorityEventKind::SessionExpired {
                AuditOutcome::Expired
            } else {
                AuditOutcome::Accepted
            },
            previous_digest: None,
            event_digest: ControlDigest::from_bytes([sequence as u8; 32]),
            human_id: intent.human_id,
            session_id: intent.session_id,
            challenge_id: intent.challenge_id,
            challenge_digest: intent.challenge_digest,
            session_authority_digest: intent.session_authority_digest,
            related_session_id: intent.related_session_id,
            server_instance_id: Some(intent.server_instance_id),
            run_id: None,
            approval_request_id: None,
            command_id: None,
            command_kind: None,
            budget_id: None,
            reservation_id: None,
            lease_id: None,
            source_lease_id: None,
            process_epoch_id: None,
            permit_id: None,
            recovery_directive_id: None,
            fence_epoch: None,
            auth_epoch: intent.auth_epoch,
            policy_revision: intent.policy_revision,
            intent_digest: None,
            call_digest: None,
            decision_digest: None,
            recovery_directive_digest: None,
            failure_scope: None,
            proof: None,
            occurred_at: Utc.with_ymd_and_hms(2035, 1, 2, 3, 4, 5).unwrap(),
        };
        if self.corrupt.load(Ordering::SeqCst) {
            event.workspace_id = different_uuid(intent.workspace_id);
        }
        self.intents.lock().unwrap().push(intent);
        Ok(ControlAuditAppendResult {
            schema: ControlAuditAppendResult::SCHEMA.to_owned(),
            event,
        })
    }
}

struct Harness {
    _workspace: DisposableWorkspace,
    signing_key: SigningKey,
    workspace_id: Uuid,
    workspace_fingerprint: ControlDigest,
    server_instance_id: Uuid,
    human_id: Uuid,
    environment: Arc<FakeEnvironment>,
    audit: Arc<RecordingAudit>,
    authority: Arc<OperatorAuthAuthority>,
}

impl Harness {
    fn new(capabilities: CapabilitySet) -> Self {
        let workspace = DisposableWorkspace::new();
        let mut rng = StdRng::seed_from_u64(workspace.sequence);
        let signing_key = SigningKey::generate(&mut rng);
        let agent_key = SigningKey::generate(&mut rng);
        let environment = Arc::new(FakeEnvironment::new());
        let audit = Arc::new(RecordingAudit::default());
        let (workspace_document, enrollment) =
            workspace.documents(&signing_key, &agent_key, capabilities);
        let workspace_id = workspace_document.workspace_id;
        let workspace_fingerprint = workspace_document.workspace_fingerprint;
        let server_instance_id = workspace.fixture_uuid(2);
        let human_id = workspace_document.human.principal_id.as_uuid();
        let policy = AuthPolicy::from_workspace(
            &workspace_document,
            &enrollment,
            server_instance_id,
            signing_key.verifying_key(),
            "http://127.0.0.1:43121".to_owned(),
        )
        .unwrap();
        let authority = Arc::new(
            OperatorAuthAuthority::new(policy, environment.clone(), audit.clone()).unwrap(),
        );
        Self {
            _workspace: workspace,
            signing_key,
            workspace_id,
            workspace_fingerprint,
            server_instance_id,
            human_id,
            environment,
            audit,
            authority,
        }
    }

    fn issue(
        &self,
        requested: CapabilitySet,
    ) -> (crate::ChallengeIssueResponse, zeroize::Zeroizing<[u8; 32]>) {
        let mut nonce = zeroize::Zeroizing::new([0_u8; 32]);
        for (index, byte) in nonce.iter_mut().enumerate() {
            *byte = 0xa0 ^ index as u8;
        }
        let response = self
            .authority
            .issue_challenge(ChallengeIssueRequest {
                schema: "proof.operator.session.challenge-issue-request/v1".to_owned(),
                client_nonce_digest: client_nonce_digest(&nonce),
                requested_capabilities: requested,
            })
            .unwrap();
        (response, nonce)
    }

    fn attest(&self, challenge: &crate::SessionChallenge) -> SessionAttestation {
        let bytes = challenge_signing_bytes(challenge).unwrap();
        let signature = self.signing_key.sign(&bytes);
        SessionAttestation {
            schema: "proof.operator.session.attestation/v1".to_owned(),
            challenge: challenge.clone(),
            signature_algorithm: "ed25519".to_owned(),
            signature: URL_SAFE_NO_PAD.encode(signature.to_bytes()),
            signed_bytes_digest: challenge_signed_bytes_digest(challenge).unwrap(),
        }
    }

    fn establish(&self, requested: CapabilitySet) -> crate::SessionHeaderValue {
        let (issued, nonce) = self.issue(requested);
        self.authority
            .submit_attestation(self.attest(&issued.challenge))
            .unwrap();
        let response = self
            .authority
            .exchange(SessionExchangeRequest {
                schema: "proof.operator.session.exchange-request/v1".to_owned(),
                challenge_id: issued.challenge.challenge_id,
                client_nonce: crate::types::encode_hex(nonce.as_ref().as_ref()),
            })
            .unwrap();
        response.session_token.header_value()
    }
}

#[test]
fn public_helpers_bind_exact_domains_and_redact_token_debug() {
    let harness = Harness::new(CapabilitySet::all());
    assert_eq!(ALL_CAPABILITIES, Capability::ALL);
    let (issued, nonce) = harness.issue(CapabilitySet::all());
    assert_eq!(
        issued.challenge_code,
        challenge_code(&issued.challenge).unwrap()
    );
    assert_eq!(issued.challenge_code.len(), 10);
    assert_ne!(
        client_nonce_digest(&nonce),
        public_key_fingerprint(&harness.signing_key.verifying_key())
    );
    let bytes = challenge_signing_bytes(&issued.challenge).unwrap();
    assert!(bytes.starts_with(b"Proof-Operator-Session-Challenge-v1\0"));
    assert_eq!(
        challenge_signed_bytes_digest(&issued.challenge).unwrap(),
        ControlDigest::from_bytes(*blake3::hash(&bytes).as_bytes())
    );
}

#[test]
fn request_dtos_reject_unknown_duplicate_and_unordered_fields() {
    let digest = client_nonce_digest(&[7; 32]).encoded();
    let unknown = format!(
        r#"{{"schema":"proof.operator.session.challenge-issue-request/v1","client_nonce_digest":"{digest}","requested_capabilities":["run.read"],"extra":true}}"#
    );
    assert!(serde_json::from_str::<ChallengeIssueRequest>(&unknown).is_err());
    let duplicate = format!(
        r#"{{"schema":"proof.operator.session.challenge-issue-request/v1","client_nonce_digest":"{digest}","client_nonce_digest":"{digest}","requested_capabilities":["run.read"]}}"#
    );
    assert!(serde_json::from_str::<ChallengeIssueRequest>(&duplicate).is_err());
    let unordered = format!(
        r#"{{"schema":"proof.operator.session.challenge-issue-request/v1","client_nonce_digest":"{digest}","requested_capabilities":["run.read","approval.read"]}}"#
    );
    assert!(serde_json::from_str::<ChallengeIssueRequest>(&unordered).is_err());

    let request = SessionExchangeRequest {
        schema: "proof.operator.session.exchange-request/v1".to_owned(),
        challenge_id: uuid_v7(1),
        client_nonce: crate::types::encode_hex(&[8; 32]),
    };
    assert!(format!("{request:?}").contains("[REDACTED]"));
    assert!(!format!("{request:?}").contains(&crate::types::encode_hex(&[8; 32])));
}

#[test]
fn inbound_uuid_v7_fields_reject_every_noncanonical_spelling_and_version() {
    let harness = Harness::new(CapabilitySet::all());
    let (issued, _) = harness.issue(CapabilitySet::all());
    let canonical = "01890f47-7f6c-7cc2-98a2-4a97c1bc8f5e";
    let wrong_variants = [
        "01890f47-7f6c-7cc2-18a2-4a97c1bc8f5e",
        "01890f47-7f6c-7cc2-c8a2-4a97c1bc8f5e",
        "01890f47-7f6c-7cc2-e8a2-4a97c1bc8f5e",
    ];
    let invalid_spellings = vec![
        canonical.to_uppercase(),
        canonical.replace('-', ""),
        format!("{{{canonical}}}"),
        format!("urn:uuid:{canonical}"),
        "01890f47-7f6c-4cc2-98a2-4a97c1bc8f5e".to_owned(),
        wrong_variants[0].to_owned(),
        wrong_variants[1].to_owned(),
        wrong_variants[2].to_owned(),
    ];

    for field in [
        "challenge_id",
        "server_instance_id",
        "workspace_id",
        "human_id",
    ] {
        for invalid in &invalid_spellings {
            let mut value = serde_json::to_value(&issued.challenge).unwrap();
            value[field] = serde_json::Value::String(invalid.clone());
            assert!(
                serde_json::from_value::<crate::SessionChallenge>(value).is_err(),
                "accepted noncanonical {field}: {invalid}"
            );
        }
    }

    for invalid in &invalid_spellings {
        let json = format!(
            r#"{{"schema":"proof.operator.session.exchange-request/v1","challenge_id":"{invalid}","client_nonce":"{}"}}"#,
            crate::types::encode_hex(&[7; 32])
        );
        assert!(serde_json::from_str::<SessionExchangeRequest>(&json).is_err());
    }

    let mut attestation = serde_json::to_value(harness.attest(&issued.challenge)).unwrap();
    attestation["challenge"]["workspace_id"] =
        serde_json::Value::String(format!("urn:uuid:{canonical}"));
    assert!(serde_json::from_value::<SessionAttestation>(attestation).is_err());

    for wrong_variant in wrong_variants {
        let mut attestation = serde_json::to_value(harness.attest(&issued.challenge)).unwrap();
        attestation["challenge"]["human_id"] = serde_json::Value::String(wrong_variant.to_owned());
        assert!(serde_json::from_value::<SessionAttestation>(attestation).is_err());
    }

    let mut wrong_variant_output = issued.challenge.clone();
    wrong_variant_output.challenge_id = Uuid::parse_str(wrong_variants[0]).unwrap();
    assert!(serde_json::to_value(&wrong_variant_output).is_err());
}

#[test]
fn inbound_challenge_times_require_exact_rfc3339_utc_shape() {
    let harness = Harness::new(CapabilitySet::all());
    let (issued, _) = harness.issue(CapabilitySet::all());
    let invalid_times = [
        "2035-01-02T03:04:05+00:00",
        "2035-01-02T03:04:05z",
        "2035-01-02 03:04:05Z",
        "2035-01-02T03:04:05.1234567890Z",
        "2035-02-30T03:04:05Z",
        "2035-01-02T03:04:60Z",
    ];
    for field in ["issued_at", "expires_at"] {
        for invalid in invalid_times {
            let mut value = serde_json::to_value(&issued.challenge).unwrap();
            value[field] = serde_json::Value::String(invalid.to_owned());
            assert!(
                serde_json::from_value::<crate::SessionChallenge>(value).is_err(),
                "accepted noncanonical {field}: {invalid}"
            );
        }
    }

    for valid in [
        "2035-01-02T03:04:05Z",
        "2035-01-02T03:04:05.1Z",
        "2035-01-02T03:04:05.123456789Z",
    ] {
        let mut value = serde_json::to_value(&issued.challenge).unwrap();
        value["issued_at"] = serde_json::Value::String(valid.to_owned());
        assert!(serde_json::from_value::<crate::SessionChallenge>(value).is_ok());
    }

    let mut leap_second_challenge = issued.challenge.clone();
    leap_second_challenge.issued_at = Utc
        .with_ymd_and_hms(2035, 1, 2, 3, 4, 59)
        .unwrap()
        .with_nanosecond(1_000_000_000)
        .unwrap();
    assert!(serde_json::to_string(&leap_second_challenge).is_err());
}

#[test]
fn policy_validation_rejects_non_v7_or_substituted_human_key() {
    let harness = Harness::new(CapabilitySet::all());
    let mut bad_policy = AuthPolicy {
        workspace_id: Uuid::nil(),
        workspace_fingerprint: ControlDigest::from_bytes([1; 32]),
        server_instance_id: harness.server_instance_id,
        human_id: harness.human_id,
        human_public_key: harness.signing_key.verifying_key(),
        human_public_key_fingerprint: public_key_fingerprint(&harness.signing_key.verifying_key()),
        auth_epoch: 1,
        policy_revision: 1,
        origin: "http://127.0.0.1:43121".to_owned(),
        enrolled_capabilities: CapabilitySet::all(),
        workspace_capabilities: CapabilitySet::all(),
        supported_capabilities: CapabilitySet::all(),
    };
    assert_eq!(
        bad_policy.validate(),
        Err(OperatorAuthError::InvalidRequest)
    );
    bad_policy.workspace_id = harness.workspace_id;
    let mut wrong_variant = *harness.server_instance_id.as_bytes();
    wrong_variant[8] &= 0x3f;
    bad_policy.server_instance_id = Uuid::from_bytes(wrong_variant);
    assert_eq!(
        bad_policy.validate(),
        Err(OperatorAuthError::InvalidRequest)
    );
    bad_policy.server_instance_id = harness.server_instance_id;
    bad_policy.human_public_key_fingerprint = ControlDigest::from_bytes([0; 32]);
    assert_eq!(
        bad_policy.validate(),
        Err(OperatorAuthError::InvalidRequest)
    );
    assert_eq!(
        OperatorAuthAuthority::new(
            bad_policy,
            harness.environment.clone(),
            harness.audit.clone()
        )
        .err(),
        Some(OperatorAuthError::InvalidRequest)
    );
}

#[test]
#[cfg(unix)]
fn disposable_trusted_workspace_rejects_substituted_identity_inputs() {
    let disposable = DisposableWorkspace::new();
    let mut rng = StdRng::seed_from_u64(disposable.sequence);
    let human_key = SigningKey::generate(&mut rng);
    let agent_key = SigningKey::generate(&mut rng);
    let substituted_key = SigningKey::generate(&mut rng);
    let capabilities = CapabilitySet::all();
    let server_instance_id = disposable.fixture_uuid(2);
    let (workspace, enrollment) = disposable.documents(&human_key, &agent_key, capabilities);

    let repository_proof = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
        .join(".proof");
    assert!(repository_proof.is_dir());
    assert_ne!(
        DisposableWorkspace::descriptor(&disposable.proof_directory),
        DisposableWorkspace::descriptor(&repository_proof),
        "the disposable trusted workspace must not reuse the forbidden repository .proof identity"
    );

    assert!(AuthPolicy::from_workspace(
        &workspace,
        &enrollment,
        server_instance_id,
        human_key.verifying_key(),
        "http://127.0.0.1:43121".to_owned(),
    )
    .is_ok());

    assert_eq!(
        AuthPolicy::from_workspace(
            &workspace,
            &enrollment,
            server_instance_id,
            substituted_key.verifying_key(),
            "http://127.0.0.1:43121".to_owned(),
        )
        .err(),
        Some(OperatorAuthError::InvalidRequest)
    );

    let mut substituted_enrollment = enrollment.clone();
    substituted_enrollment.human.principal_id =
        PrincipalId::new(different_uuid(enrollment.human.principal_id.as_uuid()));
    assert_eq!(
        AuthPolicy::from_workspace(
            &workspace,
            &substituted_enrollment,
            server_instance_id,
            human_key.verifying_key(),
            "http://127.0.0.1:43121".to_owned(),
        )
        .err(),
        Some(OperatorAuthError::InvalidRequest)
    );

    let mut substituted_workspace = workspace.clone();
    substituted_workspace.human.public_key =
        URL_SAFE_NO_PAD.encode(substituted_key.verifying_key().as_bytes());
    substituted_workspace.human.public_key_fingerprint =
        public_key_fingerprint(&substituted_key.verifying_key());
    assert_eq!(
        AuthPolicy::from_workspace(
            &substituted_workspace,
            &enrollment,
            server_instance_id,
            substituted_key.verifying_key(),
            "http://127.0.0.1:43121".to_owned(),
        )
        .err(),
        Some(OperatorAuthError::InvalidRequest)
    );
}

#[test]
fn signed_challenge_exchanges_once_and_grants_only_intersection() {
    let allowed = CapabilitySet::new(vec![Capability::ApprovalRead, Capability::RunRead]).unwrap();
    let harness = Harness::new(allowed.clone());
    let (issued, nonce) = harness.issue(CapabilitySet::all());
    assert_eq!(issued.challenge.granted_capabilities, allowed);
    harness
        .authority
        .submit_attestation(harness.attest(&issued.challenge))
        .unwrap();
    let request = || SessionExchangeRequest {
        schema: "proof.operator.session.exchange-request/v1".to_owned(),
        challenge_id: issued.challenge.challenge_id,
        client_nonce: crate::types::encode_hex(&nonce[..]),
    };
    let response = harness.authority.exchange(request()).unwrap();
    assert_eq!(response.granted_capabilities, allowed);
    assert_eq!(
        harness.authority.exchange(request()).err(),
        Some(OperatorAuthError::AuthenticationRequired)
    );
    assert_eq!(
        harness
            .audit
            .intents
            .lock()
            .unwrap()
            .iter()
            .map(|intent| intent.kind)
            .collect::<Vec<_>>(),
        vec![
            ControlAuthorityEventKind::SessionChallengeIssued,
            ControlAuthorityEventKind::SessionIssued,
        ]
    );
}

#[test]
fn second_pending_challenge_is_rejected_without_replacing_first() {
    let harness = Harness::new(CapabilitySet::all());
    let (issued, nonce) = harness.issue(CapabilitySet::all());
    assert_eq!(
        harness.authority.issue_challenge(ChallengeIssueRequest {
            schema: "proof.operator.session.challenge-issue-request/v1".to_owned(),
            client_nonce_digest: client_nonce_digest(&[9; 32]),
            requested_capabilities: CapabilitySet::all(),
        }),
        Err(OperatorAuthError::ChallengePending)
    );
    harness
        .authority
        .submit_attestation(harness.attest(&issued.challenge))
        .unwrap();
    assert!(harness
        .authority
        .exchange(SessionExchangeRequest {
            schema: "proof.operator.session.exchange-request/v1".to_owned(),
            challenge_id: issued.challenge.challenge_id,
            client_nonce: crate::types::encode_hex(&nonce[..]),
        })
        .is_ok());
}

#[test]
fn expired_pending_challenge_is_replaced_at_deadline_equality() {
    let harness = Harness::new(CapabilitySet::all());
    let (expired, _) = harness.issue(CapabilitySet::all());
    harness.environment.advance(120);

    let (replacement, _) = harness.issue(CapabilitySet::all());

    assert_ne!(
        replacement.challenge.challenge_id,
        expired.challenge.challenge_id
    );
    assert_eq!(
        harness
            .audit
            .intents
            .lock()
            .unwrap()
            .iter()
            .map(|intent| intent.kind)
            .collect::<Vec<_>>(),
        vec![
            ControlAuthorityEventKind::SessionChallengeIssued,
            ControlAuthorityEventKind::SessionChallengeIssued,
        ]
    );
}

#[test]
fn failed_terminal_ceremony_consumes_only_matching_challenge() {
    let harness = Harness::new(CapabilitySet::all());
    let (issued, _) = harness.issue(CapabilitySet::all());
    assert_eq!(
        harness
            .authority
            .consume_failed_challenge(different_uuid(issued.challenge.challenge_id)),
        Err(OperatorAuthError::AuthenticationRequired)
    );
    harness
        .authority
        .consume_failed_challenge(issued.challenge.challenge_id)
        .unwrap();
    assert!(harness
        .authority
        .issue_challenge(ChallengeIssueRequest {
            schema: "proof.operator.session.challenge-issue-request/v1".to_owned(),
            client_nonce_digest: client_nonce_digest(&[10; 32]),
            requested_capabilities: CapabilitySet::all(),
        })
        .is_ok());
}

#[test]
fn successful_reauthentication_atomically_replaces_old_session() {
    let harness = Harness::new(CapabilitySet::all());
    let old_header = harness.establish(CapabilitySet::all());
    let (issued, nonce) = harness.issue(CapabilitySet::all());
    assert!(harness
        .authority
        .authorize_any_with(&[old_header.as_bytes()], |_| Ok::<_, ()>(()))
        .is_ok());
    harness
        .authority
        .submit_attestation(harness.attest(&issued.challenge))
        .unwrap();
    let response = harness
        .authority
        .exchange(SessionExchangeRequest {
            schema: "proof.operator.session.exchange-request/v1".to_owned(),
            challenge_id: issued.challenge.challenge_id,
            client_nonce: crate::types::encode_hex(&nonce[..]),
        })
        .unwrap();
    let new_header = response.session_token.header_value();
    assert_eq!(
        harness
            .authority
            .authorize_any_with(&[old_header.as_bytes()], |_| Ok::<_, ()>(())),
        Err(AuthorizedCallError::Auth(
            OperatorAuthError::AuthenticationRequired
        ))
    );
    assert!(harness
        .authority
        .authorize_any_with(&[new_header.as_bytes()], |_| Ok::<_, ()>(()))
        .is_ok());
    assert_eq!(
        harness.audit.intents.lock().unwrap().last().unwrap().kind,
        ControlAuthorityEventKind::SessionReplaced
    );
}

#[test]
fn altered_or_wrong_human_attestation_consumes_challenge() {
    let harness = Harness::new(CapabilitySet::all());
    let (issued, _) = harness.issue(CapabilitySet::all());
    let mut altered = harness.attest(&issued.challenge);
    altered.challenge.server_instance_id = different_uuid(issued.challenge.server_instance_id);
    assert_eq!(
        harness.authority.submit_attestation(altered),
        Err(OperatorAuthError::AuthenticationRequired)
    );
    assert_eq!(
        harness
            .authority
            .submit_attestation(harness.attest(&issued.challenge)),
        Err(OperatorAuthError::AuthenticationRequired)
    );

    let (issued, _) = harness.issue(CapabilitySet::all());
    let mut other_rng = StdRng::seed_from_u64(999);
    let other = SigningKey::generate(&mut other_rng);
    let bytes = challenge_signing_bytes(&issued.challenge).unwrap();
    let mut attestation = harness.attest(&issued.challenge);
    attestation.signature = URL_SAFE_NO_PAD.encode(other.sign(&bytes).to_bytes());
    assert_eq!(
        harness.authority.submit_attestation(attestation),
        Err(OperatorAuthError::AuthenticationRequired)
    );
}

#[test]
fn challenge_deadline_and_bad_nonce_consume_without_session() {
    let harness = Harness::new(CapabilitySet::all());
    let (issued, nonce) = harness.issue(CapabilitySet::all());
    harness
        .authority
        .submit_attestation(harness.attest(&issued.challenge))
        .unwrap();
    harness.environment.advance(120);
    assert_eq!(
        harness
            .authority
            .exchange(SessionExchangeRequest {
                schema: "proof.operator.session.exchange-request/v1".to_owned(),
                challenge_id: issued.challenge.challenge_id,
                client_nonce: crate::types::encode_hex(nonce.as_ref().as_ref()),
            })
            .err(),
        Some(OperatorAuthError::AuthenticationRequired)
    );

    let (issued, _) = harness.issue(CapabilitySet::all());
    harness
        .authority
        .submit_attestation(harness.attest(&issued.challenge))
        .unwrap();
    assert_eq!(
        harness
            .authority
            .exchange(SessionExchangeRequest {
                schema: "proof.operator.session.exchange-request/v1".to_owned(),
                challenge_id: issued.challenge.challenge_id,
                client_nonce: crate::types::encode_hex(&[0xff; 32]),
            })
            .err(),
        Some(OperatorAuthError::AuthenticationRequired)
    );
    assert_eq!(
        harness
            .authority
            .exchange(SessionExchangeRequest {
                schema: "proof.operator.session.exchange-request/v1".to_owned(),
                challenge_id: issued.challenge.challenge_id,
                client_nonce: crate::types::encode_hex(&nonce[..]),
            })
            .err(),
        Some(OperatorAuthError::AuthenticationRequired)
    );
}

#[test]
fn concurrent_exchange_has_one_winner_and_no_recovery() {
    let harness = Harness::new(CapabilitySet::all());
    let (issued, nonce) = harness.issue(CapabilitySet::all());
    harness
        .authority
        .submit_attestation(harness.attest(&issued.challenge))
        .unwrap();
    let nonce = Arc::new(nonce);
    let barrier = Arc::new(Barrier::new(3));
    let winners = Arc::new(AtomicUsize::new(0));
    let mut handles = Vec::new();
    for _ in 0..2 {
        let authority = harness.authority.clone();
        let barrier = barrier.clone();
        let winners = winners.clone();
        let nonce = nonce.clone();
        let challenge_id = issued.challenge.challenge_id;
        handles.push(thread::spawn(move || {
            barrier.wait();
            if authority
                .exchange(SessionExchangeRequest {
                    schema: "proof.operator.session.exchange-request/v1".to_owned(),
                    challenge_id,
                    client_nonce: crate::types::encode_hex(nonce.as_ref().as_ref()),
                })
                .is_ok()
            {
                winners.fetch_add(1, Ordering::SeqCst);
            }
        }));
    }
    barrier.wait();
    for handle in handles {
        handle.join().unwrap();
    }
    assert_eq!(winners.load(Ordering::SeqCst), 1);
    assert_eq!(
        harness
            .authority
            .exchange(SessionExchangeRequest {
                schema: "proof.operator.session.exchange-request/v1".to_owned(),
                challenge_id: issued.challenge.challenge_id,
                client_nonce: crate::types::encode_hex(nonce.as_ref().as_ref()),
            })
            .err(),
        Some(OperatorAuthError::AuthenticationRequired)
    );
}

#[test]
fn malformed_duplicate_and_cross_capability_credentials_do_not_call_protected_code() {
    let allowed = CapabilitySet::new(vec![Capability::RunRead]).unwrap();
    let harness = Harness::new(allowed);
    let header = harness.establish(CapabilitySet::new(vec![Capability::RunRead]).unwrap());
    let calls = AtomicUsize::new(0);
    let required = CapabilitySet::new(vec![Capability::ApprovalRead]).unwrap();

    for headers in [
        vec![],
        vec![b"short".as_slice()],
        vec![header.as_bytes(), header.as_bytes()],
    ] {
        let result = harness.authority.authorize_any_with(&headers, |_| {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok::<_, ()>(())
        });
        assert_eq!(
            result,
            Err(AuthorizedCallError::Auth(
                OperatorAuthError::AuthenticationRequired
            ))
        );
    }
    assert_eq!(
        harness
            .authority
            .authorize_with(&[header.as_bytes()], &required, |_| {
                calls.fetch_add(1, Ordering::SeqCst);
                Ok::<_, ()>(())
            }),
        Err(AuthorizedCallError::Auth(
            OperatorAuthError::CapabilityRequired
        ))
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[test]
fn deadline_equality_expires_before_callback_and_audits_once() {
    let harness = Harness::new(CapabilitySet::all());
    let header = harness.establish(CapabilitySet::all());
    harness.environment.advance(300);
    let calls = AtomicUsize::new(0);
    assert_eq!(
        harness
            .authority
            .authorize_any_with(&[header.as_bytes()], |_| {
                calls.fetch_add(1, Ordering::SeqCst);
                Ok::<_, ()>(())
            }),
        Err(AuthorizedCallError::Auth(
            OperatorAuthError::AuthenticationRequired
        ))
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(
        harness
            .audit
            .intents
            .lock()
            .unwrap()
            .last()
            .map(|intent| intent.kind),
        Some(ControlAuthorityEventKind::SessionExpired)
    );
}

#[test]
fn explicit_self_revoke_wins_before_waiting_protected_callback() {
    let harness = Harness::new(CapabilitySet::all());
    let header = Arc::new(harness.establish(CapabilitySet::all()));
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let authority = harness.authority.clone();
    let revoke_header = header.clone();
    let revoke = thread::spawn(move || {
        authority.revoke_with(&[revoke_header.as_ref().as_bytes()], |_| {
            entered_tx.send(()).unwrap();
            release_rx.recv().unwrap();
            Ok::<_, ()>(())
        })
    });
    entered_rx.recv().unwrap();

    let protected_calls = Arc::new(AtomicUsize::new(0));
    let authority = harness.authority.clone();
    let protected_calls_thread = protected_calls.clone();
    let authorize_header = header.clone();
    let authorize = thread::spawn(move || {
        authority.authorize_any_with(&[authorize_header.as_ref().as_bytes()], |_| {
            protected_calls_thread.fetch_add(1, Ordering::SeqCst);
            Ok::<_, ()>(())
        })
    });
    release_tx.send(()).unwrap();
    assert_eq!(revoke.join().unwrap(), Ok(()));
    assert_eq!(
        authorize.join().unwrap(),
        Err(AuthorizedCallError::Auth(
            OperatorAuthError::AuthenticationRequired
        ))
    );
    assert_eq!(protected_calls.load(Ordering::SeqCst), 0);
}

#[test]
fn authorized_callback_finishes_before_waiting_self_revoke() {
    let harness = Harness::new(CapabilitySet::all());
    let header = Arc::new(harness.establish(CapabilitySet::all()));
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let authority = harness.authority.clone();
    let authorize_header = header.clone();
    let authorize = thread::spawn(move || {
        authority.authorize_any_with(&[authorize_header.as_ref().as_bytes()], |_| {
            entered_tx.send(()).unwrap();
            release_rx.recv().unwrap();
            Ok::<_, ()>(())
        })
    });
    entered_rx.recv().unwrap();

    let (started_tx, started_rx) = mpsc::channel();
    let authority = harness.authority.clone();
    let revoke_header = header.clone();
    let revoke = thread::spawn(move || {
        started_tx.send(()).unwrap();
        authority.revoke_with(&[revoke_header.as_ref().as_bytes()], |_| Ok::<_, ()>(()))
    });
    started_rx.recv().unwrap();
    release_tx.send(()).unwrap();

    assert_eq!(authorize.join().unwrap(), Ok(()));
    assert_eq!(revoke.join().unwrap(), Ok(()));
    assert_eq!(
        harness
            .authority
            .authorize_any_with(&[header.as_ref().as_bytes()], |_| Ok::<_, ()>(())),
        Err(AuthorizedCallError::Auth(
            OperatorAuthError::AuthenticationRequired
        ))
    );
}

#[test]
fn authorized_callback_finishes_before_waiting_expiry_check() {
    let harness = Harness::new(CapabilitySet::all());
    let header = Arc::new(harness.establish(CapabilitySet::all()));
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let authority = harness.authority.clone();
    let first_header = header.clone();
    let first = thread::spawn(move || {
        authority.authorize_any_with(&[first_header.as_ref().as_bytes()], |_| {
            entered_tx.send(()).unwrap();
            release_rx.recv().unwrap();
            Ok::<_, ()>(())
        })
    });
    entered_rx.recv().unwrap();
    harness.environment.advance(300);

    let callbacks = Arc::new(AtomicUsize::new(0));
    let authority = harness.authority.clone();
    let callbacks_thread = callbacks.clone();
    let second_header = header.clone();
    let second = thread::spawn(move || {
        authority.authorize_any_with(&[second_header.as_ref().as_bytes()], |_| {
            callbacks_thread.fetch_add(1, Ordering::SeqCst);
            Ok::<_, ()>(())
        })
    });
    release_tx.send(()).unwrap();

    assert_eq!(first.join().unwrap(), Ok(()));
    assert_eq!(
        second.join().unwrap(),
        Err(AuthorizedCallError::Auth(
            OperatorAuthError::AuthenticationRequired
        ))
    );
    assert_eq!(callbacks.load(Ordering::SeqCst), 0);
    assert_eq!(
        harness
            .audit
            .intents
            .lock()
            .unwrap()
            .iter()
            .filter(|intent| intent.kind == ControlAuthorityEventKind::SessionExpired)
            .count(),
        1
    );
}

#[test]
fn callback_failure_preserves_session_and_success_refreshes_idle_deadline() {
    let harness = Harness::new(CapabilitySet::all());
    let header = harness.establish(CapabilitySet::all());
    assert_eq!(
        harness
            .authority
            .authorize_any_with(&[header.as_bytes()], |_| Err::<(), _>("closed")),
        Err(AuthorizedCallError::Callback("closed"))
    );
    harness.environment.advance(299);
    assert_eq!(
        harness
            .authority
            .authorize_any_with(&[header.as_bytes()], |_| Ok::<_, ()>(7)),
        Ok(7)
    );
    harness.environment.advance(299);
    assert!(harness
        .authority
        .authorize_any_with(&[header.as_bytes()], |_| Ok::<_, ()>(()))
        .is_ok());
}

#[test]
fn failed_durable_revoke_clears_authority_fail_closed() {
    let harness = Harness::new(CapabilitySet::all());
    let header = harness.establish(CapabilitySet::all());
    assert_eq!(
        harness
            .authority
            .revoke_with(&[header.as_bytes()], |_| Err::<(), _>("store unavailable")),
        Err(AuthorizedCallError::Callback("store unavailable"))
    );
    assert_eq!(
        harness
            .authority
            .authorize_any_with(&[header.as_bytes()], |_| Ok::<_, ()>(())),
        Err(AuthorizedCallError::Auth(
            OperatorAuthError::AuthenticationRequired
        ))
    );
}

#[test]
fn restart_and_shutdown_invalidate_memory_only_authority() {
    let harness = Harness::new(CapabilitySet::all());
    let header = harness.establish(CapabilitySet::all());
    harness.authority.invalidate_for_shutdown().unwrap();
    assert_eq!(
        harness.audit.intents.lock().unwrap().last().unwrap().kind,
        ControlAuthorityEventKind::ControlShutdown
    );
    assert_eq!(
        harness
            .authority
            .authorize_any_with(&[header.as_bytes()], |_| Ok::<_, ()>(())),
        Err(AuthorizedCallError::Auth(
            OperatorAuthError::AuthenticationRequired
        ))
    );

    let restarted = OperatorAuthAuthority::new(
        AuthPolicy {
            workspace_id: harness.workspace_id,
            workspace_fingerprint: harness.workspace_fingerprint,
            server_instance_id: different_uuid(harness.server_instance_id),
            human_id: harness.human_id,
            human_public_key: harness.signing_key.verifying_key(),
            human_public_key_fingerprint: public_key_fingerprint(
                &harness.signing_key.verifying_key(),
            ),
            auth_epoch: 1,
            policy_revision: 1,
            origin: "http://127.0.0.1:43121".to_owned(),
            enrolled_capabilities: CapabilitySet::all(),
            workspace_capabilities: CapabilitySet::all(),
            supported_capabilities: CapabilitySet::all(),
        },
        harness.environment.clone(),
        harness.audit.clone(),
    )
    .unwrap();
    assert_eq!(
        restarted.authorize_any_with(&[header.as_bytes()], |_| Ok::<_, ()>(())),
        Err(AuthorizedCallError::Auth(
            OperatorAuthError::AuthenticationRequired
        ))
    );
}

#[test]
fn session_token_cannot_cross_workspace_instance_or_human_authority() {
    let harness = Harness::new(CapabilitySet::all());
    let header = harness.establish(CapabilitySet::all());
    for (workspace_id, instance_id, human_id) in [
        (
            different_uuid(harness.workspace_id),
            harness.server_instance_id,
            harness.human_id,
        ),
        (
            harness.workspace_id,
            different_uuid(harness.server_instance_id),
            harness.human_id,
        ),
        (
            harness.workspace_id,
            harness.server_instance_id,
            different_uuid(harness.human_id),
        ),
    ] {
        let authority = OperatorAuthAuthority::new(
            AuthPolicy {
                workspace_id,
                workspace_fingerprint: harness.workspace_fingerprint,
                server_instance_id: instance_id,
                human_id,
                human_public_key: harness.signing_key.verifying_key(),
                human_public_key_fingerprint: public_key_fingerprint(
                    &harness.signing_key.verifying_key(),
                ),
                auth_epoch: 1,
                policy_revision: 1,
                origin: "http://127.0.0.1:43121".to_owned(),
                enrolled_capabilities: CapabilitySet::all(),
                workspace_capabilities: CapabilitySet::all(),
                supported_capabilities: CapabilitySet::all(),
            },
            harness.environment.clone(),
            harness.audit.clone(),
        )
        .unwrap();
        assert_eq!(
            authority.authorize_any_with(&[header.as_bytes()], |_| Ok::<_, ()>(())),
            Err(AuthorizedCallError::Auth(
                OperatorAuthError::AuthenticationRequired
            ))
        );
    }
}

#[test]
fn environment_generated_ids_require_uuid_v7_and_rfc4122_variant() {
    let challenge_harness = Harness::new(CapabilitySet::all());
    challenge_harness
        .environment
        .invalid_uuid_variant
        .store(true, Ordering::SeqCst);
    assert_eq!(
        challenge_harness
            .authority
            .issue_challenge(ChallengeIssueRequest {
                schema: "proof.operator.session.challenge-issue-request/v1".to_owned(),
                client_nonce_digest: client_nonce_digest(&[5; 32]),
                requested_capabilities: CapabilitySet::all(),
            }),
        Err(OperatorAuthError::ControlUnavailable)
    );

    let session_harness = Harness::new(CapabilitySet::all());
    let (issued, nonce) = session_harness.issue(CapabilitySet::all());
    session_harness
        .authority
        .submit_attestation(session_harness.attest(&issued.challenge))
        .unwrap();
    session_harness
        .environment
        .invalid_uuid_variant
        .store(true, Ordering::SeqCst);
    assert_eq!(
        session_harness
            .authority
            .exchange(SessionExchangeRequest {
                schema: "proof.operator.session.exchange-request/v1".to_owned(),
                challenge_id: issued.challenge.challenge_id,
                client_nonce: crate::types::encode_hex(&nonce[..]),
            })
            .err(),
        Some(OperatorAuthError::ControlUnavailable)
    );
    assert_eq!(
        session_harness
            .audit
            .intents
            .lock()
            .unwrap()
            .iter()
            .filter(|intent| {
                matches!(
                    intent.kind,
                    ControlAuthorityEventKind::SessionIssued
                        | ControlAuthorityEventKind::SessionReplaced
                )
            })
            .count(),
        0
    );
}

#[test]
fn environment_or_audit_failure_clears_existing_authority() {
    let harness = Harness::new(CapabilitySet::all());
    let header = harness.establish(CapabilitySet::all());
    harness.environment.fail_clock.store(true, Ordering::SeqCst);
    assert_eq!(
        harness
            .authority
            .authorize_any_with(&[header.as_bytes()], |_| Ok::<_, ()>(())),
        Err(AuthorizedCallError::Auth(
            OperatorAuthError::ControlUnavailable
        ))
    );
    harness
        .environment
        .fail_clock
        .store(false, Ordering::SeqCst);
    assert_eq!(
        harness
            .authority
            .authorize_any_with(&[header.as_bytes()], |_| Ok::<_, ()>(())),
        Err(AuthorizedCallError::Auth(
            OperatorAuthError::AuthenticationRequired
        ))
    );

    harness.audit.fail.store(true, Ordering::SeqCst);
    assert_eq!(
        harness.authority.issue_challenge(ChallengeIssueRequest {
            schema: "proof.operator.session.challenge-issue-request/v1".to_owned(),
            client_nonce_digest: client_nonce_digest(&[3; 32]),
            requested_capabilities: CapabilitySet::all(),
        }),
        Err(OperatorAuthError::ControlUnavailable)
    );

    harness.audit.fail.store(false, Ordering::SeqCst);
    harness.audit.corrupt.store(true, Ordering::SeqCst);
    assert_eq!(
        harness.authority.issue_challenge(ChallengeIssueRequest {
            schema: "proof.operator.session.challenge-issue-request/v1".to_owned(),
            client_nonce_digest: client_nonce_digest(&[4; 32]),
            requested_capabilities: CapabilitySet::all(),
        }),
        Err(OperatorAuthError::ControlUnavailable)
    );
}

#[test]
fn token_debug_and_callback_scope_never_expose_raw_credential() {
    let harness = Harness::new(CapabilitySet::all());
    let (issued, nonce) = harness.issue(CapabilitySet::all());
    let expected_workspace_id = harness.workspace_id;
    let expected_server_instance_id = harness.server_instance_id;
    let expected_human_id = harness.human_id;
    harness
        .authority
        .submit_attestation(harness.attest(&issued.challenge))
        .unwrap();
    let response = harness
        .authority
        .exchange(SessionExchangeRequest {
            schema: "proof.operator.session.exchange-request/v1".to_owned(),
            challenge_id: issued.challenge.challenge_id,
            client_nonce: crate::types::encode_hex(&nonce[..]),
        })
        .unwrap();
    assert_eq!(
        format!("{:?}", response.session_token),
        "SessionToken([REDACTED])"
    );
    let header: crate::SessionHeaderValue = response.session_token.header_value();
    assert_eq!(format!("{header:?}"), "SessionHeaderValue([REDACTED])");
    let encoded = std::str::from_utf8(header.as_bytes()).unwrap();
    assert!(!format!("{response:?}").contains(encoded));
    harness
        .authority
        .authorize_any_with(&[header.as_bytes()], |session| {
            assert_eq!(session.workspace_id, expected_workspace_id);
            assert_eq!(session.server_instance_id, expected_server_instance_id);
            assert_eq!(session.human_id, expected_human_id);
            let binding = session.authority_binding();
            assert_eq!(binding.session_id, session.session_id);
            assert_eq!(binding.granted_capabilities, session.granted_capabilities);
            Ok::<_, ()>(())
        })
        .unwrap();
}

fn uuid_v7(sequence: u64) -> Uuid {
    let mut bytes = [0_u8; 16];
    bytes[..8].copy_from_slice(&sequence.to_be_bytes());
    bytes[6] = (bytes[6] & 0x0f) | 0x70;
    bytes[8] = 0x80;
    bytes[8..].copy_from_slice(&sequence.rotate_left(17).to_be_bytes());
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Uuid::from_bytes(bytes)
}

fn different_uuid(value: Uuid) -> Uuid {
    let mut bytes = *value.as_bytes();
    bytes[15] ^= 1;
    Uuid::from_bytes(bytes)
}
