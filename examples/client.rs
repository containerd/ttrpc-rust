mod protocols;

use std::thread;
use std::env;

use nix::sys::socket::*;
use nix::unistd::close;

use ttrpc::client::Client;
use log::LevelFilter;

fn main() {
    //simple_logging::log_to_stderr(LevelFilter::Trace);

    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        panic!("Usage: {} unix_addr", args[0]);
    }

    let fd = socket(AddressFamily::Unix, SockType::Stream, SockFlag::empty(), None).unwrap();
    let sockaddr = args[1].clone() + &"\x00".to_string();
    let sockaddr = UnixAddr::new_abstract(sockaddr.as_bytes()).unwrap();
    let sockaddr = SockAddr::Unix(sockaddr);
    connect(fd, &sockaddr).unwrap();

    let c = Client::new(fd);
    let hc = protocols::health_ttrpc::HealthClient::new(c.clone());
    let ac = protocols::agent_ttrpc::AgentServiceClient::new(c);

    let thc = hc.clone();
    let tac = ac.clone();
    let t = thread::spawn(move|| {
        let req = protocols::health::CheckRequest::new();

        println!("thread check: {:?}", thc.check(&req, 0));

        println!("thread version: {:?}", thc.version(&req, 0));

        let show = match tac.list_interfaces(&protocols::agent::ListInterfacesRequest::new(), 0) {
            Err(e) => {format!("{:?}", e)},
            Ok(s) => {format!("{:?}", s)},
        };
        println!("thread list_interfaces: {}", show);
    });

    println!("main check: {:?}", hc.check(&protocols::health::CheckRequest::new(), 0));

    let show = match ac.online_cpu_mem(&protocols::agent::OnlineCPUMemRequest::new(), 0) {
        Err(e) => {format!("{:?}", e)},
        Ok(s) => {format!("{:?}", s)},
    };
    println!("main online_cpu_mem: {}", show);

    t.join().unwrap();

    close(fd);
}
