//! Durable, governed agent execution over Proof operations.

mod model;
mod openai;
mod operator;
mod runtime;
mod trace_eval;

pub use model::{
    AgentFunctionTool, ModelDecision, ModelGateway, ModelGatewayError, ModelInput, ModelTurn,
    ModelTurnRequest, ModelUsage,
};
pub use openai::{OpenAiResponsesGateway, DEFAULT_OPENAI_BASE_URL};
pub use operator::{
    GovernedRuntimeCall, GovernedRuntimeOutcome, OperatorLease, OperatorLeaseClaim,
    OperatorLeaseReclaim, OperatorReclaimOutcome, OperatorRuntime, OperatorRuntimeError,
    OperatorRuntimeStage,
};
pub use runtime::{
    runtime_approval_context, runtime_state_view, AgentRuntime, AgentRuntimeError,
    AgentRuntimeOutcome, AgentRuntimeState, LiveAuthoritySetup, LiveBindingInputs,
    LivePolicyMaterial, LiveRunIntent, LiveRunSetup, ModelGatewayFactory,
    ModelGatewayFactoryContext, ModelGatewayFactoryError, PendingToolCall, RuntimeApprovalContext,
    RuntimeStateView,
};
pub use trace_eval::{
    ApprovalEvidence, DeterministicTraceEvaluator, ExpectedToolCall, FinalOutputReference,
    FinalOutputSource, TraceEvaluationError, TraceEvaluationPolicy,
};
