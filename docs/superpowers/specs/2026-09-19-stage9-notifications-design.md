# 第九阶段设计：通知模块与工资条邮件

日期：2026-09-19
状态：待审阅
基线：v0.8.0（第八阶段已发版，后端 324 测试）

## 1. 背景与目标

工具定位为"不上云、不联网（仅 OCR 可选外联）"。第九阶段在用户明确要求下新增**可选外联的通知能力**：通用通知底座（媒介抽象 + 配置加密 + 发送留痕），首个场景为**员工工资条邮件通知**——发送工资组成、个税、五险一金缴纳明细给员工本人。

外联边界：
- 仅通知模块外联（SMTP），其余模块维持零外联
- 外联凭证（SMTP 授权码）AES-GCM 加密入库
- 收件人仅员工本人邮箱，不群发
- 发送留痕（notification_logs），敏感门禁复用既有体系

## 2. 范围

| 模块 | 深度 | 新表/列 |
|---|---|---|
| 通知底座 | 媒介 trait + 配置加密 + 发送记录 + 重发 | notification_logs |
| 邮件通道 | lettre SMTP blocking，测试发送 | app_settings 加密键 smtp_config |
| 短信通道 | 接口预留（占位错误），不采集密钥 | 无 |
| 工资条邮件 | 三步向导 + HTML 明细 + 批量发送 | employees.email 列 |
| 通知设置页 | SMTP 表单/预设/测试/发送记录 Tab | 无 |

## 3. 数据模型

### 3.1 employees.email

```sql
ensure_column(c, "employees", "email", "TEXT")
```

员工管理表单、导入模板、导出清单同步加列；email 可空（无 email 员工发送时跳过并列出）。

### 3.2 notification_logs

```sql
CREATE TABLE notification_logs (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  channel TEXT NOT NULL CHECK (channel IN ('email','sms')),
  employee_id INTEGER,
  recipient TEXT NOT NULL,
  belong_month TEXT,
  subject TEXT NOT NULL,
  status TEXT NOT NULL CHECK (status IN ('sent','failed','skipped')),
  error_msg TEXT,
  operator TEXT,
  created_at TEXT NOT NULL
);
```

### 3.3 app_settings 加密键

- `smtp_config`：JSON（host/port/encryption(starttls|ssl|none)/username/from_name）**明文部分** + `password`（授权码）**AES-GCM 加密部分**——整键加密存储（复用 OCR token 加密模式），读取时解密；界面回显授权码脱敏（`****` + 末 2 位）

迁移走既有迁移事务框架（单事务/幂等/FK 检查）。

## 4. 通知底座（notification.rs）

### 4.1 媒介抽象

```rust
pub trait NotifyChannel {
    fn name(&self) -> &'static str;               // 'email' | 'sms'
    fn send(&self, message: &NotifyMessage) -> AppResult<()>;
}

pub struct NotifyMessage {
    pub recipient: String,        // email 地址 / 手机号
    pub subject: String,
    pub html_body: Option<String>,
    pub text_body: Option<String>,
}
```

- `EmailChannel`：lettre（特性 `smtp-transport` + `builder` + `rustls-tls`，blocking）；30s 超时/封
- `SmsChannel`：预留实现，`send` 返回 `AppError::General("短信通道未配置，请在设置中接入服务商")`
- 领域函数收 `&dyn NotifyChannel`——单测注入 FakeChannel，不真发

### 4.2 发送流程（不持 DB 锁）

1. 读取批次数据（工资结果/员工邮箱）后**释放 DB 锁**
2. 逐封调用 channel.send（纯网络 IO，无锁）
3. 每封结果回连写 notification_logs（sent/failed+error）
4. 单封失败不中断批量；整批结束返回汇总

### 4.3 配置管理

- `get_smtp_config`（脱敏回显）/ `set_smtp_config`（加密落库）/ `send_test_email`（发给自己验证）
- 未配置时工资条向导第一步拦截并引导设置页

## 5. 工资条邮件场景

入口：工资计算页（已锁定月份）→「邮件发送工资条」三步向导：

1. **选择与预览**：月份须 `status='已锁定'`；勾选员工（默认全选有 email 者，无 email 灰显并列"缺邮箱 N 人"）；预览 HTML 正文——版式复用工资条预览卡片（发放项/扣款项/五险一金个人+单位两侧/个税/实发合计）
2. **门禁**：发送按钮须处于敏感数据解锁状态（复用全局门禁；未解锁禁用+提示"工资明细为明文，请先解锁敏感数据"）
3. **发送**：逐封发送+进度反馈；结果逐封落 notification_logs；失败可勾选重发（重发仅失败者）；发送中可取消（剩余记 skipped）

标题模板：`{YYYY-MM} 工资条`（固定，不新增公司名配置项；发件人显示名由 SMTP 配置的 from_name 体现）。

## 6. 通知设置页

菜单「系统设置 → 通知设置」：

- **SMTP 表单**：host/port/encryption/username/授权码/from_name；预置参数一键填（QQ：smtp.qq.com:465 SSL / 163：smtp.163.com:465 SSL / Outlook：smtp.office365.com:587 STARTTLS）
- **测试发送**：「发测试邮件给自己」按钮（收件人=发件账号），成功/失败即时反馈
- **短信区块**：只读提示"预留接口，接入服务商后启用"
- **发送记录 Tab**：notification_logs 列表（月份/状态/媒介筛选），失败原因列展示

## 7. 错误处理

- SMTP 535（授权码错误）→"授权码错误，请检查邮箱设置（QQ/163 需使用授权码而非登录密码）"
- 超时/网络错误 → failed + 原因入档
- 收件地址格式非法 → 该员工 failed"邮箱地址无效"，不阻断批量
- 配置缺失 → 入口拦截（不进向导）

## 8. 测试策略

- 单测（FakeChannel 注入）：工资条 HTML 组装（金额/项目/姓名变量断言）、收件人解析（无 email 跳过+统计）、记录写入成功/失败双态、重发仅失败者、未锁定月份拦截、smtp_config 加密存取（库内为密文断言）、取消记 skipped、SmsChannel 占位错误
- lettre 真发不进单测；Windows 验收清单加：配置 QQ 邮箱授权码→测试发送→批量发 2 员工→记录核对→失败重发
- 前端 `npx tsc -b` / lint / build

## 9. 范围外

- 企业微信/钉钉 webhook 媒介（候选后续）
- 定时/自动发送（仅手动批量）
- 短信服务商接入与签名模板（仅接口预留）
- PDF 附件、工资条模板自定义引擎（固定版式）
- 阅读回执、发送统计报表

## 10. 批次任务骨架

- **9A（Task 1-4）**：DDL 迁移与模型 → 通知底座（trait+配置加密+FakeChannel 测试）→ SMTP 通道与设置页（含测试发送）→ 批次收尾
- **9B（Task 5-7）**：工资条组装器与邮件向导 → 发送记录与重发 → 全量回归+文档四件套+发版评估
