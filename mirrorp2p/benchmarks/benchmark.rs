#![feature(test)]
use crate::*;
use criterion::{black_box, Criterion, criterion_group, criterion_main};

fn counter_server() {
    let listen_addr = c"/ip4/0.0.0.0/udp/10592/quic-v1";

    let mut context: *mut NetworkContext = std::ptr::null_mut();
    
    unsafe {
        create_context(identity_alice().as_ptr(), identity_alice().len() as u16, std::ptr::null(), listen_addr, &mut context);
    };

    unsafe {
        assert_eq!(listen_mirror(context), 0);
    };

    unsafe { destroy_context(context) };
}

fn bench_counter() {
    
}

criterion_group!(benches, benchmark_sample_function);
criterion_main!(benches);
