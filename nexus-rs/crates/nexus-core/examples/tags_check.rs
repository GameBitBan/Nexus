//! cargo run --example tags_check -- cover.jpg file...  — reads, writes, re-reads tags.
use nexus_core::tags::{Cover, prepare_cover, read_tags, write_tags};
use std::path::Path;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (cov, mime) = prepare_cover(&std::fs::read(&args[0]).unwrap(), 600).unwrap();
    for p in &args[1..] {
        let p = Path::new(p);
        let before = read_tags(p, false);
        let b = before.as_ref().map(|t| (t.get("title").to_string(), t.family.clone(), t.embedded.as_ref().map(|e| e.len()))).ok();
        let changes: Vec<(String, String)> = [
            ("title", "Ünïcode Title"),
            ("artist", "A; B"),
            ("album", "Album X"),
            ("albumartist", "Various"),
            ("tracknumber", "3/12"),
            ("discnumber", "1/2"),
            ("date", "2021-05-04"),
            ("genre", "Ambient"),
            ("composer", "C. Omposer"),
            ("comment", "hello"),
            ("lyrics", "line one\nline two"),
        ]
        .iter()
        .map(|(a, b)| (a.to_string(), b.to_string()))
        .collect();
        let w = write_tags(p, &changes, &Cover::Set(cov.clone(), mime));
        let after = read_tags(p, false).unwrap();
        println!("{} before={:?} write={:?}", p.display(), b, w);
        for (k, v) in &after.fields {
            print!("  {k}={v:?}");
        }
        println!("\n  cover={:?}", after.embedded.as_ref().map(|e| e.len()));
        // blanking a field + removing the cover
        write_tags(p, &[("comment".into(), String::new())], &Cover::Remove).unwrap();
        let again = read_tags(p, false).unwrap();
        println!("  after blank: comment={:?} cover={:?} title={:?}", again.get("comment"), again.embedded.as_ref().map(|e| e.len()), again.get("title"));
        write_tags(p, &[], &Cover::Set(cov.clone(), mime)).unwrap();
    }
}
