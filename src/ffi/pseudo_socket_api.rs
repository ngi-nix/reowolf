use super::*;

use libc::{sockaddr, socklen_t};
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
struct ConnectorComplex {
    // invariants:
    // 1. connector is a upd-socket singleton
    // 2. putter and getter are ports in the native interface with the appropriate polarities
    // 3. connected_to always mirrors connector's single udp socket's connect addr. both are overwritten together.
    conencted_to: Option<SocketAddr>,
    connector_bound: Option<ConnectorBound>,
}
#[derive(Default)]
struct CcMap {
    fd_to_cc: HashMap<c_int, RwLock<ConnectorComplex>>,
    fd_allocator: FdAllocator,
}
///////////////////////////////////////////////////////////////////
unsafe fn addr_from_raw(addr: *const sockaddr, addr_len: socklen_t) -> Option<SocketAddr> {
    os_socketaddr::OsSocketAddr::from_raw_parts(addr as _, addr_len as usize).into_addr()
}
fn dummy_peer_addr() -> SocketAddr {
    // SocketAddrV4::new isn't a constant-time func
    SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(127, 0, 0, 0), 8000))
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
    // get writer lock
    let mut w = if let Ok(w) = CC_MAP.write() { w } else { return CC_MAP_LOCK_POISONED };
    let fd = w.fd_allocator.alloc();
    let cc = ConnectorComplex { peer_addr: dummy_peer_addr(), connector_bound: None };
    w.fd_to_cc.insert(fd, RwLock::new(cc));
    fd
}

#[no_mangle]
pub extern "C" fn rw_close(fd: c_int, _how: c_int) -> c_int {
    // ignoring HOW
    // get writer lock
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
    // assuming _domain is AF_INET and _type is SOCK_DGRAM
    let addr = match addr_from_raw(addr, addr_len) {
        Some(addr) => addr,
        _ => return BAD_SOCKADDR,
    };
    // get outer reader, inner writer locks
    let r = if let Ok(r) = CC_MAP.read() { r } else { return CC_MAP_LOCK_POISONED };
    let cc = if let Some(cc) = r.fd_to_cc.get(&fd) { cc } else { return BAD_FD };
    let mut cc = if let Ok(cc) = cc.write() { cc } else { return CC_MAP_LOCK_POISONED };
    let cc: &mut ConnectorComplex = &mut cc;
    if cc.connector_bound.is_some() {
        // already bound!
        return WRONG_STATE;
    }
    cc.connector_bound = {
        let mut connector = Connector::new(
            Box::new(crate::DummyLogger),
            crate::TRIVIAL_PD.clone(),
            Connector::random_id(),
        );
        // maintain invariant: if cc.connected_to.is_some():
        //   cc.connected_to matches the connected address of the socket
        let peer_addr = cc.connected_to.unwrap_or_with(dummy_peer_addr);
        let [putter, getter] = connector.new_udp_mediator_component(addr, peer_addr).unwrap();
        Some(ConnectorBound { connector, putter, getter })
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
    // get outer reader, inner writer locks
    let r = if let Ok(r) = CC_MAP.read() { r } else { return CC_MAP_LOCK_POISONED };
    let cc = if let Some(cc) = r.fd_to_cc.get(&fd) { cc } else { return BAD_FD };
    let mut cc = if let Ok(cc) = cc.write() { cc } else { return CC_MAP_LOCK_POISONED };
    let cc: &mut ConnectorComplex = &mut cc;
    if let Some(ConnectorBound { connector, .. }) = &mut cc.connector_bound {
        // already bound. maintain invariant by overwriting the socket's connection (DUMMY or otherwise)
        if connector.get_mut_udp_ee(0).unwrap().sock.connect(peer_addr).is_err() {
            return CONNECT_FAILED;
        }
    }
    cc.connected_to = Some(addr);
    ERR_OK
}

#[no_mangle]
pub unsafe extern "C" fn rw_send(
    fd: c_int,
    bytes_ptr: *const c_void,
    bytes_len: usize,
    _flags: c_int,
) -> isize {
    // ignoring flags
    // get outer reader, inner writer locks
    let r = if let Ok(r) = CC_MAP.read() { r } else { return CC_MAP_LOCK_POISONED as isize };
    let cc = if let Some(cc) = r.fd_to_cc.get(&fd) { cc } else { return BAD_FD as isize };
    let mut cc = if let Ok(cc) = cc.write() { cc } else { return CC_MAP_LOCK_POISONED as isize };
    let cc: &mut ConnectorComplex = &mut cc;
    if cc.connected_to.is_none() {
        return SEND_BEFORE_CONNECT;
    }
    if let Some(ConnectorBound { connector, putter, .. }) = &mut cc.connector_bound {
        // is bound
        let bytes = &*slice_from_raw_parts(bytes_ptr, bytes_len);
        connector.put(putter, Payload::from_bytes(bytes)).unwrap();
        connector.sync(connector, None).unwrap();
        bytes_len as isize
    } else {
        // is not bound
        WRONG_STATE as isize
    }
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
    // ignoring flags
    let addr = match addr_from_raw(addr, addr_len) {
        Some(addr) => addr,
        _ => return BAD_SOCKADDR,
    };
    // get outer reader, inner writer locks
    let r = if let Ok(r) = CC_MAP.read() { r } else { return CC_MAP_LOCK_POISONED as isize };
    let cc = if let Some(cc) = r.fd_to_cc.get(&fd) { cc } else { return BAD_FD as isize };
    let mut cc = if let Ok(cc) = cc.write() { cc } else { return CC_MAP_LOCK_POISONED as isize };
    let cc: &mut ConnectorComplex = &mut cc;
    if let Some(ConnectorBound { connector, putter, .. }) = &mut cc.connector_bound {
        // is bound
        // (temporarily) break invariant
        if connector.get_mut_udp_ee(0).unwrap().sock.connect(addr).is_err() {
            // invariant not broken. nevermind
            return CONNECT_FAILED;
        }
        // invariant broken...
        let bytes = &*slice_from_raw_parts(bytes_ptr, bytes_len);
        connector.put(putter, Payload::from_bytes(bytes)).unwrap();
        connector.sync(connector, None).unwrap();
        let old_addr = cc.connected_to.unwrap_or_with(dummy_peer_addr)
        connector.get_mut_udp_ee(0).unwrap().sock.connect(addr).unwrap();
        // ...invariant restored
        bytes_len as isize
    } else {
        // is not bound
        WRONG_STATE as isize
    }
}

#[no_mangle]
pub unsafe extern "C" fn rw_recv(
    fd: c_int,
    bytes_ptr: *mut c_void,
    bytes_len: usize,
    _flags: c_int,
) -> isize {
    // ignoring flags
    // get outer reader, inner writer locks
    let r = if let Ok(r) = CC_MAP.read() { r } else { return CC_MAP_LOCK_POISONED as isize };
    let cc = if let Some(cc) = r.fd_to_cc.get(&fd) { cc } else { return BAD_FD as isize };
    let mut cc = if let Ok(cc) = cc.write() { cc } else { return CC_MAP_LOCK_POISONED as isize };
    let cc: &mut ConnectorComplex = &mut cc;
    if let Some(ConnectorBound { connector, getter, .. }) = &mut self.connector_bound {
        connector.get(getter).unwrap();
        // this call BLOCKS until it succeeds, and its got no reason to fail
        connector.sync(connector, None).unwrap();
        // copy from gotten to caller's buffer (truncating if necessary)
        let slice = connector.gotten(*getter).unwrap().as_slice();
        let cpy_msg_bytes = slice.len().min(bytes_len);
        std::ptr::copy_nonoverlapping(slice.as_ptr(), bytes_ptr as *mut u8, cpy_msg_bytes);
        // return number of bytes sent
        cpy_msg_bytes as isize
    } else {
        WRONG_STATE as isize // not bound!
    }
}

#[no_mangle]
pub unsafe extern "C" fn rw_recvfrom(
    fd: c_int,
    bytes_ptr: *mut c_void,
    bytes_len: usize,
    _flags: c_int,
    addr: *mut sockaddr,
    addr_len: *mut socklen_t,
) -> isize {
    let addr = match addr_from_raw(addr, addr_len) {
        Some(addr) => addr,
        _ => return BAD_SOCKADDR as isize,
    };
    // ignoring flags
    // get outer reader, inner writer locks
    let r = if let Ok(r) = CC_MAP.read() { r } else { return CC_MAP_LOCK_POISONED as isize };
    let cc = if let Some(cc) = r.fd_to_cc.get(&fd) { cc } else { return BAD_FD as isize };
    let mut cc = if let Ok(cc) = cc.write() { cc } else { return CC_MAP_LOCK_POISONED as isize };
    let cc: &mut ConnectorComplex = &mut cc;
    if let Some(ConnectorBound { connector, getter, .. }) = &mut self.connector_bound {
        connector.get(getter).unwrap();
        // this call BLOCKS until it succeeds, and its got no reason to fail
        connector.sync(connector, None).unwrap();
        // overwrite addr and addr_len
        let addr = connector.get_mut_udp_ee(0).unwrap().received_from_this_round.unwrap();
        let os_addr = os_socketaddr::OsSocketAddr::from(addr);
        let cpy_addr_bytes = (*addr_len).min(os_addr.capacity());
        // ptr-return addr bytes (truncated to addr_len)
        std::ptr::copy_nonoverlapping(os_addr.as_ptr(), addr as *mut u8, cpy_addr_bytes);
        // ptr-return true addr size
        *addr_len = os_addr.capacity(); 
        // copy from gotten to caller's buffer (truncating if necessary)
        let slice = connector.gotten(*getter).unwrap().as_slice();
        let cpy_msg_bytes = slice.len().min(bytes_len);
        std::ptr::copy_nonoverlapping(slice.as_ptr(), bytes_ptr as *mut u8, cpy_msg_bytes);
        // return number of bytes received
        cpy_msg_bytes as isize
    } else {
        WRONG_STATE as isize // not bound!
    }
}
