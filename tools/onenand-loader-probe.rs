//! Isolated native research harness for examined Samsung OneNAND loaders.
//! Its address map differs from the E830 retail board. Only supplied serial
//! bytes are delivered; no firmware instructions or filesystem results change.
//! Vendor payloads and generated media stay in the excluded proof directory.
use std::collections::{BTreeMap, VecDeque};
use std::io::Write;
use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use clap::Parser;
use unicorn_engine::{RegisterARM, Unicorn};
use unicorn_engine::unicorn_const::{Arch, HookType, Mode, Prot};

#[path = "../src/onenand.rs"]
mod onenand;
#[path = "../src/uart.rs"]
mod uart;
#[path = "../src/nor.rs"]
#[allow(dead_code)]
mod nor;

const LOADER_RAM_SIZE: u64 = 0x0100_0000;

struct LoaderProfile {
    name: &'static str,
    ram: u64,
    sram: u64,
    stack: u64,
    serial: u64,
    nand: u64,
    nor: u64,
    serial_poll: u64,
}

fn loader_profile(bytes: &[u8]) -> Result<LoaderProfile> {
    if bytes.len() < 32 || bytes[..8] != [0x0c, 0xf0, 0x9f, 0xe5, 0x0c, 0xf0, 0x9f, 0xe5] {
        bail!("expected an examined original loader; use extract_onenand_loader.py");
    }
    match bytes.len() {
        0x2e3b4 => Ok(LoaderProfile { name: "Corona v1.7 / XSR1", ram: 0x18000000,
            sram: 0x0c000000, stack: 0x0c008600, serial: 0x30403000,
            nand: 0x28000000, nor: 0x10000000, serial_poll: 0x18008260 }),
        0x3d140 if bytes[0x18..0x1c] == 0x300061d4u32.to_le_bytes() =>
            Ok(LoaderProfile { name: "Clifford VE v0.1 / XSR2", ram: 0x30000000,
                sram: 0x1c000000, stack: 0x1c00860c, serial: 0x50403000,
                nand: 0x44000000, nor: 0x21c00000, serial_poll: 0x3000986c }),
        _ => bail!("unexamined loader layout"),
    }
}

#[derive(Parser)]
struct Args {
    #[arg(long)]
    loader: PathBuf,
    /// Explicit incoming serial bytes, e.g. D0 10 00 00 00 D1 E0.
    #[arg(long, default_value = "", conflicts_with = "serial_input")]
    serial_hex: String,
    /// Binary stream of the examined loader's original serial protocol.
    #[arg(long)]
    serial_input: Option<PathBuf>,
    #[arg(long, default_value_t = 30_000_000)]
    max_instructions: usize,
    #[arg(long)]
    onenand_image: Option<PathBuf>,
    #[arg(long)]
    dump_onenand: Option<PathBuf>,
    #[arg(long)]
    dump_ram: Option<PathBuf>,
    #[arg(long)]
    dump_sram: Option<PathBuf>,
    /// Stop before a loader instruction, preserving all natural guest state.
    #[arg(long, value_parser = parse_address)]
    break_at: Option<u64>,
    #[arg(long, value_parser = parse_address)]
    watch_pc: Vec<u64>,
    /// Include 128 bytes at each address in explicit PC watch observations.
    #[arg(long, value_parser = parse_address)]
    watch_memory: Vec<u64>,
    /// Record physical commands for comparison with original STL requests.
    #[arg(long)]
    trace_onenand: bool,
    /// Export logical STL sectors through original initialized driver calls.
    #[arg(long)]
    export_stl: Option<PathBuf>,
    /// Import a same-size logical image using original STL writes (research only).
    #[arg(long)]
    import_stl: Option<PathBuf>,
    /// Call the original FAT formatter with an explicit cluster size at its E0 entry.
    /// Requires --break-at 0x3000b170; only the formatter's public argument changes.
    #[arg(long)]
    format_fat_cluster_sectors: Option<u32>,
}

fn parse_address(value: &str) -> Result<u64, String> {
    u64::from_str_radix(value.strip_prefix("0x").unwrap_or(value), 16)
        .map_err(|error| error.to_string())
}

#[derive(Default)]
struct State {
    input: VecDeque<u8>,
    uart: uart::Uart,
    nand: onenand::OneNand,
    nor: Option<nor::NorFlash>,
    stop: Option<&'static str>,
    command_counts: BTreeMap<u16, u64>,
    first_commands: Vec<serde_json::Value>,
    trace_onenand: bool,
}

fn save_new(path: PathBuf, bytes: &[u8]) -> Result<()> {
    std::fs::OpenOptions::new().write(true).create_new(true).open(&path)
        .with_context(|| format!("creating new output {}", path.display()))?
        .write_all(bytes)?;
    Ok(())
}

/// Call only examined loader functions, restoring caller CPU and argument area.
/// The original driver performs every physical NAND write and metadata update.
fn guest_call(cpu: &mut Unicorn<State>, profile: &LoaderProfile, function: u32, args: &[u32]) -> Result<u32> {
    if u64::from(function & !1) < profile.ram || u64::from(function & !1) >= profile.ram + 0x3d140
        || args.len() > 16 { bail!("invalid examined loader call"); }
    let context = cpu.context_init()?;
    let pc = cpu.reg_read(RegisterARM::PC)?;
    let cpsr = cpu.reg_read(RegisterARM::CPSR)?;
    let sp = cpu.reg_read(RegisterARM::SP)?;
    let stack_args: Vec<u8> = args.iter().skip(4).flat_map(|value| value.to_le_bytes()).collect();
    let mut saved_stack = vec![0; stack_args.len()];
    cpu.mem_read(sp, &mut saved_stack)?;
    let saved_stop = cpu.get_data().stop;
    let called = (|| -> Result<u32> {
        for (register, value) in [RegisterARM::R0, RegisterARM::R1, RegisterARM::R2, RegisterARM::R3]
            .into_iter().zip(args.iter().copied().chain(std::iter::repeat(0))) {
            cpu.reg_write(register, u64::from(value))?;
        }
        if !stack_args.is_empty() { cpu.mem_write(sp, &stack_args)?; }
        cpu.reg_write(RegisterARM::LR, pc | ((cpsr >> 5) & 1))?;
        cpu.reg_write(RegisterARM::CPSR, (cpsr & !0x20) | (u64::from(function & 1) << 5))?;
        cpu.get_data_mut().stop = None;
        cpu.emu_start(u64::from(function), pc & !1, 0, 2_000_000)?;
        if cpu.reg_read(RegisterARM::PC)? != pc & !1 || cpu.get_data().stop == Some("memory_fault") {
            bail!("original call 0x{function:08x} failed to return");
        }
        Ok(cpu.reg_read(RegisterARM::R0)? as u32)
    })();
    cpu.context_restore(&context)?;
    if !saved_stack.is_empty() { cpu.mem_write(sp, &saved_stack)?; }
    cpu.get_data_mut().stop = saved_stop;
    called
}

fn transfer_stl(cpu: &mut Unicorn<State>, profile: &LoaderProfile, import: Option<PathBuf>, export: Option<PathBuf>) -> Result<()> {
    if profile.ram != 0x30000000 {
        bail!("STL transfer requires the examined Clifford VE profile");
    }
    let formatted_mount = cpu.get_data().stop == Some("breakpoint") &&
        cpu.reg_read(RegisterARM::PC)? == 0x3000b1a0;
    let serial_mount = cpu.get_data().stop == Some("waiting_for_serial_input") &&
        cpu.get_data().uart.output().last() == Some(&0xda);
    if !formatted_mount && !serial_mount {
        bail!("original DA mount did not succeed; logical transfer refused");
    }
    let mut geometry = [0; 4];
    cpu.mem_read(0x300d1aa0, &mut geometry)?;
    let sectors = u32::from_le_bytes(geometry);
    if sectors == 0 || sectors > 65536 { bail!("invalid original STL geometry {sectors}"); }
    let incoming = import.map(std::fs::read).transpose()?;
    if incoming.as_ref().is_some_and(|bytes| bytes.len() != sectors as usize * 512) {
        bail!("logical image size differs from original STL geometry");
    }
    let buffer = guest_call(cpu, profile, 0x30015f71, &[8192, 0])?;
    if buffer < 0x30000000 || u64::from(buffer) + 8192 > profile.ram + LOADER_RAM_SIZE {
        bail!("original allocator returned invalid buffer");
    }
    let transferred = (|| -> Result<()> {
        let mut logical = vec![0; sectors as usize * 512];
        for sector in (0..sectors).step_by(16) {
            let count = 16.min(sectors - sector);
            let range = sector as usize * 512..(sector + count) as usize * 512;
            if let Some(ref bytes) = incoming {
                cpu.mem_write(u64::from(buffer), &bytes[range.clone()])?;
                let result = guest_call(cpu, profile, 0x30019a39, &[sector, buffer, count, 0x300a03e0])?;
                if result != count { bail!("STL write at {sector} returned 0x{result:08x}"); }
            }
            let result = guest_call(cpu, profile, 0x300199e1, &[sector, buffer, count, 0x300a03e0])?;
            if result != count { bail!("STL read at {sector} returned 0x{result:08x}"); }
            cpu.mem_read(u64::from(buffer), &mut logical[range.clone()])?;
            if incoming.as_ref().is_some_and(|bytes| bytes[range.clone()] != logical[range]) {
                bail!("STL readback differs at logical sector {sector}");
            }
            if sector % 8192 == 0 { println!("STL transfer {sector}/{sectors} sectors"); }
        }
        if let Some(path) = export { save_new(path, &logical)?; }
        println!("STL transfer verified {sectors} sectors through original driver");
        Ok(())
    })();
    let released = guest_call(cpu, profile, 0x30015fb9, &[buffer, 8192]);
    transferred?;
    released?;
    Ok(())
}

fn main() -> Result<()> {
    let args = Args::parse();
    let loader = std::fs::read(&args.loader)?;
    let profile = loader_profile(&loader)?;
    let compact: String = args.serial_hex.chars().filter(|value| !value.is_ascii_whitespace()).collect();
    if !compact.is_ascii() || compact.len() % 2 != 0 { bail!("invalid serial hex"); }
    let mut input = (0..compact.len()).step_by(2)
        .map(|offset| u8::from_str_radix(&compact[offset..offset + 2], 16))
        .collect::<Result<VecDeque<_>, _>>()?;
    if let Some(path) = args.serial_input { input = std::fs::read(path)?.into(); }
    let mut state = State { input, trace_onenand: args.trace_onenand,
        nor: Some(nor::NorFlash::new(vec![0xff; 0x400000])), ..State::default() };
    if let Some(path) = args.onenand_image {
        state.nand.load_image(std::fs::read(path)?)
            .map_err(|size| anyhow::anyhow!("unsupported physical NAND image size {size}"))?;
    }
    let mut cpu = Unicorn::new_with_data(Arch::ARM, Mode::ARM946 | Mode::LITTLE_ENDIAN, state)
        .map_err(|error| anyhow::anyhow!("creating loader CPU: {error:?}"))?;
    cpu.mem_map(profile.ram, LOADER_RAM_SIZE, Prot::ALL)?;
    cpu.mem_write(profile.ram, &loader)?;
    cpu.mem_map(profile.sram, 0x10000, Prot::ALL)?;
    if profile.ram != 0x30000000 {
        cpu.mem_map(0x3030_0000, 0x10000, Prot::READ | Prot::WRITE)?;
    }
    cpu.mmio_map(profile.nor, 0x400000,
        Some(|uc: &mut Unicorn<State>, offset, size| uc.get_data().nor.as_ref().unwrap().read(offset, size)),
        Some(|uc: &mut Unicorn<State>, offset, size, value| uc.get_data_mut().nor.as_mut().unwrap().write(offset, size, value)))?;
    cpu.mmio_map(profile.serial, 0x1000,
        Some(|uc: &mut Unicorn<State>, offset: u64, _size| {
            let state = uc.get_data_mut();
            if offset == 0 && state.uart.read(0xc) & 0x80 == 0 {
                return state.input.pop_front().map(u64::from).unwrap_or(0);
            }
            let value = u64::from(state.uart.read(offset as u32));
            if offset == 0x14 && !state.input.is_empty() { value | 1 } else { value }
        }),
        Some(|uc: &mut Unicorn<State>, offset: u64, _size, value| uc.get_data_mut().uart.write(offset as u32, value as u32)))?;
    cpu.mmio_map(profile.nand, 0x20000,
        Some(|uc: &mut Unicorn<State>, offset: u64, size| uc.get_data().nand.read(offset as u32, size)),
        Some(|uc: &mut Unicorn<State>, offset: u64, size, value| {
            if offset == 0x1e440 && size >= 2 {
                let pc = uc.reg_read(RegisterARM::PC).unwrap_or(0);
                let state = uc.get_data_mut();
                if state.trace_onenand {
                    println!("nand pc=0x{pc:08x} cmd=0x{value:02x} block={} page_sector=0x{:04x} buffer=0x{:04x}",
                        state.nand.read(0x1e200, 2), state.nand.read(0x1e20e, 2),
                        state.nand.read(0x1e400, 2));
                    if matches!(value, 0x80 | 0x1a) {
                        for bank in 0..2 {
                            let bytes: String = (0..32).map(|byte|
                                format!("{:02x}", state.nand.read(0x10020 + bank * 32 + byte, 1)))
                                .collect();
                            println!("nand program spare bank={bank} {bytes}");
                        }
                    }
                }
                *state.command_counts.entry(value as u16).or_default() += 1;
                if state.first_commands.len() < 32 {
                    state.first_commands.push(serde_json::json!({"pc": pc, "command": value,
                        "block": state.nand.read(0x1e200, 2), "page_sector": state.nand.read(0x1e20e, 2),
                        "buffer": state.nand.read(0x1e400, 2)}));
                }
            }
            uc.get_data_mut().nand.write(offset as u32, size, value);
        }))?;
    cpu.add_code_hook(profile.serial_poll, profile.serial_poll, |uc, _, _| {
        if uc.get_data().input.is_empty() {
            uc.get_data_mut().stop = Some("waiting_for_serial_input");
            let _ = uc.emu_stop();
        }
    })?;
    if let Some(address) = args.break_at {
        cpu.add_code_hook(address, address, |uc, _, _| {
            uc.get_data_mut().stop = Some("breakpoint");
            let _ = uc.emu_stop();
        })?;
    }
    for address in args.watch_pc {
        let watched_memory = args.watch_memory.clone();
        cpu.add_code_hook(address, address, move |uc, pc, _| {
            println!("watch pc=0x{pc:08x} r0=0x{:08x} r1=0x{:08x} r2=0x{:08x} r3=0x{:08x} sp=0x{:08x} r4=0x{:08x} r5=0x{:08x} lr=0x{:08x}",
                uc.reg_read(RegisterARM::R0).unwrap_or(0), uc.reg_read(RegisterARM::R1).unwrap_or(0),
                uc.reg_read(RegisterARM::R2).unwrap_or(0), uc.reg_read(RegisterARM::R3).unwrap_or(0),
                uc.reg_read(RegisterARM::SP).unwrap_or(0), uc.reg_read(RegisterARM::R4).unwrap_or(0),
                uc.reg_read(RegisterARM::R5).unwrap_or(0), uc.reg_read(RegisterARM::LR).unwrap_or(0));
            for address in &watched_memory {
                let mut bytes = [0; 128];
                if uc.mem_read(*address, &mut bytes).is_ok() {
                    let hex: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
                    println!("watch memory=0x{address:08x} {hex}");
                }
            }
        })?;
    }
    cpu.add_mem_hook(HookType::MEM_INVALID, 0, u64::MAX, |uc, kind, address, size, value| {
        println!("memory fault {kind:?} address=0x{address:08x} size={size} value=0x{value:x}");
        uc.get_data_mut().stop = Some("memory_fault");
        false
    })?;
    // ABI stack follows each original pre-loader's system-mode calculation:
    // SRAM end + 0x8000 - 0x10 - 0x190 - 3*0x10.
    // Begin at the original RAM entry vector; its startup copies/clears data.
    cpu.reg_write(RegisterARM::CPSR, 0xdf)?;
    cpu.reg_write(RegisterARM::SP, profile.stack)?;
    let result = cpu.emu_start(profile.ram + 4, u64::MAX, 0, args.max_instructions);
    result.map_err(|error| anyhow::anyhow!("loader execution stopped with {error:?}"))?;
    if let Some(sectors) = args.format_fat_cluster_sectors {
        if profile.ram != 0x30000000 || cpu.get_data().stop != Some("breakpoint")
            || cpu.reg_read(RegisterARM::PC)? != 0x3000b170
            || !matches!(sectors, 1 | 2 | 4 | 8 | 16 | 32 | 64) {
            bail!("explicit FAT cluster size requires examined original E0 formatter entry");
        }
        let call_args = [cpu.reg_read(RegisterARM::R0)? as u32,
            cpu.reg_read(RegisterARM::R1)? as u32,
            cpu.reg_read(RegisterARM::R2)? as u32, sectors];
        let formatted = guest_call(&mut cpu, &profile, 0x3001e069, &call_args)?;
        if formatted != 0 { bail!("original FAT formatter returned 0x{formatted:08x}"); }
        println!("original FAT formatter completed with {sectors} sectors per cluster");
    }
    if args.export_stl.is_some() || args.import_stl.is_some() {
        transfer_stl(&mut cpu, &profile, args.import_stl, args.export_stl)?;
    }
    let pc = cpu.reg_read(RegisterARM::PC)?;
    let r0 = cpu.reg_read(RegisterARM::R0)?;
    let r1 = cpu.reg_read(RegisterARM::R1)?;
    let state = cpu.get_data();
    let report = serde_json::json!({"profile": profile.name, "execution": format!("{result:?}"), "pc": pc, "r0": r0, "r1": r1,
        "stop": state.stop.unwrap_or("instruction_budget"), "serial_remaining": state.input.len(),
        "serial_output_hex": state.uart.output().iter().map(|byte| format!("{byte:02x}")).collect::<String>(),
        "command_counts": state.command_counts, "first_commands": state.first_commands});
    println!("{}", serde_json::to_string_pretty(&report)?);
    if let Some(path) = args.dump_onenand { save_new(path, &state.nand.snapshot())?; }
    if let Some(path) = args.dump_ram {
        let mut ram = vec![0; LOADER_RAM_SIZE as usize];
        cpu.mem_read(profile.ram, &mut ram)?;
        save_new(path, &ram)?;
    }
    if let Some(path) = args.dump_sram {
        let mut sram = vec![0; 0x10000];
        cpu.mem_read(profile.sram, &mut sram)?;
        save_new(path, &sram)?;
    }
    result.map_err(|error| anyhow::anyhow!("loader execution stopped with {error:?}"))?;
    Ok(())
}
