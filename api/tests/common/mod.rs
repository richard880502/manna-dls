//! Integration-test harness: one throwaway database per test (`CREATE DATABASE test_<random>`),
//! migrated with the embedded migrations and dropped (WITH FORCE) when the test ends, so tests
//! can run in parallel against one Postgres server.
//!
//! Requires `DATABASE_URL` (a role allowed to CREATE DATABASE on a server with pgvector).
//! Without it tests are skipped locally, but fail when `CI` is set.

#![allow(dead_code)]

use std::str::FromStr;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use axum::Router;
use http_body_util::BodyExt;
use manna_api::config::Config;
use manna_api::{auth, build_app, db, AppState};
use rand::RngCore;
use serde_json::{json, Value};
use sqlx::postgres::PgConnectOptions;
use sqlx::{ConnectOptions, Connection, PgPool};
use tower::ServiceExt;

pub const BOOTSTRAP_CODE: &str = "boot-strap-test-code";

pub struct TestApp {
    pub app: Router,
    pub db: PgPool,
    db_name: String,
    admin_opts: PgConnectOptions,
}

/// Creates a TestApp or returns from the test when no database is configured.
#[macro_export]
macro_rules! test_app {
    () => {
        $crate::test_app!(|_c: &mut manna_api::config::Config| {})
    };
    ($customize:expr) => {
        match common::TestApp::new($customize).await {
            Some(app) => app,
            None => return,
        }
    };
}

/// A response as it left the server: for downloads and anything that is not JSON.
pub struct RawResponse {
    pub status: StatusCode,
    pub headers: axum::http::HeaderMap,
    pub body: Vec<u8>,
}

impl RawResponse {
    pub fn header(&self, name: &str) -> &str {
        self.headers
            .get(name)
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
    }
}

/// One part of a `multipart/form-data` body.
pub enum Part<'a> {
    Text(&'a str, &'a str),
    /// field name, file name, content
    File(&'a str, &'a str, &'a [u8]),
}

pub const MULTIPART_BOUNDARY: &str = "----manna-test-boundary-7MA4YWxkTrZu0gW";

pub fn multipart_body(parts: &[Part<'_>]) -> Vec<u8> {
    let mut body = Vec::new();
    for part in parts {
        body.extend_from_slice(format!("--{MULTIPART_BOUNDARY}\r\n").as_bytes());
        match part {
            Part::Text(name, value) => {
                body.extend_from_slice(
                    format!("Content-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n")
                        .as_bytes(),
                );
            }
            Part::File(name, filename, data) => {
                body.extend_from_slice(
                    format!(
                        "Content-Disposition: form-data; name=\"{name}\"; filename=\"{filename}\"\r\n\
                         Content-Type: application/octet-stream\r\n\r\n"
                    )
                    .as_bytes(),
                );
                body.extend_from_slice(data);
                body.extend_from_slice(b"\r\n");
            }
        }
    }
    body.extend_from_slice(format!("--{MULTIPART_BOUNDARY}--\r\n").as_bytes());
    body
}

impl TestApp {
    pub async fn new(customize: impl FnOnce(&mut Config)) -> Option<Self> {
        Self::new_seeded(customize, None, |_| async {}).await
    }

    /// Like [`TestApp::new`], but the database is first migrated only up to (and including)
    /// migration `stop_after`, `seed` runs against that older schema, and then the remaining
    /// migrations run: for testing a data migration exactly as a deployment performs it.
    pub async fn new_seeded<F, Fut>(
        customize: impl FnOnce(&mut Config),
        stop_after: Option<i64>,
        seed: F,
    ) -> Option<Self>
    where
        F: FnOnce(PgPool) -> Fut,
        Fut: std::future::Future<Output = ()>,
    {
        let url = match std::env::var("DATABASE_URL") {
            Ok(u) if !u.trim().is_empty() => u,
            _ => {
                if std::env::var_os("CI").is_some() {
                    panic!("DATABASE_URL must be set for integration tests in CI");
                }
                eprintln!("skipping integration test: DATABASE_URL is not set");
                return None;
            }
        };
        let admin_opts = PgConnectOptions::from_str(&url)
            .expect("valid DATABASE_URL")
            .disable_statement_logging();
        let mut buf = [0u8; 8];
        rand::rngs::OsRng.fill_bytes(&mut buf);
        let db_name = format!("test_{}", hex::encode(buf));
        let mut admin = sqlx::PgConnection::connect_with(&admin_opts)
            .await
            .expect("connect to DATABASE_URL");
        sqlx::query(&format!("CREATE DATABASE \"{db_name}\""))
            .execute(&mut admin)
            .await
            .expect("CREATE DATABASE");
        admin.close().await.ok();

        let pool = db::connect_with(admin_opts.clone().database(&db_name), 5)
            .await
            .expect("connect to test database");
        if let Some(stop_after) = stop_after {
            let older = sqlx::migrate::Migrator {
                migrations: std::borrow::Cow::Owned(
                    db::MIGRATOR
                        .migrations
                        .iter()
                        .filter(|m| m.version <= stop_after)
                        .cloned()
                        .collect(),
                ),
                ..sqlx::migrate::Migrator::DEFAULT
            };
            older.run(&pool).await.expect("older migrations");
            seed(pool.clone()).await;
        }
        db::migrate(&pool).await.expect("migrations");

        let mut config = Config::for_tests(url);
        config.admin_bootstrap_code = Some(BOOTSTRAP_CODE.to_string());
        customize(&mut config);
        auth::bootstrap_admin(&pool, config.admin_bootstrap_code.as_deref())
            .await
            .expect("bootstrap");
        let app = build_app(AppState::new(pool.clone(), config));
        Some(Self {
            app,
            db: pool,
            db_name,
            admin_opts,
        })
    }

    pub async fn send(
        &self,
        method: Method,
        path: &str,
        token: Option<&str>,
        content_type: Option<&str>,
        body: Vec<u8>,
    ) -> (StatusCode, Value) {
        let mut req = Request::builder().method(method).uri(path);
        if let Some(t) = token {
            req = req.header("authorization", format!("Bearer {t}"));
        }
        if let Some(ct) = content_type {
            req = req.header("content-type", ct);
        }
        let resp = self
            .app
            .clone()
            .oneshot(req.body(Body::from(body)).unwrap())
            .await
            .unwrap();
        let status = resp.status();
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        let value = if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes)
                .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&bytes).into_owned()))
        };
        (status, value)
    }

    /// Sends a request and returns the response untouched (headers and raw body).
    pub async fn send_raw(
        &self,
        method: Method,
        path: &str,
        headers: &[(&str, &str)],
        body: Vec<u8>,
    ) -> RawResponse {
        let mut req = Request::builder().method(method).uri(path);
        for (name, value) in headers {
            req = req.header(*name, *value);
        }
        let resp = self
            .app
            .clone()
            .oneshot(req.body(Body::from(body)).unwrap())
            .await
            .unwrap();
        RawResponse {
            status: resp.status(),
            headers: resp.headers().clone(),
            body: resp
                .into_body()
                .collect()
                .await
                .unwrap()
                .to_bytes()
                .to_vec(),
        }
    }

    /// `POST` of a `multipart/form-data` body.
    pub async fn post_multipart(
        &self,
        path: &str,
        token: Option<&str>,
        parts: &[Part<'_>],
    ) -> (StatusCode, Value) {
        self.send(
            Method::POST,
            path,
            token,
            Some(&format!(
                "multipart/form-data; boundary={MULTIPART_BOUNDARY}"
            )),
            multipart_body(parts),
        )
        .await
    }

    pub async fn delete(&self, path: &str, token: Option<&str>) -> (StatusCode, Value) {
        self.send(Method::DELETE, path, token, None, Vec::new())
            .await
    }

    pub async fn get(&self, path: &str, token: Option<&str>) -> (StatusCode, Value) {
        self.send(Method::GET, path, token, None, Vec::new()).await
    }

    pub async fn post(&self, path: &str, token: Option<&str>, body: Value) -> (StatusCode, Value) {
        self.send(
            Method::POST,
            path,
            token,
            Some("application/json"),
            body.to_string().into_bytes(),
        )
        .await
    }

    pub async fn put(&self, path: &str, token: Option<&str>, body: Value) -> (StatusCode, Value) {
        self.send(
            Method::PUT,
            path,
            token,
            Some("application/json"),
            body.to_string().into_bytes(),
        )
        .await
    }

    pub async fn patch(&self, path: &str, token: Option<&str>, body: Value) -> (StatusCode, Value) {
        self.send(
            Method::PATCH,
            path,
            token,
            Some("application/json"),
            body.to_string().into_bytes(),
        )
        .await
    }

    pub async fn post_csv(
        &self,
        path: &str,
        token: Option<&str>,
        csv: &str,
    ) -> (StatusCode, Value) {
        self.send(
            Method::POST,
            path,
            token,
            Some("text/csv"),
            csv.as_bytes().to_vec(),
        )
        .await
    }

    pub async fn login(&self, code: &str) -> (String, Value) {
        let (s, v) = self
            .post("/api/auth/login", None, json!({ "code": code }))
            .await;
        assert_eq!(s, StatusCode::OK, "login failed: {v}");
        (
            v["token"].as_str().unwrap().to_string(),
            v["teacher"].clone(),
        )
    }

    pub async fn pi_token(&self) -> String {
        self.login(BOOTSTRAP_CODE).await.0
    }

    /// Creates an invite through the PI and logs the new teacher in.
    pub async fn teacher(&self, research_id: &str, track: &str) -> String {
        let pi = self.pi_token().await;
        let (s, v) = self
            .post(
                "/api/admin/invites",
                Some(&pi),
                json!({ "research_id": research_id, "school_track": track }),
            )
            .await;
        assert_eq!(s, StatusCode::OK, "{v}");
        self.login(v["code"].as_str().unwrap()).await.0
    }

    pub async fn create_unit(&self, token: &str, title: &str) -> i64 {
        let (s, v) = self
            .post("/api/units", Some(token), json!({ "title": title }))
            .await;
        assert_eq!(s, StatusCode::OK, "{v}");
        v["id"].as_i64().unwrap()
    }

    pub async fn create_lesson(&self, token: &str, unit_id: i64, title: &str) -> i64 {
        let (s, v) = self
            .post(
                &format!("/api/units/{unit_id}/lessons"),
                Some(token),
                json!({ "title": title }),
            )
            .await;
        assert_eq!(s, StatusCode::OK, "{v}");
        v["id"].as_i64().unwrap()
    }
}

impl Drop for TestApp {
    fn drop(&mut self) {
        let name = self.db_name.clone();
        let opts = self.admin_opts.clone();
        // Drop the database from a separate runtime: Drop cannot await.
        let _ = std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            rt.block_on(async move {
                if let Ok(mut conn) = sqlx::PgConnection::connect_with(&opts).await {
                    let _ =
                        sqlx::query(&format!("DROP DATABASE IF EXISTS \"{name}\" WITH (FORCE)"))
                            .execute(&mut conn)
                            .await;
                    let _ = conn.close().await;
                }
            });
        })
        .join();
    }
}
