//! Teclado USB, configuração curta.
//!
//! O bloco é o DWC2 em `0x3F980000` (`USB_OTG_OFFSET` 0x980000 no
//! `raspi_platform.h` do QEMU 8.2). Os deslocamentos e os bits vêm de
//! `include/hw/usb/dwc2-regs.h` dessa versão. O modelo não implementa a
//! FIFO de slave, então a transferência é por DMA, com o endereço físico
//! igual ao virtual porque o mapa é identidade.
//!
//! Cabe um hub de um nível e um teclado HID de protocolo de boot. No
//! `raspi3b` o QEMU põe um hub full-speed na porta e o teclado atrás dele.
//! No Pi 3B+ o hub da LAN é high-speed e o teclado atrás dele pede split.

use core::ptr::{read_volatile, write_volatile};

use crate::timer;
use crate::uart;

const BASE: usize = 0x3F98_0000;

const GUSBCFG: usize = 0x00c;
const GRSTCTL: usize = 0x010;
const GINTSTS: usize = 0x014;
const GRXFSIZ: usize = 0x024;
const GNPTXFSIZ: usize = 0x028;
const HPTXFSIZ: usize = 0x100;
const HCFG: usize = 0x400;
const HFIR: usize = 0x404;
const HFNUM: usize = 0x408;
const HPRT0: usize = 0x440;
const GAHBCFG: usize = 0x008;

const GAHBCFG_DMA_EN: u32 = 1 << 5;
const GAHBCFG_GLBL_INTR_EN: u32 = 1 << 0;
const GUSBCFG_FORCEHOSTMODE: u32 = 1 << 29;
const GUSBCFG_TRDTIM_MASK: u32 = 0xf << 10;
const GRSTCTL_AHBIDLE: u32 = 1 << 31;
const GRSTCTL_CSFTRST: u32 = 1 << 0;
const GINTSTS_CURMODE_HOST: u32 = 1 << 0;

const HPRT0_SPD_SHIFT: u32 = 17;
const HPRT0_PWR: u32 = 1 << 12;
const HPRT0_RST: u32 = 1 << 8;
const HPRT0_ENACHG: u32 = 1 << 3;
const HPRT0_ENA: u32 = 1 << 2;
const HPRT0_CONNDET: u32 = 1 << 1;
const HPRT0_CONNSTS: u32 = 1 << 0;
const HPRT0_OVRCURRCHG: u32 = 1 << 5;

const HCCHAR_CHENA: u32 = 1 << 31;
const HCCHAR_ODDFRM: u32 = 1 << 29;
const HCCHAR_LSPDDEV: u32 = 1 << 17;
const HCCHAR_EPDIR: u32 = 1 << 15;

const HCSPLT_SPLTENA: u32 = 1 << 31;
const HCSPLT_COMPSPLT: u32 = 1 << 16;

const HCINT_XFERCOMPL: u32 = 1 << 0;
const HCINT_CHHLTD: u32 = 1 << 1;
const HCINT_STALL: u32 = 1 << 3;
const HCINT_NAK: u32 = 1 << 4;
const HCINT_NYET: u32 = 1 << 6;
const HCINT_XACTERR: u32 = 1 << 7;
const HCINT_BBLERR: u32 = 1 << 8;
const HCINT_AHBERR: u32 = 1 << 2;
const HCINT_DATATGLERR: u32 = 1 << 10;

const PID_DATA0: u32 = 0;
const PID_DATA1: u32 = 2;
const PID_SETUP: u32 = 3;

const EP_CONTROL: u32 = 0;
const EP_INTERRUPT: u32 = 3;

#[derive(Clone, Copy)]
struct Route {
    addr: u8,
    mps: u16,
    low: bool,
    split: bool,
    hub: u8,
    port: u8,
}

struct Keyboard {
    on: bool,
    route: Route,
    ep: u8,
    data1: bool,
    last: [u8; 6],
    queue: [u8; 8],
    qlen: usize,
}

#[repr(C, align(64))]
struct Dma {
    bytes: [u8; 256],
}

static mut DMA: Dma = Dma { bytes: [0; 256] };
static mut NEXT_ADDR: u8 = 1;
static mut KBD: Keyboard = Keyboard {
    on: false,
    route: Route {
        addr: 0,
        mps: 8,
        low: false,
        split: false,
        hub: 0,
        port: 0,
    },
    ep: 0,
    data1: false,
    last: [0; 6],
    queue: [0; 8],
    qlen: 0,
};

pub fn init() {
    if !host_up() {
        uart::puts("teclado: ausente\r\n");
        return;
    }
    let speed = port_speed();
    let route = Route {
        addr: 0,
        mps: 8,
        low: speed == 2,
        split: false,
        hub: 0,
        port: 0,
    };
    if enumerate(route, 0) {
        uart::puts("teclado: hid\r\n");
    } else {
        uart::puts("teclado: sem hid\r\n");
    }
}

/// Uma tecla nova do relatório HID, se o teclado já foi achado.
pub fn poll() -> Option<u8> {
    let kbd = unsafe { &mut *core::ptr::addr_of_mut!(KBD) };
    if !kbd.on {
        return None;
    }
    if kbd.qlen > 0 {
        return Some(pop(kbd));
    }
    let mut report = [0u8; 8];
    let n = match interrupt_in(kbd, &mut report) {
        Some(n) => n,
        None => return None,
    };
    if n < 3 {
        return None;
    }
    let mut fresh = [0u8; 6];
    let mut count = 0;
    for slot in 0..6 {
        let code = report[2 + slot];
        if code < 2 {
            continue;
        }
        let mut seen = false;
        for old in kbd.last {
            if old == code {
                seen = true;
                break;
            }
        }
        fresh[count] = code;
        count += 1;
        if !seen {
            if let Some(ch) = ascii(report[0], code) {
                push(kbd, ch);
            }
        }
    }
    kbd.last = [0; 6];
    for index in 0..count {
        kbd.last[index] = fresh[index];
    }
    if kbd.qlen > 0 {
        Some(pop(kbd))
    } else {
        None
    }
}

fn enumerate(mut route: Route, depth: u8) -> bool {
    let mut desc = [0u8; 18];
    if !control_in(&route, req(0x80, 6, 0x100, 0, 8), &mut desc[..8]) {
        return false;
    }
    let mps = desc[7] as u16;
    if mps == 0 || mps > 64 {
        return false;
    }
    route.mps = mps;
    if !control_in(&route, req(0x80, 6, 0x100, 0, 18), &mut desc) {
        return false;
    }
    let class = desc[4];
    let addr = alloc_addr();
    if !control_out(&route, req(0x00, 5, addr as u16, 0, 0), &[]) {
        return false;
    }
    delay_us(2_000);
    route.addr = addr;
    let mut conf = [0u8; 9];
    if !control_in(&route, req(0x80, 6, 0x200, 0, 9), &mut conf) {
        return false;
    }
    let total = u16::from_le_bytes([conf[2], conf[3]]) as usize;
    let value = conf[5];
    let mut full = [0u8; 256];
    let want = total.min(full.len());
    if want < 9
        || !control_in(
            &route,
            req(0x80, 6, 0x200, 0, want as u16),
            &mut full[..want],
        )
    {
        return false;
    }
    if !control_out(&route, req(0x00, 9, value as u16, 0, 0), &[]) {
        return false;
    }
    if class == 9 || interface_class(&full[..want], 9) {
        if depth > 0 {
            return false;
        }
        return scan_hub(&route, &full[..want]);
    }
    claim_keyboard(&route, &full[..want])
}

fn scan_hub(hub: &Route, _conf: &[u8]) -> bool {
    let mut hub_desc = [0u8; 9];
    if !control_in(hub, req(0xa0, 6, 0x2900, 0, 9), &mut hub_desc) {
        return false;
    }
    let ports = hub_desc[2].clamp(1, 8);
    let parent_high = port_speed() == 0;
    for port in 1..=ports {
        if !control_out(hub, req(0x23, 3, 8, port as u16, 0), &[]) {
            continue;
        }
        delay_us(50_000);
        let mut status = [0u8; 4];
        if !control_in(hub, req(0xa3, 0, 0, port as u16, 4), &mut status) {
            continue;
        }
        let state = u16::from_le_bytes([status[0], status[1]]);
        if state & 1 == 0 {
            continue;
        }
        if !control_out(hub, req(0x23, 3, 4, port as u16, 0), &[]) {
            continue;
        }
        if !wait_port_reset(hub, port) {
            continue;
        }
        let _ = control_out(hub, req(0x23, 1, 0x14, port as u16, 0), &[]);
        let mut after = [0u8; 4];
        if !control_in(hub, req(0xa3, 0, 0, port as u16, 4), &mut after) {
            continue;
        }
        let bits = u16::from_le_bytes([after[0], after[1]]);
        let low = bits & (1 << 9) != 0;
        let high = bits & (1 << 10) != 0;
        let child = Route {
            addr: 0,
            mps: 8,
            low,
            split: parent_high && !high,
            hub: hub.addr,
            port,
        };
        if enumerate(child, 1) {
            return true;
        }
    }
    false
}

fn claim_keyboard(route: &Route, conf: &[u8]) -> bool {
    let Some((iface, ep, mps)) = hid_keyboard(conf) else {
        return false;
    };
    if !control_out(route, req(0x21, 0x0b, 0, iface as u16, 0), &[]) {
        return false;
    }
    if !control_out(route, req(0x21, 0x0a, 0, iface as u16, 0), &[]) {
        return false;
    }
    let kbd = unsafe { &mut *core::ptr::addr_of_mut!(KBD) };
    kbd.on = true;
    kbd.route = *route;
    kbd.route.mps = mps.max(8);
    kbd.ep = ep;
    kbd.data1 = false;
    kbd.last = [0; 6];
    kbd.qlen = 0;
    true
}

fn hid_keyboard(conf: &[u8]) -> Option<(u8, u8, u16)> {
    let mut index = 0;
    let mut arm = false;
    let mut iface = 0u8;
    while index + 2 <= conf.len() {
        let len = conf[index] as usize;
        if len < 2 || index + len > conf.len() {
            break;
        }
        match conf[index + 1] {
            4 if len >= 9 => {
                arm = conf[index + 5] == 3 && conf[index + 6] == 1 && conf[index + 7] == 1;
                iface = conf[index + 2];
            }
            5 if len >= 7 && arm => {
                let addr = conf[index + 2];
                let attr = conf[index + 3];
                if addr & 0x80 != 0 && attr & 3 == 3 {
                    let mps = u16::from_le_bytes([conf[index + 4], conf[index + 5]]);
                    return Some((iface, addr & 0x0f, mps & 0x7ff));
                }
            }
            _ => {}
        }
        index += len;
    }
    None
}

fn interface_class(conf: &[u8], class: u8) -> bool {
    let mut index = 0;
    while index + 2 <= conf.len() {
        let len = conf[index] as usize;
        if len < 2 || index + len > conf.len() {
            break;
        }
        if conf[index + 1] == 4 && len >= 9 && conf[index + 5] == class {
            return true;
        }
        index += len;
    }
    false
}

fn wait_port_reset(hub: &Route, port: u8) -> bool {
    let start = timer::clo();
    while timer::clo().wrapping_sub(start) < 100_000 {
        let mut status = [0u8; 4];
        if !control_in(hub, req(0xa3, 0, 0, port as u16, 4), &mut status) {
            delay_us(1_000);
            continue;
        }
        let bits = u16::from_le_bytes([status[0], status[1]]);
        let change = u16::from_le_bytes([status[2], status[3]]);
        if bits & (1 << 4) == 0 && (bits & (1 << 1) != 0 || change & (1 << 4) != 0) {
            return true;
        }
        delay_us(1_000);
    }
    false
}

fn control_in(route: &Route, setup: [u8; 8], data: &mut [u8]) -> bool {
    control(route, setup, &[], Some(data))
}

fn control_out(route: &Route, setup: [u8; 8], data: &[u8]) -> bool {
    control(route, setup, data, None)
}

fn control(route: &Route, setup: [u8; 8], data_out: &[u8], mut data_in: Option<&mut [u8]>) -> bool {
    for _ in 0..8 {
        if !stage(route, EP_CONTROL, 0, PID_SETUP, false, 8, Some(&setup)) {
            delay_us(1_000);
            continue;
        }
        let data_ok = if let Some(buf) = data_in.as_deref_mut() {
            stage(route, EP_CONTROL, 0, PID_DATA1, true, buf.len(), None) && copy_in(buf)
        } else if !data_out.is_empty() {
            stage(
                route,
                EP_CONTROL,
                0,
                PID_DATA1,
                false,
                data_out.len(),
                Some(data_out),
            )
        } else {
            true
        };
        if !data_ok {
            delay_us(1_000);
            continue;
        }
        let status_in = data_in.is_none();
        if stage(route, EP_CONTROL, 0, PID_DATA1, status_in, 0, None) {
            return true;
        }
        delay_us(1_000);
    }
    false
}

fn copy_in(buf: &mut [u8]) -> bool {
    cache_invalidate(buf.len());
    let dma = unsafe { &*core::ptr::addr_of!(DMA) };
    let n = buf.len().min(dma.bytes.len());
    buf[..n].copy_from_slice(&dma.bytes[..n]);
    true
}

fn interrupt_in(kbd: &mut Keyboard, report: &mut [u8; 8]) -> Option<usize> {
    let pid = if kbd.data1 { PID_DATA1 } else { PID_DATA0 };
    let kind = stage(&kbd.route, EP_INTERRUPT, kbd.ep, pid, true, 8, None);
    if !kind {
        return None;
    }
    cache_invalidate(8);
    let dma = unsafe { &*core::ptr::addr_of!(DMA) };
    report.copy_from_slice(&dma.bytes[..8]);
    kbd.data1 = !kbd.data1;
    Some(8)
}

fn stage(
    route: &Route,
    kind: u32,
    ep: u8,
    pid: u32,
    dir_in: bool,
    len: usize,
    out: Option<&[u8]>,
) -> bool {
    let mps = route.mps.max(8) as usize;
    let packets = if len == 0 {
        1
    } else {
        len.div_ceil(mps).max(1)
    };
    let xfer = if dir_in && len > 0 {
        packets * mps
    } else {
        len
    };
    if xfer > 256 {
        return false;
    }
    if let Some(bytes) = out {
        let dma = unsafe { &mut *core::ptr::addr_of_mut!(DMA) };
        dma.bytes[..bytes.len()].copy_from_slice(bytes);
    }
    cache_clean(xfer.max(8));
    let mut try_split = route.split;
    for _ in 0..4 {
        match submit(
            route, kind, ep, pid, dir_in, xfer, packets, try_split, false,
        ) {
            Submit::Done => return true,
            Submit::Nak => return false,
            Submit::Again => {
                if try_split {
                    if complete_split(route, kind, ep, pid, dir_in, xfer, packets) {
                        return true;
                    }
                }
                return false;
            }
            Submit::Err => {
                try_split = false;
            }
        }
    }
    false
}

enum Submit {
    Done,
    Nak,
    Again,
    Err,
}

fn complete_split(
    route: &Route,
    kind: u32,
    ep: u8,
    pid: u32,
    dir_in: bool,
    xfer: usize,
    packets: usize,
) -> bool {
    for _ in 0..20 {
        match submit(route, kind, ep, pid, dir_in, xfer, packets, true, true) {
            Submit::Done => return true,
            Submit::Nak => return false,
            Submit::Again => delay_us(8),
            Submit::Err => return false,
        }
    }
    false
}

fn submit(
    route: &Route,
    kind: u32,
    ep: u8,
    pid: u32,
    dir_in: bool,
    xfer: usize,
    packets: usize,
    split: bool,
    complete: bool,
) -> Submit {
    let mut split_reg = 0;
    if split {
        split_reg = HCSPLT_SPLTENA | ((route.hub as u32) << 7) | (route.port as u32) | (3 << 14);
        if complete {
            split_reg |= HCSPLT_COMPSPLT;
        }
    }
    wr(hc(0x04), split_reg);
    let tsiz = (xfer as u32 & 0x7ffff) | ((packets as u32) << 19) | (pid << 29);
    wr(hc(0x10), tsiz);
    wr(hc(0x14), dma_addr());
    wr(hc(0x08), 0x7ff);
    let frame = rd(HFNUM) & 0xffff;
    let mut ch = (route.mps as u32 & 0x7ff)
        | ((ep as u32) << 11)
        | (kind << 18)
        | (1 << 20)
        | ((route.addr as u32) << 22)
        | HCCHAR_CHENA;
    if dir_in {
        ch |= HCCHAR_EPDIR;
    }
    if route.low {
        ch |= HCCHAR_LSPDDEV;
    }
    if kind == EP_INTERRUPT && frame & 1 == 0 {
        ch |= HCCHAR_ODDFRM;
    }
    wr(hc(0x00), ch);
    let ints = wait_halt();
    if ints & (HCINT_STALL | HCINT_XACTERR | HCINT_AHBERR | HCINT_BBLERR | HCINT_DATATGLERR) != 0 {
        return Submit::Err;
    }
    if ints & HCINT_XFERCOMPL != 0 {
        return Submit::Done;
    }
    if split && !complete && ints & (1 << 5) != 0 {
        return Submit::Again;
    }
    if ints & HCINT_NYET != 0 {
        return Submit::Again;
    }
    if ints & HCINT_NAK != 0 {
        return Submit::Nak;
    }
    Submit::Err
}

fn wait_halt() -> u32 {
    let start = timer::clo();
    loop {
        let ints = rd(hc(0x08));
        if ints & HCINT_CHHLTD != 0 {
            return ints;
        }
        if timer::clo().wrapping_sub(start) > 50_000 {
            return ints;
        }
    }
}

fn host_up() -> bool {
    soft_reset();
    let mut cfg = rd(GUSBCFG);
    cfg &= !GUSBCFG_TRDTIM_MASK;
    cfg |= 9 << 10;
    cfg |= GUSBCFG_FORCEHOSTMODE;
    wr(GUSBCFG, cfg);
    let start = timer::clo();
    while rd(GINTSTS) & GINTSTS_CURMODE_HOST == 0 {
        if timer::clo().wrapping_sub(start) > 25_000 {
            break;
        }
    }
    wr(HCFG, 0);
    wr(HFIR, 0xea60);
    wr(GRXFSIZ, 0x100);
    wr(GNPTXFSIZ, (0x100 << 16) | 0x100);
    wr(HPTXFSIZ, (0x100 << 16) | 0x200);
    wr(GAHBCFG, GAHBCFG_DMA_EN | (1 << 1) | GAHBCFG_GLBL_INTR_EN);
    hprt(HPRT0_PWR, 0);
    let start = timer::clo();
    while rd(HPRT0) & HPRT0_CONNSTS == 0 {
        if timer::clo().wrapping_sub(start) > 200_000 {
            return false;
        }
    }
    hprt(HPRT0_PWR | HPRT0_RST, 0);
    delay_us(50_000);
    hprt(HPRT0_PWR, HPRT0_RST);
    let start = timer::clo();
    while rd(HPRT0) & HPRT0_ENA == 0 {
        if timer::clo().wrapping_sub(start) > 100_000 {
            return false;
        }
    }
    let mut v = rd(HPRT0);
    v &= !HPRT0_ENA;
    v |= HPRT0_ENACHG | HPRT0_CONNDET;
    wr(HPRT0, v);
    true
}

fn soft_reset() {
    wr(GRSTCTL, GRSTCTL_CSFTRST);
    let start = timer::clo();
    while rd(GRSTCTL) & GRSTCTL_CSFTRST != 0 {
        if timer::clo().wrapping_sub(start) > 10_000 {
            break;
        }
    }
    let start = timer::clo();
    while rd(GRSTCTL) & GRSTCTL_AHBIDLE == 0 {
        if timer::clo().wrapping_sub(start) > 10_000 {
            break;
        }
    }
}

fn hprt(set: u32, clear: u32) {
    let mut v = rd(HPRT0);
    v &= !(HPRT0_ENA | HPRT0_CONNDET | HPRT0_ENACHG | HPRT0_OVRCURRCHG);
    v &= !clear;
    v |= set;
    wr(HPRT0, v);
}

fn port_speed() -> u32 {
    (rd(HPRT0) >> HPRT0_SPD_SHIFT) & 3
}

fn alloc_addr() -> u8 {
    unsafe {
        let addr = NEXT_ADDR;
        NEXT_ADDR = NEXT_ADDR.wrapping_add(1).max(1);
        addr
    }
}

fn req(typ: u8, request: u8, value: u16, index: u16, len: u16) -> [u8; 8] {
    [
        typ,
        request,
        value as u8,
        (value >> 8) as u8,
        index as u8,
        (index >> 8) as u8,
        len as u8,
        (len >> 8) as u8,
    ]
}

fn hc(delta: usize) -> usize {
    0x500 + delta
}

fn dma_addr() -> u32 {
    core::ptr::addr_of!(DMA) as u32
}

fn cache_clean(len: usize) {
    cache_op(len, true);
}

fn cache_invalidate(len: usize) {
    cache_op(len, false);
}

fn cache_op(len: usize, clean: bool) {
    let start = dma_addr() as usize & !63;
    let end = (dma_addr() as usize + len.max(1) + 63) & !63;
    let mut line = start;
    while line < end {
        unsafe {
            if clean {
                core::arch::asm!("dc civac, {line}", line = in(reg) line, options(nostack));
            } else {
                core::arch::asm!("dc ivac, {line}", line = in(reg) line, options(nostack));
            }
        }
        line += 64;
    }
    unsafe {
        core::arch::asm!("dsb sy", options(nostack, preserves_flags));
    }
}

fn rd(off: usize) -> u32 {
    unsafe { read_volatile((BASE + off) as *const u32) }
}

fn wr(off: usize, value: u32) {
    unsafe { write_volatile((BASE + off) as *mut u32, value) }
}

fn delay_us(us: u32) {
    let start = timer::clo();
    while timer::clo().wrapping_sub(start) < us {
        core::hint::spin_loop();
    }
}

fn push(kbd: &mut Keyboard, ch: u8) {
    if kbd.qlen < kbd.queue.len() {
        kbd.queue[kbd.qlen] = ch;
        kbd.qlen += 1;
    }
}

fn pop(kbd: &mut Keyboard) -> u8 {
    let ch = kbd.queue[0];
    for index in 1..kbd.qlen {
        kbd.queue[index - 1] = kbd.queue[index];
    }
    kbd.qlen -= 1;
    ch
}

fn ascii(modifier: u8, code: u8) -> Option<u8> {
    let shift = modifier & 0x22 != 0;
    match code {
        0x04..=0x1d => {
            let letter = b'a' + (code - 0x04);
            Some(if shift { letter - b'a' + b'A' } else { letter })
        }
        0x1e..=0x26 => Some(b'1' + (code - 0x1e)),
        0x27 => Some(b'0'),
        0x28 => Some(b'\r'),
        0x2a => Some(0x7f),
        0x2c => Some(b' '),
        _ => None,
    }
}
