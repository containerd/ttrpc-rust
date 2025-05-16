//! This module contains functionalities that where previously available in
//! the protobuf / protobuf-codegen crates, but were then removed.
//! The missing functionalities have been reimplemented in this module.

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
