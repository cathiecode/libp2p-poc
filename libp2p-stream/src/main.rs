use std::{io, ops::Deref, time::Duration};

use anyhow::{Context, Result};
use futures::{AsyncReadExt, AsyncWriteExt, StreamExt};
use libp2p::{
    identify, kad,
    multiaddr::Protocol,
    swarm::{self, NetworkBehaviour},
    Multiaddr, PeerId, Stream, StreamProtocol,
};
use libp2p_stream as stream;
use rand::{seq::IteratorRandom, RngCore};
use tracing::level_filters::LevelFilter;
use tracing_subscriber::EnvFilter;

const ECHO_PROTOCOL: StreamProtocol = StreamProtocol::new("/echo");

struct Config {
    wellknown_peers: Vec<String>,
}

impl Config {
    fn get_wellknown_peer(&self) -> &[impl Deref<Target = str>] {
        &self.wellknown_peers
    }
}

#[derive(NetworkBehaviour)]
struct Behaviour {
    kademlia: kad::Behaviour<kad::store::MemoryStore>,
    stream: stream::Behaviour,
    identify: libp2p::identify::Behaviour,
}

// cargo run <server/client> <wellknown_peer> <dial_peer>.

#[tokio::main]
async fn main() -> Result<()> {
    let arg_server_client = std::env::args().nth(1).unwrap();
    let arg_wellknown_peer = std::env::args().nth(2);
    let arg_server_address = std::env::args().nth(3);

    let wellknown_peer = if let Some(wellknown_peer) = arg_wellknown_peer {
        Some(wellknown_peer)
    } else {
        None
    };

    let config = Config {
        wellknown_peers: wellknown_peer
            .map(|wellknown_peer| vec![wellknown_peer])
            .unwrap_or(vec![]),
    };

    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::builder()
                .with_default_directive(LevelFilter::DEBUG.into())
                .from_env()?,
        )
        .init();

    let maybe_server_address = arg_server_address
        .map(|arg| arg.parse::<Multiaddr>())
        .transpose()
        .context("Failed to parse argument as `Multiaddr`")?;

    let mut swarm = libp2p::SwarmBuilder::with_new_identity()
        .with_tokio()
        .with_quic()
        // .with_tcp(libp2p::tcp::Config::default(), libp2p::noise::Config::new, libp2p::yamux::Config::default)?
        .with_behaviour(|key| {
            Ok(Behaviour {
                kademlia: kad::Behaviour::new(
                    key.public().to_peer_id(),
                    kad::store::MemoryStore::new(key.public().to_peer_id()),
                ),
                stream: stream::Behaviour::new(),
                identify: identify::Behaviour::new(identify::Config::new(
                    "/ipfs/id/1.0.0".to_string(),
                    key.public(),
                )),
            })
        })?
        .with_swarm_config(|c| c.with_idle_connection_timeout(Duration::from_secs(10)))
        .build();

    swarm.listen_on("/ip4/0.0.0.0/udp/0/quic-v1".parse()?)?;

    let may_initial_peer: Option<Multiaddr> = config
        .get_wellknown_peer()
        .iter()
        .choose(&mut rand::thread_rng())
        .map(|peer| peer.deref().parse().unwrap());

    if let Some(initial_peer) = may_initial_peer {
        tracing::info!("Dialing to an well-known peer");
        swarm.dial(initial_peer)?;
        let _ = swarm.behaviour_mut().kademlia.bootstrap();
    }

    swarm
        .behaviour_mut()
        .kademlia
        .set_mode(Some(if arg_server_client == "server" {kad::Mode::Server} else {kad::Mode::Client}));

    let mut incoming_streams = swarm
        .behaviour()
        .stream
        .new_control()
        .accept(ECHO_PROTOCOL)
        .unwrap();

    // Deal with incoming streams.
    // Spawning a dedicated task is just one way of doing this.
    // libp2p doesn't care how you handle incoming streams but you _must_ handle them somehow.
    // To mitigate DoS attacks, libp2p will internally drop incoming streams if your application
    // cannot keep up processing them.
    tokio::spawn(async move {
        // This loop handles incoming streams _sequentially_ but that doesn't have to be the case.
        // You can also spawn a dedicated task per stream if you want to.
        // Be aware that this breaks backpressure though as spawning new tasks is equivalent to an
        // unbounded buffer. Each task needs memory meaning an aggressive remote peer may
        // force you OOM this way.

        loop {
            match incoming_streams.next().await {
                Some((peer, stream)) => {
                    match echo(stream).await {
                        Ok(n) => {
                            tracing::info!(%peer, "Echoed {n} bytes!");
                        }
                        Err(e) => {
                            tracing::warn!(%peer, "Echo failed: {e}");
                            continue;
                        }
                    };
                }
                None => {
                    tracing::warn!("Incoming stream pipe broken");
                    break;
                }
            }
        }
    });

    // In this demo application, the dialing peer initiates the protocol.
    if let Some(address) = maybe_server_address {
        let Some(Protocol::P2p(peer_id)) = address.iter().last() else {
            anyhow::bail!("Provided address does not end in `/p2p`");
        };

        tokio::spawn(client_stream_connection_handler(
            peer_id,
            swarm.behaviour().stream.new_control(),
        ));
    }

    // Control thread
    // Poll the swarm to make progress.
    loop {
        let event = swarm.next().await.expect("never terminates");

        match event {
            swarm::SwarmEvent::NewListenAddr { address, .. } => {
                let listen_address = address.with_p2p(*swarm.local_peer_id()).unwrap();
                tracing::info!(%listen_address);
            }
            event => tracing::trace!(?event),
        }
    }
}

/// A very simple, `async fn`-based connection handler for our custom echo protocol.
async fn client_stream_connection_handler(peer: PeerId, mut control: stream::Control) {
    tracing::info!(%peer, "Starting echo protocol");

    loop {
        tokio::time::sleep(Duration::from_secs(1)).await; // Wait a second between echos.

        let stream = match control.open_stream(peer, ECHO_PROTOCOL).await {
            Ok(stream) => stream,
            Err(error @ stream::OpenStreamError::UnsupportedProtocol(_)) => {
                tracing::info!(%peer, %error);
                return;
            }
            Err(error) => {
                // Other errors may be temporary.
                // In production, something like an exponential backoff / circuit-breaker may be
                // more appropriate.
                tracing::debug!(%peer, %error);
                continue;
            }
        };

        if let Err(e) = send(stream).await {
            tracing::warn!(%peer, "Echo protocol failed: {e}");
            continue;
        }

        tracing::info!(%peer, "Echo complete!")
    }
}

async fn echo(mut stream: Stream) -> io::Result<usize> {
    let mut total = 0;

    let mut buf = [0u8; 100];

    loop {
        let read = stream.read(&mut buf).await?;
        if read == 0 {
            return Ok(total);
        }

        total += read;
        stream.write_all(&buf[..read]).await?;
    }
}

async fn send(mut stream: Stream) -> io::Result<()> {
    let num_bytes = rand::random::<usize>() % 1000;

    let mut bytes = vec![0; num_bytes];
    rand::thread_rng().fill_bytes(&mut bytes);

    stream.write_all(&bytes).await?;

    let mut buf = vec![0; num_bytes];
    stream.read_exact(&mut buf).await?;

    if bytes != buf {
        return Err(io::Error::new(io::ErrorKind::Other, "incorrect echo"));
    }

    stream.close().await?;

    Ok(())
}
