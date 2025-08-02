use argon2::{
    password_hash::{rand_core::OsRng, SaltString},
    Argon2, PasswordHasher,
};
use colored::*;
use dialoguer::{Input, Password, Select};
use std::{io, path::PathBuf};

use crate::{constants::constants::DEFAULT_PORT, security::config::DBConfig};

pub struct CLI {
    pub config: DBConfig,
    data_dir: PathBuf,
}

impl CLI {
    pub fn new(data_dir: PathBuf) -> Self {
        Self {
            config: DBConfig::default(),
            data_dir,
        }
    }

    pub async fn run_setup(&mut self) -> io::Result<()> {
        println!("{}", "> Configure your database...".bold().cyan());
        // Authentication Setup
        self.setup_authentication().await?;

        // Server Configuration
        self.configure_server_settings().await?;

        // Memory Database Settings
        self.configure_memory_settings().await?;

        // Logging and Performance
        self.configure_advanced_settings().await?;

        // Save the configuration
        self.config.save(&self.data_dir).await?;

        println!("\n{}", "> Configuration Complete! ".bold().green());
        println!(
            "Connection String: {}",
            format!(
                "tinycache://{}:****@database:{}",
                self.config.admin, self.config.database,
            )
            .yellow()
        );
        Ok(())
    }

    async fn setup_authentication(&mut self) -> io::Result<()> {
        println!("\n{}", "🔐 Authentication Setup".bold().blue());

        let username: String = Input::new()
            .with_prompt("Enter admin username")
            .validate_with(|input: &String| {
                if input.len() < 3 {
                    Err("Username must be at least 3 characters long")
                } else {
                    Ok(())
                }
            })
            .interact_text()
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

        // Admin Password
        let password: String = Password::new()
            .with_prompt("Enter admin password")
            .with_confirmation("Confirm password", "Passwords don't match")
            .validate_with(|input: &String| {
                if input.len() < 8 {
                    Err("Password must be at least 8 characters long")
                } else {
                    Ok(())
                }
            })
            .interact()
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

        let database: String = Input::new()
            .with_prompt("Enter database name")
            .validate_with(|input: &String| {
                if input.len() < 3 {
                    Err("database must be at least 3 characters long")
                } else {
                    Ok(())
                }
            })
            .interact_text()
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

        // let database_options = vec!["kv"];
        // let database_type_choice = Select::new()
        //     .with_prompt("Select database type")
        //     .items(&database_options)
        //     .default(0)
        //     .interact()
        //     .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

        // Generate salt and hash password
        let salt = SaltString::generate(&mut OsRng);
        let argon2 = Argon2::default();

        let password_hash = argon2
            .hash_password(password.as_bytes(), &salt)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?
            .to_string();

        // Update config
        self.config.admin = username;
        self.config.password = password_hash;
        self.config.database = database;
        // self.config.database_type = database_options[database_type_choice].to_string();
        self.config.salt = salt.to_string();

        let session_ttl: u64 = Input::new()
            .with_prompt("Session timeout (seconds)")
            .default(3600)
            .interact_text()
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

        self.config.session_ttl = session_ttl;

        Ok(())
    }

    async fn configure_server_settings(&mut self) -> io::Result<()> {
        println!("\n{}", "Server Configuration".bold().blue());

        // host selection
        let hosts = vec![
            "0.0.0.0 (All interfaces".to_string(),
            "127.0.0.1 (Localhost)".to_string(),
        ];

        let host_index = Select::new()
            .with_prompt("Select host interface")
            .items(&hosts)
            .default(0)
            .interact()
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

        self.config.host = match host_index {
            0 => std::net::IpAddr::V4(std::net::Ipv4Addr::new(0, 0, 0, 0)),
            1 => std::net::IpAddr::V4(std::net::Ipv4Addr::new(127, 0, 0, 1)),
            _ => unreachable!(),
        };

        // Port selection
        let port: String = Input::new()
            .with_prompt("Server port")
            .validate_with(|input: &String| {
                if input.len() < 4 {
                    Err("PORT variable should be atleast 4 charecters long")
                } else {
                    Ok(())
                }
            })
            .default(DEFAULT_PORT.to_string())
            .interact_text()
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        self.config.port = port;

        Ok(())
    }

    async fn configure_memory_settings(&mut self) -> io::Result<()> {
        println!("\n{}", "💾 Memory Database Settings".bold().blue());

        let max_entries: usize = Input::new()
            .with_prompt("Maximum entries per database")
            .default(1200)
            .interact_text()
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        self.config.max_entries = max_entries;

        let default_ttl: u64 = Input::new()
            .with_prompt("Default TTL for entries (seconds, 0 for none)")
            .default(604800)
            .interact_text()
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        self.config.default_ttl_secs = default_ttl;

        let eviction_options = vec!["LFRU", "LRU", "LFU"];
        let eviction_choice = Select::new()
            .with_prompt("Select eviction policy")
            .items(&eviction_options)
            .default(0)
            .interact()
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        self.config.eviction_policy = eviction_options[eviction_choice].to_string();

        Ok(())
    }

    async fn configure_advanced_settings(&mut self) -> io::Result<()> {
        println!("\n{}", "⚙️ Advanced Settings".bold().blue());

        // Worker threads
        let worker_threads: usize = Input::new()
            .with_prompt("Number of worker threads")
            .default(num_cpus::get())
            .interact_text()
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

        self.config.worker_threads = worker_threads;

        Ok(())
    }
}
