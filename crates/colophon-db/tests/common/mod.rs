//! A fresh database per test, migrated as the owner and used as `colophon_app`, the
//! application's login, so row-level security is in force exactly as in production.
#![allow(
    dead_code,
    reason = "each test file uses a different subset of these helpers"
)]

use colophon_db::{PgPool, identity};
use sqlx::{AssertSqlSafe, postgres::PgPoolOptions};
use uuid::Uuid;

fn host() -> String {
    std::env::var("COLOPHON_TEST_PG_HOST").unwrap_or_else(|_| "127.0.0.1:55432".to_owned())
}

pub(crate) struct TestDb {
    admin: PgPool,
    name: String,
    /// The application login's pool: one connection, reused as pooled requests reuse them.
    pub(crate) app: PgPool,
}

impl TestDb {
    pub(crate) async fn new() -> Self {
        let host = host();
        let admin = PgPool::connect(&format!("postgres://colophon:colophon-dev@{host}/postgres"))
            .await
            .expect("the stack's database answers (scripts/stack.sh up)");
        let name = format!("colophon_test_{}", Uuid::now_v7().simple());
        // The name is generated above from a UUID, never from input.
        sqlx::query(AssertSqlSafe(format!("CREATE DATABASE {name}")))
            .execute(&admin)
            .await
            .expect("create database");
        let owner = PgPool::connect(&format!("postgres://colophon:colophon-dev@{host}/{name}"))
            .await
            .expect("connect as owner");
        colophon_db::migrate(&owner)
            .await
            .expect("migrations apply to an empty database");
        owner.close().await;
        // One connection: every transaction below reuses it, as pooled requests do.
        let app = PgPoolOptions::new()
            .max_connections(1)
            .connect(&format!(
                "postgres://colophon_app:colophon-dev@{host}/{name}"
            ))
            .await
            .expect("connect as the application login");
        Self { admin, name, app }
    }

    /// A pool for the table owner, which row-level security doesn't apply to.
    pub(crate) async fn owner(&self) -> PgPool {
        PgPool::connect(&format!(
            "postgres://colophon:colophon-dev@{}/{}",
            host(),
            self.name
        ))
        .await
        .expect("connect as owner")
    }

    /// Another pool for the application login, for tests that need concurrent transactions.
    pub(crate) async fn app_pool(&self, connections: u32) -> PgPool {
        PgPoolOptions::new()
            .max_connections(connections)
            .connect(&format!(
                "postgres://colophon_app:colophon-dev@{}/{}",
                host(),
                self.name
            ))
            .await
            .expect("connect as the application login")
    }

    pub(crate) async fn drop(self) {
        self.app.close().await;
        sqlx::query(AssertSqlSafe(format!(
            "DROP DATABASE {} WITH (FORCE)",
            self.name
        )))
        .execute(&self.admin)
        .await
        .expect("drop database");
    }

    /// Records a sign-in for `subject` and returns the identity.
    pub(crate) async fn person(&self, subject: &str) -> Uuid {
        identity::record_sign_in(&self.app, "http://issuer.test", subject, Some(subject))
            .await
            .expect("sign-in recorded")
    }
}
