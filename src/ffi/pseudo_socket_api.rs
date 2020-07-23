use super::*;

use libc::{sockaddr, socklen_t};
use std::{
    collections::HashMap,
    ffi::c_void,
    net::SocketAddr,
    os::raw::c_int,
    sync::RwLock,
};
///////////////////////////////////////////////////////////////////

struct FdAllocator {
    next: Option<c_int>,
    freed: Vec<c_int>,
}
enum ConnectorComplex {
    Setup {
        local: Option<SocketAddr>,
        peer: Option<SocketAddr>,
    },
    Communication {
        connector: Connector,
        putter: PortId,
        getter: PortId,
    },
}
#[derive(Default)]
struct CcMap {
    fd_to_cc: HashMap<c_int, RwLock<ConnectorComplex>>,
    fd_allocator: FdAllocator,
}
///////////////////////////////////////////////////////////////////
unsafe fn payload_from_raw(bytes_ptr: *const c_void, bytes_len: usize) -> Payload {
    let bytes_ptr = std::mem::transmute(bytes_ptr);
    let bytes = &*slice_from_raw_parts(bytes_ptr, bytes_len);
    Payload::from(bytes)
}
unsafe fn libc_to_std_sockaddr(addr: *const sockaddr, addr_len: socklen_t) -> Option<SocketAddr> {
    os_socketaddr::OsSocketAddr::from_raw_parts(addr as _, addr_len as usize).into_addr()
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
    fn try_become_connected(&mut self) {
        match self {
            ConnectorComplex::Setup { local: Some(local), peer: Some(peer) } => {
                // setup complete
                let mut connector = Connector::new(Box::new(crate::DummyLogger), crate::TRIVIAL_PD.clone(), Connector::random_id());
                let [putter, getter] = connector.new_udp_mediator_component(*local, *peer).unwrap();
                *self = ConnectorComplex::Communication { connector, putter, getter }
            }
            _ => {} // setup incomplete
        }
    }
}
/////////////////////////////////
#[no_mangle]
pub extern "C" fn rw_socket(_domain: c_int, _type: c_int) -> c_int {
    // ignoring domain and type
    // get writer lock
    let mut w = if let Ok(w) = CC_MAP.write() { w } else { return LOCK_POISONED };
    let fd = w.fd_allocator.alloc();
    let cc = ConnectorComplex::Setup { local: None, peer: None };
    w.fd_to_cc.insert(fd, RwLock::new(cc));
    fd
}
#[no_mangle]
pub extern "C" fn rw_close(fd: c_int, _how: c_int) -> c_int {
    // ignoring HOW
    // get writer lock
    let mut w = if let Ok(w) = CC_MAP.write() { w } else { return LOCK_POISONED };
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
    let addr = match libc_to_std_sockaddr(addr, addr_len) {
        Some(addr) => addr,
        _ => return BAD_SOCKADDR,
    };
    // get outer reader, inner writer locks
    let r = if let Ok(r) = CC_MAP.read() { r } else { return LOCK_POISONED };
    let cc = if let Some(cc) = r.fd_to_cc.get(&fd) { cc } else { return BAD_FD };
    let mut cc = if let Ok(cc) = cc.write() { cc } else { return LOCK_POISONED };
    let cc: &mut ConnectorComplex = &mut cc;
    match cc {
        ConnectorComplex::Communication { ..} => WRONG_STATE,
        ConnectorComplex::Setup { local, .. } => {
            *local = Some(addr);
            cc.try_become_connected();
            ERR_OK
        }
    }
}
#[no_mangle]
pub unsafe extern "C" fn rw_connect(
    fd: c_int,
    addr: *const sockaddr,
    addr_len: socklen_t,
) -> c_int {
    let addr = match libc_to_std_sockaddr(addr, addr_len) {
        Some(addr) => addr,
        _ => return BAD_SOCKADDR,
    };
    // assuming _domain is AF_INET and _type is SOCK_DGRAM
    // get outer reader, inner writer locks
    let r = if let Ok(r) = CC_MAP.read() { r } else { return LOCK_POISONED };
    let cc = if let Some(cc) = r.fd_to_cc.get(&fd) { cc } else { return BAD_FD };
    let mut cc = if let Ok(cc) = cc.write() { cc } else { return LOCK_POISONED };
    let cc: &mut ConnectorComplex = &mut cc;
    match cc {
        ConnectorComplex::Communication { .. } => WRONG_STATE,
        ConnectorComplex::Setup { peer, .. } => {
            *peer = Some(addr);
            cc.try_become_connected();
            ERR_OK
        }
    }
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
    let r = if let Ok(r) = CC_MAP.read() { r } else { return LOCK_POISONED as isize };
    let cc = if let Some(cc) = r.fd_to_cc.get(&fd) { cc } else { return BAD_FD as isize };
    let mut cc = if let Ok(cc) = cc.write() { cc } else { return LOCK_POISONED as isize };
    let cc: &mut ConnectorComplex = &mut cc;
    match cc {
        ConnectorComplex::Setup { .. } => WRONG_STATE as isize,
        ConnectorComplex::Communication { connector, putter, .. } => {
            let payload = payload_from_raw(bytes_ptr, bytes_len);
            connector.put(*putter, payload).unwrap();
            connector.sync(None).unwrap();
            bytes_len as isize
        }
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
    let r = if let Ok(r) = CC_MAP.read() { r } else { return LOCK_POISONED as isize };
    let cc = if let Some(cc) = r.fd_to_cc.get(&fd) { cc } else { return BAD_FD as isize };
    let mut cc = if let Ok(cc) = cc.write() { cc } else { return LOCK_POISONED as isize };
    let cc: &mut ConnectorComplex = &mut cc;
    match cc {
        ConnectorComplex::Setup { .. } => WRONG_STATE as isize,
        ConnectorComplex::Communication { connector, getter, .. } => {
            connector.get(*getter).unwrap();
            connector.sync(None).unwrap();
            let slice = connector.gotten(*getter).unwrap().as_slice();
            if !bytes_ptr.is_null() {
                let cpy_msg_bytes = slice.len().min(bytes_len);
                std::ptr::copy_nonoverlapping(slice.as_ptr(), bytes_ptr as *mut u8, cpy_msg_bytes);
            }
            slice.len() as isize
        }
    }
}