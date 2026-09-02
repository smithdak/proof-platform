//! Schema-14 operator projections, commands, leases, budgets, and replay.

use super::store::SqliteStore;
use chrono::{DateTime, Duration, Utc};
use proof_kernel::{
    canonicalize, control_digest_serialized, digest, AgentCheckpoint, AgentRun, AgentRunEvaluation,
    AgentRunEvent, AgentRunEventKind, AgentRunMode, AgentRunStatus, AgentRunStep,
    AgentRunStepStatus, AppliedCommandOutcome, ApprovalAttentionItem, ApprovalBinding,
    ApprovalDecisionSummary, ApprovalDetail, ApprovalOutcome, ApprovalPage, ApprovalQuery,
    ApprovalSigningRequest, ApprovalState, ApprovalSummary, ArtifactKind, AttentionItem,
    AttentionKind, AttentionPage, AttentionQuery, AttentionState, AuditEvent, AuditEventKind,
    AuditOutcome, AuditPage, AuditQuery, AuthoritySummary, BudgetAccount, BudgetAccountState,
    BudgetAmounts, BudgetReservation, BudgetReservationState, BudgetReserveOutcome,
    BudgetReserveRequest, BudgetReserveResult, BudgetSettlementOutcome, BudgetSettlementRequest,
    BudgetSettlementResult, BudgetSnapshot, Capability, CheckpointTail, CommandEnvelope,
    CommandExecutionRequest, CommandKind, CommandOutcome, CommandPage, CommandQuery,
    CommandReceipt, CommandResult, CommandResultOutcome, ControlAuditAppendRequest,
    ControlAuditAppendResult, ControlAuthorityEventKind, ControlDigest, ControlTransitionOutcome,
    CreationOutcome, DecisionOutcome, DispatchOutcome, DispatchPermit, DispatchResult,
    InitialRunProjectionInput, LeaseClaimRequest, LeaseMutationOutcome, LeaseMutationResult,
    LeaseReleaseRequest, LeaseRenewRequest, OperatorAuthorityAuditStore, OperatorCommandStore,
    OperatorCursorCodec, OperatorCursorError, OperatorDirectoryStore, OperatorMutationRoute,
    OperatorProofOperation, OperatorProofSigningRequest, OperatorReadRoute, OperatorReadScope,
    OperatorReadStore, OperatorRuntimeStore, OperatorSigner, OperatorStoreError, OperatorWorkspace,
    PageInfo, PageWindowKind, PendingApprovalSummary, PendingConsequenceBody, PendingDecision,
    Principal, PrincipalId, PrincipalKind, ProofReference, ReclaimOutcome, ReclaimRequest,
    ReclaimResult, RecoveryDirective, RecoverySummary, RegisterGovernedRunRequest,
    RegisterGovernedRunResult, ReplayCompletionBinding, ReplayLookupOutcome, ReplayLookupRequest,
    ReplayLookupResult, ReplayProofEnvelope, RunAttemptSummary, RunAttentionItem, RunControl,
    RunDetail, RunLease, RunLeaseState, RunProjection, RuntimeCommit, RuntimeCommitRequest,
    RuntimeCommitResult, RuntimeFailureRequest, RuntimeFailureResult, SignedApprovalDecision,
    SignedApprovalRequest, Urgency, VerifiedPageWindow,
};
use rusqlite::{params, OptionalExtension, Transaction, TransactionBehavior};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::collections::BTreeSet;
use uuid::Uuid;

const MAX_SAFE: u64 = 9_007_199_254_740_991;

fn invalid_if(condition: bool) -> Result<(), OperatorStoreError> {
    if condition {
        Err(OperatorStoreError::Invalid)
    } else {
        Ok(())
    }
}

fn json<T: Serialize>(value: &T) -> Result<String, OperatorStoreError> {
    proof_kernel::canonicalize_serialized(value)
        .map(|value| value.to_string())
        .map_err(|_| OperatorStoreError::Invalid)
}

fn decode<T: DeserializeOwned>(value: &str) -> Result<T, OperatorStoreError> {
    let mut duplicate_check = serde_json::Deserializer::from_str(value);
    DuplicateCheckedValue::deserialize(&mut duplicate_check)
        .map_err(|_| OperatorStoreError::Corrupt)?;
    duplicate_check
        .end()
        .map_err(|_| OperatorStoreError::Corrupt)?;
    let mut typed = serde_json::Deserializer::from_str(value);
    let result = T::deserialize(&mut typed).map_err(|_| OperatorStoreError::Corrupt)?;
    typed.end().map_err(|_| OperatorStoreError::Corrupt)?;
    Ok(result)
}

struct DuplicateCheckedValue;

impl<'de> Deserialize<'de> for DuplicateCheckedValue {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Visitor;
        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = DuplicateCheckedValue;
            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("JSON without duplicate object names")
            }
            fn visit_bool<E>(self, _: bool) -> Result<Self::Value, E> {
                Ok(DuplicateCheckedValue)
            }
            fn visit_i64<E>(self, _: i64) -> Result<Self::Value, E> {
                Ok(DuplicateCheckedValue)
            }
            fn visit_u64<E>(self, _: u64) -> Result<Self::Value, E> {
                Ok(DuplicateCheckedValue)
            }
            fn visit_f64<E>(self, _: f64) -> Result<Self::Value, E> {
                Ok(DuplicateCheckedValue)
            }
            fn visit_str<E>(self, _: &str) -> Result<Self::Value, E> {
                Ok(DuplicateCheckedValue)
            }
            fn visit_string<E>(self, _: String) -> Result<Self::Value, E> {
                Ok(DuplicateCheckedValue)
            }
            fn visit_none<E>(self) -> Result<Self::Value, E> {
                Ok(DuplicateCheckedValue)
            }
            fn visit_unit<E>(self) -> Result<Self::Value, E> {
                Ok(DuplicateCheckedValue)
            }
            fn visit_some<D: serde::Deserializer<'de>>(
                self,
                deserializer: D,
            ) -> Result<Self::Value, D::Error> {
                DuplicateCheckedValue::deserialize(deserializer)
            }
            fn visit_seq<A: serde::de::SeqAccess<'de>>(
                self,
                mut sequence: A,
            ) -> Result<Self::Value, A::Error> {
                while sequence.next_element::<DuplicateCheckedValue>()?.is_some() {}
                Ok(DuplicateCheckedValue)
            }
            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                mut map: A,
            ) -> Result<Self::Value, A::Error> {
                let mut names = BTreeSet::new();
                while let Some(name) = map.next_key::<String>()? {
                    if !names.insert(name.clone()) {
                        return Err(serde::de::Error::custom(format!(
                            "duplicate object name {name}"
                        )));
                    }
                    map.next_value::<DuplicateCheckedValue>()?;
                }
                Ok(DuplicateCheckedValue)
            }
        }
        deserializer.deserialize_any(Visitor)
    }
}

fn wire<T: Serialize>(value: &T) -> Result<String, OperatorStoreError> {
    let value = serde_json::to_string(value).map_err(|_| OperatorStoreError::Invalid)?;
    Ok(value.trim_matches('"').to_string())
}

fn uuid(value: &str) -> Result<Uuid, OperatorStoreError> {
    let value = Uuid::parse_str(value).map_err(|_| OperatorStoreError::Corrupt)?;
    invalid_if(!proof_kernel::uuid_is_v7(value)).map(|()| value)
}

fn time(value: &str) -> Result<DateTime<Utc>, OperatorStoreError> {
    value
        .parse::<DateTime<Utc>>()
        .map_err(|_| OperatorStoreError::Corrupt)
}

fn i64_safe(value: u64) -> Result<i64, OperatorStoreError> {
    invalid_if(value > MAX_SAFE).map(|()| value as i64)
}

fn u64_safe(value: i64) -> Result<u64, OperatorStoreError> {
    if value < 0 || value as u64 > MAX_SAFE {
        Err(OperatorStoreError::Corrupt)
    } else {
        Ok(value as u64)
    }
}

fn map_db(_: rusqlite::Error) -> OperatorStoreError {
    OperatorStoreError::Unavailable
}

impl SqliteStore {
    fn operator_context(&self) -> Result<&super::store::OperatorStoreContext, OperatorStoreError> {
        self.operator
            .as_ref()
            .ok_or(OperatorStoreError::Unavailable)
    }

    fn operator_now(&self) -> Result<DateTime<Utc>, OperatorStoreError> {
        self.operator_context()?
            .environment
            .trusted_utc_now()
            .map_err(|_| OperatorStoreError::Unavailable)
    }

    fn operator_uuid(&self) -> Result<Uuid, OperatorStoreError> {
        self.operator_context()?
            .environment
            .new_uuid_v7()
            .map_err(|_| OperatorStoreError::Unavailable)
    }

    fn validate_read_scope(
        transaction: &Transaction<'_>,
        scope: &OperatorReadScope,
        route: OperatorReadRoute,
    ) -> Result<(), OperatorStoreError> {
        scope.validate()?;
        invalid_if(scope.route != route)?;
        let workspace: String = transaction
            .query_row(
                "SELECT binding_json FROM operator_workspaces WHERE singleton = 1",
                [],
                |row| row.get(0),
            )
            .map_err(map_db)?;
        let workspace: OperatorWorkspace = decode(&workspace)?;
        workspace
            .validate()
            .map_err(|_| OperatorStoreError::Corrupt)?;
        invalid_if(
            scope.workspace_id != workspace.workspace_id
                || scope.human_id != workspace.human.principal_id.as_uuid()
                || scope.auth_epoch != workspace.auth_epoch
                || scope.policy_revision != workspace.policy_revision
                || scope
                    .required_capabilities
                    .iter()
                    .any(|capability| !workspace.capabilities.contains(*capability)),
        )
    }
}

impl OperatorDirectoryStore for SqliteStore {
    fn load_operator_workspace(&self) -> Result<OperatorWorkspace, OperatorStoreError> {
        self.operator_context()?;
        let connection = self
            .conn
            .lock()
            .map_err(|_| OperatorStoreError::Unavailable)?;
        let serialized: String = connection
            .query_row(
                "SELECT binding_json FROM operator_workspaces WHERE singleton = 1",
                [],
                |row| row.get(0),
            )
            .map_err(map_db)?;
        let workspace: OperatorWorkspace = decode(&serialized)?;
        workspace
            .validate()
            .map_err(|_| OperatorStoreError::Corrupt)?;
        if workspace.schema_catalog_digest != self.operator_context()?.catalog.digest() {
            return Err(OperatorStoreError::Corrupt);
        }
        Ok(workspace)
    }

    fn register_governed_run(
        &self,
        request: RegisterGovernedRunRequest,
    ) -> Result<RegisterGovernedRunResult, OperatorStoreError> {
        self.operator_context()?;
        validate_register_request(&request)?;
        let connection = self
            .conn
            .lock()
            .map_err(|_| OperatorStoreError::Unavailable)?;
        let transaction = Transaction::new_unchecked(&connection, TransactionBehavior::Immediate)
            .map_err(map_db)?;

        if let Some((control_json, projection_json)) = transaction
            .query_row(
                "SELECT c.binding_json, p.snapshot_json
                 FROM operator_run_control c
                 JOIN operator_run_projections p ON p.run_id = c.run_id
                 WHERE c.run_id = ?1
                 ORDER BY p.projection_sequence ASC LIMIT 1",
                [request.run_id.to_string()],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .optional()
            .map_err(map_db)?
        {
            let control: RunControl = decode(&control_json)?;
            let projection: RunProjection = decode(&projection_json)?;
            if control.run_id != request.run_id
                || control.workspace_id != request.workspace_id
                || control.budget_id != request.budget_id
                || projection.run_id != request.run_id
                || projection.workspace_id != request.workspace_id
                || projection.source_run_revision != request.initial_projection.source_run_revision
                || projection.checkpoint_id != request.initial_projection.checkpoint_id
                || projection.checkpoint_sequence != request.initial_projection.checkpoint_sequence
                || projection.checkpoint_digest != request.initial_projection.checkpoint_digest
                || projection.run_status != request.initial_projection.run_status
            {
                return Err(OperatorStoreError::Conflict);
            }
            transaction.commit().map_err(map_db)?;
            return Ok(RegisterGovernedRunResult {
                schema: "proof.operator.register-governed-run-result/v1".into(),
                outcome: CreationOutcome::ExactExisting,
                run_id: request.run_id,
                control_revision: control.control_revision,
                run_control: control,
                initial_projection: projection,
            });
        }

        let run_json: String = transaction
            .query_row(
                "SELECT run_json FROM agent_runs WHERE id = ?1",
                [request.run_id.to_string()],
                |row| row.get(0),
            )
            .map_err(|error| match error {
                rusqlite::Error::QueryReturnedNoRows => OperatorStoreError::NotFound,
                other => map_db(other),
            })?;
        let run: AgentRun = decode(&run_json)?;
        let checkpoint_digest: String = transaction
            .query_row(
                "SELECT state_digest FROM agent_checkpoints
                 WHERE id = ?1 AND run_id = ?2 AND sequence = ?3",
                params![
                    request.initial_projection.checkpoint_id.to_string(),
                    request.run_id.to_string(),
                    i64_safe(request.initial_projection.checkpoint_sequence)?,
                ],
                |row| row.get(0),
            )
            .map_err(|error| match error {
                rusqlite::Error::QueryReturnedNoRows => OperatorStoreError::NotFound,
                other => map_db(other),
            })?;
        invalid_if(
            run.id != request.run_id
                || run.revision != request.initial_projection.source_run_revision
                || run.status != request.initial_projection.run_status
                || !matches!(run.status, AgentRunStatus::Queued | AgentRunStatus::Running)
                || checkpoint_digest != request.initial_projection.checkpoint_digest.hex(),
        )?;
        let workspace_id: String = transaction
            .query_row(
                "SELECT workspace_id FROM operator_workspaces WHERE singleton = 1",
                [],
                |row| row.get(0),
            )
            .map_err(map_db)?;
        invalid_if(workspace_id != request.workspace_id.to_string())?;
        let budget_exists: i64 = transaction
            .query_row(
                "SELECT COUNT(*) FROM operator_budget_accounts
                 WHERE budget_id = ?1 AND workspace_id = ?2",
                params![request.budget_id.to_string(), workspace_id],
                |row| row.get(0),
            )
            .map_err(map_db)?;
        invalid_if(budget_exists != 1)?;

        let now = self.operator_now()?;
        let mut control = RunControl {
            schema: RunControl::SCHEMA.into(),
            run_id: request.run_id,
            workspace_id: request.workspace_id,
            budget_id: request.budget_id,
            control_revision: 0,
            active_dispatch_reservation_id: None,
            recovery_directive_id: None,
            recovery_directive_digest: None,
            last_command_id: None,
            created_at: now,
            updated_at: now,
            binding_digest: ControlDigest::from_bytes([0; 32]),
        };
        control.binding_digest =
            digest_without_field("Proof-Operator-Run-Binding-v1", &control, "binding_digest")?;
        control
            .validate()
            .map_err(|_| OperatorStoreError::Invalid)?;
        let projection_id = self.operator_uuid()?;
        let sequence: i64 = transaction
            .query_row(
                "SELECT COALESCE(MAX(projection_sequence), 0) + 1 FROM operator_run_projections",
                [],
                |row| row.get(0),
            )
            .map_err(map_db)?;
        let mut projection = RunProjection {
            schema: RunProjection::SCHEMA.into(),
            projection_id,
            projection_sequence: u64_safe(sequence)?,
            projection_revision: 0,
            workspace_id: request.workspace_id,
            run_id: request.run_id,
            source_run_revision: run.revision,
            source_control_revision: 0,
            checkpoint_id: request.initial_projection.checkpoint_id,
            checkpoint_sequence: request.initial_projection.checkpoint_sequence,
            checkpoint_digest: request.initial_projection.checkpoint_digest,
            fence_epoch: 0,
            run_status: run.status,
            attention: AttentionState::Running,
            required_human_id: None,
            approval_request_id: None,
            recovery_directive_id: None,
            recovery_directive_digest: None,
            projected_at: now,
            snapshot_digest: ControlDigest::from_bytes([0; 32]),
        };
        projection.snapshot_digest = digest_without_field(
            "Proof-Operator-Run-Projection-v1",
            &projection,
            "snapshot_digest",
        )?;
        projection
            .validate()
            .map_err(|_| OperatorStoreError::Invalid)?;
        insert_run_control(&transaction, &control)?;
        insert_projection(&transaction, &projection)?;
        transaction.commit().map_err(map_db)?;
        Ok(RegisterGovernedRunResult {
            schema: "proof.operator.register-governed-run-result/v1".into(),
            outcome: CreationOutcome::Created,
            run_id: request.run_id,
            control_revision: 0,
            run_control: control,
            initial_projection: projection,
        })
    }
}

fn validate_register_request(
    request: &RegisterGovernedRunRequest,
) -> Result<(), OperatorStoreError> {
    invalid_if(
        request.schema != "proof.operator.register-governed-run-request/v1"
            || request.initial_projection.schema
                != "proof.operator.initial-run-projection-input/v1"
            || ![
                request.workspace_id,
                request.run_id,
                request.budget_id,
                request.initial_projection.workspace_id,
                request.initial_projection.run_id,
                request.initial_projection.checkpoint_id,
            ]
            .into_iter()
            .all(proof_kernel::uuid_is_v7)
            || request.workspace_id != request.initial_projection.workspace_id
            || request.run_id != request.initial_projection.run_id
            || request.initial_projection.source_run_revision > MAX_SAFE
            || request.initial_projection.checkpoint_sequence > MAX_SAFE,
    )
}

impl OperatorAuthorityAuditStore for SqliteStore {
    fn append_authority_event(
        &self,
        request: ControlAuditAppendRequest,
    ) -> Result<ControlAuditAppendResult, OperatorStoreError> {
        self.operator_context()?;
        request.validate()?;
        let connection = self
            .conn
            .lock()
            .map_err(|_| OperatorStoreError::Unavailable)?;
        let transaction = Transaction::new_unchecked(&connection, TransactionBehavior::Immediate)
            .map_err(map_db)?;
        ensure_workspace(&transaction, request.workspace_id)?;
        let now = self.operator_now()?;
        let mut event = event_base(
            request.workspace_id,
            self.operator_uuid()?,
            authority_event_kind(request.kind),
            authority_event_outcome(request.kind),
            now,
        );
        event.human_id = request.human_id;
        event.session_id = request.session_id;
        event.challenge_id = request.challenge_id;
        event.challenge_digest = request.challenge_digest;
        event.session_authority_digest = request.session_authority_digest;
        event.related_session_id = request.related_session_id;
        event.server_instance_id = Some(request.server_instance_id);
        event.auth_epoch = request.auth_epoch;
        event.policy_revision = request.policy_revision;
        append_audit_event(&transaction, &mut event)?;
        transaction.commit().map_err(map_db)?;
        Ok(ControlAuditAppendResult {
            schema: ControlAuditAppendResult::SCHEMA.into(),
            event,
        })
    }
}

fn authority_event_kind(kind: ControlAuthorityEventKind) -> AuditEventKind {
    match kind {
        ControlAuthorityEventKind::ControlShutdown => AuditEventKind::ControlShutdown,
        ControlAuthorityEventKind::SessionChallengeIssued => AuditEventKind::SessionChallengeIssued,
        ControlAuthorityEventKind::SessionExpired => AuditEventKind::SessionExpired,
        ControlAuthorityEventKind::SessionIssued => AuditEventKind::SessionIssued,
        ControlAuthorityEventKind::SessionReplaced => AuditEventKind::SessionReplaced,
    }
}

fn authority_event_outcome(kind: ControlAuthorityEventKind) -> AuditOutcome {
    if kind == ControlAuthorityEventKind::SessionExpired {
        AuditOutcome::Expired
    } else {
        AuditOutcome::Accepted
    }
}

fn event_base(
    workspace_id: Uuid,
    event_id: Uuid,
    kind: AuditEventKind,
    outcome: AuditOutcome,
    occurred_at: DateTime<Utc>,
) -> AuditEvent {
    AuditEvent {
        schema: AuditEvent::SCHEMA.into(),
        workspace_id,
        event_id,
        sequence: 1,
        kind,
        outcome,
        previous_digest: None,
        event_digest: ControlDigest::from_bytes([0; 32]),
        human_id: None,
        session_id: None,
        challenge_id: None,
        challenge_digest: None,
        session_authority_digest: None,
        related_session_id: None,
        server_instance_id: None,
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
        auth_epoch: None,
        policy_revision: None,
        intent_digest: None,
        call_digest: None,
        decision_digest: None,
        recovery_directive_digest: None,
        failure_scope: None,
        proof: None,
        occurred_at,
    }
}

fn digest_without_field<T: Serialize>(
    domain: &str,
    value: &T,
    field: &str,
) -> Result<ControlDigest, OperatorStoreError> {
    let mut value = serde_json::to_value(value).map_err(|_| OperatorStoreError::Invalid)?;
    value
        .as_object_mut()
        .ok_or(OperatorStoreError::Invalid)?
        .remove(field);
    control_digest_serialized(domain, &value).map_err(|_| OperatorStoreError::Invalid)
}

fn ensure_workspace(
    transaction: &Transaction<'_>,
    workspace_id: Uuid,
) -> Result<(), OperatorStoreError> {
    let count: i64 = transaction
        .query_row(
            "SELECT COUNT(*) FROM operator_workspaces WHERE singleton = 1 AND workspace_id = ?1",
            [workspace_id.to_string()],
            |row| row.get(0),
        )
        .map_err(map_db)?;
    invalid_if(count != 1)
}

fn insert_run_control(
    transaction: &Transaction<'_>,
    control: &RunControl,
) -> Result<(), OperatorStoreError> {
    transaction
        .execute(
            "INSERT INTO operator_run_control
             (run_id, workspace_id, budget_id, schema, control_revision,
              active_dispatch_reservation_id, recovery_directive_id,
              recovery_directive_digest, last_command_id, created_at, updated_at,
              binding_digest, binding_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            params![
                control.run_id.to_string(),
                control.workspace_id.to_string(),
                control.budget_id.to_string(),
                control.schema,
                i64_safe(control.control_revision)?,
                control
                    .active_dispatch_reservation_id
                    .map(|id| id.to_string()),
                control.recovery_directive_id.map(|id| id.to_string()),
                control
                    .recovery_directive_digest
                    .map(|digest| digest.to_string()),
                control.last_command_id.map(|id| id.to_string()),
                control.created_at.to_rfc3339(),
                control.updated_at.to_rfc3339(),
                control.binding_digest.to_string(),
                json(control)?,
            ],
        )
        .map_err(map_db)?;
    Ok(())
}

fn update_run_control(
    transaction: &Transaction<'_>,
    control: &RunControl,
) -> Result<(), OperatorStoreError> {
    control
        .validate()
        .map_err(|_| OperatorStoreError::Corrupt)?;
    let changed = transaction
        .execute(
            "UPDATE operator_run_control SET control_revision = ?2,
              active_dispatch_reservation_id = ?3, recovery_directive_id = ?4,
              recovery_directive_digest = ?5, last_command_id = ?6,
              updated_at = ?7, binding_digest = ?8, binding_json = ?9
             WHERE run_id = ?1",
            params![
                control.run_id.to_string(),
                i64_safe(control.control_revision)?,
                control
                    .active_dispatch_reservation_id
                    .map(|id| id.to_string()),
                control.recovery_directive_id.map(|id| id.to_string()),
                control
                    .recovery_directive_digest
                    .map(|digest| digest.to_string()),
                control.last_command_id.map(|id| id.to_string()),
                control.updated_at.to_rfc3339(),
                control.binding_digest.to_string(),
                json(control)?,
            ],
        )
        .map_err(map_db)?;
    if changed != 1 {
        return Err(OperatorStoreError::Corrupt);
    }
    Ok(())
}

fn insert_projection(
    transaction: &Transaction<'_>,
    projection: &RunProjection,
) -> Result<(), OperatorStoreError> {
    transaction
        .execute(
            "INSERT INTO operator_run_projections
             (projection_sequence, projection_id, workspace_id, run_id, schema,
              projection_revision, source_run_revision, source_control_revision,
              checkpoint_id, checkpoint_sequence, checkpoint_digest, fence_epoch,
              run_status, attention, required_human_id, approval_request_id,
              recovery_directive_id, recovery_directive_digest, projected_at,
              snapshot_json, snapshot_digest)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12,
                     ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21)",
            params![
                i64_safe(projection.projection_sequence)?,
                projection.projection_id.to_string(),
                projection.workspace_id.to_string(),
                projection.run_id.to_string(),
                projection.schema,
                i64_safe(projection.projection_revision)?,
                i64_safe(projection.source_run_revision)?,
                i64_safe(projection.source_control_revision)?,
                projection.checkpoint_id.to_string(),
                i64_safe(projection.checkpoint_sequence)?,
                projection.checkpoint_digest.hex(),
                i64_safe(projection.fence_epoch)?,
                wire(&projection.run_status)?,
                wire(&projection.attention)?,
                projection.required_human_id.map(|id| id.to_string()),
                projection.approval_request_id.map(|id| id.to_string()),
                projection.recovery_directive_id.map(|id| id.to_string()),
                projection
                    .recovery_directive_digest
                    .map(|digest| digest.to_string()),
                projection.projected_at.to_rfc3339(),
                json(projection)?,
                projection.snapshot_digest.to_string(),
            ],
        )
        .map_err(map_db)?;
    Ok(())
}

fn projection_values(
    projection: &RunProjection,
) -> Result<Vec<rusqlite::types::Value>, OperatorStoreError> {
    use rusqlite::types::Value;
    let text = |value: String| Value::Text(value);
    let optional = |value: Option<String>| value.map_or(Value::Null, Value::Text);
    Ok(vec![
        Value::Integer(i64_safe(projection.projection_sequence)?),
        text(projection.projection_id.to_string()),
        text(projection.workspace_id.to_string()),
        text(projection.run_id.to_string()),
        text(projection.schema.clone()),
        Value::Integer(i64_safe(projection.projection_revision)?),
        Value::Integer(i64_safe(projection.source_run_revision)?),
        Value::Integer(i64_safe(projection.source_control_revision)?),
        text(projection.checkpoint_id.to_string()),
        Value::Integer(i64_safe(projection.checkpoint_sequence)?),
        text(projection.checkpoint_digest.hex()),
        Value::Integer(i64_safe(projection.fence_epoch)?),
        text(wire(&projection.run_status)?),
        text(wire(&projection.attention)?),
        optional(projection.required_human_id.map(|id| id.to_string())),
        optional(projection.approval_request_id.map(|id| id.to_string())),
        optional(projection.recovery_directive_id.map(|id| id.to_string())),
        optional(
            projection
                .recovery_directive_digest
                .map(|value| value.to_string()),
        ),
        text(projection.projected_at.to_rfc3339()),
        text(json(projection)?),
        text(projection.snapshot_digest.to_string()),
    ])
}

fn read_projection_values(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<Vec<rusqlite::types::Value>> {
    let mut values = Vec::with_capacity(21);
    for index in 0..21 {
        values.push(row.get(index)?);
    }
    Ok(values)
}

fn decode_projection_values(
    values: Vec<rusqlite::types::Value>,
    expected_run_id: Uuid,
) -> Result<RunProjection, OperatorStoreError> {
    let serialized = match values.get(19) {
        Some(rusqlite::types::Value::Text(value)) => value,
        _ => return Err(OperatorStoreError::Corrupt),
    };
    let projection: RunProjection = decode(serialized)?;
    projection
        .validate()
        .map_err(|_| OperatorStoreError::Corrupt)?;
    if projection.run_id != expected_run_id || values != projection_values(&projection)? {
        return Err(OperatorStoreError::Corrupt);
    }
    Ok(projection)
}

fn load_latest_projection_exact(
    transaction: &Transaction<'_>,
    run_id: Uuid,
) -> Result<RunProjection, OperatorStoreError> {
    let values = transaction
        .query_row(
            "SELECT projection_sequence, projection_id, workspace_id, run_id, schema,
                    projection_revision, source_run_revision, source_control_revision,
                    checkpoint_id, checkpoint_sequence, checkpoint_digest, fence_epoch,
                    run_status, attention, required_human_id, approval_request_id,
                    recovery_directive_id, recovery_directive_digest, projected_at,
                    snapshot_json, snapshot_digest
             FROM operator_run_projections WHERE run_id=?1
             ORDER BY projection_sequence DESC LIMIT 1",
            [run_id.to_string()],
            read_projection_values,
        )
        .map_err(|error| match error {
            rusqlite::Error::QueryReturnedNoRows => OperatorStoreError::Corrupt,
            other => map_db(other),
        })?;
    decode_projection_values(values, run_id)
}

fn load_commit_projection_exact(
    transaction: &Transaction<'_>,
    run_id: Uuid,
    source_run_revision: u64,
    source_control_revision: u64,
) -> Result<RunProjection, OperatorStoreError> {
    let values = transaction
        .query_row(
            "SELECT projection_sequence, projection_id, workspace_id, run_id, schema,
                    projection_revision, source_run_revision, source_control_revision,
                    checkpoint_id, checkpoint_sequence, checkpoint_digest, fence_epoch,
                    run_status, attention, required_human_id, approval_request_id,
                    recovery_directive_id, recovery_directive_digest, projected_at,
                    snapshot_json, snapshot_digest
             FROM operator_run_projections
             WHERE run_id=?1 AND source_run_revision=?2 AND source_control_revision=?3
             ORDER BY projection_sequence ASC LIMIT 1",
            params![
                run_id.to_string(),
                i64_safe(source_run_revision)?,
                i64_safe(source_control_revision)?,
            ],
            read_projection_values,
        )
        .map_err(|error| match error {
            rusqlite::Error::QueryReturnedNoRows => OperatorStoreError::Corrupt,
            other => map_db(other),
        })?;
    decode_projection_values(values, run_id)
}

fn append_current_projection(
    store: &SqliteStore,
    transaction: &Transaction<'_>,
    control: &RunControl,
    fence_epoch: u64,
    now: DateTime<Utc>,
) -> Result<RunProjection, OperatorStoreError> {
    let serialized: String = transaction
        .query_row(
            "SELECT snapshot_json FROM operator_run_projections
             WHERE run_id = ?1 ORDER BY projection_sequence DESC LIMIT 1",
            [control.run_id.to_string()],
            |row| row.get(0),
        )
        .map_err(|error| match error {
            rusqlite::Error::QueryReturnedNoRows => OperatorStoreError::Corrupt,
            other => map_db(other),
        })?;
    let previous: RunProjection = decode(&serialized)?;
    previous
        .validate()
        .map_err(|_| OperatorStoreError::Corrupt)?;
    if previous.run_id != control.run_id || previous.workspace_id != control.workspace_id {
        return Err(OperatorStoreError::Corrupt);
    }
    let sequence: i64 = transaction
        .query_row(
            "SELECT COALESCE(MAX(projection_sequence), 0) + 1
             FROM operator_run_projections",
            [],
            |row| row.get(0),
        )
        .map_err(map_db)?;
    let mut projection = previous;
    projection.projection_id = store.operator_uuid()?;
    projection.projection_sequence = u64_safe(sequence)?;
    projection.projection_revision = projection
        .projection_revision
        .checked_add(1)
        .filter(|revision| *revision <= MAX_SAFE)
        .ok_or(OperatorStoreError::Corrupt)?;
    projection.source_control_revision = control.control_revision;
    projection.fence_epoch = fence_epoch;
    let run_json: String = transaction
        .query_row(
            "SELECT run_json FROM agent_runs WHERE id = ?1",
            [control.run_id.to_string()],
            |row| row.get(0),
        )
        .map_err(map_db)?;
    let run: AgentRun = decode(&run_json)?;
    if run.id != control.run_id {
        return Err(OperatorStoreError::Corrupt);
    }
    projection.source_run_revision = run.revision;
    projection.run_status = run.status;
    let checkpoint: (String, i64, String) = transaction
        .query_row(
            "SELECT id, sequence, state_digest FROM agent_checkpoints
             WHERE run_id=?1 ORDER BY sequence DESC LIMIT 1",
            [control.run_id.to_string()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .map_err(|error| match error {
            rusqlite::Error::QueryReturnedNoRows => OperatorStoreError::Corrupt,
            other => map_db(other),
        })?;
    projection.checkpoint_id = uuid(&checkpoint.0)?;
    projection.checkpoint_sequence = u64_safe(checkpoint.1)?;
    projection.checkpoint_digest = decode(&format!("\"{}\"", checkpoint.2))?;
    projection.recovery_directive_id = control.recovery_directive_id;
    projection.recovery_directive_digest = control.recovery_directive_digest;
    match run.status {
        AgentRunStatus::Queued | AgentRunStatus::Running => {
            projection.attention = AttentionState::Running;
            projection.required_human_id = None;
            projection.approval_request_id = None;
            projection.recovery_directive_id = None;
            projection.recovery_directive_digest = None;
        }
        AgentRunStatus::WaitingForInput => {
            projection.attention = AttentionState::AwaitingDecision;
            let approval: (String, String) = transaction
                .query_row(
                    "SELECT approval_request_id, required_human_id
                     FROM operator_approval_bindings WHERE run_id=?1
                     ORDER BY created_at DESC, approval_request_id DESC LIMIT 1",
                    [control.run_id.to_string()],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .map_err(|error| match error {
                    rusqlite::Error::QueryReturnedNoRows => OperatorStoreError::Corrupt,
                    other => map_db(other),
                })?;
            projection.approval_request_id = Some(uuid(&approval.0)?);
            projection.required_human_id = Some(uuid(&approval.1)?);
            projection.recovery_directive_id = None;
            projection.recovery_directive_digest = None;
        }
        AgentRunStatus::Failed if control.recovery_directive_id.is_some() => {
            projection.attention = AttentionState::Recoverable;
            projection.required_human_id = None;
            projection.approval_request_id = None;
        }
        AgentRunStatus::Succeeded | AgentRunStatus::Failed | AgentRunStatus::Cancelled => {
            projection.attention = AttentionState::Terminal;
            projection.required_human_id = None;
            projection.approval_request_id = None;
            projection.recovery_directive_id = None;
            projection.recovery_directive_digest = None;
        }
    }
    projection.projected_at = now;
    projection.snapshot_digest = digest_without_field(
        "Proof-Operator-Run-Projection-v1",
        &projection,
        "snapshot_digest",
    )?;
    projection
        .validate()
        .map_err(|_| OperatorStoreError::Corrupt)?;
    insert_projection(transaction, &projection)?;
    Ok(projection)
}

fn append_audit_event(
    transaction: &Transaction<'_>,
    event: &mut AuditEvent,
) -> Result<(), OperatorStoreError> {
    let (last_sequence, last_digest): (i64, Option<String>) = transaction
        .query_row(
            "SELECT last_sequence, last_digest FROM operator_audit_heads WHERE workspace_id = ?1",
            [event.workspace_id.to_string()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(map_db)?;
    event.sequence = u64_safe(last_sequence)?
        .checked_add(1)
        .filter(|sequence| *sequence <= MAX_SAFE)
        .ok_or(OperatorStoreError::Corrupt)?;
    event.previous_digest = last_digest
        .as_deref()
        .map(str::parse)
        .transpose()
        .map_err(|_| OperatorStoreError::Corrupt)?;
    event.event_digest =
        digest_without_field("Proof-Operator-Audit-Event-v1", event, "event_digest")?;
    event
        .validate_chain_link(event.sequence, event.previous_digest)
        .map_err(|_| OperatorStoreError::Invalid)?;
    transaction
        .execute(
            "INSERT INTO operator_audit_events
             (workspace_id, sequence, event_id, schema, kind, outcome,
              previous_digest, event_digest, human_id, session_id, challenge_id,
              challenge_digest, session_authority_digest, related_session_id,
              server_instance_id, run_id, approval_request_id, command_id,
              command_kind, budget_id, reservation_id, lease_id, source_lease_id,
              process_epoch_id, permit_id, recovery_directive_id, fence_epoch,
              auth_epoch, policy_revision, intent_digest, call_digest,
              decision_digest, recovery_directive_digest, failure_scope, proof_id,
              proof_operation, proof_digest, occurred_at, event_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12,
                     ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23,
                     ?24, ?25, ?26, ?27, ?28, ?29, ?30, ?31, ?32, ?33, ?34,
                     ?35, ?36, ?37, ?38, ?39)",
            params![
                event.workspace_id.to_string(),
                i64_safe(event.sequence)?,
                event.event_id.to_string(),
                event.schema,
                wire(&event.kind)?,
                wire(&event.outcome)?,
                event.previous_digest.map(|digest| digest.to_string()),
                event.event_digest.to_string(),
                event.human_id.map(|id| id.to_string()),
                event.session_id.map(|id| id.to_string()),
                event.challenge_id.map(|id| id.to_string()),
                event.challenge_digest.map(|digest| digest.to_string()),
                event
                    .session_authority_digest
                    .map(|digest| digest.to_string()),
                event.related_session_id.map(|id| id.to_string()),
                event.server_instance_id.map(|id| id.to_string()),
                event.run_id.map(|id| id.to_string()),
                event.approval_request_id.map(|id| id.to_string()),
                event.command_id.map(|id| id.to_string()),
                event.command_kind.map(|kind| wire(&kind)).transpose()?,
                event.budget_id.map(|id| id.to_string()),
                event.reservation_id.map(|id| id.to_string()),
                event.lease_id.map(|id| id.to_string()),
                event.source_lease_id.map(|id| id.to_string()),
                event.process_epoch_id.map(|id| id.to_string()),
                event.permit_id.map(|id| id.to_string()),
                event.recovery_directive_id.map(|id| id.to_string()),
                event.fence_epoch.map(i64_safe).transpose()?,
                event.auth_epoch.map(i64_safe).transpose()?,
                event.policy_revision.map(i64_safe).transpose()?,
                event.intent_digest.map(|digest| digest.to_string()),
                event.call_digest.map(|digest| digest.to_string()),
                event.decision_digest.map(|digest| digest.hex()),
                event
                    .recovery_directive_digest
                    .map(|digest| digest.to_string()),
                event.failure_scope.map(|scope| wire(&scope)).transpose()?,
                event.proof.as_ref().map(|proof| proof.proof_id.to_string()),
                event.proof.as_ref().map(|proof| proof.operation.clone()),
                event.proof.as_ref().map(|proof| proof.proof_digest.hex()),
                event.occurred_at.to_rfc3339(),
                json(event)?,
            ],
        )
        .map_err(map_db)?;
    let changed = transaction
        .execute(
            "UPDATE operator_audit_heads SET last_sequence = ?2, last_digest = ?3
             WHERE workspace_id = ?1 AND last_sequence = ?4",
            params![
                event.workspace_id.to_string(),
                i64_safe(event.sequence)?,
                event.event_digest.to_string(),
                last_sequence,
            ],
        )
        .map_err(map_db)?;
    if changed != 1 {
        return Err(OperatorStoreError::Corrupt);
    }
    Ok(())
}

fn load_control(
    transaction: &Transaction<'_>,
    run_id: Uuid,
) -> Result<RunControl, OperatorStoreError> {
    let row: (
        String,
        String,
        String,
        String,
        i64,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        String,
        String,
        String,
        String,
    ) = transaction
        .query_row(
            "SELECT run_id, workspace_id, budget_id, schema, control_revision,
                    active_dispatch_reservation_id, recovery_directive_id,
                    recovery_directive_digest, last_command_id, created_at, updated_at,
                    binding_digest, binding_json
             FROM operator_run_control WHERE run_id = ?1",
            [run_id.to_string()],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                    row.get(7)?,
                    row.get(8)?,
                    row.get(9)?,
                    row.get(10)?,
                    row.get(11)?,
                    row.get(12)?,
                ))
            },
        )
        .map_err(|error| match error {
            rusqlite::Error::QueryReturnedNoRows => OperatorStoreError::NotFound,
            other => map_db(other),
        })?;
    let control: RunControl = decode(&row.12)?;
    control
        .validate()
        .map_err(|_| OperatorStoreError::Corrupt)?;
    if row.0 != control.run_id.to_string()
        || row.1 != control.workspace_id.to_string()
        || row.2 != control.budget_id.to_string()
        || row.3 != control.schema
        || u64_safe(row.4)? != control.control_revision
        || row.5
            != control
                .active_dispatch_reservation_id
                .map(|id| id.to_string())
        || row.6 != control.recovery_directive_id.map(|id| id.to_string())
        || row.7
            != control
                .recovery_directive_digest
                .map(|digest| digest.to_string())
        || row.8 != control.last_command_id.map(|id| id.to_string())
        || row.9 != control.created_at.to_rfc3339()
        || row.10 != control.updated_at.to_rfc3339()
        || row.11 != control.binding_digest.to_string()
        || row.12 != json(&control).map_err(|_| OperatorStoreError::Corrupt)?
    {
        return Err(OperatorStoreError::Corrupt);
    }
    Ok(control)
}

fn load_lease(
    transaction: &Transaction<'_>,
    lease_id: Uuid,
) -> Result<RunLease, OperatorStoreError> {
    let row: (
        String,
        String,
        String,
        String,
        String,
        String,
        i64,
        i64,
        String,
        String,
        String,
        String,
        Option<String>,
        String,
        String,
    ) = transaction
        .query_row(
            "SELECT lease_id, run_id, workspace_id, owner_instance_id,
                    process_epoch_id, lease_token_digest, fence_epoch, revision,
                    state, acquired_at, renewed_at, expires_at, released_at,
                    lease_json, lease_digest
             FROM operator_run_leases WHERE lease_id = ?1",
            [lease_id.to_string()],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                    row.get(7)?,
                    row.get(8)?,
                    row.get(9)?,
                    row.get(10)?,
                    row.get(11)?,
                    row.get(12)?,
                    row.get(13)?,
                    row.get(14)?,
                ))
            },
        )
        .map_err(|error| match error {
            rusqlite::Error::QueryReturnedNoRows => OperatorStoreError::NotFound,
            other => map_db(other),
        })?;
    let lease: RunLease = decode(&row.13)?;
    lease.validate().map_err(|_| OperatorStoreError::Corrupt)?;
    if row.0 != lease.lease_id.to_string()
        || row.1 != lease.run_id.to_string()
        || row.2 != lease.workspace_id.to_string()
        || row.3 != lease.owner_instance_id.to_string()
        || row.4 != lease.process_epoch_id.to_string()
        || row.5 != lease.lease_token_digest.to_string()
        || u64_safe(row.6)? != lease.fence_epoch
        || u64_safe(row.7)? != lease.revision
        || row.8 != wire(&lease.state).map_err(|_| OperatorStoreError::Corrupt)?
        || row.9 != lease.acquired_at.to_rfc3339()
        || row.10 != lease.renewed_at.to_rfc3339()
        || row.11 != lease.expires_at.to_rfc3339()
        || row.12 != lease.released_at.map(|at| at.to_rfc3339())
        || row.14 != lease.lease_digest.to_string()
        || row.13 != json(&lease).map_err(|_| OperatorStoreError::Corrupt)?
    {
        return Err(OperatorStoreError::Corrupt);
    }
    Ok(lease)
}

fn validate_authority(
    transaction: &Transaction<'_>,
    authority: &proof_kernel::LeaseAuthority<'_>,
    now: DateTime<Utc>,
) -> Result<(RunControl, RunLease), OperatorStoreError> {
    invalid_if(
        authority.schema != proof_kernel::LeaseAuthority::SCHEMA
            || ![
                authority.workspace_id,
                authority.run_id,
                authority.lease_id,
                authority.owner_instance_id,
                authority.process_epoch_id,
            ]
            .into_iter()
            .all(proof_kernel::uuid_is_v7)
            || authority.fence_epoch == 0
            || authority.fence_epoch > MAX_SAFE
            || authority.expected_control_revision > MAX_SAFE,
    )?;
    let stored_digest: String = transaction
        .query_row(
            "SELECT lease_token_digest FROM operator_run_leases WHERE lease_id = ?1",
            [authority.lease_id.to_string()],
            |row| row.get(0),
        )
        .map_err(|error| match error {
            rusqlite::Error::QueryReturnedNoRows => OperatorStoreError::StaleFence,
            other => map_db(other),
        })?;
    let stored_digest = stored_digest
        .parse::<ControlDigest>()
        .map_err(|_| OperatorStoreError::Corrupt)?;
    if !authority.verifies_lease_token_digest(stored_digest) {
        return Err(OperatorStoreError::StaleFence);
    }
    let control = load_control(transaction, authority.run_id)?;
    let lease = load_lease(transaction, authority.lease_id)?;
    if control.workspace_id != authority.workspace_id
        || control.control_revision != authority.expected_control_revision
        || lease.workspace_id != authority.workspace_id
        || lease.run_id != authority.run_id
        || lease.owner_instance_id != authority.owner_instance_id
        || lease.process_epoch_id != authority.process_epoch_id
        || lease.fence_epoch != authority.fence_epoch
        || lease.state != RunLeaseState::Active
        || now >= lease.expires_at
    {
        return Err(OperatorStoreError::StaleFence);
    }
    Ok((control, lease))
}

fn load_budget(
    transaction: &Transaction<'_>,
    budget_id: Uuid,
) -> Result<BudgetAccount, OperatorStoreError> {
    struct RawBudget {
        budget_id: String,
        workspace_id: String,
        schema: String,
        revision: i64,
        state: String,
        maximum: [i64; 5],
        reserved: [i64; 5],
        committed: [i64; 5],
        deadline_at: String,
        created_at: String,
        updated_at: String,
        limits_digest: String,
        limits_json: String,
    }
    let row = transaction
        .query_row(
            "SELECT budget_id, workspace_id, schema, revision, state,
                    max_steps, max_tokens, max_duration_ms, max_cost_microusd,
                    max_tool_dispatches, reserved_steps, reserved_tokens,
                    reserved_duration_ms, reserved_cost_microusd,
                    reserved_tool_dispatches, committed_steps, committed_tokens,
                    committed_duration_ms, committed_cost_microusd,
                    committed_tool_dispatches, deadline_at, created_at, updated_at,
                    limits_digest, limits_json
             FROM operator_budget_accounts WHERE budget_id = ?1",
            [budget_id.to_string()],
            |row| {
                Ok(RawBudget {
                    budget_id: row.get(0)?,
                    workspace_id: row.get(1)?,
                    schema: row.get(2)?,
                    revision: row.get(3)?,
                    state: row.get(4)?,
                    maximum: [
                        row.get(5)?,
                        row.get(6)?,
                        row.get(7)?,
                        row.get(8)?,
                        row.get(9)?,
                    ],
                    reserved: [
                        row.get(10)?,
                        row.get(11)?,
                        row.get(12)?,
                        row.get(13)?,
                        row.get(14)?,
                    ],
                    committed: [
                        row.get(15)?,
                        row.get(16)?,
                        row.get(17)?,
                        row.get(18)?,
                        row.get(19)?,
                    ],
                    deadline_at: row.get(20)?,
                    created_at: row.get(21)?,
                    updated_at: row.get(22)?,
                    limits_digest: row.get(23)?,
                    limits_json: row.get(24)?,
                })
            },
        )
        .map_err(|error| match error {
            rusqlite::Error::QueryReturnedNoRows => OperatorStoreError::NotFound,
            other => map_db(other),
        })?;
    let policy: proof_kernel::BudgetPolicy = decode(&row.limits_json)?;
    let state = match row.state.as_str() {
        "active" => BudgetAccountState::Active,
        "exhausted" => BudgetAccountState::Exhausted,
        "closed" => BudgetAccountState::Closed,
        _ => return Err(OperatorStoreError::Corrupt),
    };
    let amounts = |values: [i64; 5]| -> Result<BudgetAmounts, OperatorStoreError> {
        Ok(BudgetAmounts {
            steps: u64_safe(values[0])?,
            tokens: u64_safe(values[1])?,
            duration_ms: u64_safe(values[2])?,
            cost_microusd: u64_safe(values[3])?,
            tool_dispatches: u64_safe(values[4])?,
        })
    };
    let account = BudgetAccount {
        schema: row.schema.clone(),
        policy,
        revision: u64_safe(row.revision)?,
        state,
        reserved: amounts(row.reserved)?,
        committed: amounts(row.committed)?,
        created_at: time(&row.created_at)?,
        updated_at: time(&row.updated_at)?,
    };
    account
        .validate()
        .map_err(|_| OperatorStoreError::Corrupt)?;
    if row.budget_id != account.policy.budget_id.to_string()
        || row.workspace_id != account.policy.workspace_id.to_string()
        || row.schema != account.schema
        || amounts(row.maximum)? != account.policy.limits
        || row.deadline_at != account.policy.deadline_at.to_rfc3339()
        || row.limits_digest != account.policy.limits_digest.to_string()
        || row.limits_json != json(&account.policy).map_err(|_| OperatorStoreError::Corrupt)?
    {
        return Err(OperatorStoreError::Corrupt);
    }
    Ok(account)
}

fn load_budget_readonly(
    connection: &rusqlite::Connection,
    budget_id: Uuid,
) -> Result<BudgetAccount, OperatorStoreError> {
    let transaction =
        Transaction::new_unchecked(connection, TransactionBehavior::Deferred).map_err(map_db)?;
    let budget = load_budget(&transaction, budget_id)?;
    transaction.commit().map_err(map_db)?;
    Ok(budget)
}

fn bump_control(control: &mut RunControl, now: DateTime<Utc>) -> Result<(), OperatorStoreError> {
    control.control_revision = control
        .control_revision
        .checked_add(1)
        .filter(|revision| *revision <= MAX_SAFE)
        .ok_or(OperatorStoreError::Corrupt)?;
    control.updated_at = now;
    control.binding_digest =
        digest_without_field("Proof-Operator-Run-Binding-v1", control, "binding_digest")?;
    control.validate().map_err(|_| OperatorStoreError::Corrupt)
}

fn insert_lease(transaction: &Transaction<'_>, lease: &RunLease) -> Result<(), OperatorStoreError> {
    transaction
        .execute(
            "INSERT INTO operator_run_leases
             (lease_id, run_id, workspace_id, owner_instance_id, process_epoch_id,
              lease_token_digest, fence_epoch, revision, state, acquired_at, renewed_at,
              expires_at, released_at, lease_json, lease_digest)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
            params![
                lease.lease_id.to_string(),
                lease.run_id.to_string(),
                lease.workspace_id.to_string(),
                lease.owner_instance_id.to_string(),
                lease.process_epoch_id.to_string(),
                lease.lease_token_digest.to_string(),
                i64_safe(lease.fence_epoch)?,
                i64_safe(lease.revision)?,
                wire(&lease.state)?,
                lease.acquired_at.to_rfc3339(),
                lease.renewed_at.to_rfc3339(),
                lease.expires_at.to_rfc3339(),
                lease.released_at.map(|at| at.to_rfc3339()),
                json(lease)?,
                lease.lease_digest.to_string(),
            ],
        )
        .map_err(map_db)?;
    Ok(())
}

fn update_lease(transaction: &Transaction<'_>, lease: &RunLease) -> Result<(), OperatorStoreError> {
    lease.validate().map_err(|_| OperatorStoreError::Corrupt)?;
    let changed = transaction
        .execute(
            "UPDATE operator_run_leases SET revision = ?2, state = ?3, renewed_at = ?4,
             expires_at = ?5, released_at = ?6, lease_json = ?7, lease_digest = ?8
             WHERE lease_id = ?1",
            params![
                lease.lease_id.to_string(),
                i64_safe(lease.revision)?,
                wire(&lease.state)?,
                lease.renewed_at.to_rfc3339(),
                lease.expires_at.to_rfc3339(),
                lease.released_at.map(|at| at.to_rfc3339()),
                json(lease)?,
                lease.lease_digest.to_string(),
            ],
        )
        .map_err(map_db)?;
    invalid_if(changed != 1).map_err(|_| OperatorStoreError::Corrupt)
}

fn lease_event(
    lease: &RunLease,
    event_id: Uuid,
    kind: AuditEventKind,
    now: DateTime<Utc>,
) -> AuditEvent {
    let mut event = event_base(
        lease.workspace_id,
        event_id,
        kind,
        AuditOutcome::Accepted,
        now,
    );
    event.run_id = Some(lease.run_id);
    event.server_instance_id = Some(lease.owner_instance_id);
    event.lease_id = Some(lease.lease_id);
    event.process_epoch_id = Some(lease.process_epoch_id);
    event.fence_epoch = Some(lease.fence_epoch);
    event
}

fn validate_budget_request(request: &BudgetReserveRequest<'_>) -> Result<(), OperatorStoreError> {
    invalid_if(
        request.schema != "proof.operator.budget-reserve-request/v1"
            || !proof_kernel::uuid_is_v7(request.reservation_id)
            || !proof_kernel::uuid_is_v7(request.idempotency_key)
            || request.intent.validate().is_err()
            || request
                .replay
                .as_ref()
                .is_some_and(|binding| binding.validate().is_err())
            || request
                .recovery
                .as_ref()
                .is_some_and(|directive| directive.validate().is_err())
            || control_digest_serialized("Proof-Operator-Dispatch-Intent-v1", &request.intent)
                .map_err(|_| OperatorStoreError::Invalid)?
                != request.intent_digest,
    )
}

fn budget_request_digest(
    request: &BudgetReserveRequest<'_>,
) -> Result<ControlDigest, OperatorStoreError> {
    #[derive(Serialize)]
    struct AuthorityBinding<'a> {
        schema: &'a str,
        workspace_id: Uuid,
        run_id: Uuid,
        lease_id: Uuid,
        owner_instance_id: Uuid,
        process_epoch_id: Uuid,
        fence_epoch: u64,
        expected_control_revision: u64,
    }
    #[derive(Serialize)]
    struct Binding<'a> {
        schema: &'a str,
        authority: AuthorityBinding<'a>,
        reservation_id: Uuid,
        idempotency_key: Uuid,
        intent: &'a proof_kernel::DispatchIntent,
        intent_digest: ControlDigest,
        replay: &'a Option<proof_kernel::ReplayClaimBinding>,
        recovery: &'a Option<proof_kernel::RecoveryDirective>,
    }
    control_digest_serialized(
        "Proof-Operator-Budget-Reservation-v1",
        &Binding {
            schema: request.schema.as_str(),
            authority: AuthorityBinding {
                schema: request.authority.schema.as_str(),
                workspace_id: request.authority.workspace_id,
                run_id: request.authority.run_id,
                lease_id: request.authority.lease_id,
                owner_instance_id: request.authority.owner_instance_id,
                process_epoch_id: request.authority.process_epoch_id,
                fence_epoch: request.authority.fence_epoch,
                expected_control_revision: request.authority.expected_control_revision,
            },
            reservation_id: request.reservation_id,
            idempotency_key: request.idempotency_key,
            intent: &request.intent,
            intent_digest: request.intent_digest,
            replay: &request.replay,
            recovery: &request.recovery,
        },
    )
    .map_err(|_| OperatorStoreError::Invalid)
}

fn add_amounts(target: &mut BudgetAmounts, value: BudgetAmounts) -> Result<(), OperatorStoreError> {
    target.steps = target
        .steps
        .checked_add(value.steps)
        .ok_or(OperatorStoreError::Corrupt)?;
    target.tokens = target
        .tokens
        .checked_add(value.tokens)
        .ok_or(OperatorStoreError::Corrupt)?;
    target.duration_ms = target
        .duration_ms
        .checked_add(value.duration_ms)
        .ok_or(OperatorStoreError::Corrupt)?;
    target.cost_microusd = target
        .cost_microusd
        .checked_add(value.cost_microusd)
        .ok_or(OperatorStoreError::Corrupt)?;
    target.tool_dispatches = target
        .tool_dispatches
        .checked_add(value.tool_dispatches)
        .ok_or(OperatorStoreError::Corrupt)?;
    invalid_if(!target.is_safe()).map_err(|_| OperatorStoreError::Corrupt)
}

fn subtract_amounts(
    target: &mut BudgetAmounts,
    value: BudgetAmounts,
) -> Result<(), OperatorStoreError> {
    target.steps = target
        .steps
        .checked_sub(value.steps)
        .ok_or(OperatorStoreError::Corrupt)?;
    target.tokens = target
        .tokens
        .checked_sub(value.tokens)
        .ok_or(OperatorStoreError::Corrupt)?;
    target.duration_ms = target
        .duration_ms
        .checked_sub(value.duration_ms)
        .ok_or(OperatorStoreError::Corrupt)?;
    target.cost_microusd = target
        .cost_microusd
        .checked_sub(value.cost_microusd)
        .ok_or(OperatorStoreError::Corrupt)?;
    target.tool_dispatches = target
        .tool_dispatches
        .checked_sub(value.tool_dispatches)
        .ok_or(OperatorStoreError::Corrupt)?;
    Ok(())
}

fn update_budget(
    transaction: &Transaction<'_>,
    budget: &BudgetAccount,
) -> Result<(), OperatorStoreError> {
    budget.validate().map_err(|_| OperatorStoreError::Corrupt)?;
    let changed = transaction
        .execute(
            "UPDATE operator_budget_accounts SET revision = ?2, state = ?3,
         reserved_steps = ?4, reserved_tokens = ?5, reserved_duration_ms = ?6,
         reserved_cost_microusd = ?7, reserved_tool_dispatches = ?8,
         committed_steps = ?9, committed_tokens = ?10, committed_duration_ms = ?11,
         committed_cost_microusd = ?12, committed_tool_dispatches = ?13, updated_at = ?14
         WHERE budget_id = ?1",
            params![
                budget.policy.budget_id.to_string(),
                i64_safe(budget.revision)?,
                wire(&budget.state)?,
                i64_safe(budget.reserved.steps)?,
                i64_safe(budget.reserved.tokens)?,
                i64_safe(budget.reserved.duration_ms)?,
                i64_safe(budget.reserved.cost_microusd)?,
                i64_safe(budget.reserved.tool_dispatches)?,
                i64_safe(budget.committed.steps)?,
                i64_safe(budget.committed.tokens)?,
                i64_safe(budget.committed.duration_ms)?,
                i64_safe(budget.committed.cost_microusd)?,
                i64_safe(budget.committed.tool_dispatches)?,
                budget.updated_at.to_rfc3339(),
            ],
        )
        .map_err(map_db)?;
    if changed != 1 {
        return Err(OperatorStoreError::Corrupt);
    }
    Ok(())
}

fn insert_reservation(
    transaction: &Transaction<'_>,
    reservation: &BudgetReservation,
) -> Result<(), OperatorStoreError> {
    transaction.execute(
        "INSERT INTO operator_budget_reservations
         (reservation_id, budget_id, run_id, lease_id, fence_epoch, idempotency_key,
          request_digest, schema, kind, intent_digest, intent_json,
          replay_binding_digest, replay_operation, replay_version, replay_idempotency_key,
          replay_input_digest, replay_claimed_by, replay_json, recovery_directive_id,
          recovery_directive_digest, recovery_json, state, reserved_steps, reserved_tokens,
          reserved_duration_ms, reserved_cost_microusd, reserved_tool_dispatches,
          charged_steps, charged_tokens, charged_duration_ms, charged_cost_microusd,
          charged_tool_dispatches, created_at, permit_id, dispatch_token_digest, call_digest,
          prepared_execution_digest, result_digest, prepared_binding_json, runtime_commit_json,
          dispatch_started_at, settled_at, reservation_json)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23,?24,?25,?26,?27,?28,?29,?30,?31,?32,?33,?34,?35,?36,?37,?38,?39,?40,?41,?42,?43)",
        rusqlite::params_from_iter(reservation_values(reservation)?),
    ).map_err(map_db)?;
    Ok(())
}

fn reservation_values(
    reservation: &BudgetReservation,
) -> Result<Vec<rusqlite::types::Value>, OperatorStoreError> {
    use rusqlite::types::Value;
    let text = |value: String| Value::Text(value);
    let optional = |value: Option<String>| value.map_or(Value::Null, Value::Text);
    Ok(vec![
        text(reservation.reservation_id.to_string()),
        text(reservation.budget_id.to_string()),
        text(reservation.run_id.to_string()),
        text(reservation.lease_id.to_string()),
        Value::Integer(i64_safe(reservation.fence_epoch)?),
        text(reservation.idempotency_key.to_string()),
        text(reservation.request_digest.to_string()),
        text(reservation.schema.clone()),
        text(wire(&reservation.kind)?),
        text(reservation.intent_digest.to_string()),
        text(json(&reservation.intent)?),
        optional(
            reservation
                .replay
                .as_ref()
                .map(|value| value.binding_digest.to_string()),
        ),
        optional(
            reservation
                .replay
                .as_ref()
                .map(|value| value.operation.clone()),
        ),
        optional(
            reservation
                .replay
                .as_ref()
                .map(|value| value.version.clone()),
        ),
        optional(
            reservation
                .replay
                .as_ref()
                .map(|value| value.idempotency_key.to_string()),
        ),
        optional(
            reservation
                .replay
                .as_ref()
                .map(|value| value.input_digest.hex()),
        ),
        optional(
            reservation
                .replay
                .as_ref()
                .map(|value| value.claimed_by.as_uuid().to_string()),
        ),
        optional(reservation.replay.as_ref().map(json).transpose()?),
        optional(
            reservation
                .recovery
                .as_ref()
                .map(|value| value.directive_id.to_string()),
        ),
        optional(
            reservation
                .recovery
                .as_ref()
                .map(|value| value.directive_digest.to_string()),
        ),
        optional(reservation.recovery.as_ref().map(json).transpose()?),
        text(wire(&reservation.state)?),
        Value::Integer(i64_safe(reservation.reserved.steps)?),
        Value::Integer(i64_safe(reservation.reserved.tokens)?),
        Value::Integer(i64_safe(reservation.reserved.duration_ms)?),
        Value::Integer(i64_safe(reservation.reserved.cost_microusd)?),
        Value::Integer(i64_safe(reservation.reserved.tool_dispatches)?),
        Value::Integer(i64_safe(reservation.charged.steps)?),
        Value::Integer(i64_safe(reservation.charged.tokens)?),
        Value::Integer(i64_safe(reservation.charged.duration_ms)?),
        Value::Integer(i64_safe(reservation.charged.cost_microusd)?),
        Value::Integer(i64_safe(reservation.charged.tool_dispatches)?),
        text(reservation.created_at.to_rfc3339()),
        optional(reservation.permit_id.map(|value| value.to_string())),
        optional(
            reservation
                .dispatch_token_digest
                .map(|value| value.to_string()),
        ),
        optional(reservation.call_digest.map(|value| value.to_string())),
        optional(
            reservation
                .prepared_execution_digest
                .map(|value| value.to_string()),
        ),
        optional(reservation.result_digest.map(|value| value.to_string())),
        optional(
            reservation
                .prepared_binding
                .as_ref()
                .map(json)
                .transpose()?,
        ),
        optional(reservation.runtime_commit.as_ref().map(json).transpose()?),
        optional(
            reservation
                .dispatch_started_at
                .map(|value| value.to_rfc3339()),
        ),
        optional(reservation.settled_at.map(|value| value.to_rfc3339())),
        text(json(reservation)?),
    ])
}

fn update_reservation(
    transaction: &Transaction<'_>,
    reservation: &BudgetReservation,
) -> Result<(), OperatorStoreError> {
    reservation
        .validate()
        .map_err(|_| OperatorStoreError::Corrupt)?;
    let values = reservation_values(reservation)?;
    let changed = transaction
        .execute(
            "UPDATE operator_budget_reservations SET
         budget_id=?2, run_id=?3, lease_id=?4, fence_epoch=?5, idempotency_key=?6,
         request_digest=?7, schema=?8, kind=?9, intent_digest=?10, intent_json=?11,
         replay_binding_digest=?12, replay_operation=?13, replay_version=?14,
         replay_idempotency_key=?15, replay_input_digest=?16, replay_claimed_by=?17,
         replay_json=?18, recovery_directive_id=?19, recovery_directive_digest=?20,
         recovery_json=?21, state=?22, reserved_steps=?23, reserved_tokens=?24,
         reserved_duration_ms=?25, reserved_cost_microusd=?26, reserved_tool_dispatches=?27,
         charged_steps=?28, charged_tokens=?29, charged_duration_ms=?30,
         charged_cost_microusd=?31, charged_tool_dispatches=?32, created_at=?33,
         permit_id=?34, dispatch_token_digest=?35, call_digest=?36,
         prepared_execution_digest=?37, result_digest=?38, prepared_binding_json=?39,
         runtime_commit_json=?40, dispatch_started_at=?41, settled_at=?42,
         reservation_json=?43 WHERE reservation_id=?1",
            rusqlite::params_from_iter(values),
        )
        .map_err(map_db)?;
    if changed != 1 {
        return Err(OperatorStoreError::Corrupt);
    }
    Ok(())
}

fn load_reservation(
    transaction: &Transaction<'_>,
    reservation_id: Uuid,
) -> Result<BudgetReservation, OperatorStoreError> {
    let values: Vec<rusqlite::types::Value> = transaction
        .query_row(
            "SELECT reservation_id, budget_id, run_id, lease_id, fence_epoch,
                    idempotency_key, request_digest, schema, kind, intent_digest, intent_json,
                    replay_binding_digest, replay_operation, replay_version,
                    replay_idempotency_key, replay_input_digest, replay_claimed_by, replay_json,
                    recovery_directive_id, recovery_directive_digest, recovery_json, state,
                    reserved_steps, reserved_tokens, reserved_duration_ms,
                    reserved_cost_microusd, reserved_tool_dispatches, charged_steps,
                    charged_tokens, charged_duration_ms, charged_cost_microusd,
                    charged_tool_dispatches, created_at, permit_id, dispatch_token_digest,
                    call_digest, prepared_execution_digest, result_digest,
                    prepared_binding_json, runtime_commit_json, dispatch_started_at,
                    settled_at, reservation_json
             FROM operator_budget_reservations WHERE reservation_id = ?1",
            [reservation_id.to_string()],
            |row| {
                let mut values = Vec::with_capacity(43);
                for index in 0..43 {
                    values.push(row.get(index)?);
                }
                Ok(values)
            },
        )
        .map_err(|error| match error {
            rusqlite::Error::QueryReturnedNoRows => OperatorStoreError::NotFound,
            other => map_db(other),
        })?;
    let serialized = match values.last() {
        Some(rusqlite::types::Value::Text(value)) => value,
        _ => return Err(OperatorStoreError::Corrupt),
    };
    let reservation: BudgetReservation = decode(&serialized)?;
    reservation
        .validate()
        .map_err(|_| OperatorStoreError::Corrupt)?;
    if reservation.reservation_id != reservation_id || values != reservation_values(&reservation)? {
        return Err(OperatorStoreError::Corrupt);
    }
    Ok(reservation)
}

fn release_reservation(
    transaction: &Transaction<'_>,
    reservation: &mut BudgetReservation,
    budget: &mut BudgetAccount,
    now: DateTime<Utc>,
) -> Result<(), OperatorStoreError> {
    invalid_if(reservation.state != BudgetReservationState::Reserved)?;
    subtract_amounts(&mut budget.reserved, reservation.reserved)?;
    budget.revision = budget
        .revision
        .checked_add(1)
        .ok_or(OperatorStoreError::Corrupt)?;
    budget.updated_at = now;
    reservation.state = BudgetReservationState::Released;
    reservation.settled_at = Some(now);
    update_reservation(transaction, reservation)?;
    update_budget(transaction, budget)
}

fn append_budget_event(
    store: &SqliteStore,
    transaction: &Transaction<'_>,
    control: &RunControl,
    lease: &RunLease,
    reservation_id: Uuid,
    intent_digest: ControlDigest,
    kind: AuditEventKind,
    outcome: AuditOutcome,
    now: DateTime<Utc>,
) -> Result<(), OperatorStoreError> {
    let mut event = event_base(
        control.workspace_id,
        store.operator_uuid()?,
        kind,
        outcome,
        now,
    );
    event.run_id = Some(control.run_id);
    event.budget_id = Some(control.budget_id);
    event.reservation_id = Some(reservation_id);
    event.lease_id = Some(lease.lease_id);
    event.fence_epoch = Some(lease.fence_epoch);
    event.intent_digest = Some(intent_digest);
    append_audit_event(transaction, &mut event)
}

fn append_dispatch_event(
    store: &SqliteStore,
    transaction: &Transaction<'_>,
    control: &RunControl,
    lease: &RunLease,
    reservation: &BudgetReservation,
    now: DateTime<Utc>,
) -> Result<(), OperatorStoreError> {
    let mut event = event_base(
        control.workspace_id,
        store.operator_uuid()?,
        AuditEventKind::DispatchAuthorized,
        AuditOutcome::Accepted,
        now,
    );
    event.run_id = Some(control.run_id);
    event.server_instance_id = Some(lease.owner_instance_id);
    event.budget_id = Some(control.budget_id);
    event.reservation_id = Some(reservation.reservation_id);
    event.lease_id = Some(lease.lease_id);
    event.process_epoch_id = Some(lease.process_epoch_id);
    event.fence_epoch = Some(lease.fence_epoch);
    event.permit_id = reservation.permit_id;
    event.intent_digest = Some(reservation.intent_digest);
    event.call_digest = reservation.call_digest;
    append_audit_event(transaction, &mut event)
}

fn insert_replay_binding(
    transaction: &Transaction<'_>,
    binding: &proof_kernel::ReplayClaimBinding,
    reservation_id: Uuid,
    now: DateTime<Utc>,
) -> Result<(), OperatorStoreError> {
    binding
        .validate()
        .map_err(|_| OperatorStoreError::Invalid)?;
    transaction
        .execute(
            "INSERT INTO operator_replay_bindings
         (operation, version, idempotency_key, workspace_id, run_id, step_id,
          origin_reservation_id, checkpoint_id, checkpoint_sequence, checkpoint_digest,
          input_digest, claimed_by, binding_digest, binding_json, created_at)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15)",
            params![
                binding.operation,
                binding.version,
                binding.idempotency_key.to_string(),
                binding.workspace_id.to_string(),
                binding.run_id.to_string(),
                binding.step_id.to_string(),
                reservation_id.to_string(),
                binding.checkpoint_id.to_string(),
                i64_safe(binding.checkpoint_sequence)?,
                binding.checkpoint_digest.hex(),
                binding.input_digest.hex(),
                binding.claimed_by.as_uuid().to_string(),
                binding.binding_digest.to_string(),
                json(binding)?,
                now.to_rfc3339()
            ],
        )
        .map_err(map_db)?;
    Ok(())
}

fn validate_runtime_event_range(
    transaction: &Transaction<'_>,
    run_id: Uuid,
    first_sequence: u64,
    last_sequence: u64,
) -> Result<(), OperatorStoreError> {
    invalid_if(
        first_sequence > last_sequence || first_sequence > MAX_SAFE || last_sequence > MAX_SAFE,
    )
    .map_err(|_| OperatorStoreError::Corrupt)?;
    let mut statement = transaction
        .prepare(
            "SELECT id, run_id, sequence, kind, data_digest, created_at, event_json
             FROM agent_run_events
             WHERE run_id=?1 AND sequence BETWEEN ?2 AND ?3
             ORDER BY sequence ASC",
        )
        .map_err(map_db)?;
    let rows = statement
        .query_map(
            params![
                run_id.to_string(),
                i64_safe(first_sequence)?,
                i64_safe(last_sequence)?,
            ],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                ))
            },
        )
        .map_err(map_db)?;
    let mut expected = first_sequence;
    for row in rows {
        let row = row.map_err(map_db)?;
        let event: AgentRunEvent = decode(&row.6)?;
        let canonical = canonicalize(&event.data).map_err(|_| OperatorStoreError::Corrupt)?;
        if row.0 != event.id.to_string()
            || row.1 != event.run_id.to_string()
            || u64_safe(row.2)? != u64::from(event.sequence)
            || row.3 != wire(&event.kind).map_err(|_| OperatorStoreError::Corrupt)?
            || row.4 != event.data_digest.hex()
            || row.5 != event.created_at.to_rfc3339()
            || row.6 != json(&event).map_err(|_| OperatorStoreError::Corrupt)?
            || !proof_kernel::uuid_is_v7(event.id)
            || event.run_id != run_id
            || u64::from(event.sequence) != expected
            || event.data_digest != digest(ArtifactKind::AgentEvent, &canonical)
        {
            return Err(OperatorStoreError::Corrupt);
        }
        expected = expected.checked_add(1).ok_or(OperatorStoreError::Corrupt)?;
    }
    if expected
        != last_sequence
            .checked_add(1)
            .ok_or(OperatorStoreError::Corrupt)?
    {
        return Err(OperatorStoreError::Corrupt);
    }
    Ok(())
}

fn load_verified_replay(
    transaction: &Transaction<'_>,
    request: &ReplayLookupRequest,
    catalog: &proof_kernel::OperatorSchemaCatalog,
) -> Result<ReplayLookupResult, OperatorStoreError> {
    request.validate()?;
    type ReplayRow = (
        String,
        String,
        String,
        String,
        String,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        String,
        String,
        String,
        String,
        i64,
        String,
        String,
        String,
        String,
        i64,
        String,
        i64,
        String,
        i64,
        Option<String>,
        Option<String>,
        Option<String>,
        String,
    );
    let row: Option<ReplayRow> = transaction
        .query_row(
            "SELECT e.operation, e.version, e.idempotency_key, e.input_digest, e.state,
                    e.completed_at, e.output_json, e.proof_id, e.proof_json,
                    e.execution_context_id, b.binding_json, b.binding_digest,
                    b.workspace_id, b.run_id, b.checkpoint_sequence, b.checkpoint_id,
                    b.checkpoint_digest, b.step_id, r.run_json, r.revision,
                    s.step_json, s.revision, c.binding_json, c.control_revision,
                    p.actor, p.operation, p.signature, b.origin_reservation_id
             FROM execution_replays e
             JOIN operator_replay_bindings b
               ON b.operation=e.operation AND b.version=e.version
              AND b.idempotency_key=e.idempotency_key
             JOIN agent_runs r ON r.id=b.run_id
             JOIN agent_run_steps s ON s.id=b.step_id AND s.run_id=b.run_id
             JOIN operator_run_control c ON c.run_id=b.run_id
             LEFT JOIN proofs p ON p.id=e.proof_id
             WHERE e.operation=?1 AND e.version=?2 AND e.idempotency_key=?3",
            params![
                request.binding.operation,
                request.binding.version,
                request.binding.idempotency_key.to_string()
            ],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                    row.get(7)?,
                    row.get(8)?,
                    row.get(9)?,
                    row.get(10)?,
                    row.get(11)?,
                    row.get(12)?,
                    row.get(13)?,
                    row.get(14)?,
                    row.get(15)?,
                    row.get(16)?,
                    row.get(17)?,
                    row.get(18)?,
                    row.get(19)?,
                    row.get(20)?,
                    row.get(21)?,
                    row.get(22)?,
                    row.get(23)?,
                    row.get(24)?,
                    row.get(25)?,
                    row.get(26)?,
                    row.get(27)?,
                ))
            },
        )
        .optional()
        .map_err(map_db)?;
    let Some(row) = row else {
        return Ok(ReplayLookupResult {
            schema: ReplayLookupResult::SCHEMA.into(),
            outcome: ReplayLookupOutcome::NotFound,
            completion: None,
        });
    };
    if row.4 != "completed" {
        return Ok(ReplayLookupResult {
            schema: ReplayLookupResult::SCHEMA.into(),
            outcome: ReplayLookupOutcome::NotFound,
            completion: None,
        });
    }
    let stored_binding: proof_kernel::ReplayClaimBinding = decode(&row.10)?;
    stored_binding
        .validate()
        .map_err(|_| OperatorStoreError::Corrupt)?;
    if stored_binding != request.binding {
        return Err(OperatorStoreError::Conflict);
    }
    if row.0 != stored_binding.operation
        || row.1 != stored_binding.version
        || row.2 != stored_binding.idempotency_key.to_string()
        || row.3 != stored_binding.input_digest.hex()
        || row.11 != stored_binding.binding_digest.to_string()
        || row.12 != stored_binding.workspace_id.to_string()
        || row.13 != stored_binding.run_id.to_string()
        || u64_safe(row.14)? != stored_binding.checkpoint_sequence
        || row.15 != stored_binding.checkpoint_id.to_string()
        || row.16 != stored_binding.checkpoint_digest.hex()
        || row.17 != stored_binding.step_id.to_string()
        || row.10 != json(&stored_binding).map_err(|_| OperatorStoreError::Corrupt)?
    {
        return Err(OperatorStoreError::Corrupt);
    }
    let origin_reservation_id = uuid(&row.27)?;
    let reservation = load_reservation(transaction, origin_reservation_id)?;
    if reservation.state != BudgetReservationState::Committed
        || reservation.reservation_id != origin_reservation_id
        || reservation.run_id != stored_binding.run_id
        || reservation.replay.as_ref() != Some(&stored_binding)
    {
        return Err(OperatorStoreError::Corrupt);
    }
    let prepared_binding = reservation
        .prepared_binding
        .as_ref()
        .ok_or(OperatorStoreError::Corrupt)?;
    prepared_binding
        .validate()
        .map_err(|_| OperatorStoreError::Corrupt)?;
    let runtime_commit = reservation
        .runtime_commit
        .as_ref()
        .ok_or(OperatorStoreError::Corrupt)?;
    runtime_commit
        .validate()
        .map_err(|_| OperatorStoreError::Corrupt)?;
    if prepared_binding.replay_binding_digest != Some(stored_binding.binding_digest)
        || prepared_binding.payload_digest != runtime_commit.prepared_execution_digest
        || prepared_binding.result_digest != runtime_commit.result_digest
        || prepared_binding.result.usage.boundary_kind != reservation.kind
        || prepared_binding.result.usage.adapter != reservation.intent.adapter
        || prepared_binding.result.usage.model != reservation.intent.model
        || prepared_binding.result.usage.input_digest != stored_binding.input_digest
        || prepared_binding.result.usage.steps != reservation.charged.steps
        || prepared_binding.result.usage.tokens != reservation.charged.tokens
        || prepared_binding.result.usage.cost_microusd != reservation.charged.cost_microusd
        || prepared_binding.result.usage.tool_dispatches != reservation.charged.tool_dispatches
    {
        return Err(OperatorStoreError::Corrupt);
    }
    let completed_at = row.5.as_deref().ok_or(OperatorStoreError::Corrupt)?;
    let output_json = row.6.as_deref().ok_or(OperatorStoreError::Corrupt)?;
    let proof_id = row.7.as_deref().ok_or(OperatorStoreError::Corrupt)?;
    let proof_json = row.8.as_deref().ok_or(OperatorStoreError::Corrupt)?;
    let context_id = row.9.as_deref().ok_or(OperatorStoreError::Corrupt)?;
    let output: serde_json::Value = decode(output_json)?;
    let canonical_output = canonicalize(&output).map_err(|_| OperatorStoreError::Corrupt)?;
    if canonical_output.as_str() != output_json {
        return Err(OperatorStoreError::Corrupt);
    }
    catalog
        .validate_output(&stored_binding.operation, &stored_binding.version, &output)
        .map_err(|_| OperatorStoreError::Corrupt)?;
    let proof: proof_kernel::Proof = decode(proof_json)?;
    let expected_qualified = format!("{}::{}", stored_binding.operation, stored_binding.version);
    let expected_actor = proof.body.actor.as_uuid().to_string();
    let persisted_proof: (
        String,
        String,
        String,
        Option<String>,
        String,
        String,
        String,
        String,
        Option<String>,
        String,
    ) = transaction
        .query_row(
            "SELECT id, actor, version, delegation_id, operation, input_digest,
                    output_digest, timestamp, expires_at, signature
             FROM proofs WHERE id=?1",
            [proof_id],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                    row.get(7)?,
                    row.get(8)?,
                    row.get(9)?,
                ))
            },
        )
        .map_err(|error| match error {
            rusqlite::Error::QueryReturnedNoRows => OperatorStoreError::Corrupt,
            other => map_db(other),
        })?;
    if proof.body.id.to_string() != proof_id
        || proof.body.actor != stored_binding.claimed_by
        || proof.body.operation != expected_qualified
        || proof.body.input_digest != stored_binding.input_digest
        || proof.body.output_digest != prepared_binding.result.output_digest
        || proof.body.timestamp.to_rfc3339() != completed_at
        || row.24.as_deref() != Some(expected_actor.as_str())
        || row.25.as_deref() != Some(proof.body.operation.as_str())
        || row.26.as_deref() != Some(proof_json)
        || persisted_proof.0 != proof_id
        || persisted_proof.1 != expected_actor
        || persisted_proof.2 != stored_binding.version
        || persisted_proof.3 != proof.body.delegation_id.map(|id| id.to_string())
        || persisted_proof.4 != expected_qualified
        || persisted_proof.5 != proof.body.input_digest.hex()
        || persisted_proof.6 != proof.body.output_digest.hex()
        || persisted_proof.7 != proof.body.timestamp.to_rfc3339()
        || persisted_proof.8 != proof.body.expires_at.map(|at| at.to_rfc3339())
        || persisted_proof.9 != proof_json
    {
        return Err(OperatorStoreError::Corrupt);
    }
    let principal = load_principal_record(transaction, proof.body.actor.as_uuid())?;
    if principal.kind != PrincipalKind::Agent || proof.verify(&principal.public_key).is_err() {
        return Err(OperatorStoreError::Corrupt);
    }
    let context: (String, String, Option<String>, String, String) = transaction
        .query_row(
            "SELECT id, actor, delegation_id, workspace_path, timestamp
             FROM execution_contexts WHERE id=?1",
            [context_id],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )
        .map_err(|error| match error {
            rusqlite::Error::QueryReturnedNoRows => OperatorStoreError::Corrupt,
            other => map_db(other),
        })?;
    if context.0 != context_id
        || context.1 != proof.body.actor.as_uuid().to_string()
        || context.2 != proof.body.delegation_id.map(|id| id.to_string())
        || context.3.is_empty()
        || !std::path::Path::new(&context.3).is_absolute()
        || context.4 != proof.body.timestamp.to_rfc3339()
        || prepared_binding.execution_context_id.to_string() != context_id
    {
        return Err(OperatorStoreError::Corrupt);
    }
    let run: AgentRun = decode(&row.18)?;
    let step: AgentRunStep = decode(&row.20)?;
    let control: RunControl = decode(&row.22)?;
    control
        .validate()
        .map_err(|_| OperatorStoreError::Corrupt)?;
    let exact_run = load_agent_run_exact(transaction, stored_binding.run_id)?;
    let exact_step = load_agent_step_exact(transaction, stored_binding.step_id)?;
    let exact_control = load_control(transaction, stored_binding.run_id)?;
    let proof_reference = ProofReference {
        proof_id: proof.body.id,
        actor_id: proof.body.actor.as_uuid(),
        operation: stored_binding.operation.clone(),
        proof_digest: proof
            .proof_digest()
            .map_err(|_| OperatorStoreError::Corrupt)?,
    };
    if run.id != stored_binding.run_id
        || step.id != stored_binding.step_id
        || step.run_id != stored_binding.run_id
        || control.run_id != stored_binding.run_id
        || u64_safe(row.19)? != run.revision
        || u64_safe(row.21)? != step.revision
        || u64_safe(row.23)? != control.control_revision
        || step.proof.as_ref() != Some(&proof)
        || step.output.as_ref() != Some(&output)
        || run != exact_run
        || step != exact_step
        || control != exact_control
        || prepared_binding.result.proof != proof_reference
        || prepared_binding.result.run_revision > run.revision
        || prepared_binding.result.step_revision > step.revision
        || match run.mode {
            AgentRunMode::OneShot => {
                runtime_commit.expected_run_revision.checked_add(1)
                    != Some(prepared_binding.result.run_revision)
            }
            AgentRunMode::Session => {
                runtime_commit.expected_run_revision != prepared_binding.result.run_revision
            }
        }
        || runtime_commit.expected_step_revision.checked_add(1)
            != Some(prepared_binding.result.step_revision)
        || runtime_commit.expected_checkpoint_id != stored_binding.checkpoint_id
        || runtime_commit.expected_checkpoint_sequence != stored_binding.checkpoint_sequence
        || runtime_commit.expected_checkpoint_digest != stored_binding.checkpoint_digest
        || reservation.settled_at != Some(runtime_commit.committed_at)
    {
        return Err(OperatorStoreError::Corrupt);
    }
    let resulting_control_revision = runtime_commit
        .permit
        .expected_control_revision
        .checked_add(2)
        .filter(|revision| *revision <= MAX_SAFE)
        .ok_or(OperatorStoreError::Corrupt)?;
    if control.control_revision < resulting_control_revision {
        return Err(OperatorStoreError::Corrupt);
    }
    let committed_checkpoint = match (
        prepared_binding.result.checkpoint_id,
        prepared_binding.result.checkpoint_sequence,
        prepared_binding.result.checkpoint_digest,
    ) {
        (Some(id), Some(sequence), Some(checkpoint_digest)) => (id, sequence, checkpoint_digest),
        (None, None, None) => (
            stored_binding.checkpoint_id,
            stored_binding.checkpoint_sequence,
            stored_binding.checkpoint_digest,
        ),
        _ => return Err(OperatorStoreError::Corrupt),
    };
    if load_checkpoint_identity_exact(transaction, committed_checkpoint.0, run.id)?
        != committed_checkpoint
    {
        return Err(OperatorStoreError::Corrupt);
    }
    let commit_projection = load_commit_projection_exact(
        transaction,
        run.id,
        prepared_binding.result.run_revision,
        resulting_control_revision,
    )?;
    if commit_projection.workspace_id != stored_binding.workspace_id
        || commit_projection.checkpoint_id != committed_checkpoint.0
        || commit_projection.checkpoint_sequence != committed_checkpoint.1
        || commit_projection.checkpoint_digest != committed_checkpoint.2
        || commit_projection.fence_epoch != reservation.fence_epoch
        || commit_projection.projected_at != runtime_commit.committed_at
    {
        return Err(OperatorStoreError::Corrupt);
    }
    let latest_projection = load_latest_projection_exact(transaction, run.id)?;
    let latest_checkpoint = load_latest_checkpoint_identity(transaction, run.id)?;
    if latest_projection.workspace_id != stored_binding.workspace_id
        || latest_projection.source_run_revision != run.revision
        || latest_projection.source_control_revision != control.control_revision
        || latest_projection.run_status != run.status
        || latest_checkpoint
            != (
                latest_projection.checkpoint_id,
                latest_projection.checkpoint_sequence,
                latest_projection.checkpoint_digest,
            )
    {
        return Err(OperatorStoreError::Corrupt);
    }
    validate_runtime_event_range(
        transaction,
        run.id,
        prepared_binding.result.first_event_sequence,
        prepared_binding.result.last_event_sequence,
    )?;
    let envelope = ReplayProofEnvelope {
        schema: ReplayProofEnvelope::SCHEMA.into(),
        proof_id: proof.body.id,
        actor_id: proof.body.actor.as_uuid(),
        delegation_id: proof.body.delegation_id,
        operation: stored_binding.operation.clone(),
        version: stored_binding.version.clone(),
        input_digest: proof.body.input_digest,
        output_digest: proof.body.output_digest,
        timestamp: proof.body.timestamp,
        expires_at: proof.body.expires_at,
        signature: base64url(&proof.signature.to_bytes()),
    };
    let completion = ReplayCompletionBinding {
        schema: ReplayCompletionBinding::SCHEMA.into(),
        replay_binding_digest: stored_binding.binding_digest,
        workspace_id: stored_binding.workspace_id,
        run_id: stored_binding.run_id,
        step_id: stored_binding.step_id,
        checkpoint_id: stored_binding.checkpoint_id,
        checkpoint_sequence: stored_binding.checkpoint_sequence,
        checkpoint_digest: stored_binding.checkpoint_digest,
        existing_run_revision: run.revision,
        existing_step_revision: step.revision,
        existing_control_revision: control.control_revision,
        canonical_output_json: output_json.to_owned(),
        input_digest: stored_binding.input_digest,
        output_digest: proof.body.output_digest,
        proof: envelope,
    };
    completion
        .validate()
        .map_err(|_| OperatorStoreError::Corrupt)?;
    Ok(ReplayLookupResult {
        schema: ReplayLookupResult::SCHEMA.into(),
        outcome: ReplayLookupOutcome::Completed,
        completion: Some(completion),
    })
}

fn base64url(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut output = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let bits = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        output.push(ALPHABET[((bits >> 18) & 63) as usize] as char);
        output.push(ALPHABET[((bits >> 12) & 63) as usize] as char);
        if chunk.len() > 1 {
            output.push(ALPHABET[((bits >> 6) & 63) as usize] as char);
        }
        if chunk.len() > 2 {
            output.push(ALPHABET[(bits & 63) as usize] as char);
        }
    }
    output
}

fn open_page_window(
    query_schema: &str,
    expected_schema: &str,
    page_size: u64,
    cursor_value: Option<&str>,
    scope: OperatorReadScope,
    route: OperatorReadRoute,
    cursor: &dyn OperatorCursorCodec,
) -> Result<VerifiedPageWindow, OperatorStoreError> {
    invalid_if(query_schema != expected_schema || !(1..=100).contains(&page_size))?;
    scope.validate()?;
    invalid_if(scope.route != route)?;
    let window = cursor
        .open_page(scope, cursor_value, page_size)
        .map_err(|error| match error {
            OperatorCursorError::Stale => OperatorStoreError::Conflict,
            OperatorCursorError::Unavailable => OperatorStoreError::Unavailable,
        })?;
    window.validate()?;
    invalid_if(cursor_value.is_some() != matches!(window.kind, PageWindowKind::Continuation))?;
    Ok(window)
}

fn validate_filter_digest<T: Serialize>(
    scope: &OperatorReadScope,
    query_without_cursor: &T,
) -> Result<(), OperatorStoreError> {
    let expected =
        control_digest_serialized("Proof-Operator-Cursor-Filter-v1", query_without_cursor)
            .map_err(|_| OperatorStoreError::Invalid)?;
    invalid_if(scope.filter_digest != Some(expected))
}

fn load_principal_record(
    transaction: &Transaction<'_>,
    principal_id: Uuid,
) -> Result<Principal, OperatorStoreError> {
    let (raw_id, raw_kind, raw_key): (String, String, Vec<u8>) = transaction
        .query_row(
            "SELECT id, kind, public_key FROM principals WHERE id = ?1",
            [principal_id.to_string()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .map_err(|error| match error {
            rusqlite::Error::QueryReturnedNoRows => OperatorStoreError::Corrupt,
            other => map_db(other),
        })?;
    let id = uuid(&raw_id)?;
    let kind: PrincipalKind = decode(&raw_kind)?;
    let bytes: [u8; 32] = raw_key
        .try_into()
        .map_err(|_| OperatorStoreError::Corrupt)?;
    let public_key =
        ed25519_dalek::VerifyingKey::from_bytes(&bytes).map_err(|_| OperatorStoreError::Corrupt)?;
    if id != principal_id {
        return Err(OperatorStoreError::Corrupt);
    }
    Ok(Principal {
        id: PrincipalId::new(id),
        kind,
        public_key,
        created_at: time("1970-01-01T00:00:00Z")?,
    })
}

fn validate_projection_run(
    projection: &RunProjection,
    run: &AgentRun,
    sequence: u64,
    projection_id: Uuid,
) -> Result<(), OperatorStoreError> {
    projection
        .validate()
        .map_err(|_| OperatorStoreError::Corrupt)?;
    if projection.projection_sequence != sequence
        || projection.projection_id != projection_id
        || projection.run_id != run.id
        || projection.source_run_revision != run.revision
        || projection.run_status != run.status
    {
        return Err(OperatorStoreError::Corrupt);
    }
    Ok(())
}

fn bounded_summary(value: &str) -> String {
    let normalized = value.split_whitespace().collect::<Vec<_>>().join(" ");
    let normalized = if normalized.is_empty() {
        "Governed run".to_owned()
    } else {
        normalized
    };
    normalized.chars().take(240).collect()
}

fn approval_state(
    decision: Option<&SignedApprovalDecision>,
    request: &SignedApprovalRequest,
    now: DateTime<Utc>,
) -> ApprovalState {
    match decision.map(|decision| decision.body.outcome) {
        Some(ApprovalOutcome::Approved) => ApprovalState::Approved,
        Some(ApprovalOutcome::Denied) => ApprovalState::Denied,
        None if now >= request.body.expires_at => ApprovalState::Expired,
        None => ApprovalState::Pending,
    }
}

fn validate_approval_record(
    transaction: &Transaction<'_>,
    binding: &ApprovalBinding,
    request: &SignedApprovalRequest,
    decision: Option<&SignedApprovalDecision>,
) -> Result<(), OperatorStoreError> {
    let consequence = PendingConsequenceBody {
        classification: binding.consequence.classification,
        summary: binding.consequence.summary.clone(),
    };
    let argument_digest = control_digest_serialized(
        "Proof-Operator-Approval-Argument-v1",
        &binding.review_fields,
    )
    .map_err(|_| OperatorStoreError::Corrupt)?;
    let consequence_digest =
        control_digest_serialized("Proof-Operator-Approval-Consequence-v1", &consequence)
            .map_err(|_| OperatorStoreError::Corrupt)?;
    let binding_digest = digest_without_field(
        "Proof-Operator-Approval-Binding-v1",
        binding,
        "binding_digest",
    )?;
    if binding.schema != ApprovalBinding::SCHEMA
        || ![
            binding.approval_request_id,
            binding.workspace_id,
            binding.run_id,
            binding.step_id,
            binding.checkpoint_id,
            binding.required_human_id,
        ]
        .into_iter()
        .all(proof_kernel::uuid_is_v7)
        || binding.approval_request_id != request.body.id
        || binding.input_digest != request.body.input_digest
        || binding.argument_digest != argument_digest
        || binding.consequence_digest != consequence_digest
        || binding.consequence.consequence_digest != consequence_digest
        || binding.binding_digest != binding_digest
    {
        return Err(OperatorStoreError::Corrupt);
    }
    request
        .verify(&load_principal_record(
            transaction,
            request.body.requested_by.as_uuid(),
        )?)
        .map_err(|_| OperatorStoreError::Corrupt)?;
    if let Some(decision) = decision {
        decision
            .verify(&load_principal_record(
                transaction,
                decision.body.decided_by.as_uuid(),
            )?)
            .map_err(|_| OperatorStoreError::Corrupt)?;
        if decision.body.request_id != request.body.id
            || decision.body.request_digest
                != request.digest().map_err(|_| OperatorStoreError::Corrupt)?
            || decision.body.decided_by.as_uuid() != binding.required_human_id
        {
            return Err(OperatorStoreError::Corrupt);
        }
    }
    Ok(())
}

fn approval_summary(
    binding: &ApprovalBinding,
    request: &SignedApprovalRequest,
    decision: Option<&SignedApprovalDecision>,
    now: DateTime<Utc>,
) -> ApprovalSummary {
    ApprovalSummary {
        approval_request_id: binding.approval_request_id,
        run_id: binding.run_id,
        step_id: binding.step_id,
        required_human_id: binding.required_human_id,
        operation: request.body.operation.clone(),
        version: request.body.version.clone(),
        input_digest: request.body.input_digest,
        state: approval_state(decision, request, now),
        requested_at: request.body.requested_at,
        expires_at: request.body.expires_at,
    }
}

fn proof_reference(proof: &proof_kernel::Proof) -> Result<ProofReference, OperatorStoreError> {
    let operation = proof
        .body
        .operation
        .split_once("::")
        .map(|(operation, _)| operation.to_owned())
        .ok_or(OperatorStoreError::Corrupt)?;
    let reference = ProofReference {
        proof_id: proof.body.id,
        actor_id: proof.body.actor.as_uuid(),
        operation,
        proof_digest: proof
            .proof_digest()
            .map_err(|_| OperatorStoreError::Corrupt)?,
    };
    reference
        .validate()
        .map_err(|_| OperatorStoreError::Corrupt)?;
    Ok(reference)
}

fn budget_snapshot(budget: &BudgetAccount) -> Result<BudgetSnapshot, OperatorStoreError> {
    let remaining = BudgetAmounts {
        steps: budget
            .policy
            .limits
            .steps
            .checked_sub(budget.reserved.steps)
            .and_then(|value| value.checked_sub(budget.committed.steps))
            .ok_or(OperatorStoreError::Corrupt)?,
        tokens: budget
            .policy
            .limits
            .tokens
            .checked_sub(budget.reserved.tokens)
            .and_then(|value| value.checked_sub(budget.committed.tokens))
            .ok_or(OperatorStoreError::Corrupt)?,
        duration_ms: budget
            .policy
            .limits
            .duration_ms
            .checked_sub(budget.reserved.duration_ms)
            .and_then(|value| value.checked_sub(budget.committed.duration_ms))
            .ok_or(OperatorStoreError::Corrupt)?,
        cost_microusd: budget
            .policy
            .limits
            .cost_microusd
            .checked_sub(budget.reserved.cost_microusd)
            .and_then(|value| value.checked_sub(budget.committed.cost_microusd))
            .ok_or(OperatorStoreError::Corrupt)?,
        tool_dispatches: budget
            .policy
            .limits
            .tool_dispatches
            .checked_sub(budget.reserved.tool_dispatches)
            .and_then(|value| value.checked_sub(budget.committed.tool_dispatches))
            .ok_or(OperatorStoreError::Corrupt)?,
    };
    Ok(BudgetSnapshot {
        schema: "proof.operator.budget-snapshot/v1".into(),
        budget_id: budget.policy.budget_id,
        revision: budget.revision,
        state: budget.state,
        limits: budget.policy.limits,
        reserved: budget.reserved,
        committed: budget.committed,
        remaining,
        deadline_at: budget.policy.deadline_at,
    })
}

fn canonical_slice<T: Ord>(values: &[T]) -> bool {
    values.windows(2).all(|pair| pair[0] < pair[1])
}

fn canonical_wire_slice<T: Serialize>(values: &[T]) -> Result<bool, OperatorStoreError> {
    let values = values.iter().map(wire).collect::<Result<Vec<_>, _>>()?;
    Ok(canonical_slice(&values))
}

fn next_cursor(
    cursor: &dyn OperatorCursorCodec,
    scope: OperatorReadScope,
    page_size: u64,
    high_water_sequence: u64,
    last_sequence: u64,
    last_id: Uuid,
) -> Result<String, OperatorStoreError> {
    cursor
        .seal_page(
            scope,
            page_size,
            high_water_sequence,
            last_sequence,
            last_id,
        )
        .map_err(|error| match error {
            OperatorCursorError::Stale => OperatorStoreError::Conflict,
            OperatorCursorError::Unavailable => OperatorStoreError::Unavailable,
        })
}

fn empty_page(page_size: u64, high_water_sequence: u64) -> PageInfo {
    PageInfo {
        page_size,
        returned: 0,
        high_water_sequence,
        next_cursor: None,
    }
}

fn command_kind(command: &proof_kernel::OperatorCommand) -> CommandKind {
    match command {
        proof_kernel::OperatorCommand::ApprovalDecision(_) => CommandKind::ApprovalDecide,
        proof_kernel::OperatorCommand::RunCancel(_) => CommandKind::RunCancel,
        proof_kernel::OperatorCommand::RunResume(_) => CommandKind::RunResume,
        proof_kernel::OperatorCommand::SessionRevoke(_) => CommandKind::SessionRevoke,
    }
}

fn validate_operator_command(
    command: &proof_kernel::OperatorCommand,
) -> Result<(), OperatorStoreError> {
    let result = match command {
        proof_kernel::OperatorCommand::ApprovalDecision(value) => value.validate(),
        proof_kernel::OperatorCommand::RunCancel(value) => value.validate(),
        proof_kernel::OperatorCommand::RunResume(value) => value.validate(),
        proof_kernel::OperatorCommand::SessionRevoke(value) => {
            if value.schema != proof_kernel::SessionRevokeRequest::SCHEMA {
                return Err(OperatorStoreError::Invalid);
            }
            value.binding.validate()
        }
    };
    result.map_err(|_| OperatorStoreError::Corrupt)
}

fn validate_command_receipt_binding(
    receipt: &CommandReceipt,
    envelope: &CommandEnvelope,
    sequence: u64,
    receipt_id: Uuid,
) -> Result<(), OperatorStoreError> {
    receipt
        .validate()
        .map_err(|_| OperatorStoreError::Corrupt)?;
    validate_operator_command(&envelope.command)?;
    let binding = envelope.command.binding();
    let expected_digest = control_digest_serialized("Proof-Operator-Command-v1", &envelope.command)
        .map_err(|_| OperatorStoreError::Corrupt)?;
    if envelope.schema != CommandEnvelope::SCHEMA
        || envelope.request_digest != expected_digest
        || receipt.request_digest != expected_digest
        || receipt.receipt_id != receipt_id
        || receipt.audit_sequence != sequence
        || receipt.command_id != binding.command_id
        || receipt.idempotency_key != binding.idempotency_key
        || receipt.workspace_id != binding.workspace_id
        || receipt.human_id != binding.human_id
        || receipt.kind != command_kind(&envelope.command)
    {
        return Err(OperatorStoreError::Corrupt);
    }
    Ok(())
}

impl OperatorReadStore for SqliteStore {
    fn page_attention(
        &self,
        query: AttentionQuery,
        scope: OperatorReadScope,
        cursor: &dyn OperatorCursorCodec,
    ) -> Result<AttentionPage, OperatorStoreError> {
        self.operator_context()?;
        invalid_if(!canonical_slice(&query.kinds) || !canonical_slice(&query.states))?;
        let expected_capabilities = match query.kinds.as_slice() {
            [AttentionKind::Approval] => vec![Capability::ApprovalRead],
            [AttentionKind::Run] => vec![Capability::RunRead],
            [AttentionKind::Approval, AttentionKind::Run] | [] => {
                vec![Capability::ApprovalRead, Capability::RunRead]
            }
            _ => return Err(OperatorStoreError::Invalid),
        };
        invalid_if(scope.required_capabilities != expected_capabilities)?;
        let mut filter = query.clone();
        filter.cursor = None;
        validate_filter_digest(&scope, &filter)?;
        let window = open_page_window(
            &query.schema,
            "proof.operator.attention-query/v1",
            query.page_size,
            query.cursor.as_deref(),
            scope.clone(),
            OperatorReadRoute::Attention,
            cursor,
        )?;
        let connection = self
            .conn
            .lock()
            .map_err(|_| OperatorStoreError::Unavailable)?;
        let transaction = Transaction::new_unchecked(&connection, TransactionBehavior::Deferred)
            .map_err(map_db)?;
        Self::validate_read_scope(&transaction, &scope, OperatorReadRoute::Attention)?;
        let high = match window.high_water_sequence {
            Some(value) => value,
            None => u64_safe(transaction.query_row(
                "SELECT COALESCE(MAX(projection_sequence), 0) FROM operator_run_projections",
                [],
                |row| row.get(0),
            ).map_err(map_db)?)?,
        };
        let mut statement = transaction
            .prepare(
                "SELECT p.projection_sequence, p.projection_id, p.snapshot_json,
                        r.run_json, a.request_json
                 FROM operator_run_projections p
                 JOIN agent_runs r ON r.id = p.run_id
                 LEFT JOIN approval_requests a ON a.id = p.approval_request_id
                 WHERE p.workspace_id = ?1 AND p.projection_sequence <= ?2
                   AND NOT EXISTS (
                     SELECT 1 FROM operator_run_projections newer
                     WHERE newer.run_id = p.run_id
                       AND newer.projection_sequence <= ?2
                       AND newer.projection_sequence > p.projection_sequence)
                 ORDER BY p.projection_sequence DESC, p.projection_id DESC",
            )
            .map_err(map_db)?;
        let rows = statement
            .query_map(
                params![scope.workspace_id.to_string(), i64_safe(high)?],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, Option<String>>(4)?,
                    ))
                },
            )
            .map_err(map_db)?;
        let mut items = Vec::new();
        let limit = query.page_size as usize + 1;
        for row in rows {
            let (sequence, raw_id, projection_json, run_json, approval_json) =
                row.map_err(map_db)?;
            let sequence = u64_safe(sequence)?;
            let projection_id = uuid(&raw_id)?;
            if matches!(window.kind, PageWindowKind::Continuation)
                && !(sequence < window.last_sequence.unwrap_or(0)
                    || (sequence == window.last_sequence.unwrap_or(0)
                        && projection_id < window.last_id.ok_or(OperatorStoreError::Corrupt)?))
            {
                continue;
            }
            let projection: RunProjection = decode(&projection_json)?;
            let run: AgentRun = decode(&run_json)?;
            validate_projection_run(&projection, &run, sequence, projection_id)?;
            if !query.states.is_empty() && !query.states.contains(&projection.attention) {
                continue;
            }
            let kind = if projection.attention == AttentionState::AwaitingDecision {
                AttentionKind::Approval
            } else {
                AttentionKind::Run
            };
            if !query.kinds.is_empty() && !query.kinds.contains(&kind) {
                continue;
            }
            let item = if kind == AttentionKind::Approval {
                let request_json = approval_json.ok_or(OperatorStoreError::Corrupt)?;
                let request: SignedApprovalRequest = decode(&request_json)?;
                request
                    .verify(&load_principal_record(
                        &transaction,
                        request.body.requested_by.as_uuid(),
                    )?)
                    .map_err(|_| OperatorStoreError::Corrupt)?;
                let approval_request_id = projection
                    .approval_request_id
                    .ok_or(OperatorStoreError::Corrupt)?;
                let required_human_id = projection
                    .required_human_id
                    .ok_or(OperatorStoreError::Corrupt)?;
                if request.body.id != approval_request_id {
                    return Err(OperatorStoreError::Corrupt);
                }
                AttentionItem::Approval(ApprovalAttentionItem {
                    schema: "proof.operator.attention-item.approval/v1".into(),
                    kind,
                    projection_sequence: projection.projection_sequence,
                    projection_id: projection.projection_id,
                    run_id: run.id,
                    approval_request_id,
                    required_human_id,
                    operation: request.body.operation,
                    version: request.body.version,
                    input_digest: request.body.input_digest,
                    urgency: Urgency::Critical,
                    expires_at: request.body.expires_at,
                    run_revision: run.revision,
                    control_revision: projection.source_control_revision,
                    fence_epoch: projection.fence_epoch,
                    projected_at: projection.projected_at,
                })
            } else {
                AttentionItem::Run(RunAttentionItem {
                    schema: "proof.operator.attention-item.run/v1".into(),
                    kind,
                    projection_sequence: projection.projection_sequence,
                    projection_id: projection.projection_id,
                    run_id: run.id,
                    run_status: run.status,
                    attention: projection.attention,
                    urgency: if projection.attention == AttentionState::Recoverable {
                        Urgency::High
                    } else {
                        Urgency::Normal
                    },
                    goal_summary: bounded_summary(&run.goal),
                    run_revision: run.revision,
                    control_revision: projection.source_control_revision,
                    fence_epoch: projection.fence_epoch,
                    projected_at: projection.projected_at,
                })
            };
            items.push(item);
            if items.len() == limit {
                break;
            }
        }
        drop(statement);
        let has_more = items.len() > query.page_size as usize;
        if has_more {
            items.pop();
        }
        let next = if has_more {
            let (last_sequence, last_id) = match items.last().ok_or(OperatorStoreError::Corrupt)? {
                AttentionItem::Run(item) => (item.projection_sequence, item.projection_id),
                AttentionItem::Approval(item) => (item.projection_sequence, item.projection_id),
            };
            Some(next_cursor(
                cursor,
                scope,
                query.page_size,
                high,
                last_sequence,
                last_id,
            )?)
        } else {
            None
        };
        transaction.commit().map_err(map_db)?;
        Ok(AttentionPage {
            schema: "proof.operator.attention-page/v1".into(),
            page: PageInfo {
                page_size: query.page_size,
                returned: items.len() as u64,
                high_water_sequence: high,
                next_cursor: next,
            },
            items,
        })
    }

    fn load_run_detail(
        &self,
        run_id: Uuid,
        scope: OperatorReadScope,
    ) -> Result<Option<RunDetail>, OperatorStoreError> {
        self.operator_context()?;
        invalid_if(!proof_kernel::uuid_is_v7(run_id))?;
        let connection = self
            .conn
            .lock()
            .map_err(|_| OperatorStoreError::Unavailable)?;
        let transaction = Transaction::new_unchecked(&connection, TransactionBehavior::Deferred)
            .map_err(map_db)?;
        Self::validate_read_scope(&transaction, &scope, OperatorReadRoute::RunDetail)?;
        let stored = transaction
            .query_row(
                "SELECT r.run_json, c.binding_json, p.snapshot_json
                 FROM agent_runs r
                 JOIN operator_run_control c ON c.run_id = r.id
                 JOIN operator_run_projections p ON p.run_id = r.id
                 WHERE r.id = ?1 AND c.workspace_id = ?2
                 ORDER BY p.projection_sequence DESC LIMIT 1",
                params![run_id.to_string(), scope.workspace_id.to_string()],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                },
            )
            .optional()
            .map_err(map_db)?;
        let Some((run_json, control_json, projection_json)) = stored else {
            transaction.commit().map_err(map_db)?;
            return Ok(None);
        };
        let run: AgentRun = decode(&run_json)?;
        let control: RunControl = decode(&control_json)?;
        let projection: RunProjection = decode(&projection_json)?;
        control
            .validate()
            .map_err(|_| OperatorStoreError::Corrupt)?;
        validate_projection_run(
            &projection,
            &run,
            projection.projection_sequence,
            projection.projection_id,
        )?;
        if run.id != run_id
            || control.run_id != run_id
            || control.workspace_id != scope.workspace_id
            || projection.source_control_revision != control.control_revision
        {
            return Err(OperatorStoreError::Corrupt);
        }

        let mut step_statement = transaction
            .prepare(
                "SELECT step_json FROM agent_run_steps WHERE run_id = ?1
                 ORDER BY ordinal ASC, attempt ASC, id ASC",
            )
            .map_err(map_db)?;
        let step_rows = step_statement
            .query_map([run_id.to_string()], |row| row.get::<_, String>(0))
            .map_err(map_db)?;
        let mut steps = Vec::new();
        for row in step_rows {
            let step: AgentRunStep = decode(&row.map_err(map_db)?)?;
            if step.run_id != run_id {
                return Err(OperatorStoreError::Corrupt);
            }
            steps.push(step);
        }
        drop(step_statement);
        let latest_step = steps.last();
        let mut evidence = Vec::new();
        let mut attempts = Vec::with_capacity(steps.len());
        for step in &steps {
            let proof = step.proof.as_ref().map(proof_reference).transpose()?;
            if let Some(reference) = proof.clone() {
                evidence.push((
                    step.proof
                        .as_ref()
                        .ok_or(OperatorStoreError::Corrupt)?
                        .body
                        .timestamp,
                    reference,
                ));
            }
            attempts.push(RunAttemptSummary {
                step_id: step.id,
                ordinal: u64::from(step.ordinal),
                attempt: u64::from(step.attempt),
                retry_of: step.retry_of,
                status: step.status,
                operation: step.operation.clone(),
                version: step.version.clone(),
                input_digest: step.input_digest,
                output_digest: step.proof.as_ref().map(|proof| proof.body.output_digest),
                proof,
                error_class: step.error.as_ref().map(|_| "execution_failed".into()),
                revision: step.revision,
                started_at: step.started_at,
                finished_at: step.completed_at,
            });
        }
        evidence.sort_by_key(|(timestamp, reference)| (*timestamp, reference.proof_id));
        let evidence = evidence
            .into_iter()
            .map(|(_, reference)| reference)
            .collect();

        let (checkpoint_id, checkpoint_sequence, checkpoint_digest): (String, i64, String) =
            transaction
                .query_row(
                    "SELECT id, sequence, state_digest FROM agent_checkpoints
                     WHERE run_id = ?1 ORDER BY sequence DESC LIMIT 1",
                    [run_id.to_string()],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )
                .map_err(map_db)?;
        let checkpoint_tail = CheckpointTail {
            checkpoint_id: uuid(&checkpoint_id)?,
            sequence: u64_safe(checkpoint_sequence)?,
            state_digest: decode(&format!("\"{checkpoint_digest}\""))?,
        };
        if checkpoint_tail.checkpoint_id != projection.checkpoint_id
            || checkpoint_tail.sequence != projection.checkpoint_sequence
            || checkpoint_tail.state_digest != projection.checkpoint_digest
        {
            return Err(OperatorStoreError::Corrupt);
        }

        let pending_approval = if let Some(approval_id) = projection.approval_request_id {
            let (binding_json, request_json, decision_json): (String, String, Option<String>) =
                transaction
                    .query_row(
                        "SELECT b.binding_json, a.request_json, d.decision_json
                         FROM operator_approval_bindings b
                         JOIN approval_requests a ON a.id = b.approval_request_id
                         LEFT JOIN approval_decisions d ON d.request_id = b.approval_request_id
                         WHERE b.approval_request_id = ?1",
                        [approval_id.to_string()],
                        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                    )
                    .map_err(map_db)?;
            let binding: ApprovalBinding = decode(&binding_json)?;
            let request: SignedApprovalRequest = decode(&request_json)?;
            let decision: Option<SignedApprovalDecision> =
                decision_json.as_deref().map(decode).transpose()?;
            validate_approval_record(&transaction, &binding, &request, decision.as_ref())?;
            Some(PendingApprovalSummary {
                approval_request_id: approval_id,
                required_human_id: binding.required_human_id,
                operation: request.body.operation,
                version: request.body.version,
                input_digest: request.body.input_digest,
                pending_consequence: binding.consequence,
                expires_at: request.body.expires_at,
                decision: match decision.as_ref().map(|value| value.body.outcome) {
                    Some(ApprovalOutcome::Approved) => PendingDecision::Approved,
                    Some(ApprovalOutcome::Denied) => PendingDecision::Denied,
                    None => PendingDecision::None,
                },
            })
        } else {
            None
        };

        let recovery = if let Some(directive_id) = projection.recovery_directive_id {
            let serialized: String = transaction
                .query_row(
                    "SELECT directive_json FROM operator_recovery_directives
                     WHERE directive_id = ?1 AND run_id = ?2",
                    params![directive_id.to_string(), run_id.to_string()],
                    |row| row.get(0),
                )
                .map_err(map_db)?;
            let directive: RecoveryDirective = decode(&serialized)?;
            directive
                .validate()
                .map_err(|_| OperatorStoreError::Corrupt)?;
            Some(RecoverySummary {
                directive_id: directive.directive_id,
                classification: directive.classification,
                checkpoint_id: directive.checkpoint_id,
                checkpoint_sequence: directive.checkpoint_sequence,
                checkpoint_digest: directive.checkpoint_digest,
                source_lease_id: directive.source_lease_id,
                source_fence_epoch: directive.source_fence_epoch,
                source_control_revision: directive.source_control_revision,
                intent_digest: directive.intent_digest,
                required_budget_disposition: directive.required_budget_disposition,
                directive_digest: directive.directive_digest,
            })
        } else {
            None
        };
        let budget = load_budget(&transaction, control.budget_id)?;
        let workspace_json: String = transaction
            .query_row(
                "SELECT binding_json FROM operator_workspaces WHERE singleton = 1",
                [],
                |row| row.get(0),
            )
            .map_err(map_db)?;
        let workspace: OperatorWorkspace = decode(&workspace_json)?;
        workspace
            .validate()
            .map_err(|_| OperatorStoreError::Corrupt)?;
        let agent_id = run.agent_id.ok_or(OperatorStoreError::Corrupt)?;
        let operation = latest_step
            .map(|step| step.operation.clone())
            .unwrap_or_else(|| "agent.run".into());
        let version = latest_step
            .map(|step| step.version.clone())
            .unwrap_or_else(|| "v1".into());
        let detail = RunDetail {
            schema: "proof.operator.run-detail/v1".into(),
            run_id,
            mode: run.mode,
            status: run.status,
            attention: projection.attention,
            goal_summary: bounded_summary(&run.goal),
            authority: AuthoritySummary {
                agent_id,
                human_id: projection.required_human_id,
                operation,
                version,
                delegation_id: latest_step
                    .and_then(|step| step.proof.as_ref())
                    .and_then(|proof| proof.body.delegation_id),
                policy_revision: workspace.policy_revision,
            },
            attempts,
            evidence,
            run_revision: run.revision,
            control_revision: control.control_revision,
            fence_epoch: projection.fence_epoch,
            checkpoint_tail,
            pending_approval,
            recovery,
            budget: budget_snapshot(&budget)?,
            created_at: run.created_at,
            updated_at: run.updated_at,
            completed_at: run.completed_at,
        };
        transaction.commit().map_err(map_db)?;
        Ok(Some(detail))
    }

    fn page_approvals(
        &self,
        query: ApprovalQuery,
        scope: OperatorReadScope,
        cursor: &dyn OperatorCursorCodec,
    ) -> Result<ApprovalPage, OperatorStoreError> {
        self.operator_context()?;
        invalid_if(!canonical_slice(&query.states))?;
        let mut filter = query.clone();
        filter.cursor = None;
        validate_filter_digest(&scope, &filter)?;
        let window = open_page_window(
            &query.schema,
            "proof.operator.approval-query/v1",
            query.page_size,
            query.cursor.as_deref(),
            scope.clone(),
            OperatorReadRoute::Approvals,
            cursor,
        )?;
        let connection = self
            .conn
            .lock()
            .map_err(|_| OperatorStoreError::Unavailable)?;
        let transaction = Transaction::new_unchecked(&connection, TransactionBehavior::Deferred)
            .map_err(map_db)?;
        Self::validate_read_scope(&transaction, &scope, OperatorReadRoute::Approvals)?;
        let high = match window.high_water_sequence {
            Some(value) => value,
            None => u64_safe(
                transaction
                    .query_row(
                        "SELECT COALESCE(MAX(projection_sequence), 0)
                         FROM operator_run_projections WHERE workspace_id = ?1",
                        [scope.workspace_id.to_string()],
                        |row| row.get(0),
                    )
                    .map_err(map_db)?,
            )?,
        };
        let now = self.operator_now()?;
        let mut statement = transaction
            .prepare(
                "SELECT MAX(p.projection_sequence), b.approval_request_id,
                        b.binding_json, a.request_json, d.decision_json
                 FROM operator_approval_bindings b
                 JOIN approval_requests a ON a.id = b.approval_request_id
                 JOIN operator_run_projections p
                   ON p.approval_request_id = b.approval_request_id
                  AND p.projection_sequence <= ?2
                 LEFT JOIN approval_decisions d ON d.request_id = b.approval_request_id
                 WHERE b.workspace_id = ?1
                 GROUP BY b.approval_request_id, b.binding_json, a.request_json,
                          d.decision_json
                 ORDER BY MAX(p.projection_sequence) DESC,
                          b.approval_request_id DESC",
            )
            .map_err(map_db)?;
        let rows = statement
            .query_map(
                params![scope.workspace_id.to_string(), i64_safe(high)?],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, Option<String>>(4)?,
                    ))
                },
            )
            .map_err(map_db)?;
        let mut items = Vec::new();
        let limit = query.page_size as usize + 1;
        for row in rows {
            let (sequence, raw_id, binding_json, request_json, decision_json) =
                row.map_err(map_db)?;
            let sequence = u64_safe(sequence)?;
            let id = uuid(&raw_id)?;
            if matches!(window.kind, PageWindowKind::Continuation)
                && !(sequence < window.last_sequence.unwrap_or(0)
                    || (sequence == window.last_sequence.unwrap_or(0)
                        && id < window.last_id.ok_or(OperatorStoreError::Corrupt)?))
            {
                continue;
            }
            let binding: ApprovalBinding = decode(&binding_json)?;
            let request: SignedApprovalRequest = decode(&request_json)?;
            let decision: Option<SignedApprovalDecision> =
                decision_json.as_deref().map(decode).transpose()?;
            validate_approval_record(&transaction, &binding, &request, decision.as_ref())?;
            if id != binding.approval_request_id {
                return Err(OperatorStoreError::Corrupt);
            }
            let summary = approval_summary(&binding, &request, decision.as_ref(), now);
            if !query.states.is_empty() && !query.states.contains(&summary.state) {
                continue;
            }
            items.push((sequence, summary));
            if items.len() == limit {
                break;
            }
        }
        drop(statement);
        let has_more = items.len() > query.page_size as usize;
        if has_more {
            items.pop();
        }
        let next = if has_more {
            let (last_sequence, last) = items.last().ok_or(OperatorStoreError::Corrupt)?;
            Some(next_cursor(
                cursor,
                scope,
                query.page_size,
                high,
                *last_sequence,
                last.approval_request_id,
            )?)
        } else {
            None
        };
        let items: Vec<ApprovalSummary> = items.into_iter().map(|(_, item)| item).collect();
        transaction.commit().map_err(map_db)?;
        Ok(ApprovalPage {
            schema: "proof.operator.approval-page/v1".into(),
            page: PageInfo {
                page_size: query.page_size,
                returned: items.len() as u64,
                high_water_sequence: high,
                next_cursor: next,
            },
            items,
        })
    }

    fn load_approval_detail(
        &self,
        request_id: Uuid,
        scope: OperatorReadScope,
    ) -> Result<Option<ApprovalDetail>, OperatorStoreError> {
        self.operator_context()?;
        invalid_if(!proof_kernel::uuid_is_v7(request_id))?;
        let connection = self
            .conn
            .lock()
            .map_err(|_| OperatorStoreError::Unavailable)?;
        let transaction = Transaction::new_unchecked(&connection, TransactionBehavior::Deferred)
            .map_err(map_db)?;
        Self::validate_read_scope(&transaction, &scope, OperatorReadRoute::ApprovalDetail)?;
        let stored = transaction
            .query_row(
                "SELECT b.binding_json, a.request_json, d.decision_json,
                        r.run_json, s.step_json, c.binding_json, p.snapshot_json
                 FROM operator_approval_bindings b
                 JOIN approval_requests a ON a.id = b.approval_request_id
                 JOIN agent_runs r ON r.id = b.run_id
                 JOIN agent_run_steps s ON s.id = b.step_id
                 JOIN operator_run_control c ON c.run_id = b.run_id
                 JOIN operator_run_projections p
                   ON p.approval_request_id = b.approval_request_id
                 LEFT JOIN approval_decisions d ON d.request_id = b.approval_request_id
                 WHERE b.approval_request_id = ?1 AND b.workspace_id = ?2
                 ORDER BY p.projection_sequence DESC LIMIT 1",
                params![request_id.to_string(), scope.workspace_id.to_string()],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, Option<String>>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, String>(5)?,
                        row.get::<_, String>(6)?,
                    ))
                },
            )
            .optional()
            .map_err(map_db)?;
        let Some((
            binding_json,
            request_json,
            decision_json,
            run_json,
            step_json,
            control_json,
            projection_json,
        )) = stored
        else {
            transaction.commit().map_err(map_db)?;
            return Ok(None);
        };
        let binding: ApprovalBinding = decode(&binding_json)?;
        let request: SignedApprovalRequest = decode(&request_json)?;
        let decision: Option<SignedApprovalDecision> =
            decision_json.as_deref().map(decode).transpose()?;
        let run: AgentRun = decode(&run_json)?;
        let step: proof_kernel::AgentRunStep = decode(&step_json)?;
        let control: RunControl = decode(&control_json)?;
        let projection: RunProjection = decode(&projection_json)?;
        validate_approval_record(&transaction, &binding, &request, decision.as_ref())?;
        control
            .validate()
            .map_err(|_| OperatorStoreError::Corrupt)?;
        projection
            .validate()
            .map_err(|_| OperatorStoreError::Corrupt)?;
        if binding.approval_request_id != request_id
            || binding.run_id != run.id
            || binding.step_id != step.id
            || step.run_id != run.id
            || control.run_id != run.id
            || projection.run_id != run.id
            || projection.approval_request_id != Some(request_id)
        {
            return Err(OperatorStoreError::Corrupt);
        }
        let now = self.operator_now()?;
        let decision_summary = decision
            .as_ref()
            .map(|decision| {
                Ok(ApprovalDecisionSummary {
                    decision_id: decision.body.id,
                    decided_by: decision.body.decided_by.as_uuid(),
                    outcome: match decision.body.outcome {
                        ApprovalOutcome::Approved => DecisionOutcome::Approved,
                        ApprovalOutcome::Denied => DecisionOutcome::Denied,
                    },
                    decision_digest: decision.digest().map_err(|_| OperatorStoreError::Corrupt)?,
                    decided_at: decision.body.decided_at,
                })
            })
            .transpose()?;
        let detail = ApprovalDetail {
            schema: "proof.operator.approval-detail/v1".into(),
            summary: approval_summary(&binding, &request, decision.as_ref(), now),
            request_digest: request.digest().map_err(|_| OperatorStoreError::Corrupt)?,
            checkpoint: CheckpointTail {
                checkpoint_id: binding.checkpoint_id,
                sequence: binding.checkpoint_sequence,
                state_digest: binding.checkpoint_digest,
            },
            run_revision: run.revision,
            step_revision: step.revision,
            control_revision: control.control_revision,
            fence_epoch: projection.fence_epoch,
            argument_digest: binding.argument_digest,
            consequence_digest: binding.consequence_digest,
            binding_digest: binding.binding_digest,
            pending_consequence: binding.consequence.clone(),
            review_fields: binding.review_fields.clone(),
            decision: decision_summary,
        };
        transaction.commit().map_err(map_db)?;
        Ok(Some(detail))
    }

    fn page_commands(
        &self,
        query: CommandQuery,
        scope: OperatorReadScope,
        cursor: &dyn OperatorCursorCodec,
    ) -> Result<CommandPage, OperatorStoreError> {
        self.operator_context()?;
        invalid_if(
            !canonical_wire_slice(&query.kinds)? || !canonical_wire_slice(&query.outcomes)?,
        )?;
        invalid_if(query.run_id.is_some_and(|id| !proof_kernel::uuid_is_v7(id)))?;
        let mut filter = query.clone();
        filter.cursor = None;
        validate_filter_digest(&scope, &filter)?;
        let window = open_page_window(
            &query.schema,
            "proof.operator.command-query/v1",
            query.page_size,
            query.cursor.as_deref(),
            scope.clone(),
            OperatorReadRoute::Commands,
            cursor,
        )?;
        let connection = self
            .conn
            .lock()
            .map_err(|_| OperatorStoreError::Unavailable)?;
        let transaction = Transaction::new_unchecked(&connection, TransactionBehavior::Deferred)
            .map_err(map_db)?;
        Self::validate_read_scope(&transaction, &scope, OperatorReadRoute::Commands)?;
        let high = match window.high_water_sequence {
            Some(value) => value,
            None => u64_safe(
                transaction
                    .query_row(
                        "SELECT COALESCE(MAX(audit_sequence), 0)
                         FROM operator_command_receipts WHERE workspace_id = ?1",
                        [scope.workspace_id.to_string()],
                        |row| row.get(0),
                    )
                    .map_err(map_db)?,
            )?,
        };
        let mut statement = transaction
            .prepare(
                "SELECT r.audit_sequence, r.receipt_id, r.receipt_json, c.command_json
                 FROM operator_command_receipts r
                 JOIN operator_commands c ON c.command_id = r.command_id
                 WHERE r.workspace_id = ?1 AND r.audit_sequence <= ?2
                 ORDER BY r.audit_sequence DESC, r.receipt_id DESC",
            )
            .map_err(map_db)?;
        let rows = statement
            .query_map(
                params![scope.workspace_id.to_string(), i64_safe(high)?],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                    ))
                },
            )
            .map_err(map_db)?;
        let mut items = Vec::new();
        let limit = query.page_size as usize + 1;
        for row in rows {
            let (sequence, raw_id, serialized, command_json) = row.map_err(map_db)?;
            let sequence = u64_safe(sequence)?;
            let id = uuid(&raw_id)?;
            if matches!(window.kind, PageWindowKind::Continuation)
                && !(sequence < window.last_sequence.unwrap_or(0)
                    || (sequence == window.last_sequence.unwrap_or(0)
                        && id < window.last_id.ok_or(OperatorStoreError::Corrupt)?))
            {
                continue;
            }
            let receipt: CommandReceipt = decode(&serialized)?;
            let envelope: CommandEnvelope = decode(&command_json)?;
            validate_command_receipt_binding(&receipt, &envelope, sequence, id)?;
            if (!query.kinds.is_empty() && !query.kinds.contains(&receipt.kind))
                || (!query.outcomes.is_empty() && !query.outcomes.contains(&receipt.outcome))
                || query
                    .run_id
                    .is_some_and(|run_id| receipt.target_run_id != Some(run_id))
            {
                continue;
            }
            items.push(receipt);
            if items.len() == limit {
                break;
            }
        }
        drop(statement);
        let has_more = items.len() > query.page_size as usize;
        if has_more {
            items.pop();
        }
        let next = if has_more {
            let last = items.last().ok_or(OperatorStoreError::Corrupt)?;
            Some(next_cursor(
                cursor,
                scope,
                query.page_size,
                high,
                last.audit_sequence,
                last.receipt_id,
            )?)
        } else {
            None
        };
        transaction.commit().map_err(map_db)?;
        Ok(CommandPage {
            schema: "proof.operator.command-page/v1".into(),
            page: PageInfo {
                page_size: query.page_size,
                returned: items.len() as u64,
                high_water_sequence: high,
                next_cursor: next,
            },
            items,
        })
    }

    fn load_command_receipt(
        &self,
        command_id: Uuid,
        scope: OperatorReadScope,
    ) -> Result<Option<CommandReceipt>, OperatorStoreError> {
        self.operator_context()?;
        let connection = self
            .conn
            .lock()
            .map_err(|_| OperatorStoreError::Unavailable)?;
        let transaction = Transaction::new_unchecked(&connection, TransactionBehavior::Deferred)
            .map_err(map_db)?;
        Self::validate_read_scope(&transaction, &scope, OperatorReadRoute::CommandDetail)?;
        let stored = transaction
            .query_row(
                "SELECT r.audit_sequence, r.receipt_id, r.receipt_json, c.command_json
                 FROM operator_command_receipts r
                 JOIN operator_commands c ON c.command_id = r.command_id
                 WHERE r.command_id = ?1 AND r.workspace_id = ?2",
                params![command_id.to_string(), scope.workspace_id.to_string()],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                    ))
                },
            )
            .optional()
            .map_err(map_db)?;
        let result = stored
            .map(|(sequence, raw_id, serialized, command_json)| {
                let sequence = u64_safe(sequence)?;
                let id = uuid(&raw_id)?;
                let receipt: CommandReceipt = decode(&serialized)?;
                let envelope: CommandEnvelope = decode(&command_json)?;
                validate_command_receipt_binding(&receipt, &envelope, sequence, id)?;
                Ok(receipt)
            })
            .transpose()?;
        transaction.commit().map_err(map_db)?;
        Ok(result)
    }

    fn page_operator_audit(
        &self,
        query: AuditQuery,
        scope: OperatorReadScope,
        cursor: &dyn OperatorCursorCodec,
    ) -> Result<AuditPage, OperatorStoreError> {
        self.operator_context()?;
        invalid_if(!canonical_wire_slice(&query.kinds)?)?;
        invalid_if(
            query.run_id.is_some_and(|id| !proof_kernel::uuid_is_v7(id))
                || query
                    .approval_request_id
                    .is_some_and(|id| !proof_kernel::uuid_is_v7(id)),
        )?;
        let mut filter = query.clone();
        filter.cursor = None;
        validate_filter_digest(&scope, &filter)?;
        let window = open_page_window(
            &query.schema,
            "proof.operator.audit-query/v1",
            query.page_size,
            query.cursor.as_deref(),
            scope.clone(),
            OperatorReadRoute::Audit,
            cursor,
        )?;
        let connection = self
            .conn
            .lock()
            .map_err(|_| OperatorStoreError::Unavailable)?;
        let transaction = Transaction::new_unchecked(&connection, TransactionBehavior::Deferred)
            .map_err(map_db)?;
        Self::validate_read_scope(&transaction, &scope, OperatorReadRoute::Audit)?;
        let high = match window.high_water_sequence {
            Some(value) => value,
            None => u64_safe(
                transaction
                    .query_row(
                        "SELECT COALESCE(MAX(sequence), 0) FROM operator_audit_events
                         WHERE workspace_id = ?1",
                        [scope.workspace_id.to_string()],
                        |row| row.get(0),
                    )
                    .map_err(map_db)?,
            )?,
        };
        let mut statement = transaction
            .prepare(
                "SELECT sequence, event_id, event_json FROM operator_audit_events
                 WHERE workspace_id = ?1 AND sequence <= ?2
                 ORDER BY sequence DESC, event_id DESC",
            )
            .map_err(map_db)?;
        let rows = statement
            .query_map(
                params![scope.workspace_id.to_string(), i64_safe(high)?],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                },
            )
            .map_err(map_db)?;
        let mut items = Vec::new();
        let limit = query.page_size as usize + 1;
        for row in rows {
            let (sequence, raw_id, serialized) = row.map_err(map_db)?;
            let sequence = u64_safe(sequence)?;
            let id = uuid(&raw_id)?;
            if matches!(window.kind, PageWindowKind::Continuation)
                && !(sequence < window.last_sequence.unwrap_or(0)
                    || (sequence == window.last_sequence.unwrap_or(0)
                        && id < window.last_id.ok_or(OperatorStoreError::Corrupt)?))
            {
                continue;
            }
            let event: AuditEvent = decode(&serialized)?;
            event
                .validate_chain_link(event.sequence, event.previous_digest)
                .map_err(|_| OperatorStoreError::Corrupt)?;
            if event.sequence != sequence || event.event_id != id {
                return Err(OperatorStoreError::Corrupt);
            }
            if (!query.kinds.is_empty() && !query.kinds.contains(&event.kind))
                || query
                    .run_id
                    .is_some_and(|run_id| event.run_id != Some(run_id))
                || query
                    .approval_request_id
                    .is_some_and(|approval_id| event.approval_request_id != Some(approval_id))
            {
                continue;
            }
            items.push(event);
            if items.len() == limit {
                break;
            }
        }
        drop(statement);
        let has_more = items.len() > query.page_size as usize;
        if has_more {
            items.pop();
        }
        let next = if has_more {
            let last = items.last().ok_or(OperatorStoreError::Corrupt)?;
            Some(next_cursor(
                cursor,
                scope,
                query.page_size,
                high,
                last.sequence,
                last.event_id,
            )?)
        } else {
            None
        };
        transaction.commit().map_err(map_db)?;
        Ok(AuditPage {
            schema: "proof.operator.audit-page/v1".into(),
            page: PageInfo {
                page_size: query.page_size,
                returned: items.len() as u64,
                high_water_sequence: high,
                next_cursor: next,
            },
            items,
        })
    }
}

#[derive(Clone, Copy)]
struct CommandShape {
    kind: CommandKind,
    route: OperatorMutationRoute,
    mutation_capability: Option<Capability>,
    target_run_id: Option<Uuid>,
    target_step_id: Option<Uuid>,
    approval_request_id: Option<Uuid>,
    expected_run_revision: Option<u64>,
    expected_step_revision: Option<u64>,
    expected_control_revision: Option<u64>,
    expected_checkpoint_id: Option<Uuid>,
    expected_checkpoint_sequence: Option<u64>,
    expected_checkpoint_digest: Option<proof_kernel::ContentDigest>,
    expected_fence_epoch: Option<u64>,
    recovery_directive_id: Option<Uuid>,
    recovery_directive_digest: Option<ControlDigest>,
    decision_digest: Option<proof_kernel::ContentDigest>,
}

fn command_shape(command: &proof_kernel::OperatorCommand) -> CommandShape {
    match command {
        proof_kernel::OperatorCommand::ApprovalDecision(value) => CommandShape {
            kind: CommandKind::ApprovalDecide,
            route: OperatorMutationRoute::ApprovalDecide,
            mutation_capability: Some(Capability::ApprovalDecide),
            target_run_id: Some(value.run_id),
            target_step_id: Some(value.step_id),
            approval_request_id: Some(value.approval_request_id),
            expected_run_revision: Some(value.expected_run_revision),
            expected_step_revision: Some(value.expected_step_revision),
            expected_control_revision: Some(value.expected_control_revision),
            expected_checkpoint_id: Some(value.expected_checkpoint_id),
            expected_checkpoint_sequence: Some(value.expected_checkpoint_sequence),
            expected_checkpoint_digest: Some(value.expected_checkpoint_digest),
            expected_fence_epoch: Some(value.expected_fence_epoch),
            recovery_directive_id: None,
            recovery_directive_digest: None,
            decision_digest: None,
        },
        proof_kernel::OperatorCommand::RunCancel(value) => CommandShape {
            kind: CommandKind::RunCancel,
            route: OperatorMutationRoute::RunCancel,
            mutation_capability: Some(Capability::RunCancel),
            target_run_id: Some(value.run_id),
            target_step_id: None,
            approval_request_id: None,
            expected_run_revision: Some(value.expected_run_revision),
            expected_step_revision: None,
            expected_control_revision: Some(value.expected_control_revision),
            expected_checkpoint_id: None,
            expected_checkpoint_sequence: None,
            expected_checkpoint_digest: None,
            expected_fence_epoch: Some(value.expected_fence_epoch),
            recovery_directive_id: None,
            recovery_directive_digest: None,
            decision_digest: None,
        },
        proof_kernel::OperatorCommand::RunResume(value) => CommandShape {
            kind: CommandKind::RunResume,
            route: OperatorMutationRoute::RunResume,
            mutation_capability: Some(Capability::RunResume),
            target_run_id: Some(value.run_id),
            target_step_id: Some(value.step_id),
            approval_request_id: value.approval_request_id,
            expected_run_revision: Some(value.expected_run_revision),
            expected_step_revision: Some(value.expected_step_revision),
            expected_control_revision: Some(value.expected_control_revision),
            expected_checkpoint_id: Some(value.expected_checkpoint_id),
            expected_checkpoint_sequence: Some(value.expected_checkpoint_sequence),
            expected_checkpoint_digest: Some(value.expected_checkpoint_digest),
            expected_fence_epoch: Some(value.expected_fence_epoch),
            recovery_directive_id: value.recovery_directive_id,
            recovery_directive_digest: value.recovery_directive_digest,
            decision_digest: value.decision_digest,
        },
        proof_kernel::OperatorCommand::SessionRevoke(_) => CommandShape {
            kind: CommandKind::SessionRevoke,
            route: OperatorMutationRoute::SessionRevoke,
            mutation_capability: None,
            target_run_id: None,
            target_step_id: None,
            approval_request_id: None,
            expected_run_revision: None,
            expected_step_revision: None,
            expected_control_revision: None,
            expected_checkpoint_id: None,
            expected_checkpoint_sequence: None,
            expected_checkpoint_digest: None,
            expected_fence_epoch: None,
            recovery_directive_id: None,
            recovery_directive_digest: None,
            decision_digest: None,
        },
    }
}

fn expected_command_capabilities(kind: CommandKind) -> &'static [Capability] {
    match kind {
        CommandKind::ApprovalDecide => &[Capability::ApprovalDecide, Capability::ApprovalRead],
        CommandKind::RunCancel => &[Capability::RunCancel, Capability::RunRead],
        CommandKind::RunResume => &[Capability::RunRead, Capability::RunResume],
        CommandKind::SessionRevoke => &[],
    }
}

fn load_workspace_transaction(
    transaction: &Transaction<'_>,
) -> Result<OperatorWorkspace, OperatorStoreError> {
    let serialized: String = transaction
        .query_row(
            "SELECT binding_json FROM operator_workspaces WHERE singleton=1",
            [],
            |row| row.get(0),
        )
        .map_err(map_db)?;
    let workspace: OperatorWorkspace = decode(&serialized)?;
    workspace
        .validate()
        .map_err(|_| OperatorStoreError::Corrupt)?;
    Ok(workspace)
}

fn validate_command_scope(
    transaction: &Transaction<'_>,
    request: &CommandExecutionRequest,
) -> Result<(CommandShape, OperatorWorkspace), OperatorStoreError> {
    if request.schema != "proof.operator.command-execution-request/v1"
        || request.scope.schema != "proof.operator.mutation-scope/v1"
    {
        return Err(OperatorStoreError::Invalid);
    }
    let command_validation = match &request.command {
        proof_kernel::OperatorCommand::ApprovalDecision(value) => value.validate(),
        proof_kernel::OperatorCommand::RunCancel(value) => value.validate(),
        proof_kernel::OperatorCommand::RunResume(value) => value.validate(),
        proof_kernel::OperatorCommand::SessionRevoke(value) => {
            if value.schema != proof_kernel::SessionRevokeRequest::SCHEMA {
                return Err(OperatorStoreError::Invalid);
            }
            value.binding.validate()
        }
    };
    command_validation.map_err(|_| OperatorStoreError::Invalid)?;
    request
        .scope
        .session_authority
        .validate()
        .map_err(|_| OperatorStoreError::Invalid)?;
    let shape = command_shape(&request.command);
    let binding = request.command.binding();
    let authority = &request.scope.session_authority;
    let authority_digest =
        control_digest_serialized("Proof-Operator-Session-Authority-v1", authority)
            .map_err(|_| OperatorStoreError::Invalid)?;
    let expected = expected_command_capabilities(shape.kind);
    if request.scope.route != shape.route
        || request.scope.required_capabilities.as_slice() != expected
        || request.scope.session_authority_digest != authority_digest
        || request.scope.policy_revision != authority.policy_revision
        || binding.workspace_id != authority.workspace_id
        || binding.server_instance_id != authority.server_instance_id
        || binding.session_id != authority.session_id
        || binding.human_id != authority.human_id
        || binding.auth_epoch != authority.auth_epoch
        || binding.policy_revision != authority.policy_revision
        || binding.session_authority_digest != authority_digest
        || expected
            .iter()
            .any(|capability| !authority.granted_capabilities.contains(*capability))
    {
        return Err(OperatorStoreError::Invalid);
    }
    let workspace = load_workspace_transaction(transaction)?;
    if workspace.workspace_id != binding.workspace_id
        || workspace.human.principal_id.as_uuid() != binding.human_id
        || workspace.auth_epoch != binding.auth_epoch
        || workspace.policy_revision != binding.policy_revision
        || expected
            .iter()
            .any(|capability| !workspace.capabilities.contains(*capability))
    {
        return Err(OperatorStoreError::Invalid);
    }
    Ok((shape, workspace))
}

fn command_request_digest(
    command: &proof_kernel::OperatorCommand,
) -> Result<ControlDigest, OperatorStoreError> {
    control_digest_serialized("Proof-Operator-Command-v1", command)
        .map_err(|_| OperatorStoreError::Invalid)
}

fn command_content_digest<T: Serialize>(
    kind: ArtifactKind,
    value: &T,
) -> Result<proof_kernel::ContentDigest, OperatorStoreError> {
    let canonical =
        proof_kernel::canonicalize_serialized(value).map_err(|_| OperatorStoreError::Invalid)?;
    Ok(digest(kind, &canonical))
}

fn insert_command_envelope(
    transaction: &Transaction<'_>,
    envelope: &CommandEnvelope,
    shape: CommandShape,
) -> Result<(), OperatorStoreError> {
    let binding = envelope.command.binding();
    transaction
        .execute(
            "INSERT INTO operator_commands
             (command_id, workspace_id, idempotency_key, schema, kind, human_id,
              server_instance_id, session_id, required_capability, target_run_id,
              target_step_id, approval_request_id, expected_run_revision,
              expected_step_revision, expected_control_revision, expected_checkpoint_id,
              expected_checkpoint_sequence, expected_checkpoint_digest, expected_fence_epoch,
              recovery_directive_id, recovery_directive_digest, request_digest,
              decision_digest, requested_at, command_json)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,
                     ?16,?17,?18,?19,?20,?21,?22,?23,?24,?25)",
            params![
                binding.command_id.to_string(),
                binding.workspace_id.to_string(),
                binding.idempotency_key.to_string(),
                envelope.schema,
                wire(&shape.kind)?,
                binding.human_id.to_string(),
                binding.server_instance_id.to_string(),
                binding.session_id.to_string(),
                shape
                    .mutation_capability
                    .map(|value| wire(&value))
                    .transpose()?,
                shape.target_run_id.map(|id| id.to_string()),
                shape.target_step_id.map(|id| id.to_string()),
                shape.approval_request_id.map(|id| id.to_string()),
                shape.expected_run_revision.map(i64_safe).transpose()?,
                shape.expected_step_revision.map(i64_safe).transpose()?,
                shape.expected_control_revision.map(i64_safe).transpose()?,
                shape.expected_checkpoint_id.map(|id| id.to_string()),
                shape
                    .expected_checkpoint_sequence
                    .map(i64_safe)
                    .transpose()?,
                shape.expected_checkpoint_digest.map(|value| value.hex()),
                shape.expected_fence_epoch.map(i64_safe).transpose()?,
                shape.recovery_directive_id.map(|id| id.to_string()),
                shape
                    .recovery_directive_digest
                    .map(|value| value.to_string()),
                envelope.request_digest.to_string(),
                shape.decision_digest.map(|value| value.hex()),
                envelope.requested_at.to_rfc3339(),
                json(envelope)?,
            ],
        )
        .map_err(map_db)?;
    Ok(())
}

fn populate_command_event(
    event: &mut AuditEvent,
    envelope: &CommandEnvelope,
    shape: CommandShape,
    applied: bool,
    proof: Option<ProofReference>,
) {
    let binding = envelope.command.binding();
    event.human_id = Some(binding.human_id);
    event.session_id = Some(binding.session_id);
    event.session_authority_digest = Some(binding.session_authority_digest);
    event.command_id = Some(binding.command_id);
    event.command_kind = Some(shape.kind);
    event.run_id = shape.target_run_id;
    event.approval_request_id = shape.approval_request_id;
    if matches!(shape.kind, CommandKind::RunResume) {
        event.decision_digest = shape.decision_digest;
        event.recovery_directive_id = shape.recovery_directive_id;
        event.recovery_directive_digest = shape.recovery_directive_digest;
    }
    if applied && shape.kind == CommandKind::SessionRevoke {
        event.server_instance_id = Some(binding.server_instance_id);
        event.auth_epoch = Some(binding.auth_epoch);
        event.policy_revision = Some(binding.policy_revision);
    }
    event.proof = proof;
}

fn persist_command_proof(
    transaction: &Transaction<'_>,
    proof: &proof_kernel::Proof,
    workspace: &OperatorWorkspace,
    expected_operation: &str,
    expected_id: Uuid,
    input_digest: proof_kernel::ContentDigest,
    output_digest: proof_kernel::ContentDigest,
    now: DateTime<Utc>,
) -> Result<ProofReference, OperatorStoreError> {
    let agent_id = workspace.agent.principal_id.as_uuid();
    let principal = load_principal_record(transaction, agent_id)?;
    if principal.kind != PrincipalKind::Agent
        || proof.body.id != expected_id
        || proof.body.actor.as_uuid() != agent_id
        || proof.body.delegation_id.is_some()
        || proof.body.operation != expected_operation
        || proof.body.input_digest != input_digest
        || proof.body.output_digest != output_digest
        || proof.body.timestamp != now
        || proof.body.expires_at.is_some()
        || proof.verify(&principal.public_key).is_err()
    {
        return Err(OperatorStoreError::Unavailable);
    }
    let serialized = json(proof)?;
    transaction
        .execute(
            "INSERT INTO proofs
             (id, actor, version, delegation_id, operation, input_digest, output_digest,
              timestamp, expires_at, signature)
             VALUES (?1,?2,'v1',NULL,?3,?4,?5,?6,NULL,?7)",
            params![
                proof.body.id.to_string(),
                proof.body.actor.as_uuid().to_string(),
                proof.body.operation,
                proof.body.input_digest.hex(),
                proof.body.output_digest.hex(),
                proof.body.timestamp.to_rfc3339(),
                serialized,
            ],
        )
        .map_err(map_db)?;
    Ok(ProofReference {
        proof_id: proof.body.id,
        actor_id: agent_id,
        operation: expected_operation
            .strip_suffix("::v1")
            .unwrap_or(expected_operation)
            .to_owned(),
        proof_digest: proof
            .proof_digest()
            .map_err(|_| OperatorStoreError::Unavailable)?,
    })
}

fn insert_command_receipt(
    transaction: &Transaction<'_>,
    receipt: &CommandReceipt,
) -> Result<(), OperatorStoreError> {
    receipt
        .validate()
        .map_err(|_| OperatorStoreError::Corrupt)?;
    transaction
        .execute(
            "INSERT INTO operator_command_receipts
             (receipt_id, workspace_id, command_id, schema, outcome, observed_run_revision,
              resulting_run_revision, resulting_step_revision, resulting_control_revision,
              resulting_fence_epoch, decision_id, decision_digest, proof_id, proof_digest,
              audit_sequence, completed_at, receipt_json, receipt_digest)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18)",
            params![
                receipt.receipt_id.to_string(),
                receipt.workspace_id.to_string(),
                receipt.command_id.to_string(),
                receipt.schema,
                wire(&receipt.outcome)?,
                receipt.observed_run_revision.map(i64_safe).transpose()?,
                receipt.resulting_run_revision.map(i64_safe).transpose()?,
                receipt.resulting_step_revision.map(i64_safe).transpose()?,
                receipt
                    .resulting_control_revision
                    .map(i64_safe)
                    .transpose()?,
                receipt.resulting_fence_epoch.map(i64_safe).transpose()?,
                receipt.decision_id.map(|id| id.to_string()),
                receipt.decision_digest.map(|value| value.hex()),
                receipt
                    .proof
                    .as_ref()
                    .map(|value| value.proof_id.to_string()),
                receipt.proof.as_ref().map(|value| value.proof_digest.hex()),
                i64_safe(receipt.audit_sequence)?,
                receipt.completed_at.to_rfc3339(),
                json(receipt)?,
                receipt.receipt_digest.to_string(),
            ],
        )
        .map_err(map_db)?;
    Ok(())
}

fn finalize_receipt(mut receipt: CommandReceipt) -> Result<CommandReceipt, OperatorStoreError> {
    receipt.receipt_digest = digest_without_field(
        "Proof-Operator-Command-Receipt-v1",
        &receipt,
        "receipt_digest",
    )?;
    receipt
        .validate()
        .map_err(|_| OperatorStoreError::Corrupt)?;
    Ok(receipt)
}

fn decode_base64url(value: &str) -> Result<Vec<u8>, OperatorStoreError> {
    if value.is_empty() || value.contains('=') {
        return Err(OperatorStoreError::Unavailable);
    }
    let mut output = Vec::with_capacity(value.len() * 3 / 4);
    let mut accumulator = 0_u32;
    let mut bits = 0_u8;
    for byte in value.bytes() {
        let digit = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'-' => 62,
            b'_' => 63,
            _ => return Err(OperatorStoreError::Unavailable),
        };
        accumulator = (accumulator << 6) | u32::from(digit);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            output.push((accumulator >> bits) as u8);
            accumulator &= (1_u32 << bits).saturating_sub(1);
        }
    }
    if bits >= 6 || accumulator != 0 || base64url(&output) != value {
        return Err(OperatorStoreError::Unavailable);
    }
    Ok(output)
}

fn sign_command_transition(
    store: &SqliteStore,
    transaction: &Transaction<'_>,
    signer: &dyn OperatorSigner,
    workspace: &OperatorWorkspace,
    envelope: &CommandEnvelope,
    outcome: ControlTransitionOutcome,
    now: DateTime<Utc>,
) -> Result<ProofReference, OperatorStoreError> {
    outcome
        .validate()
        .map_err(|_| OperatorStoreError::Corrupt)?;
    let input_digest = command_content_digest(ArtifactKind::OperationInput, envelope)?;
    let output_digest = command_content_digest(ArtifactKind::OperationOutput, &outcome)?;
    let proof_id = store.operator_uuid()?;
    let request = OperatorProofSigningRequest {
        schema: "proof.operator.proof-signing-request/v1".into(),
        agent_id: workspace.agent.principal_id.as_uuid(),
        command: envelope.clone(),
        command_digest: envelope.request_digest,
        input_digest,
        output_digest,
        proof_id,
        timestamp: now,
        outcome,
    };
    let operation = match command_kind(&envelope.command) {
        CommandKind::ApprovalDecide => "operator.approval_decide::v1",
        CommandKind::RunCancel => "operator.run_cancel::v1",
        CommandKind::RunResume => "operator.run_resume::v1",
        CommandKind::SessionRevoke => "operator.session_revoke::v1",
    };
    let proof = signer
        .sign_operator_proof(request)
        .map_err(|_| OperatorStoreError::Unavailable)?;
    persist_command_proof(
        transaction,
        &proof,
        workspace,
        operation,
        proof_id,
        input_digest,
        output_digest,
        now,
    )
}

fn load_command_lease(
    transaction: &Transaction<'_>,
    run_id: Uuid,
    fence_epoch: u64,
    now: DateTime<Utc>,
    require_active: bool,
) -> Result<RunLease, OperatorStoreError> {
    let lease_id: String = transaction
        .query_row(
            "SELECT lease_id FROM operator_run_leases
             WHERE run_id=?1 AND fence_epoch=?2",
            params![run_id.to_string(), i64_safe(fence_epoch)?],
            |row| row.get(0),
        )
        .map_err(|error| match error {
            rusqlite::Error::QueryReturnedNoRows => OperatorStoreError::StaleFence,
            other => map_db(other),
        })?;
    let lease = load_lease(transaction, uuid(&lease_id)?)?;
    if require_active && (lease.state != RunLeaseState::Active || now >= lease.expires_at) {
        return Err(OperatorStoreError::StaleFence);
    }
    Ok(lease)
}

fn load_approval_bundle(
    transaction: &Transaction<'_>,
    request_id: Uuid,
) -> Result<
    (
        ApprovalBinding,
        SignedApprovalRequest,
        Option<SignedApprovalDecision>,
    ),
    OperatorStoreError,
> {
    let row: (String, String, Option<String>) = transaction
        .query_row(
            "SELECT b.binding_json, r.request_json, d.decision_json
             FROM operator_approval_bindings b
             JOIN approval_requests r ON r.id=b.approval_request_id
             LEFT JOIN approval_decisions d ON d.request_id=b.approval_request_id
             WHERE b.approval_request_id=?1",
            [request_id.to_string()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .map_err(|error| match error {
            rusqlite::Error::QueryReturnedNoRows => OperatorStoreError::NotFound,
            other => map_db(other),
        })?;
    let binding: ApprovalBinding = decode(&row.0)?;
    let request: SignedApprovalRequest = decode(&row.1)?;
    let decision = row.2.as_deref().map(decode).transpose()?;
    validate_approval_record(transaction, &binding, &request, decision.as_ref())?;
    Ok((binding, request, decision))
}

impl OperatorCommandStore for SqliteStore {
    fn execute_operator_command(
        &self,
        request: CommandExecutionRequest,
        signer: &dyn OperatorSigner,
    ) -> Result<CommandResult, OperatorStoreError> {
        self.operator_context()?;
        let connection = self
            .conn
            .lock()
            .map_err(|_| OperatorStoreError::Unavailable)?;
        let transaction = Transaction::new_unchecked(&connection, TransactionBehavior::Immediate)
            .map_err(map_db)?;
        let (shape, workspace) = validate_command_scope(&transaction, &request)?;
        let binding = request.command.binding().clone();
        let request_digest = command_request_digest(&request.command)?;
        let existing: Option<(String, String, i64, String, String)> = transaction
            .query_row(
                "SELECT c.request_digest, c.command_json, r.audit_sequence, r.receipt_id,
                        r.receipt_json
                 FROM operator_commands c
                 JOIN operator_command_receipts r ON r.command_id=c.command_id
                 WHERE c.workspace_id=?1 AND c.human_id=?2 AND c.idempotency_key=?3",
                params![
                    binding.workspace_id.to_string(),
                    binding.human_id.to_string(),
                    binding.idempotency_key.to_string(),
                ],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                    ))
                },
            )
            .optional()
            .map_err(map_db)?;
        if let Some((stored_digest, command_json, sequence, receipt_id, receipt_json)) = existing {
            if stored_digest == request_digest.to_string() {
                let envelope: CommandEnvelope = decode(&command_json)?;
                let receipt: CommandReceipt = decode(&receipt_json)?;
                if envelope.command != request.command {
                    return Err(OperatorStoreError::Corrupt);
                }
                validate_command_receipt_binding(
                    &receipt,
                    &envelope,
                    u64_safe(sequence)?,
                    uuid(&receipt_id)?,
                )?;
                transaction.commit().map_err(map_db)?;
                return Ok(CommandResult {
                    schema: "proof.operator.command-result/v1".into(),
                    outcome: CommandResultOutcome::ExactReplay,
                    receipt,
                });
            }
            let now = self.operator_now()?;
            let envelope = CommandEnvelope {
                schema: CommandEnvelope::SCHEMA.into(),
                command: request.command,
                request_digest,
                required_capabilities: expected_command_capabilities(shape.kind).to_vec(),
                requested_at: now,
            };
            let mut event = event_base(
                binding.workspace_id,
                self.operator_uuid()?,
                AuditEventKind::CommandConflict,
                AuditOutcome::Conflict,
                now,
            );
            populate_command_event(&mut event, &envelope, shape, false, None);
            append_audit_event(&transaction, &mut event)?;
            transaction.commit().map_err(map_db)?;
            return Err(OperatorStoreError::Conflict);
        }
        let now = self.operator_now()?;
        if now >= request.scope.session_authority.absolute_expires_at {
            return Err(OperatorStoreError::NotActionable);
        }
        let envelope = CommandEnvelope {
            schema: CommandEnvelope::SCHEMA.into(),
            command: request.command,
            request_digest,
            required_capabilities: expected_command_capabilities(shape.kind).to_vec(),
            requested_at: now,
        };

        match &envelope.command {
            proof_kernel::OperatorCommand::ApprovalDecision(command) => {
                let mut control = load_control(&transaction, command.run_id)?;
                let run = load_agent_run_exact(&transaction, command.run_id)?;
                let step = load_agent_step_exact(&transaction, command.step_id)?;
                let checkpoint = load_latest_checkpoint_identity(&transaction, command.run_id)?;
                let (approval_binding, signed_request, existing_decision) =
                    load_approval_bundle(&transaction, command.approval_request_id)?;
                let _lease = load_command_lease(
                    &transaction,
                    command.run_id,
                    command.expected_fence_epoch,
                    now,
                    true,
                )?;
                if now >= signed_request.body.expires_at {
                    let mut expired = event_base(
                        binding.workspace_id,
                        self.operator_uuid()?,
                        AuditEventKind::ApprovalExpired,
                        AuditOutcome::Expired,
                        now,
                    );
                    expired.run_id = Some(command.run_id);
                    expired.approval_request_id = Some(command.approval_request_id);
                    append_audit_event(&transaction, &mut expired)?;
                    transaction.commit().map_err(map_db)?;
                    return Err(OperatorStoreError::NotActionable);
                }
                if existing_decision.is_some()
                    || run.status != AgentRunStatus::WaitingForInput
                    || step.status != AgentRunStepStatus::WaitingForApproval
                    || step.approval_request_id != Some(command.approval_request_id)
                    || approval_binding.run_id != run.id
                    || approval_binding.step_id != step.id
                    || approval_binding.required_human_id != binding.human_id
                    || signed_request
                        .digest()
                        .map_err(|_| OperatorStoreError::Corrupt)?
                        != command.expected_request_digest
                    || run.revision != command.expected_run_revision
                    || step.revision != command.expected_step_revision
                    || control.control_revision != command.expected_control_revision
                    || checkpoint
                        != (
                            command.expected_checkpoint_id,
                            command.expected_checkpoint_sequence,
                            command.expected_checkpoint_digest,
                        )
                {
                    return Err(OperatorStoreError::NotActionable);
                }
                insert_command_envelope(&transaction, &envelope, shape)?;
                let decision_id = self.operator_uuid()?;
                let signed = signer
                    .sign_approval(ApprovalSigningRequest {
                        schema: "proof.operator.approval-signing-request/v1".into(),
                        command_id: binding.command_id,
                        decision_id,
                        authenticated_human_id: binding.human_id,
                        approval_binding: approval_binding.clone(),
                        signed_request_digest: command.expected_request_digest,
                        outcome: command.outcome,
                        validated_at: now,
                    })
                    .map_err(|_| OperatorStoreError::Unavailable)?;
                if signed.schema != "proof.operator.signed-decision-result/v1" {
                    return Err(OperatorStoreError::Unavailable);
                }
                let decision = SignedApprovalDecision {
                    body: proof_kernel::ApprovalDecision {
                        id: decision_id,
                        request_id: command.approval_request_id,
                        request_digest: command.expected_request_digest,
                        outcome: match command.outcome {
                            DecisionOutcome::Approved => ApprovalOutcome::Approved,
                            DecisionOutcome::Denied => ApprovalOutcome::Denied,
                        },
                        decided_by: PrincipalId::new(binding.human_id),
                        decided_at: now,
                        reason: None,
                    },
                    signature: decode_base64url(&signed.signature)?,
                };
                let human = load_principal_record(&transaction, binding.human_id)?;
                if decision
                    .digest()
                    .map_err(|_| OperatorStoreError::Unavailable)?
                    != signed.decision_digest
                    || decision.verify(&human).is_err()
                {
                    return Err(OperatorStoreError::Unavailable);
                }
                transaction
                    .execute(
                        "INSERT INTO approval_decisions
                         (id, request_id, decided_by, outcome, decided_at, decision_json)
                         VALUES (?1,?2,?3,?4,?5,?6)",
                        params![
                            decision.body.id.to_string(),
                            decision.body.request_id.to_string(),
                            decision.body.decided_by.as_uuid().to_string(),
                            wire(&decision.body.outcome)?,
                            now.to_rfc3339(),
                            json(&decision)?,
                        ],
                    )
                    .map_err(map_db)?;
                control.last_command_id = Some(binding.command_id);
                bump_control(&mut control, now)?;
                update_run_control(&transaction, &control)?;
                append_current_projection(
                    self,
                    &transaction,
                    &control,
                    command.expected_fence_epoch,
                    now,
                )?;
                let transition = ControlTransitionOutcome {
                    schema: ControlTransitionOutcome::SCHEMA.into(),
                    command_id: binding.command_id,
                    kind: CommandKind::ApprovalDecide,
                    outcome: AppliedCommandOutcome::Applied,
                    proof_operation: OperatorProofOperation::ApprovalDecide,
                    target_run_id: Some(run.id),
                    approval_request_id: Some(command.approval_request_id),
                    resulting_run_revision: Some(run.revision),
                    resulting_step_revision: Some(step.revision),
                    resulting_control_revision: Some(control.control_revision),
                    resulting_fence_epoch: Some(command.expected_fence_epoch),
                    decision_digest: Some(signed.decision_digest),
                    completed_at: now,
                };
                let proof = sign_command_transition(
                    self,
                    &transaction,
                    signer,
                    &workspace,
                    &envelope,
                    transition,
                    now,
                )?;
                let mut event = event_base(
                    binding.workspace_id,
                    self.operator_uuid()?,
                    AuditEventKind::ApprovalDecided,
                    AuditOutcome::Accepted,
                    now,
                );
                let mut applied_shape = shape;
                applied_shape.decision_digest = Some(signed.decision_digest);
                populate_command_event(
                    &mut event,
                    &envelope,
                    applied_shape,
                    true,
                    Some(proof.clone()),
                );
                append_audit_event(&transaction, &mut event)?;
                let receipt = finalize_receipt(CommandReceipt {
                    schema: CommandReceipt::SCHEMA.into(),
                    receipt_id: self.operator_uuid()?,
                    command_id: binding.command_id,
                    idempotency_key: binding.idempotency_key,
                    kind: CommandKind::ApprovalDecide,
                    outcome: CommandOutcome::Applied,
                    request_digest,
                    workspace_id: binding.workspace_id,
                    human_id: binding.human_id,
                    target_run_id: Some(run.id),
                    approval_request_id: Some(command.approval_request_id),
                    observed_run_revision: Some(run.revision),
                    resulting_run_revision: Some(run.revision),
                    resulting_step_revision: Some(step.revision),
                    resulting_control_revision: Some(control.control_revision),
                    resulting_fence_epoch: Some(command.expected_fence_epoch),
                    decision_id: Some(decision_id),
                    decision_digest: Some(signed.decision_digest),
                    proof: Some(proof),
                    audit_event_id: event.event_id,
                    audit_sequence: event.sequence,
                    audit_digest: event.event_digest,
                    completed_at: now,
                    receipt_digest: ControlDigest::from_bytes([0; 32]),
                })?;
                insert_command_receipt(&transaction, &receipt)?;
                transaction.commit().map_err(map_db)?;
                Ok(CommandResult {
                    schema: "proof.operator.command-result/v1".into(),
                    outcome: CommandResultOutcome::Applied,
                    receipt,
                })
            }
            proof_kernel::OperatorCommand::RunCancel(command) => {
                let mut control = load_control(&transaction, command.run_id)?;
                let mut run = load_agent_run_exact(&transaction, command.run_id)?;
                let lease = load_command_lease(
                    &transaction,
                    command.run_id,
                    command.expected_fence_epoch,
                    now,
                    !run.status.is_terminal(),
                )?;
                if run.revision != command.expected_run_revision
                    || control.control_revision != command.expected_control_revision
                {
                    return Err(OperatorStoreError::StaleRevision);
                }
                if control.active_dispatch_reservation_id.is_some() {
                    return Err(OperatorStoreError::NotActionable);
                }
                insert_command_envelope(&transaction, &envelope, shape)?;
                if run.status.is_terminal() {
                    let mut event = event_base(
                        binding.workspace_id,
                        self.operator_uuid()?,
                        AuditEventKind::CommandRejected,
                        AuditOutcome::Rejected,
                        now,
                    );
                    populate_command_event(&mut event, &envelope, shape, false, None);
                    append_audit_event(&transaction, &mut event)?;
                    let receipt = finalize_receipt(CommandReceipt {
                        schema: CommandReceipt::SCHEMA.into(),
                        receipt_id: self.operator_uuid()?,
                        command_id: binding.command_id,
                        idempotency_key: binding.idempotency_key,
                        kind: CommandKind::RunCancel,
                        outcome: CommandOutcome::AlreadyTerminal,
                        request_digest,
                        workspace_id: binding.workspace_id,
                        human_id: binding.human_id,
                        target_run_id: Some(run.id),
                        approval_request_id: None,
                        observed_run_revision: Some(run.revision),
                        resulting_run_revision: Some(run.revision),
                        resulting_step_revision: None,
                        resulting_control_revision: Some(control.control_revision),
                        resulting_fence_epoch: Some(lease.fence_epoch),
                        decision_id: None,
                        decision_digest: None,
                        proof: None,
                        audit_event_id: event.event_id,
                        audit_sequence: event.sequence,
                        audit_digest: event.event_digest,
                        completed_at: now,
                        receipt_digest: ControlDigest::from_bytes([0; 32]),
                    })?;
                    insert_command_receipt(&transaction, &receipt)?;
                    transaction.commit().map_err(map_db)?;
                    return Ok(CommandResult {
                        schema: "proof.operator.command-result/v1".into(),
                        outcome: CommandResultOutcome::AlreadyTerminal,
                        receipt,
                    });
                }
                let reserved_id: Option<String> = transaction
                    .query_row(
                        "SELECT reservation_id FROM operator_budget_reservations
                         WHERE run_id=?1 AND state='reserved'",
                        [run.id.to_string()],
                        |row| row.get(0),
                    )
                    .optional()
                    .map_err(map_db)?;
                if let Some(reservation_id) = reserved_id {
                    let mut reservation = load_reservation(&transaction, uuid(&reservation_id)?)?;
                    let mut budget = load_budget(&transaction, reservation.budget_id)?;
                    release_reservation(&transaction, &mut reservation, &mut budget, now)?;
                    append_budget_event(
                        self,
                        &transaction,
                        &control,
                        &lease,
                        reservation.reservation_id,
                        reservation.intent_digest,
                        AuditEventKind::BudgetReleased,
                        AuditOutcome::Accepted,
                        now,
                    )?;
                }
                let observed_run_revision = run.revision;
                run.cancel(now).map_err(|_| OperatorStoreError::Corrupt)?;
                update_agent_run_exact(&transaction, &run, observed_run_revision)?;
                control.last_command_id = Some(binding.command_id);
                bump_control(&mut control, now)?;
                update_run_control(&transaction, &control)?;
                append_current_projection(self, &transaction, &control, lease.fence_epoch, now)?;
                let transition = ControlTransitionOutcome {
                    schema: ControlTransitionOutcome::SCHEMA.into(),
                    command_id: binding.command_id,
                    kind: CommandKind::RunCancel,
                    outcome: AppliedCommandOutcome::Applied,
                    proof_operation: OperatorProofOperation::RunCancel,
                    target_run_id: Some(run.id),
                    approval_request_id: None,
                    resulting_run_revision: Some(run.revision),
                    resulting_step_revision: None,
                    resulting_control_revision: Some(control.control_revision),
                    resulting_fence_epoch: Some(lease.fence_epoch),
                    decision_digest: None,
                    completed_at: now,
                };
                let proof = sign_command_transition(
                    self,
                    &transaction,
                    signer,
                    &workspace,
                    &envelope,
                    transition,
                    now,
                )?;
                let mut event = event_base(
                    binding.workspace_id,
                    self.operator_uuid()?,
                    AuditEventKind::RunCancelled,
                    AuditOutcome::Accepted,
                    now,
                );
                populate_command_event(&mut event, &envelope, shape, true, Some(proof.clone()));
                append_audit_event(&transaction, &mut event)?;
                let receipt = finalize_receipt(CommandReceipt {
                    schema: CommandReceipt::SCHEMA.into(),
                    receipt_id: self.operator_uuid()?,
                    command_id: binding.command_id,
                    idempotency_key: binding.idempotency_key,
                    kind: CommandKind::RunCancel,
                    outcome: CommandOutcome::Applied,
                    request_digest,
                    workspace_id: binding.workspace_id,
                    human_id: binding.human_id,
                    target_run_id: Some(run.id),
                    approval_request_id: None,
                    observed_run_revision: Some(observed_run_revision),
                    resulting_run_revision: Some(run.revision),
                    resulting_step_revision: None,
                    resulting_control_revision: Some(control.control_revision),
                    resulting_fence_epoch: Some(lease.fence_epoch),
                    decision_id: None,
                    decision_digest: None,
                    proof: Some(proof),
                    audit_event_id: event.event_id,
                    audit_sequence: event.sequence,
                    audit_digest: event.event_digest,
                    completed_at: now,
                    receipt_digest: ControlDigest::from_bytes([0; 32]),
                })?;
                insert_command_receipt(&transaction, &receipt)?;
                transaction.commit().map_err(map_db)?;
                Ok(CommandResult {
                    schema: "proof.operator.command-result/v1".into(),
                    outcome: CommandResultOutcome::Applied,
                    receipt,
                })
            }
            proof_kernel::OperatorCommand::RunResume(command) => {
                let mut control = load_control(&transaction, command.run_id)?;
                let mut run = load_agent_run_exact(&transaction, command.run_id)?;
                let mut step = load_agent_step_exact(&transaction, command.step_id)?;
                let checkpoint = load_latest_checkpoint_identity(&transaction, command.run_id)?;
                let lease = load_command_lease(
                    &transaction,
                    command.run_id,
                    command.expected_fence_epoch,
                    now,
                    true,
                )?;
                if run.revision != command.expected_run_revision
                    || step.revision != command.expected_step_revision
                    || control.control_revision != command.expected_control_revision
                    || checkpoint
                        != (
                            command.expected_checkpoint_id,
                            command.expected_checkpoint_sequence,
                            command.expected_checkpoint_digest,
                        )
                    || control.active_dispatch_reservation_id.is_some()
                {
                    return Err(OperatorStoreError::StaleRevision);
                }
                let observed_run_revision = run.revision;
                let mut resulting_step_revision = step.revision;
                let mut decision_id = None;
                let mut recovery = None;
                match (command.approval_request_id, command.recovery_directive_id) {
                    (Some(request_id), None) => {
                        let (approval_binding, signed_request, signed_decision) =
                            load_approval_bundle(&transaction, request_id)?;
                        let signed_decision =
                            signed_decision.ok_or(OperatorStoreError::NotActionable)?;
                        let decision_digest = signed_decision
                            .digest()
                            .map_err(|_| OperatorStoreError::Corrupt)?;
                        if run.status != AgentRunStatus::WaitingForInput
                            || step.status != AgentRunStepStatus::WaitingForApproval
                            || step.approval_request_id != Some(request_id)
                            || approval_binding.run_id != run.id
                            || approval_binding.step_id != step.id
                            || signed_request.body.expires_at < signed_decision.body.decided_at
                            || command.decision_digest != Some(decision_digest)
                        {
                            return Err(OperatorStoreError::NotActionable);
                        }
                        decision_id = Some(signed_decision.body.id);
                        let prior_step = step.revision;
                        match signed_decision.body.outcome {
                            ApprovalOutcome::Approved => {
                                run.resume(now).map_err(|_| OperatorStoreError::Corrupt)?;
                                step.resume_from_approval(now)
                                    .map_err(|_| OperatorStoreError::Corrupt)?;
                            }
                            ApprovalOutcome::Denied => {
                                run.fail(now).map_err(|_| OperatorStoreError::Corrupt)?;
                                step.fail("approval denied", now)
                                    .map_err(|_| OperatorStoreError::Corrupt)?;
                            }
                        }
                        update_agent_step_exact(&transaction, &step, prior_step)?;
                        resulting_step_revision = step.revision;
                    }
                    (None, Some(directive_id)) => {
                        let directive = load_recovery_directive(&transaction, directive_id)?;
                        let consumed: i64 = transaction
                            .query_row(
                                "SELECT COUNT(*) FROM operator_audit_events
                                 WHERE run_id=?1 AND recovery_directive_id=?2
                                   AND kind IN ('recovery_completed','run_resumed')",
                                params![run.id.to_string(), directive_id.to_string()],
                                |row| row.get(0),
                            )
                            .map_err(map_db)?;
                        if run.status != AgentRunStatus::Failed
                            || directive.run_id != run.id
                            || directive.checkpoint_id != checkpoint.0
                            || directive.checkpoint_sequence != checkpoint.1
                            || directive.checkpoint_digest != checkpoint.2
                            || control.recovery_directive_id != Some(directive_id)
                            || control.recovery_directive_digest != Some(directive.directive_digest)
                            || command.recovery_directive_digest != Some(directive.directive_digest)
                            || consumed != 0
                        {
                            return Err(OperatorStoreError::NotActionable);
                        }
                        run.resume(now).map_err(|_| OperatorStoreError::Corrupt)?;
                        control.recovery_directive_id = None;
                        control.recovery_directive_digest = None;
                        recovery = Some(directive);
                    }
                    _ => return Err(OperatorStoreError::Invalid),
                }
                insert_command_envelope(&transaction, &envelope, shape)?;
                update_agent_run_exact(&transaction, &run, observed_run_revision)?;
                control.last_command_id = Some(binding.command_id);
                bump_control(&mut control, now)?;
                update_run_control(&transaction, &control)?;
                append_current_projection(self, &transaction, &control, lease.fence_epoch, now)?;
                let transition = ControlTransitionOutcome {
                    schema: ControlTransitionOutcome::SCHEMA.into(),
                    command_id: binding.command_id,
                    kind: CommandKind::RunResume,
                    outcome: AppliedCommandOutcome::Applied,
                    proof_operation: OperatorProofOperation::RunResume,
                    target_run_id: Some(run.id),
                    approval_request_id: command.approval_request_id,
                    resulting_run_revision: Some(run.revision),
                    resulting_step_revision: Some(resulting_step_revision),
                    resulting_control_revision: Some(control.control_revision),
                    resulting_fence_epoch: Some(lease.fence_epoch),
                    decision_digest: command.decision_digest,
                    completed_at: now,
                };
                let proof = sign_command_transition(
                    self,
                    &transaction,
                    signer,
                    &workspace,
                    &envelope,
                    transition,
                    now,
                )?;
                if let Some(directive) = &recovery {
                    let mut completed = event_base(
                        binding.workspace_id,
                        self.operator_uuid()?,
                        AuditEventKind::RecoveryCompleted,
                        AuditOutcome::Accepted,
                        now,
                    );
                    completed.server_instance_id = Some(lease.owner_instance_id);
                    completed.run_id = Some(run.id);
                    completed.reservation_id = Some(directive.source_reservation_id);
                    completed.lease_id = Some(lease.lease_id);
                    completed.source_lease_id = Some(directive.source_lease_id);
                    completed.process_epoch_id = Some(lease.process_epoch_id);
                    completed.recovery_directive_id = Some(directive.directive_id);
                    completed.fence_epoch = Some(lease.fence_epoch);
                    completed.intent_digest = Some(directive.intent_digest);
                    completed.recovery_directive_digest = Some(directive.directive_digest);
                    append_audit_event(&transaction, &mut completed)?;
                }
                let mut event = event_base(
                    binding.workspace_id,
                    self.operator_uuid()?,
                    AuditEventKind::RunResumed,
                    AuditOutcome::Accepted,
                    now,
                );
                populate_command_event(&mut event, &envelope, shape, true, Some(proof.clone()));
                if recovery.is_some() {
                    event.lease_id = Some(lease.lease_id);
                    event.process_epoch_id = Some(lease.process_epoch_id);
                    event.fence_epoch = Some(lease.fence_epoch);
                }
                append_audit_event(&transaction, &mut event)?;
                let receipt = finalize_receipt(CommandReceipt {
                    schema: CommandReceipt::SCHEMA.into(),
                    receipt_id: self.operator_uuid()?,
                    command_id: binding.command_id,
                    idempotency_key: binding.idempotency_key,
                    kind: CommandKind::RunResume,
                    outcome: CommandOutcome::Applied,
                    request_digest,
                    workspace_id: binding.workspace_id,
                    human_id: binding.human_id,
                    target_run_id: Some(run.id),
                    approval_request_id: command.approval_request_id,
                    observed_run_revision: Some(observed_run_revision),
                    resulting_run_revision: Some(run.revision),
                    resulting_step_revision: Some(resulting_step_revision),
                    resulting_control_revision: Some(control.control_revision),
                    resulting_fence_epoch: Some(lease.fence_epoch),
                    decision_id,
                    decision_digest: command.decision_digest,
                    proof: Some(proof),
                    audit_event_id: event.event_id,
                    audit_sequence: event.sequence,
                    audit_digest: event.event_digest,
                    completed_at: now,
                    receipt_digest: ControlDigest::from_bytes([0; 32]),
                })?;
                insert_command_receipt(&transaction, &receipt)?;
                transaction.commit().map_err(map_db)?;
                Ok(CommandResult {
                    schema: "proof.operator.command-result/v1".into(),
                    outcome: CommandResultOutcome::Applied,
                    receipt,
                })
            }
            proof_kernel::OperatorCommand::SessionRevoke(_) => {
                insert_command_envelope(&transaction, &envelope, shape)?;
                let transition = ControlTransitionOutcome {
                    schema: ControlTransitionOutcome::SCHEMA.into(),
                    command_id: binding.command_id,
                    kind: CommandKind::SessionRevoke,
                    outcome: AppliedCommandOutcome::Applied,
                    proof_operation: OperatorProofOperation::SessionRevoke,
                    target_run_id: None,
                    approval_request_id: None,
                    resulting_run_revision: None,
                    resulting_step_revision: None,
                    resulting_control_revision: None,
                    resulting_fence_epoch: None,
                    decision_digest: None,
                    completed_at: now,
                };
                let proof = sign_command_transition(
                    self,
                    &transaction,
                    signer,
                    &workspace,
                    &envelope,
                    transition,
                    now,
                )?;
                let mut event = event_base(
                    binding.workspace_id,
                    self.operator_uuid()?,
                    AuditEventKind::SessionRevoked,
                    AuditOutcome::Accepted,
                    now,
                );
                populate_command_event(&mut event, &envelope, shape, true, Some(proof.clone()));
                append_audit_event(&transaction, &mut event)?;
                let receipt = finalize_receipt(CommandReceipt {
                    schema: CommandReceipt::SCHEMA.into(),
                    receipt_id: self.operator_uuid()?,
                    command_id: binding.command_id,
                    idempotency_key: binding.idempotency_key,
                    kind: CommandKind::SessionRevoke,
                    outcome: CommandOutcome::Applied,
                    request_digest,
                    workspace_id: binding.workspace_id,
                    human_id: binding.human_id,
                    target_run_id: None,
                    approval_request_id: None,
                    observed_run_revision: None,
                    resulting_run_revision: None,
                    resulting_step_revision: None,
                    resulting_control_revision: None,
                    resulting_fence_epoch: None,
                    decision_id: None,
                    decision_digest: None,
                    proof: Some(proof),
                    audit_event_id: event.event_id,
                    audit_sequence: event.sequence,
                    audit_digest: event.event_digest,
                    completed_at: now,
                    receipt_digest: ControlDigest::from_bytes([0; 32]),
                })?;
                insert_command_receipt(&transaction, &receipt)?;
                transaction.commit().map_err(map_db)?;
                Ok(CommandResult {
                    schema: "proof.operator.command-result/v1".into(),
                    outcome: CommandResultOutcome::Applied,
                    receipt,
                })
            }
        }
    }
}

fn load_agent_run_exact(
    transaction: &Transaction<'_>,
    run_id: Uuid,
) -> Result<AgentRun, OperatorStoreError> {
    let row: (
        String,
        String,
        Option<String>,
        String,
        String,
        i64,
        String,
        String,
        String,
    ) = transaction
        .query_row(
            "SELECT id, actor, agent_id, mode, status, revision, created_at, updated_at,
                        run_json FROM agent_runs WHERE id=?1",
            [run_id.to_string()],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                    row.get(7)?,
                    row.get(8)?,
                ))
            },
        )
        .map_err(|error| match error {
            rusqlite::Error::QueryReturnedNoRows => OperatorStoreError::NotFound,
            other => map_db(other),
        })?;
    let run: AgentRun = decode(&row.8)?;
    if row.0 != run.id.to_string()
        || row.1 != run.actor.as_uuid().to_string()
        || row.2 != run.agent_id.map(|id| id.to_string())
        || row.3 != wire(&run.mode).map_err(|_| OperatorStoreError::Corrupt)?
        || row.4 != wire(&run.status).map_err(|_| OperatorStoreError::Corrupt)?
        || u64_safe(row.5)? != run.revision
        || row.6 != run.created_at.to_rfc3339()
        || row.7 != run.updated_at.to_rfc3339()
        || row.8 != json(&run).map_err(|_| OperatorStoreError::Corrupt)?
    {
        return Err(OperatorStoreError::Corrupt);
    }
    Ok(run)
}

fn update_agent_run_exact(
    transaction: &Transaction<'_>,
    run: &AgentRun,
    previous_revision: u64,
) -> Result<(), OperatorStoreError> {
    let changed = transaction
        .execute(
            "UPDATE agent_runs SET status=?2, revision=?3, updated_at=?4, run_json=?5
             WHERE id=?1 AND revision=?6",
            params![
                run.id.to_string(),
                wire(&run.status)?,
                i64_safe(run.revision)?,
                run.updated_at.to_rfc3339(),
                json(run)?,
                i64_safe(previous_revision)?,
            ],
        )
        .map_err(map_db)?;
    if changed != 1 {
        return Err(OperatorStoreError::StaleRevision);
    }
    Ok(())
}

fn load_agent_step_exact(
    transaction: &Transaction<'_>,
    step_id: Uuid,
) -> Result<AgentRunStep, OperatorStoreError> {
    let row: (
        String,
        String,
        i64,
        i64,
        String,
        Option<String>,
        i64,
        String,
        String,
        String,
    ) = transaction
        .query_row(
            "SELECT id, run_id, ordinal, attempt, status, approval_request_id, revision,
                        created_at, updated_at, step_json FROM agent_run_steps WHERE id=?1",
            [step_id.to_string()],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                    row.get(7)?,
                    row.get(8)?,
                    row.get(9)?,
                ))
            },
        )
        .map_err(|error| match error {
            rusqlite::Error::QueryReturnedNoRows => OperatorStoreError::NotFound,
            other => map_db(other),
        })?;
    let step: AgentRunStep = decode(&row.9)?;
    if row.0 != step.id.to_string()
        || row.1 != step.run_id.to_string()
        || u64_safe(row.2)? != u64::from(step.ordinal)
        || u64_safe(row.3)? != u64::from(step.attempt)
        || row.4 != wire(&step.status).map_err(|_| OperatorStoreError::Corrupt)?
        || row.5 != step.approval_request_id.map(|id| id.to_string())
        || u64_safe(row.6)? != step.revision
        || row.7 != step.created_at.to_rfc3339()
        || row.8 != step.updated_at.to_rfc3339()
        || row.9 != json(&step).map_err(|_| OperatorStoreError::Corrupt)?
    {
        return Err(OperatorStoreError::Corrupt);
    }
    Ok(step)
}

fn load_latest_agent_step(
    transaction: &Transaction<'_>,
    run_id: Uuid,
) -> Result<Option<AgentRunStep>, OperatorStoreError> {
    let id: Option<String> = transaction
        .query_row(
            "SELECT id FROM agent_run_steps WHERE run_id=?1
             ORDER BY ordinal DESC, attempt DESC, id DESC LIMIT 1",
            [run_id.to_string()],
            |row| row.get(0),
        )
        .optional()
        .map_err(map_db)?;
    id.as_deref().map(uuid).transpose()?.map_or(Ok(None), |id| {
        load_agent_step_exact(transaction, id).map(Some)
    })
}

fn update_agent_step_exact(
    transaction: &Transaction<'_>,
    step: &AgentRunStep,
    previous_revision: u64,
) -> Result<(), OperatorStoreError> {
    let changed = transaction
        .execute(
            "UPDATE agent_run_steps SET status=?2, approval_request_id=?3, revision=?4,
                    updated_at=?5, step_json=?6 WHERE id=?1 AND revision=?7",
            params![
                step.id.to_string(),
                wire(&step.status)?,
                step.approval_request_id.map(|id| id.to_string()),
                i64_safe(step.revision)?,
                step.updated_at.to_rfc3339(),
                json(step)?,
                i64_safe(previous_revision)?,
            ],
        )
        .map_err(map_db)?;
    if changed != 1 {
        return Err(OperatorStoreError::StaleRevision);
    }
    Ok(())
}

fn validate_permit_reservation(
    reservation: &BudgetReservation,
    permit: &DispatchPermit,
) -> Result<(), OperatorStoreError> {
    permit.validate().map_err(|_| OperatorStoreError::Invalid)?;
    invalid_if(
        reservation.reservation_id != permit.reservation_id
            || reservation.run_id != permit.run_id
            || reservation.lease_id != permit.lease_id
            || reservation.fence_epoch != permit.fence_epoch
            || reservation.permit_id != Some(permit.permit_id)
            || reservation.dispatch_token_digest != Some(permit.dispatch_token_digest)
            || reservation.intent_digest != permit.intent_digest
            || reservation
                .replay
                .as_ref()
                .map(|value| value.binding_digest)
                != permit.replay_binding_digest
            || reservation.call_digest != Some(permit.call_digest)
            || reservation.dispatch_started_at != Some(permit.authorized_at),
    )
}

fn mark_bound_replay_failed(
    transaction: &Transaction<'_>,
    binding: Option<&proof_kernel::ReplayClaimBinding>,
    now: DateTime<Utc>,
) -> Result<(), OperatorStoreError> {
    let Some(binding) = binding else {
        return Ok(());
    };
    let row: (String, String, String) = transaction
        .query_row(
            "SELECT state, input_digest, claimed_by FROM execution_replays
             WHERE operation=?1 AND version=?2 AND idempotency_key=?3",
            params![
                binding.operation,
                binding.version,
                binding.idempotency_key.to_string()
            ],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .map_err(|_| OperatorStoreError::Corrupt)?;
    if row.1 != binding.input_digest.hex() || row.2 != binding.claimed_by.as_uuid().to_string() {
        return Err(OperatorStoreError::Corrupt);
    }
    match row.0.as_str() {
        "claimed" => {
            let changed = transaction
                .execute(
                    "UPDATE execution_replays SET state='failed', failed_at=?1,
                            failure='governed dispatch forfeited'
                     WHERE operation=?2 AND version=?3 AND idempotency_key=?4
                       AND state='claimed' AND input_digest=?5",
                    params![
                        now.to_rfc3339(),
                        binding.operation,
                        binding.version,
                        binding.idempotency_key.to_string(),
                        binding.input_digest.hex()
                    ],
                )
                .map_err(map_db)?;
            if changed != 1 {
                return Err(OperatorStoreError::Conflict);
            }
        }
        "failed" => {}
        _ => return Err(OperatorStoreError::Corrupt),
    }
    Ok(())
}

fn append_forfeit_events(
    store: &SqliteStore,
    transaction: &Transaction<'_>,
    control: &RunControl,
    lease: &RunLease,
    reservation: &BudgetReservation,
    now: DateTime<Utc>,
) -> Result<(), OperatorStoreError> {
    let mut budget_event = event_base(
        control.workspace_id,
        store.operator_uuid()?,
        AuditEventKind::BudgetForfeited,
        AuditOutcome::Failed,
        now,
    );
    budget_event.run_id = Some(control.run_id);
    budget_event.budget_id = Some(control.budget_id);
    budget_event.reservation_id = Some(reservation.reservation_id);
    budget_event.lease_id = Some(lease.lease_id);
    budget_event.permit_id = reservation.permit_id;
    budget_event.fence_epoch = Some(lease.fence_epoch);
    budget_event.intent_digest = Some(reservation.intent_digest);
    budget_event.call_digest = reservation.call_digest;
    append_audit_event(transaction, &mut budget_event)?;

    let mut failure_event = event_base(
        control.workspace_id,
        store.operator_uuid()?,
        AuditEventKind::ControlFailure,
        AuditOutcome::Failed,
        now,
    );
    failure_event.server_instance_id = Some(lease.owner_instance_id);
    failure_event.run_id = Some(control.run_id);
    failure_event.budget_id = Some(control.budget_id);
    failure_event.reservation_id = Some(reservation.reservation_id);
    failure_event.lease_id = Some(lease.lease_id);
    failure_event.process_epoch_id = Some(lease.process_epoch_id);
    failure_event.permit_id = reservation.permit_id;
    failure_event.fence_epoch = Some(lease.fence_epoch);
    failure_event.intent_digest = Some(reservation.intent_digest);
    failure_event.call_digest = reservation.call_digest;
    failure_event.failure_scope = Some(proof_kernel::AuditFailureScope::Runtime);
    append_audit_event(transaction, &mut failure_event)
}

fn apply_forfeit(
    store: &SqliteStore,
    transaction: &Transaction<'_>,
    mut control: RunControl,
    lease: &RunLease,
    mut reservation: BudgetReservation,
    now: DateTime<Utc>,
) -> Result<(AgentRun, BudgetAccount, RunControl), OperatorStoreError> {
    let mut budget = load_budget(transaction, reservation.budget_id)?;
    subtract_amounts(&mut budget.reserved, reservation.reserved)?;
    add_amounts(&mut budget.committed, reservation.reserved)?;
    budget.revision = budget
        .revision
        .checked_add(1)
        .filter(|revision| *revision <= MAX_SAFE)
        .ok_or(OperatorStoreError::Corrupt)?;
    budget.updated_at = now;
    reservation.state = BudgetReservationState::Forfeited;
    reservation.charged = reservation.reserved;
    reservation.settled_at = Some(now);
    update_budget(transaction, &budget)?;
    update_reservation(transaction, &reservation)?;
    mark_bound_replay_failed(transaction, reservation.replay.as_ref(), now)?;

    let mut run = load_agent_run_exact(transaction, control.run_id)?;
    let previous_run_revision = run.revision;
    if !run.status.is_terminal() {
        run.fail(now).map_err(|_| OperatorStoreError::Corrupt)?;
        update_agent_run_exact(transaction, &run, previous_run_revision)?;
    }
    if let Some(mut step) = load_latest_agent_step(transaction, control.run_id)? {
        if !step.status.is_terminal() {
            let previous_step_revision = step.revision;
            step.fail("governed runtime settlement failed", now)
                .map_err(|_| OperatorStoreError::Corrupt)?;
            update_agent_step_exact(transaction, &step, previous_step_revision)?;
        }
    }
    control.active_dispatch_reservation_id = None;
    bump_control(&mut control, now)?;
    update_run_control(transaction, &control)?;
    append_current_projection(store, transaction, &control, lease.fence_epoch, now)?;
    append_forfeit_events(store, transaction, &control, lease, &reservation, now)?;
    Ok((run, budget, control))
}

fn persist_runtime_context_and_proof(
    transaction: &Transaction<'_>,
    prepared: &proof_kernel::PreparedGovernedExecution,
) -> Result<(), OperatorStoreError> {
    let context = prepared.context();
    let proof = prepared.proof();
    if context.actor != proof.body.actor || context.timestamp != proof.body.timestamp {
        return Err(OperatorStoreError::Invalid);
    }
    let principal = load_principal_record(transaction, proof.body.actor.as_uuid())
        .map_err(|_| OperatorStoreError::Invalid)?;
    if principal.kind != PrincipalKind::Agent || proof.verify(&principal.public_key).is_err() {
        return Err(OperatorStoreError::Invalid);
    }
    transaction
        .execute(
            "INSERT INTO execution_contexts
             (id, actor, delegation_id, workspace_path, timestamp)
             VALUES (?1,?2,?3,?4,?5)",
            params![
                prepared.execution_context_id().to_string(),
                context.actor.as_uuid().to_string(),
                context.delegation_id.map(|id| id.to_string()),
                context.workspace_path.display().to_string(),
                context.timestamp.to_rfc3339(),
            ],
        )
        .map_err(map_db)?;
    let proof_json = json(proof)?;
    transaction
        .execute(
            "INSERT INTO proofs
             (id, actor, version, delegation_id, operation, input_digest, output_digest,
              timestamp, expires_at, signature)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
            params![
                proof.body.id.to_string(),
                proof.body.actor.as_uuid().to_string(),
                proof.body.operation.rsplit("::").next(),
                proof.body.delegation_id.map(|id| id.to_string()),
                proof.body.operation,
                proof.body.input_digest.hex(),
                proof.body.output_digest.hex(),
                proof.body.timestamp.to_rfc3339(),
                proof.body.expires_at.map(|at| at.to_rfc3339()),
                proof_json,
            ],
        )
        .map_err(map_db)?;
    Ok(())
}

fn persist_runtime_checkpoint(
    transaction: &Transaction<'_>,
    checkpoint: &AgentCheckpoint,
    expected_run_id: Uuid,
    expected_sequence: u64,
) -> Result<(), OperatorStoreError> {
    let canonical = canonicalize(&checkpoint.state).map_err(|_| OperatorStoreError::Invalid)?;
    invalid_if(
        checkpoint.run_id != expected_run_id
            || u64::from(checkpoint.sequence) != expected_sequence
            || checkpoint.state_digest != digest(ArtifactKind::AgentCheckpoint, &canonical),
    )?;
    transaction
        .execute(
            "INSERT INTO agent_checkpoints
             (id, run_id, sequence, state_digest, created_at, checkpoint_json)
             VALUES (?1,?2,?3,?4,?5,?6)",
            params![
                checkpoint.id.to_string(),
                checkpoint.run_id.to_string(),
                i64::from(checkpoint.sequence),
                checkpoint.state_digest.hex(),
                checkpoint.created_at.to_rfc3339(),
                json(checkpoint)?,
            ],
        )
        .map_err(map_db)?;
    Ok(())
}

fn persist_runtime_events(
    transaction: &Transaction<'_>,
    events: &[AgentRunEvent],
    run_id: Uuid,
) -> Result<(), OperatorStoreError> {
    let prior: i64 = transaction
        .query_row(
            "SELECT COALESCE(MAX(sequence), -1) FROM agent_run_events WHERE run_id=?1",
            [run_id.to_string()],
            |row| row.get(0),
        )
        .map_err(map_db)?;
    let mut expected = prior.checked_add(1).ok_or(OperatorStoreError::Corrupt)?;
    for event in events {
        let canonical = canonicalize(&event.data).map_err(|_| OperatorStoreError::Invalid)?;
        invalid_if(
            event.run_id != run_id
                || i64::from(event.sequence) != expected
                || event.data_digest != digest(ArtifactKind::AgentEvent, &canonical),
        )?;
        transaction
            .execute(
                "INSERT INTO agent_run_events
                 (id, run_id, sequence, kind, data_digest, created_at, event_json)
                 VALUES (?1,?2,?3,?4,?5,?6,?7)",
                params![
                    event.id.to_string(),
                    event.run_id.to_string(),
                    i64::from(event.sequence),
                    wire(&event.kind)?,
                    event.data_digest.hex(),
                    event.created_at.to_rfc3339(),
                    json(event)?,
                ],
            )
            .map_err(map_db)?;
        expected = expected.checked_add(1).ok_or(OperatorStoreError::Corrupt)?;
    }
    Ok(())
}

fn persist_runtime_evaluation(
    transaction: &Transaction<'_>,
    evaluation: &AgentRunEvaluation,
    run: &AgentRun,
) -> Result<(), OperatorStoreError> {
    invalid_if(
        evaluation.run_id != run.id
            || !run.status.is_terminal()
            || evaluation.evaluator.trim().is_empty()
            || evaluation.score_bps.is_some_and(|score| score > 10_000)
            || canonicalize(&evaluation.metrics).is_err(),
    )?;
    transaction
        .execute(
            "INSERT INTO agent_run_evaluations
             (id, run_id, evaluator, outcome, score_bps, created_at, evaluation_json)
             VALUES (?1,?2,?3,?4,?5,?6,?7)",
            params![
                evaluation.id.to_string(),
                evaluation.run_id.to_string(),
                evaluation.evaluator,
                wire(&evaluation.outcome)?,
                evaluation.score_bps.map(i64::from),
                evaluation.created_at.to_rfc3339(),
                json(evaluation)?,
            ],
        )
        .map_err(map_db)?;
    Ok(())
}

fn persist_runtime_approval(
    transaction: &Transaction<'_>,
    approval: &proof_kernel::PreparedApprovalBundle,
    run: &AgentRun,
    step: &AgentRunStep,
    checkpoint_id: Uuid,
    checkpoint_sequence: u64,
    checkpoint_digest: proof_kernel::ContentDigest,
) -> Result<(), OperatorStoreError> {
    let request = &approval.request;
    let binding = &approval.binding;
    let requester = load_principal_record(transaction, request.body.requested_by.as_uuid())?;
    request
        .verify(&requester)
        .map_err(|_| OperatorStoreError::Invalid)?;
    invalid_if(
        run.status != AgentRunStatus::WaitingForInput
            || step.status != AgentRunStepStatus::WaitingForApproval
            || step.approval_request_id != Some(request.body.id)
            || binding.approval_request_id != request.body.id
            || binding.workspace_id != load_control(transaction, run.id)?.workspace_id
            || binding.run_id != run.id
            || binding.step_id != step.id
            || binding.checkpoint_id != checkpoint_id
            || binding.checkpoint_sequence != checkpoint_sequence
            || binding.checkpoint_digest != checkpoint_digest
            || binding.input_digest != request.body.input_digest
            || binding.required_human_id != load_operator_human_id(transaction)?,
    )?;
    transaction
        .execute(
            "INSERT INTO approval_requests
             (id, requested_by, operation, version, input_digest, requested_at, expires_at,
              request_json) VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
            params![
                request.body.id.to_string(),
                request.body.requested_by.as_uuid().to_string(),
                request.body.operation,
                request.body.version,
                request.body.input_digest.hex(),
                request.body.requested_at.to_rfc3339(),
                request.body.expires_at.to_rfc3339(),
                json(request)?,
            ],
        )
        .map_err(map_db)?;
    transaction
        .execute(
            "INSERT INTO operator_approval_bindings
             (approval_request_id, workspace_id, run_id, step_id, checkpoint_id,
              required_human_id, schema, checkpoint_sequence, checkpoint_digest, input_digest,
              argument_digest, consequence_digest, created_at, binding_json, binding_digest)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15)",
            params![
                binding.approval_request_id.to_string(),
                binding.workspace_id.to_string(),
                binding.run_id.to_string(),
                binding.step_id.to_string(),
                binding.checkpoint_id.to_string(),
                binding.required_human_id.to_string(),
                binding.schema,
                i64_safe(binding.checkpoint_sequence)?,
                binding.checkpoint_digest.hex(),
                binding.input_digest.hex(),
                binding.argument_digest.to_string(),
                binding.consequence_digest.to_string(),
                binding.created_at.to_rfc3339(),
                json(binding)?,
                binding.binding_digest.to_string(),
            ],
        )
        .map_err(map_db)?;
    Ok(())
}

fn load_operator_human_id(transaction: &Transaction<'_>) -> Result<Uuid, OperatorStoreError> {
    let id: String = transaction
        .query_row(
            "SELECT human_id FROM operator_human_enrollments",
            [],
            |row| row.get(0),
        )
        .map_err(map_db)?;
    uuid(&id)
}

fn complete_bound_replay(
    transaction: &Transaction<'_>,
    binding: Option<&proof_kernel::ReplayClaimBinding>,
    prepared: &proof_kernel::PreparedGovernedExecution,
) -> Result<(), OperatorStoreError> {
    let Some(binding) = binding else {
        invalid_if(prepared.replay().claim().is_some())?;
        return Ok(());
    };
    let claim = prepared
        .replay()
        .claim()
        .ok_or(OperatorStoreError::Invalid)?;
    invalid_if(
        claim.key.operation != binding.operation
            || claim.key.version != binding.version
            || claim.key.idempotency_key != binding.idempotency_key
            || claim.input_digest != binding.input_digest
            || claim.claimed_by != binding.claimed_by,
    )?;
    let row: (String, String, String, String) = transaction
        .query_row(
            "SELECT state, claim_token, input_digest, claimed_by FROM execution_replays
             WHERE operation=?1 AND version=?2 AND idempotency_key=?3",
            params![
                binding.operation,
                binding.version,
                binding.idempotency_key.to_string()
            ],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .map_err(|_| OperatorStoreError::Corrupt)?;
    invalid_if(
        row.0 != "claimed"
            || row.1 != claim.claim_token.to_string()
            || row.2 != binding.input_digest.hex()
            || row.3 != binding.claimed_by.as_uuid().to_string(),
    )?;
    let canonical = canonicalize(prepared.output()).map_err(|_| OperatorStoreError::Invalid)?;
    let changed = transaction
        .execute(
            "UPDATE execution_replays SET state='completed', completed_at=?1, output_json=?2,
                    proof_id=?3, proof_json=?4, execution_context_id=?5
             WHERE operation=?6 AND version=?7 AND idempotency_key=?8
               AND state='claimed' AND claim_token=?9 AND input_digest=?10",
            params![
                prepared.proof().body.timestamp.to_rfc3339(),
                canonical.as_str(),
                prepared.proof().body.id.to_string(),
                json(prepared.proof())?,
                prepared.execution_context_id().to_string(),
                binding.operation,
                binding.version,
                binding.idempotency_key.to_string(),
                claim.claim_token.to_string(),
                binding.input_digest.hex(),
            ],
        )
        .map_err(map_db)?;
    if changed != 1 {
        return Err(OperatorStoreError::Conflict);
    }
    Ok(())
}

fn append_commit_events(
    store: &SqliteStore,
    transaction: &Transaction<'_>,
    control: &RunControl,
    lease: &RunLease,
    reservation: &BudgetReservation,
    proof: &proof_kernel::Proof,
    now: DateTime<Utc>,
) -> Result<(), OperatorStoreError> {
    let proof_reference = ProofReference {
        proof_id: proof.body.id,
        actor_id: proof.body.actor.as_uuid(),
        operation: reservation.intent.operation.clone(),
        proof_digest: proof
            .proof_digest()
            .map_err(|_| OperatorStoreError::Corrupt)?,
    };
    let mut budget_event = event_base(
        control.workspace_id,
        store.operator_uuid()?,
        AuditEventKind::BudgetCommitted,
        AuditOutcome::Accepted,
        now,
    );
    budget_event.run_id = Some(control.run_id);
    budget_event.budget_id = Some(control.budget_id);
    budget_event.reservation_id = Some(reservation.reservation_id);
    budget_event.lease_id = Some(lease.lease_id);
    budget_event.permit_id = reservation.permit_id;
    budget_event.fence_epoch = Some(lease.fence_epoch);
    budget_event.intent_digest = Some(reservation.intent_digest);
    budget_event.call_digest = reservation.call_digest;
    append_audit_event(transaction, &mut budget_event)?;

    let mut result_event = event_base(
        control.workspace_id,
        store.operator_uuid()?,
        AuditEventKind::RuntimeResultCommitted,
        AuditOutcome::Accepted,
        now,
    );
    result_event.server_instance_id = Some(lease.owner_instance_id);
    result_event.run_id = Some(control.run_id);
    result_event.budget_id = Some(control.budget_id);
    result_event.reservation_id = Some(reservation.reservation_id);
    result_event.lease_id = Some(lease.lease_id);
    result_event.process_epoch_id = Some(lease.process_epoch_id);
    result_event.permit_id = reservation.permit_id;
    result_event.fence_epoch = Some(lease.fence_epoch);
    result_event.intent_digest = Some(reservation.intent_digest);
    result_event.call_digest = reservation.call_digest;
    result_event.proof = Some(proof_reference);
    append_audit_event(transaction, &mut result_event)
}

fn commit_runtime(
    store: &SqliteStore,
    request: RuntimeCommitRequest<'_>,
    prepared: proof_kernel::PreparedGovernedExecution,
) -> Result<RuntimeCommitResult, OperatorStoreError> {
    invalid_if(request.schema != "proof.operator.runtime-commit-request/v1")?;
    let connection = store
        .conn
        .lock()
        .map_err(|_| OperatorStoreError::Unavailable)?;
    let transaction =
        Transaction::new_unchecked(&connection, TransactionBehavior::Immediate).map_err(map_db)?;
    let stored_digest: String = transaction
        .query_row(
            "SELECT dispatch_token_digest FROM operator_budget_reservations
             WHERE reservation_id=?1 AND permit_id=?2",
            params![
                request.permit.reservation_id.to_string(),
                request.permit.permit_id.to_string()
            ],
            |row| row.get(0),
        )
        .map_err(|error| match error {
            rusqlite::Error::QueryReturnedNoRows => OperatorStoreError::StaleFence,
            other => map_db(other),
        })?;
    let stored_digest = stored_digest
        .parse::<ControlDigest>()
        .map_err(|_| OperatorStoreError::Corrupt)?;
    if !request.verifies_dispatch_token_digest(stored_digest) {
        return Err(OperatorStoreError::StaleFence);
    }
    let mut reservation = load_reservation(&transaction, request.permit.reservation_id)?;
    validate_permit_reservation(&reservation, &request.permit)?;
    if reservation.state == BudgetReservationState::Committed {
        let commit = reservation
            .runtime_commit
            .as_ref()
            .ok_or(OperatorStoreError::Corrupt)?;
        let stored_prepared = reservation
            .prepared_binding
            .as_ref()
            .ok_or(OperatorStoreError::Corrupt)?;
        if stored_prepared != &request.prepared || commit.permit != request.permit {
            return Err(OperatorStoreError::Conflict);
        }
        let result = RuntimeCommitResult {
            schema: RuntimeCommitResult::SCHEMA.into(),
            run_revision: stored_prepared.result.run_revision,
            step_revision: stored_prepared.result.step_revision,
            control_revision: load_control(&transaction, reservation.run_id)?.control_revision,
            budget_revision: load_budget(&transaction, reservation.budget_id)?.revision,
            charged: reservation.charged,
            proof: stored_prepared.result.proof.clone(),
        };
        transaction.commit().map_err(map_db)?;
        result.validate()?;
        return Ok(result);
    }
    if reservation.state != BudgetReservationState::Dispatching {
        return Err(OperatorStoreError::NotActionable);
    }
    let now = store.operator_now()?;
    let (mut control, lease) = validate_authority(&transaction, &request.authority, now)?;
    let expected_control_revision = request
        .permit
        .expected_control_revision
        .checked_add(1)
        .ok_or(OperatorStoreError::Corrupt)?;
    invalid_if(
        control.control_revision != expected_control_revision
            || control.active_dispatch_reservation_id != Some(reservation.reservation_id),
    )?;
    let expected_binding = proof_kernel::PreparedExecutionBinding::from_prepared(
        &prepared,
        reservation
            .replay
            .as_ref()
            .map(|binding| binding.binding_digest),
    )
    .ok();
    let elapsed = now.signed_duration_since(
        reservation
            .dispatch_started_at
            .ok_or(OperatorStoreError::Corrupt)?,
    );
    let elapsed_floor_ms = elapsed.num_milliseconds();
    let duration_ms = (elapsed >= Duration::zero())
        .then(|| {
            u64::try_from(elapsed_floor_ms).ok()?.checked_add(u64::from(
                elapsed > Duration::milliseconds(elapsed_floor_ms),
            ))
        })
        .flatten();
    let charged = BudgetAmounts {
        steps: prepared.usage().steps(),
        tokens: prepared.usage().tokens(),
        duration_ms: duration_ms.unwrap_or(0),
        cost_microusd: prepared.usage().cost_microusd(),
        tool_dispatches: prepared.usage().tool_dispatches(),
    };
    let run = load_agent_run_exact(&transaction, reservation.run_id)?;
    let step_id = reservation
        .replay
        .as_ref()
        .map(|binding| binding.step_id)
        .unwrap_or(prepared.step_after().id);
    let step = load_agent_step_exact(&transaction, step_id)?;
    let projection = load_latest_projection_exact(&transaction, run.id)?;
    let checkpoint = load_latest_checkpoint_identity(&transaction, run.id)?;
    let mut budget = load_budget(&transaction, reservation.budget_id)?;
    let output_digest = canonicalize(prepared.output())
        .ok()
        .map(|canonical| digest(ArtifactKind::OperationOutput, &canonical));
    let catalog_valid = store
        .operator_context()?
        .catalog
        .validate_output(
            &reservation.intent.operation,
            &reservation.intent.version,
            prepared.output(),
        )
        .is_ok();
    let binding_valid = request.prepared.validate().is_ok()
        && expected_binding.as_ref() == Some(&request.prepared)
        && request.prepared_matches_dispatch()
        && duration_ms.is_some()
        && charged.fits_within(&reservation.reserved)
        && catalog_valid
        && budget.policy.budget_id == reservation.budget_id
        && budget.policy.workspace_id == control.workspace_id
        && budget.policy.deadline_at == request.permit.budget_deadline_at
        && request.permit.process_epoch_id == lease.process_epoch_id
        && request.permit.lease_id == lease.lease_id
        && request.permit.fence_epoch == lease.fence_epoch
        && request.permit.run_id == run.id
        && run.status == AgentRunStatus::Running
        && step.status == AgentRunStepStatus::Running
        && projection.workspace_id == control.workspace_id
        && projection.source_run_revision == run.revision
        && projection.source_control_revision == control.control_revision
        && projection.fence_epoch == lease.fence_epoch
        && projection.attention == AttentionState::Running
        && checkpoint
            == (
                projection.checkpoint_id,
                projection.checkpoint_sequence,
                projection.checkpoint_digest,
            )
        && prepared.usage().boundary_kind() == reservation.intent.kind
        && prepared.usage().adapter() == reservation.intent.adapter
        && prepared.usage().model() == reservation.intent.model.as_deref()
        && prepared.usage().steps() == 1
        && prepared.run_after().id == run.id
        && prepared.run_after().actor == run.actor
        && prepared.run_after().agent_id == run.agent_id
        && prepared.run_after().mode == run.mode
        && prepared.run_after().goal == run.goal
        && prepared.run_after().created_at == run.created_at
        && match run.mode {
            AgentRunMode::OneShot => {
                run.revision.checked_add(1) == Some(prepared.run_after().revision)
            }
            AgentRunMode::Session => prepared.run_after().revision == run.revision,
        }
        && prepared.step_after().id == step.id
        && prepared.step_after().run_id == step.run_id
        && prepared.step_after().ordinal == step.ordinal
        && prepared.step_after().attempt == step.attempt
        && prepared.step_after().retry_of == step.retry_of
        && prepared.step_after().operation == step.operation
        && prepared.step_after().version == step.version
        && prepared.step_after().input_digest == step.input_digest
        && prepared.step_after().created_at == step.created_at
        && step.revision.checked_add(1) == Some(prepared.step_after().revision)
        && prepared.proof().body.actor == run.actor
        && prepared.proof().body.operation
            == format!(
                "{}::{}",
                reservation.intent.operation, reservation.intent.version
            )
        && prepared.proof().body.input_digest == prepared.usage().input_digest()
        && prepared.proof().body.output_digest == prepared.usage().output_digest()
        && output_digest == Some(prepared.proof().body.output_digest);
    if !binding_valid {
        let (_, _, _) = apply_forfeit(store, &transaction, control, &lease, reservation, now)?;
        transaction.commit().map_err(map_db)?;
        return Err(OperatorStoreError::Invalid);
    }
    let prior_run_revision = run.revision;
    let prior_step_revision = step.revision;
    let run_after = prepared.run_after().clone();
    let next_step = prepared.step_after().clone();
    let forfeit_control = control.clone();
    let forfeit_reservation = reservation.clone();
    transaction
        .execute_batch("SAVEPOINT operator_runtime_candidate")
        .map_err(map_db)?;
    let persisted = (|| -> Result<RuntimeCommitResult, OperatorStoreError> {
        persist_runtime_context_and_proof(&transaction, &prepared)?;
        update_agent_run_exact(&transaction, &run_after, prior_run_revision)?;
        update_agent_step_exact(&transaction, &next_step, prior_step_revision)?;
        if let Some(checkpoint) = prepared.checkpoint() {
            persist_runtime_checkpoint(
                &transaction,
                checkpoint,
                run_after.id,
                projection
                    .checkpoint_sequence
                    .checked_add(1)
                    .ok_or(OperatorStoreError::Corrupt)?,
            )?;
        }
        persist_runtime_events(&transaction, prepared.events(), run_after.id)?;
        if let Some(evaluation) = prepared.evaluation() {
            persist_runtime_evaluation(&transaction, evaluation, &run_after)?;
        }
        let (checkpoint_id, checkpoint_sequence, checkpoint_digest) = prepared
            .checkpoint()
            .map(|checkpoint| {
                (
                    checkpoint.id,
                    u64::from(checkpoint.sequence),
                    checkpoint.state_digest,
                )
            })
            .unwrap_or((
                projection.checkpoint_id,
                projection.checkpoint_sequence,
                projection.checkpoint_digest,
            ));
        if let Some(approval) = prepared.approval() {
            persist_runtime_approval(
                &transaction,
                approval,
                &run_after,
                &next_step,
                checkpoint_id,
                checkpoint_sequence,
                checkpoint_digest,
            )?;
        }
        complete_bound_replay(&transaction, reservation.replay.as_ref(), &prepared)?;
        subtract_amounts(&mut budget.reserved, reservation.reserved)?;
        add_amounts(&mut budget.committed, charged)?;
        budget.revision = budget
            .revision
            .checked_add(1)
            .filter(|revision| *revision <= MAX_SAFE)
            .ok_or(OperatorStoreError::Corrupt)?;
        budget.updated_at = now;
        let runtime_commit = RuntimeCommit {
            schema: RuntimeCommit::SCHEMA.into(),
            permit: request.permit.clone(),
            expected_run_revision: prior_run_revision,
            expected_step_revision: prior_step_revision,
            expected_checkpoint_id: projection.checkpoint_id,
            expected_checkpoint_sequence: projection.checkpoint_sequence,
            expected_checkpoint_digest: projection.checkpoint_digest,
            actual_charge: charged,
            prepared_execution_digest: request.prepared.payload_digest,
            result_digest: request.prepared.result_digest,
            committed_at: now,
        };
        runtime_commit
            .validate()
            .map_err(|_| OperatorStoreError::Invalid)?;
        reservation.state = BudgetReservationState::Committed;
        reservation.charged = charged;
        reservation.prepared_execution_digest = Some(request.prepared.payload_digest);
        reservation.result_digest = Some(request.prepared.result_digest);
        reservation.prepared_binding = Some(request.prepared.clone());
        reservation.runtime_commit = Some(runtime_commit);
        reservation.settled_at = Some(now);
        update_budget(&transaction, &budget)?;
        update_reservation(&transaction, &reservation)?;
        control.active_dispatch_reservation_id = None;
        bump_control(&mut control, now)?;
        update_run_control(&transaction, &control)?;
        append_current_projection(store, &transaction, &control, lease.fence_epoch, now)?;
        append_commit_events(
            store,
            &transaction,
            &control,
            &lease,
            &reservation,
            prepared.proof(),
            now,
        )?;
        let result = RuntimeCommitResult {
            schema: RuntimeCommitResult::SCHEMA.into(),
            run_revision: run_after.revision,
            step_revision: next_step.revision,
            control_revision: control.control_revision,
            budget_revision: budget.revision,
            charged,
            proof: request.prepared.result.proof.clone(),
        };
        result.validate()?;
        Ok(result)
    })();
    match persisted {
        Ok(result) => {
            transaction
                .execute_batch("RELEASE operator_runtime_candidate")
                .map_err(map_db)?;
            transaction.commit().map_err(map_db)?;
            Ok(result)
        }
        Err(error) => {
            transaction
                .execute_batch(
                    "ROLLBACK TO operator_runtime_candidate;
                     RELEASE operator_runtime_candidate",
                )
                .map_err(map_db)?;
            apply_forfeit(
                store,
                &transaction,
                forfeit_control,
                &lease,
                forfeit_reservation,
                now,
            )?;
            transaction.commit().map_err(map_db)?;
            Err(error)
        }
    }
}

fn forfeit_commit(
    store: &SqliteStore,
    request: RuntimeCommitRequest<'_>,
) -> Result<RuntimeCommitResult, OperatorStoreError> {
    invalid_if(request.schema != "proof.operator.runtime-commit-request/v1")?;
    let connection = store
        .conn
        .lock()
        .map_err(|_| OperatorStoreError::Unavailable)?;
    let transaction =
        Transaction::new_unchecked(&connection, TransactionBehavior::Immediate).map_err(map_db)?;
    let stored_digest: String = transaction
        .query_row(
            "SELECT dispatch_token_digest FROM operator_budget_reservations
             WHERE reservation_id=?1 AND permit_id=?2",
            params![
                request.permit.reservation_id.to_string(),
                request.permit.permit_id.to_string()
            ],
            |row| row.get(0),
        )
        .map_err(|error| match error {
            rusqlite::Error::QueryReturnedNoRows => OperatorStoreError::StaleFence,
            other => map_db(other),
        })?;
    let stored_digest = stored_digest
        .parse::<ControlDigest>()
        .map_err(|_| OperatorStoreError::Corrupt)?;
    if !request.verifies_dispatch_token_digest(stored_digest) {
        return Err(OperatorStoreError::StaleFence);
    }
    let reservation = load_reservation(&transaction, request.permit.reservation_id)?;
    validate_permit_reservation(&reservation, &request.permit)?;
    if reservation.state != BudgetReservationState::Dispatching {
        return Err(OperatorStoreError::NotActionable);
    }
    let now = store.operator_now()?;
    let (control, lease) = validate_authority(&transaction, &request.authority, now)?;
    let expected_control_revision = request
        .permit
        .expected_control_revision
        .checked_add(1)
        .ok_or(OperatorStoreError::Corrupt)?;
    invalid_if(
        control.active_dispatch_reservation_id != Some(reservation.reservation_id)
            || control.control_revision != expected_control_revision,
    )?;
    let (run, budget, control) =
        apply_forfeit(store, &transaction, control, &lease, reservation, now)?;
    transaction.commit().map_err(map_db)?;
    let _ = (run, budget, control);
    Err(OperatorStoreError::NotActionable)
}

fn forfeit_failure(
    store: &SqliteStore,
    request: RuntimeFailureRequest<'_>,
) -> Result<RuntimeFailureResult, OperatorStoreError> {
    invalid_if(
        request.schema != "proof.operator.runtime-failure-request/v1"
            || request.failure.schema != "proof.operator.runtime-failure-body/v1"
            || request.failure.reservation_id != request.permit.reservation_id
            || request.failure.permit_id != request.permit.permit_id
            || request.failure.classification
                != proof_kernel::RuntimeFailureClassification::AmbiguousForfeitRequired
            || request.failure.intent_digest != request.permit.intent_digest
            || request.failure.call_digest != request.permit.call_digest
            || control_digest_serialized("Proof-Operator-Runtime-Failure-v1", &request.failure)
                .map_err(|_| OperatorStoreError::Invalid)?
                != request.error_digest,
    )?;
    let connection = store
        .conn
        .lock()
        .map_err(|_| OperatorStoreError::Unavailable)?;
    let transaction =
        Transaction::new_unchecked(&connection, TransactionBehavior::Immediate).map_err(map_db)?;
    let stored_digest: String = transaction
        .query_row(
            "SELECT dispatch_token_digest FROM operator_budget_reservations
             WHERE reservation_id=?1 AND permit_id=?2",
            params![
                request.permit.reservation_id.to_string(),
                request.permit.permit_id.to_string()
            ],
            |row| row.get(0),
        )
        .map_err(|error| match error {
            rusqlite::Error::QueryReturnedNoRows => OperatorStoreError::StaleFence,
            other => map_db(other),
        })?;
    let stored_digest = stored_digest
        .parse::<ControlDigest>()
        .map_err(|_| OperatorStoreError::Corrupt)?;
    if !request.verifies_dispatch_token_digest(stored_digest) {
        return Err(OperatorStoreError::StaleFence);
    }
    let reservation = load_reservation(&transaction, request.permit.reservation_id)?;
    validate_permit_reservation(&reservation, &request.permit)?;
    if reservation.state == BudgetReservationState::Forfeited {
        let run = load_agent_run_exact(&transaction, reservation.run_id)?;
        let budget = load_budget(&transaction, reservation.budget_id)?;
        let control = load_control(&transaction, reservation.run_id)?;
        transaction.commit().map_err(map_db)?;
        return Ok(RuntimeFailureResult {
            schema: RuntimeFailureResult::SCHEMA.into(),
            run_revision: run.revision,
            control_revision: control.control_revision,
            budget_revision: budget.revision,
            directive: None,
        });
    }
    if reservation.state != BudgetReservationState::Dispatching {
        return Err(OperatorStoreError::NotActionable);
    }
    let now = store.operator_now()?;
    let (control, lease) = validate_authority(&transaction, &request.authority, now)?;
    let expected_control_revision = request
        .permit
        .expected_control_revision
        .checked_add(1)
        .ok_or(OperatorStoreError::Corrupt)?;
    invalid_if(
        control.active_dispatch_reservation_id != Some(reservation.reservation_id)
            || control.control_revision != expected_control_revision,
    )?;
    let (run, budget, control) =
        apply_forfeit(store, &transaction, control, &lease, reservation, now)?;
    transaction.commit().map_err(map_db)?;
    let result = RuntimeFailureResult {
        schema: RuntimeFailureResult::SCHEMA.into(),
        run_revision: run.revision,
        control_revision: control.control_revision,
        budget_revision: budget.revision,
        directive: None,
    };
    result.validate()?;
    Ok(result)
}

fn load_latest_checkpoint_identity(
    transaction: &Transaction<'_>,
    run_id: Uuid,
) -> Result<(Uuid, u64, proof_kernel::ContentDigest), OperatorStoreError> {
    let checkpoint_id: String = transaction
        .query_row(
            "SELECT id FROM agent_checkpoints
             WHERE run_id=?1 ORDER BY sequence DESC LIMIT 1",
            [run_id.to_string()],
            |row| row.get(0),
        )
        .map_err(|error| match error {
            rusqlite::Error::QueryReturnedNoRows => OperatorStoreError::Corrupt,
            other => map_db(other),
        })?;
    load_checkpoint_identity_exact(transaction, uuid(&checkpoint_id)?, run_id)
}

fn load_checkpoint_identity_exact(
    transaction: &Transaction<'_>,
    checkpoint_id: Uuid,
    run_id: Uuid,
) -> Result<(Uuid, u64, proof_kernel::ContentDigest), OperatorStoreError> {
    let row: (String, String, i64, String, String, String) = transaction
        .query_row(
            "SELECT id, run_id, sequence, state_digest, created_at, checkpoint_json
             FROM agent_checkpoints WHERE id=?1",
            [checkpoint_id.to_string()],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                ))
            },
        )
        .map_err(|error| match error {
            rusqlite::Error::QueryReturnedNoRows => OperatorStoreError::Corrupt,
            other => map_db(other),
        })?;
    let checkpoint: AgentCheckpoint = decode(&row.5)?;
    let sequence = u64::from(checkpoint.sequence);
    if row.0 != checkpoint.id.to_string()
        || row.1 != checkpoint.run_id.to_string()
        || u64_safe(row.2)? != sequence
        || row.3 != checkpoint.state_digest.hex()
        || row.4 != checkpoint.created_at.to_rfc3339()
        || row.5 != json(&checkpoint).map_err(|_| OperatorStoreError::Corrupt)?
        || checkpoint.id != checkpoint_id
        || checkpoint.run_id != run_id
    {
        return Err(OperatorStoreError::Corrupt);
    }
    Ok((checkpoint.id, sequence, checkpoint.state_digest))
}

fn replacement_lease(
    request: &ReclaimRequest<'_>,
    token_digest: ControlDigest,
    now: DateTime<Utc>,
) -> Result<RunLease, OperatorStoreError> {
    let fence_epoch = request
        .expected_fence_epoch
        .checked_add(1)
        .filter(|value| *value <= MAX_SAFE)
        .ok_or(OperatorStoreError::Invalid)?;
    let mut lease = RunLease {
        schema: RunLease::SCHEMA.into(),
        run_id: request.run_id,
        workspace_id: request.workspace_id,
        lease_id: request.new_lease_id,
        owner_instance_id: request.owner_instance_id,
        process_epoch_id: request.new_process_epoch_id,
        lease_token_digest: token_digest,
        fence_epoch,
        revision: 0,
        state: RunLeaseState::Active,
        acquired_at: now,
        renewed_at: now,
        expires_at: now + Duration::seconds(30),
        released_at: None,
        lease_digest: ControlDigest::from_bytes([0; 32]),
    };
    lease.lease_digest = digest_without_field("Proof-Operator-Lease-v1", &lease, "lease_digest")?;
    lease.validate().map_err(|_| OperatorStoreError::Invalid)?;
    Ok(lease)
}

fn insert_recovery_directive(
    transaction: &Transaction<'_>,
    directive: &RecoveryDirective,
) -> Result<(), OperatorStoreError> {
    directive
        .validate()
        .map_err(|_| OperatorStoreError::Invalid)?;
    transaction
        .execute(
            "INSERT INTO operator_recovery_directives
             (directive_id, workspace_id, run_id, source_lease_id, source_reservation_id,
              source_budget_id, source_idempotency_key, source_request_digest, schema,
              classification, checkpoint_id, checkpoint_sequence, checkpoint_digest,
              source_fence_epoch, source_control_revision, intent_digest,
              replay_binding_digest, replay_json, required_budget_disposition, created_at,
              directive_json, directive_digest)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,
                     ?17,?18,?19,?20,?21,?22)",
            params![
                directive.directive_id.to_string(),
                directive.workspace_id.to_string(),
                directive.run_id.to_string(),
                directive.source_lease_id.to_string(),
                directive.source_reservation_id.to_string(),
                directive.source_budget_id.to_string(),
                directive.source_idempotency_key.to_string(),
                directive.source_request_digest.to_string(),
                directive.schema,
                wire(&directive.classification)?,
                directive.checkpoint_id.to_string(),
                i64_safe(directive.checkpoint_sequence)?,
                directive.checkpoint_digest.hex(),
                i64_safe(directive.source_fence_epoch)?,
                i64_safe(directive.source_control_revision)?,
                directive.intent_digest.to_string(),
                directive
                    .replay
                    .as_ref()
                    .map(|value| value.binding_digest.to_string()),
                directive.replay.as_ref().map(json).transpose()?,
                wire(&directive.required_budget_disposition)?,
                directive.created_at.to_rfc3339(),
                json(directive)?,
                directive.directive_digest.to_string(),
            ],
        )
        .map_err(map_db)?;
    Ok(())
}

fn load_recovery_directive(
    transaction: &Transaction<'_>,
    directive_id: Uuid,
) -> Result<RecoveryDirective, OperatorStoreError> {
    let row: (String, String, String) = transaction
        .query_row(
            "SELECT directive_json, directive_digest, run_id
             FROM operator_recovery_directives WHERE directive_id=?1",
            [directive_id.to_string()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .map_err(|_| OperatorStoreError::Corrupt)?;
    let directive: RecoveryDirective = decode(&row.0)?;
    directive
        .validate()
        .map_err(|_| OperatorStoreError::Corrupt)?;
    if directive.directive_id != directive_id
        || row.1 != directive.directive_digest.to_string()
        || row.2 != directive.run_id.to_string()
        || row.0 != json(&directive).map_err(|_| OperatorStoreError::Corrupt)?
    {
        return Err(OperatorStoreError::Corrupt);
    }
    Ok(directive)
}

fn append_recovery_started(
    store: &SqliteStore,
    transaction: &Transaction<'_>,
    lease: &RunLease,
    reservation: &BudgetReservation,
    directive: &RecoveryDirective,
    now: DateTime<Utc>,
) -> Result<(), OperatorStoreError> {
    let mut event = event_base(
        directive.workspace_id,
        store.operator_uuid()?,
        AuditEventKind::RecoveryStarted,
        AuditOutcome::Accepted,
        now,
    );
    event.server_instance_id = Some(lease.owner_instance_id);
    event.run_id = Some(directive.run_id);
    event.reservation_id = Some(reservation.reservation_id);
    event.source_lease_id = Some(lease.lease_id);
    event.recovery_directive_id = Some(directive.directive_id);
    event.fence_epoch = Some(lease.fence_epoch);
    event.intent_digest = Some(directive.intent_digest);
    event.recovery_directive_digest = Some(directive.directive_digest);
    append_audit_event(transaction, &mut event)
}

fn append_reclaim_event(
    store: &SqliteStore,
    transaction: &Transaction<'_>,
    replacement: &RunLease,
    source_lease_id: Uuid,
    reservation_id: Option<Uuid>,
    directive: Option<&RecoveryDirective>,
    now: DateTime<Utc>,
) -> Result<(), OperatorStoreError> {
    let mut event = lease_event(
        replacement,
        store.operator_uuid()?,
        AuditEventKind::LeaseReclaimed,
        now,
    );
    event.source_lease_id = Some(source_lease_id);
    if let Some(directive) = directive {
        event.reservation_id = reservation_id;
        event.recovery_directive_id = Some(directive.directive_id);
        event.intent_digest = Some(directive.intent_digest);
        event.recovery_directive_digest = Some(directive.directive_digest);
    }
    append_audit_event(transaction, &mut event)
}

fn reclaim(
    store: &SqliteStore,
    request: ReclaimRequest<'_>,
) -> Result<ReclaimResult, OperatorStoreError> {
    invalid_if(
        request.schema != "proof.operator.reclaim-request/v1"
            || ![
                request.workspace_id,
                request.run_id,
                request.expired_lease_id,
                request.new_lease_id,
                request.owner_instance_id,
                request.new_process_epoch_id,
                request.checkpoint_id,
            ]
            .into_iter()
            .all(proof_kernel::uuid_is_v7)
            || request.expected_fence_epoch == 0
            || request.expected_fence_epoch > MAX_SAFE
            || request.expected_control_revision > MAX_SAFE
            || request.checkpoint_sequence > MAX_SAFE,
    )?;
    let replacement_digest = request.new_lease_token_digest();
    let connection = store
        .conn
        .lock()
        .map_err(|_| OperatorStoreError::Unavailable)?;
    let transaction =
        Transaction::new_unchecked(&connection, TransactionBehavior::Immediate).map_err(map_db)?;
    let now = store.operator_now()?;
    let mut control = load_control(&transaction, request.run_id)?;
    let mut expired = load_lease(&transaction, request.expired_lease_id)?;
    let checkpoint = load_latest_checkpoint_identity(&transaction, request.run_id)?;
    if control.workspace_id != request.workspace_id
        || control.control_revision != request.expected_control_revision
        || expired.workspace_id != request.workspace_id
        || expired.run_id != request.run_id
        || expired.fence_epoch != request.expected_fence_epoch
        || expired.state != RunLeaseState::Active
        || now < expired.expires_at
        || checkpoint
            != (
                request.checkpoint_id,
                request.checkpoint_sequence,
                request.checkpoint_digest,
            )
    {
        return Err(OperatorStoreError::StaleFence);
    }
    let active: i64 = transaction
        .query_row(
            "SELECT COUNT(*) FROM operator_run_leases WHERE run_id=?1 AND state='active'",
            [request.run_id.to_string()],
            |row| row.get(0),
        )
        .map_err(map_db)?;
    if active != 1 {
        return Err(OperatorStoreError::Corrupt);
    }
    let mut statement = transaction
        .prepare(
            "SELECT reservation_id FROM operator_budget_reservations
             WHERE run_id=?1 AND state IN ('reserved','dispatching')
             ORDER BY created_at, reservation_id",
        )
        .map_err(map_db)?;
    let open = statement
        .query_map([request.run_id.to_string()], |row| row.get::<_, String>(0))
        .map_err(map_db)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(map_db)?;
    drop(statement);
    if open.len() > 1 {
        return Err(OperatorStoreError::Corrupt);
    }
    expired.revision = expired
        .revision
        .checked_add(1)
        .ok_or(OperatorStoreError::Corrupt)?;
    expired.state = RunLeaseState::Released;
    expired.released_at = Some(now);
    expired.lease_digest =
        digest_without_field("Proof-Operator-Lease-v1", &expired, "lease_digest")?;
    update_lease(&transaction, &expired)?;

    if let Some(raw_reservation_id) = open.first() {
        let mut reservation = load_reservation(&transaction, uuid(raw_reservation_id)?)?;
        if reservation.state == BudgetReservationState::Dispatching {
            let (_, _, control) =
                apply_forfeit(store, &transaction, control, &expired, reservation, now)?;
            transaction.commit().map_err(map_db)?;
            let result = ReclaimResult {
                schema: ReclaimResult::SCHEMA.into(),
                outcome: ReclaimOutcome::AmbiguousForfeited,
                lease: None,
                directive: None,
                control_revision: control.control_revision,
            };
            result.validate()?;
            return Ok(result);
        }
        if reservation.state != BudgetReservationState::Reserved
            || control.active_dispatch_reservation_id.is_some()
        {
            return Err(OperatorStoreError::Corrupt);
        }
        let mut budget = load_budget(&transaction, reservation.budget_id)?;
        release_reservation(&transaction, &mut reservation, &mut budget, now)?;
        let source_control_revision = control.control_revision;
        let mut directive = RecoveryDirective {
            schema: RecoveryDirective::SCHEMA.into(),
            directive_id: store.operator_uuid()?,
            workspace_id: request.workspace_id,
            run_id: request.run_id,
            classification: proof_kernel::RecoveryClassification::PreDispatchRecoverable,
            source_lease_id: expired.lease_id,
            source_reservation_id: reservation.reservation_id,
            source_budget_id: reservation.budget_id,
            source_idempotency_key: reservation.idempotency_key,
            source_request_digest: reservation.request_digest,
            checkpoint_id: checkpoint.0,
            checkpoint_sequence: checkpoint.1,
            checkpoint_digest: checkpoint.2,
            source_fence_epoch: expired.fence_epoch,
            source_control_revision,
            intent_digest: reservation.intent_digest,
            replay: reservation.replay.clone(),
            required_budget_disposition: proof_kernel::RecoveryBudgetDisposition::None,
            created_at: now,
            directive_digest: ControlDigest::from_bytes([0; 32]),
        };
        directive.directive_digest = digest_without_field(
            "Proof-Operator-Recovery-Directive-v1",
            &directive,
            "directive_digest",
        )?;
        insert_recovery_directive(&transaction, &directive)?;
        control.recovery_directive_id = Some(directive.directive_id);
        control.recovery_directive_digest = Some(directive.directive_digest);
        let mut run = load_agent_run_exact(&transaction, request.run_id)?;
        if run.status.is_terminal() {
            return Err(OperatorStoreError::NotActionable);
        }
        let prior_run_revision = run.revision;
        run.fail(now).map_err(|_| OperatorStoreError::Corrupt)?;
        update_agent_run_exact(&transaction, &run, prior_run_revision)?;
        let replacement = replacement_lease(&request, replacement_digest, now)?;
        insert_lease(&transaction, &replacement)?;
        bump_control(&mut control, now)?;
        update_run_control(&transaction, &control)?;
        append_current_projection(store, &transaction, &control, replacement.fence_epoch, now)?;
        append_budget_event(
            store,
            &transaction,
            &control,
            &expired,
            reservation.reservation_id,
            reservation.intent_digest,
            AuditEventKind::BudgetReleased,
            AuditOutcome::Accepted,
            now,
        )?;
        append_recovery_started(store, &transaction, &expired, &reservation, &directive, now)?;
        append_reclaim_event(
            store,
            &transaction,
            &replacement,
            expired.lease_id,
            Some(reservation.reservation_id),
            Some(&directive),
            now,
        )?;
        transaction.commit().map_err(map_db)?;
        let result = ReclaimResult {
            schema: ReclaimResult::SCHEMA.into(),
            outcome: ReclaimOutcome::PreDispatchRecovered,
            lease: Some(replacement),
            directive: Some(directive),
            control_revision: control.control_revision,
        };
        result.validate()?;
        return Ok(result);
    }

    let run = load_agent_run_exact(&transaction, request.run_id)?;
    let existing_directive = match (
        control.recovery_directive_id,
        control.recovery_directive_digest,
        run.status,
    ) {
        (Some(id), Some(digest), AgentRunStatus::Failed) => {
            let directive = load_recovery_directive(&transaction, id)?;
            let source = load_reservation(&transaction, directive.source_reservation_id)?;
            let consumed: i64 = transaction
                .query_row(
                    "SELECT COUNT(*) FROM operator_audit_events
                     WHERE run_id=?1 AND recovery_directive_id=?2
                       AND kind IN ('recovery_completed','run_resumed')",
                    params![request.run_id.to_string(), id.to_string()],
                    |row| row.get(0),
                )
                .map_err(map_db)?;
            if directive.directive_digest != digest
                || directive.workspace_id != request.workspace_id
                || directive.run_id != request.run_id
                || directive.checkpoint_id != checkpoint.0
                || directive.checkpoint_sequence != checkpoint.1
                || directive.checkpoint_digest != checkpoint.2
                || source.state != BudgetReservationState::Released
                || source.reservation_id != directive.source_reservation_id
                || source.lease_id != directive.source_lease_id
                || source.budget_id != directive.source_budget_id
                || source.idempotency_key != directive.source_idempotency_key
                || source.request_digest != directive.source_request_digest
                || source.intent_digest != directive.intent_digest
                || source.replay != directive.replay
                || consumed != 0
            {
                return Err(OperatorStoreError::Corrupt);
            }
            Some(directive)
        }
        (
            None,
            None,
            AgentRunStatus::Queued | AgentRunStatus::Running | AgentRunStatus::WaitingForInput,
        ) => None,
        (None, None, _) => return Err(OperatorStoreError::NotActionable),
        _ => return Err(OperatorStoreError::Corrupt),
    };
    let replacement = replacement_lease(&request, replacement_digest, now)?;
    insert_lease(&transaction, &replacement)?;
    bump_control(&mut control, now)?;
    update_run_control(&transaction, &control)?;
    append_current_projection(store, &transaction, &control, replacement.fence_epoch, now)?;
    append_reclaim_event(
        store,
        &transaction,
        &replacement,
        expired.lease_id,
        existing_directive
            .as_ref()
            .map(|directive| directive.source_reservation_id),
        existing_directive.as_ref(),
        now,
    )?;
    transaction.commit().map_err(map_db)?;
    let result = ReclaimResult {
        schema: ReclaimResult::SCHEMA.into(),
        outcome: if existing_directive.is_some() {
            ReclaimOutcome::RecoverableReclaimed
        } else {
            ReclaimOutcome::IdleReclaimed
        },
        lease: Some(replacement),
        directive: existing_directive,
        control_revision: control.control_revision,
    };
    result.validate()?;
    Ok(result)
}

impl OperatorRuntimeStore for SqliteStore {
    fn load_completed_replay(
        &self,
        request: ReplayLookupRequest,
    ) -> Result<ReplayLookupResult, OperatorStoreError> {
        self.operator_context()?;
        request.validate()?;
        let connection = self
            .conn
            .lock()
            .map_err(|_| OperatorStoreError::Unavailable)?;
        let transaction = Transaction::new_unchecked(&connection, TransactionBehavior::Deferred)
            .map_err(map_db)?;
        let result = load_verified_replay(
            &transaction,
            &request,
            self.operator_context()?.catalog.as_ref(),
        )?;
        transaction.commit().map_err(map_db)?;
        Ok(result)
    }

    fn claim_run_lease(
        &self,
        request: LeaseClaimRequest<'_>,
    ) -> Result<LeaseMutationResult, OperatorStoreError> {
        self.operator_context()?;
        invalid_if(
            request.schema != "proof.operator.lease-claim-request/v1"
                || ![
                    request.workspace_id,
                    request.run_id,
                    request.lease_id,
                    request.owner_instance_id,
                    request.process_epoch_id,
                ]
                .into_iter()
                .all(proof_kernel::uuid_is_v7)
                || request.expected_fence_epoch > MAX_SAFE
                || request.expected_control_revision > MAX_SAFE,
        )?;
        let token_digest = request.lease_token_digest();
        let connection = self
            .conn
            .lock()
            .map_err(|_| OperatorStoreError::Unavailable)?;
        let transaction = Transaction::new_unchecked(&connection, TransactionBehavior::Immediate)
            .map_err(map_db)?;
        let mut control = load_control(&transaction, request.run_id)?;
        if control.workspace_id != request.workspace_id
            || control.control_revision != request.expected_control_revision
        {
            return Err(OperatorStoreError::StaleRevision);
        }
        let active: i64 = transaction
            .query_row(
                "SELECT COUNT(*) FROM operator_run_leases WHERE run_id = ?1 AND state = 'active'",
                [request.run_id.to_string()],
                |row| row.get(0),
            )
            .map_err(map_db)?;
        if active != 0 {
            return Err(OperatorStoreError::NotActionable);
        }
        let maximum: i64 = transaction
            .query_row(
                "SELECT COALESCE(MAX(fence_epoch), 0) FROM operator_run_leases WHERE run_id = ?1",
                [request.run_id.to_string()],
                |row| row.get(0),
            )
            .map_err(map_db)?;
        if u64_safe(maximum)? != request.expected_fence_epoch {
            return Err(OperatorStoreError::StaleFence);
        }
        let now = self.operator_now()?;
        let mut lease = RunLease {
            schema: RunLease::SCHEMA.into(),
            run_id: request.run_id,
            workspace_id: request.workspace_id,
            lease_id: request.lease_id,
            owner_instance_id: request.owner_instance_id,
            process_epoch_id: request.process_epoch_id,
            lease_token_digest: token_digest,
            fence_epoch: request.expected_fence_epoch + 1,
            revision: 0,
            state: RunLeaseState::Active,
            acquired_at: now,
            renewed_at: now,
            expires_at: now + Duration::seconds(30),
            released_at: None,
            lease_digest: ControlDigest::from_bytes([0; 32]),
        };
        lease.lease_digest =
            digest_without_field("Proof-Operator-Lease-v1", &lease, "lease_digest")?;
        lease.validate().map_err(|_| OperatorStoreError::Invalid)?;
        insert_lease(&transaction, &lease)?;
        bump_control(&mut control, now)?;
        update_run_control(&transaction, &control)?;
        append_current_projection(self, &transaction, &control, lease.fence_epoch, now)?;
        let mut event = lease_event(
            &lease,
            self.operator_uuid()?,
            AuditEventKind::LeaseAcquired,
            now,
        );
        append_audit_event(&transaction, &mut event)?;
        transaction.commit().map_err(map_db)?;
        Ok(LeaseMutationResult {
            schema: LeaseMutationResult::SCHEMA.into(),
            outcome: LeaseMutationOutcome::Acquired,
            lease,
            control_revision: control.control_revision,
        })
    }

    fn renew_run_lease(
        &self,
        request: LeaseRenewRequest<'_>,
    ) -> Result<LeaseMutationResult, OperatorStoreError> {
        self.operator_context()?;
        invalid_if(request.schema != "proof.operator.lease-renew-request/v1")?;
        let connection = self
            .conn
            .lock()
            .map_err(|_| OperatorStoreError::Unavailable)?;
        let transaction = Transaction::new_unchecked(&connection, TransactionBehavior::Immediate)
            .map_err(map_db)?;
        let now = self.operator_now()?;
        let (mut control, mut lease) = validate_authority(&transaction, &request.authority, now)?;
        lease.revision = lease
            .revision
            .checked_add(1)
            .ok_or(OperatorStoreError::Corrupt)?;
        lease.renewed_at = now;
        lease.expires_at = now + Duration::seconds(30);
        lease.lease_digest =
            digest_without_field("Proof-Operator-Lease-v1", &lease, "lease_digest")?;
        update_lease(&transaction, &lease)?;
        bump_control(&mut control, now)?;
        update_run_control(&transaction, &control)?;
        append_current_projection(self, &transaction, &control, lease.fence_epoch, now)?;
        let mut event = lease_event(
            &lease,
            self.operator_uuid()?,
            AuditEventKind::LeaseRenewed,
            now,
        );
        append_audit_event(&transaction, &mut event)?;
        transaction.commit().map_err(map_db)?;
        Ok(LeaseMutationResult {
            schema: LeaseMutationResult::SCHEMA.into(),
            outcome: LeaseMutationOutcome::Renewed,
            lease,
            control_revision: control.control_revision,
        })
    }

    fn release_run_lease(
        &self,
        request: LeaseReleaseRequest,
    ) -> Result<LeaseMutationResult, OperatorStoreError> {
        self.operator_context()?;
        invalid_if(request.schema != "proof.operator.lease-release-request/v1")?;
        let authority = request
            .authority()
            .map_err(|_| OperatorStoreError::Invalid)?;
        let connection = self
            .conn
            .lock()
            .map_err(|_| OperatorStoreError::Unavailable)?;
        let transaction = Transaction::new_unchecked(&connection, TransactionBehavior::Immediate)
            .map_err(map_db)?;
        let now = self.operator_now()?;
        let (mut control, mut lease) = validate_authority(&transaction, &authority, now)?;
        let open: i64 = transaction
            .query_row(
                "SELECT COUNT(*) FROM operator_budget_reservations
                 WHERE lease_id = ?1 AND state IN ('reserved', 'dispatching')",
                [lease.lease_id.to_string()],
                |row| row.get(0),
            )
            .map_err(map_db)?;
        if open != 0 || control.active_dispatch_reservation_id.is_some() {
            return Err(OperatorStoreError::NotActionable);
        }
        lease.revision = lease
            .revision
            .checked_add(1)
            .ok_or(OperatorStoreError::Corrupt)?;
        lease.state = RunLeaseState::Released;
        lease.released_at = Some(now);
        lease.lease_digest =
            digest_without_field("Proof-Operator-Lease-v1", &lease, "lease_digest")?;
        update_lease(&transaction, &lease)?;
        bump_control(&mut control, now)?;
        update_run_control(&transaction, &control)?;
        append_current_projection(self, &transaction, &control, lease.fence_epoch, now)?;
        let mut event = lease_event(
            &lease,
            self.operator_uuid()?,
            AuditEventKind::LeaseReleased,
            now,
        );
        append_audit_event(&transaction, &mut event)?;
        transaction.commit().map_err(map_db)?;
        Ok(LeaseMutationResult {
            schema: LeaseMutationResult::SCHEMA.into(),
            outcome: LeaseMutationOutcome::Released,
            lease,
            control_revision: control.control_revision,
        })
    }

    fn reserve_aggregate_budget(
        &self,
        request: BudgetReserveRequest<'_>,
    ) -> Result<BudgetReserveResult, OperatorStoreError> {
        self.operator_context()?;
        validate_budget_request(&request)?;
        let connection = self
            .conn
            .lock()
            .map_err(|_| OperatorStoreError::Unavailable)?;
        let transaction = Transaction::new_unchecked(&connection, TransactionBehavior::Immediate)
            .map_err(map_db)?;
        let now = self.operator_now()?;
        let (mut control, lease) = validate_authority(&transaction, &request.authority, now)?;
        if let Some(serialized) = transaction
            .query_row(
                "SELECT reservation_json FROM operator_budget_reservations
                 WHERE budget_id = ?1 AND idempotency_key = ?2",
                params![
                    control.budget_id.to_string(),
                    request.idempotency_key.to_string()
                ],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(map_db)?
        {
            let reservation: BudgetReservation = decode(&serialized)?;
            reservation
                .validate()
                .map_err(|_| OperatorStoreError::Corrupt)?;
            let expected_digest = budget_request_digest(&request)?;
            if reservation.request_digest != expected_digest
                || reservation.reservation_id != request.reservation_id
            {
                return Err(OperatorStoreError::Conflict);
            }
            transaction.commit().map_err(map_db)?;
            let budget = load_budget_readonly(&connection, control.budget_id)?;
            return Ok(BudgetReserveResult {
                schema: BudgetReserveResult::SCHEMA.into(),
                outcome: BudgetReserveOutcome::ExactExisting,
                reservation,
                budget_revision: budget.revision,
                control_revision: control.control_revision,
            });
        }
        let open: i64 = transaction
            .query_row(
                "SELECT COUNT(*) FROM operator_budget_reservations
                 WHERE run_id=?1 AND state IN ('reserved','dispatching')",
                [control.run_id.to_string()],
                |row| row.get(0),
            )
            .map_err(map_db)?;
        if open != 0 || control.active_dispatch_reservation_id.is_some() {
            append_budget_event(
                self,
                &transaction,
                &control,
                &lease,
                request.reservation_id,
                request.intent_digest,
                AuditEventKind::BudgetRejected,
                AuditOutcome::Rejected,
                now,
            )?;
            transaction.commit().map_err(map_db)?;
            return Err(OperatorStoreError::Conflict);
        }
        let run = load_agent_run_exact(&transaction, control.run_id)?;
        if run.status != AgentRunStatus::Running {
            return Err(OperatorStoreError::NotActionable);
        }
        let projection_json: String = transaction
            .query_row(
                "SELECT snapshot_json FROM operator_run_projections WHERE run_id=?1
                 ORDER BY projection_sequence DESC LIMIT 1",
                [control.run_id.to_string()],
                |row| row.get(0),
            )
            .map_err(map_db)?;
        let projection: RunProjection = decode(&projection_json)?;
        projection
            .validate()
            .map_err(|_| OperatorStoreError::Corrupt)?;
        if projection.source_run_revision != run.revision
            || projection.source_control_revision != control.control_revision
            || projection.fence_epoch != lease.fence_epoch
            || projection.attention != AttentionState::Running
        {
            return Err(OperatorStoreError::Corrupt);
        }
        let checkpoint = load_latest_checkpoint_identity(&transaction, control.run_id)?;
        if checkpoint
            != (
                projection.checkpoint_id,
                projection.checkpoint_sequence,
                projection.checkpoint_digest,
            )
        {
            return Err(OperatorStoreError::Corrupt);
        }
        if let Some(replay) = &request.replay {
            let step = load_agent_step_exact(&transaction, replay.step_id)?;
            if replay.workspace_id != control.workspace_id
                || replay.run_id != control.run_id
                || replay.checkpoint_id != checkpoint.0
                || replay.checkpoint_sequence != checkpoint.1
                || replay.checkpoint_digest != checkpoint.2
                || replay.operation != request.intent.operation
                || replay.version != request.intent.version
                || replay.input_digest != step.input_digest
                || replay.claimed_by != run.actor
                || step.run_id != run.id
                || step.status != AgentRunStepStatus::Running
            {
                return Err(OperatorStoreError::Invalid);
            }
        }
        match &request.recovery {
            None => {
                if control.recovery_directive_id.is_some()
                    || control.recovery_directive_digest.is_some()
                {
                    return Err(OperatorStoreError::NotActionable);
                }
            }
            Some(recovery) => {
                let stored = load_recovery_directive(&transaction, recovery.directive_id)?;
                let source = load_reservation(&transaction, recovery.source_reservation_id)?;
                let reused: i64 = transaction
                    .query_row(
                        "SELECT COUNT(*) FROM operator_budget_reservations
                         WHERE recovery_directive_id=?1",
                        [recovery.directive_id.to_string()],
                        |row| row.get(0),
                    )
                    .map_err(map_db)?;
                let resumed: i64 = transaction
                    .query_row(
                        "SELECT COUNT(*) FROM operator_audit_events
                         WHERE run_id=?1 AND recovery_directive_id=?2
                           AND kind IN ('recovery_completed','run_resumed')",
                        params![run.id.to_string(), recovery.directive_id.to_string()],
                        |row| row.get(0),
                    )
                    .map_err(map_db)?;
                if &stored != recovery
                    || control.recovery_directive_id.is_some()
                    || control.recovery_directive_digest.is_some()
                    || source.state != BudgetReservationState::Released
                    || source.reservation_id != recovery.source_reservation_id
                    || source.budget_id != recovery.source_budget_id
                    || source.idempotency_key != recovery.source_idempotency_key
                    || source.request_digest != recovery.source_request_digest
                    || source.intent_digest != recovery.intent_digest
                    || source.replay != recovery.replay
                    || request.intent_digest != recovery.intent_digest
                    || request.replay != recovery.replay
                    || reused != 0
                    || resumed < 2
                {
                    return Err(OperatorStoreError::NotActionable);
                }
            }
        }
        let mut budget = load_budget(&transaction, control.budget_id)?;
        if budget.can_reserve(&request.intent.ceiling, now).is_err() {
            append_budget_event(
                self,
                &transaction,
                &control,
                &lease,
                request.reservation_id,
                request.intent_digest,
                AuditEventKind::BudgetRejected,
                AuditOutcome::Rejected,
                now,
            )?;
            transaction.commit().map_err(map_db)?;
            return Err(OperatorStoreError::NotActionable);
        }
        let request_digest = budget_request_digest(&request)?;
        let reservation = BudgetReservation {
            schema: BudgetReservation::SCHEMA.into(),
            reservation_id: request.reservation_id,
            budget_id: control.budget_id,
            run_id: control.run_id,
            lease_id: lease.lease_id,
            fence_epoch: lease.fence_epoch,
            idempotency_key: request.idempotency_key,
            request_digest,
            kind: request.intent.kind,
            intent: request.intent.clone(),
            intent_digest: request.intent_digest,
            replay: request.replay.clone(),
            recovery: request.recovery.clone(),
            state: BudgetReservationState::Reserved,
            reserved: request.intent.ceiling,
            charged: BudgetAmounts::default(),
            created_at: now,
            permit_id: None,
            dispatch_token_digest: None,
            call_digest: None,
            prepared_execution_digest: None,
            result_digest: None,
            prepared_binding: None,
            runtime_commit: None,
            dispatch_started_at: None,
            settled_at: None,
        };
        reservation
            .validate()
            .map_err(|_| OperatorStoreError::Invalid)?;
        insert_reservation(&transaction, &reservation)?;
        add_amounts(&mut budget.reserved, reservation.reserved)?;
        budget.revision += 1;
        budget.updated_at = now;
        update_budget(&transaction, &budget)?;
        bump_control(&mut control, now)?;
        update_run_control(&transaction, &control)?;
        append_current_projection(self, &transaction, &control, lease.fence_epoch, now)?;
        append_budget_event(
            self,
            &transaction,
            &control,
            &lease,
            reservation.reservation_id,
            reservation.intent_digest,
            AuditEventKind::BudgetReserved,
            AuditOutcome::Accepted,
            now,
        )?;
        transaction.commit().map_err(map_db)?;
        Ok(BudgetReserveResult {
            schema: BudgetReserveResult::SCHEMA.into(),
            outcome: BudgetReserveOutcome::Reserved,
            reservation,
            budget_revision: budget.revision,
            control_revision: control.control_revision,
        })
    }

    fn settle_budget_reservation(
        &self,
        request: BudgetSettlementRequest<'_>,
    ) -> Result<BudgetSettlementResult, OperatorStoreError> {
        self.operator_context()?;
        invalid_if(
            request.schema != "proof.operator.budget-settlement-request/v1"
                || !proof_kernel::uuid_is_v7(request.reservation_id),
        )?;
        let connection = self
            .conn
            .lock()
            .map_err(|_| OperatorStoreError::Unavailable)?;
        let transaction = Transaction::new_unchecked(&connection, TransactionBehavior::Immediate)
            .map_err(map_db)?;
        let now = self.operator_now()?;
        let (mut control, lease) = validate_authority(&transaction, &request.authority, now)?;
        let mut reservation = load_reservation(&transaction, request.reservation_id)?;
        if reservation.state != BudgetReservationState::Reserved
            || reservation.run_id != control.run_id
            || reservation.lease_id != lease.lease_id
        {
            return Err(OperatorStoreError::NotActionable);
        }
        let mut budget = load_budget(&transaction, reservation.budget_id)?;
        subtract_amounts(&mut budget.reserved, reservation.reserved)?;
        budget.revision += 1;
        budget.updated_at = now;
        reservation.state = BudgetReservationState::Released;
        reservation.settled_at = Some(now);
        update_reservation(&transaction, &reservation)?;
        update_budget(&transaction, &budget)?;
        bump_control(&mut control, now)?;
        update_run_control(&transaction, &control)?;
        append_current_projection(self, &transaction, &control, lease.fence_epoch, now)?;
        append_budget_event(
            self,
            &transaction,
            &control,
            &lease,
            reservation.reservation_id,
            reservation.intent_digest,
            AuditEventKind::BudgetReleased,
            AuditOutcome::Accepted,
            now,
        )?;
        transaction.commit().map_err(map_db)?;
        Ok(BudgetSettlementResult {
            schema: BudgetSettlementResult::SCHEMA.into(),
            outcome: BudgetSettlementOutcome::Released,
            reservation,
            budget_revision: budget.revision,
            control_revision: control.control_revision,
        })
    }

    fn begin_dispatch(
        &self,
        request: proof_kernel::BeginDispatchRequest<'_>,
    ) -> Result<DispatchResult, OperatorStoreError> {
        self.operator_context()?;
        invalid_if(
            request.schema != "proof.operator.begin-dispatch-request/v1"
                || !proof_kernel::uuid_is_v7(request.reservation_id)
                || request.intent.validate().is_err()
                || control_digest_serialized("Proof-Operator-Dispatch-Intent-v1", &request.intent)
                    .map_err(|_| OperatorStoreError::Invalid)?
                    != request.intent_digest
                || control_digest_serialized("Proof-Operator-Dispatch-Call-v1", &request.intent)
                    .map_err(|_| OperatorStoreError::Invalid)?
                    != request.call_digest,
        )?;
        let dispatch_digest = request.dispatch_token_digest();
        let connection = self
            .conn
            .lock()
            .map_err(|_| OperatorStoreError::Unavailable)?;
        let transaction = Transaction::new_unchecked(&connection, TransactionBehavior::Immediate)
            .map_err(map_db)?;
        let now = self.operator_now()?;
        let (mut control, lease) = validate_authority(&transaction, &request.authority, now)?;
        let mut reservation = load_reservation(&transaction, request.reservation_id)?;
        if reservation.state != BudgetReservationState::Reserved
            || reservation.run_id != control.run_id
            || reservation.lease_id != lease.lease_id
            || reservation.fence_epoch != lease.fence_epoch
            || reservation.intent != request.intent
            || reservation.intent_digest != request.intent_digest
            || reservation.replay != request.replay
        {
            return Err(OperatorStoreError::NotActionable);
        }
        let mut budget = load_budget(&transaction, reservation.budget_id)?;
        let open: i64 = transaction
            .query_row(
                "SELECT COUNT(*) FROM operator_budget_reservations
                 WHERE run_id=?1 AND state IN ('reserved','dispatching')",
                [control.run_id.to_string()],
                |row| row.get(0),
            )
            .map_err(map_db)?;
        let run = load_agent_run_exact(&transaction, control.run_id)?;
        let projection_json: String = transaction
            .query_row(
                "SELECT snapshot_json FROM operator_run_projections WHERE run_id=?1
                 ORDER BY projection_sequence DESC LIMIT 1",
                [control.run_id.to_string()],
                |row| row.get(0),
            )
            .map_err(map_db)?;
        let projection: RunProjection = decode(&projection_json)?;
        let checkpoint = load_latest_checkpoint_identity(&transaction, control.run_id)?;
        let step = match &request.replay {
            Some(binding) => load_agent_step_exact(&transaction, binding.step_id)?,
            None => load_latest_agent_step(&transaction, control.run_id)?
                .ok_or(OperatorStoreError::NotActionable)?,
        };
        if open != 1
            || control.active_dispatch_reservation_id.is_some()
            || run.status != AgentRunStatus::Running
            || projection.source_run_revision != run.revision
            || projection.source_control_revision != control.control_revision
            || projection.fence_epoch != lease.fence_epoch
            || projection.attention != AttentionState::Running
            || checkpoint
                != (
                    projection.checkpoint_id,
                    projection.checkpoint_sequence,
                    projection.checkpoint_digest,
                )
            || step.run_id != run.id
            || step.status != AgentRunStepStatus::Running
            || step.operation != request.intent.operation
            || step.version != request.intent.version
            || !self
                .operator_context()?
                .catalog
                .binding()
                .entries
                .iter()
                .any(|entry| {
                    entry.operation == request.intent.operation
                        && entry.version == request.intent.version
                })
        {
            return Err(OperatorStoreError::NotActionable);
        }
        if let Some(binding) = &request.replay {
            if binding.workspace_id != control.workspace_id
                || binding.run_id != run.id
                || binding.step_id != step.id
                || binding.checkpoint_id != checkpoint.0
                || binding.checkpoint_sequence != checkpoint.1
                || binding.checkpoint_digest != checkpoint.2
                || binding.input_digest != step.input_digest
                || binding.claimed_by != run.actor
                || request.replay_claim_token.is_none()
            {
                return Err(OperatorStoreError::Invalid);
            }
            let completed = load_verified_replay(
                &transaction,
                &ReplayLookupRequest {
                    schema: ReplayLookupRequest::SCHEMA.into(),
                    binding: binding.clone(),
                },
                self.operator_context()?.catalog.as_ref(),
            )?;
            if completed.outcome == ReplayLookupOutcome::Completed {
                release_reservation(&transaction, &mut reservation, &mut budget, now)?;
                bump_control(&mut control, now)?;
                update_run_control(&transaction, &control)?;
                append_current_projection(self, &transaction, &control, lease.fence_epoch, now)?;
                append_budget_event(
                    self,
                    &transaction,
                    &control,
                    &lease,
                    reservation.reservation_id,
                    reservation.intent_digest,
                    AuditEventKind::BudgetReleased,
                    AuditOutcome::Accepted,
                    now,
                )?;
                transaction.commit().map_err(map_db)?;
                return Ok(DispatchResult {
                    schema: DispatchResult::SCHEMA.into(),
                    outcome: DispatchOutcome::ExactReplay,
                    permit: None,
                    replay_completion: completed.completion,
                    control_revision: control.control_revision,
                });
            }
        } else if request.replay_claim_token.is_some() {
            return Err(OperatorStoreError::Invalid);
        }
        if now >= budget.policy.deadline_at {
            release_reservation(&transaction, &mut reservation, &mut budget, now)?;
            bump_control(&mut control, now)?;
            update_run_control(&transaction, &control)?;
            append_current_projection(self, &transaction, &control, lease.fence_epoch, now)?;
            append_budget_event(
                self,
                &transaction,
                &control,
                &lease,
                reservation.reservation_id,
                reservation.intent_digest,
                AuditEventKind::BudgetRejected,
                AuditOutcome::Rejected,
                now,
            )?;
            transaction.commit().map_err(map_db)?;
            return Err(OperatorStoreError::NotActionable);
        }
        if request.replay.is_some() {
            // The transaction-local replay claim is completed in replay.rs.
            let claim_token = request
                .replay_claim_token
                .ok_or(OperatorStoreError::Invalid)?;
            let binding = request.replay.as_ref().ok_or(OperatorStoreError::Invalid)?;
            let claim = proof_kernel::ExecutionReplayClaim {
                key: proof_kernel::ExecutionReplayKey {
                    operation: binding.operation.clone(),
                    version: binding.version.clone(),
                    idempotency_key: binding.idempotency_key,
                },
                input_digest: binding.input_digest,
                claim_token,
                claimed_by: binding.claimed_by,
                claimed_at: now,
            };
            match super::replay::claim_execution_replay_in_transaction(&transaction, &claim)
                .map_err(|_| OperatorStoreError::Corrupt)?
            {
                proof_kernel::ExecutionReplayClaimResult::Acquired => {
                    insert_replay_binding(&transaction, binding, reservation.reservation_id, now)?;
                }
                proof_kernel::ExecutionReplayClaimResult::Completed(_) => {
                    let lookup = ReplayLookupRequest {
                        schema: ReplayLookupRequest::SCHEMA.into(),
                        binding: binding.clone(),
                    };
                    let completed = load_verified_replay(
                        &transaction,
                        &lookup,
                        self.operator_context()?.catalog.as_ref(),
                    )?;
                    release_reservation(&transaction, &mut reservation, &mut budget, now)?;
                    bump_control(&mut control, now)?;
                    update_run_control(&transaction, &control)?;
                    append_current_projection(
                        self,
                        &transaction,
                        &control,
                        lease.fence_epoch,
                        now,
                    )?;
                    append_budget_event(
                        self,
                        &transaction,
                        &control,
                        &lease,
                        reservation.reservation_id,
                        reservation.intent_digest,
                        AuditEventKind::BudgetReleased,
                        AuditOutcome::Accepted,
                        now,
                    )?;
                    transaction.commit().map_err(map_db)?;
                    return Ok(DispatchResult {
                        schema: DispatchResult::SCHEMA.into(),
                        outcome: DispatchOutcome::ExactReplay,
                        permit: None,
                        replay_completion: completed.completion,
                        control_revision: control.control_revision,
                    });
                }
                other => {
                    release_reservation(&transaction, &mut reservation, &mut budget, now)?;
                    bump_control(&mut control, now)?;
                    update_run_control(&transaction, &control)?;
                    append_current_projection(
                        self,
                        &transaction,
                        &control,
                        lease.fence_epoch,
                        now,
                    )?;
                    append_budget_event(
                        self,
                        &transaction,
                        &control,
                        &lease,
                        reservation.reservation_id,
                        reservation.intent_digest,
                        AuditEventKind::BudgetRejected,
                        AuditOutcome::Rejected,
                        now,
                    )?;
                    transaction.commit().map_err(map_db)?;
                    return Ok(DispatchResult {
                        schema: DispatchResult::SCHEMA.into(),
                        outcome: match other {
                            proof_kernel::ExecutionReplayClaimResult::Conflict => {
                                DispatchOutcome::ReplayConflict
                            }
                            proof_kernel::ExecutionReplayClaimResult::Failed => {
                                DispatchOutcome::ReplayFailed
                            }
                            proof_kernel::ExecutionReplayClaimResult::InProgress => {
                                DispatchOutcome::ReplayInProgress
                            }
                            proof_kernel::ExecutionReplayClaimResult::Unsupported => {
                                DispatchOutcome::ReplayUnsupported
                            }
                            _ => return Err(OperatorStoreError::Corrupt),
                        },
                        permit: None,
                        replay_completion: None,
                        control_revision: control.control_revision,
                    });
                }
            }
        }
        let permit = DispatchPermit {
            schema: DispatchPermit::SCHEMA.into(),
            permit_id: self.operator_uuid()?,
            run_id: control.run_id,
            reservation_id: reservation.reservation_id,
            lease_id: lease.lease_id,
            process_epoch_id: lease.process_epoch_id,
            fence_epoch: lease.fence_epoch,
            expected_control_revision: control.control_revision,
            intent_digest: reservation.intent_digest,
            replay_binding_digest: reservation
                .replay
                .as_ref()
                .map(|value| value.binding_digest),
            dispatch_token_digest: dispatch_digest,
            call_digest: request.call_digest,
            authorized_at: now,
            budget_deadline_at: budget.policy.deadline_at,
        };
        permit.validate().map_err(|_| OperatorStoreError::Invalid)?;
        reservation.state = BudgetReservationState::Dispatching;
        reservation.permit_id = Some(permit.permit_id);
        reservation.dispatch_token_digest = Some(dispatch_digest);
        reservation.call_digest = Some(request.call_digest);
        reservation.dispatch_started_at = Some(now);
        update_reservation(&transaction, &reservation)?;
        control.active_dispatch_reservation_id = Some(reservation.reservation_id);
        bump_control(&mut control, now)?;
        update_run_control(&transaction, &control)?;
        append_current_projection(self, &transaction, &control, lease.fence_epoch, now)?;
        append_dispatch_event(self, &transaction, &control, &lease, &reservation, now)?;
        transaction.commit().map_err(map_db)?;
        Ok(DispatchResult {
            schema: DispatchResult::SCHEMA.into(),
            outcome: DispatchOutcome::DispatchAuthorized,
            permit: Some(permit),
            replay_completion: None,
            control_revision: control.control_revision,
        })
    }

    fn commit_runtime_barrier(
        &self,
        request: RuntimeCommitRequest<'_>,
        prepared: proof_kernel::PreparedGovernedExecution,
    ) -> Result<RuntimeCommitResult, OperatorStoreError> {
        // All invalid prepared projections must reach the atomic forfeit barrier.
        if !request.prepared_matches_dispatch() {
            return forfeit_commit(self, request);
        }
        commit_runtime(self, request, prepared)
    }

    fn settle_runtime_failure(
        &self,
        request: RuntimeFailureRequest<'_>,
    ) -> Result<RuntimeFailureResult, OperatorStoreError> {
        forfeit_failure(self, request)
    }

    fn reclaim_run(
        &self,
        request: ReclaimRequest<'_>,
    ) -> Result<ReclaimResult, OperatorStoreError> {
        reclaim(self, request)
    }
}
