use super::*;
use atomic_refcell::AtomicRefCell;

use std::{collections::HashMap, ffi::c_void, net::SocketAddr, os::raw::c_int, sync::RwLock};
///////////////////////////////////////////////////////////////////

struct FdAllocator {
    next: Option<c_int>,
    freed: Vec<c_int>,
}
enum MaybeConnector {
    New,
    Bound { local_addr: SocketAddr },
    Connected { connector: Connector, putter: PortId, getter: PortId },
}
#[derive(Default)]
struct ConnectorStorage {
    fd_to_connector: HashMap<c_int, AtomicRefCell<MaybeConnector>>,
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
    static ref CONNECTOR_STORAGE: RwLock<ConnectorStorage> = Default::default();
}
///////////////////////////////////////////////////////////////////

#[no_mangle]
pub extern "C" fn rw_socket(_domain: c_int, _type: c_int) -> c_int {
    // assuming _domain is AF_INET and _type is SOCK_DGRAM
    let mut w = if let Ok(w) = CONNECTOR_STORAGE.write() { w } else { return FD_LOCK_POISONED };
    let fd = w.fd_allocator.alloc();
    w.fd_to_connector.insert(fd, AtomicRefCell::new(MaybeConnector::New));
    fd
}

#[no_mangle]
pub extern "C" fn rw_close(fd: c_int, _how: c_int) -> c_int {
    // ignoring HOW
    let mut w = if let Ok(w) = CONNECTOR_STORAGE.write() { w } else { return FD_LOCK_POISONED };
    w.fd_allocator.free(fd);
    if w.fd_to_connector.remove(&fd).is_some() {
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
    use MaybeConnector as Mc;
    // assuming _domain is AF_INET and _type is SOCK_DGRAM
    let r = if let Ok(r) = CONNECTOR_STORAGE.read() { r } else { return FD_LOCK_POISONED };
    let mc = if let Some(mc) = r.fd_to_connector.get(&fd) { mc } else { return BAD_FD };
    let mc: &mut Mc = &mut mc.borrow_mut();
    let _ = if let Mc::New = mc { () } else { return WRONG_STATE };
    *mc = Mc::Bound { local_addr: local_addr.read() };
    ERR_OK
}

#[no_mangle]
pub unsafe extern "C" fn rw_connect(
    fd: c_int,
    peer_addr: *const SocketAddr,
    _address_len: usize,
) -> c_int {
    use MaybeConnector as Mc;
    // assuming _domain is AF_INET and _type is SOCK_DGRAM
    let r = if let Ok(r) = CONNECTOR_STORAGE.read() { r } else { return FD_LOCK_POISONED };
    let mc = if let Some(mc) = r.fd_to_connector.get(&fd) { mc } else { return BAD_FD };
    let mc: &mut Mc = &mut mc.borrow_mut();
    let local_addr =
        if let Mc::Bound { local_addr } = mc { local_addr } else { return WRONG_STATE };
    let peer_addr = peer_addr.read();
    let (connector, [putter, getter]) = {
        let mut c = Connector::new(
            Box::new(DummyLogger),
            crate::TRIVIAL_PD.clone(),
            Connector::random_id(),
            8,
        );
        let [putter, getter] = c.new_udp_port(*local_addr, peer_addr).unwrap();
        (c, [putter, getter])
    };
    *mc = Mc::Connected { connector, putter, getter };
    ERR_OK
}
#[no_mangle]
pub unsafe extern "C" fn rw_send(
    fd: c_int,
    bytes_ptr: *const c_void,
    bytes_len: usize,
    _flags: c_int,
) -> isize {
    use MaybeConnector as Mc;
    // ignoring flags
    let r =
        if let Ok(r) = CONNECTOR_STORAGE.read() { r } else { return FD_LOCK_POISONED as isize };
    let mc = if let Some(mc) = r.fd_to_connector.get(&fd) { mc } else { return BAD_FD as isize };
    let mc: &mut Mc = &mut mc.borrow_mut();
    let (connector, putter) = if let Mc::Connected { connector, putter, .. } = mc {
        (connector, *putter)
    } else {
        return WRONG_STATE as isize;
    };
    match connector_put_bytes(connector, putter, bytes_ptr as _, bytes_len) {
        ERR_OK => {}
        err => return err as isize,
    }
    connector_sync(connector, -1)
}

#[no_mangle]
pub unsafe extern "C" fn rw_recv(
    fd: c_int,
    bytes_ptr: *mut c_void,
    bytes_len: usize,
    _flags: c_int,
) -> isize {
    use MaybeConnector as Mc;
    // ignoring flags
    let r =
        if let Ok(r) = CONNECTOR_STORAGE.read() { r } else { return FD_LOCK_POISONED as isize };
    let mc = if let Some(mc) = r.fd_to_connector.get(&fd) { mc } else { return BAD_FD as isize };
    let mc: &mut Mc = &mut mc.borrow_mut();
    let (connector, getter) = if let Mc::Connected { connector, getter, .. } = mc {
        (connector, *getter)
    } else {
        return WRONG_STATE as isize;
    };
    match connector_get(connector, getter) {
        ERR_OK => {}
        err => return err as isize,
    }
    match connector_sync(connector, -1) {
        0 => {} // singleton batch index
        err => return err as isize,
    };
    let slice = connector.gotten(getter).unwrap().as_slice();
    let copied_bytes = slice.len().min(bytes_len);
    std::ptr::copy_nonoverlapping(slice.as_ptr(), bytes_ptr as *mut u8, copied_bytes);
    copied_bytes as isize
}
