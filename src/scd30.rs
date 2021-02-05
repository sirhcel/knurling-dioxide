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

        let mut crc = self.new_sdc30_crc();
        crc.update(&response[0..2]);
        let our_crc = crc.finish();
        defmt::trace!("response: {:[u8]}, our_crc: {:u8}", response, our_crc);

        if response_crc == our_crc {
            Ok(FirmwareVersion{ major, minor })
        } else {
            Err(Error::<E>::CrcError)
        }
    }


    fn new_sdc30_crc(&self) -> Crc<u8> {
        // See 'Interface Description Sensirion SCD30 Sensor Module', section
        // 1.1.3 'I2C Checksum calculation' for CRC parameter definition.
        Crc::<u8>::new(0x31, 8, 0xff, 0x00, false)
    }
}
