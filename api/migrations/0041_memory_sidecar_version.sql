-- Keep the actual independently deployed engine version in every job's audit trace.
ALTER TABLE memory_indexes ADD COLUMN leased_graphiti_version TEXT NOT NULL DEFAULT 'unknown';
