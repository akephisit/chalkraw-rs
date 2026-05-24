use crate::catalog::{Catalog, WATERMARKS_TABLE};
use crate::error::CatalogError;
use chalkraw_core::{WatermarkId, WatermarkPreset};

impl Catalog {
    pub fn insert_watermark(&self, preset: &WatermarkPreset) -> Result<(), CatalogError> {
        let bytes = bincode::serialize(preset)?;
        let write = self.db().begin_write()?;
        {
            let mut tbl = write.open_table(WATERMARKS_TABLE)?;
            tbl.insert(preset.id.as_bytes(), bytes.as_slice())?;
        }
        write.commit()?;
        Ok(())
    }

    pub fn list_watermarks(&self) -> Result<Vec<WatermarkPreset>, CatalogError> {
        use redb::{ReadableDatabase, ReadableTable};
        let read = self.db().begin_read()?;
        let tbl = read.open_table(WATERMARKS_TABLE)?;
        let mut out = Vec::new();
        for entry in tbl.iter()? {
            let (_, v): (_, redb::AccessGuard<&[u8]>) = entry?;
            let p: WatermarkPreset = bincode::deserialize(v.value())?;
            out.push(p);
        }
        out.sort_by_key(|b| std::cmp::Reverse(b.created_at));
        Ok(out)
    }

    pub fn delete_watermark(&self, id: WatermarkId) -> Result<(), CatalogError> {
        let write = self.db().begin_write()?;
        {
            let mut tbl = write.open_table(WATERMARKS_TABLE)?;
            tbl.remove(id.as_bytes())?;
        }
        write.commit()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chalkraw_core::{ImageLayer, WatermarkAnchor, WatermarkLayer, WatermarkPreset};
    use std::path::PathBuf;
    use tempfile::tempdir;

    fn cat() -> (tempfile::TempDir, Catalog) {
        let dir = tempdir().unwrap();
        let path = dir.path().join("t.chalkraw");
        let cat = Catalog::open_or_create(&path, "t").unwrap();
        (dir, cat)
    }

    #[test]
    fn insert_then_list_returns_watermark() {
        let (_dir, cat) = cat();
        let mut p = WatermarkPreset::new("Studio Logo".into());
        p.layers.push(WatermarkLayer::Image(ImageLayer {
            png_path: PathBuf::from("/logo.png"),
            anchor: WatermarkAnchor::BottomRight,
            size_pct: 15.0,
            opacity: 0.75,
            margin_pct: 3.0,
            rotation_deg: 0.0,
        }));
        cat.insert_watermark(&p).unwrap();
        let listed = cat.list_watermarks().unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].name, "Studio Logo");
    }

    #[test]
    fn delete_watermark_removes_it() {
        let (_dir, cat) = cat();
        let p = WatermarkPreset::new("Tmp".into());
        cat.insert_watermark(&p).unwrap();
        cat.delete_watermark(p.id).unwrap();
        assert!(cat.list_watermarks().unwrap().is_empty());
    }
}
