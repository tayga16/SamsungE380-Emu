use std::path::{Path, PathBuf};
use std::sync::{atomic::{AtomicBool, Ordering}, mpsc::{self, Receiver}, Arc};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use e830::{E830Machine, FirmwareSet, StopReason};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FirmwarePaths {
    pub cla: PathBuf,
    pub eeprom: PathBuf,
    pub tfs: Option<PathBuf>,
    #[serde(default)]
    pub onenand: Option<PathBuf>,
}

impl FirmwarePaths {
    pub fn bundled(root: &Path) -> Option<Self> {
        let firmware = root.join("firmware");
        let cla = firmware.join("E830XEGG1.cla");
        let eeprom = firmware.join("E830 eeprom.nsf");
        let tfs = firmware.join("E830XEGG1.tfs");
        (cla.is_file() && eeprom.is_file()).then(|| Self {
            cla,
            eeprom,
            tfs: tfs.is_file().then_some(tfs),
            onenand: None,
        })
    }

    pub fn onenand_path(&self) -> Option<PathBuf> {
        self.onenand.clone().or_else(|| {
            let sibling = self.cla.with_extension("onenand.bin");
            sibling.is_file().then_some(sibling)
        })
    }

    pub fn load(&self) -> Result<FirmwareSet, e830::FirmwareError> {
        // The retail TFS file is an installer package, staged separately from NOR.
        let firmware = FirmwareSet::load(&self.cla, &self.eeprom, None, self.tfs.as_deref())?;
        // An explicit GUI selection wins, including when it is missing: report
        // its read error rather than silently booting from a different image.
        if let Some(nand) = self.onenand_path() {
            firmware.with_onenand_image(&nand)
        } else { Ok(firmware) }
    }
}

pub fn validate_nand_image(path: &Path) -> Result<(), String> {
    let metadata = std::fs::metadata(path)
        .map_err(|error| format!("Could not open NAND {}: {error}", path.display()))?;
    if !metadata.is_file() {
        return Err("Select a physical NAND file, not a directory.".into());
    }
    if !matches!(metadata.len(), 33_554_432 | 34_603_008) {
        return Err(format!(
            "Unsupported NAND size: {} bytes. Expected 33,554,432 (main only) or 34,603,008 (1024+32 per page). A .tfs package is not a NAND dump.",
            metadata.len()
        ));
    }
    Ok(())
}

pub enum WorkerEvent {
    Frame(Vec<u8>),
    Finished(String),
}

pub struct Worker {
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
    pub events: Receiver<WorkerEvent>,
}

impl Worker {
    pub fn start(paths: FirmwarePaths) -> std::io::Result<Self> {
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = stop.clone();
        let (sender, events) = mpsc::channel();
        let handle = std::thread::Builder::new().name("e830-firmware".into()).spawn(move || {
            // Unicorn owns Rc state and must be constructed inside this thread.
            let result = (|| -> anyhow::Result<String> {
                let mut machine = E830Machine::new(paths.load()?, false)?;
                let mut last_frame_count = 0;
                let mut latest_frame = None;
                let mut last_emit_at = Instant::now() - Duration::from_secs(1);
                let mut last_report = None;
                while !worker_stop.load(Ordering::Relaxed) {
                    let report = machine.run(2_000_000)?;
                    if report.lcd_frames > last_frame_count {
                        last_frame_count = report.lcd_frames;
                        latest_frame = machine.lcd_snapshot_rgb();
                    }
                    if last_emit_at.elapsed() >= Duration::from_millis(50) {
                        if let Some(rgb) = latest_frame.take() {
                            if sender.send(WorkerEvent::Frame(rgb)).is_err() { return Ok("Window closed.".into()); }
                            last_emit_at = Instant::now();
                        }
                    }
                    if let Some(fault) = &report.memory_fault {
                        anyhow::bail!("Guest memory fault: {} at 0x{:08x}, PC=0x{:08x}", fault.kind, fault.address, fault.pc);
                    }
                    if let StopReason::CpuError(error) = &report.stop_reason {
                        anyhow::bail!("CPU stopped: {error}; PC=0x{:08x}", report.pc);
                    }
                    last_report = Some(report);
                    std::thread::sleep(Duration::from_millis(1));
                }
                if let Some(rgb) = latest_frame { let _ = sender.send(WorkerEvent::Frame(rgb)); }
                Ok(last_report.map(|report| format!(
                    "Stopped. LCD frames: {}  PC: 0x{:08x}  Instructions: {}",
                    report.lcd_frames, report.pc, report.instructions
                )).unwrap_or_else(|| "Stopped.".into()))
            })();
            let summary = result.unwrap_or_else(|error| format!("E830 error: {error:#}"));
            let _ = sender.send(WorkerEvent::Finished(summary));
        })?;
        Ok(Self { stop, handle: Some(handle), events })
    }

    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() { let _ = handle.join(); }
    }
}

impl Drop for Worker {
    fn drop(&mut self) { self.stop(); }
}

pub fn classify_firmware_files(paths: Vec<PathBuf>) -> Result<FirmwarePaths, String> {
    let mut cla = None;
    let mut eeprom = None;
    let mut tfs = None;
    for path in paths {
        let extension = path.extension().and_then(|value| value.to_str()).unwrap_or_default().to_ascii_lowercase();
        let destination = match extension.as_str() {
            "cla" => &mut cla,
            "nsf" => &mut eeprom,
            "tfs" => &mut tfs,
            _ => return Err("Select .cla, .nsf and optional .tfs files only.".into()),
        };
        if destination.replace(path).is_some() { return Err("Select only one file of each E830 type.".into()); }
    }
    Ok(FirmwarePaths {
        cla: cla.ok_or_else(|| "The .cla firmware file was not selected.".to_string())?,
        eeprom: eeprom.ok_or_else(|| "The .nsf EEPROM file was not selected.".to_string())?,
        tfs,
        onenand: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::sync::atomic::AtomicU64;

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let root = std::env::temp_dir().join(format!(
                "e830-nand-test-{}-{}-{}", std::process::id(),
                std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir(&root).unwrap();
            Self(root)
        }

        fn file(&self, name: &str, length: u64, marker: u8) -> PathBuf {
            let path = self.0.join(name);
            let mut file = std::fs::File::create(&path).unwrap();
            file.set_len(length).unwrap();
            if length > 0 { file.write_all(&[marker]).unwrap(); }
            path
        }

        fn firmware(&self) -> FirmwarePaths {
            let cla = self.0.join("test.cla");
            let mut bytes = vec![0; 0x200];
            bytes[..4].copy_from_slice(&[0x54, 0xf1, 0x9f, 0xe5]);
            bytes[0x15c..0x160].copy_from_slice(&(e830::FLASH_BASE + 0x100).to_le_bytes());
            std::fs::write(&cla, bytes).unwrap();
            FirmwarePaths { cla, eeprom: self.file("test.nsf", 1, 1), tfs: None, onenand: None }
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            // Only this test's uniquely created directory is removed.
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn old_firmware_config_remains_compatible() {
        let paths: FirmwarePaths = serde_json::from_str(r#"{"cla":"test.cla","eeprom":"test.nsf","tfs":null}"#).unwrap();
        assert!(paths.onenand.is_none());
        let mut selected = paths;
        selected.onenand = Some(PathBuf::from("selected.onenand.bin"));
        let restored: FirmwarePaths = serde_json::from_slice(&serde_json::to_vec(&selected).unwrap()).unwrap();
        assert_eq!(restored.onenand, selected.onenand);
    }

    #[test]
    fn selected_nand_overrides_sibling_and_never_falls_back_if_missing() {
        let directory = TestDirectory::new();
        let mut paths = directory.firmware();
        assert!(paths.onenand_path().is_none());
        assert!(paths.load().unwrap().onenand_image().is_none());
        let sibling = directory.file("test.onenand.bin", 33_554_432, 0x11);
        assert_eq!(paths.onenand_path(), Some(sibling.clone()));
        assert_eq!(paths.load().unwrap().onenand_image().unwrap()[0], 0x11);
        let selected = directory.file("chosen.bin", 34_603_008, 0x22);
        paths.onenand = Some(selected.clone());
        assert_eq!(paths.onenand_path(), Some(selected.clone()));
        {
            let loaded = paths.load().unwrap();
            let image = loaded.onenand_image().unwrap();
            assert_eq!(image.len(), 34_603_008);
            assert_eq!(image[0], 0x22);
        }
        assert_eq!(std::fs::read(&selected).unwrap()[0], 0x22);
        assert_eq!(std::fs::read(&sibling).unwrap()[0], 0x11);
        let missing = directory.0.join("missing.bin");
        paths.onenand = Some(missing.clone());
        assert_eq!(paths.onenand_path(), Some(missing.clone()));
        match paths.load().unwrap_err() {
            e830::FirmwareError::Read { path, .. } => assert_eq!(path, missing),
            error => panic!("Unexpected error: {error}"),
        }
    }

    #[test]
    fn nand_validation_checks_both_physical_geometries_without_writing() {
        let directory = TestDirectory::new();
        for length in [33_554_432, 34_603_008] {
            let path = directory.file(&format!("nand-{length}.bin"), length, 0x5a);
            validate_nand_image(&path).unwrap();
            assert_eq!(std::fs::metadata(&path).unwrap().len(), length);
            assert_eq!(std::fs::read(&path).unwrap()[0], 0x5a);
        }
        for length in [0, 1024, 67_108_864] {
            let path = directory.file(&format!("invalid-{length}.tfs"), length, 0);
            assert!(validate_nand_image(&path).unwrap_err().contains("Unsupported NAND size"));
        }
        assert!(validate_nand_image(&directory.0).unwrap_err().contains("directory"));
        assert!(validate_nand_image(&directory.0.join("missing.bin")).is_err());
    }

    #[test]
    fn firmware_picker_keeps_nand_separate() {
        let paths = classify_firmware_files(vec!["test.CLA".into(), "test.NSF".into(), "test.TFS".into()]).unwrap();
        assert!(paths.onenand.is_none());
        assert_eq!(paths.tfs, Some("test.TFS".into()));
        assert!(classify_firmware_files(vec!["test.cla".into(), "test.nsf".into(), "nand.bin".into()]).is_err());
        assert!(classify_firmware_files(vec!["test.cla".into(), "other.cla".into(), "test.nsf".into()]).is_err());
        assert!(classify_firmware_files(vec!["test.cla".into()]).is_err());
    }
}
