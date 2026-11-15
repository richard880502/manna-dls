-- Manna DLS Phase 2.4: embeddings of knowledge items and hybrid (keyword + vector) retrieval.
-- Contract: docs/API.md「Phase 2.4：語意索引與混合檢索」; evaluation: docs/RETRIEVAL_EVAL.md.
--
-- * Vectors live in a side table, one row per knowledge_items row (= one version of an item),
--   together with the model that produced them, its dimensions and a hash of the embedded
--   text. Retrieval only reads rows of the currently configured model and dimensions; rows of
--   another model are ignored and replaced by the next indexing run. Changing the model is
--   therefore a configuration change, not a migration.
-- * The column is an untyped `vector` so models of different sizes can coexist while one is
--   being replaced. pgvector indexes need a fixed size: the HNSW index covers the 768-dimension
--   rows (embeddinggemma-2, the evaluated model) through an expression. A model of another
--   size works without it (exact scan) and gets its own index in a later migration if needed.
-- * knowledge_items.embedding (vector(1024), reserved since 0001, never written) is dropped.
-- * retrieval_logs records how each retrieval was done.

ALTER TABLE knowledge_items DROP COLUMN embedding;

CREATE TABLE knowledge_embeddings (
    knowledge_item_id  BIGINT PRIMARY KEY REFERENCES knowledge_items (id) ON DELETE CASCADE,
    model              TEXT NOT NULL,              -- as configured, e.g. embeddinggemma-2
    dims               INT NOT NULL CHECK (dims > 0),
    content_hash       TEXT NOT NULL,              -- SHA-256 of the embedded text (with recipe version)
    embedding          vector NOT NULL,
    created_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK (vector_dims(embedding) = dims)
);
CREATE INDEX knowledge_embeddings_model_idx ON knowledge_embeddings (model, dims);
CREATE INDEX knowledge_embeddings_hnsw_768_idx ON knowledge_embeddings
    USING hnsw ((embedding::vector(768)) vector_cosine_ops) WHERE dims = 768;

-- One row per indexing run that had something to do (or was asked for by hand).
CREATE TABLE embedding_jobs (
    id           BIGSERIAL PRIMARY KEY,
    trigger      TEXT NOT NULL CHECK (trigger IN ('import', 'manual', 'startup')),
    model        TEXT NOT NULL,
    dims         INT NOT NULL,
    status       TEXT NOT NULL DEFAULT 'running' CHECK (status IN ('running', 'done', 'failed')),
    total        INT NOT NULL,                -- items to embed in this run
    embedded     INT NOT NULL DEFAULT 0,
    error        TEXT,                        -- why it stopped; never contains the API key
    started_by   BIGINT REFERENCES teachers (id),
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    finished_at  TIMESTAMPTZ
);

ALTER TABLE retrieval_logs
    ADD COLUMN method           TEXT NOT NULL DEFAULT 'keyword'
        CHECK (method IN ('keyword', 'hybrid', 'all')),
    ADD COLUMN embedding_model  TEXT,
    ADD COLUMN fallback_reason  TEXT,     -- why an enabled embedding model was not used
    -- [{item_code, keyword_rank, keyword_score, vector_rank, similarity, fused}] in result order
    ADD COLUMN candidates       JSONB NOT NULL DEFAULT '[]';
