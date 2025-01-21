use std::time::Duration;

use anyhow::{anyhow, Result};
use futures::{AsyncReadExt, AsyncWriteExt, StreamExt};
use libp2p::{
    identify, kad, swarm::{self, NetworkBehaviour}, Multiaddr, PeerId, StreamProtocol
};
use libp2p_stream::{self as stream, IncomingStreams};
use tokio::task::JoinHandle;

use crate::result::{map_ffi_error, CommonError};

const MIRROR_PROTOCOL: StreamProtocol = StreamProtocol::new("/mirror"); // TODO: version number?

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum NetworkMode {
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
pub struct Behaviour {
    kademlia: kad::Behaviour<kad::store::MemoryStore>,
    mirror: stream::Behaviour,
    identify: libp2p::identify::Behaviour,
}

async fn network_control_thread(
    network_mode: NetworkMode,
    mut identity: Vec<u8>,
    mut command_receiver: NetworkCommandReceiver,
    may_initial_peer: Option<Multiaddr>,
    may_listen_addr: Option<String>,
) -> Result<()> {

    let identity = libp2p::identity::ed25519::Keypair::try_from_bytes(&mut identity)?.into();
    let mut swarm = libp2p::SwarmBuilder::with_existing_identity(identity)
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
        .with_swarm_config(|c| {
            c.with_idle_connection_timeout(Duration::from_secs(10))
        })
        .build();

    let listen_addr: Multiaddr = match may_listen_addr {
        Some(multiaddr) => multiaddr.parse().map_err(|_| CommonError::InvalidInput)?,
        None => {
            "/ip4/0.0.0.0/udp/0/quic-v1".parse().unwrap()
        }
    };

    swarm.listen_on(listen_addr)?;

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

/// A very simple, `async fn`-based connection handler for our custom echo protocol.
async fn client_mirror_connection_thread(
    peer: PeerId,
    mut control: stream::Control,
    result: tokio::sync::oneshot::Sender<Result<libp2p::Stream>>,
) {
    tracing::debug!(%peer, "Opening stream to peer");
    let stream = control
        .open_stream(peer, MIRROR_PROTOCOL)
        .await
        .map_err(|e| {
            tracing::debug!(%peer, %e);
            CommonError::FailedToConnect.into()
        });

    result.send(stream).unwrap();
}

pub struct MirrorClient {
    stream: libp2p::Stream,
}

impl MirrorClient {
    pub async fn read(&mut self, buffer: &mut [u8], offset: usize, count: usize) -> Result<usize> {
        self.stream
            .read(&mut buffer[offset..offset + count])
            .await
            .map_err(|_| anyhow!("Failed to read stream"))
    }

    pub async fn write(&mut self, buffer: &[u8], offset: usize, count: usize) -> Result<()> {
        self.stream
            .write_all(&buffer[offset..offset + count])
            .await
            .map_err(|_| anyhow!("Failed to write to stream"))
    }

    pub fn into_inner(self) -> libp2p::Stream {
        self.stream
    }
}

pub struct MirrorListener {
    incoming_streams: IncomingStreams,
}

impl MirrorListener {
    pub async fn accept(&mut self) -> Result<MirrorClient> {
        // FIXME: Use stream itself?
        match self.incoming_streams.next().await {
            Some((_peer, stream)) => Ok(MirrorClient { stream }),
            None => {
                tracing::warn!("Incoming stream closed");
                Err(anyhow!("Incoming stream closed"))
            }
        }
    }
}

pub struct NetworkContext {
    command_sender: NetworkCommandSender,
    network_thread: JoinHandle<std::result::Result<(), anyhow::Error>>,
}

impl NetworkContext {
    pub fn new(network_mode: NetworkMode, identity: Vec<u8>, initial_peer: Option<Multiaddr>, listen_addr: Option<String>) -> Self {
        let (command_sender, command_receiver) = tokio::sync::mpsc::unbounded_channel();

        // TODO: join support
        let network_thread: JoinHandle<std::result::Result<(), anyhow::Error>> = tokio::spawn(
            network_control_thread(network_mode, identity, command_receiver, initial_peer, listen_addr),
        );

        Self {
            command_sender,
            network_thread,
        }
    }

    pub async fn connect_mirror(&self, peer: PeerId) -> Result<MirrorClient> {
        let (response_sender, response_receiver) = tokio::sync::oneshot::channel();
        self.command_sender
            .send(NetworkCommand::ConnectMirror(ConnectMirrorNetworkCommand {
                peer,
                response: response_sender,
            }))
            .map_err(|_| CommonError::LogicError(56))?;

        let stream = response_receiver
            .await
            .map_err(|_| CommonError::LogicError(72))?
            .map_err(map_ffi_error(88))?;

        Ok(MirrorClient { stream })
    }

    pub async fn listen_mirror(&self) -> Result<MirrorListener> {
        let (response_sender, response_receiver) = tokio::sync::oneshot::channel();
        self.command_sender
            .send(NetworkCommand::ListenMirror(ListenMirrorNetworkCommand {
                response: response_sender,
            }))?;

        let incoming_streams = response_receiver.await.unwrap()?;

        Ok(MirrorListener { incoming_streams })
    }

    pub async fn wait(self) -> Result<()> {
        tokio::join!(self.network_thread).0??;
        Ok(())
    }
}
