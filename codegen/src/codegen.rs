use prost::Message;
use prost_build::Config;
use prost_types::FileDescriptorSet;
use std::fs::{self, File};
use std::io::{self, BufReader, Read, Write};
use std::path::{Path, PathBuf};

use crate::svcgen::{AsyncMode, TtrpcServiceGenerator};

const FILE_DESCRIPTOR_SET: &str = "fd_set.bin";

/// Selects the protobuf backend used by [`Codegen::run`].
///
/// This crate implements the Prost backend; select it explicitly with
/// [`Codegen::prost`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    /// Generate messages and services with [Prost](https://crates.io/crates/prost).
    Prost,
}

/// ttrpc client and server generation options.
///
/// Mirrors the options of the rust-protobuf based `ttrpc-codegen` so build
/// scripts only need to switch the backend selector.
#[derive(Debug, Clone, Default)]
pub struct Customize {
    /// Generate a module declaration file (`mod.rs`) alongside the bindings.
    ///
    /// When false, the declarations are written to `_include.rs` instead.
    pub gen_mod: bool,
    /// Generate asynchronous code for both the client and the server.
    pub async_all: bool,
    /// Generate asynchronous code for the server only.
    pub async_server: bool,
    /// Generate asynchronous code for the client only.
    pub async_client: bool,
    /// Derive `serde::Serialize` and `serde::Deserialize` on the generated
    /// message types.
    pub serde: bool,
}

impl Customize {
    fn async_mode(&self) -> AsyncMode {
        if self.async_all {
            AsyncMode::All
        } else if self.async_server {
            AsyncMode::Server
        } else if self.async_client {
            AsyncMode::Client
        } else {
            AsyncMode::None
        }
    }
}

/// Builder for Prost-based ttrpc code generation.
///
/// The fluent API matches the rust-protobuf based `ttrpc-codegen` crate:
/// select the backend with [`Codegen::prost`] and run the generation from a
/// build script.
///
/// # Examples
///
/// ```no_run
/// use ttrpc_codegen::{Codegen, Customize};
///
/// # fn main() -> std::io::Result<()> {
/// Codegen::new()
///     .out_dir(std::env::var("OUT_DIR").unwrap())
///     .input("proto/greeter.proto")
///     .include("proto")
///     .prost()
///     .customize(Customize {
///         async_all: true,
///         ..Default::default()
///     })
///     .run()?;
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Default)]
pub struct Codegen {
    /// Output directory, or `OUT_DIR` when unset.
    out_dir: Option<PathBuf>,
    /// Directories searched for imported `.proto` files.
    includes: Vec<PathBuf>,
    /// `.proto` files to compile.
    inputs: Vec<PathBuf>,
    /// Selected protobuf backend; required before `run`.
    backend: Option<Backend>,
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

    /// Selects the Prost backend for message and service generation.
    pub fn prost(&mut self) -> &mut Self {
        self.backend = Some(Backend::Prost);
        self
    }

    /// Sets ttrpc client and server generation options.
    pub fn customize(&mut self, customize: Customize) -> &mut Self {
        self.customize = customize;
        self
    }

    /// Parses the configured inputs and writes the generated Rust files.
    ///
    /// # Errors
    ///
    /// Returns an error if `protoc` cannot be executed, an input cannot be
    /// parsed, or the generated files cannot be written.
    pub fn run(&mut self) -> io::Result<()> {
        let backend = self
            .backend
            .ok_or_else(|| io::Error::other("no protobuf backend selected: call .prost()"))?;
        debug_assert_eq!(backend, Backend::Prost);

        if self.inputs.is_empty() {
            return Err(io::Error::other("no .proto inputs configured"));
        }
        if self.includes.is_empty() {
            return Err(io::Error::other("no include directories configured"));
        }

        // When out_dir is not set, fall back to OUT_DIR or the current
        // directory, matching the rust-protobuf based crate.
        let out_dir = self.out_dir.clone().unwrap_or_else(|| {
            std::env::var("OUT_DIR").map_or_else(
                |_| std::env::current_dir().unwrap_or_default(),
                PathBuf::from,
            )
        });

        CodegenImpl {
            out_dir,
            protos: self.inputs.clone(),
            includes: self.includes.clone(),
            customize: self.customize.clone(),
        }
        .generate()
    }
}

struct CodegenImpl {
    out_dir: PathBuf,
    protos: Vec<PathBuf>,
    includes: Vec<PathBuf>,
    customize: Customize,
}

impl CodegenImpl {
    fn generate(&self) -> io::Result<()> {
        self.compile_protos()?;
        self.write_header()?;
        self.clean_up()?;

        Ok(())
    }

    // TODO: Do not write header to the files that already has the header
    // TODO: Write header to the files generated by the codegen
    fn write_header(&self) -> io::Result<()> {
        // Read fd_set.bin
        let f = File::open(self.out_dir.join(FILE_DESCRIPTOR_SET))?;
        let mut reader = BufReader::new(f);
        let mut buffer = Vec::new();
        reader.read_to_end(&mut buffer)?;

        let fd_set = FileDescriptorSet::decode(&buffer as &[u8])
            .map_err(|e| io::Error::other(format!("decode fd_set: {e}")))?;

        for fd in fd_set.file.iter() {
            let rs_path = self.out_dir.join(format!("{}.rs", fd.package()));
            let mut f = match File::open(&rs_path) {
                Ok(f) => f,
                _ => continue,
            };
            let header = format!(
                r#"// This file is generated by ttrpc-codegen {}. Do not edit
// @generated

"#,
                env!("CARGO_PKG_VERSION")
            );

            let mut buf = Vec::<u8>::new();
            buf.write_all(header.as_bytes())?;
            f.read_to_end(&mut buf)
                .map_err(|e| io::Error::other(format!("read from rust file {rs_path:?}: {e}")))?;
            let mut f = File::create(&rs_path)
                .map_err(|e| io::Error::other(format!("open rust file {rs_path:?}: {e}")))?;
            f.write_all(buf.as_slice())
                .map_err(|e| io::Error::other(format!("write to rust file {rs_path:?}: {e}")))?;
        }

        Ok(())
    }

    fn compile_protos(&self) -> io::Result<()> {
        let mut config = Config::new();
        config.out_dir(&self.out_dir);
        // Services are always generated by this crate; there is no
        // service-less mode.
        config.service_generator(Box::new(TtrpcServiceGenerator::new(
            self.customize.async_mode(),
        )));
        config.protoc_arg("--experimental_allow_proto3_optional");
        config.compile_well_known_types();
        config.file_descriptor_set_path(self.out_dir.join(FILE_DESCRIPTOR_SET));
        let include_file = if self.customize.gen_mod {
            "mod.rs"
        } else {
            "_include.rs"
        };
        config.include_file(include_file);
        if self.customize.serde {
            config.message_attribute(".", "#[derive(::serde::Serialize, ::serde::Deserialize)]");
        }
        config
            .compile_protos(&self.protos, &self.includes)
            .map_err(|e| io::Error::other(format!("compile protos by prost: {e}")))?;
        Ok(())
    }

    fn clean_up(&self) -> io::Result<()> {
        fs::remove_file(self.out_dir.join(FILE_DESCRIPTOR_SET))?;
        Ok(())
    }
}
