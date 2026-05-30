use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub type CollectionId = Uuid;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Collection {
    pub id: CollectionId,
    pub name: String,
    pub created_at: DateTime<Utc>,
}

impl Collection {
    pub fn new(name: impl AsRef<str>) -> Self {
        Self {
            id: Uuid::now_v7(),
            name: normalise_collection_name(name),
            created_at: Utc::now(),
        }
    }

    pub fn rename(&mut self, name: impl AsRef<str>) {
        self.name = normalise_collection_name(name);
    }
}

fn normalise_collection_name(name: impl AsRef<str>) -> String {
    let trimmed = name.as_ref().trim();
    if trimmed.is_empty() {
        "Untitled Collection".to_string()
    } else {
        trimmed.to_string()
    }
}
