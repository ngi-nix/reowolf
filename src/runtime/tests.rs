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
    let _ = std::fs::create_dir(dir_path); // we will check failure soon
    let path = dir_path.join(format!("cid_{:?}.txt", connector_id));
    let file = File::create(path).unwrap();
    let file_logger = Box::new(FileLogger::new(connector_id, file));
    Connector::new(file_logger, MINIMAL_PROTO.clone(), connector_id, 8)
}
static MINIMAL_PDL: &'static [u8] = b"
primitive together(in ia, in ib, out oa, out ob){
  while(true) synchronous() {
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
    c.add_component(b"sync", &[g, p]).unwrap();
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
    c.add_component(b"sync", &[g0, p1]).unwrap();
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
            c.add_component(b"sync", &[g, p]).unwrap();
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
            c.add_component(b"together", &[p1, p2, p3, p4]).unwrap();
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
            c.add_component(b"together", &[p1, p2, p3, p4]).unwrap();
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
    c.new_udp_port(sock_addrs[0], sock_addrs[1]).unwrap();
    c.new_udp_port(sock_addrs[1], sock_addrs[0]).unwrap();
    c.connect(SEC1).unwrap();
}

#[test]
fn solo_udp_put_success() {
    let test_log_path = Path::new("./logs/solo_udp_put_success");
    let sock_addrs = [next_test_addr(), next_test_addr()];
    let mut c = file_logged_connector(0, test_log_path);
    let [p0, _] = c.new_udp_port(sock_addrs[0], sock_addrs[1]).unwrap();
    c.connect(SEC1).unwrap();
    c.put(p0, TEST_MSG.clone()).unwrap();
    c.sync(MS300).unwrap();
}

#[test]
fn solo_udp_get_fail() {
    let test_log_path = Path::new("./logs/solo_udp_get_fail");
    let sock_addrs = [next_test_addr(), next_test_addr()];
    let mut c = file_logged_connector(0, test_log_path);
    let [_, p0] = c.new_udp_port(sock_addrs[0], sock_addrs[1]).unwrap();
    c.connect(SEC1).unwrap();
    c.get(p0).unwrap();
    c.sync(MS300).unwrap_err();
}

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
            let [p0, _] = c.new_udp_port(sock_addrs[0], sock_addrs[1]).unwrap();
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
            let [_, p0] = c.new_udp_port(sock_addrs[0], sock_addrs[1]).unwrap();
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
            let [p0, p1] = c.new_udp_port(sock_addrs[0], sock_addrs[1]).unwrap();
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
            udp.send(TEST_MSG_BYTES).unwrap();
            let len = udp.recv(&mut buf).unwrap();
            assert_eq!(TEST_MSG_BYTES, &buf[0..len]);
            barrier.wait();
        });
    })
    .unwrap();
}

#[test]
fn pres_3() {
    let test_log_path = Path::new("./logs/pres_3");
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
