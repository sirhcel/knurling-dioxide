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
    primitives::{Line, PrimitiveStyle, Rectangle},
    text::{Alignment, Text},
};
use embedded_hal::blocking::delay::DelayMs;
use embedded_vintage_fonts::FONT_6X8;
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


const MAX_CO2_PPM: f32 = 2_500f32;
const MAX_QUICK_UPDATES: usize = 10;
const TICKS_MARGIN: i32 = 1;
const TICKS_SIZE: i32 = 2;


fn draw_co2_history<D: DrawTarget<Color = BinaryColor>, const N: usize>(
    target: &mut D,
    destination: &Rectangle,
    measurements: &Queue<scd30::Measurement, N>) -> Result<(), D::Error>
{
    let bar_style = PrimitiveStyle::with_fill(BinaryColor::On);
    let tick_style = PrimitiveStyle::with_stroke(BinaryColor::On, 1);

    let plot_rect = history_plot_rect(destination);
    let origin = plot_rect.top_left;

    let bar_width = plot_rect.size.width / measurements.capacity() as u32;
    let norm_height = plot_rect.size.height;
    let ppm_height_scaler = norm_height as f32 / MAX_CO2_PPM;

    // Draw axis tick marks.
    for ppm in (0..=2500).step_by(500) {
        let y = norm_height as i32 - (ppm as f32 * ppm_height_scaler) as i32;

        let margin = Point::new(1, 0);
        let delta = Point::new(1, 0);
        let left_start = Point::new(-1, y) - margin;
        let right_start = Point::new(1, y) + plot_rect.size.x_axis() + margin;

        Line::new(left_start, left_start - delta)
            .translate(origin)
            .into_styled(tick_style)
            .draw(target)?;
        Line::new(right_start, right_start + delta)
            .translate(origin)
            .into_styled(tick_style)
            .draw(target)?;
    }

    // Draw actual data.
    for (index, measurement) in measurements.iter().enumerate() {
        // TODO: Clean up type conversions below.
        let offset = Point::new(index as i32 * bar_width as i32, 0);
        let height = core::cmp::min(norm_height, (measurement.co2_ppm * ppm_height_scaler) as u32);

        let pos = Point::new(0, (norm_height - height + 1) as i32);
        let size = Size::new(bar_width, height);
        let rect = Rectangle::new(origin + offset + pos, size);

        defmt::debug!("{}: offset: {}, height: {}, rect: {}", index,
            defmt::Debug2Format(&offset), height,
            defmt::Debug2Format(&rect));

        rect.into_styled(bar_style).draw(target)?;
    }

    Ok(())
}


fn draw_measurement<D: DrawTarget<Color = BinaryColor>>(target: &mut D, measurement: &scd30::Measurement) -> Result<(), D::Error> {
    let label_style = MonoTextStyle::new(&FONT_6X8, BinaryColor::On);
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


fn history_plot_rect(destination: &Rectangle) -> Rectangle {
    let offset = TICKS_MARGIN + TICKS_SIZE;
    destination.offset(-offset)
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

    let mut epd = Epd2in9::new(&mut spi, cs, busy, dc, rst, &mut epd_timer).unwrap();
    let mut display = Display2in9::default();
    display.set_rotation(DisplayRotation::Rotate0);


    defmt::info!("Turning LED on ...");
    led_1.on().unwrap();
    timer.delay_ms(1000u32);

    defmt::info!("Measuring temperature ...");
    let temperature = temp.measure();
    defmt::info!("temperature: {=f32} °C", temperature.to_num::<f32>());

    let sensor_fw_version = sensor.get_firmware_version().unwrap();
    defmt::info!("SCD30 firmware version: {:?}", sensor_fw_version);
    let pressure_mbar = 1020_u16;
    sensor.start_continuous_measurement(pressure_mbar).unwrap();


    defmt::info!("Entering loop ...");

    let mut updates = 0usize;
    let mut measurements: Queue<scd30::Measurement, 108> = Queue::new();

    let measurements_destination = Rectangle::new(Point::new(7, 175), Size::new(114, 56));

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
                draw_co2_history(&mut display, &measurements_destination, &measurements).unwrap();
                draw_stats(&mut display, updates, measurements.len()).unwrap();
                epd.set_lut(&mut spi, Some(RefreshLut::Full)).unwrap();
                epd.update_frame(&mut spi, &display.buffer(), &mut epd_timer).unwrap();
                epd.display_frame(&mut spi, &mut epd_timer).expect("display new measurement frame");
            } else {
                epd.set_lut(&mut spi, Some(RefreshLut::Quick)).unwrap();
                epd.update_old_frame(&mut spi, &display.buffer(), &mut epd_timer).unwrap();

                display.clear_buffer(DEFAULT_BACKGROUND_COLOR);
                draw_measurement(&mut display, &measurement).unwrap();
                draw_co2_history(&mut display, &measurements_destination, &measurements).unwrap();
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
