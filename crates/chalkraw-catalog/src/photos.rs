use crate::catalog::{Catalog, EDITS_TABLE, PHOTOS_TABLE};
use crate::error::CatalogError;
use chalkraw_core::{Flag, ImageFormat, Photo, PhotoId};
use redb::{ReadableDatabase, ReadableTable};
use std::path::PathBuf;

pub struct PhotoPathUpdate {
    pub new_path: PathBuf,
    pub new_hash: [u8; 32],
    pub width: u32,
    pub height: u32,
    pub format: ImageFormat,
    pub thumbnail: Vec<u8>,
}

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

    pub fn insert_photos(&self, photos: &[Photo]) -> Result<(), CatalogError> {
        if photos.is_empty() {
            return Ok(());
        }
        let write = self.db().begin_write()?;
        {
            let mut tbl = write.open_table(PHOTOS_TABLE)?;
            for photo in photos {
                let bytes = bincode::serialize(photo)?;
                tbl.insert(photo.id.as_bytes(), bytes.as_slice())?;
            }
        }
        write.commit()?;
        Ok(())
    }

    pub fn get_photo(&self, id: PhotoId) -> Result<Photo, CatalogError> {
        let read = self.db().begin_read()?;
        let tbl = read.open_table(PHOTOS_TABLE)?;
        let v = tbl
            .get(id.as_bytes())?
            .ok_or(CatalogError::PhotoNotFound(id))?;
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

    pub fn update_flag(&self, id: PhotoId, flag: Flag) -> Result<(), CatalogError> {
        let mut photo = self.get_photo(id)?;
        photo.flag = flag;
        self.insert_photo(&photo)
    }

    pub fn remove_photo_with_edit(&self, id: PhotoId) -> Result<(), CatalogError> {
        let write = self.db().begin_write()?;
        {
            let mut photos = write.open_table(PHOTOS_TABLE)?;
            photos.remove(id.as_bytes())?;
        }
        {
            let mut edits = write.open_table(EDITS_TABLE)?;
            edits.remove(id.as_bytes())?;
        }
        write.commit()?;
        Ok(())
    }

    pub fn update_photo_path(
        &self,
        id: PhotoId,
        update: PhotoPathUpdate,
    ) -> Result<Photo, CatalogError> {
        let mut photo = self.get_photo(id)?;
        photo.original_path = update.new_path;
        photo.file_hash = update.new_hash;
        photo.width = update.width;
        photo.height = update.height;
        photo.format = update.format;
        photo.thumbnail = update.thumbnail;
        self.insert_photo(&photo)?;
        Ok(photo)
    }

    pub fn find_photo_by_hash(&self, hash: &[u8; 32]) -> Result<Option<Photo>, CatalogError> {
        let read = self.db().begin_read()?;
        let tbl = read.open_table(PHOTOS_TABLE)?;
        for entry in tbl.iter()? {
            let (_, v) = entry?;
            let p: Photo = bincode::deserialize(v.value())?;
            if p.file_hash == *hash {
                return Ok(Some(p));
            }
        }
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chalkraw_core::{EditState, Flag, ImageFormat, Photo};
    use std::fs;
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
        let p = Photo::new(
            PathBuf::from("/x/a.jpg"),
            [0u8; 32],
            100,
            100,
            ImageFormat::Jpeg,
        );
        cat.insert_photo(&p).unwrap();
        let back = cat.get_photo(p.id).unwrap();
        assert_eq!(p, back);
    }

    #[test]
    fn list_returns_all_inserted() {
        let (_dir, cat) = cat();
        let p1 = Photo::new(
            PathBuf::from("/x/a.jpg"),
            [0u8; 32],
            1,
            1,
            ImageFormat::Jpeg,
        );
        let p2 = Photo::new(PathBuf::from("/x/b.jpg"), [1u8; 32], 2, 2, ImageFormat::Png);
        cat.insert_photo(&p1).unwrap();
        cat.insert_photo(&p2).unwrap();
        let list = cat.list_photos().unwrap();
        assert_eq!(list.len(), 2);
    }

    #[test]
    fn find_photo_by_hash_returns_inserted() {
        let (_dir, cat) = cat();
        let mut p = Photo::new(
            PathBuf::from("/x/a.jpg"),
            [0u8; 32],
            1,
            1,
            ImageFormat::Jpeg,
        );
        p.file_hash = [7u8; 32];
        cat.insert_photo(&p).unwrap();
        let found = cat.find_photo_by_hash(&[7u8; 32]).unwrap();
        assert_eq!(found, Some(p));
        let missing = cat.find_photo_by_hash(&[0u8; 32]).unwrap();
        assert_eq!(missing, None);
    }

    #[test]
    fn update_flag_round_trips() {
        let (_dir, cat) = cat();
        let p = Photo::new(
            PathBuf::from("/x/a.jpg"),
            [0u8; 32],
            1,
            1,
            ImageFormat::Jpeg,
        );
        cat.insert_photo(&p).unwrap();
        cat.update_flag(p.id, Flag::Pick).unwrap();
        assert_eq!(cat.get_photo(p.id).unwrap().flag, Flag::Pick);
        cat.update_flag(p.id, Flag::Reject).unwrap();
        assert_eq!(cat.get_photo(p.id).unwrap().flag, Flag::Reject);
        cat.update_flag(p.id, Flag::None).unwrap();
        assert_eq!(cat.get_photo(p.id).unwrap().flag, Flag::None);
    }

    #[test]
    fn insert_then_list_preserves_thumbnail() {
        let (_dir, cat) = cat();
        let mut p = Photo::new(
            PathBuf::from("/x/a.jpg"),
            [0u8; 32],
            1,
            1,
            ImageFormat::Jpeg,
        );
        p.thumbnail = vec![1, 2, 3, 4, 5];
        cat.insert_photo(&p).unwrap();
        let listed = cat.list_photos().unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].thumbnail, vec![1, 2, 3, 4, 5]);
    }

    #[test]
    fn insert_photos_writes_all_in_one_call() {
        let (_dir, cat) = cat();
        let p1 = Photo::new(
            PathBuf::from("/x/a.jpg"),
            [1u8; 32],
            1,
            1,
            ImageFormat::Jpeg,
        );
        let p2 = Photo::new(PathBuf::from("/x/b.png"), [2u8; 32], 2, 2, ImageFormat::Png);
        cat.insert_photos(&[p1.clone(), p2.clone()]).unwrap();
        let listed = cat.list_photos().unwrap();
        assert_eq!(listed.len(), 2);
        assert!(listed.contains(&p1));
        assert!(listed.contains(&p2));
    }

    #[test]
    fn remove_photo_with_edit_removes_catalog_rows_not_original_file() {
        let (dir, cat) = cat();
        let original = dir.path().join("source.jpg");
        fs::write(&original, b"not a real image").unwrap();
        let p = Photo::new(original.clone(), [3u8; 32], 10, 12, ImageFormat::Jpeg);
        cat.insert_photo(&p).unwrap();

        let mut edit = EditState::default();
        edit.tone.exposure = 2.0;
        cat.upsert_edit(p.id, &edit).unwrap();

        cat.remove_photo_with_edit(p.id).unwrap();

        assert!(original.exists());
        assert!(matches!(
            cat.get_photo(p.id),
            Err(CatalogError::PhotoNotFound(id)) if id == p.id
        ));
        assert_eq!(cat.get_edit(p.id).unwrap().tone.exposure, 0.0);
    }

    #[test]
    fn update_photo_path_relinks_path_and_metadata() {
        let (_dir, cat) = cat();
        let p = Photo::new(
            PathBuf::from("/old/source.jpg"),
            [4u8; 32],
            10,
            12,
            ImageFormat::Jpeg,
        );
        cat.insert_photo(&p).unwrap();

        let updated = cat
            .update_photo_path(
                p.id,
                PhotoPathUpdate {
                    new_path: PathBuf::from("/new/source.png"),
                    new_hash: [5u8; 32],
                    width: 20,
                    height: 24,
                    format: ImageFormat::Png,
                    thumbnail: vec![9, 8, 7],
                },
            )
            .unwrap();

        assert_eq!(updated.id, p.id);
        assert_eq!(updated.original_path, PathBuf::from("/new/source.png"));
        assert_eq!(updated.file_hash, [5u8; 32]);
        assert_eq!(updated.width, 20);
        assert_eq!(updated.height, 24);
        assert_eq!(updated.format, ImageFormat::Png);
        assert_eq!(updated.thumbnail, vec![9, 8, 7]);
        assert_eq!(cat.get_photo(p.id).unwrap(), updated);
    }
}
