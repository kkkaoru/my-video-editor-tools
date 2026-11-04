//! The font families installed on this computer, read with the pure-Rust `fontdb` on macOS and Windows alike.

use std::collections::BTreeMap;

/// Hidden system faces start with a dot; they cannot be chosen by name in other apps.
const HIDDEN_PREFIX: char = '.';
/// `Settings::validate` refuses these, because the name goes into XML attributes.
const MARKUP_CHARACTERS: [char; 3] = ['"', '<', '&'];

fn is_choosable(family: &str) -> bool {
    !family.trim().is_empty()
        && !family.starts_with(HIDDEN_PREFIX)
        && !family.contains(MARKUP_CHARACTERS)
}

/// Sorted without regard to case, one row per family however many faces it has.
fn choosable_families<'a>(names: impl Iterator<Item = &'a str>) -> Vec<String> {
    let by_folded_name: BTreeMap<String, &str> = names
        .filter(|name| is_choosable(name))
        .map(|name| (name.to_lowercase(), name))
        .collect();
    by_folded_name
        .into_values()
        .map(ToOwned::to_owned)
        .collect()
}

/// Reads every installed font's header, so it can take a moment; callers keep the result.
#[must_use]
pub fn system_families() -> Vec<String> {
    let mut database = fontdb::Database::new();
    database.load_system_fonts();
    // `fontdb` lists a face's English name first; that is the name other apps resolve.
    choosable_families(
        database
            .faces()
            .filter_map(|face| face.families.first())
            .map(|(name, _)| name.as_str()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hidden_markup_and_blank_names_are_left_out() {
        assert_eq!(
            choosable_families(["A", ".SF NS", "B<C", "D&E", "F\"G", "  "].into_iter()),
            vec!["A".to_owned()]
        );
    }

    #[test]
    fn families_are_sorted_without_regard_to_case_and_listed_once() {
        assert_eq!(
            choosable_families(["b", "A", "c", "A", "ヒラギノ"].into_iter()),
            vec![
                "A".to_owned(),
                "b".to_owned(),
                "c".to_owned(),
                "ヒラギノ".to_owned()
            ]
        );
    }

    #[test]
    fn every_system_family_can_be_saved_in_the_settings() {
        let unusable: Vec<String> = system_families()
            .into_iter()
            .filter(|family| {
                crate::settings::Settings {
                    font_family: family.clone(),
                    ..crate::settings::Settings::default()
                }
                .validate()
                .is_err()
            })
            .collect();
        assert_eq!(unusable, Vec::<String>::new());
    }
}
