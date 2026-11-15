//! Invite-code login, bearer sessions and role checks.
//!
//! Invite codes and session tokens are never stored in clear text: only their SHA-256 hex.

use axum::extract::{FromRequestParts, State};
use axum::http::request::Parts;
use axum::http::HeaderMap;
use axum::Json;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use sqlx::PgPool;

use crate::error::{ApiJson, AppError, AppResult};
use crate::AppState;

pub const SESSION_DAYS: i64 = 30;
pub const BOOTSTRAP_RESEARCH_ID: &str = "PI-001";

pub fn sha256_hex(data: impl AsRef<[u8]>) -> String {
    hex::encode(Sha256::digest(data.as_ref()))
}

/// 32 random bytes from the OS, hex encoded (64 chars).
pub fn new_token() -> String {
    let mut buf = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut buf);
    hex::encode(buf)
}

/// Human-friendly invite code, e.g. `K7QM-3XRP-WD9F` (no 0/O/1/I/L).
pub fn new_invite_code() -> String {
    const ALPHABET: &[u8] = b"23456789ABCDEFGHJKMNPQRSTUVWXYZ";
    let mut buf = [0u8; 12];
    rand::rngs::OsRng.fill_bytes(&mut buf);
    let chars: Vec<char> = buf
        .iter()
        .map(|b| ALPHABET[(*b as usize) % ALPHABET.len()] as char)
        .collect();
    chars
        .chunks(4)
        .map(|c| c.iter().collect::<String>())
        .collect::<Vec<_>>()
        .join("-")
}

/// Codes are compared case-insensitively and ignoring surrounding whitespace.
pub fn normalize_code(code: &str) -> String {
    code.trim().to_uppercase()
}

pub fn valid_track(s: &str) -> bool {
    matches!(s, "general" | "vocational" | "comprehensive")
}

pub fn valid_role(s: &str) -> bool {
    matches!(
        s,
        "teacher" | "pi" | "kb_manager" | "ra" | "rater" | "engineer"
    )
}

/// What an account may do beyond its own lesson preparation. Handlers ask for a capability
/// ([`AuthTeacher::require`]); which roles hold it is decided only in [`role_can`], so a new
/// capability (e.g. material management) is one enum variant and one match arm.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Capability {
    /// Invites, prompt versions, platform settings, teacher-case import: anything touching
    /// identities, research data or AI behaviour.
    Administer,
    /// Import, retire and browse the knowledge base (curriculum items) and the vocabulary list.
    ManageKnowledge,
    /// The account's own material library: upload, paste, annotate, delete. Held by every role
    /// today (everyone can prepare lessons); kept as a capability so it can be narrowed, for
    /// example during a pilot, in one place.
    UseMaterials,
}

impl Capability {
    pub const ALL: [Capability; 3] = [
        Capability::Administer,
        Capability::ManageKnowledge,
        Capability::UseMaterials,
    ];

    /// Name used in `GET /api/me` (`capabilities`).
    pub fn name(self) -> &'static str {
        match self {
            Capability::Administer => "administer",
            Capability::ManageKnowledge => "manage_knowledge",
            Capability::UseMaterials => "use_materials",
        }
    }

    fn denied(self) -> &'static str {
        match self {
            Capability::Administer => "只有計畫主持人可以使用這個功能。",
            Capability::ManageKnowledge => "只有計畫主持人或知識庫管理員可以使用這個功能。",
            Capability::UseMaterials => "這個帳號不能使用教材庫。",
        }
    }
}

/// The permission table (docs/API.md「角色與權限」).
pub fn role_can(role: &str, capability: Capability) -> bool {
    match capability {
        Capability::Administer => role == "pi",
        Capability::ManageKnowledge => matches!(role, "pi" | "kb_manager"),
        Capability::UseMaterials => valid_role(role),
    }
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Teacher {
    pub id: i64,
    pub research_id: String,
    pub school_track: String,
    pub role: String,
}

/// The authenticated teacher. Extracting it from a request enforces a valid bearer session.
#[derive(Debug, Clone)]
pub struct AuthTeacher(pub Teacher);

impl AuthTeacher {
    pub fn id(&self) -> i64 {
        self.0.id
    }
    pub fn track(&self) -> &str {
        &self.0.school_track
    }
    pub fn can(&self, capability: Capability) -> bool {
        role_can(&self.0.role, capability)
    }
    /// 403 unless this account's role holds `capability`.
    pub fn require(&self, capability: Capability) -> AppResult<()> {
        if self.can(capability) {
            Ok(())
        } else {
            Err(AppError::Forbidden(capability.denied().into()))
        }
    }
    pub fn require_pi(&self) -> AppResult<()> {
        self.require(Capability::Administer)
    }
}

/// The token from an `Authorization: Bearer <token>` header.
pub fn bearer_token(headers: &HeaderMap) -> Option<String> {
    let v = headers
        .get(axum::http::header::AUTHORIZATION)?
        .to_str()
        .ok()?;
    let (scheme, token) = v.split_once(' ')?;
    if !scheme.eq_ignore_ascii_case("bearer") {
        return None;
    }
    let token = token.trim();
    (!token.is_empty()).then(|| token.to_string())
}

impl FromRequestParts<AppState> for AuthTeacher {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let unauthorized = || AppError::Unauthorized("請先登入。".into());
        let token = bearer_token(&parts.headers).ok_or_else(unauthorized)?;
        authenticate(&state.db, &token).await
    }
}

/// The teacher a session token belongs to; 401 when it is unknown, revoked or expired.
pub async fn authenticate(db: &PgPool, token: &str) -> AppResult<AuthTeacher> {
    let teacher: Option<Teacher> = sqlx::query_as(
        "SELECT t.id, t.research_id, t.school_track, t.role
           FROM sessions s JOIN teachers t ON t.id = s.teacher_id
          WHERE s.token_hash = $1 AND s.revoked_at IS NULL AND s.expires_at > now()
            AND t.deleted_at IS NULL",
    )
    .bind(sha256_hex(token))
    .fetch_optional(db)
    .await?;
    teacher
        .map(AuthTeacher)
        .ok_or_else(|| AppError::Unauthorized("登入已失效，請重新登入。".into()))
}

/// Looks up a teacher by id for requests authenticated another way (a signed file link).
pub async fn teacher_by_id(db: &PgPool, id: i64) -> AppResult<AuthTeacher> {
    let teacher: Option<Teacher> = sqlx::query_as(
        "SELECT id, research_id, school_track, role FROM teachers
          WHERE id = $1 AND deleted_at IS NULL",
    )
    .bind(id)
    .fetch_optional(db)
    .await?;
    teacher
        .map(AuthTeacher)
        .ok_or_else(|| AppError::Unauthorized("登入已失效，請重新登入。".into()))
}

/// Creates the PI bootstrap invite if `ADMIN_BOOTSTRAP_CODE` is set and no invite with its hash exists.
pub async fn bootstrap_admin(db: &PgPool, code: Option<&str>) -> anyhow::Result<()> {
    let Some(code) = code.map(normalize_code).filter(|c| !c.is_empty()) else {
        return Ok(());
    };
    let inserted = sqlx::query(
        "INSERT INTO invite_codes (code_hash, research_id, school_track, role)
         VALUES ($1, $2, 'general', 'pi')
         ON CONFLICT (code_hash) DO NOTHING",
    )
    .bind(sha256_hex(&code))
    .bind(BOOTSTRAP_RESEARCH_ID)
    .execute(db)
    .await?
    .rows_affected();
    if inserted > 0 {
        tracing::info!("created bootstrap PI invite for {BOOTSTRAP_RESEARCH_ID}");
    }
    Ok(())
}

#[derive(Deserialize)]
pub struct LoginRequest {
    pub code: String,
}

#[derive(sqlx::FromRow)]
struct InviteRow {
    id: i64,
    research_id: String,
    school_track: String,
    role: String,
}

pub async fn login(
    State(state): State<AppState>,
    ApiJson(req): ApiJson<LoginRequest>,
) -> AppResult<Json<Value>> {
    let invalid = || AppError::Unauthorized("邀請碼無效，請確認後再試一次。".into());
    let code = normalize_code(&req.code);
    if code.is_empty() {
        return Err(invalid());
    }
    let mut tx = state.db.begin().await?;
    let invite: Option<InviteRow> = sqlx::query_as(
        "SELECT id, research_id, school_track, role FROM invite_codes
          WHERE code_hash = $1 AND revoked_at IS NULL",
    )
    .bind(sha256_hex(&code))
    .fetch_optional(&mut *tx)
    .await?;
    let invite = invite.ok_or_else(invalid)?;

    // First use creates the teacher; later logins (or other invites for the same
    // research_id) land on the same row.
    sqlx::query(
        "INSERT INTO teachers (research_id, school_track, role) VALUES ($1, $2, $3)
         ON CONFLICT (research_id) DO NOTHING",
    )
    .bind(&invite.research_id)
    .bind(&invite.school_track)
    .bind(&invite.role)
    .execute(&mut *tx)
    .await?;
    let teacher: Teacher = sqlx::query_as(
        "SELECT id, research_id, school_track, role FROM teachers
          WHERE research_id = $1 AND deleted_at IS NULL",
    )
    .bind(&invite.research_id)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(invalid)?;
    sqlx::query("UPDATE invite_codes SET used_at = now() WHERE id = $1 AND used_at IS NULL")
        .bind(invite.id)
        .execute(&mut *tx)
        .await?;

    let token = new_token();
    sqlx::query(
        "INSERT INTO sessions (token_hash, teacher_id, expires_at)
         VALUES ($1, $2, now() + make_interval(days => $3))",
    )
    .bind(sha256_hex(&token))
    .bind(teacher.id)
    .bind(SESSION_DAYS as i32)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;

    Ok(Json(json!({ "token": token, "teacher": teacher })))
}

pub async fn logout(State(state): State<AppState>, headers: HeaderMap) -> AppResult<Json<Value>> {
    if let Some(token) = bearer_token(&headers) {
        sqlx::query(
            "UPDATE sessions SET revoked_at = now() WHERE token_hash = $1 AND revoked_at IS NULL",
        )
        .bind(sha256_hex(&token))
        .execute(&state.db)
        .await?;
    }
    Ok(Json(json!({ "ok": true })))
}

/// The signed-in account plus the capabilities its role holds.
pub async fn me(teacher: AuthTeacher) -> Json<serde_json::Value> {
    let capabilities: Vec<&str> = Capability::ALL
        .into_iter()
        .filter(|c| teacher.can(*c))
        .map(Capability::name)
        .collect();
    let mut body = serde_json::to_value(&teacher.0).unwrap_or_default();
    body["capabilities"] = serde_json::json!(capabilities);
    Json(body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permission_table() {
        use Capability::*;
        for role in ["teacher", "ra", "rater", "engineer", "unknown"] {
            assert!(
                !role_can(role, Administer) && !role_can(role, ManageKnowledge),
                "{role}"
            );
        }
        for role in ["teacher", "pi", "kb_manager", "ra", "rater", "engineer"] {
            assert!(role_can(role, UseMaterials), "{role}");
        }
        assert!(!role_can("unknown", UseMaterials));
        assert!(role_can("pi", Administer) && role_can("pi", ManageKnowledge));
        assert!(role_can("kb_manager", ManageKnowledge));
        assert!(!role_can("kb_manager", Administer));
        assert!(valid_role("kb_manager") && !valid_role("admin"));
    }

    #[test]
    fn sha256_is_hex_of_digest() {
        assert_eq!(
            sha256_hex("abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn tokens_are_64_hex_chars_and_unique() {
        let a = new_token();
        let b = new_token();
        assert_eq!(a.len(), 64);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a, b);
    }

    #[test]
    fn invite_codes_are_grouped_and_normalized() {
        let c = new_invite_code();
        assert_eq!(c.len(), 14);
        assert_eq!(c.matches('-').count(), 2);
        assert_eq!(normalize_code(&format!("  {}  ", c.to_lowercase())), c);
    }

    #[test]
    fn bearer_parsing() {
        let mut h = HeaderMap::new();
        assert!(bearer_token(&h).is_none());
        h.insert("authorization", "Bearer abc".parse().unwrap());
        assert_eq!(bearer_token(&h).as_deref(), Some("abc"));
        h.insert("authorization", "Basic abc".parse().unwrap());
        assert!(bearer_token(&h).is_none());
    }
}
