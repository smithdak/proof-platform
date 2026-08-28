use proof_conformance::load_case;
use proof_kernel::{Proof, ProofError};
use serde::Deserialize;

#[derive(Deserialize)]
struct ProofCase {
    proof: Proof,
    public_key: String,
    tamper_field: String,
    tamper_value: String,
    expect_valid: bool,
    expect_tamper_invalid: bool,
}

fn decode_hex(value: &str) -> Result<Vec<u8>, String> {
    if value.len() % 2 != 0 {
        return Err("odd-length hex".to_string());
    }
    (0..value.len())
        .step_by(2)
        .map(|index| {
            u8::from_str_radix(&value[index..index + 2], 16).map_err(|error| error.to_string())
        })
        .collect()
}

#[test]
fn signed_proof_is_valid_and_tampering_fails() {
    let case: ProofCase =
        serde_json::from_value(load_case("../conformance/cases/proof_sign_verify.json").unwrap())
            .unwrap();
    assert!(case.expect_valid);
    assert!(case.expect_tamper_invalid);

    let public_key_bytes = decode_hex(&case.public_key).unwrap();
    let public_key =
        ed25519_dalek::VerifyingKey::from_bytes(&public_key_bytes.try_into().unwrap()).unwrap();
    assert_eq!(case.proof.verify(&public_key), Ok(()));

    let mut tampered = case.proof.clone();
    match case.tamper_field.as_str() {
        "operation" => tampered.body.operation = case.tamper_value,
        unknown => panic!("unsupported tamper field: {unknown}"),
    }
    assert_eq!(
        tampered.verify(&public_key),
        Err(ProofError::InvalidSignature)
    );
}
