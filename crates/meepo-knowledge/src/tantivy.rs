//! Tantivy full-text search index

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;
use tantivy::{
    collector::TopDocs,
    query::QueryParser,
    schema::*,
    Index, IndexWriter, ReloadPolicy, TantivyDocument,
};
use tracing::{debug, info};

use crate::sqlite::KnowledgeDb;

/// Search result with score and snippet
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub id: String,
    pub content: String,
    pub entity_type: String,
    pub score: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snippet: Option<String>,
}

/// Tantivy search index wrapper
pub struct TantivyIndex {
    index: Index,
    #[allow(dead_code)]
    schema: Schema,
    id_field: Field,
    content_field: Field,
    entity_type_field: Field,
    created_at_field: Field,
}

impl TantivyIndex {
    /// Create or open a Tantivy index
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self> {
        info!("Initializing Tantivy index at {:?}", path.as_ref());

        // Create directory if it doesn't exist
        std::fs::create_dir_all(path.as_ref())?;

        // Define schema
        let mut schema_builder = Schema::builder();
        let id_field = schema_builder.add_text_field("id", STRING | STORED);
        let content_field = schema_builder.add_text_field("content", TEXT | STORED);
        let entity_type_field = schema_builder.add_text_field("entity_type", STRING | STORED);
        let created_at_field = schema_builder.add_text_field("created_at", STRING | STORED);
        let schema = schema_builder.build();

        // Open or create index
        let index = if path.as_ref().join("meta.json").exists() {
            Index::open_in_dir(path.as_ref())?
        } else {
            Index::create_in_dir(path.as_ref(), schema.clone())?
        };

        debug!("Tantivy index initialized successfully");

        Ok(Self {
            index,
            schema,
            id_field,
            content_field,
            entity_type_field,
            created_at_field,
        })
    }

    /// Index a document
    pub fn index_document(
        &self,
        id: &str,
        content: &str,
        entity_type: &str,
        created_at: &str,
    ) -> Result<()> {
        let mut writer = self.get_writer()?;

        // Delete existing document with same ID (if any)
        let id_query = tantivy::query::TermQuery::new(
            tantivy::Term::from_field_text(self.id_field, id),
            tantivy::schema::IndexRecordOption::Basic,
        );
        let _ = writer.delete_query(Box::new(id_query));

        // Create document
        let mut doc = TantivyDocument::default();
        doc.add_text(self.id_field, id);
        doc.add_text(self.content_field, content);
        doc.add_text(self.entity_type_field, entity_type);
        doc.add_text(self.created_at_field, created_at);

        writer.add_document(doc)?;
        writer.commit()?;

        debug!("Indexed document: {} ({})", id, entity_type);
        Ok(())
    }

    /// Search the index
    pub fn search(&self, query_str: &str, limit: usize) -> Result<Vec<SearchResult>> {
        let reader = self
            .index
            .reader_builder()
            .reload_policy(ReloadPolicy::OnCommitWithDelay)
            .try_into()?;

        let searcher = reader.searcher();

        // Parse query
        let query_parser = QueryParser::for_index(&self.index, vec![self.content_field]);
        let query = query_parser
            .parse_query(query_str)
            .context("Failed to parse search query")?;

        // Search
        let top_docs = searcher.search(&query, &TopDocs::with_limit(limit))?;

        let mut results = Vec::new();
        for (score, doc_address) in top_docs {
            let retrieved_doc: TantivyDocument = searcher.doc(doc_address)?;

            let id = retrieved_doc
                .get_first(self.id_field)
                .and_then(|v: &tantivy::schema::OwnedValue| v.as_str())
                .unwrap_or("")
                .to_string();

            let content = retrieved_doc
                .get_first(self.content_field)
                .and_then(|v: &tantivy::schema::OwnedValue| v.as_str())
                .unwrap_or("")
                .to_string();

            let entity_type = retrieved_doc
                .get_first(self.entity_type_field)
                .and_then(|v: &tantivy::schema::OwnedValue| v.as_str())
                .unwrap_or("")
                .to_string();

            // Generate snippet (first 200 chars)
            let snippet = if content.len() > 200 {
                Some(format!("{}...", &content[..197]))
            } else {
                Some(content.clone())
            };

            results.push(SearchResult {
                id,
                content,
                entity_type,
                score,
                snippet,
            });
        }

        debug!("Search for '{}' returned {} results", query_str, results.len());
        Ok(results)
    }

    /// Delete a document by ID
    pub fn delete_document(&self, id: &str) -> Result<()> {
        let mut writer = self.get_writer()?;

        let id_query = tantivy::query::TermQuery::new(
            tantivy::Term::from_field_text(self.id_field, id),
            tantivy::schema::IndexRecordOption::Basic,
        );

        let _ = writer.delete_query(Box::new(id_query));
        writer.commit()?;

        debug!("Deleted document: {}", id);
        Ok(())
    }

    /// Reindex all entities from the database
    pub fn reindex_all(&self, db: &KnowledgeDb) -> Result<()> {
        info!("Reindexing all entities");

        let entities = db.get_all_entities()?;
        let mut writer = self.get_writer()?;

        // Delete all documents
        writer.delete_all_documents()?;

        let entity_count = entities.len();
        // Index all entities
        for entity in &entities {
            let content = format!(
                "{} {} {}",
                entity.name,
                entity.entity_type,
                entity.metadata
                    .as_ref()
                    .map(|m| m.to_string())
                    .unwrap_or_default()
            );

            let mut doc = TantivyDocument::default();
            doc.add_text(self.id_field, &entity.id);
            doc.add_text(self.content_field, &content);
            doc.add_text(self.entity_type_field, &entity.entity_type);
            doc.add_text(self.created_at_field, &entity.created_at.to_rfc3339());

            writer.add_document(doc)?;
        }

        writer.commit()?;

        info!("Reindexed {} entities", entity_count);
        Ok(())
    }

    /// Get index writer
    fn get_writer(&self) -> Result<IndexWriter> {
        // 50MB heap size for writer
        self.index
            .writer(50_000_000)
            .context("Failed to create index writer")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    #[test]
    fn test_index_and_search() -> Result<()> {
        let temp_path = env::temp_dir().join(format!("test_tantivy_index_{}", uuid::Uuid::new_v4()));
        let _ = std::fs::remove_dir_all(&temp_path);

        let index = TantivyIndex::new(&temp_path)?;

        // Index a document
        index.index_document(
            "test-id-1",
            "This is a test document about Rust programming",
            "concept",
            &chrono::Utc::now().to_rfc3339(),
        )?;

        index.index_document(
            "test-id-2",
            "Another document about Python programming",
            "concept",
            &chrono::Utc::now().to_rfc3339(),
        )?;

        // Search
        let results = index.search("Rust", 10)?;
        assert!(!results.is_empty());
        assert_eq!(results[0].id, "test-id-1");

        let results = index.search("Python", 10)?;
        assert!(!results.is_empty());
        assert_eq!(results[0].id, "test-id-2");

        // Search for common term
        let results = index.search("programming", 10)?;
        assert_eq!(results.len(), 2);

        let _ = std::fs::remove_dir_all(&temp_path);
        Ok(())
    }

    #[test]
    fn test_delete_document() -> Result<()> {
        let temp_path = env::temp_dir().join(format!("test_tantivy_delete_{}", uuid::Uuid::new_v4()));
        let _ = std::fs::remove_dir_all(&temp_path);

        let index = TantivyIndex::new(&temp_path)?;

        // Index a document
        index.index_document(
            "delete-test-id",
            "Document to be deleted",
            "concept",
            &chrono::Utc::now().to_rfc3339(),
        )?;

        // Verify it exists
        let results = index.search("deleted", 10)?;
        assert!(!results.is_empty());

        // Delete it
        index.delete_document("delete-test-id")?;

        // Verify it's gone
        let results = index.search("deleted", 10)?;
        assert!(results.is_empty());

        let _ = std::fs::remove_dir_all(&temp_path);
        Ok(())
    }
}
