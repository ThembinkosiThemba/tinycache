mod cli;
mod constants;
mod db;
mod persistance;
mod query;
mod requests;
mod security;
mod utils;

use clap::{Arg, Command};
use cli::cli::CLI;
use colored::*;
use db::db::TinyCache;
use dotenv::dotenv;
use persistance::persistance::PersistenceConfig;
use requests::client::handle_client;
use security::config::DBConfig;
use std::sync::Arc;
use tokio::net::TcpListener;
use utils::{
    logs::LogLevel,
    utils::{display_startup_info, print_art, return_data_dir},
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenv().ok();
    print_art();

    // Returns the data directory for tinycache database
    let data_dir = return_data_dir()?;
    // let var_dir = return_var_dir()?;

    tokio::fs::create_dir_all(&data_dir).await?;
    // tokio::fs::create_dir_all(&var_dir).await?;

    let persist_config = PersistenceConfig::default(&data_dir);

    // Configuration for the database
    let config = DBConfig::load_or_create(&data_dir).await?;

    let _ = Command::new("")
        .version("0.1.0")
        .author("Thembinkosi Mkhonta")
        .about("TinyCache is the main database engine")
        .arg(
            Arg::new("command")
                .value_name("COMMAND")
                .help("Commands: setup, edit"),
        )
        .get_matches();

    let mut cli = CLI::new(data_dir.clone());
    cli.config = config.clone();

    if config.password.is_empty() || config.password.is_empty() {
        cli.run_setup().await?;
        println!("{}", ">>> Setup complete. Starting server...".dimmed());
    } else {
        println!("{}", ">>> Existing configuration found...".dimmed());
    }

    // Initialize TinyCache with the data directory and the database configurations
    let db = Arc::new(TinyCache::new(data_dir, config.clone(), persist_config).await?);

    display_startup_info(&db).await;

    // Starting the TCP server using the db, port and host from the configuration
    run_tcp_server(db, &config.host.to_string(), &config.port).await?;

    Ok(())
}

async fn run_tcp_server(db: Arc<TinyCache>, host: &str, port: &str) -> std::io::Result<()> {
    let address = format!("{}:{}", host, port);
    let listener = TcpListener::bind(&address).await?;

    db.logger
        .log_info(
            &format!("Database listening on {}", address),
            LogLevel::System,
            &db,
        )
        .await?;

    loop {
        let (socket, addr) = listener.accept().await?;
        let db = Arc::clone(&db);

        db.logger
            .log_info(
                &format!("New connection from: {}", addr),
                LogLevel::System,
                &db,
            )
            .await?;

        tokio::spawn(async move {
            handle_client(socket, db).await;
        });
    }
}
