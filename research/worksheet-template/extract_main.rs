use manna_api::materials::{extract::extract, file_type_for_name};
fn main() {
    let mut args = std::env::args().skip(1);
    let out = args.next().unwrap();
    for path in args {
        let name = std::path::Path::new(&path).file_name().unwrap().to_string_lossy().to_string();
        let ft = file_type_for_name(&name).expect("type");
        let data = std::fs::read(&path).unwrap();
        let ex = extract(ft, &data);
        let text = ex.text();
        let stem = name.rsplit_once(".").unwrap().0;
        std::fs::write(format!("{out}/{stem}.txt"), &text).unwrap();
        println!("{stem} {:?} blocks={} chars={} pages={:?} trunc={}", ex.status, ex.blocks.len(), text.chars().count(), ex.page_count, ex.truncated);
    }
}
