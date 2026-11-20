//! Optional Graphiti index. Postgres remains the source of truth.
//! Deployment config, transport, queue policy, observation and forwarding have separate owners.
mod client;
mod config;
mod forwarder;
mod observation;
mod runtime;
mod status;
mod worker;

pub use client::{compatible_sidecar, forget_health, SidecarInfo};
pub use config::{MemoryConfig, HEALTH_TTL};
pub use forwarder::{
    chat, embeddings, internal_router, sanitize_chat, sanitize_embeddings, summarize_embeddings,
    ProxyKind, API_VERSION_HEADER, GRAPHITI_VERSION_HEADER, INTERNAL_BODY_LIMIT, INTERNAL_PREFIX,
    MAX_CALLS_PER_JOB, MAX_CHAT_TOKENS, MAX_EMBEDDING_BATCH, MAX_EMBEDDING_INPUT_CHARS,
    MAX_MESSAGES, MAX_MESSAGE_CHARS, MIN_CHAT_TOKENS, SERVICE_TOKEN_HEADER,
};
pub use observation::{observe, GraphObservation};
pub use runtime::{providers_ready, Runtime};
pub use status::{index_status, IndexStatus};
pub use worker::{collect_garbage, reconcile, spawn_worker, sweep_orphans, sync_one};
