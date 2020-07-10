use super::*;
use atomic_refcell::AtomicRefCell;
use std::{collections::HashMap, ffi::c_void, net::SocketAddr, os::raw::c_int, sync::RwLock};
///////////////////////////////////////////////////////////////////

type ConnectorFd = c_int;
struct Connector {}
struct FdAllocator {
    next: Option<ConnectorFd>,
    freed: Vec<ConnectorFd>,
}
enum MaybeConnector {
    New,
    Bound(SocketAddr),
    Connected(Connector),
}
#[derive(Default)]
struct ConnectorStorage {
    fd_to_connector: HashMap<ConnectorFd, AtomicRefCell<MaybeConnector>>,
    fd_allocator: FdAllocator,
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
    fn alloc(&mut self) -> ConnectorFd {
        if let Some(fd) = self.freed.pop() {
            return fd;
        }
        if let Some(fd) = self.next {
            self.next = fd.checked_add(1);
            return fd;
        }
        panic!("No more Connector FDs to allocate!")
    }
    fn free(&mut self, fd: ConnectorFd) {
        self.freed.push(fd);
    }
}
lazy_static::lazy_static! {
    static ref CONNECTOR_STORAGE: RwLock<ConnectorStorage> = Default::default();
}
///////////////////////////////////////////////////////////////////

#[no_mangle]
pub extern "C" fn socket(_domain: c_int, _type: c_int) -> c_int {
    // assuming _domain is AF_INET and _type is SOCK_DGRAM
    let w = if let Some(w) = CONNECTOR_STORAGE.write() { w } else { return FD_LOCK_POISONED };
    let fd = w.fd_allocator.alloc();
    w.fd_to_connector.insert(fd, AtomicRefCell::new(MaybeConnector::New));
    fd
}

#[no_mangle]
pub extern "C" fn close(fd: ConnectorFd, _how: c_int) -> c_int {
    // ignoring HOW
    let w = if let Some(w) = CONNECTOR_STORAGE.write() { w } else { return FD_LOCK_POISONED };
    w.fd_allocator.free(fd);
    if w.fd_to_connector.remove(&fd).is_some() {
        ERR_OK
    } else {
        CLOSE_FAIL
    }
}

#[no_mangle]
pub extern "C" fn bind(fd: ConnectorFd, address: *const SocketAddr, _address_len: usize) -> c_int {
    use MaybeConnector as Mc;
    // assuming _domain is AF_INET and _type is SOCK_DGRAM
    let r = if let Some(r) = CONNECTOR_STORAGE.read() { r } else { return FD_LOCK_POISONED };
    let mc = if let Some(mc) = r.fd_to_connector.get(&fd) { mc } else { return BAD_FD };
    let mc: &mut Mc = &mut maybe_conn.borrow_mut();
    let _ = if let Mc::New = mc { () } else { return WRONG_STATE };
    *mc = Mc::Bound(address.read());
    ERR_OK
}

#[no_mangle]
pub extern "C" fn connect(
    fd: ConnectorFd,
    _address: *const SocketAddr,
    _address_len: usize,
) -> c_int {
    use MaybeConnector as Mc;
    // assuming _domain is AF_INET and _type is SOCK_DGRAM
    let r = if let Some(r) = CONNECTOR_STORAGE.read() { r } else { return FD_LOCK_POISONED };
    let mc = if let Some(mc) = r.fd_to_connector.get(&fd) { mc } else { return BAD_FD };
    let mc: &mut Mc = &mut maybe_conn.borrow_mut();
    let local = if let Mc::Bound(local) = mc { local } else { return WRONG_STATE };
    *mc = Mc::Connected(Connector {});
    ERR_OK
}
#[no_mangle]
pub extern "C" fn send(fd: ConnectorFd, msg: *const c_void, len: usize, flags: c_int) -> isize {
    use MaybeConnector as Mc;
    // assuming _domain is AF_INET and _type is SOCK_DGRAM
    let r = if let Some(r) = CONNECTOR_STORAGE.read() { r } else { return FD_LOCK_POISONED };
    let mc = if let Some(mc) = r.fd_to_connector.get(&fd) { mc } else { return BAD_FD };
    let mc: &mut Mc = &mut maybe_conn.borrow_mut();
    let c = if let Mc::Connected(c) = mc { c } else { return WRONG_STATE };
    // TODO
    ERR_OK
}
