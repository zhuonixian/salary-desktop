import { useCallback, useEffect, useMemo, useState } from 'react';
import {
  Alert,
  Button,
  Card,
  Col,
  Descriptions,
  List,
  Modal,
  Row,
  Space,
  Spin,
  Statistic,
  Table,
  Tag,
  Typography,
  message,
} from 'antd';
import {
  CheckCircleOutlined,
  CloudDownloadOutlined,
  DatabaseOutlined,
  FolderOpenOutlined,
  ReloadOutlined,
  SafetyCertificateOutlined,
  ToolOutlined,
  UploadOutlined,
} from '@ant-design/icons';
import { open } from '@tauri-apps/plugin-dialog';
import dayjs from 'dayjs';
import {
  backupDatabase,
  compactDatabase,
  getDataSafetyStatus,
  openAppDataDir,
  restoreDatabase,
  verifyDatabase,
} from '@/api';
import type { DataSafetyCheckResult, DataSafetyStatus, DataTableCount } from '@/types';

const { Text } = Typography;

const fmtBytes = (value?: number) => {
  const bytes = value ?? 0;
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  if (bytes < 1024 * 1024 * 1024) return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
  return `${(bytes / 1024 / 1024 / 1024).toFixed(2)} GB`;
};

const fmtTime = (value?: string) => (value ? dayjs(value).format('YYYY-MM-DD HH:mm:ss') : '-');

const DataSafety: React.FC = () => {
  const [status, setStatus] = useState<DataSafetyStatus | null>(null);
  const [checkResult, setCheckResult] = useState<DataSafetyCheckResult | null>(null);
  const [loading, setLoading] = useState(false);
  const [action, setAction] = useState<string | null>(null);

  const fetchStatus = useCallback(async () => {
    setLoading(true);
    try {
      setStatus(await getDataSafetyStatus());
    } catch (e: unknown) {
      message.error('获取数据安全状态失败: ' + (e instanceof Error ? e.message : String(e)));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    fetchStatus();
  }, [fetchStatus]);

  const tableData = useMemo(() => status?.table_counts ?? [], [status]);

  const runAction = async <T,>(key: string, task: () => Promise<T>, onSuccess: (result: T) => void) => {
    setAction(key);
    try {
      const result = await task();
      onSuccess(result);
      await fetchStatus();
    } catch (e: unknown) {
      message.error((e instanceof Error ? e.message : String(e)) || '操作失败');
    } finally {
      setAction(null);
    }
  };

  const handleBackup = async () => {
    const selected = await open({ directory: true, multiple: false, title: '选择备份保存目录' });
    if (!selected) return;
    await runAction('backup', () => backupDatabase(String(selected)), (result) => {
      message.success(`备份完成: ${result.backup_dir}`);
    });
  };

  const handleRestore = async () => {
    const selected = await open({ directory: true, multiple: false, title: '选择备份目录' });
    if (!selected) return;
    Modal.confirm({
      title: '确认恢复备份?',
      content: '恢复会替换当前本地数据库和发票归档。系统会先自动生成一份当前数据的保护备份。',
      okText: '确认恢复',
      cancelText: '取消',
      okButtonProps: { danger: true },
      onOk: async () => {
        await runAction('restore', () => restoreDatabase(String(selected)), (result) => {
          message.success(`恢复完成，保护备份位于: ${result.safety_backup_dir}`);
        });
      },
    });
  };

  const handleVerify = async () => {
    await runAction('verify', verifyDatabase, (result) => {
      setCheckResult(result);
      if (result.ok) {
        message.success('数据库体检通过');
      } else {
        message.warning('数据库体检发现异常');
      }
    });
  };

  const handleCompact = async () => {
    await runAction('compact', compactDatabase, () => {
      message.success('数据库压缩整理完成');
    });
  };

  const countColumns = [
    { title: '数据项', dataIndex: 'label', key: 'label', width: 160 },
    { title: '表名', dataIndex: 'table_name', key: 'table_name' },
    {
      title: '记录数',
      dataIndex: 'count',
      key: 'count',
      width: 120,
      align: 'right' as const,
      render: (value: number) => value.toLocaleString('zh-CN'),
    },
  ];

  return (
    <div>
      <div className="page-header">
        <span className="page-title">数据安全</span>
        <div className="page-header-actions">
          <Button icon={<FolderOpenOutlined />} onClick={() => runAction('openDir', openAppDataDir, () => undefined)}>
            打开数据目录
          </Button>
          <Button icon={<ReloadOutlined />} onClick={fetchStatus} loading={loading}>
            刷新
          </Button>
        </div>
      </div>

      <Spin spinning={loading}>
        <Alert
          type="info"
          showIcon
          style={{ marginBottom: 16 }}
          message="本页只管理本机数据"
          description="备份包含 salary.db 和发票归档目录。恢复前会自动生成保护备份；恢复后建议重启应用再继续录入。"
        />

        <Row gutter={[16, 16]} className="mb-16">
          <Col xs={24} md={8}>
            <Card className="stat-card">
              <Statistic
                title="数据库文件"
                value={fmtBytes(status?.database_size)}
                prefix={<DatabaseOutlined />}
              />
              <Tag color={status?.database_exists ? 'green' : 'red'} style={{ marginTop: 12 }}>
                {status?.database_exists ? '存在' : '缺失'}
              </Tag>
            </Card>
          </Col>
          <Col xs={24} md={8}>
            <Card className="stat-card">
              <Statistic
                title="发票归档"
                value={fmtBytes(status?.invoice_dir_size)}
                prefix={<CloudDownloadOutlined />}
              />
              <Tag color={status?.invoice_dir_exists ? 'green' : 'default'} style={{ marginTop: 12 }}>
                {status?.invoice_dir_exists ? '已创建' : '未创建'}
              </Tag>
            </Card>
          </Col>
          <Col xs={24} md={8}>
            <Card className="stat-card">
              <Statistic
                title="最近备份"
                value={fmtTime(status?.last_backup_at)}
                prefix={<SafetyCertificateOutlined />}
                valueStyle={{ fontSize: 18 }}
              />
              <Text type="secondary" ellipsis style={{ display: 'block', marginTop: 10 }}>
                {status?.last_backup_path || '暂无备份记录'}
              </Text>
            </Card>
          </Col>
        </Row>

        <Row gutter={[16, 16]} className="mb-16">
          <Col xs={24} xl={14}>
            <Card title="本地数据位置">
              <Descriptions column={1} size="small" bordered>
                <Descriptions.Item label="应用数据目录">
                  <Text copyable ellipsis>{status?.app_data_dir || '-'}</Text>
                </Descriptions.Item>
                <Descriptions.Item label="数据库文件">
                  <Text copyable ellipsis>{status?.database_path || '-'}</Text>
                </Descriptions.Item>
                <Descriptions.Item label="发票归档目录">
                  <Text copyable ellipsis>{status?.invoice_dir || '-'}</Text>
                </Descriptions.Item>
                <Descriptions.Item label="最近恢复">
                  {fmtTime(status?.last_restore_at)}
                </Descriptions.Item>
                <Descriptions.Item label="第七阶段迁移">
                  {status?.stage7_migration_status ? (
                    <Space size={8}>
                      <Tag color={status.stage7_migration_status === 'done' ? 'green' : 'gold'}>
                        {status.stage7_migration_status === 'done' ? '已完成' : status.stage7_migration_status}
                      </Tag>
                      <span>待归集 {status.stage7_pending_count ?? 0} 项</span>
                    </Space>
                  ) : (
                    '-'
                  )}
                </Descriptions.Item>
              </Descriptions>
            </Card>
          </Col>
          <Col xs={24} xl={10}>
            <Card title="维护操作">
              <Space direction="vertical" style={{ width: '100%' }} size={12}>
                <Button
                  type="primary"
                  icon={<CloudDownloadOutlined />}
                  loading={action === 'backup'}
                  onClick={handleBackup}
                  block
                >
                  立即备份
                </Button>
                <Button
                  danger
                  icon={<UploadOutlined />}
                  loading={action === 'restore'}
                  onClick={handleRestore}
                  block
                >
                  从备份恢复
                </Button>
                <Button
                  icon={<CheckCircleOutlined />}
                  loading={action === 'verify'}
                  onClick={handleVerify}
                  block
                >
                  数据库体检
                </Button>
                <Button
                  icon={<ToolOutlined />}
                  loading={action === 'compact'}
                  onClick={handleCompact}
                  block
                >
                  压缩整理数据库
                </Button>
              </Space>
            </Card>
          </Col>
        </Row>

        <Row gutter={[16, 16]} className="mb-16">
          <Col xs={24} xl={14}>
            <Card title="业务附件与资金数据">
              <Descriptions column={2} size="small" bordered>
                <Descriptions.Item label="附件总数">
                  {status?.attachment_count ?? 0}
                </Descriptions.Item>
                <Descriptions.Item label="已加密附件">
                  {status?.attachment_encrypted_count ?? 0}
                </Descriptions.Item>
                <Descriptions.Item
                  label="磁盘孤儿文件"
                  contentStyle={{ color: (status?.attachment_orphan_count ?? 0) > 0 ? '#cf1322' : undefined }}
                >
                  {status?.attachment_orphan_count ?? 0}
                </Descriptions.Item>
                <Descriptions.Item
                  label="缺失文件"
                  contentStyle={{ color: (status?.attachment_missing_count ?? 0) > 0 ? '#cf1322' : undefined }}
                >
                  {status?.attachment_missing_count ?? 0}
                </Descriptions.Item>
              </Descriptions>
              <Text type="secondary" style={{ display: 'block', marginTop: 8 }}>
                孤儿文件：磁盘上有、数据库无引用（可手动清理）；缺失文件：有记录、磁盘上没有（建议从备份恢复）。
              </Text>
            </Card>
          </Col>
        </Row>

        <Row gutter={[16, 16]}>
          <Col xs={24} xl={14}>
            <Card title="主要数据量">
              <Table<DataTableCount>
                rowKey="table_name"
                columns={countColumns}
                dataSource={tableData}
                pagination={false}
                size="small"
              />
            </Card>
          </Col>
          <Col xs={24} xl={10}>
            <Card title="最近体检">
              {checkResult ? (
                <Space direction="vertical" style={{ width: '100%' }}>
                  <Tag color={checkResult.ok ? 'green' : 'red'}>
                    {checkResult.ok ? '通过' : '异常'} / integrity_check={checkResult.integrity_check}
                  </Tag>
                  <Text type="secondary">{fmtTime(checkResult.checked_at)}</Text>
                  <List
                    size="small"
                    dataSource={checkResult.messages}
                    renderItem={(item) => <List.Item>{item}</List.Item>}
                  />
                </Space>
              ) : (
                <Text type="secondary">尚未执行体检</Text>
              )}
            </Card>
          </Col>
        </Row>
      </Spin>
    </div>
  );
};

export default DataSafety;
