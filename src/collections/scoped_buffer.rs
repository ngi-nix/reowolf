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

/// The buffer itself. This struct should be the shared buffer. The type `T` is
/// intentionally `Copy` such that it can be copied out and the underlying
/// container can be truncated.
pub(crate) struct ScopedBuffer<T: Sized + Copy> {
    pub inner: Vec<T>,
}

/// A section of the buffer. Keeps track of where we started the section. When
/// done with the section one must call `into_vec` or `forget` to remove the
/// section from the underlying buffer.
pub(crate) struct ScopedSection<T: Sized + Copy> {
    inner: *mut Vec<T>,
    start_size: u32,
    #[cfg(debug_assertions)] cur_size: u32,
}

impl<T: Sized + Copy> ScopedBuffer<T> {
    pub(crate) fn new_reserved(capacity: usize) -> Self {
        Self{ inner: Vec::with_capacity(capacity) }
    }

    pub(crate) fn start_section(&mut self) -> ScopedSection<T> {
        let start_size = self.inner.len() as u32;
        ScopedSection{
            inner: &mut self.inner,
            start_size,
            cur_size: start_size
        }
    }

    pub(crate) fn start_section_initialized(&mut self, initialize_with: &[T]) -> ScopedSection<T> {
        let start_size = self.inner.len() as u32;
        let data_size = initialize_with.len() as u32;
        self.inner.extend_from_slice(initialize_with);
        ScopedSection{
            inner: &mut self.inner,
            start_size,
            cur_size: start_size + data_size,
        }
    }
}

#[cfg(debug_assertions)]
impl<T: Sized + Copy> Drop for ScopedBuffer<T> {
    fn drop(&mut self) {
        // Make sure that everyone cleaned up the buffer neatly
        debug_assert!(self.inner.is_empty(), "dropped non-empty scoped buffer");
    }
}

impl<T: Sized + Copy> ScopedSection<T> {
    #[inline]
    pub(crate) fn push(&mut self, value: T) {
        let vec = unsafe{&mut *self.inner};
        debug_assert_eq!(vec.len(), self.cur_size as usize, "trying to push onto section, but size is larger than expected");
        vec.push(value);
        if cfg!(debug_assertions) { self.cur_size += 1; }
    }

    pub(crate) fn len(&self) -> usize {
        let vec = unsafe{&mut *self.inner};
        debug_assert_eq!(vec.len(), self.cur_size as usize, "trying to get section length, but size is larger than expected");
        return vec.len() - self.start_size;
    }

    #[inline]
    pub(crate) fn forget(self) {
        let vec = unsafe{&mut *self.inner};
        debug_assert_eq!(vec.len(), self.cur_size as usize, "trying to forget section, but size is larger than expected");
        vec.truncate(self.start_size as usize);
    }

    #[inline]
    pub(crate) fn into_vec(self) -> Vec<T> {
        let vec = unsafe{&mut *self.inner};
        debug_assert_eq!(vec.len(), self.cur_size as usize, "trying to turn section into vec, but size is larger than expected");
        let section = Vec::from(&vec[self.start_size as usize..]);
        vec.truncate(self.start_size as usize);
        section
    }
}

impl<T: Sized + Copy> std::ops::Index<usize> for ScopedSection<T> {
    type Output = T;

    fn index(&self, idx: usize) -> &Self::Output {
        let vec = unsafe{&*self.inner};
        return vec[self.start_size as usize + idx]
    }
}

#[cfg(debug_assertions)]
impl<T: Sized + Copy> Drop for ScopedBuffer<T> {
    fn drop(&mut self) {
        // Make sure that the data was actually taken out of the scoped section
        let vec = unsafe{&*self.inner};
        debug_assert_eq!(vec.len(), self.start_size as usize);
    }
}