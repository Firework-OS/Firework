// Copyright (c) ChefKiss Inc 2021-2023. Licensed under the Thou Shalt Not Profit License version 1.5. See LICENSE for details.

use alloc::{boxed::Box, collections::VecDeque, string::String, vec::Vec};

use amd64::paging::{pml4::PML4, PageTableEntry};
use fireworkkit::msg::Message;
use hashbrown::HashMap;

use super::gdt::{PrivilegeLevel, SegmentSelector};

pub mod scheduler;
pub mod userland;

pub const STACK_SIZE: u64 = 0x14000;

#[derive(PartialEq, Eq)]
pub enum ThreadState {
    Active,
    Inactive,
    Suspended,
}

impl ThreadState {
    #[inline]
    pub fn is_suspended(&self) -> bool {
        *self == Self::Suspended
    }

    #[inline]
    pub fn is_inactive(&self) -> bool {
        *self == Self::Inactive
    }
}

pub struct Thread {
    pub id: u64,
    pub pid: u64,
    pub state: ThreadState,
    pub regs: super::RegisterState,
    pub fs_base: usize,
    pub gs_base: usize,
    pub stack_addr: u64,
}

impl Thread {
    #[inline]
    pub fn new(id: u64, pid: u64, rip: u64, stack_addr: u64) -> Self {
        Self {
            id,
            pid,
            state: ThreadState::Inactive,
            regs: super::RegisterState {
                rip,
                cs: SegmentSelector::new(3, PrivilegeLevel::User).0.into(),
                rflags: 0x202,
                rsp: stack_addr + STACK_SIZE,
                ss: SegmentSelector::new(4, PrivilegeLevel::User).0.into(),
                ..Default::default()
            },
            fs_base: 0,
            gs_base: 0,
            stack_addr,
        }
    }
}

pub struct Process {
    pub id: u64,
    pub path: String,
    pub image_base: u64,
    pub cr3: Box<userland::page_table::UserPML4>,
    pub messages: VecDeque<Message>,
    pub allocations: HashMap<u64, (u64, bool)>,
    pub msg_id_to_addr: HashMap<u64, u64>,
    pub addr_to_msg_id: HashMap<u64, u64>,
    pub thread_ids: Vec<u64>,
}

impl Process {
    #[inline]
    pub fn new(id: u64, path: String, image_base: u64) -> Self {
        Self {
            id,
            path,
            image_base,
            cr3: Box::new(userland::page_table::UserPML4::new(id)),
            messages: VecDeque::new(),
            allocations: HashMap::new(),
            msg_id_to_addr: HashMap::new(),
            addr_to_msg_id: HashMap::new(),
            thread_ids: vec![],
        }
    }

    pub fn track_alloc(&mut self, addr: u64, size: u64, writable: Option<bool>) {
        let page_count = (size + 0xFFF) / 0x1000;
        trace!(
            "PID {}: Tracking {addr:#X} ({page_count} pages, {size} bytes)",
            self.id
        );
        self.allocations.insert(addr, (size, writable.is_some()));

        let Some(writable) = writable else {
            return;
        };

        unsafe {
            self.cr3.map_pages(
                addr,
                addr - fireworkkit::USER_PHYS_VIRT_OFFSET,
                page_count,
                PageTableEntry::new()
                    .with_present(true)
                    .with_writable(writable)
                    .with_user(true),
            );
        }
    }

    pub fn track_kernelside_alloc(&mut self, addr: u64, size: u64) -> u64 {
        let addr = addr - amd64::paging::PHYS_VIRT_OFFSET + fireworkkit::USER_PHYS_VIRT_OFFSET;
        self.track_alloc(addr, size, Some(false));
        addr
    }

    pub fn free_alloc(&mut self, addr: u64) {
        let (size, mapped) = self.allocations.remove(&addr).unwrap();
        let page_count = (size + 0xFFF) / 0x1000;
        trace!(
            "PID {}: Freeing {addr:#X} ({page_count} pages, {size} bytes)",
            self.id
        );

        unsafe {
            (*crate::system::state::SYS_STATE.get())
                .pmm
                .as_mut()
                .unwrap()
                .lock()
                .free(
                    (addr - fireworkkit::USER_PHYS_VIRT_OFFSET) as *mut _,
                    page_count,
                );
        }

        if mapped {
            unsafe { self.cr3.unmap_pages(addr, page_count) }
        }
    }

    pub fn track_msg(&mut self, id: u64, addr: u64) {
        trace!("PID {}: Marking {addr:#X} as message {id}", self.id);
        self.msg_id_to_addr.insert(id, addr);
        self.addr_to_msg_id.insert(addr, id);
    }

    pub fn free_msg(&mut self, id: u64) {
        trace!("PID {}: Freeing message {id}", self.id);
        let addr = self.msg_id_to_addr.remove(&id).unwrap();
        self.addr_to_msg_id.remove(&addr).unwrap();
        self.free_alloc(addr);
    }

    pub fn is_msg(&self, addr: u64) -> bool {
        self.addr_to_msg_id.contains_key(&addr)
    }

    pub fn allocate(&mut self, size: u64) -> (u64, u64) {
        let page_count = (size + 0xFFF) / 0x1000;
        let addr = unsafe {
            (*crate::system::state::SYS_STATE.get())
                .pmm
                .as_mut()
                .unwrap()
                .lock()
                .alloc(page_count)
                .unwrap() as u64
        };
        let virt = addr + fireworkkit::USER_PHYS_VIRT_OFFSET;
        self.track_alloc(virt, size, Some(true));
        (virt, page_count)
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        let addrs: Vec<_> = self.allocations.keys().copied().collect();
        for addr in addrs {
            self.free_alloc(addr);
        }
    }
}
