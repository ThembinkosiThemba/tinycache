use crate::{db::db::TinyCache, security::auth::Session, utils::logs::LogLevel};
use std::sync::Arc;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};

use super::requests::process_requests;

/// *handle_client* handles a single client connection and continuously reads requests from the client
///
/// Each requests is processed using the process_request function and
/// Sends responses back to the client
/// Closes the connection when the client disconnects or an error occurs
pub async fn handle_client(mut socket: TcpStream, db: Arc<TinyCache>) {
    // handle client function will operate different based on the deployment mode.
    // if deployment mode is standalone, then we will have to check if the connected client has
    // the neccessary credentials/access and authenticate them
    let mut connections = db.active_connections.write().await;

    if *connections >= db.config.max_connections {
        let _ = socket.write_all(b"ERROR: Max connections reached\n").await;
        let _ = db
            .logger
            .log_error(
                "Max connections reached, rejecting new client",
                LogLevel::System,
                &db,
            )
            .await;
        return;
    }
    *connections += 1;
    drop(connections);

    // now validting worker threads availability
    if let Err(e) = db.config.validate() {
        let _ = socket.write_all(format!("ERROR: {}\n", e).as_bytes()).await;
        let _ = db.logger.log_error(
            &format!("Worker thread validation failed: {}", e),
            LogLevel::System,
            &db,
        );
        return;
    }

    let mut buffer = [0; 1024];
    let peer_addr = socket
        .peer_addr()
        .unwrap_or_else(|_| "Unknown".parse().unwrap());

    match socket.read(&mut buffer).await {
        Ok(n) => {
            let connection_str = String::from_utf8_lossy(&buffer[..n]).trim().to_string();

            // Attempting authentication
            match db
                .auth_manager
                .authenticate(&connection_str, db.clone())
                .await
            {
                Ok(session) => {
                    // Send success response with session ID
                    let response = format!("AUTH OK {}\n", session.id);

                    let _ = db
                        .logger
                        .log_info(
                            &format!("Auth Response  {}", response),
                            LogLevel::System,
                            &db,
                        )
                        .await;

                    if let Err(e) = socket.write_all(response.as_bytes()).await {
                        let _ = db
                            .logger
                            .log_error(
                                &format!("Failed to write auth response: {}", e),
                                LogLevel::System,
                                &db,
                            )
                            .await;
                        return;
                    }

                    handle_authenticated_requests(socket, db.clone(), session).await;
                }
                Err(e) => {
                    let response = format!("AUTH ERROR {}\n", e);
                    let _ = socket.write_all(response.as_bytes()).await;
                    let _ = db
                        .logger
                        .log_warn(
                            &format!("Authentication failed for {}: {}", peer_addr, e),
                            LogLevel::Application,
                            &db,
                        )
                        .await;
                    return;
                }
            }
        }
        Err(e) => {
            let _ = db
                .logger
                .log_error(
                    &format!("Failed to read from socket: {}", e),
                    LogLevel::System,
                    &db,
                )
                .await;
            return;
        }
    }

    let mut connections = db.active_connections.write().await;
    *connections -= 1;
}

async fn handle_authenticated_requests(mut socket: TcpStream, db: Arc<TinyCache>, session: Session) {
    let mut buffer = [0; 1024];
    loop {
        tokio::select! {
            result = socket.read(&mut buffer) => {
                match result {
                    Ok(0) => break,
                    Ok(n) => {
                        let request = String::from_utf8_lossy(&buffer[..n]).trim().to_string();

                        // for authenticated services, have to validate their sessions to make sure everything is good
                        if db.auth_manager.validate_session(&session.id, db.clone()).await.is_none() {
                           let _ = socket.write_all(b"Session expired. Please reconnect.\n").await;
                            break;
                        }

                        // once we are validate, send the request, session to the process_request function which handles all requests

                        let response = process_requests(request, &db).await;

                        if let Err(e) = socket.write_all(response.as_bytes()).await {
                            let _ = db.logger.log_error(&format!("Failed to write response: {}", e), LogLevel::System, &db).await;
                            break;
                        }
                    }
                    Err(e) => {
                        let _ = db.logger.log_error(&format!("Failed to read from socket: {}", e), LogLevel::System, &db).await;
                        break;
                    }
                }
            }
        }
    }
}
