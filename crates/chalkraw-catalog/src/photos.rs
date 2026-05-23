use crate::catalog::{Catalog, PHOTOS_TABLE};
use crate::error::CatalogError;
use chalkraw_core::{Photo, PhotoId};
use redb::{ReadableDatabase, ReadableTable};

impl Catalog {
    pub fn insert_photo(&self, photo: &Photo) -> Result<(), CatalogError> {
        let bytes = bincode::serialize(photo)?;
        let write = self.db().begin_write()?;
        {
            let mut tbl = write.open_table(PHOTOS_TABLE)?;
            tbl.insert(photo.id.as_bytes(), bytes.as_slice())?;
        }
        write.commit()?;
        Ok(())
    }

    pub fn get_photo(&self, id: PhotoId) -> Result<Photo, CatalogError> {
        let read = self.db().begin_read()?;
        let tbl = read.open_table(PHOTOS_TABLE)?;
        let v = tbl.get(id.as_bytes())?.ok_or(CatalogError::PhotoNotFound(id))?;
        Ok(bincode::deserialize(v.value())?)
    }

    pub fn list_photos(&self) -> Result<Vec<Photo>, CatalogError> {
        let read = self.db().begin_read()?;
        let tbl = read.open_table(PHOTOS_TABLE)?;
        let mut out = Vec::new();
        for entry in tbl.iter()? {
            let (_, v): (_, redb::AccessGuard<&[u8]>) = entry?;
            let p: Photo = bincode::deserialize(v.value())?;
            out.push(p);
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chalkraw_core::{ImageFormat, Photo};
    use std::path::PathBuf;
    use tempfile::tempdir;

    fn cat() -> (tempfile::TempDir, Catalog) {
        let dir = tempdir().unwrap();
        let path = dir.path().join("t.chalkraw");
        let cat = Catalog::open_or_create(&path, "t").unwrap();
        (dir, cat)
    }

    #[test]
    fn insert_then_get_returns_same_photo() {
        let (_dir, cat) = cat();
        let p = Photo::new(PathBuf::from("/x/a.jpg"), [0u8; 32], 100, 100, ImageFormat::Jpeg);
        cat.insert_photo(&p).unwrap();
        let back = cat.get_photo(p.id).unwrap();
        assert_eq!(p, back);
    }

    #[test]
    fn list_returns_all_inserted() {
        let (_dir, cat) = cat();
        let p1 = Photo::new(PathBuf::from("/x/a.jpg"), [0u8; 32], 1, 1, ImageFormat::Jpeg);
        let p2 = Photo::new(PathBuf::from("/x/b.jpg"), [1u8; 32], 2, 2, ImageFormat::Png);
        cat.insert_photo(&p1).unwrap();
        cat.insert_photo(&p2).unwrap();
        let list = cat.list_photos().unwrap();
        assert_eq!(list.len(), 2);
    }
}
