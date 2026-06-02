use std::path::Path;

const BULK_DATA_URL: &str = "https://api.scryfall.com/bulk-data";

pub fn update_cards(data_dir: &Path) -> Result<(), String> {
    println!("Fetching bulk data index from Scryfall...");
    let index: serde_json::Value = ureq::get(BULK_DATA_URL)
        .call()
        .map_err(|e| format!("Failed to fetch bulk data index: {e}"))?
        .body_mut()
        .read_json()
        .map_err(|e| format!("Failed to parse bulk data index: {e}"))?;

    let download_uri = index["data"]
        .as_array()
        .ok_or("bulk-data response missing 'data' array")?
        .iter()
        .find(|entry| entry["type"].as_str() == Some("oracle_cards"))
        .ok_or("oracle_cards entry not found in bulk data")?["download_uri"]
        .as_str()
        .ok_or("oracle_cards entry missing download_uri")?
        .to_string();

    println!("Downloading Oracle Cards from {download_uri}...");
    let mut response = ureq::get(&download_uri)
        .call()
        .map_err(|e| format!("Failed to download oracle_cards: {e}"))?;

    let dest = data_dir.join("oracle_cards.json");
    let tmp = data_dir.join("oracle_cards.json.tmp");

    let mut file =
        std::fs::File::create(&tmp).map_err(|e| format!("Failed to create temp file: {e}"))?;

    std::io::copy(&mut response.body_mut().as_reader(), &mut file)
        .map_err(|e| format!("Failed to write oracle_cards: {e}"))?;

    std::fs::rename(&tmp, &dest).map_err(|e| format!("Failed to move file into place: {e}"))?;

    let bytes = std::fs::metadata(&dest).map(|m| m.len()).unwrap_or(0);
    println!("Saved {bytes} bytes to {}", dest.display());
    Ok(())
}
