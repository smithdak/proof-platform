use jsonschema::Validator;
use proof_conformance::load_case;
use proof_kernel::RegistryEntry;
use serde::Deserialize;
use serde_json::{Map, Value};
use std::path::Path;

#[derive(Deserialize)]
struct CaseFile {
    cases: Vec<BenchmarkCase>,
}

#[derive(Deserialize)]
struct BenchmarkCase {
    name: String,
    operation: String,
    version: String,
    governance: String,
    consequence: String,
    idempotency: String,
    max_duration_ms: u64,
    input: Value,
    output_success_criteria: Value,
    invalid_inputs: Vec<InvalidInputCase>,
    #[serde(default)]
    validate_format: bool,
}

#[derive(Deserialize)]
struct InvalidInputCase {
    name: String,
    input: Value,
    error: String,
}

fn registry() -> Vec<RegistryEntry> {
    let directory = proof_conformance::project_root().join("registry/commerce");
    std::fs::read_dir(directory)
        .unwrap()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension().and_then(|extension| extension.to_str()) == Some("json")
                && !path.to_string_lossy().ends_with(".input.json")
                && !path.to_string_lossy().ends_with(".output.json")
        })
        .map(|path| serde_json::from_slice::<RegistryEntry>(&std::fs::read(path).unwrap()).unwrap())
        .collect()
}

fn resolve_input(input: &Value, previous_outputs: &Map<String, Value>) -> Value {
    match input {
        Value::String(value) => {
            let Some(path) = value.strip_prefix('$') else {
                return input.clone();
            };
            let mut current = previous_outputs;
            let mut segments = path.split(':');
            let Some(source) = segments.next() else {
                return input.clone();
            };
            let mut pointer = String::new();
            for segment in segments {
                if segment != "data" {
                    pointer.push('/');
                    pointer.push_str(segment);
                }
            }
            current
                .get(source)
                .and_then(|output| output.pointer(&pointer))
                .cloned()
                .unwrap_or_else(|| input.clone())
        }
        Value::Object(object) => Value::Object(
            object
                .iter()
                .map(|(key, value)| (key.clone(), resolve_input(value, previous_outputs)))
                .collect(),
        ),
        Value::Array(values) => Value::Array(
            values
                .iter()
                .map(|value| resolve_input(value, previous_outputs))
                .collect(),
        ),
        _ => input.clone(),
    }
}

fn assert_valid_input_schema(schema: &Value, input: &Value, operation: &str) {
    let validator = Validator::new(schema).unwrap();
    assert!(
        validator.is_valid(input),
        "input schema failure for {operation}: {input}"
    );
}

fn assert_additional_properties(schema: &Value, input: &Value, case: &str) {
    let Value::Object(object) = input else {
        return;
    };
    let properties = schema
        .get("properties")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    if schema.get("additionalProperties").and_then(Value::as_bool) == Some(false) {
        for key in object.keys() {
            assert!(
                properties.contains_key(key),
                "unknown property `{key}` in {case}"
            );
        }
    }
    for (key, value) in object {
        if let Some(child_schema) = properties.get(key) {
            if child_schema
                .get("additionalProperties")
                .and_then(Value::as_bool)
                == Some(false)
            {
                assert_additional_properties(child_schema, value, case);
            }
        }
    }
}

fn assert_valid_output_schema(schema: &Value, output: &Value, operation: &str) {
    let validator = Validator::new(schema).unwrap();
    assert!(
        validator.is_valid(output),
        "output schema failure for {operation}: {output}"
    );
}

fn assert_success_criteria(criteria: &Value, output: &Value, operation: &str) {
    let validator = Validator::new(criteria).unwrap();
    assert!(
        validator.is_valid(output),
        "benchmark success criteria failure for {operation}: {output}"
    );
}

fn assert_expected_error(actual: &str, expected: &str, case: &str) {
    let expected_parts = expected.split_whitespace().collect::<Vec<_>>();
    assert!(
        expected_parts.iter().all(|part| actual.contains(part)),
        "error mismatch for {case}: expected `{expected}`, got `{actual}`"
    );
}

fn schema_path(schema: &str) -> std::path::PathBuf {
    proof_conformance::project_root()
        .join("registry")
        .join(schema)
}

fn required_fields(document: &Value, pointer: &str) -> Vec<String> {
    document
        .pointer(pointer)
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .map(ToOwned::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn commerce_benchmark_golden_cases_cover_all_operations() {
    let registry = registry();
    let file: CaseFile =
        serde_json::from_value(load_case("cases/commerce_benchmarks.json").unwrap()).unwrap();
    assert_eq!(file.cases.len(), 5);

    for case in file.cases {
        let entry = registry
            .iter()
            .find(|entry| entry.operation == case.operation && entry.version == case.version)
            .unwrap_or_else(|| panic!("registry entry missing for {}", case.name));
        assert!(case.max_duration_ms > 0, "threshold must be positive");

        assert_eq!(
            serde_json::to_value(entry.governance).unwrap(),
            serde_json::json!(case.governance)
        );
        assert_eq!(entry.consequence, case.consequence);
        assert_eq!(entry.idempotency, case.idempotency);

        let input_schema = load_case(schema_path(&entry.input_schema)).unwrap();
        let output_schema = load_case(schema_path(&entry.output_schema)).unwrap();
        assert_valid_input_schema(&input_schema, &case.input, &case.operation);

        let validator = Validator::new(&case.output_success_criteria).unwrap();
        assert!(
            validator.is_valid(&Value::Null) == false,
            "success criteria must be a valid schema"
        );

        let criteria_operation = case
            .output_success_criteria
            .pointer("/properties/operation/const")
            .and_then(Value::as_str)
            .unwrap_or_default();
        assert_eq!(criteria_operation, case.operation);
        let criteria_data_required =
            required_fields(&case.output_success_criteria, "/properties/data/required");
        let schema_data_required = required_fields(&output_schema, "/properties/data/required");
        assert_eq!(criteria_data_required, schema_data_required);

        for invalid in case.invalid_inputs {
            let validator = Validator::new(&input_schema).unwrap();
            assert!(
                !(if case.validate_format {
                    let properties = input_schema
                        .get("properties")
                        .and_then(Value::as_object)
                        .cloned()
                        .unwrap_or_default();
                    let mut formatted = input_schema.clone();
                    if let Some(Value::Object(properties)) = formatted.get_mut("properties") {
                        for (key, property) in properties {
                            if property.get("format").and_then(Value::as_str) == Some("uuid") {
                                property["format"] = serde_json::json!("uuid");
                            }
                        }
                    }
                    Validator::options()
                        .should_validate_formats(true)
                        .build(&formatted)
                        .unwrap()
                        .is_valid(&invalid.input)
                } else {
                    validator.is_valid(&invalid.input)
                }),
                "invalid input should fail schema: {}",
                invalid.name
            );
            assert!(!invalid.error.trim().is_empty());
        }
    }
}
