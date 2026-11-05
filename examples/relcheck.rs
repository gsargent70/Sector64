//! Dump a REL file's side-sector view and its record stream. For manual/diff use.
use std::io::Write;
fn main() {
    let path = std::env::args().nth(1).expect("usage: relcheck <img> <name>");
    let name = std::env::args().nth(2).expect("name");
    let disk = sector64::Disk::load(std::fs::read(&path).unwrap()).unwrap();
    let e = disk.find_entry(&name).unwrap().expect("entry");
    eprintln!("type={:?} first={}/{} side={}/{} reclen={}",
        e.file_type, e.first_track, e.first_sector, e.side_track, e.side_sector, e.rel_record_len);
    eprintln!("side_sector_chain = {:?}", disk.side_sector_chain(&e).unwrap());
    eprintln!("rel_data_blocks   = {:?}", disk.rel_data_blocks(&e).unwrap());
    let rel = disk.read_rel(&e).unwrap();
    eprintln!("records = {}", rel.records.len());
    // Emit the concatenated record stream to stdout for byte comparison.
    let mut out = std::io::stdout();
    for r in &rel.records { out.write_all(r).unwrap(); }
}
