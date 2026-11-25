//! Compile-time struct metadata, the Rust equivalent of `os/gstructs` (ADR-183).
//!
//! GoFrame reads struct fields and tags at runtime through reflection. Rust has
//! no reflection, so [`StructMeta`] captures the same information at compile
//! time: derive it on a struct and read [`FieldMeta`] slices, field maps and
//! tag-name maps. Go-style tags are written as
//! `#[gf(tag = "json:\"name,omitempty\" gconv:\"nick\"")]`.
//!
//! Not ported: runtime lookup of arbitrary types, pointer/embedded-struct
//! traversal, and the `FieldsInput`/`FieldMapInput` option surface.

pub use gf_structs_derive::StructMeta;

use indexmap::IndexMap;

/// One struct field's compile-time metadata, like `gstructs.Field`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FieldMeta {
    /// The Rust field name, like Go's `Name`.
    pub name: &'static str,
    /// The Rust type as written, like Go's `Type().String()`.
    pub type_name: &'static str,
    /// Whether the field is an `Option<...>` (nilable, like a pointer).
    pub optional: bool,
    /// The Go-style tags, `(key, value)` pairs.
    pub tags: &'static [(&'static str, &'static str)],
}

impl FieldMeta {
    /// Returns the tag value of `key`, like `Field.Tag(key)`.
    pub fn tag(&self, key: &str) -> Option<&'static str> {
        self.tags
            .iter()
            .find(|(tag_key, _)| *tag_key == key)
            .map(|(_, value)| *value)
    }

    /// Returns the tag name used for maps: the first tag in `priority` that
    /// exists on the field (its value before `,`), falling back to the Rust
    /// name, like `Field.TagPriorityName`.
    pub fn tag_name(&self, priority: &[&str]) -> &'static str {
        for key in priority {
            if let Some(value) = self.tag(key) {
                return value.split(',').next().unwrap_or(value);
            }
        }
        self.name
    }
}

/// The compile-time metadata of a struct, like `gstructs.StructType`.
pub trait StructMeta {
    /// The struct name.
    const NAME: &'static str;
    /// Every field in declaration order.
    const FIELDS: &'static [FieldMeta];
}

/// Returns every field of `T`, like `gstructs.Fields`.
pub fn fields<T: StructMeta>() -> &'static [FieldMeta] {
    T::FIELDS
}

/// Returns a map keyed by each field's priority tag name, like
/// `gstructs.FieldMap`.
pub fn field_map<T: StructMeta>(priority: &[&str]) -> IndexMap<String, FieldMeta> {
    let mut map = IndexMap::new();
    for field in T::FIELDS {
        map.insert(field.tag_name(priority).to_owned(), *field);
    }
    map
}

/// Returns a map of priority tag name to Rust field name, like
/// `gstructs.TagMapName`.
pub fn tag_map_name<T: StructMeta>(priority: &[&str]) -> IndexMap<String, String> {
    let mut map = IndexMap::new();
    for field in T::FIELDS {
        map.insert(field.name.to_owned(), field.tag_name(priority).to_owned());
    }
    map
}

/// Returns the field whose priority tag name equals `name`, like looking a
/// field up in `gstructs.FieldMap`.
pub fn field_by_tag<T: StructMeta>(name: &str, priority: &[&str]) -> Option<FieldMeta> {
    T::FIELDS
        .iter()
        .copied()
        .find(|field| field.tag_name(priority) == name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trait_defaults_are_usable_without_derive() {
        struct Manual;
        impl StructMeta for Manual {
            const NAME: &'static str = "Manual";
            const FIELDS: &'static [FieldMeta] = &[FieldMeta {
                name: "id",
                type_name: "u64",
                optional: false,
                tags: &[("json", "id")],
            }];
        }
        assert_eq!(fields::<Manual>().len(), 1);
        assert_eq!(fields::<Manual>()[0].tag("json"), Some("id"));
        assert_eq!(fields::<Manual>()[0].tag_name(&["gconv", "json"]), "id");
    }
}
