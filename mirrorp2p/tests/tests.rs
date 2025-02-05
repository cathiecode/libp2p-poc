use mirrorp2p::*;

#[derive(Clone)]
struct PromiseSendable<T>(T);

unsafe impl<T> std::marker::Send for PromiseSendable<T> {}
unsafe impl<T> std::marker::Sync for PromiseSendable<T> {}


fn setup() {
    static INITIALIZED: std::sync::Once = std::sync::Once::new();

    INITIALIZED.call_once(|| {
        init();
    });
}

fn identity_alice() -> Vec<u8> {
    // Peer id: 12D3KooWAtTTz3ZUiWJR3jGNNmBvvQMTtD2VYbJqq9ekqL8GeM7M
    include_bytes!("./resources/alice.key").to_vec()
}

fn identity_bob() -> Vec<u8> {
    // Peer id:
    include_bytes!("./resources/bob.key").to_vec()
}

fn echo_server() {
    let listen_addr = c"/ip4/0.0.0.0/udp/10592/quic-v1";

    let mut context: *mut NetworkContext = std::ptr::null_mut();
    let mut listener: *mut MirrorListener = std::ptr::null_mut();

    unsafe {
        create_context(
            identity_alice().as_ptr(),
            identity_alice().len() as u16,
            std::ptr::null(),
            listen_addr.as_ptr(),
            &mut context,
        );
    };

    unsafe {
        assert_eq!(listen_mirror(context, 0, &mut listener), 0);
    };

    loop {
        let mut client: *mut MirrorClient = std::ptr::null_mut();
        let mut buffer = [0u8; 4];

        unsafe {
            assert_eq!(accept_mirror(listener, &mut client), 0);
        }

        unsafe {
            loop {
                if read_mirror_client(client, buffer.as_mut_ptr(), 0, buffer.len()) < 0 {
                    break;
                }

                if write_mirror_client(client, buffer.as_mut_ptr(), 0, buffer.len()) < 0 {
                    break;
                };
            }

            destroy_mirror_client(client);
        }
    }
}

#[test]
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
        assert_eq!(listen_mirror(context, 0, &mut listener_2), 0);
    }

    unsafe {
        destroy_mirror_listener(listener_1);
        destroy_mirror_listener(listener_2);
    }

    unsafe { destroy_context(context) };
}

#[test]
fn stream_test() {
    setup();

    std::thread::spawn(|| {
        echo_server();
    });

    std::thread::sleep(std::time::Duration::from_secs(5));

    let mut context: *mut crate::NetworkContext = std::ptr::null_mut();
    let mut client: *mut crate::MirrorClient = std::ptr::null_mut();

    unsafe {
        assert_eq!(
            crate::create_context(
                identity_bob().as_ptr(),
                identity_bob().len() as u16,
                c"/ip4/127.0.0.1/udp/10592/quic-v1/p2p/12D3KooWAtTTz3ZUiWJR3jGNNmBvvQMTtD2VYbJqq9ekqL8GeM7M".as_ptr(),
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
                c"12D3KooWAtTTz3ZUiWJR3jGNNmBvvQMTtD2VYbJqq9ekqL8GeM7M".as_ptr(),
                0,
                &mut client,
            ),
            0
        );

        let mut send_buffer = [0u8; 1];
        let mut recv_buffer = [0u8; 1];

        for counter in 0..100 {
            send_buffer[0] = counter as u8;

            let result = write_mirror_client(client, send_buffer.as_mut_ptr(), 0, 4);
            if result < 0 {
                println!("write_mirror_client failed, {}", result);
                break;
            }

            let result = read_mirror_client(client, recv_buffer.as_mut_ptr(), 0, 4);

            if result < 0 {
                println!("read_mirror_client failed, {}", result);
                break;
            }

            let received_counter = recv_buffer[0] as u32;

            assert_eq!(received_counter, counter);
        }
    }
}
#[test]
fn stream_test_par() {
    setup();

    std::thread::spawn(|| {
        echo_server();
    });

    std::thread::sleep(std::time::Duration::from_secs(5));

    let mut context: *mut crate::NetworkContext = std::ptr::null_mut();
    let mut client: PromiseSendable<*mut crate::MirrorClient> = PromiseSendable(std::ptr::null_mut());

    unsafe {
        assert_eq!(
            crate::create_context(
                identity_bob().as_ptr(),
                identity_bob().len() as u16,
                c"/ip4/127.0.0.1/udp/10592/quic-v1/p2p/12D3KooWAtTTz3ZUiWJR3jGNNmBvvQMTtD2VYbJqq9ekqL8GeM7M".as_ptr(),
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
                c"12D3KooWAtTTz3ZUiWJR3jGNNmBvvQMTtD2VYbJqq9ekqL8GeM7M".as_ptr(),
                0,
                &mut client.0,
            ),
            0
        );

        let mut send_buffer = [0u8; 1];
        let mut recv_buffer = [0u8; 1];

        let client_write = client.clone();
        let client_read = client.clone();

        std::thread::spawn(move || {
            let client = client_write;

            for counter in 0..100 {
                send_buffer[0] = counter as u8;

                let result = write_mirror_client(client.0, send_buffer.as_mut_ptr(), 0, 4);
                if result < 0 {
                    println!("write_mirror_client failed, {}", result);
                    break;
                }
            }
        });

        std::thread::spawn(move || {
            let client = client_read;

            for counter in 0..100 {
                let result = read_mirror_client(client.0, recv_buffer.as_mut_ptr(), 0, 4);
    
                if result < 0 {
                    println!("read_mirror_client failed, {}", result);
                    break;
                }
    
                let received_counter = recv_buffer[0] as u32;
    
                assert_eq!(received_counter, counter);

                println!("Received: {}", received_counter);
            }    
        });
    }
}
