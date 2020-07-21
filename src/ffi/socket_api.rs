use super::*;

use std::{
    collections::HashMap,
    ffi::c_void,
    net::{Ipv4Addr, SocketAddr, SocketAddrV4},
    os::raw::c_int,
    sync::RwLock,
};
///////////////////////////////////////////////////////////////////

struct FdAllocator {
    next: Option<c_int>,
    freed: Vec<c_int>,
}
struct ConnectorBound {
    connector: Connector,
    putter: PortId,
    getter: PortId,
}
struct MaybeConnector {
    // invariants:
    // 1. connector is a upd-socket singleton
    // 2. putter and getter are ports in the native interface with the appropriate polarities
    // 3. peer_addr always mirrors connector's single udp socket's connect addr. both are overwritten together.
    peer_addr: SocketAddr,
    connector_bound: Option<ConnectorBound>,
}
#[derive(Default)]
struct FdcStorage {
    fd_to_c: HashMap<c_int, RwLock<MaybeConnector>>,
    fd_allocator: FdAllocator,
}
fn trivial_peer_addr() -> SocketAddr {
    // SocketAddrV4::new isn't a constant-time func
    SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(0, 0, 0, 0), 0))
}
///////////////////////////////////////////////////////////////////

impl Default for FdAllocator {
    fn default() -> Self {
        Self {
            next: Some(0), // positive values used only
            freed: vec![],
        }
    }
}
impl FdAllocator {
    fn alloc(&mut self) -> c_int {
        if let Some(fd) = self.freed.pop() {
            return fd;
        }
        if let Some(fd) = self.next {
            self.next = fd.checked_add(1);
            return fd;
        }
        panic!("No more Connector FDs to allocate!")
    }
    fn free(&mut self, fd: c_int) {
        self.freed.push(fd);
    }
}
lazy_static::lazy_static! {
    static ref FDC_STORAGE: RwLock<FdcStorage> = Default::default();
}
impl MaybeConnector {
    fn connect(&mut self, peer_addr: SocketAddr) -> c_int {
        self.peer_addr = peer_addr;
        if let Some(ConnectorBound { connector, .. }) = &mut self.connector_bound {
            if connector.get_mut_udp_sock(0).unwrap().connect(peer_addr).is_err() {
                return CONNECT_FAILED;
            }
        }
        ERR_OK
    }
    unsafe fn send(&mut self, bytes_ptr: *const c_void, bytes_len: usize) -> isize {
        if let Some(ConnectorBound { connector, putter, .. }) = &mut self.connector_bound {
            match connector_put_bytes(connector, *putter, bytes_ptr as _, bytes_len) {
                ERR_OK => connector_sync(connector, -1),
                err => err as isize,
            }
        } else {
            WRONG_STATE as isize // not bound!
        }
    }
    unsafe fn recv(&mut self, bytes_ptr: *const c_void, bytes_len: usize) -> isize {
        if let Some(ConnectorBound { connector, getter, .. }) = &mut self.connector_bound {
            connector_get(connector, *getter);
            match connector_sync(connector, -1) {
                0 => {
                    // batch index 0 means OK
                    let slice = connector.gotten(*getter).unwrap().as_slice();
                    let copied_bytes = slice.len().min(bytes_len);
                    std::ptr::copy_nonoverlapping(
                        slice.as_ptr(),
                        bytes_ptr as *mut u8,
                        copied_bytes,
                    );
                    copied_bytes as isize
                }
                err => return err as isize,
            }
        } else {
            WRONG_STATE as isize // not bound!
        }
    }
}
///////////////////////////////////////////////////////////////////

#[no_mangle]
pub extern "C" fn rw_socket(_domain: c_int, _type: c_int) -> c_int {
    // ignoring domain and type
    let mut w = if let Ok(w) = FDC_STORAGE.write() { w } else { return FD_LOCK_POISONED };
    let fd = w.fd_allocator.alloc();
    let mc = MaybeConnector { peer_addr: trivial_peer_addr(), connector_bound: None };
    w.fd_to_c.insert(fd, RwLock::new(mc));
    fd
}

#[no_mangle]
pub extern "C" fn rw_close(fd: c_int, _how: c_int) -> c_int {
    // ignoring HOW
    let mut w = if let Ok(w) = FDC_STORAGE.write() { w } else { return FD_LOCK_POISONED };
    if w.fd_to_c.remove(&fd).is_some() {
        w.fd_allocator.free(fd);
        ERR_OK
    } else {
        CLOSE_FAIL
    }
}

#[no_mangle]
pub unsafe extern "C" fn rw_bind(
    fd: c_int,
    local_addr: *const SocketAddr,
    _addr_len: usize,
) -> c_int {
    // assuming _domain is AF_INET and _type is SOCK_DGRAM
    let r = if let Ok(r) = FDC_STORAGE.read() { r } else { return FD_LOCK_POISONED };
    let mc = if let Some(mc) = r.fd_to_c.get(&fd) { mc } else { return BAD_FD };
    let mut mc = if let Ok(mc) = mc.write() { mc } else { return FD_LOCK_POISONED };
    let mc: &mut MaybeConnector = &mut mc;
    if mc.connector_bound.is_some() {
        return WRONG_STATE;
    }
    mc.connector_bound = {
        let mut connector = Connector::new(
            Box::new(crate::DummyLogger),
            crate::TRIVIAL_PD.clone(),
            Connector::random_id(),
        );
        let [putter, getter] =
            connector.new_udp_mediator_component(local_addr.read(), mc.peer_addr).unwrap();
        Some(ConnectorBound { connector, putter, getter })
    };
    ERR_OK
}

#[no_mangle]
pub unsafe extern "C" fn rw_connect(
    fd: c_int,
    peer_addr: *const SocketAddr,
    _address_len: usize,
) -> c_int {
    // assuming _domain is AF_INET and _type is SOCK_DGRAM
    let r = if let Ok(r) = FDC_STORAGE.read() { r } else { return FD_LOCK_POISONED };
    let mc = if let Some(mc) = r.fd_to_c.get(&fd) { mc } else { return BAD_FD };
    let mut mc = if let Ok(mc) = mc.write() { mc } else { return FD_LOCK_POISONED };
    let mc: &mut MaybeConnector = &mut mc;
    mc.connect(peer_addr.read())
}

#[no_mangle]
pub unsafe extern "C" fn rw_send(
    fd: c_int,
    bytes_ptr: *const c_void,
    bytes_len: usize,
    _flags: c_int,
) -> isize {
    // ignoring flags
    let r = if let Ok(r) = FDC_STORAGE.read() { r } else { return FD_LOCK_POISONED as isize };
    let mc = if let Some(mc) = r.fd_to_c.get(&fd) { mc } else { return BAD_FD as isize };
    let mut mc = if let Ok(mc) = mc.write() { mc } else { return FD_LOCK_POISONED as isize };
    let mc: &mut MaybeConnector = &mut mc;
    mc.send(bytes_ptr, bytes_len)
}

#[no_mangle]
pub unsafe extern "C" fn rw_recv(
    fd: c_int,
    bytes_ptr: *mut c_void,
    bytes_len: usize,
    _flags: c_int,
) -> isize {
    // ignoring flags
    let r = if let Ok(r) = FDC_STORAGE.read() { r } else { return FD_LOCK_POISONED as isize };
    let mc = if let Some(mc) = r.fd_to_c.get(&fd) { mc } else { return BAD_FD as isize };
    let mut mc = if let Ok(mc) = mc.write() { mc } else { return FD_LOCK_POISONED as isize };
    let mc: &mut MaybeConnector = &mut mc;
    mc.recv(bytes_ptr, bytes_len)
}

#[no_mangle]
pub unsafe extern "C" fn rw_sendto(
    fd: c_int,
    bytes_ptr: *mut c_void,
    bytes_len: usize,
    _flags: c_int,
    peer_addr: *const SocketAddr,
    _addr_len: usize,
) -> isize {
    let r = if let Ok(r) = FDC_STORAGE.read() { r } else { return FD_LOCK_POISONED as isize };
    let mc = if let Some(mc) = r.fd_to_c.get(&fd) { mc } else { return BAD_FD as isize };
    let mut mc = if let Ok(mc) = mc.write() { mc } else { return FD_LOCK_POISONED as isize };
    let mc: &mut MaybeConnector = &mut mc;
    // copy currently connected peer addr
    let connected = mc.peer_addr;
    // connect to given peer_addr
    match mc.connect(peer_addr.read()) {
        e if e != ERR_OK => return e as isize,
        _ => {}
    }
    // send
    let ret = mc.send(bytes_ptr, bytes_len);
    // restore connected peer addr
    match mc.connect(connected) {
        e if e != ERR_OK => return e as isize,
        _ => {}
    }
    ret
}

#[no_mangle]
#[no_mangle]
pub unsafe extern "C" fn rw_recvfrom(
    fd: c_int,
    bytes_ptr: *mut c_void,
    bytes_len: usize,
    _flags: c_int,
    peer_addr: *const SocketAddr,
    _addr_len: usize,
) -> isize {
    let r = if let Ok(r) = FDC_STORAGE.read() { r } else { return FD_LOCK_POISONED as isize };
    let mc = if let Some(mc) = r.fd_to_c.get(&fd) { mc } else { return BAD_FD as isize };
    let mut mc = if let Ok(mc) = mc.write() { mc } else { return FD_LOCK_POISONED as isize };
    let mc: &mut MaybeConnector = &mut mc;
    // copy currently connected peer addr
    let connected = mc.peer_addr;
    // connect to given peer_addr
    match mc.connect(peer_addr.read()) {
        e if e != ERR_OK => return e as isize,
        _ => {}
    }
    // send
    let ret = mc.send(bytes_ptr, bytes_len);
    // restore connected peer addr
    match mc.connect(connected) {
        e if e != ERR_OK => return e as isize,
        _ => {}
    }
    ret
}
