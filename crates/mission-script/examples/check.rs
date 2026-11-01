use ra2_map_mission_script::{native::Catalog, typescript::Compiler};
use std::collections::BTreeMap;

fn main() -> anyhow::Result<()> {
    std::thread::Builder::new().stack_size(16 * 1024 * 1024).spawn(|| -> anyhow::Result<()> {
        let started = std::time::Instant::now();
        let mut compiler = Compiler::new()?;
        eprintln!("Compiler initialized in {:?}", started.elapsed());
        let schema = Catalog { houses: vec!["Neutral".into()], ..Default::default() };
        // Optionally benchmark the generated catalog of a real map:
        // cargo run -p ra2-map-mission-script --example check -- path/to/map.generated.ts
        let map_types = match std::env::args().nth(1) {
            Some(path) => std::fs::read_to_string(path)?,
            None => schema.typescript()?,
        };
        let mut files = BTreeMap::from([
            ("/map.generated.ts".into(), map_types),
            ("/mission.ts".into(), "import {defineMission, event, action} from './sdk'; import {map, House} from './map.generated'; export default defineMission(map, {id:'test', player:House.Neutral}, m => { m.rule('win', {when:event.sinceScenarioStart(10), once:true, actions:[action.winPlayer()]}); });".into()),
        ]);
        let build_started = std::time::Instant::now();
        let mission = compiler.build(files.clone())?;
        eprintln!("First build in {:?}", build_started.elapsed());
        println!("{}", serde_json::to_string_pretty(&mission)?);
        let build_started = std::time::Instant::now();
        compiler.build(files.clone())?;
        eprintln!("Unchanged build in {:?}", build_started.elapsed());
        files.get_mut("/mission.ts").unwrap().push_str("\n// source edit\n");
        let build_started = std::time::Instant::now();
        compiler.build(files.clone())?;
        eprintln!("Edited build in {:?}", build_started.elapsed());
        files.get_mut("/mission.ts").unwrap().push_str("\nconst invalid: number = 'wrong';");
        let build_started = std::time::Instant::now();
        compiler.build(files.clone())?;
        eprintln!("Type-only error accepted in {:?} (checked by external IDE)", build_started.elapsed());
        files.get_mut("/mission.ts").unwrap().push_str("\nconst broken = ;");
        let build_started = std::time::Instant::now();
        let error = compiler.build(files).expect_err("must reject invalid syntax");
        eprintln!("Syntax error in {:?}", build_started.elapsed());
        println!("Expected syntax error: {error:#}");
        eprintln!("Finished in {:?}", started.elapsed());
        Ok(())
    })?.join().expect("compiler thread")
}
