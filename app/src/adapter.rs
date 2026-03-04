use reqwest::Error;
use serde::{Deserialize, Serialize};
use tokio::runtime::Runtime;

#[derive(Debug, Deserialize, Serialize)]
pub struct NodeInfo {
    pub host: String,
    pub addr: String,
    pub local_rank: u32,
    pub rank: u32,
    pub world_size: u32,
    pub group_rank: u32,
    pub group_world_size: u32,
    pub role_name: String,
    pub role_rank: u32,
    pub role_world_size: u32,
    pub status: String,
    pub timestamp: u64,
}

pub async fn get_node_info(url: &str) -> Result<Vec<NodeInfo>, Error> {
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
                println!("Host: {}, Rank: {}, Status: {}, Address: {}", 
                    node.host, node.rank, node.status, node.addr);
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
    async fn test_get_node_info_with_real_api() {
        let url = "http://10.107.204.71:9933/apis/nodes";
        let result = get_node_info(url).await;
        
        match result {
            Ok(nodes) => {
                println!("Successfully parsed {} nodes", nodes.len());
                for node in &nodes {
                    println!("Host: {}, Rank: {}, Status: {}, Address: {}", 
                        node.host, node.rank, node.status, node.addr);
                }
                // 基本的验证
                assert!(!nodes.is_empty());
                // 验证每个节点都有必要的字段
                for node in nodes {
                    assert!(!node.host.is_empty());
                    assert!(!node.addr.is_empty());
                    assert!(!node.status.is_empty());
                    assert!(!node.role_name.is_empty());
                }
            },
            Err(e) => {
                panic!("Failed to get node info: {}", e);
            }
        }
    }

    #[tokio::test]
    async fn test_get_node_info_with_mock() {
        // 使用mock测试，基于真实数据格式
        let expected_json = r#"[{
            "host": "test-host",
            "addr": "0.0.0.0:9933",
            "local_rank": 0,
            "rank": 0,
            "world_size": 1,
            "group_rank": 0,
            "group_world_size": 1,
            "role_name": "default",
            "role_rank": 0,
            "role_world_size": 1,
            "status": "running",
            "timestamp": 1772605483868205
        }]"#;
        
        let _m = mock("GET", "/apis/nodes")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(expected_json)
            .create();

        let url = &mockito::server_url();
        let full_url = format!("{}/apis/nodes", url);
        let result = get_node_info(&full_url).await;
        
        match result {
            Ok(nodes) => {
                assert_eq!(nodes.len(), 1);
                let node = &nodes[0];
                assert_eq!(node.host, "test-host");
                assert_eq!(node.rank, 0);
                assert_eq!(node.status, "running");
                assert_eq!(node.role_name, "default");
            },
            Err(e) => panic!("Test failed with error: {}", e),
        }
    }
}
