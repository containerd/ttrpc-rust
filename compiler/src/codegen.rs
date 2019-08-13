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

use std::collections::HashMap;

use protobuf;
use protobuf::compiler_plugin;
use protobuf::descriptor::*;
use protobuf::descriptorx::*;
use protobuf_codegen::code_writer::CodeWriter;

use super::util::{self, fq_grpc, to_snake_case, MethodType};

struct MethodGen<'a> {
    proto: &'a MethodDescriptorProto,
    service_name: String,
    service_path: String,
    root_scope: &'a RootScope<'a>,
}

impl<'a> MethodGen<'a> {
    fn new(
        proto: &'a MethodDescriptorProto,
        service_name: String,
        service_path: String,
        root_scope: &'a RootScope<'a>,
    ) -> MethodGen<'a> {
        MethodGen {
            proto,
            service_name,
            service_path,
            root_scope,
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

    fn fq_name(&self) -> String {
        format!("\"{}/{}\"", self.service_path, &self.proto.get_name())
    }

    fn const_method_name(&self) -> String {
        format!(
            "METHOD_{}_{}",
            self.service_name().to_uppercase(),
            self.name().to_uppercase()
        )
    }

    fn write_definition(&self, w: &mut CodeWriter) {
        let head = format!(
            "const {}: {}<{}, {}> = {} {{",
            self.const_method_name(),
            fq_grpc("Method"),
            self.input(),
            self.output(),
            fq_grpc("Method")
        );
        let pb_mar = format!(
            "{} {{ ser: {}, de: {} }}",
            fq_grpc("Marshaller"),
            fq_grpc("pb_ser"),
            fq_grpc("pb_de")
        );
        w.block(&head, "};", |w| {
            w.field_entry("ty", &self.method_type().1);
            w.field_entry("name", &self.fq_name());
            w.field_entry("req_mar", &pb_mar);
            w.field_entry("resp_mar", &pb_mar);
        });
    }

    fn write_handler(&self, w: &mut CodeWriter) {
        w.block(&format!("struct {}_method {{", self.name()), "}",
        |w| {
            w.write_line(&format!("service: Arc<std::boxed::Box<{} + Send + Sync>>,", self.service_name));
        });
        w.write_line("");
        w.block(&format!("impl ::ttrpc::MethodHandler for {}_method {{", self.name()), "}",
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

    // Method signatures
    fn unary(&self, method_name: &str) -> String {
        format!(
            "{}(&self, req: &{}, timeout_nano: i64) -> {}<{}>",
            method_name,
            self.input(),
            fq_grpc("Result"),
            self.output()
        )
    }

    fn unary_opt(&self, method_name: &str) -> String {
        format!(
            "{}_opt(&self, req: &{}, opt: {}) -> {}<{}>",
            method_name,
            self.input(),
            fq_grpc("CallOption"),
            fq_grpc("Result"),
            self.output()
        )
    }

    fn unary_async(&self, method_name: &str) -> String {
        format!(
            "{}_async(&self, req: &{}) -> {}<{}<{}>>",
            method_name,
            self.input(),
            fq_grpc("Result"),
            fq_grpc("ClientUnaryReceiver"),
            self.output()
        )
    }

    fn unary_async_opt(&self, method_name: &str) -> String {
        format!(
            "{}_async_opt(&self, req: &{}, opt: {}) -> {}<{}<{}>>",
            method_name,
            self.input(),
            fq_grpc("CallOption"),
            fq_grpc("Result"),
            fq_grpc("ClientUnaryReceiver"),
            self.output()
        )
    }

    fn client_streaming(&self, method_name: &str) -> String {
        format!(
            "{}(&self) -> {}<({}<{}>, {}<{}>)>",
            method_name,
            fq_grpc("Result"),
            fq_grpc("ClientCStreamSender"),
            self.input(),
            fq_grpc("ClientCStreamReceiver"),
            self.output()
        )
    }

    fn client_streaming_opt(&self, method_name: &str) -> String {
        format!(
            "{}_opt(&self, opt: {}) -> {}<({}<{}>, {}<{}>)>",
            method_name,
            fq_grpc("CallOption"),
            fq_grpc("Result"),
            fq_grpc("ClientCStreamSender"),
            self.input(),
            fq_grpc("ClientCStreamReceiver"),
            self.output()
        )
    }

    fn server_streaming(&self, method_name: &str) -> String {
        format!(
            "{}(&self, req: &{}) -> {}<{}<{}>>",
            method_name,
            self.input(),
            fq_grpc("Result"),
            fq_grpc("ClientSStreamReceiver"),
            self.output()
        )
    }

    fn server_streaming_opt(&self, method_name: &str) -> String {
        format!(
            "{}_opt(&self, req: &{}, opt: {}) -> {}<{}<{}>>",
            method_name,
            self.input(),
            fq_grpc("CallOption"),
            fq_grpc("Result"),
            fq_grpc("ClientSStreamReceiver"),
            self.output()
        )
    }

    fn duplex_streaming(&self, method_name: &str) -> String {
        format!(
            "{}(&self) -> {}<({}<{}>, {}<{}>)>",
            method_name,
            fq_grpc("Result"),
            fq_grpc("ClientDuplexSender"),
            self.input(),
            fq_grpc("ClientDuplexReceiver"),
            self.output()
        )
    }

    fn duplex_streaming_opt(&self, method_name: &str) -> String {
        format!(
            "{}_opt(&self, opt: {}) -> {}<({}<{}>, {}<{}>)>",
            method_name,
            fq_grpc("CallOption"),
            fq_grpc("Result"),
            fq_grpc("ClientDuplexSender"),
            self.input(),
            fq_grpc("ClientDuplexReceiver"),
            self.output()
        )
    }

    fn write_client(&self, w: &mut CodeWriter) {
        let method_name = self.name();
        match self.method_type().0 {
            // Unary
            MethodType::Unary => {
                w.pub_fn(&self.unary(&method_name), |w| {
                    w.write_line(&format!(
                        "let mut cres = {}::new();",
                        self.output()
                    ));
                    w.write_line(&format!(
                        "::ttrpc::client_request!(self, req, timeout_nano, \"grpc.{}\", \"{}\", cres);",
                        self.service_name,
                        &self.proto.get_name()
                    ));
                    w.write_line("Ok(cres)");
                });
            }

            _ => {}
        };
    }

    fn write_service(&self, w: &mut CodeWriter) {
        let req_stream_type = format!("{}<{}>", fq_grpc("RequestStream"), self.input());
        let (req, req_type, resp_type) = match self.method_type().0 {
            MethodType::Unary => ("req", self.input(), "UnarySink"),
            MethodType::ClientStreaming => ("stream", req_stream_type, "ClientStreamingSink"),
            MethodType::ServerStreaming => ("req", self.input(), "ServerStreamingSink"),
            MethodType::Duplex => ("stream", req_stream_type, "DuplexSink"),
        };

        let sig = format!(
            "{}(&self, ctx: &{}, {}: {}) -> ::ttrpc::Result<{}>",
            self.name(),
            fq_grpc("TtrpcContext"),
            req,
            req_type,
            self.output()
        );

        w.def_fn(&sig, |w| {
            w.write_line(format!("Err(::ttrpc::Error::RpcStatus(::ttrpc::get_Status(::ttrpc::Code::NOT_FOUND, \"/grpc.{}/{} is not supported\".to_string())))",
                                 self.service_name, self.proto.get_name(),));
        });
    }

    fn write_bind(&self, w: &mut CodeWriter) {
        let s = format!("methods.insert(\"/grpc.{}/{}\".to_string(),
                    std::boxed::Box::new({}_method{{service: service.clone()}}) as std::boxed::Box<::ttrpc::MethodHandler + Send + Sync>);",
                        self.service_name, self.proto.get_name(), self.name());
        w.write_line(&s);
    }
}

struct ServiceGen<'a> {
    proto: &'a ServiceDescriptorProto,
    methods: Vec<MethodGen<'a>>,
}

impl<'a> ServiceGen<'a> {
    fn new(
        proto: &'a ServiceDescriptorProto,
        file: &FileDescriptorProto,
        root_scope: &'a RootScope,
    ) -> ServiceGen<'a> {
        let service_path = if file.get_package().is_empty() {
            format!("/{}", proto.get_name())
        } else {
            format!("/{}.{}", file.get_package(), proto.get_name())
        };
        let methods = proto
            .get_method()
            .iter()
            .map(|m| {
                MethodGen::new(
                    m,
                    util::to_camel_case(proto.get_name()),
                    service_path.clone(),
                    root_scope,
                )
            })
            .collect();

        ServiceGen { proto, methods }
    }

    fn service_name(&self) -> String {
        util::to_camel_case(self.proto.get_name())
    }

    fn client_name(&self) -> String {
        format!("{}Client", self.service_name())
    }

    fn write_client(&self, w: &mut CodeWriter) {
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

    fn write_server(&self, w: &mut CodeWriter) {
        w.pub_trait(&self.service_name(), |w| {
            for method in &self.methods {
                method.write_service(w);
            }
        });

        w.write_line("");

        let s = format!(
            "create_{}(service: Arc<std::boxed::Box<{} + Send + Sync>>) -> HashMap <String, Box<::ttrpc::MethodHandler + Send + Sync>>",
            to_snake_case(&self.service_name()), self.service_name()
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

    fn write_method_definitions(&self, w: &mut CodeWriter) {
        for (i, method) in self.methods.iter().enumerate() {
            if i != 0 {
                w.write_line("");
            }

            method.write_definition(w);
        }
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

fn gen_file(
    file: &FileDescriptorProto,
    root_scope: &RootScope,
) -> Option<compiler_plugin::GenResult> {
    if file.get_service().is_empty() {
        return None;
    }

    let base = protobuf::descriptorx::proto_path_to_rust_mod(file.get_name());

    let mut v = Vec::new();
    {
        let mut w = CodeWriter::new(&mut v);

        w.write_line("use protobuf::{CodedInputStream, CodedOutputStream, Message};");
        w.write_line("use std::collections::HashMap;");
        w.write_line("use std::sync::Arc;");

        for service in file.get_service() {
            w.write_line("");
            ServiceGen::new(service, file, root_scope).write(&mut w);
        }
    }

    Some(compiler_plugin::GenResult {
        name: base + "_ttrpc.rs",
        content: v,
    })
}

pub fn gen(
    file_descriptors: &[FileDescriptorProto],
    files_to_generate: &[String],
) -> Vec<compiler_plugin::GenResult> {
    let files_map: HashMap<&str, &FileDescriptorProto> =
        file_descriptors.iter().map(|f| (f.get_name(), f)).collect();

    let root_scope = RootScope { file_descriptors };

    let mut results = Vec::new();

    for file_name in files_to_generate {
        let file = files_map[&file_name[..]];

        if file.get_service().is_empty() {
            continue;
        }

        results.extend(gen_file(file, &root_scope).into_iter());
    }

    results
}

pub fn protoc_gen_grpc_rust_main() {
    compiler_plugin::plugin_main(gen);
}
