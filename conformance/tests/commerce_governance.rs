use proof_conformance::load_case;
use proof_kernel::{Governance, RegistryEntry};
use serde::Deserialize;
use serde_json::Value;

#[derive(Deserialize)]
struct CaseFile {
    cases: Vec<GovernanceCase>,
}

#[derive(Deserialize)]
struct GovernanceCase {
    name: String,
    operation: String,
    version: String,
    domain: String,
    governance: String,
    #[serde(default)]
    idempotency: Option<String>,
    #[serde(default)]
    sequence: Option<Vec<String>>,
    #[serde(default)]
    before_sequence: Option<Value>,
    #[serde(default)]
    after_sequence: Option<Value>,
    #[serde(default)]
    without_key: Option<Value>,
    #[serde(default)]
    duplicate_with_same_key: Option<Value>,
    #[serde(default)]
    with_new_key: Option<Value>,
}

fn registry() -> Vec<RegistryEntry> {
    let directory = proof_conformance::project_root().join("registry/commerce");
    [
        "catalog-create",
        "catalog-update",
        "order-create",
        "order-approve",
        "order-fulfill",
    ]
    .iter()
    .map(|name| directory.join(format!("{name}.json")))
    .map(|path| {
        serde_json::from_slice::<RegistryEntry>(&std::fs::read(&path).unwrap())
            .unwrap_or_else(|error| panic!("failed to load {}: {error}", path.display()))
    })
    .collect()
}

fn assert_rejection(reason: Option<&Value>, case: &GovernanceCase) {
    let reason = reason.expect("rejection contract");
    assert!(
        reason.get("error").is_some_and(Value::is_string),
        "rejection must name error"
    );
    assert!(
        reason
            .get("message_contains")
            .and_then(Value::as_str)
            .map(str::trim)
            .is_some_and(|message| !message.is_empty()),
        "rejection must assert user-facing message"
    );
    assert!(reason.get("expected_status").is_none());
    assert!(reason.get("expected_allowed").is_none());
}

fn assert_acceptance(reason: Option<&Value>, case: &GovernanceCase) {
    let reason = reason.expect("acceptance contract");
    assert_eq!(
        reason.get("expected_allowed").and_then(Value::as_bool),
        Some(true),
        "acceptance must be asserted for {}",
        case.name
    );
}

#[test]
fn commerce_governance_cases_match_registry_contract() {
    let registry = registry();
    let file: CaseFile =
        serde_json::from_value(load_case("cases/commerce_governance.json").unwrap()).unwrap();
    assert!(file.cases.len() >= 3);

    for case in file.cases {
        let entry = registry
            .iter()
            .find(|entry| entry.operation == case.operation && entry.version == case.version)
            .unwrap_or_else(|| panic!("missing registry entry for {}", case.name));
        assert_eq!(entry.domain, case.domain);
        assert_eq!(
            serde_json::to_value(entry.governance).unwrap(),
            serde_json::json!(case.governance)
        );

        if let Some(idempotency) = &case.idempotency {
            assert_eq!(entry.idempotency, *idempotency);
            assert!(case.without_key.is_some());
            assert!(case.duplicate_with_same_key.is_some());
            assert!(case.with_new_key.is_some());
            assert_rejection(
                case.without_key
                    .as_ref()
                    .and_then(|value| value.get("expected_rejection")),
                &case,
            );
            assert_rejection(
                case.duplicate_with_same_key
                    .as_ref()
                    .and_then(|value| value.get("expected_rejection")),
                &case,
            );
            assert_acceptance(case.with_new_key.as_ref(), &case);
        }

        if entry.governance == Governance::HumanOnly {
            assert!(case.operation.starts_with("order."));
            assert!(case.sequence.is_none());
        }

        if let Some(sequence) = &case.sequence {
            assert_eq!(
                sequence,
                &["order.create", "order.approve", "order.fulfill"]
            );
            let before = case.before_sequence.as_ref().expect("before sequence");
            let after = case.after_sequence.as_ref().expect("after sequence");
            assert!(before.get("expected_rejection").is_some());
            assert_eq!(
                after.get("expected_status").and_then(Value::as_str),
                Some("fulfilled")
            );
        }
    }
}
