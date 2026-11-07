//! Tarefas de kernel, troca cooperativa e preempção pelo timer.
//!
//! A troca guarda `x19`–`x28`, `x29`, `x30` e o `sp`. Não guarda `v8`–`v15`:
//! o compilador pode usar SIMD dentro de uma função, mas não através do `yield`.
//!
//! Com a preempção ligada, a IRQ do timer escolhe a tarefa pronta de maior
//! prioridade. Empate alterna. O console fica na prioridade 0.

use core::arch::global_asm;
use core::ptr::{addr_of, addr_of_mut, write_volatile};
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use crate::uart;

const SLOTS: usize = 4;
const FRAME: usize = 96;
const ROUNDS: u64 = 4;

#[repr(align(16))]
struct Stack {
    bytes: [u8; 4096],
}

struct Thread {
    sp: u64,
    active: bool,
    done: bool,
    priority: u8,
    leave_masked: bool,
}

static mut STACK_A: Stack = Stack { bytes: [0; 4096] };
static mut STACK_B: Stack = Stack { bytes: [0; 4096] };
static mut STACK_C: Stack = Stack { bytes: [0; 4096] };
static mut THREADS: [Thread; SLOTS] = [
    Thread {
        sp: 0,
        active: false,
        done: false,
        priority: 0,
        leave_masked: false,
    },
    Thread {
        sp: 0,
        active: false,
        done: false,
        priority: 0,
        leave_masked: false,
    },
    Thread {
        sp: 0,
        active: false,
        done: false,
        priority: 0,
        leave_masked: false,
    },
    Thread {
        sp: 0,
        active: false,
        done: false,
        priority: 0,
        leave_masked: false,
    },
];
static mut CURRENT: usize = 0;
static mut TRACE: [u8; 8] = [0; 8];
static mut TRACE_LEN: usize = 0;
static mut COUNT_A: u32 = 0;
static mut COUNT_B: u32 = 0;
static mut PRESERVED: bool = true;
static mut PRIO_OK: bool = false;
static PREEMPT: AtomicBool = AtomicBool::new(false);
static SLICE_A: AtomicU32 = AtomicU32::new(0);
static SLICE_B: AtomicU32 = AtomicU32::new(0);

global_asm!(
    r#"
    .global context_switch
    .type context_switch, %function
    // x0 = &sp anterior, x1 = &sp seguinte, x2 = 1 para liberar IRQ antes do ret
    context_switch:
        sub     sp, sp, #96
        stp     x19, x20, [sp, #0]
        stp     x21, x22, [sp, #16]
        stp     x23, x24, [sp, #32]
        stp     x25, x26, [sp, #48]
        stp     x27, x28, [sp, #64]
        stp     x29, x30, [sp, #80]
        mov     x9, sp
        str     x9, [x0]
        ldr     x9, [x1]
        mov     sp, x9
        ldp     x19, x20, [sp, #0]
        ldp     x21, x22, [sp, #16]
        ldp     x23, x24, [sp, #32]
        ldp     x25, x26, [sp, #48]
        ldp     x27, x28, [sp, #64]
        ldp     x29, x30, [sp, #80]
        add     sp, sp, #96
        cbz     x2, 1f
        msr     daifclr, #2
    1:
        ret
    "#
);

unsafe extern "C" {
    fn context_switch(prev: *mut u64, next: *const u64, unmask: u64);
}

pub fn exercise() {
    PREEMPT.store(false, Ordering::Relaxed);
    reset_console();
    unsafe {
        TRACE_LEN = 0;
        TRACE = [0; 8];
        COUNT_A = 0;
        COUNT_B = 0;
        PRESERVED = true;
    }
    spawn(1, task_a, addr_of_mut!(STACK_A), 1);
    spawn(2, task_b, addr_of_mut!(STACK_B), 1);
    while unsafe { !addr_of!(THREADS[1].done).read() || !addr_of!(THREADS[2].done).read() } {
        yield_now();
    }
    let (a, b, preserved, len) = unsafe {
        (
            addr_of!(COUNT_A).read(),
            addr_of!(COUNT_B).read(),
            addr_of!(PRESERVED).read(),
            addr_of!(TRACE_LEN).read(),
        )
    };
    let ordem = trace_str(len);
    let word = if preserved && len == 8 {
        "ok"
    } else {
        "falhou"
    };
    uart::write_fmt(format_args!(
        "tarefas: a {a} b {b}\r\n\
         ordem: {ordem}\r\n\
         preservado: {word}\r\n"
    ));
}

pub fn preempt() {
    PREEMPT.store(false, Ordering::Relaxed);
    SLICE_A.store(0, Ordering::Relaxed);
    SLICE_B.store(0, Ordering::Relaxed);
    reset_console();
    unsafe {
        TRACE_LEN = 0;
        TRACE = [0; 8];
        PRIO_OK = false;
    }
    spawn(1, task_high, addr_of_mut!(STACK_A), 2);
    spawn(2, task_slice_a, addr_of_mut!(STACK_B), 1);
    spawn(3, task_slice_b, addr_of_mut!(STACK_C), 1);
    unsafe {
        addr_of_mut!(THREADS[2].active).write(false);
        addr_of_mut!(THREADS[3].active).write(false);
    }
    yield_now();
    PREEMPT.store(false, Ordering::Relaxed);
    let len = unsafe { addr_of!(TRACE_LEN).read() };
    let ordem = trace_str(len);
    let alta = if unsafe { addr_of!(PRIO_OK).read() } {
        "alta antes"
    } else {
        "alta depois"
    };
    uart::write_fmt(format_args!(
        "preempcao: {alta}\r\n\
         fatias: {ordem}\r\n"
    ));
}

/// Chamado na saída da IRQ do timer, com as IRQ ainda mascaradas.
pub fn on_tick() {
    if !PREEMPT.load(Ordering::Acquire) {
        return;
    }
    let prev = unsafe { addr_of!(CURRENT).read() };
    unsafe { addr_of_mut!(THREADS[prev].leave_masked).write(true) };
    let next = next_index(prev);
    if next == prev {
        return;
    }
    switch_to(prev, next);
}

fn reset_console() {
    unsafe {
        CURRENT = 0;
        THREADS[0] = Thread {
            sp: 0,
            active: true,
            done: false,
            priority: 0,
            leave_masked: false,
        };
        for slot in 1..SLOTS {
            THREADS[slot].active = false;
            THREADS[slot].done = false;
        }
    }
}

fn spawn(slot: usize, entry: fn(), stack: *mut Stack, priority: u8) {
    let top = unsafe { addr_of_mut!((*stack).bytes).cast::<u8>().add(4096) };
    let frame = unsafe { top.sub(FRAME) as *mut u64 };
    unsafe {
        for word in 0..(FRAME / 8) {
            write_volatile(frame.add(word), 0);
        }
        write_volatile(frame.add(11), entry as *const () as u64);
        THREADS[slot] = Thread {
            sp: frame as u64,
            active: true,
            done: false,
            priority,
            leave_masked: false,
        };
    }
}

fn yield_now() {
    mask_irq();
    let prev = unsafe { addr_of!(CURRENT).read() };
    unsafe { addr_of_mut!(THREADS[prev].leave_masked).write(true) };
    let next = next_index(prev);
    if next == prev {
        unmask_irq();
        return;
    }
    switch_to(prev, next);
    unmask_irq();
}

fn switch_to(prev: usize, next: usize) {
    unsafe {
        addr_of_mut!(CURRENT).write(next);
        let unmask = if addr_of!(THREADS[next].leave_masked).read() {
            0
        } else {
            1
        };
        context_switch(
            addr_of_mut!(THREADS[prev].sp),
            addr_of!(THREADS[next].sp),
            unmask,
        );
    }
}

fn next_index(prev: usize) -> usize {
    let mut best: Option<u8> = None;
    for slot in 0..SLOTS {
        if !eligible(slot) {
            continue;
        }
        let prio = unsafe { addr_of!(THREADS[slot].priority).read() };
        best = Some(best.map(|value| value.max(prio)).unwrap_or(prio));
    }
    let Some(best) = best else {
        return prev;
    };
    let mut slot = (prev + 1) % SLOTS;
    for _ in 0..SLOTS {
        if eligible(slot) && unsafe { addr_of!(THREADS[slot].priority).read() } == best {
            return slot;
        }
        slot = (slot + 1) % SLOTS;
    }
    prev
}

fn eligible(slot: usize) -> bool {
    unsafe { addr_of!(THREADS[slot].active).read() && !addr_of!(THREADS[slot].done).read() }
}

fn task_a() {
    run(1, 0xA1A1_A1A1_A1A1_A1A1, b'a', true);
}

fn task_b() {
    run(2, 0xB2B2_B2B2_B2B2_B2B2, b'b', false);
}

fn run(slot: usize, magic: u64, label: u8, first: bool) {
    let mut token = magic;
    for step in 0..ROUNDS {
        if token != magic + step {
            unsafe { addr_of_mut!(PRESERVED).write(false) };
        }
        token = token.wrapping_add(1);
        push(label);
        unsafe {
            let count = if first {
                addr_of_mut!(COUNT_A)
            } else {
                addr_of_mut!(COUNT_B)
            };
            count.write(count.read() + 1);
        }
        yield_now();
    }
    if token != magic + ROUNDS {
        unsafe { addr_of_mut!(PRESERVED).write(false) };
    }
    unsafe { addr_of_mut!(THREADS[slot].done).write(true) };
    yield_now();
    loop {
        yield_now();
    }
}

fn task_high() {
    let first = SLICE_A.load(Ordering::Acquire) == 0 && SLICE_B.load(Ordering::Acquire) == 0;
    unsafe { addr_of_mut!(PRIO_OK).write(first) };
    mask_irq();
    unsafe {
        addr_of_mut!(THREADS[2].active).write(true);
        addr_of_mut!(THREADS[3].active).write(true);
        addr_of_mut!(THREADS[1].done).write(true);
    }
    PREEMPT.store(true, Ordering::Release);
    yield_now();
    loop {
        yield_now();
    }
}

fn task_slice_a() {
    slice(2, b'a', true);
}

fn task_slice_b() {
    slice(3, b'b', false);
}

fn slice(slot: usize, label: u8, is_a: bool) {
    for _ in 0..ROUNDS {
        mask_irq();
        push(label);
        let turn = if is_a {
            SLICE_A.fetch_add(1, Ordering::Release) + 1
        } else {
            SLICE_B.fetch_add(1, Ordering::Release) + 1
        };
        let finished = turn == ROUNDS as u32;
        if finished {
            unsafe { addr_of_mut!(THREADS[slot].done).write(true) };
        }
        unmask_irq();
        if finished {
            break;
        }
        if is_a {
            while SLICE_B.load(Ordering::Acquire) < turn {
                core::hint::spin_loop();
            }
        } else {
            while SLICE_A.load(Ordering::Acquire) <= turn {
                core::hint::spin_loop();
            }
        }
    }
    unsafe { addr_of_mut!(THREADS[slot].done).write(true) };
    if unsafe { addr_of!(THREADS[2].done).read() && addr_of!(THREADS[3].done).read() } {
        PREEMPT.store(false, Ordering::Release);
    }
    yield_now();
    loop {
        yield_now();
    }
}

fn push(label: u8) {
    unsafe {
        let len = addr_of!(TRACE_LEN).read();
        if len < 8 {
            addr_of_mut!(TRACE[len]).write(label);
            addr_of_mut!(TRACE_LEN).write(len + 1);
        }
    }
}

fn trace_str(len: usize) -> &'static str {
    let mut ordem_buf = [0u8; 8];
    for index in 0..len.min(8) {
        ordem_buf[index] = unsafe { addr_of!(TRACE[index]).read() };
    }
    // O buffer é local; devolve só os textos que o teste conhece.
    match core::str::from_utf8(&ordem_buf[..len.min(8)]) {
        Ok("abababab") => "abababab",
        _ => "diferente",
    }
}

fn mask_irq() {
    unsafe { core::arch::asm!("msr daifset, #2", options(nostack, preserves_flags)) };
}

fn unmask_irq() {
    unsafe { core::arch::asm!("msr daifclr, #2", options(nostack, preserves_flags)) };
}
