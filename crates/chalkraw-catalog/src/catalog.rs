use crate::error::CatalogError;
use redb::{Database, ReadableDatabase, ReadableTable, TableDefinition};
use std::path::{Path, PathBuf};

pub const SCHEMA_VERSION: u32 = 1;

// Table definitions referenced from sibling modules.
pub(crate) const PHOTOS_TABLE: TableDefinition<&[u8; 16], &[u8]> =
    TableDefinition::new("photos");
pub(crate) const EDITS_TABLE: TableDefinition<&[u8; 16], &[u8]> =
    TableDefinition::new("edits");
pub(crate) const META_TABLE: TableDefinition<&str, &[u8]> =
    TableDefinition::new("meta");

const META_KEY: &str = "meta";

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct CatalogMeta {
    pub name: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub app_version: String,
    pub schema_version: u32,
}

pub struct Catalog {
    db: Database,
    path: PathBuf,
}

impl Catalog {
    /// Open existing catalog or create a new one. Initialises tables and meta row.
    pub fn open_or_create(path: impl AsRef<Path>, name: &str) -> Result<Self, CatalogError> {
        let path = path.as_ref().to_path_buf();
        let existed = path.exists();
        let db = Database::create(&path)?;

        // Ensure all tables exist (idempotent), and write meta if new.
        {
            let write = db.begin_write()?;
            {
                let _ = write.open_table(PHOTOS_TABLE)?;
                let _ = write.open_table(EDITS_TABLE)?;
                let mut meta = write.open_table(META_TABLE)?;
                // FIXME(phase 2): existed-but-missing-meta currently silently
                // recreates meta, masking corruption. Consider returning
                // CatalogError::MissingMeta(path) instead.
                if !existed || meta.get(META_KEY)?.is_none() {
                    let m = CatalogMeta {
                        name: name.to_string(),
                        created_at: chrono::Utc::now(),
                        app_version: env!("CARGO_PKG_VERSION").to_string(),
                        schema_version: SCHEMA_VERSION,
                    };
                    let bytes = bincode::serialize(&m)?;
                    meta.insert(META_KEY, bytes.as_slice())?;
                }
            }
            write.commit()?;
        }

        // Verify schema version on existing catalogs.
        let read = db.begin_read()?;
        let meta_tbl = read.open_table(META_TABLE)?;
        let stored = meta_tbl.get(META_KEY)?.ok_or_else(|| CatalogError::Path(path.clone()))?;
        let meta: CatalogMeta = bincode::deserialize(stored.value())?;
        if meta.schema_version != SCHEMA_VERSION {
            return Err(CatalogError::SchemaVersion {
                found: meta.schema_version,
                expected: SCHEMA_VERSION,
            });
        }

        Ok(Self { db, path })
    }

    pub fn path(&self) -> &Path { &self.path }

    pub fn meta(&self) -> Result<CatalogMeta, CatalogError> {
        let read = self.db.begin_read()?;
        let meta_tbl = read.open_table(META_TABLE)?;
        let stored = meta_tbl.get(META_KEY)?.ok_or_else(|| CatalogError::Path(self.path.clone()))?;
        Ok(bincode::deserialize(stored.value())?)
    }

    /// Sibling-module access to the underlying redb database.
    #[allow(dead_code)] // used by photos/edits modules in Task 7
    pub(crate) fn db(&self) -> &Database { &self.db }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn creates_new_catalog_with_meta() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.chalkraw");
        let cat = Catalog::open_or_create(&path, "test").unwrap();
        let meta = cat.meta().unwrap();
        assert_eq!(meta.name, "test");
        assert_eq!(meta.schema_version, SCHEMA_VERSION);
    }

    #[test]
    fn reopening_preserves_meta() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.chalkraw");
        let first_created = {
            let cat = Catalog::open_or_create(&path, "first").unwrap();
            cat.meta().unwrap().created_at
        };
        let cat = Catalog::open_or_create(&path, "ignored-on-reopen").unwrap();
        let meta = cat.meta().unwrap();
        assert_eq!(meta.name, "first");
        assert_eq!(meta.created_at, first_created);
    }

    #[test]
    fn mismatched_schema_version_returns_error() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("bad.chalkraw");

        // Write a catalog with the wrong schema version directly.
        {
            let db = Database::create(&path).unwrap();
            let write = db.begin_write().unwrap();
            {
                let _ = write.open_table(PHOTOS_TABLE).unwrap();
                let _ = write.open_table(EDITS_TABLE).unwrap();
                let mut meta = write.open_table(META_TABLE).unwrap();
                let m = CatalogMeta {
                    name: "wrong".into(),
                    created_at: chrono::Utc::now(),
                    app_version: "0.0.0".into(),
                    schema_version: 99,
                };
                let bytes = bincode::serialize(&m).unwrap();
                meta.insert("meta", bytes.as_slice()).unwrap();
            }
            write.commit().unwrap();
        }
        // db drops here, releasing file lock.

        let err = Catalog::open_or_create(&path, "ignored").err().expect("expected SchemaVersion error");
        match err {
            CatalogError::SchemaVersion { found, expected } => {
                assert_eq!(found, 99);
                assert_eq!(expected, SCHEMA_VERSION);
            }
            other => panic!("expected SchemaVersion error, got {:?}", other),
        }
    }
}
