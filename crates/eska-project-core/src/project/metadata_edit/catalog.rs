//! Indexed Designer field and enum declarations shared by the metadata readers and writers.

use std::{collections::HashMap, sync::LazyLock};

pub struct Field {
    pub namespace: &'static str,
    pub name: &'static str,
    pub model_type: &'static str,
    pub property: bool,
    pub since: &'static str,
}

impl Field {
    fn from_row(row: &'static str) -> Option<(&'static str, Self)> {
        let mut columns = row.split('\t');
        let owner = columns.next()?;
        let field = Self {
            namespace: columns.next()?,
            name: columns.next()?,
            model_type: columns.next()?,
            property: columns.next()? == "1",
            since: columns.next()?,
        };
        Some((owner, field))
    }
}

/// Fields of one metadata class, in the order used by creation and editing selectors.
pub(super) fn fields(class: &str) -> &'static [Field] {
    static BY_CLASS: LazyLock<HashMap<&str, Vec<Field>>> = LazyLock::new(|| {
        let mut classes = HashMap::<_, Vec<_>>::new();
        for (owner, field) in include_str!("fields.tsv")
            .lines()
            .skip(1)
            .filter_map(Field::from_row)
        {
            classes.entry(owner).or_default().push(field);
        }
        classes
    });
    BY_CLASS.get(class).map_or(&[], Vec::as_slice)
}

pub fn field(class: &str, namespace: &str, name: &str) -> Option<&'static Field> {
    fields(class)
        .iter()
        .find(|field| field.namespace == namespace && field.name == name)
}

pub(super) fn enum_values(domain: &str) -> &'static [&'static str] {
    static BY_DOMAIN: LazyLock<HashMap<&str, Vec<&str>>> = LazyLock::new(|| {
        let mut domains = HashMap::<_, Vec<_>>::new();
        for (domain, value) in include_str!("enums.tsv")
            .lines()
            .skip(1)
            .filter_map(|line| line.split_once('\t'))
        {
            domains.entry(domain).or_default().push(value);
        }
        domains
    });
    BY_DOMAIN.get(domain).map_or(&[], Vec::as_slice)
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    #[test]
    fn every_qualified_field_resolves_to_one_writer_declaration() {
        let mut keys = HashSet::new();
        for row in include_str!("fields.tsv").lines().skip(1) {
            let columns: Vec<_> = row.split('\t').collect();
            let [owner, namespace, name, model_type, property, since] = columns[..] else {
                panic!("Invalid field row: {row}");
            };
            assert!(keys.insert((owner, namespace, name)), "{row}");
            let field = super::field(owner, namespace, name).expect("indexed field");
            assert_eq!(
                (field.model_type, field.property, field.since),
                (model_type, property == "1", since)
            );
        }
    }
}
