CREATE TABLE operator_workspaces (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    workspace_id TEXT NOT NULL UNIQUE
        CHECK (length(workspace_id) = 36 AND lower(workspace_id) = workspace_id),
    schema TEXT NOT NULL
        CHECK (schema = 'proof-operator-workspace/v1'),
    database_name TEXT NOT NULL
        CHECK (database_name = 'storage.db'),
    fingerprint_json TEXT NOT NULL CHECK (json_valid(fingerprint_json)),
    workspace_fingerprint TEXT NOT NULL UNIQUE
        CHECK (length(workspace_fingerprint) = 75
               AND workspace_fingerprint GLOB 'blake3-256:[0-9a-f]*'),
    schema_catalog_digest TEXT NOT NULL
        CHECK (length(schema_catalog_digest) = 75
               AND schema_catalog_digest GLOB 'blake3-256:[0-9a-f]*'),
    binding_digest TEXT NOT NULL UNIQUE
        CHECK (length(binding_digest) = 75
               AND binding_digest GLOB 'blake3-256:[0-9a-f]*'),
    agent_id TEXT NOT NULL REFERENCES principals(id),
    human_id TEXT NOT NULL REFERENCES principals(id),
    auth_epoch INTEGER NOT NULL
        CHECK (auth_epoch BETWEEN 1 AND 9007199254740991),
    policy_revision INTEGER NOT NULL
        CHECK (policy_revision BETWEEN 1 AND 9007199254740991),
    capabilities_json TEXT NOT NULL CHECK (json_valid(capabilities_json)),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    binding_json TEXT NOT NULL CHECK (json_valid(binding_json)),
    CHECK (COALESCE(json_extract(fingerprint_json, '$.schema') =
                     'proof.operator.workspace-fingerprint-input/v1', 0) = 1),
    CHECK (COALESCE(json_extract(binding_json, '$.schema_catalog_digest') =
                     schema_catalog_digest, 0) = 1)
);

CREATE TABLE operator_human_enrollments (
    workspace_id TEXT PRIMARY KEY REFERENCES operator_workspaces(workspace_id),
    human_id TEXT NOT NULL UNIQUE REFERENCES principals(id),
    schema TEXT NOT NULL
        CHECK (schema = 'proof-operator-human-enrollment/v1'),
    capability_set_digest TEXT NOT NULL
        CHECK (length(capability_set_digest) = 75
               AND capability_set_digest GLOB 'blake3-256:[0-9a-f]*'),
    enrolled_at TEXT NOT NULL,
    enrollment_json TEXT NOT NULL CHECK (json_valid(enrollment_json))
);

CREATE TABLE operator_budget_accounts (
    budget_id TEXT PRIMARY KEY
        CHECK (length(budget_id) = 36 AND lower(budget_id) = budget_id),
    workspace_id TEXT NOT NULL UNIQUE REFERENCES operator_workspaces(workspace_id),
    schema TEXT NOT NULL CHECK (schema = 'proof-operator-budget-account/v1'),
    revision INTEGER NOT NULL
        CHECK (revision BETWEEN 0 AND 9007199254740991),
    state TEXT NOT NULL CHECK (state IN ('active', 'exhausted', 'closed')),
    max_steps INTEGER NOT NULL CHECK (max_steps BETWEEN 0 AND 9007199254740991),
    max_tokens INTEGER NOT NULL CHECK (max_tokens BETWEEN 0 AND 9007199254740991),
    max_duration_ms INTEGER NOT NULL
        CHECK (max_duration_ms BETWEEN 0 AND 9007199254740991),
    max_cost_microusd INTEGER NOT NULL
        CHECK (max_cost_microusd BETWEEN 0 AND 9007199254740991),
    max_tool_dispatches INTEGER NOT NULL
        CHECK (max_tool_dispatches BETWEEN 0 AND 9007199254740991),
    reserved_steps INTEGER NOT NULL DEFAULT 0
        CHECK (reserved_steps BETWEEN 0 AND 9007199254740991),
    reserved_tokens INTEGER NOT NULL DEFAULT 0
        CHECK (reserved_tokens BETWEEN 0 AND 9007199254740991),
    reserved_duration_ms INTEGER NOT NULL DEFAULT 0
        CHECK (reserved_duration_ms BETWEEN 0 AND 9007199254740991),
    reserved_cost_microusd INTEGER NOT NULL DEFAULT 0
        CHECK (reserved_cost_microusd BETWEEN 0 AND 9007199254740991),
    reserved_tool_dispatches INTEGER NOT NULL DEFAULT 0
        CHECK (reserved_tool_dispatches BETWEEN 0 AND 9007199254740991),
    committed_steps INTEGER NOT NULL DEFAULT 0
        CHECK (committed_steps BETWEEN 0 AND 9007199254740991),
    committed_tokens INTEGER NOT NULL DEFAULT 0
        CHECK (committed_tokens BETWEEN 0 AND 9007199254740991),
    committed_duration_ms INTEGER NOT NULL DEFAULT 0
        CHECK (committed_duration_ms BETWEEN 0 AND 9007199254740991),
    committed_cost_microusd INTEGER NOT NULL DEFAULT 0
        CHECK (committed_cost_microusd BETWEEN 0 AND 9007199254740991),
    committed_tool_dispatches INTEGER NOT NULL DEFAULT 0
        CHECK (committed_tool_dispatches BETWEEN 0 AND 9007199254740991),
    deadline_at TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    limits_digest TEXT NOT NULL
        CHECK (length(limits_digest) = 75
               AND limits_digest GLOB 'blake3-256:[0-9a-f]*'),
    limits_json TEXT NOT NULL CHECK (json_valid(limits_json)),
    CHECK (committed_steps <= max_steps
           AND reserved_steps <= max_steps - committed_steps),
    CHECK (committed_tokens <= max_tokens
           AND reserved_tokens <= max_tokens - committed_tokens),
    CHECK (committed_duration_ms <= max_duration_ms
           AND reserved_duration_ms <= max_duration_ms - committed_duration_ms),
    CHECK (committed_cost_microusd <= max_cost_microusd
           AND reserved_cost_microusd <= max_cost_microusd - committed_cost_microusd),
    CHECK (committed_tool_dispatches <= max_tool_dispatches
           AND reserved_tool_dispatches <= max_tool_dispatches - committed_tool_dispatches)
);

CREATE TABLE operator_run_control (
    run_id TEXT PRIMARY KEY REFERENCES agent_runs(id),
    workspace_id TEXT NOT NULL REFERENCES operator_workspaces(workspace_id),
    budget_id TEXT NOT NULL REFERENCES operator_budget_accounts(budget_id),
    schema TEXT NOT NULL CHECK (schema = 'proof-operator-run-control/v1'),
    control_revision INTEGER NOT NULL
        CHECK (control_revision BETWEEN 0 AND 9007199254740991),
    active_dispatch_reservation_id TEXT,
    recovery_directive_id TEXT,
    recovery_directive_digest TEXT
        CHECK (recovery_directive_digest IS NULL
               OR (length(recovery_directive_digest) = 75
                   AND recovery_directive_digest GLOB 'blake3-256:[0-9a-f]*')),
    last_command_id TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    binding_digest TEXT NOT NULL UNIQUE
        CHECK (length(binding_digest) = 75
               AND binding_digest GLOB 'blake3-256:[0-9a-f]*'),
    binding_json TEXT NOT NULL CHECK (json_valid(binding_json)),
    UNIQUE (budget_id, run_id),
    CHECK ((recovery_directive_id IS NULL AND recovery_directive_digest IS NULL)
           OR (recovery_directive_id IS NOT NULL
               AND recovery_directive_digest IS NOT NULL))
);

CREATE TABLE operator_run_leases (
    lease_id TEXT PRIMARY KEY
        CHECK (length(lease_id) = 36 AND lower(lease_id) = lease_id),
    run_id TEXT NOT NULL REFERENCES operator_run_control(run_id),
    workspace_id TEXT NOT NULL REFERENCES operator_workspaces(workspace_id),
    owner_instance_id TEXT NOT NULL
        CHECK (length(owner_instance_id) = 36 AND lower(owner_instance_id) = owner_instance_id),
    process_epoch_id TEXT NOT NULL
        CHECK (length(process_epoch_id) = 36 AND lower(process_epoch_id) = process_epoch_id),
    lease_token_digest TEXT NOT NULL
        CHECK (length(lease_token_digest) = 75
               AND lease_token_digest GLOB 'blake3-256:[0-9a-f]*'),
    fence_epoch INTEGER NOT NULL
        CHECK (fence_epoch BETWEEN 1 AND 9007199254740991),
    revision INTEGER NOT NULL
        CHECK (revision BETWEEN 0 AND 9007199254740991),
    state TEXT NOT NULL CHECK (state IN ('active', 'released')),
    acquired_at TEXT NOT NULL,
    renewed_at TEXT NOT NULL,
    expires_at TEXT NOT NULL,
    released_at TEXT,
    lease_json TEXT NOT NULL CHECK (json_valid(lease_json)),
    lease_digest TEXT NOT NULL
        CHECK (length(lease_digest) = 75
               AND lease_digest GLOB 'blake3-256:[0-9a-f]*'),
    UNIQUE (run_id, fence_epoch),
    UNIQUE (run_id, lease_id, fence_epoch),
    CHECK (COALESCE((state = 'active' AND released_at IS NULL)
           OR (state = 'released' AND released_at IS NOT NULL), 0) = 1)
);

CREATE UNIQUE INDEX idx_operator_run_leases_one_active
    ON operator_run_leases(run_id) WHERE state = 'active';
CREATE INDEX idx_operator_run_leases_recovery
    ON operator_run_leases(state, expires_at, run_id);

CREATE TABLE operator_budget_reservations (
    reservation_id TEXT PRIMARY KEY
        CHECK (length(reservation_id) = 36 AND lower(reservation_id) = reservation_id),
    budget_id TEXT NOT NULL REFERENCES operator_budget_accounts(budget_id),
    run_id TEXT NOT NULL REFERENCES operator_run_control(run_id),
    lease_id TEXT NOT NULL,
    fence_epoch INTEGER NOT NULL
        CHECK (fence_epoch BETWEEN 1 AND 9007199254740991),
    idempotency_key TEXT NOT NULL
        CHECK (length(idempotency_key) = 36 AND lower(idempotency_key) = idempotency_key),
    request_digest TEXT NOT NULL
        CHECK (length(request_digest) = 75
               AND request_digest GLOB 'blake3-256:[0-9a-f]*'),
    schema TEXT NOT NULL CHECK (schema = 'proof-operator-budget-reservation/v1'),
    kind TEXT NOT NULL CHECK (kind IN ('provider', 'tool')),
    intent_digest TEXT NOT NULL
        CHECK (length(intent_digest) = 75
               AND intent_digest GLOB 'blake3-256:[0-9a-f]*'),
    intent_json TEXT NOT NULL CHECK (json_valid(intent_json)),
    replay_binding_digest TEXT
        CHECK (replay_binding_digest IS NULL
               OR (length(replay_binding_digest) = 75
                   AND replay_binding_digest GLOB 'blake3-256:[0-9a-f]*')),
    replay_operation TEXT,
    replay_version TEXT,
    replay_idempotency_key TEXT
        CHECK (replay_idempotency_key IS NULL
               OR (length(replay_idempotency_key) = 36
                   AND lower(replay_idempotency_key) = replay_idempotency_key)),
    replay_input_digest TEXT
        CHECK (replay_input_digest IS NULL OR length(replay_input_digest) = 64),
    replay_claimed_by TEXT REFERENCES principals(id),
    replay_json TEXT CHECK (replay_json IS NULL OR json_valid(replay_json)),
    recovery_directive_id TEXT UNIQUE,
    recovery_directive_digest TEXT
        CHECK (recovery_directive_digest IS NULL
               OR (length(recovery_directive_digest) = 75
                   AND recovery_directive_digest GLOB 'blake3-256:[0-9a-f]*')),
    recovery_json TEXT CHECK (recovery_json IS NULL OR json_valid(recovery_json)),
    state TEXT NOT NULL
        CHECK (state IN ('reserved', 'dispatching', 'committed', 'released', 'forfeited')),
    reserved_steps INTEGER NOT NULL CHECK (reserved_steps BETWEEN 0 AND 9007199254740991),
    reserved_tokens INTEGER NOT NULL CHECK (reserved_tokens BETWEEN 0 AND 9007199254740991),
    reserved_duration_ms INTEGER NOT NULL
        CHECK (reserved_duration_ms BETWEEN 0 AND 9007199254740991),
    reserved_cost_microusd INTEGER NOT NULL
        CHECK (reserved_cost_microusd BETWEEN 0 AND 9007199254740991),
    reserved_tool_dispatches INTEGER NOT NULL
        CHECK (reserved_tool_dispatches BETWEEN 0 AND 9007199254740991),
    charged_steps INTEGER NOT NULL DEFAULT 0
        CHECK (charged_steps BETWEEN 0 AND 9007199254740991),
    charged_tokens INTEGER NOT NULL DEFAULT 0
        CHECK (charged_tokens BETWEEN 0 AND 9007199254740991),
    charged_duration_ms INTEGER NOT NULL DEFAULT 0
        CHECK (charged_duration_ms BETWEEN 0 AND 9007199254740991),
    charged_cost_microusd INTEGER NOT NULL DEFAULT 0
        CHECK (charged_cost_microusd BETWEEN 0 AND 9007199254740991),
    charged_tool_dispatches INTEGER NOT NULL DEFAULT 0
        CHECK (charged_tool_dispatches BETWEEN 0 AND 9007199254740991),
    created_at TEXT NOT NULL,
    permit_id TEXT UNIQUE
        CHECK (permit_id IS NULL
               OR (length(permit_id) = 36 AND lower(permit_id) = permit_id)),
    dispatch_token_digest TEXT
        CHECK (dispatch_token_digest IS NULL
               OR (length(dispatch_token_digest) = 75
                   AND dispatch_token_digest GLOB 'blake3-256:[0-9a-f]*')),
    call_digest TEXT
        CHECK (call_digest IS NULL
               OR (length(call_digest) = 75
                   AND call_digest GLOB 'blake3-256:[0-9a-f]*')),
    prepared_execution_digest TEXT
        CHECK (prepared_execution_digest IS NULL
               OR (length(prepared_execution_digest) = 75
                   AND prepared_execution_digest GLOB 'blake3-256:[0-9a-f]*')),
    result_digest TEXT
        CHECK (result_digest IS NULL
               OR (length(result_digest) = 75
                   AND result_digest GLOB 'blake3-256:[0-9a-f]*')),
    prepared_binding_json TEXT
        CHECK (prepared_binding_json IS NULL OR json_valid(prepared_binding_json)),
    runtime_commit_json TEXT
        CHECK (runtime_commit_json IS NULL OR json_valid(runtime_commit_json)),
    dispatch_started_at TEXT,
    settled_at TEXT,
    reservation_json TEXT NOT NULL CHECK (json_valid(reservation_json)),
    UNIQUE (budget_id, idempotency_key),
    FOREIGN KEY (budget_id, run_id)
        REFERENCES operator_run_control(budget_id, run_id),
    FOREIGN KEY (run_id, lease_id, fence_epoch)
        REFERENCES operator_run_leases(run_id, lease_id, fence_epoch),
    CHECK (COALESCE((json_extract(intent_json, '$.schema') =
                     'proof.operator.dispatch-intent/v1'
                     AND json_extract(intent_json, '$.kind') = kind), 0) = 1),
    CHECK (COALESCE((replay_binding_digest IS NULL
                     AND replay_operation IS NULL AND replay_version IS NULL
                     AND replay_idempotency_key IS NULL
                     AND replay_input_digest IS NULL
                     AND replay_claimed_by IS NULL AND replay_json IS NULL)
           OR (kind = 'tool' AND replay_binding_digest IS NOT NULL
               AND replay_operation IS NOT NULL AND replay_version IS NOT NULL
               AND replay_idempotency_key IS NOT NULL
               AND replay_input_digest IS NOT NULL
               AND replay_claimed_by IS NOT NULL
               AND replay_json IS NOT NULL
               AND json_extract(replay_json, '$.run_id') = run_id
               AND json_extract(replay_json, '$.operation') = replay_operation
               AND json_extract(replay_json, '$.version') = replay_version
               AND json_extract(replay_json, '$.idempotency_key') = replay_idempotency_key
               AND json_extract(replay_json, '$.input_digest') = replay_input_digest
               AND json_extract(replay_json, '$.claimed_by') = replay_claimed_by), 0) = 1),
    CHECK (COALESCE((recovery_directive_id IS NULL
                     AND recovery_directive_digest IS NULL AND recovery_json IS NULL)
           OR (recovery_directive_id IS NOT NULL
               AND recovery_directive_digest IS NOT NULL AND recovery_json IS NOT NULL
               AND json_extract(recovery_json, '$.directive_id') = recovery_directive_id
               AND json_extract(recovery_json, '$.directive_digest') = recovery_directive_digest
               AND json_extract(recovery_json, '$.run_id') = run_id
               AND json_extract(recovery_json, '$.intent_digest') = intent_digest), 0) = 1),
    CHECK (charged_steps <= reserved_steps
           AND charged_tokens <= reserved_tokens
           AND charged_duration_ms <= reserved_duration_ms
           AND charged_cost_microusd <= reserved_cost_microusd
           AND charged_tool_dispatches <= reserved_tool_dispatches),
    CHECK (COALESCE((state = 'reserved'
            AND permit_id IS NULL AND dispatch_token_digest IS NULL
            AND call_digest IS NULL
            AND prepared_execution_digest IS NULL AND result_digest IS NULL
            AND prepared_binding_json IS NULL AND runtime_commit_json IS NULL
            AND dispatch_started_at IS NULL AND settled_at IS NULL
            AND charged_steps = 0 AND charged_tokens = 0
            AND charged_duration_ms = 0 AND charged_cost_microusd = 0
            AND charged_tool_dispatches = 0)
           OR (state = 'dispatching'
               AND permit_id IS NOT NULL AND dispatch_token_digest IS NOT NULL
               AND call_digest IS NOT NULL
               AND prepared_execution_digest IS NULL AND result_digest IS NULL
               AND prepared_binding_json IS NULL AND runtime_commit_json IS NULL
               AND dispatch_started_at IS NOT NULL AND settled_at IS NULL
               AND charged_steps = 0 AND charged_tokens = 0
               AND charged_duration_ms = 0 AND charged_cost_microusd = 0
               AND charged_tool_dispatches = 0)
           OR (state = 'committed'
               AND permit_id IS NOT NULL AND dispatch_token_digest IS NOT NULL
               AND call_digest IS NOT NULL
               AND prepared_execution_digest IS NOT NULL AND result_digest IS NOT NULL
               AND prepared_binding_json IS NOT NULL AND runtime_commit_json IS NOT NULL
               AND json_extract(prepared_binding_json, '$.payload_digest') = prepared_execution_digest
               AND json_extract(prepared_binding_json, '$.result_digest') = result_digest
               AND json_extract(runtime_commit_json, '$.prepared_execution_digest') = prepared_execution_digest
               AND json_extract(runtime_commit_json, '$.result_digest') = result_digest
               AND dispatch_started_at IS NOT NULL AND settled_at IS NOT NULL)
           OR (state = 'released'
               AND permit_id IS NULL AND dispatch_token_digest IS NULL
               AND call_digest IS NULL
               AND prepared_execution_digest IS NULL AND result_digest IS NULL
               AND prepared_binding_json IS NULL AND runtime_commit_json IS NULL
               AND dispatch_started_at IS NULL AND settled_at IS NOT NULL
               AND charged_steps = 0 AND charged_tokens = 0
               AND charged_duration_ms = 0 AND charged_cost_microusd = 0
               AND charged_tool_dispatches = 0)
           OR (state = 'forfeited'
               AND permit_id IS NOT NULL AND dispatch_token_digest IS NOT NULL
               AND call_digest IS NOT NULL
               AND prepared_execution_digest IS NULL AND result_digest IS NULL
               AND prepared_binding_json IS NULL AND runtime_commit_json IS NULL
               AND dispatch_started_at IS NOT NULL AND settled_at IS NOT NULL
               AND charged_steps = reserved_steps
               AND charged_tokens = reserved_tokens
               AND charged_duration_ms = reserved_duration_ms
               AND charged_cost_microusd = reserved_cost_microusd
               AND charged_tool_dispatches = reserved_tool_dispatches), 0) = 1)
);

CREATE INDEX idx_operator_budget_reservations_open
    ON operator_budget_reservations(budget_id, state, created_at, reservation_id);
CREATE UNIQUE INDEX idx_operator_budget_reservations_one_open_run
    ON operator_budget_reservations(run_id)
    WHERE state IN ('reserved', 'dispatching');
CREATE INDEX idx_operator_budget_reservations_run
    ON operator_budget_reservations(run_id, created_at, reservation_id);

CREATE TABLE operator_replay_bindings (
    operation TEXT NOT NULL,
    version TEXT NOT NULL,
    idempotency_key TEXT NOT NULL
        CHECK (length(idempotency_key) = 36
               AND lower(idempotency_key) = idempotency_key),
    workspace_id TEXT NOT NULL REFERENCES operator_workspaces(workspace_id),
    run_id TEXT NOT NULL REFERENCES operator_run_control(run_id),
    step_id TEXT NOT NULL REFERENCES agent_run_steps(id),
    origin_reservation_id TEXT NOT NULL UNIQUE
        REFERENCES operator_budget_reservations(reservation_id),
    checkpoint_id TEXT NOT NULL REFERENCES agent_checkpoints(id),
    checkpoint_sequence INTEGER NOT NULL
        CHECK (checkpoint_sequence BETWEEN 0 AND 9007199254740991),
    checkpoint_digest TEXT NOT NULL CHECK (length(checkpoint_digest) = 64),
    input_digest TEXT NOT NULL CHECK (length(input_digest) = 64),
    claimed_by TEXT NOT NULL REFERENCES principals(id),
    binding_digest TEXT NOT NULL UNIQUE
        CHECK (length(binding_digest) = 75
               AND binding_digest GLOB 'blake3-256:[0-9a-f]*'),
    binding_json TEXT NOT NULL CHECK (json_valid(binding_json)),
    created_at TEXT NOT NULL,
    PRIMARY KEY (operation, version, idempotency_key),
    UNIQUE (run_id, step_id),
    FOREIGN KEY (operation, version, idempotency_key)
        REFERENCES execution_replays(operation, version, idempotency_key),
    CHECK (COALESCE(
        json_extract(binding_json, '$.schema') =
            'proof.operator.replay-claim-binding/v1'
        AND json_extract(binding_json, '$.workspace_id') = workspace_id
        AND json_extract(binding_json, '$.run_id') = run_id
        AND json_extract(binding_json, '$.step_id') = step_id
        AND json_extract(binding_json, '$.checkpoint_id') = checkpoint_id
        AND json_extract(binding_json, '$.checkpoint_sequence') = checkpoint_sequence
        AND json_extract(binding_json, '$.checkpoint_digest') = checkpoint_digest
        AND json_extract(binding_json, '$.operation') = operation
        AND json_extract(binding_json, '$.version') = version
        AND json_extract(binding_json, '$.idempotency_key') = idempotency_key
        AND json_extract(binding_json, '$.input_digest') = input_digest
        AND json_extract(binding_json, '$.claimed_by') = claimed_by
        AND json_extract(binding_json, '$.binding_digest') = binding_digest,
        0) = 1)
);

CREATE INDEX idx_operator_replay_bindings_run
    ON operator_replay_bindings(run_id, step_id);

CREATE TABLE operator_recovery_directives (
    directive_id TEXT PRIMARY KEY
        CHECK (length(directive_id) = 36 AND lower(directive_id) = directive_id),
    workspace_id TEXT NOT NULL REFERENCES operator_workspaces(workspace_id),
    run_id TEXT NOT NULL REFERENCES operator_run_control(run_id),
    source_lease_id TEXT NOT NULL REFERENCES operator_run_leases(lease_id),
    source_reservation_id TEXT NOT NULL REFERENCES operator_budget_reservations(reservation_id),
    source_budget_id TEXT NOT NULL REFERENCES operator_budget_accounts(budget_id),
    source_idempotency_key TEXT NOT NULL
        CHECK (length(source_idempotency_key) = 36
               AND lower(source_idempotency_key) = source_idempotency_key),
    source_request_digest TEXT NOT NULL
        CHECK (length(source_request_digest) = 75
               AND source_request_digest GLOB 'blake3-256:[0-9a-f]*'),
    schema TEXT NOT NULL CHECK (schema = 'proof.operator.recovery-directive/v1'),
    classification TEXT NOT NULL
        CHECK (classification = 'pre_dispatch_recoverable'),
    checkpoint_id TEXT NOT NULL REFERENCES agent_checkpoints(id),
    checkpoint_sequence INTEGER NOT NULL
        CHECK (checkpoint_sequence BETWEEN 0 AND 9007199254740991),
    checkpoint_digest TEXT NOT NULL CHECK (length(checkpoint_digest) = 64),
    source_fence_epoch INTEGER NOT NULL
        CHECK (source_fence_epoch BETWEEN 1 AND 9007199254740991),
    source_control_revision INTEGER NOT NULL
        CHECK (source_control_revision BETWEEN 0 AND 9007199254740991),
    intent_digest TEXT NOT NULL
        CHECK (length(intent_digest) = 75
               AND intent_digest GLOB 'blake3-256:[0-9a-f]*'),
    replay_binding_digest TEXT
        CHECK (replay_binding_digest IS NULL
               OR (length(replay_binding_digest) = 75
                   AND replay_binding_digest GLOB 'blake3-256:[0-9a-f]*')),
    replay_json TEXT CHECK (replay_json IS NULL OR json_valid(replay_json)),
    required_budget_disposition TEXT NOT NULL
        CHECK (required_budget_disposition = 'none'),
    created_at TEXT NOT NULL,
    directive_json TEXT NOT NULL CHECK (json_valid(directive_json)),
    directive_digest TEXT NOT NULL UNIQUE
        CHECK (length(directive_digest) = 75
               AND directive_digest GLOB 'blake3-256:[0-9a-f]*'),
    UNIQUE (run_id, directive_id),
    FOREIGN KEY (run_id, source_lease_id, source_fence_epoch)
        REFERENCES operator_run_leases(run_id, lease_id, fence_epoch),
    FOREIGN KEY (source_budget_id, source_idempotency_key)
        REFERENCES operator_budget_reservations(budget_id, idempotency_key),
    CHECK (COALESCE((replay_binding_digest IS NULL AND replay_json IS NULL)
           OR (replay_binding_digest IS NOT NULL AND replay_json IS NOT NULL
               AND json_extract(replay_json, '$.binding_digest') = replay_binding_digest), 0) = 1),
    CHECK (COALESCE(json_extract(directive_json, '$.source_reservation_id') = source_reservation_id
           AND json_extract(directive_json, '$.source_budget_id') = source_budget_id
           AND json_extract(directive_json, '$.source_idempotency_key') = source_idempotency_key
           AND json_extract(directive_json, '$.source_request_digest') = source_request_digest
           AND json_extract(directive_json, '$.intent_digest') = intent_digest, 0) = 1),
    CHECK (classification = 'pre_dispatch_recoverable'
           AND required_budget_disposition = 'none')
);

CREATE INDEX idx_operator_recovery_directives_run
    ON operator_recovery_directives(run_id, created_at DESC, directive_id);

CREATE TABLE operator_approval_bindings (
    approval_request_id TEXT PRIMARY KEY REFERENCES approval_requests(id),
    workspace_id TEXT NOT NULL REFERENCES operator_workspaces(workspace_id),
    run_id TEXT NOT NULL REFERENCES operator_run_control(run_id),
    step_id TEXT NOT NULL UNIQUE REFERENCES agent_run_steps(id),
    checkpoint_id TEXT NOT NULL REFERENCES agent_checkpoints(id),
    required_human_id TEXT NOT NULL REFERENCES principals(id),
    schema TEXT NOT NULL CHECK (schema = 'proof-operator-approval-binding/v1'),
    checkpoint_sequence INTEGER NOT NULL
        CHECK (checkpoint_sequence BETWEEN 0 AND 9007199254740991),
    checkpoint_digest TEXT NOT NULL CHECK (length(checkpoint_digest) = 64),
    input_digest TEXT NOT NULL CHECK (length(input_digest) = 64),
    argument_digest TEXT NOT NULL
        CHECK (length(argument_digest) = 75
               AND argument_digest GLOB 'blake3-256:[0-9a-f]*'),
    consequence_digest TEXT NOT NULL
        CHECK (length(consequence_digest) = 75
               AND consequence_digest GLOB 'blake3-256:[0-9a-f]*'),
    created_at TEXT NOT NULL,
    binding_json TEXT NOT NULL CHECK (json_valid(binding_json)),
    binding_digest TEXT NOT NULL UNIQUE
        CHECK (length(binding_digest) = 75
               AND binding_digest GLOB 'blake3-256:[0-9a-f]*')
);

CREATE INDEX idx_operator_approval_bindings_run
    ON operator_approval_bindings(run_id, approval_request_id);

CREATE TABLE operator_run_projections (
    projection_sequence INTEGER PRIMARY KEY AUTOINCREMENT,
    projection_id TEXT NOT NULL UNIQUE
        CHECK (length(projection_id) = 36 AND lower(projection_id) = projection_id),
    workspace_id TEXT NOT NULL REFERENCES operator_workspaces(workspace_id),
    run_id TEXT NOT NULL REFERENCES operator_run_control(run_id),
    schema TEXT NOT NULL CHECK (schema = 'proof-operator-run-projection/v1'),
    projection_revision INTEGER NOT NULL
        CHECK (projection_revision BETWEEN 0 AND 9007199254740991),
    source_run_revision INTEGER NOT NULL
        CHECK (source_run_revision BETWEEN 0 AND 9007199254740991),
    source_control_revision INTEGER NOT NULL
        CHECK (source_control_revision BETWEEN 0 AND 9007199254740991),
    checkpoint_id TEXT NOT NULL REFERENCES agent_checkpoints(id),
    checkpoint_sequence INTEGER NOT NULL
        CHECK (checkpoint_sequence BETWEEN 0 AND 9007199254740991),
    checkpoint_digest TEXT NOT NULL CHECK (length(checkpoint_digest) = 64),
    fence_epoch INTEGER NOT NULL
        CHECK (fence_epoch BETWEEN 0 AND 9007199254740991),
    run_status TEXT NOT NULL
        CHECK (run_status IN ('queued', 'running', 'waiting_for_input',
                              'succeeded', 'failed', 'cancelled')),
    attention TEXT NOT NULL
        CHECK (attention IN ('awaiting_decision', 'running', 'recoverable', 'terminal')),
    required_human_id TEXT REFERENCES principals(id),
    approval_request_id TEXT REFERENCES approval_requests(id),
    recovery_directive_id TEXT REFERENCES operator_recovery_directives(directive_id),
    recovery_directive_digest TEXT
        CHECK (recovery_directive_digest IS NULL
               OR (length(recovery_directive_digest) = 75
                   AND recovery_directive_digest GLOB 'blake3-256:[0-9a-f]*')),
    projected_at TEXT NOT NULL,
    snapshot_json TEXT NOT NULL CHECK (json_valid(snapshot_json)),
    snapshot_digest TEXT NOT NULL
        CHECK (length(snapshot_digest) = 75
               AND snapshot_digest GLOB 'blake3-256:[0-9a-f]*'),
    UNIQUE (workspace_id, run_id, projection_revision),
    CHECK (COALESCE(
        (attention = 'awaiting_decision'
         AND run_status = 'waiting_for_input'
         AND required_human_id IS NOT NULL AND approval_request_id IS NOT NULL
         AND recovery_directive_id IS NULL AND recovery_directive_digest IS NULL)
        OR (attention = 'recoverable'
            AND run_status = 'failed'
            AND required_human_id IS NULL AND approval_request_id IS NULL
            AND recovery_directive_id IS NOT NULL
            AND recovery_directive_digest IS NOT NULL)
        OR (attention = 'running'
            AND run_status IN ('queued', 'running')
            AND required_human_id IS NULL AND approval_request_id IS NULL
            AND recovery_directive_id IS NULL AND recovery_directive_digest IS NULL)
        OR (attention = 'terminal'
            AND run_status IN ('succeeded', 'failed', 'cancelled')
            AND required_human_id IS NULL AND approval_request_id IS NULL
            AND recovery_directive_id IS NULL AND recovery_directive_digest IS NULL), 0) = 1),
    CHECK (length(checkpoint_digest) = 64)
);

CREATE INDEX idx_operator_run_projections_latest
    ON operator_run_projections(workspace_id, run_id, projection_sequence DESC);
CREATE INDEX idx_operator_run_projections_page
    ON operator_run_projections(workspace_id, attention, projection_sequence DESC, run_id);
CREATE INDEX idx_operator_run_projections_approval
    ON operator_run_projections(approval_request_id, projection_sequence DESC);

CREATE TABLE operator_commands (
    command_id TEXT PRIMARY KEY
        CHECK (length(command_id) = 36 AND lower(command_id) = command_id),
    workspace_id TEXT NOT NULL REFERENCES operator_workspaces(workspace_id),
    idempotency_key TEXT NOT NULL
        CHECK (length(idempotency_key) = 36 AND lower(idempotency_key) = idempotency_key),
    schema TEXT NOT NULL CHECK (schema = 'proof.operator.command-envelope/v1'),
    kind TEXT NOT NULL
        CHECK (kind IN ('approval_decide', 'run_cancel', 'run_resume', 'session_revoke')),
    human_id TEXT NOT NULL REFERENCES principals(id),
    server_instance_id TEXT NOT NULL
        CHECK (length(server_instance_id) = 36 AND lower(server_instance_id) = server_instance_id),
    session_id TEXT NOT NULL
        CHECK (length(session_id) = 36 AND lower(session_id) = session_id),
    required_capability TEXT
        CHECK (required_capability IN ('approval.decide', 'run.cancel', 'run.resume')),
    target_run_id TEXT REFERENCES operator_run_control(run_id),
    target_step_id TEXT REFERENCES agent_run_steps(id),
    approval_request_id TEXT REFERENCES approval_requests(id),
    expected_run_revision INTEGER
        CHECK (expected_run_revision BETWEEN 0 AND 9007199254740991),
    expected_step_revision INTEGER
        CHECK (expected_step_revision BETWEEN 0 AND 9007199254740991),
    expected_control_revision INTEGER
        CHECK (expected_control_revision BETWEEN 0 AND 9007199254740991),
    expected_checkpoint_id TEXT REFERENCES agent_checkpoints(id),
    expected_checkpoint_sequence INTEGER
        CHECK (expected_checkpoint_sequence BETWEEN 0 AND 9007199254740991),
    expected_checkpoint_digest TEXT,
    expected_fence_epoch INTEGER
        CHECK (expected_fence_epoch BETWEEN 1 AND 9007199254740991),
    recovery_directive_id TEXT REFERENCES operator_recovery_directives(directive_id),
    recovery_directive_digest TEXT
        CHECK (recovery_directive_digest IS NULL
               OR (length(recovery_directive_digest) = 75
                   AND recovery_directive_digest GLOB 'blake3-256:[0-9a-f]*')),
    request_digest TEXT NOT NULL UNIQUE
        CHECK (length(request_digest) = 75
               AND request_digest GLOB 'blake3-256:[0-9a-f]*'),
    decision_digest TEXT CHECK (decision_digest IS NULL OR length(decision_digest) = 64),
    requested_at TEXT NOT NULL,
    command_json TEXT NOT NULL CHECK (json_valid(command_json)),
    UNIQUE (workspace_id, human_id, idempotency_key),
    CHECK (COALESCE((kind = 'approval_decide'
            AND required_capability IS NOT NULL
            AND required_capability = 'approval.decide'
            AND target_run_id IS NOT NULL AND target_step_id IS NOT NULL
            AND approval_request_id IS NOT NULL
            AND expected_run_revision IS NOT NULL
            AND expected_step_revision IS NOT NULL
            AND expected_control_revision IS NOT NULL
            AND expected_checkpoint_id IS NOT NULL
            AND expected_checkpoint_sequence IS NOT NULL
            AND expected_checkpoint_digest IS NOT NULL
            AND length(expected_checkpoint_digest) = 64
            AND expected_fence_epoch IS NOT NULL
            AND recovery_directive_id IS NULL
            AND recovery_directive_digest IS NULL
            AND decision_digest IS NULL)
           OR (kind = 'run_cancel'
               AND required_capability IS NOT NULL
               AND required_capability = 'run.cancel'
               AND target_run_id IS NOT NULL
               AND target_step_id IS NULL AND approval_request_id IS NULL
               AND expected_run_revision IS NOT NULL
               AND expected_step_revision IS NULL
               AND expected_control_revision IS NOT NULL
               AND expected_checkpoint_id IS NULL
               AND expected_checkpoint_sequence IS NULL
               AND expected_checkpoint_digest IS NULL
               AND expected_fence_epoch IS NOT NULL
               AND recovery_directive_id IS NULL
               AND recovery_directive_digest IS NULL
               AND decision_digest IS NULL)
           OR (kind = 'run_resume'
               AND required_capability IS NOT NULL
               AND required_capability = 'run.resume'
               AND target_run_id IS NOT NULL AND target_step_id IS NOT NULL
               AND expected_run_revision IS NOT NULL
               AND expected_step_revision IS NOT NULL
               AND expected_control_revision IS NOT NULL
               AND expected_checkpoint_id IS NOT NULL
               AND expected_checkpoint_sequence IS NOT NULL
               AND expected_checkpoint_digest IS NOT NULL
               AND length(expected_checkpoint_digest) = 64
               AND expected_fence_epoch IS NOT NULL
               AND ((approval_request_id IS NOT NULL
                     AND decision_digest IS NOT NULL
                     AND length(decision_digest) = 64
                     AND recovery_directive_id IS NULL
                     AND recovery_directive_digest IS NULL)
                    OR (approval_request_id IS NULL
                        AND decision_digest IS NULL
                        AND recovery_directive_id IS NOT NULL
                        AND recovery_directive_digest IS NOT NULL)))
           OR (kind = 'session_revoke'
               AND required_capability IS NULL
               AND target_run_id IS NULL AND target_step_id IS NULL
               AND approval_request_id IS NULL
               AND expected_run_revision IS NULL
               AND expected_step_revision IS NULL
               AND expected_control_revision IS NULL
               AND expected_checkpoint_id IS NULL
               AND expected_checkpoint_sequence IS NULL
               AND expected_checkpoint_digest IS NULL
               AND expected_fence_epoch IS NULL
               AND recovery_directive_id IS NULL
               AND recovery_directive_digest IS NULL
               AND decision_digest IS NULL), 0) = 1)
);

CREATE INDEX idx_operator_commands_run
    ON operator_commands(target_run_id, requested_at, command_id);
CREATE INDEX idx_operator_commands_approval
    ON operator_commands(approval_request_id, requested_at, command_id);
CREATE INDEX idx_operator_commands_workspace
    ON operator_commands(workspace_id, requested_at, command_id);

CREATE TABLE operator_audit_heads (
    workspace_id TEXT PRIMARY KEY REFERENCES operator_workspaces(workspace_id),
    last_sequence INTEGER NOT NULL
        CHECK (last_sequence BETWEEN 0 AND 9007199254740991),
    last_digest TEXT,
    CHECK (COALESCE((last_sequence = 0 AND last_digest IS NULL)
           OR (last_sequence > 0 AND last_digest IS NOT NULL
               AND length(last_digest) = 75
               AND last_digest GLOB 'blake3-256:[0-9a-f]*'), 0) = 1)
);

CREATE TABLE operator_audit_events (
    workspace_id TEXT NOT NULL REFERENCES operator_workspaces(workspace_id),
    sequence INTEGER NOT NULL
        CHECK (sequence BETWEEN 1 AND 9007199254740991),
    event_id TEXT NOT NULL UNIQUE
        CHECK (length(event_id) = 36 AND lower(event_id) = event_id),
    schema TEXT NOT NULL CHECK (schema = 'proof.operator.audit-event/v1'),
    kind TEXT NOT NULL CHECK (kind IN (
        'session_challenge_issued', 'session_issued', 'session_replaced',
        'session_revoked', 'session_expired',
        'approval_decided', 'approval_expired',
        'command_rejected', 'command_conflict',
        'run_cancelled', 'run_resumed',
        'lease_acquired', 'lease_renewed', 'lease_reclaimed', 'lease_released',
        'stale_fence_rejected',
        'budget_reserved', 'budget_committed', 'budget_released',
        'budget_forfeited', 'budget_rejected',
        'dispatch_authorized', 'runtime_result_committed',
        'recovery_started', 'recovery_completed',
        'control_failure', 'control_shutdown'
    )),
    outcome TEXT NOT NULL CHECK (outcome IN ('accepted', 'rejected', 'conflict', 'expired', 'failed')),
    previous_digest TEXT,
    event_digest TEXT NOT NULL UNIQUE
        CHECK (length(event_digest) = 75
               AND event_digest GLOB 'blake3-256:[0-9a-f]*'),
    human_id TEXT REFERENCES principals(id),
    session_id TEXT,
    challenge_id TEXT,
    challenge_digest TEXT
        CHECK (challenge_digest IS NULL
               OR (length(challenge_digest) = 75
                   AND challenge_digest GLOB 'blake3-256:[0-9a-f]*')),
    session_authority_digest TEXT
        CHECK (session_authority_digest IS NULL
               OR (length(session_authority_digest) = 75
                   AND session_authority_digest GLOB 'blake3-256:[0-9a-f]*')),
    related_session_id TEXT,
    server_instance_id TEXT,
    run_id TEXT REFERENCES operator_run_control(run_id),
    approval_request_id TEXT REFERENCES approval_requests(id),
    command_id TEXT,
    command_kind TEXT
        CHECK (command_kind IS NULL OR command_kind IN (
            'approval_decide', 'run_cancel', 'run_resume', 'session_revoke')),
    budget_id TEXT REFERENCES operator_budget_accounts(budget_id),
    reservation_id TEXT,
    lease_id TEXT REFERENCES operator_run_leases(lease_id),
    source_lease_id TEXT REFERENCES operator_run_leases(lease_id),
    process_epoch_id TEXT,
    permit_id TEXT,
    recovery_directive_id TEXT REFERENCES operator_recovery_directives(directive_id),
    fence_epoch INTEGER
        CHECK (fence_epoch BETWEEN 0 AND 9007199254740991),
    auth_epoch INTEGER
        CHECK (auth_epoch BETWEEN 1 AND 9007199254740991),
    policy_revision INTEGER
        CHECK (policy_revision BETWEEN 1 AND 9007199254740991),
    intent_digest TEXT
        CHECK (intent_digest IS NULL
               OR (length(intent_digest) = 75
                   AND intent_digest GLOB 'blake3-256:[0-9a-f]*')),
    call_digest TEXT
        CHECK (call_digest IS NULL
               OR (length(call_digest) = 75
                   AND call_digest GLOB 'blake3-256:[0-9a-f]*')),
    decision_digest TEXT
        CHECK (decision_digest IS NULL OR length(decision_digest) = 64),
    recovery_directive_digest TEXT
        CHECK (recovery_directive_digest IS NULL
               OR (length(recovery_directive_digest) = 75
                   AND recovery_directive_digest GLOB 'blake3-256:[0-9a-f]*')),
    failure_scope TEXT
        CHECK (failure_scope IS NULL
               OR failure_scope IN ('command', 'runtime', 'storage', 'workspace')),
    proof_id TEXT REFERENCES proofs(id),
    proof_operation TEXT,
    proof_digest TEXT
        CHECK (proof_digest IS NULL OR length(proof_digest) = 64),
    occurred_at TEXT NOT NULL,
    event_json TEXT NOT NULL CHECK (json_valid(event_json)),
    PRIMARY KEY (workspace_id, sequence),
    CHECK (COALESCE((sequence = 1 AND previous_digest IS NULL)
           OR (sequence > 1 AND previous_digest IS NOT NULL
               AND length(previous_digest) = 75
               AND previous_digest GLOB 'blake3-256:[0-9a-f]*'), 0) = 1),
    CHECK ((proof_id IS NULL AND proof_operation IS NULL AND proof_digest IS NULL)
           OR (proof_id IS NOT NULL AND proof_operation IS NOT NULL
               AND proof_digest IS NOT NULL)),
    CHECK (((kind IN ('session_challenge_issued', 'session_issued',
                     'session_replaced')) AND challenge_digest IS NOT NULL)
           OR ((kind NOT IN ('session_challenge_issued', 'session_issued',
                            'session_replaced')) AND challenge_digest IS NULL)),
    CHECK ((session_id IS NULL AND session_authority_digest IS NULL)
           OR (session_id IS NOT NULL AND session_authority_digest IS NOT NULL)),
    CHECK (session_id IS NULL
           OR (length(session_id) = 36 AND lower(session_id) = session_id)),
    CHECK (server_instance_id IS NULL
           OR (length(server_instance_id) = 36
               AND lower(server_instance_id) = server_instance_id)),
    CHECK (challenge_id IS NULL
           OR (length(challenge_id) = 36 AND lower(challenge_id) = challenge_id)),
    CHECK (related_session_id IS NULL
           OR (length(related_session_id) = 36
               AND lower(related_session_id) = related_session_id)),
    CHECK (command_id IS NULL
           OR (length(command_id) = 36 AND lower(command_id) = command_id)),
    CHECK (reservation_id IS NULL
           OR (length(reservation_id) = 36
               AND lower(reservation_id) = reservation_id)),
    CHECK (process_epoch_id IS NULL
           OR (length(process_epoch_id) = 36
               AND lower(process_epoch_id) = process_epoch_id)),
    CHECK (permit_id IS NULL
           OR (length(permit_id) = 36 AND lower(permit_id) = permit_id)),
    CHECK (COALESCE(
        json_extract(event_json, '$.schema') = schema
        AND json_extract(event_json, '$.workspace_id') = workspace_id
        AND json_extract(event_json, '$.event_id') = event_id
        AND json_extract(event_json, '$.sequence') = sequence
        AND json_extract(event_json, '$.kind') = kind
        AND json_extract(event_json, '$.outcome') = outcome
        AND json_extract(event_json, '$.previous_digest') IS previous_digest
        AND json_extract(event_json, '$.event_digest') = event_digest
        AND json_extract(event_json, '$.human_id') IS human_id
        AND json_extract(event_json, '$.session_id') IS session_id
        AND json_extract(event_json, '$.challenge_id') IS challenge_id
        AND json_extract(event_json, '$.challenge_digest') IS challenge_digest
        AND json_extract(event_json, '$.session_authority_digest') IS session_authority_digest
        AND json_extract(event_json, '$.related_session_id') IS related_session_id
        AND json_extract(event_json, '$.server_instance_id') IS server_instance_id
        AND json_extract(event_json, '$.run_id') IS run_id
        AND json_extract(event_json, '$.approval_request_id') IS approval_request_id
        AND json_extract(event_json, '$.command_id') IS command_id
        AND json_extract(event_json, '$.command_kind') IS command_kind
        AND json_extract(event_json, '$.budget_id') IS budget_id
        AND json_extract(event_json, '$.reservation_id') IS reservation_id
        AND json_extract(event_json, '$.lease_id') IS lease_id
        AND json_extract(event_json, '$.source_lease_id') IS source_lease_id
        AND json_extract(event_json, '$.process_epoch_id') IS process_epoch_id
        AND json_extract(event_json, '$.permit_id') IS permit_id
        AND json_extract(event_json, '$.recovery_directive_id') IS recovery_directive_id
        AND json_extract(event_json, '$.fence_epoch') IS fence_epoch
        AND json_extract(event_json, '$.auth_epoch') IS auth_epoch
        AND json_extract(event_json, '$.policy_revision') IS policy_revision
        AND json_extract(event_json, '$.intent_digest') IS intent_digest
        AND json_extract(event_json, '$.call_digest') IS call_digest
        AND json_extract(event_json, '$.decision_digest') IS decision_digest
        AND json_extract(event_json, '$.recovery_directive_digest') IS recovery_directive_digest
        AND json_extract(event_json, '$.failure_scope') IS failure_scope
        AND json_extract(event_json, '$.proof.proof_id') IS proof_id
        AND json_extract(event_json, '$.proof.operation') IS proof_operation
        AND json_extract(event_json, '$.proof.proof_digest') IS proof_digest
        AND json_extract(event_json, '$.occurred_at') = occurred_at,
        0) = 1)
);

CREATE INDEX idx_operator_audit_run
    ON operator_audit_events(workspace_id, run_id, sequence DESC);
CREATE INDEX idx_operator_audit_approval
    ON operator_audit_events(workspace_id, approval_request_id, sequence DESC);
CREATE INDEX idx_operator_audit_human
    ON operator_audit_events(workspace_id, human_id, sequence DESC);
CREATE INDEX idx_operator_audit_kind
    ON operator_audit_events(workspace_id, kind, sequence DESC);

CREATE TABLE operator_command_receipts (
    receipt_id TEXT PRIMARY KEY
        CHECK (length(receipt_id) = 36 AND lower(receipt_id) = receipt_id),
    workspace_id TEXT NOT NULL REFERENCES operator_workspaces(workspace_id),
    command_id TEXT NOT NULL UNIQUE REFERENCES operator_commands(command_id),
    schema TEXT NOT NULL CHECK (schema = 'proof.operator.command-receipt/v1'),
    outcome TEXT NOT NULL
        CHECK (outcome IN ('applied', 'already_terminal')),
    observed_run_revision INTEGER
        CHECK (observed_run_revision BETWEEN 0 AND 9007199254740991),
    resulting_run_revision INTEGER
        CHECK (resulting_run_revision BETWEEN 0 AND 9007199254740991),
    resulting_step_revision INTEGER
        CHECK (resulting_step_revision BETWEEN 0 AND 9007199254740991),
    resulting_control_revision INTEGER
        CHECK (resulting_control_revision BETWEEN 0 AND 9007199254740991),
    resulting_fence_epoch INTEGER
        CHECK (resulting_fence_epoch BETWEEN 0 AND 9007199254740991),
    decision_id TEXT REFERENCES approval_decisions(id),
    decision_digest TEXT
        CHECK (decision_digest IS NULL OR length(decision_digest) = 64),
    proof_id TEXT REFERENCES proofs(id),
    proof_digest TEXT
        CHECK (proof_digest IS NULL OR length(proof_digest) = 64),
    audit_sequence INTEGER NOT NULL,
    completed_at TEXT NOT NULL,
    receipt_json TEXT NOT NULL CHECK (json_valid(receipt_json)),
    receipt_digest TEXT NOT NULL UNIQUE
        CHECK (length(receipt_digest) = 75
               AND receipt_digest GLOB 'blake3-256:[0-9a-f]*'),
    FOREIGN KEY (workspace_id, audit_sequence)
        REFERENCES operator_audit_events(workspace_id, sequence),
    CHECK ((decision_id IS NULL AND decision_digest IS NULL)
           OR (decision_id IS NOT NULL AND decision_digest IS NOT NULL)),
    CHECK (COALESCE((outcome = 'applied'
                     AND proof_id IS NOT NULL AND proof_digest IS NOT NULL)
           OR (outcome = 'already_terminal'
               AND proof_id IS NULL AND proof_digest IS NULL), 0) = 1)
);

CREATE INDEX idx_operator_command_receipts_page
    ON operator_command_receipts(workspace_id, audit_sequence DESC, receipt_id);

CREATE INDEX idx_agent_runs_operator_attention
    ON agent_runs(status, updated_at DESC, id);

CREATE TRIGGER operator_workspaces_no_update
BEFORE UPDATE ON operator_workspaces
BEGIN SELECT RAISE(ABORT, 'operator workspace identity is immutable'); END;
CREATE TRIGGER operator_workspaces_no_delete
BEFORE DELETE ON operator_workspaces
BEGIN SELECT RAISE(ABORT, 'operator workspace identity is immutable'); END;

CREATE TRIGGER operator_human_enrollments_no_update
BEFORE UPDATE ON operator_human_enrollments
BEGIN SELECT RAISE(ABORT, 'operator Human enrollment is immutable'); END;
CREATE TRIGGER operator_human_enrollments_no_delete
BEFORE DELETE ON operator_human_enrollments
BEGIN SELECT RAISE(ABORT, 'operator Human enrollment is immutable'); END;

CREATE TRIGGER operator_approval_bindings_no_update
BEFORE UPDATE ON operator_approval_bindings
BEGIN SELECT RAISE(ABORT, 'operator approval binding is immutable'); END;
CREATE TRIGGER operator_approval_bindings_no_delete
BEFORE DELETE ON operator_approval_bindings
BEGIN SELECT RAISE(ABORT, 'operator approval binding is immutable'); END;

CREATE TRIGGER operator_recovery_directives_no_update
BEFORE UPDATE ON operator_recovery_directives
BEGIN SELECT RAISE(ABORT, 'operator recovery directive is immutable'); END;
CREATE TRIGGER operator_recovery_directives_no_delete
BEFORE DELETE ON operator_recovery_directives
BEGIN SELECT RAISE(ABORT, 'operator recovery directive is immutable'); END;

CREATE TRIGGER operator_replay_bindings_no_update
BEFORE UPDATE ON operator_replay_bindings
BEGIN SELECT RAISE(ABORT, 'operator replay binding is immutable'); END;
CREATE TRIGGER operator_replay_bindings_no_delete
BEFORE DELETE ON operator_replay_bindings
BEGIN SELECT RAISE(ABORT, 'operator replay binding is immutable'); END;

CREATE TRIGGER operator_run_projections_no_update
BEFORE UPDATE ON operator_run_projections
BEGIN SELECT RAISE(ABORT, 'operator run projection is append-only'); END;
CREATE TRIGGER operator_run_projections_no_delete
BEFORE DELETE ON operator_run_projections
BEGIN SELECT RAISE(ABORT, 'operator run projection is append-only'); END;

CREATE TRIGGER operator_commands_no_update
BEFORE UPDATE ON operator_commands
BEGIN SELECT RAISE(ABORT, 'operator command is immutable'); END;
CREATE TRIGGER operator_commands_no_delete
BEFORE DELETE ON operator_commands
BEGIN SELECT RAISE(ABORT, 'operator command is immutable'); END;

CREATE TRIGGER operator_audit_events_no_update
BEFORE UPDATE ON operator_audit_events
BEGIN SELECT RAISE(ABORT, 'operator audit event is append-only'); END;
CREATE TRIGGER operator_audit_events_no_delete
BEFORE DELETE ON operator_audit_events
BEGIN SELECT RAISE(ABORT, 'operator audit event is append-only'); END;

CREATE TRIGGER operator_command_receipts_no_update
BEFORE UPDATE ON operator_command_receipts
BEGIN SELECT RAISE(ABORT, 'operator command receipt is immutable'); END;
CREATE TRIGGER operator_command_receipts_no_delete
BEFORE DELETE ON operator_command_receipts
BEGIN SELECT RAISE(ABORT, 'operator command receipt is immutable'); END;

CREATE TRIGGER operator_linked_proofs_no_update
BEFORE UPDATE ON proofs
WHEN EXISTS (
    SELECT 1 FROM operator_command_receipts WHERE proof_id = OLD.id
    UNION ALL
    SELECT 1 FROM operator_audit_events WHERE proof_id = OLD.id
)
BEGIN SELECT RAISE(ABORT, 'operator-linked proof is immutable'); END;
CREATE TRIGGER operator_linked_proofs_no_delete
BEFORE DELETE ON proofs
WHEN EXISTS (
    SELECT 1 FROM operator_command_receipts WHERE proof_id = OLD.id
    UNION ALL
    SELECT 1 FROM operator_audit_events WHERE proof_id = OLD.id
)
BEGIN SELECT RAISE(ABORT, 'operator-linked proof is immutable'); END;
