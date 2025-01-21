fn main() {
    let path = std::env::args().nth(1).expect("Usage: generate-key <path>");

    let keypair = libp2p::identity::ed25519::Keypair::generate();

    let mut result = keypair.to_bytes();

    std::fs::write(path, result).unwrap();

    result.iter_mut().for_each(|x| *x = 0);
}
