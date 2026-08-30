//! Deterministic task-correctness evaluation over persisted agent traces.

use std::collections::BTreeSet;

use chrono::{DateTime, Utc};
use proof_kernel::{
    canonicalize, digest, AgentDefinition, AgentEvaluationOutcome, AgentRun, AgentRunError,
    AgentRunEvaluation, AgentRunEvent, AgentRunEventKind, AgentRunStatus, AgentRunStep,
    AgentRunStepStatus, AgentTool, ApprovalExecution, ApprovalOutcome, ArtifactKind, ContentDigest,
    Principal, PrincipalKind, SignedApprovalDecision, SignedApprovalRequest,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use thiserror::Error;
use uuid::Uuid;

const METRICS_SCHEMA: &str = "proof-agent-trace-evaluation/v1";
const POLICY_BINDING_SCHEMA: &str = "proof-agent-trace-policy/v1";
const TRACE_BINDING_SCHEMA: &str = "proof-agent-trace-snapshot/v1";

/// One exact tool call required by a deterministic task policy.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExpectedToolCall {
    pub operation: String,
    pub version: String,
    pub arguments: Value,
    pub requires_approved_execution: bool,
}

impl ExpectedToolCall {
    pub fn new(
        operation: impl Into<String>,
        version: impl Into<String>,
        arguments: Value,
        requires_approved_execution: bool,
    ) -> Self {
        Self {
            operation: operation.into(),
            version: version.into(),
            arguments,
            requires_approved_execution,
        }
    }
}

/// Persisted tool evidence whose scalar value must be reported in the final
/// model output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FinalOutputSource {
    Arguments,
    Output,
    ProofId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FinalOutputReference {
    pub call_index: usize,
    pub source: FinalOutputSource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pointer: Option<String>,
}

/// Ordered task expectations for one agent trace.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TraceEvaluationPolicy {
    pub expected_calls: Vec<ExpectedToolCall>,
    pub allow_additional_calls: bool,
    #[serde(default)]
    pub required_final_output_references: Vec<FinalOutputReference>,
}

impl TraceEvaluationPolicy {
    pub fn new(expected_calls: Vec<ExpectedToolCall>, allow_additional_calls: bool) -> Self {
        Self {
            expected_calls,
            allow_additional_calls,
            required_final_output_references: Vec::new(),
        }
    }
}

/// Persisted evidence for one signed and executed approval lifecycle.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ApprovalEvidence {
    pub request: SignedApprovalRequest,
    pub decision: SignedApprovalDecision,
    pub approver: Principal,
    pub execution: ApprovalExecution,
}

impl ApprovalEvidence {
    pub fn new(
        request: SignedApprovalRequest,
        decision: SignedApprovalDecision,
        approver: Principal,
        execution: ApprovalExecution,
    ) -> Self {
        Self {
            request,
            decision,
            approver,
            execution,
        }
    }
}

#[derive(Debug, Error)]
pub enum TraceEvaluationError {
    #[error("invalid expected tool call at index {index}: {reason}")]
    InvalidExpectedCall { index: usize, reason: String },
    #[error("invalid final output reference at index {index}: {reason}")]
    InvalidFinalOutputReference { index: usize, reason: String },
    #[error("could not bind deterministic evaluation input: {0}")]
    Binding(String),
    #[error("agent run evaluation contract failed: {0}")]
    Evaluation(#[from] AgentRunError),
}

/// Reusable, provider-neutral evaluator for deterministic task correctness.
///
/// The supplied steps, events, and approvals must be the complete immutable
/// persisted trace for the run. Runtime health remains a separate concern from
/// the task policy checks here.
#[derive(Debug, Clone)]
pub struct DeterministicTraceEvaluator {
    policy: TraceEvaluationPolicy,
    policy_digest: ContentDigest,
    expected_argument_digests: Vec<ContentDigest>,
}

impl DeterministicTraceEvaluator {
    pub fn new(policy: TraceEvaluationPolicy) -> Result<Self, TraceEvaluationError> {
        let policy_digest = binding_digest(
            POLICY_BINDING_SCHEMA,
            &json!({
                "policy": &policy,
            }),
        )?;
        let mut expected_argument_digests = Vec::with_capacity(policy.expected_calls.len());
        for (index, expected) in policy.expected_calls.iter().enumerate() {
            AgentTool::new(expected.operation.clone(), expected.version.clone()).map_err(
                |error| TraceEvaluationError::InvalidExpectedCall {
                    index,
                    reason: error.to_string(),
                },
            )?;
            let canonical = canonicalize(&expected.arguments).map_err(|error| {
                TraceEvaluationError::InvalidExpectedCall {
                    index,
                    reason: error.to_string(),
                }
            })?;
            expected_argument_digests.push(digest(ArtifactKind::OperationInput, &canonical));
        }
        for (index, reference) in policy.required_final_output_references.iter().enumerate() {
            let Some(expected) = policy.expected_calls.get(reference.call_index) else {
                return Err(TraceEvaluationError::InvalidFinalOutputReference {
                    index,
                    reason: format!(
                        "call index {} is outside {} expected calls",
                        reference.call_index,
                        policy.expected_calls.len()
                    ),
                });
            };
            match reference.source {
                FinalOutputSource::Arguments | FinalOutputSource::Output => {
                    let Some(pointer) = reference.pointer.as_deref() else {
                        return Err(TraceEvaluationError::InvalidFinalOutputReference {
                            index,
                            reason: "argument and output references require a JSON pointer"
                                .to_string(),
                        });
                    };
                    if !pointer.starts_with('/') {
                        return Err(TraceEvaluationError::InvalidFinalOutputReference {
                            index,
                            reason: "JSON pointer must start with '/'".to_string(),
                        });
                    }
                    if reference.source == FinalOutputSource::Arguments
                        && expected
                            .arguments
                            .pointer(pointer)
                            .and_then(scalar_reference_text)
                            .is_none()
                    {
                        return Err(TraceEvaluationError::InvalidFinalOutputReference {
                            index,
                            reason: format!(
                                "argument pointer {pointer:?} does not select a scalar value"
                            ),
                        });
                    }
                }
                FinalOutputSource::ProofId if reference.pointer.is_some() => {
                    return Err(TraceEvaluationError::InvalidFinalOutputReference {
                        index,
                        reason: "proof ID references cannot include a JSON pointer".to_string(),
                    });
                }
                FinalOutputSource::ProofId => {}
            }
        }
        Ok(Self {
            policy,
            policy_digest,
            expected_argument_digests,
        })
    }

    pub const fn policy(&self) -> &TraceEvaluationPolicy {
        &self.policy
    }

    #[allow(clippy::too_many_arguments)]
    pub fn evaluate(
        &self,
        run: &AgentRun,
        agent: &AgentDefinition,
        run_actor: &Principal,
        trusted_approvers: &[Principal],
        steps: &[AgentRunStep],
        events: &[AgentRunEvent],
        approvals: &[ApprovalEvidence],
        evaluator_id: impl Into<String>,
        created_at: DateTime<Utc>,
    ) -> Result<AgentRunEvaluation, TraceEvaluationError> {
        let mut observed_steps = steps.iter().collect::<Vec<_>>();
        observed_steps.sort_by_key(|step| (step.ordinal, step.attempt, step.id));

        let matched_positions = match_expected_calls(
            &self.policy.expected_calls,
            &self.expected_argument_digests,
            &observed_steps,
        );
        let matched_call_count = matched_positions
            .iter()
            .filter(|position| position.is_some())
            .count();
        let expected_calls_passed = matched_call_count == self.policy.expected_calls.len()
            && (self.policy.allow_additional_calls
                || observed_steps.len() == self.policy.expected_calls.len());

        let trusted_actor = run_actor.id == run.actor && run_actor.kind == PrincipalKind::Agent;
        let invalid_steps = observed_steps
            .iter()
            .filter_map(|step| invalid_step_details(run, run_actor, step))
            .collect::<Vec<_>>();
        let unallowlisted_calls = observed_steps
            .iter()
            .filter(|step| {
                !agent.tools.iter().any(|allowed| {
                    allowed.operation == step.operation && allowed.version == step.version
                })
            })
            .map(|step| {
                json!({
                    "step_id": step.id,
                    "operation": step.operation,
                    "version": step.version,
                })
            })
            .collect::<Vec<_>>();
        let approval_results = required_approval_results(
            run,
            run_actor,
            trusted_approvers,
            &self.policy.expected_calls,
            &matched_positions,
            &observed_steps,
            approvals,
        );
        let approvals_passed = approval_results
            .iter()
            .all(|result| result["passed"] == Value::Bool(true));
        let final_output_results = final_output_reference_results(
            &self.policy,
            &matched_positions,
            &observed_steps,
            events,
        );
        let final_output_passed = final_output_results
            .iter()
            .all(|result| result["passed"] == Value::Bool(true));
        let required_approval_step_ids = self
            .policy
            .expected_calls
            .iter()
            .zip(&matched_positions)
            .filter(|(expected, _)| expected.requires_approved_execution)
            .filter_map(|(_, position)| position.map(|position| observed_steps[position].id))
            .collect::<BTreeSet<_>>();
        let lifecycle_issues = lifecycle_issues(
            run,
            agent,
            &observed_steps,
            events,
            approvals,
            &required_approval_step_ids,
        );
        let failure_events = events
            .iter()
            .filter(|event| {
                matches!(
                    event.kind,
                    AgentRunEventKind::ToolFailed
                        | AgentRunEventKind::Failed
                        | AgentRunEventKind::BudgetExceeded
                )
            })
            .map(|event| {
                json!({
                    "event_id": event.id,
                    "sequence": event.sequence,
                    "kind": event.kind,
                })
            })
            .collect::<Vec<_>>();

        let checks = vec![
            Check::new(
                "run_succeeded",
                run.status == AgentRunStatus::Succeeded,
                json!({"status": run.status}),
            ),
            Check::new(
                "run_bound_to_agent",
                run.agent_id == Some(agent.id),
                json!({"run_agent_id": run.agent_id, "evaluated_agent_id": agent.id}),
            ),
            Check::new(
                "trusted_run_actor",
                trusted_actor,
                json!({
                    "run_actor_id": run.actor,
                    "trusted_actor_id": run_actor.id,
                    "trusted_actor_kind": run_actor.kind,
                }),
            ),
            Check::new(
                "expected_tool_calls",
                expected_calls_passed,
                json!({
                    "allow_additional_calls": self.policy.allow_additional_calls,
                    "expected_count": self.policy.expected_calls.len(),
                    "observed_count": observed_steps.len(),
                    "matched_count": matched_call_count,
                    "expected": expected_call_metrics(
                        &self.policy.expected_calls,
                        &self.expected_argument_digests,
                        &matched_positions,
                        &observed_steps,
                    ),
                    "observed": observed_call_metrics(&observed_steps),
                }),
            ),
            Check::new(
                "successful_steps_with_valid_proofs",
                invalid_steps.is_empty(),
                json!({
                    "checked_count": observed_steps.len(),
                    "invalid_steps": invalid_steps,
                }),
            ),
            Check::new(
                "calls_allowlisted",
                unallowlisted_calls.is_empty(),
                json!({"unallowlisted_calls": unallowlisted_calls}),
            ),
            Check::new(
                "required_approvals",
                approvals_passed,
                json!({"required": approval_results}),
            ),
            Check::new(
                "final_output_references",
                final_output_passed,
                json!({"required": final_output_results}),
            ),
            Check::new(
                "lifecycle_integrity",
                lifecycle_issues.is_empty(),
                json!({"issues": lifecycle_issues}),
            ),
            Check::new(
                "no_failure_events",
                failure_events.is_empty(),
                json!({"failure_events": failure_events}),
            ),
        ];

        let passed_checks = checks.iter().filter(|check| check.passed).count();
        let score_bps = ((passed_checks as u64 * 10_000) / checks.len() as u64) as u16;
        let failed_check_names = checks
            .iter()
            .filter(|check| !check.passed)
            .map(|check| check.name)
            .collect::<Vec<_>>();
        let outcome = if failed_check_names.is_empty() {
            AgentEvaluationOutcome::Passed
        } else {
            AgentEvaluationOutcome::Failed
        };
        let summary = if failed_check_names.is_empty() {
            format!("all {} deterministic task checks passed", checks.len())
        } else {
            format!(
                "failed deterministic task checks: {}",
                failed_check_names.join(", ")
            )
        };
        let check_count = checks.len();
        let mut trusted_approver_bindings = trusted_approvers
            .iter()
            .map(principal_binding)
            .collect::<Vec<_>>();
        trusted_approver_bindings
            .sort_by(|left, right| left["id"].as_str().cmp(&right["id"].as_str()));
        let mut approval_bindings = approvals.iter().map(approval_binding).collect::<Vec<_>>();
        approval_bindings.sort_by(|left, right| {
            left["request"]["body"]["id"]
                .as_str()
                .cmp(&right["request"]["body"]["id"].as_str())
        });
        let trace_digest = binding_digest(
            TRACE_BINDING_SCHEMA,
            &json!({
                "run": run,
                "agent": agent,
                "run_actor": principal_binding(run_actor),
                "trusted_approvers": trusted_approver_bindings,
                "steps": observed_steps,
                "events": events,
                "approvals": approval_bindings,
            }),
        )?;
        let metrics = json!({
            "schema": METRICS_SCHEMA,
            "evaluation_kind": "task_correctness",
            "binding": {
                "algorithm": self.policy_digest.algorithm(),
                "policy_schema": POLICY_BINDING_SCHEMA,
                "policy_digest": self.policy_digest,
                "trace_schema": TRACE_BINDING_SCHEMA,
                "trace_digest": trace_digest,
                "run_revision": run.revision,
                "step_count": steps.len(),
                "event_count": events.len(),
                "approval_count": approvals.len(),
            },
            "passed_checks": passed_checks,
            "total_checks": check_count,
            "score_bps": score_bps,
            "checks": checks
                .into_iter()
                .map(Check::into_value)
                .collect::<Vec<_>>(),
        });

        AgentRunEvaluation::create(
            run,
            evaluator_id,
            outcome,
            Some(score_bps),
            metrics,
            Some(summary),
            created_at,
        )
        .map_err(TraceEvaluationError::from)
    }
}

fn binding_digest(schema: &str, value: &Value) -> Result<ContentDigest, TraceEvaluationError> {
    let canonical = canonicalize(&json!({
        "schema": schema,
        "value": value,
    }))
    .map_err(|error| TraceEvaluationError::Binding(error.to_string()))?;
    Ok(digest(ArtifactKind::Generic, &canonical))
}

// SQLite's legacy principal table does not persist `created_at`, so loaded
// principals receive a read timestamp. Bind only the durable identity fields;
// otherwise identical sealed traces produce a different digest on every read.
fn principal_binding(principal: &Principal) -> Value {
    json!({
        "id": principal.id,
        "kind": principal.kind,
        "public_key": principal.public_key.as_bytes(),
    })
}

fn approval_binding(approval: &ApprovalEvidence) -> Value {
    json!({
        "request": &approval.request,
        "decision": &approval.decision,
        "approver": principal_binding(&approval.approver),
        "execution": &approval.execution,
    })
}

fn proof_operation(operation: &str, version: &str) -> String {
    format!("{operation}::{version}")
}

fn scalar_reference_text(value: &Value) -> Option<String> {
    let value = match value {
        Value::String(value) => Some(value.clone()),
        Value::Number(value) => Some(value.to_string()),
        Value::Bool(value) => Some(value.to_string()),
        _ => None,
    }?;
    (!value.trim().is_empty()).then_some(value)
}

fn final_output_reference_results(
    policy: &TraceEvaluationPolicy,
    matched_positions: &[Option<usize>],
    observed_steps: &[&AgentRunStep],
    events: &[AgentRunEvent],
) -> Vec<Value> {
    let final_output = events
        .last()
        .filter(|event| event.kind == AgentRunEventKind::Completed)
        .and_then(|event| event.data.get("output"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    policy
        .required_final_output_references
        .iter()
        .enumerate()
        .map(|(index, reference)| {
            let Some(position) = matched_positions
                .get(reference.call_index)
                .copied()
                .flatten()
            else {
                return json!({
                    "index": index,
                    "call_index": reference.call_index,
                    "source": reference.source,
                    "pointer": reference.pointer,
                    "issue": "expected_call_not_matched",
                    "passed": false,
                });
            };
            let step = observed_steps[position];
            let expected_call = &policy.expected_calls[reference.call_index];
            let expected_value = match reference.source {
                FinalOutputSource::Arguments => reference
                    .pointer
                    .as_deref()
                    .and_then(|pointer| expected_call.arguments.pointer(pointer))
                    .and_then(scalar_reference_text),
                FinalOutputSource::Output => reference
                    .pointer
                    .as_deref()
                    .and_then(|pointer| step.output.as_ref()?.pointer(pointer))
                    .and_then(scalar_reference_text),
                FinalOutputSource::ProofId => {
                    step.proof.as_ref().map(|proof| proof.body.id.to_string())
                }
            };
            let Some(expected_value) = expected_value else {
                return json!({
                    "index": index,
                    "call_index": reference.call_index,
                    "matched_step_id": step.id,
                    "source": reference.source,
                    "pointer": reference.pointer,
                    "issue": "reference_value_missing_or_not_scalar",
                    "passed": false,
                });
            };
            let reported = final_output.contains(&expected_value);
            json!({
                "index": index,
                "call_index": reference.call_index,
                "matched_step_id": step.id,
                "source": reference.source,
                "pointer": reference.pointer,
                "expected": expected_value,
                "reported": reported,
                "passed": reported,
            })
        })
        .collect()
}

struct Check {
    name: &'static str,
    passed: bool,
    details: Value,
}

impl Check {
    fn new(name: &'static str, passed: bool, details: Value) -> Self {
        Self {
            name,
            passed,
            details,
        }
    }

    fn into_value(self) -> Value {
        json!({
            "name": self.name,
            "passed": self.passed,
            "details": self.details,
        })
    }
}

fn match_expected_calls(
    expected_calls: &[ExpectedToolCall],
    expected_digests: &[ContentDigest],
    observed_steps: &[&AgentRunStep],
) -> Vec<Option<usize>> {
    let mut next_observed = 0;
    expected_calls
        .iter()
        .zip(expected_digests)
        .map(|(expected, expected_digest)| {
            let matched = observed_steps[next_observed..]
                .iter()
                .position(|step| {
                    step.operation == expected.operation
                        && step.version == expected.version
                        && step.input_digest == *expected_digest
                })
                .map(|offset| next_observed + offset);
            if let Some(position) = matched {
                next_observed = position + 1;
            }
            matched
        })
        .collect()
}

fn expected_call_metrics(
    expected_calls: &[ExpectedToolCall],
    expected_digests: &[ContentDigest],
    matched_positions: &[Option<usize>],
    observed_steps: &[&AgentRunStep],
) -> Vec<Value> {
    expected_calls
        .iter()
        .zip(expected_digests)
        .zip(matched_positions)
        .enumerate()
        .map(|(index, ((expected, expected_digest), position))| {
            json!({
                "index": index,
                "operation": expected.operation,
                "version": expected.version,
                "argument_digest": expected_digest,
                "requires_approved_execution": expected.requires_approved_execution,
                "matched_step_id": position.map(|position| observed_steps[position].id),
            })
        })
        .collect()
}

fn observed_call_metrics(observed_steps: &[&AgentRunStep]) -> Vec<Value> {
    observed_steps
        .iter()
        .map(|step| {
            json!({
                "step_id": step.id,
                "ordinal": step.ordinal,
                "attempt": step.attempt,
                "operation": step.operation,
                "version": step.version,
                "argument_digest": step.input_digest,
            })
        })
        .collect()
}

fn invalid_step_details(
    run: &AgentRun,
    run_actor: &Principal,
    step: &AgentRunStep,
) -> Option<Value> {
    let mut issues = Vec::new();
    if step.run_id != run.id {
        issues.push("step_run_mismatch");
    }
    if step.status != AgentRunStepStatus::Succeeded {
        issues.push("step_not_succeeded");
    }
    let Some(output) = &step.output else {
        issues.push("missing_output");
        return Some(json!({"step_id": step.id, "issues": issues}));
    };
    let Some(proof) = &step.proof else {
        issues.push("missing_proof");
        return Some(json!({"step_id": step.id, "issues": issues}));
    };
    if proof.body.actor != run.actor || proof.body.actor != run_actor.id {
        issues.push("proof_actor_mismatch");
    }
    if proof.body.operation != proof_operation(&step.operation, &step.version) {
        issues.push("proof_operation_mismatch");
    }
    if proof.body.input_digest != step.input_digest {
        issues.push("proof_input_mismatch");
    }
    match canonical_digest(ArtifactKind::OperationOutput, output) {
        Some(output_digest) if proof.body.output_digest != output_digest => {
            issues.push("proof_output_mismatch");
        }
        None => issues.push("invalid_output"),
        _ => {}
    }
    if proof.verify(&run_actor.public_key).is_err() {
        issues.push("invalid_proof_signature");
    }
    match (step.started_at, step.completed_at) {
        (Some(started_at), Some(completed_at)) => {
            if proof.body.timestamp < started_at || proof.body.timestamp > completed_at {
                issues.push("proof_timestamp_outside_step_window");
            }
        }
        _ => issues.push("missing_step_execution_window"),
    }
    (!issues.is_empty()).then(|| json!({"step_id": step.id, "issues": issues}))
}

#[allow(clippy::too_many_arguments)]
fn required_approval_results(
    run: &AgentRun,
    run_actor: &Principal,
    trusted_approvers: &[Principal],
    expected_calls: &[ExpectedToolCall],
    matched_positions: &[Option<usize>],
    observed_steps: &[&AgentRunStep],
    approvals: &[ApprovalEvidence],
) -> Vec<Value> {
    let mut results = Vec::new();
    let mut checked_steps = BTreeSet::new();
    let mut used_request_ids = BTreeSet::new();

    for (expected_index, (expected, position)) in
        expected_calls.iter().zip(matched_positions).enumerate()
    {
        if !expected.requires_approved_execution {
            continue;
        }
        let step = position.map(|position| observed_steps[position]);
        if let Some(step) = step {
            checked_steps.insert(step.id);
        }
        results.push(approval_result(
            run,
            run_actor,
            trusted_approvers,
            approvals,
            step,
            Some(expected_index),
            &mut used_request_ids,
        ));
    }

    for step in observed_steps {
        if step.approval_request_id.is_some() && checked_steps.insert(step.id) {
            results.push(approval_result(
                run,
                run_actor,
                trusted_approvers,
                approvals,
                Some(step),
                None,
                &mut used_request_ids,
            ));
        }
    }

    for evidence in approvals {
        if !used_request_ids.contains(&evidence.request.body.id) {
            results.push(json!({
                "expected_index": Value::Null,
                "step_id": Value::Null,
                "request_id": evidence.request.body.id,
                "passed": false,
                "issues": ["unused_approval_evidence"],
            }));
        }
    }
    results
}

#[allow(clippy::too_many_arguments)]
fn approval_result(
    run: &AgentRun,
    run_actor: &Principal,
    trusted_approvers: &[Principal],
    approvals: &[ApprovalEvidence],
    step: Option<&AgentRunStep>,
    expected_index: Option<usize>,
    used_request_ids: &mut BTreeSet<Uuid>,
) -> Value {
    let Some(step) = step else {
        return json!({
            "expected_index": expected_index,
            "step_id": Value::Null,
            "request_id": Value::Null,
            "passed": false,
            "issues": ["missing_expected_step"],
        });
    };
    let Some(request_id) = step.approval_request_id else {
        return json!({
            "expected_index": expected_index,
            "step_id": step.id,
            "request_id": Value::Null,
            "passed": false,
            "issues": ["missing_step_approval_request"],
        });
    };
    if !used_request_ids.insert(request_id) {
        return json!({
            "expected_index": expected_index,
            "step_id": step.id,
            "request_id": request_id,
            "passed": false,
            "issues": ["approval_request_reused"],
        });
    }
    let matches = approvals
        .iter()
        .filter(|evidence| evidence.request.body.id == request_id)
        .collect::<Vec<_>>();
    if matches.len() != 1 {
        return json!({
            "expected_index": expected_index,
            "step_id": step.id,
            "request_id": request_id,
            "passed": false,
            "issues": if matches.is_empty() {
                vec!["missing_approval_evidence"]
            } else {
                vec!["duplicate_approval_evidence"]
            },
        });
    }
    let evidence = matches[0];
    let issues = approval_evidence_issues(run, run_actor, trusted_approvers, step, evidence);
    json!({
        "expected_index": expected_index,
        "step_id": step.id,
        "request_id": request_id,
        "approver_id": evidence.approver.id,
        "passed": issues.is_empty(),
        "issues": issues,
    })
}

fn approval_evidence_issues(
    run: &AgentRun,
    run_actor: &Principal,
    trusted_approvers: &[Principal],
    step: &AgentRunStep,
    evidence: &ApprovalEvidence,
) -> Vec<&'static str> {
    let mut issues = Vec::new();
    let request = &evidence.request;
    let decision = &evidence.decision;
    let execution = &evidence.execution;

    if !trusted_approvers
        .iter()
        .any(|trusted| same_principal(trusted, &evidence.approver))
    {
        issues.push("untrusted_approver");
    }
    if request.verify(run_actor).is_err() {
        issues.push("invalid_request_signature");
    }
    if request.body.requested_by != run.actor || request.body.requested_by != run_actor.id {
        issues.push("request_actor_mismatch");
    }
    if request.body.operation != step.operation {
        issues.push("request_operation_mismatch");
    }
    if request.body.version != step.version {
        issues.push("request_version_mismatch");
    }
    if request.body.input_digest != step.input_digest {
        issues.push("request_input_mismatch");
    }
    if request.body.expires_at <= request.body.requested_at {
        issues.push("invalid_request_window");
    }
    if step
        .started_at
        .is_none_or(|started_at| request.body.requested_at < started_at)
    {
        issues.push("approval_request_precedes_step");
    }
    if decision.verify(&evidence.approver).is_err() {
        issues.push("invalid_decision_signature");
    }
    if decision.body.request_id != request.body.id {
        issues.push("decision_request_mismatch");
    }
    if match request.digest() {
        Ok(request_digest) => request_digest != decision.body.request_digest,
        Err(_) => true,
    } {
        issues.push("decision_request_digest_mismatch");
    }
    if decision.body.outcome != ApprovalOutcome::Approved {
        issues.push("decision_not_approved");
    }
    if decision.body.decided_at < request.body.requested_at
        || decision.body.decided_at > request.body.expires_at
    {
        issues.push("decision_out_of_window");
    }
    if execution.request_id != request.body.id {
        issues.push("execution_request_mismatch");
    }
    if execution.executed_at < decision.body.decided_at
        || execution.executed_at > request.body.expires_at
    {
        issues.push("execution_out_of_window");
    }
    if step
        .completed_at
        .is_none_or(|completed_at| execution.executed_at > completed_at)
    {
        issues.push("approval_execution_outside_step");
    }
    let outputs_match = step.output.as_ref().is_some_and(|step_output| {
        match (canonicalize(step_output), canonicalize(&execution.output)) {
            (Ok(step_output), Ok(execution_output)) => step_output == execution_output,
            _ => false,
        }
    });
    if !outputs_match {
        issues.push("execution_output_mismatch");
    }
    if step.proof.as_ref() != Some(&execution.proof) {
        issues.push("execution_proof_mismatch");
    }
    if execution.proof.body.timestamp != execution.executed_at {
        issues.push("execution_timestamp_mismatch");
    }
    if execution.proof.body.actor != run.actor
        || execution.proof.body.operation != proof_operation(&step.operation, &step.version)
        || execution.proof.body.input_digest != step.input_digest
    {
        issues.push("execution_proof_binding_mismatch");
    }
    if canonical_digest(ArtifactKind::OperationOutput, &execution.output)
        != Some(execution.proof.body.output_digest)
    {
        issues.push("execution_proof_output_mismatch");
    }
    if execution.proof.verify(&run_actor.public_key).is_err() {
        issues.push("invalid_execution_proof_signature");
    }
    issues
}

fn lifecycle_issues(
    run: &AgentRun,
    agent: &AgentDefinition,
    observed_steps: &[&AgentRunStep],
    events: &[AgentRunEvent],
    approvals: &[ApprovalEvidence],
    required_approval_step_ids: &BTreeSet<Uuid>,
) -> Vec<Value> {
    let mut issues = step_topology_issues(run, observed_steps);
    for (index, event) in events.iter().enumerate() {
        if index > 0 && event.created_at < events[index - 1].created_at {
            push_issue(
                &mut issues,
                "event_timestamp_not_monotonic",
                json!({
                    "index": index,
                    "previous_created_at": events[index - 1].created_at,
                    "created_at": event.created_at,
                }),
            );
        }
        if event.run_id != run.id {
            push_issue(
                &mut issues,
                "event_run_mismatch",
                json!({"index": index, "event_id": event.id}),
            );
        }
        if usize::try_from(event.sequence).ok() != Some(index) {
            push_issue(
                &mut issues,
                "event_sequence_not_contiguous",
                json!({"index": index, "sequence": event.sequence}),
            );
        }
        if canonical_digest(ArtifactKind::AgentEvent, &event.data) != Some(event.data_digest) {
            push_issue(
                &mut issues,
                "invalid_event_data_digest",
                json!({"index": index, "sequence": event.sequence}),
            );
        }
    }
    let Some(started) = events.first() else {
        push_issue(&mut issues, "missing_started_event", json!({}));
        push_issue(&mut issues, "missing_completed_event", json!({}));
        return issues;
    };
    if started.kind != AgentRunEventKind::Started {
        push_issue(
            &mut issues,
            "started_event_not_first",
            json!({"first_kind": started.kind}),
        );
    } else {
        if started.created_at < run.created_at {
            push_issue(&mut issues, "started_event_precedes_run", json!({}));
        }
        if data_uuid(&started.data, "agent_id") != Some(agent.id) {
            push_issue(&mut issues, "started_agent_mismatch", json!({}));
        }
        if started.data.get("goal").and_then(Value::as_str) != Some(run.goal.as_str()) {
            push_issue(&mut issues, "started_goal_mismatch", json!({}));
        }
    }
    let Some(completed) = events.last() else {
        unreachable!("first event checked above")
    };
    if completed.kind != AgentRunEventKind::Completed {
        push_issue(
            &mut issues,
            "completed_event_not_last",
            json!({"last_kind": completed.kind}),
        );
        return issues;
    }
    if run
        .completed_at
        .is_none_or(|completed_at| completed.created_at < completed_at)
    {
        push_issue(
            &mut issues,
            "completed_event_precedes_run_completion",
            json!({}),
        );
    }
    let completed_output = completed.data.get("output").and_then(Value::as_str);
    if completed_output.is_none_or(|output| output.trim().is_empty()) {
        push_issue(&mut issues, "completed_output_empty", json!({}));
    }
    if events
        .iter()
        .filter(|event| event.kind == AgentRunEventKind::Started)
        .count()
        != 1
    {
        push_issue(&mut issues, "started_event_count_invalid", json!({}));
    }
    if events
        .iter()
        .filter(|event| event.kind == AgentRunEventKind::Completed)
        .count()
        != 1
    {
        push_issue(&mut issues, "completed_event_count_invalid", json!({}));
    }
    if started.kind != AgentRunEventKind::Started || completed.kind != AgentRunEventKind::Completed
    {
        return issues;
    }

    let last_index = events.len() - 1;
    let mut index = 1;
    let mut step_index = 0;
    let mut model_call = 1_u64;
    let mut previous_response_id: Option<String> = None;
    let mut saw_finish = false;
    while index < last_index {
        let requested = &events[index];
        if requested.kind != AgentRunEventKind::ModelRequested {
            push_issue(
                &mut issues,
                "expected_model_requested",
                json!({"index": index, "kind": requested.kind}),
            );
            break;
        }
        if requested.data.get("model").and_then(Value::as_str) != Some(agent.model.as_str()) {
            push_issue(
                &mut issues,
                "model_request_model_mismatch",
                json!({"index": index}),
            );
        }
        if requested.data.get("model_call").and_then(Value::as_u64) != Some(model_call) {
            push_issue(
                &mut issues,
                "model_call_number_mismatch",
                json!({"index": index, "expected": model_call}),
            );
        }
        if requested
            .data
            .get("previous_response_id")
            .and_then(Value::as_str)
            != previous_response_id.as_deref()
        {
            push_issue(
                &mut issues,
                "previous_response_id_mismatch",
                json!({"index": index}),
            );
        }
        let Some(responded) = events.get(index + 1) else {
            push_issue(
                &mut issues,
                "model_response_missing",
                json!({"index": index}),
            );
            break;
        };
        if responded.kind != AgentRunEventKind::ModelResponded {
            push_issue(
                &mut issues,
                "model_request_not_paired",
                json!({"index": index, "next_kind": responded.kind}),
            );
            break;
        }
        let response_id = responded
            .data
            .get("response_id")
            .and_then(Value::as_str)
            .filter(|response_id| !response_id.trim().is_empty());
        if response_id.is_none() {
            push_issue(
                &mut issues,
                "model_response_id_empty",
                json!({"index": index + 1}),
            );
        }
        let decision = responded.data.get("decision").unwrap_or(&Value::Null);
        index += 2;
        match decision.get("type").and_then(Value::as_str) {
            Some("tool_call") => {
                let Some(step) = observed_steps.get(step_index).copied() else {
                    push_issue(
                        &mut issues,
                        "tool_event_without_step",
                        json!({"index": index}),
                    );
                    break;
                };
                let Some(tool_requested) = events.get(index) else {
                    push_issue(
                        &mut issues,
                        "tool_requested_missing",
                        json!({"step_id": step.id}),
                    );
                    break;
                };
                if tool_requested.kind != AgentRunEventKind::ToolRequested {
                    push_issue(
                        &mut issues,
                        "tool_requested_out_of_order",
                        json!({"index": index, "kind": tool_requested.kind}),
                    );
                    break;
                }
                validate_tool_requested(&mut issues, step, decision, tool_requested);
                let call_id = tool_requested
                    .data
                    .get("call_id")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                index += 1;
                if required_approval_step_ids.contains(&step.id)
                    && step.approval_request_id.is_none()
                {
                    push_issue(
                        &mut issues,
                        "required_step_approval_request_missing",
                        json!({"step_id": step.id}),
                    );
                }
                if let Some(request_id) = step.approval_request_id {
                    let Some(required) = events.get(index) else {
                        push_issue(
                            &mut issues,
                            "approval_required_missing",
                            json!({"step_id": step.id}),
                        );
                        break;
                    };
                    if required.kind != AgentRunEventKind::ApprovalRequired {
                        push_issue(
                            &mut issues,
                            "approval_required_out_of_order",
                            json!({"index": index, "kind": required.kind}),
                        );
                        break;
                    }
                    validate_approval_required(
                        &mut issues,
                        step,
                        request_id,
                        tool_requested,
                        required,
                        approvals,
                    );
                    index += 1;
                    let Some(resumed) = events.get(index) else {
                        push_issue(
                            &mut issues,
                            "approval_resumed_missing",
                            json!({"step_id": step.id}),
                        );
                        break;
                    };
                    if resumed.kind != AgentRunEventKind::ApprovalResumed {
                        push_issue(
                            &mut issues,
                            "approval_resumed_out_of_order",
                            json!({"index": index, "kind": resumed.kind}),
                        );
                        break;
                    }
                    validate_approval_resumed(
                        &mut issues,
                        step,
                        request_id,
                        required,
                        resumed,
                        approvals,
                    );
                    index += 1;
                }
                let Some(succeeded) = events.get(index) else {
                    push_issue(
                        &mut issues,
                        "tool_succeeded_missing",
                        json!({"step_id": step.id}),
                    );
                    break;
                };
                if succeeded.kind != AgentRunEventKind::ToolSucceeded {
                    push_issue(
                        &mut issues,
                        "tool_succeeded_out_of_order",
                        json!({"index": index, "kind": succeeded.kind}),
                    );
                    break;
                }
                validate_tool_succeeded(&mut issues, step, call_id, succeeded);
                index += 1;
                step_index += 1;
            }
            Some("finish") => {
                saw_finish = true;
                if index != last_index {
                    push_issue(
                        &mut issues,
                        "finish_not_followed_by_completed",
                        json!({"index": index}),
                    );
                }
                let finish_output = decision.get("output").and_then(Value::as_str);
                if finish_output.is_none_or(|output| output.trim().is_empty()) {
                    push_issue(&mut issues, "finish_output_empty", json!({}));
                }
                if finish_output != completed_output {
                    push_issue(&mut issues, "finish_completed_output_mismatch", json!({}));
                }
                index = last_index;
            }
            _ => {
                push_issue(
                    &mut issues,
                    "invalid_model_decision",
                    json!({"index": index.saturating_sub(1)}),
                );
                break;
            }
        }
        previous_response_id = response_id.map(ToOwned::to_owned);
        model_call += 1;
    }
    if !saw_finish {
        push_issue(&mut issues, "missing_finish_decision", json!({}));
    }
    if step_index != observed_steps.len() {
        push_issue(
            &mut issues,
            "step_event_count_mismatch",
            json!({"steps": observed_steps.len(), "bound_steps": step_index}),
        );
    }
    issues
}

fn step_topology_issues(run: &AgentRun, observed_steps: &[&AgentRunStep]) -> Vec<Value> {
    let mut issues = Vec::new();
    if run.updated_at < run.created_at {
        push_issue(&mut issues, "run_timestamp_order_invalid", json!({}));
    }
    match run.completed_at {
        Some(completed_at) if completed_at < run.created_at || completed_at != run.updated_at => {
            push_issue(&mut issues, "run_completion_timestamp_invalid", json!({}));
        }
        None if run.status.is_terminal() => {
            push_issue(&mut issues, "terminal_run_completion_missing", json!({}));
        }
        Some(_) if !run.status.is_terminal() => {
            push_issue(&mut issues, "nonterminal_run_has_completion", json!({}));
        }
        _ => {}
    }

    let mut seen_ids = BTreeSet::new();
    let mut current_ordinal = None;
    let mut expected_ordinal = 0_u32;
    let mut expected_attempt = 1_u32;
    let mut previous_attempt: Option<&AgentRunStep> = None;
    for step in observed_steps {
        if !seen_ids.insert(step.id) {
            push_issue(
                &mut issues,
                "duplicate_step_id",
                json!({"step_id": step.id}),
            );
        }
        if current_ordinal != Some(step.ordinal) {
            if step.ordinal != expected_ordinal {
                push_issue(
                    &mut issues,
                    "step_ordinal_not_contiguous",
                    json!({
                        "step_id": step.id,
                        "expected": expected_ordinal,
                        "actual": step.ordinal,
                    }),
                );
            }
            expected_ordinal = expected_ordinal.saturating_add(1);
            current_ordinal = Some(step.ordinal);
            expected_attempt = 1;
            previous_attempt = None;
        }
        if step.attempt != expected_attempt {
            push_issue(
                &mut issues,
                "step_attempt_not_contiguous",
                json!({
                    "step_id": step.id,
                    "expected": expected_attempt,
                    "actual": step.attempt,
                }),
            );
        }
        if let Some(previous) = previous_attempt {
            if step.retry_of != Some(previous.id) {
                push_issue(
                    &mut issues,
                    "step_retry_lineage_invalid",
                    json!({"step_id": step.id, "expected_retry_of": previous.id}),
                );
            }
            if !matches!(
                previous.status,
                AgentRunStepStatus::Failed | AgentRunStepStatus::Cancelled
            ) {
                push_issue(
                    &mut issues,
                    "step_retry_parent_not_retryable",
                    json!({"step_id": step.id, "retry_of": previous.id}),
                );
            }
            if step.operation != previous.operation
                || step.version != previous.version
                || step.input_digest != previous.input_digest
            {
                push_issue(
                    &mut issues,
                    "step_retry_call_mismatch",
                    json!({"step_id": step.id, "retry_of": previous.id}),
                );
            }
            if previous
                .completed_at
                .is_none_or(|completed_at| step.created_at < completed_at)
            {
                push_issue(
                    &mut issues,
                    "step_retry_precedes_parent_completion",
                    json!({"step_id": step.id, "retry_of": previous.id}),
                );
            }
        } else if step.retry_of.is_some() {
            push_issue(
                &mut issues,
                "first_step_attempt_has_retry_parent",
                json!({"step_id": step.id, "retry_of": step.retry_of}),
            );
        }
        expected_attempt = expected_attempt.saturating_add(1);
        previous_attempt = Some(step);

        if step.created_at < run.created_at || step.updated_at < step.created_at {
            push_issue(
                &mut issues,
                "step_timestamp_order_invalid",
                json!({"step_id": step.id}),
            );
        }
        if step
            .started_at
            .is_some_and(|started_at| started_at < step.created_at)
            || step.completed_at.is_some_and(|completed_at| {
                step.started_at
                    .is_none_or(|started_at| completed_at < started_at)
            })
        {
            push_issue(
                &mut issues,
                "step_execution_window_invalid",
                json!({"step_id": step.id}),
            );
        }
        if let Some(run_completed_at) = run.completed_at {
            let after_run = step.created_at > run_completed_at
                || step.updated_at > run_completed_at
                || step
                    .started_at
                    .is_some_and(|started_at| started_at > run_completed_at)
                || step
                    .completed_at
                    .is_some_and(|completed_at| completed_at > run_completed_at);
            if after_run {
                push_issue(
                    &mut issues,
                    "step_timestamp_outside_run",
                    json!({"step_id": step.id}),
                );
            }
        }
    }
    issues
}

fn validate_tool_requested(
    issues: &mut Vec<Value>,
    step: &AgentRunStep,
    decision: &Value,
    event: &AgentRunEvent,
) {
    if data_uuid(&event.data, "step_id") != Some(step.id) {
        push_issue(issues, "tool_requested_step_mismatch", json!({}));
    }
    if event.data.get("operation").and_then(Value::as_str) != Some(step.operation.as_str())
        || event.data.get("version").and_then(Value::as_str) != Some(step.version.as_str())
    {
        push_issue(issues, "tool_requested_operation_mismatch", json!({}));
    }
    let call_id = event.data.get("call_id").and_then(Value::as_str);
    if call_id.is_none_or(|call_id| call_id.trim().is_empty())
        || call_id != decision.get("call_id").and_then(Value::as_str)
    {
        push_issue(issues, "tool_requested_call_id_mismatch", json!({}));
    }
    if event.data.get("tool").and_then(Value::as_str)
        != decision.get("name").and_then(Value::as_str)
    {
        push_issue(issues, "tool_requested_name_mismatch", json!({}));
    }
    let event_arguments = event.data.get("arguments");
    let decision_arguments = decision.get("arguments");
    if event_arguments
        .and_then(|arguments| canonical_digest(ArtifactKind::OperationInput, arguments))
        != Some(step.input_digest)
    {
        push_issue(issues, "tool_requested_arguments_mismatch", json!({}));
    }
    if match (event_arguments, decision_arguments) {
        (Some(event_arguments), Some(decision_arguments)) => {
            canonicalize(event_arguments).ok() != canonicalize(decision_arguments).ok()
        }
        _ => true,
    } {
        push_issue(issues, "model_tool_arguments_mismatch", json!({}));
    }
}

fn validate_approval_required(
    issues: &mut Vec<Value>,
    step: &AgentRunStep,
    request_id: Uuid,
    tool_requested: &AgentRunEvent,
    event: &AgentRunEvent,
    approvals: &[ApprovalEvidence],
) {
    if data_uuid(&event.data, "step_id") != Some(step.id)
        || data_uuid(&event.data, "request_id") != Some(request_id)
    {
        push_issue(issues, "approval_required_id_mismatch", json!({}));
    }
    if event.data.get("operation").and_then(Value::as_str) != Some(step.operation.as_str())
        || event.data.get("version").and_then(Value::as_str) != Some(step.version.as_str())
    {
        push_issue(issues, "approval_required_operation_mismatch", json!({}));
    }
    let Some(evidence) = approval_by_request(approvals, request_id) else {
        push_issue(issues, "approval_required_evidence_missing", json!({}));
        return;
    };
    if event.data.get("expires_at") != Some(&json!(evidence.request.body.expires_at)) {
        push_issue(issues, "approval_required_expiration_mismatch", json!({}));
    }
    if event.created_at < evidence.request.body.requested_at
        || event.created_at > evidence.request.body.expires_at
    {
        push_issue(
            issues,
            "approval_required_timestamp_out_of_window",
            json!({}),
        );
    }
    if evidence.request.body.requested_at < tool_requested.created_at {
        push_issue(issues, "approval_request_precedes_tool", json!({}));
    }
}

fn validate_approval_resumed(
    issues: &mut Vec<Value>,
    step: &AgentRunStep,
    request_id: Uuid,
    required: &AgentRunEvent,
    event: &AgentRunEvent,
    approvals: &[ApprovalEvidence],
) {
    if data_uuid(&event.data, "step_id") != Some(step.id)
        || data_uuid(&event.data, "request_id") != Some(request_id)
    {
        push_issue(issues, "approval_resumed_id_mismatch", json!({}));
    }
    let Some(evidence) = approval_by_request(approvals, request_id) else {
        push_issue(issues, "approval_resumed_evidence_missing", json!({}));
        return;
    };
    if data_principal_id(&event.data, "decided_by") != Some(evidence.decision.body.decided_by)
        || event.data.get("outcome") != Some(&json!(evidence.decision.body.outcome))
    {
        push_issue(issues, "approval_resumed_decision_mismatch", json!({}));
    }
    if event.created_at < evidence.decision.body.decided_at
        || event.created_at > evidence.request.body.expires_at
    {
        push_issue(
            issues,
            "approval_resumed_timestamp_out_of_window",
            json!({}),
        );
    }
    if evidence.decision.body.decided_at < required.created_at {
        push_issue(
            issues,
            "approval_decision_precedes_required_event",
            json!({}),
        );
    }
    if evidence.execution.executed_at < event.created_at {
        push_issue(
            issues,
            "approval_execution_precedes_resumed_event",
            json!({}),
        );
    }
}

fn validate_tool_succeeded(
    issues: &mut Vec<Value>,
    step: &AgentRunStep,
    call_id: &str,
    event: &AgentRunEvent,
) {
    if data_uuid(&event.data, "step_id") != Some(step.id) {
        push_issue(issues, "tool_succeeded_step_mismatch", json!({}));
    }
    if event.data.get("call_id").and_then(Value::as_str) != Some(call_id) {
        push_issue(issues, "tool_succeeded_call_id_mismatch", json!({}));
    }
    if event.data.get("operation").and_then(Value::as_str) != Some(step.operation.as_str())
        || event.data.get("version").and_then(Value::as_str) != Some(step.version.as_str())
    {
        push_issue(issues, "tool_succeeded_operation_mismatch", json!({}));
    }
    let proof_id = step.proof.as_ref().map(|proof| proof.body.id);
    if data_uuid(&event.data, "proof_id") != proof_id {
        push_issue(issues, "tool_succeeded_proof_mismatch", json!({}));
    }
    if step
        .proof
        .as_ref()
        .is_some_and(|proof| event.created_at < proof.body.timestamp)
        || step
            .completed_at
            .is_some_and(|completed_at| event.created_at < completed_at)
    {
        push_issue(issues, "tool_succeeded_timestamp_precedes_step", json!({}));
    }
}

fn approval_by_request(
    approvals: &[ApprovalEvidence],
    request_id: Uuid,
) -> Option<&ApprovalEvidence> {
    let mut matches = approvals
        .iter()
        .filter(|evidence| evidence.request.body.id == request_id);
    let evidence = matches.next()?;
    matches.next().is_none().then_some(evidence)
}

fn same_principal(left: &Principal, right: &Principal) -> bool {
    left.id == right.id && left.kind == right.kind && left.public_key == right.public_key
}

fn data_uuid(data: &Value, key: &str) -> Option<Uuid> {
    data.get(key)
        .and_then(Value::as_str)
        .and_then(|value| Uuid::parse_str(value).ok())
}

fn data_principal_id(data: &Value, key: &str) -> Option<proof_kernel::PrincipalId> {
    data_uuid(data, key).map(proof_kernel::PrincipalId::new)
}

fn canonical_digest(kind: ArtifactKind, value: &Value) -> Option<ContentDigest> {
    canonicalize(value)
        .ok()
        .map(|canonical| digest(kind, &canonical))
}

fn push_issue(issues: &mut Vec<Value>, code: &'static str, details: Value) {
    issues.push(json!({"code": code, "details": details}));
}

#[cfg(test)]
mod tests {
    use chrono::Duration;
    use ed25519_dalek::SigningKey;
    use proof_kernel::{
        create_proof, generate_keypair, generate_keypair_for, principal_from_keypair, sign,
        AgentLimits, AgentRunMode, Keypair, PrincipalId,
    };

    use super::*;

    const OPERATION: &str = "release.publish";
    const VERSION: &str = "v1";

    fn release_arguments() -> Value {
        json!({"environment": "preview", "version_label": "2026.08.29-rc1"})
    }

    fn release_output() -> Value {
        json!({
            "operation": "release.publish",
            "data": {
                "release": {
                    "id": "018f0000-0000-7000-8000-000000000020",
                    "edition_id": "018f0000-0000-7000-8000-000000000021",
                    "environment": "preview",
                    "published_at": "2026-08-29T20:00:00Z",
                    "published_by": "018f0000-0000-7000-8000-000000000001"
                },
                "version_label": "2026.08.29-rc1"
            }
        })
    }

    fn policy(expected_calls: Vec<ExpectedToolCall>) -> DeterministicTraceEvaluator {
        DeterministicTraceEvaluator::new(TraceEvaluationPolicy::new(expected_calls, false)).unwrap()
    }

    fn release_policy() -> DeterministicTraceEvaluator {
        let policy: TraceEvaluationPolicy = serde_json::from_str(include_str!(
            "../../../evals/release-manager-preview-v1.json"
        ))
        .unwrap();
        DeterministicTraceEvaluator::new(policy).unwrap()
    }

    fn fixture_time() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-08-29T20:00:00Z")
            .unwrap()
            .with_timezone(&Utc)
    }

    fn fixture_uuid(number: u16) -> Uuid {
        Uuid::parse_str(&format!("018f0000-0000-7{number:03x}-8000-000000000000")).unwrap()
    }

    fn fixture_keypair(number: u16, kind: PrincipalKind, at: DateTime<Utc>) -> Keypair {
        Keypair {
            principal_id: PrincipalId::new(fixture_uuid(number)),
            kind,
            created_at: at,
            signing_key: SigningKey::from_bytes(&[number as u8; 32]),
        }
    }

    fn signed_bytes<T: Serialize>(value: &T, keypair: &Keypair) -> Vec<u8> {
        sign(
            keypair,
            proof_kernel::canonicalize_serialized(value)
                .unwrap()
                .as_bytes(),
        )
        .to_bytes()
        .to_vec()
    }

    fn agent(at: DateTime<Utc>, tools: Vec<AgentTool>) -> AgentDefinition {
        AgentDefinition::new(
            "release-manager",
            "Publish the exact approved release.",
            "test",
            "test-model",
            tools,
            AgentLimits::default(),
            at,
        )
        .unwrap()
    }

    fn release_agent(at: DateTime<Utc>) -> AgentDefinition {
        agent(at, vec![AgentTool::new(OPERATION, VERSION).unwrap()])
    }

    fn started_run(actor: &Keypair, agent: &AgentDefinition, at: DateTime<Utc>) -> AgentRun {
        let mut run = AgentRun::new_for_agent(
            actor.principal_id,
            agent.id,
            AgentRunMode::OneShot,
            "Publish the preview release.",
            at,
        )
        .unwrap();
        run.start(at + Duration::seconds(1)).unwrap();
        run
    }

    fn direct_step(
        run: &AgentRun,
        actor: &Keypair,
        ordinal: u32,
        operation: &str,
        version: &str,
        input: &Value,
        output: &Value,
        started_at: DateTime<Utc>,
        proof_at: DateTime<Utc>,
        completed_at: DateTime<Utc>,
    ) -> AgentRunStep {
        let mut step =
            AgentRunStep::new(run.id, ordinal, operation, version, input, started_at).unwrap();
        step.start(started_at).unwrap();
        let proof = create_proof(
            actor.principal_id,
            None,
            &proof_operation(operation, version),
            input,
            output,
            proof_at,
            actor,
        )
        .unwrap();
        step.succeed(output.clone(), proof, completed_at).unwrap();
        step
    }

    fn approved_step(
        run: &AgentRun,
        actor: &Keypair,
        approver: &Keypair,
        ordinal: u32,
        input: &Value,
        output: &Value,
        at: DateTime<Utc>,
    ) -> (AgentRunStep, ApprovalEvidence) {
        let requested_at = at + Duration::seconds(2);
        let decided_at = at + Duration::seconds(3);
        let executed_at = at + Duration::seconds(5);
        let mut step = AgentRunStep::new(run.id, ordinal, OPERATION, VERSION, input, at).unwrap();
        step.id = fixture_uuid(5);
        step.start(at + Duration::seconds(1)).unwrap();
        let mut request = SignedApprovalRequest::create(
            OPERATION,
            VERSION,
            input,
            requested_at,
            at + Duration::minutes(5),
            actor,
        )
        .unwrap();
        request.body.id = fixture_uuid(6);
        request.signature = signed_bytes(&request.body, actor);
        step.wait_for_approval(request.body.id, requested_at)
            .unwrap();
        let mut decision = SignedApprovalDecision::create(
            &request,
            ApprovalOutcome::Approved,
            Some("approved for preview".to_string()),
            decided_at,
            approver,
        )
        .unwrap();
        decision.body.id = fixture_uuid(7);
        decision.body.request_digest = request.digest().unwrap();
        decision.signature = signed_bytes(&decision.body, approver);
        step.resume_from_approval(at + Duration::seconds(4))
            .unwrap();
        let mut proof = create_proof(
            actor.principal_id,
            None,
            &proof_operation(OPERATION, VERSION),
            input,
            output,
            executed_at,
            actor,
        )
        .unwrap();
        proof.body.id = fixture_uuid(8);
        proof = proof.sign(actor).unwrap();
        step.succeed(output.clone(), proof.clone(), at + Duration::seconds(6))
            .unwrap();
        let execution = ApprovalExecution {
            request_id: request.body.id,
            executed_at,
            output: output.clone(),
            proof,
        };
        (
            step,
            ApprovalEvidence::new(
                request,
                decision,
                principal_from_keypair(approver),
                execution,
            ),
        )
    }

    struct EventBuilder {
        run_id: Uuid,
        model: String,
        at: DateTime<Utc>,
        terminal_at: Option<DateTime<Utc>>,
        model_call: u64,
        previous_response_id: Option<String>,
        events: Vec<AgentRunEvent>,
    }

    impl EventBuilder {
        fn new(run: &AgentRun, agent: &AgentDefinition, at: DateTime<Utc>) -> Self {
            let mut builder = Self {
                run_id: run.id,
                model: agent.model.clone(),
                at,
                terminal_at: run.completed_at,
                model_call: 1,
                previous_response_id: None,
                events: Vec::new(),
            };
            builder.push(
                AgentRunEventKind::Started,
                json!({"agent_id": agent.id, "goal": run.goal}),
            );
            builder
        }

        fn push(&mut self, kind: AgentRunEventKind, data: Value) {
            let sequence = u32::try_from(self.events.len()).unwrap();
            let mut event =
                AgentRunEvent::create(self.run_id, sequence, kind, data, self.at).unwrap();
            event.id = fixture_uuid(30 + sequence as u16);
            self.events.push(event);
            self.at += Duration::milliseconds(1);
        }

        fn push_at_or_after(
            &mut self,
            kind: AgentRunEventKind,
            data: Value,
            earliest: DateTime<Utc>,
        ) {
            if self.at < earliest {
                self.at = earliest;
            }
            self.push(kind, data);
        }

        fn model_requested(&mut self) {
            self.push(
                AgentRunEventKind::ModelRequested,
                json!({
                    "model": self.model,
                    "model_call": self.model_call,
                    "previous_response_id": self.previous_response_id,
                }),
            );
        }

        fn tool_turn(
            &mut self,
            step: &AgentRunStep,
            arguments: &Value,
            approval: Option<&ApprovalEvidence>,
            call_suffix: &str,
        ) {
            self.model_requested();
            let call_id = format!("call_{call_suffix}");
            let response_id = format!("resp_{call_suffix}");
            let tool_name = format!(
                "proof_{}_{}",
                step.operation.replace('.', "_"),
                step.version
            );
            let decision = json!({
                "type": "tool_call",
                "call_id": call_id,
                "name": tool_name,
                "arguments": arguments,
            });
            self.push(
                AgentRunEventKind::ModelResponded,
                json!({"response_id": response_id, "decision": decision, "usage": {}}),
            );
            self.push(
                AgentRunEventKind::ToolRequested,
                json!({
                    "step_id": step.id,
                    "call_id": call_id,
                    "tool": tool_name,
                    "operation": step.operation,
                    "version": step.version,
                    "arguments": arguments,
                }),
            );
            if let Some(approval) = approval {
                self.push_at_or_after(
                    AgentRunEventKind::ApprovalRequired,
                    json!({
                        "step_id": step.id,
                        "request_id": approval.request.body.id,
                        "operation": approval.request.body.operation,
                        "version": approval.request.body.version,
                        "expires_at": approval.request.body.expires_at,
                    }),
                    approval.request.body.requested_at,
                );
                self.push_at_or_after(
                    AgentRunEventKind::ApprovalResumed,
                    json!({
                        "step_id": step.id,
                        "request_id": approval.request.body.id,
                        "decided_by": approval.decision.body.decided_by,
                        "outcome": approval.decision.body.outcome,
                    }),
                    approval.decision.body.decided_at,
                );
            }
            let proof_at = step.proof.as_ref().unwrap().body.timestamp;
            let succeeded_at = step
                .completed_at
                .map_or(proof_at, |completed_at| completed_at.max(proof_at));
            self.push_at_or_after(
                AgentRunEventKind::ToolSucceeded,
                json!({
                    "step_id": step.id,
                    "call_id": call_id,
                    "operation": step.operation,
                    "version": step.version,
                    "proof_id": step.proof.as_ref().unwrap().body.id,
                }),
                succeeded_at,
            );
            self.previous_response_id = Some(response_id);
            self.model_call += 1;
        }

        fn finish(mut self, output: &str) -> Vec<AgentRunEvent> {
            self.model_requested();
            self.push(
                AgentRunEventKind::ModelResponded,
                json!({
                    "response_id": "resp_finish",
                    "decision": {"type": "finish", "output": output},
                    "usage": {},
                }),
            );
            if self
                .terminal_at
                .is_some_and(|terminal_at| self.at < terminal_at)
            {
                self.at = self.terminal_at.unwrap();
            }
            self.push(
                AgentRunEventKind::Completed,
                json!({"output": output, "evaluation_id": fixture_uuid(50)}),
            );
            self.events
        }
    }

    struct ReleaseFixture {
        at: DateTime<Utc>,
        actor: Keypair,
        actor_principal: Principal,
        approver_principal: Principal,
        agent: AgentDefinition,
        run: AgentRun,
        step: AgentRunStep,
        approval: ApprovalEvidence,
        events: Vec<AgentRunEvent>,
    }

    fn release_fixture() -> ReleaseFixture {
        let at = fixture_time();
        let actor = fixture_keypair(1, PrincipalKind::Agent, at);
        let approver = fixture_keypair(2, PrincipalKind::Human, at);
        let actor_principal = principal_from_keypair(&actor);
        let approver_principal = principal_from_keypair(&approver);
        let mut agent = release_agent(at);
        agent.id = fixture_uuid(3);
        let mut run = started_run(&actor, &agent, at);
        run.id = fixture_uuid(4);
        run.wait_for_input(at + Duration::seconds(3)).unwrap();
        let arguments = release_arguments();
        let (step, approval) = approved_step(
            &run,
            &actor,
            &approver,
            0,
            &arguments,
            &release_output(),
            at + Duration::seconds(1),
        );
        run.resume(at + Duration::seconds(6)).unwrap();
        run.succeed(at + Duration::seconds(8)).unwrap();
        let mut events = EventBuilder::new(&run, &agent, at);
        events.tool_turn(&step, &arguments, Some(&approval), "release");
        let final_output = format!(
            "Release {} for edition {} published to preview as 2026.08.29-rc1 with proof {}.",
            step.output.as_ref().unwrap()["data"]["release"]["id"]
                .as_str()
                .unwrap(),
            step.output.as_ref().unwrap()["data"]["release"]["edition_id"]
                .as_str()
                .unwrap(),
            step.proof.as_ref().unwrap().body.id,
        );
        let events = events.finish(&final_output);
        ReleaseFixture {
            at,
            actor,
            actor_principal,
            approver_principal,
            agent,
            run,
            step,
            approval,
            events,
        }
    }

    fn evaluate_release(fixture: &ReleaseFixture) -> AgentRunEvaluation {
        let mut evaluation = release_policy()
            .evaluate(
                &fixture.run,
                &fixture.agent,
                &fixture.actor_principal,
                std::slice::from_ref(&fixture.approver_principal),
                std::slice::from_ref(&fixture.step),
                &fixture.events,
                std::slice::from_ref(&fixture.approval),
                "release-manager-eval/v1",
                fixture.at + Duration::seconds(9),
            )
            .unwrap();
        evaluation.id = fixture_uuid(51);
        evaluation
    }

    fn check_passed(evaluation: &AgentRunEvaluation, name: &str) -> bool {
        evaluation.metrics["checks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|check| check["name"] == name)
            .unwrap()["passed"]
            .as_bool()
            .unwrap()
    }

    fn metrics_text(evaluation: &AgentRunEvaluation) -> String {
        serde_json::to_string(&evaluation.metrics).unwrap()
    }

    #[test]
    fn no_tool_finish_fails_task_correctness() {
        let at = Utc::now();
        let actor = generate_keypair();
        let actor_principal = principal_from_keypair(&actor);
        let agent = release_agent(at);
        let mut run = started_run(&actor, &agent, at);
        run.succeed(at + Duration::seconds(2)).unwrap();
        let events = EventBuilder::new(&run, &agent, at).finish("Done.");

        let evaluation = release_policy()
            .evaluate(
                &run,
                &agent,
                &actor_principal,
                &[],
                &[],
                &events,
                &[],
                "test",
                at + Duration::seconds(3),
            )
            .unwrap();

        assert_eq!(evaluation.outcome, AgentEvaluationOutcome::Failed);
        assert!(!check_passed(&evaluation, "expected_tool_calls"));
        assert!(check_passed(&evaluation, "lifecycle_integrity"));
    }

    #[test]
    fn invalid_final_output_reference_is_rejected() {
        let mut policy = TraceEvaluationPolicy::new(
            vec![ExpectedToolCall::new(
                OPERATION,
                VERSION,
                release_arguments(),
                true,
            )],
            false,
        );
        policy.required_final_output_references = vec![FinalOutputReference {
            call_index: 1,
            source: FinalOutputSource::ProofId,
            pointer: None,
        }];

        assert!(matches!(
            DeterministicTraceEvaluator::new(policy),
            Err(TraceEvaluationError::InvalidFinalOutputReference { index: 0, .. })
        ));
    }

    #[test]
    fn policy_rejects_unknown_fields_instead_of_weakening_checks() {
        let error = serde_json::from_value::<TraceEvaluationPolicy>(json!({
            "expected_calls": [],
            "allow_additional_calls": false,
            "required_final_output_referencess": []
        }))
        .unwrap_err();

        assert!(error.to_string().contains("unknown field"));
        assert!(error
            .to_string()
            .contains("required_final_output_referencess"));
    }

    #[test]
    fn correct_approved_release_passes() {
        let fixture = release_fixture();
        let evaluation = evaluate_release(&fixture);

        assert_eq!(evaluation.outcome, AgentEvaluationOutcome::Passed);
        assert_eq!(evaluation.score_bps, Some(10_000));
        assert!(evaluation.metrics["checks"]
            .as_array()
            .unwrap()
            .iter()
            .all(|check| check["passed"] == true));
        assert_eq!(
            evaluation.metrics["binding"]["policy_digest"]
                .as_str()
                .unwrap()
                .len(),
            64
        );
        assert_eq!(
            evaluation.metrics["binding"]["trace_digest"]
                .as_str()
                .unwrap()
                .len(),
            64
        );
    }

    #[test]
    fn two_fixed_release_manager_runs_match_checked_in_policy_and_trace_digest() {
        let first = release_fixture();
        let second = release_fixture();
        let first_evaluation = evaluate_release(&first);
        let second_evaluation = evaluate_release(&second);

        for (fixture, evaluation) in [(&first, &first_evaluation), (&second, &second_evaluation)] {
            assert_eq!(evaluation.outcome, AgentEvaluationOutcome::Passed);
            assert_eq!(evaluation.score_bps, Some(10_000));
            assert_eq!(evaluation.metrics["passed_checks"], 10);
            assert_eq!(evaluation.metrics["total_checks"], 10);
            assert!(evaluation.metrics["checks"]
                .as_array()
                .unwrap()
                .iter()
                .all(|check| check["passed"] == true));
            assert!(fixture
                .events
                .iter()
                .any(|event| event.kind == AgentRunEventKind::ApprovalResumed));
            assert!(fixture.events.iter().all(|event| {
                !matches!(
                    event.kind,
                    AgentRunEventKind::ToolFailed
                        | AgentRunEventKind::Failed
                        | AgentRunEventKind::BudgetExceeded
                )
            }));
            assert!(check_passed(evaluation, "no_failure_events"));
        }

        let trace_digest = &first_evaluation.metrics["binding"]["trace_digest"];
        assert_eq!(
            trace_digest,
            &second_evaluation.metrics["binding"]["trace_digest"]
        );
        assert_eq!(
            trace_digest,
            "b14a94ed884b758f9503fdb95f85a65bd9d384d56aa13130ffe181fb69deae34"
        );
        assert_eq!(first_evaluation.id, fixture_uuid(51));
        assert_eq!(second_evaluation.id, fixture_uuid(51));
        assert_eq!(
            first_evaluation.metrics["binding"]["policy_digest"],
            second_evaluation.metrics["binding"]["policy_digest"]
        );
        assert_eq!(
            first_evaluation.metrics["binding"]["policy_digest"],
            "1e33747b44100727056c00407103deedf2b0c852349fd6489aa71d4246569f33"
        );
    }

    #[test]
    fn missing_required_final_report_values_fails() {
        let mut fixture = release_fixture();
        let terse_output = "Preview release published.";
        let responded_index = fixture.events.len() - 2;
        fixture.events[responded_index].data["decision"]["output"] = json!(terse_output);
        fixture.events[responded_index].data_digest = canonical_digest(
            ArtifactKind::AgentEvent,
            &fixture.events[responded_index].data,
        )
        .unwrap();
        let completed_index = fixture.events.len() - 1;
        fixture.events[completed_index].data["output"] = json!(terse_output);
        fixture.events[completed_index].data_digest = canonical_digest(
            ArtifactKind::AgentEvent,
            &fixture.events[completed_index].data,
        )
        .unwrap();

        let evaluation = evaluate_release(&fixture);

        assert_eq!(evaluation.outcome, AgentEvaluationOutcome::Failed);
        assert!(!check_passed(&evaluation, "final_output_references"));
        assert!(check_passed(&evaluation, "lifecycle_integrity"));
        assert!(metrics_text(&evaluation).contains("018f0000-0000-7000-8000-000000000020"));
    }

    #[test]
    fn evaluation_digests_bind_the_exact_policy_and_trace_snapshot() {
        let fixture = release_fixture();
        let baseline = evaluate_release(&fixture);

        let mut permissive_policy = release_policy().policy().clone();
        permissive_policy.allow_additional_calls = true;
        let permissive = DeterministicTraceEvaluator::new(permissive_policy)
            .unwrap()
            .evaluate(
                &fixture.run,
                &fixture.agent,
                &fixture.actor_principal,
                std::slice::from_ref(&fixture.approver_principal),
                std::slice::from_ref(&fixture.step),
                &fixture.events,
                std::slice::from_ref(&fixture.approval),
                "release-manager-eval/v1",
                fixture.at + Duration::seconds(9),
            )
            .unwrap();
        assert_ne!(
            baseline.metrics["binding"]["policy_digest"],
            permissive.metrics["binding"]["policy_digest"]
        );
        assert_eq!(
            baseline.metrics["binding"]["trace_digest"],
            permissive.metrics["binding"]["trace_digest"]
        );

        let mut later_trace = fixture.events.clone();
        later_trace.last_mut().unwrap().created_at += Duration::milliseconds(1);
        let later = release_policy()
            .evaluate(
                &fixture.run,
                &fixture.agent,
                &fixture.actor_principal,
                std::slice::from_ref(&fixture.approver_principal),
                std::slice::from_ref(&fixture.step),
                &later_trace,
                std::slice::from_ref(&fixture.approval),
                "release-manager-eval/v1",
                fixture.at + Duration::seconds(9),
            )
            .unwrap();
        assert_ne!(
            baseline.metrics["binding"]["trace_digest"],
            later.metrics["binding"]["trace_digest"]
        );
    }

    #[test]
    fn trace_digest_ignores_non_durable_principal_read_timestamps() {
        let fixture = release_fixture();
        let baseline = evaluate_release(&fixture);
        let mut reloaded_actor = fixture.actor_principal.clone();
        reloaded_actor.created_at += Duration::minutes(1);
        let mut reloaded_approver = fixture.approver_principal.clone();
        reloaded_approver.created_at += Duration::minutes(2);
        let mut reloaded_approval = fixture.approval.clone();
        reloaded_approval.approver = reloaded_approver.clone();

        let reloaded = release_policy()
            .evaluate(
                &fixture.run,
                &fixture.agent,
                &reloaded_actor,
                std::slice::from_ref(&reloaded_approver),
                std::slice::from_ref(&fixture.step),
                &fixture.events,
                std::slice::from_ref(&reloaded_approval),
                "release-manager-eval/v1",
                fixture.at + Duration::seconds(9),
            )
            .unwrap();

        assert_eq!(
            baseline.metrics["binding"]["trace_digest"],
            reloaded.metrics["binding"]["trace_digest"]
        );
    }

    #[test]
    fn trace_digest_normalizes_step_input_order() {
        let fixture = release_fixture();
        let mut second = fixture.step.clone();
        second.id = Uuid::now_v7();
        second.ordinal = 1;
        second.approval_request_id = None;
        let ordered = vec![fixture.step.clone(), second.clone()];
        let reversed = vec![second, fixture.step.clone()];
        let evaluate = |steps: &[AgentRunStep]| {
            release_policy()
                .evaluate(
                    &fixture.run,
                    &fixture.agent,
                    &fixture.actor_principal,
                    std::slice::from_ref(&fixture.approver_principal),
                    steps,
                    &fixture.events,
                    std::slice::from_ref(&fixture.approval),
                    "release-manager-eval/v1",
                    fixture.at + Duration::seconds(9),
                )
                .unwrap()
        };

        assert_eq!(
            evaluate(&ordered).metrics["binding"]["trace_digest"],
            evaluate(&reversed).metrics["binding"]["trace_digest"]
        );
    }

    #[test]
    fn approval_evidence_must_follow_the_step_and_approval_events() {
        let mut before_step = release_fixture();
        before_step.step.started_at =
            Some(before_step.approval.request.body.requested_at + Duration::milliseconds(1));
        let evaluation = evaluate_release(&before_step);
        assert!(!check_passed(&evaluation, "required_approvals"));
        assert!(metrics_text(&evaluation).contains("approval_request_precedes_step"));

        let mut impossible_events = release_fixture();
        let required = impossible_events
            .events
            .iter()
            .position(|event| event.kind == AgentRunEventKind::ApprovalRequired)
            .unwrap();
        let resumed = impossible_events
            .events
            .iter()
            .position(|event| event.kind == AgentRunEventKind::ApprovalResumed)
            .unwrap();
        impossible_events.events[required].created_at =
            impossible_events.approval.decision.body.decided_at + Duration::milliseconds(1);
        impossible_events.events[resumed].created_at =
            impossible_events.approval.execution.executed_at + Duration::milliseconds(1);
        let evaluation = evaluate_release(&impossible_events);
        assert!(!check_passed(&evaluation, "lifecycle_integrity"));
        assert!(metrics_text(&evaluation).contains("approval_decision_precedes_required_event"));
        assert!(metrics_text(&evaluation).contains("approval_execution_precedes_resumed_event"));
    }

    #[test]
    fn malformed_step_topology_and_timestamps_fail_lifecycle_integrity() {
        let mut fixture = release_fixture();
        fixture.step.ordinal = 99;
        fixture.step.attempt = 7;
        fixture.step.retry_of = Some(Uuid::now_v7());
        fixture.step.created_at = fixture.run.completed_at.unwrap() + Duration::seconds(1);

        let evaluation = evaluate_release(&fixture);

        assert!(!check_passed(&evaluation, "lifecycle_integrity"));
        let metrics = metrics_text(&evaluation);
        assert!(metrics.contains("step_ordinal_not_contiguous"));
        assert!(metrics.contains("step_attempt_not_contiguous"));
        assert!(metrics.contains("first_step_attempt_has_retry_parent"));
        assert!(metrics.contains("step_timestamp_outside_run"));
    }

    #[test]
    fn missing_approval_evidence_fails() {
        let fixture = release_fixture();
        let evaluation = release_policy()
            .evaluate(
                &fixture.run,
                &fixture.agent,
                &fixture.actor_principal,
                std::slice::from_ref(&fixture.approver_principal),
                std::slice::from_ref(&fixture.step),
                &fixture.events,
                &[],
                "test",
                fixture.at + Duration::seconds(9),
            )
            .unwrap();

        assert_eq!(evaluation.outcome, AgentEvaluationOutcome::Failed);
        assert!(!check_passed(&evaluation, "required_approvals"));
        assert!(metrics_text(&evaluation).contains("missing_approval_evidence"));
    }

    #[test]
    fn unexpected_tools_fail_when_additional_calls_are_disallowed() {
        let at = Utc::now();
        let actor = generate_keypair();
        let approver = generate_keypair_for(PrincipalKind::Human);
        let actor_principal = principal_from_keypair(&actor);
        let approver_principal = principal_from_keypair(&approver);
        let agent = agent(
            at,
            vec![
                AgentTool::new(OPERATION, VERSION).unwrap(),
                AgentTool::new("audit.record", VERSION).unwrap(),
            ],
        );
        let mut run = started_run(&actor, &agent, at);
        run.wait_for_input(at + Duration::seconds(3)).unwrap();
        let arguments = release_arguments();
        let (release_step, approval) = approved_step(
            &run,
            &actor,
            &approver,
            0,
            &arguments,
            &release_output(),
            at + Duration::seconds(1),
        );
        run.resume(at + Duration::seconds(6)).unwrap();
        let audit_input = json!({"release_id": "rel_preview_1"});
        let audit_output = json!({"recorded": true});
        let audit_step = direct_step(
            &run,
            &actor,
            1,
            "audit.record",
            VERSION,
            &audit_input,
            &audit_output,
            at + Duration::seconds(7),
            at + Duration::seconds(8),
            at + Duration::seconds(9),
        );
        run.succeed(at + Duration::seconds(10)).unwrap();
        let mut events = EventBuilder::new(&run, &agent, at);
        events.tool_turn(&release_step, &arguments, Some(&approval), "release");
        events.tool_turn(&audit_step, &audit_input, None, "audit");
        let events = events.finish("Published and audited.");

        let evaluation = release_policy()
            .evaluate(
                &run,
                &agent,
                &actor_principal,
                &[approver_principal],
                &[release_step, audit_step],
                &events,
                &[approval],
                "test",
                at + Duration::seconds(11),
            )
            .unwrap();

        assert_eq!(evaluation.outcome, AgentEvaluationOutcome::Failed);
        assert!(!check_passed(&evaluation, "expected_tool_calls"));
        assert!(check_passed(&evaluation, "calls_allowlisted"));
    }

    #[test]
    fn forged_approval_signature_fails() {
        let mut fixture = release_fixture();
        fixture.approval.decision.signature[0] ^= 0xff;

        let evaluation = evaluate_release(&fixture);

        assert!(!check_passed(&evaluation, "required_approvals"));
        assert!(metrics_text(&evaluation).contains("invalid_decision_signature"));
    }

    #[test]
    fn proof_from_wrong_signer_fails() {
        let mut fixture = release_fixture();
        let wrong_actor = generate_keypair();
        let proof_at = fixture.step.proof.as_ref().unwrap().body.timestamp;
        let wrong_proof = create_proof(
            wrong_actor.principal_id,
            None,
            &proof_operation(OPERATION, VERSION),
            &release_arguments(),
            &release_output(),
            proof_at,
            &wrong_actor,
        )
        .unwrap();
        fixture.step.proof = Some(wrong_proof.clone());
        fixture.approval.execution.proof = wrong_proof;

        let evaluation = evaluate_release(&fixture);

        assert!(!check_passed(
            &evaluation,
            "successful_steps_with_valid_proofs"
        ));
        assert!(metrics_text(&evaluation).contains("proof_actor_mismatch"));
    }

    #[test]
    fn proof_from_wrong_operation_version_fails() {
        let mut fixture = release_fixture();
        let proof_at = fixture.step.proof.as_ref().unwrap().body.timestamp;
        let wrong_proof = create_proof(
            fixture.actor.principal_id,
            None,
            &proof_operation(OPERATION, "v2"),
            &release_arguments(),
            &release_output(),
            proof_at,
            &fixture.actor,
        )
        .unwrap();
        fixture.step.proof = Some(wrong_proof.clone());
        fixture.approval.execution.proof = wrong_proof;

        let evaluation = evaluate_release(&fixture);

        assert!(!check_passed(
            &evaluation,
            "successful_steps_with_valid_proofs"
        ));
        assert!(metrics_text(&evaluation).contains("proof_operation_mismatch"));
        assert!(metrics_text(&evaluation).contains("execution_proof_binding_mismatch"));
    }

    #[test]
    fn invalid_proof_signature_fails() {
        let mut fixture = release_fixture();
        let wrong_actor = generate_keypair();
        let wrong_proof = create_proof(
            wrong_actor.principal_id,
            None,
            &proof_operation(OPERATION, VERSION),
            &release_arguments(),
            &release_output(),
            fixture.step.proof.as_ref().unwrap().body.timestamp,
            &wrong_actor,
        )
        .unwrap();
        fixture.step.proof.as_mut().unwrap().signature = wrong_proof.signature.clone();
        fixture.approval.execution.proof = fixture.step.proof.clone().unwrap();

        let evaluation = evaluate_release(&fixture);

        assert!(metrics_text(&evaluation).contains("invalid_proof_signature"));
        assert!(!check_passed(
            &evaluation,
            "successful_steps_with_valid_proofs"
        ));
    }

    #[test]
    fn valid_old_proof_outside_step_window_fails() {
        let at = Utc::now();
        let actor = generate_keypair();
        let actor_principal = principal_from_keypair(&actor);
        let agent = release_agent(at);
        let mut run = started_run(&actor, &agent, at);
        let arguments = release_arguments();
        let output = release_output();
        let step = direct_step(
            &run,
            &actor,
            0,
            OPERATION,
            VERSION,
            &arguments,
            &output,
            at + Duration::seconds(3),
            at - Duration::days(1),
            at + Duration::seconds(4),
        );
        run.succeed(at + Duration::seconds(5)).unwrap();
        let mut events = EventBuilder::new(&run, &agent, at);
        events.tool_turn(&step, &arguments, None, "release");
        let events = events.finish("Published.");
        let evaluator = policy(vec![ExpectedToolCall::new(
            OPERATION, VERSION, arguments, false,
        )]);

        let evaluation = evaluator
            .evaluate(
                &run,
                &agent,
                &actor_principal,
                &[],
                &[step],
                &events,
                &[],
                "test",
                at + Duration::seconds(6),
            )
            .unwrap();

        assert!(metrics_text(&evaluation).contains("proof_timestamp_outside_step_window"));
    }

    #[test]
    fn missing_reordered_and_tampered_events_fail_lifecycle_integrity() {
        let fixture = release_fixture();
        let mut missing = fixture.events.clone();
        missing.remove(5);
        let mut reordered = fixture.events.clone();
        reordered.swap(3, 4);
        let mut tampered = fixture.events.clone();
        tampered[3].data["arguments"]["environment"] = json!("production");

        for events in [missing, reordered, tampered] {
            let evaluation = release_policy()
                .evaluate(
                    &fixture.run,
                    &fixture.agent,
                    &fixture.actor_principal,
                    std::slice::from_ref(&fixture.approver_principal),
                    std::slice::from_ref(&fixture.step),
                    &events,
                    std::slice::from_ref(&fixture.approval),
                    "test",
                    fixture.at + Duration::seconds(9),
                )
                .unwrap();
            assert!(!check_passed(&evaluation, "lifecycle_integrity"));
        }
    }

    #[test]
    fn temporally_impossible_events_fail_lifecycle_integrity() {
        let fixture = release_fixture();

        let mut nonmonotonic = fixture.events.clone();
        nonmonotonic[8].created_at = nonmonotonic[7].created_at - Duration::milliseconds(1);
        let nonmonotonic = release_policy()
            .evaluate(
                &fixture.run,
                &fixture.agent,
                &fixture.actor_principal,
                std::slice::from_ref(&fixture.approver_principal),
                std::slice::from_ref(&fixture.step),
                &nonmonotonic,
                std::slice::from_ref(&fixture.approval),
                "test",
                fixture.at + Duration::seconds(9),
            )
            .unwrap();
        assert!(metrics_text(&nonmonotonic).contains("event_timestamp_not_monotonic"));

        let mut premature_resume = fixture.events.clone();
        premature_resume[5].created_at =
            fixture.approval.decision.body.decided_at - Duration::milliseconds(1);
        let premature_resume = release_policy()
            .evaluate(
                &fixture.run,
                &fixture.agent,
                &fixture.actor_principal,
                std::slice::from_ref(&fixture.approver_principal),
                std::slice::from_ref(&fixture.step),
                &premature_resume,
                std::slice::from_ref(&fixture.approval),
                "test",
                fixture.at + Duration::seconds(9),
            )
            .unwrap();
        assert!(
            metrics_text(&premature_resume).contains("approval_resumed_timestamp_out_of_window")
        );

        let mut premature_success = fixture.events.clone();
        premature_success[6].created_at =
            fixture.approval.execution.executed_at - Duration::milliseconds(1);
        let premature_success = release_policy()
            .evaluate(
                &fixture.run,
                &fixture.agent,
                &fixture.actor_principal,
                std::slice::from_ref(&fixture.approver_principal),
                std::slice::from_ref(&fixture.step),
                &premature_success,
                std::slice::from_ref(&fixture.approval),
                "test",
                fixture.at + Duration::seconds(9),
            )
            .unwrap();
        assert!(metrics_text(&premature_success).contains("tool_succeeded_timestamp_precedes_step"));
    }

    #[test]
    fn tool_failed_event_is_rejected() {
        let fixture = release_fixture();
        let mut events = fixture.events.clone();
        events[6] = AgentRunEvent::create(
            fixture.run.id,
            6,
            AgentRunEventKind::ToolFailed,
            json!({"step_id": fixture.step.id, "error": "provider timeout"}),
            fixture.at + Duration::milliseconds(6),
        )
        .unwrap();

        let evaluation = release_policy()
            .evaluate(
                &fixture.run,
                &fixture.agent,
                &fixture.actor_principal,
                std::slice::from_ref(&fixture.approver_principal),
                std::slice::from_ref(&fixture.step),
                &events,
                std::slice::from_ref(&fixture.approval),
                "test",
                fixture.at + Duration::seconds(9),
            )
            .unwrap();

        assert!(!check_passed(&evaluation, "lifecycle_integrity"));
        assert!(!check_passed(&evaluation, "no_failure_events"));
    }

    #[test]
    fn approval_request_cannot_be_reused_across_steps() {
        let at = Utc::now();
        let actor = generate_keypair();
        let approver = generate_keypair_for(PrincipalKind::Human);
        let actor_principal = principal_from_keypair(&actor);
        let approver_principal = principal_from_keypair(&approver);
        let agent = release_agent(at);
        let mut run = started_run(&actor, &agent, at);
        run.wait_for_input(at + Duration::seconds(3)).unwrap();
        let arguments = release_arguments();
        let output = release_output();
        let (first, approval) = approved_step(
            &run,
            &actor,
            &approver,
            0,
            &arguments,
            &output,
            at + Duration::seconds(1),
        );
        run.resume(at + Duration::seconds(7)).unwrap();
        let mut second = AgentRunStep::new(
            run.id,
            1,
            OPERATION,
            VERSION,
            &arguments,
            at + Duration::seconds(8),
        )
        .unwrap();
        second.start(at + Duration::seconds(8)).unwrap();
        second
            .wait_for_approval(approval.request.body.id, at + Duration::seconds(9))
            .unwrap();
        second
            .resume_from_approval(at + Duration::seconds(10))
            .unwrap();
        let second_proof = create_proof(
            actor.principal_id,
            None,
            &proof_operation(OPERATION, VERSION),
            &arguments,
            &output,
            at + Duration::seconds(11),
            &actor,
        )
        .unwrap();
        second
            .succeed(output, second_proof, at + Duration::seconds(12))
            .unwrap();
        run.succeed(at + Duration::seconds(13)).unwrap();
        let mut events = EventBuilder::new(&run, &agent, at);
        events.tool_turn(&first, &arguments, Some(&approval), "first");
        events.tool_turn(&second, &arguments, Some(&approval), "second");
        let events = events.finish("Published twice.");
        let evaluator = policy(vec![
            ExpectedToolCall::new(OPERATION, VERSION, arguments.clone(), true),
            ExpectedToolCall::new(OPERATION, VERSION, arguments, true),
        ]);

        let evaluation = evaluator
            .evaluate(
                &run,
                &agent,
                &actor_principal,
                &[approver_principal],
                &[first, second],
                &events,
                &[approval],
                "test",
                at + Duration::seconds(14),
            )
            .unwrap();

        assert!(!check_passed(&evaluation, "required_approvals"));
        assert!(metrics_text(&evaluation).contains("approval_request_reused"));
    }
}
