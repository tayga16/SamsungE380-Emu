//! Explicit diagnostic calls to original firmware; used only by the CLI.
//! Buffer writes are restricted to memory returned by the guest allocator.
use std::collections::HashMap;
use std::path::Path;

use anyhow::{bail, Context, Result};
use e830::{E830Machine, StopReason, RAM_BASE, RAM_SIZE};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(untagged)]
enum Argument { Number(u32), Reference(String) }

#[derive(Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
enum Step {
    Read { address: Argument, name: String },
    Allocate { function: Argument, size: u32, name: String,
               #[serde(default)] args: Option<Vec<Argument>> },
    WriteBuffer { name: String, #[serde(default)] offset: u32, hex: String },
    Call { function: Argument, #[serde(default)] args: Vec<Argument>,
           #[serde(default)] name: Option<String>, #[serde(default)] expect: Option<u32>,
           #[serde(default = "call_budget")] budget: u64 },
}

fn call_budget() -> u64 { 2_000_000 }

fn resolve(argument: &Argument, values: &HashMap<String, u32>) -> Result<u32> {
    match argument {
        Argument::Number(number) => Ok(*number),
        Argument::Reference(raw) => {
            if let Some(hex) = raw.strip_prefix("0x") { return Ok(u32::from_str_radix(hex, 16)?); }
            let (name, offset) = raw.split_once('+').unwrap_or((raw, "0"));
            let value = values.get(name).with_context(|| format!("unknown guest value {name}"))?;
            let offset = if let Some(hex) = offset.strip_prefix("0x") {
                u32::from_str_radix(hex, 16)?
            } else { offset.parse::<u32>()? };
            value.checked_add(offset).context("guest argument address overflow")
        }
    }
}

fn require_return(report: &e830::BootReport) -> Result<()> {
    if report.stop_reason != StopReason::Breakpoint || report.memory_fault.is_some() {
        bail!("original firmware call did not return: {report:#?}");
    }
    Ok(())
}

pub fn execute(machine: &mut E830Machine, path: &Path) -> Result<()> {
    let steps: Vec<Step> = serde_json::from_slice(&std::fs::read(path)?).context("reading firmware-call script")?;
    let mut values = HashMap::new();
    let mut buffers = HashMap::new();
    for (index, step) in steps.into_iter().enumerate() {
        match step {
            Step::Read { address, name } => {
                let address = resolve(&address, &values)?;
                let value = machine.peek_u32(address).context("reading initialized guest value")?;
                println!("step {index}: {name}=0x{value:08x} from 0x{address:08x}");
                values.insert(name, value);
            }
            Step::Allocate { function, size, name, args } => {
                if size == 0 { bail!("guest allocation size is zero"); }
                let function = resolve(&function, &values)?;
                let args = match args {
                    Some(args) => args.iter().map(|argument| resolve(argument, &values)).collect::<Result<Vec<_>>>()?,
                    None => vec![size],
                };
                let report = machine.call_firmware(function, &args, call_budget())?;
                require_return(&report)?;
                if report.r0 < RAM_BASE || u64::from(report.r0) + u64::from(size) > u64::from(RAM_BASE) + u64::from(RAM_SIZE) {
                    bail!("guest allocator returned invalid buffer 0x{:08x}", report.r0);
                }
                println!("step {index}: allocated {name}=0x{:08x}, {size} bytes", report.r0);
                values.insert(name.clone(), report.r0);
                buffers.insert(name, (report.r0, size));
            }
            Step::WriteBuffer { name, offset, hex } => {
                let &(address, size) = buffers.get(&name).context("writes require a guest-allocated buffer")?;
                let compact: String = hex.chars().filter(|character| !character.is_ascii_whitespace()).collect();
                if compact.len() % 2 != 0 || !compact.is_ascii() { bail!("invalid buffer hex data"); }
                let bytes: Vec<u8> = (0..compact.len()).step_by(2)
                    .map(|start| u8::from_str_radix(&compact[start..start + 2], 16))
                    .collect::<Result<_, _>>()?;
                if u64::from(offset) + bytes.len() as u64 > u64::from(size) { bail!("installer data exceeds guest allocation"); }
                machine.write_installer_buffer(address + offset, &bytes)?;
                println!("step {index}: wrote {} bytes to {name}+0x{offset:x}", bytes.len());
            }
            Step::Call { function, args, name, expect, budget } => {
                let function = resolve(&function, &values)?;
                let args: Vec<u32> = args.iter().map(|argument| resolve(argument, &values)).collect::<Result<_>>()?;
                let report = machine.call_firmware(function, &args, budget)?;
                require_return(&report)?;
                println!("step {index}: call 0x{function:08x} => r0=0x{:08x}", report.r0);
                if let Some(expected) = expect {
                    if report.r0 != expected { bail!("original function returned 0x{:08x}, expected 0x{expected:08x}", report.r0); }
                }
                if let Some(name) = name { values.insert(name, report.r0); }
            }
        }
    }
    Ok(())
}
