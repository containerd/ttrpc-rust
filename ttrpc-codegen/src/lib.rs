//! Pure-Rust build-time code generation for ttrpc services.
//!
//! `ttrpc-codegen` parses `.proto` files and generates Protocol Buffers messages, typed ttrpc
//! clients, server traits, and registration helpers. It does not require a `protoc` executable.
//!
//! Add this crate under `[build-dependencies]` and invoke [`Codegen`] from `build.rs`.
//!
//! # Examples
//!
//! ```no_run
//! use ttrpc_codegen::{Codegen, Customize, ProtobufCustomize};
//!
//! # fn main() -> std::io::Result<()> {
//! Codegen::new()
//!     .out_dir(std::env::var("OUT_DIR").expect("OUT_DIR is set for build scripts"))
//!     .input("proto/greeter.proto")
//!     .include("proto")
//!     .rust_protobuf()
//!     .customize(Customize {
//!         async_all: true,
//!         gen_mod: true,
//!         ..Default::default()
//!     })
//!     .rust_protobuf_customize(ProtobufCustomize::default().gen_mod_rs(true))
//!     .run()?;
//! # Ok(())
//! # }
//! ```
//!
//! When no output directory is configured, [`Codegen::run`] uses Cargo's `OUT_DIR` environment
//! variable. Generated module declarations can then be included with:
//!
//! ```ignore
//! include!(concat!(env!("OUT_DIR"), "/mod.rs"));
//! ```

#![warn(missing_docs)]
#![warn(rustdoc::broken_intra_doc_links)]

pub use protobuf_codegen::{
    Customize as ProtobufCustomize, CustomizeCallback as ProtobufCustomizeCallback,
};
use std::io;
use std::path::Path;
use std::path::PathBuf;
pub use ttrpc_compiler::Customize;

/// Builder for pure-Rust Protocol Buffers and ttrpc code generation.
#[derive(Debug, Default)]
pub struct Codegen {
    /// Output directory, or `OUT_DIR` when unset.
    out_dir: Option<PathBuf>,
    /// Directories searched for imported `.proto` files.
    includes: Vec<PathBuf>,
    /// `.proto` files to compile.
    inputs: Vec<PathBuf>,
    /// Whether to generate `rust-protobuf` message files alongside service bindings.
    rust_protobuf: bool,
    /// Underlying `rust-protobuf` generator configuration.
    rust_protobuf_codegen: protobuf_codegen::Codegen,
    /// ttrpc service generator configuration.
    customize: Customize,
}

impl Codegen {
    /// Creates an empty code generation builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the output directory.
    pub fn out_dir(&mut self, out_dir: impl AsRef<Path>) -> &mut Self {
        self.out_dir = Some(out_dir.as_ref().to_owned());
        self
    }

    /// Adds a directory searched for imported `.proto` files.
    ///
    /// Canonical Google well-known type imports are resolved automatically and
    /// do not need to be present in an include directory.
    pub fn include(&mut self, include: impl AsRef<Path>) -> &mut Self {
        self.includes.push(include.as_ref().to_owned());
        self
    }

    /// Adds directories searched for imported `.proto` files.
    pub fn includes(&mut self, includes: impl IntoIterator<Item = impl AsRef<Path>>) -> &mut Self {
        for include in includes {
            self.include(include);
        }
        self
    }

    /// Adds a `.proto` file to compile.
    pub fn input(&mut self, input: impl AsRef<Path>) -> &mut Self {
        self.inputs.push(input.as_ref().to_owned());
        self
    }

    /// Adds `.proto` files to compile.
    pub fn inputs(&mut self, inputs: impl IntoIterator<Item = impl AsRef<Path>>) -> &mut Self {
        for input in inputs {
            self.input(input);
        }
        self
    }

    /// Enables generation of `rust-protobuf` message types alongside ttrpc services.
    pub fn rust_protobuf(&mut self) -> &mut Self {
        self.rust_protobuf = true;
        self
    }

    /// Sets the `rust-protobuf` code generation options.
    pub fn rust_protobuf_customize(&mut self, customize: ProtobufCustomize) -> &mut Self {
        self.rust_protobuf_codegen.customize(customize);
        self
    }

    /// Sets a callback for per-element `rust-protobuf` customization.
    pub fn rust_protobuf_customize_callback(
        &mut self,
        customize: impl ProtobufCustomizeCallback,
    ) -> &mut Self {
        self.rust_protobuf_codegen.customize_callback(customize);
        self
    }

    /// Sets ttrpc client and server generation options.
    pub fn customize(&mut self, customize: Customize) -> &mut Self {
        self.customize = customize;
        self
    }

    /// Parses the configured inputs and writes generated Rust files.
    ///
    /// This is equivalent to a `protoc`-based generation step but does not require `protoc` or
    /// `protoc-gen-rust` in `PATH`.
    ///
    /// # Errors
    ///
    /// Returns an error if an input cannot be read or parsed, imports or types cannot be resolved,
    /// or generated service files cannot be written.
    ///
    /// # Panics
    ///
    /// Panics if `rust_protobuf` generation is enabled and message generation fails.
    pub fn run(&mut self) -> io::Result<()> {
        let includes: Vec<&Path> = self.includes.iter().map(|p| p.as_path()).collect();
        let inputs: Vec<&Path> = self.inputs.iter().map(|p| p.as_path()).collect();
        let p = parse_and_typecheck(&includes, &inputs)?;
        // If out_dir is none ,dst_path will be setting in path_dir
        let dst_path = self.out_dir.clone().unwrap_or_else(|| {
            // Add default path from env OUT_DIR, if no OUT_DIR env ,that's will be current path
            std::env::var("OUT_DIR").map_or_else(
                |_| std::env::current_dir().unwrap_or_default(),
                PathBuf::from,
            )
        });

        if self.rust_protobuf {
            self.rust_protobuf_codegen
                .pure()
                .out_dir(&dst_path)
                .inputs(&self.inputs)
                .includes(&self.includes)
                .run()
                .expect("Gen rust protobuf failed.");
        }

        ttrpc_compiler::codegen::gen_and_write(
            p.file_descriptors.as_slice(),
            &p.relative_paths,
            &dst_path,
            &self.customize,
        )
    }
}

#[doc(hidden)]
pub struct ParsedAndTypechecked {
    pub relative_paths: Vec<String>,
    pub file_descriptors: Vec<protobuf::descriptor::FileDescriptorProto>,
}

#[doc(hidden)]
pub fn parse_and_typecheck(
    includes: &[&Path],
    input: &[&Path],
) -> io::Result<ParsedAndTypechecked> {
    let mut parser = protobuf_parse::Parser::new();
    parser
        .pure()
        .includes(includes.iter().copied())
        .inputs(input.iter().copied());

    let parsed = parser
        .parse_and_typecheck()
        .map_err(|error| io::Error::other(format!("{error:#}")))?;

    Ok(ParsedAndTypechecked {
        relative_paths: parsed
            .relative_paths
            .into_iter()
            .map(|path| path.to_string())
            .collect(),
        file_descriptors: parsed.file_descriptors,
    })
}
