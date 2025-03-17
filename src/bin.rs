use std::path::PathBuf;

use argh::FromArgs;

/// Small Matrix client tailored for debugging the Matrix Rust SDK.
#[derive(Debug, FromArgs)]
pub struct Options {
    /// the homeserver the client should connect to.
    #[argh(option, short = 's', default = "\"matrix.org\".to_owned()")]
    pub server_name: String,

    /// the path where session specific data should be stored.
    #[argh(option, default = "PathBuf::from(\"/tmp/\")")]
    pub session_path: PathBuf,
}
