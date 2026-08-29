use std::sync::Arc;

use proof_kernel::{
    create_proof, generate_keypair, ExecutionContext, ExecutionEngine, ExecutionError, Keypair,
    Proof,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::digest::canonical_digest;
use crate::models::{Order, OrderLine, OrderStatus};

const ORDER_CREATE: &str = "order.create";
const ORDER_APPROVE: &str = "order.approve";
const ORDER_FULFILL: &str = "order.fulfill";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrderEvidence {
    pub operation: String,
    pub order_id: Uuid,
    pub status: OrderStatus,
    pub content_digest: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub record_digest: Option<String>,
    pub proof: Proof,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FulfillmentManifest {
    pub order_id: Uuid,
    pub fulfillment_digest: String,
    pub evidence: Vec<OrderEvidence>,
}

#[derive(Debug)]
pub struct FulfillmentPipelineOutput {
    pub order: Order,
    pub manifest: FulfillmentManifest,
    pub evidence_proofs: Vec<Proof>,
}

pub struct FulfillmentPipeline<'a> {
    engine: &'a ExecutionEngine,
    keypair: Arc<Keypair>,
}

impl std::fmt::Debug for FulfillmentPipeline<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("FulfillmentPipeline")
            .finish_non_exhaustive()
    }
}

impl<'a> FulfillmentPipeline<'a> {
    pub fn new(engine: &'a ExecutionEngine) -> Self {
        Self {
            engine,
            keypair: Arc::new(generate_keypair()),
        }
    }

    pub fn new_with_keypair(engine: &'a ExecutionEngine, keypair: Keypair) -> Self {
        Self {
            engine,
            keypair: Arc::new(keypair),
        }
    }

    pub fn fulfill(
        &self,
        lines: Vec<OrderLine>,
        context: &ExecutionContext,
    ) -> Result<FulfillmentPipelineOutput, ExecutionError> {
        let mut evidence = Vec::new();
        let mut evidence_proofs = Vec::new();
        let created = self.execute_step(ORDER_CREATE, &create_order_input(&lines), context)?;
        let order_id =
            Uuid::parse_str(created["data"]["order_id"].as_str().ok_or_else(|| {
                ExecutionError::HandlerFailed("handler returned no order_id".into())
            })?)
            .map_err(|error| ExecutionError::HandlerFailed(format!("invalid order_id: {error}")))?;
        let mut order = load_order(context, order_id)?;

        let approval_input = serde_json::json!({ "order_id": order.id });
        self.execute_step(ORDER_APPROVE, &approval_input, context)?;
        order
            .transition_to(OrderStatus::Approved)
            .map_err(|error| {
                ExecutionError::HandlerFailed(format!("approval produced invalid order: {error}"))
            })?;

        let fulfillment_input = serde_json::json!({ "order_id": order.id });
        self.execute_step(ORDER_FULFILL, &fulfillment_input, context)?;
        order
            .transition_to(OrderStatus::Fulfilled)
            .map_err(|error| {
                ExecutionError::HandlerFailed(format!(
                    "fulfillment produced invalid order: {error}"
                ))
            })?;

        let order_create_input = create_order_input(&lines);
        evidence.push(self.evidence(ORDER_CREATE, &order, &order_create_input, context)?);
        evidence.push(self.evidence(ORDER_APPROVE, &order, &approval_input, context)?);
        evidence.push(self.evidence(ORDER_FULFILL, &order, &fulfillment_input, context)?);

        let manifest = FulfillmentManifest {
            order_id: order.id,
            fulfillment_digest: canonical_digest(&evidence),
            evidence: evidence.clone(),
        };
        for item in &evidence {
            evidence_proofs.push(item.proof.clone());
        }

        Ok(FulfillmentPipelineOutput {
            order,
            manifest,
            evidence_proofs,
        })
    }

    fn execute_step(
        &self,
        operation: &'static str,
        input: &Value,
        context: &ExecutionContext,
    ) -> Result<Value, ExecutionError> {
        self.engine.execute(operation, "v1", input, context)
    }

    fn evidence(
        &self,
        operation: &'static str,
        order: &Order,
        input: &Value,
        context: &ExecutionContext,
    ) -> Result<OrderEvidence, ExecutionError> {
        let proof = create_proof(
            context.actor,
            context.delegation_id,
            operation,
            input,
            &serde_json::to_value(order)
                .map_err(|error| ExecutionError::EvidenceFailed(error.to_string()))?,
            context.timestamp,
            &self.keypair,
        )
        .map_err(|error| ExecutionError::EvidenceFailed(error.to_string()))?;
        Ok(OrderEvidence {
            operation: operation.to_string(),
            order_id: order.id,
            status: order.status,
            content_digest: canonical_digest(order),
            record_digest: Some(canonical_digest(order)),
            proof,
        })
    }
}

fn load_order(context: &ExecutionContext, order_id: Uuid) -> Result<Order, ExecutionError> {
    let path = context
        .workspace_path
        .join(".proof/data/commerce/orders")
        .join(format!("{order_id}.json"));
    let contents = std::fs::read_to_string(path).map_err(|error| {
        ExecutionError::HandlerFailed(format!("failed to load order {order_id}: {error}"))
    })?;
    serde_json::from_str::<Order>(&contents).map_err(|error| {
        ExecutionError::HandlerFailed(format!("invalid stored order {order_id}: {error}"))
    })
}

fn create_order_input(lines: &[OrderLine]) -> Value {
    serde_json::json!({ "lines": lines })
}

pub fn verify_fulfillment(
    manifest: &FulfillmentManifest,
    order: &Order,
) -> Result<(), ExecutionError> {
    if manifest.order_id != order.id {
        return Err(ExecutionError::HandlerFailed(
            "manifest and order ids differ".to_string(),
        ));
    }
    if manifest.evidence.len() != 3 {
        return Err(ExecutionError::HandlerFailed(
            "manifest must contain create, approve, and fulfill evidence".to_string(),
        ));
    }
    let expected_operations = [ORDER_CREATE, ORDER_APPROVE, ORDER_FULFILL];
    for (expected, evidence) in expected_operations.iter().zip(&manifest.evidence) {
        if evidence.operation != *expected || evidence.order_id != order.id {
            return Err(ExecutionError::HandlerFailed(format!(
                "invalid evidence for {expected}"
            )));
        }
        if evidence.content_digest != canonical_digest(order) {
            return Err(ExecutionError::HandlerFailed(format!(
                "content digest mismatch for {expected}"
            )));
        }
    }
    Ok(())
}
