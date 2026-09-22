//! Image auto-correction for Intel Flash Descriptor images

use crate::device::{Em100, HwVersion};
use crate::error::Result;
use byteorder::{ByteOrder, LittleEndian};

/// Flash descriptor signature
const FD_SIGNATURE: u32 = 0x0FF0A55A;

/// IFD versions
#[derive(Debug, Clone, Copy, PartialEq)]
enum IfdVersion {
    V1,
    V2,
}

/// SPI frequency settings
#[derive(Debug, Clone, Copy)]
#[repr(u8)]
#[allow(dead_code, clippy::enum_variant_names)]
enum SpiFrequency {
    Freq20MHz = 0,
    Freq33MHz = 1,
    Freq48MHz = 2,
    Freq50MHz30MHz = 4,
    Freq17MHz = 6,
}

/// Find flash descriptor in image
fn find_fd(image: &[u8]) -> Option<usize> {
    (0..=image.len().checked_sub(4)?)
        .step_by(4)
        .find(|&i| LittleEndian::read_u32(&image[i..]) == FD_SIGNATURE)
}

fn find_fcba(image: &[u8]) -> Option<usize> {
    let fd_offset = find_fd(image)?;
    let flmap0 = LittleEndian::read_u32(image.get(fd_offset + 4..fd_offset + 8)?);
    let fcba_offset = ((flmap0 & 0xff) as usize) << 4;
    image.get(fcba_offset..fcba_offset + 4)?;
    Some(fcba_offset)
}

/// Get IFD version from FCBA
fn get_ifd_version(flcomp: u32) -> IfdVersion {
    let read_freq = (flcomp >> 17) & 7;

    match read_freq {
        0 => IfdVersion::V1,     // 20MHz
        4 | 6 => IfdVersion::V2, // 50MHz/30MHz or 17MHz
        _ => {
            eprintln!("Unknown descriptor version: {}", read_freq);
            IfdVersion::V2
        }
    }
}

/// Set SPI frequency in FCBA
fn set_spi_frequency(flcomp: &mut u32, freq: SpiFrequency) {
    // Clear bits 21-30
    *flcomp &= !0x7fe00000;
    // Read ID and Read Status Clock Frequency
    *flcomp |= (freq as u32) << 27;
    // Write and Erase Clock Frequency
    *flcomp |= (freq as u32) << 24;
    // Fast Read Clock Frequency
    *flcomp |= (freq as u32) << 21;
}

/// Set EM100 mode in flash descriptor
fn set_em100_mode(image: &mut [u8], fcba_offset: usize, em100: &Em100) {
    if em100.hw_version == HwVersion::Em100ProG2 {
        println!("Warning: EM100Pro-G2 can run at full speed.");
    }

    let flcomp = LittleEndian::read_u32(&image[fcba_offset..]);
    let ifd_version = get_ifd_version(flcomp);

    let (freq, freq_name) = match ifd_version {
        IfdVersion::V1 => (SpiFrequency::Freq20MHz, "20MHz"),
        IfdVersion::V2 => (SpiFrequency::Freq17MHz, "17MHz"),
    };

    println!("Limit SPI frequency to {}.", freq_name);

    let mut new_flcomp = flcomp;
    set_spi_frequency(&mut new_flcomp, freq);
    LittleEndian::write_u32(&mut image[fcba_offset..], new_flcomp);
}

/// Auto-correct image to work with EM100
///
/// Currently supports Intel Flash Descriptor (IFD) images.
///
/// Returns Ok(true) if the image was patched, Ok(false) if the image
/// type was not recognized.
pub fn autocorrect_image(em100: &Em100, image: &mut [u8]) -> Result<bool> {
    print!("Auto-detecting image type ... ");

    if let Some(fcba_offset) = find_fcba(image) {
        println!("IFD");
        set_em100_mode(image, fcba_offset, em100);
        Ok(true)
    } else {
        println!("<unknown or inconsistent>");
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use super::{FD_SIGNATURE, find_fcba, find_fd};

    #[test]
    fn descriptor_requires_complete_map_and_component() {
        let mut image = [0u8; 32];
        image[0..4].copy_from_slice(&FD_SIGNATURE.to_le_bytes());
        image[4] = 1; // FCBA at 0x10
        assert_eq!(find_fcba(&image), Some(16));
        assert_eq!(find_fcba(&image[..19]), None);
        assert_eq!(find_fcba(&image[..7]), None);
    }

    #[test]
    fn signature_at_end_is_found_without_reading_past_image() {
        assert_eq!(find_fd(&FD_SIGNATURE.to_le_bytes()), Some(0));
        let mut image = [0u8; 8];
        image[4..].copy_from_slice(&FD_SIGNATURE.to_le_bytes());
        assert_eq!(find_fd(&image), Some(4));
        assert_eq!(find_fcba(&image), None);
    }
}
