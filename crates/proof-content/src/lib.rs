pub mod changeset;
pub mod digest;
pub mod edition;
pub mod error;
pub mod object;
pub mod principal;
pub mod release;
pub mod schema;

pub use changeset::{
    BaseState, ChangeSet, ChangeSetEdit, ChangeSetStatus, ObjectCreateEdit, ObjectDeleteEdit,
    ObjectUpdateEdit,
};
pub use edition::Edition;
pub use error::ContentError;
pub use object::{Object, ObjectStatus};
pub use principal::PrincipalId;
pub use release::Release;
pub use schema::{FieldType, SchemaDefinition, SchemaField};

#[cfg(test)]
mod digest_tests {
    use serde_json::json;

    #[test]
    fn digest_is_stable_and_key_order_insensitive() {
        let first = json!({"b": 2, "a": {"d": 4, "c": 3}});
        let second = json!({"a": {"c": 3, "d": 4}, "b": 2});
        assert_eq!(
            super::digest::canonical_digest(&first),
            super::digest::canonical_digest(&second)
        );
    }
}
