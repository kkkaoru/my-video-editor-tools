//! The font families installed on this computer, read with the pure-Rust `fontdb` on macOS and Windows alike.

use crate::settings::FONT_FAMILY_FORBIDDEN;
use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::sync::{Mutex, OnceLock};

/// Hidden system faces start with a dot; they cannot be chosen by name in other apps.
const HIDDEN_PREFIX: char = '.';
/// The face FCPXML titles ask for; a variable font's Latin advances change along this axis.
const BOLD_WEIGHT: f32 = 700.0;

fn is_choosable(family: &str) -> bool {
    !family.trim().is_empty()
        && !family.starts_with(HIDDEN_PREFIX)
        && !family.contains(FONT_FAMILY_FORBIDDEN)
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

/// One font's Bold advances in ems, per character it has a glyph for.
#[derive(PartialEq)]
pub struct GlyphWidths {
    ems: HashMap<char, f64>,
}

impl GlyphWidths {
    /// `None` for a character the font lacks: which font the renderer falls back to cannot be read.
    #[must_use]
    pub fn em(&self, c: char) -> Option<f64> {
        self.ems.get(&c).copied()
    }
}

#[cfg(test)]
impl GlyphWidths {
    pub(crate) fn uniform(characters: &str, em: f64) -> Self {
        Self {
            ems: characters.chars().map(|c| (c, em)).collect(),
        }
    }
}

impl fmt::Debug for GlyphWidths {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "GlyphWidths({} glyphs)", self.ems.len())
    }
}

fn face_widths(data: &[u8], index: u32) -> Option<GlyphWidths> {
    let mut face = ttf_parser::Face::parse(data, index).ok()?;
    // Does nothing to a font without this axis.
    face.set_variation(ttf_parser::Tag::from_bytes(b"wght"), BOLD_WEIGHT);
    let per_em = f64::from(face.units_per_em());
    let mut ems = HashMap::new();
    for table in face.tables().cmap?.subtables {
        if table.is_unicode() {
            table.codepoints(|point| {
                let advance = table
                    .glyph_index(point)
                    .and_then(|glyph| face.glyph_hor_advance(glyph));
                if let (Some(c), Some(advance)) = (char::from_u32(point), advance) {
                    ems.insert(c, f64::from(advance) / per_em);
                }
            });
        }
    }
    Some(GlyphWidths { ems })
}

fn bold_widths_in(database: &fontdb::Database, family: &str) -> Option<GlyphWidths> {
    let id = database.query(&fontdb::Query {
        families: &[fontdb::Family::Name(family)],
        weight: fontdb::Weight::BOLD,
        ..fontdb::Query::default()
    })?;
    database.with_face_data(id, face_widths)?
}

/// The chosen family's Bold advances, or `None` when this computer does not have it. Each family is
/// read once per run, and a miss is read again in case the font was installed meanwhile.
#[must_use]
pub fn bold_widths(family: &str) -> Option<&'static GlyphWidths> {
    static READ: OnceLock<Mutex<HashMap<String, &'static GlyphWidths>>> = OnceLock::new();
    let mut read = READ
        .get_or_init(Mutex::default)
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(widths) = read.get(family) {
        return Some(*widths);
    }
    let mut database = fontdb::Database::new();
    database.load_system_fonts();
    let widths: &'static GlyphWidths = Box::leak(Box::new(bold_widths_in(&database, family)?));
    read.insert(family.to_owned(), widths);
    Some(widths)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A TrueType file with the tables `ttf-parser` and `fontdb` need, mapping 'A' to a 600/1000 em
    /// glyph and 'あ' to a 1000/1000 em one.
    fn synthetic_font(family: &str) -> Vec<u8> {
        let mut head = vec![0u8; 54];
        head[0..4].copy_from_slice(&0x0001_0000u32.to_be_bytes());
        head[12..16].copy_from_slice(&0x5F0F_3CF5u32.to_be_bytes());
        head[18..20].copy_from_slice(&1000u16.to_be_bytes());
        let mut hhea = vec![0u8; 36];
        hhea[0..4].copy_from_slice(&0x0001_0000u32.to_be_bytes());
        hhea[34..36].copy_from_slice(&3u16.to_be_bytes());
        let mut maxp = 0x0000_5000u32.to_be_bytes().to_vec();
        maxp.extend(3u16.to_be_bytes());
        let hmtx: Vec<u8> = [500u16, 0, 600, 0, 1000, 0]
            .iter()
            .flat_map(|value| value.to_be_bytes())
            .collect();
        let mut cmap: Vec<u8> = [0u16, 1, 3, 10]
            .iter()
            .flat_map(|value| value.to_be_bytes())
            .collect();
        cmap.extend(12u32.to_be_bytes());
        cmap.extend([12u16, 0].iter().flat_map(|value| value.to_be_bytes()));
        let groups = [(0x41u32, 0x41u32, 1u32), (0x3042, 0x3042, 2)];
        cmap.extend((16u32 + 12 * groups.len() as u32).to_be_bytes());
        cmap.extend(0u32.to_be_bytes());
        cmap.extend((groups.len() as u32).to_be_bytes());
        cmap.extend(groups.iter().flat_map(|(first, last, glyph)| {
            [*first, *last, *glyph]
                .into_iter()
                .flat_map(u32::to_be_bytes)
        }));
        let encoded: Vec<u8> = family.encode_utf16().flat_map(u16::to_be_bytes).collect();
        let length = encoded.len() as u16;
        let mut name: Vec<u8> = [
            0u16, 2, 30, 3, 1, 0x409, 1, length, 0, 3, 1, 0x409, 6, length, 0,
        ]
        .iter()
        .flat_map(|value| value.to_be_bytes())
        .collect();
        name.extend(&encoded);
        let tables: [(&[u8; 4], &[u8]); 6] = [
            (b"cmap", &cmap),
            (b"head", &head),
            (b"hhea", &hhea),
            (b"hmtx", &hmtx),
            (b"maxp", &maxp),
            (b"name", &name),
        ];
        let mut file = 0x0001_0000u32.to_be_bytes().to_vec();
        file.extend([6u16, 0, 0, 0].iter().flat_map(|value| value.to_be_bytes()));
        let mut offset = 12 + 16 * tables.len() as u32;
        let mut body = Vec::new();
        tables.iter().for_each(|(tag, data)| {
            file.extend(*tag);
            file.extend(0u32.to_be_bytes());
            file.extend(offset.to_be_bytes());
            file.extend((data.len() as u32).to_be_bytes());
            body.extend(*data);
            body.resize(body.len() + (4 - data.len() % 4) % 4, 0);
            offset = 12 + 16 * tables.len() as u32 + body.len() as u32;
        });
        file.extend(body);
        file
    }

    #[test]
    fn a_fonts_own_advances_are_read_in_ems() {
        let widths = face_widths(&synthetic_font("Fixture"), 0).unwrap();
        assert_eq!(widths.em('A'), Some(0.6));
        assert_eq!(widths.em('あ'), Some(1.0));
        assert_eq!(widths.em('B'), None);
        assert_eq!(format!("{widths:?}"), "GlyphWidths(2 glyphs)");
    }

    #[test]
    fn a_family_is_found_by_name_and_a_missing_one_gives_nothing() {
        let mut database = fontdb::Database::new();
        database.load_font_data(synthetic_font("Fixture"));
        assert_eq!(
            bold_widths_in(&database, "Fixture").unwrap().em('A'),
            Some(0.6)
        );
        assert_eq!(bold_widths_in(&database, "Absent"), None);
    }

    #[test]
    fn bytes_that_are_no_font_give_nothing() {
        assert_eq!(face_widths(b"not a font", 0), None);
    }

    #[test]
    fn a_family_this_computer_lacks_gives_nothing() {
        assert_eq!(bold_widths("No Such Family For Tests"), None);
    }

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
