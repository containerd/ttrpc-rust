#!/bin/bash

PB_REL="https://github.com/protocolbuffers/protobuf/releases"
VERSION="22.0"

mkdir -p $HOME/protoc

if [[ "$OSTYPE" == "linux-gnu"* ]]; then
    ARCH="linux-x86_64"
elif [[ "$OSTYPE" == "darwin"* ]]; then
    ARCH="osx-universal_binary"
fi

curl -LO $PB_REL/download/v$VERSION/protoc-$VERSION-$ARCH.zip
unzip protoc-$VERSION-$ARCH.zip -d $HOME/protoc
rm -rf protoc-$VERSION-$ARCH.zip