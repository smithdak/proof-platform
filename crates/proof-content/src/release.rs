use std::collections::BTreeMap;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use proof_kernel::{
    create_proof, generate_keypair, ExecutionContext, ExecutionEngine, ExecutionError, Keypair,
    Proof,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::digest::canonical_digest;
use crate::object::{Object, ObjectStatus};
use crate::principal::PrincipalId;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Release {
    pub id: Uuid,
    pub edition_id: Uuid,
    pub environment: String,
    pub published_at: DateTime<Utc>,
    pub published_by: PrincipalId,
}

impl Release {
    pub fn new(
        edition_id: Uuid,
        environment: impl Into<String>,
        published_by: PrincipalId,
    ) -> Self {
        Self {
            id: Uuid::now_v7(),
            edition_id,
            environment: environment.into(),
            published_at: Utc::now(),
            published_by,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContentChangeKind {
    Create,
    Edit,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContentChange {
    pub kind: ContentChangeKind,
    pub locale: String,
    pub schema: Value,
    pub content: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub object_id: Option<Uuid>,
}

impl ContentChange {
    pub fn create(schema: Value, locale: impl Into<String>, content: Value) -> Self {
        Self {
            kind: ContentChangeKind::Create,
            locale: locale.into(),
            schema,
            content,
            object_id: None,
        }
    }

    pub fn edit(schema: Value, object_id: Uuid, locale: impl Into<String>, content: Value) -> Self {
        Self {
            kind: ContentChangeKind::Edit,
            locale: locale.into(),
            schema,
            content,
            object_id: Some(object_id),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReleaseManifestEntry {
    pub object_id: Uuid,
    pub operation: String,
    pub locale: String,
    pub content_digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReleaseManifest {
    pub release_id: Uuid,
    pub edition_id: Uuid,
    pub environment: String,
    pub created_at: DateTime<Utc>,
    pub release_digest: String,
    pub entries: Vec<ReleaseManifestEntry>,
}

impl ReleaseManifest {
    fn from_results(release: &Release, mut objects: Vec<Object>) -> Self {
        objects.sort_by(|left, right| left.id.cmp(&right.id));
        let entries = objects
            .iter()
            .map(|object| ReleaseManifestEntry {
                object_id: object.id,
                operation: match object.status() {
                    ObjectStatus::Draft => "object.create".to_string(),
                    _ => "object.edit".to_string(),
                },
                locale: object.locale.clone(),
                content_digest: canonical_digest(object),
            })
            .collect();
        Self {
            release_id: release.id,
            edition_id: release.edition_id,
            environment: release.environment.clone(),
            created_at: release.published_at,
            release_digest: canonical_digest(&entries),
            entries,
        }
    }
}

pub struct ReleasePipeline<'a> {
    engine: &'a ExecutionEngine,
    keypair: Arc<Keypair>,
    change_proofs: std::sync::Mutex<Vec<Proof>>,
}
impl std::fmt::Debug for ReleasePipeline<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ReleasePipeline")
            .finish_non_exhaustive()
    }
}

#[derive(Debug)]
pub struct ReleasePipelineOutput {
    pub release: Release,
    pub manifest: ReleaseManifest,
    pub objects: Vec<Object>,
    pub release_proof: Proof,
    pub change_proofs: Vec<Proof>,
}

impl<'a> ReleasePipeline<'a> {
    pub fn new(engine: &'a ExecutionEngine) -> Self {
        Self {
            engine,
            keypair: Arc::new(generate_keypair()),
            change_proofs: std::sync::Mutex::new(Vec::new()),
        }
    }

    pub fn new_with_keypair(engine: &'a ExecutionEngine, keypair: Keypair) -> Self {
        Self {
            engine,
            keypair: Arc::new(keypair),
            change_proofs: std::sync::Mutex::new(Vec::new()),
        }
    }

    pub fn publish(
        &self,
        environment: impl Into<String>,
        changes: Vec<ContentChange>,
        context: &ExecutionContext,
    ) -> Result<ReleasePipelineOutput, ExecutionError> {
        let mut objects: BTreeMap<Uuid, Object> = BTreeMap::new();

        for change in changes {
            let object = self.apply_change(&mut objects, change, context)?;
            objects.insert(object.id, object);
        }

        let version_label = Utc::now().timestamp().to_string();
        let release_input = Value::Object(
            [
                ("environment".to_string(), Value::String(environment.into())),
                (
                    "version_label".to_string(),
                    Value::String(version_label.clone()),
                ),
            ]
            .into_iter()
            .collect(),
        );
        let governed_release_input = serde_json::json!({
            "environment": release_input["environment"],
            "version_label": version_label,
        });
        self.engine
            .execute("release.publish", "v1", &governed_release_input, context)?;

        let release = Release::new(
            Uuid::now_v7(),
            release_input["environment"].as_str().unwrap().to_string(),
            PrincipalId(context.actor.as_uuid()),
        );
        let objects: Vec<Object> = objects.into_values().collect();
        let manifest = ReleaseManifest::from_results(&release, objects.clone());
        let release_proof = create_proof(
            context.actor,
            context.delegation_id,
            "release.publish",
            &release_input,
            &serde_json::to_value(&manifest)
                .map_err(|error| ExecutionError::EvidenceFailed(error.to_string()))?,
            context.timestamp,
            &self.keypair,
        )
        .map_err(|error| ExecutionError::EvidenceFailed(error.to_string()))?;

        Ok(ReleasePipelineOutput {
            release,
            manifest,
            objects,
            release_proof,
            change_proofs: std::mem::take(&mut *self.change_proofs.lock().unwrap()),
        })
    }

    fn apply_change(
        &self,
        objects: &mut BTreeMap<Uuid, Object>,
        change: ContentChange,
        context: &ExecutionContext,
    ) -> Result<Object, ExecutionError> {
        let (operation, input) = match change.kind {
            ContentChangeKind::Create => {
                let input = serde_json::json!({
                    "schema": change.schema,
                    "locale": change.locale,
                    "content": change.content,
                });
                ("object.create", input)
            }
            ContentChangeKind::Edit => {
                let object_id = change.object_id.ok_or_else(|| {
                    ExecutionError::HandlerFailed("edit change requires object_id".to_string())
                })?;
                let object = objects.get(&object_id).ok_or_else(|| {
                    ExecutionError::HandlerFailed(format!(
                        "edit refers to unknown object {object_id}"
                    ))
                })?;
                let object = object.clone();
                let input = serde_json::json!({
                    "object_id": object_id,
                    "schema": change.schema,
                    "object": object,
                    "content": change.content,
                });
                ("object.edit", input)
            }
        };

        let result = self.engine.execute(operation, "v1", &input, context)?;
        let proof = create_proof(
            context.actor,
            context.delegation_id,
            operation,
            &input,
            &result,
            context.timestamp,
            &self.keypair,
        )
        .map_err(|error| ExecutionError::EvidenceFailed(error.to_string()))?;
        self.change_proofs.lock().unwrap().push(proof);
        let object = serde_json::from_value::<Object>(result["data"]["object"].clone()).map_err(
            |error| {
                ExecutionError::HandlerFailed(format!("handler returned invalid object: {error}"))
            },
        )?;
        Ok(object)
    }
}

pub fn verify_release(
    manifest: &ReleaseManifest,
    objects: &[Object],
) -> Result<(), ExecutionError> {
    if manifest.entries.len() != objects.len() {
        return Err(ExecutionError::HandlerFailed(
            "manifest and object count differ".to_string(),
        ));
    }
    if manifest.entries.len()
        != objects
            .iter()
            .map(|object| object.id)
            .collect::<std::collections::BTreeSet<_>>()
            .len()
    {
        return Err(ExecutionError::HandlerFailed(
            "manifest and object ids differ".to_string(),
        ));
    }
    for entry in &manifest.entries {
        let object = objects
            .iter()
            .find(|object| object.id == entry.object_id)
            .ok_or_else(|| {
                ExecutionError::HandlerFailed(format!(
                    "manifest object {} is missing",
                    entry.object_id
                ))
            })?;
        if canonical_digest(object) != entry.content_digest {
            return Err(ExecutionError::HandlerFailed(format!(
                "content digest mismatch for object {}",
                object.id
            )));
        }
    }
    Ok(())
}
