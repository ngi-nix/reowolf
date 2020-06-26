use crate as reowolf;
use crossbeam_utils::thread::scope;
use reowolf::{
    error::*,
    EndpointPolarity::{Active, Passive},
    Polarity::{Getter, Putter},
    *,
};
use std::{fs::File, net::SocketAddr, path::Path, sync::Arc, time::Duration};
//////////////////////////////////////////
fn next_test_addr() -> SocketAddr {
    use std::{
        net::{Ipv4Addr, SocketAddrV4},
        sync::atomic::{AtomicU16, Ordering::SeqCst},
    };
    static TEST_PORT: AtomicU16 = AtomicU16::new(5_000);
    let port = TEST_PORT.fetch_add(1, SeqCst);
    SocketAddrV4::new(Ipv4Addr::LOCALHOST, port).into()
}

fn file_logged_connector(connector_id: ConnectorId, dir_path: &Path) -> Connector {
    let _ = std::fs::create_dir(dir_path); // we will check failure soon
    let path = dir_path.join(format!("cid_{:?}.txt", connector_id));
    let file = File::create(path).unwrap();
    let file_logger = Box::new(FileLogger::new(connector_id, file));
    Connector::new(file_logger, MINIMAL_PROTO.clone(), connector_id, 8)
}

lazy_static::lazy_static! {
    static ref MINIMAL_PROTO: Arc<ProtocolDescription> = {
        Arc::new(reowolf::ProtocolDescription::parse(b"").unwrap())
    };
}
lazy_static::lazy_static! {
    static ref TEST_MSG: Payload = {
        Payload::from(b"hello" as &[u8])
    };
}

//////////////////////////////////////////

#[test]
fn basic_connector() {
    Connector::new(Box::new(DummyLogger), MINIMAL_PROTO.clone(), 0, 0);
}

#[test]
fn basic_logged_connector() {
    let test_log_path = Path::new("./logs/basic_logged_connector");
    file_logged_connector(0, test_log_path);
}

#[test]
fn new_port_pair() {
    let test_log_path = Path::new("./logs/new_port_pair");
    let mut c = file_logged_connector(0, test_log_path);
    let [_, _] = c.new_port_pair();
    let [_, _] = c.new_port_pair();
}

#[test]
fn new_sync() {
    let test_log_path = Path::new("./logs/new_sync");
    let mut c = file_logged_connector(0, test_log_path);
    let [o, i] = c.new_port_pair();
    c.add_component(b"sync", &[i, o]).unwrap();
}

#[test]
fn new_net_port() {
    let test_log_path = Path::new("./logs/new_net_port");
    let mut c = file_logged_connector(0, test_log_path);
    let sock_addr = next_test_addr();
    let _ = c.new_net_port(Getter, sock_addr, Passive).unwrap();
    let _ = c.new_net_port(Putter, sock_addr, Active).unwrap();
}

#[test]
fn trivial_connect() {
    let test_log_path = Path::new("./logs/trivial_connect");
    let mut c = file_logged_connector(0, test_log_path);
    c.connect(Some(Duration::from_secs(1))).unwrap();
}

#[test]
fn single_node_connect() {
    let sock_addr = next_test_addr();
    let test_log_path = Path::new("./logs/single_node_connect");
    let mut c = file_logged_connector(0, test_log_path);
    let _ = c.new_net_port(Getter, sock_addr, Passive).unwrap();
    let _ = c.new_net_port(Putter, sock_addr, Active).unwrap();
    c.connect(Some(Duration::from_secs(1))).unwrap();
}

#[test]
fn minimal_net_connect() {
    let sock_addr = next_test_addr();
    let test_log_path = Path::new("./logs/minimal_net_connect");
    scope(|s| {
        s.spawn(|_| {
            let mut c = file_logged_connector(0, test_log_path);
            let _ = c.new_net_port(Getter, sock_addr, Active).unwrap();
            c.connect(Some(Duration::from_secs(1))).unwrap();
        });
        s.spawn(|_| {
            let mut c = file_logged_connector(1, test_log_path);
            let _ = c.new_net_port(Putter, sock_addr, Passive).unwrap();
            c.connect(Some(Duration::from_secs(1))).unwrap();
        });
    })
    .unwrap();
}

#[test]
fn put_no_sync() {
    let test_log_path = Path::new("./logs/put_no_sync");
    let mut c = file_logged_connector(0, test_log_path);
    let [o, _] = c.new_port_pair();
    c.connect(Some(Duration::from_secs(1))).unwrap();
    c.put(o, TEST_MSG.clone()).unwrap();
}

#[test]
fn wrong_polarity_bad() {
    let test_log_path = Path::new("./logs/wrong_polarity_bad");
    let mut c = file_logged_connector(0, test_log_path);
    let [_, i] = c.new_port_pair();
    c.connect(Some(Duration::from_secs(1))).unwrap();
    c.put(i, TEST_MSG.clone()).unwrap_err();
}

#[test]
fn dup_put_bad() {
    let test_log_path = Path::new("./logs/dup_put_bad");
    let mut c = file_logged_connector(0, test_log_path);
    let [o, _] = c.new_port_pair();
    c.connect(Some(Duration::from_secs(1))).unwrap();
    c.put(o, TEST_MSG.clone()).unwrap();
    c.put(o, TEST_MSG.clone()).unwrap_err();
}

#[test]
fn trivial_sync() {
    let test_log_path = Path::new("./logs/trivial_sync");
    let mut c = file_logged_connector(0, test_log_path);
    c.connect(Some(Duration::from_secs(1))).unwrap();
    c.sync(Some(Duration::from_secs(1))).unwrap();
}

#[test]
fn unconnected_gotten_err() {
    let test_log_path = Path::new("./logs/unconnected_gotten_err");
    let mut c = file_logged_connector(0, test_log_path);
    let [_, i] = c.new_port_pair();
    assert_eq!(reowolf::error::GottenError::NoPreviousRound, c.gotten(i).unwrap_err());
}

#[test]
fn connected_gotten_err_no_round() {
    let test_log_path = Path::new("./logs/connected_gotten_err_no_round");
    let mut c = file_logged_connector(0, test_log_path);
    let [_, i] = c.new_port_pair();
    c.connect(Some(Duration::from_secs(1))).unwrap();
    assert_eq!(reowolf::error::GottenError::NoPreviousRound, c.gotten(i).unwrap_err());
}

#[test]
fn connected_gotten_err_ungotten() {
    let test_log_path = Path::new("./logs/connected_gotten_err_ungotten");
    let mut c = file_logged_connector(0, test_log_path);
    let [_, i] = c.new_port_pair();
    c.connect(Some(Duration::from_secs(1))).unwrap();
    c.sync(Some(Duration::from_secs(1))).unwrap();
    assert_eq!(reowolf::error::GottenError::PortDidntGet, c.gotten(i).unwrap_err());
}

#[test]
fn native_polarity_checks() {
    let test_log_path = Path::new("./logs/native_polarity_checks");
    let mut c = file_logged_connector(0, test_log_path);
    let [o, i] = c.new_port_pair();
    c.connect(Some(Duration::from_secs(1))).unwrap();
    // fail...
    c.get(o).unwrap_err();
    c.put(i, TEST_MSG.clone()).unwrap_err();
    // succeed..
    c.get(i).unwrap();
    c.put(o, TEST_MSG.clone()).unwrap();
}

#[test]
fn native_multiple_gets() {
    let test_log_path = Path::new("./logs/native_multiple_gets");
    let mut c = file_logged_connector(0, test_log_path);
    let [_, i] = c.new_port_pair();
    c.connect(Some(Duration::from_secs(1))).unwrap();
    c.get(i).unwrap();
    c.get(i).unwrap_err();
}

#[test]
fn next_batch() {
    let test_log_path = Path::new("./logs/next_batch");
    let mut c = file_logged_connector(0, test_log_path);
    c.next_batch().unwrap_err();
    c.connect(Some(Duration::from_secs(1))).unwrap();
    c.next_batch().unwrap();
    c.next_batch().unwrap();
    c.next_batch().unwrap();
}

#[test]
fn native_self_msg() {
    let test_log_path = Path::new("./logs/native_self_msg");
    let mut c = file_logged_connector(0, test_log_path);
    let [o, i] = c.new_port_pair();
    c.connect(Some(Duration::from_secs(1))).unwrap();
    c.get(i).unwrap();
    c.put(o, TEST_MSG.clone()).unwrap();
    c.sync(Some(Duration::from_secs(1))).unwrap();
}

#[test]
fn two_natives_msg() {
    let test_log_path = Path::new("./logs/two_natives_msg");
    let sock_addr = next_test_addr();
    scope(|s| {
        s.spawn(|_| {
            let mut c = file_logged_connector(0, test_log_path);
            let g = c.new_net_port(Getter, sock_addr, Active).unwrap();
            c.connect(Some(Duration::from_secs(1))).unwrap();
            c.get(g).unwrap();
            c.sync(Some(Duration::from_secs(1))).unwrap();
            c.gotten(g).unwrap();
        });
        s.spawn(|_| {
            let mut c = file_logged_connector(1, test_log_path);
            let p = c.new_net_port(Putter, sock_addr, Passive).unwrap();
            c.connect(Some(Duration::from_secs(1))).unwrap();
            c.put(p, TEST_MSG.clone()).unwrap();
            c.sync(Some(Duration::from_secs(1))).unwrap();
        });
    })
    .unwrap();
}

#[test]
fn trivial_nondet() {
    let test_log_path = Path::new("./logs/trivial_nondet");
    let mut c = file_logged_connector(0, test_log_path);
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
    let test_log_path = Path::new("./logs/connector_pair_nondet");
    let sock_addr = next_test_addr();
    scope(|s| {
        s.spawn(|_| {
            let mut c = file_logged_connector(0, test_log_path);
            let g = c.new_net_port(Getter, sock_addr, Active).unwrap();
            c.connect(Some(Duration::from_secs(1))).unwrap();
            c.next_batch().unwrap();
            c.get(g).unwrap();
            assert_eq!(1, c.sync(Some(Duration::from_secs(1))).unwrap());
            c.gotten(g).unwrap();
        });
        s.spawn(|_| {
            let mut c = file_logged_connector(1, test_log_path);
            let p = c.new_net_port(Putter, sock_addr, Passive).unwrap();
            c.connect(Some(Duration::from_secs(1))).unwrap();
            c.put(p, TEST_MSG.clone()).unwrap();
            c.sync(Some(Duration::from_secs(1))).unwrap();
        });
    })
    .unwrap();
}

#[test]
fn native_immediately_inconsistent() {
    let test_log_path = Path::new("./logs/native_immediately_inconsistent");
    let mut c = file_logged_connector(0, test_log_path);
    let [_, g] = c.new_port_pair();
    c.connect(Some(Duration::from_secs(1))).unwrap();
    c.get(g).unwrap();
    c.sync(Some(Duration::from_secs(30))).unwrap_err();
}

#[test]
fn native_recovers() {
    let test_log_path = Path::new("./logs/native_recovers");
    let mut c = file_logged_connector(0, test_log_path);
    let [p, g] = c.new_port_pair();
    c.connect(Some(Duration::from_secs(1))).unwrap();
    c.get(g).unwrap();
    c.sync(Some(Duration::from_secs(30))).unwrap_err();
    c.put(p, TEST_MSG.clone()).unwrap();
    c.get(g).unwrap();
    c.sync(Some(Duration::from_secs(30))).unwrap();
}

#[test]
fn cannot_use_moved_ports() {
    /*
    native p|-->|g sync
    */
    let test_log_path = Path::new("./logs/cannot_use_moved_ports");
    let mut c = file_logged_connector(0, test_log_path);
    let [p, g] = c.new_port_pair();
    c.add_component(b"sync", &[g, p]).unwrap();
    c.connect(Some(Duration::from_secs(1))).unwrap();
    c.put(p, TEST_MSG.clone()).unwrap_err();
    c.get(g).unwrap_err();
}

#[test]
fn sync_sync() {
    /*
    native p0|-->|g0 sync
           g1|<--|p1
    */
    let test_log_path = Path::new("./logs/sync_sync");
    let mut c = file_logged_connector(0, test_log_path);
    let [p0, g0] = c.new_port_pair();
    let [p1, g1] = c.new_port_pair();
    c.add_component(b"sync", &[g0, p1]).unwrap();
    c.connect(Some(Duration::from_secs(1))).unwrap();
    c.put(p0, TEST_MSG.clone()).unwrap();
    c.get(g1).unwrap();
    c.sync(Some(Duration::from_secs(1))).unwrap();
    c.gotten(g1).unwrap();
}

#[test]
fn double_net_connect() {
    let test_log_path = Path::new("./logs/double_net_connect");
    let sock_addrs = [next_test_addr(), next_test_addr()];
    scope(|s| {
        s.spawn(|_| {
            let mut c = file_logged_connector(0, test_log_path);
            let [_p, _g] = [
                c.new_net_port(Putter, sock_addrs[0], Active).unwrap(),
                c.new_net_port(Getter, sock_addrs[1], Active).unwrap(),
            ];
            c.connect(Some(Duration::from_secs(1))).unwrap();
        });
        s.spawn(|_| {
            let mut c = file_logged_connector(1, test_log_path);
            let [_g, _p] = [
                c.new_net_port(Getter, sock_addrs[0], Passive).unwrap(),
                c.new_net_port(Putter, sock_addrs[1], Passive).unwrap(),
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
    let test_log_path = Path::new("./logs/distributed_msg_bounce");
    let sock_addrs = [next_test_addr(), next_test_addr()];
    scope(|s| {
        s.spawn(|_| {
            /*
            native | sync p|-->
                   |      g|<--
            */
            let mut c = file_logged_connector(0, test_log_path);
            let [p, g] = [
                c.new_net_port(Putter, sock_addrs[0], Active).unwrap(),
                c.new_net_port(Getter, sock_addrs[1], Active).unwrap(),
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
            let mut c = file_logged_connector(1, test_log_path);
            let [g, p] = [
                c.new_net_port(Getter, sock_addrs[0], Passive).unwrap(),
                c.new_net_port(Putter, sock_addrs[1], Passive).unwrap(),
            ];
            c.connect(Some(Duration::from_secs(1))).unwrap();
            c.put(p, TEST_MSG.clone()).unwrap();
            c.get(g).unwrap();
            c.sync(Some(Duration::from_secs(1))).unwrap();
            c.gotten(g).unwrap();
        });
    })
    .unwrap();
}

#[test]
fn local_timeout() {
    let test_log_path = Path::new("./logs/local_timeout");
    let mut c = file_logged_connector(0, test_log_path);
    let [_, g] = c.new_port_pair();
    c.connect(Some(Duration::from_secs(1))).unwrap();
    c.get(g).unwrap();
    match c.sync(Some(Duration::from_millis(200))) {
        Err(SyncError::RoundFailure) => {}
        res => panic!("expeted timeout. but got {:?}", res),
    }
}

#[test]
fn parent_timeout() {
    let test_log_path = Path::new("./logs/parent_timeout");
    let sock_addr = next_test_addr();
    scope(|s| {
        s.spawn(|_| {
            // parent; times out
            let mut c = file_logged_connector(999, test_log_path);
            let _ = c.new_net_port(Putter, sock_addr, Active).unwrap();
            c.connect(Some(Duration::from_secs(1))).unwrap();
            c.sync(Some(Duration::from_millis(300))).unwrap_err(); // timeout
        });
        s.spawn(|_| {
            // child
            let mut c = file_logged_connector(000, test_log_path);
            let g = c.new_net_port(Getter, sock_addr, Passive).unwrap();
            c.connect(Some(Duration::from_secs(1))).unwrap();
            c.get(g).unwrap(); // not matched by put
            c.sync(None).unwrap_err(); // no timeout
        });
    })
    .unwrap();
}

#[test]
fn child_timeout() {
    let test_log_path = Path::new("./logs/child_timeout");
    let sock_addr = next_test_addr();
    scope(|s| {
        s.spawn(|_| {
            // child; times out
            let mut c = file_logged_connector(000, test_log_path);
            let _ = c.new_net_port(Putter, sock_addr, Active).unwrap();
            c.connect(Some(Duration::from_secs(1))).unwrap();
            c.sync(Some(Duration::from_millis(300))).unwrap_err(); // timeout
        });
        s.spawn(|_| {
            // parent
            let mut c = file_logged_connector(999, test_log_path);
            let g = c.new_net_port(Getter, sock_addr, Passive).unwrap();
            c.connect(Some(Duration::from_secs(1))).unwrap();
            c.get(g).unwrap(); // not matched by put
            c.sync(None).unwrap_err(); // no timeout
        });
    })
    .unwrap();
}

#[test]
fn chain_connect() {
    let test_log_path = Path::new("./logs/chain_connect");
    let sock_addrs = [next_test_addr(), next_test_addr(), next_test_addr(), next_test_addr()];
    scope(|s| {
        s.spawn(|_| {
            let mut c = file_logged_connector(0, test_log_path);
            c.new_net_port(Putter, sock_addrs[0], Passive).unwrap();
            c.connect(Some(Duration::from_secs(1))).unwrap();
        });
        s.spawn(|_| {
            let mut c = file_logged_connector(10, test_log_path);
            c.new_net_port(Getter, sock_addrs[0], Active).unwrap();
            c.new_net_port(Putter, sock_addrs[1], Passive).unwrap();
            c.connect(Some(Duration::from_secs(1))).unwrap();
        });
        s.spawn(|_| {
            // LEADER
            let mut c = file_logged_connector(7, test_log_path);
            c.new_net_port(Getter, sock_addrs[1], Active).unwrap();
            c.new_net_port(Putter, sock_addrs[2], Passive).unwrap();
            c.connect(Some(Duration::from_secs(1))).unwrap();
        });
        s.spawn(|_| {
            let mut c = file_logged_connector(4, test_log_path);
            c.new_net_port(Getter, sock_addrs[2], Active).unwrap();
            c.new_net_port(Putter, sock_addrs[3], Passive).unwrap();
            c.connect(Some(Duration::from_secs(1))).unwrap();
        });
        s.spawn(|_| {
            let mut c = file_logged_connector(1, test_log_path);
            c.new_net_port(Getter, sock_addrs[3], Active).unwrap();
            c.connect(Some(Duration::from_secs(1))).unwrap();
        });
    })
    .unwrap();
}

#[test]
fn net_self_loop() {
    let test_log_path = Path::new("./logs/net_self_loop");
    let sock_addr = next_test_addr();
    let mut c = file_logged_connector(0, test_log_path);
    let p = c.new_net_port(Putter, sock_addr, Active).unwrap();
    let g = c.new_net_port(Getter, sock_addr, Passive).unwrap();
    c.connect(Some(Duration::from_secs(1))).unwrap();
    c.put(p, TEST_MSG.clone()).unwrap();
    c.get(g).unwrap();
    c.sync(Some(Duration::from_millis(500))).unwrap();
}
