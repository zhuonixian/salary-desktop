#!/usr/bin/env python3
"""
工资核算桌面工具 - OCR Sidecar 入口文件

通过命令行调用，对考勤表截图进行 OCR 识别并输出结构化 JSON 数据。

用法:
    python main.py --image xxx.png --mode attendance --output result.json
    python main.py --image xxx.png --mode attendance
"""

import argparse
import json
import logging
import os
import sys
import time

# 将当前目录加入 sys.path，确保模块可被正确导入
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from ocr_engine import create_engine  # noqa: E402
from attendance_parser import parse_attendance  # noqa: E402


def setup_logging(verbose: bool = False):
    """配置日志"""
    level = logging.DEBUG if verbose else logging.INFO
    logging.basicConfig(
        level=level,
        format="%(asctime)s [%(levelname)s] %(name)s: %(message)s",
        datefmt="%Y-%m-%d %H:%M:%S",
    )


def build_parser() -> argparse.ArgumentParser:
    """构建命令行参数解析器"""
    parser = argparse.ArgumentParser(
        description="工资核算桌面工具 - OCR Sidecar",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog="""
示例:
  python main.py --image attendance.png --mode attendance
  python main.py --image attendance.png --mode attendance --output result.json
  python main.py --image attendance.png --mode attendance --verbose
        """,
    )
    parser.add_argument(
        "--image",
        required=True,
        help="图片文件路径（支持 PNG、JPG、BMP 等格式）",
    )
    parser.add_argument(
        "--mode",
        default="attendance",
        choices=["attendance", "raw"],
        help="识别模式: attendance=考勤表格解析, raw=仅输出原始文本 (默认: attendance)",
    )
    parser.add_argument(
        "--output",
        default=None,
        help="输出 JSON 文件路径（不指定则输出到 stdout）",
    )
    parser.add_argument(
        "--no-preprocess",
        action="store_true",
        help="禁用图像预处理",
    )
    parser.add_argument(
        "--verbose",
        action="store_true",
        help="显示详细日志",
    )
    parser.add_argument(
        "--gpu",
        action="store_true",
        help="启用 GPU 加速（需要安装 paddlepaddle-gpu）",
    )
    return parser


def run(args: argparse.Namespace) -> int:
    """
    执行 OCR 识别和解析流程。

    Returns:
        退出码，0=成功，1=失败
    """
    logger = logging.getLogger("main")

    # 检查图片文件
    if not os.path.isfile(args.image):
        logger.error("图片文件不存在: %s", args.image)
        print(json.dumps({
            "success": False,
            "error": f"图片文件不存在: {args.image}",
            "rows": [],
            "warnings": [],
        }, ensure_ascii=False, indent=2))
        return 1

    # 创建 OCR 引擎
    engine = create_engine(use_gpu=args.gpu)
    if not engine.available:
        error_msg = (
            "OCR 引擎不可用。请安装依赖:\n"
            "  pip install paddlepaddle>=2.6.0 paddleocr>=2.7.0 opencv-python>=4.8.0"
        )
        logger.error(error_msg)
        print(json.dumps({
            "success": False,
            "error": error_msg,
            "rows": [],
            "warnings": [],
        }, ensure_ascii=False, indent=2))
        return 1

    # 执行 OCR 识别
    logger.info("开始识别图片: %s", args.image)
    start_time = time.time()

    ocr_results = engine.recognize(
        image_path=args.image,
        preprocess=not args.no_preprocess,
    )

    elapsed = time.time() - start_time
    logger.info("OCR 识别完成，耗时 %.2f 秒，识别到 %d 个文本块", elapsed, len(ocr_results))

    # 根据模式处理结果
    if args.mode == "raw":
        # 原始文本模式
        result = {
            "success": True,
            "mode": "raw",
            "elapsed_seconds": round(elapsed, 2),
            "blocks": [
                {
                    "text": text,
                    "confidence": round(conf, 4),
                    "bbox": bbox,
                }
                for text, conf, bbox in ocr_results
            ],
        }
    else:
        # 考勤表格解析模式
        logger.info("开始解析考勤表格...")
        result = parse_attendance(ocr_results)
        result["elapsed_seconds"] = round(elapsed, 2)
        row_count = len(result.get("rows", []))
        logger.info("解析完成，共识别 %d 条员工记录", row_count)

    # 输出结果
    output_json = json.dumps(result, ensure_ascii=False, indent=2)

    if args.output:
        output_dir = os.path.dirname(args.output)
        if output_dir and not os.path.isdir(output_dir):
            os.makedirs(output_dir, exist_ok=True)
        with open(args.output, "w", encoding="utf-8") as f:
            f.write(output_json)
        logger.info("结果已写入: %s", args.output)
    else:
        print(output_json)

    return 0 if result.get("success", False) else 1


def main():
    """主入口"""
    parser = build_parser()
    args = parser.parse_args()
    setup_logging(verbose=args.verbose)
    sys.exit(run(args))


if __name__ == "__main__":
    main()
