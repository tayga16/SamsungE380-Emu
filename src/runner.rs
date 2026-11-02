use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::{atomic::{AtomicBool, Ordering}, mpsc::{self, Receiver}, Arc};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use e830::{E830Machine, FirmwareSet, StopReason};
use serde::{Deserialize, Serialize};

// Keep the worker responsive to phone contacts. At the old 2M-instruction
// quantum a batch can occupy the CPU thread for hundreds of milliseconds,
// allowing a complete GUI click (down + up) to accumulate before the guest
// gets another chance to service IRQ16.
const NORMAL_RUN_QUANTUM: u64 = 2_000_000;
// The post-animation UI path needs up to the same one-million-instruction
// separation used by the successful late-key CLI probe. 250k was enough for
// IRQ16/HKBD delivery, but could release a direction contact before the UI
// task consumed its queued press message.
const INPUT_RUN_QUANTUM: u64 = 1_000_000;

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
    InputApplied(String),
    Finished(String),
}

fn describe_key_transition(previous: u32, keys: u32) -> String {
    let mut changes = Vec::new();
    for key in e830::PhoneKey::ALL {
        if (previous ^ keys) & key.bit() != 0 {
            changes.push(format!("{key:?} {}", if keys & key.bit() != 0 { "down" } else { "up" }));
        }
    }
    if changes.is_empty() { "no contact change".into() } else { changes.join(", ") }
}

fn input_history_suffix(history: &VecDeque<String>) -> String {
    if history.is_empty() { String::new() } else {
        format!("; recent input: {}", history.iter().cloned().collect::<Vec<_>>().join(" | "))
    }
}

pub struct Worker {
    input: mpsc::Sender<u32>,
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
    pub events: Receiver<WorkerEvent>,
}

impl Worker {
    pub fn start(paths: FirmwarePaths) -> std::io::Result<Self> {
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = stop.clone();
        let (sender, events) = mpsc::channel();
        let (input, worker_input) = mpsc::channel();
        let handle = std::thread::Builder::new().name("e830-firmware".into()).spawn(move || {
            // Unicorn owns Rc state and must be constructed inside this thread.
            let result = (|| -> anyhow::Result<String> {
                let mut machine = E830Machine::new(paths.load()?, false)?;
                let mut last_lcd_revision = 0;
                let mut latest_frame = None;
                let mut last_emit_at = Instant::now() - Duration::from_secs(1);
                let mut last_report: Option<e830::BootReport> = None;
                let mut last_keys = 0;
                let mut input_history = VecDeque::with_capacity(8);
                while !worker_stop.load(Ordering::Relaxed) {
                    // Apply at most one contact transition per guest quantum.
                    // This preserves ordering while ensuring a queued release
                    // cannot overtake the firmware's opportunity to service
                    // the preceding press interrupt.
                    let input_applied = if let Ok(keys) = worker_input.try_recv() {
                        let instructions = last_report.as_ref().map_or(0, |report| report.instructions);
                        let pc = last_report.as_ref().map_or(0, |report| report.pc);
                        let entry = format!(
                            "{} at {} (mask=0x{keys:06x}, PC=0x{pc:08x})",
                            describe_key_transition(last_keys, keys), instructions,
                        );
                        machine.set_phone_keys(keys);
                        last_keys = keys;
                        if input_history.len() == 8 { input_history.pop_front(); }
                        input_history.push_back(entry.clone());
                        if sender.send(WorkerEvent::InputApplied(entry)).is_err() {
                            return Ok("Window closed.".into());
                        }
                        true
                    } else { false };
                    let quantum = if input_applied { INPUT_RUN_QUANTUM } else { NORMAL_RUN_QUANTUM };
                    let report = machine.run(quantum).map_err(|error| anyhow::anyhow!(
                        "running guest: {error}{}", input_history_suffix(&input_history)
                    ))?;
                    let revision = machine.lcd_write_revision();
                    if revision != last_lcd_revision {
                        last_lcd_revision = revision;
                        latest_frame = machine.lcd_snapshot_rgb();
                    }
                    if last_emit_at.elapsed() >= Duration::from_millis(50) {
                        if let Some(rgb) = latest_frame.take() {
                            if sender.send(WorkerEvent::Frame(rgb)).is_err() { return Ok("Window closed.".into()); }
                            last_emit_at = Instant::now();
                        }
                    }
                    if let Some(fault) = &report.memory_fault {
                        anyhow::bail!("Guest memory fault: {} at 0x{:08x}, PC=0x{:08x}{}",
                            fault.kind, fault.address, fault.pc, input_history_suffix(&input_history));
                    }
                    if let StopReason::CpuError(error) = &report.stop_reason {
                        anyhow::bail!("CPU stopped: {error}; PC=0x{:08x}; CPSR=0x{:08x}; SP=0x{:08x}; LR=0x{:08x}{}",
                            report.pc, report.cpsr, report.sp, report.lr, input_history_suffix(&input_history));
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
        Ok(Self { input, stop, handle: Some(handle), events })
    }

    pub fn set_phone_keys(&self, keys: u32) { let _ = self.input.send(keys); }

    pub fn stop(&mut self) {
        self.set_phone_keys(0);
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
    fn input_channel_preserves_short_click_and_stop_releases_before_join() {
        let (input, receive) = mpsc::channel();
        let (_events, events) = mpsc::channel();
        let mut worker = Worker { input, stop: Arc::new(AtomicBool::new(false)), handle: None, events };
        worker.set_phone_keys(e830::PhoneKey::Ok.bit());
        worker.set_phone_keys(0);
        assert_eq!(receive.try_iter().collect::<Vec<_>>(), vec![e830::PhoneKey::Ok.bit(), 0]);
        worker.stop();
        assert_eq!(receive.try_recv().unwrap(), 0);
        assert!(worker.stop.load(Ordering::Relaxed));
    }

    #[test]
    fn input_diagnostics_describe_press_and_release() {
        let one = e830::PhoneKey::One.bit();
        assert_eq!(describe_key_transition(0, one), "One down");
        assert_eq!(describe_key_transition(one, 0), "One up");
        assert_eq!(describe_key_transition(0, 0), "no contact change");
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
