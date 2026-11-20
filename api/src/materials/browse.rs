//! Browsing the library areas (docs/API.md「Phase 2.9」): the marketplace-style picker and pages.
//! Material cards (教材參考, 我的教材) and template cards (平台範本, 我的範本), with a search,
//! filters, facet counts and a keyset cursor. The admin lists use the same queries over all
//! platform rows and add who uploaded each and its size.
//!
//! Facets are counted over the rows that match the area and the search only (not the other
//! filters), so each chip shows how many rows choosing it would give.

use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::{PgPool, Postgres, QueryBuilder};

use super::catalog::{
    self, excerpt, FILE_TYPES, FILE_TYPE_SQL, OWNER_ME, OWNER_PLATFORM_MATERIAL,
    OWNER_PLATFORM_TEMPLATE, REFERENCE_CATEGORIES, SAMPLE_CATEGORIES, SCOPE_PLATFORM,
};
use crate::error::{AppError, AppResult};
use crate::sop::word_count;
use crate::worksheet::template::WsTemplate;

pub const DEFAULT_LIMIT: i64 = 24;
pub const MAX_LIMIT: i64 = 60;
/// A longer search is cut here.
pub const MAX_QUERY_CHARS: usize = 200;
/// At most this many tags are counted in the facets (the most used first).
pub const MAX_TAG_FACETS: i64 = 40;
/// How many section titles a template card shows.
pub const CARD_SECTION_TITLES: usize = 6;
/// `lang` values (a template's `instruction_language`), in facet order.
pub const LANGS: [&str; 3] = ["英文", "中文", "中英並用"];

// ---------------------------------------------------------------- query parameters

/// The query string as pairs (`tag` may repeat).
pub struct Params(pub Vec<(String, String)>);

impl Params {
    /// The last value of `key`, trimmed; `None` when absent or blank.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.0
            .iter()
            .rev()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.trim())
            .filter(|v| !v.is_empty())
    }

    /// Every non-blank value of `key`, trimmed, in order, without repeats.
    pub fn all(&self, key: &str) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for (_, v) in self.0.iter().filter(|(k, _)| k == key) {
            let v = v.trim();
            if !v.is_empty() && !out.iter().any(|o| o == v) {
                out.push(v.to_string());
            }
        }
        out
    }

    fn search(&self) -> String {
        self.get("q")
            .map(|q| q.chars().take(MAX_QUERY_CHARS).collect::<String>())
            .unwrap_or_default()
    }

    fn tags(&self) -> Vec<String> {
        self.all("tag")
    }

    fn limit(&self) -> AppResult<i64> {
        match self.get("limit") {
            None => Ok(DEFAULT_LIMIT),
            Some(raw) => raw
                .parse::<i64>()
                .map(|n| n.clamp(1, MAX_LIMIT))
                .map_err(|_| AppError::bad_request("limit 需為 1 到 60 的數字。")),
        }
    }

    fn sort(&self) -> AppResult<Sort> {
        match self.get("sort") {
            None | Some("new") => Ok(Sort::New),
            Some("title") => Ok(Sort::Title),
            Some(_) => Err(AppError::bad_request("sort 需為 new 或 title。")),
        }
    }

    fn cursor(&self, sort: Sort) -> AppResult<Option<Cursor>> {
        self.get("cursor")
            .map(|raw| {
                Cursor::parse(raw, sort)
                    .ok_or_else(|| AppError::bad_request("cursor 不正確；請從第一頁重新載入。"))
            })
            .transpose()
    }

    fn flag(&self, key: &str) -> AppResult<bool> {
        match self.get(key) {
            None | Some("false") | Some("0") => Ok(false),
            Some("true") | Some("1") => Ok(true),
            Some(_) => Err(AppError::bad_request(format!("{key} 需為 true 或 false。"))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sort {
    /// Newest first (`created_at, id` descending).
    New,
    /// By title (`title, id` ascending).
    Title,
}

/// Where the next page starts: after the last row of this one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cursor {
    New(DateTime<Utc>, i64),
    Title(String, i64),
}

impl Cursor {
    /// `n.<created_at µs>.<id>` or `t.<hex title>.<id>`; opaque to the client.
    pub fn encode(&self) -> String {
        match self {
            Self::New(at, id) => format!("n.{}.{id}", at.timestamp_micros()),
            Self::Title(title, id) => format!("t.{}.{id}", hex::encode(title.as_bytes())),
        }
    }

    pub fn parse(raw: &str, sort: Sort) -> Option<Self> {
        let mut parts = raw.splitn(3, '.');
        let (tag, key, id) = (parts.next()?, parts.next()?, parts.next()?);
        let id: i64 = id.parse().ok()?;
        match (tag, sort) {
            ("n", Sort::New) => Some(Self::New(
                DateTime::from_timestamp_micros(key.parse().ok()?)?,
                id,
            )),
            ("t", Sort::Title) => Some(Self::Title(
                String::from_utf8(hex::decode(key).ok()?).ok()?,
                id,
            )),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------- cards

#[derive(Debug, Clone, Serialize)]
pub struct MaterialCard {
    pub id: i64,
    pub title: String,
    /// `personal` | `platform`
    pub scope: String,
    pub category: String,
    pub tags: Vec<String>,
    pub kind: String,
    /// `pdf` | `docx` | `text` | `image` | `audio` | `video` | `link`
    pub file_type: &'static str,
    pub page_count: Option<i32>,
    pub paragraph_count: i32,
    pub word_count: usize,
    /// The first ~280 characters of the text, whitespace collapsed ("" without text).
    pub excerpt: String,
    pub text_status: String,
    pub looks_like_worksheet: bool,
    pub created_at: DateTime<Utc>,
    /// 「Manna 知識庫」 or 「我」.
    pub owner_label: &'static str,
    /// Text recognition of a scanned file (docs/API.md「Phase 2.10」).
    pub ocr: crate::materials::ocr::MaterialOcr,
    /// `file` | `ocr`
    pub text_source: String,
    /// Admin lists only: the research id of the admin who uploaded it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uploaded_by: Option<String>,
    /// Admin lists only: bytes of the stored file (the text's bytes for pasted text).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size_bytes: Option<i64>,
}

#[derive(sqlx::FromRow)]
struct MaterialCardRow {
    id: i64,
    title: String,
    scope: String,
    category: String,
    tags: Vec<String>,
    kind: String,
    ext: Option<String>,
    page_count: Option<i32>,
    paragraph_count: i32,
    text: String,
    text_status: String,
    looks_like_worksheet: Option<bool>,
    created_at: DateTime<Utc>,
    size_bytes: i64,
    uploaded_by: String,
    text_source: String,
    ocr_status: String,
    ocr_done_pages: i32,
    ocr_total_pages: i32,
    ocr_error: Option<String>,
}

impl MaterialCardRow {
    fn card(self, admin: bool) -> MaterialCard {
        MaterialCard {
            file_type: catalog::file_type(&self.kind, self.ext.as_deref()),
            owner_label: if self.scope == SCOPE_PLATFORM {
                OWNER_PLATFORM_MATERIAL
            } else {
                OWNER_ME
            },
            word_count: word_count(&self.text),
            excerpt: excerpt(&self.text),
            looks_like_worksheet: self
                .looks_like_worksheet
                .unwrap_or_else(|| super::detect::looks_like_worksheet(&self.text)),
            uploaded_by: admin.then_some(self.uploaded_by),
            ocr: crate::materials::ocr::MaterialOcr {
                status: self.ocr_status,
                done_pages: self.ocr_done_pages,
                total_pages: self.ocr_total_pages,
                error: self.ocr_error,
            },
            text_source: self.text_source,
            size_bytes: admin.then_some(self.size_bytes),
            id: self.id,
            title: self.title,
            scope: self.scope,
            category: self.category,
            tags: self.tags,
            kind: self.kind,
            page_count: self.page_count,
            paragraph_count: self.paragraph_count,
            text_status: self.text_status,
            created_at: self.created_at,
        }
    }

    fn cursor(&self, sort: Sort) -> Cursor {
        match sort {
            Sort::New => Cursor::New(self.created_at, self.id),
            Sort::Title => Cursor::Title(self.title.clone(), self.id),
        }
    }
}

const MATERIAL_SELECT: &str = "SELECT m.id, m.title, m.scope, m.category, m.tags, m.kind, m.ext,
       m.page_count, jsonb_array_length(m.paragraphs) AS paragraph_count, m.text, m.text_status,
       m.looks_like_worksheet, m.created_at, m.size_bytes, u.research_id AS uploaded_by,
       m.text_source, m.ocr_status, m.ocr_done_pages, m.ocr_total_pages, m.ocr_error
  FROM materials m JOIN teachers u ON u.id = m.teacher_id";

/// The original file of a template (its sample material), on a template card.
#[derive(Debug, Clone, Serialize)]
pub struct CardSource {
    pub id: i64,
    pub title: String,
    pub file_type: &'static str,
    pub page_count: Option<i32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TemplateCard {
    pub id: i64,
    pub title: String,
    /// `personal` | `platform`
    pub scope: String,
    /// `extracting` | `ready` | `failed`
    pub status: String,
    pub category: String,
    pub tags: Vec<String>,
    /// "" while there is no version yet.
    pub worksheet_kind: String,
    pub instruction_language: String,
    pub section_count: usize,
    /// The first six `title_pattern`s.
    pub section_titles: Vec<String>,
    pub source_material: Option<CardSource>,
    pub updated_at: DateTime<Utc>,
    /// 「Manna 範本」 or 「我」.
    pub owner_label: &'static str,
    /// Admin lists only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uploaded_by: Option<String>,
    /// Admin lists only: bytes of the sample's file (0 when it is gone).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size_bytes: Option<i64>,
    /// Admin lists only: why the extraction failed (null otherwise).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<Option<String>>,
}

#[derive(sqlx::FromRow)]
struct TemplateCardRow {
    id: i64,
    title: String,
    scope: String,
    status: String,
    error: Option<String>,
    category: String,
    tags: Vec<String>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    content: Option<serde_json::Value>,
    material_id: Option<i64>,
    material_title: Option<String>,
    material_kind: Option<String>,
    material_ext: Option<String>,
    material_page_count: Option<i32>,
    material_size: Option<i64>,
    uploaded_by: String,
}

impl TemplateCardRow {
    fn card(self, admin: bool) -> TemplateCard {
        let content: Option<WsTemplate> = self.content.and_then(|c| serde_json::from_value(c).ok());
        let source_material = match (self.material_id, self.material_title) {
            (Some(id), Some(title)) => Some(CardSource {
                id,
                title,
                file_type: catalog::file_type(
                    self.material_kind.as_deref().unwrap_or_default(),
                    self.material_ext.as_deref(),
                ),
                page_count: self.material_page_count,
            }),
            _ => None,
        };
        TemplateCard {
            owner_label: if self.scope == SCOPE_PLATFORM {
                OWNER_PLATFORM_TEMPLATE
            } else {
                OWNER_ME
            },
            worksheet_kind: content
                .as_ref()
                .map(|c| c.worksheet_kind.clone())
                .unwrap_or_default(),
            instruction_language: content
                .as_ref()
                .map(|c| c.instruction_language.clone())
                .unwrap_or_default(),
            section_count: content.as_ref().map_or(0, |c| c.sections.len()),
            section_titles: content
                .as_ref()
                .map(|c| {
                    c.sections
                        .iter()
                        .take(CARD_SECTION_TITLES)
                        .map(|s| s.title_pattern.clone())
                        .collect()
                })
                .unwrap_or_default(),
            source_material,
            uploaded_by: admin.then_some(self.uploaded_by),
            size_bytes: admin.then_some(self.material_size.unwrap_or(0)),
            error: admin.then_some(self.error),
            id: self.id,
            title: self.title,
            scope: self.scope,
            status: self.status,
            category: self.category,
            tags: self.tags,
            updated_at: self.updated_at,
        }
    }

    fn cursor(&self, sort: Sort) -> Cursor {
        match sort {
            Sort::New => Cursor::New(self.created_at, self.id),
            Sort::Title => Cursor::Title(self.title.clone(), self.id),
        }
    }
}

const TEMPLATE_FROM: &str = " FROM worksheet_templates t
  JOIN teachers u ON u.id = t.teacher_id
  LEFT JOIN materials m ON m.id = t.source_material_id
  LEFT JOIN LATERAL (SELECT content FROM worksheet_template_versions
                      WHERE template_id = t.id ORDER BY version_no DESC LIMIT 1) v ON TRUE";

const TEMPLATE_COLUMNS: &str = "SELECT t.id, t.title, t.scope, t.status, t.error, t.category,
       t.tags, t.created_at, t.updated_at, v.content, m.id AS material_id,
       m.title AS material_title, m.kind AS material_kind, m.ext AS material_ext,
       m.page_count AS material_page_count, m.size_bytes AS material_size,
       u.research_id AS uploaded_by";

/// One material's card (for the admin's create and edit answers).
pub async fn material_card(db: &PgPool, id: i64, admin: bool) -> AppResult<MaterialCard> {
    let row: Option<MaterialCardRow> =
        sqlx::query_as(&format!("{MATERIAL_SELECT} WHERE m.id = $1"))
            .bind(id)
            .fetch_optional(db)
            .await?;
    row.map(|r| r.card(admin))
        .ok_or_else(|| AppError::not_found("找不到這份教材。"))
}

/// One template's card.
pub async fn template_card(db: &PgPool, id: i64, admin: bool) -> AppResult<TemplateCard> {
    let row: Option<TemplateCardRow> = sqlx::query_as(&format!(
        "{TEMPLATE_COLUMNS}{TEMPLATE_FROM} WHERE t.id = $1"
    ))
    .bind(id)
    .fetch_optional(db)
    .await?;
    row.map(|r| r.card(admin))
        .ok_or_else(|| AppError::not_found("找不到這個版型。"))
}

// ---------------------------------------------------------------- pages

#[derive(Debug, Clone, Serialize)]
pub struct FacetCount {
    pub key: String,
    pub count: i64,
}

#[derive(Debug, Serialize)]
pub struct MaterialFacets {
    pub categories: Vec<FacetCount>,
    pub tags: Vec<FacetCount>,
    pub file_types: Vec<FacetCount>,
}

#[derive(Debug, Serialize)]
pub struct TemplateFacets {
    pub categories: Vec<FacetCount>,
    pub tags: Vec<FacetCount>,
    pub langs: Vec<FacetCount>,
}

#[derive(Debug, Serialize)]
pub struct Page<T, F> {
    pub items: Vec<T>,
    pub next_cursor: Option<String>,
    pub facets: F,
    /// Rows matching the area, the search and every filter.
    pub total: i64,
}

/// Category facets: the known list in its order first, then any other value; empty ones left
/// out.
fn order_categories(known: &[&str], mut counts: Vec<FacetCount>) -> Vec<FacetCount> {
    counts.retain(|c| !c.key.is_empty() && c.count > 0);
    counts.sort_by(|a, b| {
        let rank = |k: &str| known.iter().position(|c| *c == k).unwrap_or(known.len());
        (rank(&a.key), &a.key).cmp(&(rank(&b.key), &b.key))
    });
    counts
}

/// Facets in a fixed order (file types, languages), only those that occur.
fn order_fixed(order: &[&str], counts: Vec<FacetCount>) -> Vec<FacetCount> {
    order
        .iter()
        .filter_map(|k| counts.iter().find(|c| c.key == *k && c.count > 0).cloned())
        .collect()
}

#[derive(sqlx::FromRow)]
struct Counted {
    key: Option<String>,
    count: i64,
}

async fn facet_counts(
    db: &PgPool,
    mut qb: QueryBuilder<'_, Postgres>,
    tail: &str,
) -> sqlx::Result<Vec<FacetCount>> {
    qb.push(tail);
    let rows: Vec<Counted> = qb.build_query_as().fetch_all(db).await?;
    Ok(rows
        .into_iter()
        .filter_map(|r| {
            r.key.map(|key| FacetCount {
                key,
                count: r.count,
            })
        })
        .collect())
}

// ---------------------------------------------------------------- materials

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MaterialArea {
    /// 教材參考: platform materials.
    Reference,
    /// 我的教材: the teacher's own materials.
    Mine(i64),
    /// The admin list: every platform material (same rows as `Reference`).
    Admin,
}

#[derive(Debug, Clone)]
pub struct MaterialQuery {
    pub area: MaterialArea,
    pub q: String,
    pub category: Option<String>,
    pub tags: Vec<String>,
    pub file_type: Option<String>,
    pub fits_lesson: bool,
    pub sort: Sort,
    pub cursor: Option<Cursor>,
    pub limit: i64,
}

impl MaterialQuery {
    /// `GET /api/browse/materials`: `area=reference|mine` (default `reference`), `q`, `category`,
    /// `tag` (repeatable; all must match), `file_type`, `fits_lesson=true`, `sort=new|title`,
    /// `cursor`, `limit` (24; at most 60).
    pub fn teacher(params: &Params, teacher_id: i64) -> AppResult<Self> {
        let area = match params.get("area") {
            None | Some("reference") => MaterialArea::Reference,
            Some("mine") => MaterialArea::Mine(teacher_id),
            Some(_) => return Err(AppError::bad_request("area 需為 reference 或 mine。")),
        };
        Self::parse(params, area)
    }

    /// The admin list (`GET /api/admin/library?kind=materials`): every platform material.
    pub fn admin(params: &Params) -> AppResult<Self> {
        Self::parse(params, MaterialArea::Admin)
    }

    fn parse(params: &Params, area: MaterialArea) -> AppResult<Self> {
        let file_type = params.get("file_type").map(str::to_string);
        if let Some(t) = &file_type {
            if !FILE_TYPES.contains(&t.as_str()) {
                return Err(AppError::bad_request(format!(
                    "file_type 需為 {} 其中之一。",
                    FILE_TYPES.join("、")
                )));
            }
        }
        let sort = params.sort()?;
        Ok(Self {
            area,
            q: params.search(),
            category: params.get("category").map(str::to_string),
            tags: params.tags(),
            file_type,
            fits_lesson: params.flag("fits_lesson")?,
            sort,
            cursor: params.cursor(sort)?,
            limit: params.limit()?,
        })
    }

    /// `WHERE` for the area and the search (what the facets count over).
    fn push_base(&self, qb: &mut QueryBuilder<'_, Postgres>) {
        qb.push(" WHERE m.purpose = 'material' AND ");
        match self.area {
            MaterialArea::Reference | MaterialArea::Admin => {
                qb.push("m.scope = 'platform'");
            }
            MaterialArea::Mine(teacher_id) => {
                qb.push("m.scope = 'personal' AND m.teacher_id = ");
                qb.push_bind(teacher_id);
            }
        }
        if !self.q.is_empty() {
            let q = self.q.to_lowercase();
            qb.push(" AND (strpos(lower(m.title), ");
            qb.push_bind(q.clone());
            qb.push(") > 0 OR strpos(lower(m.source_note), ");
            qb.push_bind(q.clone());
            qb.push(") > 0 OR strpos(lower(m.text), ");
            qb.push_bind(q.clone());
            qb.push(") > 0 OR EXISTS (SELECT 1 FROM unnest(m.tags) AS tg WHERE strpos(lower(tg), ");
            qb.push_bind(q);
            qb.push(") > 0))");
        }
    }

    /// The other filters.
    fn push_filters(&self, qb: &mut QueryBuilder<'_, Postgres>) {
        if let Some(category) = &self.category {
            qb.push(" AND m.category = ");
            qb.push_bind(category.clone());
        }
        if !self.tags.is_empty() {
            qb.push(" AND m.tags @> ");
            qb.push_bind(self.tags.clone());
        }
        if let Some(file_type) = &self.file_type {
            qb.push(format!(" AND {FILE_TYPE_SQL} = "));
            qb.push_bind(file_type.clone());
        }
        if self.fits_lesson {
            qb.push(
                " AND m.text_status = 'ready'
                  AND jsonb_array_length(m.paragraphs) >= 1",
            );
        }
    }
}

/// One page of material cards with the facets and the total.
pub async fn materials(
    db: &PgPool,
    query: &MaterialQuery,
) -> AppResult<Page<MaterialCard, MaterialFacets>> {
    let admin = query.area == MaterialArea::Admin;
    let mut qb = QueryBuilder::new(MATERIAL_SELECT);
    query.push_base(&mut qb);
    query.push_filters(&mut qb);
    match &query.cursor {
        Some(Cursor::New(at, id)) => {
            qb.push(" AND (m.created_at, m.id) < (");
            qb.push_bind(*at);
            qb.push(", ");
            qb.push_bind(*id);
            qb.push(")");
        }
        Some(Cursor::Title(title, id)) => {
            qb.push(" AND (m.title, m.id) > (");
            qb.push_bind(title.clone());
            qb.push(", ");
            qb.push_bind(*id);
            qb.push(")");
        }
        None => {}
    }
    qb.push(match query.sort {
        Sort::New => " ORDER BY m.created_at DESC, m.id DESC",
        Sort::Title => " ORDER BY m.title ASC, m.id ASC",
    });
    qb.push(" LIMIT ");
    qb.push_bind(query.limit + 1);
    let mut rows: Vec<MaterialCardRow> = qb.build_query_as().fetch_all(db).await?;
    let next_cursor = if rows.len() as i64 > query.limit {
        rows.truncate(query.limit as usize);
        rows.last().map(|r| r.cursor(query.sort).encode())
    } else {
        None
    };

    let mut total = QueryBuilder::new("SELECT COUNT(*) FROM materials m");
    query.push_base(&mut total);
    query.push_filters(&mut total);
    let total: i64 = total.build_query_scalar().fetch_one(db).await?;

    let mut categories =
        QueryBuilder::new("SELECT m.category AS key, COUNT(*) AS count FROM materials m");
    query.push_base(&mut categories);
    let categories = facet_counts(db, categories, " GROUP BY m.category").await?;
    let mut tags = QueryBuilder::new(
        "SELECT tg AS key, COUNT(*) AS count FROM materials m CROSS JOIN LATERAL unnest(m.tags) AS tg",
    );
    query.push_base(&mut tags);
    let tags = facet_counts(
        db,
        tags,
        &format!(" GROUP BY tg ORDER BY COUNT(*) DESC, tg LIMIT {MAX_TAG_FACETS}"),
    )
    .await?;
    let mut file_types = QueryBuilder::new(format!(
        "SELECT {FILE_TYPE_SQL} AS key, COUNT(*) AS count FROM materials m"
    ));
    query.push_base(&mut file_types);
    let file_types = facet_counts(db, file_types, " GROUP BY 1").await?;

    Ok(Page {
        items: rows.into_iter().map(|r| r.card(admin)).collect(),
        next_cursor,
        facets: MaterialFacets {
            categories: order_categories(&REFERENCE_CATEGORIES, categories),
            tags,
            file_types: order_fixed(&FILE_TYPES, file_types),
        },
        total,
    })
}

// ---------------------------------------------------------------- templates

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TemplateArea {
    /// 平台範本: platform templates that are `ready`.
    Platform,
    /// 我的範本: the teacher's own templates, any status.
    Mine(i64),
    /// The admin list: every platform template, any status.
    Admin,
}

#[derive(Debug, Clone)]
pub struct TemplateQuery {
    pub area: TemplateArea,
    pub q: String,
    pub category: Option<String>,
    pub tags: Vec<String>,
    pub lang: Option<String>,
    pub sort: Sort,
    pub cursor: Option<Cursor>,
    pub limit: i64,
}

impl TemplateQuery {
    /// `GET /api/browse/templates`: `area=platform|mine` (default `platform`), `q`, `category`,
    /// `tag` (repeatable), `lang=英文|中文|中英並用`, `sort=new|title`, `cursor`, `limit`.
    pub fn teacher(params: &Params, teacher_id: i64) -> AppResult<Self> {
        let area = match params.get("area") {
            None | Some("platform") => TemplateArea::Platform,
            Some("mine") => TemplateArea::Mine(teacher_id),
            Some(_) => return Err(AppError::bad_request("area 需為 platform 或 mine。")),
        };
        Self::parse(params, area)
    }

    pub fn admin(params: &Params) -> AppResult<Self> {
        Self::parse(params, TemplateArea::Admin)
    }

    fn parse(params: &Params, area: TemplateArea) -> AppResult<Self> {
        let lang = params.get("lang").map(str::to_string);
        if let Some(l) = &lang {
            if !LANGS.contains(&l.as_str()) {
                return Err(AppError::bad_request(format!(
                    "lang 需為 {} 其中之一。",
                    LANGS.join("、")
                )));
            }
        }
        let sort = params.sort()?;
        Ok(Self {
            area,
            q: params.search(),
            category: params.get("category").map(str::to_string),
            tags: params.tags(),
            lang,
            sort,
            cursor: params.cursor(sort)?,
            limit: params.limit()?,
        })
    }

    fn push_base(&self, qb: &mut QueryBuilder<'_, Postgres>) {
        qb.push(" WHERE ");
        match self.area {
            TemplateArea::Platform => {
                qb.push("t.scope = 'platform' AND t.status = 'ready'");
            }
            TemplateArea::Admin => {
                qb.push("t.scope = 'platform'");
            }
            TemplateArea::Mine(teacher_id) => {
                qb.push("t.scope = 'personal' AND t.teacher_id = ");
                qb.push_bind(teacher_id);
            }
        }
        if !self.q.is_empty() {
            let q = self.q.to_lowercase();
            qb.push(" AND (strpos(lower(t.title), ");
            qb.push_bind(q.clone());
            qb.push(") > 0 OR strpos(lower(t.category), ");
            qb.push_bind(q.clone());
            qb.push(") > 0 OR strpos(lower(COALESCE(v.content ->> 'worksheet_kind', '')), ");
            qb.push_bind(q.clone());
            qb.push(") > 0 OR strpos(lower(COALESCE(m.title, '')), ");
            qb.push_bind(q.clone());
            qb.push(") > 0 OR EXISTS (SELECT 1 FROM unnest(t.tags) AS tg WHERE strpos(lower(tg), ");
            qb.push_bind(q);
            qb.push(") > 0))");
        }
    }

    fn push_filters(&self, qb: &mut QueryBuilder<'_, Postgres>) {
        if let Some(category) = &self.category {
            qb.push(" AND t.category = ");
            qb.push_bind(category.clone());
        }
        if !self.tags.is_empty() {
            qb.push(" AND t.tags @> ");
            qb.push_bind(self.tags.clone());
        }
        if let Some(lang) = &self.lang {
            qb.push(" AND v.content ->> 'instruction_language' = ");
            qb.push_bind(lang.clone());
        }
    }
}

/// One page of template cards with the facets and the total.
pub async fn templates(
    db: &PgPool,
    query: &TemplateQuery,
) -> AppResult<Page<TemplateCard, TemplateFacets>> {
    let admin = query.area == TemplateArea::Admin;
    let mut qb = QueryBuilder::new(format!("{TEMPLATE_COLUMNS}{TEMPLATE_FROM}"));
    query.push_base(&mut qb);
    query.push_filters(&mut qb);
    match &query.cursor {
        Some(Cursor::New(at, id)) => {
            qb.push(" AND (t.created_at, t.id) < (");
            qb.push_bind(*at);
            qb.push(", ");
            qb.push_bind(*id);
            qb.push(")");
        }
        Some(Cursor::Title(title, id)) => {
            qb.push(" AND (t.title, t.id) > (");
            qb.push_bind(title.clone());
            qb.push(", ");
            qb.push_bind(*id);
            qb.push(")");
        }
        None => {}
    }
    qb.push(match query.sort {
        Sort::New => " ORDER BY t.created_at DESC, t.id DESC",
        Sort::Title => " ORDER BY t.title ASC, t.id ASC",
    });
    qb.push(" LIMIT ");
    qb.push_bind(query.limit + 1);
    let mut rows: Vec<TemplateCardRow> = qb.build_query_as().fetch_all(db).await?;
    let next_cursor = if rows.len() as i64 > query.limit {
        rows.truncate(query.limit as usize);
        rows.last().map(|r| r.cursor(query.sort).encode())
    } else {
        None
    };

    let mut total = QueryBuilder::new(format!("SELECT COUNT(*){TEMPLATE_FROM}"));
    query.push_base(&mut total);
    query.push_filters(&mut total);
    let total: i64 = total.build_query_scalar().fetch_one(db).await?;

    let mut categories = QueryBuilder::new(format!(
        "SELECT t.category AS key, COUNT(*) AS count{TEMPLATE_FROM}"
    ));
    query.push_base(&mut categories);
    let categories = facet_counts(db, categories, " GROUP BY t.category").await?;
    let mut tags = QueryBuilder::new(format!(
        "SELECT tg AS key, COUNT(*) AS count{TEMPLATE_FROM} CROSS JOIN LATERAL unnest(t.tags) AS tg"
    ));
    query.push_base(&mut tags);
    let tags = facet_counts(
        db,
        tags,
        &format!(" GROUP BY tg ORDER BY COUNT(*) DESC, tg LIMIT {MAX_TAG_FACETS}"),
    )
    .await?;
    let mut langs = QueryBuilder::new(format!(
        "SELECT v.content ->> 'instruction_language' AS key, COUNT(*) AS count{TEMPLATE_FROM}"
    ));
    query.push_base(&mut langs);
    let langs = facet_counts(db, langs, " GROUP BY 1").await?;

    Ok(Page {
        items: rows.into_iter().map(|r| r.card(admin)).collect(),
        next_cursor,
        facets: TemplateFacets {
            categories: order_categories(&SAMPLE_CATEGORIES, categories),
            tags,
            langs: order_fixed(&LANGS, langs),
        },
        total,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params(pairs: &[(&str, &str)]) -> Params {
        Params(
            pairs
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        )
    }

    #[test]
    fn cursors_round_trip_and_belong_to_their_sort() {
        let at = DateTime::from_timestamp_micros(1_763_280_000_123_456).unwrap();
        let c = Cursor::New(at, 42);
        assert_eq!(Cursor::parse(&c.encode(), Sort::New), Some(c.clone()));
        assert_eq!(Cursor::parse(&c.encode(), Sort::Title), None);
        let t = Cursor::Title("學測 114. 英文".into(), 7);
        assert_eq!(Cursor::parse(&t.encode(), Sort::Title), Some(t));
        for junk in ["", "n", "n.1", "n.x.1", "t.zz.1", "x.1.1", "n.1.y"] {
            assert_eq!(Cursor::parse(junk, Sort::New), None, "{junk}");
        }
    }

    #[test]
    fn material_queries_are_parsed_and_checked() {
        let q = MaterialQuery::teacher(
            &params(&[
                ("area", "mine"),
                ("q", "  trust "),
                ("tag", "114"),
                ("tag", "試題"),
                ("tag", "114"),
                ("tag", " "),
                ("file_type", "pdf"),
                ("fits_lesson", "true"),
                ("limit", "500"),
            ]),
            9,
        )
        .unwrap();
        assert_eq!(q.area, MaterialArea::Mine(9));
        assert_eq!(q.q, "trust");
        assert_eq!(q.tags, ["114", "試題"]);
        assert_eq!(q.limit, MAX_LIMIT);
        assert!(q.fits_lesson);
        let d = MaterialQuery::teacher(&params(&[]), 9).unwrap();
        assert_eq!(
            (d.area, d.limit, d.sort, d.fits_lesson),
            (MaterialArea::Reference, DEFAULT_LIMIT, Sort::New, false)
        );
        for bad in [
            ("area", "all"),
            ("file_type", "doc"),
            ("fits_lesson", "maybe"),
            ("limit", "x"),
            ("sort", "old"),
            ("cursor", "t.00.1"),
        ] {
            assert!(
                MaterialQuery::teacher(&params(&[bad]), 9).is_err(),
                "{bad:?}"
            );
        }
        assert!(TemplateQuery::teacher(&params(&[("lang", "日文")]), 1).is_err());
        assert_eq!(
            TemplateQuery::teacher(&params(&[("lang", "中英並用")]), 1)
                .unwrap()
                .lang
                .as_deref(),
            Some("中英並用")
        );
    }

    #[test]
    fn facets_keep_the_list_order() {
        let f = |k: &str, n: i64| FacetCount {
            key: k.into(),
            count: n,
        };
        let ordered = order_categories(
            &REFERENCE_CATEGORIES,
            vec![f("其他", 1), f("", 4), f("自訂", 2), f("學測英文", 3)],
        );
        let keys: Vec<&str> = ordered.iter().map(|c| c.key.as_str()).collect();
        assert_eq!(keys, ["學測英文", "其他", "自訂"]);
        let fixed = order_fixed(&FILE_TYPES, vec![f("text", 2), f("pdf", 1), f("link", 0)]);
        let keys: Vec<&str> = fixed.iter().map(|c| c.key.as_str()).collect();
        assert_eq!(keys, ["pdf", "text"]);
    }
}
