// firmware/src/panic.rs
use core::panic::PanicInfo;
use defmt::error;

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    error!("PANIC: {}", defmt::Display2Format(info));
    cortex_m::asm::udf();
}
