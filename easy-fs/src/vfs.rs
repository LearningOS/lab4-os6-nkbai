use super::{
    BlockDevice,
    DiskInode,
    DiskInodeType,
    DirEntry,
    EasyFileSystem,
    DIRENT_SZ,
    get_block_cache,
    block_cache_sync_all,
};
use alloc::sync::Arc;
use alloc::string::String;
use alloc::vec::Vec;
use spin::{Mutex, MutexGuard};
use crate::{BLOCK_SZ, println};


/// Virtual filesystem layer over easy-fs
pub struct Inode {
    block_id: usize,
    block_offset: usize,
    fs: Arc<Mutex<EasyFileSystem>>,
    block_device: Arc<dyn BlockDevice>,
    inode_id: u32,
}

impl Inode {
    /// Create a vfs inode
    pub fn new(
        block_id: u32,
        block_offset: usize,
        fs: Arc<Mutex<EasyFileSystem>>,
        block_device: Arc<dyn BlockDevice>,
        inode_id: u32,
    ) -> Self {
        Self {
            block_id: block_id as usize,
            block_offset,
            fs,
            block_device,
            inode_id,
        }
    }
    /// Call a function over a disk inode to read it
    fn read_disk_inode<V>(&self, f: impl FnOnce(&DiskInode) -> V) -> V {
        get_block_cache(
            self.block_id,
            Arc::clone(&self.block_device),
        ).lock().read(self.block_offset, f)
    }
    /// Call a function over a disk inode to modify it
    fn modify_disk_inode<V>(&self, f: impl FnOnce(&mut DiskInode) -> V) -> V {
        get_block_cache(
            self.block_id,
            Arc::clone(&self.block_device),
        ).lock().modify(self.block_offset, f)
    }
    /// Find inode under a disk inode by name
    fn find_inode_id(
        &self,
        name: &str,
        disk_inode: &DiskInode,
    ) -> Option<u32> {
        // assert it is a directory
        assert!(disk_inode.is_dir());
        let file_count = (disk_inode.size as usize) / DIRENT_SZ;
        let mut dirent = DirEntry::empty();
        for i in 0..file_count {
            assert_eq!(
                disk_inode.read_at(
                    DIRENT_SZ * i,
                    dirent.as_bytes_mut(),
                    &self.block_device,
                ),
                DIRENT_SZ,
            );
            if dirent.name() == name {
                return Some(dirent.inode_number() as u32);
            }
        }
        None
    }
    //采用交换策略,保证目录下面至少有一个文件,否则panic
    fn remove_dir_entry(&self, name: &str, disk_inode: &mut DiskInode) {
        assert!(disk_inode.is_dir());
        let file_count = (disk_inode.size as usize) / DIRENT_SZ;
        let mut dirent = DirEntry::empty();
        let mut index = -1;
        for i in 0..file_count {
            assert_eq!(
                disk_inode.read_at(
                    DIRENT_SZ * i,
                    dirent.as_bytes_mut(),
                    &self.block_device,
                ),
                DIRENT_SZ,
            );
            if dirent.name() == name {
                index = i as i32;
                break;
            }
        }
        if index < 0 {
            panic!("no such file");
        }

        //第一步,先将最后一个搬到index位置,
        let mut last_dir_entry = DirEntry::empty();
        assert_eq!(
            disk_inode.read_at(DIRENT_SZ * (file_count - 1), last_dir_entry.as_bytes_mut(), &self.block_device),
            DIRENT_SZ);
        disk_inode.write_at(DIRENT_SZ * index  as usize, last_dir_entry.as_bytes(), &self.block_device);
        //第二步,清空最后一个
        disk_inode.write_at(
            DIRENT_SZ * (file_count - 1),
            &[0; DIRENT_SZ],
            &self.block_device,
        );
        //最后修改大小
        self.decrease_size(disk_inode.size-DIRENT_SZ as u32 , disk_inode);
    }

    /// Find inode under current inode by name
    pub fn find(&self, name: &str) -> Option<Arc<Inode>> {
        let fs = self.fs.lock();
        self.read_disk_inode(|disk_inode| {
            self.find_inode_id(name, disk_inode)
                .map(|inode_id| {
                    let (block_id, block_offset) = fs.get_disk_inode_pos(inode_id);
                    Arc::new(Self::new(
                        block_id,
                        block_offset,
                        self.fs.clone(),
                        self.block_device.clone(),
                        inode_id,
                    ))
                })
        })
    }
    /// Increase the size of a disk inode
    fn increase_size(
        &self,
        new_size: u32,
        disk_inode: &mut DiskInode,
        fs: &mut MutexGuard<EasyFileSystem>,
    ) {
        if new_size < disk_inode.size {
            return;
        }
        let blocks_needed = disk_inode.blocks_num_needed(new_size);
        let mut v: Vec<u32> = Vec::new();
        for _ in 0..blocks_needed {
            v.push(fs.alloc_data());
        }
        disk_inode.increase_size(new_size, v, &self.block_device);
    }
    fn decrease_size(&self, new_size: u32, disk_inode: &mut DiskInode) {
        if new_size > disk_inode.size {
            panic!("new size is bigger than old size");
        }

        let new_blocks_size = DiskInode::total_blocks(new_size);
        let old_blocks_size = DiskInode::total_blocks(disk_inode.size);
        disk_inode.size=  new_size;
        assert_eq!(new_blocks_size, old_blocks_size, "truncation should not change block size");
    }
    /// Create inode under current inode by name
    pub fn create(&self, name: &str) -> Option<Arc<Inode>> {
        let mut fs = self.fs.lock();
        if self.modify_disk_inode(|root_inode| {
            // assert it is a directory
            assert!(root_inode.is_dir());
            // has the file been created?
            self.find_inode_id(name, root_inode)
        }).is_some() {
            return None;
        }
        // create a new file
        // alloc a inode with an indirect block
        let new_inode_id = fs.alloc_inode();
        // initialize inode
        let (new_inode_block_id, new_inode_block_offset)
            = fs.get_disk_inode_pos(new_inode_id);
        get_block_cache(
            new_inode_block_id as usize,
            Arc::clone(&self.block_device),
        ).lock().modify(new_inode_block_offset, |new_inode: &mut DiskInode| {
            new_inode.initialize(DiskInodeType::File);
        });
        self.modify_disk_inode(|root_inode| {
            // append file in the dirent
            let file_count = (root_inode.size as usize) / DIRENT_SZ;
            let new_size = (file_count + 1) * DIRENT_SZ;
            // increase size
            self.increase_size(new_size as u32, root_inode, &mut fs);
            // write dirent
            let dirent = DirEntry::new(name, new_inode_id);
            root_inode.write_at(
                file_count * DIRENT_SZ,
                dirent.as_bytes(),
                &self.block_device,
            );
        });

        let (block_id, block_offset) = fs.get_disk_inode_pos(new_inode_id);
        block_cache_sync_all();
        // return inode
        Some(Arc::new(Self::new(
            block_id,
            block_offset,
            self.fs.clone(),
            self.block_device.clone(),
            new_inode_id,
        )))
        // release efs lock automatically by compiler
    }
    /// List inodes under current inode
    pub fn ls(&self) -> Vec<String> {
         let _fs = self.fs.lock();
        self.read_disk_inode(|disk_inode| {
            let file_count = (disk_inode.size as usize) / DIRENT_SZ;
            let mut v: Vec<String> = Vec::new();
            for i in 0..file_count {
                let mut dirent = DirEntry::empty();
                assert_eq!(
                    disk_inode.read_at(
                        i * DIRENT_SZ,
                        dirent.as_bytes_mut(),
                        &self.block_device,
                    ),
                    DIRENT_SZ,
                );
                v.push(String::from(dirent.name()));
            }
            v
        })
    }
    /// 给出这个inode对应的inode以及hardlink,.0 是inode,.1是nlink
    pub fn stat(&self) -> (u32, u32, DiskInodeType) {
        let _fs = self.fs.lock();
        let mut nlink = 0;
        let mut typ = DiskInodeType::File;
        self.read_disk_inode(|disk_inode| {
            if disk_inode.hard_link==0 {
                println!("stat  hard_link is 0");
            }
            nlink = disk_inode.hard_link;
            typ = disk_inode.INodeType();
        });

        return (self.inode_id, nlink, typ);
    }
    /// Read data from current inode
    pub fn read_at(&self, offset: usize, buf: &mut [u8]) -> usize {
        let _fs = self.fs.lock();
        self.read_disk_inode(|disk_inode| {
            disk_inode.read_at(offset, buf, &self.block_device)
        })
    }
    /// Write data to current inode
    pub fn write_at(&self, offset: usize, buf: &[u8]) -> usize {
         let mut fs = self.fs.lock();
        let size = self.modify_disk_inode(|disk_inode| {
            self.increase_size((offset + buf.len()) as u32, disk_inode, &mut fs);
            disk_inode.write_at(offset, buf, &self.block_device)
        });
        block_cache_sync_all();
        size
    }
    pub fn clear(&self) {
        let mut fs = self.fs.lock();
         self.clearInternal(&mut fs);
        block_cache_sync_all();
    }
    /// Clear the data in current inode
      fn clearInternal(&self,fs:&mut MutexGuard<EasyFileSystem>) {
         self.modify_disk_inode(|disk_inode| {
             // println!("clear");
             // println!("actually clear");
            let size = disk_inode.size;
             // println!("size={},block_size={}",size,BLOCK_SZ);
            let data_blocks_dealloc = disk_inode.clear_size(&self.block_device);
            assert!(data_blocks_dealloc.len() == DiskInode::total_blocks(size) as usize);
             // println!("clearInternal :{:?}",data_blocks_dealloc);
            for data_block in data_blocks_dealloc.into_iter() {
                fs.dealloc_data(data_block);
            }
        });
    }
    pub fn link(&self, old_name: &str, new_name: &str) -> Option<()> {
        // println!("link {} {}",old_name,new_name);
        let old_inode = self.find(old_name)?;
        let mut fs = self.fs.lock();
        self.modify_disk_inode(|root_inode| {
            let file_count = (root_inode.size as usize) / DIRENT_SZ;
            let new_size = (file_count + 1) * DIRENT_SZ;
            self.increase_size(new_size as u32, root_inode, &mut fs);
            let dirent = DirEntry::new(new_name, old_inode.inode_id);
            let mut hard_link=0;
            old_inode.modify_disk_inode(|disk_inode| {
                disk_inode.hard_link += 1;
                hard_link = disk_inode.hard_link;
            });
            root_inode.write_at(
                file_count * DIRENT_SZ,
                dirent.as_bytes(),
                &self.block_device,
            );
            // println!("{} link={}",old_name,hard_link);
        });
        block_cache_sync_all();
        Some(())
    }
    pub fn unlink(&self, name: &str) -> Option<()> {
        // println!("unlink {}", name);

        let old_inode=if let Some(old_inode) = self.find(name) {
            old_inode
        }else{
            // println!("unlink {} not found", name);
            return None;
        };
        let mut fs = self.fs.lock();
        // println!("unlink {}: {}",name, old_inode.inode_id);
        //1. 先从目录中移除这一项
        self.modify_disk_inode(|root_inode| {
            self.remove_dir_entry(name,root_inode);
        });
        let mut need_clear=false ;
        let mut hard_link=0;
        old_inode.modify_disk_inode(|disk_inode| {
            disk_inode.hard_link -= 1;
            if disk_inode.hard_link<=0 {
                need_clear=true;
            }
            hard_link = disk_inode.hard_link;
        });
        // println!("{} unlink={}",name,hard_link);
        //2. 然后删除文件的数据块
        if need_clear{
            old_inode.clearInternal(&mut fs );
        }
        block_cache_sync_all();
        Some(())
    }
}
