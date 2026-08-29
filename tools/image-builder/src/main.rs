use std::env;
use std::fs;
use std::io;
use std::path::Path;

const SECTOR_SIZE: usize = 512;
const IMAGE_SECTORS: u64 = 131_072;
const PART_START: u64 = 2_048;
const PART_END: u64 = IMAGE_SECTORS - 34;
const RESERVED_SECTORS: u32 = 32;
const FAT_COUNT: u32 = 2;
const SECTORS_PER_CLUSTER: u32 = 1;

fn main() -> io::Result<()> {
    let args: Vec<String> = env::args().collect();
    if args.len() != 3 {
        eprintln!("usage: image-builder <BOOTX64.EFI> <output.img>");
        std::process::exit(2);
    }

    let efi = fs::read(&args[1])?;
    let out = Path::new(&args[2]);
    if let Some(parent) = out.parent() {
        fs::create_dir_all(parent)?;
    }

    let mut image = vec![0u8; IMAGE_SECTORS as usize * SECTOR_SIZE];
    write_protective_mbr(&mut image);
    write_gpt(&mut image);
    write_fat32_partition(&mut image, &efi);
    fs::write(out, image)?;
    println!("wrote {}", out.display());
    Ok(())
}

fn write_protective_mbr(image: &mut [u8]) {
    let mbr = sector_mut(image, 0);
    mbr[446 + 4] = 0xee;
    put_u32(mbr, 446 + 8, 1);
    put_u32(mbr, 446 + 12, 0xffff_ffff);
    mbr[510] = 0x55;
    mbr[511] = 0xaa;
}

fn write_gpt(image: &mut [u8]) {
    let entries_lba = 2;
    let entry_count = 128u32;
    let entry_size = 128u32;
    let entries_bytes = entry_count as usize * entry_size as usize;
    let backup_entries_lba = IMAGE_SECTORS - 33;

    let mut entries = vec![0u8; entries_bytes];
    let entry = &mut entries[..128];
    entry[0..16].copy_from_slice(&[
        0x28, 0x73, 0x2a, 0xc1, 0x1f, 0xf8, 0xd2, 0x11, 0xba, 0x4b, 0x00, 0xa0, 0xc9, 0x3e,
        0xc9, 0x3b,
    ]);
    entry[16..32].copy_from_slice(&[
        0x52, 0x75, 0x73, 0x74, 0x4f, 0x53, 0x40, 0x50, 0x8a, 0x5d, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x01,
    ]);
    put_u64(entry, 32, PART_START);
    put_u64(entry, 40, PART_END);
    let name = "RustOS POC";
    for (i, unit) in name.encode_utf16().enumerate() {
        put_u16(entry, 56 + i * 2, unit);
    }

    let entries_crc = crc32(&entries);
    write_bytes_at_lba(image, entries_lba, &entries);
    write_bytes_at_lba(image, backup_entries_lba, &entries);

    let primary = make_gpt_header(1, IMAGE_SECTORS - 1, 34, IMAGE_SECTORS - 34, entries_lba, entry_count, entry_size, entries_crc);
    let backup = make_gpt_header(IMAGE_SECTORS - 1, 1, 34, IMAGE_SECTORS - 34, backup_entries_lba, entry_count, entry_size, entries_crc);
    sector_mut(image, 1).copy_from_slice(&primary);
    sector_mut(image, IMAGE_SECTORS - 1).copy_from_slice(&backup);
}

fn make_gpt_header(
    current_lba: u64,
    backup_lba: u64,
    first_usable: u64,
    last_usable: u64,
    entries_lba: u64,
    entry_count: u32,
    entry_size: u32,
    entries_crc: u32,
) -> [u8; SECTOR_SIZE] {
    let mut header = [0u8; SECTOR_SIZE];
    header[0..8].copy_from_slice(b"EFI PART");
    put_u32(&mut header, 8, 0x0001_0000);
    put_u32(&mut header, 12, 92);
    put_u64(&mut header, 24, current_lba);
    put_u64(&mut header, 32, backup_lba);
    put_u64(&mut header, 40, first_usable);
    put_u64(&mut header, 48, last_usable);
    header[56..72].copy_from_slice(&[
        0x52, 0x75, 0x73, 0x74, 0x4f, 0x53, 0x44, 0x69, 0x73, 0x6b, 0x20, 0x20, 0x20, 0x20,
        0x20, 0x20,
    ]);
    put_u64(&mut header, 72, entries_lba);
    put_u32(&mut header, 80, entry_count);
    put_u32(&mut header, 84, entry_size);
    put_u32(&mut header, 88, entries_crc);
    let crc = crc32(&header[..92]);
    put_u32(&mut header, 16, crc);
    header
}

fn write_fat32_partition(image: &mut [u8], efi: &[u8]) {
    let part_sectors = (PART_END - PART_START + 1) as u32;
    let fat_sectors = compute_fat_sectors(part_sectors);
    let data_start = RESERVED_SECTORS + FAT_COUNT * fat_sectors;
    let cluster_count = (part_sectors - data_start) / SECTORS_PER_CLUSTER;
    let fat_entries = (fat_sectors as usize * SECTOR_SIZE) / 4;

    let root_cluster = 2u32;
    let efi_dir_cluster = 3u32;
    let boot_dir_cluster = 4u32;
    let file_start_cluster = 5u32;
    let file_cluster_count = efi.len().div_ceil(SECTOR_SIZE) as u32;

    assert!(cluster_count > 65_525, "partition is too small for FAT32");
    assert!((file_start_cluster + file_cluster_count) < fat_entries as u32, "EFI file is too large for image");

    write_fat32_boot_sector(image, part_sectors, fat_sectors);
    write_fsinfo(image, cluster_count, file_start_cluster + file_cluster_count);
    let backup = sector(image, PART_START).to_vec();
    sector_mut(image, PART_START + 6).copy_from_slice(&backup);

    let mut fat = vec![0u8; fat_sectors as usize * SECTOR_SIZE];
    fat_set(&mut fat, 0, 0x0fff_fff8);
    fat_set(&mut fat, 1, 0xffff_ffff);
    fat_set(&mut fat, root_cluster, 0x0fff_ffff);
    fat_set(&mut fat, efi_dir_cluster, 0x0fff_ffff);
    fat_set(&mut fat, boot_dir_cluster, 0x0fff_ffff);
    for i in 0..file_cluster_count {
        let cluster = file_start_cluster + i;
        let next = if i + 1 == file_cluster_count { 0x0fff_ffff } else { cluster + 1 };
        fat_set(&mut fat, cluster, next);
    }

    let fat1_lba = PART_START + RESERVED_SECTORS as u64;
    let fat2_lba = fat1_lba + fat_sectors as u64;
    write_bytes_at_lba(image, fat1_lba, &fat);
    write_bytes_at_lba(image, fat2_lba, &fat);

    let mut root = [0u8; SECTOR_SIZE];
    write_dir_entry(&mut root, 0, b"EFI        ", 0x10, efi_dir_cluster, 0);
    write_cluster(image, data_start, root_cluster, &root);

    let mut efi_dir = [0u8; SECTOR_SIZE];
    write_dir_entry(&mut efi_dir, 0, b".          ", 0x10, efi_dir_cluster, 0);
    write_dir_entry(&mut efi_dir, 1, b"..         ", 0x10, root_cluster, 0);
    write_dir_entry(&mut efi_dir, 2, b"BOOT       ", 0x10, boot_dir_cluster, 0);
    write_cluster(image, data_start, efi_dir_cluster, &efi_dir);

    let mut boot_dir = [0u8; SECTOR_SIZE];
    write_dir_entry(&mut boot_dir, 0, b".          ", 0x10, boot_dir_cluster, 0);
    write_dir_entry(&mut boot_dir, 1, b"..         ", 0x10, efi_dir_cluster, 0);
    write_dir_entry(&mut boot_dir, 2, b"BOOTX64 EFI", 0x20, file_start_cluster, efi.len() as u32);
    write_cluster(image, data_start, boot_dir_cluster, &boot_dir);

    for i in 0..file_cluster_count as usize {
        let start = i * SECTOR_SIZE;
        let end = std::cmp::min(start + SECTOR_SIZE, efi.len());
        let mut cluster = [0u8; SECTOR_SIZE];
        cluster[..end - start].copy_from_slice(&efi[start..end]);
        write_cluster(image, data_start, file_start_cluster + i as u32, &cluster);
    }
}

fn compute_fat_sectors(part_sectors: u32) -> u32 {
    let mut fat_sectors = ((part_sectors - RESERVED_SECTORS) * 4)
        .div_ceil(SECTOR_SIZE as u32 * SECTORS_PER_CLUSTER + FAT_COUNT * 4);
    loop {
        let data_sectors = part_sectors - RESERVED_SECTORS - FAT_COUNT * fat_sectors;
        let clusters = data_sectors / SECTORS_PER_CLUSTER;
        let needed = ((clusters + 2) * 4).div_ceil(SECTOR_SIZE as u32);
        if needed <= fat_sectors {
            return fat_sectors;
        }
        fat_sectors = needed;
    }
}

fn write_fat32_boot_sector(image: &mut [u8], part_sectors: u32, fat_sectors: u32) {
    let b = sector_mut(image, PART_START);
    b[0..3].copy_from_slice(&[0xeb, 0x58, 0x90]);
    b[3..11].copy_from_slice(b"MSWIN4.1");
    put_u16(b, 11, SECTOR_SIZE as u16);
    b[13] = SECTORS_PER_CLUSTER as u8;
    put_u16(b, 14, RESERVED_SECTORS as u16);
    b[16] = FAT_COUNT as u8;
    put_u16(b, 17, 0);
    put_u16(b, 19, 0);
    b[21] = 0xf8;
    put_u16(b, 22, 0);
    put_u16(b, 24, 63);
    put_u16(b, 26, 255);
    put_u32(b, 28, PART_START as u32);
    put_u32(b, 32, part_sectors);
    put_u32(b, 36, fat_sectors);
    put_u16(b, 40, 0);
    put_u16(b, 42, 0);
    put_u32(b, 44, 2);
    put_u16(b, 48, 1);
    put_u16(b, 50, 6);
    b[64] = 0x80;
    b[66] = 0x29;
    put_u32(b, 67, 0x2026_0625);
    b[71..82].copy_from_slice(b"RUSTOSPOC  ");
    b[82..90].copy_from_slice(b"FAT32   ");
    b[510] = 0x55;
    b[511] = 0xaa;
}

fn write_fsinfo(image: &mut [u8], cluster_count: u32, next_free: u32) {
    let b = sector_mut(image, PART_START + 1);
    put_u32(b, 0, 0x4161_5252);
    put_u32(b, 484, 0x6141_7272);
    put_u32(b, 488, cluster_count.saturating_sub(next_free));
    put_u32(b, 492, next_free);
    b[510] = 0x55;
    b[511] = 0xaa;
}

fn write_dir_entry(dir: &mut [u8; SECTOR_SIZE], index: usize, name: &[u8; 11], attr: u8, cluster: u32, size: u32) {
    let off = index * 32;
    dir[off..off + 11].copy_from_slice(name);
    dir[off + 11] = attr;
    put_u16(dir, off + 20, (cluster >> 16) as u16);
    put_u16(dir, off + 26, cluster as u16);
    put_u32(dir, off + 28, size);
}

fn write_cluster(image: &mut [u8], data_start: u32, cluster: u32, data: &[u8; SECTOR_SIZE]) {
    let lba = PART_START + data_start as u64 + (cluster - 2) as u64 * SECTORS_PER_CLUSTER as u64;
    sector_mut(image, lba).copy_from_slice(data);
}

fn fat_set(fat: &mut [u8], cluster: u32, value: u32) {
    put_u32(fat, cluster as usize * 4, value);
}

fn sector(image: &[u8], lba: u64) -> &[u8] {
    let start = lba as usize * SECTOR_SIZE;
    &image[start..start + SECTOR_SIZE]
}

fn sector_mut(image: &mut [u8], lba: u64) -> &mut [u8] {
    let start = lba as usize * SECTOR_SIZE;
    &mut image[start..start + SECTOR_SIZE]
}

fn write_bytes_at_lba(image: &mut [u8], lba: u64, bytes: &[u8]) {
    let start = lba as usize * SECTOR_SIZE;
    image[start..start + bytes.len()].copy_from_slice(bytes);
}

fn put_u16(bytes: &mut [u8], offset: usize, value: u16) {
    bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

fn put_u32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn put_u64(bytes: &mut [u8], offset: usize, value: u64) {
    bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for &byte in bytes {
        crc ^= byte as u32;
        for _ in 0..8 {
            let mask = 0u32.wrapping_sub(crc & 1);
            crc = (crc >> 1) ^ (0xedb8_8320 & mask);
        }
    }
    !crc
}
