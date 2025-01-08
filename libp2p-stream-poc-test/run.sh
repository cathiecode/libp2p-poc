#!/usr/bin/bash

rm -r artifact
mkdir -p artifact

cd ../libp2p-stream-poc
cargo build
cd -

cp ../target/debug/liblibp2p_stream_poc.so artifact
cp ../libp2p-stream-poc/bindings/c/libp2p_stream_poc.h artifact

gcc -o artifact/test test.c -lstdc++ -llibp2p_stream_poc -Lartifact

LD_LIBRARY_PATH=$LD_LIBRARY_PATH:$(realpath artifact) valgrind --leak-check=full --show-leak-kinds=all --track-origins=yes -s ./artifact/test
