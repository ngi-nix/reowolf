use crate as reowolf;
use reowolf::Polarity::*;
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
    static ref MINIMAL_PROTO: Arc<reowolf::ProtocolDescription> =
        { Arc::new(reowolf::ProtocolDescription::parse(b"").unwrap()) };
}

#[test]
fn simple_connector() {
    let c = reowolf::Connector::new_simple(MINIMAL_PROTO.clone(), 0);
    println!("{:#?}", c);
}

#[test]
fn add_port_pair() {
    let mut c = reowolf::Connector::new_simple(MINIMAL_PROTO.clone(), 0);
    let [_, _] = c.add_port_pair();
    let [_, _] = c.add_port_pair();
    println!("{:#?}", c);
}

#[test]
fn add_sync() {
    let mut c = reowolf::Connector::new_simple(MINIMAL_PROTO.clone(), 0);
    let [o, i] = c.add_port_pair();
    c.add_component(b"sync", &[i, o]).unwrap();
    println!("{:#?}", c);
}

#[test]
fn add_net_port() {
    let mut c = reowolf::Connector::new_simple(MINIMAL_PROTO.clone(), 0);
    let sock_addr = next_test_addr();
    let _ = c
        .add_net_port(reowolf::EndpointSetup { polarity: Getter, sock_addr, is_active: false })
        .unwrap();
    let _ = c
        .add_net_port(reowolf::EndpointSetup { polarity: Putter, sock_addr, is_active: true })
        .unwrap();
    println!("{:#?}", c);
}

#[test]
fn trivial_connect() {
    let mut c = reowolf::Connector::new_simple(MINIMAL_PROTO.clone(), 0);
    c.connect(Duration::from_secs(1)).unwrap();
    println!("{:#?}", c);
}

#[test]
fn single_node_connect() {
    let mut c = reowolf::Connector::new_simple(MINIMAL_PROTO.clone(), 0);
    let sock_addr = next_test_addr();
    let _ = c
        .add_net_port(reowolf::EndpointSetup { polarity: Getter, sock_addr, is_active: false })
        .unwrap();
    let _ = c
        .add_net_port(reowolf::EndpointSetup { polarity: Putter, sock_addr, is_active: true })
        .unwrap();
    c.connect(Duration::from_secs(1)).unwrap();
    println!("{:#?}", c);
    c.get_logger().dump_log(&mut std::io::stdout().lock());
}
