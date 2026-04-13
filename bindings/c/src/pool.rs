use std::mem::MaybeUninit;
use std::os::raw::c_void;

/// A pool for storing pointers of the same type
pub struct Pool<V> {
    /// The free slots in our pool
    free: Vec<u64>,

    /// The items of our pool
    items: Box<[MaybeUninit<V>]>,

    /// Is this pool complete
    complete: bool,

    /// The next pool
    next: Option<Box<Pool<V>>>,
}

impl<V> Pool<V> {
    pub fn new(size: usize) -> Self {
        let mut items: Vec<MaybeUninit<V>> = Vec::with_capacity(size * 64);
        unsafe {
            // Safety: MaybeUninit does not require initialization
            items.set_len(size * 64);
        }

        Pool {
            free: vec![!0u64; size], // all bits = free

            items: items.into_boxed_slice(),

            complete: false,

            next: None,
        }
    }

    fn size(&self) -> usize {
        self.free.len()
    }

    fn next(&mut self) -> &mut Self {
        if self.next.is_none() {
            self.next = Some(Box::new(Pool::new(self.size() * 2)));
        }

        self.next.as_mut().unwrap()
    }

    /// Returns a pointer to a value
    pub fn alloc(&mut self, v: V) -> *mut c_void {
        // If this pool still has free slots
        if !self.complete {
            for (block_idx, mask) in self.free.iter_mut().enumerate() {
                if *mask != 0 {
                    let bit = mask.trailing_zeros() as usize;
                    *mask &= !(1u64 << bit);

                    let idx = block_idx * 64 + bit;

                    self.items[idx].write(v);
                    return self.items[idx].as_ptr() as *mut c_void;
                }
            }
            self.complete = true;
        }

        // No free slots here -> try next pool
        self.next().alloc(v)
    }

    /// Gets the index of a pointer in this pool
    /// If the pointer does not belong to this pool return None
    fn get_index(&self, ptr: *const c_void) -> Option<usize> {
        let p = ptr as *mut V;
        let base = self.items.as_ptr() as *const V;

        // Safety: why is this unsafe ???
        let end = unsafe { base.add(64 * self.size()) };

        if (p as *const V) >= base && (p as *const V) < end {
            // Safety: caller promises ptr is from this pool therefore from the slice
            unsafe { Some(p.offset_from(base) as usize) }
        } else {
            None
        }
    }

    /// Marks an index as free
    fn mark_free(&mut self, idx: usize) {
        let block = idx / 64;
        let bit = idx % 64;
        self.free[block] |= 1u64 << bit;
        self.complete = false;
    }

    /// Frees a pointer
    /// Safety: caller promises ptr is valid and allocated in this pool
    pub unsafe fn free(&mut self, ptr: *mut c_void) {
        match self.get_index(ptr) {
            Some(idx) => {
                // Safety: caller promises ptr is valid
                unsafe {
                    ptr.drop_in_place();
                }
                self.mark_free(idx);
            }

            None => {
                match self.next.as_mut() {
                    Some(n) => {
                        // Safety: caller promises ptr is valid and allocated in this pool
                        unsafe { n.free(ptr) }
                    }
                    None => panic!("Pointer does not belong to the pool"),
                }
            }
        }
    }

    /// Takes ownership of a value in the pool
    /// Safety: caller promises ptr is valid and allocated in this pool
    pub unsafe fn take(&mut self, ptr: *mut c_void) -> V {
        match self.get_index(ptr) {
            Some(idx) => {
                self.mark_free(idx);
                // Safety: caller promises ptr is valid
                unsafe { self.items[idx].assume_init_read() }
            }

            None => {
                match self.next.as_mut() {
                    Some(n) => {
                        // Safety: caller promises ptr is valid and allocated in this pool
                        unsafe { n.take(ptr) }
                    }
                    None => panic!("Pointer does not belong to the pool"),
                }
            }
        }
    }
}
