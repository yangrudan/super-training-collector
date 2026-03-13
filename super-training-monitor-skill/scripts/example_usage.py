#!/usr/bin/env python3
"""
超级训练监控面板API使用示例

这个脚本展示了如何使用超级训练监控面板的API获取各种监控数据。
在实际使用中，Claude Code可以使用类似的逻辑与监控面板交互。
"""

import json
import requests
from typing import Dict, Any, Optional
import sys

class SuperTrainingMonitor:
    """超级训练监控面板客户端"""

    def __init__(self, base_url: str = "http://127.0.0.1:3000"):
        self.base_url = base_url.rstrip('/')

    def _make_request(self, endpoint: str, payload: Dict = None) -> Dict:
        """发送API请求"""
        url = f"{self.base_url}/api/{endpoint}"
        headers = {'Content-Type': 'application/json'}

        try:
            response = requests.post(
                url,
                headers=headers,
                json=payload or {},
                timeout=10
            )
            response.raise_for_status()
            return response.json()
        except requests.exceptions.ConnectionError:
            raise ConnectionError(f"无法连接到监控面板: {url}。请确认服务是否运行。")
        except requests.exceptions.HTTPError as e:
            raise RuntimeError(f"API请求失败 ({e.response.status_code}): {e.response.text}")
        except Exception as e:
            raise RuntimeError(f"请求失败: {str(e)}")

    def get_global_metrics(self) -> Dict:
        """获取全局聚合指标"""
        return self._make_request("get_global_metrics")

    def get_nodes(self, sort_field: str = "SlowRatio", sort_order: str = "Desc",
                  status_filter: str = "All") -> Dict:
        """获取节点列表"""
        payload = {
            "sort_field": sort_field,
            "sort_order": sort_order,
            "status_filter": status_filter
        }
        return self._make_request("get_nodes", payload)

    def get_node_ranks(self, ip: str) -> Dict:
        """获取节点Rank详情"""
        return self._make_request("get_node_ranks", {"ip": ip})

    def get_node_flamegraph(self, ip: str) -> str:
        """获取节点火焰图SVG"""
        url = f"{self.base_url}/api/get_node_flamegraph"
        response = requests.post(url, json={"ip": ip})
        response.raise_for_status()
        return response.text  # SVG内容

    def get_global_step_metrics(self) -> Dict:
        """获取全局Step指标"""
        return self._make_request("get_global_step_metrics")

    def get_rank_step_metrics(self, ip: str, local_rank: int = 0, rank_id: int = 0) -> Dict:
        """获取Rank Step指标"""
        payload = {
            "ip": ip,
            "local_rank": local_rank,
            "rank_id": rank_id
        }
        return self._make_request("get_rank_step_metrics", payload)

def print_global_summary(metrics: Dict):
    """打印全局指标摘要"""
    print("=" * 60)
    print("训练集群全局状态")
    print("=" * 60)

    # 健康状态
    print(f"\n📊 健康状态分布:")
    print(f"  节点: 🟢 {metrics['healthy_nodes']}/{metrics['total_nodes']} 健康 | "
          f"🟡 {metrics['warning_nodes']}/{metrics['total_nodes']} 警告 | "
          f"🔴 {metrics['critical_nodes']}/{metrics['total_nodes']} 故障")
    print(f"  Rank: 🟢 {metrics['healthy_ranks']}/{metrics['total_ranks']} 健康 | "
          f"🟡 {metrics['warning_ranks']}/{metrics['total_ranks']} 警告 | "
          f"🔴 {metrics['critical_ranks']}/{metrics['total_ranks']} 故障")

    # 性能指标
    print(f"\n⚡ 性能指标:")
    print(f"  P50 Step Time: {metrics['global_p50_step_time_ms']:.1f} ms")
    print(f"  P99 Step Time: {metrics['global_p99_step_time_ms']:.1f} ms")
    print(f"  平均GPU利用率: {metrics['global_avg_gpu_utilization']:.1f} %")
    print(f"  慢节点占比: {metrics['slow_node_ratio']*100:.1f} %")

    # 训练进度
    print(f"\n🚀 训练进度:")
    print(f"  当前Step: {metrics['current_step']:,}")
    print(f"  训练速度: {metrics['steps_per_second']:.2f} steps/s")
    if metrics['estimated_remaining_hours']:
        print(f"  预计剩余: {metrics['estimated_remaining_hours']:.1f} 小时")

    print(f"\n🕒 最后更新: {metrics['last_update']}")

def print_nodes_table(nodes_response: Dict):
    """打印节点表格"""
    nodes = nodes_response['nodes']
    total = nodes_response['total']

    print(f"\n📋 节点列表 (共 {total} 个节点)")
    print("=" * 100)
    print(f"{'IP地址':<15} {'主机名':<15} {'状态':<8} {'慢Rank%':<8} {'Step Time':<12} {'GPU%':<8} {'NCCL延迟':<10}")
    print("-" * 100)

    for node in nodes[:10]:  # 只显示前10个
        status_icon = "🟢" if node['status'] == "Healthy" else "🟡" if node['status'] == "Warning" else "🔴"
        status_text = {"Healthy": "健康", "Warning": "警告", "Critical": "故障"}[node['status']]

        print(f"{node['node_ip']:<15} {node['hostname']:<15} {status_icon}{status_text:<6} "
              f"{node['slow_ratio']*100:>6.1f}% {node['avg_step_time_ms']:>10.1f}ms "
              f"{node['avg_gpu_utilization']:>7.1f}% {node['avg_nccl_latency_ms']:>9.1f}ms")

    if len(nodes) > 10:
        print(f"... 还有 {len(nodes) - 10} 个节点未显示")

def diagnose_node(node: Dict) -> str:
    """诊断节点问题"""
    issues = []

    if node['slow_ratio'] > 0.3:
        issues.append(f"慢Rank占比过高 ({node['slow_ratio']*100:.1f}%)")
    if node['avg_gpu_utilization'] < 50:
        issues.append(f"GPU利用率低 ({node['avg_gpu_utilization']:.1f}%)")
    if node['avg_nccl_latency_ms'] > 10:
        issues.append(f"NCCL延迟高 ({node['avg_nccl_latency_ms']:.1f}ms)")

    if not issues:
        return "✅ 节点运行正常"
    else:
        return f"⚠️ 潜在问题: {', '.join(issues)}"

def main():
    """主函数：演示各种API使用"""
    import argparse

    parser = argparse.ArgumentParser(description='超级训练监控面板API演示')
    parser.add_argument('--url', default='http://127.0.0.1:3000',
                       help='监控面板地址 (默认: http://127.0.0.1:3000)')
    parser.add_argument('--ip', help='特定节点IP地址')
    args = parser.parse_args()

    monitor = SuperTrainingMonitor(args.url)

    try:
        # 演示1：获取全局指标
        print("🔍 正在获取全局指标...")
        global_metrics = monitor.get_global_metrics()
        print_global_summary(global_metrics)

        # 演示2：获取节点列表（按慢节点排序）
        print("\n" + "="*60)
        print("🔍 正在获取节点列表（按慢节点排序）...")
        nodes_response = monitor.get_nodes(sort_field="SlowRatio", sort_order="Desc")
        print_nodes_table(nodes_response)

        # 演示3：诊断最慢的节点
        if nodes_response['nodes']:
            slowest_node = nodes_response['nodes'][0]
            print(f"\n📊 最慢节点诊断: {slowest_node['node_ip']} ({slowest_node['hostname']})")
            print(f"  诊断结果: {diagnose_node(slowest_node)}")

            # 演示4：获取节点详情（如果提供了IP或使用最慢节点）
            target_ip = args.ip or slowest_node['node_ip']
            print(f"\n" + "="*60)
            print(f"🔍 正在获取节点详情: {target_ip}...")
            try:
                node_ranks = monitor.get_node_ranks(target_ip)
                ranks = node_ranks['ranks']
                print(f"  节点 {target_ip} 有 {len(ranks)} 个rank")

                # 显示前几个rank的指标
                if ranks:
                    print(f"\n  Rank性能摘要:")
                    for rank in ranks[:3]:  # 只显示前3个
                        print(f"    Rank {rank['rank_id']} (GPU{rank['local_rank']}): "
                              f"Step Time: {rank['step_time_ms']:.1f}ms, "
                              f"GPU: {rank['gpu_utilization']:.1f}%, "
                              f"状态: {rank['status']}")
            except Exception as e:
                print(f"  获取节点详情失败: {e}")

        # 演示5：检查Step功能
        print("\n" + "="*60)
        print("🔍 检查Step功能状态...")
        try:
            step_metrics = monitor.get_global_step_metrics()
            print(f"  ✅ Step功能已启用")
            print(f"  当前Step: {step_metrics.get('current_step', 'N/A')}")
            if 'latest_duration_ms' in step_metrics and step_metrics['latest_duration_ms']:
                print(f"  最近Duration: {step_metrics['latest_duration_ms']:.2f} ms")
            if step_metrics.get('records'):
                print(f"  有 {len(step_metrics['records'])} 条Step记录")
        except Exception as e:
            print(f"  ⚠️ Step功能未启用或出错: {e}")

        print("\n" + "="*60)
        print("✅ 演示完成!")

    except ConnectionError as e:
        print(f"❌ 连接错误: {e}", file=sys.stderr)
        print("\n💡 解决方法:")
        print("  1. 确认监控面板服务正在运行")
        print(f"  2. 检查地址是否正确: {args.url}")
        print("  3. 使用正确的IP和端口")
        sys.exit(1)
    except Exception as e:
        print(f"❌ 错误: {e}", file=sys.stderr)
        sys.exit(1)

if __name__ == "__main__":
    main()