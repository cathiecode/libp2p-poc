use criterion::{black_box, criterion_group, criterion_main, Criterion};
use futures::{AsyncReadExt, AsyncWriteExt};
use mirrorp2p::*;

fn identity_alice() -> Vec<u8> {
    include_bytes!("../tests/resources/alice.key").to_vec()
}

fn identity_bob() -> Vec<u8> {
    include_bytes!("../tests/resources/bob.key").to_vec()
}

#[derive(Clone)]
struct PromiseSendable<T>(T);

unsafe impl<T> std::marker::Send for PromiseSendable<T> {}
unsafe impl<T> std::marker::Sync for PromiseSendable<T> {}

fn counter_server() {
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
        assert_eq!(listen_mirror(context, &mut listener), 0);
    };

    loop {
        let mut client: PromiseSendable<*mut MirrorClient> = PromiseSendable(std::ptr::null_mut());
        let mut buffer = [0u8; 4];

        unsafe {
            assert_eq!(accept_mirror(listener, &mut client.0), 0);
        }

        std::thread::spawn(move || {
            let client = client;

            unsafe {
                loop {
                    if read_mirror_client(client.0, buffer.as_mut_ptr(), 0, buffer.len()) < 0 {
                        break;
                    }

                    if write_mirror_client(client.0, buffer.as_mut_ptr(), 0, buffer.len()) < 0 {
                        break;
                    }
                }

                destroy_mirror_client(client.0);
            }
        });
    }
}

fn bench_counter(c: &mut Criterion) {
    // assert_eq!(init(), 0);

    let (sender, receiver) = std::sync::mpsc::channel::<()>();

    std::thread::spawn(move || {
        counter_server();

        receiver.recv().unwrap();

        println!("Server exited");
    });

    let mut context: *mut crate::NetworkContext = std::ptr::null_mut();
    let mut client: PromiseSendable<*mut crate::MirrorClient> =
        PromiseSendable(std::ptr::null_mut());

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
                &mut client.0,
            ),
            0
        );
    }

    c.bench_function("echo(seq) 1000", |b| {
        b.iter(|| unsafe {
            let mut send_buffer = [0u8; 4];
            let mut recv_buffer = [0u8; 4];

            for counter in 0..1000 {
                send_buffer[0] = (counter >> 24) as u8;
                send_buffer[1] = (counter >> 16) as u8;
                send_buffer[2] = (counter >> 8) as u8;
                send_buffer[3] = counter as u8;

                let result = write_mirror_client(client.0, send_buffer.as_mut_ptr(), 0, 4);
                if result < 0 {
                    panic!("write_mirror_client failed, {}", result);
                }

                let result = read_mirror_client(client.0, recv_buffer.as_mut_ptr(), 0, 4);

                if result < 0 {
                    panic!("read_mirror_client failed, {}", result);
                }

                let mut received_counter = 0;

                received_counter |= recv_buffer[0] as i32;
                received_counter <<= 8;
                received_counter |= recv_buffer[1] as i32;
                received_counter <<= 8;
                received_counter |= recv_buffer[2] as i32;
                received_counter <<= 8;
                received_counter |= recv_buffer[3] as i32;

                assert_eq!(received_counter, counter);
            }
        });
    });

    c.bench_function("echo par", |b| {
        b.iter(|| unsafe {
            let mut send_buffer = [0u8; 4];
            let mut recv_buffer = [0u8; 4];

            for _ in 0..10 {
                for counter in 0..100 {
                    send_buffer[0] = (counter >> 24) as u8;
                    send_buffer[1] = (counter >> 16) as u8;
                    send_buffer[2] = (counter >> 8) as u8;
                    send_buffer[3] = counter as u8;

                    let result = write_mirror_client(client.0, send_buffer.as_mut_ptr(), 0, 4);
                    if result < 0 {
                        panic!("write_mirror_client failed, {}", result);
                    }
                }

                for counter in 0..100 {
                    let result = read_mirror_client(client.0, recv_buffer.as_mut_ptr(), 0, 4);

                    if result < 0 {
                        panic!("read_mirror_client failed, {}", result);
                    }

                    let mut received_counter = 0;

                    received_counter |= recv_buffer[0] as i32;
                    received_counter <<= 8;
                    received_counter |= recv_buffer[1] as i32;
                    received_counter <<= 8;
                    received_counter |= recv_buffer[2] as i32;
                    received_counter <<= 8;
                    received_counter |= recv_buffer[3] as i32;

                    assert_eq!(received_counter, counter);
                }
            }
        });
    });

    c.bench_function("echo threaded", |b| {
        b.iter(|| unsafe {
            let client_threaded_write = client.clone();
            let client_threaded_read = client.clone();

            let write_thread = std::thread::spawn(move || {
                let mut send_buffer = [0u8; 4];
                let client = client_threaded_write;
                for counter in 0..1000 {
                    send_buffer[0] = (counter >> 24) as u8;
                    send_buffer[1] = (counter >> 16) as u8;
                    send_buffer[2] = (counter >> 8) as u8;
                    send_buffer[3] = counter as u8;

                    let result = write_mirror_client(client.0, send_buffer.as_mut_ptr(), 0, 4);

                    if result < 0 {
                        panic!("write_mirror_client failed, {}", result);
                    }
                }
            });

            let read_thread = std::thread::spawn(move || {
                let mut recv_buffer = [0u8; 4];
                let client = client_threaded_read;

                for counter in 0..1000 {
                    let result = read_mirror_client(client.0, recv_buffer.as_mut_ptr(), 0, 4);

                    if result < 0 {
                        panic!("read_mirror_client failed, {}", result);
                    }

                    let mut received_counter = 0;

                    received_counter |= recv_buffer[0] as i32;
                    received_counter <<= 8;
                    received_counter |= recv_buffer[1] as i32;
                    received_counter <<= 8;
                    received_counter |= recv_buffer[2] as i32;
                    received_counter <<= 8;
                    received_counter |= recv_buffer[3] as i32;

                    assert_eq!(received_counter, counter);
                }
            });

            read_thread.join().unwrap();
            write_thread.join().unwrap();
        });
    });
}

criterion_group!(benches, bench_counter);
criterion_main!(benches);
