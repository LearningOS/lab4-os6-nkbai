#![no_std]
#![feature(core_panic)]
#![feature(panic_info_message)]
extern crate alloc;

mod block_dev;
mod layout;
mod efs;
mod bitmap;
mod vfs;
mod block_cache;

#[macro_use]
mod console;

/// Use a block size of 512 bytes
pub const BLOCK_SZ: usize = 512;
pub use block_dev::BlockDevice;
pub use efs::EasyFileSystem;
pub use vfs::Inode;
pub use layout::DiskInodeType;
use layout::*;
use bitmap::Bitmap;
use block_cache::{get_block_cache, block_cache_sync_all};
pub use console::set_console_putchar;

pub fn hello_world_in_easy_fs() {
    println!("Hello, world in easy fs!");
}