use micropb_gen::{Config, Generator, config::OptionalRepr};
use std::{env, fs};
fn main() {
    let args: Vec<_> = env::args().collect();
    assert_eq!(args.len(), 4, "generator PROFILE DESCRIPTOR OUTPUT");
    let profile: serde_json::Value = serde_json::from_slice(&fs::read(&args[1]).unwrap()).unwrap();
    let mut generator = Generator::new();
    generator.use_container_heapless();
    generator.configure(".", Config::new().optional_repr(OptionalRepr::Option));
    for (field, capacity) in profile["string_max_utf8_bytes"].as_object().unwrap() {
        generator.configure(
            &format!(".messaging.v1.{field}"),
            Config::new().max_bytes(capacity.as_u64().unwrap() as u32),
        );
    }
    generator.compile_fdset_file(&args[2], &args[3]).unwrap();
}
