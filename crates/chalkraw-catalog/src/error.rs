use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum CatalogError {
    #[error("redb error: {0}")]
    Redb(#[from] redb::Error),

    #[error("redb storage error: {0}")]
    Storage(#[from] redb::StorageError),

    #[error("redb transaction error: {0}")]
    Transaction(#[from] redb::TransactionError),

    #[error("redb commit error: {0}")]
    Commit(#[from] redb::CommitError),

    #[error("redb table error: {0}")]
    Table(#[from] redb::TableError),

    #[error("redb database error: {0}")]
    Database(#[from] redb::DatabaseError),

    #[error("serialization error: {0}")]
    Serde(#[from] Box<bincode::ErrorKind>),

    #[error("schema version {found} not supported (this build expects {expected})")]
    SchemaVersion { found: u32, expected: u32 },

    #[error("photo not found: {0}")]
    PhotoNotFound(uuid::Uuid),

    #[error("path error for {0:?}")]
    Path(PathBuf),
}
