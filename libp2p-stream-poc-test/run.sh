#!/usr/bin/bash

#RUNNER="valgrind --leak-check=full --show-leak-kinds=all --track-origins=yes -s"
RUNNER=""

rm -r artifact
mkdir -p artifact

cd ../libp2p-stream-poc
cargo build
cd -

cp ../target/debug/liblibp2p_stream_poc.so artifact
cp ../libp2p-stream-poc/bindings/c/libp2p_stream_poc.h artifact

gcc -O0 -g -Wall -o ./artifact/test test.c -lstdc++ -llibp2p_stream_poc -lpthread -Lartifact

sh -c "$RUNNER ./artifact/test $1 $2"
