//! Dumps the 25AE first-person weapon animations from the remaster's
//! `sq_scripts/animations_weapons.nut`, to check the parser against the real
//! shipped file rather than only a fixture.
//!
//! ```bash
//! cargo run -p dark --example dump_weapon_animations -- <path>/animations_weapons.nut
//! ```

fn main() {
    let path = std::env::args().nth(1).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    let animations = dark::weapon_animation::parse(&text);
    let mut categories: Vec<&String> = animations.by_category.keys().collect();
    categories.sort();
    let mut total = 0;
    for category in categories {
        let clips = &animations.by_category[category];
        let mut names: Vec<&String> = clips.keys().collect();
        names.sort();
        for name in names {
            let a = &clips[name];
            total += 1;
            let tracks: Vec<String> = a
                .tracks
                .iter()
                .map(|t| format!("{}:{}", t.joint, t.keys.len()))
                .collect();
            println!(
                "{category:24} {name:8} {}fps {:>3}f {:.2}s  [{}]",
                a.fps as i32,
                a.length as i32,
                a.duration_seconds(),
                tracks.join(" ")
            );
        }
    }
    println!("\n{total} animations parsed");
}
