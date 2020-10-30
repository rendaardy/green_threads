#![allow(dead_code)]
#![feature(llvm_asm, naked_functions)]

use std::ptr;

const DEFAULT_STACK_SIZE: usize = 1024 * 1024 * 2;
const MAX_THREADS: usize = 4;
static mut RUNTIME: usize = 0;

struct Runtime {
    threads: Vec<Thread>,
    current: usize,
}

impl Runtime {
    fn new() -> Self {
        // This will be our base thread, which will be initialized in
        // the `State::Running` state
        let base_thread = Thread {
            id: 0,
            stack: vec![0_u8; DEFAULT_STACK_SIZE],
            ctx: ThreadContext::default(),
            state: State::Running,
        };

        let mut threads = vec![base_thread];
        let mut avaliable_threads: Vec<Thread> = (1..MAX_THREADS).map(|i| Thread::new(i)).collect();
        threads.append(&mut avaliable_threads);

        Self {
            threads,
            current: 0,
        }
    }

    /// This is cheating a bit, but we need a pointer to our `Runtime`
    /// stored so we can call yield on it even if we don't have a
    /// reference to it.
    fn init(&self) {
        unsafe {
            let r_ptr: *const Runtime = self;
            RUNTIME = r_ptr as usize;
        }
    }

    fn run(&mut self) -> ! {
        while self.t_yield() {}
        std::process::exit(0);
    }

    fn t_return(&mut self) {
        if self.current != 0 {
            self.threads[self.current].state = State::Available;
            self.t_yield();
        }
    }

    fn t_yield(&mut self) -> bool {
        let mut pos = self.current;

        while self.threads[pos].state != State::Ready {
            pos += 1;

            if pos == self.threads.len() {
                pos = 0;
            }
            if pos == self.current {
                return false;
            }
        }

        if self.threads[self.current].state != State::Available {
            self.threads[self.current].state = State::Ready;
        }

        self.threads[pos].state = State::Running;
        let old_pos = self.current;
        self.current = pos;

        unsafe {
            switch(&mut self.threads[old_pos].ctx, &self.threads[pos].ctx);
        }

        // Prevents compiler from optimizing our code away on Windows.
        self.threads.len() > 0
    }

    fn spawn(&mut self, f: fn()) {
        let available = self
            .threads
            .iter_mut()
            .find(|t| t.state == State::Available)
            .expect("no available thread");

        let size = available.stack.len();
        unsafe {
            let s_ptr = available.stack.as_mut_ptr().offset(size as isize);
            let s_ptr = (s_ptr as usize & !15) as *mut u8;
            ptr::write(s_ptr.offset(-16) as *mut u64, guard as u64);
            ptr::write(s_ptr.offset(-24) as *mut u64, skip as u64);
            ptr::write(s_ptr.offset(-32) as *mut u64, f as u64);
            available.ctx.rsp = s_ptr.offset(-32) as u64;
        }
        available.state = State::Ready;
    }
}

fn guard() {
    unsafe {
        let rt_ptr = RUNTIME as *mut Runtime;
        (*rt_ptr).t_return();
    };
}

#[naked]
fn skip() {}

fn yield_thread() {
    unsafe {
        let rt_ptr = RUNTIME as *mut Runtime;
        (*rt_ptr).t_yield();
    }
}

#[derive(Debug, PartialEq, Eq)]
enum State {
    Available,
    Running,
    Ready,
}

struct Thread {
    id: usize,
    stack: Vec<u8>,
    ctx: ThreadContext,
    state: State,
}

impl Thread {
    fn new(id: usize) -> Self {
        Self {
            id,
            stack: vec![0_u8; DEFAULT_STACK_SIZE],
            ctx: ThreadContext::default(),
            state: State::Available,
        }
    }
}

// const SSIZE: isize = 48;
// static mut S_PTR: *const u8 = 0 as *const u8;

#[derive(Debug, Default)]
#[repr(C)]
struct ThreadContext {
    rsp: u64,
    r15: u64,
    r14: u64,
    r13: u64,
    r12: u64,
    rbx: u64,
    rbp: u64,
}

// fn print_stack(filename: &str) {
//     let mut f = std::fs::File::create(filename).unwrap();
//     unsafe {
//         for i in (0..SSIZE).rev() {
//             writeln!(
//                 f,
//                 "mem: {}, val: {}",
//                 S_PTR.offset(i as isize) as usize,
//                 *S_PTR.offset(i as isize)
//             )
//             .expect("Error writing to file");
//         }
//     }
// }

// fn hello() -> ! {
//     println!("I love waking up on a new stack!");
//     print_stack("AFTER.txt");

//     loop {}
// }

#[naked]
#[inline(never)]
unsafe fn switch(old: *const ThreadContext, new: *const ThreadContext) {
    llvm_asm!("
        mov %rsp, 0x00($0)
        mov %r15, 0x08($0)
        mov %r14, 0x10($0)
        mov %r13, 0x18($0)
        mov %r12, 0x20($0)
        mov %rbx, 0x28($0)
        mov %rbp, 0x30($0)

        mov 0x00($1), %rsp
        mov 0x08($1), %r15
        mov 0x10($1), %r14
        mov 0x18($1), %r13
        mov 0x20($1), %r12
        mov 0x28($1), %rbx
        mov 0x30($1), %rbp
        ret
        "
    :
    : "r"(old), "r"(new)
    :
    : "alignstack", "volatile"
    );
}

fn main() {
    let mut runtime = Runtime::new();
    runtime.init();
    runtime.spawn(|| {
        println!("Thread 1 Starting");
        let id = 1;
        for i in 0..10 {
            println!("thread: {} counter: {}", id, i);
            yield_thread();
        }
        println!("Thread 1 Finished");
    });

    runtime.spawn(|| {
        println!("Thread 2 Starting");
        let id = 2;
        for i in 0..15 {
            println!("thread: {} counter: {}", id, i);
            yield_thread();
        }
        println!("Thread 2 Finished");
    });

    runtime.run();

    // let mut ctx = ThreadContext::default();
    // let mut stack = vec![0_u8; SSIZE as usize];
    // let stack_ptr = stack.as_mut_ptr();

    // unsafe {
    // let stack_bottom = stack.as_mut_ptr().offset(SSIZE);
    // let sb_aligned = (stack_bottom as usize & !15) as *mut u8;
    // std::ptr::write(sb_aligned.offset(-16) as *mut u64, hello as u64);

    // S_PTR = stack_ptr;
    // std::ptr::write(stack_ptr.offset(SSIZE - 16) as *mut u64, hello as u64);
    // print_stack("BEFORE.txt");
    // ctx.rsp = stack_ptr.offset(SSIZE - 16) as u64;

    // gt_switch(&mut ctx);
    // }
}
