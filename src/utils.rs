
use std::{fs::File, io::BufReader};

pub fn read_json_file(file_path: &str) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let file = File::open(file_path)?;
    let reader = BufReader::new(file);

    let data = serde_json::from_reader(reader)?;

    Ok(data)
}
