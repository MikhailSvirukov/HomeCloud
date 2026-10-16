//! Bounded, size-classed payload buffer pool.
//!
//! The pool owns a fixed number of large payload buffers per size class and
//! lends them out with explicit ownership: a caller must return a buffer to the
//! pool it came from. When every suitable class is drained, [`PayloadBufferPool::acquire`]
//! returns the controlled [`PoolError::Exhausted`] error instead of growing
//! memory, which callers translate into backpressure.
//!
//! This is a plain owned struct taking `&mut self`, intentionally **not** a
//! shared global lock: it is designed to live inside a single actor, so the hot
//! path never synchronizes through a shared `Mutex`/`RwLock`. It manages I/O
//! payload buffers only and does not replace the global Rust allocator.

type Result<T> = core::result::Result<T, PoolError>;

/// Default payload size classes (64 KiB, 128 KiB, 256 KiB, 512 KiB).
pub const DEFAULT_CLASS_CAPACITIES: [usize; 4] = [64 * 1024, 128 * 1024, 256 * 1024, 512 * 1024];

/// Default number of buffers pre-allocated per size class.
pub const DEFAULT_BUFFERS_PER_CLASS: usize = 8;

/// Description of one size class: `buffers` buffers of `capacity` bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SizeClass {
    /// Buffer capacity in bytes for this class.
    pub capacity: usize,
    /// Number of buffers pre-allocated for this class.
    pub buffers: usize,
}

impl SizeClass {
    /// Creates a size-class description.
    pub const fn new(capacity: usize, buffers: usize) -> Self {
        Self { capacity, buffers }
    }
}

/// A payload buffer owned by one actor at a time.
///
/// Its storage is fixed at the size-class capacity; `len` is a logical length
/// starting at `0` and bounded by [`PayloadBuffer::capacity`].
pub struct PayloadBuffer {
    class_index: usize,
    storage: Vec<u8>,
    len: usize,
}

impl std::fmt::Debug for PayloadBuffer {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PayloadBuffer")
            .field("capacity", &self.capacity())
            .field("len", &self.len)
            .finish()
    }
}

impl PayloadBuffer {
    /// Allocates the backing storage for `spec` at `class_index`.
    fn allocate(spec: SizeClass, class_index: usize) -> Self {
        Self {
            class_index,
            storage: vec![0u8; spec.capacity],
            len: 0,
        }
    }

    /// Fixed capacity of the backing storage in bytes.
    pub fn capacity(&self) -> usize {
        self.storage.len()
    }

    /// Logical length currently exposed through the slice accessors.
    pub fn len(&self) -> usize {
        self.len
    }

    /// Whether the logical length is zero.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Read access to the first [`PayloadBuffer::len`] bytes.
    pub fn as_slice(&self) -> &[u8] {
        &self.storage[..self.len]
    }

    /// Write access to the first [`PayloadBuffer::len`] bytes.
    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        &mut self.storage[..self.len]
    }

    /// Sets the logical length, which must not exceed the capacity.
    pub fn set_len(&mut self, len: usize) -> Result<()> {
        if len > self.capacity() {
            Err(PoolError::LengthExceedsCapacity {
                requested: len,
                capacity: self.capacity(),
            })
        } else {
            self.len = len;
            Ok(())
        }
    }
}

/// Per-class free list owned by the pool.
struct PoolClass {
    spec: SizeClass,
    free: Vec<PayloadBuffer>,
}

/// Bounded pool of size-classed payload buffers, owned by a single actor.
pub struct PayloadBufferPool {
    classes: Vec<PoolClass>,
    in_use: usize,
}

impl PayloadBufferPool {
    /// Builds a pool from a strictly increasing list of size classes, with
    /// every buffer pre-allocated so [`PayloadBufferPool::acquire`] never
    /// allocates.
    pub fn new(classes: &[SizeClass]) -> Result<Self> {
        if classes.is_empty() {
            return Err(PoolError::NoSizeClasses);
        }

        let strictly_increasing = classes
            .windows(2)
            .all(|pair| pair[0].capacity < pair[1].capacity);
        if !strictly_increasing {
            return Err(PoolError::SizeClassesNotIncreasing);
        }

        let classes = classes
            .iter()
            .enumerate()
            .map(|(index, spec)| PoolClass {
                spec: *spec,
                free: (0..spec.buffers)
                    .map(|_| PayloadBuffer::allocate(*spec, index))
                    .collect(),
            })
            .collect();

        Ok(Self { classes, in_use: 0 })
    }

    /// Builds a pool using [`DEFAULT_CLASS_CAPACITIES`] and
    /// [`DEFAULT_BUFFERS_PER_CLASS`].
    pub fn with_default_classes() -> Result<Self> {
        let classes = DEFAULT_CLASS_CAPACITIES
            .map(|capacity| SizeClass::new(capacity, DEFAULT_BUFFERS_PER_CLASS));
        Self::new(&classes)
    }

    /// Acquires the smallest suitable free buffer for `min_capacity` bytes,
    /// continuing with larger suitable classes when the smallest is drained; a
    /// buffer is never split.
    pub fn acquire(&mut self, min_capacity: usize) -> Result<PayloadBuffer> {
        let first_suitable = self
            .classes
            .iter()
            .position(|class| class.spec.capacity >= min_capacity);

        let Some(first) = first_suitable else {
            return Err(PoolError::NoSuitableClass {
                requested: min_capacity,
            });
        };

        match self.classes[first..]
            .iter_mut()
            .find_map(|class| class.free.pop())
        {
            Some(mut buffer) => {
                buffer.len = 0;
                self.in_use += 1;
                Ok(buffer)
            }
            None => Err(PoolError::Exhausted {
                requested: min_capacity,
            }),
        }
    }

    /// Returns a buffer to its originating size class, resetting its logical
    /// length so the storage can be reused.
    pub fn release(&mut self, mut buffer: PayloadBuffer) {
        let class_index = buffer.class_index;
        buffer.len = 0;

        debug_assert_eq!(buffer.capacity(), self.classes[class_index].spec.capacity);
        debug_assert!(self.in_use > 0);
        self.in_use -= 1;
        self.classes[class_index].free.push(buffer);
    }

    /// Number of buffers currently checked out.
    pub const fn in_use(&self) -> usize {
        self.in_use
    }

    /// Number of free buffers across every size class.
    pub fn available(&self) -> usize {
        self.classes.iter().map(|class| class.free.len()).sum()
    }
}

impl std::fmt::Debug for PayloadBufferPool {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PayloadBufferPool")
            .field("class_count", &self.classes.len())
            .field("in_use", &self.in_use)
            .field("available", &self.available())
            .finish()
    }
}

/// Failure modes when configuring or using a [`PayloadBufferPool`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum PoolError {
    /// No size class can hold the requested capacity.
    #[error("no class for {requested} bytes")]
    NoSuitableClass { requested: usize },
    /// Every suitable size class is fully checked out.
    #[error("exhausted at {requested} bytes")]
    Exhausted { requested: usize },
    /// The pool was configured with an empty size-class list.
    #[error("no size classes configured")]
    NoSizeClasses,
    /// Size-class capacities are not strictly increasing.
    #[error("size class capacities are not strictly increasing")]
    SizeClassesNotIncreasing,
    /// A buffer length was set beyond its capacity.
    #[error("length {requested} exceeds capacity {capacity}")]
    LengthExceedsCapacity { requested: usize, capacity: usize },
}

#[cfg(test)]
mod tests {
    use super::{PayloadBufferPool, PoolError, SizeClass, DEFAULT_CLASS_CAPACITIES};

    fn default_pool() -> PayloadBufferPool {
        PayloadBufferPool::with_default_classes().expect("default configuration is valid")
    }

    #[test]
    fn acquire_reports_no_suitable_class_when_request_exceeds_largest() {
        let mut pool = default_pool();
        let requested = DEFAULT_CLASS_CAPACITIES[DEFAULT_CLASS_CAPACITIES.len() - 1] + 1;

        assert_eq!(
            pool.acquire(requested).err(),
            Some(PoolError::NoSuitableClass { requested })
        );
    }

    #[test]
    fn acquire_and_release_update_pool_bookkeeping() {
        let mut pool = default_pool();
        let initial_available = pool.available();

        let buffer = pool
            .acquire(DEFAULT_CLASS_CAPACITIES[0])
            .expect("smallest class has free buffers");
        assert_eq!(pool.in_use(), 1);
        assert_eq!(pool.available(), initial_available - 1);

        pool.release(buffer);
        assert_eq!(pool.in_use(), 0);
        assert_eq!(pool.available(), initial_available);
    }

    #[test]
    fn set_len_rejects_length_beyond_capacity() {
        let mut pool = default_pool();
        let mut buffer = pool.acquire(1).expect("smallest class has free buffers");
        let capacity = buffer.capacity();

        buffer
            .set_len(4)
            .expect("length within capacity is allowed");
        buffer.as_mut_slice().copy_from_slice(&[1, 2, 3, 4]);
        assert_eq!(buffer.as_slice(), &[1, 2, 3, 4]);

        buffer
            .set_len(capacity)
            .expect("length equal to capacity is allowed");
        assert_eq!(buffer.len(), capacity);

        assert_eq!(buffer.set_len(0), Ok(()));
        assert!(buffer.is_empty());
        assert_eq!(
            buffer.set_len(capacity + 1),
            Err(PoolError::LengthExceedsCapacity {
                requested: capacity + 1,
                capacity,
            })
        );
    }

    #[test]
    fn new_rejects_empty_class_list() {
        assert_eq!(
            PayloadBufferPool::new(&[]).err(),
            Some(PoolError::NoSizeClasses)
        );
    }

    #[test]
    fn new_rejects_non_increasing_capacities() {
        let classes = [
            SizeClass::new(DEFAULT_CLASS_CAPACITIES[1], 1),
            SizeClass::new(DEFAULT_CLASS_CAPACITIES[0], 1),
        ];

        assert_eq!(
            PayloadBufferPool::new(&classes).err(),
            Some(PoolError::SizeClassesNotIncreasing)
        );
    }

    #[test]
    fn buffer_debug_reports_capacity_and_len_without_bytes() {
        let mut pool = default_pool();
        let mut buffer = pool.acquire(1).expect("smallest class has free buffers");
        buffer
            .set_len(4)
            .expect("length within capacity is allowed");
        buffer.as_mut_slice().copy_from_slice(b"SECR");

        let text = format!("{buffer:?}");

        assert!(text.contains("len: 4"));
        assert!(!text.contains("SECR"));
    }
}
