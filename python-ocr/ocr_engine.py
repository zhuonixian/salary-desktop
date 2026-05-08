"""
OCR 引擎封装模块

封装 PaddleOCR 的初始化、调用和图像预处理功能。
提供 PaddleOCR 未安装时的回退方案。
"""

import os
import sys
import logging
from typing import List, Tuple, Optional

logger = logging.getLogger(__name__)

# OCR 结果条目类型: (text, confidence, bbox)
OcrResult = List[Tuple[str, float, List[List[int]]]]


def _check_paddleocr_available() -> bool:
    """检查 PaddleOCR 是否可用"""
    try:
        import paddleocr  # noqa: F401
        return True
    except ImportError:
        return False


def _check_cv2_available() -> bool:
    """检查 OpenCV 是否可用"""
    try:
        import cv2  # noqa: F401
        return True
    except ImportError:
        return False


def _preprocess_image(image_path: str) -> Optional[str]:
    """
    图像预处理：灰度转换、对比度增强、降噪。
    返回预处理后图片的临时路径；如果 OpenCV 不可用则返回 None。
    """
    if not _check_cv2_available():
        logger.warning("OpenCV 未安装，跳过图像预处理")
        return None

    import cv2
    import numpy as np

    if not os.path.isfile(image_path):
        logger.error("图片文件不存在: %s", image_path)
        return None

    img = cv2.imread(image_path)
    if img is None:
        logger.error("无法读取图片: %s", image_path)
        return None

    # 1. 灰度转换
    gray = cv2.cvtColor(img, cv2.COLOR_BGR2GRAY)

    # 2. 对比度增强 (CLAHE)
    clahe = cv2.createCLAHE(clipLimit=2.0, tileGridSize=(8, 8))
    enhanced = clahe.apply(gray)

    # 3. 降噪
    denoised = cv2.fastNlMeansDenoising(enhanced, h=10, templateWindowSize=7, searchWindowSize=21)

    # 4. 二值化（自适应阈值，有助于表格线识别）
    binary = cv2.adaptiveThreshold(
        denoised, 255, cv2.ADAPTIVE_THRESH_GAUSSIAN_C, cv2.THRESH_BINARY, 11, 2
    )

    # 保存临时文件
    tmp_path = image_path + ".preprocessed.png"
    cv2.imwrite(tmp_path, binary)
    logger.info("预处理图片已保存: %s", tmp_path)
    return tmp_path


class OcrEngine:
    """OCR 引擎封装类"""

    def __init__(self, use_gpu: bool = False, lang: str = "ch"):
        """
        初始化 OCR 引擎。

        Args:
            use_gpu: 是否使用 GPU
            lang: 语言，默认中文 'ch'
        """
        self._ocr = None
        self._use_gpu = use_gpu
        self._lang = lang

        if _check_paddleocr_available():
            self._init_paddleocr()
        else:
            logger.warning(
                "PaddleOCR 未安装，OCR 功能不可用。"
                "请运行: pip install paddlepaddle paddleocr"
            )

    def _init_paddleocr(self):
        """初始化 PaddleOCR 实例"""
        try:
            from paddleocr import PaddleOCR

            logger.info("正在初始化 PaddleOCR (PP-OCRv5, lang=%s, use_gpu=%s)...", self._lang, self._use_gpu)
            self._ocr = PaddleOCR(
                use_angle_cls=True,
                lang=self._lang,
                use_gpu=self._use_gpu,
                ocr_version="PP-OCRv5",
                show_log=False,
            )
            logger.info("PaddleOCR 初始化完成")
        except Exception as e:
            logger.error("PaddleOCR 初始化失败: %s", e)
            self._ocr = None

    @property
    def available(self) -> bool:
        """OCR 引擎是否可用"""
        return self._ocr is not None

    def recognize(self, image_path: str, preprocess: bool = True) -> OcrResult:
        """
        对图片进行 OCR 识别。

        Args:
            image_path: 图片文件路径
            preprocess: 是否进行图像预处理

        Returns:
            按位置排序的文本列表 [(text, confidence, bbox), ...]
        """
        if not os.path.isfile(image_path):
            logger.error("图片文件不存在: %s", image_path)
            return []

        # 尝试预处理
        processed_path = None
        if preprocess:
            processed_path = _preprocess_image(image_path)

        target_path = processed_path or image_path

        if self._ocr is not None:
            results = self._recognize_with_paddle(target_path)
        else:
            results = self._recognize_fallback(target_path)

        # 清理临时文件
        if processed_path and os.path.isfile(processed_path):
            try:
                os.remove(processed_path)
            except OSError:
                pass

        # 按位置排序（从上到下，从左到右）
        results.sort(key=lambda item: (item[2][0][1], item[2][0][0]))
        return results

    def _recognize_with_paddle(self, image_path: str) -> OcrResult:
        """使用 PaddleOCR 进行识别"""
        try:
            raw_result = self._ocr.ocr(image_path, cls=True)
        except Exception as e:
            logger.error("PaddleOCR 识别失败: %s", e)
            return []

        if not raw_result or not raw_result[0]:
            logger.warning("PaddleOCR 未识别到任何文本")
            return []

        results: OcrResult = []
        for line in raw_result[0]:
            bbox = line[0]  # [[x1,y1],[x2,y2],[x3,y3],[x4,y4]]
            text = line[1][0]
            confidence = line[1][1]
            # 将坐标转为整数
            int_bbox = [[int(p[0]), int(p[1])] for p in bbox]
            results.append((text, float(confidence), int_bbox))

        return results

    def _recognize_fallback(self, image_path: str) -> OcrResult:
        """
        PaddleOCR 不可用时的回退方案。
        尝试使用系统 Tesseract；如果也不可用，返回空结果。
        """
        try:
            import subprocess
            # 尝试调用 tesseract
            result = subprocess.run(
                ["tesseract", image_path, "stdout", "-l", "chi_sim", "--psm", "6"],
                capture_output=True,
                text=True,
                timeout=60,
            )
            if result.returncode == 0 and result.stdout.strip():
                lines = result.stdout.strip().split("\n")
                fallback_results: OcrResult = []
                for i, line in enumerate(lines):
                    text = line.strip()
                    if text:
                        # Tesseract 不提供 bbox，使用模拟坐标
                        fallback_results.append((text, 0.5, [[0, i * 30], [800, i * 30], [800, (i + 1) * 30], [0, (i + 1) * 30]]))
                logger.info("使用 Tesseract 回退识别，共 %d 行", len(fallback_results))
                return fallback_results
        except FileNotFoundError:
            logger.warning("Tesseract 也未安装")
        except Exception as e:
            logger.warning("Tesseract 回退识别失败: %s", e)

        logger.error(
            "无可用的 OCR 引擎。请安装 PaddleOCR: pip install paddlepaddle paddleocr"
        )
        return []


def create_engine(use_gpu: bool = False) -> OcrEngine:
    """工厂方法：创建 OCR 引擎实例"""
    return OcrEngine(use_gpu=use_gpu)
