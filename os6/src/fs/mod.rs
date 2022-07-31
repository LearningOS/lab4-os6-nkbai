use alloc::sync::Arc;
use core::any::{Any, TypeId};

pub use inode::{linkat, list_apps, open_file, OpenFlags, OSInode, unlinkat};
pub use pipe::{make_pipe, Pipe};
pub use stdio::{Stdin, Stdout};

use crate::mm::UserBuffer;

mod stdio;
mod inode;
mod pipe;

/// The common abstraction of all IO resources
pub trait File: Send + Sync {
    fn readable(&self) -> bool;
    fn writable(&self) -> bool;
    fn read(&self, buf: UserBuffer) -> usize;
    fn write(&self, buf: UserBuffer) -> usize;
    fn stat(&self) -> Stat;
}


/// The stat of a inode
#[repr(C)]
#[derive(Debug)]
pub struct Stat {
    /// ID of device containing file
    pub dev: u64,
    /// inode number
    pub ino: u64,
    /// file type and mode
    pub mode: StatMode,
    /// number of hard links
    pub nlink: u32,
    /// unused pad
    pad: [u64; 7],
}

impl Stat {
    pub fn new(inode: usize, mode: StatMode, nlink: u32) -> Self {
        Stat {
            dev: 0,
            ino: inode as u64,
            mode,
            nlink,
            pad: [0; 7],
        }
    }
}
bitflags! {
    /// The mode of a inode
    /// whether a directory or a file
    pub struct StatMode: u32 {
        const NULL  = 0;
        /// directory
        const DIR   = 0o040000;
        /// ordinary regular file
        const FILE  = 0o100000;
    }
}

