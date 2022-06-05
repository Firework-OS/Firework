//! Copyright (c) VisualDevelopment 2021-2022.
//! This project is licensed by the Creative Commons Attribution-NoCommercial-NoDerivatives licence.

use alloc::vec::Vec;

use acpi::tables::madt::ic::{
    ioapic::{IoApic, Iso},
    proc_lapic::ProcessorLocalApic,
    InterruptController,
};
use log::debug;

pub struct Madt {
    pub proc_lapics: Vec<&'static ProcessorLocalApic>,
    pub ioapics: Vec<&'static IoApic>,
    pub isos: Vec<&'static Iso>,
    pub lapic_addr: u64,
}

impl Madt {
    pub fn new(madt: &'static acpi::tables::madt::Madt) -> Self {
        // Disable PIC
        if madt.flags.pcat_compat() {
            amd64::intrs::pic::Pic::new().remap_and_disable();
        }

        let mut proc_lapics = Vec::new();
        let mut ioapics = Vec::new();
        let mut isos = Vec::new();
        let mut lapic_addr = madt.local_ic_addr();

        for ent in madt.into_iter() {
            match ent {
                InterruptController::ProcessorLocalApic(lapic) => {
                    debug!("Found Local APIC: {:#X?}", lapic);
                    proc_lapics.push(lapic);
                }
                InterruptController::IoApic(ioapic) => {
                    debug!(
                        "Found I/O APIC with ver {:#X?}: {:#X?}",
                        ioapic.read_ver(),
                        ioapic,
                    );
                    ioapics.push(ioapic);
                }
                InterruptController::Iso(iso) => {
                    debug!("Found Interrupt Source Override: {:#X?}", iso);
                    isos.push(iso);
                }
                InterruptController::LocalApicAddrOverride(a) => {
                    debug!("Found Local APIC Address Override: {:#X?}", a);
                    lapic_addr = a.addr;
                }
                rest => debug!("Ignoring {:X?}", rest),
            }
        }

        Self {
            proc_lapics,
            ioapics,
            isos,
            lapic_addr,
        }
    }
}
