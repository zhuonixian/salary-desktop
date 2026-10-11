import { useCallback, useEffect, useMemo, useState } from 'react';
import {
  Alert,
  Button,
  Card,
  Checkbox,
  Form,
  Input,
  Select,
  Space,
  Steps,
  Tag,
  Typography,
  message,
} from 'antd';
import {
  CheckCircleOutlined,
  CopyOutlined,
  RightOutlined,
  RocketOutlined,
} from '@ant-design/icons';
import { useNavigate } from 'react-router-dom';
import {
  finishSetupWizard,
  getBootstrapStatus,
  getCompanyInfo,
  saveCompanyInfo,
  seedDemoData,
} from '@/api';
import { useSecurity } from '@/contexts/SecurityContext';
import type { BootstrapStatus } from '@/types';

const { Paragraph, Text } = Typography;

// 安全问题选项与 SetupSecurity 保持同一组（忘记密码的备用找回方式）。
const SECURITY_QUESTIONS = [
  '你小学班主任姓什么?',
  '你父亲的名字最后一个字?',
  '你出生的城市?',
  '你的第一家公司名称?',
  '你最喜欢的菜品?',
];

// 恢复码生成规则与 SetupSecurity 一致：排除易混字符 O/0/I/1 的 base32 风格。
function generateRecoveryCode(): string {
  const chars = 'ABCDEFGHIJKLMNPQRSTUVWXYZ23456789';
  const random = new Uint32Array(24);
  window.crypto.getRandomValues(random);
  let result = '';
  for (let i = 0; i < 24; i++) {
    result += chars[random[i] % chars.length];
    if ((i + 1) % 4 === 0 && i !== 23) result += '-';
  }
  return result;
}

function getPasswordStrength(pw: string): { label: string; color: string } {
  if (!pw) return { label: '尚未输入', color: '#999' };
  let score = 0;
  if (pw.length >= 8) score += 1;
  if (/[a-zA-Z]/.test(pw) && /\d/.test(pw)) score += 1;
  if (pw.length >= 12 && /[^a-zA-Z0-9]/.test(pw)) score += 1;
  if (score <= 0) return { label: '过短', color: '#ff4d4f' };
  if (score === 1) return { label: '弱', color: '#fa8c16' };
  if (score === 2) return { label: '中等', color: '#52c41a' };
  return { label: '强', color: '#1677ff' };
}

const errText = (e: unknown): string => (e instanceof Error ? e.message : String(e));

interface CompanyFormValues {
  company_name?: string;
  taxpayer_id?: string;
}

const WIZARD_STEPS = [
  { title: '欢迎' },
  { title: '启动密码' },
  { title: '企业信息' },
  { title: '工资与社保' },
  { title: '员工导入' },
  { title: '期初余额' },
  { title: '完成' },
];

const recoveryBoxStyle: React.CSSProperties = {
  background: '#f6f8fa',
  border: '1px dashed #d9d9d9',
  borderRadius: 8,
  padding: '10px 16px',
  fontFamily: 'ui-monospace, SFMono-Regular, Menlo, Consolas, monospace',
  fontSize: 18,
  letterSpacing: 1.5,
  textAlign: 'center',
  wordBreak: 'break-all',
};

interface SetupWizardProps {
  /**
   * standalone：由 App 启动门禁在"全新账套首次运行"时渲染（此时安全未初始化，
   * 不在 AppLayout 路由树内）。该模式下启动密码推迟到向导完成时生效——
   * setup() 会翻转 isInitialized 导致门禁切换、向导中途被卸载。
   * 应用内重进入口（/setup 路由）不传此 prop，保持即时设置。
   */
  standalone?: boolean;
  /** standalone 模式下向导结束后通知宿主重新探测账套状态（跳过未设密码 → 回退安全设置页） */
  onFinished?: () => void;
}

const SetupWizard: React.FC<SetupWizardProps> = ({ standalone = false, onFinished }) => {
  const navigate = useNavigate();
  const { setup } = useSecurity();

  const [current, setCurrent] = useState(0);
  const [doneSteps, setDoneSteps] = useState<number[]>([]);
  const [bootstrap, setBootstrap] = useState<BootstrapStatus | null>(null);

  // 第 2 步：启动密码（密码 / 找回码 / 安全问题，复用 security 模块 setup）
  const [pw, setPw] = useState('');
  const [pw2, setPw2] = useState('');
  const [recovery] = useState<string>(() => generateRecoveryCode());
  const [savedAck, setSavedAck] = useState(false);
  const [question, setQuestion] = useState<string>(SECURITY_QUESTIONS[0]);
  const [answer, setAnswer] = useState('');
  const [answer2, setAnswer2] = useState('');
  const [pwBusy, setPwBusy] = useState(false);

  // 第 3 步：企业信息
  const [form] = Form.useForm<CompanyFormValues>();
  const [companySaving, setCompanySaving] = useState(false);

  // 第 7 步：完成动作
  const [finishing, setFinishing] = useState<'seed' | 'start' | null>(null);

  const markDone = useCallback((index: number) => {
    setDoneSteps((prev) => (prev.includes(index) ? prev : [...prev, index]));
  }, []);

  const goNext = useCallback(() => {
    setCurrent((step) => Math.min(step + 1, WIZARD_STEPS.length - 1));
  }, []);

  const goPrev = useCallback(() => {
    setCurrent((step) => Math.max(step - 1, 0));
  }, []);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const status = await getBootstrapStatus();
        if (cancelled) return;
        setBootstrap(status);
        if (status.has_password) markDone(1);
        if (status.company_name || status.taxpayer_id) markDone(2);
      } catch (e: unknown) {
        if (!cancelled) message.error('获取初始化状态失败: ' + errText(e));
      }
      try {
        const info = await getCompanyInfo();
        if (cancelled) return;
        form.setFieldsValue({
          company_name: info.company_name ?? '',
          taxpayer_id: info.taxpayer_id ?? '',
        });
      } catch {
        // 企业信息回显失败不阻断向导，保存时由后端校验。
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [form, markDone]);

  const pwStrength = useMemo(() => getPasswordStrength(pw), [pw]);
  const pwStrengthOk = pw.length >= 8 && /[a-zA-Z]/.test(pw) && /\d/.test(pw);
  const hasPassword = Boolean(bootstrap?.has_password);
  const isEmptyLedger = Boolean(bootstrap?.is_empty_ledger);

  const copyRecovery = async () => {
    try {
      await navigator.clipboard.writeText(recovery);
      message.success('已复制到剪贴板');
    } catch {
      message.error('复制失败，请手动抄写');
    }
  };

  // 第 2 步提交：应用内模式复用 SecurityContext.setup（setup_security 后自动刷新安全状态）；
  // standalone 模式仅校验并暂存，密码推迟到向导完成时生效（见 finalizeStandalone）。
  const submitPassword = async () => {
    if (!pwStrengthOk) {
      message.warning('密码至少 8 位且同时包含字母和数字');
      return;
    }
    if (pw !== pw2) {
      message.warning('两次输入的密码不一致');
      return;
    }
    if (!savedAck) {
      message.warning('请先勾选“我已抄写保存恢复码”');
      return;
    }
    if (!answer.trim()) {
      message.warning('请填写安全问题答案');
      return;
    }
    if (answer.trim() !== answer2.trim()) {
      message.warning('两次输入的安全问题答案不一致');
      return;
    }
    if (standalone) {
      message.success('密码校验通过，将在向导完成时生效');
      markDone(1);
      goNext();
      return;
    }
    setPwBusy(true);
    try {
      await setup(pw, recovery, question, answer.trim());
      message.success('启动密码已设置');
      setBootstrap((prev) => (prev ? { ...prev, has_password: true } : prev));
      markDone(1);
      goNext();
    } catch (e: unknown) {
      message.error('设置启动密码失败: ' + errText(e));
    } finally {
      setPwBusy(false);
    }
  };

  const handleSaveCompany = async (values: CompanyFormValues) => {
    setCompanySaving(true);
    try {
      await saveCompanyInfo({
        company_name: values.company_name?.trim() || null,
        taxpayer_id: values.taxpayer_id?.trim() || null,
      });
      message.success('企业信息已保存');
      setBootstrap((prev) =>
        prev
          ? {
              ...prev,
              company_name: values.company_name?.trim() || null,
              taxpayer_id: values.taxpayer_id?.trim() || null,
            }
          : prev,
      );
      markDone(2);
    } catch (e: unknown) {
      message.error('保存企业信息失败: ' + errText(e));
    } finally {
      setCompanySaving(false);
    }
  };

  // standalone 收尾：先标记向导完成，再设置密码（已填且校验通过时——跳过向导也生效，
  // 避免用户填了密码却被送回安全设置页重填），最后回首页并通知宿主重新探测。
  // 顺序不能反：setup() 会翻转 isInitialized 使门禁切换、本组件被卸载。
  const finalizeStandalone = async () => {
    await finishSetupWizard();
    if (!bootstrap?.has_password && pwStrengthOk && pw === pw2 && savedAck && answer.trim()) {
      await setup(pw, recovery, question, answer.trim());
    }
    window.location.hash = '#/';
    onFinished?.();
  };

  // 跳过向导总入口：任一步骤均可直接结束向导回首页
  const handleSkipWizard = async () => {
    try {
      if (standalone) {
        await finalizeStandalone();
      } else {
        await finishSetupWizard();
        navigate('/');
      }
    } catch (e: unknown) {
      message.error('结束向导失败: ' + errText(e));
    }
  };

  // 第 7 步两个可选动作：载入示例数据 / 直接开始使用
  const handleFinish = async (mode: 'seed' | 'start') => {
    setFinishing(mode);
    try {
      if (mode === 'seed') {
        const summary = await seedDemoData();
        const lockedText = summary.locked_month ? `，已锁定 ${summary.locked_month}` : '';
        message.success(
          `示例数据已载入：${summary.employees} 名员工 / 覆盖 ${summary.months.length} 个月份${lockedText}` +
            (summary.try_auto_match_amount != null
              ? `；付款批次标记已付款后可在银行对账中自动匹配 ${summary.try_auto_match_amount} 元流水`
              : ''),
        );
      }
      if (standalone) {
        await finalizeStandalone();
      } else {
        await finishSetupWizard();
        navigate('/');
      }
    } catch (e: unknown) {
      message.error((mode === 'seed' ? '载入示例数据失败: ' : '结束向导失败: ') + errText(e));
    } finally {
      setFinishing(null);
    }
  };

  const renderStepBody = () => {
    switch (current) {
      // 1. 欢迎
      case 0:
        return (
          <div style={{ display: 'grid', gap: 12, padding: '16px 4px' }}>
            <Paragraph style={{ marginBottom: 0 }}>
              工资核算助手是一款<b>本地单机</b>的工资与财务工具：数据仅保存在本机数据库，
              不会上传到任何服务器。
            </Paragraph>
            <Paragraph style={{ marginBottom: 0 }}>
              首次使用建议按步骤完成初始化。<Text type="secondary">每一步都可以跳过，稍后可在对应页面随时补充。</Text>
            </Paragraph>
            <Paragraph style={{ marginBottom: 0 }} type="secondary">
              也可以先载入示例数据体验各功能，体验完成后在首页一键清除示例数据。
            </Paragraph>
          </div>
        );

      // 2. 启动密码
      case 1:
        if (hasPassword) {
          return (
            <div style={{ display: 'grid', gap: 12, padding: '16px 4px' }}>
              <Alert
                type="success"
                showIcon
                message="已设置启动密码"
                description="检测到本机已完成安全初始化，无需重复设置；如需修改请前往安全中心。"
              />
            </div>
          );
        }
        return (
          <div style={{ display: 'grid', gap: 12, padding: '16px 4px', maxWidth: 520 }}>
            {standalone && (
              <Alert
                type="info"
                showIcon
                message="首次运行向导：启动密码将在向导完成时生效；也可跳过向导，稍后在安全中心继续设置。"
              />
            )}
            <div>
              <Text type="secondary" style={{ fontSize: 12 }}>启动密码（用于每次打开应用解锁）</Text>
              <Input.Password
                placeholder="至少 8 位，需同时包含字母和数字"
                value={pw}
                onChange={(e) => setPw(e.target.value)}
                style={{ marginTop: 4 }}
              />
              <Text type="secondary" style={{ fontSize: 12 }}>
                强度：<Text style={{ color: pwStrength.color }}>{pwStrength.label}</Text>
              </Text>
            </div>
            <div>
              <Text type="secondary" style={{ fontSize: 12 }}>确认启动密码</Text>
              <Input.Password
                placeholder="请再次输入启动密码"
                value={pw2}
                onChange={(e) => setPw2(e.target.value)}
                style={{ marginTop: 4 }}
              />
            </div>
            <div>
              <Text type="secondary" style={{ fontSize: 12 }}>
                找回码（忘记密码时找回访问权限，仅显示一次，请抄写保存）
              </Text>
              <div style={{ ...recoveryBoxStyle, marginTop: 4 }}>{recovery}</div>
              <Space style={{ marginTop: 8 }}>
                <Button size="small" icon={<CopyOutlined />} onClick={() => void copyRecovery()}>
                  复制到剪贴板
                </Button>
                <Checkbox checked={savedAck} onChange={(e) => setSavedAck(e.target.checked)}>
                  我已抄写保存恢复码
                </Checkbox>
              </Space>
            </div>
            <div>
              <Text type="secondary" style={{ fontSize: 12 }}>安全问题（备用找回方式）</Text>
              <Select
                value={question}
                onChange={setQuestion}
                style={{ width: '100%', marginTop: 4 }}
                options={SECURITY_QUESTIONS.map((q) => ({ value: q, label: q }))}
              />
              <Input
                placeholder="安全问题答案"
                value={answer}
                onChange={(e) => setAnswer(e.target.value)}
                style={{ marginTop: 8 }}
              />
              <Input
                placeholder="请再次输入答案"
                value={answer2}
                onChange={(e) => setAnswer2(e.target.value)}
                style={{ marginTop: 8 }}
              />
            </div>
          </div>
        );

      // 3. 企业信息
      case 2:
        return (
          <div style={{ padding: '16px 4px', maxWidth: 520 }}>
            <Paragraph type="secondary">
              选填。企业信息用于申报模板表头，可随时在导出中心使用前补充。
            </Paragraph>
            <Form<CompanyFormValues> form={form} layout="vertical" onFinish={handleSaveCompany}>
              <Form.Item name="company_name" label="公司名称">
                <Input placeholder="如：某某科技有限公司" allowClear />
              </Form.Item>
              <Form.Item name="taxpayer_id" label="统一社会信用代码">
                <Input placeholder="如：91110108MA01X0001X" allowClear />
              </Form.Item>
              <Button htmlType="submit" loading={companySaving}>
                保存企业信息
              </Button>
            </Form>
          </div>
        );

      // 4. 工资规则与社保基数
      case 3:
        return (
          <div style={{ display: 'grid', gap: 12, padding: '16px 4px' }}>
            <Paragraph style={{ marginBottom: 0 }}>
              工资规则（个税起征点、三险一金比例等）与社保缴费基数台账
              <Text strong>已预置常规默认值，可稍后调整</Text>。
            </Paragraph>
            <Paragraph type="secondary" style={{ marginBottom: 0 }}>
              如当地比例或基数上下限不同，可前往对应页面修改后再算薪。
            </Paragraph>
            <Space>
              <Button icon={<RightOutlined />} onClick={() => navigate('/rules')}>
                跳转工资规则页
              </Button>
              <Button icon={<RightOutlined />} onClick={() => navigate('/social-insurance')}>
                跳转社保台账页
              </Button>
            </Space>
          </div>
        );

      // 5. 员工导入
      case 4:
        return (
          <div style={{ display: 'grid', gap: 12, padding: '16px 4px' }}>
            <Paragraph style={{ marginBottom: 0 }}>
              在员工管理页逐个添加员工或通过 Excel 模板批量导入，员工是算薪与社保台账的基础资料。
            </Paragraph>
            <Paragraph type="secondary" style={{ marginBottom: 0 }}>
              建议至少补全员工号、姓名、部门与基本工资；邮箱用于发送工资条（选填）。
            </Paragraph>
            <Space>
              <Button icon={<RightOutlined />} onClick={() => navigate('/employees')}>
                跳转员工管理页
              </Button>
            </Space>
          </div>
        );

      // 6. 期初余额
      case 5:
        return (
          <div style={{ display: 'grid', gap: 12, padding: '16px 4px' }}>
            <Paragraph style={{ marginBottom: 0 }}>
              期初余额用于让财务报表从正确的起点开始记账。在科目表页录入启用期间的各科目期初余额，
              借贷合计需平衡。
            </Paragraph>
            <Paragraph type="secondary" style={{ marginBottom: 0 }}>
              纯工资核算场景若暂无存量账务，可跳过本步，稍后在科目表页补充。
            </Paragraph>
            <Space>
              <Button icon={<RightOutlined />} onClick={() => navigate('/accounts')}>
                跳转科目表页
              </Button>
            </Space>
          </div>
        );

      // 7. 完成
      case 6:
      default:
        return (
          <div style={{ display: 'grid', gap: 16, padding: '16px 4px' }}>
            {isEmptyLedger ? (
              <div>
                <Paragraph style={{ marginBottom: 8 }}>
                  还没有业务数据？可先载入一套示例账套体验完整流程（算薪、发票、报销、
                  付款批次与银行对账），体验完成后可在首页一键清除。
                </Paragraph>
                <Button
                  icon={<RocketOutlined />}
                  loading={finishing === 'seed'}
                  onClick={() => void handleFinish('seed')}
                >
                  载入示例数据先体验
                </Button>
              </div>
            ) : (
              <Alert
                type="info"
                showIcon
                message="检测到账套中已有业务数据，已跳过示例数据入口。"
              />
            )}
            <div>
              <Paragraph style={{ marginBottom: 8 }}>初始化完成，直接开始使用：</Paragraph>
              <Button
                type="primary"
                icon={<CheckCircleOutlined />}
                loading={finishing === 'start'}
                onClick={() => void handleFinish('start')}
              >
                直接开始使用
              </Button>
            </div>
          </div>
        );
    }
  };

  // 底部主按钮：第 2 步未设密码时主按钮为「设置并下一步」，其余为「下一步」
  const primaryButton =
    current === 1 && !hasPassword ? (
      <Button type="primary" loading={pwBusy} onClick={() => void submitPassword()}>
        设置密码并下一步
      </Button>
    ) : (
      <Button type="primary" onClick={goNext}>
        下一步
      </Button>
    );

  return (
    <div>
      <div className="page-header">
        <span className="page-title">初始化向导</span>
        <Button type="link" onClick={() => void handleSkipWizard()}>
          跳过向导，直接开始使用
        </Button>
      </div>

      <Card>
        <Steps
          current={current}
          size="small"
          items={WIZARD_STEPS.map((item, index) => ({
            ...item,
            ...(doneSteps.includes(index) && index !== current
              ? { status: 'finish' as const }
              : {}),
          }))}
        />

        <div style={{ marginTop: 16, minHeight: 260 }}>
          <Space style={{ marginBottom: 4 }}>
            <span style={{ fontWeight: 600, fontSize: 15 }}>{WIZARD_STEPS[current].title}</span>
            {doneSteps.includes(current) && (
              <Tag icon={<CheckCircleOutlined />} color="success">
                已完成
              </Tag>
            )}
          </Space>
          {renderStepBody()}
        </div>

        <div
          style={{
            marginTop: 24,
            paddingTop: 16,
            borderTop: '1px solid #f0f0f0',
            display: 'flex',
            justifyContent: 'space-between',
          }}
        >
          <Button disabled={current === 0} onClick={goPrev}>
            上一步
          </Button>
          <Space>
            {current < WIZARD_STEPS.length - 1 && (
              <Button onClick={goNext}>跳过该步</Button>
            )}
            {/* 末步的动作按钮（载入示例数据 / 直接开始使用）已在步骤内容区提供 */}
            {current < WIZARD_STEPS.length - 1 && primaryButton}
          </Space>
        </div>
      </Card>
    </div>
  );
};

export default SetupWizard;
