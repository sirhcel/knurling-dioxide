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
    text::{Alignment, Text},
};
use embedded_hal::blocking::delay::DelayMs;
use epd_waveshare::{
    epd2in9_v2::*,
    graphics::Display,
    prelude::*,
};
use heapless::{
    String,
    spsc::Queue,
};
use profont::*;
use nrf52840_hal::{
    Temp,
    Timer,
    gpio::{p0::Parts as P0Parts, p1::Parts as P1Parts, Level},
    self as hal,
    spim::{self, Spim},
    twim::{self, Twim},
};
use switch_hal::{OutputSwitch, IntoSwitch};


const MAX_QUICK_UPDATES: usize = 10;


fn draw_measurement<D: DrawTarget<Color = BinaryColor>>(target: &mut D, measurement: &scd30::Measurement) -> Result<(), D::Error> {
    let label_style = MonoTextStyle::new(&PROFONT_10_POINT, BinaryColor::On);
    let value_style = MonoTextStyle::new(&PROFONT_24_POINT, BinaryColor::On);
    let mut message: String<16> = String::new();

    let label_origin = Point::new(0, 0);
    let value_origin = Point::new(128, 0);

    Text::new("CO2 [ppm]", label_origin + Point::new(0, 13), label_style)
        .draw(target)?;
    write!(&mut message, "{:.2}", measurement.co2_ppm)
        .expect("failed to write to buffer");
    Text::with_alignment(&message, value_origin + Point::new(0, 40), value_style, Alignment::Right)
        .draw(target)?;

    Text::new("Temperature [°C]", label_origin + Point::new(0, 63), label_style)
        .draw(target)?;
    message.clear();
    write!(&mut message, "{:.2}", measurement.temperature_celsius)
        .expect("failed to write to buffer");
    Text::with_alignment(&message, value_origin + Point::new(0, 90), value_style, Alignment::Right)
        .draw(target)?;

    Text::new("Humidity [%]", label_origin + Point::new(0, 113), label_style)
        .draw(target)?;
    message.clear();
    write!(&mut message, "{:.2}", measurement.humidity_percent)
        .expect("failed to write to buffer");
    Text::with_alignment(&message, value_origin + Point::new(0, 140), value_style, Alignment::Right)
        .draw(target)?;

    Ok(())
}

fn draw_measurements<D: DrawTarget<Color = BinaryColor>, const N: usize>(
    target: &mut D,
    measurements: &Queue<scd30::Measurement, N>) -> Result<(), D::Error>
{
    let bar_style = PrimitiveStyleBuilder::new()
        .fill_color(BinaryColor::On)
        .build();

    let origin = Point::new(5, 200);

    let bar_width = 256u32 / measurements.capacity() as u32;
    let norm_height = 50;
    let max_co2_ppm = 2_500;
    let ppm_height_scaler = norm_height as f32 / max_co2_ppm as f32;

    for (index, measurement) in measurements.iter().enumerate() {
        // TODO: Clean up type conversions below.
        let offset = Point::new(index as i32 * bar_width as i32, 0);
        let height = core::cmp::min(norm_height, (measurement.co2_ppm * ppm_height_scaler) as u32);

        let pos = Point::new(0, (norm_height - height) as i32);
        let size = Size::new(bar_width, height);
        let rect = Rectangle::new(origin + offset + pos, size);

        defmt::debug!("{}: offset: {}, height: {}, rect: {}", index,
            defmt::Debug2Format(&offset), height,
            defmt::Debug2Format(&rect));

        rect.into_styled(bar_style).draw(target)?;
    }

    Ok(())
}


fn draw_stats<D: DrawTarget<Color = BinaryColor>>(
    target: &mut D,
    updates: usize,
    queue_len: usize) -> Result<(), D::Error>
{
    let style = MonoTextStyle::new(&PROFONT_7_POINT, BinaryColor::On);
    let mut message: String<32> = String::new();

    write!(&mut message, "updates: {}, queue: {}", updates, queue_len)
        .expect("failed to write to buffer");
    Text::new(&message, Point::new(5, 294), style).draw(target)?;

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
    let mut temp = Temp::new(board.TEMP);
    let mut timer = Timer::new(board.TIMER0);

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

    let mut epd = Epd2in9::new(&mut spi, cs, busy, dc, rst, &mut epd_timer).unwrap();
    let mut display = Display2in9::default();
    display.set_rotation(DisplayRotation::Rotate0);


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


    defmt::info!("Entering loop ...");

    let mut updates = 0usize;
    let mut measurements: Queue<scd30::Measurement, 256> = Queue::new();

    loop {
        led_1.on().unwrap();

        if sensor.is_measurement_ready().unwrap() {
            let measurement = sensor.get_measurement().unwrap();
            defmt::info!("measurement: {:?}", measurement);

            if measurements.is_full() {
                measurements.dequeue().unwrap();
            }
            measurements.enqueue(measurement).expect("enqueueing measurement failed");
            defmt::info!("queue len: {}", measurements.len());

            epd.wake_up(&mut spi, &mut epd_timer).unwrap();

            defmt::info!("updates: {}", updates);
            if updates % MAX_QUICK_UPDATES == 0 {
                display.clear_buffer(DEFAULT_BACKGROUND_COLOR);
                draw_measurement(&mut display, &measurement).unwrap();
                draw_measurements(&mut display, &measurements).unwrap();
                draw_stats(&mut display, updates, measurements.len()).unwrap();
                epd.set_lut(&mut spi, Some(RefreshLut::Full)).unwrap();
                epd.update_frame(&mut spi, &display.buffer(), &mut epd_timer).unwrap();
                epd.display_frame(&mut spi, &mut epd_timer).expect("display new measurement frame");
            } else {
                epd.set_lut(&mut spi, Some(RefreshLut::Quick)).unwrap();
                epd.update_old_frame(&mut spi, &display.buffer(), &mut epd_timer).unwrap();

                display.clear_buffer(DEFAULT_BACKGROUND_COLOR);
                draw_measurement(&mut display, &measurement).unwrap();
                draw_measurements(&mut display, &measurements).unwrap();
                draw_stats(&mut display, updates, measurements.len()).unwrap();
                epd.update_new_frame(&mut spi, &display.buffer(), &mut epd_timer).unwrap();
                epd.display_new_frame(&mut spi, &mut epd_timer).unwrap();
            }

            epd.sleep(&mut spi, &mut epd_timer).unwrap();
            updates += 1;
        }

        timer.delay_ms(5000u32);
        led_1.off().unwrap();
        timer.delay_ms(5000u32);
    }
}
