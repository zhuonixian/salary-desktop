# Python OCR Sidecar

工资核算桌面工具的 OCR 识别模块，基于 PaddleOCR 实现考勤表格截图的自动识别和结构化解析。

## 环境要求

- Python 3.9+
- pip

## 安装

```bash
cd python-ocr
pip install -r requirements.txt
```

> 首次运行时 PaddleOCR 会自动下载 PP-OCRv5 中文模型（约 100MB），请确保网络畅通。

## 使用方法

### 命令行调用

```bash
# 识别考勤表并输出 JSON 到 stdout
python main.py --image attendance.png

# 识别考勤表并保存到文件
python main.py --image attendance.png --output result.json

# 仅输出原始 OCR 文本（不解析为考勤数据）
python main.py --image attendance.png --mode raw

# 显示详细日志
python main.py --image attendance.png --verbose

# 禁用图像预处理
python main.py --image attendance.png --no-preprocess

# 启用 GPU 加速（需安装 paddlepaddle-gpu）
python main.py --image attendance.png --gpu
```

### 参数说明

| 参数 | 必需 | 说明 |
|------|------|------|
| `--image` | 是 | 图片文件路径 |
| `--mode` | 否 | 识别模式: `attendance`(考勤表格解析, 默认) 或 `raw`(原始文本) |
| `--output` | 否 | 输出 JSON 文件路径，不指定则输出到 stdout |
| `--no-preprocess` | 否 | 禁用图像预处理 |
| `--verbose` | 否 | 显示详细日志 |
| `--gpu` | 否 | 启用 GPU 加速 |

### 输出格式

考勤模式 (`--mode attendance`) 的输出结构：

```json
{
  "success": true,
  "raw_text": "...",
  "elapsed_seconds": 3.21,
  "rows": [
    {
      "employee_no": "001",
      "name": "张三",
      "expected_days": 22,
      "actual_days": 21,
      "late_count": 2,
      "early_leave_count": 0,
      "personal_leave_days": 1,
      "sick_leave_days": 0,
      "absent_days": 0,
      "overtime_hours": 6.5
    }
  ],
  "warnings": []
}
```

### 作为 Python 模块调用

```python
from ocr_engine import create_engine
from attendance_parser import parse_attendance

engine = create_engine()
ocr_results = engine.recognize("attendance.png")
data = parse_attendance(ocr_results)

for row in data["rows"]:
    print(f"{row['name']}: 出勤 {row['actual_days']} 天")
```

## 文件结构

```
python-ocr/
├── main.py              # 命令行入口
├── ocr_engine.py        # OCR 引擎封装 (PaddleOCR)
├── attendance_parser.py # 考勤表格解析器
├── requirements.txt     # Python 依赖
└── README.md            # 本文件
```

## 支持的考勤字段

- 工号 / 员工编号
- 姓名
- 应出勤天数
- 实际出勤天数
- 迟到次数
- 早退次数
- 事假天数
- 病假天数
- 旷工天数
- 加班时长
- 年假 / 产假 / 婚假 / 丧假（扩展字段）
