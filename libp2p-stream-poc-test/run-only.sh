#!/usr/bin/sh

LD_LIBRARY_PATH=$LD_LIBRARY_PATH:$(realpath artifact) ./artifact/test $1