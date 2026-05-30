pub mod collection;
pub mod edit;
pub mod photo;
pub mod watermark;

pub use collection::*;
pub use edit::*;
pub use photo::*;
pub use watermark::*;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collection_new_trims_name_and_assigns_id() {
        let before = chrono::Utc::now();
        let collection = Collection::new("  Downloads  ");
        let after = chrono::Utc::now();

        assert_eq!(collection.name, "Downloads");
        assert!(collection.created_at >= before && collection.created_at <= after);
        assert_eq!(collection.id.get_version(), Some(uuid::Version::SortRand));
    }

    #[test]
    fn collection_roundtrips_through_serde() {
        let collection = Collection::new("Portfolio");
        let bytes = bincode::serialize(&collection).unwrap();
        let back: Collection = bincode::deserialize(&bytes).unwrap();

        assert_eq!(collection, back);
    }
}
