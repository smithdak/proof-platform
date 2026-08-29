use super::store::SqliteStore;
use super::workflow::{
    WorkflowDefinition, WorkflowRun, WorkflowRunStatus, WorkflowStep, WorkflowStepKind,
    WorkflowStepStatus, WorkflowStepTemplate,
};
use crate::StorageError;
use chrono::Utc;
use uuid::Uuid;

fn test_definition() -> WorkflowDefinition {
    WorkflowDefinition {
        id: Uuid::now_v7(),
        name: "Review and approve".to_string(),
        description: "A governed workflow".to_string(),
        steps: vec![
            WorkflowStepTemplate {
                name: "Prepare draft".to_string(),
                kind: WorkflowStepKind::Agent,
                description: "Agent prepares the draft".to_string(),
            },
            WorkflowStepTemplate {
                name: "Approve".to_string(),
                kind: WorkflowStepKind::Human,
                description: "Human approves the result".to_string(),
            },
        ],
        created_at: Utc::now(),
        updated_at: Utc::now(),
    }
}

fn test_run(definition: &WorkflowDefinition) -> WorkflowRun {
    WorkflowRun {
        id: Uuid::now_v7(),
        workflow_definition_id: definition.id,
        status: WorkflowRunStatus::Pending,
        created_at: Utc::now(),
        completed_at: None,
        approved_at: None,
    }
}

fn test_steps(run: &WorkflowRun) -> Vec<WorkflowStep> {
    vec![
        WorkflowStep {
            id: Uuid::now_v7(),
            run_id: run.id,
            name: "Prepare draft".to_string(),
            kind: WorkflowStepKind::Agent,
            description: "Agent prepares the draft".to_string(),
            status: WorkflowStepStatus::Pending,
            ordinal: 0,
            completed_at: None,
        },
        WorkflowStep {
            id: Uuid::now_v7(),
            run_id: run.id,
            name: "Approve".to_string(),
            kind: WorkflowStepKind::Human,
            description: "Human approves the result".to_string(),
            status: WorkflowStepStatus::Completed,
            ordinal: 1,
            completed_at: Some(Utc::now()),
        },
    ]
}

#[test]
fn workflow_definition_round_trips() {
    let store = SqliteStore::in_memory().unwrap();
    let definition = test_definition();

    store.save_workflow_definition(&definition).unwrap();
    let loaded = store.load_workflow_definition(&definition.id).unwrap();

    assert_eq!(loaded, definition);
}

#[test]
fn workflow_definition_update_round_trips() {
    let store = SqliteStore::in_memory().unwrap();
    let mut definition = test_definition();
    store.save_workflow_definition(&definition).unwrap();

    definition.name = "Updated workflow".to_string();
    definition.description = "Updated description".to_string();
    definition.steps[0].name = "Updated step".to_string();
    definition.updated_at = Utc::now();
    store.save_workflow_definition(&definition).unwrap();

    assert_eq!(
        store.load_workflow_definition(&definition.id).unwrap(),
        definition
    );
}

#[test]
fn workflow_definition_rejects_empty_steps() {
    let store = SqliteStore::in_memory().unwrap();
    let mut definition = test_definition();
    definition.steps.clear();

    assert!(matches!(
        store.save_workflow_definition(&definition),
        Err(StorageError::Conflict(_))
    ));
}

#[test]
fn workflow_definition_list_and_delete_round_trip() {
    let store = SqliteStore::in_memory().unwrap();
    let mut first = test_definition();
    let mut second = test_definition();
    second.created_at = first.created_at + chrono::Duration::seconds(1);
    second.updated_at = second.created_at;
    let run = test_run(&first);
    let steps = test_steps(&run);
    store.save_workflow_definition(&first).unwrap();
    store.save_workflow_definition(&second).unwrap();
    store.save_workflow_run(&run).unwrap();
    for step in &steps {
        store.save_workflow_step(step).unwrap();
    }

    let definitions = store.list_workflow_definitions().unwrap();
    assert_eq!(definitions, vec![first.clone(), second.clone()]);

    assert!(store.delete_workflow_definition(&first.id).unwrap());
    assert!(matches!(
        store.load_workflow_definition(&first.id),
        Err(StorageError::NotFound(_))
    ));
    assert!(matches!(
        store.load_workflow_run(&run.id),
        Err(StorageError::NotFound(_))
    ));
    for step in &steps {
        assert!(matches!(
            store.load_workflow_step(&step.id),
            Err(StorageError::NotFound(_))
        ));
    }
    assert_eq!(store.list_workflow_definitions().unwrap(), vec![second]);
}

#[test]
fn workflow_run_round_trips() {
    let store = SqliteStore::in_memory().unwrap();
    let definition = test_definition();
    let run = test_run(&definition);
    store.save_workflow_definition(&definition).unwrap();

    store.save_workflow_run(&run).unwrap();
    let loaded = store.load_workflow_run(&run.id).unwrap();

    assert_eq!(loaded, run);
}

#[test]
fn workflow_run_status_transitions_round_trip() {
    let store = SqliteStore::in_memory().unwrap();
    let definition = test_definition();
    let mut run = test_run(&definition);
    store.save_workflow_definition(&definition).unwrap();
    store.save_workflow_run(&run).unwrap();

    run.status = WorkflowRunStatus::InProgress;
    run.completed_at = Some(run.created_at + chrono::Duration::seconds(1));
    store.save_workflow_run(&run).unwrap();
    assert_eq!(
        store.load_workflow_run(&run.id).unwrap().status,
        WorkflowRunStatus::InProgress
    );

    run.status = WorkflowRunStatus::Approved;
    run.approved_at = Some(run.created_at + chrono::Duration::seconds(2));
    store.save_workflow_run(&run).unwrap();

    assert_eq!(store.load_workflow_run(&run.id).unwrap(), run);
}

#[test]
fn workflow_run_list_filters_and_deletes() {
    let store = SqliteStore::in_memory().unwrap();
    let definition = test_definition();
    let other_definition = test_definition();
    let mut first = test_run(&definition);
    let mut second = test_run(&definition);
    let mut other = test_run(&other_definition);
    first.created_at += chrono::Duration::seconds(1);
    second.created_at += chrono::Duration::seconds(2);
    other.created_at += chrono::Duration::seconds(3);
    store.save_workflow_definition(&definition).unwrap();
    store.save_workflow_definition(&other_definition).unwrap();
    store.save_workflow_run(&first).unwrap();
    store.save_workflow_run(&second).unwrap();
    store.save_workflow_run(&other).unwrap();

    assert_eq!(
        store.list_workflow_runs(Some(&definition.id)).unwrap(),
        vec![first.clone(), second.clone()]
    );
    assert_eq!(
        store.list_workflow_runs(None).unwrap(),
        vec![first, second, other]
    );

    let mut run = test_run(&definition);
    run.created_at += chrono::Duration::seconds(4);
    store.save_workflow_definition(&definition).unwrap();
    store.save_workflow_run(&run).unwrap();
    store.save_workflow_step(&test_steps(&run)[0]).unwrap();
    assert!(store.delete_workflow_run(&run.id).unwrap());
    assert!(matches!(
        store.load_workflow_run(&run.id),
        Err(StorageError::NotFound(_))
    ));
    assert!(!store.delete_workflow_run(&run.id).unwrap());
}

#[test]
fn workflow_step_round_trips() {
    let store = SqliteStore::in_memory().unwrap();
    let definition = test_definition();
    let run = test_run(&definition);
    let step = test_steps(&run).remove(0);
    store.save_workflow_definition(&definition).unwrap();
    store.save_workflow_run(&run).unwrap();

    store.save_workflow_step(&step).unwrap();
    let loaded = store.load_workflow_step(&step.id).unwrap();

    assert_eq!(loaded, step);
}

#[test]
fn workflow_step_update_and_list_round_trip() {
    let store = SqliteStore::in_memory().unwrap();
    let definition = test_definition();
    let run = test_run(&definition);
    let steps = test_steps(&run);
    store.save_workflow_definition(&definition).unwrap();
    store.save_workflow_run(&run).unwrap();
    for step in &steps {
        store.save_workflow_step(step).unwrap();
    }

    let mut first = steps[0].clone();
    first.status = WorkflowStepStatus::Completed;
    first.completed_at = Some(Utc::now());
    store.save_workflow_step(&first).unwrap();

    let mut updated_steps = steps.clone();
    updated_steps[0] = first.clone();
    assert_eq!(store.list_workflow_steps(&run.id).unwrap(), updated_steps);
    assert_eq!(store.load_workflow_step(&first.id).unwrap(), first);
}

#[test]
fn workflow_step_delete_round_trips() {
    let store = SqliteStore::in_memory().unwrap();
    let definition = test_definition();
    let run = test_run(&definition);
    let step = test_steps(&run).remove(0);
    store.save_workflow_definition(&definition).unwrap();
    store.save_workflow_run(&run).unwrap();
    store.save_workflow_step(&step).unwrap();

    assert!(store.delete_workflow_step(&step.id).unwrap());
    assert!(matches!(
        store.load_workflow_step(&step.id),
        Err(StorageError::NotFound(_))
    ));
    assert!(!store.delete_workflow_step(&step.id).unwrap());
}
