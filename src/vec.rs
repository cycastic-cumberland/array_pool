use std::fmt::{Display, Formatter};
use std::mem::swap;
use std::ops::{Deref, DerefMut};
use std::ptr::drop_in_place;
use std::sync::Arc;
use crate::pool::{ArrayPool, BorrowingSlice};

/// A vector implementation that uses pooled arrays.
pub struct PooledVec<T: Send> {
    empty_buffer: [T; 0],
    pool: Arc<ArrayPool<T>>,
    buffer: Option<BorrowingSlice<T>>,
    length: usize
}

impl<T: Send> PooledVec<T> {
    /// Create a new vector.
    pub fn create(pool: Arc<ArrayPool<T>>) -> Self {
        Self{
            empty_buffer: [],
            pool,
            buffer: None,
            length: 0,
        }
    }

    fn push_with_buffer(&mut self, mut buffer: BorrowingSlice<T>, value: T) {
        let index = self.length;
        let buffer_size = buffer.len();
        if index >= buffer_size {
            unsafe {
                buffer = self.pool.expand_buffer(buffer)
                    .expect("Could not request buffer");
            }
        }
        unsafe { std::ptr::write(&mut buffer[index], value); }
        self.buffer = Some(buffer);
        self.length += 1;
    }

    /// Push a new element. Expand the internal buffer if needed.
    pub fn push(&mut self, value: T) {
        let mut curr: Option<BorrowingSlice<T>> = None;
        swap(&mut curr, &mut self.buffer);
        if let Some(buffer) = curr {
            self.push_with_buffer(buffer, value);
        } else if let Ok(buffer) = unsafe { self.pool.rent_or_create_minimum_uninitialized(false) } {
            self.push_with_buffer(buffer, value);
        } else {
            panic!("Could not borrow a buffer from given array pool");
        }
    }

    /// Get the length of this vector.
    pub fn len(&self) -> usize {
        self.length
    }

    /// Get the capacity of this vector.
    pub fn capacity(&self) -> usize {
        if let Some(buffer) = &self.buffer{
            buffer.len()
        } else { 0 }
    }

    fn try_shrink(&mut self, mut buffer: BorrowingSlice<T>) {
        let len = self.length;
        let cap = buffer.len();
        if self.pool.min_size() < cap && len * 2 < cap {
            unsafe { buffer = self.pool.shrink_buffer(buffer); }
        }
        self.buffer = Some(buffer);
    }

    /// Pop the last element from this vector and return it.
    /// Shrink the buffer if needed.
    pub fn pop(&mut self) -> Option<T> {
        let mut curr: Option<BorrowingSlice<T>> = None;
        swap(&mut curr, &mut self.buffer);
        if let Some(mut buffer) = curr {
            if self.length == 0 { return None; }
            self.length -= 1;
            let return_value = unsafe { std::ptr::read(&mut buffer[self.length]) };
            self.try_shrink(buffer);
            Some(return_value)
        } else { None }
    }

    /// Clear all elements in this vector and return its last length.
    pub fn clear(&mut self) -> usize {
        let mut curr: Option<BorrowingSlice<T>> = None;
        swap(&mut curr, &mut self.buffer);
        if let Some(mut buffer) = curr{
            unsafe {
                for i in 0..self.len(){
                    drop_in_place(&mut buffer[i])
                }
                drop(buffer);
                let old_length = self.length;
                self.length = 0;
                old_length
            }
        } else { 0 }
    }

    /// Gets a reference to an element at a specific index,
    /// may return `None` if index is out of bound.
    pub fn at(&self, index: usize) -> Option<&T> {
        if index >= self.length { return None; }
        Some(&self[index])
    }

    /// Gets a mutable reference to an element at a specific index,
    /// may return `None` if index is out of bound.
    pub fn at_mut(&mut self, index: usize) -> Option<&mut T> {
        if index >= self.length { return None; }
        Some(&mut self[index])
    }
}

impl<T: Send> Deref for PooledVec<T>{
    type Target = [T];

    fn deref(&self) -> &Self::Target {
        match &self.buffer {
            Some(r) => &r.deref()[0..self.length],
            None => &self.empty_buffer
        }
    }
}

impl<T: Send> DerefMut for PooledVec<T>{
    fn deref_mut(&mut self) -> &mut Self::Target {
        match &mut self.buffer {
            Some(r) => &mut r.deref_mut()[0..self.length],
            None => &mut self.empty_buffer
        }
    }
}

impl<T: Send + Clone> Clone for PooledVec<T>{
    fn clone(&self) -> Self {
        Self{
            empty_buffer: [],
            pool: self.pool.clone(),
            buffer: self.buffer.clone(),
            length: self.length,
        }
    }
}

impl<T: Send + Display> Display for PooledVec<T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "[ ")?;

        let mut insert_colon = false;

        for x in self.iter() {
            if insert_colon{
                write!(f, ", ")?;
            }
            write!(f, "{x}")?;
            insert_colon = true;
        }

        write!(f, " ]")?;
        Ok(())
    }
}
