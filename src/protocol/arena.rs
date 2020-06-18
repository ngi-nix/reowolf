use core::hash::Hash;
use core::marker::PhantomData;

#[derive(Debug, serde::Serialize)]
pub struct Id<T> {
    index: u32,
    _phantom: PhantomData<T>,
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

pub struct Arena<T> {
    store: Vec<T>,
}
impl<T> Arena<T> {
    pub fn new() -> Self {
        Self { store: vec![] }
    }
    pub fn alloc_with_id(&mut self, f: impl FnOnce(Id<T>) -> T) -> Id<T> {
        use std::convert::TryFrom;
        let id = Id {
            index: u32::try_from(self.store.len()).expect("Out of capacity!"),
            _phantom: Default::default(),
        };
        self.store.push(f(id));
        id
    }
    pub fn iter(&self) -> impl Iterator<Item = (Id<T>, &T)> {
        (0..).map(|index| Id { index, _phantom: Default::default() }).zip(self.store.iter())
    }
}
impl<T> core::ops::Index<Id<T>> for Arena<T> {
    type Output = T;
    fn index(&self, id: Id<T>) -> &Self::Output {
        self.store.index(id.index as usize)
    }
}
impl<T> core::ops::IndexMut<Id<T>> for Arena<T> {
    fn index_mut(&mut self, id: Id<T>) -> &mut Self::Output {
        self.store.index_mut(id.index as usize)
    }
}
