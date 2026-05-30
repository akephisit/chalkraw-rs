use crate::catalog::{Catalog, COLLECTIONS_TABLE, COLLECTION_PHOTOS_TABLE};
use crate::error::CatalogError;
use chalkraw_core::{Collection, CollectionId, PhotoId};
use redb::{ReadableDatabase, ReadableTable};

fn member_key(collection_id: CollectionId, photo_id: PhotoId) -> [u8; 32] {
    let mut key = [0u8; 32];
    key[..16].copy_from_slice(collection_id.as_bytes());
    key[16..].copy_from_slice(photo_id.as_bytes());
    key
}

fn key_collection_id(key: &[u8; 32]) -> CollectionId {
    CollectionId::from_bytes(key[..16].try_into().expect("collection id length"))
}

fn key_photo_id(key: &[u8; 32]) -> PhotoId {
    PhotoId::from_bytes(key[16..].try_into().expect("photo id length"))
}

impl Catalog {
    pub fn create_collection(&self, name: impl AsRef<str>) -> Result<Collection, CatalogError> {
        let collection = Collection::new(name);
        self.insert_collection(&collection)?;
        Ok(collection)
    }

    pub fn insert_collection(&self, collection: &Collection) -> Result<(), CatalogError> {
        let bytes = bincode::serialize(collection)?;
        let write = self.db().begin_write()?;
        {
            let mut tbl = write.open_table(COLLECTIONS_TABLE)?;
            tbl.insert(collection.id.as_bytes(), bytes.as_slice())?;
        }
        write.commit()?;
        Ok(())
    }

    pub fn rename_collection(
        &self,
        id: CollectionId,
        name: impl AsRef<str>,
    ) -> Result<Collection, CatalogError> {
        let mut collection = self.get_collection(id)?;
        collection.rename(name);
        self.insert_collection(&collection)?;
        Ok(collection)
    }

    pub fn get_collection(&self, id: CollectionId) -> Result<Collection, CatalogError> {
        let read = self.db().begin_read()?;
        let tbl = read.open_table(COLLECTIONS_TABLE)?;
        let v = tbl.get(id.as_bytes())?.ok_or(CatalogError::Path(
            self.path().join(format!("collection:{id}")),
        ))?;
        Ok(bincode::deserialize(v.value())?)
    }

    pub fn list_collections(&self) -> Result<Vec<Collection>, CatalogError> {
        let read = self.db().begin_read()?;
        let tbl = read.open_table(COLLECTIONS_TABLE)?;
        let mut out: Vec<Collection> = Vec::new();
        for entry in tbl.iter()? {
            let (_, v) = entry?;
            out.push(bincode::deserialize(v.value())?);
        }
        out.sort_by_key(|collection| collection.created_at);
        Ok(out)
    }

    pub fn delete_collection(&self, id: CollectionId) -> Result<(), CatalogError> {
        let write = self.db().begin_write()?;
        {
            let mut collections = write.open_table(COLLECTIONS_TABLE)?;
            collections.remove(id.as_bytes())?;
        }
        {
            let mut members = write.open_table(COLLECTION_PHOTOS_TABLE)?;
            let mut keys_to_remove = Vec::new();
            for entry in members.iter()? {
                let (key, _) = entry?;
                let key = *key.value();
                if key_collection_id(&key) == id {
                    keys_to_remove.push(key);
                }
            }
            for key in keys_to_remove {
                members.remove(&key)?;
            }
        }
        write.commit()?;
        Ok(())
    }

    pub fn add_photo_to_collection(
        &self,
        collection_id: CollectionId,
        photo_id: PhotoId,
    ) -> Result<(), CatalogError> {
        self.add_photos_to_collection(collection_id, &[photo_id])
    }

    pub fn add_photos_to_collection(
        &self,
        collection_id: CollectionId,
        photo_ids: &[PhotoId],
    ) -> Result<(), CatalogError> {
        let write = self.db().begin_write()?;
        {
            let mut tbl = write.open_table(COLLECTION_PHOTOS_TABLE)?;
            for photo_id in photo_ids {
                let key = member_key(collection_id, *photo_id);
                let empty: &[u8] = &[];
                tbl.insert(&key, empty)?;
            }
        }
        write.commit()?;
        Ok(())
    }

    pub fn remove_photo_from_collection(
        &self,
        collection_id: CollectionId,
        photo_id: PhotoId,
    ) -> Result<(), CatalogError> {
        let write = self.db().begin_write()?;
        {
            let mut tbl = write.open_table(COLLECTION_PHOTOS_TABLE)?;
            let key = member_key(collection_id, photo_id);
            tbl.remove(&key)?;
        }
        write.commit()?;
        Ok(())
    }

    pub fn list_collection_photo_ids(
        &self,
        collection_id: CollectionId,
    ) -> Result<Vec<PhotoId>, CatalogError> {
        let read = self.db().begin_read()?;
        let tbl = read.open_table(COLLECTION_PHOTOS_TABLE)?;
        let mut out = Vec::new();
        for entry in tbl.iter()? {
            let (key, _) = entry?;
            let key = *key.value();
            if key_collection_id(&key) == collection_id {
                out.push(key_photo_id(&key));
            }
        }
        Ok(out)
    }

    pub fn remove_photo_from_all_collections(&self, photo_id: PhotoId) -> Result<(), CatalogError> {
        let write = self.db().begin_write()?;
        {
            let mut tbl = write.open_table(COLLECTION_PHOTOS_TABLE)?;
            let mut keys_to_remove = Vec::new();
            for entry in tbl.iter()? {
                let (key, _) = entry?;
                let key = *key.value();
                if key_photo_id(&key) == photo_id {
                    keys_to_remove.push(key);
                }
            }
            for key in keys_to_remove {
                tbl.remove(&key)?;
            }
        }
        write.commit()?;
        Ok(())
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

    fn photo(path: &str, hash_byte: u8) -> Photo {
        Photo::new(
            PathBuf::from(path),
            [hash_byte; 32],
            100,
            100,
            ImageFormat::Jpeg,
        )
    }

    #[test]
    fn collections_roundtrip_membership_and_survive_reopen() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("t.chalkraw");
        let p1 = photo("/x/a.jpg", 1);
        let p2 = photo("/x/b.jpg", 2);
        let collection_id = {
            let cat = Catalog::open_or_create(&path, "t").unwrap();
            cat.insert_photos(&[p1.clone(), p2.clone()]).unwrap();
            let collection = cat.create_collection("  Downloads  ").unwrap();
            cat.add_photos_to_collection(collection.id, &[p1.id, p2.id])
                .unwrap();
            collection.id
        };

        let cat = Catalog::open_or_create(&path, "ignored").unwrap();
        let collections = cat.list_collections().unwrap();
        assert_eq!(collections.len(), 1);
        assert_eq!(collections[0].id, collection_id);
        assert_eq!(collections[0].name, "Downloads");

        let mut member_ids = cat.list_collection_photo_ids(collection_id).unwrap();
        member_ids.sort();
        let mut expected = vec![p1.id, p2.id];
        expected.sort();
        assert_eq!(member_ids, expected);
    }

    #[test]
    fn same_photo_can_belong_to_multiple_collections() {
        let (_dir, cat) = cat();
        let p = photo("/x/a.jpg", 1);
        cat.insert_photo(&p).unwrap();
        let first = cat.create_collection("First").unwrap();
        let second = cat.create_collection("Second").unwrap();

        cat.add_photo_to_collection(first.id, p.id).unwrap();
        cat.add_photo_to_collection(second.id, p.id).unwrap();

        assert_eq!(cat.list_collection_photo_ids(first.id).unwrap(), vec![p.id]);
        assert_eq!(
            cat.list_collection_photo_ids(second.id).unwrap(),
            vec![p.id]
        );
    }

    #[test]
    fn removing_photo_removes_collection_memberships() {
        let (_dir, cat) = cat();
        let p = photo("/x/a.jpg", 1);
        cat.insert_photo(&p).unwrap();
        let collection = cat.create_collection("Downloads").unwrap();
        cat.add_photo_to_collection(collection.id, p.id).unwrap();

        cat.remove_photo_with_edit(p.id).unwrap();

        assert!(cat
            .list_collection_photo_ids(collection.id)
            .unwrap()
            .is_empty());
    }
}
