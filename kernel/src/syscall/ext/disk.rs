use super::*;

pub(in crate::syscall) const SYS_HAMIX_DISK_LIST: u64 = 9070;
pub(in crate::syscall) const SYS_HAMIX_DISK_IO: u64 = 9071;
pub(in crate::syscall) const SYS_HAMIX_DISK_RESCAN: u64 = 9072;
pub(in crate::syscall) const SYS_HAMIX_MKFS: u64 = 9073;
pub(in crate::syscall) const SYS_HAMIX_MOUNT: u64 = 9074;
pub(in crate::syscall) const SYS_HAMIX_UMOUNT: u64 = 9075;

pub(super) fn dispatch(number: u64, a1: u64, a2: u64, a3: u64, a4: u64, a5: u64, _a6: u64) -> Option<i64> {
    Some(match number {
        SYS_HAMIX_DISK_LIST => copy_out(a1, a2, disk_listing().as_bytes()),
        SYS_HAMIX_DISK_IO => sys_disk_io(a1, a2, a3, a4, a5),
        SYS_HAMIX_DISK_RESCAN => {
            block::rescan();
            fs::refresh_block_nodes();
            0
        }
        SYS_HAMIX_MKFS => sys_mkfs(a1, a2),
        SYS_HAMIX_MOUNT => sys_mount(a1, a2),
        SYS_HAMIX_UMOUNT => sys_umount(a1),
        _ => return None,
    })
}

pub(in crate::syscall) fn disk_listing() -> String {
    let mut text = String::new();
    let infos = block::volume_infos();
    for (index, disk) in block::disks().iter().enumerate() {
        let whole = infos.iter().find(|v| v.volume.disk == index && v.partition.is_none());
        let probe = whole.and_then(|w| w.probe.clone());
        text.push_str(&format!(
            "disk\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
            disk.name,
            disk.sectors,
            disk.bus,
            disk.model,
            probe.as_ref().map(|p| p.kind).unwrap_or(""),
            probe.as_ref().map(|p| p.label.as_str()).unwrap_or(""),
            probe.as_ref().map(|p| p.uuid.as_str()).unwrap_or(""),
            fs::hextfs::mount_path_of(index, 0).unwrap_or_default()
        ));
        for info in infos.iter().filter(|v| v.volume.disk == index) {
            let Some(part) = &info.partition else {
                continue;
            };
            text.push_str(&format!(
                "part\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
                part.name,
                disk.name,
                part.start,
                part.sectors,
                part.kind,
                info.probe.as_ref().map(|p| p.kind).unwrap_or(""),
                info.probe.as_ref().map(|p| p.label.as_str()).unwrap_or(""),
                info.probe.as_ref().map(|p| p.uuid.as_str()).unwrap_or(""),
                fs::hextfs::mount_path_of(index, part.start).unwrap_or_default(),
                if part.bootable { "boot" } else { "" }
            ));
        }
    }
    text
}

pub(in crate::syscall) fn sys_disk_io(name_ptr: u64, lba: u64, buf: u64, sectors: u64, write: u64) -> i64 {
    if let Err(e) = require_root() {
        return e;
    }
    if sectors == 0 || sectors > 4096 {
        return EINVAL;
    }
    let name = match user_cstr(name_ptr) {
        Ok(n) => n,
        Err(e) => return e,
    };
    let Some(volume) = block::find_volume(&name) else {
        return ENODEV;
    };
    if lba + sectors > volume.sectors {
        return EINVAL;
    }
    let bytes = (sectors * 512) as usize;
    let user = match user_slice(buf, bytes as u64) {
        Ok(b) => b,
        Err(e) => return e,
    };
    let absolute_lba = volume.offset + lba;
    if write != 0 {
        if fs::hextfs::overlaps_mounted(volume.disk, absolute_lba, sectors) {
            return EBUSY;
        }
        let data = match crate::memory::try_copy(user) {
            Ok(data) => data,
            Err(e) => return e,
        };
        if block::write(volume.disk, absolute_lba, &data).is_err() {
            return EIO;
        }
        let _ = block::flush(volume.disk);
        0
    } else {
        let mut data = match crate::memory::try_zeroed(bytes) {
            Ok(data) => data,
            Err(e) => return e,
        };
        if block::read(volume.disk, absolute_lba, &mut data).is_err() {
            return EIO;
        }
        match user_slice_mut(buf, bytes as u64) {
            Ok(out) => {
                out.copy_from_slice(&data);
                0
            }
            Err(e) => e,
        }
    }
}

pub(in crate::syscall) fn sys_mkfs(dev_ptr: u64, label_ptr: u64) -> i64 {
    if let Err(e) = require_root() {
        return e;
    }
    let (dev, label) = match (user_cstr(dev_ptr), user_cstr(label_ptr)) {
        (Ok(d), Ok(l)) => (d, l),
        (Err(e), _) | (_, Err(e)) => return e,
    };
    match fs::hextfs::mkfs(&dev, &label) {
        Ok(msg) => {
            crate::drivers::klog::log(&format!("mkfs: {}", msg));
            0
        }
        Err("no such block device") => ENODEV,
        Err("the device is mounted") => EBUSY,
        Err(_) => EIO,
    }
}

pub(in crate::syscall) fn sys_mount(dev_ptr: u64, path_ptr: u64) -> i64 {
    if let Err(e) = require_root() {
        return e;
    }
    let (dev, path) = match (user_cstr(dev_ptr), path_arg(path_ptr)) {
        (Ok(d), Ok(p)) => (d, p),
        (Err(e), _) | (_, Err(e)) => return e,
    };
    match fs::hextfs::mount(&dev, &path) {
        Ok(msg) => {
            crate::drivers::klog::log(&format!("mount: {}", msg));
            0
        }
        Err("no such block device") => ENODEV,
        Err("the device is already mounted") | Err("something is already mounted there") => EBUSY,
        Err("mount point does not exist") => ENOENT,
        Err("mount point must be a directory other than /") => ENOTDIR,
        Err("no hext filesystem on the device") => EINVAL,
        Err(_) => EIO,
    }
}

pub(in crate::syscall) fn sys_umount(path_ptr: u64) -> i64 {
    if let Err(e) = require_root() {
        return e;
    }
    let path = match path_arg(path_ptr) {
        Ok(p) => p,
        Err(e) => return e,
    };
    match fs::hextfs::umount(&path) {
        Ok(()) => 0,
        Err("not mounted") => EINVAL,
        Err(_) => EBUSY,
    }
}
