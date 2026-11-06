//! Repeatable synthetic audit of the document's single-object allocation cost.
use anyhow::{Result, ensure};
use ra2_map_document::{
    document::{Baseline, MapDocument},
    new_map::NewMapOptions,
};
use std::{hint::black_box, time::Instant};
use vera20k_core::map::map_file;

fn main() -> Result<()> {
    println!("existing_units,median_add_record_ms");
    for count in [100, 1000, 3000] {
        let mut bytes = NewMapOptions::default().bytes()?;
        bytes.extend_from_slice(b"\r\n[Units]\r\n");
        for index in 0..count {
            bytes.extend_from_slice(
                format!("{index}=Neutral,HTNK,256,80,80,0,Guard,None,0,-1,0,-1,1,1\r\n").as_bytes(),
            );
        }
        let map = map_file::load_from_bytes(bytes.clone())?;
        let original = MapDocument::new(Baseline::parse(bytes.into(), &map)?);
        let mut samples = Vec::new();
        for _ in 0..7 {
            let mut trial = original.fork();
            let started = Instant::now();
            let id = black_box(trial.add_record("Units", [81, 80], "Neutral", "HTNK")?);
            samples.push(started.elapsed().as_secs_f64() * 1000.);
            ensure!(trial.records()[&id].key == count.to_string());
        }
        samples.sort_by(f64::total_cmp);
        println!("{count},{:.6}", samples[samples.len() / 2]);
    }
    Ok(())
}
