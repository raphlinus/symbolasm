#![no_std]
#![no_main]

use core::arch::asm;
use core::panic::PanicInfo;

#[inline(always)]
fn bkpt_0() -> ! {
    unsafe {
        asm!(
            "bkpt #0",
            options(noreturn)
        );
    }
}

#[inline(always)]
fn bkpt_1(val: u32) {
    unsafe {
        asm!(
            "bkpt #1",
            in("r0") val,
            options(nomem, preserves_flags, nostack)
        );
    }
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    bkpt_0()
}

#[unsafe(no_mangle)]
fn _start() -> ! {
    bkpt_1(42);
    bkpt_0()
}
