use std::{borrow::Cow, collections::HashMap};

use topcoat::validate::{
    Number, Schema, Value,
    descriptor::{FieldDescriptor, FieldType, SchemaDescriptor, ValidatorDescriptor},
};

#[derive(Debug, Schema, PartialEq)]
struct SignUp {
    #[schema(email, max_length = 254)]
    email: String,

    #[schema(min_length = 8)]
    password: String,

    #[schema(min = 13)]
    age: u32,

    #[schema(default = false)]
    newsletter: bool,
}

#[test]
fn validate_from_form_pairs() {
    let data = vec![
        ("email".to_string(), "user@example.com".to_string()),
        ("password".to_string(), "secret123".to_string()),
        ("age".to_string(), "25".to_string()),
        ("newsletter".to_string(), "on".to_string()),
    ];

    let user = SignUp::validate(&data).unwrap();
    assert_eq!(
        user,
        SignUp {
            email: "user@example.com".to_string(),
            password: "secret123".to_string(),
            age: 25,
            newsletter: true,
        }
    );
}

#[test]
fn validate_from_json() {
    let data = serde_json::json!({
        "email": "user@example.com",
        "password": "secret123",
        "age": 25,
        "newsletter": true,
    });

    let user = SignUp::validate(&data).unwrap();
    assert_eq!(user.email, "user@example.com");
    assert_eq!(user.age, 25);
    assert!(user.newsletter);
}

#[test]
fn validation_errors_reported_per_field() {
    let data: Vec<(String, String)> = vec![
        ("email".to_string(), "not-an-email".to_string()),
        ("password".to_string(), "short".to_string()),
        ("age".to_string(), "10".to_string()),
        ("newsletter".to_string(), "on".to_string()),
    ];

    let errors = SignUp::validate(&data).unwrap_err();
    assert!(!errors.is_empty());
    assert_eq!(errors.get("email").unwrap().code(), "email");
    assert_eq!(errors.get("password").unwrap().code(), "min_length");
    assert_eq!(errors.get("age").unwrap().code(), "min");
    assert!(errors.get("newsletter").is_none());
}

#[test]
fn missing_required_field() {
    let data: Vec<(String, String)> = vec![];
    let errors = SignUp::validate(&data).unwrap_err();
    assert_eq!(errors.get("email").unwrap().code(), "required");
    assert_eq!(errors.get("password").unwrap().code(), "required");
    assert_eq!(errors.get("age").unwrap().code(), "required");
    assert!(errors.get("newsletter").is_none());
}

#[test]
fn default_field_falls_back_when_missing() {
    let data = vec![
        ("email".to_string(), "user@example.com".to_string()),
        ("password".to_string(), "secret123".to_string()),
        ("age".to_string(), "25".to_string()),
    ];

    let user = SignUp::validate(&data).unwrap();
    assert!(!user.newsletter);
}

#[test]
fn default_field_not_used_when_present() {
    let data = vec![
        ("email".to_string(), "user@example.com".to_string()),
        ("password".to_string(), "secret123".to_string()),
        ("age".to_string(), "25".to_string()),
        ("newsletter".to_string(), "on".to_string()),
    ];

    let user = SignUp::validate(&data).unwrap();
    assert!(user.newsletter);
}

#[test]
fn rename_attr() {
    #[derive(Debug, Schema, PartialEq)]
    struct Renamed {
        #[schema(rename = "emailAddress", email)]
        email_address: String,
    }

    let data = HashMap::from([("emailAddress".to_string(), "a@b.com".to_string())]);
    let value = Renamed::validate(&data).unwrap();
    assert_eq!(value.email_address, "a@b.com");

    let missing = HashMap::<String, String>::new();
    let errors = Renamed::validate(&missing).unwrap_err();
    assert_eq!(errors.get("emailAddress").unwrap().code(), "required");
}

#[test]
fn trim_and_range_validators() {
    #[derive(Debug, Schema, PartialEq)]
    struct Trimmed {
        #[schema(trim, min_length = 2)]
        name: String,

        #[schema(range(min = 1, max = 10))]
        count: u32,
    }

    let data = vec![
        ("name".to_string(), "  xx  ".to_string()),
        ("count".to_string(), "5".to_string()),
    ];
    let value = Trimmed::validate(&data).unwrap();
    assert_eq!(value.name, "xx");
    assert_eq!(value.count, 5);

    let bad = vec![
        ("name".to_string(), " x ".to_string()),
        ("count".to_string(), "15".to_string()),
    ];
    let errors = Trimmed::validate(&bad).unwrap_err();
    assert_eq!(errors.get("name").unwrap().code(), "min_length");
    assert_eq!(errors.get("count").unwrap().code(), "range");
}

#[test]
fn one_of_and_regex_validators() {
    #[derive(Debug, Schema, PartialEq)]
    struct Choices {
        #[schema(one_of = "red, green, blue")]
        color: String,

        #[schema(regex = r"^[a-z]+$")]
        slug: String,
    }

    let data = HashMap::from([
        ("color".to_string(), "green".to_string()),
        ("slug".to_string(), "abc".to_string()),
    ]);
    let value = Choices::validate(&data).unwrap();
    assert_eq!(value.color, "green");

    let bad = HashMap::from([
        ("color".to_string(), "yellow".to_string()),
        ("slug".to_string(), "ABC".to_string()),
    ]);
    let errors = Choices::validate(&bad).unwrap_err();
    assert_eq!(errors.get("color").unwrap().code(), "one_of");
    assert_eq!(errors.get("slug").unwrap().code(), "regex");
}

#[test]
fn custom_validator() {
    use std::borrow::Cow;

    use topcoat::validate::{ValidationError, validator::CustomValidator};

    struct StartsWithA;

    impl CustomValidator for StartsWithA {
        fn validate(value: &Value) -> Result<Value, ValidationError> {
            match value.as_str() {
                Some(s) if s.starts_with('a') => Ok(value.clone()),
                _ => Err(ValidationError::with_code(
                    "starts_with_a",
                    "Must start with a",
                )),
            }
        }

        fn name() -> &'static str {
            "starts_with_a"
        }

        fn message() -> Cow<'static, str> {
            Cow::Borrowed("Must start with the letter a")
        }
    }

    #[derive(Debug, Schema, PartialEq)]
    struct Custom {
        #[schema(custom = StartsWithA)]
        name: String,
    }

    let data = HashMap::from([("name".to_string(), "alice".to_string())]);
    assert_eq!(Custom::validate(&data).unwrap().name, "alice");

    let bad = HashMap::from([("name".to_string(), "bob".to_string())]);
    let errors = Custom::validate(&bad).unwrap_err();
    assert_eq!(errors.get("name").unwrap().code(), "starts_with_a");
}

#[test]
fn fields_named_like_codegen_internals() {
    #[derive(Debug, Schema, PartialEq)]
    struct Collides {
        data: String,
        #[schema(min_length = 2)]
        errors: String,
        raw: Option<String>,
        coerced: u32,
        value: bool,
    }

    let data = vec![
        ("data".to_string(), "payload".to_string()),
        ("errors".to_string(), "none".to_string()),
        ("coerced".to_string(), "42".to_string()),
        ("value".to_string(), "true".to_string()),
    ];
    let value = Collides::validate(&data).unwrap();
    assert_eq!(
        value,
        Collides {
            data: "payload".to_string(),
            errors: "none".to_string(),
            raw: None,
            coerced: 42,
            value: true,
        }
    );

    let bad = vec![
        ("data".to_string(), "payload".to_string()),
        ("errors".to_string(), "x".to_string()),
        ("coerced".to_string(), "42".to_string()),
        ("value".to_string(), "true".to_string()),
    ];
    let errors = Collides::validate(&bad).unwrap_err();
    assert_eq!(errors.get("errors").unwrap().code(), "min_length");
}

#[test]
fn validators_run_in_declared_order() {
    use std::borrow::Cow;

    use topcoat::validate::{ValidationError, Value, validator::CustomValidator};

    struct AlwaysFails;
    impl CustomValidator for AlwaysFails {
        fn validate(_value: &Value) -> Result<Value, ValidationError> {
            Err(ValidationError::with_code("always_fails", "custom failed"))
        }
        fn name() -> &'static str {
            "always_fails"
        }
        fn message() -> Cow<'static, str> {
            Cow::Borrowed("custom failed")
        }
    }

    #[derive(Debug, Schema)]
    #[allow(dead_code)]
    struct MinLengthFirst {
        #[schema(min_length = 10, custom = AlwaysFails)]
        name: String,
    }

    #[derive(Debug, Schema)]
    #[allow(dead_code)]
    struct CustomFirst {
        #[schema(custom = AlwaysFails, min_length = 10)]
        name: String,
    }

    // "abc" fails both validators; the declared-first one wins.
    let data = HashMap::from([("name".to_string(), "abc".to_string())]);
    let errors = MinLengthFirst::validate(&data).unwrap_err();
    assert_eq!(errors.get("name").unwrap().code(), "min_length");

    let errors = CustomFirst::validate(&data).unwrap_err();
    assert_eq!(errors.get("name").unwrap().code(), "always_fails");
}

#[test]
fn unsupported_type_rejected_at_compile_time() {
    // This test is deliberately commented out: the macro should fail to compile
    // for unsupported types. It is left as documentation of the expected error.
    //
    // #[derive(Schema)]
    // struct Bad {
    //     unsupported: Vec<u8>,
    // }
}

#[test]
fn option_string_empty_becomes_none() {
    #[derive(Debug, Schema, PartialEq)]
    struct Optional {
        name: Option<String>,
    }

    let data = HashMap::from([("name".to_string(), String::new())]);
    let value = Optional::validate(&data).unwrap();
    assert_eq!(value.name, None);
}

#[test]
fn option_string_present_becomes_some() {
    #[derive(Debug, Schema, PartialEq)]
    struct Optional {
        name: Option<String>,
    }

    let data = HashMap::from([("name".to_string(), "Alice".to_string())]);
    let value = Optional::validate(&data).unwrap();
    assert_eq!(value.name, Some("Alice".to_string()));
}

#[test]
fn option_number_from_json() {
    #[derive(Debug, Schema, PartialEq)]
    struct Optional {
        age: Option<u32>,
    }

    let data = serde_json::json!({ "age": 30 });
    let value = Optional::validate(&data).unwrap();
    assert_eq!(value.age, Some(30));

    let missing = serde_json::json!({});
    let value = Optional::validate(&missing).unwrap();
    assert_eq!(value.age, None);
}

#[test]
fn float_fields_from_form_pairs_and_json() {
    #[derive(Debug, Schema, PartialEq)]
    struct Measurements {
        temperature: f32,
        ratio: f64,
    }

    let data = vec![
        ("temperature".to_string(), "36.5".to_string()),
        ("ratio".to_string(), "0.125".to_string()),
    ];
    let value = Measurements::validate(&data).unwrap();
    assert_eq!(
        value,
        Measurements {
            temperature: 36.5,
            ratio: 0.125,
        }
    );

    let data = serde_json::json!({ "temperature": 36.5, "ratio": 0.125 });
    let value = Measurements::validate(&data).unwrap();
    assert_eq!(
        value,
        Measurements {
            temperature: 36.5,
            ratio: 0.125,
        }
    );
}

#[test]
fn option_string_empty_from_pair_list_becomes_none() {
    #[derive(Debug, Schema, PartialEq)]
    struct Optional {
        name: Option<String>,
    }

    let data = vec![("name".to_string(), String::new())];
    let value = Optional::validate(&data).unwrap();
    assert_eq!(value.name, None);
}

#[test]
fn required_string_empty_from_pair_list_is_a_required_error() {
    #[derive(Debug, Schema, PartialEq)]
    struct Required {
        name: String,
    }

    let data = vec![("name".to_string(), String::new())];
    let errors = Required::validate(&data).unwrap_err();
    assert_eq!(errors.get("name").unwrap().code(), "required");
}

#[test]
fn nested_empty_from_flat_form_is_a_required_error() {
    #[derive(Debug, Schema, PartialEq)]
    struct Address {
        city: String,
    }

    #[derive(Debug, Schema, PartialEq)]
    struct Contact {
        address: Address,
    }

    let data = vec![("address.city".to_string(), String::new())];
    let errors = Contact::validate(&data).unwrap_err();
    assert_eq!(errors.get("address.city").unwrap().code(), "required");
}

#[test]
fn vec_from_single_repeated_form_key() {
    #[derive(Debug, Schema, PartialEq)]
    struct Tags {
        tags: Vec<String>,
    }

    let data = vec![("tags".to_string(), "a".to_string())];
    let value = Tags::validate(&data).unwrap();
    assert_eq!(value.tags, vec!["a"]);
}

#[test]
fn option_vec_from_pair_list() {
    #[derive(Debug, Schema, PartialEq)]
    struct Form {
        tags: Option<Vec<String>>,
    }

    let data = vec![("tags".to_string(), String::new())];
    assert_eq!(Form::validate(&data).unwrap().tags, None);

    let data = vec![("tags".to_string(), "a".to_string())];
    assert_eq!(
        Form::validate(&data).unwrap().tags,
        Some(vec!["a".to_string()])
    );

    let data = vec![
        ("tags".to_string(), "a".to_string()),
        ("tags".to_string(), "b".to_string()),
    ];
    assert_eq!(
        Form::validate(&data).unwrap().tags,
        Some(vec!["a".to_string(), "b".to_string()])
    );
}

#[test]
fn option_nested_schema() {
    #[derive(Debug, Schema, PartialEq)]
    struct Address {
        city: String,
    }

    #[derive(Debug, Schema, PartialEq)]
    struct Contact {
        address: Option<Address>,
    }

    let data = vec![("address.city".to_string(), "Sydney".to_string())];
    let value = Contact::validate(&data).unwrap();
    assert_eq!(
        value.address,
        Some(Address {
            city: "Sydney".to_string()
        })
    );

    let data: Vec<(String, String)> = vec![];
    assert_eq!(Contact::validate(&data).unwrap().address, None);

    let data = vec![("address.city".to_string(), String::new())];
    let errors = Contact::validate(&data).unwrap_err();
    assert_eq!(errors.get("address.city").unwrap().code(), "required");

    let descriptor = Contact::descriptor();
    assert!(!descriptor.fields[0].required);
    assert!(matches!(descriptor.fields[0].ty, FieldType::Nested(_)));
}

#[test]
fn two_level_nesting_from_flat_form_data() {
    #[derive(Debug, Schema, PartialEq)]
    struct Geo {
        lat: f64,
    }

    #[derive(Debug, Schema, PartialEq)]
    struct Address {
        geo: Geo,
    }

    #[derive(Debug, Schema, PartialEq)]
    struct Contact {
        address: Address,
    }

    let data = vec![("address.geo.lat".to_string(), "-33.86".to_string())];
    let value = Contact::validate(&data).unwrap();
    assert_eq!(
        value,
        Contact {
            address: Address {
                geo: Geo { lat: -33.86 }
            }
        }
    );

    // An absent nested object reports the parent as required rather than
    // cascading an error per leaf field.
    let errors = Contact::validate(&Vec::<(String, String)>::new()).unwrap_err();
    assert_eq!(errors.get("address").unwrap().code(), "required");

    // A present-but-invalid leaf reports the full dotted path.
    let data = vec![("address.geo.lat".to_string(), "not-a-number".to_string())];
    let errors = Contact::validate(&data).unwrap_err();
    assert_eq!(errors.get("address.geo.lat").unwrap().code(), "float");
}

#[test]
fn vec_of_nested_schemas() {
    #[derive(Debug, Schema, PartialEq)]
    struct Address {
        city: String,
    }

    #[derive(Debug, Schema, PartialEq)]
    struct Contact {
        addresses: Vec<Address>,
    }

    let data = serde_json::json!({
        "addresses": [{ "city": "Sydney" }, { "city": "Melbourne" }],
    });
    let value = Contact::validate(&data).unwrap();
    assert_eq!(
        value.addresses,
        vec![
            Address {
                city: "Sydney".to_string()
            },
            Address {
                city: "Melbourne".to_string()
            },
        ]
    );

    let data = serde_json::json!({ "addresses": [{ "city": "" }] });
    let errors = Contact::validate(&data).unwrap_err();
    assert_eq!(errors.get("addresses.0.city").unwrap().code(), "required");

    let descriptor = Contact::descriptor();
    assert!(matches!(
        descriptor.fields[0].ty,
        FieldType::List(ref inner) if matches!(**inner, FieldType::Nested(_))
    ));
}

#[test]
fn vec_of_nested_schemas_from_flat_form() {
    #[derive(Debug, Schema, PartialEq)]
    struct Address {
        city: String,
    }

    #[derive(Debug, Schema, PartialEq)]
    struct Contact {
        name: String,
        addresses: Vec<Address>,
    }

    let data = vec![
        ("name".to_string(), "Alice".to_string()),
        ("addresses.0.city".to_string(), "Sydney".to_string()),
        ("addresses.1.city".to_string(), "Melbourne".to_string()),
    ];
    let value = Contact::validate(&data).unwrap();
    assert_eq!(value.name, "Alice");
    assert_eq!(
        value.addresses,
        vec![
            Address {
                city: "Sydney".to_string()
            },
            Address {
                city: "Melbourne".to_string()
            },
        ]
    );

    let data = vec![("addresses.0.city".to_string(), String::new())];
    let errors = Contact::validate(&data).unwrap_err();
    assert_eq!(errors.get("addresses.0.city").unwrap().code(), "required");
}

#[test]
fn negative_number_bounds() {
    #[derive(Debug, Schema, PartialEq)]
    struct Reading {
        #[schema(range(min = -40, max = 50))]
        temperature: i32,

        #[schema(min = -1.5)]
        ratio: f64,
    }

    let data = vec![
        ("temperature".to_string(), "-10".to_string()),
        ("ratio".to_string(), "0.0".to_string()),
    ];
    let value = Reading::validate(&data).unwrap();
    assert_eq!(
        value,
        Reading {
            temperature: -10,
            ratio: 0.0,
        }
    );

    let bad = vec![
        ("temperature".to_string(), "-41".to_string()),
        ("ratio".to_string(), "-2.0".to_string()),
    ];
    let errors = Reading::validate(&bad).unwrap_err();
    assert_eq!(errors.get("temperature").unwrap().code(), "range");
    assert_eq!(errors.get("ratio").unwrap().code(), "min");

    let descriptor = Reading::descriptor();
    assert_eq!(
        descriptor.fields[0].validators,
        vec![ValidatorDescriptor::Range {
            min: Number::Integer(-40),
            max: Number::Integer(50),
        }]
    );
    assert_eq!(
        descriptor.fields[1].validators,
        vec![ValidatorDescriptor::Min(Number::Float(-1.5))]
    );
}

#[test]
fn vec_from_repeated_form_keys() {
    #[derive(Debug, Schema, PartialEq)]
    struct Tags {
        tags: Vec<String>,
    }

    let data = vec![
        ("tags".to_string(), "a".to_string()),
        ("tags".to_string(), "b".to_string()),
        ("tags".to_string(), "c".to_string()),
    ];
    let value = Tags::validate(&data).unwrap();
    assert_eq!(value.tags, vec!["a", "b", "c"]);
}

#[test]
fn vec_from_json_array() {
    #[derive(Debug, Schema, PartialEq)]
    struct Tags {
        tags: Vec<String>,
    }

    let data = serde_json::json!({ "tags": ["a", "b", "c"] });
    let value = Tags::validate(&data).unwrap();
    assert_eq!(value.tags, vec!["a", "b", "c"]);
}

#[test]
fn vec_element_coercion_failure() {
    #[derive(Debug, Schema, PartialEq)]
    struct Counts {
        counts: Vec<u32>,
    }

    let data = serde_json::json!({ "counts": [1, "not-a-number", 3] });
    let errors = Counts::validate(&data).unwrap_err();
    assert_eq!(errors.get("counts.1").unwrap().code(), "integer");
}

#[test]
fn nested_schema_from_flat_form_data() {
    #[derive(Debug, Schema, PartialEq)]
    struct Address {
        city: String,
        zip: String,
    }

    #[derive(Debug, Schema, PartialEq)]
    struct Contact {
        name: String,
        address: Address,
    }

    let data = vec![
        ("name".to_string(), "Alice".to_string()),
        ("address.city".to_string(), "Sydney".to_string()),
        ("address.zip".to_string(), "2000".to_string()),
    ];
    let value = Contact::validate(&data).unwrap();
    assert_eq!(value.name, "Alice");
    assert_eq!(value.address.city, "Sydney");
    assert_eq!(value.address.zip, "2000");
}

#[test]
fn nested_schema_from_json_object() {
    #[derive(Debug, Schema, PartialEq)]
    struct Address {
        city: String,
        zip: String,
    }

    #[derive(Debug, Schema, PartialEq)]
    struct Contact {
        name: String,
        address: Address,
    }

    let data = serde_json::json!({
        "name": "Alice",
        "address": { "city": "Sydney", "zip": "2000" },
    });
    let value = Contact::validate(&data).unwrap();
    assert_eq!(value.name, "Alice");
    assert_eq!(value.address.city, "Sydney");
    assert_eq!(value.address.zip, "2000");
}

#[test]
fn nested_errors_carry_dotted_paths() {
    #[derive(Debug, Schema, PartialEq)]
    struct Address {
        #[schema(email)]
        contact: String,
    }

    #[derive(Debug, Schema, PartialEq)]
    struct Contact {
        address: Address,
    }

    let data = HashMap::from([("address.contact".to_string(), "not-an-email".to_string())]);
    let errors = Contact::validate(&data).unwrap_err();
    assert_eq!(errors.get("address.contact").unwrap().code(), "email");
}

#[test]
fn descriptor_for_all_features() {
    #[derive(Debug, Schema, PartialEq)]
    struct Address {
        city: String,
        zip: u32,
    }

    #[derive(Debug, Schema)]
    #[allow(dead_code)]
    struct Person {
        #[schema(email, max_length = 254)]
        email: String,

        #[schema(min_length = 8)]
        password: String,

        #[schema(min = 13, max = 120)]
        age: u32,

        #[schema(range(min = 0, max = 10))]
        score: u32,

        #[schema(one_of = "red, green, blue")]
        color: String,

        #[schema(trim, min_length = 1)]
        name: String,

        #[schema(default = "user".to_string())]
        username: String,

        role: Option<String>,

        tags: Vec<String>,

        address: Address,
    }

    let descriptor = Person::descriptor();
    assert_eq!(descriptor.fields.len(), 10);

    let email = &descriptor.fields[0];
    assert_eq!(email.name, "email");
    assert_eq!(email.ty, FieldType::String);
    assert!(email.required);
    assert_eq!(
        email.validators,
        vec![
            ValidatorDescriptor::Email,
            ValidatorDescriptor::MaxLength(254)
        ]
    );

    let password = &descriptor.fields[1];
    assert_eq!(password.name, "password");
    assert_eq!(password.validators, vec![ValidatorDescriptor::MinLength(8)]);

    let age = &descriptor.fields[2];
    assert_eq!(
        age.validators,
        vec![
            ValidatorDescriptor::Min(Number::Integer(13)),
            ValidatorDescriptor::Max(Number::Integer(120)),
        ]
    );

    let score = &descriptor.fields[3];
    assert_eq!(
        score.validators,
        vec![ValidatorDescriptor::Range {
            min: Number::Integer(0),
            max: Number::Integer(10),
        }]
    );

    let color = &descriptor.fields[4];
    assert_eq!(
        color.validators,
        vec![ValidatorDescriptor::OneOf(&["red", "green", "blue"])]
    );

    let name = &descriptor.fields[5];
    assert_eq!(
        name.validators,
        vec![ValidatorDescriptor::Trim, ValidatorDescriptor::MinLength(1)]
    );

    let username = &descriptor.fields[6];
    assert_eq!(username.name, "username");
    assert!(!username.required);
    assert_eq!(username.validators, vec![ValidatorDescriptor::Default]);

    let role = &descriptor.fields[7];
    assert_eq!(role.name, "role");
    assert_eq!(role.ty, FieldType::String);
    assert!(!role.required);
    assert!(role.validators.is_empty());

    let tags = &descriptor.fields[8];
    assert_eq!(tags.name, "tags");
    assert_eq!(tags.ty, FieldType::List(Box::new(FieldType::String)));
    assert!(tags.required);
    assert!(tags.validators.is_empty());

    let address = &descriptor.fields[9];
    assert_eq!(address.name, "address");
    assert_eq!(
        address.ty,
        FieldType::Nested(SchemaDescriptor {
            fields: vec![
                FieldDescriptor {
                    name: Cow::Borrowed("city"),
                    ty: FieldType::String,
                    validators: vec![],
                    required: true,
                },
                FieldDescriptor {
                    name: Cow::Borrowed("zip"),
                    ty: FieldType::Integer,
                    validators: vec![],
                    required: true,
                },
            ],
        })
    );
}
