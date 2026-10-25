// The process-globals guard's fixture, its second target (T-TEST-PROCESS-GLOBALS): a test
// target whose manifest says `harness = false`, so its `main` is its only case. A global its
// `main` writes, one a foreign callback writes for a type `main` names, and one its global
// allocator writes are each a `one main` row that holds. Read by bt-source's `test_globals`
// test; never compiled.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use winit::application::ApplicationHandler;

static PROBED: AtomicBool = AtomicBool::new(false);
static RESUMED: AtomicBool = AtomicBool::new(false);
static COUNTED: AtomicUsize = AtomicUsize::new(0);

struct Counting;

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        COUNTED.fetch_add(layout.size(), Ordering::Relaxed);
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        unsafe { System.dealloc(pointer, layout) }
    }
}

#[global_allocator]
static COUNTING: Counting = Counting;

struct Probe;

impl ApplicationHandler for Probe {
    fn resumed(&mut self, _event_loop: &winit::event_loop::ActiveEventLoop) {
        RESUMED.store(true, Ordering::SeqCst);
    }
}

fn main() {
    PROBED.store(true, Ordering::SeqCst);
    let event_loop = winit::event_loop::EventLoop::new().unwrap();
    event_loop.run_app(&mut Probe).unwrap();
}
