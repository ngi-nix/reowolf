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
const MS100: Option<Duration> = Some(Duration::from_millis(100));
const MS300: Option<Duration> = Some(Duration::from_millis(300));
const SEC1: Option<Duration> = Some(Duration::from_secs(1));
const SEC5: Option<Duration> = Some(Duration::from_secs(5));
const SEC15: Option<Duration> = Some(Duration::from_secs(15));
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
    file_logged_configured_connector(connector_id, dir_path, MINIMAL_PROTO.clone())
}
fn file_logged_configured_connector(
    connector_id: ConnectorId,
    dir_path: &Path,
    pd: Arc<ProtocolDescription>,
) -> Connector {
    let _ = std::fs::create_dir_all(dir_path).expect("Failed to create log output dir");
    let path = dir_path.join(format!("cid_{:?}.txt", connector_id));
    let file = File::create(path).expect("Failed to create log output file!");
    let file_logger = Box::new(FileLogger::new(connector_id, file));
    Connector::new(file_logger, pd, connector_id)
}
static MINIMAL_PDL: &'static [u8] = b"
primitive together(in<msg> ia, in<msg> ib, out<msg> oa, out<msg> ob){
  while(true) synchronous {
    if(fires(ia)) {
      put(oa, get(ia));
      put(ob, get(ib));
    }
  } 
}
";
lazy_static::lazy_static! {
    static ref MINIMAL_PROTO: Arc<ProtocolDescription> = {
        Arc::new(reowolf::ProtocolDescription::parse(MINIMAL_PDL).unwrap())
    };
}
static TEST_MSG_BYTES: &'static [u8] = b"hello";
lazy_static::lazy_static! {
    static ref TEST_MSG: Payload = {
        Payload::from(TEST_MSG_BYTES)
    };
}
fn new_u8_buffer(cap: usize) -> Vec<u8> {
    let mut v = Vec::with_capacity(cap);
    // Safe! len will cover owned bytes in valid state
    unsafe { v.set_len(cap) }
    v
}
//////////////////////////////////////////

#[test]
fn basic_connector() {
    Connector::new(Box::new(DummyLogger), MINIMAL_PROTO.clone(), 0);
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
    c.add_component(b"", b"sync", &[i, o]).unwrap();
}

#[test]
fn new_net_port() {
    let test_log_path = Path::new("./logs/new_net_port");
    let mut c = file_logged_connector(0, test_log_path);
    let sock_addrs = [next_test_addr()];
    let _ = c.new_net_port(Getter, sock_addrs[0], Passive).unwrap();
    let _ = c.new_net_port(Putter, sock_addrs[0], Active).unwrap();
}

#[test]
fn trivial_connect() {
    let test_log_path = Path::new("./logs/trivial_connect");
    let mut c = file_logged_connector(0, test_log_path);
    c.connect(SEC1).unwrap();
}

#[test]
fn single_node_connect() {
    let test_log_path = Path::new("./logs/single_node_connect");
    let sock_addrs = [next_test_addr()];
    let mut c = file_logged_connector(0, test_log_path);
    let _ = c.new_net_port(Getter, sock_addrs[0], Passive).unwrap();
    let _ = c.new_net_port(Putter, sock_addrs[0], Active).unwrap();
    c.connect(SEC1).unwrap();
}

#[test]
fn minimal_net_connect() {
    let test_log_path = Path::new("./logs/minimal_net_connect");
    let sock_addrs = [next_test_addr()];
    scope(|s| {
        s.spawn(|_| {
            let mut c = file_logged_connector(0, test_log_path);
            let _ = c.new_net_port(Getter, sock_addrs[0], Active).unwrap();
            c.connect(SEC1).unwrap();
        });
        s.spawn(|_| {
            let mut c = file_logged_connector(1, test_log_path);
            let _ = c.new_net_port(Putter, sock_addrs[0], Passive).unwrap();
            c.connect(SEC1).unwrap();
        });
    })
    .unwrap();
}

#[test]
fn put_no_sync() {
    let test_log_path = Path::new("./logs/put_no_sync");
    let mut c = file_logged_connector(0, test_log_path);
    let [o, _] = c.new_port_pair();
    c.connect(SEC1).unwrap();
    c.put(o, TEST_MSG.clone()).unwrap();
}

#[test]
fn wrong_polarity_bad() {
    let test_log_path = Path::new("./logs/wrong_polarity_bad");
    let mut c = file_logged_connector(0, test_log_path);
    let [_, i] = c.new_port_pair();
    c.connect(SEC1).unwrap();
    c.put(i, TEST_MSG.clone()).unwrap_err();
}

#[test]
fn dup_put_bad() {
    let test_log_path = Path::new("./logs/dup_put_bad");
    let mut c = file_logged_connector(0, test_log_path);
    let [o, _] = c.new_port_pair();
    c.connect(SEC1).unwrap();
    c.put(o, TEST_MSG.clone()).unwrap();
    c.put(o, TEST_MSG.clone()).unwrap_err();
}

#[test]
fn trivial_sync() {
    let test_log_path = Path::new("./logs/trivial_sync");
    let mut c = file_logged_connector(0, test_log_path);
    c.connect(SEC1).unwrap();
    c.sync(SEC1).unwrap();
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
    c.connect(SEC1).unwrap();
    assert_eq!(reowolf::error::GottenError::NoPreviousRound, c.gotten(i).unwrap_err());
}

#[test]
fn connected_gotten_err_ungotten() {
    let test_log_path = Path::new("./logs/connected_gotten_err_ungotten");
    let mut c = file_logged_connector(0, test_log_path);
    let [_, i] = c.new_port_pair();
    c.connect(SEC1).unwrap();
    c.sync(SEC1).unwrap();
    assert_eq!(reowolf::error::GottenError::PortDidntGet, c.gotten(i).unwrap_err());
}

#[test]
fn native_polarity_checks() {
    let test_log_path = Path::new("./logs/native_polarity_checks");
    let mut c = file_logged_connector(0, test_log_path);
    let [o, i] = c.new_port_pair();
    c.connect(SEC1).unwrap();
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
    c.connect(SEC1).unwrap();
    c.get(i).unwrap();
    c.get(i).unwrap_err();
}

#[test]
fn next_batch() {
    let test_log_path = Path::new("./logs/next_batch");
    let mut c = file_logged_connector(0, test_log_path);
    c.next_batch().unwrap_err();
    c.connect(SEC1).unwrap();
    c.next_batch().unwrap();
    c.next_batch().unwrap();
    c.next_batch().unwrap();
}

#[test]
fn native_self_msg() {
    let test_log_path = Path::new("./logs/native_self_msg");
    let mut c = file_logged_connector(0, test_log_path);
    let [o, i] = c.new_port_pair();
    c.connect(SEC1).unwrap();
    c.get(i).unwrap();
    c.put(o, TEST_MSG.clone()).unwrap();
    c.sync(SEC1).unwrap();
}

#[test]
fn two_natives_msg() {
    let test_log_path = Path::new("./logs/two_natives_msg");
    let sock_addrs = [next_test_addr()];
    scope(|s| {
        s.spawn(|_| {
            let mut c = file_logged_connector(0, test_log_path);
            let g = c.new_net_port(Getter, sock_addrs[0], Active).unwrap();
            c.connect(SEC1).unwrap();
            c.get(g).unwrap();
            c.sync(SEC1).unwrap();
            c.gotten(g).unwrap();
        });
        s.spawn(|_| {
            let mut c = file_logged_connector(1, test_log_path);
            let p = c.new_net_port(Putter, sock_addrs[0], Passive).unwrap();
            c.connect(SEC1).unwrap();
            c.put(p, TEST_MSG.clone()).unwrap();
            c.sync(SEC1).unwrap();
        });
    })
    .unwrap();
}

#[test]
fn trivial_nondet() {
    let test_log_path = Path::new("./logs/trivial_nondet");
    let mut c = file_logged_connector(0, test_log_path);
    let [_, i] = c.new_port_pair();
    c.connect(SEC1).unwrap();
    c.get(i).unwrap();
    // getting 0 batch
    c.next_batch().unwrap();
    // silent 1 batch
    assert_eq!(1, c.sync(SEC1).unwrap());
    c.gotten(i).unwrap_err();
}

#[test]
fn connector_pair_nondet() {
    let test_log_path = Path::new("./logs/connector_pair_nondet");
    let sock_addrs = [next_test_addr()];
    scope(|s| {
        s.spawn(|_| {
            let mut c = file_logged_connector(0, test_log_path);
            let g = c.new_net_port(Getter, sock_addrs[0], Active).unwrap();
            c.connect(SEC1).unwrap();
            c.next_batch().unwrap();
            c.get(g).unwrap();
            assert_eq!(1, c.sync(SEC1).unwrap());
            c.gotten(g).unwrap();
        });
        s.spawn(|_| {
            let mut c = file_logged_connector(1, test_log_path);
            let p = c.new_net_port(Putter, sock_addrs[0], Passive).unwrap();
            c.connect(SEC1).unwrap();
            c.put(p, TEST_MSG.clone()).unwrap();
            c.sync(SEC1).unwrap();
        });
    })
    .unwrap();
}

#[test]
fn native_immediately_inconsistent() {
    let test_log_path = Path::new("./logs/native_immediately_inconsistent");
    let mut c = file_logged_connector(0, test_log_path);
    let [_, g] = c.new_port_pair();
    c.connect(SEC1).unwrap();
    c.get(g).unwrap();
    c.sync(SEC15).unwrap_err();
}

#[test]
fn native_recovers() {
    let test_log_path = Path::new("./logs/native_recovers");
    let mut c = file_logged_connector(0, test_log_path);
    let [p, g] = c.new_port_pair();
    c.connect(SEC1).unwrap();
    c.get(g).unwrap();
    c.sync(SEC15).unwrap_err();
    c.put(p, TEST_MSG.clone()).unwrap();
    c.get(g).unwrap();
    c.sync(SEC15).unwrap();
}

#[test]
fn cannot_use_moved_ports() {
    /*
    native p|-->|g sync
    */
    let test_log_path = Path::new("./logs/cannot_use_moved_ports");
    let mut c = file_logged_connector(0, test_log_path);
    let [p, g] = c.new_port_pair();
    c.add_component(b"", b"sync", &[g, p]).unwrap();
    c.connect(SEC1).unwrap();
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
    c.add_component(b"", b"sync", &[g0, p1]).unwrap();
    c.connect(SEC1).unwrap();
    c.put(p0, TEST_MSG.clone()).unwrap();
    c.get(g1).unwrap();
    c.sync(SEC1).unwrap();
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
            c.connect(SEC1).unwrap();
        });
        s.spawn(|_| {
            let mut c = file_logged_connector(1, test_log_path);
            let [_g, _p] = [
                c.new_net_port(Getter, sock_addrs[0], Passive).unwrap(),
                c.new_net_port(Putter, sock_addrs[1], Passive).unwrap(),
            ];
            c.connect(SEC1).unwrap();
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
            c.add_component(b"", b"sync", &[g, p]).unwrap();
            c.connect(SEC1).unwrap();
            c.sync(SEC1).unwrap();
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
            c.connect(SEC1).unwrap();
            c.put(p, TEST_MSG.clone()).unwrap();
            c.get(g).unwrap();
            c.sync(SEC1).unwrap();
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
    c.connect(SEC1).unwrap();
    c.get(g).unwrap();
    match c.sync(MS300) {
        Err(SyncError::RoundFailure) => {}
        res => panic!("expeted timeout. but got {:?}", res),
    }
}

#[test]
fn parent_timeout() {
    let test_log_path = Path::new("./logs/parent_timeout");
    let sock_addrs = [next_test_addr()];
    scope(|s| {
        s.spawn(|_| {
            // parent; times out
            let mut c = file_logged_connector(999, test_log_path);
            let _ = c.new_net_port(Putter, sock_addrs[0], Active).unwrap();
            c.connect(SEC1).unwrap();
            c.sync(MS300).unwrap_err(); // timeout
        });
        s.spawn(|_| {
            // child
            let mut c = file_logged_connector(000, test_log_path);
            let g = c.new_net_port(Getter, sock_addrs[0], Passive).unwrap();
            c.connect(SEC1).unwrap();
            c.get(g).unwrap(); // not matched by put
            c.sync(None).unwrap_err(); // no timeout
        });
    })
    .unwrap();
}

#[test]
fn child_timeout() {
    let test_log_path = Path::new("./logs/child_timeout");
    let sock_addrs = [next_test_addr()];
    scope(|s| {
        s.spawn(|_| {
            // child; times out
            let mut c = file_logged_connector(000, test_log_path);
            let _ = c.new_net_port(Putter, sock_addrs[0], Active).unwrap();
            c.connect(SEC1).unwrap();
            c.sync(MS300).unwrap_err(); // timeout
        });
        s.spawn(|_| {
            // parent
            let mut c = file_logged_connector(999, test_log_path);
            let g = c.new_net_port(Getter, sock_addrs[0], Passive).unwrap();
            c.connect(SEC1).unwrap();
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
            c.connect(SEC5).unwrap();
        });
        s.spawn(|_| {
            let mut c = file_logged_connector(10, test_log_path);
            c.new_net_port(Getter, sock_addrs[0], Active).unwrap();
            c.new_net_port(Putter, sock_addrs[1], Passive).unwrap();
            c.connect(SEC5).unwrap();
        });
        s.spawn(|_| {
            // LEADER
            let mut c = file_logged_connector(7, test_log_path);
            c.new_net_port(Getter, sock_addrs[1], Active).unwrap();
            c.new_net_port(Putter, sock_addrs[2], Passive).unwrap();
            c.connect(SEC5).unwrap();
        });
        s.spawn(|_| {
            let mut c = file_logged_connector(4, test_log_path);
            c.new_net_port(Getter, sock_addrs[2], Active).unwrap();
            c.new_net_port(Putter, sock_addrs[3], Passive).unwrap();
            c.connect(SEC5).unwrap();
        });
        s.spawn(|_| {
            let mut c = file_logged_connector(1, test_log_path);
            c.new_net_port(Getter, sock_addrs[3], Active).unwrap();
            c.connect(SEC5).unwrap();
        });
    })
    .unwrap();
}

#[test]
fn net_self_loop() {
    let test_log_path = Path::new("./logs/net_self_loop");
    let sock_addrs = [next_test_addr()];
    let mut c = file_logged_connector(0, test_log_path);
    let p = c.new_net_port(Putter, sock_addrs[0], Active).unwrap();
    let g = c.new_net_port(Getter, sock_addrs[0], Passive).unwrap();
    c.connect(SEC1).unwrap();
    c.put(p, TEST_MSG.clone()).unwrap();
    c.get(g).unwrap();
    c.sync(MS300).unwrap();
}

#[test]
fn nobody_connects_active() {
    let test_log_path = Path::new("./logs/nobody_connects_active");
    let sock_addrs = [next_test_addr()];
    let mut c = file_logged_connector(0, test_log_path);
    let _g = c.new_net_port(Getter, sock_addrs[0], Active).unwrap();
    c.connect(Some(Duration::from_secs(5))).unwrap_err();
}
#[test]
fn nobody_connects_passive() {
    let test_log_path = Path::new("./logs/nobody_connects_passive");
    let sock_addrs = [next_test_addr()];
    let mut c = file_logged_connector(0, test_log_path);
    let _g = c.new_net_port(Getter, sock_addrs[0], Passive).unwrap();
    c.connect(Some(Duration::from_secs(5))).unwrap_err();
}

#[test]
fn together() {
    let test_log_path = Path::new("./logs/together");
    let sock_addrs = [next_test_addr(), next_test_addr()];
    scope(|s| {
        s.spawn(|_| {
            let mut c = file_logged_connector(0, test_log_path);
            let [p0, p1] = c.new_port_pair();
            let p2 = c.new_net_port(Getter, sock_addrs[0], Passive).unwrap();
            let p3 = c.new_net_port(Putter, sock_addrs[1], Active).unwrap();
            let [p4, p5] = c.new_port_pair();
            c.add_component(b"", b"together", &[p1, p2, p3, p4]).unwrap();
            c.connect(SEC1).unwrap();
            c.put(p0, TEST_MSG.clone()).unwrap();
            c.get(p5).unwrap();
            c.sync(MS300).unwrap();
            c.gotten(p5).unwrap();
        });
        s.spawn(|_| {
            let mut c = file_logged_connector(1, test_log_path);
            let [p0, p1] = c.new_port_pair();
            let p2 = c.new_net_port(Getter, sock_addrs[1], Passive).unwrap();
            let p3 = c.new_net_port(Putter, sock_addrs[0], Active).unwrap();
            let [p4, p5] = c.new_port_pair();
            c.add_component(b"", b"together", &[p1, p2, p3, p4]).unwrap();
            c.connect(SEC1).unwrap();
            c.put(p0, TEST_MSG.clone()).unwrap();
            c.get(p5).unwrap();
            c.sync(MS300).unwrap();
            c.gotten(p5).unwrap();
        });
    })
    .unwrap();
}

#[test]
fn native_batch_distinguish() {
    let test_log_path = Path::new("./logs/native_batch_distinguish");
    let mut c = file_logged_connector(0, test_log_path);
    c.connect(SEC1).unwrap();
    c.next_batch().unwrap();
    c.sync(SEC1).unwrap();
}

#[test]
fn multirounds() {
    let test_log_path = Path::new("./logs/multirounds");
    let sock_addrs = [next_test_addr(), next_test_addr()];
    scope(|s| {
        s.spawn(|_| {
            let mut c = file_logged_connector(0, test_log_path);
            let p0 = c.new_net_port(Putter, sock_addrs[0], Active).unwrap();
            let p1 = c.new_net_port(Getter, sock_addrs[1], Passive).unwrap();
            c.connect(SEC1).unwrap();
            for _ in 0..10 {
                c.put(p0, TEST_MSG.clone()).unwrap();
                c.get(p1).unwrap();
                c.sync(SEC1).unwrap();
            }
        });
        s.spawn(|_| {
            let mut c = file_logged_connector(1, test_log_path);
            let p0 = c.new_net_port(Getter, sock_addrs[0], Passive).unwrap();
            let p1 = c.new_net_port(Putter, sock_addrs[1], Active).unwrap();
            c.connect(SEC1).unwrap();
            for _ in 0..10 {
                c.get(p0).unwrap();
                c.put(p1, TEST_MSG.clone()).unwrap();
                c.sync(SEC1).unwrap();
            }
        });
    })
    .unwrap();
}

#[test]
fn multi_recover() {
    let test_log_path = Path::new("./logs/multi_recover");
    let sock_addrs = [next_test_addr(), next_test_addr()];
    let success_iter = [true, false].iter().copied().cycle().take(10);
    scope(|s| {
        s.spawn(|_| {
            let mut c = file_logged_connector(0, test_log_path);
            let p0 = c.new_net_port(Putter, sock_addrs[0], Active).unwrap();
            let p1 = c.new_net_port(Getter, sock_addrs[1], Passive).unwrap();
            c.connect(SEC1).unwrap();
            for succeeds in success_iter.clone() {
                c.put(p0, TEST_MSG.clone()).unwrap();
                if succeeds {
                    c.get(p1).unwrap();
                }
                let res = c.sync(MS300);
                assert_eq!(res.is_ok(), succeeds);
            }
        });
        s.spawn(|_| {
            let mut c = file_logged_connector(1, test_log_path);
            let p0 = c.new_net_port(Getter, sock_addrs[0], Passive).unwrap();
            let p1 = c.new_net_port(Putter, sock_addrs[1], Active).unwrap();
            c.connect(SEC1).unwrap();
            for succeeds in success_iter.clone() {
                c.get(p0).unwrap();
                c.put(p1, TEST_MSG.clone()).unwrap();
                let res = c.sync(MS300);
                assert_eq!(res.is_ok(), succeeds);
            }
        });
    })
    .unwrap();
}

#[test]
fn udp_self_connect() {
    let test_log_path = Path::new("./logs/udp_self_connect");
    let sock_addrs = [next_test_addr(), next_test_addr()];
    let mut c = file_logged_connector(0, test_log_path);
    c.new_udp_mediator_component(sock_addrs[0], sock_addrs[1]).unwrap();
    c.new_udp_mediator_component(sock_addrs[1], sock_addrs[0]).unwrap();
    c.connect(SEC1).unwrap();
}

#[test]
fn solo_udp_put_success() {
    let test_log_path = Path::new("./logs/solo_udp_put_success");
    let sock_addrs = [next_test_addr(), next_test_addr()];
    let mut c = file_logged_connector(0, test_log_path);
    let [p0, _] = c.new_udp_mediator_component(sock_addrs[0], sock_addrs[1]).unwrap();
    c.connect(SEC1).unwrap();
    c.put(p0, TEST_MSG.clone()).unwrap();
    c.sync(MS300).unwrap();
}

#[test]
fn solo_udp_get_fail() {
    let test_log_path = Path::new("./logs/solo_udp_get_fail");
    let sock_addrs = [next_test_addr(), next_test_addr()];
    let mut c = file_logged_connector(0, test_log_path);
    let [_, p0] = c.new_udp_mediator_component(sock_addrs[0], sock_addrs[1]).unwrap();
    c.connect(SEC1).unwrap();
    c.get(p0).unwrap();
    c.sync(MS300).unwrap_err();
}

#[ignore]
#[test]
fn reowolf_to_udp() {
    let test_log_path = Path::new("./logs/reowolf_to_udp");
    let sock_addrs = [next_test_addr(), next_test_addr()];
    let barrier = std::sync::Barrier::new(2);
    scope(|s| {
        s.spawn(|_| {
            barrier.wait();
            // reowolf thread
            let mut c = file_logged_connector(0, test_log_path);
            let [p0, _] = c.new_udp_mediator_component(sock_addrs[0], sock_addrs[1]).unwrap();
            c.connect(SEC1).unwrap();
            c.put(p0, TEST_MSG.clone()).unwrap();
            c.sync(MS300).unwrap();
            barrier.wait();
        });
        s.spawn(|_| {
            barrier.wait();
            // udp thread
            let udp = std::net::UdpSocket::bind(sock_addrs[1]).unwrap();
            udp.connect(sock_addrs[0]).unwrap();
            let mut buf = new_u8_buffer(256);
            let len = udp.recv(&mut buf).unwrap();
            assert_eq!(TEST_MSG_BYTES, &buf[0..len]);
            barrier.wait();
        });
    })
    .unwrap();
}

#[ignore]
#[test]
fn udp_to_reowolf() {
    let test_log_path = Path::new("./logs/udp_to_reowolf");
    let sock_addrs = [next_test_addr(), next_test_addr()];
    let barrier = std::sync::Barrier::new(2);
    scope(|s| {
        s.spawn(|_| {
            barrier.wait();
            // reowolf thread
            let mut c = file_logged_connector(0, test_log_path);
            let [_, p0] = c.new_udp_mediator_component(sock_addrs[0], sock_addrs[1]).unwrap();
            c.connect(SEC1).unwrap();
            c.get(p0).unwrap();
            c.sync(SEC5).unwrap();
            assert_eq!(c.gotten(p0).unwrap().as_slice(), TEST_MSG_BYTES);
            barrier.wait();
        });
        s.spawn(|_| {
            barrier.wait();
            // udp thread
            let udp = std::net::UdpSocket::bind(sock_addrs[1]).unwrap();
            udp.connect(sock_addrs[0]).unwrap();
            for _ in 0..15 {
                udp.send(TEST_MSG_BYTES).unwrap();
                std::thread::sleep(MS100.unwrap());
            }
            barrier.wait();
        });
    })
    .unwrap();
}

#[test]
fn udp_reowolf_swap() {
    let test_log_path = Path::new("./logs/udp_reowolf_swap");
    let sock_addrs = [next_test_addr(), next_test_addr()];
    let barrier = std::sync::Barrier::new(2);
    scope(|s| {
        s.spawn(|_| {
            barrier.wait();
            // reowolf thread
            let mut c = file_logged_connector(0, test_log_path);
            let [p0, p1] = c.new_udp_mediator_component(sock_addrs[0], sock_addrs[1]).unwrap();
            c.connect(SEC1).unwrap();
            c.put(p0, TEST_MSG.clone()).unwrap();
            c.get(p1).unwrap();
            c.sync(SEC5).unwrap();
            assert_eq!(c.gotten(p1).unwrap().as_slice(), TEST_MSG_BYTES);
            barrier.wait();
        });
        s.spawn(|_| {
            barrier.wait();
            // udp thread
            let udp = std::net::UdpSocket::bind(sock_addrs[1]).unwrap();
            udp.connect(sock_addrs[0]).unwrap();
            let mut buf = new_u8_buffer(256);
            for _ in 0..5 {
                std::thread::sleep(Duration::from_millis(60));
                udp.send(TEST_MSG_BYTES).unwrap();
            }
            let len = udp.recv(&mut buf).unwrap();
            assert_eq!(TEST_MSG_BYTES, &buf[0..len]);
            barrier.wait();
        });
    })
    .unwrap();
}

#[test]
fn example_pres_3() {
    let test_log_path = Path::new("./logs/example_pres_3");
    let sock_addrs = [next_test_addr(), next_test_addr()];
    scope(|s| {
        s.spawn(|_| {
            // "amy"
            let mut c = file_logged_connector(0, test_log_path);
            let p0 = c.new_net_port(Putter, sock_addrs[0], Active).unwrap();
            let p1 = c.new_net_port(Putter, sock_addrs[1], Active).unwrap();
            c.connect(SEC1).unwrap();
            // put {A} and FAIL
            c.put(p0, TEST_MSG.clone()).unwrap();
            c.sync(SEC1).unwrap_err();
            // put {B} and FAIL
            c.put(p1, TEST_MSG.clone()).unwrap();
            c.sync(SEC1).unwrap_err();
            // put {A, B} and SUCCEED
            c.put(p0, TEST_MSG.clone()).unwrap();
            c.put(p1, TEST_MSG.clone()).unwrap();
            c.sync(SEC1).unwrap();
        });
        s.spawn(|_| {
            // "bob"
            let mut c = file_logged_connector(1, test_log_path);
            let p0 = c.new_net_port(Getter, sock_addrs[0], Passive).unwrap();
            let p1 = c.new_net_port(Getter, sock_addrs[1], Passive).unwrap();
            c.connect(SEC1).unwrap();
            for _ in 0..2 {
                // get {A, B} and FAIL
                c.get(p0).unwrap();
                c.get(p1).unwrap();
                c.sync(SEC1).unwrap_err();
            }
            // get {A, B} and SUCCEED
            c.get(p0).unwrap();
            c.get(p1).unwrap();
            c.sync(SEC1).unwrap();
        });
    })
    .unwrap();
}

#[test]
fn ac_not_b() {
    let test_log_path = Path::new("./logs/ac_not_b");
    let sock_addrs = [next_test_addr(), next_test_addr()];
    scope(|s| {
        s.spawn(|_| {
            // "amy"
            let mut c = file_logged_connector(0, test_log_path);
            let p0 = c.new_net_port(Putter, sock_addrs[0], Active).unwrap();
            let p1 = c.new_net_port(Putter, sock_addrs[1], Active).unwrap();
            c.connect(SEC5).unwrap();

            // put both A and B
            c.put(p0, TEST_MSG.clone()).unwrap();
            c.put(p1, TEST_MSG.clone()).unwrap();
            c.sync(SEC1).unwrap_err();
        });
        s.spawn(|_| {
            // "bob"
            let pdl = b"
            primitive ac_not_b(in<msg> a, in<msg> b, out<msg> c){
                // forward A to C but keep B silent
                synchronous{ put(c, get(a)); }
            }";
            let pd = Arc::new(reowolf::ProtocolDescription::parse(pdl).unwrap());
            let mut c = file_logged_configured_connector(1, test_log_path, pd);
            let p0 = c.new_net_port(Getter, sock_addrs[0], Passive).unwrap();
            let p1 = c.new_net_port(Getter, sock_addrs[1], Passive).unwrap();
            let [a, b] = c.new_port_pair();

            c.add_component(b"", b"ac_not_b", &[p0, p1, a]).unwrap();

            c.connect(SEC1).unwrap();

            c.get(b).unwrap();
            c.sync(SEC1).unwrap_err();
        });
    })
    .unwrap();
}

#[test]
fn many_rounds_net() {
    let test_log_path = Path::new("./logs/many_rounds_net");
    let sock_addrs = [next_test_addr()];
    const NUM_ROUNDS: usize = 1_000;
    scope(|s| {
        s.spawn(|_| {
            let mut c = file_logged_connector(0, test_log_path);
            let p0 = c.new_net_port(Putter, sock_addrs[0], Active).unwrap();
            c.connect(SEC1).unwrap();
            for _ in 0..NUM_ROUNDS {
                c.put(p0, TEST_MSG.clone()).unwrap();
                c.sync(SEC1).unwrap();
            }
        });
        s.spawn(|_| {
            let mut c = file_logged_connector(1, test_log_path);
            let p0 = c.new_net_port(Getter, sock_addrs[0], Passive).unwrap();
            c.connect(SEC1).unwrap();
            for _ in 0..NUM_ROUNDS {
                c.get(p0).unwrap();
                c.sync(SEC1).unwrap();
            }
        });
    })
    .unwrap();
}
#[test]
fn many_rounds_mem() {
    let test_log_path = Path::new("./logs/many_rounds_mem");
    const NUM_ROUNDS: usize = 1_000;
    let mut c = file_logged_connector(0, test_log_path);
    let [p0, p1] = c.new_port_pair();
    c.connect(SEC1).unwrap();
    for _ in 0..NUM_ROUNDS {
        c.put(p0, TEST_MSG.clone()).unwrap();
        c.get(p1).unwrap();
        c.sync(SEC1).unwrap();
    }
}

#[test]
fn pdl_reo_lossy() {
    let pdl = b"
    primitive lossy(in<msg> a, out<msg> b) {
        while(true) synchronous {
            msg m = null;
            if(fires(a)) {
                m = get(a);
                if(fires(b)) {
                    put(b, m);
                }
            }
        }
    }
    ";
    reowolf::ProtocolDescription::parse(pdl).unwrap();
}

#[test]
fn pdl_reo_fifo1() {
    let pdl = b"
    primitive fifo1(in<msg> a, out<msg> b) {
        msg m = null;
        while(true) synchronous {
            if(m == null) {
                if(fires(a)) m=get(a);
            } else {
                if(fires(b)) put(b, m);
                m = null;
            }
        }
    }
    ";
    reowolf::ProtocolDescription::parse(pdl).unwrap();
}

#[test]
fn pdl_reo_fifo1full() {
    let test_log_path = Path::new("./logs/pdl_reo_fifo1full");
    let pdl = b"
    primitive fifo1full(in<msg> a, out<msg> b) {
        msg m = create(0);
        while(true) synchronous {
            if(m == null) {
                if(fires(a)) m=get(a);
            } else {
                if(fires(b)) put(b, m);
                m = null;
            }
        }
    }
    ";
    let pd = reowolf::ProtocolDescription::parse(pdl).unwrap();
    let mut c = file_logged_configured_connector(0, test_log_path, Arc::new(pd));
    let [_p0, g0] = c.new_port_pair();
    let [p1, g1] = c.new_port_pair();
    c.add_component(b"", b"fifo1full", &[g0, p1]).unwrap();
    c.connect(None).unwrap();
    c.get(g1).unwrap();
    c.sync(None).unwrap();
    assert_eq!(0, c.gotten(g1).unwrap().len());
}

#[test]
fn pdl_msg_consensus() {
    let test_log_path = Path::new("./logs/pdl_msg_consensus");
    let pdl = b"
    primitive msgconsensus(in<msg> a, in<msg> b) {
        while(true) synchronous {
            msg x = get(a);
            msg y = get(b);
            assert(x == y);
        }
    }
    ";
    let pd = reowolf::ProtocolDescription::parse(pdl).unwrap();
    let mut c = file_logged_configured_connector(0, test_log_path, Arc::new(pd));
    let [p0, g0] = c.new_port_pair();
    let [p1, g1] = c.new_port_pair();
    c.add_component(b"", b"msgconsensus", &[g0, g1]).unwrap();
    c.connect(None).unwrap();
    c.put(p0, Payload::from(b"HELLO" as &[_])).unwrap();
    c.put(p1, Payload::from(b"HELLO" as &[_])).unwrap();
    c.sync(SEC1).unwrap();

    c.put(p0, Payload::from(b"HEY" as &[_])).unwrap();
    c.put(p1, Payload::from(b"HELLO" as &[_])).unwrap();
    c.sync(SEC1).unwrap_err();
}

#[test]
fn sequencer3_prim() {
    let test_log_path = Path::new("./logs/sequencer3_prim");
    let pdl = b"
    primitive sequencer3(out<msg> a, out<msg> b, out<msg> c) {
        int i = 0;
        while(true) synchronous {
            out to = a;
            if     (i==1) to = b;
            else if(i==2) to = c;
            if(fires(to)) {
                put(to, create(0));
                i = (i + 1)%3;
            }
        }
    }
    ";
    let pd = reowolf::ProtocolDescription::parse(pdl).unwrap();
    let mut c = file_logged_configured_connector(0, test_log_path, Arc::new(pd));

    // setup a session between (a) native, and (b) sequencer3, connected by 3 ports.
    let [p0, g0] = c.new_port_pair();
    let [p1, g1] = c.new_port_pair();
    let [p2, g2] = c.new_port_pair();
    c.add_component(b"", b"sequencer3", &[p0, p1, p2]).unwrap();
    c.connect(None).unwrap();

    let which_of_three = move |c: &mut Connector| {
        // setup three sync batches. sync. return which succeeded
        c.get(g0).unwrap();
        c.next_batch().unwrap();
        c.get(g1).unwrap();
        c.next_batch().unwrap();
        c.get(g2).unwrap();
        c.sync(None).unwrap()
    };

    const TEST_ROUNDS: usize = 50;
    // check that the batch index for rounds 0..TEST_ROUNDS are [0, 1, 2, 0, 1, 2, ...]
    for expected_batch_idx in (0..=2).cycle().take(TEST_ROUNDS) {
        // silent round
        assert_eq!(0, c.sync(None).unwrap());
        // non silent round
        assert_eq!(expected_batch_idx, which_of_three(&mut c));
    }
}

#[test]
fn sequencer3_comp() {
    let test_log_path = Path::new("./logs/sequencer3_comp");
    let pdl = b"
    primitive fifo1_init<T>(T m, in<T> a, out<T> b) {
        while(true) synchronous {
            if(m != null && fires(b)) {
                put(b, m);
                m = null;
            } else if (m == null && fires(a)) {
                m = get(a);
            }
        }
    }
    composite fifo1_full<T>(in<T> a, out<T> b) {
        new fifo1_init(create(0), a, b);
    }
    composite fifo1<T>(in<T> a, out<T> b) {
        new fifo1_init(null, a, b);
    }
    composite sequencer3(out<msg> a, out<msg> b, out<msg> c) {
        channel d -> e;
        channel f -> g;
        channel h -> i;
        channel j -> k;
        channel l -> m;
        channel n -> o;

        new fifo1_full(o, d);
        new replicator(e, f, a);
        new fifo1(g, h);
        new replicator(i, j, b);
        new fifo1(k, l);
        new replicator(m, n, c);
    }
    ";
    let pd = reowolf::ProtocolDescription::parse(pdl).unwrap();
    let mut c = file_logged_configured_connector(0, test_log_path, Arc::new(pd));

    // setup a session between (a) native, and (b) sequencer3, connected by 3 ports.
    let [p0, g0] = c.new_port_pair();
    let [p1, g1] = c.new_port_pair();
    let [p2, g2] = c.new_port_pair();
    c.add_component(b"", b"sequencer3", &[p0, p1, p2]).unwrap();
    c.connect(None).unwrap();

    let which_of_three = move |c: &mut Connector| {
        // setup three sync batches. sync. return which succeeded
        c.get(g0).unwrap();
        c.next_batch().unwrap();
        c.get(g1).unwrap();
        c.next_batch().unwrap();
        c.get(g2).unwrap();
        c.sync(SEC1).unwrap()
    };

    const TEST_ROUNDS: usize = 50;
    // check that the batch index for rounds 0..TEST_ROUNDS are [0, 1, 2, 0, 1, 2, ...]
    for expected_batch_idx in (0..=2).cycle().take(TEST_ROUNDS) {
        // silent round
        assert_eq!(0, c.sync(SEC1).unwrap());
        // non silent round
        assert_eq!(expected_batch_idx, which_of_three(&mut c));
    }
}

enum XRouterItem {
    Silent,
    GetA,
    GetB,
}
// Hardcoded pseudo-random sequence of round behaviors for the native component
const XROUTER_ITEMS: &[XRouterItem] = {
    use XRouterItem::{GetA as A, GetB as B, Silent as S};
    &[
        B, A, S, B, A, A, B, S, B, S, A, A, S, B, B, S, B, S, B, B, S, B, B, A, B, B, A, B, A, B,
        S, B, S, B, S, A, S, B, A, S, B, A, B, S, B, S, B, S, S, B, B, A, A, A, S, S, S, B, A, A,
        A, S, S, B, B, B, A, B, S, S, A, A, B, A, B, B, A, A, A, B, A, B, S, A, B, S, A, A, B, S,
    ]
};

#[test]
fn xrouter_prim() {
    let test_log_path = Path::new("./logs/xrouter_prim");
    let pdl = b"
    primitive xrouter(in<msg> a, out<msg> b, out<msg> c) {
        while(true) synchronous {
            if(fires(a)) {
                if(fires(b)) put(b, get(a));
                else         put(c, get(a));
            }
        }
    }
    ";
    let pd = reowolf::ProtocolDescription::parse(pdl).unwrap();
    let mut c = file_logged_configured_connector(0, test_log_path, Arc::new(pd));

    // setup a session between (a) native, and (b) xrouter2, connected by 3 ports.
    let [p0, g0] = c.new_port_pair();
    let [p1, g1] = c.new_port_pair();
    let [p2, g2] = c.new_port_pair();
    c.add_component(b"", b"xrouter", &[g0, p1, p2]).unwrap();
    c.connect(None).unwrap();

    let now = std::time::Instant::now();
    for item in XROUTER_ITEMS.iter() {
        match item {
            XRouterItem::Silent => {}
            XRouterItem::GetA => {
                c.put(p0, TEST_MSG.clone()).unwrap();
                c.get(g1).unwrap();
            }
            XRouterItem::GetB => {
                c.put(p0, TEST_MSG.clone()).unwrap();
                c.get(g2).unwrap();
            }
        }
        assert_eq!(0, c.sync(SEC1).unwrap());
    }
    println!("PRIM {:?}", now.elapsed());
}
#[test]
fn xrouter_comp() {
    let test_log_path = Path::new("./logs/xrouter_comp");
    let pdl = b"
    primitive lossy<T>(in<T> a, out<T> b) {
        while(true) synchronous {
            if(fires(a)) {
                auto m = get(a);
                if(fires(b)) put(b, m);
            }
        }
    }
    primitive sync_drain<T>(in<T> a, in<T> b) {
        while(true) synchronous {
            if(fires(a)) {
                get(a);
                get(b);
            }
        }
    }
    composite xrouter(in<msg> a, out<msg> b, out<msg> c) {
        channel d -> e;
        channel f -> g;
        channel h -> i;
        channel j -> k;
        channel l -> m;
        channel n -> o;
        channel p -> q;
        channel r -> s;
        channel t -> u;

        new replicator(a, d, f);
        new replicator(g, t, h);
        new lossy(e, l);
        new lossy(i, j);
        new replicator(m, b, p);
        new replicator(k, n, c);
        new merger(q, o, r);
        new sync_drain(u, s);
    }
    ";
    let pd = reowolf::ProtocolDescription::parse(pdl).unwrap();
    let mut c = file_logged_configured_connector(0, test_log_path, Arc::new(pd));

    // setup a session between (a) native, and (b) xrouter2, connected by 3 ports.
    let [p0, g0] = c.new_port_pair();
    let [p1, g1] = c.new_port_pair();
    let [p2, g2] = c.new_port_pair();
    c.add_component(b"", b"xrouter", &[g0, p1, p2]).unwrap();
    c.connect(None).unwrap();

    let now = std::time::Instant::now();
    for item in XROUTER_ITEMS.iter() {
        match item {
            XRouterItem::Silent => {}
            XRouterItem::GetA => {
                c.put(p0, TEST_MSG.clone()).unwrap();
                c.get(g1).unwrap();
            }
            XRouterItem::GetB => {
                c.put(p0, TEST_MSG.clone()).unwrap();
                c.get(g2).unwrap();
            }
        }
        assert_eq!(0, c.sync(SEC1).unwrap());
    }
    println!("COMP {:?}", now.elapsed());
}

#[test]
fn count_stream() {
    let test_log_path = Path::new("./logs/count_stream");
    let pdl = b"
    primitive count_stream(out<msg> o) {
        msg m = create(1);
        m[0] = 0;
        while(true) synchronous {
            put(o, m);
            m[0] += 1;
        }
    }
    ";
    let pd = reowolf::ProtocolDescription::parse(pdl).unwrap();
    let mut c = file_logged_configured_connector(0, test_log_path, Arc::new(pd));

    // setup a session between (a) native, and (b) sequencer3, connected by 3 ports.
    let [p0, g0] = c.new_port_pair();
    c.add_component(b"", b"count_stream", &[p0]).unwrap();
    c.connect(None).unwrap();

    for expecting in 0u8..16 {
        c.get(g0).unwrap();
        c.sync(None).unwrap();
        assert_eq!(&[expecting], c.gotten(g0).unwrap().as_slice());
    }
}

#[test]
fn for_msg_byte() {
    let test_log_path = Path::new("./logs/for_msg_byte");
    let pdl = b"
    primitive for_msg_byte(out<msg> o) {
        byte i = 0;
        int idx = 0;
        while(i<8) {
            msg m = create(1);
            m[idx] = i;
            synchronous put(o, m);
            i++;
        }
    }
    ";
    let pd = reowolf::ProtocolDescription::parse(pdl).unwrap();
    let mut c = file_logged_configured_connector(0, test_log_path, Arc::new(pd));

    // setup a session between (a) native, and (b) sequencer3, connected by 3 ports.
    let [p0, g0] = c.new_port_pair();
    c.add_component(b"", b"for_msg_byte", &[p0]).unwrap();
    c.connect(None).unwrap();

    for expecting in 0u8..8 {
        c.get(g0).unwrap();
        c.sync(None).unwrap();
        assert_eq!(&[expecting], c.gotten(g0).unwrap().as_slice());
    }
    c.sync(None).unwrap();
}

#[test]
fn eq_causality() {
    let test_log_path = Path::new("./logs/eq_causality");
    let pdl = b"
    primitive eq(in<msg> a, in<msg> b, out<msg> c) {
        msg ma = null;
        msg mb = null;
        while(true) synchronous {
            if(fires(a)) {
                // b and c also fire!
                // left first!
                ma = get(a);
                put(c, ma);
                mb = get(b);
                assert(ma == mb);
            }
        }
    }
    ";
    let pd = reowolf::ProtocolDescription::parse(pdl).unwrap();
    let mut c = file_logged_configured_connector(0, test_log_path, Arc::new(pd));

    /*
    [native]p0-->g0[eq]p1--.
                 g1        |
                 ^---------`
    */
    let [p0, g0] = c.new_port_pair();
    let [p1, g1] = c.new_port_pair();
    c.add_component(b"", b"eq", &[g0, g1, p1]).unwrap();

    /*
                  V--------.
                 g2        |
    [native]p2-->g3[eq]p3--`
    */
    let [p2, g2] = c.new_port_pair();
    let [p3, g3] = c.new_port_pair();
    c.add_component(b"", b"eq", &[g3, g2, p3]).unwrap();
    c.connect(None).unwrap();

    for _ in 0..4 {
        // everything is fine with LEFT FIRST
        c.put(p0, TEST_MSG.clone()).unwrap();
        c.sync(MS100).unwrap();

        // no solution when left is NOT FIRST
        c.put(p2, TEST_MSG.clone()).unwrap();
        c.sync(MS100).unwrap_err();
    }
}

#[test]
fn eq_no_causality() {
    let test_log_path = Path::new("./logs/eq_no_causality");
    let pdl = b"
    composite eq(in<msg> a, in<msg> b, out<msg> c) {
        channel leftfirsto -> leftfirsti;
        new eqinner(a, b, c, leftfirsto, leftfirsti);
    }
    primitive eqinner(in<msg> a, in<msg> b, out<msg> c, out<msg> leftfirsto, in<msg> leftfirsti) {
        msg ma = null;
        msg mb = null;
        while(true) synchronous {
            if(fires(a)) {
                // b and c also fire!
                if(fires(leftfirsti)) {
                    // left first! DO USE DUMMY
                    ma = get(a);
                    put(c, ma);
                    mb = get(b);

                    // using dummy!
                    put(leftfirsto, ma);
                    get(leftfirsti);
                } else {
                    // right first! DON'T USE DUMMY
                    mb = get(b);
                    put(c, mb);
                    ma = get(a);
                }
                assert(ma == mb);
            }
        }
    }
    T some_function<T>(int a, int b) {
        T something = a;
        return something;
    }
    primitive quick_test(in<int> a, in<int> b) {
        // msg ma = null;
        auto test1 = 0;
        auto test2 = 0;
        auto ma = some_function(test1, test2);
        while(true) synchronous {
            if (fires(a)) {
                ma = get(a);
            }
            if (fires(b)) {
                ma = get(b);
            }
            if (fires(a) && fires(b)) {
                ma = get(a) + get(b);
            }
        }
    }
    ";
    let pd = reowolf::ProtocolDescription::parse(pdl).unwrap();
    let mut c = file_logged_configured_connector(0, test_log_path, Arc::new(pd));

    /*
    [native]p0-->g0[eq]p1--.
                 g1        |
                 ^---------`
    */
    let [p0, g0] = c.new_port_pair();
    let [p1, g1] = c.new_port_pair();
    c.add_component(b"", b"eq", &[g0, g1, p1]).unwrap();

    /*
                  V--------.
                 g2        |
    [native]p2-->g3[eq]p3--`
    */
    let [p2, g2] = c.new_port_pair();
    let [p3, g3] = c.new_port_pair();
    c.add_component(b"", b"eq", &[g3, g2, p3]).unwrap();
    c.connect(None).unwrap();

    for _ in 0..32 {
        // ok when they send
        c.put(p0, TEST_MSG.clone()).unwrap();
        c.put(p2, TEST_MSG.clone()).unwrap();
        c.sync(SEC1).unwrap();
        // ok when they don't
        c.sync(SEC1).unwrap();
    }
}
