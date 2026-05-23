use crate::catalog::{Catalog, PRESETS_TABLE};
use crate::error::CatalogError;
use chalkraw_core::{Preset, PresetId};

impl Catalog {
    pub fn insert_preset(&self, preset: &Preset) -> Result<(), CatalogError> {
        let bytes = bincode::serialize(preset)?;
        let write = self.db().begin_write()?;
        {
            let mut tbl = write.open_table(PRESETS_TABLE)?;
            tbl.insert(preset.id.as_bytes(), bytes.as_slice())?;
        }
        write.commit()?;
        Ok(())
    }

    pub fn list_presets(&self) -> Result<Vec<Preset>, CatalogError> {
        use redb::{ReadableDatabase, ReadableTable};
        let read = self.db().begin_read()?;
        let tbl = read.open_table(PRESETS_TABLE)?;
        let mut out = Vec::new();
        for entry in tbl.iter()? {
            let (_, v): (_, redb::AccessGuard<&[u8]>) = entry?;
            let p: Preset = bincode::deserialize(v.value())?;
            out.push(p);
        }
        // Newest-first (UUID v7 sorts chronologically).
        out.sort_by_key(|p| std::cmp::Reverse(p.created_at));
        Ok(out)
    }

    pub fn delete_preset(&self, id: PresetId) -> Result<(), CatalogError> {
        let write = self.db().begin_write()?;
        {
            let mut tbl = write.open_table(PRESETS_TABLE)?;
            tbl.remove(id.as_bytes())?;
        }
        write.commit()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chalkraw_core::DevelopPreset;
    use tempfile::tempdir;

    fn cat() -> (tempfile::TempDir, Catalog) {
        let dir = tempdir().unwrap();
        let path = dir.path().join("t.chalkraw");
        let cat = Catalog::open_or_create(&path, "t").unwrap();
        (dir, cat)
    }

    #[test]
    fn insert_then_list_returns_preset() {
        let (_dir, cat) = cat();
        let p = Preset::new("Warm Pop".into(), DevelopPreset::default());
        cat.insert_preset(&p).unwrap();
        let listed = cat.list_presets().unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].name, "Warm Pop");
    }

    #[test]
    fn delete_preset_removes_it() {
        let (_dir, cat) = cat();
        let p = Preset::new("Cold".into(), DevelopPreset::default());
        cat.insert_preset(&p).unwrap();
        cat.delete_preset(p.id).unwrap();
        assert!(cat.list_presets().unwrap().is_empty());
    }
}
