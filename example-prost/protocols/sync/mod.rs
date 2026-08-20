// Copyright (c) 2020 Ant Financial
//
// SPDX-License-Identifier: Apache-2.0
//

#![allow(dead_code, unused_imports, unused_qualifications)]

include!("_include.rs");

// Module glue: the shared schemas declare `package grpc` for the health,
// agent, and oci definitions, so prost emits them into a single `grpc`
// module. Re-export them through the module names used by the example
// sources instead of changing the protobuf packages.
pub mod agent {
    pub use super::grpc::*;
}
pub mod health {
    pub use super::grpc::*;
}
