use futures::future::join_all;
use serde_json::Value;
use std::fs::{create_dir_all, File};
use std::io::Write;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Fetches JSON data from URLs in batches, calling the handler for each batch.
/// This allows memory to be released after processing each batch.
///
/// # Arguments
/// * `urls` - List of URLs to fetch JSON data from
/// * `batch_size` - Number of URLs fetched concurrently within each worker's batch
/// * `num_workers` - Number of independent parallel workers. Each worker owns a
///   contiguous slice of the URL list and processes it sequentially batch by batch.
///   Use `1` for fully sequential behaviour; `2`–`4` works well in practice.
/// * `handler` - Async callback that receives `Vec<(usize, serde_json::Value)>` where
///               the usize is the global rank index across all URLs
///
/// # Returns
/// * `Ok(())` on success, or an error if the handler fails
pub async fn fetch_urls_batched<F, Fut>(
    urls: Vec<String>,
    batch_size: usize,
    num_workers: usize,
    handler: F,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>>
where
    F: Fn(Vec<(usize, Value)>) -> Fut + Send + Sync + 'static,
    Fut: std::future::Future<Output = Result<(), Box<dyn std::error::Error + Send + Sync>>>
        + Send
        + 'static,
{
    let num_workers = num_workers.max(1);
    let total = urls.len();
    if total == 0 {
        return Ok(());
    }

    // Each worker gets a roughly equal contiguous slice of the URL list.
    // Workers run as independent tokio tasks so the multi-thread scheduler
    // distributes them across OS threads (true CPU parallelism).
    let failed_urls: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let handler = Arc::new(handler);

    let segment_size = (total + num_workers - 1) / num_workers;
    let mut worker_handles = Vec::new();

    for w in 0..num_workers {
        let start = w * segment_size;
        if start >= total {
            break;
        }
        let end = (start + segment_size).min(total);
        let segment: Vec<String> = urls[start..end].to_vec();
        let global_offset = start;

        // Each worker owns its own client — avoids pool-mutex contention across workers.
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .pool_max_idle_per_host(10)
            .build()?;
        let failed_urls = failed_urls.clone();
        let handler = handler.clone();

        worker_handles.push(tokio::spawn(async move {
            let mut local_index = 0usize;

            for chunk in segment.chunks(batch_size) {
                let chunk_start = global_offset + local_index;

                let tasks = chunk.iter().enumerate().map(|(local_idx, url)| {
                    let client = client.clone();
                    let url_clone = url.clone();
                    let rank_index = chunk_start + local_idx;
                    async move {
                        match client.get(&url_clone).send().await {
                            Ok(res) => {
                                let body = res.text().await?;
                                match serde_json::from_str::<Value>(&body) {
                                    Ok(json) => Ok((rank_index, Some(json), url_clone)),
                                    Err(e) => {
                                        eprintln!("Error parsing JSON from {}: {}", url_clone, e);
                                        Ok((rank_index, None, url_clone))
                                    }
                                }
                            }
                            Err(e) => {
                                eprintln!("Error fetching {}: {}", url_clone, e);
                                Ok((rank_index, None, url_clone))
                            }
                        }
                    }
                });

                let fetch_results: Vec<Result<(usize, Option<Value>, String), reqwest::Error>> =
                    join_all(tasks).await;

                let mut batch_data = Vec::with_capacity(chunk.len());
                for res in fetch_results {
                    match res {
                        Ok((rank_index, Some(json), _url)) => {
                            batch_data.push((rank_index, json));
                        }
                        Ok((rank_index, None, url)) => {
                            failed_urls.lock().unwrap().push(url);
                            batch_data.push((rank_index, Value::Array(Vec::new())));
                        }
                        Err(e) => eprintln!("Unexpected error: {}", e),
                    }
                }

                batch_data.sort_by_key(|(idx, _)| *idx);
                handler(batch_data).await?;

                local_index += chunk.len();
            }

            Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
        }));
    }

    for handle in worker_handles {
        handle
            .await
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)??;
    }

    let failed = failed_urls.lock().unwrap();
    if !failed.is_empty() {
        eprintln!("\n⚠️  Failed to fetch data from {} URLs", failed.len());
        for url in failed.iter() {
            eprintln!("  - {}", url);
        }
    }

    Ok(())
}

/// Fetches JSON data from a list of URLs and saves the combined data to a file.
///
/// # Deprecated
/// This function collects all data into memory before writing to file.
/// For large datasets, use `fetch_urls_batched` instead to process data
/// in batches and reduce memory pressure.
#[deprecated(
    since = "0.1.0",
    note = "Use fetch_urls_batched for better memory efficiency with large datasets"
)]
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
    use std::sync::{Arc, Mutex};

    #[tokio::test]
    #[allow(deprecated)]
    async fn test_fetch_and_save_urls_empty_list() {
        let result = fetch_and_save_urls(vec![]).await;
        assert!(result.is_ok(), "Should handle empty URL list");
    }

    #[tokio::test]
    async fn test_fetch_urls_batched_empty_list() {
        let batch_count = Arc::new(Mutex::new(0));
        let batch_count_clone = batch_count.clone();

        let result = fetch_urls_batched(vec![], 5, 1, move |_batch| {
            let counter = batch_count_clone.clone();
            async move {
                let mut count = counter.lock().unwrap();
                *count += 1;
                Ok(())
            }
        })
        .await;

        assert!(result.is_ok(), "Should handle empty URL list");
        assert_eq!(
            *batch_count.lock().unwrap(),
            0,
            "No batches should be processed for empty list"
        );
    }

    #[tokio::test]
    async fn test_fetch_urls_batched_processes_in_batches() {
        use mockito::{mock, server_url};

        // Set up 5 mock endpoints
        let mocks: Vec<_> = (0..5)
            .map(|i| {
                mock("GET", format!("/data/{}", i).as_str())
                    .with_status(200)
                    .with_header("content-type", "application/json")
                    .with_body(format!(r#"{{"rank": {}}}"#, i))
                    .create()
            })
            .collect();

        let urls: Vec<String> = (0..5)
            .map(|i| format!("{}/data/{}", server_url(), i))
            .collect();

        let received_batches: Arc<Mutex<Vec<Vec<(usize, Value)>>>> =
            Arc::new(Mutex::new(Vec::new()));
        let received_batches_clone = received_batches.clone();

        let result = fetch_urls_batched(urls, 2, 1, move |batch| {
            let batches = received_batches_clone.clone();
            async move {
                let mut batches_guard = batches.lock().unwrap();
                batches_guard.push(batch);
                Ok(())
            }
        })
        .await;

        assert!(result.is_ok(), "Should successfully fetch all URLs");

        let batches = received_batches.lock().unwrap();
        assert_eq!(batches.len(), 3, "Should have 3 batches (2+2+1)");

        // Verify batch sizes
        assert_eq!(batches[0].len(), 2, "First batch should have 2 items");
        assert_eq!(batches[1].len(), 2, "Second batch should have 2 items");
        assert_eq!(batches[2].len(), 1, "Third batch should have 1 item");

        // Verify global indices are correct
        assert_eq!(batches[0][0].0, 0, "First item should have index 0");
        assert_eq!(batches[0][1].0, 1, "Second item should have index 1");
        assert_eq!(batches[1][0].0, 2, "Third item should have index 2");
        assert_eq!(batches[1][1].0, 3, "Fourth item should have index 3");
        assert_eq!(batches[2][0].0, 4, "Fifth item should have index 4");

        // Drop mocks to clean up
        drop(mocks);
    }

    #[tokio::test]
    async fn test_fetch_urls_batched_handles_failed_urls() {
        use mockito::{mock, server_url};

        // Only set up endpoint for index 0, leaving index 1 to fail (404)
        let _mock = mock("GET", "/data/0")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"success": true}"#)
            .create();

        let urls = vec![
            format!("{}/data/0", server_url()),
            format!("{}/data/1", server_url()), // This will fail (404)
        ];

        let received_data: Arc<Mutex<Vec<(usize, Value)>>> = Arc::new(Mutex::new(Vec::new()));
        let received_data_clone = received_data.clone();

        let result = fetch_urls_batched(urls, 10, 1, move |batch| {
            let data = received_data_clone.clone();
            async move {
                let mut data_guard = data.lock().unwrap();
                data_guard.extend(batch);
                Ok(())
            }
        })
        .await;

        assert!(result.is_ok(), "Should complete even with failed URLs");

        let data = received_data.lock().unwrap();
        assert_eq!(data.len(), 2, "Should have 2 items");

        // First URL succeeded
        assert_eq!(data[0].0, 0);
        assert!(
            data[0].1.get("success").is_some(),
            "First item should have success field"
        );

        // Second URL failed - should have empty array
        assert_eq!(data[1].0, 1);
        assert!(
            data[1].1.as_array().map(|a| a.is_empty()).unwrap_or(false),
            "Failed URL should return empty array"
        );
    }
}
