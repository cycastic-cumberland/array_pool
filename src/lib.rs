pub mod pool;
pub mod vec;
pub(crate) mod raw_buffer;

#[cfg(test)]
mod tests {
    use std::ops::{Deref};
    use std::rc::Rc;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::thread;
    use lazy_static::lazy_static;
    use crate::pool::ArrayPool;
    use crate::vec::PooledVec;
    // use crate::raw_buffer::{TestFlag};
    // use super::*;

    lazy_static!{
        static ref POOL: Arc<ArrayPool<u32>> = {
            Arc::new(ArrayPool::new())
        };
    }

    struct DropTestStruct(Rc<AtomicUsize>);

    impl DropTestStruct {
        fn new(counter: Rc<AtomicUsize>) -> Self {
            counter.fetch_add(1, Ordering::Relaxed);
            Self(counter)
        }
    }
    
    impl Drop for DropTestStruct {
        fn drop(&mut self) {
            self.0.fetch_sub(1, Ordering::Relaxed);
        }
    }

    #[test]
    fn drop_test(){
        let counter_1 = Rc::new(AtomicUsize::default());
        let counter_2 = Rc::new(AtomicUsize::default());
        {
            let mut arr: [DropTestStruct; 1] = [DropTestStruct::new(counter_1.clone())];
            assert_eq!(counter_1.load(Ordering::Relaxed), 1);
            assert_eq!(counter_2.load(Ordering::Relaxed), 0);
            arr[0] = DropTestStruct::new(counter_2.clone());
            assert_eq!(counter_1.load(Ordering::Relaxed), 0);
            assert_eq!(counter_2.load(Ordering::Relaxed), 1);
        }
        assert_eq!(counter_1.load(Ordering::Relaxed), 0);
        assert_eq!(counter_2.load(Ordering::Relaxed), 0);
    }

    fn simple_pool_test(pool: &ArrayPool<u32>) {
        let mut borrowed = unsafe { pool.rent_or_create_uninitialized(3, false).unwrap() };
        borrowed[1] = 1;
        assert_eq!(borrowed[1], 1)
    }

    fn general_test_internal(){
        let pool = POOL.deref();
        simple_pool_test(pool);
    }

    fn test_wrapper<F: Fn() -> ()>(f: &F) {
        f();
    }

    #[test]
    fn general_test() {
        let a = 1;
        test_wrapper(&general_test_internal);
    }

    fn threading_test_internal(){
        let pool = POOL.deref();
        let cloned_pool_1 = pool.clone();
        let cloned_pool_2 = pool.clone();
        let handle_1 = thread::spawn(move ||{
            // Since the minimum capacity is 12, the returning slice's capacity is 16
            // If failed to borrow from any cached chain, create a new array
            // and initialized each element with the default value
            let mut slice = cloned_pool_1.rent(11).unwrap();
            slice[11] = 11;
            slice
        });
        let handle_2 = thread::spawn(move ||{
            // If failed to borrow from any cached chain, create a new array
            // without initializing any value, use with caution
            let mut slice = unsafe{ cloned_pool_2.rent_or_create_uninitialized(12, false) }.unwrap();
            slice[12] = 12;
            slice
        });

        let value_1 = handle_1.join().unwrap();
        let value_2 = handle_2.join().unwrap();

        assert_eq!(value_1[11], 11);
        assert_eq!(value_2[12], 12);
    }

    #[test]
    fn threading_test() {
        test_wrapper(&threading_test_internal)
    }

    fn test_vec_internal(){
        let pool = POOL.deref();
        let mut vec: PooledVec<u32> = PooledVec::new_with_pool(pool.clone());
        assert_eq!(vec.len(), 0);
        for x in 0..12{
            vec.push(x * 2);
        }
        let mut vec2 = vec.clone();
        let mut curr = 11usize * 2;
        let mut it = 0usize;
        while let Some(x) = vec2.pop(){
            assert_eq!(curr, x as usize);
            curr = curr.overflowing_sub(2).0;
            it += 1;
        }
        // println!("{vec2}");
        assert_eq!(it, 12);
        assert_eq!(vec2.len(), 0);
    }

    #[test]
    fn test_vec(){
        test_wrapper(&test_vec_internal)
    }
}
