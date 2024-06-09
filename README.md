# Array pool

A simple, tiered caching array pool implementation in Rust.

## Example

```rust
use std::sync::Arc;
use std::thread;
use array_pool::pool::ArrayPool;

fn main(){
    let pool: Arc<ArrayPool<i32>> = Arc::new(ArrayPool::new());
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
        let mut slice = unsafe{ cloned_pool_2.rent_or_create_uninitialized(12) }.unwrap();
        slice[12] = 12;
        slice
    });

    let value_1 = handle_1.join().unwrap();
    let value_2 = handle_2.join().unwrap();

    assert_eq!(value_1[11], 11);
    assert_eq!(value_2[12], 12);
}
```

The provided `PooledVec` type can utilize the array pool:

```rust
use std::sync::Arc;
use array_pool::{vec::PooledVec, pool::ArrayPool};

fn main(){
    let pool: Arc<ArrayPool<u32>> = Arc::new(ArrayPool::new());
    let mut vec: PooledVec<u32> = PooledVec::new_with_pool(pool);
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
    println!("{vec2}");
    assert_eq!(it, 12);
    assert_eq!(vec2.len(), 0);
}
```
