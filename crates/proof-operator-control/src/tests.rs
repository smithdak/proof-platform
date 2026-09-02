use std::{
    fs::{self, File},
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, AtomicUsize, Ordering},
        Arc, Mutex,
    },
    time::Duration as StdDuration,
};

use axum::{
    body::{to_bytes, Body},
    extract::connect_info::ConnectInfo,
    http::{header, HeaderValue, Method, Request, Response, StatusCode},
};
use base64::{
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
    Engine as _,
};
use chrono::{DateTime, Duration, TimeZone, Utc};
use ed25519_dalek::{Signer, SigningKey};
use proof_kernel::{
    control_digest_serialized, ArtifactDigest, AuditEvent, AuditEventKind, AuditOutcome,
    Capability, CapabilitySet, CommandBinding, ControlAuditAppendRequest, ControlAuditAppendResult,
    ControlAuthorityEventKind, ControlDigest, DescriptorIdentity, HumanEnrollment,
    OperatorAuthorityAuditStore, OperatorCommand, OperatorControlEnvironment, OperatorCursorCodec,
    OperatorDirectoryStore, OperatorEnvironmentError, OperatorRandomPurpose, OperatorReadRoute,
    OperatorReadScope, OperatorStoreError, OperatorWorkspace, PrincipalBinding, PrincipalId,
    PrincipalKind, RegisterGovernedRunRequest, RegisterGovernedRunResult, SessionRevokeRequest,
    WorkspaceFingerprintInput,
};
use proof_operator_auth::{
    challenge_code, challenge_signed_bytes_digest, challenge_signing_bytes, client_nonce_digest,
    public_key_fingerprint, AuthPolicy, AuthorizedSession, ChallengeIssueRequest,
    ChallengeIssueResponse, OperatorAuthAuthority, SessionAttestation, SessionChallenge,
    SessionExchangeRequest, SessionHeaderValue,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use uuid::Uuid;

use crate::{
    ceremony::complete_challenge_ceremony_with_timeout,
    routing::dispatch_for_test,
    startup::{preflight_control_plane, StartupTerminal},
    *,
};

fn uuid_v7(sequence: u64) -> Uuid {
    let mut bytes = [0_u8; 16];
    bytes[..8].copy_from_slice(&sequence.to_be_bytes());
    bytes[6] = (bytes[6] & 0x0f) | 0x70;
    bytes[8..].copy_from_slice(&sequence.rotate_left(17).to_be_bytes());
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Uuid::from_bytes(bytes)
}

struct TestEnvironment {
    utc: Mutex<DateTime<Utc>>,
    tick: AtomicU64,
    sequence: AtomicU64,
    stages: Option<Arc<Mutex<Vec<&'static str>>>>,
}

impl TestEnvironment {
    fn new(seed: u64) -> Self {
        Self {
            utc: Mutex::new(Utc.with_ymd_and_hms(2032, 1, 1, 0, 0, 0).unwrap()),
            tick: AtomicU64::new(0),
            sequence: AtomicU64::new(seed),
            stages: None,
        }
    }

    fn recording(seed: u64, stages: Arc<Mutex<Vec<&'static str>>>) -> Self {
        Self {
            stages: Some(stages),
            ..Self::new(seed)
        }
    }

    fn advance(&self, duration: Duration) {
        *self.utc.lock().unwrap() += duration;
        self.tick.fetch_add(
            u64::try_from(duration.num_milliseconds()).unwrap(),
            Ordering::SeqCst,
        );
    }

    fn record(&self, stage: &'static str) {
        if let Some(stages) = &self.stages {
            stages.lock().unwrap().push(stage);
        }
    }
}

impl OperatorControlEnvironment for TestEnvironment {
    fn trusted_utc_now(&self) -> Result<DateTime<Utc>, OperatorEnvironmentError> {
        self.record("clock");
        Ok(*self.utc.lock().unwrap())
    }

    fn monotonic_millis(&self) -> Result<u64, OperatorEnvironmentError> {
        self.record("tick");
        Ok(self.tick.load(Ordering::SeqCst))
    }

    fn fill_random(
        &self,
        purpose: OperatorRandomPurpose,
        output: &mut [u8],
    ) -> Result<(), OperatorEnvironmentError> {
        self.record("random");
        let sequence = self.sequence.fetch_add(1, Ordering::SeqCst) as u8;
        for (index, byte) in output.iter_mut().enumerate() {
            *byte = sequence ^ purpose as u8 ^ index as u8;
        }
        Ok(())
    }

    fn new_uuid_v7(&self) -> Result<Uuid, OperatorEnvironmentError> {
        self.record("uuid");
        Ok(uuid_v7(self.sequence.fetch_add(1, Ordering::SeqCst)))
    }
}

#[derive(Default)]
struct TestAudit {
    requests: Mutex<Vec<ControlAuditAppendRequest>>,
}

impl OperatorAuthorityAuditStore for TestAudit {
    fn append_authority_event(
        &self,
        request: ControlAuditAppendRequest,
    ) -> Result<ControlAuditAppendResult, OperatorStoreError> {
        append_test_audit(&self.requests, request)
    }
}

fn append_test_audit(
    requests: &Mutex<Vec<ControlAuditAppendRequest>>,
    request: ControlAuditAppendRequest,
) -> Result<ControlAuditAppendResult, OperatorStoreError> {
    let sequence = requests.lock().unwrap().len() as u64 + 1;
    let event = AuditEvent {
        schema: AuditEvent::SCHEMA.to_owned(),
        workspace_id: request.workspace_id,
        event_id: uuid_v7(1_000 + sequence),
        sequence,
        kind: match request.kind {
            ControlAuthorityEventKind::ControlShutdown => AuditEventKind::ControlShutdown,
            ControlAuthorityEventKind::SessionChallengeIssued => {
                AuditEventKind::SessionChallengeIssued
            }
            ControlAuthorityEventKind::SessionExpired => AuditEventKind::SessionExpired,
            ControlAuthorityEventKind::SessionIssued => AuditEventKind::SessionIssued,
            ControlAuthorityEventKind::SessionReplaced => AuditEventKind::SessionReplaced,
        },
        outcome: if request.kind == ControlAuthorityEventKind::SessionExpired {
            AuditOutcome::Expired
        } else {
            AuditOutcome::Accepted
        },
        previous_digest: None,
        event_digest: ControlDigest::from_bytes([sequence as u8; 32]),
        human_id: request.human_id,
        session_id: request.session_id,
        challenge_id: request.challenge_id,
        challenge_digest: request.challenge_digest,
        session_authority_digest: request.session_authority_digest,
        related_session_id: request.related_session_id,
        server_instance_id: Some(request.server_instance_id),
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
        auth_epoch: request.auth_epoch,
        policy_revision: request.policy_revision,
        intent_digest: None,
        call_digest: None,
        decision_digest: None,
        recovery_directive_digest: None,
        failure_scope: None,
        proof: None,
        occurred_at: Utc.with_ymd_and_hms(2032, 1, 1, 0, 0, 0).unwrap(),
    };
    requests.lock().unwrap().push(request);
    Ok(ControlAuditAppendResult {
        schema: ControlAuditAppendResult::SCHEMA.to_owned(),
        event,
    })
}

fn workspace_material(capabilities: CapabilitySet) -> (OperatorWorkspace, SigningKey) {
    let human_key = SigningKey::from_bytes(&[7_u8; 32]);
    let agent_key = SigningKey::from_bytes(&[8_u8; 32]);
    let human_id = uuid_v7(3);
    let agent_id = uuid_v7(4);
    let workspace_id = uuid_v7(1);
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
        workspace_id,
        proof_directory: DescriptorIdentity {
            device: 1,
            inode: 2,
        },
        control_lock: DescriptorIdentity {
            device: 1,
            inode: 3,
        },
        agent_key_file: DescriptorIdentity {
            device: 1,
            inode: 4,
        },
        human_key_file: DescriptorIdentity {
            device: 1,
            inode: 5,
        },
        agent_id,
        human_id,
        agent_public_key: agent.public_key.clone(),
        human_public_key: human.public_key.clone(),
    };
    let now = Utc.with_ymd_and_hms(2032, 1, 1, 0, 0, 0).unwrap();
    let mut workspace = OperatorWorkspace {
        schema: OperatorWorkspace::SCHEMA.to_owned(),
        workspace_id,
        database_name: "storage.db".to_owned(),
        workspace_fingerprint: control_digest_serialized(
            "Proof-Operator-Workspace-v1",
            &fingerprint_input,
        )
        .unwrap(),
        fingerprint_input,
        schema_catalog_digest: ControlDigest::from_bytes([9; 32]),
        agent,
        human,
        auth_epoch: 1,
        policy_revision: 1,
        capabilities,
        created_at: now,
        updated_at: now,
        binding_digest: ControlDigest::from_bytes([0; 32]),
    };
    let mut binding = serde_json::to_value(&workspace).unwrap();
    binding.as_object_mut().unwrap().remove("binding_digest");
    workspace.binding_digest =
        control_digest_serialized("Proof-Operator-Workspace-Binding-v1", &binding).unwrap();
    workspace.validate().unwrap();
    (workspace, human_key)
}

struct AuthHarness {
    authority: Arc<OperatorAuthAuthority>,
    signing_key: SigningKey,
    environment: Arc<TestEnvironment>,
    audit: Arc<TestAudit>,
}

impl AuthHarness {
    fn new(origin: &str, capabilities: CapabilitySet, instance: u64) -> Self {
        let (workspace, signing_key) = workspace_material(capabilities.clone());
        let enrollment = HumanEnrollment {
            schema: HumanEnrollment::SCHEMA.to_owned(),
            workspace_id: workspace.workspace_id,
            human: workspace.human.clone(),
            capabilities: capabilities.clone(),
            capability_set_digest: control_digest_serialized(
                "Proof-Operator-Capability-Set-v1",
                &capabilities,
            )
            .unwrap(),
            enrolled_at: workspace.created_at,
        };
        let environment = Arc::new(TestEnvironment::new(100 + instance));
        let audit = Arc::new(TestAudit::default());
        let policy = AuthPolicy::from_workspace(
            &workspace,
            &enrollment,
            uuid_v7(instance),
            signing_key.verifying_key(),
            origin.to_owned(),
        )
        .unwrap();
        let authority = Arc::new(
            OperatorAuthAuthority::new(policy, environment.clone(), audit.clone()).unwrap(),
        );
        Self {
            authority,
            signing_key,
            environment,
            audit,
        }
    }

    fn challenge(&self, nonce: &[u8; 32]) -> SessionChallenge {
        self.authority
            .issue_challenge(ChallengeIssueRequest {
                schema: "proof.operator.session.challenge-issue-request/v1".to_owned(),
                client_nonce_digest: client_nonce_digest(nonce),
                requested_capabilities: CapabilitySet::all(),
            })
            .unwrap()
            .challenge
    }

    fn establish_session(&self, nonce: [u8; 32]) -> (SessionHeaderValue, AuthorizedSession) {
        establish_authority_session(&self.authority, &self.signing_key, nonce)
    }
}

fn establish_authority_session(
    authority: &OperatorAuthAuthority,
    signing_key: &SigningKey,
    nonce: [u8; 32],
) -> (SessionHeaderValue, AuthorizedSession) {
    let challenge = authority
        .issue_challenge(ChallengeIssueRequest {
            schema: "proof.operator.session.challenge-issue-request/v1".to_owned(),
            client_nonce_digest: client_nonce_digest(&nonce),
            requested_capabilities: CapabilitySet::all(),
        })
        .unwrap()
        .challenge;
    let attestation = TestSigner(signing_key.clone())
        .sign_challenge(&challenge)
        .unwrap();
    authority.submit_attestation(attestation).unwrap();
    let response = authority
        .exchange(SessionExchangeRequest {
            schema: "proof.operator.session.exchange-request/v1".to_owned(),
            challenge_id: challenge.challenge_id,
            client_nonce: nonce.iter().map(|byte| format!("{byte:02x}")).collect(),
        })
        .unwrap();
    let header = response.session_token.header_value();
    let session = authority
        .authorize_any_with(&[header.as_bytes()], |session| {
            Ok::<AuthorizedSession, ()>(session.clone())
        })
        .unwrap();
    (header, session)
}

fn session_header(value: &SessionHeaderValue) -> &str {
    std::str::from_utf8(value.as_bytes()).expect("session header is lowercase ASCII hex")
}

struct TestSigner(SigningKey);

impl ChallengeSigner for TestSigner {
    fn sign_challenge(
        &self,
        challenge: &SessionChallenge,
    ) -> Result<SessionAttestation, TerminalCeremonyError> {
        let bytes =
            challenge_signing_bytes(challenge).map_err(|_| TerminalCeremonyError::SigningFailed)?;
        Ok(SessionAttestation {
            schema: "proof.operator.session.attestation/v1".to_owned(),
            challenge: challenge.clone(),
            signature_algorithm: "ed25519".to_owned(),
            signature: URL_SAFE_NO_PAD.encode(self.0.sign(&bytes).to_bytes()),
            signed_bytes_digest: challenge_signed_bytes_digest(challenge)
                .map_err(|_| TerminalCeremonyError::SigningFailed)?,
        })
    }
}

struct TestTerminal {
    confirmation: Result<String, TerminalCeremonyError>,
    output: String,
    echo: bool,
    fail_restore: bool,
    stages: Option<Arc<Mutex<Vec<&'static str>>>>,
}

impl TestTerminal {
    fn confirming(confirmation: String) -> Self {
        Self {
            confirmation: Ok(confirmation),
            output: String::new(),
            echo: true,
            fail_restore: false,
            stages: None,
        }
    }
}

impl ControllingTerminal for TestTerminal {
    fn write_nonsecret(&mut self, text: &str) -> Result<(), TerminalCeremonyError> {
        self.output.push_str(text);
        Ok(())
    }

    fn set_echo(&mut self, enabled: bool) -> Result<(), TerminalCeremonyError> {
        if enabled && self.fail_restore {
            return Err(TerminalCeremonyError::RestorationFailed);
        }
        self.echo = enabled;
        Ok(())
    }

    fn read_confirmation(
        &mut self,
        _maximum_bytes: usize,
        _timeout: StdDuration,
    ) -> Result<String, TerminalCeremonyError> {
        self.confirmation.clone()
    }
}

impl StartupTerminal for TestTerminal {
    fn verify_restoration(&mut self) -> Result<(), TerminalCeremonyError> {
        if let Some(stages) = &self.stages {
            stages.lock().unwrap().push("tty_verify");
        }
        self.set_echo(false)?;
        self.set_echo(true)
    }
}

fn test_bundle() -> EmbeddedStaticBundle {
    let bytes = Arc::<[u8]>::from(b"<!doctype html><title>Proof</title>".as_slice());
    EmbeddedStaticBundle::from_frozen_manifest(
        vec![StaticManifestEntry::new(
            "/",
            "text/html; charset=utf-8",
            proof_kernel::raw_artifact_sha256(&bytes),
        )],
        vec![StaticSource::new("/", bytes)],
    )
    .unwrap()
}

#[test]
fn os_environment_uses_linux_identity_clock_entropy_and_uuid_v7() {
    let environment = OsOperatorControlEnvironment::new();
    assert_eq!(
        environment.effective_user_id(),
        rustix::process::geteuid().as_raw()
    );
    assert!(environment.trusted_utc_now().is_ok());
    assert!(environment.monotonic_millis().is_ok());
    let mut random = [0_u8; 32];
    environment
        .fill_random(OperatorRandomPurpose::SessionToken, &mut random)
        .unwrap();
    assert_ne!(random, [0; 32]);
    assert_eq!(environment.new_uuid_v7().unwrap().get_version_num(), 7);
    let _ = OsControllingTerminal::open();
}

#[test]
fn cursor_is_scope_bound_tamper_evident_and_restart_local() {
    let environment = Arc::new(TestEnvironment::new(100));
    let codec = ProcessCursorCodec::new(environment.clone()).unwrap();
    let scope = OperatorReadScope {
        schema: OperatorReadScope::SCHEMA.to_owned(),
        workspace_id: uuid_v7(10),
        server_instance_id: uuid_v7(11),
        session_id: uuid_v7(12),
        human_id: uuid_v7(13),
        auth_epoch: 1,
        policy_revision: 1,
        session_absolute_expires_at: environment.trusted_utc_now().unwrap()
            + Duration::seconds(600),
        route: OperatorReadRoute::Attention,
        filter_digest: Some(ControlDigest::from_bytes([2; 32])),
        granted_capabilities: CapabilitySet::all(),
        required_capabilities: vec![Capability::RunRead],
    };
    assert_eq!(
        codec.open_page(scope.clone(), None, 25).unwrap().kind,
        proof_kernel::PageWindowKind::First
    );
    let token = codec
        .seal_page(scope.clone(), 25, 20, 10, uuid_v7(14))
        .unwrap();
    assert_eq!(
        codec
            .open_page(scope.clone(), Some(&token), 25)
            .unwrap()
            .high_water_sequence,
        Some(20)
    );
    let mut tampered = token.clone().into_bytes();
    tampered[0] = if tampered[0] == b'A' { b'B' } else { b'A' };
    assert!(codec
        .open_page(
            scope.clone(),
            Some(std::str::from_utf8(&tampered).unwrap()),
            25,
        )
        .is_err());
    let restarted = ProcessCursorCodec::new(environment).unwrap();
    assert!(restarted.open_page(scope, Some(&token), 25).is_err());
}

#[test]
fn static_manifest_is_independent_digest_bound_and_closed() {
    let script = Arc::<[u8]>::from(b"export const ready = true;".as_slice());
    let digest = proof_kernel::raw_artifact_sha256(&script);
    let digest_hex = digest.encoded().trim_start_matches("sha256:").to_owned();
    let path = format!("/assets/app.{digest_hex}.js");
    let bundle = EmbeddedStaticBundle::from_frozen_manifest(
        vec![
            StaticManifestEntry::new(
                "/",
                "text/html; charset=utf-8",
                proof_kernel::raw_artifact_sha256(b"index"),
            ),
            StaticManifestEntry::new(&path, "application/javascript; charset=utf-8", digest),
        ],
        vec![
            StaticSource::new("/", Arc::<[u8]>::from(b"index".as_slice())),
            StaticSource::new(&path, script.clone()),
        ],
    )
    .unwrap();
    assert_eq!(bundle.paths(), vec!["/".to_owned(), path.clone()]);
    assert!(bundle.asset("/assets/missing.js").is_none());
    assert!(EmbeddedStaticBundle::from_frozen_manifest(
        vec![StaticManifestEntry::new(
            &path,
            "application/javascript; charset=utf-8",
            ArtifactDigest::from_bytes([0; 32]),
        )],
        vec![StaticSource::new(&path, script)],
    )
    .is_err());
    assert_eq!(frozen_route_inventory().len(), 15);
    for forbidden in [
        "/health",
        "/audit",
        "/proofs",
        "/v1/proofs",
        "/v1/operations/:name/:version",
    ] {
        assert!(!frozen_route_inventory()
            .iter()
            .any(|route| route.path == forbidden));
    }
}

struct TestStore {
    workspace: OperatorWorkspace,
    requests: Mutex<Vec<ControlAuditAppendRequest>>,
    stages: Option<Arc<Mutex<Vec<&'static str>>>>,
    drops: Arc<AtomicUsize>,
}

impl Drop for TestStore {
    fn drop(&mut self) {
        self.drops.fetch_add(1, Ordering::SeqCst);
    }
}

impl OperatorDirectoryStore for TestStore {
    fn load_operator_workspace(&self) -> Result<OperatorWorkspace, OperatorStoreError> {
        if let Some(stages) = &self.stages {
            stages.lock().unwrap().push("store_load");
        }
        Ok(self.workspace.clone())
    }

    fn register_governed_run(
        &self,
        _request: RegisterGovernedRunRequest,
    ) -> Result<RegisterGovernedRunResult, OperatorStoreError> {
        Err(OperatorStoreError::Unavailable)
    }
}

impl OperatorAuthorityAuditStore for TestStore {
    fn append_authority_event(
        &self,
        request: ControlAuditAppendRequest,
    ) -> Result<ControlAuditAppendResult, OperatorStoreError> {
        append_test_audit(&self.requests, request)
    }
}

struct TestOpener {
    workspace: OperatorWorkspace,
    seen: Mutex<Vec<(PathBuf, PathBuf, Vec<PathBuf>)>>,
    stages: Option<Arc<Mutex<Vec<&'static str>>>>,
    drops: Arc<AtomicUsize>,
}

impl OperatorStoreOpener for TestOpener {
    type Store = TestStore;

    fn open_existing(
        &self,
        request: &TrustedStoreOpenRequest,
        _environment: Arc<dyn OperatorControlEnvironment>,
    ) -> Result<Self::Store, ControlShellError> {
        if let Some(stages) = &self.stages {
            stages.lock().unwrap().push("store_open");
        }
        self.seen.lock().unwrap().push((
            request.workspace_root().to_owned(),
            request.authoritative_database().to_owned(),
            request.forbidden_proof_directories().to_vec(),
        ));
        Ok(TestStore {
            workspace: self.workspace.clone(),
            requests: Mutex::new(Vec::new()),
            stages: self.stages.clone(),
            drops: self.drops.clone(),
        })
    }
}

fn test_opener(stages: Option<Arc<Mutex<Vec<&'static str>>>>) -> TestOpener {
    TestOpener {
        workspace: workspace_material(CapabilitySet::all()).0,
        seen: Mutex::new(Vec::new()),
        stages,
        drops: Arc::new(AtomicUsize::new(0)),
    }
}

#[test]
fn store_opener_receives_only_build_anchored_existing_paths() {
    let opener = test_opener(None);
    let environment = Arc::new(TestEnvironment::new(100));
    let store =
        open_authoritative_store(Path::new("/tmp/operator-workspace"), environment, &opener)
            .unwrap();
    drop(store);
    let seen = opener.seen.lock().unwrap();
    assert_eq!(seen[0].0, PathBuf::from("/tmp/operator-workspace"));
    assert_eq!(
        seen[0].1,
        PathBuf::from("/tmp/operator-workspace/.proof/storage/storage.db")
    );
    assert_eq!(
        seen[0].2,
        vec![mandatory_repository_proof_directory().unwrap()]
    );
    let repository_root = mandatory_repository_proof_directory()
        .unwrap()
        .parent()
        .unwrap()
        .to_owned();
    drop(seen);
    assert!(open_authoritative_store(
        &repository_root,
        Arc::new(TestEnvironment::new(101)),
        &opener,
    )
    .is_err());
}

struct RecordingStaticBundle {
    inner: EmbeddedStaticBundle,
    stages: Arc<Mutex<Vec<&'static str>>>,
}

impl StaticBundle for RecordingStaticBundle {
    fn validate(&self) -> Result<(), ControlShellError> {
        self.stages.lock().unwrap().push("static");
        self.inner.validate()
    }

    fn asset(&self, path: &str) -> Option<StaticAsset> {
        self.inner.asset(path)
    }

    fn paths(&self) -> Vec<String> {
        self.inner.paths()
    }
}

#[tokio::test]
async fn preflight_orders_checks_and_drops_store_on_intermediate_failure() {
    let stages = Arc::new(Mutex::new(Vec::new()));
    let opener = test_opener(Some(stages.clone()));
    let drops = opener.drops.clone();
    let static_bundle: Arc<dyn StaticBundle> = Arc::new(RecordingStaticBundle {
        inner: test_bundle(),
        stages: stages.clone(),
    });
    let handler = Arc::new(SyntheticRouteHandler::new());
    let result = preflight_control_plane(
        Path::new("/tmp/operator-workspace"),
        static_bundle,
        handler,
        &opener,
        {
            let stages = stages.clone();
            move || {
                stages.lock().unwrap().push("tty_open");
                Ok(TestTerminal {
                    confirmation: Err(TerminalCeremonyError::TimedOut),
                    output: String::new(),
                    echo: true,
                    fail_restore: false,
                    stages: Some(stages),
                })
            }
        },
        {
            let stages = stages.clone();
            move || -> Arc<dyn OperatorControlEnvironment> {
                Arc::new(TestEnvironment::recording(300, stages))
            }
        },
        {
            let stages = stages.clone();
            move || async move {
                stages.lock().unwrap().push("bind");
                Err(ControlShellError::ListenerUnavailable)
            }
        },
    )
    .await;
    assert!(matches!(
        result,
        Err(ControlShellError::ListenerUnavailable)
    ));
    assert_eq!(drops.load(Ordering::SeqCst), 1);
    assert_eq!(
        *stages.lock().unwrap(),
        [
            "static",
            "tty_open",
            "tty_verify",
            "clock",
            "tick",
            "uuid",
            "random",
            "random",
            "random",
            "store_open",
            "store_load",
            "bind",
        ]
    );

    let relative_opener = test_opener(None);
    let relative = preflight_os_control_plane(
        Path::new("relative"),
        Arc::new(test_bundle()),
        Arc::new(SyntheticRouteHandler::new()),
        &relative_opener,
    )
    .await;
    assert!(matches!(relative, Err(ControlShellError::UnsafeWorkspace)));
}

#[tokio::test]
async fn successful_preflight_publishes_one_origin_and_restart_fresh_authority() {
    let first_environment = Arc::new(TestEnvironment::new(500));
    let first_environment_object: Arc<dyn OperatorControlEnvironment> = first_environment.clone();
    let first_opener = test_opener(None);
    let mut first = preflight_control_plane(
        Path::new("/tmp/operator-workspace-one"),
        Arc::new(test_bundle()),
        Arc::new(SyntheticRouteHandler::new()),
        &first_opener,
        || Ok(TestTerminal::confirming(String::new())),
        move || first_environment_object,
        || LoopbackListener::bind(),
    )
    .await
    .unwrap();
    assert!(first.clean_url().starts_with("http://127.0.0.1:"));
    assert_eq!(first.clean_url().trim_end_matches('/'), first.origin());
    drop(first.router());
    drop(first.store());
    first.terminal_mut().write_nonsecret("ready").unwrap();
    let signing_key = SigningKey::from_bytes(&[7; 32]);
    let (old_token, session) =
        establish_authority_session(&first.authority(), &signing_key, [40; 32]);
    let scope = OperatorReadScope {
        schema: OperatorReadScope::SCHEMA.to_owned(),
        workspace_id: session.workspace_id,
        server_instance_id: session.server_instance_id,
        session_id: session.session_id,
        human_id: session.human_id,
        auth_epoch: session.auth_epoch,
        policy_revision: session.policy_revision,
        session_absolute_expires_at: session.absolute_expires_at,
        route: OperatorReadRoute::Audit,
        filter_digest: Some(ControlDigest::from_bytes([41; 32])),
        granted_capabilities: session.granted_capabilities.clone(),
        required_capabilities: vec![Capability::AuditRead],
    };
    let cursor = first
        .cursor()
        .seal_page(scope.clone(), 25, 20, 10, uuid_v7(42))
        .unwrap();

    let second_environment = Arc::new(TestEnvironment::new(600));
    let second_environment_object: Arc<dyn OperatorControlEnvironment> = second_environment.clone();
    let second_opener = test_opener(None);
    let second = preflight_control_plane(
        Path::new("/tmp/operator-workspace-two"),
        Arc::new(test_bundle()),
        Arc::new(SyntheticRouteHandler::new()),
        &second_opener,
        || Ok(TestTerminal::confirming(String::new())),
        move || second_environment_object,
        || LoopbackListener::bind(),
    )
    .await
    .unwrap();
    assert_ne!(first.server_instance_id(), second.server_instance_id());
    assert_ne!(first.origin(), second.origin());
    assert!(second
        .authority()
        .authorize_any_with(&[old_token.as_bytes()], |_| Ok::<_, ()>(()))
        .is_err());
    assert!(second.cursor().open_page(scope, Some(&cursor), 25).is_err());
    first.serve_until(async {}).await.unwrap();
    second.serve_until(async {}).await.unwrap();
}

#[test]
fn ceremony_is_one_attempt_timeout_and_restoration_fail_closed() {
    let harness = AuthHarness::new("http://127.0.0.1:43121", CapabilitySet::all(), 2);
    let nonce = [5_u8; 32];
    let challenge = harness.challenge(&nonce);
    let code = challenge_code(&challenge).unwrap();
    let mut terminal = TestTerminal::confirming(format!("AUTHORIZE {code}\n"));
    complete_challenge_ceremony(
        &harness.authority,
        &challenge,
        &mut terminal,
        &TestSigner(harness.signing_key.clone()),
    )
    .unwrap();
    assert!(terminal.echo);
    assert!(!terminal.output.contains(&code));
    assert!(harness
        .authority
        .exchange(SessionExchangeRequest {
            schema: "proof.operator.session.exchange-request/v1".to_owned(),
            challenge_id: challenge.challenge_id,
            client_nonce: nonce.iter().map(|byte| format!("{byte:02x}")).collect(),
        })
        .is_ok());

    let challenge = harness.challenge(&[6; 32]);
    let mut timeout = TestTerminal {
        confirmation: Err(TerminalCeremonyError::TimedOut),
        output: String::new(),
        echo: true,
        fail_restore: false,
        stages: None,
    };
    assert_eq!(
        complete_challenge_ceremony_with_timeout(
            &harness.authority,
            &challenge,
            &mut timeout,
            &TestSigner(harness.signing_key.clone()),
            StdDuration::from_millis(1),
        ),
        Err(TerminalCeremonyError::TimedOut)
    );
    assert!(timeout.echo);

    let challenge = harness.challenge(&[7; 32]);
    let mut failed_restore = TestTerminal::confirming("bad\n".to_owned());
    failed_restore.fail_restore = true;
    assert_eq!(
        complete_challenge_ceremony(
            &harness.authority,
            &challenge,
            &mut failed_restore,
            &TestSigner(harness.signing_key.clone()),
        ),
        Err(TerminalCeremonyError::RestorationFailed)
    );
    assert!(harness
        .authority
        .exchange(SessionExchangeRequest {
            schema: "proof.operator.session.exchange-request/v1".to_owned(),
            challenge_id: challenge.challenge_id,
            client_nonce: "07".repeat(32),
        })
        .is_err());
}

#[test]
fn descriptor_human_signer_revalidates_exact_tuple() {
    let harness = AuthHarness::new("http://127.0.0.1:43121", CapabilitySet::all(), 2);
    let challenge = harness.challenge(&[8; 32]);
    let directory = std::env::temp_dir().join(format!("proof-human-{}", uuid_v7(8_000)));
    fs::create_dir(&directory).unwrap();
    fs::set_permissions(&directory, fs::Permissions::from_mode(0o700)).unwrap();
    let path = directory.join(format!("{}.json", challenge.human_id));
    let key_document = serde_json::json!({
        "principal_id": challenge.human_id,
        "kind": "human",
        "created_at": "2032-01-01T00:00:00Z",
        "public_key": harness.signing_key.verifying_key().to_bytes(),
        "signing_key": STANDARD.encode(harness.signing_key.to_bytes()),
    });
    fs::write(&path, serde_json::to_vec(&key_document).unwrap()).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    let metadata = fs::metadata(&path).unwrap();
    let human = PrincipalBinding {
        principal_id: PrincipalId::new(challenge.human_id),
        kind: PrincipalKind::Human,
        public_key: URL_SAFE_NO_PAD.encode(harness.signing_key.verifying_key().as_bytes()),
        public_key_fingerprint: challenge.human_public_key_fingerprint,
    };
    let signer = DescriptorHumanChallengeSigner::new(
        File::open(&directory).unwrap(),
        DescriptorIdentity {
            device: metadata.dev(),
            inode: metadata.ino(),
        },
        human,
    )
    .unwrap();
    assert!(signer.sign_challenge(&challenge).is_ok());
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
    assert_eq!(
        signer.sign_challenge(&challenge),
        Err(TerminalCeremonyError::SigningFailed)
    );
    fs::remove_file(path).unwrap();
    fs::remove_dir(directory).unwrap();
}

struct ShutdownLog {
    entries: Arc<Mutex<Vec<&'static str>>>,
    fail_at: Option<&'static str>,
}

impl ShutdownLog {
    fn step(&self, name: &'static str) -> Result<(), ControlShellError> {
        self.entries.lock().unwrap().push(name);
        if self.fail_at == Some(name) {
            Err(ControlShellError::ControlUnavailable)
        } else {
            Ok(())
        }
    }
}

impl ShutdownCoordinator for ShutdownLog {
    fn stop_listener_accepts(&mut self) -> Result<(), ControlShellError> {
        self.step("listener")
    }

    fn stop_new_permits(&mut self) -> Result<(), ControlShellError> {
        self.step("permits")
    }

    fn drain_mutations(&mut self) -> Result<(), ControlShellError> {
        self.step("drain")
    }

    fn release_pre_dispatch_reservations(&mut self) -> Result<(), ControlShellError> {
        self.step("reservations")
    }

    fn checkpoint_durable_work(&mut self) -> Result<(), ControlShellError> {
        self.step("checkpoint")
    }

    fn zeroize_runtime_custody(&mut self) -> Result<(), ControlShellError> {
        self.step("custody")
    }

    fn zeroize_cursor_and_signers(&mut self) -> Result<(), ControlShellError> {
        self.step("cursor_signers")
    }

    fn close_trusted_store(&mut self) -> Result<(), ControlShellError> {
        self.step("store")
    }

    fn release_workspace_lock(&mut self) -> Result<(), ControlShellError> {
        self.step("lock")
    }
}

#[test]
fn signals_share_ordered_shutdown_and_cleanup_tail_survives_failure() {
    for (index, signal) in [ControlSignal::Interrupt, ControlSignal::Terminate]
        .into_iter()
        .enumerate()
    {
        let harness = AuthHarness::new(
            "http://127.0.0.1:43121",
            CapabilitySet::all(),
            20 + index as u64,
        );
        let (token, _) = harness.establish_session([30 + index as u8; 32]);
        let entries = Arc::new(Mutex::new(Vec::new()));
        let result = shutdown_for_signal(
            signal,
            &harness.authority,
            &mut ShutdownLog {
                entries: entries.clone(),
                fail_at: Some("drain"),
            },
        );
        assert_eq!(result, Err(ControlShellError::ControlUnavailable));
        assert_eq!(
            *entries.lock().unwrap(),
            [
                "listener",
                "permits",
                "drain",
                "reservations",
                "checkpoint",
                "custody",
                "cursor_signers",
                "store",
                "lock",
            ]
        );
        assert_eq!(
            harness.audit.requests.lock().unwrap().last().unwrap().kind,
            ControlAuthorityEventKind::ControlShutdown
        );
        assert!(harness
            .authority
            .authorize_any_with(&[token.as_bytes()], |_| Ok::<_, ()>(()))
            .is_err());
    }
}

fn router_harness(
    capabilities: CapabilitySet,
    handler: Arc<dyn OperatorRouteHandler>,
) -> (OperatorRouterState, AuthHarness) {
    let endpoint = LoopbackOrigin::for_test(43121);
    let harness = AuthHarness::new(endpoint.origin(), capabilities, 2);
    let state = OperatorRouterState::new(
        endpoint,
        Arc::new(test_bundle()),
        handler,
        harness.authority.clone(),
        harness.environment.clone(),
    )
    .unwrap();
    (state, harness)
}

fn control_request(
    method: Method,
    target: &str,
    host: Option<&str>,
    peer: Option<SocketAddr>,
    headers: &[(&str, &str)],
    body: Vec<u8>,
) -> Request<Body> {
    let mut request = Request::builder()
        .method(method)
        .uri(target)
        .body(Body::from(body))
        .unwrap();
    if let Some(host) = host {
        request
            .headers_mut()
            .insert(header::HOST, HeaderValue::from_str(host).unwrap());
    }
    for (name, value) in headers {
        request.headers_mut().append(
            header::HeaderName::from_bytes(name.as_bytes()).unwrap(),
            HeaderValue::from_str(value).unwrap(),
        );
    }
    if let Some(peer) = peer {
        request.extensions_mut().insert(ConnectInfo(peer));
    }
    request
}

fn loopback_peer() -> SocketAddr {
    SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 54_321)
}

async fn response_body(response: Response<Body>) -> Vec<u8> {
    to_bytes(response.into_body(), 32 * 1024)
        .await
        .unwrap()
        .to_vec()
}

fn assert_security_headers(response: &Response<Body>, html: bool) {
    let headers = response.headers();
    assert_eq!(headers.get(header::CACHE_CONTROL).unwrap(), "no-store");
    assert_eq!(headers.get("x-content-type-options").unwrap(), "nosniff");
    assert_eq!(headers.get("referrer-policy").unwrap(), "no-referrer");
    assert_eq!(headers.get("x-frame-options").unwrap(), "DENY");
    assert_eq!(
        headers.get("cross-origin-opener-policy").unwrap(),
        "same-origin"
    );
    assert_eq!(
        headers.get("cross-origin-resource-policy").unwrap(),
        "same-origin"
    );
    assert_eq!(
        headers.get("permissions-policy").unwrap(),
        "camera=(), microphone=(), geolocation=(), payment=(), usb=()"
    );
    if html {
        assert_eq!(
            headers.get("content-security-policy").unwrap(),
            "default-src 'none'; script-src 'self'; style-src 'self'; connect-src 'self'; img-src 'self'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'; object-src 'none'"
        );
    } else {
        assert!(!headers.contains_key("content-security-policy"));
    }
    for forbidden in [
        header::SET_COOKIE.as_str(),
        "access-control-allow-origin",
        header::EXPIRES.as_str(),
        header::PRAGMA.as_str(),
    ] {
        assert!(!headers.contains_key(forbidden));
    }
}

#[tokio::test]
async fn request_boundary_is_peer_host_target_and_auth_first() {
    let handler = Arc::new(SyntheticRouteHandler::new());
    let (state, _) = router_harness(CapabilitySet::all(), handler.clone());
    for request in [
        control_request(
            Method::GET,
            "/operator/v1/audit",
            Some("127.0.0.1:43121"),
            None,
            &[("x-forwarded-for", "127.0.0.1")],
            Vec::new(),
        ),
        control_request(
            Method::GET,
            "/operator/v1/audit",
            Some("localhost:43121"),
            Some(loopback_peer()),
            &[],
            Vec::new(),
        ),
        control_request(
            Method::GET,
            "/operator/v1/audit",
            Some("127.0.0.1:43121"),
            Some(SocketAddr::new(IpAddr::V6(Ipv6Addr::LOCALHOST), 54_321)),
            &[],
            Vec::new(),
        ),
        control_request(
            Method::GET,
            "/operator/v1/audit",
            Some("127.0.0.1:43121"),
            Some(loopback_peer()),
            &[("host", "127.0.0.1:43121")],
            Vec::new(),
        ),
    ] {
        let response = dispatch_for_test(state.clone(), request).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        assert_security_headers(&response, false);
    }
    let long_target = format!("/{}", "a".repeat(2_049));
    let response = dispatch_for_test(
        state.clone(),
        control_request(
            Method::GET,
            &long_target,
            Some("127.0.0.1:43121"),
            Some(loopback_peer()),
            &[],
            Vec::new(),
        ),
    )
    .await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    let html = dispatch_for_test(
        state.clone(),
        control_request(
            Method::GET,
            "/",
            Some("127.0.0.1:43121"),
            Some(loopback_peer()),
            &[],
            Vec::new(),
        ),
    )
    .await;
    assert_eq!(html.status(), StatusCode::OK);
    assert_security_headers(&html, true);
    let static_query = dispatch_for_test(
        state.clone(),
        control_request(
            Method::GET,
            "/?unexpected=1",
            Some("127.0.0.1:43121"),
            Some(loopback_peer()),
            &[],
            Vec::new(),
        ),
    )
    .await;
    assert_eq!(static_query.status(), StatusCode::BAD_REQUEST);
    assert_security_headers(&static_query, false);

    let response = dispatch_for_test(
        state,
        control_request(
            Method::POST,
            "/operator/v1/runs/not-an-id/cancel",
            Some("127.0.0.1:43121"),
            Some(loopback_peer()),
            &[
                ("origin", "http://evil.invalid"),
                ("content-type", "text/plain"),
            ],
            vec![0; 9_000],
        ),
    )
    .await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(
        handler.effect_snapshot(),
        SyntheticEffectSnapshot::default()
    );
}

#[tokio::test]
async fn least_capability_precedes_target_and_handler() {
    let handler = Arc::new(SyntheticRouteHandler::new());
    let capabilities = CapabilitySet::new(vec![Capability::RunRead]).unwrap();
    let (state, harness) = router_harness(capabilities, handler.clone());
    let (token, _) = harness.establish_session([9; 32]);
    let duplicated = dispatch_for_test(
        state.clone(),
        control_request(
            Method::GET,
            "/operator/v1/audit?bad-query",
            Some("127.0.0.1:43121"),
            Some(loopback_peer()),
            &[
                ("x-proof-operator-session", session_header(&token)),
                ("x-proof-operator-session", session_header(&token)),
            ],
            Vec::new(),
        ),
    )
    .await;
    assert_eq!(duplicated.status(), StatusCode::UNAUTHORIZED);
    let attention = dispatch_for_test(
        state.clone(),
        control_request(
            Method::GET,
            "/operator/v1/attention?schema=proof.operator.attention-query%2Fv1&kinds=approval&bad=hidden",
            Some("127.0.0.1:43121"),
            Some(loopback_peer()),
            &[("x-proof-operator-session", session_header(&token))],
            Vec::new(),
        ),
    )
    .await;
    assert_eq!(attention.status(), StatusCode::FORBIDDEN);
    let attention_with_deferred_malformed_tail = dispatch_for_test(
        state.clone(),
        control_request(
            Method::GET,
            "/operator/v1/attention?schema=proof.operator.attention-query%2Fv1&kinds=approval&broken",
            Some("127.0.0.1:43121"),
            Some(loopback_peer()),
            &[("x-proof-operator-session", session_header(&token))],
            Vec::new(),
        ),
    )
    .await;
    assert_eq!(
        attention_with_deferred_malformed_tail.status(),
        StatusCode::FORBIDDEN
    );
    let response = dispatch_for_test(
        state,
        control_request(
            Method::GET,
            "/operator/v1/audit?bad-query",
            Some("127.0.0.1:43121"),
            Some(loopback_peer()),
            &[("x-proof-operator-session", session_header(&token))],
            Vec::new(),
        ),
    )
    .await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert_eq!(
        handler.effect_snapshot(),
        SyntheticEffectSnapshot::default()
    );
}

#[tokio::test]
async fn mutation_origin_media_body_and_decode_precede_handler() {
    let handler = Arc::new(SyntheticRouteHandler::new());
    let (state, harness) = router_harness(CapabilitySet::all(), handler.clone());
    let (token, _) = harness.establish_session([10; 32]);
    let cases = [
        (
            vec![("x-proof-operator-session", session_header(&token))],
            b"{}".to_vec(),
            StatusCode::BAD_REQUEST,
        ),
        (
            vec![
                ("x-proof-operator-session", session_header(&token)),
                ("origin", "http://127.0.0.1:43121"),
                ("origin", "http://127.0.0.1:43121"),
                ("content-type", "application/json"),
            ],
            b"{}".to_vec(),
            StatusCode::BAD_REQUEST,
        ),
        (
            vec![
                ("x-proof-operator-session", session_header(&token)),
                ("origin", "http://127.0.0.1:43121"),
                ("content-type", "application/json"),
                ("content-type", "application/json"),
            ],
            b"{}".to_vec(),
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
        ),
        (
            vec![
                ("x-proof-operator-session", session_header(&token)),
                ("origin", "http://127.0.0.1:43121"),
                ("content-type", "text/plain"),
            ],
            b"{}".to_vec(),
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
        ),
        (
            vec![
                ("x-proof-operator-session", session_header(&token)),
                ("origin", "http://127.0.0.1:43121"),
                ("content-type", "application/json"),
            ],
            vec![b'x'; 8_193],
            StatusCode::PAYLOAD_TOO_LARGE,
        ),
        (
            vec![
                ("x-proof-operator-session", session_header(&token)),
                ("origin", "http://127.0.0.1:43121"),
                ("content-type", "application/json"),
            ],
            br#"{"schema":"x","schema":"y"}"#.to_vec(),
            StatusCode::BAD_REQUEST,
        ),
    ];
    for (headers, body, expected) in cases {
        let response = dispatch_for_test(
            state.clone(),
            control_request(
                Method::POST,
                "/operator/v1/session/revoke",
                Some("127.0.0.1:43121"),
                Some(loopback_peer()),
                &headers,
                body,
            ),
        )
        .await;
        assert_eq!(response.status(), expected);
        assert_security_headers(&response, false);
    }
    assert_eq!(
        handler.effect_snapshot(),
        SyntheticEffectSnapshot::default()
    );
}

struct HeaderInjectingHandler {
    calls: AtomicUsize,
}

impl OperatorRouteHandler for HeaderInjectingHandler {
    fn handle(&self, _request: ProtectedRequest) -> Result<Response<Body>, ControlShellError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::SET_COOKIE, "authority=forbidden")
            .header("access-control-allow-origin", "*")
            .header(header::CACHE_CONTROL, "public")
            .header(header::EXPIRES, "tomorrow")
            .body(Body::from("{}"))
            .unwrap())
    }
}

#[tokio::test]
async fn protected_reload_revoke_restart_and_response_hardening_are_exact() {
    let handler = Arc::new(HeaderInjectingHandler {
        calls: AtomicUsize::new(0),
    });
    let (state, harness) = router_harness(CapabilitySet::all(), handler.clone());
    let (token, session) = harness.establish_session([11; 32]);
    let audit_target =
        "/operator/v1/audit?schema=proof.operator.audit-query%2Fv1&kinds=control_shutdown&page_size=25";
    for _ in 0..2 {
        let response = dispatch_for_test(
            state.clone(),
            control_request(
                Method::GET,
                audit_target,
                Some("127.0.0.1:43121"),
                Some(loopback_peer()),
                &[("x-proof-operator-session", session_header(&token))],
                Vec::new(),
            ),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_security_headers(&response, false);
    }
    assert_eq!(handler.calls.load(Ordering::SeqCst), 2);

    let command = OperatorCommand::SessionRevoke(SessionRevokeRequest {
        schema: SessionRevokeRequest::SCHEMA.to_owned(),
        binding: CommandBinding {
            command_id: uuid_v7(50),
            idempotency_key: uuid_v7(51),
            workspace_id: session.workspace_id,
            server_instance_id: session.server_instance_id,
            session_id: session.session_id,
            human_id: session.human_id,
            auth_epoch: session.auth_epoch,
            session_authority_digest: session.authority_digest,
            policy_revision: session.policy_revision,
        },
    });
    let mut invalid_command = command.clone();
    let OperatorCommand::SessionRevoke(invalid_revoke) = &mut invalid_command else {
        unreachable!("the fixture is a session revoke");
    };
    invalid_revoke.schema = "not-the-revoke-schema".to_owned();
    let response = dispatch_for_test(
        state.clone(),
        control_request(
            Method::POST,
            "/operator/v1/session/revoke?unexpected=1",
            Some("127.0.0.1:43121"),
            Some(loopback_peer()),
            &[
                ("x-proof-operator-session", session_header(&token)),
                ("origin", "http://127.0.0.1:43121"),
                ("content-type", "application/json"),
            ],
            serde_json::to_vec(&command).unwrap(),
        ),
    )
    .await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(handler.calls.load(Ordering::SeqCst), 2);
    let response = dispatch_for_test(
        state.clone(),
        control_request(
            Method::POST,
            "/operator/v1/session/revoke",
            Some("127.0.0.1:43121"),
            Some(loopback_peer()),
            &[
                ("x-proof-operator-session", session_header(&token)),
                ("origin", "http://127.0.0.1:43121"),
                ("content-type", "application/json"),
            ],
            serde_json::to_vec(&invalid_command).unwrap(),
        ),
    )
    .await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(handler.calls.load(Ordering::SeqCst), 2);
    let response = dispatch_for_test(
        state.clone(),
        control_request(
            Method::POST,
            "/operator/v1/session/revoke",
            Some("127.0.0.1:43121"),
            Some(loopback_peer()),
            &[
                ("x-proof-operator-session", session_header(&token)),
                ("origin", "http://127.0.0.1:43121"),
                ("content-type", "application/json"),
            ],
            serde_json::to_vec(&command).unwrap(),
        ),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_security_headers(&response, false);
    let response = dispatch_for_test(
        state,
        control_request(
            Method::GET,
            audit_target,
            Some("127.0.0.1:43121"),
            Some(loopback_peer()),
            &[("x-proof-operator-session", session_header(&token))],
            Vec::new(),
        ),
    )
    .await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    let restarted = AuthHarness::new("http://127.0.0.1:43121", CapabilitySet::all(), 99);
    assert!(restarted
        .authority
        .authorize_any_with(&[token.as_bytes()], |_| Ok::<_, ()>(()))
        .is_err());
}

#[tokio::test]
async fn challenge_exchange_replay_expiry_and_rates_fail_closed() {
    let handler = Arc::new(SyntheticRouteHandler::new());
    let (state, harness) = router_harness(CapabilitySet::all(), handler.clone());
    let issue = ChallengeIssueRequest {
        schema: "proof.operator.session.challenge-issue-request/v1".to_owned(),
        client_nonce_digest: client_nonce_digest(&[12; 32]),
        requested_capabilities: CapabilitySet::all(),
    };
    let request = || {
        control_request(
            Method::POST,
            "/operator/v1/session/challenges",
            Some("127.0.0.1:43121"),
            Some(loopback_peer()),
            &[
                ("origin", "http://127.0.0.1:43121"),
                ("content-type", "application/json"),
            ],
            serde_json::to_vec(&issue).unwrap(),
        )
    };
    let bad_origin = dispatch_for_test(
        state.clone(),
        control_request(
            Method::POST,
            "/operator/v1/session/challenges",
            Some("127.0.0.1:43121"),
            Some(loopback_peer()),
            &[
                ("origin", "http://evil.invalid"),
                ("content-type", "application/json"),
            ],
            serde_json::to_vec(&issue).unwrap(),
        ),
    )
    .await;
    assert_eq!(bad_origin.status(), StatusCode::BAD_REQUEST);
    let bad_media = dispatch_for_test(
        state.clone(),
        control_request(
            Method::POST,
            "/operator/v1/session/challenges",
            Some("127.0.0.1:43121"),
            Some(loopback_peer()),
            &[
                ("origin", "http://127.0.0.1:43121"),
                ("content-type", "application/json; charset=utf-8"),
            ],
            serde_json::to_vec(&issue).unwrap(),
        ),
    )
    .await;
    assert_eq!(bad_media.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);
    let unexpected_query = dispatch_for_test(
        state.clone(),
        control_request(
            Method::POST,
            "/operator/v1/session/challenges?unexpected=1",
            Some("127.0.0.1:43121"),
            Some(loopback_peer()),
            &[
                ("origin", "http://127.0.0.1:43121"),
                ("content-type", "application/json"),
            ],
            serde_json::to_vec(&issue).unwrap(),
        ),
    )
    .await;
    assert_eq!(unexpected_query.status(), StatusCode::BAD_REQUEST);
    let too_large = dispatch_for_test(
        state.clone(),
        control_request(
            Method::POST,
            "/operator/v1/session/challenges",
            Some("127.0.0.1:43121"),
            Some(loopback_peer()),
            &[
                ("origin", "http://127.0.0.1:43121"),
                ("content-type", "application/json"),
            ],
            vec![b'x'; 4_097],
        ),
    )
    .await;
    assert_eq!(too_large.status(), StatusCode::PAYLOAD_TOO_LARGE);
    let first = dispatch_for_test(state.clone(), request()).await;
    assert_eq!(first.status(), StatusCode::CREATED);
    let first: ChallengeIssueResponse =
        serde_json::from_slice(&response_body(first).await).unwrap();
    let second = dispatch_for_test(state.clone(), request()).await;
    assert_eq!(second.status(), StatusCode::SERVICE_UNAVAILABLE);
    harness
        .authority
        .submit_attestation(
            TestSigner(harness.signing_key.clone())
                .sign_challenge(&first.challenge)
                .unwrap(),
        )
        .unwrap();
    let exchange_body = serde_json::to_vec(&SessionExchangeRequest {
        schema: "proof.operator.session.exchange-request/v1".to_owned(),
        challenge_id: first.challenge.challenge_id,
        client_nonce: "0c".repeat(32),
    })
    .unwrap();
    let exchange = || {
        control_request(
            Method::POST,
            "/operator/v1/session/exchange",
            Some("127.0.0.1:43121"),
            Some(loopback_peer()),
            &[
                ("origin", "http://127.0.0.1:43121"),
                ("content-type", "application/json"),
            ],
            exchange_body.clone(),
        )
    };
    let issued = dispatch_for_test(state.clone(), exchange()).await;
    assert_eq!(issued.status(), StatusCode::CREATED);
    drop(response_body(issued).await);
    let replayed = dispatch_for_test(state.clone(), exchange()).await;
    assert_eq!(replayed.status(), StatusCode::UNAUTHORIZED);
    for index in 0..=8 {
        let response = dispatch_for_test(state.clone(), exchange()).await;
        assert_eq!(
            response.status(),
            if index < 8 {
                StatusCode::UNAUTHORIZED
            } else {
                StatusCode::TOO_MANY_REQUESTS
            }
        );
    }

    let malformed = dispatch_for_test(
        state.clone(),
        control_request(
            Method::POST,
            "/operator/v1/session/challenges",
            Some("127.0.0.1:43121"),
            Some(loopback_peer()),
            &[
                ("origin", "http://127.0.0.1:43121"),
                ("content-type", "application/json"),
            ],
            br#"{"schema":"x","nested":{"a":1,"a":2}}"#.to_vec(),
        ),
    )
    .await;
    assert_eq!(malformed.status(), StatusCode::BAD_REQUEST);

    let nonce = [13; 32];
    let challenge = harness.challenge(&nonce);
    let attestation = TestSigner(harness.signing_key.clone())
        .sign_challenge(&challenge)
        .unwrap();
    harness.authority.submit_attestation(attestation).unwrap();
    harness.environment.advance(Duration::seconds(120));
    assert!(harness
        .authority
        .exchange(SessionExchangeRequest {
            schema: "proof.operator.session.exchange-request/v1".to_owned(),
            challenge_id: challenge.challenge_id,
            client_nonce: "0d".repeat(32),
        })
        .is_err());

    let (_, expiry_harness) =
        router_harness(CapabilitySet::all(), Arc::new(SyntheticRouteHandler::new()));
    let (token, _) = expiry_harness.establish_session([14; 32]);
    expiry_harness.environment.advance(Duration::seconds(300));
    assert!(expiry_harness
        .authority
        .authorize_any_with(&[token.as_bytes()], |_| Ok::<_, ()>(()))
        .is_err());

    let (rate_state, rate_harness) =
        router_harness(CapabilitySet::all(), Arc::new(SyntheticRouteHandler::new()));
    let (rate_token, _) = rate_harness.establish_session([15; 32]);
    for index in 0..=120 {
        let response = dispatch_for_test(
            rate_state.clone(),
            control_request(
                Method::GET,
                "/operator/v1/unknown",
                Some("127.0.0.1:43121"),
                Some(loopback_peer()),
                &[("x-proof-operator-session", session_header(&rate_token))],
                Vec::new(),
            ),
        )
        .await;
        assert_eq!(
            response.status(),
            if index < 120 {
                StatusCode::NOT_FOUND
            } else {
                StatusCode::TOO_MANY_REQUESTS
            }
        );
    }
    assert_eq!(
        handler.effect_snapshot(),
        SyntheticEffectSnapshot::default()
    );
}

#[test]
fn concurrent_challenge_has_one_winner_and_lost_exchange_cannot_replay() {
    let harness = Arc::new(AuthHarness::new(
        "http://127.0.0.1:43121",
        CapabilitySet::all(),
        55,
    ));
    let barrier = Arc::new(std::sync::Barrier::new(3));
    let mut threads = Vec::new();
    for nonce in [[16_u8; 32], [17_u8; 32]] {
        let harness = harness.clone();
        let barrier = barrier.clone();
        threads.push(std::thread::spawn(move || {
            barrier.wait();
            harness.authority.issue_challenge(ChallengeIssueRequest {
                schema: "proof.operator.session.challenge-issue-request/v1".to_owned(),
                client_nonce_digest: client_nonce_digest(&nonce),
                requested_capabilities: CapabilitySet::all(),
            })
        }));
    }
    barrier.wait();
    let outcomes: Vec<_> = threads
        .into_iter()
        .map(|thread| thread.join().unwrap())
        .collect();
    assert_eq!(outcomes.iter().filter(|outcome| outcome.is_ok()).count(), 1);
    let winner = outcomes.into_iter().find_map(Result::ok).unwrap().challenge;
    let nonce = if winner.client_nonce_digest == client_nonce_digest(&[16; 32]) {
        [16; 32]
    } else {
        [17; 32]
    };
    harness
        .authority
        .submit_attestation(
            TestSigner(harness.signing_key.clone())
                .sign_challenge(&winner)
                .unwrap(),
        )
        .unwrap();
    let nonce_hex: String = nonce.iter().map(|byte| format!("{byte:02x}")).collect();
    drop(
        harness
            .authority
            .exchange(SessionExchangeRequest {
                schema: "proof.operator.session.exchange-request/v1".to_owned(),
                challenge_id: winner.challenge_id,
                client_nonce: nonce_hex.clone(),
            })
            .unwrap(),
    );
    assert!(harness
        .authority
        .exchange(SessionExchangeRequest {
            schema: "proof.operator.session.exchange-request/v1".to_owned(),
            challenge_id: winner.challenge_id,
            client_nonce: nonce_hex,
        })
        .is_err());
}

#[tokio::test]
async fn listener_bind_port_and_real_socket_router_are_loopback_only() {
    let occupied = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let occupied_address = occupied.local_addr().unwrap();
    assert!(matches!(
        LoopbackListener::bind_for_test(occupied_address).await,
        Err(ControlShellError::ListenerUnavailable)
    ));
    drop(occupied);

    let listener = LoopbackListener::bind().await.unwrap();
    let endpoint = listener.origin();
    assert_eq!(endpoint.address().ip(), IpAddr::V4(Ipv4Addr::LOCALHOST));
    assert_ne!(endpoint.address().port(), 0);
    let harness = AuthHarness::new(endpoint.origin(), CapabilitySet::all(), 2);
    let handler = Arc::new(SyntheticRouteHandler::new());
    let state = OperatorRouterState::new(
        endpoint.clone(),
        Arc::new(test_bundle()),
        handler.clone(),
        harness.authority,
        harness.environment,
    )
    .unwrap();
    let router = build_operator_router(state);
    let address = endpoint.address();
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(listener.serve_until(router, async move {
        let _ = shutdown_rx.await;
    }));

    for (target, expected) in [
        ("/v1/proofs", "HTTP/1.1 404"),
        ("/operator/v1/audit", "HTTP/1.1 401"),
    ] {
        let mut connection = tokio::net::TcpStream::connect(address).await.unwrap();
        connection
            .write_all(
                format!(
                    "GET {target} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n\r\n",
                    endpoint.host()
                )
                .as_bytes(),
            )
            .await
            .unwrap();
        let mut response = Vec::new();
        connection.read_to_end(&mut response).await.unwrap();
        let response = String::from_utf8(response).unwrap();
        assert!(response.starts_with(expected));
        let lower = response.to_ascii_lowercase();
        assert!(lower.contains("cache-control: no-store"));
        assert!(lower.contains("x-frame-options: deny"));
    }
    assert_eq!(
        handler.effect_snapshot(),
        SyntheticEffectSnapshot::default()
    );
    shutdown_tx.send(()).unwrap();
    server.await.unwrap().unwrap();
}

#[test]
fn artifact_digest_type_remains_algorithm_distinct() {
    let digest = proof_kernel::raw_artifact_sha256(b"asset");
    let encoded = digest.to_string();
    assert!(encoded.starts_with("sha256:"));
    assert!(encoded.parse::<ArtifactDigest>().is_ok());
}
