use unicorn_engine::unicorn_const::{Arch, Mode};
pub use unicorn_engine::unicorn_const::{HookType, Prot as EngineProt};
pub use unicorn_engine::{RegisterARM, Unicorn};

/// Only the ARM946 profile used by the E830 board. Architectural MMU behavior
/// matches the original board path; no application loader or virtual TLB exists.
pub struct Arm946Cpu<D: 'static> {
    engine: Unicorn<'static, D>,
}

/// TCM capacities from the E830XEGG1 boot region table: 16 KiB instruction
/// memory at zero and 16 KiB data memory at 0x50600000. Unicorn's ARM946
/// implementation returns zero for TCMTR and does not implement these banks.
pub const E830_TCM_SIZE: u64 = 0x4000;
pub const E830_DTCM_BASE: u64 = 0x5060_0000;
const E830_TCMTR: u32 = (5 << 18) | (5 << 6);

#[derive(Default)]
pub struct TcmRegisters {
    regions: [u32; 2],
}

impl TcmRegisters {
    /// Recognize ARM MRC/MCR instructions for TCM identification and control.
    /// No firmware addresses or replacement guest instructions are involved.
    pub fn recognizes(instruction: u32) -> bool {
        let operation = instruction & 0x0fff_0fff;
        operation == 0x0e10_0f50 // MRC p15,0,Rd,c0,c0,2 (TCMTR)
            || matches!(operation, 0x0e09_0f11 | 0x0e09_0f31 | 0x0e19_0f11 | 0x0e19_0f31)
    }

    pub fn execute(&mut self, instruction: u32, value: u32) -> Option<u32> {
        let operation = instruction & 0x0fff_0fff;
        if operation == 0x0e10_0f50 {
            return Some(E830_TCMTR);
        }
        let index = ((instruction >> 5) & 1) as usize;
        if instruction & (1 << 20) != 0 {
            Some(self.regions[index])
        } else {
            self.regions[index] = value;
            None
        }
    }
}

pub fn arm_condition_passed(condition: u32, cpsr: u32) -> bool {
    let n = cpsr & (1 << 31) != 0;
    let z = cpsr & (1 << 30) != 0;
    let c = cpsr & (1 << 29) != 0;
    let v = cpsr & (1 << 28) != 0;
    match condition {
        0 => z, 1 => !z, 2 => c, 3 => !c, 4 => n, 5 => !n,
        6 => v, 7 => !v, 8 => c && !z, 9 => !c || z,
        10 => n == v, 11 => n != v, 12 => !z && n == v,
        13 => z || n != v, 14 => true, _ => false,
    }
}

impl<D: 'static> Arm946Cpu<D> {
    pub fn new_for_arm946_with_data(data: D) -> Result<Self, unicorn_engine::unicorn_const::uc_error> {
        let mut engine = Unicorn::new_with_data(Arch::ARM, Mode::ARM946 | Mode::LITTLE_ENDIAN, data)?;
        // Preserve the original ARM backend's initialization. Unsupported
        // coprocessor registers are ignored by that backend as well.
        let _ = engine.reg_write(RegisterARM::FPEXC, 0x4000_0000);
        let _ = engine.reg_write(RegisterARM::C1_C0_2, 0x00f0_0000);
        Ok(Self { engine })
    }

    pub fn raw_engine(&mut self) -> &mut Unicorn<'static, D> {
        &mut self.engine
    }
}
