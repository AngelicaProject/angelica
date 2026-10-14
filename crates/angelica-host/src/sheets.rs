//! Named columns of the game's sheets: the game map's sheet schemas applied
//! to the layouts the installed game has (`docs/formats/game-map-v2.md`).
#![forbid(unsafe_code)]
use crate::{
    error::{Error, Kind, Result},
    game_data::GameData,
};
use angelica_map::{ColumnType, SheetSchema};
use std::{
    collections::BTreeMap,
    sync::{Mutex, PoisonError, RwLock},
};

/// A named column in the running game's layout of its sheet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    pub name: String,
    pub index: u16,
    pub column_type: ColumnType,
}

/// The game map's sheet schemas and the fields they give each sheet.
#[derive(Default)]
pub struct Sheets {
    schemas: RwLock<BTreeMap<String, SheetSchema>>,
    resolved: Mutex<BTreeMap<String, std::result::Result<Vec<Field>, String>>>,
}

impl Sheets {
    #[must_use]
    pub fn new(schemas: impl IntoIterator<Item = SheetSchema>) -> Self {
        let sheets = Self::default();
        sheets.set(schemas);
        sheets
    }

    /// Uses these schemas from now on.
    pub fn set(&self, schemas: impl IntoIterator<Item = SheetSchema>) {
        *self.schemas.write().unwrap_or_else(PoisonError::into_inner) = schemas
            .into_iter()
            .map(|schema| (schema.sheet.clone(), schema))
            .collect();
        self.resolved
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clear();
    }

    /// Adds or replaces the schema of one sheet.
    pub fn insert(&self, schema: SheetSchema) {
        self.resolved
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&schema.sheet);
        self.schemas
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(schema.sheet.clone(), schema);
    }

    /// The named columns of `sheet` in the running game, sorted by name.
    ///
    /// # Errors
    /// `unavailable` when the map names no columns of the sheet or does not
    /// know the game's layout of it; the game data's errors otherwise.
    pub fn fields(&self, sheet: &str, data: &GameData) -> Result<Vec<Field>> {
        if let Some(known) = self
            .resolved
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(sheet)
        {
            return known
                .clone()
                .map_err(|reason| Error::new(Kind::Unavailable, reason));
        }
        let info = data.sheet(sheet)?;
        let result = {
            let schemas = self.schemas.read().unwrap_or_else(PoisonError::into_inner);
            match schemas.get(sheet) {
                None => Err(format!("the game map names no columns of sheet {sheet}")),
                Some(schema) => schema.resolve(&info.layout()).map(|columns| {
                    columns
                        .into_iter()
                        .map(|(name, index)| Field {
                            column_type: schema.columns[&name].column_type,
                            name,
                            index,
                        })
                        .collect()
                }),
            }
        };
        self.resolved
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(sheet.to_owned(), result.clone());
        result.map_err(|reason| Error::new(Kind::Unavailable, reason))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use angelica_map::{KnownLayout, NamedColumn};
    use angelica_sqpack::{
        excel::{ColumnKind, Language},
        testing::{FakeGame, FakeRow, FakeSheet, FakeValue},
    };

    fn data() -> (tempfile::TempDir, GameData) {
        let folder = tempfile::tempdir().unwrap();
        let sheet = FakeSheet::new(vec![ColumnKind::UInt32, ColumnKind::String]).with_rows(
            Language::English,
            vec![FakeRow::new(
                1,
                vec![FakeValue::UInt32(7), FakeValue::String(b"Potion".to_vec())],
            )],
        );
        FakeGame::new("1")
            .with_sheet("Item", sheet)
            .write(folder.path())
            .unwrap();
        let data = GameData::new(Some(folder.path()));
        (folder, data)
    }

    fn schema(fingerprint: String) -> SheetSchema {
        SheetSchema {
            sheet: "Item".into(),
            doc: "Items.".into(),
            columns: [(
                "name".to_owned(),
                NamedColumn {
                    column_type: ColumnType::Text,
                    doc: "The name.".into(),
                },
            )]
            .into(),
            layouts: vec![KnownLayout {
                fingerprint,
                columns: [("name".to_owned(), 1)].into(),
            }],
        }
    }

    #[test]
    fn names_apply_only_to_a_layout_the_map_knows() {
        let (_folder, data) = data();
        let sheets = Sheets::default();
        let error = sheets.fields("Item", &data).unwrap_err();
        assert_eq!(error.kind, Kind::Unavailable);
        assert!(
            error.message.contains("names no columns"),
            "{}",
            error.message
        );

        sheets.insert(schema("0123456789abcdef".into()));
        let error = sheets.fields("Item", &data).unwrap_err();
        assert_eq!(error.kind, Kind::Unavailable);
        assert!(
            error.message.contains("does not know layout"),
            "{}",
            error.message
        );

        let fingerprint = data.sheet("Item").unwrap().layout().fingerprint();
        sheets.insert(schema(fingerprint));
        assert_eq!(
            sheets.fields("Item", &data).unwrap(),
            [Field {
                name: "name".into(),
                index: 1,
                column_type: ColumnType::Text
            }]
        );
    }
}
