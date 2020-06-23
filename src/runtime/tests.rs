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
fn new_port_pair() {
    let mut c = Connector::new_simple(MINIMAL_PROTO.clone(), 0);
    let [_, _] = c.new_port_pair();
    let [_, _] = c.new_port_pair();
    println!("{:#?}", c);
}

#[test]
fn new_sync() {
    let mut c = Connector::new_simple(MINIMAL_PROTO.clone(), 0);
    let [o, i] = c.new_port_pair();
    c.add_component(b"sync", &[i, o]).unwrap();
    println!("{:#?}", c);
}

#[test]
fn new_net_port() {
    let mut c = Connector::new_simple(MINIMAL_PROTO.clone(), 0);
    let sock_addr = next_test_addr();
    let _ = c.new_net_port(Getter, EndpointSetup { sock_addr, is_active: false }).unwrap();
    let _ = c.new_net_port(Putter, EndpointSetup { sock_addr, is_active: true }).unwrap();
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
    let _ = c.new_net_port(Getter, EndpointSetup { sock_addr, is_active: false }).unwrap();
    let _ = c.new_net_port(Putter, EndpointSetup { sock_addr, is_active: true }).unwrap();
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
            let _ = c.new_net_port(Getter, EndpointSetup { sock_addr, is_active: true }).unwrap();
            c.connect(Duration::from_secs(1)).unwrap();
            c.print_state();
        });
        s.spawn(|_| {
            let mut c = Connector::new_simple(MINIMAL_PROTO.clone(), 1);
            let _ = c.new_net_port(Putter, EndpointSetup { sock_addr, is_active: false }).unwrap();
            c.connect(Duration::from_secs(1)).unwrap();
            c.print_state();
        });
    })
    .unwrap();
}

#[test]
fn put_no_sync() {
    let mut c = Connector::new_simple(MINIMAL_PROTO.clone(), 0);
    let [o, _] = c.new_port_pair();
    c.connect(Duration::from_secs(1)).unwrap();
    c.put(o, (b"hi" as &[_]).into()).unwrap();
}

#[test]
fn wrong_polarity_bad() {
    let mut c = Connector::new_simple(MINIMAL_PROTO.clone(), 0);
    let [_, i] = c.new_port_pair();
    c.connect(Duration::from_secs(1)).unwrap();
    c.put(i, (b"hi" as &[_]).into()).unwrap_err();
}

#[test]
fn dup_put_bad() {
    let mut c = Connector::new_simple(MINIMAL_PROTO.clone(), 0);
    let [o, _] = c.new_port_pair();
    c.connect(Duration::from_secs(1)).unwrap();
    c.put(o, (b"hi" as &[_]).into()).unwrap();
    c.put(o, (b"hi" as &[_]).into()).unwrap_err();
}

#[test]
fn trivial_sync() {
    let mut c = Connector::new_simple(MINIMAL_PROTO.clone(), 0);
    c.connect(Duration::from_secs(1)).unwrap();
    c.sync(Duration::from_secs(1)).unwrap();
    c.print_state();
}

#[test]
fn unconnected_gotten_err() {
    let mut c = Connector::new_simple(MINIMAL_PROTO.clone(), 0);
    let [_, i] = c.new_port_pair();
    assert_eq!(reowolf::error::GottenError::NoPreviousRound, c.gotten(i).unwrap_err());
}

#[test]
fn connected_gotten_err_no_round() {
    let mut c = Connector::new_simple(MINIMAL_PROTO.clone(), 0);
    let [_, i] = c.new_port_pair();
    c.connect(Duration::from_secs(1)).unwrap();
    assert_eq!(reowolf::error::GottenError::NoPreviousRound, c.gotten(i).unwrap_err());
}

#[test]
fn connected_gotten_err_ungotten() {
    let mut c = Connector::new_simple(MINIMAL_PROTO.clone(), 0);
    let [_, i] = c.new_port_pair();
    c.connect(Duration::from_secs(1)).unwrap();
    c.sync(Duration::from_secs(1)).unwrap();
    assert_eq!(reowolf::error::GottenError::PortDidntGet, c.gotten(i).unwrap_err());
}

#[test]
fn native_polarity_checks() {
    let mut c = Connector::new_simple(MINIMAL_PROTO.clone(), 0);
    let [o, i] = c.new_port_pair();
    c.connect(Duration::from_secs(1)).unwrap();
    // fail...
    c.get(o).unwrap_err();
    c.put(i, (b"hi" as &[_]).into()).unwrap_err();
    // succeed..
    c.get(i).unwrap();
    c.put(o, (b"hi" as &[_]).into()).unwrap();
}

#[test]
fn native_multiple_gets() {
    let mut c = Connector::new_simple(MINIMAL_PROTO.clone(), 0);
    let [_, i] = c.new_port_pair();
    c.connect(Duration::from_secs(1)).unwrap();
    c.get(i).unwrap();
    c.get(i).unwrap_err();
}

#[test]
fn next_batch() {
    let mut c = Connector::new_simple(MINIMAL_PROTO.clone(), 0);
    c.next_batch().unwrap_err();
    c.connect(Duration::from_secs(1)).unwrap();
    c.next_batch().unwrap();
    c.next_batch().unwrap();
    c.next_batch().unwrap();
}

#[test]
fn native_sync() {
    let mut c = Connector::new_simple(MINIMAL_PROTO.clone(), 0);
    let [o, i] = c.new_port_pair();
    c.connect(Duration::from_secs(1)).unwrap();
    c.get(i).unwrap();
    c.put(o, (b"hi" as &[_]).into()).unwrap();
    c.sync(Duration::from_secs(1)).unwrap();
}

#[test]
fn native_message_pass() {
    let sock_addr = next_test_addr();
    scope(|s| {
        s.spawn(|_| {
            let mut c = Connector::new_simple(MINIMAL_PROTO.clone(), 0);
            let g = c.new_net_port(Getter, EndpointSetup { sock_addr, is_active: true }).unwrap();
            c.connect(Duration::from_secs(1)).unwrap();
            c.get(g).unwrap();
            c.sync(Duration::from_secs(1)).unwrap();
            c.gotten(g).unwrap();
        });
        s.spawn(|_| {
            let mut c = Connector::new_simple(MINIMAL_PROTO.clone(), 1);
            let p = c.new_net_port(Putter, EndpointSetup { sock_addr, is_active: false }).unwrap();
            c.connect(Duration::from_secs(1)).unwrap();
            c.put(p, (b"hello" as &[_]).into()).unwrap();
            c.sync(Duration::from_secs(1)).unwrap();
        });
    })
    .unwrap();
}
