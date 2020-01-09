#!/bin/bash

# Copyright (c) 2019 Ant Financial
#
# Licensed under the Apache License, Version 2.0 (the "License");
# you may not use this file except in compliance with the License.
# You may obtain a copy of the License at
#
#     http://www.apache.org/licenses/LICENSE-2.0
#
# Unless required by applicable law or agreed to in writing, software
# distributed under the License is distributed on an "AS IS" BASIS,
# WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
# See the License for the specific language governing permissions and
# limitations under the License.

die() {
    echo $1
    exit
}

get_source_version() {
    if [ ! -d $GOPATH/src/$1 ]; then
        go get -d -v $1
    fi
    [ $? -eq 0 ] || die "Failed to get $1"
    if [ "$2" != "" ] ; then
	    pushd "${GOPATH}/src/$1"
        if [ $(git rev-parse HEAD) != $2 ] ; then
            git checkout $2
            [ $? -eq 0 ] || die "Failed to get $1 $2"
        fi
        popd
    fi
}

get_rs() {
    local cmd="protoc --rust_out=./ --ttrpc_out=./,plugins=ttrpc:./ --plugin=protoc-gen-ttrpc=`which ttrpc_rust_plugin` -I ./protos/ ./protos/$1"
    echo $cmd
    $cmd
    [ $? -eq 0 ] || die "Failed to get rust from $1"
}

if [ "$(basename $(pwd))" != "protocols" ] || [ ! -d "./hack/" ]; then
	die "Please go to directory of protocols before execute this shell"
fi
which protoc
[ $? -eq 0 ] || die "Please install protoc from github.com/protocolbuffers/protobuf"
which protoc-gen-rust
[ $? -eq 0 ] || die "Please install protobuf-codegen from github.com/pingcap/grpc-rs"
which ttrpc_rust_plugin
[ $? -eq 0 ] || die "Please install ttrpc_rust_plugin from ttrpc-rust/compiler"

if [ $UPDATE_PROTOS ]; then
    if [ ! $GOPATH ]; then
        die 'Need $GOPATH to get the proto files'
    fi

    get_source_version "github.com/kata-containers/agent" ""
    cp $GOPATH/src/github.com/kata-containers/agent/protocols/grpc/agent.proto ./protos/
    cp $GOPATH/src/github.com/kata-containers/agent/protocols/grpc/oci.proto ./protos/
    cp $GOPATH/src/github.com/kata-containers/agent/protocols/grpc/health.proto ./protos/
    mkdir -p ./protos/github.com/kata-containers/agent/pkg/types/
    cp $GOPATH/src/github.com/kata-containers/agent/pkg/types/types.proto ./protos/github.com/kata-containers/agent/pkg/types/

    # The version is get from https://github.com/kata-containers/agent/blob/master/Gopkg.toml
    get_source_version "github.com/gogo/protobuf" "4cbf7e384e768b4e01799441fdf2a706a5635ae7"
    mkdir -p ./protos/github.com/gogo/protobuf/gogoproto/
    cp $GOPATH/src/github.com/gogo/protobuf/gogoproto/gogo.proto ./protos/github.com/gogo/protobuf/gogoproto/
    mkdir -p ./protos/google/protobuf/
    cp $GOPATH/src/github.com/gogo/protobuf/protobuf/google/protobuf/empty.proto ./protos/google/protobuf/
fi

get_rs agent.proto
get_rs health.proto
get_rs github.com/kata-containers/agent/pkg/types/types.proto
get_rs google/protobuf/empty.proto

get_rs oci.proto
# Need change Box<Self> to ::std::boxed::Box<Self> because there is another struct Box
sed 's/self: Box<Self>/self: ::std::boxed::Box<Self>/g' oci.rs > new_oci.rs
mv new_oci.rs oci.rs
