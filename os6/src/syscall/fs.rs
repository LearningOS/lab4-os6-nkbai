//! File and filesystem-related syscalls

use alloc::sync::Arc;
use core::mem::size_of;

use crate::fs::{File, linkat, open_file, OSInode, StatMode, Stdin, unlinkat};
use crate::fs::make_pipe;
use crate::fs::OpenFlags;
use crate::fs::Stat;
use crate::mm::translated_byte_buffer;
use crate::mm::translated_refmut;
use crate::mm::translated_str;
use crate::mm::UserBuffer;
use crate::task::current_task;
use crate::task::current_user_token;

pub fn sys_write(fd: usize, buf: *const u8, len: usize) -> isize {
    let token = current_user_token();
    let task = current_task().unwrap();
    let inner = task.inner_exclusive_access();
    if fd >= inner.fd_table.len() {
        return -1;
    }
    if let Some(file) = &inner.fd_table[fd] {
        let file = file.clone();
        // release current task TCB manually to avoid multi-borrow
        drop(inner);
        file.write(
            UserBuffer::new(translated_byte_buffer(token, buf, len))
        ) as isize
    } else {
        -1
    }
}

pub fn sys_read(fd: usize, buf: *const u8, len: usize) -> isize {
    let token = current_user_token();
    let task = current_task().unwrap();
    let inner = task.inner_exclusive_access();
    if fd >= inner.fd_table.len() {
        return -1;
    }
    if let Some(file) = &inner.fd_table[fd] {
        let file = file.clone();
        // release current task TCB manually to avoid multi-borrow
        drop(inner);
        file.read(
            UserBuffer::new(translated_byte_buffer(token, buf, len))
        ) as isize
    } else {
        -1
    }
}

pub fn sys_open(path: *const u8, flags: u32) -> isize {
    let task = current_task().unwrap();
    let token = current_user_token();
    let path = translated_str(token, path);
    // println!("sys_open: {}", path);
    let ret = if let Some(inode) = open_file(
        path.as_str(),
        OpenFlags::from_bits(flags).unwrap(),
    ) {
        let mut inner = task.inner_exclusive_access();
        let fd = inner.alloc_fd();
        inner.fd_table[fd] = Some(inode);
        fd as isize
    } else {
        -1
    };
    // println!("sys_open {} ret={}", path, ret);
    ret
}

pub fn sys_close(fd: usize) -> isize {
    let task = current_task().unwrap();
    let mut inner = task.inner_exclusive_access();
    if fd >= inner.fd_table.len() {
        return -1;
    }
    if inner.fd_table[fd].is_none() {
        return -1;
    }
    inner.fd_table[fd].take();
    0
}

pub fn sys_pipe(pipe: *mut usize) -> isize {
    let task = current_task().unwrap();
    let token = current_user_token();
    let mut inner = task.inner_exclusive_access();
    let (pipe_read, pipe_write) = make_pipe();
    let read_fd = inner.alloc_fd();
    inner.fd_table[read_fd] = Some(pipe_read);
    let write_fd = inner.alloc_fd();
    inner.fd_table[write_fd] = Some(pipe_write);
    *translated_refmut(token, pipe) = read_fd;
    *translated_refmut(token, unsafe { pipe.add(1) }) = write_fd;
    0
}

pub fn sys_dup(fd: usize) -> isize {
    let task = current_task().unwrap();
    let mut inner = task.inner_exclusive_access();
    if fd >= inner.fd_table.len() {
        return -1;
    }
    if inner.fd_table[fd].is_none() {
        return -1;
    }
    let new_fd = inner.alloc_fd();
    inner.fd_table[new_fd] = Some(Arc::clone(inner.fd_table[fd].as_ref().unwrap()));
    new_fd as isize
}

/**
硬链接的基本思路是,同一个inode,在多个文件夹里出现.
问题是unlink的时候,要检测到inode还在另一个文件夹中被引用, 所以diskinode肯定要知道这种情况.
 */

pub fn sys_fstat(fd: usize, st: *mut Stat) -> isize {
    let buffers = translated_byte_buffer(current_user_token(), st as *mut u8, size_of::<Stat>());
    assert_eq!(1, buffers.len());
    let st = unsafe { (buffers[0].as_ptr() as *mut Stat).as_mut().unwrap() };
    //剩下来的就是普通文件,文件夹了.
    let task = current_task().unwrap();
    let mut inner = task.inner_exclusive_access();
    if fd >= inner.fd_table.len() {
        return -1;
    }
    *st = inner.fd_table[fd].as_ref().unwrap().stat();
    0
}

pub fn sys_linkat(old_name: *const u8, new_name: *const u8) -> isize {
    let token = current_user_token();
    let path_old = translated_str(token, old_name);
    let path_new = translated_str(token, new_name);
    if path_new == path_old {
        return -1;
    }
    // println!("linkat {} {} start", path_old, path_new);
    if let Some(_) = linkat(&path_old, &path_new) {
        // println!("linkat {} {} success", path_old, path_new);
        return 0;
    }
    println!("linkat {} {} failed", path_old, path_new);
    -1
}

pub fn sys_unlinkat(name: *const u8) -> isize {
    let token = current_user_token();
    let path = translated_str(token, name);

    if let Some(_) = unlinkat(&path) {
        // println!("unlinkat {} success", path);
        return 0;
    }
    println!("unlinkat {} failed", path);
    -1
}
