use proof_conformance::{expected_digest, load_case, project_root, DigestCase, ScopeCase};
use proof_kernel::delegation::DelegationScope;
use serde::Deserialize;
use std::path::PathBuf;

#[derive(Deserialize)]
struct CaseFile<T> {
    cases: Vec<T>,
}

fn case_path(name: &str) -> PathBuf {
    project_root().join("conformance").join("cases").join(name)
}

#[test]
fn canonical_digest_cases_match_pinned_values() {
    let file: CaseFile<DigestCase> =
        serde_json::from_value(load_case(case_path("canonical_digests.json")).unwrap()).unwrap();
    assert!(!file.cases.is_empty());

    for case in file.cases {
        let actual = expected_digest(&case.input, &case.artifact_kind).unwrap();
        assert_eq!(
            actual, case.expected_digest,
            "digest drift in case `{}`",
            case.name
        );
    }
}

#[test]
fn delegation_scope_cases_match_authorization_matrix() {
    let file: CaseFile<ScopeCase> =
        serde_json::from_value(load_case(case_path("delegation_scope_matrix.json")).unwrap())
            .unwrap();
    assert!(file.cases.len() >= 6);

    for case in file.cases {
        let scope = DelegationScope::from(&case);
        assert_eq!(
            scope.scope_allows_operation(&case.operation, &case.domain),
            case.expected_allowed,
            "scope mismatch in case `{}`",
            case.name
        );
    }
}
