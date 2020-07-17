// Copyright 2017 PingCAP, Inc.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// See the License for the specific language governing permissions and
// limitations under the License.

//!
//! A compiler of ttrpc-rust.
//!
//! *generate rust version ttrpc codes from proto files.*
//!
//!
//! Usage
//!
//!- [Manual Generation](https://github.com/containerd/ttrpc-rust#1-generate-with-protoc-command) uses ttrpc-compiler as a protoc plugin
//!
//!- [Programmatic Generation](https://github.com/containerd/ttrpc-rust#2-generate-programmatically) uses ttrpc-compiler as a rust crate

pub mod codegen;
pub mod prost_codegen;
mod util;

#[derive(Default, Debug, Clone)]
pub struct Customize {
    pub async_all: bool,
    pub async_client: bool,
    pub async_server: bool,
}
