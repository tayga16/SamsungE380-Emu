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
        })
    }

    pub fn load(&self) -> Result<FirmwareSet, e830::FirmwareError> {
        // The retail TFS file is an installer package, staged separately from NOR.
        let firmware = FirmwareSet::load(&self.cla, &self.eeprom, None, self.tfs.as_deref())?;
        // Optional physical dump beside the CLA, without changing the file picker.
        let nand = self.cla.with_extension("onenand.bin");
        if nand.is_file() { firmware.with_onenand_image(&nand) } else { Ok(firmware) }
    }
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
    })
}
