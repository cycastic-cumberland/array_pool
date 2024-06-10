use std::alloc::{alloc, alloc_zeroed, dealloc, Layout};
use std::marker::PhantomData;
use std::ops::{Deref, DerefMut};
use std::ptr::{read, slice_from_raw_parts, slice_from_raw_parts_mut};

#[cfg(test)] #[derive(Copy, Clone)] pub(crate) enum TestFlag {
    Increment,
    Decrement,
    Fetch
}

pub struct RawBuffer<T>{
    phantom_of_the_opera: PhantomData<T>,
    initialized: bool,
    capacity: usize,
    layout: Layout,
    pointer: usize
}

#[cfg(test)]
pub fn global_allocation(flag: TestFlag) -> usize {
    static GLOBAL_ALLOC_COUNT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

    match flag {
        TestFlag::Increment => GLOBAL_ALLOC_COUNT.fetch_add(1, std::sync::atomic::Ordering::SeqCst),
        TestFlag::Decrement => GLOBAL_ALLOC_COUNT.fetch_sub(1, std::sync::atomic::Ordering::SeqCst),
        TestFlag::Fetch => GLOBAL_ALLOC_COUNT.load(std::sync::atomic::Ordering::SeqCst)
    }
}

impl<T> RawBuffer<T>{
    pub const fn empty() -> Self {
        Self {
            phantom_of_the_opera: PhantomData{},
            initialized: false,
            capacity: 0,
            layout: Layout::new::<()>(),
            pointer: 0usize,
        }
    }

    pub unsafe fn new(capacity: usize, zeroed: bool) -> Self {
        if capacity == 0 { return Self::empty() }
        #[cfg(test)]{
            let d = global_allocation(TestFlag::Increment);
            _ = d;
        }
        let layout = Layout::array::<T>(capacity).unwrap();
        Self {
            phantom_of_the_opera: PhantomData{},
            initialized: false,
            capacity,
            layout,
            pointer: { if zeroed { alloc_zeroed(layout) } else { alloc(layout) } } as usize,
        }
    }

    pub fn with_fabricator<F: FnMut() -> T>(capacity: usize, f: &mut F) -> Self{
        let mut ret = unsafe { Self::new(capacity, false) };
        ret.initialize(f);
        ret
    }

    #[inline]
    pub unsafe fn set_initialized(&mut self){
        self.initialized = true;
    }

    #[inline]
    pub fn len(&self) -> usize { self.capacity }

    #[inline]
    fn get_ref(&self) -> &[T]{
        unsafe { &*slice_from_raw_parts(self.pointer as *const T, self.capacity) }
    }

    #[inline]
    fn get_ref_mut(&mut self) -> &mut [T]{
        unsafe { &mut *slice_from_raw_parts_mut(self.pointer as *mut T, self.capacity) }
    }

    fn initialize<F: FnMut() -> T>(&mut self, fabricator: &mut F) -> Option<()> {
        if self.initialized { None }
        else {
            let length = self.len();
            let reference = self.get_ref_mut();
            for i in 0..length{
                // Avoid dropping the old, invalid value
                let ptr = (&mut reference[i]) as *mut T;
                unsafe { ptr.write(fabricator()) };
            }

            self.initialized = true;
            Some(())
        }
    }
}

impl<T> Drop for RawBuffer<T>{
    fn drop(&mut self) {
        #[cfg(test)] let mut d = 0usize;
        unsafe {
            if self.initialized {
                for elem in self.deref_mut() {
                    let ptr = elem as *mut T;
                    drop(read(ptr));
                }
            }

            if self.capacity > 0 {
                dealloc(self.pointer as *mut u8, self.layout);
                #[cfg(test)]{
                    d = global_allocation(TestFlag::Decrement);
                }
            }
        }
        #[cfg(test)] { _ = d; }
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

impl<T: Clone> Clone for RawBuffer<T>{
    fn clone(&self) -> Self {
        if !self.initialized{
            panic!("RawBuffer not initialized")
        }

        let cap = self.capacity;
        let mut new_buffer = unsafe { Self::new(cap, false) };
        for i in 0..cap {
            // Avoid dropping invalid values
            let ptr = (&mut new_buffer[i]) as *mut T;
            unsafe { ptr.write(self[i].clone()) };
        }

        new_buffer.initialized = true;
        new_buffer
    }
}
