// TODO: Why does declaring no_std here cause a compiler warning? Is this
// something which gets declared per crate?
//
// #![no_std]


use crc_all::Crc;
use embedded_hal::blocking::i2c::{Write, WriteRead};
use defmt::Format;




// TODO: How to be agnostic of any formatting stuff while being able to output
// this struct via defmt?
#[derive(Format)]
pub struct FirmwareVersion {
    major: u8,
    minor: u8,
}


pub struct Scd30<I2C: Write + WriteRead> {
    i2c: I2C,
}




pub const I2C_ADDRESS: u8 = 0x61;




impl<I2C, E> Scd30<I2C> where I2C: Write<Error = E> + WriteRead<Error = E> {
    pub fn new(i2c: I2C) -> Self {
        Scd30{ i2c }
    }


    pub fn get_firmware_version(&mut self) -> Result<FirmwareVersion, E> {
        let command: [u8; 2] = [0xd1, 0x00];
        let mut response = [0u8; 3];

        // FIXME: Cross check whether write_read pauses at least 3 ms between
        // the inital address write and the repeated start condition.
        self.i2c.write_read(I2C_ADDRESS, &command, &mut response)?;

        let major = response[0];
        let minor = response[1];
        let response_crc = response[2];

        let mut crc = self.new_sdc30_crc();
        crc.update(&response[0..2]);
        let our_crc = crc.finish();

        defmt::trace!("response: {:[u8]}", response);
        defmt::trace!("our_crc: {:u8}", our_crc);

        if response_crc == our_crc {
            Ok(FirmwareVersion{ major, minor })
        } else {
            // FIXME: How to return a custom SCD30 error in case of a failed
            // CRC check?
            Ok(FirmwareVersion{ major: 0, minor: 0 })
        }
    }


    fn new_sdc30_crc(&self) -> Crc<u8> {
        // See 'Interface Description Sensirion SCD30 Sensor Module', section
        // 1.1.3 'I2C Checksum calculation'.
        Crc::<u8>::new(0x31, 8, 0xff, 0x00, false)
    }
}
