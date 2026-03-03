use reqwest::Error;
use serde::{Deserialize, Serialize};
use tokio::runtime::Runtime;

#[derive(Debug, Deserialize, Serialize)]
struct NodeInfo {
    node_id: String,
    rank: u32,
    // 其他可能需要的字段
}

async fn get_node_info(url: &str) -> Result<Vec<NodeInfo>, Error> {
    let resp = reqwest::get(url).await?;
    let data: Vec<NodeInfo> = resp.json().await?;
    Ok(data)
}

fn main() {
    let rt = Runtime::new().unwrap();
    let url = "http://10.107.204.71:9933/apis/nodes";
    
    let result = rt.block_on(get_node_info(url));
    match result {
        Ok(nodes) => {
            for node in nodes {
                println!("Node ID: {}, Rank: {}", node.node_id, node.rank);
            }
        },
        Err(e) => println!("Error: {}", e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mockito::mock;

    #[tokio::test]
    async fn test_get_node_info() {
        let expected_json = r#"[{"node_id": "node1", "rank": 1}, {"node_id": "node2", "rank": 2}]"#;
        let _m = mock("GET", "/apis/nodes")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(expected_json)
            .create();

        let url = &mockito::server_url();
        let result = get_node_info(&format!("{}/apis/nodes", url)).await;

        match result {
            Ok(nodes) => {
                assert_eq!(nodes.len(), 2);
                assert_eq!(nodes[0].node_id, "node1");
                assert_eq!(nodes[0].rank, 1);
                assert_eq!(nodes[1].node_id, "node2");
                assert_eq!(nodes[1].rank, 2);
            },
            Err(e) => panic!("Test failed with error: {}", e),
        }
    }
}
