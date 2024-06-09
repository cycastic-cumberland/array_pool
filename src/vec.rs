use std::fmt::{Display, Formatter};
use std::mem::swap;
use std::ops::{Deref, DerefMut};
use std::sync::Arc;
use crate::pool::{ArrayPool, BorrowingSlice};

pub struct PooledVec<T: Send, F: Clone + FnMut() -> T = fn() -> T> {
    empty_buffer: [T; 0],
    pool: Arc<ArrayPool<T>>,
    fabricator: F,
    buffer: Option<BorrowingSlice<T>>,
    length: usize
}

impl<T: Send, F: Clone + FnMut() -> T> PooledVec<T, F> {
    pub fn create(pool: Arc<ArrayPool<T>>, fabricator: F) -> Self {
        Self{
            empty_buffer: [],
            pool,
            fabricator,
            buffer: None,
            length: 0,
        }
    }

    fn push_with_buffer(&mut self, mut buffer: BorrowingSlice<T>, value: T) {
        let index = self.length;
        let buffer_size = buffer.len();
        if index >= buffer_size {
            buffer = self.pool.expand_buffer(buffer, &mut self.fabricator)
                .expect("Could not request buffer");
        }
        buffer[index] = value;
        self.buffer = Some(buffer);
        self.length += 1;
    }

    pub fn push(&mut self, value: T) {
        let mut curr: Option<BorrowingSlice<T>> = None;
        swap(&mut curr, &mut self.buffer);
        if let Some(buffer) = curr {
            self.push_with_buffer(buffer, value);
        } else if let Ok(buffer) = self.pool.rent_minimum_with(&mut self.fabricator) {
            self.push_with_buffer(buffer, value);
        } else {
            panic!("Could not borrow a buffer from given array pool");
        }
    }

    pub fn len(&self) -> usize {
        self.length
    }

    pub fn capacity(&self) -> usize {
        if let Some(buffer) = &self.buffer{
            buffer.len()
        } else { 0 }
    }

    fn try_shrink(&mut self, mut buffer: BorrowingSlice<T>) {
        let len = self.length;
        let cap = buffer.len();
        if self.pool.min_size() < cap && len * 2 < cap {
            buffer = self.pool.shrink_buffer(buffer, &mut self.fabricator);
        }
        self.buffer = Some(buffer);
    }

    pub fn pop(&mut self) -> Option<T> {
        let mut curr: Option<BorrowingSlice<T>> = None;
        swap(&mut curr, &mut self.buffer);
        if let Some(mut buffer) = curr {
            if self.length == 0 { return None; }
            let fab = &mut self.fabricator;
            let mut return_value = fab();
            self.length -= 1;
            swap(&mut return_value, &mut buffer[self.length]);
            self.try_shrink(buffer);
            Some(return_value)
        } else { None }
    }

    pub fn clear(&mut self) -> usize {
        let mut curr: Option<BorrowingSlice<T>> = None;
        swap(&mut curr, &mut self.buffer);
        if let Some(buffer) = curr{
            drop(buffer);
            self.length
        } else { 0 }
    }

    pub fn at(&self, index: usize) -> Option<&T> {
        if index >= self.length { return None; }
        Some(&self[index])
    }

    pub fn at_mut(&mut self, index: usize) -> Option<&mut T> {
        if index >= self.length { return None; }
        Some(&mut self[index])
    }
}

impl<T: Send, F: Clone + FnMut() -> T> Deref for PooledVec<T, F>{
    type Target = [T];

    fn deref(&self) -> &Self::Target {
        match &self.buffer {
            Some(r) => &r.deref()[0..self.length],
            None => &self.empty_buffer
        }
    }
}

impl<T: Send, F: Clone + FnMut() -> T> DerefMut for PooledVec<T, F>{
    fn deref_mut(&mut self) -> &mut Self::Target {
        match &mut self.buffer {
            Some(r) => &mut r.deref_mut()[0..self.length],
            None => &mut self.empty_buffer
        }
    }
}

impl<T: Send + Clone, F: Clone + FnMut() -> T> Clone for PooledVec<T, F>{
    fn clone(&self) -> Self {
        Self{
            empty_buffer: [],
            pool: self.pool.clone(),
            fabricator: self.fabricator.clone(),
            buffer: self.buffer.clone(),
            length: self.length,
        }
    }
}

impl<T: Send + Display, F: Clone + FnMut() -> T> Display for PooledVec<T, F> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match &self.buffer {
            Some(v) => write!(f, "{v}"),
            None => write!(f, "[]")
        }
    }
}

impl<T: Send + Default> PooledVec<T, fn() -> T> {
    pub fn new_with_pool(pool: Arc<ArrayPool<T>>) -> Self {
        Self::create(pool, T::default)
    }
}
