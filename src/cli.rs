use std::io::Write;
use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Parser;
use e830::{E830Machine, FirmwareSet, PhoneKey, StopReason, LCD_HEIGHT, LCD_WIDTH};

mod provision;

#[derive(Parser)]
#[command(about = "Samsung SGH-E830 firmware boot probe", version)]
struct Args {
    /// Original E830 .cla firmware
    cla: PathBuf,
    /// Matching .nsf EEPROM image
    eeprom: PathBuf,
    /// Retail TFS installer package (physical filesystem installation is not implemented)
    #[arg(long)]
    tfs: Option<PathBuf>,
    /// Optional diagnostic loader source at 0x1c000000 (takes precedence over --tfs)
    #[arg(long)]
    tfs_source: Option<PathBuf>,
    /// Physical OneNAND dump: 32 MiB main-only or 1024+32 bytes per page
    #[arg(long)]
    onenand_image: Option<PathBuf>,
    /// Save current physical OneNAND including guest writes; destination must be new
    #[arg(long)]
    dump_onenand: Option<PathBuf>,
    /// Diagnostic original-firmware call script; requires a reached --break-at
    #[arg(long, requires = "break_at")]
    firmware_script: Option<PathBuf>,
    #[arg(long, default_value_t = 1_200_000_000)]
    max_instructions: u64,
    #[arg(long)]
    dump_lcd_frame: Option<PathBuf>,
    #[arg(long)]
    trace_mmio: bool,
    #[arg(long, value_parser = parse_address)]
    peek: Vec<u32>,
    /// Stop before executing this PC (ARM or Thumb)
    #[arg(long, value_parser = parse_address)]
    break_at: Option<u32>,
    /// Start looking for the breakpoint after this instruction count
    #[arg(long, default_value_t = 0)]
    break_after: u64,
    /// Save the guest's 32 MiB SDRAM for analysis
    #[arg(long)]
    dump_ram: Option<PathBuf>,
    /// Save bytes transmitted by the first UART (boot/download serial port)
    #[arg(long)]
    dump_uart: Option<PathBuf>,
    /// Save DSP boot memory space 0 as little-endian 16-bit words
    #[arg(long)]
    dump_dsp_space0: Option<PathBuf>,
    /// Save DSP boot memory space 1 as little-endian 16-bit words
    #[arg(long)]
    dump_dsp_space1: Option<PathBuf>,
    /// External phone contact transition: INSTRUCTION:KEY:down|up (repeatable)
    #[arg(long, value_parser = parse_key_transition)]
    key_at: Vec<KeyTransition>,
}

#[derive(Clone, Debug)]
struct KeyTransition { at: u64, key: PhoneKey, down: bool }

fn parse_key_transition(raw: &str) -> Result<KeyTransition, String> {
    let parts: Vec<_> = raw.split(':').collect();
    if parts.len() != 3 { return Err("Expected INSTRUCTION:KEY:down|up".into()); }
    Ok(KeyTransition {
        at: parts[0].parse::<u64>().map_err(|error| error.to_string())?,
        key: PhoneKey::parse(parts[1]).ok_or_else(|| "Unknown phone key".to_string())?,
        down: match parts[2] { "down" => true, "up" => false, _ => return Err("Use down or up".into()) },
    })
}

fn parse_address(raw: &str) -> Result<u32, String> {
    raw.strip_prefix("0x").or_else(|| raw.strip_prefix("0X"))
        .map(|value| u32::from_str_radix(value, 16))
        .unwrap_or_else(|| raw.parse()).map_err(|error| error.to_string())
}

fn main() -> Result<()> {
    let args = Args::parse();
    let mut firmware = FirmwareSet::load(&args.cla, &args.eeprom, args.tfs_source.as_deref(), args.tfs.as_deref())?;
    if let Some(path) = args.onenand_image.as_deref() {
        firmware = firmware.with_onenand_image(path)?;
    }
    let mut machine = E830Machine::new(firmware, args.trace_mmio)?;
    if let Some(pc) = args.break_at { machine.set_breakpoint(pc, args.break_after); }
    let mut transitions = args.key_at;
    transitions.sort_by_key(|event| event.at);
    anyhow::ensure!(transitions.iter().all(|event| event.at < args.max_instructions),
        "key transition must precede max-instructions");
    let mut keys = 0;
    let mut executed = 0;
    let mut stopped = None;
    for event in transitions {
        if event.at > executed {
            let report = machine.run(event.at - executed)?;
            executed = report.instructions;
            if report.memory_fault.is_some() || report.stop_reason != StopReason::InstructionBudget {
                stopped = Some(report); break;
            }
        }
        if event.down { keys |= event.key.bit(); } else { keys &= !event.key.bit(); }
        machine.set_phone_keys(keys);
        println!("Phone contact at {executed}: {:?} {}", event.key, if event.down { "down" } else { "up" });
    }
    let report = match stopped {
        Some(report) => report,
        None => machine.run(args.max_instructions - executed)?,
    };
    println!("{report:#?}");
    if let Some(path) = args.firmware_script {
        anyhow::ensure!(report.stop_reason == StopReason::Breakpoint && report.memory_fault.is_none(),
            "provisioning requires a reached breakpoint without a memory fault");
        provision::execute(&mut machine, &path)?;
    }
    if let Some(path) = args.dump_onenand {
        let mut output = std::fs::OpenOptions::new().write(true).create_new(true).open(&path)
            .context("creating a new NAND capture (existing files are preserved)")?;
        output.write_all(&machine.onenand_snapshot()).context("saving physical NAND capture")?;
        println!("OneNAND saved: {}", path.display());
    }
    if let Some(path) = args.dump_uart {
        std::fs::write(&path, machine.uart_output(0).unwrap_or_default())
            .context("saving UART diagnostic output")?;
        println!("UART saved: {}", path.display());
    }
    if let Some(path) = args.dump_ram {
        std::fs::write(&path, machine.sdram_snapshot()?).context("saving diagnostic SDRAM capture")?;
        println!("SDRAM saved: {}", path.display());
    }
    for (space, path) in [(0, args.dump_dsp_space0), (1, args.dump_dsp_space1)] {
        if let Some(path) = path {
            let bytes = machine.dsp_memory_snapshot(space).context("reading DSP boot memory")?;
            std::fs::write(&path, bytes).context("saving DSP boot memory")?;
            println!("DSP space {space} saved: {}", path.display());
        }
    }
    for address in args.peek {
        println!("peek 0x{address:08x}: {:?}", machine.peek_u32(address));
    }
    if let Some(path) = args.dump_lcd_frame {
        if let Some(rgb) = machine.lcd_snapshot_rgb() {
            let mut file = std::fs::File::create(&path).context("creating LCD capture")?;
            write!(file, "P6\n{LCD_WIDTH} {LCD_HEIGHT}\n255\n")?;
            file.write_all(&rgb)?;
            println!("LCD saved: {}", path.display());
        } else {
            println!("No complete LCD frame was produced.");
        }
    }
    if let Some(fault) = report.memory_fault { anyhow::bail!("Guest memory fault: {fault:?}"); }
    if let StopReason::CpuError(error) = report.stop_reason { anyhow::bail!("CPU stopped: {error}"); }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phone_transition_parser_rejects_ambiguous_or_invalid_input() {
        let event = parse_key_transition("600000000:soft-left:down").unwrap();
        assert_eq!(event.at, 600_000_000);
        assert_eq!(event.key, PhoneKey::SoftLeft);
        assert!(event.down);
        assert!(!parse_key_transition("610000000:ok:up").unwrap().down);
        for raw in ["ok", "1:unknown:down", "-1:ok:up", "1:ok:click", "1:ok:up:extra"] {
            assert!(parse_key_transition(raw).is_err());
        }
    }
}
