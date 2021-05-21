use std::collections::VecDeque;

/// Simple double ended queue that ensures that all elements are unique. Queue
/// elements are not ordered (we expect the queue to be rather small).
pub struct DequeSet<T: Eq> {
    inner: VecDeque<T>,
}

impl<T: Eq> DequeSet<T> {
    pub fn new() -> Self {
        Self{ inner: VecDeque::new() }
    }

    #[inline]
    pub fn pop_front(&mut self) -> Option<T> {
        self.inner.pop_front()
    }

    #[inline]
    pub fn pop_back(&mut self) -> Option<T> {
        self.inner.pop_back()
    }

    #[inline]
    pub fn push_back(&mut self, to_push: T) {
        for element in self.inner.iter() {
            if *element == to_push {
                return;
            }
        }

        self.inner.push_back(to_push);
    }

    #[inline]
    pub fn push_front(&mut self, to_push: T) {
        for element in self.inner.iter() {
            if *element == to_push {
                return;
            }
        }

        self.inner.push_front(to_push);
    }

    #[inline]
    pub fn clear(&mut self) {
        self.inner.clear();
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }
}

/// Simple vector set that ensures that all elements are unique. Elements are
/// not ordered (we expect the vector to be small).
pub struct VecSet<T: Eq> {
    inner: Vec<T>,
}

impl<T: Eq> VecSet<T> {
    pub fn new() -> Self {
        Self{ inner: Vec::new() }
    }

    #[inline]
    pub fn pop(&mut self) -> Option<T> {
        self.inner.pop()
    }

    #[inline]
    pub fn push(&mut self, to_push: T) {
        for element in self.inner.iter() {
            if *element == to_push {
                return;
            }
        }

        self.inner.push(to_push);
    }

    #[inline]
    pub fn clear(&mut self) {
        self.inner.clear();
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }
}