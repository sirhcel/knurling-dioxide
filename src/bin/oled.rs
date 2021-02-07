#![no_main]
#![no_std]


use core::fmt::Write;
use dioxide as _; // global logger + panicking-behavior + memory layout
use dioxide::scd30;
use embedded_graphics::{
    egtext,
    fonts::{Font6x8, Text},
    pixelcolor::BinaryColor,
    prelude::*,
    primitives::Rectangle,
    style::{PrimitiveStyleBuilder, TextStyle},
    text_style,
};
use embedded_hal::blocking::delay::DelayMs;
use heapless::{
    String,
    consts::*};
use nrf52840_hal::{
    Temp,
    Timer,
    gpio::{p0::Parts as P0Parts, Level},
    self as hal,
    twim::{self, Twim},
};
use sh1106::{
    Builder,
    prelude::*,
};
use shared_bus;
use switch_hal::{OutputSwitch, InputSwitch, IntoSwitch};


#[cortex_m_rt::entry]
fn main() -> ! {
    defmt::info!("Hello, world!");

    let board = hal::pac::Peripherals::take().unwrap();
    let pins_0 = P0Parts::new(board.P0);
    let mut led_1 = pins_0.p0_13.into_push_pull_output(Level::High)
        .into_active_low_switch();
    let mut led_2 = pins_0.p0_14.into_push_pull_output(Level::High)
        .into_active_low_switch();
    let mut temp = Temp::new(board.TEMP);
    let mut timer = Timer::new(board.TIMER0);

    let button_1 = pins_0.p0_11.into_pullup_input().into_active_low_switch();

    let scl = pins_0.p0_30.degrade();
    let sda = pins_0.p0_31.degrade();
    let i2c_pins = twim::Pins{ scl, sda };
    let i2c = Twim::new(board.TWIM0, i2c_pins, twim::Frequency::K100);
    let shared_i2c = shared_bus::BusManagerSimple::new(i2c);
    let mut sensor = scd30::Scd30::new(shared_i2c.acquire_i2c());
    let mut oled: GraphicsMode<_> = Builder::new().connect_i2c(shared_i2c.acquire_i2c()).into();


    defmt::info!("Turning LED on ...");
    led_1.on().unwrap();
    timer.delay_ms(1000u32);

    defmt::info!("Measuring temperature ...");
    let temperature = temp.measure();
    defmt::info!("temperature: {:f32} °C", temperature.to_num());

    let sensor_fw_version = sensor.get_firmware_version().unwrap();
    defmt::info!("SCD30 firmware version: {:?}", sensor_fw_version);
    let pressure_mbar = 1020_u16;
    sensor.start_continuous_measurement(pressure_mbar).unwrap();


    oled.init().unwrap();
    oled.flush().unwrap();
    Text::new("Hello OLED!", Point::new(0, 16))
        .into_styled(TextStyle::new(Font6x8, BinaryColor::On))
        .draw(&mut oled)
        .unwrap();
    oled.flush().unwrap();

    let clear_style = PrimitiveStyleBuilder::new()
        .fill_color(BinaryColor::Off)
        .build();
    let clear_rect = Rectangle::new(Point::new(0, 32), Point::new(128, 40))
        .into_styled(clear_style);


    defmt::info!("Entering loop ...");

    loop {
        led_1.on().unwrap();
        if button_1.is_active().unwrap() {
            defmt::info!("Button 1 pressed");
            led_2.on().unwrap();
        }

        if sensor.is_measurement_ready().unwrap() {
            let measurement = sensor.get_measurement().unwrap();
            defmt::info!("measurement: {:?}", measurement);

            // Clear and draw a "line" of text.
            clear_rect.draw(&mut oled).unwrap();
            let mut message: String<U16> = String::new();
            write!(&mut message, "CO2: {:.2} ppm", measurement.co2_ppm)
                .expect("failed to write to buffer");
            egtext!(
                text = &message,
                top_left = (0, 32),
                style = text_style!(font = Font6x8, text_color = BinaryColor::On)
            ).draw(&mut oled).unwrap();
            oled.flush().unwrap();
        }

        timer.delay_ms(500u32);
        led_1.off().unwrap();
        led_2.off().unwrap();
        timer.delay_ms(500u32);
    }
}
