//! Integration test for `#[derive(Validate)]` (ADR-092).

use gf_valid::gvalid::Validator;
use gf_valid_derive::Validate;

#[derive(Validate)]
struct User {
    #[gf(rule = "required|length:2,10")]
    name: String,
    #[gf(rule = "min:18")]
    age: i64,
    #[gf(rule = "email")]
    email: String,
    #[gf(rule = "required")]
    nickname: Option<String>,
    // No `gf` attribute: included in the data map, no rule.
    active: bool,
}

fn valid_user() -> User {
    User {
        name: "john".to_owned(),
        age: 20,
        email: "john@example.com".to_owned(),
        nickname: Some("johnny".to_owned()),
        active: true,
    }
}

#[test]
fn valid_struct_passes() {
    Validator::new()
        .check_struct(&valid_user())
        .expect("valid user");
}

#[test]
fn required_rule_reports_missing_value() {
    let mut user = valid_user();
    user.name.clear();
    let error = Validator::new()
        .check_struct(&user)
        .expect_err("empty name");
    assert!(
        error.to_string().contains("name"),
        "error should mention the field: {error}"
    );
}

#[test]
fn numeric_and_format_rules_apply() {
    let mut user = valid_user();
    user.age = 10;
    assert!(Validator::new().check_struct(&user).is_err());

    let mut user = valid_user();
    user.email = "not-an-email".to_owned();
    assert!(Validator::new().check_struct(&user).is_err());
}

#[test]
fn option_field_rules_apply() {
    let mut user = valid_user();
    user.nickname = None;
    assert!(Validator::new().check_struct(&user).is_err());
}

#[test]
fn named_rules_are_ordered() {
    use gf_valid::gvalid::{Rules, Validatable};
    let Rules::Map(rules) = User::validation_rules() else {
        panic!("expected a rule map");
    };
    assert_eq!(
        rules.keys().cloned().collect::<Vec<String>>(),
        vec![
            "name".to_owned(),
            "age".to_owned(),
            "email".to_owned(),
            "nickname".to_owned()
        ]
    );
    let data = valid_user().validation_data();
    assert!(matches!(data, gf_valid::gvalid::Value::Map(_)));
    assert!(matches!(data, gf_valid::gvalid::Value::Map(ref map) if map.contains_key("active")));
}
