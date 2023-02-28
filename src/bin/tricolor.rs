#![no_main]
#![no_std]


use core::fmt::Write;
use dioxide as _; // global logger + panicking-behavior + memory layout
use dioxide::scd30;
use embedded_graphics::{
    geometry::{Point, Size},
    mono_font::MonoTextStyle,
    pixelcolor::BinaryColor,
    prelude::*,
    primitives::{PrimitiveStyleBuilder, Rectangle},
    text::Text,
};
use embedded_hal::blocking::delay::DelayMs;
use epd_waveshare::{
    epd2in9bc::*,
    graphics::Display,
    prelude::*,
};
use heapless::String;
use profont::{PROFONT_18_POINT, PROFONT_24_POINT};
use nrf52840_hal::{
    Temp,
    Timer,
    gpio::{p0::Parts as P0Parts, p1::Parts as P1Parts, Level},
    self as hal,
    spim::{self, Spim},
    twim::{self, Twim},
};
use switch_hal::{OutputSwitch, InputSwitch, IntoSwitch};


fn clear_measurement<D: DrawTarget<Color = BinaryColor>>(target: &mut D) -> Result<(), D::Error> {
    let style = PrimitiveStyleBuilder::new()
        .fill_color(BinaryColor::Off)
        .build();

    Rectangle::new(Point::new(20, 50), Size::new(360, 70))
        .into_styled(style)
        .draw(target)?;

    Ok(())
}

fn draw_measurement<D: DrawTarget<Color = BinaryColor>>(target: &mut D, measurement: &scd30::Measurement) -> Result<(), D::Error> {
    let style = MonoTextStyle::new(&PROFONT_18_POINT, BinaryColor::On);
    let mut message: String<16> = String::new();

    clear_measurement(target)?;

    write!(&mut message, "CO2: {:.2} ppm", measurement.co2_ppm)
        .expect("failed to write to buffer");
    Text::new(&message, Point::new(20, 70), style)
        .draw(target)?;

    message.clear();
    write!(&mut message, "T:   {:.2} °C", measurement.temperature_celsius)
        .expect("failed to write to buffer");
    Text::new(&message, Point::new(20, 90), style)
        .draw(target)?;

    message.clear();
    write!(&mut message, "RH:  {:.2} %", measurement.humidity_percent)
        .expect("failed to write to buffer");
    Text::new(&message, Point::new(20, 110), style)
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

    let scl = pins_0.p0_30.into_floating_input().degrade();
    let sda = pins_0.p0_31.into_floating_input().degrade();
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
    let spi_pins = spim::Pins{ sck: Some(clk), miso: None, mosi: Some(din) };
    let mut spi = Spim::new(board.SPIM3, spi_pins, spim::Frequency::K500, spim::MODE_0, 0);
    let mut epd_timer = Timer::new(board.TIMER1);

    let mut epd = Epd2in9bc::new(&mut spi, cs, busy, dc, rst, &mut epd_timer).unwrap();
    let mut black_display = Display2in9bc::default();
    let mut chromatic_display = Display2in9bc::default();
    black_display.set_rotation(DisplayRotation::Rotate270);
    chromatic_display.set_rotation(DisplayRotation::Rotate270);


    defmt::info!("Turning LED on ...");
    led_1.on().unwrap();
    timer.delay_ms(1000u32);

    defmt::info!("Measuring temperature ...");
    let temperature = temp.measure();
    defmt::info!("temperature: {} °C", temperature.to_num::<f32>());

    let sensor_fw_version = sensor.get_firmware_version().unwrap();
    defmt::info!("SCD30 firmware version: {:?}", sensor_fw_version);
    let pressure_mbar = 1020_u16;
    sensor.start_continuous_measurement(pressure_mbar).unwrap();


    let header_style = MonoTextStyle::new(&PROFONT_24_POINT, BinaryColor::On);
    Text::new("Hello Knurling!", Point::new(20, 30), header_style)
        .draw(&mut chromatic_display)
        .unwrap();
    epd.update_color_frame(&mut spi, black_display.buffer(), chromatic_display.buffer()).unwrap();
    epd.display_frame(&mut spi, &mut epd_timer).expect("display frame new graphics");


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

            epd.wake_up(&mut spi, &mut epd_timer).unwrap();

            draw_measurement(&mut black_display, &measurement).unwrap();
            epd.update_color_frame(&mut spi, black_display.buffer(), chromatic_display.buffer()).unwrap();
            epd.display_frame(&mut spi, &mut epd_timer).expect("display new measurement frame");

            epd.sleep(&mut spi, &mut epd_timer).unwrap();
        }

        timer.delay_ms(5000u32);
        led_1.off().unwrap();
        led_2.off().unwrap();
        timer.delay_ms(5000u32);
    }
}
