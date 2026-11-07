use core::panic::PanicInfo;

use crate::uart;

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    uart::puts("\r\nPANIC\r\n");
    if let Some(loc) = info.location() {
        uart::write_fmt(format_args!(
            "{}:{}:{}\r\n",
            loc.file(),
            loc.line(),
            loc.column()
        ));
    }
    uart::write_fmt(format_args!("{}\r\n", info.message()));
    loop {
        core::hint::spin_loop();
    }
}
