// Copyright (c) 2019 Ant Financial
//
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

// Copyright (c) 2016, Stepan Koltsov
//
// Permission is hereby granted, free of charge, to any person obtaining
// a copy of this software and associated documentation files (the
// "Software"), to deal in the Software without restriction, including
// without limitation the rights to use, copy, modify, merge, publish,
// distribute, sublicense, and/or sell copies of the Software, and to
// permit persons to whom the Software is furnished to do so, subject to
// the following conditions:
//
// The above copyright notice and this permission notice shall be
// included in all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND,
// EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
// MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND
// NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE
// LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION
// OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION
// WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.

#![allow(dead_code)]

use std::collections::HashMap;

use crate::Customize;
use protobuf::{
    compiler_plugin::{GenRequest, GenResult},
    descriptor::*,
    descriptorx::*,
    plugin::{
        CodeGeneratorRequest, CodeGeneratorResponse, CodeGeneratorResponse_Feature,
        CodeGeneratorResponse_File,
    },
    Message,
};
use protobuf_codegen::code_writer::CodeWriter;
use std::fs::File;
use std::io::{self, stdin, stdout, Write};
use std::path::Path;

use super::util::{
    self, async_on, def_async_fn, fq_grpc, pub_async_fn, to_camel_case, to_snake_case, MethodType,
};

struct MethodGen<'a> {
    proto: &'a MethodDescriptorProto,
    package_name: String,
    service_name: String,
    root_scope: &'a RootScope<'a>,
    customize: &'a Customize,
}

impl<'a> MethodGen<'a> {
    fn new(
        proto: &'a MethodDescriptorProto,
        package_name: String,
        service_name: String,
        root_scope: &'a RootScope<'a>,
        customize: &'a Customize,
    ) -> MethodGen<'a> {
        MethodGen {
            proto,
            package_name,
            service_name,
            root_scope,
            customize,
        }
    }

    fn input(&self) -> String {
        format!(
            "super::{}",
            self.root_scope
                .find_message(self.proto.get_input_type())
                .rust_fq_name()
        )
    }

    fn output(&self) -> String {
        format!(
            "super::{}",
            self.root_scope
                .find_message(self.proto.get_output_type())
                .rust_fq_name()
        )
    }

    fn method_type(&self) -> (MethodType, String) {
        match (
            self.proto.get_client_streaming(),
            self.proto.get_server_streaming(),
        ) {
            (false, false) => (MethodType::Unary, fq_grpc("MethodType::Unary")),
            (true, false) => (
                MethodType::ClientStreaming,
                fq_grpc("MethodType::ClientStreaming"),
            ),
            (false, true) => (
                MethodType::ServerStreaming,
                fq_grpc("MethodType::ServerStreaming"),
            ),
            (true, true) => (MethodType::Duplex, fq_grpc("MethodType::Duplex")),
        }
    }

    fn service_name(&self) -> String {
        to_snake_case(&self.service_name)
    }

    fn name(&self) -> String {
        to_snake_case(self.proto.get_name())
    }

    fn struct_name(&self) -> String {
        to_camel_case(self.proto.get_name())
    }

    fn const_method_name(&self) -> String {
        format!(
            "METHOD_{}_{}",
            self.service_name().to_uppercase(),
            self.name().to_uppercase()
        )
    }

    fn write_handler(&self, w: &mut CodeWriter) {
        w.block(
            &format!("struct {}Method {{", self.struct_name()),
            "}",
            |w| {
                w.write_line(&format!(
                    "service: Arc<Box<dyn {} + Send + Sync>>,",
                    self.service_name
                ));
            },
        );
        w.write_line("");
        if async_on(self.customize, "server") {
            self.write_handler_impl_async(w)
        } else {
            self.write_handler_impl(w)
        }
    }

    fn write_handler_impl(&self, w: &mut CodeWriter) {
        w.block(&format!("impl ::ttrpc::MethodHandler for {}Method {{", self.struct_name()), "}",
        |w| {
            w.block("fn handler(&self, ctx: ::ttrpc::TtrpcContext, req: ::ttrpc::Request) -> ::ttrpc::Result<()> {", "}",
            |w| {
                w.write_line(&format!("::ttrpc::request_handler!(self, ctx, req, {}, {}, {});",
                                        proto_path_to_rust_mod(self.root_scope.find_message(self.proto.get_input_type()).get_scope().get_file_descriptor().get_name()),
                                        self.root_scope.find_message(self.proto.get_input_type()).rust_name(),
                                        self.name()));
                w.write_line("Ok(())");
            });
        });
    }

    fn write_handler_impl_async(&self, w: &mut CodeWriter) {
        w.write_line("#[async_trait]");
        match self.method_type().0 {
            MethodType::Unary => {
                w.block(&format!("impl ::ttrpc::r#async::MethodHandler for {}Method {{", self.struct_name()), "}",
                |w| {
                    w.block("async fn handler(&self, ctx: ::ttrpc::r#async::TtrpcContext, req: ::ttrpc::Request) -> ::ttrpc::Result<::ttrpc::Response> {", "}",
                        |w| {
                            w.write_line(&format!("::ttrpc::async_request_handler!(self, ctx, req, {}, {}, {});",
                                        proto_path_to_rust_mod(self.root_scope.find_message(self.proto.get_input_type()).get_scope().get_file_descriptor().get_name()),
                                        self.root_scope.find_message(self.proto.get_input_type()).rust_name(),
                                        self.name()));
                    });
            });
            }
            // only receive
            MethodType::ClientStreaming => {
                w.block(&format!("impl ::ttrpc::r#async::StreamHandler for {}Method {{", self.struct_name()), "}",
                |w| {
                    w.block("async fn handler(&self, ctx: ::ttrpc::r#async::TtrpcContext, inner: ::ttrpc::r#async::StreamInner) -> ::ttrpc::Result<Option<::ttrpc::Response>> {", "}",
                        |w| {
                            w.write_line(&format!("::ttrpc::async_client_streamimg_handler!(self, ctx, inner, {});",
                                        self.name()));
                    });
            });
            }
            // only send
            MethodType::ServerStreaming => {
                w.block(&format!("impl ::ttrpc::r#async::StreamHandler for {}Method {{", self.struct_name()), "}",
                |w| {
                    w.block("async fn handler(&self, ctx: ::ttrpc::r#async::TtrpcContext, mut inner: ::ttrpc::r#async::StreamInner) -> ::ttrpc::Result<Option<::ttrpc::Response>> {", "}",
                        |w| {
                            w.write_line(&format!("::ttrpc::async_server_streamimg_handler!(self, ctx, inner, {}, {}, {});",
                                        proto_path_to_rust_mod(self.root_scope.find_message(self.proto.get_input_type()).get_scope().get_file_descriptor().get_name()),
                                        self.root_scope.find_message(self.proto.get_input_type()).rust_name(),
                                        self.name()));
                    });
            });
            }
            // receive and send
            MethodType::Duplex => {
                w.block(&format!("impl ::ttrpc::r#async::StreamHandler for {}Method {{", self.struct_name()), "}",
                |w| {
                    w.block("async fn handler(&self, ctx: ::ttrpc::r#async::TtrpcContext, inner: ::ttrpc::r#async::StreamInner) -> ::ttrpc::Result<Option<::ttrpc::Response>> {", "}",
                        |w| {
                            w.write_line(&format!("::ttrpc::async_duplex_streamimg_handler!(self, ctx, inner, {});",
                                        self.name()));
                    });
            });
            }
        }
    }

    // Method signatures
    fn unary(&self, method_name: &str) -> String {
        format!(
            "{}(&self, ctx: ttrpc::context::Context, req: &{}) -> {}<{}>",
            method_name,
            self.input(),
            fq_grpc("Result"),
            self.output()
        )
    }

    fn client_streaming(&self, method_name: &str) -> String {
        format!(
            "{}(&self, ctx: ttrpc::context::Context) -> {}<{}<{}, {}>>",
            method_name,
            fq_grpc("Result"),
            fq_grpc("r#async::ClientStreamSender"),
            self.input(),
            self.output()
        )
    }

    fn server_streaming(&self, method_name: &str) -> String {
        format!(
            "{}(&self, ctx: ttrpc::context::Context, req: &{}) -> {}<{}<{}>>",
            method_name,
            self.input(),
            fq_grpc("Result"),
            fq_grpc("r#async::ClientStreamReceiver"),
            self.output()
        )
    }

    fn duplex_streaming(&self, method_name: &str) -> String {
        format!(
            "{}(&self, ctx: ttrpc::context::Context) -> {}<{}<{}, {}>>",
            method_name,
            fq_grpc("Result"),
            fq_grpc("r#async::ClientStream"),
            self.input(),
            self.output()
        )
    }

    fn write_client(&self, w: &mut CodeWriter) {
        let method_name = self.name();
        if let MethodType::Unary = self.method_type().0 {
            w.pub_fn(&self.unary(&method_name), |w| {
                w.write_line(&format!("let mut cres = {}::new();", self.output()));
                w.write_line(&format!(
                    "::ttrpc::client_request!(self, ctx, req, \"{}.{}\", \"{}\", cres);",
                    self.package_name,
                    self.service_name,
                    &self.proto.get_name(),
                ));
                w.write_line("Ok(cres)");
            });
        }
    }

    fn write_async_client(&self, w: &mut CodeWriter) {
        let method_name = self.name();
        match self.method_type().0 {
            // Unary RPC
            MethodType::Unary => {
                pub_async_fn(w, &self.unary(&method_name), |w| {
                    w.write_line(&format!("let mut cres = {}::new();", self.output()));
                    w.write_line(&format!(
                        "::ttrpc::async_client_request!(self, ctx, req, \"{}.{}\", \"{}\", cres);",
                        self.package_name,
                        self.service_name,
                        &self.proto.get_name(),
                    ));
                });
            }
            // Client Streaming RPC
            MethodType::ClientStreaming => {
                pub_async_fn(w, &self.client_streaming(&method_name), |w| {
                    w.write_line(&format!(
                        "::ttrpc::async_client_stream_send!(self, ctx, \"{}.{}\", \"{}\");",
                        self.package_name,
                        self.service_name,
                        &self.proto.get_name(),
                    ));
                });
            }
            // Server Streaming RPC
            MethodType::ServerStreaming => {
                pub_async_fn(w, &self.server_streaming(&method_name), |w| {
                    w.write_line(&format!(
                        "::ttrpc::async_client_stream_receive!(self, ctx, req, \"{}.{}\", \"{}\");",
                        self.package_name,
                        self.service_name,
                        &self.proto.get_name(),
                    ));
                });
            }
            // Bidirectional streaming RPC
            MethodType::Duplex => {
                pub_async_fn(w, &self.duplex_streaming(&method_name), |w| {
                    w.write_line(&format!(
                        "::ttrpc::async_client_stream!(self, ctx, \"{}.{}\", \"{}\");",
                        self.package_name,
                        self.service_name,
                        &self.proto.get_name(),
                    ));
                });
            }
        };
    }

    fn write_service(&self, w: &mut CodeWriter) {
        let (_req, req_type, resp_type) = match self.method_type().0 {
            MethodType::Unary => ("req", self.input(), self.output()),
            MethodType::ClientStreaming => (
                "stream",
                format!("::ttrpc::r#async::ServerStreamReceiver<{}>", self.input()),
                self.output(),
            ),
            MethodType::ServerStreaming => (
                "req",
                format!(
                    "{}, _: {}<{}>",
                    self.input(),
                    "::ttrpc::r#async::ServerStreamSender",
                    self.output()
                ),
                "()".to_string(),
            ),
            MethodType::Duplex => (
                "stream",
                format!(
                    "{}<{}, {}>",
                    "::ttrpc::r#async::ServerStream",
                    self.output(),
                    self.input(),
                ),
                "()".to_string(),
            ),
        };

        let get_sig = |context_name| {
            format!(
                "{}(&self, _ctx: &{}, _: {}) -> ::ttrpc::Result<{}>",
                self.name(),
                fq_grpc(context_name),
                req_type,
                resp_type,
            )
        };

        let cb = |w: &mut CodeWriter| {
            w.write_line(format!("Err(::ttrpc::Error::RpcStatus(::ttrpc::get_status(::ttrpc::Code::NOT_FOUND, \"/{}.{}/{} is not supported\".to_string())))",
            self.package_name,
            self.service_name, self.proto.get_name(),));
        };

        if async_on(self.customize, "server") {
            let sig = get_sig("r#async::TtrpcContext");
            def_async_fn(w, &sig, cb);
        } else {
            let sig = get_sig("TtrpcContext");
            w.def_fn(&sig, cb);
        }
    }

    fn write_bind(&self, w: &mut CodeWriter) {
        let method_handler_name = "::ttrpc::MethodHandler";

        let s = format!(
            "methods.insert(\"/{}.{}/{}\".to_string(),
                    Box::new({}Method{{service: service.clone()}}) as Box<dyn {} + Send + Sync>);",
            self.package_name,
            self.service_name,
            self.proto.get_name(),
            self.struct_name(),
            method_handler_name,
        );
        w.write_line(&s);
    }

    fn write_async_bind(&self, w: &mut CodeWriter) {
        let s = if matches!(self.method_type().0, MethodType::Unary) {
            format!(
                "methods.insert(\"{}\".to_string(),
                    Box::new({}Method{{service: service.clone()}}) as {});",
                self.proto.get_name(),
                self.struct_name(),
                "Box<dyn ::ttrpc::r#async::MethodHandler + Send + Sync>"
            )
        } else {
            format!(
                "streams.insert(\"{}\".to_string(),
                    Arc::new({}Method{{service: service.clone()}}) as {});",
                self.proto.get_name(),
                self.struct_name(),
                "Arc<dyn ::ttrpc::r#async::StreamHandler + Send + Sync>"
            )
        };
        w.write_line(&s);
    }
}

struct ServiceGen<'a> {
    proto: &'a ServiceDescriptorProto,
    methods: Vec<MethodGen<'a>>,
    customize: &'a Customize,
    package_name: String,
}

impl<'a> ServiceGen<'a> {
    fn new(
        proto: &'a ServiceDescriptorProto,
        file: &FileDescriptorProto,
        root_scope: &'a RootScope,
        customize: &'a Customize,
    ) -> ServiceGen<'a> {
        let methods = proto
            .get_method()
            .iter()
            .map(|m| {
                MethodGen::new(
                    m,
                    file.get_package().to_string(),
                    util::to_camel_case(proto.get_name()),
                    root_scope,
                    customize,
                )
            })
            .collect();

        ServiceGen {
            proto,
            methods,
            customize,
            package_name: file.get_package().to_string(),
        }
    }

    fn service_name(&self) -> String {
        util::to_camel_case(self.proto.get_name())
    }

    fn service_path(&self) -> String {
        format!("{}.{}", self.package_name, self.service_name())
    }

    fn client_name(&self) -> String {
        format!("{}Client", self.service_name())
    }

    fn has_stream_method(&self) -> bool {
        self.methods
            .iter()
            .any(|method| !matches!(method.method_type().0, MethodType::Unary))
    }

    fn write_client(&self, w: &mut CodeWriter) {
        if async_on(self.customize, "client") {
            self.write_async_client(w)
        } else {
            self.write_sync_client(w)
        }
    }

    fn write_sync_client(&self, w: &mut CodeWriter) {
        w.write_line("#[derive(Clone)]");
        w.pub_struct(&self.client_name(), |w| {
            w.field_decl("client", "::ttrpc::Client");
        });

        w.write_line("");

        w.impl_self_block(&self.client_name(), |w| {
            w.pub_fn("new(client: ::ttrpc::Client) -> Self", |w| {
                w.expr_block(&self.client_name(), |w| {
                    w.field_entry("client", "client");
                });
            });

            for method in &self.methods {
                w.write_line("");
                method.write_client(w);
            }
        });
    }

    fn write_async_client(&self, w: &mut CodeWriter) {
        w.write_line("#[derive(Clone)]");
        w.pub_struct(&self.client_name(), |w| {
            w.field_decl("client", "::ttrpc::r#async::Client");
        });

        w.write_line("");

        w.impl_self_block(&self.client_name(), |w| {
            w.pub_fn("new(client: ::ttrpc::r#async::Client) -> Self", |w| {
                w.expr_block(&self.client_name(), |w| {
                    w.field_entry("client", "client");
                });
            });

            for method in &self.methods {
                w.write_line("");
                method.write_async_client(w);
            }
        });
    }

    fn write_server(&self, w: &mut CodeWriter) {
        let mut trait_name = self.service_name();
        if async_on(self.customize, "server") {
            w.write_line("#[async_trait]");
            trait_name = format!("{}: Sync", &self.service_name());
        }

        w.pub_trait(&trait_name, |w| {
            for method in &self.methods {
                method.write_service(w);
            }
        });

        w.write_line("");
        if async_on(self.customize, "server") {
            self.write_async_server_create(w);
        } else {
            self.write_sync_server_create(w);
        }
    }

    fn write_sync_server_create(&self, w: &mut CodeWriter) {
        let method_handler_name = "::ttrpc::MethodHandler";
        let s = format!(
            "create_{}(service: Arc<Box<dyn {} + Send + Sync>>) -> HashMap<String, Box<dyn {} + Send + Sync>>",
            to_snake_case(&self.service_name()),
            self.service_name(),
            method_handler_name,
        );

        w.pub_fn(&s, |w| {
            w.write_line("let mut methods = HashMap::new();");
            for method in &self.methods[0..self.methods.len()] {
                w.write_line("");
                method.write_bind(w);
            }
            w.write_line("");
            w.write_line("methods");
        });
    }

    fn write_async_server_create(&self, w: &mut CodeWriter) {
        let s = format!(
            "create_{}(service: Arc<Box<dyn {} + Send + Sync>>) -> HashMap<String, {}>",
            to_snake_case(&self.service_name()),
            self.service_name(),
            "::ttrpc::r#async::Service"
        );

        let has_stream_method = self.has_stream_method();
        w.pub_fn(&s, |w| {
            w.write_line("let mut ret = HashMap::new();");
            w.write_line("let mut methods = HashMap::new();");
            if has_stream_method {
                w.write_line("let mut streams = HashMap::new();");
            } else {
                w.write_line("let streams = HashMap::new();");
            }
            for method in &self.methods[0..self.methods.len()] {
                w.write_line("");
                method.write_async_bind(w);
            }
            w.write_line("");
            w.write_line(format!(
                "ret.insert(\"{}\".to_string(), {});",
                self.service_path(),
                "::ttrpc::r#async::Service{ methods, streams }"
            ));
            w.write_line("ret");
        });
    }

    fn write_method_handlers(&self, w: &mut CodeWriter) {
        for (i, method) in self.methods.iter().enumerate() {
            if i != 0 {
                w.write_line("");
            }

            method.write_handler(w);
        }
    }

    fn write(&self, w: &mut CodeWriter) {
        self.write_client(w);
        w.write_line("");
        self.write_method_handlers(w);
        w.write_line("");
        self.write_server(w);
    }
}

pub fn write_generated_by(w: &mut CodeWriter, pkg: &str, version: &str) {
    w.write_line(format!(
        "// This file is generated by {pkg} {version}. Do not edit",
        pkg = pkg,
        version = version
    ));
    write_generated_common(w);
}

fn write_generated_common(w: &mut CodeWriter) {
    // https://secure.phabricator.com/T784
    w.write_line("// @generated");

    w.write_line("");
    w.write_line("#![cfg_attr(rustfmt, rustfmt_skip)]");
    w.write_line("#![allow(unknown_lints)]");
    w.write_line("#![allow(clipto_camel_casepy)]");
    w.write_line("#![allow(box_pointers)]");
    w.write_line("#![allow(dead_code)]");
    w.write_line("#![allow(missing_docs)]");
    w.write_line("#![allow(non_camel_case_types)]");
    w.write_line("#![allow(non_snake_case)]");
    w.write_line("#![allow(non_upper_case_globals)]");
    w.write_line("#![allow(trivial_casts)]");
    w.write_line("#![allow(unsafe_code)]");
    w.write_line("#![allow(unused_imports)]");
    w.write_line("#![allow(unused_results)]");
    w.write_line("#![allow(clippy::all)]");
}

fn gen_file(
    file: &FileDescriptorProto,
    root_scope: &RootScope,
    customize: &Customize,
) -> Option<GenResult> {
    if file.get_service().is_empty() {
        return None;
    }

    let base = protobuf::descriptorx::proto_path_to_rust_mod(file.get_name());

    let mut v = Vec::new();
    {
        let mut w = CodeWriter::new(&mut v);

        write_generated_by(&mut w, "ttrpc-compiler", env!("CARGO_PKG_VERSION"));

        w.write_line("use protobuf::{CodedInputStream, CodedOutputStream, Message};");
        w.write_line("use std::collections::HashMap;");
        w.write_line("use std::sync::Arc;");
        if customize.async_all || customize.async_client || customize.async_server {
            w.write_line("use async_trait::async_trait;");
        }

        for service in file.get_service() {
            w.write_line("");
            ServiceGen::new(service, file, root_scope, customize).write(&mut w);
        }
    }

    Some(GenResult {
        name: base + "_ttrpc.rs",
        content: v,
    })
}

pub fn gen(
    file_descriptors: &[FileDescriptorProto],
    files_to_generate: &[String],
    customize: &Customize,
) -> Vec<GenResult> {
    let files_map: HashMap<&str, &FileDescriptorProto> =
        file_descriptors.iter().map(|f| (f.get_name(), f)).collect();

    let root_scope = RootScope { file_descriptors };

    let mut results = Vec::new();

    for file_name in files_to_generate {
        let file = files_map[&file_name[..]];

        if file.get_service().is_empty() {
            continue;
        }

        results.extend(gen_file(file, &root_scope, customize).into_iter());
    }

    results
}

pub fn gen_and_write(
    file_descriptors: &[FileDescriptorProto],
    files_to_generate: &[String],
    out_dir: &Path,
    customize: &Customize,
) -> io::Result<()> {
    let results = gen(file_descriptors, files_to_generate, customize);

    for r in &results {
        let mut file_path = out_dir.to_owned();
        file_path.push(&r.name);
        let mut file_writer = File::create(&file_path)?;
        file_writer.write_all(&r.content)?;
        file_writer.flush()?;
    }

    Ok(())
}

pub fn protoc_gen_grpc_rust_main() {
    plugin_main(|file_descriptors, files_to_generate| {
        gen(
            file_descriptors,
            files_to_generate,
            &Customize {
                ..Default::default()
            },
        )
    });
}

fn plugin_main<F>(gen: F)
where
    F: Fn(&[FileDescriptorProto], &[String]) -> Vec<GenResult>,
{
    plugin_main_2(|r| gen(r.file_descriptors, r.files_to_generate))
}

fn plugin_main_2<F>(gen: F)
where
    F: Fn(&GenRequest) -> Vec<GenResult>,
{
    let req = CodeGeneratorRequest::parse_from_reader(&mut stdin()).unwrap();
    let result = gen(&GenRequest {
        file_descriptors: req.get_proto_file(),
        files_to_generate: req.get_file_to_generate(),
        parameter: req.get_parameter(),
    });
    let mut resp = CodeGeneratorResponse::new();
    resp.set_supported_features(CodeGeneratorResponse_Feature::FEATURE_PROTO3_OPTIONAL as u64);
    resp.set_file(
        result
            .iter()
            .map(|file| {
                let mut r = CodeGeneratorResponse_File::new();
                r.set_name(file.name.to_string());
                r.set_content(
                    std::str::from_utf8(file.content.as_ref())
                        .unwrap()
                        .to_string(),
                );
                r
            })
            .collect(),
    );
    resp.write_to_writer(&mut stdout()).unwrap();
}
