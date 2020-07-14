use crate::{common::*, runtime::*};
use core::{cell::RefCell, convert::TryFrom};
use std::os::raw::c_int;
use std::slice::from_raw_parts as slice_from_raw_parts;

#[cfg(feature = "ffi_socket_api")]
pub mod socket_api;
///////////////////////////////////////////////
#[derive(Default)]
struct StoredError {
    // invariant: len is zero IFF its occupied
    // contents are 1+ bytes because we also store the NULL TERMINATOR
    buf: Vec<u8>,
}
impl StoredError {
    const NULL_TERMINATOR: u8 = 0;
    fn clear(&mut self) {
        // no null terminator either!
        self.buf.clear();
    }
    fn debug_store<E: Debug>(&mut self, error: &E) {
        let _ = write!(&mut self.buf, "{:?}", error);
        self.buf.push(Self::NULL_TERMINATOR);
    }
    fn tl_debug_store<E: Debug>(error: &E) {
        STORED_ERROR.with(|stored_error| {
            let mut stored_error = stored_error.borrow_mut();
            stored_error.clear();
            stored_error.debug_store(error);
        })
    }
    fn bytes_store(&mut self, bytes: &[u8]) {
        let _ = self.buf.write_all(bytes);
        self.buf.push(Self::NULL_TERMINATOR);
    }
    fn tl_bytes_store(bytes: &[u8]) {
        STORED_ERROR.with(|stored_error| {
            let mut stored_error = stored_error.borrow_mut();
            stored_error.clear();
            stored_error.bytes_store(bytes);
        })
    }
    fn tl_clear() {
        STORED_ERROR.with(|stored_error| {
            let mut stored_error = stored_error.borrow_mut();
            stored_error.clear();
        })
    }
    fn tl_bytes_peek() -> (*const u8, usize) {
        STORED_ERROR.with(|stored_error| {
            let stored_error = stored_error.borrow();
            match stored_error.buf.len() {
                0 => (core::ptr::null(), 0), // no error!
                n => {
                    // stores an error of length n-1 AND a NULL TERMINATOR
                    (stored_error.buf.as_ptr(), n - 1)
                }
            }
        })
    }
}
thread_local! {
    static STORED_ERROR: RefCell<StoredError> = RefCell::new(StoredError::default());
}
unsafe fn tl_socketaddr_from_raw(
    bytes_ptr: *const u8,
    bytes_len: usize,
) -> Result<SocketAddr, c_int> {
    std::str::from_utf8(&*slice_from_raw_parts(bytes_ptr, bytes_len))
        .map_err(|err| {
            StoredError::tl_debug_store(&err);
            ERR_REOWOLF
        })?
        .parse()
        .map_err(|err| {
            StoredError::tl_debug_store(&err);
            ERR_REOWOLF
        })
}

pub const ERR_OK: c_int = 0;
pub const ERR_REOWOLF: c_int = -1;
pub const WRONG_STATE: c_int = -2;
pub const FD_LOCK_POISONED: c_int = -3;
pub const CLOSE_FAIL: c_int = -4;
pub const BAD_FD: c_int = -5;

///////////////////// REOWOLF //////////////////////////

/// Returns length (via out pointer) and pointer (via return value) of the last Reowolf error.
/// - pointer is NULL iff there was no last error
/// - data at pointer is null-delimited
/// - len does NOT include the length of the null-delimiter
/// If len is NULL, it will not written to.
#[no_mangle]
pub unsafe extern "C" fn reowolf_error_peek(len: *mut usize) -> *const u8 {
    let (err_ptr, err_len) = StoredError::tl_bytes_peek();
    if !len.is_null() {
        len.write(err_len);
    }
    err_ptr
}

///////////////////// PROTOCOL DESCRIPTION //////////////////////////

/// Parses the utf8-encoded string slice to initialize a new protocol description object.
/// - On success, initializes `out` and returns 0
/// - On failure, stores an error string (see `reowolf_error_peek`) and returns -1
#[no_mangle]
pub unsafe extern "C" fn protocol_description_parse(
    pdl: *const u8,
    pdl_len: usize,
) -> *mut Arc<ProtocolDescription> {
    StoredError::tl_clear();
    match ProtocolDescription::parse(&*slice_from_raw_parts(pdl, pdl_len)) {
        Ok(new) => Box::into_raw(Box::new(Arc::new(new))),
        Err(err) => {
            StoredError::tl_bytes_store(err.as_bytes());
            std::ptr::null_mut()
        }
    }
}

/// Destroys the given initialized protocol description and frees its resources.
#[no_mangle]
pub unsafe extern "C" fn protocol_description_destroy(pd: *mut Arc<ProtocolDescription>) {
    drop(Box::from_raw(pd))
}

/// Given an initialized protocol description, initializes `out` with a clone which can be independently created or destroyed.
#[no_mangle]
pub unsafe extern "C" fn protocol_description_clone(
    pd: &Arc<ProtocolDescription>,
) -> *mut Arc<ProtocolDescription> {
    Box::into_raw(Box::new(pd.clone()))
}

///////////////////// CONNECTOR //////////////////////////

#[no_mangle]
pub unsafe extern "C" fn connector_new_logging(
    pd: &Arc<ProtocolDescription>,
    path_ptr: *const u8,
    path_len: usize,
) -> *mut Connector {
    StoredError::tl_clear();
    let path_bytes = &*slice_from_raw_parts(path_ptr, path_len);
    let path_str = match std::str::from_utf8(path_bytes) {
        Ok(path_str) => path_str,
        Err(err) => {
            StoredError::tl_debug_store(&err);
            return std::ptr::null_mut();
        }
    };
    match std::fs::File::create(path_str) {
        Ok(file) => {
            let connector_id = Connector::random_id();
            let file_logger = Box::new(FileLogger::new(connector_id, file));
            let c = Connector::new(file_logger, pd.clone(), connector_id, 8);
            Box::into_raw(Box::new(c))
        }
        Err(err) => {
            StoredError::tl_debug_store(&err);
            std::ptr::null_mut()
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn connector_print_debug(connector: &mut Connector) {
    println!("Debug print dump {:#?}", connector);
}

/// Initializes `out` with a new connector using the given protocol description as its configuration.
/// The connector uses the given (internal) connector ID.
#[no_mangle]
pub unsafe extern "C" fn connector_new(pd: &Arc<ProtocolDescription>) -> *mut Connector {
    let c = Connector::new(Box::new(DummyLogger), pd.clone(), Connector::random_id(), 8);
    Box::into_raw(Box::new(c))
}

/// Destroys the given a pointer to the connector on the heap, freeing its resources.
/// Usable in {setup, communication} states.
#[no_mangle]
pub unsafe extern "C" fn connector_destroy(connector: *mut Connector) {
    drop(Box::from_raw(connector))
}

/// Given an initialized connector in setup or connecting state,
/// - Creates a new directed port pair with logical channel putter->getter,
/// - adds the ports to the native component's interface,
/// - and returns them using the given out pointers.
/// Usable in {setup, communication} states.
#[no_mangle]
pub unsafe extern "C" fn connector_add_port_pair(
    connector: &mut Connector,
    out_putter: *mut PortId,
    out_getter: *mut PortId,
) {
    let [o, i] = connector.new_port_pair();
    out_putter.write(o);
    out_getter.write(i);
}

/// Given
/// - an initialized connector in setup or connecting state,
/// - a string slice for the component's identifier in the connector's configured protocol description,
/// - a set of ports (represented as a slice; duplicates are ignored) in the native component's interface,
/// the connector creates a new (internal) protocol component C, such that the set of native ports are moved to C.
/// Usable in {setup, communication} states.
#[no_mangle]
pub unsafe extern "C" fn connector_add_component(
    connector: &mut Connector,
    ident_ptr: *const u8,
    ident_len: usize,
    ports_ptr: *const PortId,
    ports_len: usize,
) -> c_int {
    StoredError::tl_clear();
    match connector.add_component(
        &*slice_from_raw_parts(ident_ptr, ident_len),
        &*slice_from_raw_parts(ports_ptr, ports_len),
    ) {
        Ok(()) => ERR_OK,
        Err(err) => {
            StoredError::tl_debug_store(&err);
            ERR_REOWOLF
        }
    }
}

/// Given
/// - an initialized connector in setup or connecting state,
/// - a utf-8 encoded socket address,
/// - the logical polarity of P,
/// - the "physical" polarity in {Active, Passive} of the endpoint through which P's peer will be discovered,
/// returns P, a port newly added to the native interface.
#[no_mangle]
pub unsafe extern "C" fn connector_add_net_port(
    connector: &mut Connector,
    port: *mut PortId,
    addr_str_ptr: *const u8,
    addr_str_len: usize,
    port_polarity: Polarity,
    endpoint_polarity: EndpointPolarity,
) -> c_int {
    StoredError::tl_clear();
    let addr = match tl_socketaddr_from_raw(addr_str_ptr, addr_str_len) {
        Ok(local) => local,
        Err(errcode) => return errcode,
    };
    match connector.new_net_port(port_polarity, addr, endpoint_polarity) {
        Ok(p) => {
            if !port.is_null() {
                port.write(p);
            }
            ERR_OK
        }
        Err(err) => {
            StoredError::tl_debug_store(&err);
            ERR_REOWOLF
        }
    }
}

/// Given
/// - an initialized connector in setup or connecting state,
/// - a utf-8 encoded BIND socket addresses (i.e., "local"),
/// - a utf-8 encoded CONNECT socket addresses (i.e., "peer"),
/// returns [P, G] via out pointers [putter, getter],
/// - where P is a Putter port that sends messages into the socket
/// - where G is a Getter port that recvs messages from the socket
#[no_mangle]
pub unsafe extern "C" fn connector_add_udp_port(
    connector: &mut Connector,
    putter: *mut PortId,
    getter: *mut PortId,
    local_addr_str_ptr: *const u8,
    local_addr_str_len: usize,
    peer_addr_str_ptr: *const u8,
    peer_addr_str_len: usize,
) -> c_int {
    StoredError::tl_clear();
    let local = match tl_socketaddr_from_raw(local_addr_str_ptr, local_addr_str_len) {
        Ok(local) => local,
        Err(errcode) => return errcode,
    };
    let peer = match tl_socketaddr_from_raw(peer_addr_str_ptr, peer_addr_str_len) {
        Ok(local) => local,
        Err(errcode) => return errcode,
    };
    match connector.new_udp_port(local, peer) {
        Ok([p, g]) => {
            if !putter.is_null() {
                putter.write(p);
            }
            if !getter.is_null() {
                getter.write(g);
            }
            ERR_OK
        }
        Err(err) => {
            StoredError::tl_debug_store(&err);
            ERR_REOWOLF
        }
    }
}

/// Connects this connector to the distributed system of connectors reachable through endpoints,
/// Usable in setup state, and changes the state to communication.
#[no_mangle]
pub unsafe extern "C" fn connector_connect(
    connector: &mut Connector,
    timeout_millis: i64,
) -> c_int {
    StoredError::tl_clear();
    let option_timeout_millis: Option<u64> = TryFrom::try_from(timeout_millis).ok();
    let timeout = option_timeout_millis.map(Duration::from_millis);
    match connector.connect(timeout) {
        Ok(()) => ERR_OK,
        Err(err) => {
            StoredError::tl_debug_store(&err);
            ERR_REOWOLF
        }
    }
}

// #[no_mangle]
// pub unsafe extern "C" fn connector_put_payload(
//     connector: &mut Connector,
//     port: PortId,
//     payload: *mut Payload,
// ) -> c_int {
//     match connector.put(port, payload.read()) {
//         Ok(()) => 0,
//         Err(err) => {
//             StoredError::tl_debug_store(&err);
//             -1
//         }
//     }
// }

// #[no_mangle]
// pub unsafe extern "C" fn connector_put_payload_cloning(
//     connector: &mut Connector,
//     port: PortId,
//     payload: &Payload,
// ) -> c_int {
//     match connector.put(port, payload.clone()) {
//         Ok(()) => 0,
//         Err(err) => {
//             StoredError::tl_debug_store(&err);
//             -1
//         }
//     }
// }

/// Convenience function combining the functionalities of
/// "payload_new" with "connector_put_payload".
#[no_mangle]
pub unsafe extern "C" fn connector_put_bytes(
    connector: &mut Connector,
    port: PortId,
    bytes_ptr: *const u8,
    bytes_len: usize,
) -> c_int {
    StoredError::tl_clear();
    let bytes = &*slice_from_raw_parts(bytes_ptr, bytes_len);
    match connector.put(port, Payload::from(bytes)) {
        Ok(()) => ERR_OK,
        Err(err) => {
            StoredError::tl_debug_store(&err);
            ERR_REOWOLF
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn connector_get(connector: &mut Connector, port: PortId) -> c_int {
    StoredError::tl_clear();
    match connector.get(port) {
        Ok(()) => ERR_OK,
        Err(err) => {
            StoredError::tl_debug_store(&err);
            ERR_REOWOLF
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn connector_next_batch(connector: &mut Connector) -> isize {
    StoredError::tl_clear();
    match connector.next_batch() {
        Ok(n) => n as isize,
        Err(err) => {
            StoredError::tl_debug_store(&err);
            ERR_REOWOLF as isize
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn connector_sync(connector: &mut Connector, timeout_millis: i64) -> isize {
    StoredError::tl_clear();
    let option_timeout_millis: Option<u64> = TryFrom::try_from(timeout_millis).ok();
    let timeout = option_timeout_millis.map(Duration::from_millis);
    match connector.sync(timeout) {
        Ok(n) => n as isize,
        Err(err) => {
            StoredError::tl_debug_store(&err);
            ERR_REOWOLF as isize
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn connector_gotten_bytes(
    connector: &mut Connector,
    port: PortId,
    out_len: *mut usize,
) -> *const u8 {
    StoredError::tl_clear();
    match connector.gotten(port) {
        Ok(payload_borrow) => {
            let slice = payload_borrow.as_slice();
            if !out_len.is_null() {
                out_len.write(slice.len());
            }
            slice.as_ptr()
        }
        Err(err) => {
            StoredError::tl_debug_store(&err);
            std::ptr::null()
        }
    }
}

// #[no_mangle]
// unsafe extern "C" fn connector_gotten_payload(
//     connector: &mut Connector,
//     port: PortId,
// ) -> *const Payload {
//     StoredError::tl_clear();
//     match connector.gotten(port) {
//         Ok(payload_borrow) => payload_borrow,
//         Err(err) => {
//             StoredError::tl_debug_store(&err);
//             std::ptr::null()
//         }
//     }
// }

///////////////////// PAYLOAD //////////////////////////
// #[no_mangle]
// unsafe extern "C" fn payload_new(
//     bytes_ptr: *const u8,
//     bytes_len: usize,
//     out_payload: *mut Payload,
// ) {
//     let bytes: &[u8] = &*slice_from_raw_parts(bytes_ptr, bytes_len);
//     out_payload.write(Payload::from(bytes));
// }

// #[no_mangle]
// unsafe extern "C" fn payload_destroy(payload: *mut Payload) {
//     drop(Box::from_raw(payload))
// }

// #[no_mangle]
// unsafe extern "C" fn payload_clone(payload: &Payload, out_payload: *mut Payload) {
//     out_payload.write(payload.clone())
// }

// #[no_mangle]
// unsafe extern "C" fn payload_peek_bytes(payload: &Payload, bytes_len: *mut usize) -> *const u8 {
//     let slice = payload.as_slice();
//     bytes_len.write(slice.len());
//     slice.as_ptr()
// }
