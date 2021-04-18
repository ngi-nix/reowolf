use std::ptr::null_mut;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::marker::PhantomData;

const SLAB_SIZE: usize = u16::max_value() as usize;

#[derive(Clone)]
pub struct StringRef<'a> {
    data: *const u8,
    length: usize,
    _phantom: PhantomData<&'a [u8]>,
}

impl<'a> StringRef<'a> {
    /// `new` constructs a new StringRef whose data is not owned by the
    /// `StringPool`, hence cannot have a `'static` lifetime.
    pub(crate) fn new(data: &'a [u8]) -> StringRef<'a> {
        // This is an internal (compiler) function: so debug_assert that the
        // string is valid ascii. Most commonly the input will come from the
        // code's source file, which is checked for ASCII-ness anyway.
        debug_assert!(data.is_ascii());
        let length = data.len();
        let data = data.as_ptr();
        StringRef{ data, length, _phantom: PhantomData }
    }

    pub fn as_str(&self) -> &'a str {
        unsafe {
            let slice = std::slice::from_raw_parts::<'a, u8>(self.data, self.length);
            std::str::from_utf8_unchecked(slice)
        }
    }

    pub fn as_bytes(&self) -> &'a [u8] {
        unsafe {
            std::slice::from_raw_parts::<'a, u8>(self.data, self.length)
        }
    }
}

impl PartialEq for StringRef {
    fn eq(&self, other: &StringRef) -> bool {
        self.as_str() == other.as_str()
    }
}

impl Eq for StringRef {}

impl Hash for StringRef {
    fn hash<H: Hasher>(&self, state: &mut H) {
        unsafe{
            state.write(std::slice::from_raw_parts(self.data, self.length));
        }
    }
}

struct StringPoolSlab {
    prev: *mut StringPoolSlab,
    data: Vec<u8>,
    remaining: usize,
}

impl StringPoolSlab {
    fn new(prev: *mut StringPoolSlab) -> Self {
        Self{ prev, data: Vec::with_capacity(SLAB_SIZE), remaining: SLAB_SIZE }
    }
}

/// StringPool is a ever-growing pool of strings. Strings have a maximum size
/// equal to the slab size. The slabs are essentially a linked list to maintain
/// pointer-stability of the strings themselves.
/// All `StringRef` instances are invalidated when the string pool is dropped
pub(crate) struct StringPool {
    last: *mut StringPoolSlab,
}

impl StringPool {
    pub(crate) fn new() -> Self {
        // To have some stability we just turn a box into a raw ptr.
        let initial_slab = Box::new(StringPoolSlab::new(null_mut()));
        let initial_slab = Box::into_raw(initial_slab);
        StringPool{
            last: initial_slab,
        }
    }

    /// Interns a string to the `StringPool`, returning a reference to it. The
    /// pointer owned by `StringRef` is `'static` as the `StringPool` doesn't
    /// reallocate/deallocate until dropped (which only happens at the end of
    /// the program.)
    pub(crate) fn intern(&mut self, data: &[u8]) -> StringRef<'static> {
        // TODO: Large string allocations, if ever needed.
        let data_len = data.len();
        assert!(data_len <= SLAB_SIZE, "string is too large for slab");
        debug_assert!(std::str::from_utf8(data).is_ok(), "string to intern is not valid UTF-8 encoded");
        
        let mut last = unsafe{&mut *self.last};
        if data.len() > last.remaining {
            // Doesn't fit: allocate new slab
            self.alloc_new_slab();
            last = unsafe{&mut *self.last};
        }

        // Must fit now, compute hash and put in buffer
        debug_assert!(data_len <= last.remaining);
        let range_start = last.data.len();
        last.data.extend_from_slice(data);
        last.remaining -= data_len;
        debug_assert_eq!(range_start + data_len, last.data.len());

        unsafe {
            let start = last.data.as_ptr().offset(range_start as isize);
            StringRef{ data: start, length: data_len, _phantom: PhantomData }
        }
    }

    fn alloc_new_slab(&mut self) {
        let new_slab = Box::new(StringPoolSlab::new(self.last));
        let new_slab = Box::into_raw(new_slab);
        self.last = new_slab;
    }
}

impl Drop for StringPool {
    fn drop(&mut self) {
        let mut new_slab = self.last;
        while !new_slab.is_null() {
            let cur_slab = new_slab;
            unsafe {
                new_slab = (*cur_slab).prev;
                Box::from_raw(cur_slab); // consume and deallocate
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_string_just_fits() {
        let large = "0".repeat(SLAB_SIZE);
        let mut pool = StringPool::new();
        let interned = pool.intern(large.as_bytes());
        assert_eq!(interned.as_str(), large);
    }

    #[test]
    #[should_panic]
    fn test_string_too_large() {
        let large = "0".repeat(SLAB_SIZE + 1);
        let mut pool = StringPool::new();
        let _interned = pool.intern(large.as_bytes());
    }

    #[test]
    fn test_lots_of_small_allocations() {
        const NUM_PER_SLAB: usize = 32;
        const NUM_SLABS: usize = 4;

        let to_intern = "0".repeat(SLAB_SIZE / NUM_PER_SLAB);
        let mut pool = StringPool::new();

        let mut last_slab = pool.last;
        let mut all_refs = Vec::new();

        // Fill up first slab
        for _alloc_idx in 0..NUM_PER_SLAB {
            let interned = pool.intern(to_intern.as_bytes());
            all_refs.push(interned);
            assert!(std::ptr::eq(last_slab, pool.last));
        }

        for _slab_idx in 0..NUM_SLABS-1 {
            for alloc_idx in 0..NUM_PER_SLAB {
                let interned = pool.intern(to_intern.as_bytes());
                all_refs.push(interned);

                if alloc_idx == 0 {
                    // First allocation produces a new slab
                    assert!(!std::ptr::eq(last_slab, pool.last));
                    last_slab = pool.last;
                } else {
                    assert!(std::ptr::eq(last_slab, pool.last));
                }
            }
        }

        // All strings are still correct
        for string_ref in all_refs {
            assert_eq!(string_ref.as_str(), to_intern);
        }
    }
}