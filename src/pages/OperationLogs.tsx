import { useCallback, useEffect, useMemo, useState } from 'react';
import { Button, Card, DatePicker, Input, Select, Space, Table, Tag, message } from 'antd';
import { ReloadOutlined, SearchOutlined } from '@ant-design/icons';
import dayjs from 'dayjs';
import type { Dayjs } from 'dayjs';
import { queryOperationLogs } from '@/api';
import type { OperationLog, OperationLogQuery } from '@/types';

const { RangePicker } = DatePicker;

const OperationLogs: React.FC = () => {
  const [logs, setLogs] = useState<OperationLog[]>([]);
  const [loading, setLoading] = useState(false);
  const [operationType, setOperationType] = useState<string | undefined>(undefined);
  const [keyword, setKeyword] = useState('');
  const [range, setRange] = useState<[Dayjs, Dayjs] | null>(null);

  const operationOptions = useMemo(() => {
    const values = Array.from(new Set(logs.map((log) => log.operation_type))).filter(Boolean);
    return values.map((value) => ({ value, label: value }));
  }, [logs]);

  const fetchData = useCallback(async () => {
    setLoading(true);
    try {
      const query: OperationLogQuery = {
        operation_type: operationType,
        keyword: keyword || undefined,
        start_date: range?.[0]?.startOf('day').toISOString(),
        end_date: range?.[1]?.endOf('day').toISOString(),
        limit: 300,
      };
      setLogs(await queryOperationLogs(query));
    } catch (e: unknown) {
      message.error('获取操作日志失败: ' + (e instanceof Error ? e.message : String(e)));
    } finally {
      setLoading(false);
    }
  }, [operationType, keyword, range]);

  useEffect(() => {
    fetchData();
  }, [fetchData]);

  const columns = [
    {
      title: '时间',
      dataIndex: 'created_at',
      key: 'created_at',
      width: 190,
      render: (value?: string) => (value ? dayjs(value).format('YYYY-MM-DD HH:mm:ss') : '-'),
    },
    {
      title: '操作类型',
      dataIndex: 'operation_type',
      key: 'operation_type',
      width: 190,
      render: (value: string) => <Tag color="blue">{value}</Tag>,
    },
    { title: '说明', dataIndex: 'description', key: 'description', width: 360, ellipsis: true },
    { title: '操作人', dataIndex: 'operator', key: 'operator', width: 110 },
    { title: '详情', dataIndex: 'detail', key: 'detail', ellipsis: true },
  ];

  return (
    <div>
      <div className="page-header">
        <span className="page-title">操作日志</span>
        <Button icon={<ReloadOutlined />} onClick={fetchData} loading={loading}>
          刷新
        </Button>
      </div>

      <Card style={{ marginBottom: 16 }}>
        <Space wrap>
          <Select
            style={{ width: 220 }}
            allowClear
            showSearch
            placeholder="操作类型"
            value={operationType}
            onChange={setOperationType}
            options={operationOptions}
          />
          <RangePicker value={range} onChange={(value) => setRange(value as [Dayjs, Dayjs] | null)} />
          <Input.Search
            style={{ width: 280 }}
            allowClear
            prefix={<SearchOutlined />}
            placeholder="搜索说明/详情/操作人"
            value={keyword}
            onChange={(e) => setKeyword(e.target.value)}
            onSearch={fetchData}
          />
          <Button type="primary" onClick={fetchData}>查询</Button>
        </Space>
      </Card>

      <Card>
        <Table
          rowKey="id"
          columns={columns}
          dataSource={logs}
          loading={loading}
          size="small"
          pagination={{ pageSize: 30, showSizeChanger: true, showTotal: (t) => `共 ${t} 条` }}
          scroll={{ x: 1100 }}
        />
      </Card>
    </div>
  );
};

export default OperationLogs;
