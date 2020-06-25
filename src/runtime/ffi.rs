use super::*;

use core::cell::RefCell;
use std::os::raw::{c_char, c_int, c_uchar, c_uint};

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
    fn store<E: Debug>(&mut self, error: &E) {
        write!(&mut self.buf, "{:?}", error);
        self.buf.push(Self::NULL_TERMINATOR);
    }
    fn tl_store<E: Debug>(error: &E) {
        STORED_ERROR.with(|stored_error| {
            let mut stored_error = stored_error.borrow_mut();
            stored_error.clear();
            stored_error.store(error);
        })
    }
    fn tl_clear() {
        STORED_ERROR.with(|stored_error| {
            let mut stored_error = stored_error.borrow_mut();
            stored_error.clear();
        })
    }
    fn tl_raw_peek() -> (*const u8, usize) {
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

type ErrorCode = i32;

//////////////////////////////////////

/// Returns length (via out pointer) and pointer (via return value) of the last Reowolf error.
/// - pointer is NULL iff there was no last error
/// - data at pointer is null-delimited
/// - len does NOT include the length of the null-delimiter
#[no_mangle]
pub unsafe extern "C" fn reowolf_error_peek(len: *mut usize) -> *const u8 {
    let (err_ptr, err_len) = StoredError::tl_raw_peek();
    len.write(err_len);
    err_ptr
}

#[no_mangle]
pub unsafe extern "C" fn protocol_description_parse(
    pdl: *const u8,
    pdl_len: usize,
    pd: *mut Arc<ProtocolDescription>,
) -> ErrorCode {
    StoredError::tl_clear();
    let slice: *const [u8] = std::slice::from_raw_parts(pdl, pdl_len);
    let slice: &[u8] = &*slice;
    match ProtocolDescription::parse(slice) {
        Ok(new) => {
            pd.write(Arc::new(new));
            0
        }
        Err(err) => {
            StoredError::tl_store(&err);
            -1
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn protocol_description_destroy(pd: Arc<ProtocolDescription>) {
    drop(pd)
}

#[no_mangle]
pub unsafe extern "C" fn protocol_description_clone(
    pd: &Arc<ProtocolDescription>,
) -> Arc<ProtocolDescription> {
    pd.clone()
}

// #[no_mangle]
// pub extern "C" fn connector_new(pd: *const Arc<ProtocolDescription>) -> *mut Connector {
//     Box::into_raw(Box::new(Connector::default()))
// }

// /// Creates and returns Reowolf Connector structure allocated on the heap.
// #[no_mangle]
// pub extern "C" fn connector_with_controller_id(controller_id: ControllerId) -> *mut Connector {
//     Box::into_raw(Box::new(Connector::Unconfigured(Unconfigured { controller_id })))
// }
