use std::collections::BTreeMap;
use std::fmt::{Display, Formatter};
use std::mem::{size_of, swap};
use std::ops::{Deref, DerefMut};
use std::ptr::drop_in_place;
use std::sync::{Arc, Mutex, Weak};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::thread::ThreadId;

use thread_local::ThreadLocal;
use crate::raw_buffer::RawBuffer;

struct LocalBufferChain<T> {
    chunk_linked_list: Mutex<Vec<RawBuffer<T>>>,
    chunk_count: Arc<AtomicUsize>
}

struct BufferChain<T: Send>{
    chunk_size: usize,
    chunk_count: Arc<AtomicUsize>,
    chains: Mutex<BTreeMap<u64, Weak<LocalBufferChain<T>>>>,
    local_chain: ThreadLocal<Arc<LocalBufferChain<T>>>
}

pub struct BorrowingSlice<T: Send>{
    array: RawBuffer<T>,
    chain: Arc<BufferChain<T>>,
    pub(crate) initialized: bool,
}

impl<T> Drop for LocalBufferChain<T>{
    fn drop(&mut self) {
        let locked = self.chunk_linked_list.lock().unwrap();
        let len = locked.len();
        self.chunk_count.fetch_sub(len, Ordering::SeqCst);
    }
}

impl<T> LocalBufferChain<T>{
    pub unsafe fn borrow(self: &Arc<Self>) -> Option<RawBuffer<T>>{
        let mut lock_guard = self.chunk_linked_list.lock().unwrap();
        if let Some(slice) = lock_guard.pop() {
            self.chunk_count.fetch_sub(1, Ordering::SeqCst);
            Some(slice)
        } else { None }
    }
}

pub struct ArrayPool<T: Send> {
    empty_chain: Arc<BufferChain<T>>,
    chunk_map: BTreeMap<usize, Arc<BufferChain<T>>>
}

impl<T: Send> BufferChain<T>{
    pub fn new(size_power: u8) -> Arc<Self> {
        Arc::new(Self {
            chunk_size: 1usize << size_power,
            chunk_count: Arc::new(AtomicUsize::default()),
            chains: Mutex::new(BTreeMap::new()),
            local_chain: ThreadLocal::new(),
        })
    }

    fn new_array<F: FnMut() -> T>(&self, fabricator: &mut F) -> RawBuffer<T> {
        unsafe {
            let mut buffer = RawBuffer::<T>::new(self.chunk_size, false);
            let length = buffer.len();
            let reference = buffer.get_ref_mut();
            for i in 0..length{
                // Avoid dropping the old, invalid value
                std::ptr::write(&mut reference[i], fabricator());
            }

            buffer
        }
    }

    fn get_local(&self) -> &Arc<LocalBufferChain<T>> {
        let arc_count = self.chunk_count.clone();
        self.local_chain.get_or(move ||{
            let arc = Arc::new(LocalBufferChain {
                chunk_linked_list: Mutex::new(vec![]),
                chunk_count: arc_count,
            });
            let mut lock_guard = self.chains.lock().unwrap();
            let tid = thread::current().id();
            lock_guard.insert(unsafe { *(&tid as *const ThreadId as *const u64) }, Arc::downgrade(&arc));

            arc
        })
    }

    fn borrow_from_other_chains(&self) -> Option<RawBuffer<T>> {
        let mut lock_guard = self.chains.lock().unwrap();
        let mut remove_queue: Vec<u64> = Vec::new();
        let mut found: Option<RawBuffer<T>> = None;

        for (id, chain_weak) in lock_guard.iter() {
            if let Some(chain) = chain_weak.upgrade() {
                if let Some(cached) = unsafe{ chain.borrow() }{
                    found = Some(cached);
                    break;
                }
            } else {
                remove_queue.push(*id);
            }
        }

        for id in &remove_queue {
            lock_guard.remove(id);
        }

        found
    }

    pub fn rent_with<F: FnMut() -> T>(self: &Arc<Self>, fabricator: &mut F) -> BorrowingSlice<T> {
        let local_chain = self.get_local();
        let array;
        if self.chunk_count.load(Ordering::Acquire) == 0 {
            array = self.new_array(fabricator);
        } else if let Some(cached) = unsafe{ local_chain.borrow() }{
            array = cached;
        } else if let Some(cached) = self.borrow_from_other_chains() {
            array = cached
        } else {
            array = self.new_array(fabricator);
        }
        BorrowingSlice{
            array,
            chain: self.clone(),
            initialized: true,
        }
    }

    pub(crate) unsafe fn new_uninitialized(&self, zeroed: bool) -> RawBuffer<T> {
        RawBuffer::new(self.chunk_size, zeroed)
    }

    pub unsafe fn rent_or_create_uninitialized(self: &Arc<Self>, zeroed: bool) -> BorrowingSlice<T>{
        let local_chain = self.get_local();
        let array;
        if self.chunk_count.load(Ordering::Acquire) == 0 {
            array = self.new_uninitialized(zeroed);
        } else if let Some(cached) = local_chain.borrow(){
            array = cached;
        } else if let Some(cached) = self.borrow_from_other_chains() {
            array = cached
        } else {
            array = self.new_uninitialized(zeroed);
        }
        BorrowingSlice{
            array,
            chain: self.clone(),
            initialized: false,
        }
    }
}

impl<T: Send> Drop for BorrowingSlice<T>{
    fn drop(&mut self) {
        if self.array.is_empty() { return; }
        if self.initialized {
            unsafe {
                for i in 0..self.len() {
                    let elem = &mut self[i];
                    drop_in_place(elem);
                }
            }
        }
        let mut lock_guard = self.chain.get_local().chunk_linked_list.lock().unwrap();
        let mut store = RawBuffer::<T>::empty();
        swap(&mut store, &mut self.array);
        lock_guard.push(store);
        self.chain.chunk_count.fetch_add(1, Ordering::SeqCst);
    }
}

impl<T: Send> Deref for BorrowingSlice<T>{
    type Target = [T];

    fn deref(&self) -> &Self::Target {
        self.array.deref()
    }
}
impl<T: Send> DerefMut for BorrowingSlice<T>{
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.array.deref_mut()
    }
}

impl<T: Send + Display> Display for BorrowingSlice<T> {
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

impl<T: Send + Clone> Clone for BorrowingSlice<T> {
    fn clone(&self) -> Self {
        let mut new_buffer: RawBuffer<T>;
        unsafe {
            new_buffer = match self.chain.get_local().borrow(){
                Some(v) => v,
                None => self.chain.new_uninitialized(false)
            };
            for i in 0..self.len(){
                // ptr contain uninitialized value
                std::ptr::write(&mut new_buffer[i], self[i].clone());
            }
        }


        Self{
            array: new_buffer,
            chain: self.chain.clone(),
            initialized: true,
        }
    }
}

#[derive(Copy, Clone, Debug)]
pub enum ArrayPoolError {
    MaxPowerTooSmall,
    MaxChunkSizeNotSufficient
}

impl<T: Send> ArrayPool<T>{
    pub fn with_max_power(max_power: u8) -> Result<Self, ArrayPoolError> {
        let mut map: BTreeMap<usize, Arc<BufferChain<T>>> = BTreeMap::new();
        if max_power < 4 { return Err(ArrayPoolError::MaxPowerTooSmall); }
        for x in 3..max_power {
            map.insert(1usize << x, BufferChain::new(x));
        }
        Ok(Self {
            empty_chain: BufferChain::new(0),
            chunk_map: map
        })
    }

    pub fn new() -> Self {
        Self::with_max_power((size_of::<usize>() - 1) as u8).unwrap()
    }

    fn get_chain(&self, minimum_capacity: usize) -> Option<&Arc<BufferChain<T>>>{
        for (size, chunk_chain) in &self.chunk_map {
            if minimum_capacity <= *size {
                return Some(chunk_chain);
            }
        }

        None
    }

    pub fn rent_with<F: FnMut() -> T>(&self, minimum_capacity: usize, fabricator: &mut F) -> Result<BorrowingSlice<T>, ArrayPoolError> {
        if let Some(chunk_chain) = self.get_chain(minimum_capacity){
            return Ok(chunk_chain.rent_with(fabricator));
        }

        Err(ArrayPoolError::MaxChunkSizeNotSufficient)
    }

    pub unsafe fn rent_or_create_uninitialized(&self, minimum_capacity: usize, zeroed: bool) -> Result<BorrowingSlice<T>, ArrayPoolError> {
        if let Some(chunk_chain) = self.get_chain(minimum_capacity){
            return Ok(chunk_chain.rent_or_create_uninitialized(zeroed));
        }

        Err(ArrayPoolError::MaxChunkSizeNotSufficient)
    }

    pub fn rent_minimum_with<F: FnMut() -> T>(&self, fabricator: &mut F) -> Result<BorrowingSlice<T>, ArrayPoolError>{
        for (_, chunk_chain) in &self.chunk_map {
            return Ok(chunk_chain.rent_with(fabricator));
        }

        Err(ArrayPoolError::MaxChunkSizeNotSufficient)
    }

    pub unsafe fn rent_or_create_minimum_uninitialized(&self, zeroed: bool) -> Result<BorrowingSlice<T>, ArrayPoolError> {
        for (_, chunk_chain) in &self.chunk_map {
            return Ok(chunk_chain.rent_or_create_uninitialized(zeroed));
        }

        Err(ArrayPoolError::MaxChunkSizeNotSufficient)
    }

    pub unsafe  fn expand_buffer(&self, mut old_buffer: BorrowingSlice<T>) -> Result<BorrowingSlice<T>, ArrayPoolError> {
        let old_size = old_buffer.len();
        let new_size = old_size * 2;
        if let Ok(mut new_buffer) = unsafe {self.rent_or_create_uninitialized(new_size, false)} {
            for i in 0..old_size {
                swap(&mut old_buffer[i], &mut new_buffer[i]);
            }

            old_buffer.initialized = false;
            drop(old_buffer);
            Ok(new_buffer)
        } else { Err(ArrayPoolError::MaxChunkSizeNotSufficient) }
    }

    pub unsafe fn shrink_buffer(&self, mut old_buffer: BorrowingSlice<T>) -> BorrowingSlice<T> {
        let old_size = old_buffer.len();
        let new_size = old_size / 2;

        if let Ok(mut new_buffer) = unsafe {self.rent_or_create_uninitialized(new_size, false)} {
            for i in 0..new_size {
                swap(&mut old_buffer[i], &mut new_buffer[i]);
            }

            old_buffer.initialized = false;
            drop(old_buffer);
            new_buffer
        } else {
            old_buffer
        }
    }

    pub fn rent_empty(&self) -> BorrowingSlice<T> {
        BorrowingSlice{
            array: RawBuffer::empty(),
            chain: self.empty_chain.clone(),
            initialized: true,
        }
    }

    pub fn min_size(&self) -> usize {
        *self.chunk_map.first_key_value().unwrap().0
    }

    pub fn max_size(&self) -> usize {
        *self.chunk_map.last_key_value().unwrap().0
    }
}

impl<T: Default + Send> ArrayPool<T>{
    pub fn rent(&self, minimum_capacity: usize) -> Result<BorrowingSlice<T>, ArrayPoolError> {
        self.rent_with(minimum_capacity, &mut T::default)
    }

    pub fn rent_minimum(&self) -> Result<BorrowingSlice<T>, ArrayPoolError>{
        self.rent_minimum_with(&mut T::default)
    }
}
