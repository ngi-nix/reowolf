use crate as reowolf;
use crossbeam_utils::thread::scope;
use reowolf::{Connector, EndpointSetup, Polarity::*, ProtocolDescription};
use std::net::SocketAddr;
use std::{sync::Arc, time::Duration};

fn next_test_addr() -> SocketAddr {
    use std::{
        net::{Ipv4Addr, SocketAddrV4},
        sync::atomic::{AtomicU16, Ordering::SeqCst},
    };
    static TEST_PORT: AtomicU16 = AtomicU16::new(5_000);
    let port = TEST_PORT.fetch_add(1, SeqCst);
    SocketAddrV4::new(Ipv4Addr::LOCALHOST, port).into()
}

lazy_static::lazy_static! {
    static ref MINIMAL_PROTO: Arc<ProtocolDescription> =
        { Arc::new(reowolf::ProtocolDescription::parse(b"").unwrap()) };
}

#[test]
fn simple_connector() {
    let c = Connector::new_simple(MINIMAL_PROTO.clone(), 0);
    println!("{:#?}", c);
}

#[test]
fn add_port_pair() {
    let mut c = Connector::new_simple(MINIMAL_PROTO.clone(), 0);
    let [_, _] = c.add_port_pair();
    let [_, _] = c.add_port_pair();
    println!("{:#?}", c);
}

#[test]
fn add_sync() {
    let mut c = Connector::new_simple(MINIMAL_PROTO.clone(), 0);
    let [o, i] = c.add_port_pair();
    c.add_component(b"sync", &[i, o]).unwrap();
    println!("{:#?}", c);
}

#[test]
fn add_net_port() {
    let mut c = Connector::new_simple(MINIMAL_PROTO.clone(), 0);
    let sock_addr = next_test_addr();
    let _ = c.add_net_port(Getter, EndpointSetup { sock_addr, is_active: false }).unwrap();
    let _ = c.add_net_port(Putter, EndpointSetup { sock_addr, is_active: true }).unwrap();
    println!("{:#?}", c);
}

#[test]
fn trivial_connect() {
    let mut c = Connector::new_simple(MINIMAL_PROTO.clone(), 0);
    c.connect(Duration::from_secs(1)).unwrap();
    println!("{:#?}", c);
}

#[test]
fn single_node_connect() {
    let sock_addr = next_test_addr();
    let mut c = Connector::new_simple(MINIMAL_PROTO.clone(), 0);
    let _ = c.add_net_port(Getter, EndpointSetup { sock_addr, is_active: false }).unwrap();
    let _ = c.add_net_port(Putter, EndpointSetup { sock_addr, is_active: true }).unwrap();
    let res = c.connect(Duration::from_secs(1));
    println!("{:#?}", c);
    c.get_logger().dump_log(&mut std::io::stdout().lock());
    res.unwrap();
}

#[test]
fn multithreaded_connect() {
    let sock_addr = next_test_addr();
    scope(|s| {
        s.spawn(|_| {
            let mut c = Connector::new_simple(MINIMAL_PROTO.clone(), 0);
            let _ = c.add_net_port(Getter, EndpointSetup { sock_addr, is_active: true }).unwrap();
            c.connect(Duration::from_secs(1)).unwrap();
            c.print_state();
        });
        s.spawn(|_| {
            let mut c = Connector::new_simple(MINIMAL_PROTO.clone(), 1);
            let _ = c.add_net_port(Putter, EndpointSetup { sock_addr, is_active: false }).unwrap();
            c.connect(Duration::from_secs(1)).unwrap();
            c.print_state();
        });
    })
    .unwrap();
}
