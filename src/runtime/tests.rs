use crate as reowolf;
use crossbeam_utils::thread::scope;
use reowolf::{
    Polarity::{Getter, Putter},
    *,
};
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
    Connector::new_simple(MINIMAL_PROTO.clone(), 0);
}

#[test]
fn new_port_pair() {
    let mut c = Connector::new_simple(MINIMAL_PROTO.clone(), 0);
    let [_, _] = c.new_port_pair();
    let [_, _] = c.new_port_pair();
}

#[test]
fn new_sync() {
    let mut c = Connector::new_simple(MINIMAL_PROTO.clone(), 0);
    let [o, i] = c.new_port_pair();
    c.add_component(b"sync", &[i, o]).unwrap();
}

#[test]
fn new_net_port() {
    let mut c = Connector::new_simple(MINIMAL_PROTO.clone(), 0);
    let sock_addr = next_test_addr();
    let _ = c.new_net_port(Getter, EndpointSetup { sock_addr, is_active: false }).unwrap();
    let _ = c.new_net_port(Putter, EndpointSetup { sock_addr, is_active: true }).unwrap();
}

#[test]
fn trivial_connect() {
    let mut c = Connector::new_simple(MINIMAL_PROTO.clone(), 0);
    c.connect(Some(Duration::from_secs(1))).unwrap();
}

#[test]
fn single_node_connect() {
    let sock_addr = next_test_addr();
    let mut c = Connector::new_simple(MINIMAL_PROTO.clone(), 0);
    let _ = c.new_net_port(Getter, EndpointSetup { sock_addr, is_active: false }).unwrap();
    let _ = c.new_net_port(Putter, EndpointSetup { sock_addr, is_active: true }).unwrap();
    c.connect(Some(Duration::from_secs(1))).unwrap();
}

#[test]
fn multithreaded_connect() {
    let sock_addr = next_test_addr();
    scope(|s| {
        s.spawn(|_| {
            let mut c = Connector::new_simple(MINIMAL_PROTO.clone(), 0);
            let _ = c.new_net_port(Getter, EndpointSetup { sock_addr, is_active: true }).unwrap();
            c.connect(Some(Duration::from_secs(1))).unwrap();
        });
        s.spawn(|_| {
            let mut c = Connector::new_simple(MINIMAL_PROTO.clone(), 1);
            let _ = c.new_net_port(Putter, EndpointSetup { sock_addr, is_active: false }).unwrap();
            c.connect(Some(Duration::from_secs(1))).unwrap();
        });
    })
    .unwrap();
}

#[test]
fn put_no_sync() {
    let mut c = Connector::new_simple(MINIMAL_PROTO.clone(), 0);
    let [o, _] = c.new_port_pair();
    c.connect(Some(Duration::from_secs(1))).unwrap();
    c.put(o, (b"hi" as &[_]).into()).unwrap();
}

#[test]
fn wrong_polarity_bad() {
    let mut c = Connector::new_simple(MINIMAL_PROTO.clone(), 0);
    let [_, i] = c.new_port_pair();
    c.connect(Some(Duration::from_secs(1))).unwrap();
    c.put(i, (b"hi" as &[_]).into()).unwrap_err();
}

#[test]
fn dup_put_bad() {
    let mut c = Connector::new_simple(MINIMAL_PROTO.clone(), 0);
    let [o, _] = c.new_port_pair();
    c.connect(Some(Duration::from_secs(1))).unwrap();
    c.put(o, (b"hi" as &[_]).into()).unwrap();
    c.put(o, (b"hi" as &[_]).into()).unwrap_err();
}

#[test]
fn trivial_sync() {
    let mut c = Connector::new_simple(MINIMAL_PROTO.clone(), 0);
    c.connect(Some(Duration::from_secs(1))).unwrap();
    c.sync(Some(Duration::from_secs(1))).unwrap();
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
    c.connect(Some(Duration::from_secs(1))).unwrap();
    assert_eq!(reowolf::error::GottenError::NoPreviousRound, c.gotten(i).unwrap_err());
}

#[test]
fn connected_gotten_err_ungotten() {
    let mut c = Connector::new_simple(MINIMAL_PROTO.clone(), 0);
    let [_, i] = c.new_port_pair();
    c.connect(Some(Duration::from_secs(1))).unwrap();
    c.sync(Some(Duration::from_secs(1))).unwrap();
    assert_eq!(reowolf::error::GottenError::PortDidntGet, c.gotten(i).unwrap_err());
}

#[test]
fn native_polarity_checks() {
    let mut c = Connector::new_simple(MINIMAL_PROTO.clone(), 0);
    let [o, i] = c.new_port_pair();
    c.connect(Some(Duration::from_secs(1))).unwrap();
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
    c.connect(Some(Duration::from_secs(1))).unwrap();
    c.get(i).unwrap();
    c.get(i).unwrap_err();
}

#[test]
fn next_batch() {
    let mut c = Connector::new_simple(MINIMAL_PROTO.clone(), 0);
    c.next_batch().unwrap_err();
    c.connect(Some(Duration::from_secs(1))).unwrap();
    c.next_batch().unwrap();
    c.next_batch().unwrap();
    c.next_batch().unwrap();
}

#[test]
fn native_self_msg() {
    let mut c = Connector::new_simple(MINIMAL_PROTO.clone(), 0);
    let [o, i] = c.new_port_pair();
    c.connect(Some(Duration::from_secs(1))).unwrap();
    c.get(i).unwrap();
    c.put(o, (b"hi" as &[_]).into()).unwrap();
    c.sync(Some(Duration::from_secs(1))).unwrap();
}

#[test]
fn two_natives_msg() {
    let sock_addr = next_test_addr();
    scope(|s| {
        s.spawn(|_| {
            let mut c = Connector::new_simple(MINIMAL_PROTO.clone(), 0);
            let g = c.new_net_port(Getter, EndpointSetup { sock_addr, is_active: true }).unwrap();
            c.connect(Some(Duration::from_secs(1))).unwrap();
            c.get(g).unwrap();
            c.sync(Some(Duration::from_secs(1))).unwrap();
            c.gotten(g).unwrap();
        });
        s.spawn(|_| {
            let mut c = Connector::new_simple(MINIMAL_PROTO.clone(), 1);
            let p = c.new_net_port(Putter, EndpointSetup { sock_addr, is_active: false }).unwrap();
            c.connect(Some(Duration::from_secs(1))).unwrap();
            c.put(p, (b"hello" as &[_]).into()).unwrap();
            c.sync(Some(Duration::from_secs(1))).unwrap();
        });
    })
    .unwrap();
}

#[test]
fn trivial_nondet() {
    let mut c = Connector::new_simple(MINIMAL_PROTO.clone(), 0);
    let [_, i] = c.new_port_pair();
    c.connect(Some(Duration::from_secs(1))).unwrap();
    c.get(i).unwrap();
    // getting 0 batch
    c.next_batch().unwrap();
    // silent 1 batch
    assert_eq!(1, c.sync(Some(Duration::from_secs(1))).unwrap());
    c.gotten(i).unwrap_err();
}

#[test]
fn connector_pair_nondet() {
    let sock_addr = next_test_addr();
    scope(|s| {
        s.spawn(|_| {
            let mut c = Connector::new_simple(MINIMAL_PROTO.clone(), 0);
            let g = c.new_net_port(Getter, EndpointSetup { sock_addr, is_active: true }).unwrap();
            c.connect(Some(Duration::from_secs(1))).unwrap();
            c.next_batch().unwrap();
            c.get(g).unwrap();
            assert_eq!(1, c.sync(Some(Duration::from_secs(1))).unwrap());
            c.gotten(g).unwrap();
        });
        s.spawn(|_| {
            let mut c = Connector::new_simple(MINIMAL_PROTO.clone(), 1);
            let p = c.new_net_port(Putter, EndpointSetup { sock_addr, is_active: false }).unwrap();
            c.connect(Some(Duration::from_secs(1))).unwrap();
            c.put(p, (b"hello" as &[_]).into()).unwrap();
            c.sync(Some(Duration::from_secs(1))).unwrap();
        });
    })
    .unwrap();
}

#[test]
fn cannot_use_moved_ports() {
    /*
    native p|-->|g sync
    */
    let mut c = Connector::new_simple(MINIMAL_PROTO.clone(), 1);
    let [p, g] = c.new_port_pair();
    c.add_component(b"sync", &[g, p]).unwrap();
    c.connect(Some(Duration::from_secs(1))).unwrap();
    c.put(p, (b"hello" as &[_]).into()).unwrap_err();
    c.get(g).unwrap_err();
}

#[test]
fn sync_sync() {
    /*
    native p0|-->|g0 sync
           g1|<--|p1
    */
    let mut c = Connector::new_simple(MINIMAL_PROTO.clone(), 0);
    let [p0, g0] = c.new_port_pair();
    let [p1, g1] = c.new_port_pair();
    c.add_component(b"sync", &[g0, p1]).unwrap();
    c.connect(Some(Duration::from_secs(1))).unwrap();
    c.put(p0, (b"hello" as &[_]).into()).unwrap();
    c.get(g1).unwrap();
    c.sync(Some(Duration::from_secs(1))).unwrap();
    c.gotten(g1).unwrap();
}

fn file_logged_connector(controller_id: ControllerId, path: &str) -> Connector {
    let file = std::fs::File::create(path).unwrap();
    let file_logger = Box::new(FileLogger::new(controller_id, file));
    Connector::new(file_logger, MINIMAL_PROTO.clone(), controller_id, 8)
}

#[test]
fn double_net_connect() {
    let sock_addrs = [next_test_addr(), next_test_addr()];
    scope(|s| {
        s.spawn(|_| {
            let mut c = file_logged_connector(0, "./logs/double_net_a.txt");
            let [_p, _g] = [
                c.new_net_port(Putter, EndpointSetup { sock_addr: sock_addrs[0], is_active: true })
                    .unwrap(),
                c.new_net_port(Getter, EndpointSetup { sock_addr: sock_addrs[1], is_active: true })
                    .unwrap(),
            ];
            c.connect(Some(Duration::from_secs(1))).unwrap();
        });
        s.spawn(|_| {
            let mut c = file_logged_connector(1, "./logs/double_net_b.txt");
            let [_g, _p] = [
                c.new_net_port(
                    Getter,
                    EndpointSetup { sock_addr: sock_addrs[0], is_active: false },
                )
                .unwrap(),
                c.new_net_port(
                    Putter,
                    EndpointSetup { sock_addr: sock_addrs[1], is_active: false },
                )
                .unwrap(),
            ];
            c.connect(Some(Duration::from_secs(1))).unwrap();
        });
    })
    .unwrap();
}

#[test]
fn distributed_msg_bounce() {
    /*
    native[0] | sync 0.p|-->|1.p native[1]
                     0.g|<--|1.g
    */
    let sock_addrs = [next_test_addr(), next_test_addr()];
    scope(|s| {
        s.spawn(|_| {
            /*
            native | sync p|-->
                   |      g|<--
            */
            let mut c = file_logged_connector(0, "./logs/distributed_msg_bounce_a.txt");
            let [p, g] = [
                c.new_net_port(Putter, EndpointSetup { sock_addr: sock_addrs[0], is_active: true })
                    .unwrap(),
                c.new_net_port(Getter, EndpointSetup { sock_addr: sock_addrs[1], is_active: true })
                    .unwrap(),
            ];
            c.add_component(b"sync", &[g, p]).unwrap();
            c.connect(Some(Duration::from_secs(1))).unwrap();
            c.sync(Some(Duration::from_secs(1))).unwrap();
        });
        s.spawn(|_| {
            /*
            native p|-->
                   g|<--
            */
            let mut c = file_logged_connector(1, "./logs/distributed_msg_bounce_b.txt");
            let [g, p] = [
                c.new_net_port(
                    Getter,
                    EndpointSetup { sock_addr: sock_addrs[0], is_active: false },
                )
                .unwrap(),
                c.new_net_port(
                    Putter,
                    EndpointSetup { sock_addr: sock_addrs[1], is_active: false },
                )
                .unwrap(),
            ];
            c.connect(Some(Duration::from_secs(1))).unwrap();
            c.put(p, (b"hello" as &[_]).into()).unwrap();
            c.get(g).unwrap();
            c.sync(Some(Duration::from_secs(1))).unwrap();
            c.gotten(g).unwrap();
        });
    })
    .unwrap();
}
