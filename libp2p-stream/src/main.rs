use std::{io, ops::Deref, time::Duration};

use anyhow::{anyhow, Context, Result};
use futures::{io::BufReader, AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, StreamExt};
use libp2p::{
    identify, kad,
    multiaddr::Protocol,
    swarm::{self, NetworkBehaviour},
    Multiaddr, PeerId, Stream, StreamProtocol,
};
use libp2p_stream as stream;
use rand::{seq::IteratorRandom, RngCore};
use tokio::io::AsyncBufReadExt as _;
use tracing::level_filters::LevelFilter;
use tracing_subscriber::EnvFilter;

const MIRROR_PROTOCOL: StreamProtocol = StreamProtocol::new("/mirror"); // TODO: version number?

struct Config {
    wellknown_peers: Vec<String>,
}

impl Config {
    fn get_wellknown_peer(&self) -> &[impl Deref<Target = str>] {
        &self.wellknown_peers
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum NetworkMode {
    Server,
    Client,
}

struct ConnectMirrorNetworkCommand {
    peer: PeerId,
    response: tokio::sync::oneshot::Sender<Result<libp2p::Stream>>,
}

enum NetworkCommand {
    ConnectMirror(ConnectMirrorNetworkCommand),
    ListenMirror, // TODO
}

type NetworkCommandSender = tokio::sync::mpsc::UnboundedSender<NetworkCommand>;
type NetworkCommandReceiver = tokio::sync::mpsc::UnboundedReceiver<NetworkCommand>;

#[derive(NetworkBehaviour)]
struct Behaviour {
    kademlia: kad::Behaviour<kad::store::MemoryStore>,
    mirror: stream::Behaviour,
    identify: libp2p::identify::Behaviour,
}

async fn network_control_thread(
    network_mode: NetworkMode,
    mut command_receiver: NetworkCommandReceiver,
    may_initial_peer: Option<Multiaddr>,
) -> Result<()> {
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
                mirror: stream::Behaviour::new(),
                identify: identify::Behaviour::new(identify::Config::new(
                    "/ipfs/id/1.0.0".to_string(),
                    key.public(),
                )),
            })
        })?
        .with_swarm_config(|c| c.with_idle_connection_timeout(Duration::from_secs(10)))
        .build();

    swarm.listen_on("/ip4/0.0.0.0/udp/0/quic-v1".parse()?)?;

    if let Some(initial_peer) = may_initial_peer {
        tracing::info!("Dialing to an well-known peer");
        swarm.dial(initial_peer)?;
        let _ = swarm.behaviour_mut().kademlia.bootstrap();
    }

    swarm
        .behaviour_mut()
        .kademlia
        .set_mode(if network_mode == NetworkMode::Server {
            Some(kad::Mode::Server)
        } else {
            Some(kad::Mode::Client)
        });

    let mut incoming_streams = swarm
        .behaviour()
        .mirror
        .new_control()
        .accept(MIRROR_PROTOCOL)
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

    // Control thread
    // Poll the swarm to make progress.
    loop {
        tokio::select! {
            command = command_receiver.recv() => {
                let command = command;

                match command {
                    Some(NetworkCommand::ConnectMirror(command)) => {
                        let ConnectMirrorNetworkCommand { peer, response } = command;
                        tokio::spawn(client_mirror_connection_thread(peer, swarm.behaviour().mirror.new_control(), response));
                    }
                    Some(NetworkCommand::ListenMirror) => {
                        // TODO
                    }
                    None => {
                        tracing::warn!("Command channel closed");
                        break Ok(());
                    }
                }
            },
            event = swarm.next() => {
                let event = event.expect("never terminates");
                match event {
                    swarm::SwarmEvent::NewListenAddr { address, .. } => {
                        let listen_address = address.with_p2p(*swarm.local_peer_id()).unwrap();
                        tracing::info!(%listen_address);
                    }
                    event => tracing::trace!(?event),
                }
            }
        }
    }
}

// cargo run <server/client> <wellknown_peer> <dial_peer>.

#[tokio::main]
async fn main() -> Result<()> {
    let arg_server_client = std::env::args().nth(1).unwrap();
    let arg_wellknown_peer = std::env::args().nth(2);
    let arg_server_address = std::env::args().nth(3);

    let network_mode = match arg_server_client.as_str() {
        "server" => NetworkMode::Server,
        "client" => NetworkMode::Client,
        _ => anyhow::bail!("Invalid argument: expected `server` or `client`"),
    };

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

    let may_server_peerid = arg_server_address
        .map(|arg| arg.parse::<PeerId>())
        .transpose()
        .context("Failed to parse argument as `Multiaddr`")?;

    let may_initial_peer: Option<Multiaddr> = config
        .get_wellknown_peer()
        .iter()
        .choose(&mut rand::thread_rng())
        .map(|peer| peer.deref().parse().unwrap());

    let (command_sender, command_receiver) = tokio::sync::mpsc::unbounded_channel();

    let network_control_thread = tokio::spawn(network_control_thread(network_mode, command_receiver, may_initial_peer));

    if let Some(server_peerid) = may_server_peerid {
        let (response_sender, mut response_receiver) = tokio::sync::oneshot::channel();
        command_sender
            .send(NetworkCommand::ConnectMirror(ConnectMirrorNetworkCommand {
                peer: server_peerid,
                response: response_sender,
            })).expect("Server thread already exited");

        let stream = response_receiver.await.unwrap().unwrap();
        let (rx, mut tx) = stream.split();

        let mut stdin_lines = {
            let stdin = tokio::io::stdin();
            let reader = tokio::io::BufReader::new(stdin);    
            reader.lines()
        };

        let mut recv_lines = futures::io::BufReader::new(rx).lines();

        loop {
            tokio::select! {
                stdin_line = stdin_lines.next_line() => {
                    let stdin_line = stdin_line.expect("stdin closed");
                    let stdin_line = stdin_line.expect("stdin error");
                    tx.write_all(stdin_line.as_bytes()).await?;
                    tx.write("\n".as_bytes()).await?;
                    tx.flush().await?;
                    println!("Sent: {}", stdin_line);
                },
                recv_line = recv_lines.next() => {
                    let recv_line = recv_line.expect("recv closed");
                    let recv_line = recv_line.expect("recv error");
                    println!("Recv: {}", recv_line);
                }
            }
        }
    } else {
        let (result,) = tokio::join!(network_control_thread);

        result??;
    }

    Ok(())
}

/// A very simple, `async fn`-based connection handler for our custom echo protocol.
async fn client_mirror_connection_thread(peer: PeerId, mut control: stream::Control, result: tokio::sync::oneshot::Sender<Result<libp2p::Stream>>) {
    let stream = control.open_stream(peer, MIRROR_PROTOCOL).await;

    result.send(stream.map_err(|e| {
        tracing::debug!(%peer, %e);
        anyhow!("Failed to open stream to peer {peer}")
    })).unwrap();
}

async fn echo(mut stream: Stream) -> io::Result<usize> {
    let mut total = 0;

    let mut buf = [0u8; 100];

    loop {
        let read = stream.read(&mut buf).await?;

        tracing::debug!("Echoing {} bytes", read);

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
