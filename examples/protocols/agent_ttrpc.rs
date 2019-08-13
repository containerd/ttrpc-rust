use protobuf::{CodedInputStream, CodedOutputStream, Message};
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Clone)]
pub struct AgentServiceClient {
    client: ::ttrpc::Client,
}

impl AgentServiceClient {
    pub fn new(client: ::ttrpc::Client) -> Self {
        AgentServiceClient {
            client: client,
        }
    }

    pub fn create_container(&self, req: &super::agent::CreateContainerRequest, timeout_nano: i64) -> ::ttrpc::Result<super::empty::Empty> {
        let mut cres = super::empty::Empty::new();
        ::ttrpc::client_request!(self, req, timeout_nano, "grpc.AgentService", "CreateContainer", cres);
        Ok(cres)
    }

    pub fn start_container(&self, req: &super::agent::StartContainerRequest, timeout_nano: i64) -> ::ttrpc::Result<super::empty::Empty> {
        let mut cres = super::empty::Empty::new();
        ::ttrpc::client_request!(self, req, timeout_nano, "grpc.AgentService", "StartContainer", cres);
        Ok(cres)
    }

    pub fn remove_container(&self, req: &super::agent::RemoveContainerRequest, timeout_nano: i64) -> ::ttrpc::Result<super::empty::Empty> {
        let mut cres = super::empty::Empty::new();
        ::ttrpc::client_request!(self, req, timeout_nano, "grpc.AgentService", "RemoveContainer", cres);
        Ok(cres)
    }

    pub fn exec_process(&self, req: &super::agent::ExecProcessRequest, timeout_nano: i64) -> ::ttrpc::Result<super::empty::Empty> {
        let mut cres = super::empty::Empty::new();
        ::ttrpc::client_request!(self, req, timeout_nano, "grpc.AgentService", "ExecProcess", cres);
        Ok(cres)
    }

    pub fn signal_process(&self, req: &super::agent::SignalProcessRequest, timeout_nano: i64) -> ::ttrpc::Result<super::empty::Empty> {
        let mut cres = super::empty::Empty::new();
        ::ttrpc::client_request!(self, req, timeout_nano, "grpc.AgentService", "SignalProcess", cres);
        Ok(cres)
    }

    pub fn wait_process(&self, req: &super::agent::WaitProcessRequest, timeout_nano: i64) -> ::ttrpc::Result<super::agent::WaitProcessResponse> {
        let mut cres = super::agent::WaitProcessResponse::new();
        ::ttrpc::client_request!(self, req, timeout_nano, "grpc.AgentService", "WaitProcess", cres);
        Ok(cres)
    }

    pub fn list_processes(&self, req: &super::agent::ListProcessesRequest, timeout_nano: i64) -> ::ttrpc::Result<super::agent::ListProcessesResponse> {
        let mut cres = super::agent::ListProcessesResponse::new();
        ::ttrpc::client_request!(self, req, timeout_nano, "grpc.AgentService", "ListProcesses", cres);
        Ok(cres)
    }

    pub fn update_container(&self, req: &super::agent::UpdateContainerRequest, timeout_nano: i64) -> ::ttrpc::Result<super::empty::Empty> {
        let mut cres = super::empty::Empty::new();
        ::ttrpc::client_request!(self, req, timeout_nano, "grpc.AgentService", "UpdateContainer", cres);
        Ok(cres)
    }

    pub fn stats_container(&self, req: &super::agent::StatsContainerRequest, timeout_nano: i64) -> ::ttrpc::Result<super::agent::StatsContainerResponse> {
        let mut cres = super::agent::StatsContainerResponse::new();
        ::ttrpc::client_request!(self, req, timeout_nano, "grpc.AgentService", "StatsContainer", cres);
        Ok(cres)
    }

    pub fn pause_container(&self, req: &super::agent::PauseContainerRequest, timeout_nano: i64) -> ::ttrpc::Result<super::empty::Empty> {
        let mut cres = super::empty::Empty::new();
        ::ttrpc::client_request!(self, req, timeout_nano, "grpc.AgentService", "PauseContainer", cres);
        Ok(cres)
    }

    pub fn resume_container(&self, req: &super::agent::ResumeContainerRequest, timeout_nano: i64) -> ::ttrpc::Result<super::empty::Empty> {
        let mut cres = super::empty::Empty::new();
        ::ttrpc::client_request!(self, req, timeout_nano, "grpc.AgentService", "ResumeContainer", cres);
        Ok(cres)
    }

    pub fn write_stdin(&self, req: &super::agent::WriteStreamRequest, timeout_nano: i64) -> ::ttrpc::Result<super::agent::WriteStreamResponse> {
        let mut cres = super::agent::WriteStreamResponse::new();
        ::ttrpc::client_request!(self, req, timeout_nano, "grpc.AgentService", "WriteStdin", cres);
        Ok(cres)
    }

    pub fn read_stdout(&self, req: &super::agent::ReadStreamRequest, timeout_nano: i64) -> ::ttrpc::Result<super::agent::ReadStreamResponse> {
        let mut cres = super::agent::ReadStreamResponse::new();
        ::ttrpc::client_request!(self, req, timeout_nano, "grpc.AgentService", "ReadStdout", cres);
        Ok(cres)
    }

    pub fn read_stderr(&self, req: &super::agent::ReadStreamRequest, timeout_nano: i64) -> ::ttrpc::Result<super::agent::ReadStreamResponse> {
        let mut cres = super::agent::ReadStreamResponse::new();
        ::ttrpc::client_request!(self, req, timeout_nano, "grpc.AgentService", "ReadStderr", cres);
        Ok(cres)
    }

    pub fn close_stdin(&self, req: &super::agent::CloseStdinRequest, timeout_nano: i64) -> ::ttrpc::Result<super::empty::Empty> {
        let mut cres = super::empty::Empty::new();
        ::ttrpc::client_request!(self, req, timeout_nano, "grpc.AgentService", "CloseStdin", cres);
        Ok(cres)
    }

    pub fn tty_win_resize(&self, req: &super::agent::TtyWinResizeRequest, timeout_nano: i64) -> ::ttrpc::Result<super::empty::Empty> {
        let mut cres = super::empty::Empty::new();
        ::ttrpc::client_request!(self, req, timeout_nano, "grpc.AgentService", "TtyWinResize", cres);
        Ok(cres)
    }

    pub fn update_interface(&self, req: &super::agent::UpdateInterfaceRequest, timeout_nano: i64) -> ::ttrpc::Result<super::types::Interface> {
        let mut cres = super::types::Interface::new();
        ::ttrpc::client_request!(self, req, timeout_nano, "grpc.AgentService", "UpdateInterface", cres);
        Ok(cres)
    }

    pub fn update_routes(&self, req: &super::agent::UpdateRoutesRequest, timeout_nano: i64) -> ::ttrpc::Result<super::agent::Routes> {
        let mut cres = super::agent::Routes::new();
        ::ttrpc::client_request!(self, req, timeout_nano, "grpc.AgentService", "UpdateRoutes", cres);
        Ok(cres)
    }

    pub fn list_interfaces(&self, req: &super::agent::ListInterfacesRequest, timeout_nano: i64) -> ::ttrpc::Result<super::agent::Interfaces> {
        let mut cres = super::agent::Interfaces::new();
        ::ttrpc::client_request!(self, req, timeout_nano, "grpc.AgentService", "ListInterfaces", cres);
        Ok(cres)
    }

    pub fn list_routes(&self, req: &super::agent::ListRoutesRequest, timeout_nano: i64) -> ::ttrpc::Result<super::agent::Routes> {
        let mut cres = super::agent::Routes::new();
        ::ttrpc::client_request!(self, req, timeout_nano, "grpc.AgentService", "ListRoutes", cres);
        Ok(cres)
    }

    pub fn start_tracing(&self, req: &super::agent::StartTracingRequest, timeout_nano: i64) -> ::ttrpc::Result<super::empty::Empty> {
        let mut cres = super::empty::Empty::new();
        ::ttrpc::client_request!(self, req, timeout_nano, "grpc.AgentService", "StartTracing", cres);
        Ok(cres)
    }

    pub fn stop_tracing(&self, req: &super::agent::StopTracingRequest, timeout_nano: i64) -> ::ttrpc::Result<super::empty::Empty> {
        let mut cres = super::empty::Empty::new();
        ::ttrpc::client_request!(self, req, timeout_nano, "grpc.AgentService", "StopTracing", cres);
        Ok(cres)
    }

    pub fn create_sandbox(&self, req: &super::agent::CreateSandboxRequest, timeout_nano: i64) -> ::ttrpc::Result<super::empty::Empty> {
        let mut cres = super::empty::Empty::new();
        ::ttrpc::client_request!(self, req, timeout_nano, "grpc.AgentService", "CreateSandbox", cres);
        Ok(cres)
    }

    pub fn destroy_sandbox(&self, req: &super::agent::DestroySandboxRequest, timeout_nano: i64) -> ::ttrpc::Result<super::empty::Empty> {
        let mut cres = super::empty::Empty::new();
        ::ttrpc::client_request!(self, req, timeout_nano, "grpc.AgentService", "DestroySandbox", cres);
        Ok(cres)
    }

    pub fn online_cpu_mem(&self, req: &super::agent::OnlineCPUMemRequest, timeout_nano: i64) -> ::ttrpc::Result<super::empty::Empty> {
        let mut cres = super::empty::Empty::new();
        ::ttrpc::client_request!(self, req, timeout_nano, "grpc.AgentService", "OnlineCPUMem", cres);
        Ok(cres)
    }

    pub fn reseed_random_dev(&self, req: &super::agent::ReseedRandomDevRequest, timeout_nano: i64) -> ::ttrpc::Result<super::empty::Empty> {
        let mut cres = super::empty::Empty::new();
        ::ttrpc::client_request!(self, req, timeout_nano, "grpc.AgentService", "ReseedRandomDev", cres);
        Ok(cres)
    }

    pub fn get_guest_details(&self, req: &super::agent::GuestDetailsRequest, timeout_nano: i64) -> ::ttrpc::Result<super::agent::GuestDetailsResponse> {
        let mut cres = super::agent::GuestDetailsResponse::new();
        ::ttrpc::client_request!(self, req, timeout_nano, "grpc.AgentService", "GetGuestDetails", cres);
        Ok(cres)
    }

    pub fn mem_hotplug_by_probe(&self, req: &super::agent::MemHotplugByProbeRequest, timeout_nano: i64) -> ::ttrpc::Result<super::empty::Empty> {
        let mut cres = super::empty::Empty::new();
        ::ttrpc::client_request!(self, req, timeout_nano, "grpc.AgentService", "MemHotplugByProbe", cres);
        Ok(cres)
    }

    pub fn set_guest_date_time(&self, req: &super::agent::SetGuestDateTimeRequest, timeout_nano: i64) -> ::ttrpc::Result<super::empty::Empty> {
        let mut cres = super::empty::Empty::new();
        ::ttrpc::client_request!(self, req, timeout_nano, "grpc.AgentService", "SetGuestDateTime", cres);
        Ok(cres)
    }

    pub fn copy_file(&self, req: &super::agent::CopyFileRequest, timeout_nano: i64) -> ::ttrpc::Result<super::empty::Empty> {
        let mut cres = super::empty::Empty::new();
        ::ttrpc::client_request!(self, req, timeout_nano, "grpc.AgentService", "CopyFile", cres);
        Ok(cres)
    }
}

struct create_container_method {
    service: Arc<std::boxed::Box<AgentService + Send + Sync>>,
}

impl ::ttrpc::MethodHandler for create_container_method {
    fn handler(&self, ctx: ::ttrpc::TtrpcContext, req: ::ttrpc::Request) -> ::ttrpc::Result<()> {
        ::ttrpc::request_handler!(self, ctx, req, agent, CreateContainerRequest, create_container);
        Ok(())
    }
}

struct start_container_method {
    service: Arc<std::boxed::Box<AgentService + Send + Sync>>,
}

impl ::ttrpc::MethodHandler for start_container_method {
    fn handler(&self, ctx: ::ttrpc::TtrpcContext, req: ::ttrpc::Request) -> ::ttrpc::Result<()> {
        ::ttrpc::request_handler!(self, ctx, req, agent, StartContainerRequest, start_container);
        Ok(())
    }
}

struct remove_container_method {
    service: Arc<std::boxed::Box<AgentService + Send + Sync>>,
}

impl ::ttrpc::MethodHandler for remove_container_method {
    fn handler(&self, ctx: ::ttrpc::TtrpcContext, req: ::ttrpc::Request) -> ::ttrpc::Result<()> {
        ::ttrpc::request_handler!(self, ctx, req, agent, RemoveContainerRequest, remove_container);
        Ok(())
    }
}

struct exec_process_method {
    service: Arc<std::boxed::Box<AgentService + Send + Sync>>,
}

impl ::ttrpc::MethodHandler for exec_process_method {
    fn handler(&self, ctx: ::ttrpc::TtrpcContext, req: ::ttrpc::Request) -> ::ttrpc::Result<()> {
        ::ttrpc::request_handler!(self, ctx, req, agent, ExecProcessRequest, exec_process);
        Ok(())
    }
}

struct signal_process_method {
    service: Arc<std::boxed::Box<AgentService + Send + Sync>>,
}

impl ::ttrpc::MethodHandler for signal_process_method {
    fn handler(&self, ctx: ::ttrpc::TtrpcContext, req: ::ttrpc::Request) -> ::ttrpc::Result<()> {
        ::ttrpc::request_handler!(self, ctx, req, agent, SignalProcessRequest, signal_process);
        Ok(())
    }
}

struct wait_process_method {
    service: Arc<std::boxed::Box<AgentService + Send + Sync>>,
}

impl ::ttrpc::MethodHandler for wait_process_method {
    fn handler(&self, ctx: ::ttrpc::TtrpcContext, req: ::ttrpc::Request) -> ::ttrpc::Result<()> {
        ::ttrpc::request_handler!(self, ctx, req, agent, WaitProcessRequest, wait_process);
        Ok(())
    }
}

struct list_processes_method {
    service: Arc<std::boxed::Box<AgentService + Send + Sync>>,
}

impl ::ttrpc::MethodHandler for list_processes_method {
    fn handler(&self, ctx: ::ttrpc::TtrpcContext, req: ::ttrpc::Request) -> ::ttrpc::Result<()> {
        ::ttrpc::request_handler!(self, ctx, req, agent, ListProcessesRequest, list_processes);
        Ok(())
    }
}

struct update_container_method {
    service: Arc<std::boxed::Box<AgentService + Send + Sync>>,
}

impl ::ttrpc::MethodHandler for update_container_method {
    fn handler(&self, ctx: ::ttrpc::TtrpcContext, req: ::ttrpc::Request) -> ::ttrpc::Result<()> {
        ::ttrpc::request_handler!(self, ctx, req, agent, UpdateContainerRequest, update_container);
        Ok(())
    }
}

struct stats_container_method {
    service: Arc<std::boxed::Box<AgentService + Send + Sync>>,
}

impl ::ttrpc::MethodHandler for stats_container_method {
    fn handler(&self, ctx: ::ttrpc::TtrpcContext, req: ::ttrpc::Request) -> ::ttrpc::Result<()> {
        ::ttrpc::request_handler!(self, ctx, req, agent, StatsContainerRequest, stats_container);
        Ok(())
    }
}

struct pause_container_method {
    service: Arc<std::boxed::Box<AgentService + Send + Sync>>,
}

impl ::ttrpc::MethodHandler for pause_container_method {
    fn handler(&self, ctx: ::ttrpc::TtrpcContext, req: ::ttrpc::Request) -> ::ttrpc::Result<()> {
        ::ttrpc::request_handler!(self, ctx, req, agent, PauseContainerRequest, pause_container);
        Ok(())
    }
}

struct resume_container_method {
    service: Arc<std::boxed::Box<AgentService + Send + Sync>>,
}

impl ::ttrpc::MethodHandler for resume_container_method {
    fn handler(&self, ctx: ::ttrpc::TtrpcContext, req: ::ttrpc::Request) -> ::ttrpc::Result<()> {
        ::ttrpc::request_handler!(self, ctx, req, agent, ResumeContainerRequest, resume_container);
        Ok(())
    }
}

struct write_stdin_method {
    service: Arc<std::boxed::Box<AgentService + Send + Sync>>,
}

impl ::ttrpc::MethodHandler for write_stdin_method {
    fn handler(&self, ctx: ::ttrpc::TtrpcContext, req: ::ttrpc::Request) -> ::ttrpc::Result<()> {
        ::ttrpc::request_handler!(self, ctx, req, agent, WriteStreamRequest, write_stdin);
        Ok(())
    }
}

struct read_stdout_method {
    service: Arc<std::boxed::Box<AgentService + Send + Sync>>,
}

impl ::ttrpc::MethodHandler for read_stdout_method {
    fn handler(&self, ctx: ::ttrpc::TtrpcContext, req: ::ttrpc::Request) -> ::ttrpc::Result<()> {
        ::ttrpc::request_handler!(self, ctx, req, agent, ReadStreamRequest, read_stdout);
        Ok(())
    }
}

struct read_stderr_method {
    service: Arc<std::boxed::Box<AgentService + Send + Sync>>,
}

impl ::ttrpc::MethodHandler for read_stderr_method {
    fn handler(&self, ctx: ::ttrpc::TtrpcContext, req: ::ttrpc::Request) -> ::ttrpc::Result<()> {
        ::ttrpc::request_handler!(self, ctx, req, agent, ReadStreamRequest, read_stderr);
        Ok(())
    }
}

struct close_stdin_method {
    service: Arc<std::boxed::Box<AgentService + Send + Sync>>,
}

impl ::ttrpc::MethodHandler for close_stdin_method {
    fn handler(&self, ctx: ::ttrpc::TtrpcContext, req: ::ttrpc::Request) -> ::ttrpc::Result<()> {
        ::ttrpc::request_handler!(self, ctx, req, agent, CloseStdinRequest, close_stdin);
        Ok(())
    }
}

struct tty_win_resize_method {
    service: Arc<std::boxed::Box<AgentService + Send + Sync>>,
}

impl ::ttrpc::MethodHandler for tty_win_resize_method {
    fn handler(&self, ctx: ::ttrpc::TtrpcContext, req: ::ttrpc::Request) -> ::ttrpc::Result<()> {
        ::ttrpc::request_handler!(self, ctx, req, agent, TtyWinResizeRequest, tty_win_resize);
        Ok(())
    }
}

struct update_interface_method {
    service: Arc<std::boxed::Box<AgentService + Send + Sync>>,
}

impl ::ttrpc::MethodHandler for update_interface_method {
    fn handler(&self, ctx: ::ttrpc::TtrpcContext, req: ::ttrpc::Request) -> ::ttrpc::Result<()> {
        ::ttrpc::request_handler!(self, ctx, req, agent, UpdateInterfaceRequest, update_interface);
        Ok(())
    }
}

struct update_routes_method {
    service: Arc<std::boxed::Box<AgentService + Send + Sync>>,
}

impl ::ttrpc::MethodHandler for update_routes_method {
    fn handler(&self, ctx: ::ttrpc::TtrpcContext, req: ::ttrpc::Request) -> ::ttrpc::Result<()> {
        ::ttrpc::request_handler!(self, ctx, req, agent, UpdateRoutesRequest, update_routes);
        Ok(())
    }
}

struct list_interfaces_method {
    service: Arc<std::boxed::Box<AgentService + Send + Sync>>,
}

impl ::ttrpc::MethodHandler for list_interfaces_method {
    fn handler(&self, ctx: ::ttrpc::TtrpcContext, req: ::ttrpc::Request) -> ::ttrpc::Result<()> {
        ::ttrpc::request_handler!(self, ctx, req, agent, ListInterfacesRequest, list_interfaces);
        Ok(())
    }
}

struct list_routes_method {
    service: Arc<std::boxed::Box<AgentService + Send + Sync>>,
}

impl ::ttrpc::MethodHandler for list_routes_method {
    fn handler(&self, ctx: ::ttrpc::TtrpcContext, req: ::ttrpc::Request) -> ::ttrpc::Result<()> {
        ::ttrpc::request_handler!(self, ctx, req, agent, ListRoutesRequest, list_routes);
        Ok(())
    }
}

struct start_tracing_method {
    service: Arc<std::boxed::Box<AgentService + Send + Sync>>,
}

impl ::ttrpc::MethodHandler for start_tracing_method {
    fn handler(&self, ctx: ::ttrpc::TtrpcContext, req: ::ttrpc::Request) -> ::ttrpc::Result<()> {
        ::ttrpc::request_handler!(self, ctx, req, agent, StartTracingRequest, start_tracing);
        Ok(())
    }
}

struct stop_tracing_method {
    service: Arc<std::boxed::Box<AgentService + Send + Sync>>,
}

impl ::ttrpc::MethodHandler for stop_tracing_method {
    fn handler(&self, ctx: ::ttrpc::TtrpcContext, req: ::ttrpc::Request) -> ::ttrpc::Result<()> {
        ::ttrpc::request_handler!(self, ctx, req, agent, StopTracingRequest, stop_tracing);
        Ok(())
    }
}

struct create_sandbox_method {
    service: Arc<std::boxed::Box<AgentService + Send + Sync>>,
}

impl ::ttrpc::MethodHandler for create_sandbox_method {
    fn handler(&self, ctx: ::ttrpc::TtrpcContext, req: ::ttrpc::Request) -> ::ttrpc::Result<()> {
        ::ttrpc::request_handler!(self, ctx, req, agent, CreateSandboxRequest, create_sandbox);
        Ok(())
    }
}

struct destroy_sandbox_method {
    service: Arc<std::boxed::Box<AgentService + Send + Sync>>,
}

impl ::ttrpc::MethodHandler for destroy_sandbox_method {
    fn handler(&self, ctx: ::ttrpc::TtrpcContext, req: ::ttrpc::Request) -> ::ttrpc::Result<()> {
        ::ttrpc::request_handler!(self, ctx, req, agent, DestroySandboxRequest, destroy_sandbox);
        Ok(())
    }
}

struct online_cpu_mem_method {
    service: Arc<std::boxed::Box<AgentService + Send + Sync>>,
}

impl ::ttrpc::MethodHandler for online_cpu_mem_method {
    fn handler(&self, ctx: ::ttrpc::TtrpcContext, req: ::ttrpc::Request) -> ::ttrpc::Result<()> {
        ::ttrpc::request_handler!(self, ctx, req, agent, OnlineCPUMemRequest, online_cpu_mem);
        Ok(())
    }
}

struct reseed_random_dev_method {
    service: Arc<std::boxed::Box<AgentService + Send + Sync>>,
}

impl ::ttrpc::MethodHandler for reseed_random_dev_method {
    fn handler(&self, ctx: ::ttrpc::TtrpcContext, req: ::ttrpc::Request) -> ::ttrpc::Result<()> {
        ::ttrpc::request_handler!(self, ctx, req, agent, ReseedRandomDevRequest, reseed_random_dev);
        Ok(())
    }
}

struct get_guest_details_method {
    service: Arc<std::boxed::Box<AgentService + Send + Sync>>,
}

impl ::ttrpc::MethodHandler for get_guest_details_method {
    fn handler(&self, ctx: ::ttrpc::TtrpcContext, req: ::ttrpc::Request) -> ::ttrpc::Result<()> {
        ::ttrpc::request_handler!(self, ctx, req, agent, GuestDetailsRequest, get_guest_details);
        Ok(())
    }
}

struct mem_hotplug_by_probe_method {
    service: Arc<std::boxed::Box<AgentService + Send + Sync>>,
}

impl ::ttrpc::MethodHandler for mem_hotplug_by_probe_method {
    fn handler(&self, ctx: ::ttrpc::TtrpcContext, req: ::ttrpc::Request) -> ::ttrpc::Result<()> {
        ::ttrpc::request_handler!(self, ctx, req, agent, MemHotplugByProbeRequest, mem_hotplug_by_probe);
        Ok(())
    }
}

struct set_guest_date_time_method {
    service: Arc<std::boxed::Box<AgentService + Send + Sync>>,
}

impl ::ttrpc::MethodHandler for set_guest_date_time_method {
    fn handler(&self, ctx: ::ttrpc::TtrpcContext, req: ::ttrpc::Request) -> ::ttrpc::Result<()> {
        ::ttrpc::request_handler!(self, ctx, req, agent, SetGuestDateTimeRequest, set_guest_date_time);
        Ok(())
    }
}

struct copy_file_method {
    service: Arc<std::boxed::Box<AgentService + Send + Sync>>,
}

impl ::ttrpc::MethodHandler for copy_file_method {
    fn handler(&self, ctx: ::ttrpc::TtrpcContext, req: ::ttrpc::Request) -> ::ttrpc::Result<()> {
        ::ttrpc::request_handler!(self, ctx, req, agent, CopyFileRequest, copy_file);
        Ok(())
    }
}

pub trait AgentService {
    fn create_container(&self, ctx: &::ttrpc::TtrpcContext, req: super::agent::CreateContainerRequest) -> ::ttrpc::Result<super::empty::Empty> {
        Err(::ttrpc::Error::RpcStatus(::ttrpc::get_Status(::ttrpc::Code::NOT_FOUND, "/grpc.AgentService/CreateContainer is not supported".to_string())))
    }
    fn start_container(&self, ctx: &::ttrpc::TtrpcContext, req: super::agent::StartContainerRequest) -> ::ttrpc::Result<super::empty::Empty> {
        Err(::ttrpc::Error::RpcStatus(::ttrpc::get_Status(::ttrpc::Code::NOT_FOUND, "/grpc.AgentService/StartContainer is not supported".to_string())))
    }
    fn remove_container(&self, ctx: &::ttrpc::TtrpcContext, req: super::agent::RemoveContainerRequest) -> ::ttrpc::Result<super::empty::Empty> {
        Err(::ttrpc::Error::RpcStatus(::ttrpc::get_Status(::ttrpc::Code::NOT_FOUND, "/grpc.AgentService/RemoveContainer is not supported".to_string())))
    }
    fn exec_process(&self, ctx: &::ttrpc::TtrpcContext, req: super::agent::ExecProcessRequest) -> ::ttrpc::Result<super::empty::Empty> {
        Err(::ttrpc::Error::RpcStatus(::ttrpc::get_Status(::ttrpc::Code::NOT_FOUND, "/grpc.AgentService/ExecProcess is not supported".to_string())))
    }
    fn signal_process(&self, ctx: &::ttrpc::TtrpcContext, req: super::agent::SignalProcessRequest) -> ::ttrpc::Result<super::empty::Empty> {
        Err(::ttrpc::Error::RpcStatus(::ttrpc::get_Status(::ttrpc::Code::NOT_FOUND, "/grpc.AgentService/SignalProcess is not supported".to_string())))
    }
    fn wait_process(&self, ctx: &::ttrpc::TtrpcContext, req: super::agent::WaitProcessRequest) -> ::ttrpc::Result<super::agent::WaitProcessResponse> {
        Err(::ttrpc::Error::RpcStatus(::ttrpc::get_Status(::ttrpc::Code::NOT_FOUND, "/grpc.AgentService/WaitProcess is not supported".to_string())))
    }
    fn list_processes(&self, ctx: &::ttrpc::TtrpcContext, req: super::agent::ListProcessesRequest) -> ::ttrpc::Result<super::agent::ListProcessesResponse> {
        Err(::ttrpc::Error::RpcStatus(::ttrpc::get_Status(::ttrpc::Code::NOT_FOUND, "/grpc.AgentService/ListProcesses is not supported".to_string())))
    }
    fn update_container(&self, ctx: &::ttrpc::TtrpcContext, req: super::agent::UpdateContainerRequest) -> ::ttrpc::Result<super::empty::Empty> {
        Err(::ttrpc::Error::RpcStatus(::ttrpc::get_Status(::ttrpc::Code::NOT_FOUND, "/grpc.AgentService/UpdateContainer is not supported".to_string())))
    }
    fn stats_container(&self, ctx: &::ttrpc::TtrpcContext, req: super::agent::StatsContainerRequest) -> ::ttrpc::Result<super::agent::StatsContainerResponse> {
        Err(::ttrpc::Error::RpcStatus(::ttrpc::get_Status(::ttrpc::Code::NOT_FOUND, "/grpc.AgentService/StatsContainer is not supported".to_string())))
    }
    fn pause_container(&self, ctx: &::ttrpc::TtrpcContext, req: super::agent::PauseContainerRequest) -> ::ttrpc::Result<super::empty::Empty> {
        Err(::ttrpc::Error::RpcStatus(::ttrpc::get_Status(::ttrpc::Code::NOT_FOUND, "/grpc.AgentService/PauseContainer is not supported".to_string())))
    }
    fn resume_container(&self, ctx: &::ttrpc::TtrpcContext, req: super::agent::ResumeContainerRequest) -> ::ttrpc::Result<super::empty::Empty> {
        Err(::ttrpc::Error::RpcStatus(::ttrpc::get_Status(::ttrpc::Code::NOT_FOUND, "/grpc.AgentService/ResumeContainer is not supported".to_string())))
    }
    fn write_stdin(&self, ctx: &::ttrpc::TtrpcContext, req: super::agent::WriteStreamRequest) -> ::ttrpc::Result<super::agent::WriteStreamResponse> {
        Err(::ttrpc::Error::RpcStatus(::ttrpc::get_Status(::ttrpc::Code::NOT_FOUND, "/grpc.AgentService/WriteStdin is not supported".to_string())))
    }
    fn read_stdout(&self, ctx: &::ttrpc::TtrpcContext, req: super::agent::ReadStreamRequest) -> ::ttrpc::Result<super::agent::ReadStreamResponse> {
        Err(::ttrpc::Error::RpcStatus(::ttrpc::get_Status(::ttrpc::Code::NOT_FOUND, "/grpc.AgentService/ReadStdout is not supported".to_string())))
    }
    fn read_stderr(&self, ctx: &::ttrpc::TtrpcContext, req: super::agent::ReadStreamRequest) -> ::ttrpc::Result<super::agent::ReadStreamResponse> {
        Err(::ttrpc::Error::RpcStatus(::ttrpc::get_Status(::ttrpc::Code::NOT_FOUND, "/grpc.AgentService/ReadStderr is not supported".to_string())))
    }
    fn close_stdin(&self, ctx: &::ttrpc::TtrpcContext, req: super::agent::CloseStdinRequest) -> ::ttrpc::Result<super::empty::Empty> {
        Err(::ttrpc::Error::RpcStatus(::ttrpc::get_Status(::ttrpc::Code::NOT_FOUND, "/grpc.AgentService/CloseStdin is not supported".to_string())))
    }
    fn tty_win_resize(&self, ctx: &::ttrpc::TtrpcContext, req: super::agent::TtyWinResizeRequest) -> ::ttrpc::Result<super::empty::Empty> {
        Err(::ttrpc::Error::RpcStatus(::ttrpc::get_Status(::ttrpc::Code::NOT_FOUND, "/grpc.AgentService/TtyWinResize is not supported".to_string())))
    }
    fn update_interface(&self, ctx: &::ttrpc::TtrpcContext, req: super::agent::UpdateInterfaceRequest) -> ::ttrpc::Result<super::types::Interface> {
        Err(::ttrpc::Error::RpcStatus(::ttrpc::get_Status(::ttrpc::Code::NOT_FOUND, "/grpc.AgentService/UpdateInterface is not supported".to_string())))
    }
    fn update_routes(&self, ctx: &::ttrpc::TtrpcContext, req: super::agent::UpdateRoutesRequest) -> ::ttrpc::Result<super::agent::Routes> {
        Err(::ttrpc::Error::RpcStatus(::ttrpc::get_Status(::ttrpc::Code::NOT_FOUND, "/grpc.AgentService/UpdateRoutes is not supported".to_string())))
    }
    fn list_interfaces(&self, ctx: &::ttrpc::TtrpcContext, req: super::agent::ListInterfacesRequest) -> ::ttrpc::Result<super::agent::Interfaces> {
        Err(::ttrpc::Error::RpcStatus(::ttrpc::get_Status(::ttrpc::Code::NOT_FOUND, "/grpc.AgentService/ListInterfaces is not supported".to_string())))
    }
    fn list_routes(&self, ctx: &::ttrpc::TtrpcContext, req: super::agent::ListRoutesRequest) -> ::ttrpc::Result<super::agent::Routes> {
        Err(::ttrpc::Error::RpcStatus(::ttrpc::get_Status(::ttrpc::Code::NOT_FOUND, "/grpc.AgentService/ListRoutes is not supported".to_string())))
    }
    fn start_tracing(&self, ctx: &::ttrpc::TtrpcContext, req: super::agent::StartTracingRequest) -> ::ttrpc::Result<super::empty::Empty> {
        Err(::ttrpc::Error::RpcStatus(::ttrpc::get_Status(::ttrpc::Code::NOT_FOUND, "/grpc.AgentService/StartTracing is not supported".to_string())))
    }
    fn stop_tracing(&self, ctx: &::ttrpc::TtrpcContext, req: super::agent::StopTracingRequest) -> ::ttrpc::Result<super::empty::Empty> {
        Err(::ttrpc::Error::RpcStatus(::ttrpc::get_Status(::ttrpc::Code::NOT_FOUND, "/grpc.AgentService/StopTracing is not supported".to_string())))
    }
    fn create_sandbox(&self, ctx: &::ttrpc::TtrpcContext, req: super::agent::CreateSandboxRequest) -> ::ttrpc::Result<super::empty::Empty> {
        Err(::ttrpc::Error::RpcStatus(::ttrpc::get_Status(::ttrpc::Code::NOT_FOUND, "/grpc.AgentService/CreateSandbox is not supported".to_string())))
    }
    fn destroy_sandbox(&self, ctx: &::ttrpc::TtrpcContext, req: super::agent::DestroySandboxRequest) -> ::ttrpc::Result<super::empty::Empty> {
        Err(::ttrpc::Error::RpcStatus(::ttrpc::get_Status(::ttrpc::Code::NOT_FOUND, "/grpc.AgentService/DestroySandbox is not supported".to_string())))
    }
    fn online_cpu_mem(&self, ctx: &::ttrpc::TtrpcContext, req: super::agent::OnlineCPUMemRequest) -> ::ttrpc::Result<super::empty::Empty> {
        Err(::ttrpc::Error::RpcStatus(::ttrpc::get_Status(::ttrpc::Code::NOT_FOUND, "/grpc.AgentService/OnlineCPUMem is not supported".to_string())))
    }
    fn reseed_random_dev(&self, ctx: &::ttrpc::TtrpcContext, req: super::agent::ReseedRandomDevRequest) -> ::ttrpc::Result<super::empty::Empty> {
        Err(::ttrpc::Error::RpcStatus(::ttrpc::get_Status(::ttrpc::Code::NOT_FOUND, "/grpc.AgentService/ReseedRandomDev is not supported".to_string())))
    }
    fn get_guest_details(&self, ctx: &::ttrpc::TtrpcContext, req: super::agent::GuestDetailsRequest) -> ::ttrpc::Result<super::agent::GuestDetailsResponse> {
        Err(::ttrpc::Error::RpcStatus(::ttrpc::get_Status(::ttrpc::Code::NOT_FOUND, "/grpc.AgentService/GetGuestDetails is not supported".to_string())))
    }
    fn mem_hotplug_by_probe(&self, ctx: &::ttrpc::TtrpcContext, req: super::agent::MemHotplugByProbeRequest) -> ::ttrpc::Result<super::empty::Empty> {
        Err(::ttrpc::Error::RpcStatus(::ttrpc::get_Status(::ttrpc::Code::NOT_FOUND, "/grpc.AgentService/MemHotplugByProbe is not supported".to_string())))
    }
    fn set_guest_date_time(&self, ctx: &::ttrpc::TtrpcContext, req: super::agent::SetGuestDateTimeRequest) -> ::ttrpc::Result<super::empty::Empty> {
        Err(::ttrpc::Error::RpcStatus(::ttrpc::get_Status(::ttrpc::Code::NOT_FOUND, "/grpc.AgentService/SetGuestDateTime is not supported".to_string())))
    }
    fn copy_file(&self, ctx: &::ttrpc::TtrpcContext, req: super::agent::CopyFileRequest) -> ::ttrpc::Result<super::empty::Empty> {
        Err(::ttrpc::Error::RpcStatus(::ttrpc::get_Status(::ttrpc::Code::NOT_FOUND, "/grpc.AgentService/CopyFile is not supported".to_string())))
    }
}

pub fn create_agent_service(service: Arc<std::boxed::Box<AgentService + Send + Sync>>) -> HashMap <String, Box<::ttrpc::MethodHandler + Send + Sync>> {
    let mut methods = HashMap::new();

    methods.insert("/grpc.AgentService/CreateContainer".to_string(),
                    std::boxed::Box::new(create_container_method{service: service.clone()}) as std::boxed::Box<::ttrpc::MethodHandler + Send + Sync>);

    methods.insert("/grpc.AgentService/StartContainer".to_string(),
                    std::boxed::Box::new(start_container_method{service: service.clone()}) as std::boxed::Box<::ttrpc::MethodHandler + Send + Sync>);

    methods.insert("/grpc.AgentService/RemoveContainer".to_string(),
                    std::boxed::Box::new(remove_container_method{service: service.clone()}) as std::boxed::Box<::ttrpc::MethodHandler + Send + Sync>);

    methods.insert("/grpc.AgentService/ExecProcess".to_string(),
                    std::boxed::Box::new(exec_process_method{service: service.clone()}) as std::boxed::Box<::ttrpc::MethodHandler + Send + Sync>);

    methods.insert("/grpc.AgentService/SignalProcess".to_string(),
                    std::boxed::Box::new(signal_process_method{service: service.clone()}) as std::boxed::Box<::ttrpc::MethodHandler + Send + Sync>);

    methods.insert("/grpc.AgentService/WaitProcess".to_string(),
                    std::boxed::Box::new(wait_process_method{service: service.clone()}) as std::boxed::Box<::ttrpc::MethodHandler + Send + Sync>);

    methods.insert("/grpc.AgentService/ListProcesses".to_string(),
                    std::boxed::Box::new(list_processes_method{service: service.clone()}) as std::boxed::Box<::ttrpc::MethodHandler + Send + Sync>);

    methods.insert("/grpc.AgentService/UpdateContainer".to_string(),
                    std::boxed::Box::new(update_container_method{service: service.clone()}) as std::boxed::Box<::ttrpc::MethodHandler + Send + Sync>);

    methods.insert("/grpc.AgentService/StatsContainer".to_string(),
                    std::boxed::Box::new(stats_container_method{service: service.clone()}) as std::boxed::Box<::ttrpc::MethodHandler + Send + Sync>);

    methods.insert("/grpc.AgentService/PauseContainer".to_string(),
                    std::boxed::Box::new(pause_container_method{service: service.clone()}) as std::boxed::Box<::ttrpc::MethodHandler + Send + Sync>);

    methods.insert("/grpc.AgentService/ResumeContainer".to_string(),
                    std::boxed::Box::new(resume_container_method{service: service.clone()}) as std::boxed::Box<::ttrpc::MethodHandler + Send + Sync>);

    methods.insert("/grpc.AgentService/WriteStdin".to_string(),
                    std::boxed::Box::new(write_stdin_method{service: service.clone()}) as std::boxed::Box<::ttrpc::MethodHandler + Send + Sync>);

    methods.insert("/grpc.AgentService/ReadStdout".to_string(),
                    std::boxed::Box::new(read_stdout_method{service: service.clone()}) as std::boxed::Box<::ttrpc::MethodHandler + Send + Sync>);

    methods.insert("/grpc.AgentService/ReadStderr".to_string(),
                    std::boxed::Box::new(read_stderr_method{service: service.clone()}) as std::boxed::Box<::ttrpc::MethodHandler + Send + Sync>);

    methods.insert("/grpc.AgentService/CloseStdin".to_string(),
                    std::boxed::Box::new(close_stdin_method{service: service.clone()}) as std::boxed::Box<::ttrpc::MethodHandler + Send + Sync>);

    methods.insert("/grpc.AgentService/TtyWinResize".to_string(),
                    std::boxed::Box::new(tty_win_resize_method{service: service.clone()}) as std::boxed::Box<::ttrpc::MethodHandler + Send + Sync>);

    methods.insert("/grpc.AgentService/UpdateInterface".to_string(),
                    std::boxed::Box::new(update_interface_method{service: service.clone()}) as std::boxed::Box<::ttrpc::MethodHandler + Send + Sync>);

    methods.insert("/grpc.AgentService/UpdateRoutes".to_string(),
                    std::boxed::Box::new(update_routes_method{service: service.clone()}) as std::boxed::Box<::ttrpc::MethodHandler + Send + Sync>);

    methods.insert("/grpc.AgentService/ListInterfaces".to_string(),
                    std::boxed::Box::new(list_interfaces_method{service: service.clone()}) as std::boxed::Box<::ttrpc::MethodHandler + Send + Sync>);

    methods.insert("/grpc.AgentService/ListRoutes".to_string(),
                    std::boxed::Box::new(list_routes_method{service: service.clone()}) as std::boxed::Box<::ttrpc::MethodHandler + Send + Sync>);

    methods.insert("/grpc.AgentService/StartTracing".to_string(),
                    std::boxed::Box::new(start_tracing_method{service: service.clone()}) as std::boxed::Box<::ttrpc::MethodHandler + Send + Sync>);

    methods.insert("/grpc.AgentService/StopTracing".to_string(),
                    std::boxed::Box::new(stop_tracing_method{service: service.clone()}) as std::boxed::Box<::ttrpc::MethodHandler + Send + Sync>);

    methods.insert("/grpc.AgentService/CreateSandbox".to_string(),
                    std::boxed::Box::new(create_sandbox_method{service: service.clone()}) as std::boxed::Box<::ttrpc::MethodHandler + Send + Sync>);

    methods.insert("/grpc.AgentService/DestroySandbox".to_string(),
                    std::boxed::Box::new(destroy_sandbox_method{service: service.clone()}) as std::boxed::Box<::ttrpc::MethodHandler + Send + Sync>);

    methods.insert("/grpc.AgentService/OnlineCPUMem".to_string(),
                    std::boxed::Box::new(online_cpu_mem_method{service: service.clone()}) as std::boxed::Box<::ttrpc::MethodHandler + Send + Sync>);

    methods.insert("/grpc.AgentService/ReseedRandomDev".to_string(),
                    std::boxed::Box::new(reseed_random_dev_method{service: service.clone()}) as std::boxed::Box<::ttrpc::MethodHandler + Send + Sync>);

    methods.insert("/grpc.AgentService/GetGuestDetails".to_string(),
                    std::boxed::Box::new(get_guest_details_method{service: service.clone()}) as std::boxed::Box<::ttrpc::MethodHandler + Send + Sync>);

    methods.insert("/grpc.AgentService/MemHotplugByProbe".to_string(),
                    std::boxed::Box::new(mem_hotplug_by_probe_method{service: service.clone()}) as std::boxed::Box<::ttrpc::MethodHandler + Send + Sync>);

    methods.insert("/grpc.AgentService/SetGuestDateTime".to_string(),
                    std::boxed::Box::new(set_guest_date_time_method{service: service.clone()}) as std::boxed::Box<::ttrpc::MethodHandler + Send + Sync>);

    methods.insert("/grpc.AgentService/CopyFile".to_string(),
                    std::boxed::Box::new(copy_file_method{service: service.clone()}) as std::boxed::Box<::ttrpc::MethodHandler + Send + Sync>);

    methods
}
