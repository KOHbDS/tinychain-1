use std::net::IpAddr;
use std::path::PathBuf;
use std::str::FromStr;

use bytes::Bytes;
use destream::de::FromStream;
use futures::{future, stream, TryFutureExt};
use structopt::StructOpt;
use tokio::time::Duration;

use tc_error::*;
use tc_transact::{Transact, TxnId};

use tc_value::{LinkHost, LinkProtocol};
use tinychain::gateway::Gateway;
use tinychain::object::InstanceClass;
use tinychain::*;

type TokioError = Box<dyn std::error::Error + Send + Sync + 'static>;

const MIN_CACHE_SIZE: usize = 5000;

fn data_size(flag: &str) -> TCResult<u64> {
    const ERR: &str = "unable to parse data size";

    if flag.is_empty() || flag == "0" {
        return Ok(0);
    }

    let size = u64::from_str_radix(&flag[0..flag.len() - 1], 10)
        .map_err(|_| TCError::bad_request(ERR, flag))?;

    if flag.ends_with('K') {
        Ok(size * 1000)
    } else if flag.ends_with('M') {
        Ok(size * 1_000_000)
    } else if flag.ends_with('G') {
        Ok(size * 1_000_000_000)
    } else {
        Err(TCError::bad_request(ERR, flag))
    }
}

fn duration(flag: &str) -> TCResult<Duration> {
    u64::from_str(flag)
        .map(Duration::from_secs)
        .map_err(|_| TCError::bad_request("invalid duration", flag))
}

#[derive(Clone, StructOpt)]
struct Config {
    #[structopt(
        long = "address",
        default_value = "0.0.0.0",
        about = "The IP address to bind"
    )]
    pub address: IpAddr,

    #[structopt(long = "log_level", default_value = "warn")]
    pub log_level: String,

    #[structopt(
        long = "workspace",
        default_value = "/tmp/tc/tmp",
        about = "workspace directory"
    )]
    pub workspace: PathBuf,

    #[structopt(long = "cache_size", default_value = "1G", parse(try_from_str = data_size))]
    pub cache_size: u64,

    #[structopt(
        long = "data_dir",
        about = "data directory (required to host a Cluster)"
    )]
    pub data_dir: Option<PathBuf>,

    #[structopt(long = "cluster", about = "path(s) to Cluster config files")]
    pub clusters: Vec<PathBuf>,

    #[structopt(
        long = "request_ttl",
        default_value = "30",
        parse(try_from_str = duration),
        about = "maximum allowed request duration"
    )]
    pub request_ttl: Duration,

    #[structopt(long = "http_port", default_value = "8702")]
    pub http_port: u16,
}

impl Config {
    fn gateway(&self) -> gateway::Config {
        gateway::Config {
            addr: self.address,
            http_port: self.http_port,
            request_ttl: self.request_ttl,
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), TokioError> {
    let config = Config::from_args();
    let gateway_config = config.gateway();

    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or(config.log_level))
        .init();

    if !config.workspace.exists() {
        log::info!(
            "workspace directory {:?} does not exist, attempting to create it...",
            config.workspace
        );

        std::fs::create_dir_all(&config.workspace)?;
    }

    let cache_size = config.cache_size as usize;
    if cache_size < MIN_CACHE_SIZE {
        return Err(TCError::bad_request("the minimum cache size is", MIN_CACHE_SIZE).into());
    }

    let cache = freqfs::Cache::new(config.cache_size as usize, Duration::from_secs(1), None);
    let workspace = cache.clone().load(config.workspace).await?;
    let txn_id = TxnId::new(Gateway::time());

    let data_dir = if let Some(data_dir) = config.data_dir {
        if !data_dir.exists() {
            panic!("{:?} does not exist--create it or provide a different path for the --data_dir flag", data_dir);
        }

        let data_dir = cache.load(data_dir).await?;
        tinychain::fs::Dir::load(data_dir, txn_id)
            .map_ok(Some)
            .await?
    } else {
        None
    };

    #[cfg(feature = "tensor")]
    {
        tc_tensor::print_af_info();
        println!();
    }

    let txn_server = tinychain::txn::TxnServer::new(workspace).await;
    let mut clusters = Vec::with_capacity(config.clusters.len());
    if !config.clusters.is_empty() {
        let txn_server = txn_server.clone();
        let kernel = tinychain::Kernel::new(std::iter::empty());
        let gateway = Gateway::new(gateway_config.clone(), kernel, txn_server.clone());
        let token = gateway.new_token(&txn_id)?;
        let txn = txn_server.new_txn(gateway, txn_id, token).await?;

        let data_dir = data_dir.ok_or_else(|| {
            TCError::internal("the --data_dir option is required to host a Cluster")
        })?;

        let host = LinkHost::from((
            LinkProtocol::HTTP,
            config.address.clone(),
            Some(config.http_port),
        ));

        for path in config.clusters {
            let config = tokio::fs::read(&path)
                .await
                .expect(&format!("read from {:?}", &path));

            let mut decoder = destream_json::de::Decoder::from_stream(stream::once(future::ready(
                Ok(Bytes::from(config)),
            )));

            let cluster = match InstanceClass::from_stream((), &mut decoder).await {
                Ok(class) => {
                    cluster::instantiate(&txn, host.clone(), class, data_dir.clone()).await?
                }
                Err(cause) => panic!("error parsing cluster config {:?}: {}", path, cause),
            };

            clusters.push(cluster);
        }

        data_dir.commit(&txn_id).await;
    }

    let kernel = tinychain::Kernel::new(clusters);
    let gateway = tinychain::gateway::Gateway::new(gateway_config, kernel, txn_server);

    log::info!("starting server, cache size is {}", config.cache_size);
    gateway.listen().await
}
