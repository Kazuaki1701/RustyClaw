use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use tantivy::collector::TopDocs;
use tantivy::query::QueryParser;
use tantivy::schema::{STORED, STRING, Schema, TEXT};
use tantivy::{Index, ReloadPolicy, TantivyDocument, doc};

pub struct SearchIndexManager {
    index: Index,
    schema: Schema,
}

impl SearchIndexManager {
    /// 検索インデックスの初期化または読み込み
    pub fn new<P: AsRef<Path>>(index_dir: P) -> Result<Self> {
        let index_dir = index_dir.as_ref();
        std::fs::create_dir_all(index_dir).context("Failed to create index directory")?;

        // スキーマの定義
        let mut schema_builder = Schema::builder();

        // パス（一意キー）
        let _path_field = schema_builder.add_text_field("path", STRING | STORED);
        // 本文（全文検索対象）
        let _content_field = schema_builder.add_text_field("content", TEXT);
        // 日付（日付フィルタ等用）
        let _date_field = schema_builder.add_text_field("date", STRING | STORED);

        let schema = schema_builder.build();

        // ディレクトリを開く、または新規作成
        let mmap_directory = tantivy::directory::MmapDirectory::open(index_dir)
            .context("Failed to open index directory with Mmap")?;

        let index = Index::open_or_create(mmap_directory, schema.clone())
            .context("Failed to open or create Tantivy index")?;

        Ok(Self { index, schema })
    }

    /// ファイルの内容をインデックスに追加（既に存在する場合は上書き）
    pub fn index_file(&self, path: &Path, content: &str, date: &str) -> Result<()> {
        let path_str = path.to_string_lossy().to_string();

        // RPi4 などの省メモリ環境向けにインデックスバッファサイズを 15MB に制限
        let mut index_writer = self
            .index
            .writer(15_000_000)
            .context("Failed to create index writer")?;

        let schema = &self.schema;
        let path_field = schema.get_field("path")?;
        let content_field = schema.get_field("content")?;
        let date_field = schema.get_field("date")?;

        // 既存 of 同じパスのドキュメントを一旦削除して重複を防止 (Upsertエミュレーション)
        let term = tantivy::Term::from_field_text(path_field, &path_str);
        index_writer.delete_term(term);

        // 新しいドキュメントを追加
        index_writer.add_document(doc!(
            path_field => path_str,
            content_field => content.to_string(),
            date_field => date.to_string(),
        ))?;

        index_writer
            .commit()
            .context("Failed to commit index changes")?;

        Ok(())
    }

    /// 全文検索を実行し、マッチしたファイルの PathBuf リストを返却する
    pub fn search(&self, query_str: &str) -> Result<Vec<PathBuf>> {
        let reader = self
            .index
            .reader_builder()
            .reload_policy(ReloadPolicy::OnCommitWithDelay)
            .try_into()
            .context("Failed to create index reader")?;

        let searcher = reader.searcher();

        let content_field = self.schema.get_field("content")?;
        let path_field = self.schema.get_field("path")?;

        // クエリパーサーの作成（本文フィールドを検索対象とする）
        let query_parser = QueryParser::for_index(&self.index, vec![content_field]);
        let query = query_parser
            .parse_query(query_str)
            .context("Failed to parse search query")?;

        // 最大 50 件のドキュメントを取得
        let top_docs = searcher
            .search(&query, &TopDocs::with_limit(50))
            .context("Failed to execute search")?;

        let mut matched_paths = Vec::new();
        for (_score, doc_address) in top_docs {
            let retrieved_doc: TantivyDocument = searcher
                .doc(doc_address)
                .context("Failed to retrieve document from searcher")?;

            if let Some(tantivy::schema::OwnedValue::Str(path_str)) = retrieved_doc.get_first(path_field) {
                matched_paths.push(PathBuf::from(path_str));
            }
        }

        Ok(matched_paths)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_search_index_manager() -> Result<()> {
        let temp_dir = tempfile::tempdir()?;
        let manager = SearchIndexManager::new(temp_dir.path())?;

        let doc_path = Path::new("/dummy/workspace/memory/logs/2026-05-26.md");
        manager.index_file(
            doc_path,
            "Today we performed debugging on the agent pipeline. SQLite WAL mode is active.",
            "2026-05-26",
        )?;

        // マッチするはずのクエリ
        let results = manager.search("debugging")?;
        assert_eq!(results.len(), 1);
        assert_eq!(results[0], doc_path);

        // マッチしないはずのクエリ
        let empty_results = manager.search("無関係キーワード")?;
        assert!(empty_results.is_empty());

        Ok(())
    }
}
