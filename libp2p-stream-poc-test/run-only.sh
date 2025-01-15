#!/usr/bin/sh

RUNNER="valgrind --leak-check=full --show-leak-kinds=all --track-origins=yes -s"
RUNNER=

LD_LIBRARY_PATH=$LD_LIBRARY_PATH:$(realpath artifact) sh -c "$RUNNER ./artifact/test $1 $2"
