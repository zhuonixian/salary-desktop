"""
考勤表格解析器

将 OCR 识别的文本结果解析为结构化考勤数据。
支持多种考勤表格格式的容错解析。
"""

import re
import logging
from typing import List, Dict, Any, Optional, Tuple

logger = logging.getLogger(__name__)

# ---------------------------------------------------------------------------
# 字段关键词映射表
# ---------------------------------------------------------------------------

# 表头关键词 -> 输出字段名
HEADER_KEYWORDS: Dict[str, List[str]] = {
    "employee_no": ["工号", "员工编号", "编号", "员工号", "EMPNO", "Emp No", "ID"],
    "name": ["姓名", "名字", "员工姓名", "Name", "名称"],
    "expected_days": ["应出勤", "应出勤天数", "应出勤天数(天)", "应到", "应出勤(天)", "规定出勤"],
    "actual_days": ["实出勤", "实际出勤", "实出勤天数", "实出勤天数(天)", "实到", "实际出勤(天)", "出勤天数"],
    "late_count": ["迟到", "迟到次数", "迟到(次)", "迟到天数", "迟到(天)"],
    "early_leave_count": ["早退", "早退次数", "早退(次)", "早退天数", "早退(天)"],
    "personal_leave_days": ["事假", "事假天数", "事假(天)", "个人请假"],
    "sick_leave_days": ["病假", "病假天数", "病假(天)"],
    "absent_days": ["旷工", "旷工天数", "旷工(天)", "旷职"],
    "overtime_hours": ["加班", "加班时长", "加班(小时)", "加班(h)", "加班(H)", "加班小时", "加班时数"],
}

# 休假相关（有时合并显示）
LEAVE_KEYWORDS: Dict[str, List[str]] = {
    "annual_leave_days": ["年假", "年休假", "年假天数"],
    "maternity_leave_days": ["产假", "产假天数"],
    "marriage_leave_days": ["婚假", "婚假天数"],
    "bereavement_leave_days": ["丧假", "丧假天数"],
}

# 全部字段
ALL_FIELDS = {**HEADER_KEYWORDS, **LEAVE_KEYWORDS}

# 数值字段列表
NUMERIC_FIELDS = [
    "expected_days", "actual_days", "late_count", "early_leave_count",
    "personal_leave_days", "sick_leave_days", "absent_days", "overtime_hours",
    "annual_leave_days", "maternity_leave_days", "marriage_leave_days",
    "bereavement_leave_days",
]


def _normalize_text(text: str) -> str:
    """规范化文本：去除多余空白和特殊字符"""
    text = text.strip()
    # 去除零宽字符
    text = re.sub(r"[\u200b\u200c\u200d\ufeff]", "", text)
    # 全角转半角（数字和常见符号）
    text = text.translate(_FULLWIDTH_MAP)
    # 合并连续空格
    text = re.sub(r"\s+", " ", text)
    return text


# 全角 -> 半角映射
_FULLWIDTH_MAP = {}
for i in range(0x21, 0x7F):
    _FULLWIDTH_MAP[0xFF00 + i - 0x20] = i


def _match_header(text: str) -> Optional[str]:
    """
    尝试将文本匹配到字段名。
    返回字段 key 或 None。
    """
    normalized = _normalize_text(text)
    for field_key, keywords in ALL_FIELDS.items():
        for kw in keywords:
            if kw in normalized:
                return field_key
    return None


def _safe_float(value: str) -> Optional[float]:
    """安全地将字符串转为浮点数"""
    if not value:
        return None
    # 清除非数字字符（保留小数点、负号）
    cleaned = re.sub(r"[^\d.\-]", "", value)
    if not cleaned:
        return None
    try:
        return float(cleaned)
    except ValueError:
        return None


def _safe_int(value: str) -> Optional[int]:
    """安全地将字符串转为整数"""
    f = _safe_float(value)
    if f is None:
        return None
    try:
        return int(round(f))
    except (ValueError, OverflowError):
        return None


def _parse_numeric_field(value: str, field_key: str) -> Optional[Any]:
    """根据字段类型解析数值"""
    if field_key == "overtime_hours":
        return _safe_float(value)
    return _safe_int(value)


def _try_parse_as_number(text: str) -> Optional[str]:
    """检查文本是否主要是数字"""
    cleaned = re.sub(r"[^\d.\-]", "", text)
    if cleaned and cleaned.replace(".", "").replace("-", ""):
        return cleaned
    return None


class AttendanceParser:
    """考勤表格解析器"""

    def __init__(self):
        self.warnings: List[str] = []

    def parse(self, ocr_results: List[Tuple[str, float, List[List[int]]]]) -> Dict[str, Any]:
        """
        解析 OCR 识别结果为结构化考勤数据。

        Args:
            ocr_results: OCR 引擎输出 [(text, confidence, bbox), ...]

        Returns:
            结构化考勤数据字典
        """
        self.warnings = []

        if not ocr_results:
            return {
                "success": False,
                "error": "OCR 未识别到任何文本",
                "raw_text": "",
                "rows": [],
                "warnings": [],
            }

        raw_text = "\n".join(item[0] for item in ocr_results)

        # 尝试基于位置的表格解析
        rows = self._parse_table_by_position(ocr_results)

        if not rows:
            # 回退：尝试逐行解析
            rows = self._parse_line_by_line(ocr_results)

        return {
            "success": len(rows) > 0,
            "raw_text": raw_text,
            "rows": rows,
            "warnings": self.warnings,
        }

    def _compute_center(self, bbox: List[List[int]]) -> Tuple[int, int]:
        """计算 bbox 的中心点"""
        xs = [p[0] for p in bbox]
        ys = [p[1] for p in bbox]
        return (sum(xs) // 4, sum(ys) // 4)

    def _compute_width(self, bbox: List[List[int]]) -> int:
        """计算 bbox 宽度"""
        xs = [p[0] for p in bbox]
        return max(xs) - min(xs)

    def _compute_height(self, bbox: List[List[int]]) -> int:
        """计算 bbox 高度"""
        ys = [p[1] for p in bbox]
        return max(ys) - min(ys)

    def _group_by_rows(
        self, ocr_results: List[Tuple[str, float, List[List[int]]]]
    ) -> List[List[Tuple[str, float, List[List[int]]]]]:
        """将 OCR 结果按行分组（基于 y 坐标聚类）"""
        if not ocr_results:
            return []

        # 计算每个条目的 y 中心
        items_with_y = []
        for item in ocr_results:
            _, _, bbox = item
            _, cy = self._compute_center(bbox)
            height = self._compute_height(bbox)
            items_with_y.append((cy, height, item))

        # 按中心 y 排序
        items_with_y.sort(key=lambda x: x[0])

        # 用平均高度的一半作为行合并阈值
        avg_height = sum(h for _, h, _ in items_with_y) / len(items_with_y)
        threshold = max(avg_height * 0.6, 10)

        rows: List[List[Tuple[str, float, List[List[int]]]]] = []
        current_row: List[Tuple[str, float, List[List[int]]]] = []
        current_y = items_with_y[0][0]

        for cy, _, item in items_with_y:
            if not current_row or abs(cy - current_y) <= threshold:
                current_row.append(item)
                if not current_row:
                    current_y = cy
                else:
                    current_y = (current_y + cy) / 2
            else:
                # 当前行按 x 排序
                current_row.sort(key=lambda it: self._compute_center(it[2])[0])
                rows.append(current_row)
                current_row = [item]
                current_y = cy

        if current_row:
            current_row.sort(key=lambda it: self._compute_center(it[2])[0])
            rows.append(current_row)

        return rows

    def _detect_columns(
        self, header_row: List[Tuple[str, float, List[List[int]]]]
    ) -> Dict[str, Tuple[int, int]]:
        """
        从表头行检测列位置。
        返回 {field_key: (x_start, x_end)}
        """
        columns: Dict[str, Tuple[int, int]] = {}

        for text, conf, bbox in header_row:
            field_key = _match_header(text)
            if field_key:
                xs = [p[0] for p in bbox]
                columns[field_key] = (min(xs), max(xs))

        return columns

    def _find_closest_column(
        self, x: int, columns: Dict[str, Tuple[int, int]]
    ) -> Optional[str]:
        """找到最近的列"""
        best_key = None
        best_dist = float("inf")

        for key, (x_start, x_end) in columns.items():
            # 如果在列范围内
            if x_start <= x <= x_end:
                return key
            # 计算到列中心的距离
            col_center = (x_start + x_end) / 2
            dist = abs(x - col_center)
            if dist < best_dist:
                best_dist = dist
                best_key = key

        # 如果距离太远，不匹配
        if best_dist > 300:
            return None
        return best_key

    def _parse_table_by_position(
        self, ocr_results: List[Tuple[str, float, List[List[int]]]]
    ) -> List[Dict[str, Any]]:
        """基于位置的表格解析"""
        text_rows = self._group_by_rows(ocr_results)

        if len(text_rows) < 2:
            return []

        # 尝试在前 3 行中找到表头
        header_row = None
        header_row_idx = -1

        for i, row in enumerate(text_rows[:3]):
            header_count = 0
            for text, _, _ in row:
                if _match_header(text):
                    header_count += 1
            if header_count >= 3:
                header_row = row
                header_row_idx = i
                break

        if header_row is None:
            logger.info("未检测到明确的表头行，尝试其他解析方式")
            return []

        columns = self._detect_columns(header_row)
        if not columns:
            return []

        logger.info("检测到表头列: %s", list(columns.keys()))

        # 如果缺少姓名/工号列，发出警告
        if "name" not in columns and "employee_no" not in columns:
            self.warnings.append("未检测到姓名或工号列，解析结果可能不准确")

        # 解析数据行
        rows: List[Dict[str, Any]] = []
        for row in text_rows[header_row_idx + 1:]:
            record = self._parse_data_row(row, columns)
            if record and (record.get("name") or record.get("employee_no")):
                rows.append(record)

        return rows

    def _parse_data_row(
        self,
        row: List[Tuple[str, float, List[List[int]]]],
        columns: Dict[str, Tuple[int, int]],
    ) -> Optional[Dict[str, Any]]:
        """解析单行数据"""
        record: Dict[str, Any] = {}
        assigned_fields: set = set()

        for text, conf, bbox in row:
            normalized = _normalize_text(text)
            if not normalized:
                continue

            cx, _ = self._compute_center(bbox)
            col_key = self._find_closest_column(cx, columns)

            if col_key and col_key not in assigned_fields:
                if col_key in NUMERIC_FIELDS:
                    num_val = _parse_numeric_field(normalized, col_key)
                    if num_val is not None:
                        record[col_key] = num_val
                        assigned_fields.add(col_key)
                    elif normalized and _try_parse_as_number(normalized):
                        # 尝试提取数字
                        num_val = _parse_numeric_field(normalized, col_key)
                        if num_val is not None:
                            record[col_key] = num_val
                            assigned_fields.add(col_key)
                else:
                    record[col_key] = normalized
                    assigned_fields.add(col_key)

        # 检查是否至少有一个标识字段
        if record.get("name") or record.get("employee_no"):
            if "name" in record and "employee_no" not in record:
                self.warnings.append(f"员工 {record['name']} 未匹配到工号，请人工确认")
            return record

        return None

    def _parse_line_by_line(
        self, ocr_results: List[Tuple[str, float, List[List[int]]]]
    ) -> List[Dict[str, Any]]:
        """
        逐行解析（回退方案）。
        当无法检测到表头时使用，通过关键词匹配来提取数据。
        """
        rows: List[Dict[str, Any]] = []
        text_rows = self._group_by_rows(ocr_results)

        for row in text_rows:
            full_text = " ".join(_normalize_text(item[0]) for item in row)
            record = self._extract_from_text(full_text)
            if record and (record.get("name") or record.get("employee_no")):
                rows.append(record)

        return rows

    def _extract_from_text(self, text: str) -> Optional[Dict[str, Any]]:
        """从一行文本中提取考勤信息"""
        record: Dict[str, Any] = {}

        # 提取姓名（2-4 个中文字符，前后有分隔符）
        name_match = re.search(r"[\s|,\t]([\u4e00-\u9fff]{2,4})[\s|,\t]", text)
        if name_match:
            record["name"] = name_match.group(1)

        # 提取工号
        emp_match = re.search(r"(?:工号|编号|No)[：:\s]*(\d+)", text, re.IGNORECASE)
        if emp_match:
            record["employee_no"] = emp_match.group(1)
        else:
            # 尝试匹配纯数字工号（在行首或分格符后）
            emp_match2 = re.search(r"(?:^|[\s|,\t])(\d{3,8})(?:[\s|,\t])", text)
            if emp_match2 and "name" in record:
                record["employee_no"] = emp_match2.group(1)

        # 提取数值字段
        for field_key, keywords in ALL_FIELDS.items():
            if field_key in ("name", "employee_no"):
                continue
            for kw in keywords:
                pattern = re.escape(kw) + r"[：:\s]*(\d+\.?\d*)"
                match = re.search(pattern, text)
                if match:
                    val = _parse_numeric_field(match.group(1), field_key)
                    if val is not None:
                        record[field_key] = val
                    break

        return record if record else None


def parse_attendance(
    ocr_results: List[Tuple[str, float, List[List[int]]]]
) -> Dict[str, Any]:
    """
    解析考勤数据的便捷函数。

    Args:
        ocr_results: OCR 引擎输出

    Returns:
        结构化考勤数据
    """
    parser = AttendanceParser()
    return parser.parse(ocr_results)
