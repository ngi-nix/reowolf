/// scoped_buffer.rs
///
/// Solves the common pattern where we are performing some kind of recursive
/// pattern while using a temporary buffer. At the start, or during the
/// procedure, we push stuff into the buffer. At the end we take out what we
/// have put in.
///
/// It is unsafe because we're using pointers to circumvent borrowing rules in
/// the name of code cleanliness. The correctness of use is checked in debug
/// mode.

use std::iter::FromIterator;

pub(crate) struct ScopedBuffer<T: Sized> {
    pub inner: Vec<T>,
}

/// A section of the buffer. Keeps track of where we started the section. When
/// done with the section one must call `into_vec` or `forget` to remove the
/// section from the underlying buffer. This will also be done upon dropping the
/// ScopedSection in case errors are being handled.
pub(crate) struct ScopedSection<T: Sized> {
    inner: *mut Vec<T>,
    start_size: u32,
    #[cfg(debug_assertions)] cur_size: u32,
}

impl<T: Sized> ScopedBuffer<T> {
    pub(crate) fn new_reserved(capacity: usize) -> Self {
        Self { inner: Vec::with_capacity(capacity) }
    }

    pub(crate) fn start_section(&mut self) -> ScopedSection<T> {
        let start_size = self.inner.len() as u32;
        ScopedSection {
            inner: &mut self.inner,
            start_size,
            #[cfg(debug_assertions)] cur_size: start_size
        }
    }
}

impl<T: Clone> ScopedBuffer<T> {
    pub(crate) fn start_section_initialized(&mut self, initialize_with: &[T]) -> ScopedSection<T> {
        let start_size = self.inner.len() as u32;
        let _data_size = initialize_with.len() as u32;
        self.inner.extend_from_slice(initialize_with);
        ScopedSection{
            inner: &mut self.inner,
            start_size,
            #[cfg(debug_assertions)] cur_size: start_size + _data_size,
        }
    }
}

#[cfg(debug_assertions)]
impl<T: Sized> Drop for ScopedBuffer<T> {
    fn drop(&mut self) {
        // Make sure that everyone cleaned up the buffer neatly
        debug_assert!(self.inner.is_empty(), "dropped non-empty scoped buffer");
    }
}

impl<T: Sized> ScopedSection<T> {
    #[inline]
    pub(crate) fn push(&mut self, value: T) {
        let vec = unsafe{&mut *self.inner};
        #[cfg(debug_assertions)] debug_assert_eq!(vec.len(), self.cur_size as usize, "trying to push onto section, but size is larger than expected");
        vec.push(value);
        #[cfg(debug_assertions)] { self.cur_size += 1; }
    }

    pub(crate) fn len(&self) -> usize {
        let vec = unsafe{&mut *self.inner};
        #[cfg(debug_assertions)] debug_assert_eq!(vec.len(), self.cur_size as usize, "trying to get section length, but size is larger than expected");
        return vec.len() - self.start_size as usize;
    }

    #[inline]
    #[allow(unused_mut)] // used in debug mode
    pub(crate) fn forget(mut self) {
        let vec = unsafe{&mut *self.inner};
        #[cfg(debug_assertions)] {
            debug_assert_eq!(
                vec.len(), self.cur_size as usize,
                "trying to forget section, but size is larger than expected"
            );
            self.cur_size = self.start_size;
        }
        vec.truncate(self.start_size as usize);
    }

    #[inline]
    #[allow(unused_mut)] // used in debug mode
    pub(crate) fn into_vec(mut self) -> Vec<T> {
        let vec = unsafe{&mut *self.inner};
        #[cfg(debug_assertions)]  {
            debug_assert_eq!(
                vec.len(), self.cur_size as usize,
                "trying to turn section into vec, but size is larger than expected"
            );
            self.cur_size = self.start_size;
        }
        let section = Vec::from_iter(vec.drain(self.start_size as usize..));
        section
    }
}

impl<T: Sized> std::ops::Index<usize> for ScopedSection<T> {
    type Output = T;

    fn index(&self, idx: usize) -> &Self::Output {
        let vec = unsafe{&*self.inner};
        return &vec[self.start_size as usize + idx]
    }
}

#[cfg(debug_assertions)]
impl<T: Sized> Drop for ScopedSection<T> {
    fn drop(&mut self) {
        let vec = unsafe{&mut *self.inner};
        #[cfg(debug_assertions)] debug_assert_eq!(vec.len(), self.cur_size as usize);
        vec.truncate(self.start_size as usize);
    }
}