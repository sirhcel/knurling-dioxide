#![no_main]
#![no_std]


use dioxide as _; // global logger + panicking-behavior + memory layout
use embedded_hal::blocking::delay::DelayMs;
use nrf52840_hal::{
    self as hal,
    gpio::{p0::Parts as P0Parts, Level},
    Temp,
    Timer,
};
use switch_hal::{OutputSwitch, InputSwitch, IntoSwitch};


#[cortex_m_rt::entry]
fn main() -> ! {
    defmt::info!("Hello, world!");

    let board = hal::pac::Peripherals::take().unwrap();
    let pins = P0Parts::new(board.P0);
    let mut led_1 = pins.p0_13.into_push_pull_output(Level::High)
        .into_active_low_switch();
    let mut led_2 = pins.p0_14.into_push_pull_output(Level::High)
        .into_active_low_switch();
    let mut temp = Temp::new(board.TEMP);
    let mut timer = Timer::new(board.TIMER0);

    let button_1 = pins.p0_11.into_pullup_input().into_active_low_switch();

    defmt::info!("Turning LED on ...");
    led_1.on().unwrap();
    timer.delay_ms(1000u32);

    defmt::info!("Measuring temperature ...");
    let temperature = temp.measure();
    defmt::info!("temperature: {:f32} °C", temperature.to_num());

    defmt::info!("Entering LED loop ...");

    loop {
        led_1.on().unwrap();
        if button_1.is_active().unwrap() {
            defmt::info!("Button 1 pressed");
            led_2.on().unwrap();
        }
        timer.delay_ms(500u32);
        led_1.off().unwrap();
        led_2.off().unwrap();
        timer.delay_ms(500u32);
    }
}
