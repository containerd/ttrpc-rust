// Copyright (c) 2020 Ant Financial
//
// SPDX-License-Identifier: Apache-2.0
//
#[cfg(unix)]
pub mod asynchronous;
#[cfg(unix)]
pub use asynchronous as r#async;
pub mod sync;
