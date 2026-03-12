# 功能说明

通过和训练进程所对应的RESTFUL API交互, 实现RANK的Step信息获取, 并更新到web页面(和已经实现的堆栈火焰图请求数据的方式类似);

其中首页的step以MASTER_ADDR的ip加上配置文件里的端口+1的端口, 进行获取;

三级子页面的各个RANK按照下面的请求方式, 在对应的ip和端口上进行获取.

## 请求方式

以curl的方式作为参考, 实际过程需要用各个node上rank对应的ip和端口;

```bash
#!/bin/bash
# 在大多数 Linux 上，date +%s%6N 能返回 epoch 毫秒/微秒（若你的 date 不支持 %6N，请用 python 获取）
TIMESTAMP=$(date +%s%6N)

curl -v -X POST \
  -H "Content-Type: application/json" \
  --data-binary @- http://10.107.204.71:9933/query <<JSON
{
  "version": {
    "major": 0,
    "minor": 1,
    "patch": 0
  },
  "timestamp": $TIMESTAMP,
  "payload": {
    "expr": "SELECT step, module, stage, duration, allocated FROM python.torch_trace WHERE step >= 5 ORDER BY step DESC LIMIT 3",
    "opts": {
      "limit": 3
    }
  }
}
JSON
```

## 环境变量

通过环境变量开启上述metrics的接入功能, 如STEP_SHOW=true时开启.