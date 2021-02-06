// TODO: Why does declaring no_std here cause a compiler warning? Is this
// something which gets declared per crate?
//
// #![no_std]


use crc_all::Crc;
use embedded_hal::blocking::i2c::{Read, Write};
use defmt::Format;




// A custom error type for reporting errors from both, the driver itself and
// the underlying I2C implementation.
//
// TODO: I see that Debug should be derived for printing backtraces. What about
// Copy and Clone? Which traits should be implemented by errors? And what about
// Eq and PartialEq? Are they meant for collections? Or are they required for
// matching?
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum Error<E> {
    CrcError,
    I2cError(E),
}


// TODO: How to be agnostic of any formatting stuff while being able to output
// this struct via defmt?
#[derive(Format)]
pub struct FirmwareVersion {
    major: u8,
    minor: u8,
}


#[derive(Format)]
pub struct Measurement {
    co2_ppm: f32,
    temperature_celsius: f32,
    humidity_percent: f32,
}


pub struct Scd30<I2C: Read + Write> {
    i2c: I2C,
}




pub const I2C_ADDRESS: u8 = 0x61;




// Allow automatic conversion from the I2C implementation's error type to the
// driver's error type (for the question mark operator).
impl<E> From<E> for Error<E> {
    fn from(err: E) -> Error<E> {
        Error::I2cError(err)
    }
}


impl<I2C, E> Scd30<I2C> where I2C: Read<Error = E> + Write<Error = E> {
    pub fn new(i2c: I2C) -> Self {
        Scd30{ i2c }
    }


    pub fn get_firmware_version(&mut self) -> Result<FirmwareVersion, Error<E>> {
        let command: [u8; 2] = [0xd1, 0x00];
        let mut response = [0u8; 3];

        self.i2c.write(I2C_ADDRESS, &command)?;
        self.i2c.read(I2C_ADDRESS, &mut response)?;

        let major = response[0];
        let minor = response[1];
        let response_crc = response[2];
        let our_crc = self.sdc30_crc(&response[0..2]);
        defmt::trace!("response: {:[u8]}, our_crc: {:u8}", response, our_crc);

        if response_crc == our_crc {
            Ok(FirmwareVersion{ major, minor })
        } else {
            Err(Error::<E>::CrcError)
        }
    }


    pub fn get_measurement(&mut self) -> Result<Measurement, Error<E>> {
        let command: [u8; 2] = [0x03, 0x00];
        let mut response = [0u8; 18];

        self.i2c.write(I2C_ADDRESS, &command)?;
        // FIXME: The 'Interface Description Sensirion SCD30 Sensor Module'
        // points out that there should be a pause of at least 3 ms between the
        // stop of the command and the start for reading the response.
        // Shouldn't there be an explicit delay here?
        self.i2c.read(I2C_ADDRESS, &mut response)?;
        defmt::trace!("response: {:[u8]}", response);

        // FIXME: Process response data and provide a meaningful result.
        Ok(Measurement{ co2_ppm: -1.0, temperature_celsius: -1.0, humidity_percent: -1.0 })
    }


    pub fn is_measurement_ready(&mut self) -> Result<bool, Error<E>> {
        let command: [u8; 2] = [0x02, 0x02];
        let mut response = [0u8; 3];

        self.i2c.write(I2C_ADDRESS, &command)?;
        self.i2c.read(I2C_ADDRESS, &mut response)?;

        // TODO: It seems there is no such thing as a slice with compile-time
        // constant length to please u16::from_be_bytes. Is there any way of
        // getting a slice into a suitable form for from_be_bytes?
        let mut ready_be = [0u8; 2];
        ready_be.copy_from_slice(&response[0..2]);
        let response_crc = response[2];
        let our_crc = self.sdc30_crc(&ready_be);

        if response_crc == our_crc {
            let ready = u16::from_be_bytes(ready_be);
            Ok(ready == 1u16)
        } else {
            Err(Error::<E>::CrcError)
        }
    }


    fn new_sdc30_crc(&self) -> Crc<u8> {
        // See 'Interface Description Sensirion SCD30 Sensor Module', section
        // 1.1.3 'I2C Checksum calculation' for CRC parameter definition.
        Crc::<u8>::new(0x31, 8, 0xff, 0x00, false)
    }


    fn sdc30_crc(&self, data: &[u8]) -> u8 {
        let mut crc = self.new_sdc30_crc();
        crc.update(data);
        crc.finish()
    }


    pub fn start_continuous_measurement(&mut self, pressure: u16) -> Result<(), Error<E>> {
        let mut command: [u8; 5] = [0x00, 0x10, 0x00, 0x00, 0x00];

        let pressure_be = pressure.to_be_bytes();
        command[2..4].copy_from_slice(&pressure_be);
        command[4] = self.sdc30_crc(&pressure_be);
        defmt::trace!("command: {:[u8]}", command);

        self.i2c.write(I2C_ADDRESS, &command)?;
        Ok(())
    }
}
