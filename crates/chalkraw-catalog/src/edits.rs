use crate::catalog::{Catalog, EDITS_TABLE};
use crate::error::CatalogError;
use chalkraw_core::{EditState, PhotoId};
use redb::ReadableDatabase;

impl Catalog {
    pub fn upsert_edit(&self, photo_id: PhotoId, edit: &EditState) -> Result<(), CatalogError> {
        let bytes = bincode::serialize(edit)?;
        let write = self.db().begin_write()?;
        {
            let mut tbl = write.open_table(EDITS_TABLE)?;
            tbl.insert(photo_id.as_bytes(), bytes.as_slice())?;
        }
        write.commit()?;
        Ok(())
    }

    /// Returns Default `EditState` if none was ever stored for this photo.
    pub fn get_edit(&self, photo_id: PhotoId) -> Result<EditState, CatalogError> {
        let read = self.db().begin_read()?;
        let tbl = read.open_table(EDITS_TABLE)?;
        match tbl.get(photo_id.as_bytes())? {
            Some(v) => Ok(bincode::deserialize::<EditState>(v.value())?),
            None => Ok(EditState::default()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chalkraw_core::{ImageFormat, Photo};
    use std::path::PathBuf;
    use tempfile::tempdir;

    #[test]
    fn missing_edit_returns_default() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("t.chalkraw");
        let cat = Catalog::open_or_create(&path, "t").unwrap();
        let p = Photo::new(PathBuf::from("/x/a.jpg"), [0u8; 32], 1, 1, ImageFormat::Jpeg);
        let e = cat.get_edit(p.id).unwrap();
        assert!(e.is_identity());
    }

    #[test]
    fn upsert_then_get_round_trips_exposure() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("t.chalkraw");
        let cat = Catalog::open_or_create(&path, "t").unwrap();
        let p = Photo::new(PathBuf::from("/x/a.jpg"), [0u8; 32], 1, 1, ImageFormat::Jpeg);
        cat.insert_photo(&p).unwrap();
        let mut e = EditState::default();
        e.tone.exposure = 1.25;
        cat.upsert_edit(p.id, &e).unwrap();
        let back = cat.get_edit(p.id).unwrap();
        assert_eq!(back.tone.exposure, 1.25);
    }
}
