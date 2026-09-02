//! SQLite storage adapter (modular: see sqlite/ directory).

pub mod agent;
pub mod agent_run;
#[cfg(test)]
pub mod agent_run_tests;
#[cfg(test)]
pub mod agent_tests;
pub mod analytics;
#[cfg(test)]
pub mod analytics_tests;
pub mod approval;
#[cfg(test)]
pub mod approval_tests;
pub mod commerce;
#[cfg(test)]
pub mod commerce_tests;
mod delegation;
#[cfg(test)]
mod delegation_tests;
mod methods;
mod migrations;
mod operator_lifecycle;
mod operator_store;
mod replay;
#[cfg(test)]
mod replay_tests;
mod store;
#[cfg(test)]
pub mod tests;
mod trusted_open;
#[cfg(test)]
mod trusted_open_tests;
pub mod workflow;
#[cfg(test)]
pub mod workflow_tests;

pub use analytics::{AnalyticsInsight, AnalyticsInsightStatus, AnalyticsQuery, AnalyticsSnapshot};
pub use commerce::{Catalog, CatalogProduct, Order, OrderLine, OrderStatus};
pub use migrations::{rollback_to, run_migrations, schema_version, Migration, MIGRATIONS};
pub use operator_lifecycle::{
    acquire_operator_workspace_lock, initialize_operator_workspace_guarded,
    open_operator_schema14_existing, release_operator_workspace_lock,
    upgrade_operator_schema14_offline, OperatorLockMode, OwnedOperatorWorkspaceLock,
};
pub use store::{ProofFilter, SqliteStore};
pub use workflow::{
    WorkflowDefinition, WorkflowRun, WorkflowRunStatus, WorkflowStep, WorkflowStepKind,
    WorkflowStepStatus, WorkflowStepTemplate,
};
