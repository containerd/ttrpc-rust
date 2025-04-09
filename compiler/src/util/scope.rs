//! This module contains functionalities that where previously available in
//! the protobuf / protobuf-codegen crates, but were then removed.
//! The missing functionalities have been reimplemented in this module.

use protobuf::descriptor::{DescriptorProto, FileDescriptorProto};

// vendered from https://github.com/stepancheg/rust-protobuf/blob/v3.7.2/protobuf-codegen/src/gen/rust/keywords.rs
fn is_rust_keyword(ident: &str) -> bool {
    #[rustfmt::skip]
    static RUST_KEYWORDS: &[&str] = &[
        "_",
        "as",
        "async",
        "await",
        "break",
        "crate",
        "dyn",
        "else",
        "enum",
        "extern",
        "false",
        "fn",
        "for",
        "if",
        "impl",
        "in",
        "let",
        "loop",
        "match",
        "mod",
        "move",
        "mut",
        "pub",
        "ref",
        "return",
        "static",
        "self",
        "Self",
        "struct",
        "super",
        "true",
        "trait",
        "type",
        "unsafe",
        "use",
        "while",
        "continue",
        "box",
        "const",
        "where",
        "virtual",
        "proc",
        "alignof",
        "become",
        "offsetof",
        "priv",
        "pure",
        "sizeof",
        "typeof",
        "unsized",
        "yield",
        "do",
        "abstract",
        "final",
        "override",
        "macro",
    ];
    RUST_KEYWORDS.contains(&ident)
}

// reimplementation based on https://github.com/stepancheg/rust-protobuf/blob/v3.7.2/protobuf-codegen/src/gen/scope.rs#L26
// it only implements the `find_message` method with not extra dependencies
pub struct RootScope<'a> {
    pub file_descriptors: &'a [FileDescriptorProto],
}

// re-implementation of https://github.com/stepancheg/rust-protobuf/blob/v3.7.2/protobuf-codegen/src/gen/scope.rs#L340
// also based on https://github.com/stepancheg/rust-protobuf/blob/v3.7.2/protobuf-codegen/src/gen/scope.rs#L156
pub struct ScopedMessage<'a> {
    pub fd: &'a FileDescriptorProto,
    pub path: Vec<&'a DescriptorProto>,
    pub msg: &'a DescriptorProto,
}

impl ScopedMessage<'_> {
    pub fn prefix(&self) -> String {
        let mut prefix = String::new();
        for m in &self.path {
            prefix.push_str(m.name());
            prefix.push('.');
        }
        prefix
    }

    // rust type name prefix for this scope
    pub fn rust_prefix(&self) -> String {
        self.prefix().replace(".", "_")
    }

    // rust type name of this descriptor
    pub fn rust_name(&self) -> String {
        let mut r = self.rust_prefix();
        // Only escape if prefix is not empty
        if r.is_empty() && is_rust_keyword(self.msg.name()) {
            r.push_str("message_");
        }
        r.push_str(self.msg.name());
        r
    }

    // fully-qualified name of this type
    pub fn rust_fq_name(&self) -> String {
        format!(
            "{}::{}",
            super::proto_path_to_rust_mod(self.fd.name()),
            self.rust_name()
        )
    }
}

impl<'a> RootScope<'a> {
    pub fn find_message(&'a self, fqn: impl AsRef<str>) -> ScopedMessage<'a> {
        let Some(fqn1) = fqn.as_ref().strip_prefix(".") else {
            panic!("name must start with dot: {}", fqn.as_ref())
        };
        for fd in self.file_descriptors {
            let mut fqn2 = match fqn1.strip_prefix(fd.package()) {
                Some(rest) if fd.package().is_empty() => rest,
                Some(rest) if rest.starts_with(".") => &rest[1..],
                _ => continue,
            };

            assert!(!fqn2.starts_with("."));

            let mut pending = Some(fd.message_type.as_slice());
            let mut path = vec![];
            while let Some(msgs) = pending.take() {
                for msg in msgs {
                    fqn2 = match fqn2.strip_prefix(msg.name()) {
                        Some("") => return ScopedMessage { msg, path, fd },
                        Some(rest) if rest.starts_with(".") => &rest[1..],
                        _ => continue,
                    };
                    path.push(msg);
                    pending = Some(&msg.nested_type);
                    break;
                }
            }
        }
        panic!("enum not found by name: {}", fqn.as_ref())
    }
}
