use std::{cell::OnceCell, io, ops::Deref, sync::OnceLock, time::Duration};

use anyhow::{anyhow, Context, Result};
use futures::{AsyncReadExt, AsyncWriteExt, StreamExt};
use libp2p::{
    identify, kad,
    swarm::{self, NetworkBehaviour},
    Multiaddr, PeerId, Stream, StreamProtocol,
};
use libp2p_stream::{self as stream, IncomingStreams};
use rand::{seq::IteratorRandom, RngCore};
use tokio::{io::AsyncBufReadExt as _, task::JoinHandle};
use tracing::{level_filters::LevelFilter, span};
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

struct ListenMirrorNetworkCommand {
    response: tokio::sync::oneshot::Sender<Result<IncomingStreams>>,
}

enum NetworkCommand {
    ConnectMirror(ConnectMirrorNetworkCommand),
    ListenMirror(ListenMirrorNetworkCommand),
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

    let mut incoming_streams = Some(
        swarm
            .behaviour()
            .mirror
            .new_control()
            .accept(MIRROR_PROTOCOL)
            .unwrap(),
    );

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
                    Some(NetworkCommand::ListenMirror(command)) => {
                        let ListenMirrorNetworkCommand { response } = command;

                        let incoming_streams = incoming_streams.take();

                        if incoming_streams.is_none() {
                            response.send(Err(anyhow!("Incoming stream already taken"))).map_err(|_| anyhow!("Failed to send incoming streams")).unwrap();
                            continue;
                        }

                        response.send(Ok(incoming_streams.unwrap())).map_err(|_| anyhow!("Failed to send incoming streams")).unwrap();
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

static RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();

fn get_tokio_runtime() -> &'static tokio::runtime::Runtime {
    RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("Failed to create runtime")
    })
}

struct MirrorClient {
    stream: libp2p::Stream,
}

impl MirrorClient {
    async fn read(&mut self, buffer: &mut [u8], offset: usize, count: usize) -> Result<usize> {
        self.stream
            .read(&mut buffer[offset..offset + count])
            .await
            .map_err(|_| anyhow!("Failed to read stream"))
    }

    async fn write(&mut self, buffer: &[u8], offset: usize, count: usize) -> Result<()> {
        self.stream
            .write_all(&buffer[offset..offset + count])
            .await
            .map_err(|_| anyhow!("Failed to write to stream"))
    }
}

struct MirrorListener {
    incoming_streams: IncomingStreams,
}

impl MirrorListener {
    async fn accept(&mut self) -> Result<MirrorClient> {
        // FIXME: Use stream itself?
        match self.incoming_streams.next().await {
            Some((peer, stream)) => Ok(MirrorClient { stream }),
            None => {
                tracing::warn!("Incoming stream closed");
                Err(anyhow!("Incoming stream closed"))
            }
        }
    }
}

struct NetworkContext {
    command_sender: NetworkCommandSender,
    network_thread: JoinHandle<std::result::Result<(), anyhow::Error>>,
}

impl NetworkContext {
    fn new(network_mode: NetworkMode, initial_peer: Option<Multiaddr>) -> Self {
        let (command_sender, command_receiver) = tokio::sync::mpsc::unbounded_channel();

        // TODO: join support
        let network_thread: JoinHandle<std::result::Result<(), anyhow::Error>> = tokio::spawn(
            network_control_thread(network_mode, command_receiver, initial_peer),
        );

        Self {
            command_sender,
            network_thread,
        }
    }

    async fn connect_mirror(&self, peer: PeerId) -> Result<MirrorClient> {
        let (response_sender, response_receiver) = tokio::sync::oneshot::channel();
        self.command_sender
            .send(NetworkCommand::ConnectMirror(ConnectMirrorNetworkCommand {
                peer,
                response: response_sender,
            }))?;

        let stream = response_receiver.await.unwrap()?;

        Ok(MirrorClient { stream })
    }

    async fn listen_mirror(&self) -> Result<MirrorListener> {
        let (response_sender, response_receiver) = tokio::sync::oneshot::channel();
        self.command_sender
            .send(NetworkCommand::ListenMirror(ListenMirrorNetworkCommand {
                response: response_sender,
            }))?;

        let incoming_streams = response_receiver.await.unwrap()?;

        Ok(MirrorListener { incoming_streams })
    }

    async fn wait(self) -> Result<()> {
        tokio::join!(self.network_thread).0??;
        Ok(())
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

    let may_server_peer = arg_server_address
        .map(|arg| arg.parse::<PeerId>())
        .transpose()
        .context("Failed to parse argument as `Multiaddr`")?;

    let may_initial_peer: Option<Multiaddr> = config
        .get_wellknown_peer()
        .iter()
        .choose(&mut rand::thread_rng())
        .map(|peer| peer.deref().parse().unwrap());

    let network = NetworkContext::new(network_mode, may_initial_peer);

    if let Some(server_peer) = may_server_peer {
        let span = span!(tracing::Level::INFO, "client", %server_peer);
        let _enter = span.enter();
        let mut client = network.connect_mirror(server_peer).await?;

        let mut recv_buffer = [0u8; 100];
        let mut send_buffer = [0u8; 100];

        let mut stdin_lines = {
            let stdin = tokio::io::stdin();
            let reader = tokio::io::BufReader::new(stdin);
            reader.lines()
        };

        tracing::info!("Connected to server");

        loop {
            tokio::select! {
                read_count = client.read(&mut recv_buffer, 0, 100) => {
                    match read_count {
                        Ok(0) => {
                            tracing::info!("Connection closed");
                            break;
                        }
                        Ok(n) => {
                            tracing::info!("Received {} bytes", n);
                        }
                        Err(e) => {
                            tracing::error!(%e);
                            break;
                        }
                    }
                }

                stdin = stdin_lines.next_line() => {
                    let stdin = stdin.expect("stdin closed");
                    let stdin = stdin.expect("stdin error");

                    client.write(&stdin.as_bytes(), 0, stdin.as_bytes().len()).await?;
                }
            }
        }
    } else {
        let mut listener = network.listen_mirror().await?;

        while let Ok(client) = listener.accept().await {
            // FIXME: OOM when client count is too much
            tokio::spawn(echo(client.stream));
        }
        network.wait().await?;
    }

    Ok(())
}

/// A very simple, `async fn`-based connection handler for our custom echo protocol.
async fn client_mirror_connection_thread(
    peer: PeerId,
    mut control: stream::Control,
    result: tokio::sync::oneshot::Sender<Result<libp2p::Stream>>,
) {
    let stream = control.open_stream(peer, MIRROR_PROTOCOL).await;

    result
        .send(stream.map_err(|e| {
            tracing::debug!(%peer, %e);
            anyhow!("Failed to open stream to peer {peer}")
        }))
        .unwrap();
}

async fn echo(mut stream: Stream) -> io::Result<usize> {
    let mut total = 0;

    let mut buf = [0u8; 2048];

    loop {
        let read = stream.read(&mut buf).await?;

        tracing::trace!("Echoing {} bytes", read);

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
