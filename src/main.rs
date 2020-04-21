use std::path::PathBuf;

use structopt::StructOpt;

mod cache;
mod chain;
mod context;
mod error;
mod fs;
mod host;
mod http;
mod state;
mod transaction;
mod value;

const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Clone, StructOpt)]
pub struct HostConfig {
    #[structopt(long = "http_port", default_value = "8702")]
    pub http_port: u16,

    #[structopt(long = "data_dir", default_value = "/tmp/tc/data")]
    pub data_dir: PathBuf,

    #[structopt(long = "workspace", default_value = "/tmp/tc/tmp")]
    pub workspace: PathBuf,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let config = HostConfig::from_args();

    println!("Tinychain version {}", VERSION);
    println!("Working directory: {}", &config.workspace.to_str().unwrap());

    let data_dir = fs::Dir::new(value::Link::to("/")?, config.data_dir);
    let workspace = fs::Dir::new_tmp(value::Link::to("/")?, config.workspace);
    let host = host::Host::new(data_dir, workspace)?;
    http::listen(host, config.http_port).await?;
    Ok(())
}
