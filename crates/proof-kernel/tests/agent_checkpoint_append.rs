use chrono::Utc;
use proof_kernel::{
    AgentCheckpoint, AgentCheckpointAppendResult, AgentCheckpointTail, AgentRun,
    AgentRunEvaluation, AgentRunStep, AgentRunStore, RecordingAgentRunStore,
};
use serde_json::json;
use uuid::Uuid;

fn checkpoint(run_id: Uuid, sequence: u32, value: u32) -> AgentCheckpoint {
    AgentCheckpoint::create(run_id, sequence, json!({"value": value}), Utc::now()).unwrap()
}

fn tail(checkpoint: &AgentCheckpoint) -> AgentCheckpointTail {
    AgentCheckpointTail::from(checkpoint)
}

#[test]
fn recording_store_appends_against_the_exact_expected_tail() {
    let store = RecordingAgentRunStore::default();
    let run_id = Uuid::now_v7();
    let first = checkpoint(run_id, 0, 0);
    let second = checkpoint(run_id, 1, 1);

    assert_eq!(
        store.append_agent_checkpoint(None, &first).unwrap(),
        AgentCheckpointAppendResult::Appended
    );
    assert_eq!(
        store
            .append_agent_checkpoint(Some(&tail(&first)), &second)
            .unwrap(),
        AgentCheckpointAppendResult::Appended
    );
    assert_eq!(
        store.list_agent_checkpoints(&run_id).unwrap(),
        vec![first, second]
    );
}

#[test]
fn stale_expected_tail_performs_no_write() {
    let store = RecordingAgentRunStore::default();
    let run_id = Uuid::now_v7();
    let first = checkpoint(run_id, 0, 0);
    let candidate = checkpoint(run_id, 1, 1);
    let wrong_tail = tail(&checkpoint(run_id, 0, 9));
    store.append_agent_checkpoint(None, &first).unwrap();
    let before = store.list_agent_checkpoints(&run_id).unwrap();

    assert_eq!(
        store
            .append_agent_checkpoint(Some(&wrong_tail), &candidate)
            .unwrap(),
        AgentCheckpointAppendResult::Stale
    );
    assert_eq!(store.list_agent_checkpoints(&run_id).unwrap(), before);
}

#[test]
fn exact_retry_requires_the_current_checkpoint_and_its_predecessor() {
    let store = RecordingAgentRunStore::default();
    let run_id = Uuid::now_v7();
    let first = checkpoint(run_id, 0, 0);
    let second = checkpoint(run_id, 1, 1);
    let third = checkpoint(run_id, 2, 2);
    store.append_agent_checkpoint(None, &first).unwrap();
    store
        .append_agent_checkpoint(Some(&tail(&first)), &second)
        .unwrap();

    assert_eq!(
        store
            .append_agent_checkpoint(Some(&tail(&first)), &second)
            .unwrap(),
        AgentCheckpointAppendResult::Appended
    );
    let wrong_predecessor = tail(&checkpoint(run_id, 0, 9));
    assert_eq!(
        store
            .append_agent_checkpoint(Some(&wrong_predecessor), &second)
            .unwrap(),
        AgentCheckpointAppendResult::Stale
    );
    assert_eq!(store.list_agent_checkpoints(&run_id).unwrap().len(), 2);

    store
        .append_agent_checkpoint(Some(&tail(&second)), &third)
        .unwrap();
    assert_eq!(
        store
            .append_agent_checkpoint(Some(&tail(&first)), &second)
            .unwrap(),
        AgentCheckpointAppendResult::Stale
    );
    assert_eq!(store.list_agent_checkpoints(&run_id).unwrap().len(), 3);
}

#[test]
fn malformed_sequence_id_and_digest_candidates_error_and_write_nothing() {
    let store = RecordingAgentRunStore::default();
    let run_id = Uuid::now_v7();
    let first = checkpoint(run_id, 0, 0);
    store.append_agent_checkpoint(None, &first).unwrap();
    let expected = tail(&first);

    let sequence_conflict = checkpoint(run_id, 0, 1);
    let mut id_conflict = checkpoint(run_id, 1, 2);
    id_conflict.id = first.id;
    let mut digest_conflict = checkpoint(run_id, 1, 3);
    digest_conflict.state_digest = checkpoint(run_id, 1, 4).state_digest;

    for conflicting in [sequence_conflict, id_conflict, digest_conflict] {
        let before = store.list_agent_checkpoints(&run_id).unwrap();
        assert!(store
            .append_agent_checkpoint(Some(&expected), &conflicting)
            .is_err());
        assert_eq!(store.list_agent_checkpoints(&run_id).unwrap(), before);
    }
}

#[test]
fn expected_tail_from_another_run_errors_and_writes_nothing() {
    let store = RecordingAgentRunStore::default();
    let first_run_id = Uuid::now_v7();
    let second_run_id = Uuid::now_v7();
    let first = checkpoint(first_run_id, 0, 0);
    let candidate = checkpoint(second_run_id, 1, 1);
    store.append_agent_checkpoint(None, &first).unwrap();

    assert!(store
        .append_agent_checkpoint(Some(&tail(&first)), &candidate)
        .is_err());
    assert!(store
        .list_agent_checkpoints(&second_run_id)
        .unwrap()
        .is_empty());
    assert_eq!(
        store.list_agent_checkpoints(&first_run_id).unwrap(),
        vec![first]
    );
}

#[test]
fn corrupted_current_checkpoint_errors_without_appending() {
    let store = RecordingAgentRunStore::default();
    let run_id = Uuid::now_v7();
    let mut corrupted_current = checkpoint(run_id, 0, 0);
    corrupted_current.state = json!({"value": "tampered-after-digest"});
    store.save_agent_checkpoint(&corrupted_current).unwrap();
    let candidate = checkpoint(run_id, 1, 1);
    let before = store.list_agent_checkpoints(&run_id).unwrap();

    let error = store
        .append_agent_checkpoint(Some(&tail(&corrupted_current)), &candidate)
        .unwrap_err();
    assert!(error.contains("stored current agent checkpoint state digest"));
    assert_eq!(store.list_agent_checkpoints(&run_id).unwrap(), before);
}

#[test]
fn corrupted_exact_retry_predecessor_errors_without_writing() {
    let store = RecordingAgentRunStore::default();
    let run_id = Uuid::now_v7();
    let mut corrupted_predecessor = checkpoint(run_id, 0, 0);
    corrupted_predecessor.state = json!({"value": "tampered-after-digest"});
    let current = checkpoint(run_id, 1, 1);
    store.save_agent_checkpoint(&corrupted_predecessor).unwrap();
    store.save_agent_checkpoint(&current).unwrap();
    let before = store.list_agent_checkpoints(&run_id).unwrap();

    let error = store
        .append_agent_checkpoint(Some(&tail(&corrupted_predecessor)), &current)
        .unwrap_err();
    assert!(error.contains("stored predecessor agent checkpoint state digest"));
    assert_eq!(store.list_agent_checkpoints(&run_id).unwrap(), before);
}

struct LegacyStore;

impl AgentRunStore for LegacyStore {
    fn save_agent_run(&self, _run: &AgentRun) -> Result<(), String> {
        unreachable!()
    }

    fn load_agent_run(&self, _run_id: &Uuid) -> Result<Option<AgentRun>, String> {
        unreachable!()
    }

    fn list_agent_runs(&self) -> Result<Vec<AgentRun>, String> {
        unreachable!()
    }

    fn save_agent_run_step(&self, _step: &AgentRunStep) -> Result<(), String> {
        unreachable!()
    }

    fn load_agent_run_step(&self, _step_id: &Uuid) -> Result<Option<AgentRunStep>, String> {
        unreachable!()
    }

    fn list_agent_run_steps(&self, _run_id: &Uuid) -> Result<Vec<AgentRunStep>, String> {
        unreachable!()
    }

    fn find_agent_run_step_by_approval(
        &self,
        _approval_request_id: &Uuid,
    ) -> Result<Option<AgentRunStep>, String> {
        unreachable!()
    }

    fn save_agent_checkpoint(&self, _checkpoint: &AgentCheckpoint) -> Result<(), String> {
        unreachable!()
    }

    fn list_agent_checkpoints(&self, _run_id: &Uuid) -> Result<Vec<AgentCheckpoint>, String> {
        unreachable!()
    }

    fn save_agent_run_evaluation(&self, _evaluation: &AgentRunEvaluation) -> Result<(), String> {
        unreachable!()
    }

    fn list_agent_run_evaluations(
        &self,
        _run_id: &Uuid,
    ) -> Result<Vec<AgentRunEvaluation>, String> {
        unreachable!()
    }
}

#[test]
fn legacy_store_uses_the_unsupported_default() {
    let checkpoint = checkpoint(Uuid::now_v7(), 0, 0);
    assert_eq!(
        LegacyStore
            .append_agent_checkpoint(None, &checkpoint)
            .unwrap(),
        AgentCheckpointAppendResult::Unsupported
    );
}
