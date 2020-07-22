use super::*;

use std::{
    collections::HashMap,
    ffi::c_void,
    libc::{sockaddr, socklen_t},
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
    is_nonblocking: bool,
    putter: PortId,
    getter: PortId,
}
struct ConnectorComplex {
    // invariants:
    // 1. connector is a upd-socket singleton
    // 2. putter and getter are ports in the native interface with the appropriate polarities
    // 3. peer_addr always mirrors connector's single udp socket's connect addr. both are overwritten together.
    peer_addr: SocketAddr,
    connector_bound: Option<ConnectorBound>,
}
#[derive(Default)]
struct CcMap {
    fd_to_cc: HashMap<c_int, RwLock<ConnectorComplex>>,
    fd_allocator: FdAllocator,
}
///////////////////////////////////////////////////////////////////
fn addr_from_raw(addr: *const sockaddr, addr_len: socklen_t) -> Option<SocketAddr> {
    os_socketaddr::OsSocketAddr::from_raw_parts(addr, addr_len as usize).into_addr()
}
fn trivial_peer_addr() -> SocketAddr {
    // SocketAddrV4::new isn't a constant-time func
    SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(0, 0, 0, 0), 0))
}
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
    static ref CC_MAP: RwLock<CcMap> = Default::default();
}
impl ConnectorComplex {
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
    let mut w = if let Ok(w) = CC_MAP.write() { w } else { return CC_MAP_LOCK_POISONED };
    let fd = w.fd_allocator.alloc();
    let cc = ConnectorComplex { peer_addr: trivial_peer_addr(), connector_bound: None };
    w.fd_to_cc.insert(fd, RwLock::new(cc));
    fd
}

#[no_mangle]
pub extern "C" fn rw_close(fd: c_int, _how: c_int) -> c_int {
    // ignoring HOW
    let mut w = if let Ok(w) = CC_MAP.write() { w } else { return CC_MAP_LOCK_POISONED };
    if w.fd_to_cc.remove(&fd).is_some() {
        w.fd_allocator.free(fd);
        ERR_OK
    } else {
        CLOSE_FAIL
    }
}

#[no_mangle]
pub unsafe extern "C" fn rw_bind(fd: c_int, addr: *const sockaddr, addr_len: socklen_t) -> c_int {
    let addr = match addr_from_raw(addr, addr_len) {
        Some(addr) => addr,
        _ => return BAD_SOCKADDR,
    };
    // assuming _domain is AF_INET and _type is SOCK_DGRAM
    let r = if let Ok(r) = CC_MAP.read() { r } else { return CC_MAP_LOCK_POISONED };
    let cc = if let Some(cc) = r.fd_to_cc.get(&fd) { cc } else { return BAD_FD };
    let mut cc = if let Ok(cc) = cc.write() { cc } else { return CC_MAP_LOCK_POISONED };
    let cc: &mut ConnectorComplex = &mut cc;
    if cc.connector_bound.is_some() {
        return WRONG_STATE;
    }
    cc.connector_bound = {
        let mut connector = Connector::new(
            Box::new(crate::DummyLogger),
            crate::TRIVIAL_PD.clone(),
            Connector::random_id(),
        );
        let [putter, getter] = connector.new_udp_mediator_component(addr, cc.peer_addr).unwrap();
        Some(ConnectorBound { connector, putter, getter, is_nonblocking: false })
    };
    ERR_OK
}

#[no_mangle]
pub unsafe extern "C" fn rw_connect(
    fd: c_int,
    addr: *const sockaddr,
    addr_len: socklen_t,
) -> c_int {
    let addr = match addr_from_raw(addr, addr_len) {
        Some(addr) => addr,
        _ => return BAD_SOCKADDR,
    };
    // assuming _domain is AF_INET and _type is SOCK_DGRAM
    let r = if let Ok(r) = CC_MAP.read() { r } else { return CC_MAP_LOCK_POISONED };
    let cc = if let Some(cc) = r.fd_to_cc.get(&fd) { cc } else { return BAD_FD };
    let mut cc = if let Ok(cc) = cc.write() { cc } else { return CC_MAP_LOCK_POISONED };
    let cc: &mut ConnectorComplex = &mut cc;
    cc.connect(addr)
}

#[no_mangle]
pub unsafe extern "C" fn rw_send(
    fd: c_int,
    bytes_ptr: *const c_void,
    bytes_len: usize,
    _flags: c_int,
) -> isize {
    // ignoring flags
    let r = if let Ok(r) = CC_MAP.read() { r } else { return CC_MAP_LOCK_POISONED as isize };
    let cc = if let Some(cc) = r.fd_to_cc.get(&fd) { cc } else { return BAD_FD as isize };
    let mut cc = if let Ok(cc) = cc.write() { cc } else { return CC_MAP_LOCK_POISONED as isize };
    let cc: &mut ConnectorComplex = &mut cc;
    cc.send(bytes_ptr, bytes_len)
}

#[no_mangle]
pub unsafe extern "C" fn rw_recv(
    fd: c_int,
    bytes_ptr: *mut c_void,
    bytes_len: usize,
    _flags: c_int,
) -> isize {
    // ignoring flags
    let r = if let Ok(r) = CC_MAP.read() { r } else { return CC_MAP_LOCK_POISONED as isize };
    let cc = if let Some(cc) = r.fd_to_cc.get(&fd) { cc } else { return BAD_FD as isize };
    let mut cc = if let Ok(cc) = cc.write() { cc } else { return CC_MAP_LOCK_POISONED as isize };
    let cc: &mut ConnectorComplex = &mut cc;
    cc.recv(bytes_ptr, bytes_len)
}

#[no_mangle]
pub unsafe extern "C" fn rw_sendto(
    fd: c_int,
    bytes_ptr: *mut c_void,
    bytes_len: usize,
    _flags: c_int,
    addr: *const sockaddr,
    addr_len: socklen_t,
) -> isize {
    let addr = match addr_from_raw(addr, addr_len) {
        Some(addr) => addr,
        _ => return BAD_SOCKADDR,
    };
    let r = if let Ok(r) = CC_MAP.read() { r } else { return CC_MAP_LOCK_POISONED as isize };
    let cc = if let Some(cc) = r.fd_to_cc.get(&fd) { cc } else { return BAD_FD as isize };
    let mut cc = if let Ok(cc) = cc.write() { cc } else { return CC_MAP_LOCK_POISONED as isize };
    let cc: &mut ConnectorComplex = &mut cc;
    // copy currently old_addr
    let old_addr = cc.peer_addr;
    // connect to given peer_addr
    match cc.connect(addr) {
        e if e != ERR_OK => return e as isize,
        _ => {}
    }
    // send
    let ret = cc.send(bytes_ptr, bytes_len);
    // restore old_addr
    match cc.connect(old_addr) {
        e if e != ERR_OK => return e as isize,
        _ => {}
    }
    ret
}

#[no_mangle]
pub unsafe extern "C" fn rw_recvfrom(
    fd: c_int,
    bytes_ptr: *mut c_void,
    bytes_len: usize,
    _flags: c_int,
    addr: *const sockaddr,
    addr_len: socklen_t,
) -> isize {
    let addr = match addr_from_raw(addr, addr_len) {
        Some(addr) => addr,
        _ => return BAD_SOCKADDR,
    };
    let r = if let Ok(r) = CC_MAP.read() { r } else { return CC_MAP_LOCK_POISONED as isize };
    let cc = if let Some(cc) = r.fd_to_cc.get(&fd) { cc } else { return BAD_FD as isize };
    let mut cc = if let Ok(cc) = cc.write() { cc } else { return CC_MAP_LOCK_POISONED as isize };
    let cc: &mut ConnectorComplex = &mut cc;
    // copy currently old_addr
    let old_addr = cc.peer_addr;
    // connect to given peer_addr
    match cc.connect(peer_addr.read()) {
        e if e != ERR_OK => return e as isize,
        _ => {}
    }
    // send
    let ret = cc.send(bytes_ptr, bytes_len);
    // restore old_addr
    match cc.connect(old_addr) {
        e if e != ERR_OK => return e as isize,
        _ => {}
    }
    ret
}
