use std::alloc::{alloc, alloc_zeroed, dealloc, Layout};
use std::marker::PhantomData;
use std::ops::{Deref, DerefMut};
use std::ptr::{slice_from_raw_parts, slice_from_raw_parts_mut};

pub struct RawBuffer<T>{
    phantom_of_the_opera: PhantomData<T>,
    capacity: usize,
    layout: Layout,
    pointer: usize
}

impl<T> RawBuffer<T>{
    pub const fn empty() -> Self {
        Self {
            phantom_of_the_opera: PhantomData{},
            capacity: 0,
            layout: Layout::new::<()>(),
            pointer: 0usize,
        }
    }

    pub unsafe fn new(capacity: usize, zeroed: bool) -> Self {
        if capacity == 0 { return Self::empty() }
        let layout = Layout::array::<T>(capacity).unwrap();
        Self {
            phantom_of_the_opera: PhantomData{},
            capacity,
            layout,
            pointer: { if zeroed { alloc_zeroed(layout) } else { alloc(layout) } } as usize,
        }
    }

    #[inline]
    pub fn len(&self) -> usize { self.capacity }

    #[inline]
    pub(crate) fn get_ref(&self) -> &[T]{
        unsafe { &*slice_from_raw_parts(self.pointer as *const T, self.capacity) }
    }

    #[inline]
    pub(crate) fn get_ref_mut(&mut self) -> &mut [T]{
        unsafe { &mut *slice_from_raw_parts_mut(self.pointer as *mut T, self.capacity) }
    }
}

impl<T> Deref for RawBuffer<T>{
    type Target = [T];

    fn deref(&self) -> &Self::Target {
        self.get_ref()
    }
}

impl<T> DerefMut for RawBuffer<T>{
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.get_ref_mut()
    }
}

impl<T> Drop for RawBuffer<T>{
    fn drop(&mut self) {
        unsafe {
            if self.capacity > 0 {
                dealloc(self.pointer as *mut u8, self.layout);
            }
        }
    }
}