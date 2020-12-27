#![no_main]
#![no_std]


use dioxide as _; // global logger + panicking-behavior + memory layout
use embedded_hal::blocking::delay::DelayMs;
use nrf52840_hal::{
    self as hal,
    gpio::{p0::Parts as P0Parts, Level},
    Timer,
};
use switch_hal::{OutputSwitch, IntoSwitch};


#[cortex_m_rt::entry]
fn main() -> ! {
    defmt::info!("Hello, world!");

    let board = hal::pac::Peripherals::take().unwrap();
    let pins = P0Parts::new(board.P0);
    let mut led_1 = pins.p0_13.into_push_pull_output(Level::High)
        .into_active_low_switch();
    let mut timer = Timer::new(board.TIMER0);

    defmt::info!("Turning LED on ...");

    led_1.on().unwrap();
    timer.delay_ms(1000u32);

    defmt::info!("Goodbye!");

    dioxide::exit()
}
