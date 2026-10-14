import { useState, useEffect, useCallback } from 'react';
import {
  Button, Table, Card, Row, Col, Input, Select, DatePicker, Modal,
  message, Space, Tag, Form, Drawer, Spin, Alert, Statistic, Tabs,
} from 'antd';
import {
  UploadOutlined, ExportOutlined, SettingOutlined,
  EditOutlined, DeleteOutlined, EyeOutlined, PlusOutlined,
} from '@ant-design/icons';
import { open, save } from '@tauri-apps/plugin-dialog';
import { convertFileSrc } from '@tauri-apps/api/core';
import {
  getInvoiceExpenseTypes, saveInvoiceExpenseType, deleteInvoiceExpenseType,
  ocrInvoice, saveInvoice, updateInvoice, deleteInvoice, queryInvoices, exportInvoiceList,
  getEmployees, getDecryptedInvoiceUrl, getCompanyInfo,
} from '@/api';
import { SensitiveText } from '@/components/SensitiveText';
import { SensitiveStatistic } from '@/components/SensitiveStatistic';
import { useBusinessMonth } from '@/contexts/BusinessMonthContext';
import type {
  Invoice, InvoiceInput, InvoiceOcrPreview, InvoiceQuery,
  InvoiceExpenseType, InvoiceExpenseTypeInput, Employee, CompanyInfo,
} from '@/types';

// 发票原图预览：后端 get_decrypted_invoice_url 统一返回文件系统绝对路径
// （encrypted=0 直接返回原图路径；encrypted=1 解密落盘后返回临时文件路径）。
// Tauri 2 webview 默认拦截 file:// 或裸 fs path，前端必须用 convertFileSrc
// 包装成 asset 协议（tauri://localhost/... 或 asset://...）才能放进 <img>/<iframe>。
function InvoiceImage({ invoiceId }: { invoiceId: number }) {
  const [url, setUrl] = useState<string>('');
  useEffect(() => {
    let cancelled = false;
    if (invoiceId > 0) {
      getDecryptedInvoiceUrl(invoiceId)
        .then((path) => {
          if (!cancelled && path) {
            setUrl(convertFileSrc(path));
          }
        })
        .catch(() => {
          if (!cancelled) setUrl('');
        });
    }
    return () => { cancelled = true; };
  }, [invoiceId]);
  if (!url) return null;
  if (url.toLowerCase().endsWith('.pdf')) {
    return <iframe src={url} style={{ width: '100%', height: 400 }} title="发票原图" />;
  }
  return <img src={url} alt="发票原图" style={{ width: '100%' }} />;
}

const { TextArea } = Input;

const Invoices: React.FC = () => {
  const [list, setList] = useState<Invoice[]>([]);
  const [loading, setLoading] = useState(false);
  const [expenseTypes, setExpenseTypes] = useState<InvoiceExpenseType[]>([]);
  const [employees, setEmployees] = useState<Employee[]>([]);
  /** 发票方向 Tab：input 进项（默认）/ output 销项（第十二阶段收入侧闭环） */
  const [direction, setDirection] = useState<'input' | 'output'>('input');
  const isOutput = direction === 'output';
  /** 企业信息：销项口径销售方=本公司，自动预填（仍可改） */
  const [company, setCompany] = useState<CompanyInfo | null>(null);

  const { month: filterMonth, setMonth: setFilterMonth } = useBusinessMonth();
  const [filterEmployee, setFilterEmployee] = useState<number | undefined>(undefined);
  const [filterExpenseType, setFilterExpenseType] = useState<string | undefined>(undefined);
  const [filterInvoiceType, setFilterInvoiceType] = useState<string | undefined>(undefined);
  const [filterKeyword, setFilterKeyword] = useState('');

  const [uploadModal, setUploadModal] = useState<{
    visible: boolean;
    ocrLoading: boolean;
    preview: InvoiceOcrPreview | null;
    selectedFilePath: string | null;
    editingId: number | null;
    direction: 'input' | 'output';
    form: InvoiceInput;
  }>({
    visible: false, ocrLoading: false, preview: null,
    selectedFilePath: null, editingId: null, direction: 'input', form: {},
  });

  const [viewDrawer, setViewDrawer] = useState<Invoice | null>(null);
  const [expenseDrawer, setExpenseDrawer] = useState(false);
  const [expenseForm, setExpenseForm] = useState<InvoiceExpenseTypeInput>({});

  const fetchList = useCallback(async () => {
    setLoading(true);
    try {
      const query: InvoiceQuery = {
        belong_month: filterMonth.format('YYYY-MM'),
        direction,
        // 销项不参与报销：报销人/费用类型筛选仅进项口径下发
        employee_id: isOutput ? undefined : filterEmployee,
        expense_type_code: isOutput ? undefined : filterExpenseType,
        invoice_type: filterInvoiceType || undefined,
        keyword: filterKeyword || undefined,
      };
      const data = await queryInvoices(query);
      setList(data);
    } catch (e: unknown) {
      message.error('查询失败: ' + (e instanceof Error ? e.message : String(e)));
    } finally {
      setLoading(false);
    }
  }, [filterMonth, direction, isOutput, filterEmployee, filterExpenseType, filterInvoiceType, filterKeyword]);

  const fetchExpenseTypes = useCallback(async () => {
    try { setExpenseTypes(await getInvoiceExpenseTypes()); } catch { /* ignore */ }
  }, []);

  const fetchEmployees = useCallback(async () => {
    try { setEmployees(await getEmployees()); } catch { /* ignore */ }
  }, []);

  useEffect(() => { fetchList(); }, [fetchList]);
  useEffect(() => { fetchExpenseTypes(); fetchEmployees(); }, [fetchExpenseTypes, fetchEmployees]);
  useEffect(() => {
    // 销项预填本公司抬头：失败不阻断（仍可手工填写）
    getCompanyInfo().then(setCompany).catch(() => setCompany(null));
  }, []);

  const totalAmount = list.reduce((s, i) => s + (i.total_amount || 0), 0);
  const duplicateCount = 0; // 当前查询结果里的重复数（实际入库的不会是重复，留作未来扩展）

  const handleUploadClick = async () => {
    try {
      const selected = await open({
        multiple: false,
        filters: [{ name: '发票图片/PDF', extensions: ['pdf', 'png', 'jpg', 'jpeg', 'bmp'] }],
      });
      if (!selected) return;
      const filePath = selected as string;

      // 体积检查（10MB 上限）
      // Tauri dialog 不返回 size，需要用 fs stat；这里简化：直接发请求让后端报错

      setUploadModal({
        visible: true, ocrLoading: true, preview: null,
        selectedFilePath: filePath, editingId: null, direction, form: {},
      });

      try {
        const preview = await ocrInvoice(filePath);
        setUploadModal(prev => ({
          ...prev,
          ocrLoading: false,
          preview,
          form: {
            invoice_code: preview.invoice_code,
            invoice_number: preview.invoice_number,
            invoice_type: preview.invoice_type,
            issue_date: preview.issue_date,
            check_code: preview.check_code,
            amount: preview.amount,
            tax_amount: preview.tax_amount,
            total_amount: preview.total_amount,
            // 销项口径：销售方=本公司，OCR 未识别到时用企业信息兜底预填
            seller_name: direction === 'output'
              ? preview.seller_name || company?.company_name || undefined
              : preview.seller_name,
            seller_tax_id: direction === 'output'
              ? preview.seller_tax_id || company?.taxpayer_id || undefined
              : preview.seller_tax_id,
            buyer_name: preview.buyer_name,
            buyer_tax_id: preview.buyer_tax_id,
            belong_month: filterMonth.format('YYYY-MM'),
            image_path: filePath,
            raw_ocr_json: preview.raw_ocr_json,
          },
        }));
      } catch (e: unknown) {
        const msg = e instanceof Error ? e.message : String(e);
        message.error('OCR识别失败: ' + msg + '（可手工录入）');
        setUploadModal(prev => ({
          ...prev, ocrLoading: false, preview: null,
          form: {
            belong_month: filterMonth.format('YYYY-MM'),
            image_path: filePath,
            // 销项口径：手工兜底也预填本公司抬头
            seller_name: direction === 'output' ? company?.company_name ?? undefined : undefined,
            seller_tax_id: direction === 'output' ? company?.taxpayer_id ?? undefined : undefined,
          },
        }));
      }
    } catch (e: unknown) {
      message.error('选择文件失败: ' + (e instanceof Error ? e.message : String(e)));
    }
  };

  const handleManualAdd = () => {
    setUploadModal({
      visible: true, ocrLoading: false, preview: null,
      selectedFilePath: null, editingId: null, direction,
      form: {
        belong_month: filterMonth.format('YYYY-MM'),
        // 销项口径：销售方=本公司自动预填（仍可改）
        seller_name: direction === 'output' ? company?.company_name ?? undefined : undefined,
        seller_tax_id: direction === 'output' ? company?.taxpayer_id ?? undefined : undefined,
      },
    });
  };

  const handleSaveInvoice = async () => {
    const { form, editingId, selectedFilePath, direction: modalDirection } = uploadModal;
    if (!form.invoice_number) {
      message.warning('发票号码必填（全电票可无发票代码）');
      return;
    }
    if (modalDirection === 'output') {
      // 销项口径：购买方=客户必填；不参与报销，无报销人/费用类型
      if (!form.buyer_name?.trim()) {
        message.warning('销项发票请填写购买方（客户名称）');
        return;
      }
    } else {
      if (!form.employee_id) {
        message.warning('请选择报销人');
        return;
      }
      if (!form.expense_type_code) {
        message.warning('请选择费用类型');
        return;
      }
    }
    if (uploadModal.preview?.is_duplicate && !editingId) {
      message.error('该发票已存在，禁止保存');
      return;
    }

    try {
      // direction 仅创建时生效（更新不可改方向）；编辑沿用记录原方向
      const payload: InvoiceInput = {
        ...form,
        image_path: selectedFilePath ?? form.image_path,
        direction: modalDirection,
      };
      if (editingId) {
        await updateInvoice(editingId, payload);
        message.success('更新成功');
      } else {
        await saveInvoice(payload);
        message.success('保存成功');
      }
      setUploadModal(prev => ({ ...prev, visible: false }));
      fetchList();
    } catch (e: unknown) {
      message.error('保存失败: ' + (e instanceof Error ? e.message : String(e)));
    }
  };

  const handleEdit = (record: Invoice) => {
    setUploadModal({
      visible: true,
      ocrLoading: false,
      preview: null,
      selectedFilePath: null,
      editingId: record.id,
      // 编辑沿用记录自身方向（销项票在销项 Tab 编辑，进项同理）
      direction: record.direction === 'output' ? 'output' : 'input',
      form: {
        invoice_code: record.invoice_code,
        invoice_number: record.invoice_number,
        invoice_type: record.invoice_type,
        issue_date: record.issue_date,
        check_code: record.check_code,
        amount: record.amount,
        tax_amount: record.tax_amount,
        total_amount: record.total_amount,
        seller_name: record.seller_name,
        seller_tax_id: record.seller_tax_id,
        buyer_name: record.buyer_name,
        buyer_tax_id: record.buyer_tax_id,
        expense_type_code: record.expense_type_code,
        employee_id: record.employee_id,
        belong_month: record.belong_month,
        remark: record.remark,
        image_path: record.image_path,
      },
    });
  };

  const handleDelete = async (id: number) => {
    Modal.confirm({
      title: '确认删除',
      content: '删除后发票记录将标记为作废，不会物理删除。是否继续？',
      okType: 'danger',
      okText: '删除',
      cancelText: '取消',
      onOk: async () => {
        try {
          await deleteInvoice(id);
          message.success('已删除');
          fetchList();
        } catch (e: unknown) {
          message.error('删除失败: ' + (e instanceof Error ? e.message : String(e)));
        }
      },
    });
  };

  const handleExport = async () => {
    try {
      const savePath = await save({
        defaultPath: `发票清单_${filterMonth.format('YYYY-MM')}.xlsx`,
        filters: [{ name: 'Excel', extensions: ['xlsx'] }],
      });
      if (!savePath) return;
      const query: InvoiceQuery = {
        belong_month: filterMonth.format('YYYY-MM'),
        direction,
        employee_id: isOutput ? undefined : filterEmployee,
        expense_type_code: isOutput ? undefined : filterExpenseType,
        invoice_type: filterInvoiceType || undefined,
        keyword: filterKeyword || undefined,
      };
      await exportInvoiceList(query, savePath);
      message.success('已导出');
    } catch (e: unknown) {
      message.error('导出失败: ' + (e instanceof Error ? e.message : String(e)));
    }
  };

  const handleSaveExpenseType = async () => {
    if (!expenseForm.name || !expenseForm.code) {
      message.warning('编码和名称必填');
      return;
    }
    try {
      await saveInvoiceExpenseType(expenseForm);
      message.success('保存成功');
      setExpenseForm({});
      fetchExpenseTypes();
    } catch (e: unknown) {
      message.error('保存失败: ' + (e instanceof Error ? e.message : String(e)));
    }
  };

  const handleDeleteExpenseType = (id: number, code: string) => {
    if (code === 'other') {
      message.warning('「其他」类型不允许删除');
      return;
    }
    Modal.confirm({
      title: '确认删除',
      content: '删除后无法恢复。在用的类型不允许删除。',
      okType: 'danger',
      onOk: async () => {
        try {
          await deleteInvoiceExpenseType(id);
          message.success('已删除');
          fetchExpenseTypes();
        } catch (e: unknown) {
          message.error('删除失败: ' + (e instanceof Error ? e.message : String(e)));
        }
      },
    });
  };

  const columns = [
    {
      title: '发票代码/号码',
      key: 'code_number',
      width: 200,
      render: (_: unknown, r: Invoice) => (
        <div>
          <div style={{ fontSize: 12, color: '#999' }}>{r.invoice_code || '-'}</div>
          <div style={{ fontWeight: 500 }}>{r.invoice_number || '-'}</div>
        </div>
      ),
    },
    { title: '类型', dataIndex: 'invoice_type', key: 'type', width: 120 },
    { title: '开票日期', dataIndex: 'issue_date', key: 'date', width: 110 },
    // 进项展示销售方（供应商），销项展示购买方（客户）
    {
      title: isOutput ? '购买方' : '销售方',
      dataIndex: isOutput ? 'buyer_name' : 'seller_name',
      key: 'counterparty',
      ellipsis: true,
      render: (v?: string) => v || '-',
    },
    // 报销人/费用类型仅进项口径（销项不参与报销）
    ...(!isOutput
      ? [
          {
            title: '报销人',
            key: 'employee',
            width: 100,
            render: (_: unknown, r: Invoice) => {
              const emp = employees.find(e => e.id === r.employee_id);
              return emp?.name || '-';
            },
          },
          {
            title: '费用类型',
            key: 'expense',
            width: 100,
            render: (_: unknown, r: Invoice) => {
              const t = expenseTypes.find(e => e.code === r.expense_type_code);
              return t ? <Tag>{t.name}</Tag> : '-';
            },
          },
        ]
      : []),
    {
      title: '价税合计',
      dataIndex: 'total_amount',
      key: 'total',
      width: 140,
      align: 'right' as const,
      render: (v: number) => <SensitiveText type="amount" value={v || 0} />,
    },
    {
      title: '操作',
      key: 'actions',
      width: 160,
      render: (_: unknown, r: Invoice) => (
        <Space>
          <Button size="small" icon={<EyeOutlined />} onClick={() => setViewDrawer(r)} />
          <Button size="small" icon={<EditOutlined />} onClick={() => handleEdit(r)} />
          <Button size="small" danger icon={<DeleteOutlined />} onClick={() => handleDelete(r.id)} />
        </Space>
      ),
    },
  ];

  const isSaveDisabled =
    !uploadModal.form.invoice_number ||
    (uploadModal.direction === 'output'
      ? !uploadModal.form.buyer_name?.trim()
      : !uploadModal.form.employee_id || !uploadModal.form.expense_type_code) ||
    (!!uploadModal.preview?.is_duplicate && !uploadModal.editingId);

  return (
    <div>
      <div className="page-header" style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
        <span className="page-title">发票管理</span>
        <Space>
          <Button icon={<PlusOutlined />} onClick={handleManualAdd}>
            {isOutput ? '手工录入销项' : '手工录入'}
          </Button>
          <Button type="primary" icon={<UploadOutlined />} onClick={handleUploadClick}>上传发票识别</Button>
          <Button icon={<ExportOutlined />} onClick={handleExport}>导出清单</Button>
          <Button icon={<SettingOutlined />} onClick={() => setExpenseDrawer(true)}>费用类型</Button>
        </Space>
      </div>

      {/* 方向 Tab：进项（默认）/销项；查询与录入均按方向区分（第十二阶段） */}
      <Tabs
        activeKey={direction}
        onChange={(key) => setDirection(key as 'input' | 'output')}
        items={[
          { key: 'input', label: '进项发票' },
          { key: 'output', label: '销项发票' },
        ]}
        style={{ marginBottom: 8 }}
      />

      <Row gutter={16} style={{ marginBottom: 16 }}>
        <Col span={6}><Card><Statistic title={isOutput ? '销项发票张数' : '发票张数'} value={list.length} /></Card></Col>
        <Col span={6}><Card><SensitiveStatistic title="价税合计" value={totalAmount} /></Card></Col>
        <Col span={6}><Card><Statistic title="本月去重拦截" value={duplicateCount} /></Card></Col>
      </Row>

      <Card style={{ marginBottom: 16 }}>
        <Space wrap>
          <DatePicker
            picker="month" allowClear={false}
            value={filterMonth}
            onChange={(value) => value && setFilterMonth(value)}
            placeholder="归属月份"
          />
          {!isOutput && (
            <Select
              style={{ width: 160 }} allowClear placeholder="报销人"
              value={filterEmployee}
              onChange={(v) => setFilterEmployee(v)}
              options={employees.map(e => ({ value: e.id, label: `${e.name} (${e.employee_no})` }))}
            />
          )}
          {!isOutput && (
            <Select
              style={{ width: 160 }} allowClear placeholder="费用类型"
              value={filterExpenseType}
              onChange={(v) => setFilterExpenseType(v)}
              options={expenseTypes.filter(t => t.enabled === 1).map(t => ({ value: t.code, label: t.name }))}
            />
          )}
          <Select
            style={{ width: 160 }} allowClear placeholder="发票类型"
            value={filterInvoiceType}
            onChange={(v) => setFilterInvoiceType(v)}
            options={[
              { value: '增值税普通发票', label: '增值税普通发票' },
              { value: '增值税专用发票', label: '增值税专用发票' },
              { value: '增值税电子普通发票', label: '增值税电子普通发票' },
              { value: '电子发票(普通发票)', label: '电子发票(普通发票) - 全电票' },
              { value: '电子发票(增值税专用发票)', label: '电子发票(增值税专用发票) - 全电票' },
            ]}
          />
          <Input.Search
            style={{ width: 240 }} allowClear placeholder="销售方/购买方/备注"
            value={filterKeyword}
            onChange={(e) => setFilterKeyword(e.target.value)}
            onSearch={fetchList}
          />
          <Button type="primary" onClick={fetchList}>查询</Button>
        </Space>
      </Card>

      <Card>
        <Table
          rowKey="id"
          loading={loading}
          columns={columns}
          dataSource={list}
          size="small"
          scroll={{ x: 'max-content' }}
        />
      </Card>

      {/* 上传/编辑 Modal */}
      <Modal
        title={
          uploadModal.editingId
            ? '编辑发票'
            : uploadModal.direction === 'output'
              ? '录入销项发票'
              : '上传发票'
        }
        open={uploadModal.visible}
        onCancel={() => setUploadModal(prev => ({ ...prev, visible: false }))}
        onOk={handleSaveInvoice}
        okText="保存"
        cancelText="取消"
        okButtonProps={{ disabled: isSaveDisabled }}
        width={900}
      >
        <Spin spinning={uploadModal.ocrLoading} tip="正在识别...">
          {uploadModal.preview?.is_duplicate && (
            <Alert
              style={{ marginBottom: 12 }}
              type="error"
              showIcon
              message="重复发票"
              description={`该发票已存在（ID=${uploadModal.preview.duplicate_invoice_id}），不能重复报销。`}
            />
          )}
          {uploadModal.preview?.warnings && uploadModal.preview.warnings.length > 0 && (
            <Alert
              style={{ marginBottom: 12 }}
              type="warning"
              showIcon
              message="识别提醒"
              description={uploadModal.preview.warnings.map((w, i) => <div key={i}>{w}</div>)}
            />
          )}
          <Row gutter={16}>
            <Col span={12}>
              <Form layout="vertical" size="small">
                <Form.Item label="发票代码（全电票可空）">
                  <Input
                    value={uploadModal.form.invoice_code || ''}
                    onChange={(e) => setUploadModal(prev => ({
                      ...prev, form: { ...prev.form, invoice_code: e.target.value }
                    }))}
                  />
                </Form.Item>
                <Form.Item label="发票号码" required>
                  <Input
                    value={uploadModal.form.invoice_number || ''}
                    onChange={(e) => setUploadModal(prev => ({
                      ...prev, form: { ...prev.form, invoice_number: e.target.value }
                    }))}
                  />
                </Form.Item>
                <Form.Item label="发票类型">
                  <Input
                    value={uploadModal.form.invoice_type || ''}
                    onChange={(e) => setUploadModal(prev => ({
                      ...prev, form: { ...prev.form, invoice_type: e.target.value }
                    }))}
                  />
                </Form.Item>
                <Form.Item label="开票日期">
                  <Input
                    value={uploadModal.form.issue_date || ''}
                    onChange={(e) => setUploadModal(prev => ({
                      ...prev, form: { ...prev.form, issue_date: e.target.value }
                    }))}
                    placeholder="2026-08-01"
                  />
                </Form.Item>
                <Form.Item label="金额（不含税）">
                  <Input
                    type="number"
                    value={uploadModal.form.amount ?? ''}
                    onChange={(e) => setUploadModal(prev => ({
                      ...prev, form: { ...prev.form, amount: parseFloat(e.target.value) || 0 }
                    }))}
                  />
                </Form.Item>
                <Form.Item label="税额">
                  <Input
                    type="number"
                    value={uploadModal.form.tax_amount ?? ''}
                    onChange={(e) => setUploadModal(prev => ({
                      ...prev, form: { ...prev.form, tax_amount: parseFloat(e.target.value) || 0 }
                    }))}
                  />
                </Form.Item>
                <Form.Item label="价税合计">
                  <Input
                    type="number"
                    value={uploadModal.form.total_amount ?? ''}
                    onChange={(e) => setUploadModal(prev => ({
                      ...prev, form: { ...prev.form, total_amount: parseFloat(e.target.value) || 0 }
                    }))}
                  />
                </Form.Item>
              </Form>
            </Col>
            <Col span={12}>
              <Form layout="vertical" size="small">
                <Form.Item
                  label="销售方"
                  required={uploadModal.direction === 'output'}
                  extra={uploadModal.direction === 'output' ? '销项口径：销售方为本公司（已按企业信息预填，可修改）' : undefined}
                >
                  <Input
                    value={uploadModal.form.seller_name || ''}
                    onChange={(e) => setUploadModal(prev => ({
                      ...prev, form: { ...prev.form, seller_name: e.target.value }
                    }))}
                  />
                </Form.Item>
                <Form.Item label="销售方税号">
                  <Input
                    value={uploadModal.form.seller_tax_id || ''}
                    onChange={(e) => setUploadModal(prev => ({
                      ...prev, form: { ...prev.form, seller_tax_id: e.target.value }
                    }))}
                  />
                </Form.Item>
                <Form.Item
                  label="购买方"
                  required={uploadModal.direction === 'output'}
                  extra={uploadModal.direction === 'output' ? '销项口径：购买方为客户，必填' : undefined}
                >
                  <Input
                    value={uploadModal.form.buyer_name || ''}
                    onChange={(e) => setUploadModal(prev => ({
                      ...prev, form: { ...prev.form, buyer_name: e.target.value }
                    }))}
                  />
                </Form.Item>
                {/* 销项不参与报销：报销人/费用类型仅进项口径展示 */}
                {uploadModal.direction !== 'output' && (
                  <Form.Item label="报销人" required>
                    <Select
                      value={uploadModal.form.employee_id}
                      onChange={(v) => setUploadModal(prev => ({
                        ...prev, form: { ...prev.form, employee_id: v }
                      }))}
                      options={employees.map(e => ({ value: e.id, label: `${e.name} (${e.employee_no})` }))}
                      placeholder="选择报销人"
                    />
                  </Form.Item>
                )}
                {uploadModal.direction !== 'output' && (
                  <Form.Item label="费用类型" required>
                    <Select
                      value={uploadModal.form.expense_type_code}
                      onChange={(v) => setUploadModal(prev => ({
                        ...prev, form: { ...prev.form, expense_type_code: v }
                      }))}
                      options={expenseTypes.filter(t => t.enabled === 1).map(t => ({ value: t.code, label: t.name }))}
                      placeholder="选择费用类型"
                    />
                  </Form.Item>
                )}
                <Form.Item label="归属月份">
                  <Input
                    value={uploadModal.form.belong_month || ''}
                    onChange={(e) => setUploadModal(prev => ({
                      ...prev, form: { ...prev.form, belong_month: e.target.value }
                    }))}
                    placeholder="2026-08"
                  />
                </Form.Item>
                <Form.Item label="备注">
                  <TextArea
                    rows={2}
                    value={uploadModal.form.remark || ''}
                    onChange={(e) => setUploadModal(prev => ({
                      ...prev, form: { ...prev.form, remark: e.target.value }
                    }))}
                  />
                </Form.Item>
              </Form>
            </Col>
          </Row>
        </Spin>
      </Modal>

      {/* 详情 Drawer */}
      <Drawer
        title="发票详情"
        open={!!viewDrawer}
        onClose={() => setViewDrawer(null)}
        width={500}
      >
        {viewDrawer && (
          <div>
            {viewDrawer.image_path && (
              <div style={{ marginBottom: 16 }}>
                <InvoiceImage invoiceId={viewDrawer.id} />
              </div>
            )}
            <p><b>发票代码：</b>{viewDrawer.invoice_code || '-'}</p>
            <p><b>发票号码：</b>{viewDrawer.invoice_number || '-'}</p>
            <p><b>方向：</b>{viewDrawer.direction === 'output' ? '销项发票' : '进项发票'}</p>
            <p><b>类型：</b>{viewDrawer.invoice_type || '-'}</p>
            <p><b>开票日期：</b>{viewDrawer.issue_date || '-'}</p>
            <p><b>金额：</b><SensitiveText type="amount" value={viewDrawer.amount || 0} /></p>
            <p><b>税额：</b><SensitiveText type="amount" value={viewDrawer.tax_amount || 0} /></p>
            <p><b>价税合计：</b><SensitiveText type="amount" value={viewDrawer.total_amount || 0} /></p>
            <p><b>销售方：</b>{viewDrawer.seller_name || '-'}</p>
            <p><b>购买方：</b>{viewDrawer.buyer_name || '-'}</p>
            <p><b>录入时间：</b>{viewDrawer.created_at || '-'}</p>
          </div>
        )}
      </Drawer>

      {/* 费用类型管理 Drawer */}
      <Drawer
        title="费用类型管理"
        open={expenseDrawer}
        onClose={() => setExpenseDrawer(false)}
        width={500}
      >
        <Card title="新增/编辑类型" size="small" style={{ marginBottom: 16 }}>
          <Form layout="vertical" size="small">
            <Form.Item label="编码（创建后不可改）">
              <Input
                value={expenseForm.code || ''}
                onChange={(e) => setExpenseForm(prev => ({ ...prev, code: e.target.value }))}
                disabled={!!expenseForm.id}
              />
            </Form.Item>
            <Form.Item label="名称">
              <Input
                value={expenseForm.name || ''}
                onChange={(e) => setExpenseForm(prev => ({ ...prev, name: e.target.value }))}
              />
            </Form.Item>
            <Form.Item label="排序">
              <Input
                type="number"
                value={expenseForm.sort_order ?? 99}
                onChange={(e) => setExpenseForm(prev => ({ ...prev, sort_order: parseInt(e.target.value) || 99 }))}
              />
            </Form.Item>
            <Space>
              <Button type="primary" onClick={handleSaveExpenseType}>保存</Button>
              <Button onClick={() => setExpenseForm({})}>重置</Button>
            </Space>
          </Form>
        </Card>
        <Card title="已有类型" size="small">
          {expenseTypes.map(t => (
            <div key={t.id} style={{ display: 'flex', justifyContent: 'space-between', padding: '6px 0' }}>
              <span>{t.name} <Tag>{t.code}</Tag> {t.enabled === 0 && <Tag color="default">已禁用</Tag>}</span>
              <Space>
                <Button size="small" onClick={() => setExpenseForm(t)}>编辑</Button>
                <Button size="small" danger onClick={() => handleDeleteExpenseType(t.id, t.code)}>删除</Button>
              </Space>
            </div>
          ))}
        </Card>
      </Drawer>
    </div>
  );
};

export default Invoices;
