use std::{ffi::{CStr, CString}, sync::mpsc::Receiver};

use libp2p::PeerId;
use mirrorp2p::*;
use serial_test::serial;

#[derive(Copy, Clone)]
struct PromiseSendable<T>(T);

unsafe impl<T> std::marker::Send for PromiseSendable<T> {}
unsafe impl<T> std::marker::Sync for PromiseSendable<T> {}

impl<T> PromiseSendable<T> {
    fn inner(self) -> T {
        self.0
    }

    fn as_mut_ptr(&mut self) -> *mut T {
        &mut self.0
    }
}

fn setup() {
    static INITIALIZED: std::sync::Once = std::sync::Once::new();

    INITIALIZED.call_once(|| {
        init();
    });
}

fn identity_alice() -> Vec<u8> {
    include_bytes!("./resources/alice.key").to_vec()
}

fn identity_bob() -> Vec<u8> {
    // Peer id:
    include_bytes!("./resources/bob.key").to_vec()
}

const ALICE_LISTEN_ADDR: &CStr = c"/ip4/0.0.0.0/udp/10592/quic-v1";
const ALICE_PEER_ID: &CStr = c"12D3KooWAtTTz3ZUiWJR3jGNNmBvvQMTtD2VYbJqq9ekqL8GeM7M";
const ALICE_PEER_MULTIADDR: &CStr = c"/ip4/127.0.0.1/udp/10592/quic-v1/p2p/12D3KooWAtTTz3ZUiWJR3jGNNmBvvQMTtD2VYbJqq9ekqL8GeM7M";

// const BOB_LISTEN_ADDR: &CStr = c"/ip4/0.0.0.0/udp/10593/quic-v1";

fn echo_server_context() -> PromiseSendable<*mut NetworkContext> {
    let listen_addr = ALICE_LISTEN_ADDR;
    let mut context: *mut NetworkContext = std::ptr::null_mut();

    unsafe {
        create_context(
            identity_alice().as_ptr(),
            identity_alice().len() as u16,
            std::ptr::null(),
            listen_addr.as_ptr(),
            &mut context,
        );
    }

    PromiseSendable(context)
}

fn echo_server(context: PromiseSendable<*mut NetworkContext>) {
    let mut listener: *mut MirrorListener = std::ptr::null_mut();

    unsafe {
        assert_eq!(listen_mirror(context.inner(), 0, &mut listener), 0);
    };

    loop {
        let mut client: *mut MirrorClient = std::ptr::null_mut();
        let mut buffer = [0u8; 4];

        unsafe {
            assert_eq!(accept_mirror(listener, &mut client), 0);
        }

        loop {
            if unsafe { read_mirror_client(client, buffer.as_mut_ptr(), 0, buffer.len()) } < 0 {
                break;
            }

            if unsafe { write_mirror_client(client, buffer.as_mut_ptr(), 0, buffer.len()) } < 0 {
                break;
            };
        }

        unsafe {
            destroy_mirror_client(client);
        };
    }
}

#[test]
#[serial]
fn test_usecase_server() {
    setup();
    let mut context: *mut NetworkContext = std::ptr::null_mut();
    let mut listener: *mut MirrorListener = std::ptr::null_mut();

    unsafe {
        assert_eq!(
            create_context(
                identity_alice().as_ptr(),
                identity_alice().len() as u16,
                std::ptr::null(),
                std::ptr::null(),
                &mut context
            ),
            0
        );
    };

    assert!(!context.is_null());

    unsafe {
        assert_eq!(listen_mirror(context, 0, &mut listener), 0);
    };

    unsafe {
        destroy_mirror_listener(listener);
    }

    unsafe { destroy_context(context) };
}

#[test]
#[serial]
fn test_usecase_client() {
    let mut context: *mut NetworkContext = std::ptr::null_mut();

    unsafe {
        assert_eq!(
            create_context(
                identity_alice().as_ptr(),
                identity_alice().len() as u16,
                std::ptr::null(),
                std::ptr::null(),
                &mut context
            ),
            0
        );
    };

    assert!(!context.is_null());

    unsafe { destroy_context(context) };
}

#[test]
#[serial]
fn test_usecase_double_listen() {
    setup();
    let mut context: *mut NetworkContext = std::ptr::null_mut();
    let mut listener_1: *mut MirrorListener = std::ptr::null_mut();
    let mut listener_2: *mut MirrorListener = std::ptr::null_mut();

    unsafe {
        assert_eq!(
            create_context(
                identity_alice().as_ptr(),
                identity_alice().len() as u16,
                std::ptr::null(),
                std::ptr::null(),
                &mut context
            ),
            0
        );
    };

    assert!(!context.is_null());

    unsafe {
        assert_eq!(listen_mirror(context, 0, &mut listener_1), 0);
    };

    unsafe {
        assert_eq!(listen_mirror(context, 1, &mut listener_2), 0);
    }

    unsafe {
        destroy_mirror_listener(listener_1);
        destroy_mirror_listener(listener_2);
    }

    unsafe { destroy_context(context) };
}

#[serial]
fn stream_test() {
    setup();

    let echo_server_context = echo_server_context();

    std::thread::spawn(move || {echo_server(echo_server_context)});

    std::thread::sleep(std::time::Duration::from_secs(5));

    let mut context: *mut crate::NetworkContext = std::ptr::null_mut();
    let mut client: *mut crate::MirrorClient = std::ptr::null_mut();

    unsafe {
        assert_eq!(
            crate::create_context(
                identity_bob().as_ptr(),
                identity_bob().len() as u16,
                ALICE_PEER_MULTIADDR.as_ptr(),
                std::ptr::null(),
                &mut context
            ),
            0
        );
    };

    assert!(!context.is_null());

    unsafe {
        assert_eq!(
            connect_mirror(
                context,
                ALICE_PEER_ID.as_ptr(),
                0,
                &mut client,
            ),
            0
        );
    }

    let mut send_buffer = [0u8; 1];
    let mut recv_buffer = [0u8; 1];

    for counter in 0..100 {
        send_buffer[0] = counter as u8;

        let result = unsafe { write_mirror_client(client, send_buffer.as_mut_ptr(), 0, 4) };

        if result < 0 {
            println!("write_mirror_client failed, {}", result);
            break;
        }

        let result = unsafe { read_mirror_client(client, recv_buffer.as_mut_ptr(), 0, 4) };

        if result < 0 {
            println!("read_mirror_client failed, {}", result);
            break;
        }

        let received_counter = recv_buffer[0] as u32;

        assert_eq!(received_counter, counter);
    }

    println!("Test passed");

    unsafe {
        destroy_mirror_client(client);
        destroy_context(context);
        destroy_context(echo_server_context.inner());
    }
}

#[test]
#[serial]
fn stream_test_par() {
    setup();

    let echo_server_context = echo_server_context();

    std::thread::spawn(move || {echo_server(echo_server_context)});

    std::thread::sleep(std::time::Duration::from_secs(5));

    let mut context: *mut crate::NetworkContext = std::ptr::null_mut();
    let client: PromiseSendable<*mut crate::MirrorClient> = PromiseSendable(std::ptr::null_mut());

    unsafe {
        assert_eq!(
            crate::create_context(
                identity_bob().as_ptr(),
                identity_bob().len() as u16,
                ALICE_PEER_MULTIADDR.as_ptr(),
                std::ptr::null(),
                &mut context
            ),
            0
        );
    };

    assert!(!context.is_null());

    unsafe {
        assert_eq!(
            connect_mirror(
                context,
                ALICE_PEER_ID.as_ptr(),
                0,
                &mut client.inner(),
            ),
            0
        );
    }

    let mut send_buffer = [0u8; 1];
    let mut recv_buffer = [0u8; 1];

    let write_end = std::thread::spawn(move || {
        for counter in 0..100 {
            send_buffer[0] = counter as u8;

            let result =
                unsafe { write_mirror_client(client.inner(), send_buffer.as_mut_ptr(), 0, 4) };
            if result < 0 {
                println!("write_mirror_client failed, {}", result);
                break;
            }
        }
    });

    let read_end = std::thread::spawn(move || {
        for counter in 0..100 {
            let result =
                unsafe { read_mirror_client(client.inner(), recv_buffer.as_mut_ptr(), 0, 4) };

            if result < 0 {
                println!("read_mirror_client failed, {}", result);
                break;
            }

            let received_counter = recv_buffer[0] as u32;

            assert_eq!(received_counter, counter);

            println!("Received: {}", received_counter);
        }
    });

    write_end.join().unwrap();
    read_end.join().unwrap();

    unsafe {
        destroy_context(context);
        destroy_context(echo_server_context.inner());
    }
}

#[test]
#[serial]
fn test_destroy_context() {
    setup();

    let mut context: *mut crate::NetworkContext = std::ptr::null_mut();

    unsafe {
        assert_eq!(
            crate::create_context(
                identity_bob().as_ptr(),
                identity_bob().len() as u16,
                ALICE_PEER_MULTIADDR.as_ptr(),
                std::ptr::null(),
                &mut context
            ),
            0
        );
    };

    let mut listener = PromiseSendable(std::ptr::null_mut());

    unsafe {
        assert_eq!(crate::listen_mirror(context, 0, listener.as_mut_ptr()), 0);
    }

    let accept_thread = std::thread::spawn(move || {
        let mut client = std::ptr::null_mut();

        tracing::info!("listener.inner(): {:?}", listener.inner());

        unsafe {
            crate::accept_mirror(listener.inner(), &mut client)
        }
    });

    std::thread::sleep(std::time::Duration::from_secs(5));

    unsafe {
        assert_eq!(destroy_context(context), 0);
    }

    assert!(accept_thread.join().unwrap() < 0);
}
