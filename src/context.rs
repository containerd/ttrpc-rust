// Copyright (c) 2021 Ant group
//
// SPDX-License-Identifier: Apache-2.0
//

//! Per-request timeout and metadata.
//!
//! A [`Context`](crate::context::Context) is passed to methods on generated clients. A timeout of
//! zero means that the call has no client-side deadline. Metadata supports multiple values per key.

use crate::proto::KeyValue;
use core::time::Duration;
use std::collections::HashMap;

/// Configuration carried with a single RPC request.
///
/// Use [`with_duration`] to set a timeout and [`Context::add`] or [`Context::set`] to attach
/// metadata. Metadata keys should be lowercase for consistent lookup behavior.
///
/// # Examples
///
/// ```
/// use std::time::Duration;
/// use ttrpc::context;
///
/// let mut ctx = context::with_duration(Duration::from_secs(1));
/// ctx.add("trace-id".into(), "abc123".into());
/// ctx.add("trace-id".into(), "def456".into());
///
/// assert_eq!(ctx.metadata["trace-id"], ["abc123", "def456"]);
/// ```
#[derive(Clone, Default, Debug)]
pub struct Context {
    /// Metadata values grouped by key.
    pub metadata: HashMap<String, Vec<String>>,
    /// Request timeout in nanoseconds, or zero for no timeout.
    pub timeout_nano: i64,
}

/// Creates a context with a timeout expressed in nanoseconds.
///
/// A value of zero disables the client-side timeout. New code should generally prefer
/// [`with_duration`], which makes the unit explicit.
pub fn with_timeout(i: i64) -> Context {
    Context {
        timeout_nano: i,
        ..Default::default()
    }
}
/// Creates a context with the specified request timeout.
///
/// A zero duration disables the client-side timeout.
pub fn with_duration(du: Duration) -> Context {
    with_timeout(du.as_nanos() as i64)
}

/// Creates a context containing the supplied metadata and no timeout.
///
/// The map is stored as provided; callers should use lowercase keys for consistency with
/// [`Context::add`] and [`Context::set`].
pub fn with_metadata(md: HashMap<String, Vec<String>>) -> Context {
    Context {
        metadata: md,
        ..Default::default()
    }
}

impl Context {
    /// Appends a metadata value to `key`.
    ///
    /// New keys are normalized to lowercase. Use lowercase keys when appending to an existing
    /// entry.
    pub fn add(&mut self, key: String, value: String) {
        if let Some(ref mut vl) = self.metadata.get_mut(&key) {
            vl.push(value);
        } else {
            self.metadata.insert(key.to_lowercase(), vec![value]);
        }
    }

    /// Replaces all metadata values for `key`.
    ///
    /// Non-empty keys are normalized to lowercase. Passing an empty vector removes `key` exactly
    /// as provided, so use its normalized lowercase form.
    pub fn set(&mut self, key: String, value: Vec<String>) {
        if value.is_empty() {
            self.metadata.remove(&key);
        } else {
            self.metadata.insert(key.to_lowercase(), value);
        }
    }
}

/// Converts Protocol Buffers key-value entries into a metadata map.
///
/// Repeated keys are preserved as multiple values in insertion order.
pub fn from_pb(kvs: &Vec<KeyValue>) -> HashMap<String, Vec<String>> {
    let mut meta: HashMap<String, Vec<String>> = HashMap::new();
    for kv in kvs {
        if let Some(ref mut vl) = meta.get_mut(&kv.key) {
            vl.push(kv.value.clone());
        } else {
            meta.insert(kv.key.clone(), vec![kv.value.clone()]);
        }
    }
    meta
}

/// Converts a metadata map into Protocol Buffers key-value entries.
///
/// Each value becomes a separate entry. The order of keys is unspecified because [`HashMap`]
/// does not preserve insertion order.
pub fn to_pb(kvs: HashMap<String, Vec<String>>) -> Vec<KeyValue> {
    let mut meta = Vec::with_capacity(kvs.len());

    for (k, vl) in kvs {
        for v in vl {
            #[cfg(not(feature = "prost"))]
            let key = KeyValue {
                key: k.clone(),
                value: v.clone(),
                ..Default::default()
            };
            #[cfg(feature = "prost")]
            let key = KeyValue {
                key: k.clone(),
                value: v.clone(),
            };
            meta.push(key);
        }
    }

    meta
}

#[cfg(test)]
mod tests {
    use crate::context;
    use crate::proto::KeyValue;

    #[test]
    fn test_metadata() {
        // RepeatedField -> HashMap, test from_pb()
        let mut src = Vec::new();
        for i in &[
            ("key1", "value1-1"),
            ("key1", "value1-2"),
            ("key2", "value2"),
        ] {
            #[cfg(not(feature = "prost"))]
            let key = KeyValue {
                key: i.0.to_string(),
                value: i.1.to_string(),
                ..Default::default()
            };
            #[cfg(feature = "prost")]
            let key = KeyValue {
                key: i.0.to_string(),
                value: i.1.to_string(),
            };
            src.push(key);
        }

        let dst = context::from_pb(&src);
        assert_eq!(dst.len(), 2);

        assert_eq!(
            dst.get("key1"),
            Some(&vec!["value1-1".to_string(), "value1-2".to_string()])
        );
        assert_eq!(dst.get("key2"), Some(&vec!["value2".to_string()]));
        assert_eq!(dst.get("key3"), None);

        // HashMap -> RepeatedField , test to_pb()
        let mut kvs = context::to_pb(dst);
        kvs.sort_by(|a, b| a.key.partial_cmp(&b.key).unwrap());

        assert_eq!(kvs.len(), 3);

        assert_eq!(kvs[0].key, "key1");
        assert_eq!(kvs[0].value, "value1-1");

        assert_eq!(kvs[1].key, "key1");
        assert_eq!(kvs[1].value, "value1-2");

        assert_eq!(kvs[2].key, "key2");
        assert_eq!(kvs[2].value, "value2");
    }

    #[test]
    fn test_context() {
        let ctx: context::Context = Default::default();
        assert_eq!(0, ctx.timeout_nano);
        assert_eq!(ctx.metadata.len(), 0);

        let mut ctx = context::with_duration(core::time::Duration::from_nanos(99));
        assert_eq!(99, ctx.timeout_nano);
        assert_eq!(ctx.metadata.len(), 0);

        ctx.add("key1".to_string(), "value1-1".to_string());
        assert_eq!(ctx.metadata.len(), 1);
        assert_eq!(
            ctx.metadata.get("key1"),
            Some(&vec!["value1-1".to_string()])
        );

        ctx.add("key1".to_string(), "value1-2".to_string());
        assert_eq!(ctx.metadata.len(), 1);
        assert_eq!(
            ctx.metadata.get("key1"),
            Some(&vec!["value1-1".to_string(), "value1-2".to_string()])
        );

        ctx.set("key2".to_string(), vec!["value2".to_string()]);
        assert_eq!(ctx.metadata.len(), 2);
        assert_eq!(ctx.metadata.get("key2"), Some(&vec!["value2".to_string()]));

        ctx.set("key1".to_string(), vec![]);
        assert_eq!(ctx.metadata.len(), 1);
        assert_eq!(ctx.metadata.get("key1"), None);
    }
}
