use crate::common::*;
use core::hash::Hash;
use core::marker::PhantomData;

pub struct Id<T> {
    // Not actually a signed index into the heap. But the index is set to -1 if
    // we don't know an ID yet. This is checked during debug mode.
    pub(crate) index: i32,
    _phantom: PhantomData<T>,
}

impl<T> Id<T> {
    #[inline] pub(crate) fn new_invalid() -> Self     { Self{ index: -1, _phantom: Default::default() } }
    #[inline] pub(crate) fn new(index: i32) -> Self   { Self{ index, _phantom: Default::default() } }
    #[inline] pub(crate) fn is_invalid(&self) -> bool { self.index < 0 }
}

#[derive(Debug)]
pub(crate) struct Arena<T> {
    store: Vec<T>,
}
//////////////////////////////////

impl<T> Debug for Id<T> {
    fn fmt(&self, f: &mut Formatter) -> std::fmt::Result {
        f.debug_struct("Id").field("index", &self.index).finish()
    }
}
impl<T> Clone for Id<T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T> Copy for Id<T> {}
impl<T> PartialEq for Id<T> {
    fn eq(&self, other: &Self) -> bool {
        self.index.eq(&other.index)
    }
}
impl<T> Eq for Id<T> {}
impl<T> Hash for Id<T> {
    fn hash<H: std::hash::Hasher>(&self, h: &mut H) {
        self.index.hash(h);
    }
}

impl<T> Arena<T> {
    pub fn new() -> Self {
        Self { store: vec![] }
    }

    pub fn alloc_with_id(&mut self, f: impl FnOnce(Id<T>) -> T) -> Id<T> {
        // Lets keep this a runtime assert.
        assert!(self.store.len() < i32::max_value() as usize, "Arena out of capacity");
        let id = Id::new(self.store.len() as i32);
        self.store.push(f(id));
        id
    }

    // Compiler-internal direct retrieval
    pub(crate) fn get_id(&self, idx: usize) -> Id<T> {
        debug_assert!(idx < self.store.len());
        return Id::new(idx as i32);
    }

    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.store.iter()
    }

    pub fn len(&self) -> usize {
        self.store.len()
    }
}
impl<T> core::ops::Index<Id<T>> for Arena<T> {
    type Output = T;
    fn index(&self, id: Id<T>) -> &Self::Output {
        debug_assert!(!id.is_invalid(), "attempted to index into Arena with an invalid id (index < 0)");
        self.store.index(id.index as usize)
    }
}
impl<T> core::ops::IndexMut<Id<T>> for Arena<T> {
    fn index_mut(&mut self, id: Id<T>) -> &mut Self::Output {
        debug_assert!(!id.is_invalid(), "attempted to index_mut into Arena with an invalid id (index < 0)");
        self.store.index_mut(id.index as usize)
    }
}