-- 工资核算助手 数据库初始化脚本
-- SQLite

CREATE TABLE IF NOT EXISTS employees (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  employee_no TEXT UNIQUE NOT NULL,
  name TEXT NOT NULL,
  department TEXT,
  position TEXT,
  id_card TEXT,
  phone TEXT,
  bank_account TEXT,
  bank_name TEXT,
  hire_date TEXT,
  status TEXT DEFAULT 'active',
  base_salary REAL DEFAULT 0,
  position_salary REAL DEFAULT 0,
  performance_salary REAL DEFAULT 0,
  social_security_base REAL DEFAULT 0,
  housing_fund_base REAL DEFAULT 0,
  special_deduction REAL DEFAULT 0,
  remark TEXT,
  created_at TEXT,
  updated_at TEXT
);

CREATE TABLE IF NOT EXISTS attendance_records (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  salary_month TEXT NOT NULL,
  employee_no TEXT NOT NULL,
  name TEXT,
  expected_days REAL DEFAULT 0,
  actual_days REAL DEFAULT 0,
  late_count INTEGER DEFAULT 0,
  early_leave_count INTEGER DEFAULT 0,
  personal_leave_days REAL DEFAULT 0,
  sick_leave_days REAL DEFAULT 0,
  absent_days REAL DEFAULT 0,
  overtime_hours REAL DEFAULT 0,
  source_type TEXT,
  ocr_batch_id INTEGER,
  remark TEXT,
  created_at TEXT,
  updated_at TEXT
);

CREATE TABLE IF NOT EXISTS salary_rules (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  rule_key TEXT UNIQUE NOT NULL,
  rule_name TEXT NOT NULL,
  rule_value REAL DEFAULT 0,
  rule_type TEXT,
  enabled INTEGER DEFAULT 1,
  remark TEXT
);

CREATE TABLE IF NOT EXISTS tax_rules (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  min_amount REAL NOT NULL,
  max_amount REAL,
  tax_rate REAL NOT NULL,
  quick_deduction REAL NOT NULL
);

CREATE TABLE IF NOT EXISTS salary_monthly_results (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  salary_month TEXT NOT NULL,
  employee_no TEXT NOT NULL,
  name TEXT,
  department TEXT,
  base_salary REAL DEFAULT 0,
  position_salary REAL DEFAULT 0,
  performance_salary REAL DEFAULT 0,
  overtime_salary REAL DEFAULT 0,
  meal_allowance REAL DEFAULT 0,
  transport_allowance REAL DEFAULT 0,
  other_allowance REAL DEFAULT 0,
  gross_salary REAL DEFAULT 0,
  social_security_personal REAL DEFAULT 0,
  housing_fund_personal REAL DEFAULT 0,
  attendance_deduction REAL DEFAULT 0,
  tax_amount REAL DEFAULT 0,
  other_deduction REAL DEFAULT 0,
  net_salary REAL DEFAULT 0,
  status TEXT DEFAULT 'draft',
  locked INTEGER DEFAULT 0,
  remark TEXT,
  created_at TEXT,
  updated_at TEXT
);

CREATE TABLE IF NOT EXISTS ocr_batches (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  batch_name TEXT,
  salary_month TEXT,
  image_path TEXT,
  raw_text TEXT,
  parsed_json TEXT,
  status TEXT DEFAULT 'pending',
  created_at TEXT
);

CREATE TABLE IF NOT EXISTS operation_logs (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  operation_type TEXT NOT NULL,
  description TEXT,
  operator TEXT,
  detail TEXT,
  created_at TEXT
);

-- 默认工资规则
INSERT OR IGNORE INTO salary_rules (rule_key, rule_name, rule_value, rule_type, remark) VALUES
  ('late_penalty', '迟到一次扣款', 20, 'attendance', '每次迟到扣款金额'),
  ('early_leave_penalty', '早退一次扣款', 20, 'attendance', '每次早退扣款金额'),
  ('personal_leave_rate', '事假扣款倍率', 1.0, 'attendance', '事假每天扣款 = 日工资 * 倍率'),
  ('sick_leave_rate', '病假扣款倍率', 0.5, 'attendance', '病假每天扣款 = 日工资 * 倍率'),
  ('absent_rate', '旷工扣款倍率', 2.0, 'attendance', '旷工每天扣款 = 日工资 * 倍率'),
  ('overtime_rate', '加班工资倍率', 1.5, 'attendance', '加班工资 = 小时工资 * 倍率'),
  ('social_security_rate', '社保个人比例', 0.105, 'insurance', '养老8%+医疗2%+失业0.5%'),
  ('housing_fund_rate', '公积金个人比例', 0.12, 'insurance', '公积金个人缴纳比例'),
  ('tax_threshold', '个税起征点', 5000, 'tax', '个人所得税起征点'),
  ('meal_allowance', '餐补标准', 0, 'allowance', '每月餐补金额'),
  ('transport_allowance', '交通补助标准', 0, 'allowance', '每月交通补助金额');

-- 默认个税税率表（中国个税7级超额累进税率）
INSERT INTO tax_rules (min_amount, max_amount, tax_rate, quick_deduction) VALUES
  (0, 3000, 0.03, 0),
  (3000, 12000, 0.10, 210),
  (12000, 25000, 0.20, 1410),
  (25000, 35000, 0.25, 2660),
  (35000, 55000, 0.30, 4410),
  (55000, 80000, 0.35, 7160),
  (80000, NULL, 0.45, 15160);

-- 发票费用类型字典
CREATE TABLE IF NOT EXISTS invoice_expense_types (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  code TEXT UNIQUE NOT NULL,
  name TEXT NOT NULL,
  sort_order INTEGER DEFAULT 0,
  enabled INTEGER DEFAULT 1,
  remark TEXT
);

-- 发票主表
CREATE TABLE IF NOT EXISTS invoices (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  invoice_code TEXT,
  invoice_number TEXT,
  invoice_type TEXT,
  issue_date TEXT,
  check_code TEXT,
  amount REAL DEFAULT 0,
  tax_amount REAL DEFAULT 0,
  total_amount REAL DEFAULT 0,
  seller_name TEXT,
  seller_tax_id TEXT,
  buyer_name TEXT,
  buyer_tax_id TEXT,
  expense_type_code TEXT,
  employee_id INTEGER,
  belong_month TEXT,
  status TEXT DEFAULT 'normal',
  remark TEXT,
  image_path TEXT,
  raw_ocr_json TEXT,
  created_at TEXT,
  updated_at TEXT,
  FOREIGN KEY (employee_id) REFERENCES employees(id) ON DELETE SET NULL,
  FOREIGN KEY (expense_type_code) REFERENCES invoice_expense_types(code) ON DELETE SET NULL
);

-- 发票相关索引
CREATE UNIQUE INDEX IF NOT EXISTS idx_invoices_code_number
  ON invoices(invoice_code, invoice_number);
CREATE INDEX IF NOT EXISTS idx_invoices_employee ON invoices(employee_id);
CREATE INDEX IF NOT EXISTS idx_invoices_month ON invoices(belong_month);
CREATE INDEX IF NOT EXISTS idx_invoices_expense_type ON invoices(expense_type_code);

-- 默认发票费用类型
INSERT OR IGNORE INTO invoice_expense_types (code, name, sort_order) VALUES
  ('office', '办公费', 1),
  ('travel', '差旅费', 2),
  ('meal', '餐饮费', 3),
  ('transport', '交通费', 4),
  ('accommodation', '住宿费', 5),
  ('communication', '通讯费', 6),
  ('other', '其他', 99);
