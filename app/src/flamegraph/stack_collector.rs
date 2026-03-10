use serde_json::Value;
use std::fs::{create_dir_all, File};
use std::io::Write;
use futures::future::join_all;
use std::time::Duration;

/// Fetches JSON data from a list of URLs and saves the combined data to a file.
pub async fn fetch_and_save_urls(urls: Vec<String>) -> Result<(), Box<dyn std::error::Error>> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .pool_max_idle_per_host(10)
        .build()?;

    const BATCH_SIZE: usize = 10;
    let mut data_list = Vec::new();
    let mut failed_urls = Vec::new();

    for chunk in urls.chunks(BATCH_SIZE) {
        let mut tasks = Vec::new();
        for url in chunk {
            let client = client.clone();
            let url_clone = url.clone();
            tasks.push(async move {
                match client.get(&url_clone).send().await {
                    Ok(res) => {
                        let body = res.text().await?;
                        match serde_json::from_str::<Value>(&body) {
                            Ok(json) => Ok((Some(json), url_clone)),
                            Err(e) => {
                                eprintln!("Error parsing JSON from {}: {}", url_clone, e);
                                Ok((None, url_clone))
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("Error fetching {}: {}", url_clone, e);
                        Ok((None, url_clone))
                    }
                }
            });
        }

        let results: Vec<Result<(Option<Value>, String), reqwest::Error>> = join_all(tasks).await;

        for result in results {
            match result {
                Ok((Some(json), _url)) => data_list.push(json),
                Ok((None, url)) => {
                    failed_urls.push(url);
                    data_list.push(Value::Array(Vec::new()));
                }
                Err(e) => eprintln!("Unexpected error: {}", e),
            }
        }
    }

    if !failed_urls.is_empty() {
        eprintln!("\n⚠️  Failed to fetch data from {} URLs", failed_urls.len());
        for url in &failed_urls {
            eprintln!("  - {}", url);
        }
    }

    create_dir_all("./output")?;
    let output = serde_json::to_string_pretty(&data_list)?;
    let mut file = File::create("./output/output.json")?;
    file.write_all(output.as_bytes())?;
    println!("Data has been saved to output/output.json");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_fetch_and_save_urls_empty_list() {
        let result = fetch_and_save_urls(vec![]).await;
        assert!(result.is_ok(), "Should handle empty URL list");
    }
}
