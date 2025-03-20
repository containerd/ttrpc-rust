//! This module contains functionalities that where previously available in
//! the protobuf / protobuf-codegen crates, but were then removed.
//! The missing functionalities have been reimplemented in this module.

use protobuf::descriptor::{DescriptorProto, FileDescriptorProto};
use protobuf_codegen::proto_name_to_rs;

// proto_name_to_rs is constructor as "{proto_path_to_rust_mod}.rs"
// see https://github.com/stepancheg/rust-protobuf/blob/v3.7.2/protobuf-codegen/src/gen/paths.rs#L43
pub fn proto_path_to_rust_mod(path: &str) -> String {
    proto_name_to_rs(path)
        .strip_suffix(".rs")
        .unwrap()
        .to_string()
}

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

// adapted from https://github.com/stepancheg/rust-protobuf/blob/v3.7.2/protobuf-codegen/src/gen/code_writer.rs#L12
#[derive(Default)]
pub struct CodeWriter {
    writer: String,
    indent: String,
}

impl CodeWriter {
    pub fn new() -> CodeWriter {
        Self::default()
    }

    pub fn code(&self) -> &str {
        &self.writer
    }

    pub fn take_code(&mut self) -> String {
        std::mem::take(&mut self.writer)
    }

    pub fn write_line(&mut self, line: impl AsRef<str>) {
        if line.as_ref().is_empty() {
            self.writer.push('\n');
        } else {
            self.writer.push_str(&self.indent);
            self.writer.push_str(line.as_ref());
            self.writer.push('\n');
        }
    }

    pub fn block(
        &mut self,
        first_line: impl AsRef<str>,
        last_line: impl AsRef<str>,
        cb: impl FnOnce(&mut CodeWriter),
    ) {
        self.write_line(first_line);
        self.indented(cb);
        self.write_line(last_line);
    }

    pub fn expr_block(&mut self, prefix: impl AsRef<str>, cb: impl FnOnce(&mut CodeWriter)) {
        self.block(format!("{} {{", prefix.as_ref()), "}", cb);
    }

    pub fn indented(&mut self, cb: impl FnOnce(&mut CodeWriter)) {
        self.indent.push_str("    ");
        cb(self);
        self.indent.truncate(self.indent.len() - 4);
    }

    pub fn pub_fn(&mut self, sig: impl AsRef<str>, cb: impl FnOnce(&mut CodeWriter)) {
        self.expr_block(format!("pub fn {}", sig.as_ref()), cb)
    }

    pub fn def_fn(&mut self, sig: impl AsRef<str>, cb: impl FnOnce(&mut CodeWriter)) {
        self.expr_block(format!("fn {}", sig.as_ref()), cb)
    }

    pub fn pub_struct(&mut self, name: impl AsRef<str>, cb: impl FnOnce(&mut CodeWriter)) {
        self.expr_block(format!("pub struct {}", name.as_ref()), cb);
    }

    pub fn field_decl(&mut self, name: impl AsRef<str>, field_type: impl AsRef<str>) {
        self.write_line(format!("{}: {},", name.as_ref(), field_type.as_ref()));
    }

    pub fn impl_self_block(&mut self, name: impl AsRef<str>, cb: impl FnOnce(&mut CodeWriter)) {
        self.expr_block(format!("impl {}", name.as_ref()), cb);
    }

    pub fn pub_trait(&mut self, name: impl AsRef<str>, cb: impl FnOnce(&mut CodeWriter)) {
        self.expr_block(format!("pub trait {}", name.as_ref()), cb);
    }
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
            proto_path_to_rust_mod(self.fd.name()),
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
