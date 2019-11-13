#[macro_use]
extern crate log;
extern crate nimiq_blockchain as blockchain;
extern crate nimiq_consensus as consensus;
extern crate nimiq_mempool as mempool;
extern crate nimiq_network as network;
extern crate nimiq_block as block;

use std::io;
use std::io::Read;
use std::fs::File;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;

use futures::future::Future;
use futures::stream::Stream;
use hyper::server::conn::Http;
use native_tls::{Identity, TlsAcceptor};
use tokio::net::TcpListener;

use consensus::consensus::Consensus;

use crate::error::Error;
use crate::metrics::chain::ChainMetrics;
use crate::metrics::mempool::MempoolMetrics;
use crate::metrics::network::NetworkMetrics;

macro_rules! attributes {
    // Empty attributes.
    {} => ({
        use $crate::server::attributes::VecAttributes;

        VecAttributes::new()
    });

    // Non-empty attributes, no trailing comma.
    //
    // In this implementation, key/value pairs separated by commas.
    { $( $key:expr => $value:expr ),* } => {
        attributes!( $(
            $key => $value,
        )* )
    };

    // Non-empty attributes, trailing comma.
    //
    // In this implementation, the comma is part of the value.
    { $( $key:expr => $value:expr, )* } => ({
        use $crate::server::attributes::VecAttributes;

        let mut attributes = VecAttributes::new();

        $(
            attributes.add($key, $value);
        )*

        attributes
    })
}

pub mod server;
pub mod metrics;
pub mod error;

pub fn metrics_server(consensus: Arc<Consensus>, ip: IpAddr, port: u16, password: Option<String>, identity_file: String, identity_password: String) -> Result<Box<dyn Future<Item=(), Error=()> + Send + Sync>, Error> {

    let mut file = File::open(identity_file).unwrap();
    let mut pkcs12 = vec![];
    file.read_to_end(&mut pkcs12).unwrap();
    let pkcs12 = Identity::from_pkcs12(&pkcs12,  &identity_password).unwrap();

    let tls_cx = TlsAcceptor::builder(pkcs12).build().unwrap();
    let tls_cx = tokio_tls::TlsAcceptor::from(tls_cx);

    let srv = TcpListener::bind(&SocketAddr::new(ip, port)).expect("Error binding local port");
    let http_proto = Http::new();

    let http_server = http_proto
        .serve_incoming(
            srv.incoming().and_then(move |socket| {
                tls_cx
                    .accept(socket)
                    .map_err(|e| io::Error::new(io::ErrorKind::Other, e))
            }),
            move || {
                server::MetricsServer::new(
                    vec![
                        Arc::new(ChainMetrics::new(consensus.blockchain.clone())),
                        Arc::new(MempoolMetrics::new(consensus.mempool.clone())),
                        Arc::new(NetworkMetrics::new(consensus.network.clone()))
                    ],
                    attributes!{ "peer" => consensus.network.network_config.peer_address() },
                password.clone())
            }
        )
        .then(|res| {
            match res {
                Ok(conn) => Ok(Some(conn)),
                Err(e) => {
                    error!("Metrics server failed: {}", e);
                    Ok(None)
                },
            }
        })
        .for_each(|conn_opt| {
            if let Some(conn) = conn_opt {
                hyper::rt::spawn(
                    conn.and_then(|c| c.map_err(|e| panic!("Metrics server unrecoverable error {}", e)))
                        .map_err(|e| error!("Metrics server connection error: {}", e)),
                );
            }

            Ok(())
        });
        Ok(Box::new(http_server))
}
