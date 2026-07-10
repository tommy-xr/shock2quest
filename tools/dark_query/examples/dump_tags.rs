// Dump the environmental-sound schema's tag and value name maps - the
// vocabulary usable in `cargo dq sound +tag:value` queries (e.g. tag
// "material" with values "flesh"/"metal"/"plasticrete"). Run with:
//   cargo run -p dark_query --example dump_tags
use std::fs::File;
use std::io::BufReader;

fn main() {
    let (properties, links, links_with_data) = dark::properties::get();
    let data_root = shock2vr::paths::data_root();
    let gam_path = data_root.join("shock2.gam");
    let game_file = File::open(&gam_path).unwrap();
    let mut game_reader = BufReader::new(game_file);
    let gamesys = dark::gamesys::read(&mut game_reader, &links, &links_with_data, &properties);

    let db = gamesys.speech_db();
    println!("== TAGS ({}):", db.tag_map.count());
    for (i, name) in db.tag_map.entries() {
        println!("  {i}: {name}");
    }
    println!("== VALUES ({}):", db.value_map.count());
    for (i, name) in db.value_map.entries() {
        println!("  {i}: {name}");
    }
}
