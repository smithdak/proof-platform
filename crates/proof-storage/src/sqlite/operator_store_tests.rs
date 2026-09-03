use super::*;
use crate::sqlite::{migrations::run_migrations_through, store::SqliteStore};
use ed25519_dalek::SigningKey;
use proof_kernel::{
    control_digest, AgentCheckpoint, BoundaryKind, BudgetPolicy, BudgetSettlementDisposition,
    CapabilitySet, DescriptorIdentity, DispatchIntent, DispatchTokenCustody, Governance,
    LeaseTokenCustody, OperatorDirectoryStore, OperatorSchemaCatalog, OperatorSchemaSource,
    OperatorSchemaSourceInventory, PrincipalBinding, RecordingOperatorControlEnvironment,
    RegistryEntry, RuntimeFailureBody, RuntimeFailureClassification, RuntimeFailureCode,
    VersionStatus, WorkspaceFingerprintInput,
};
use rusqlite::{Connection, TransactionBehavior};
use serde_json::json as json_value;
use std::{fs::File, path::Path, sync::Arc};

const LEASE_TOKEN: [u8; 32] = [0x31; 32];
const DISPATCH_TOKEN: [u8; 32] = [0x42; 32];

#[derive(Debug, PartialEq)]
struct SettlementState {
    reservation: BudgetReservation,
    budget: BudgetAccount,
    control: RunControl,
}

fn id(suffix: u8) -> Uuid {
    let mut bytes = [0_u8; 16];
    bytes[..6].copy_from_slice(&[0x01, 0x94, 0x1f, 0x29, 0x7c, 0x00]);
    bytes[6] = 0x70;
    bytes[8] = 0x80;
    bytes[15] = suffix;
    Uuid::from_bytes(bytes)
}

fn base64url(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut output = String::new();
    let mut index = 0;
    while index + 3 <= bytes.len() {
        let chunk = (u32::from(bytes[index]) << 16)
            | (u32::from(bytes[index + 1]) << 8)
            | u32::from(bytes[index + 2]);
        output.push(ALPHABET[((chunk >> 18) & 63) as usize] as char);
        output.push(ALPHABET[((chunk >> 12) & 63) as usize] as char);
        output.push(ALPHABET[((chunk >> 6) & 63) as usize] as char);
        output.push(ALPHABET[(chunk & 63) as usize] as char);
        index += 3;
    }
    match bytes.len() - index {
        1 => {
            let chunk = u32::from(bytes[index]) << 16;
            output.push(ALPHABET[((chunk >> 18) & 63) as usize] as char);
            output.push(ALPHABET[((chunk >> 12) & 63) as usize] as char);
        }
        2 => {
            let chunk = (u32::from(bytes[index]) << 16) | (u32::from(bytes[index + 1]) << 8);
            output.push(ALPHABET[((chunk >> 18) & 63) as usize] as char);
            output.push(ALPHABET[((chunk >> 12) & 63) as usize] as char);
            output.push(ALPHABET[((chunk >> 6) & 63) as usize] as char);
        }
        _ => {}
    }
    output
}

fn catalog() -> Arc<OperatorSchemaCatalog> {
    let entry = RegistryEntry {
        operation: "test.echo".into(),
        domain: "test".into(),
        version: "v1".into(),
        action: "test:echo".into(),
        description: "test".into(),
        input_schema: "test/echo.input.json".into(),
        output_schema: "test/echo.output.json".into(),
        required_authority: "delegation-grant".into(),
        governance: Governance::AgentExecutable,
        idempotency: "none".into(),
        consequence: "none".into(),
        evidence_contract: "operation-effect-v1".into(),
        benchmark: None,
        status: VersionStatus::Active,
        deprecated_since: None,
        replacement_operation: None,
    };
    let schema = br#"{"$schema":"https://json-schema.org/draft/2020-12/schema","type":"object","additionalProperties":false,"required":["value"],"properties":{"value":{"type":"string"}}}"#;
    Arc::new(
        OperatorSchemaCatalog::from_source_inventory(OperatorSchemaSourceInventory {
            entries: vec![OperatorSchemaSource {
                registry_entry_path: "test/echo.json".into(),
                registry_entry: serde_json::to_vec(&entry).unwrap(),
                input_schema_path: entry.input_schema,
                input_schema: schema.to_vec(),
                output_schema_path: entry.output_schema,
                output_schema: schema.to_vec(),
            }],
        })
        .unwrap(),
    )
}

fn open_operator_store(
    path: &Path,
    environment: Arc<RecordingOperatorControlEnvironment>,
    catalog: Arc<OperatorSchemaCatalog>,
) -> SqliteStore {
    let connection = Connection::open(path).unwrap();
    connection
        .pragma_update(None, "foreign_keys", "ON")
        .unwrap();
    let directory = File::open(path.parent().unwrap()).unwrap();
    let database = File::open(path).unwrap();
    SqliteStore::from_operator_existing_connection(
        connection,
        directory,
        database,
        environment,
        catalog,
    )
}

fn initialize_policy(
    store: &SqliteStore,
    catalog: &OperatorSchemaCatalog,
    now: DateTime<Utc>,
) -> (Uuid, Uuid, PrincipalId) {
    let workspace_id = id(1);
    let budget_id = id(2);
    let agent_id = id(3);
    let human_id = id(4);
    let agent_key = SigningKey::from_bytes(&[0x11; 32]);
    let human_key = SigningKey::from_bytes(&[0x22; 32]);
    let agent = Principal {
        id: PrincipalId::new(agent_id),
        kind: PrincipalKind::Agent,
        public_key: agent_key.verifying_key(),
        created_at: now,
    };
    let human = Principal {
        id: PrincipalId::new(human_id),
        kind: PrincipalKind::Human,
        public_key: human_key.verifying_key(),
        created_at: now,
    };
    store.save_principal(&agent).unwrap();
    store.save_principal(&human).unwrap();

    let principal_binding = |principal: &Principal| PrincipalBinding {
        principal_id: principal.id,
        kind: principal.kind,
        public_key: base64url(principal.public_key.as_bytes()),
        public_key_fingerprint: control_digest(
            "Proof-Operator-Public-Key-v1",
            principal.public_key.as_bytes(),
        ),
    };
    let agent_binding = principal_binding(&agent);
    let human_binding = principal_binding(&human);
    let fingerprint_input = WorkspaceFingerprintInput {
        schema: WorkspaceFingerprintInput::SCHEMA.into(),
        workspace_id,
        proof_directory: DescriptorIdentity {
            device: 1,
            inode: 1,
        },
        control_lock: DescriptorIdentity {
            device: 1,
            inode: 2,
        },
        agent_key_file: DescriptorIdentity {
            device: 1,
            inode: 3,
        },
        human_key_file: DescriptorIdentity {
            device: 1,
            inode: 4,
        },
        agent_id,
        human_id,
        agent_public_key: agent_binding.public_key.clone(),
        human_public_key: human_binding.public_key.clone(),
    };
    let mut workspace = OperatorWorkspace {
        schema: OperatorWorkspace::SCHEMA.into(),
        workspace_id,
        database_name: "storage.db".into(),
        workspace_fingerprint: control_digest_serialized(
            "Proof-Operator-Workspace-v1",
            &fingerprint_input,
        )
        .unwrap(),
        fingerprint_input,
        schema_catalog_digest: catalog.digest(),
        agent: agent_binding,
        human: human_binding,
        auth_epoch: 1,
        policy_revision: 1,
        capabilities: CapabilitySet::all(),
        created_at: now,
        updated_at: now,
        binding_digest: ControlDigest::from_bytes([0; 32]),
    };
    workspace.binding_digest = digest_without_field(
        "Proof-Operator-Workspace-Binding-v1",
        &workspace,
        "binding_digest",
    )
    .unwrap();
    workspace.validate().unwrap();

    let mut policy = BudgetPolicy {
        schema: BudgetPolicy::SCHEMA.into(),
        budget_id,
        workspace_id,
        limits: BudgetAmounts {
            steps: 10,
            tokens: 100,
            duration_ms: 10_000,
            cost_microusd: 100,
            tool_dispatches: 10,
        },
        deadline_at: now + Duration::minutes(5),
        limits_digest: ControlDigest::from_bytes([0; 32]),
    };
    policy.limits_digest =
        digest_without_field("Proof-Operator-Budget-Limits-v1", &policy, "limits_digest").unwrap();
    let account = BudgetAccount {
        schema: BudgetAccount::SCHEMA.into(),
        policy: policy.clone(),
        revision: 0,
        state: BudgetAccountState::Active,
        reserved: BudgetAmounts::default(),
        committed: BudgetAmounts::default(),
        created_at: now,
        updated_at: now,
    };
    account.validate().unwrap();

    let connection = store.conn.lock().unwrap();
    let transaction =
        Transaction::new_unchecked(&connection, TransactionBehavior::Immediate).unwrap();
    transaction
        .execute(
            "INSERT INTO operator_workspaces
             (singleton, workspace_id, schema, database_name, fingerprint_json,
              workspace_fingerprint, schema_catalog_digest, binding_digest,
              agent_id, human_id, auth_epoch, policy_revision, capabilities_json,
              created_at, updated_at, binding_json)
             VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 1, 1, ?10, ?11, ?11, ?12)",
            params![
                workspace_id.to_string(),
                workspace.schema,
                workspace.database_name,
                json(&workspace.fingerprint_input).unwrap(),
                workspace.workspace_fingerprint.to_string(),
                workspace.schema_catalog_digest.to_string(),
                workspace.binding_digest.to_string(),
                agent_id.to_string(),
                human_id.to_string(),
                json(&workspace.capabilities).unwrap(),
                now.to_rfc3339(),
                json(&workspace).unwrap(),
            ],
        )
        .unwrap();
    transaction
        .execute(
            "INSERT INTO operator_budget_accounts
             (budget_id, workspace_id, schema, revision, state,
              max_steps, max_tokens, max_duration_ms, max_cost_microusd,
              max_tool_dispatches, deadline_at, created_at, updated_at,
              limits_digest, limits_json)
             VALUES (?1, ?2, ?3, 0, 'active', ?4, ?5, ?6, ?7, ?8, ?9, ?10,
                     ?10, ?11, ?12)",
            params![
                budget_id.to_string(),
                workspace_id.to_string(),
                account.schema,
                i64_safe(policy.limits.steps).unwrap(),
                i64_safe(policy.limits.tokens).unwrap(),
                i64_safe(policy.limits.duration_ms).unwrap(),
                i64_safe(policy.limits.cost_microusd).unwrap(),
                i64_safe(policy.limits.tool_dispatches).unwrap(),
                policy.deadline_at.to_rfc3339(),
                now.to_rfc3339(),
                policy.limits_digest.to_string(),
                json(&policy).unwrap(),
            ],
        )
        .unwrap();
    transaction
        .execute(
            "INSERT INTO operator_audit_heads (workspace_id, last_sequence, last_digest)
             VALUES (?1, 0, NULL)",
            [workspace_id.to_string()],
        )
        .unwrap();
    transaction.commit().unwrap();
    (workspace_id, budget_id, agent.id)
}

fn settlement_state(
    store: &SqliteStore,
    reservation_id: Uuid,
    budget_id: Uuid,
    run_id: Uuid,
) -> SettlementState {
    let connection = store.conn.lock().unwrap();
    let transaction =
        Transaction::new_unchecked(&connection, TransactionBehavior::Deferred).unwrap();
    let state = SettlementState {
        reservation: load_reservation(&transaction, reservation_id).unwrap(),
        budget: load_budget(&transaction, budget_id).unwrap(),
        control: load_control(&transaction, run_id).unwrap(),
    };
    transaction.commit().unwrap();
    state
}

fn audit_head(store: &SqliteStore, workspace_id: Uuid) -> (u64, Option<ControlDigest>) {
    let connection = store.conn.lock().unwrap();
    let (sequence, digest): (i64, Option<String>) = connection
        .query_row(
            "SELECT last_sequence, last_digest FROM operator_audit_heads
             WHERE workspace_id=?1",
            [workspace_id.to_string()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    (
        u64_safe(sequence).unwrap(),
        digest.map(|value| value.parse().unwrap()),
    )
}

fn effect_rows(store: &SqliteStore) -> (i64, i64, i64, i64) {
    let connection = store.conn.lock().unwrap();
    let count = |table: &str| {
        connection
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                row.get(0)
            })
            .unwrap()
    };
    (
        count("proofs"),
        count("execution_contexts"),
        count("execution_replays"),
        count("agent_run_events"),
    )
}

#[test]
fn dispatching_pre_dispatch_release_commits_only_rejection_then_requires_full_forfeit() {
    let directory = tempfile::tempdir().unwrap();
    let database_path = directory.path().join("operator-ordinal-078.db");
    let connection = Connection::open(&database_path).unwrap();
    connection
        .pragma_update(None, "foreign_keys", "ON")
        .unwrap();
    run_migrations_through(&connection, 14, TransactionBehavior::Exclusive).unwrap();
    drop(connection);

    let now: DateTime<Utc> = "2032-05-06T07:08:09Z".parse().unwrap();
    let environment = Arc::new(RecordingOperatorControlEnvironment::new(now, [0x53; 32]));
    let catalog = catalog();
    let store = open_operator_store(&database_path, environment.clone(), catalog.clone());
    let (workspace_id, budget_id, actor) = initialize_policy(&store, &catalog, now);
    assert_eq!(
        OperatorDirectoryStore::load_operator_workspace(&store)
            .unwrap()
            .workspace_id,
        workspace_id
    );

    let mut run =
        AgentRun::new_for_agent(actor, id(6), AgentRunMode::Session, "test", now).unwrap();
    run.id = id(5);
    store.save_agent_run(&run).unwrap();
    run.start(now).unwrap();
    store.save_agent_run(&run).unwrap();
    let input = json_value!({"value": "input"});
    let mut step = AgentRunStep::new(run.id, 0, "test.echo", "v1", &input, now).unwrap();
    step.id = id(7);
    store.save_agent_run_step(&step).unwrap();
    step.start(now).unwrap();
    store.save_agent_run_step(&step).unwrap();
    let mut checkpoint =
        AgentCheckpoint::create(run.id, 0, json_value!({"cursor": 0}), now).unwrap();
    checkpoint.id = id(8);
    store.save_agent_checkpoint(&checkpoint).unwrap();
    {
        // Governed operator rows are strict canonical JSON, while these setup
        // helpers intentionally preserve the legacy agent-store encoding.
        let connection = store.conn.lock().unwrap();
        connection
            .execute(
                "UPDATE agent_runs SET run_json=?2 WHERE id=?1",
                params![run.id.to_string(), json(&run).unwrap()],
            )
            .unwrap();
        connection
            .execute(
                "UPDATE agent_run_steps SET step_json=?2 WHERE id=?1",
                params![step.id.to_string(), json(&step).unwrap()],
            )
            .unwrap();
        connection
            .execute(
                "UPDATE agent_checkpoints SET checkpoint_json=?2 WHERE id=?1",
                params![checkpoint.id.to_string(), json(&checkpoint).unwrap()],
            )
            .unwrap();
    }

    let registration = store
        .register_governed_run(RegisterGovernedRunRequest {
            schema: "proof.operator.register-governed-run-request/v1".into(),
            workspace_id,
            run_id: run.id,
            budget_id,
            initial_projection: InitialRunProjectionInput {
                schema: "proof.operator.initial-run-projection-input/v1".into(),
                workspace_id,
                run_id: run.id,
                source_run_revision: run.revision,
                checkpoint_id: checkpoint.id,
                checkpoint_sequence: u64::from(checkpoint.sequence),
                checkpoint_digest: checkpoint.state_digest,
                run_status: run.status,
            },
        })
        .unwrap();
    assert_eq!(registration.control_revision, 0);

    let lease_id = id(9);
    let owner_instance_id = id(10);
    let process_epoch_id = id(11);
    let mut lease_custody = LeaseTokenCustody::new(LEASE_TOKEN);
    let lease_result = store
        .claim_run_lease(
            lease_custody
                .claim_request(
                    workspace_id,
                    run.id,
                    lease_id,
                    owner_instance_id,
                    process_epoch_id,
                    0,
                    registration.control_revision,
                )
                .unwrap(),
        )
        .unwrap();
    lease_custody.bind_claim_result(&lease_result).unwrap();
    {
        let connection = store.conn.lock().unwrap();
        let transaction =
            Transaction::new_unchecked(&connection, TransactionBehavior::Deferred).unwrap();
        assert_eq!(load_agent_run_exact(&transaction, run.id).unwrap(), run);
        assert_eq!(
            load_control(&transaction, run.id).unwrap().control_revision,
            lease_result.control_revision
        );
        assert_eq!(
            load_lease(&transaction, lease_id).unwrap(),
            lease_result.lease
        );
        assert_eq!(load_budget(&transaction, budget_id).unwrap().revision, 0);
        let projection_json: String = transaction
            .query_row(
                "SELECT snapshot_json FROM operator_run_projections
                 WHERE run_id=?1 ORDER BY projection_sequence DESC LIMIT 1",
                [run.id.to_string()],
                |row| row.get(0),
            )
            .unwrap();
        let projection: RunProjection = decode(&projection_json).unwrap();
        projection.validate().unwrap();
        assert_eq!(projection.source_run_revision, run.revision);
        assert_eq!(
            projection.source_control_revision,
            lease_result.control_revision
        );
        assert_eq!(projection.fence_epoch, lease_result.lease.fence_epoch);
        assert_eq!(projection.attention, AttentionState::Running);
        assert_eq!(
            load_latest_checkpoint_identity(&transaction, run.id).unwrap(),
            (
                projection.checkpoint_id,
                projection.checkpoint_sequence,
                projection.checkpoint_digest,
            )
        );
        transaction.commit().unwrap();
    }

    let intent = DispatchIntent {
        schema: DispatchIntent::SCHEMA.into(),
        kind: BoundaryKind::Provider,
        adapter: "synthetic".into(),
        model: Some("fixed-v1".into()),
        operation: "test.echo".into(),
        version: "v1".into(),
        argument_digest: control_digest(
            "Proof-Operator-Dispatch-Argument-v1",
            canonicalize(&input).unwrap().as_bytes(),
        ),
        ceiling: BudgetAmounts {
            steps: 1,
            tokens: 10,
            duration_ms: 100,
            cost_microusd: 3,
            tool_dispatches: 0,
        },
    };
    let intent_digest =
        control_digest_serialized("Proof-Operator-Dispatch-Intent-v1", &intent).unwrap();
    let call_digest =
        control_digest_serialized("Proof-Operator-Dispatch-Call-v1", &intent).unwrap();
    let reservation_id = id(12);
    let reservation_result = store
        .reserve_aggregate_budget(BudgetReserveRequest {
            schema: "proof.operator.budget-reserve-request/v1".into(),
            authority: lease_custody
                .authority(lease_result.control_revision)
                .unwrap(),
            reservation_id,
            idempotency_key: id(13),
            intent: intent.clone(),
            intent_digest,
            replay: None,
            recovery: None,
        })
        .unwrap();

    let mut dispatch_custody = DispatchTokenCustody::new(DISPATCH_TOKEN);
    let dispatch_result = store
        .begin_dispatch(
            dispatch_custody
                .begin_request(
                    lease_custody
                        .authority(reservation_result.control_revision)
                        .unwrap(),
                    reservation_id,
                    intent,
                    intent_digest,
                    None,
                    None,
                    call_digest,
                )
                .unwrap(),
        )
        .unwrap();
    dispatch_custody
        .bind_permit(&dispatch_result, environment.as_ref())
        .unwrap();
    let permit = dispatch_result.permit.clone().unwrap();
    let control_revision = dispatch_result.control_revision;
    let unchanged = settlement_state(&store, reservation_id, budget_id, run.id);
    assert_eq!(
        unchanged.reservation.state,
        BudgetReservationState::Dispatching
    );
    let baseline_head = audit_head(&store, workspace_id);
    let baseline_effect_rows = effect_rows(&store);

    let unproven = store.settle_budget_reservation(BudgetSettlementRequest {
        schema: "proof.operator.budget-settlement-request/v1".into(),
        authority: lease_custody.authority(control_revision + 1).unwrap(),
        reservation_id,
        disposition: BudgetSettlementDisposition::ReleasePreDispatch,
    });
    assert_eq!(unproven, Err(OperatorStoreError::StaleFence));
    assert_eq!(audit_head(&store, workspace_id), baseline_head);

    let rejected = store.settle_budget_reservation(BudgetSettlementRequest {
        schema: "proof.operator.budget-settlement-request/v1".into(),
        authority: lease_custody.authority(control_revision).unwrap(),
        reservation_id,
        disposition: BudgetSettlementDisposition::ReleasePreDispatch,
    });
    assert_eq!(rejected, Err(OperatorStoreError::NotActionable));
    assert_eq!(
        settlement_state(&store, reservation_id, budget_id, run.id),
        unchanged
    );
    assert_eq!(effect_rows(&store), baseline_effect_rows);
    drop(store);

    let reopened = open_operator_store(&database_path, environment.clone(), catalog);
    assert_eq!(
        settlement_state(&reopened, reservation_id, budget_id, run.id),
        unchanged
    );
    assert_eq!(effect_rows(&reopened), baseline_effect_rows);
    let rejected_head = audit_head(&reopened, workspace_id);
    assert_eq!(rejected_head.0, baseline_head.0 + 1);

    let event_json: String = reopened
        .conn
        .lock()
        .unwrap()
        .query_row(
            "SELECT event_json FROM operator_audit_events
             WHERE workspace_id=?1 AND sequence=?2",
            params![workspace_id.to_string(), i64_safe(rejected_head.0).unwrap()],
            |row| row.get(0),
        )
        .unwrap();
    let event: AuditEvent = decode(&event_json).unwrap();
    let mut expected_event = event_base(
        workspace_id,
        event.event_id,
        AuditEventKind::BudgetRejected,
        AuditOutcome::Rejected,
        now,
    );
    expected_event.sequence = baseline_head.0 + 1;
    expected_event.previous_digest = baseline_head.1;
    expected_event.run_id = Some(run.id);
    expected_event.budget_id = Some(budget_id);
    expected_event.reservation_id = Some(reservation_id);
    expected_event.lease_id = Some(lease_id);
    expected_event.fence_epoch = Some(1);
    expected_event.intent_digest = Some(intent_digest);
    expected_event.event_digest = digest_without_field(
        "Proof-Operator-Audit-Event-v1",
        &expected_event,
        "event_digest",
    )
    .unwrap();
    assert_eq!(event, expected_event);
    assert_eq!(rejected_head.1, Some(event.event_digest));
    let matching_events: i64 = reopened
        .conn
        .lock()
        .unwrap()
        .query_row(
            "SELECT COUNT(*) FROM operator_audit_events
             WHERE workspace_id=?1 AND reservation_id=?2
               AND kind='budget_rejected' AND outcome='rejected'",
            params![workspace_id.to_string(), reservation_id.to_string()],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(matching_events, 1);

    let failure = RuntimeFailureBody {
        schema: "proof.operator.runtime-failure-body/v1".into(),
        reservation_id,
        permit_id: permit.permit_id,
        classification: RuntimeFailureClassification::AmbiguousForfeitRequired,
        failure_code: RuntimeFailureCode::ProcessShutdown,
        intent_digest,
        call_digest,
    };
    let error_digest =
        control_digest_serialized("Proof-Operator-Runtime-Failure-v1", &failure).unwrap();
    let failure_result = reopened
        .settle_runtime_failure(
            dispatch_custody
                .into_failure_request(
                    lease_custody.authority(control_revision).unwrap(),
                    failure,
                    error_digest,
                )
                .unwrap(),
        )
        .unwrap();
    failure_result.validate().unwrap();
    let forfeited = settlement_state(&reopened, reservation_id, budget_id, run.id);
    assert_eq!(
        forfeited.reservation.state,
        BudgetReservationState::Forfeited
    );
    assert_eq!(
        forfeited.reservation.charged,
        forfeited.reservation.reserved
    );
    assert_eq!(forfeited.budget.reserved, BudgetAmounts::default());
    assert_eq!(forfeited.budget.committed, forfeited.reservation.reserved);
    assert_eq!(forfeited.control.active_dispatch_reservation_id, None);
    let final_kinds: Vec<String> = reopened
        .conn
        .lock()
        .unwrap()
        .prepare(
            "SELECT kind FROM operator_audit_events
             WHERE workspace_id=?1 AND sequence>?2 ORDER BY sequence",
        )
        .unwrap()
        .query_map(
            params![workspace_id.to_string(), i64_safe(baseline_head.0).unwrap()],
            |row| row.get(0),
        )
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(
        final_kinds,
        vec!["budget_rejected", "budget_forfeited", "control_failure"]
    );
}
