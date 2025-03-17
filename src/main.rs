mod app;
mod bin;
mod block;
mod input;
mod mode;
mod room;
mod scrollbar;
mod task_ext;
mod textarea;
mod timeline;

use std::io::{self, Write};

use matrix_sdk::{
    AuthSession, Client, ClientBuildError, SqliteCryptoStore, SqliteEventCacheStore,
    SqliteStateStore,
    authentication::matrix::MatrixSession,
    encryption::{BackupDownloadStrategy, EncryptionSettings},
    ruma::exports::serde_json,
    store::StoreConfig,
};
use matrix_sdk_sqlite::OpenStoreError;
use textarea::TextArea;

#[derive(Debug, thiserror::Error)]
enum Error {
    #[error(transparent)]
    Io(#[from] io::Error),

    #[error(transparent)]
    ClientError(#[from] ClientBuildError),

    #[error(transparent)]
    OpenStore(#[from] OpenStoreError),

    #[error(transparent)]
    Session(#[from] serde_json::Error),

    #[error(transparent)]
    Matrix(#[from] matrix_sdk::Error),

    #[error(transparent)]
    MatrixSyncService(#[from] matrix_sdk_ui::sync_service::Error),
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    logger();

    let options = argh::from_env();
    let client = client(&options).await?;
    let client = session(client, &options).await?;

    let event_cache = client.event_cache();
    event_cache.subscribe().unwrap();
    event_cache.enable_storage().unwrap();

    app(client).await?;

    Ok(())
}

fn logger() {
    use tracing_subscriber::prelude::*;

    tracing_subscriber::registry().with(tui_logger::TuiTracingSubscriberLayer).init();
    tui_logger::init_logger(tui_logger::LevelFilter::Trace).unwrap();
}

async fn client(options: &bin::Options) -> Result<Client, Error> {
    let bin::Options { server_name, session_path } = options;

    let client_builder = Client::builder()
        .store_config(
            StoreConfig::new("multiverse".to_owned())
                .crypto_store(SqliteCryptoStore::open(session_path.join("crypto"), None).await?)
                .state_store(SqliteStateStore::open(session_path.join("state"), None).await?)
                .event_cache_store(
                    SqliteEventCacheStore::open(session_path.join("cache"), None).await?,
                ),
        )
        .server_name_or_homeserver_url(&server_name)
        .with_encryption_settings(EncryptionSettings {
            auto_enable_cross_signing: true,
            backup_download_strategy: BackupDownloadStrategy::AfterDecryptionFailure,
            auto_enable_backups: true,
        });

    Ok(client_builder.build().await?)
}

async fn session(client: Client, options: &bin::Options) -> Result<Client, Error> {
    let session_path = options.session_path.join("session.json");

    if let Ok(serialized) = std::fs::read_to_string(&session_path) {
        let session: MatrixSession = serde_json::from_str(&serialized)?;
        client.restore_session(session).await?;
    } else {
        println!("Logging in with username and password…");

        loop {
            print!("\nUsername: ");
            io::stdout().flush().expect("Unable to write to stdout");
            let mut username = String::new();
            io::stdin().read_line(&mut username).expect("Unable to read user input");
            username = username.trim().to_owned();

            let password = rpassword::prompt_password("Password: ")?;

            match client.matrix_auth().login_username(&username, password.trim()).await {
                Ok(_) => {
                    println!("Logged in as {username}");
                    break;
                }
                Err(error) => {
                    println!("Error logging in: {error}");
                    println!("Please try again\n");
                }
            }
        }

        // Immediately save the session to disk.
        if let Some(session) = client.session() {
            let AuthSession::Matrix(session) = session else { panic!("unexpected oidc session") };
            let serialized = serde_json::to_string(&session)?;
            std::fs::write(session_path, serialized)?;

            println!("Session saved");
        }
    }

    Ok(client)
}

async fn app(client: Client) -> Result<(), Error> {
    let mut terminal = ratatui::init();
    let app_result = app::App::new(client).await?.run(&mut terminal).await;

    ratatui::restore();

    app_result
}
