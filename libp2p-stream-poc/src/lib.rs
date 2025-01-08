// Based on libp2p stream example 2025-01-07

use std::{backtrace, ffi::{c_char, CStr}, io, time::Duration};

use anyhow::{Context, Result};
use futures::{AsyncReadExt, AsyncWriteExt, StreamExt};
use libp2p::{multiaddr::Protocol, Multiaddr, PeerId, Stream, StreamProtocol};
use libp2p_stream as stream;
use rand::RngCore;
use tokio::sync::mpsc::error::TryRecvError;
use tracing::{event, level_filters::LevelFilter};
use tracing_subscriber::EnvFilter;

const ECHO_PROTOCOL: StreamProtocol = StreamProtocol::new("/echo");

/// A very simple, `async fn`-based connection handler for our custom echo protocol.
async fn connection_handler(peer: PeerId, mut control: stream::Control) {
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

enum NetworkThreadCommand {
    Shutdown,
    BrokenCommandPipe,
    BrokenIncomingStreamPipe,
}

enum NetworkListenerEvent {
    Command(NetworkThreadCommand),
    IncomingStream((PeerId, Stream)),
}

enum NetworkEvent {
    Message(String),
}

async fn network_thread_listen(
    mut control: tokio::sync::mpsc::Receiver<NetworkThreadCommand>,
    mut event_channel: tokio::sync::mpsc::Sender<NetworkEvent>,
) -> Result<()> {
    let mut swarm = libp2p::SwarmBuilder::with_new_identity()
        .with_tokio()
        .with_quic()
        .with_behaviour(|_| stream::Behaviour::new())?
        .with_swarm_config(|c| c.with_idle_connection_timeout(Duration::from_secs(10)))
        .build();

    swarm.listen_on("/ip4/0.0.0.0/udp/0/quic-v1".parse()?)?;

    let mut incoming_streams = swarm
        .behaviour()
        .new_control()
        .accept(ECHO_PROTOCOL)
        .unwrap();

    tokio::spawn(async move {
        println!("Network thread started");

        loop {
            let event = tokio::select! {
                incoming_stream = async {
                    match incoming_streams.next().await {
                        Some((peer, stream)) => NetworkListenerEvent::IncomingStream((peer, stream)),
                        None => NetworkListenerEvent::Command(NetworkThreadCommand::BrokenIncomingStreamPipe),
                    }
                } => incoming_stream,
                command = async {
                    match control.recv().await {
                        Some(command) => NetworkListenerEvent::Command(command),
                        None => NetworkListenerEvent::Command(NetworkThreadCommand::BrokenCommandPipe),
                    }
                } => command,
            };

            match event {
                NetworkListenerEvent::Command(network_thread_command) => {
                    match network_thread_command {
                        NetworkThreadCommand::Shutdown => {
                            println!("Shutting down network thread");
                            break;
                        }
                        NetworkThreadCommand::BrokenCommandPipe => {
                            println!("Broken command pipe");
                            break;
                        }
                        NetworkThreadCommand::BrokenIncomingStreamPipe => {
                            println!("Broken incoming stream pipe");
                            break;
                        }
                    }
                }
                NetworkListenerEvent::IncomingStream((peer, stream)) => {
                    match echo(stream).await {
                        Ok(n) => {
                            tracing::info!(%peer, "Echoed {n} bytes!");
                            event_channel.send(NetworkEvent::Message(format!("Echoed {n} bytes!"))).await.unwrap();
                        }
                        Err(e) => {
                            tracing::warn!(%peer, "Echo failed: {e}");
                        }
                    };
                }
            }
        }
    });

    loop {
        let event = swarm.next().await.expect("never terminates");

        match event {
            libp2p::swarm::SwarmEvent::NewListenAddr { address, .. } => {
                let listen_address = address.with_p2p(*swarm.local_peer_id()).unwrap();
                tracing::info!(%listen_address);
            }
            event => tracing::trace!(?event),
        }
    }

    Ok(())
}

pub trait MessageCallback: for<'a> Fn(&'a str) -> () {}

impl <S> MessageCallback for S where S: for<'a> Fn(&'a str) -> () {}

pub struct NetworkContext {
    counter: i32,
    command_sender: tokio::sync::mpsc::Sender<NetworkThreadCommand>,
    event_receiver: tokio::sync::mpsc::Receiver<NetworkEvent>,
    callback: Option<Box<dyn MessageCallback>>
}

impl NetworkContext {
    pub fn new() -> NetworkContext {
        let (command_sender, command_receiver) = tokio::sync::mpsc::channel(64);
        let (event_sender, event_receiver) = tokio::sync::mpsc::channel(64);

        std::thread::spawn(move || {
            let result = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .unwrap()
                .block_on(network_thread_listen(command_receiver, event_sender));
            println!("Network thread started: {:?}", result);
        });

        NetworkContext {
            counter: 0,
            command_sender,
            callback: None,
            event_receiver,
        }
    }

    pub fn shutdown(&mut self) {
        println!("Shutting down");
        let _ = self.command_sender.send(NetworkThreadCommand::Shutdown);
    }

    pub fn on_message<F>(&mut self, callback: F) where F: MessageCallback + 'static {
        self.callback = Some(Box::new(callback));
    }

    pub fn update(&mut self) {
        loop {
            let may_event = self.event_receiver.try_recv();

            let callback = match self.callback {
                Some(ref callback) => callback,
                None => break,
            };

            match may_event {
                Ok(NetworkEvent::Message(message)) => {
                    callback(&message);
                },
                Err(TryRecvError::Empty) => {
                    break;
                },
                Err(TryRecvError::Disconnected) => {
                    // TODO
                    break;
                }
            }
        }

        while let Ok(event) = self.event_receiver.try_recv() {
            match event {
                NetworkEvent::Message(message) => {
                    if let Some(callback) = &self.callback {
                        callback(&message);
                    }
                }
            }
        }


    }
}

pub fn init_internal() -> Result<(), ()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::builder()
                .with_default_directive(LevelFilter::INFO.into())
                .from_env()
                .map_err(|_| ())?,
        )
        .init();

    Ok(())
}

#[no_mangle]
pub extern "C" fn init() -> i32 {
    init_internal().map_or(1, |_| 0)
}

#[no_mangle]
pub extern "C" fn create_context() -> *mut NetworkContext {
    Box::into_raw(Box::new(NetworkContext::new()))
}

#[no_mangle]
pub extern "C" fn destroy_context(ptr: *mut NetworkContext) {
    if ptr.is_null() {
        return;
    }

    unsafe {
        let mut context = Box::from_raw(ptr);

        context.shutdown();
    }
}

#[no_mangle]
pub extern "C" fn increment_counter(ptr: *mut NetworkContext) {
    if ptr.is_null() {
        return;
    }

    unsafe {
        (*ptr).counter += 1;
    }
}

#[no_mangle]
pub extern "C" fn get_counter(ptr: *mut NetworkContext) -> i32 {
    if ptr.is_null() {
        return 0;
    }

    unsafe { (*ptr).counter }
}

#[no_mangle]
pub extern "C" fn on_receive(ptr: *mut NetworkContext, callback: extern "C" fn (*const c_char) -> ()) {
    let wrapped_callback = move |message: &str| {
        callback(message.as_ptr() as *const c_char);
    };

    unsafe {
        (*ptr).on_message(wrapped_callback);
    }
}

#[no_mangle]
pub extern "C" fn update(ptr: *mut NetworkContext) {
    if ptr.is_null() {
        return;
    }

    unsafe {
        (*ptr).update();
    }
}
