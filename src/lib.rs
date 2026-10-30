use std::path::{Path, PathBuf};

use thiserror::Error;

pub const FLASH_BASE: u32 = 0x2000_0000;
pub const RAM_BASE: u32 = 0x3000_0000;
pub const RAM_SIZE: u32 = 0x0200_0000;
pub const E830_TFS_SOURCE_BASE: u64 = 0x1c00_0000;
const RESET_VECTOR_OFFSET: usize = 0x15c;
const RESET_VECTOR_INSTRUCTION: [u8; 4] = [0x54, 0xf1, 0x9f, 0xe5];
const NSF_HEADER_MAGIC: &[u8; 3] = b"NF4";
const NSF_HEADER_SIZE: usize = 20;

#[derive(Debug, Error)]
pub enum FirmwareError {
    #[error("could not read firmware file {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("CLA image is too short to contain the E830 reset vector")]
    ClaTooShort,
    #[error("CLA image does not start with the E830 ARM reset-vector instruction")]
    UnsupportedCla,
    #[error(
        "CLA reset handler 0x{target:08x} is outside the mapped image 0x{start:08x}..0x{end:08x}"
    )]
    ResetHandlerOutsideImage { target: u32, start: u32, end: u64 },
    #[error("EEPROM image is empty")]
    EmptyEeprom,
    #[error("TFS image is empty")]
    EmptyTfs,
    #[error("TFS installer package is empty")]
    EmptyTfsPackage,
}

#[derive(Debug)]
pub struct FirmwareSet {
    cla: Vec<u8>,
    eeprom: Vec<u8>,
    tfs_image: Option<Vec<u8>>,
    tfs_package: Option<Vec<u8>>,
    onenand_image: Option<Vec<u8>>,
    reset_handler: u32,
}

impl FirmwareSet {
    pub fn load(
        cla_path: &Path,
        eeprom_path: &Path,
        tfs_image_path: Option<&Path>,
        tfs_package_path: Option<&Path>,
    ) -> Result<Self, FirmwareError> {
        let cla = read_file(cla_path)?;
        let eeprom = read_file(eeprom_path)?;
        let tfs_image = tfs_image_path.map(read_file).transpose()?;
        let tfs_package = tfs_package_path.map(read_file).transpose()?;
        Self::from_bytes(cla, eeprom, tfs_image, tfs_package)
    }

    pub fn from_bytes(
        cla: Vec<u8>,
        eeprom: Vec<u8>,
        tfs_image: Option<Vec<u8>>,
        tfs_package: Option<Vec<u8>>,
    ) -> Result<Self, FirmwareError> {
        let reset_handler = validate_cla(&cla)?;
        if eeprom_payload_offset(&eeprom) >= eeprom.len() {
            return Err(FirmwareError::EmptyEeprom);
        }
        if tfs_image.as_ref().is_some_and(Vec::is_empty) {
            return Err(FirmwareError::EmptyTfs);
        }
        if tfs_package.as_ref().is_some_and(Vec::is_empty) {
            return Err(FirmwareError::EmptyTfsPackage);
        }
        Ok(Self {
            cla,
            eeprom,
            tfs_image,
            tfs_package,
            onenand_image: None,
            reset_handler,
        })
    }

    pub fn cla(&self) -> &[u8] {
        &self.cla
    }

    pub fn eeprom(&self) -> &[u8] {
        &self.eeprom
    }

    pub fn eeprom_payload(&self) -> &[u8] {
        &self.eeprom[eeprom_payload_offset(&self.eeprom)..]
    }

    pub fn tfs_image(&self) -> Option<&[u8]> {
        self.tfs_image.as_deref()
    }

    pub fn tfs_package(&self) -> Option<&[u8]> {
        self.tfs_package.as_deref()
    }

    pub fn with_onenand_image(mut self, path: &Path) -> Result<Self, FirmwareError> {
        self.onenand_image = Some(read_file(path)?);
        Ok(self)
    }

    pub fn onenand_image(&self) -> Option<&[u8]> {
        self.onenand_image.as_deref()
    }

    pub fn reset_handler(&self) -> u32 {
        self.reset_handler
    }
}

fn read_file(path: &Path) -> Result<Vec<u8>, FirmwareError> {
    std::fs::read(path).map_err(|source| FirmwareError::Read {
        path: path.to_path_buf(),
        source,
    })
}

fn eeprom_payload_offset(eeprom: &[u8]) -> usize {
    if eeprom.starts_with(NSF_HEADER_MAGIC) && eeprom.len() >= NSF_HEADER_SIZE {
        NSF_HEADER_SIZE
    } else {
        0
    }
}

fn validate_cla(cla: &[u8]) -> Result<u32, FirmwareError> {
    if cla.len() < RESET_VECTOR_OFFSET + 4 {
        return Err(FirmwareError::ClaTooShort);
    }
    if cla.get(..4) != Some(RESET_VECTOR_INSTRUCTION.as_slice()) {
        return Err(FirmwareError::UnsupportedCla);
    }

    let target = u32::from_le_bytes(
        cla[RESET_VECTOR_OFFSET..RESET_VECTOR_OFFSET + 4]
            .try_into()
            .expect("four-byte reset vector"),
    );
    let start = FLASH_BASE;
    let end = u64::from(FLASH_BASE) + cla.len() as u64;
    if u64::from(target) < u64::from(start) || u64::from(target) >= end {
        return Err(FirmwareError::ResetHandlerOutsideImage { target, start, end });
    }
    Ok(target)
}

mod machine;

pub use machine::{
    BootReport, E830Machine, MachineError, MemoryFault, MmioAccess, StopReason, LCD_HEIGHT,
    LCD_WIDTH,
};

#[cfg(test)]
mod tests {
    use super::{FirmwareError, FirmwareSet, FLASH_BASE};

    fn test_cla(target: u32) -> Vec<u8> {
        let mut cla = vec![0; 0x200];
        cla[..4].copy_from_slice(&[0x54, 0xf1, 0x9f, 0xe5]);
        cla[0x15c..0x160].copy_from_slice(&target.to_le_bytes());
        cla
    }

    #[test]
    fn e830_image_uses_the_reset_handler_stored_in_its_vector_table() {
        let target = FLASH_BASE + 0x100;
        let images = FirmwareSet::from_bytes(test_cla(target), vec![1, 2, 3], None, None).unwrap();
        assert_eq!(images.reset_handler(), target);
        assert_eq!(images.eeprom(), &[1, 2, 3]);
    }

    #[test]
    fn strips_the_observed_nsf_container_header_from_mapped_eeprom_data() {
        let mut eeprom = vec![0xff; 24];
        eeprom[..3].copy_from_slice(b"NF4");
        eeprom[20..].copy_from_slice(&[1, 2, 3, 4]);
        let images =
            FirmwareSet::from_bytes(test_cla(FLASH_BASE + 0x100), eeprom, None, None).unwrap();
        assert_eq!(images.eeprom_payload(), &[1, 2, 3, 4]);
    }

    #[test]
    fn rejects_an_image_without_the_e830_reset_vector() {
        let error = FirmwareSet::from_bytes(vec![0; 0x200], vec![1], None, None).unwrap_err();
        assert!(matches!(error, FirmwareError::UnsupportedCla));
    }

    #[test]
    fn rejects_a_reset_handler_outside_the_firmware_image() {
        let error =
            FirmwareSet::from_bytes(test_cla(FLASH_BASE + 0x400), vec![1], None, None).unwrap_err();
        assert!(matches!(
            error,
            FirmwareError::ResetHandlerOutsideImage { .. }
        ));
    }

    #[test]
    fn requires_an_eeprom_image() {
        let error =
            FirmwareSet::from_bytes(test_cla(FLASH_BASE + 0x100), vec![], None, None).unwrap_err();
        assert!(matches!(error, FirmwareError::EmptyEeprom));
    }
}

mod cpu;
mod uart;
mod i2c;
mod serial;
mod onenand;

mod nor;
