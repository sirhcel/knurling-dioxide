#![no_main]
#![no_std]


use core::fmt::Write;
use dioxide as _; // global logger + panicking-behavior + memory layout
use dioxide::scd30;
use embedded_graphics::{
    fonts::{Font12x16, Font24x32, Text},
    geometry::Point,
    pixelcolor::BinaryColor,
    prelude::*,
    primitives::Rectangle,
    style::{
        PrimitiveStyleBuilder,
        TextStyle,
    },
};
use embedded_hal::blocking::delay::DelayMs;
use epd_waveshare::{
    epd4in2::*,
    graphics::Display,
    prelude::*,
};
use heapless::String;
use nrf52840_hal::{
    Temp,
    Timer,
    gpio::{p0::Parts as P0Parts, p1::Parts as P1Parts, Level},
    self as hal,
    spim::{self, Spim},
    twim::{self, Twim},
};
use switch_hal::{OutputSwitch, InputSwitch, IntoSwitch};


const MAX_QUICK_UPDATES: usize = 10;


fn build_clear_rect() -> impl Iterator<Item = Pixel<BinaryColor>> {
    let style = PrimitiveStyleBuilder::new()
        .fill_color(BinaryColor::Off)
        .build();

    Rectangle::new(Point::new(20, 100), Point::new(380, 159))
        .into_styled(style)
        .into_iter()
}


fn draw_measurement<D: DrawTarget<BinaryColor>>(target: &mut D, measurement: &scd30::Measurement) -> Result<(), D::Error> {
    let style = TextStyle::new(Font12x16, BinaryColor::On);
    let mut message: String<16> = String::new();

    build_clear_rect().draw(target)?;

    write!(&mut message, "CO2: {:.2} ppm", measurement.co2_ppm)
        .expect("failed to write to buffer");
    Text::new(&message, Point::new(20, 100))
        .into_styled(style)
        .draw(target)?;

    message.clear();
    write!(&mut message, "T:   {:.2} °C", measurement.temperature_celsius)
        .expect("failed to write to buffer");
    Text::new(&message, Point::new(20, 120))
        .into_styled(style)
        .draw(target)?;

    message.clear();
    write!(&mut message, "RH:  {:.2} %", measurement.humidity_percent)
        .expect("failed to write to buffer");
    Text::new(&message, Point::new(20, 140))
        .into_styled(style)
        .draw(target)?;

    Ok(())
}


#[cortex_m_rt::entry]
fn main() -> ! {
    defmt::info!("Hello, world!");

    let board = hal::pac::Peripherals::take().unwrap();
    let pins_0 = P0Parts::new(board.P0);
    let pins_1 = P1Parts::new(board.P1);
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
    let mut sensor = scd30::Scd30::new(i2c);

    // TODO: Why do we need to degrade two of the pins?
    let din = pins_1.p1_01.into_push_pull_output(Level::Low).degrade();
    let clk = pins_1.p1_02.into_push_pull_output(Level::Low).degrade();
    let cs = pins_1.p1_03.into_push_pull_output(Level::Low);
    let dc = pins_1.p1_04.into_push_pull_output(Level::Low);
    let rst = pins_1.p1_05.into_push_pull_output(Level::Low);
    let busy = pins_1.p1_06.into_floating_input();
    let spi_pins = spim::Pins{ sck: clk, miso: None, mosi: Some(din) };
    let mut spi = Spim::new(board.SPIM3, spi_pins, spim::Frequency::K500, spim::MODE_0, 0);
    let mut epd_timer = Timer::new(board.TIMER1);
    let mut epd = EPD4in2::new(&mut spi, cs, busy, dc, rst, &mut epd_timer).unwrap();


    defmt::info!("Turning LED on ...");
    led_1.on().unwrap();
    timer.delay_ms(1000u32);

    defmt::info!("Measuring temperature ...");
    let temperature = temp.measure();
    defmt::info!("temperature: {=f32} °C", temperature.to_num());

    let sensor_fw_version = sensor.get_firmware_version().unwrap();
    defmt::info!("SCD30 firmware version: {:?}", sensor_fw_version);
    let pressure_mbar = 1020_u16;
    sensor.start_continuous_measurement(pressure_mbar).unwrap();


    let mut display = Display4in2::default();
    Text::new("Hello Knurling!", Point::new(20, 30))
        .into_styled(TextStyle::new(Font24x32, BinaryColor::On))
        .draw(&mut display)
        .unwrap();
    epd.update_frame(&mut spi, &display.buffer()).unwrap();
    epd.display_frame(&mut spi).expect("display frame new graphics");


    defmt::info!("Entering loop ...");

    let mut updates = 0usize;

    loop {
        led_1.on().unwrap();
        if button_1.is_active().unwrap() {
            defmt::info!("Button 1 pressed");
            led_2.on().unwrap();
        }

        if sensor.is_measurement_ready().unwrap() {
            let measurement = sensor.get_measurement().unwrap();
            defmt::info!("measurement: {:?}", measurement);

            defmt::info!("updates: {}", updates);
            let lut = if updates % MAX_QUICK_UPDATES == 0 { RefreshLUT::FULL } else { RefreshLUT::QUICK };
            epd.set_lut(&mut spi, Some(lut)).unwrap();
            updates += 1;

            draw_measurement(&mut display, &measurement).unwrap();
            epd.update_frame(&mut spi, &display.buffer()).unwrap();
            epd.display_frame(&mut spi).expect("display new measurement frame");
        }

        timer.delay_ms(5000u32);
        led_1.off().unwrap();
        led_2.off().unwrap();
        timer.delay_ms(5000u32);
    }
}
