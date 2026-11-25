use gf_structs::{StructMeta, field_by_tag, field_map, fields, tag_map_name};

#[expect(dead_code, reason = "metadata-only fixture")]
#[derive(StructMeta)]
struct User {
    #[gf(tag = "json:\"id\"")]
    id: u64,
    #[gf(tag = "json:\"name,omitempty\" gconv:\"nick\"")]
    name: String,
    #[gf(tag = "json:\"note\"")]
    note: Option<String>,
}

#[test]
fn derived_metadata_and_tag_priority() {
    assert_eq!(User::NAME, "User");
    let all = fields::<User>();
    assert_eq!(all.len(), 3);
    assert_eq!(all[0].name, "id");
    assert_eq!(all[0].tag("json"), Some("id"));
    assert!(!all[0].optional);
    assert_eq!(all[1].tag("gconv"), Some("nick"));
    assert!(all[2].optional, "Option<T> fields are nilable");

    let by_gconv = field_map::<User>(&["gconv", "json"]);
    let keys: Vec<String> = by_gconv.keys().cloned().collect();
    assert_eq!(keys, vec!["id", "nick", "note"]);

    let by_json = field_map::<User>(&["json"]);
    let keys: Vec<String> = by_json.keys().cloned().collect();
    assert_eq!(keys, vec!["id", "name", "note"]);

    let names = tag_map_name::<User>(&["gconv", "json"]);
    assert_eq!(names.get("name"), Some(&"nick".to_owned()));

    let found = field_by_tag::<User>("nick", &["gconv", "json"]).expect("nick tag");
    assert_eq!(found.name, "name");
}
