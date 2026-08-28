use proof_content::{
    BaseState, ChangeSet, ChangeSetEdit, FieldType, Object, ObjectCreateEdit, ObjectDeleteEdit,
    ObjectStatus, ObjectUpdateEdit, PrincipalId, Release, SchemaDefinition, SchemaField,
};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use uuid::Uuid;

fn field(name: &str, field_type: FieldType, required: bool) -> SchemaField {
    SchemaField {
        name: name.to_string(),
        field_type,
        required,
        localized: true,
        default_value: None,
    }
}

fn schema() -> SchemaDefinition {
    SchemaDefinition::new(
        "Article",
        1,
        vec![
            field("title", FieldType::Text, true),
            field("body", FieldType::RichText, false),
            field("views", FieldType::Number, false),
        ],
    )
}

fn content(title: &str) -> Value {
    json!({"title": title, "body": "Hello"})
}

fn base_state() -> (SchemaDefinition, BaseState, Object) {
    let schema = schema();
    let object = Object::create(&schema, "en-US", content("Original")).unwrap();
    let mut state = BTreeMap::new();
    state.insert(object.id, object.clone());
    (schema, state, object)
}

#[test]
fn schema_validates_required_fields_and_types() {
    let schema = schema();
    schema.validate_object(&content("Valid")).unwrap();

    assert!(schema
        .validate_object(&json!({"body": "Missing title"}))
        .is_err());
    assert!(schema
        .validate_object(&json!({"title": 12, "body": "Bad title"}))
        .is_err());
    assert!(schema
        .validate_object(&json!({"title": "Valid", "unknown": true}))
        .is_err());
    assert!(schema
        .validate_object(&Value::String("not object".into()))
        .is_err());
}

#[test]
fn object_lifecycle_has_exact_allowed_transitions() {
    let schema = schema();
    let mut object = Object::create(&schema, "en-US", content("Lifecycle")).unwrap();
    assert_eq!(object.status(), ObjectStatus::Draft);

    object.transition_to(ObjectStatus::Submitted).unwrap();
    object.transition_to(ObjectStatus::Approved).unwrap();
    object.transition_to(ObjectStatus::Committed).unwrap();
    object.transition_to(ObjectStatus::Published).unwrap();
    object.transition_to(ObjectStatus::Draft).unwrap();
    assert_eq!(object.revision, 2);

    let mut invalid = Object::create(&schema, "en-US", content("Invalid")).unwrap();
    assert!(invalid.transition_to(ObjectStatus::Approved).is_err());
    assert!(invalid.transition_to(ObjectStatus::Published).is_err());
}

#[test]
fn object_updates_are_schema_bound_and_versioned() {
    let schema = schema();
    let mut object = Object::create(&schema, "en-US", content("Original")).unwrap();
    object.update_content(&schema, content("Updated")).unwrap();
    assert_eq!(object.revision, 2);
    assert!(object.update_content(&schema, json!({"title": 5})).is_err());
    assert_eq!(object.revision, 2);

    let other = SchemaDefinition::new("Article", 2, schema.fields.clone());
    assert!(object
        .update_content(&other, content("Wrong schema"))
        .is_err());

    object.transition_to(ObjectStatus::Submitted).unwrap();
    assert!(object
        .update_content(&schema, content("Draft only"))
        .is_err());
}

#[test]
fn edition_is_deterministic_and_immutable_snapshot() {
    let (schema, _, _) = base_state();
    let first = Object::create(&schema, "en-US", content("One")).unwrap();
    let second = Object::create(&schema, "en-US", content("Two")).unwrap();
    let changeset_id = Uuid::now_v7();

    let edition = proof_content::Edition::new(changeset_id, vec![first.clone(), second.clone()]);
    assert_eq!(edition.objects.len(), 2);
    assert_eq!(edition.changeset_id, changeset_id);

    let reordered = proof_content::Edition::new(changeset_id, vec![second.clone(), first.clone()]);
    assert_eq!(edition.content_digest, reordered.content_digest);
    assert_eq!(edition.object(first.id).unwrap().content, first.content);

    let mut mutated = edition.clone();
    mutated.objects[0].content = json!({"title": "Mutated"});
    assert_ne!(mutated.objects, edition.objects);
    assert_ne!(
        proof_content::Edition::new(changeset_id, mutated.objects.clone()).content_digest,
        edition.content_digest
    );
}

#[test]
fn changeset_commit_applies_create_update_and_delete_atomically() {
    let (schema, mut state, existing) = base_state();
    let created = Object::create(&schema, "en-US", content("Created")).unwrap();
    let created_id = created.id;

    let mut changeset = ChangeSet::new(
        "Replace article content",
        &state,
        vec![
            ChangeSetEdit::ObjectCreate(ObjectCreateEdit { object: created }),
            ChangeSetEdit::ObjectUpdate(ObjectUpdateEdit {
                object_id: existing.id,
                expected_revision: existing.revision,
                content: content("Updated"),
            }),
            ChangeSetEdit::ObjectDelete(ObjectDeleteEdit {
                object_id: existing.id,
                expected_revision: existing.revision + 1,
            }),
        ],
    );
    changeset
        .transition_to(proof_content::ChangeSetStatus::Submitted)
        .unwrap();
    changeset
        .transition_to(proof_content::ChangeSetStatus::Approved)
        .unwrap();
    let next = changeset.commit(&[schema.clone()], &mut state).unwrap();
    assert!(!state.contains_key(&existing.id));
    assert!(state.contains_key(&created_id));
    assert_eq!(
        next.get(&created_id).unwrap().status(),
        ObjectStatus::Committed
    );
}

#[test]
fn changeset_validation_catches_every_invalid_edit() {
    let (schema, state, existing) = base_state();
    let duplicate = existing.clone();
    let ghost = Uuid::now_v7();

    let cases = vec![
        vec![ChangeSetEdit::ObjectCreate(ObjectCreateEdit {
            object: duplicate,
        })],
        vec![ChangeSetEdit::ObjectUpdate(ObjectUpdateEdit {
            object_id: existing.id,
            expected_revision: existing.revision + 1,
            content: content("Conflict"),
        })],
        vec![ChangeSetEdit::ObjectUpdate(ObjectUpdateEdit {
            object_id: ghost,
            expected_revision: 1,
            content: content("Ghost"),
        })],
        vec![ChangeSetEdit::ObjectDelete(ObjectDeleteEdit {
            object_id: ghost,
            expected_revision: 1,
        })],
        vec![],
    ];

    for edits in cases {
        let changeset = ChangeSet::new("invalid", &state, edits);
        assert!(changeset.validate(&[schema.clone()], &state).is_err());
    }

    let changed_state = state.clone();
    let changeset = ChangeSet::new(
        "stale base",
        &state,
        vec![ChangeSetEdit::ObjectDelete(ObjectDeleteEdit {
            object_id: existing.id,
            expected_revision: existing.revision,
        })],
    );
    let mut stale = changed_state;
    stale.clear();
    assert!(changeset.validate(&[schema], &stale).is_err());
}

#[test]
fn changeset_requires_governed_status_and_fails_closed() {
    let (schema, mut state, existing) = base_state();
    let changeset = ChangeSet::new(
        "unapproved",
        &state,
        vec![ChangeSetEdit::ObjectUpdate(ObjectUpdateEdit {
            object_id: existing.id,
            expected_revision: existing.revision,
            content: content("Unapproved"),
        })],
    );
    assert!(changeset.commit(&[schema], &mut state).is_err());
    assert_eq!(
        state.get(&existing.id).unwrap().content,
        content("Original")
    );
}

#[test]
fn release_binds_immutable_edition_to_environment() {
    let (schema, _, _) = base_state();
    let object = Object::create(&schema, "en-US", content("Released")).unwrap();
    let edition = proof_content::Edition::new(Uuid::now_v7(), vec![object]);
    let release = Release::new(edition.id, "production", PrincipalId::new());
    assert_eq!(release.edition_id, edition.id);
    assert_eq!(release.environment, "production");
}

#[test]
fn domain_types_round_trip_through_json() {
    let schema = schema();
    let object = Object::create(&schema, "en-US", content("Serializable")).unwrap();
    let mut state = BTreeMap::new();
    state.insert(object.id, object.clone());
    let mut changeset = ChangeSet::new(
        "round trip",
        &state,
        vec![ChangeSetEdit::ObjectUpdate(ObjectUpdateEdit {
            object_id: object.id,
            expected_revision: object.revision,
            content: content("Updated"),
        })],
    );
    changeset
        .transition_to(proof_content::ChangeSetStatus::Submitted)
        .unwrap();
    changeset
        .transition_to(proof_content::ChangeSetStatus::Approved)
        .unwrap();

    let encoded = serde_json::to_string(&changeset).unwrap();
    let decoded: ChangeSet = serde_json::from_str(&encoded).unwrap();
    assert_eq!(decoded, changeset);

    let encoded_object = serde_json::to_string(&object).unwrap();
    let decoded_object: Object = serde_json::from_str(&encoded_object).unwrap();
    assert_eq!(decoded_object, object);
    assert_eq!(decoded_object.status(), ObjectStatus::Draft);
}

#[test]
fn schema_defaults_are_type_checked() {
    let fields = vec![
        SchemaField {
            name: "title".into(),
            field_type: FieldType::Text,
            required: true,
            localized: false,
            default_value: Some(json!("Default")),
        },
        SchemaField {
            name: "views".into(),
            field_type: FieldType::Number,
            required: false,
            localized: false,
            default_value: Some(json!(5)),
        },
    ];
    let valid = SchemaDefinition::new("Article", 1, fields);
    valid.validate().unwrap();

    let invalid = SchemaDefinition::new(
        "Article",
        1,
        vec![SchemaField {
            name: "views".into(),
            field_type: FieldType::Number,
            required: false,
            localized: false,
            default_value: Some(json!("not number")),
        }],
    );
    assert!(invalid.validate().is_err());
}
