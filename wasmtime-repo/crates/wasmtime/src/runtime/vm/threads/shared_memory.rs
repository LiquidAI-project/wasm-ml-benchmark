use crate::prelude::*;
use crate::runtime::vm::memory::{validate_atomic_addr, MmapMemory};
use crate::runtime::vm::threads::parking_spot::{ParkingSpot, Waiter};
use crate::runtime::vm::vmcontext::VMMemoryDefinition;
use crate::runtime::vm::{Memory, RuntimeLinearMemory, Store, WaitResult};
use anyhow::Error;
use anyhow::{bail, Result};
use std::cell::RefCell;
use std::ops::Range;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};
use wasmtime_environ::{MemoryPlan, MemoryStyle, Trap};

/// For shared memory (and only for shared memory), this lock-version restricts
/// access when growing the memory or checking its size. This is to conform with
/// the [thread proposal]: "When `IsSharedArrayBuffer(...)` is true, the return
/// value should be the result of an atomic read-modify-write of the new size to
/// the internal `length` slot."
///
/// [thread proposal]:
///     https://github.com/WebAssembly/threads/blob/master/proposals/threads/Overview.md#webassemblymemoryprototypegrow
#[derive(Clone)]
pub struct SharedMemory(Arc<SharedMemoryInner>);

struct SharedMemoryInner {
    memory: RwLock<Box<dyn RuntimeLinearMemory>>,
    spot: ParkingSpot,
    ty: wasmtime_environ::Memory,
    def: LongTermVMMemoryDefinition,
}

impl SharedMemory {
    /// Construct a new [`SharedMemory`].
    pub fn new(plan: MemoryPlan) -> Result<Self> {
        let (minimum_bytes, maximum_bytes) = Memory::limit_new(&plan, None)?;
        let mmap_memory = MmapMemory::new(&plan, minimum_bytes, maximum_bytes, None)?;
        Self::wrap(&plan, Box::new(mmap_memory), plan.memory)
    }

    /// Wrap an existing [Memory] with the locking provided by a [SharedMemory].
    pub fn wrap(
        plan: &MemoryPlan,
        mut memory: Box<dyn RuntimeLinearMemory>,
        ty: wasmtime_environ::Memory,
    ) -> Result<Self> {
        if !ty.shared {
            bail!("shared memory must have a `shared` memory type");
        }
        if !matches!(plan.style, MemoryStyle::Static { .. }) {
            bail!("shared memory can only be built from a static memory allocation")
        }
        assert!(
            memory.as_any_mut().type_id() != std::any::TypeId::of::<SharedMemory>(),
            "cannot re-wrap a shared memory"
        );
        Ok(Self(Arc::new(SharedMemoryInner {
            ty,
            spot: ParkingSpot::default(),
            def: LongTermVMMemoryDefinition(memory.vmmemory()),
            memory: RwLock::new(memory),
        })))
    }

    /// Return the memory type for this [`SharedMemory`].
    pub fn ty(&self) -> wasmtime_environ::Memory {
        self.0.ty
    }

    /// Convert this shared memory into a [`Memory`].
    pub fn as_memory(self) -> Memory {
        Memory(Box::new(self))
    }

    /// Return a pointer to the shared memory's [VMMemoryDefinition].
    pub fn vmmemory_ptr(&self) -> *const VMMemoryDefinition {
        &self.0.def.0
    }

    /// Same as `RuntimeLinearMemory::grow`, except with `&self`.
    pub fn grow(
        &self,
        delta_pages: u64,
        store: Option<&mut dyn Store>,
    ) -> Result<Option<(usize, usize)>, Error> {
        let mut memory = self.0.memory.write().unwrap();
        let result = memory.grow(delta_pages, store)?;
        if let Some((_old_size_in_bytes, new_size_in_bytes)) = result {
            // Store the new size to the `VMMemoryDefinition` for JIT-generated
            // code (and runtime functions) to access. No other code can be
            // growing this memory due to the write lock, but code in other
            // threads could have access to this shared memory and we want them
            // to see the most consistent version of the `current_length`; a
            // weaker consistency is possible if we accept them seeing an older,
            // smaller memory size (assumption: memory only grows) but presently
            // we are aiming for accuracy.
            //
            // Note that it could be possible to access a memory address that is
            // now-valid due to changes to the page flags in `grow` above but
            // beyond the `memory.size` that we are about to assign to. In these
            // and similar cases, discussion in the thread proposal concluded
            // that: "multiple accesses in one thread racing with another
            // thread's `memory.grow` that are in-bounds only after the grow
            // commits may independently succeed or trap" (see
            // https://github.com/WebAssembly/threads/issues/26#issuecomment-433930711).
            // In other words, some non-determinism is acceptable when using
            // `memory.size` on work being done by `memory.grow`.
            self.0
                .def
                .0
                .current_length
                .store(new_size_in_bytes, Ordering::SeqCst);
        }
        Ok(result)
    }

    /// Implementation of `memory.atomic.notify` for this shared memory.
    pub fn atomic_notify(&self, addr_index: u64, count: u32) -> Result<u32, Trap> {
        let ptr = validate_atomic_addr(&self.0.def.0, addr_index, 4, 4)?;
        log::trace!("memory.atomic.notify(addr={addr_index:#x}, count={count})");
        let ptr = unsafe { &*ptr };
        Ok(self.0.spot.notify(ptr, count))
    }

    /// Implementation of `memory.atomic.wait32` for this shared memory.
    pub fn atomic_wait32(
        &self,
        addr_index: u64,
        expected: u32,
        timeout: Option<Duration>,
    ) -> Result<WaitResult, Trap> {
        let addr = validate_atomic_addr(&self.0.def.0, addr_index, 4, 4)?;
        log::trace!(
            "memory.atomic.wait32(addr={addr_index:#x}, expected={expected}, timeout={timeout:?})"
        );

        // SAFETY: `addr_index` was validated by `validate_atomic_addr` above.
        assert!(std::mem::size_of::<AtomicU32>() == 4);
        assert!(std::mem::align_of::<AtomicU32>() <= 4);
        let atomic = unsafe { AtomicU32::from_ptr(addr.cast()) };
        let deadline = timeout.map(|d| Instant::now() + d);

        WAITER.with(|waiter| {
            let mut waiter = waiter.borrow_mut();
            Ok(self.0.spot.wait32(atomic, expected, deadline, &mut waiter))
        })
    }

    /// Implementation of `memory.atomic.wait64` for this shared memory.
    pub fn atomic_wait64(
        &self,
        addr_index: u64,
        expected: u64,
        timeout: Option<Duration>,
    ) -> Result<WaitResult, Trap> {
        let addr = validate_atomic_addr(&self.0.def.0, addr_index, 8, 8)?;
        log::trace!(
            "memory.atomic.wait64(addr={addr_index:#x}, expected={expected}, timeout={timeout:?})"
        );

        // SAFETY: `addr_index` was validated by `validate_atomic_addr` above.
        assert!(std::mem::size_of::<AtomicU64>() == 8);
        assert!(std::mem::align_of::<AtomicU64>() <= 8);
        let atomic = unsafe { AtomicU64::from_ptr(addr.cast()) };
        let deadline = timeout.map(|d| Instant::now() + d);

        WAITER.with(|waiter| {
            let mut waiter = waiter.borrow_mut();
            Ok(self.0.spot.wait64(atomic, expected, deadline, &mut waiter))
        })
    }
}

thread_local! {
    /// Structure used in conjunction with `ParkingSpot` to block the current
    /// thread if necessary. Note that this is lazily initialized.
    static WAITER: RefCell<Waiter> = const { RefCell::new(Waiter::new()) };
}

/// Shared memory needs some representation of a `VMMemoryDefinition` for
/// JIT-generated code to access. This structure owns the base pointer and
/// length to the actual memory and we share this definition across threads by:
/// - never changing the base pointer; according to the specification, shared
///   memory must be created with a known maximum size so it can be allocated
///   once and never moved
/// - carefully changing the length, using atomic accesses in both the runtime
///   and JIT-generated code.
struct LongTermVMMemoryDefinition(VMMemoryDefinition);
unsafe impl Send for LongTermVMMemoryDefinition {}
unsafe impl Sync for LongTermVMMemoryDefinition {}

/// Proxy all calls through the [`RwLock`].
impl RuntimeLinearMemory for SharedMemory {
    fn page_size_log2(&self) -> u8 {
        self.0.memory.read().unwrap().page_size_log2()
    }

    fn byte_size(&self) -> usize {
        self.0.memory.read().unwrap().byte_size()
    }

    fn maximum_byte_size(&self) -> Option<usize> {
        self.0.memory.read().unwrap().maximum_byte_size()
    }

    fn grow(
        &mut self,
        delta_pages: u64,
        store: Option<&mut dyn Store>,
    ) -> Result<Option<(usize, usize)>, Error> {
        SharedMemory::grow(self, delta_pages, store)
    }

    fn grow_to(&mut self, size: usize) -> Result<()> {
        self.0.memory.write().unwrap().grow_to(size)
    }

    fn vmmemory(&mut self) -> VMMemoryDefinition {
        // `vmmemory()` is used for writing the `VMMemoryDefinition` of a memory
        // into its `VMContext`; this should never be possible for a shared
        // memory because the only `VMMemoryDefinition` for it should be stored
        // in its own `def` field.
        unreachable!()
    }

    fn needs_init(&self) -> bool {
        self.0.memory.read().unwrap().needs_init()
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    fn wasm_accessible(&self) -> Range<usize> {
        self.0.memory.read().unwrap().wasm_accessible()
    }
}
