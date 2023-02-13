use std::fmt;

use proc_macro2::{Ident, TokenStream};
use prost_build::{Method, Service, ServiceGenerator};
use quote::{format_ident, quote};

use crate::util::{to_camel_case, to_snake_case, ttrpc_mod, type_token};

/// An implementation of Ttrpc service generator with the prost.
/// The types supported by the generator are:
/// - service sync/async
pub struct TtrpcServiceGenerator {
    async_mode: AsyncMode,
}

impl TtrpcServiceGenerator {
    pub fn new(async_mode: AsyncMode) -> Self {
        Self { async_mode }
    }
}

impl ServiceGenerator for TtrpcServiceGenerator {
    fn finalize(&mut self, _buf: &mut String) {}
    fn finalize_package(&mut self, _package: &str, _buf: &mut String) {}
    /// Generate services
    fn generate(&mut self, service: Service, buf: &mut String) {
        self.generate_type_aliases(buf);
        self.generate_trait(&service, buf);
        self.generate_method_handlers(&service, buf);
        self.generate_creating_service_method(&service, buf);
        self.generate_client(&service, buf);
    }
}

// Generator engine
impl TtrpcServiceGenerator {
    /// Generate type aliases at the beginning of the service
    fn generate_type_aliases(&self, buf: &mut String) {
        let async_trait_token = if self.async_mode != AsyncMode::None {
            quote!(
                use async_trait::async_trait;
            )
        } else {
            quote!()
        };
        let type_aliases = quote!(
            use std::collections::HashMap;
            use std::sync::Arc;
            use prost::Message;
            #async_trait_token
        );
        buf.push_str(type_aliases.to_string().as_str());
    }

    /// Generate the service trait for the server.
    /// Assumed that there is a service named "Example", which enables async
    /// feature, then the trait would be:
    /// #[async_trait]
    /// pub trait ExampleService: Sync {
    ///     // === the methods are snipped ===
    /// }
    fn generate_trait(&self, service: &Service, buf: &mut String) {
        let trait_name = format_ident!("{}", service.name);
        let (derive_token, sync_token) = if async_on(self.async_mode, Side::Server) {
            (quote!( #[async_trait] ), quote!( : Sync ))
        } else {
            (quote!(), quote!())
        };
        let method_signatures: Vec<_> = service
            .methods
            .iter()
            .map(|method| self.trait_method_signature_token(service, method))
            .collect();
        let trait_token = quote!(
            #derive_token
            pub trait #trait_name #sync_token {
                #(#method_signatures)*
            }
        );
        buf.push_str(&trait_token.to_string())
    }

    fn trait_method_signature_token(&self, service: &Service, method: &Method) -> TokenStream {
        let mod_path = ttrpc_mod();
        let name = format_ident!("{}", self.method_name_rust(method));
        let method_type = MethodType::from_method(method);
        // Input/output type
        let input_type = type_token(&method.input_type);
        let output_type = type_token(&method.output_type);
        let (req_type, resp_type) = match method_type {
            MethodType::Unary => (quote!( #input_type ), quote!( #output_type )),
            MethodType::ClientStreaming => (
                quote!( #mod_path::r#async::ServerStreamReceiver<#input_type> ),
                quote!( #output_type ),
            ),
            MethodType::ServerStreaming => (
                quote!( #input_type, _: #mod_path::r#async::ServerStreamSender<#output_type> ),
                quote!(()),
            ),
            MethodType::Duplex => (
                quote!( #mod_path::r#async::ServerStream<#output_type, #input_type> ),
                quote!(()),
            ),
        };
        let context = self.ttrpc_context(async_on(self.async_mode, Side::Server));
        let err_msg = format!(
            "{}.{}/{} is not supported",
            service.package, service.name, method.proto_name
        );
        // Prepend a function prefix if necessary
        let async_token = if async_on(self.async_mode, Side::Server) {
            quote!(async)
        } else {
            quote!()
        };

        quote!(
            #async_token fn #name(&self, _ctx: &#context, _: #req_type) -> #mod_path::Result<#resp_type> {
                Err(
                    #mod_path::Error::RpcStatus(
                        #mod_path::get_status(
                            #mod_path::Code::NotFound,
                            #err_msg,
                        )
                    )
                )
            }
        )
    }

    /// Generate method handlers for each method.
    fn generate_method_handlers(&self, service: &Service, buf: &mut String) {
        for method in service.methods.iter() {
            let method_handler = self.method_handler_token(service, method);
            buf.push_str(&method_handler.to_string());
        }
    }

    fn method_handler_token(&self, service: &Service, method: &Method) -> TokenStream {
        let struct_name = format_ident!("{}Method", to_camel_case(method.proto_name.as_str()));
        let service_name = format_ident!("{}", service.name);
        let method_handler_impl = if async_on(self.async_mode, Side::Server) {
            self.method_handler_impl_async_token(&struct_name, method)
        } else {
            self.method_handler_impl_sync_token(&struct_name, method)
        };
        quote!(
            struct #struct_name {
                service: Arc<Box<dyn #service_name + Send + Sync>>,
            }
            #method_handler_impl
        )
    }

    fn method_handler_impl_sync_token(&self, struct_name: &Ident, method: &Method) -> TokenStream {
        let mod_path = ttrpc_mod();
        let context = self.ttrpc_context(false);
        let input_type = format_ident!("{}", method.input_type);
        let method_name = format_ident!("{}", self.method_name_rust(method));
        quote!(
            impl #mod_path::MethodHandler for #struct_name {
                fn handler(&self, ctx: #context, req: #mod_path::Request) -> #mod_path::Result<()> {
                    #mod_path::request_handler!(self, ctx, req, #input_type, #method_name);
                    Ok(())
                }
            }
        )
    }

    fn method_handler_impl_async_token(&self, struct_name: &Ident, method: &Method) -> TokenStream {
        let mod_path = ttrpc_mod();
        let context = self.ttrpc_context(true);
        let input_type = format_ident!("{}", method.input_type);
        let method_name = format_ident!("{}", self.method_name_rust(method));

        let (handler_trait, inner_token, result_type_token, handler_token) =
            match MethodType::from_method(method) {
                MethodType::Unary => (
                    quote!(MethodHandler),
                    quote!( req: #mod_path::Request ),
                    quote!( #mod_path::Response ),
                    quote!( #mod_path::async_request_handler!(self, ctx, req, #input_type, #method_name); ),
                ),
                MethodType::ClientStreaming => (
                    quote!(StreamHandler),
                    quote!( inner: #mod_path::r#async::StreamInner ),
                    quote!( Option<#mod_path::Response> ),
                    quote!( #mod_path::async_client_streamimg_handler!(self, ctx, inner, #method_name); ),
                ),
                MethodType::ServerStreaming => (
                    quote!(StreamHandler),
                    quote!( mut inner: #mod_path::r#async::StreamInner ),
                    quote!( Option<#mod_path::Response> ),
                    quote!( #mod_path::async_server_streamimg_handler!(self, ctx, inner, #input_type, #method_name); ),
                ),
                MethodType::Duplex => (
                    quote!(StreamHandler),
                    quote!( inner: #mod_path::r#async::StreamInner ),
                    quote!( Option<#mod_path::Response> ),
                    quote!( #mod_path::async_duplex_streamimg_handler!(self, ctx, inner, #method_name); ),
                ),
            };

        quote!(
            #[async_trait]
            impl ::ttrpc::r#async::#handler_trait for #struct_name {
                async fn handler(&self, ctx: #context, #inner_token) -> #mod_path::Result<#result_type_token> {
                    #handler_token
                }
            }
        )
    }

    fn generate_creating_service_method(&self, service: &Service, buf: &mut String) {
        let creating_service_method = if async_on(self.async_mode, Side::Server) {
            self.async_creating_service_method_token(service)
        } else {
            self.sync_creating_service_method_token(service)
        };
        buf.push_str(&creating_service_method.to_string());
    }

    fn sync_creating_service_method_token(&self, service: &Service) -> TokenStream {
        let create_service_name = format_ident!("create_{}", self.service_name_rust(service));
        let service_trait = format_ident!("{}", service.name);
        let mod_path = ttrpc_mod();
        let method_inserts: Vec<_> = service.methods.iter().map(|method| {
            let key = format!("/{}.{}/{}", service.package, service.name, method.proto_name);
            let mm = format_ident!("{}Method", to_camel_case(&method.proto_name));
            quote!(
                methods.insert(
                    #key.to_string(),
                    Box::new(#mm{service: service.clone()}) as Box<dyn #mod_path::MethodHandler + Send + Sync>);
            )
        }).collect();

        quote!(
            pub fn #create_service_name(service: Arc<Box<dyn #service_trait + Send + Sync>>) -> HashMap<String, Box<dyn #mod_path::MethodHandler + Send + Sync>> {
                let mut methods = HashMap::new();
                #(#method_inserts)*
                methods
            }
        )
    }

    fn async_creating_service_method_token(&self, service: &Service) -> TokenStream {
        let create_service_name = format_ident!("create_{}", self.service_name_rust(service));
        let service_trait = format_ident!("{}", service.name);
        let mod_path = ttrpc_mod();
        let stream_token = if self.has_stream_method(service) {
            quote!( let mut streams = HashMap::new(); )
        } else {
            quote!( let streams = HashMap::new(); )
        };
        let method_inserts: Vec<_> = service.methods.iter().map(|method| {
            let key = format!("{}", method.proto_name);
            let mm = format_ident!("{}Method", to_camel_case(&method.proto_name));
            match MethodType::from_method(method) {
                MethodType::Unary => {
                    quote!(
                        methods.insert(
                            #key.to_string(),
                            Box::new(#mm{service: service.clone()}) as Box<dyn #mod_path::r#async::MethodHandler + Send + Sync>); 
                    )
                },
                _ => {
                    quote!(
                        streams.insert(
                            #key.to_string(),
                            Arc::new(#mm{service: service.clone()}) as Arc<dyn #mod_path::r#async::StreamHandler + Send + Sync>); 
                    )
                }
            }
        }).collect();
        let service_path = format!("{}.{}", service.package, to_camel_case(&service.proto_name));

        quote!(
            pub fn #create_service_name(service: Arc<Box<dyn #service_trait + Send + Sync>>) -> HashMap<String, #mod_path::r#async::Service> {
                let mut ret = HashMap::new();
                let mut methods = HashMap::new();
                #stream_token
                #(#method_inserts)*
                ret.insert(#service_path.to_string(), #mod_path::r#async::Service{ methods, streams });
                ret
            }
        )
    }

    fn generate_client(&self, service: &Service, buf: &mut String) {
        let client_struct = self.client_sturct_token(service);
        let client_methods = self.client_methods_token(service);

        let client_token = quote!(
            #client_struct
            #client_methods
        );
        buf.push_str(&client_token.to_string());
    }

    fn client_sturct_token(&self, service: &Service) -> TokenStream {
        let client_type = format_ident!("{}", self.client_type(service));
        let mod_path = ttrpc_mod();
        let client_field_type = if async_on(self.async_mode, Side::Client) {
            quote!( #mod_path::r#async::Client )
        } else {
            quote!( #mod_path::Client )
        };

        quote!(
            #[derive(Clone)]
            pub struct #client_type {
                client: #client_field_type,
            }

            impl #client_type {
                pub fn new(client: #client_field_type) -> Self {
                    #client_type {
                        client,
                    }
                }
            }
        )
    }

    fn client_methods_token(&self, service: &Service) -> TokenStream {
        let client_type = format_ident!("{}", self.client_type(service));
        let methods: Vec<_> = service
            .methods
            .iter()
            .map(|method| {
                if async_on(self.async_mode, Side::Client) {
                    self.async_client_method_token(service, method)
                } else {
                    self.sync_client_method_token(service, method)
                }
            })
            .collect();

        quote!(
            impl #client_type {
                #(#methods)*
            }
        )
    }

    fn sync_client_method_token(&self, service: &Service, method: &Method) -> TokenStream {
        let method_name = format_ident!("{}", method.name);
        let mod_path = ttrpc_mod();
        let input = type_token(&method.input_type);
        let output = type_token(&method.output_type);
        let server_str = format!("{}.{}", service.package, service.name);
        let method_str = format!("{}", method.proto_name);

        match MethodType::from_method(method) {
            MethodType::Unary => {
                quote!(
                    pub fn #method_name(&self, ctx: #mod_path::context::Context, req: &#input) -> #mod_path::Result<#output> {
                        let mut cres = #output::default();
                        #mod_path::client_request!(self, ctx, req, #server_str, #method_str, cres);
                        Ok(cres)
                    }
                )
            }
            _ => {
                panic!("Reaching here is prohibited.")
            }
        }
    }

    fn async_client_method_token(&self, service: &Service, method: &Method) -> TokenStream {
        let method_name = format_ident!("{}", method.name);
        let mod_path = ttrpc_mod();
        let input = type_token(&method.input_type);
        let output = type_token(&method.output_type);
        let server_str = format!("{}.{}", service.package, service.name);
        let method_str = format!("{}", method.proto_name);

        let (mut arg_tokens, ret_token, body_token) = match MethodType::from_method(method) {
            MethodType::Unary => (
                vec![quote!(req: &#input)],
                quote!( #mod_path::Result<#output> ),
                quote!(
                    let mut cres = #output::default();
                    #mod_path::async_client_request!(self, ctx, req, #server_str, #method_str, cres);
                ),
            ),
            MethodType::ClientStreaming => (
                vec![],
                quote!( #mod_path::Result<#mod_path::r#async::ClientStreamSender<#input, #output>> ),
                quote!( ::ttrpc::async_client_stream_send!(self, ctx, #server_str, #method_str); ),
            ),
            MethodType::ServerStreaming => (
                vec![quote!( req: &#input )],
                quote!( #mod_path::Result<#mod_path::r#async::ClientStreamReceiver<#output>> ),
                quote!( #mod_path::async_client_stream_receive!(self, ctx, req, #server_str, #method_str); ),
            ),
            MethodType::Duplex => (
                vec![],
                quote!( #mod_path::Result<#mod_path::r#async::ClientStream<#input, #output>> ),
                quote!( ::ttrpc::async_client_stream!(self, ctx, #server_str, #method_str); ),
            ),
        };

        let mut args = vec![quote!(&self), quote!( ctx: #mod_path::context::Context )];
        args.append(&mut arg_tokens);

        quote!(
            pub async fn #method_name(#(#args),*) -> #ret_token {
                #body_token
            }
        )
    }
}

// Utils
impl TtrpcServiceGenerator {
    /// Generate a token stream of the TtrpcContext
    fn ttrpc_context(&self, r#async: bool) -> TokenStream {
        let mod_path = ttrpc_mod();
        if r#async {
            quote!( #mod_path::r#async::TtrpcContext )
        } else {
            quote!( #mod_path::TtrpcContext )
        }
    }

    fn service_name_rust(&self, service: &Service) -> String {
        to_snake_case(&service.name)
    }

    fn method_name_rust(&self, method: &Method) -> String {
        to_snake_case(&method.name)
    }

    fn client_type(&self, service: &Service) -> String {
        format!("{}Client", service.name)
    }

    fn has_stream_method(&self, service: &Service) -> bool {
        service
            .methods
            .iter()
            .any(|method| !matches!(MethodType::from_method(method), MethodType::Unary))
    }
}

pub enum MethodType {
    Unary,
    ClientStreaming,
    ServerStreaming,
    Duplex,
}

impl fmt::Display for MethodType {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "{}",
            match self {
                MethodType::Unary => "MethodType::Unary",
                MethodType::ClientStreaming => "MethodType::ClientStreaming",
                MethodType::ServerStreaming => "MethodType::ServerStreaming",
                MethodType::Duplex => "MethodType::Duplex",
            }
        )
    }
}

impl MethodType {
    pub fn from_method(method: &Method) -> Self {
        match (method.client_streaming, method.server_streaming) {
            (false, false) => MethodType::Unary,
            (true, false) => MethodType::ClientStreaming,
            (false, true) => MethodType::ServerStreaming,
            (true, true) => MethodType::Duplex,
        }
    }
}

#[derive(PartialEq, Clone, Copy)]
pub enum AsyncMode {
    /// Both client and server are async.
    All,
    /// Only client is async.
    Client,
    /// Only server is async.
    Server,
    /// None of client and server are async, it is the default value.
    None,
}

#[derive(PartialEq)]
/// Indicated the service side
enum Side {
    Client,
    Server,
}

fn async_on(mode: AsyncMode, side: Side) -> bool {
    return mode == AsyncMode::All
        || (side == Side::Server && mode == AsyncMode::Server)
        || (side == Side::Client && mode == AsyncMode::Client);
}
