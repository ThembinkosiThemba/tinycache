use argon2::{Argon2, PasswordHash, PasswordVerifier};
use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
    time::{Duration, SystemTime},
};
use uuid::Uuid;

use super::config::DBConfig;
use crate::db::db::TinyCache;
use crate::utils::logs::LogLevel;
use crate::utils::utils::parse_connection_string;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum DeploymentMode {
    Standalone,
    Cloud,
}

use colored::*;

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct Session {
    pub id: String,
    pub username: String,
    pub database: String,
    pub created_at: SystemTime,
    pub expires_at: SystemTime,
}

pub struct AuthManager {
    config: Arc<DBConfig>,
    sessions: Arc<RwLock<HashMap<String, Session>>>,
}

impl AuthManager {
    pub fn new(config: DBConfig) -> Self {
        println!("{}", ">>> auth manager initialised".dimmed());
        Self {
            config: Arc::new(config),
            sessions: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn authenticate(
        &self,
        connection_string: &str,
        db: Arc<TinyCache>,
    ) -> Result<Session, String> {
        let (username, password, database, db_type) = parse_connection_string(connection_string)?;

        if username != self.config.admin {
            return Err("Invalid username".to_string());
        }

        if database != self.config.database {
            return Err("Invalid database name".to_string());
        }

        if db_type != self.config.database_type {
            return Err("Invalid database type".to_string());
        }

        let parsed_hash = PasswordHash::new(&self.config.password)
            .map_err(|_| "Invalid password hash format".to_string())?;

        let argon2 = Argon2::default();
        let is_valid = argon2
            .verify_password(password.as_bytes(), &parsed_hash)
            .is_ok();

        if !is_valid {
            return Err("Invalid password".to_string());
        }

        let _ = db
            .logger
            .log_info("user credentials are valid", LogLevel::System, &db)
            .await;

        let session = self.create_session(username, database, db).await?;

        self.cleanup_expired_sessions().await;

        Ok(session)
    }

    async fn create_session(
        &self,
        username: String,
        database: String,
        db: Arc<TinyCache>,
    ) -> Result<Session, String> {
        let session_id = Uuid::new_v4().to_string();
        let now = SystemTime::now();

        let session = Session {
            id: session_id.clone(),
            username,
            database,
            created_at: now,
            expires_at: now + Duration::from_secs(self.config.session_ttl),
        };

        {
            let mut sessions = self
                .sessions
                .write()
                .map_err(|_| "Failed to acquire write lock".to_string())?;
            sessions.insert(session_id.clone(), session.clone());
        }

        let _ = db
            .logger
            .log_info(
                &format!("session with id {} created successfully", session_id),
                LogLevel::System,
                &db,
            )
            .await;

        Ok(session)
    }

    async fn cleanup_expired_sessions(&self) {
        let sessions = self
            .sessions
            .write()
            .map_err(|e| format!("Failed to acquire write lock: {}", e));
        let now = SystemTime::now();
        sessions
            .unwrap()
            .retain(|_, session| session.expires_at > now);
    }

    pub async fn validate_session(&self, session_id: &str, db: Arc<TinyCache>) -> Option<Session> {
        let session = {
            let sessions = self.sessions.read().ok()?;
            if let Some(session) = sessions.get(session_id) {
                if session.expires_at > SystemTime::now() {
                    Some(session.clone())
                } else {
                    None
                }
            } else {
                None
            }
        };

        if let Some(_session) = session {
            let _ = db
                .logger
                .log_info(
                    &format!("session with id {} validated successfully", session_id),
                    LogLevel::System,
                    &db,
                )
                .await;
            self.refresh_session(session_id).await
        } else {
            None
        }
    }

    pub async fn refresh_session(&self, session_id: &str) -> Option<Session> {
        let mut sessions = self.sessions.write().ok()?;

        if let Some(session) = sessions.get_mut(session_id) {
            if session.expires_at > SystemTime::now() {
                session.expires_at =
                    SystemTime::now() + Duration::from_secs(self.config.session_ttl);
                return Some(session.clone());
            }
        }
        None
    }
}
